//! Solids a union joins without intersecting them: whether two B-reps are proved apart
//! ([`proved_apart`]), and the union of two so proved, side by side ([`union_apart`]) (not
//! upstream).
//!
//! The union of two solids that lie apart is the two solids as they are: no point of either is
//! inside the other or within a tolerance of it, so no face is cut or dropped and nothing merges.
//! A boolean of them finds nothing in whatever face pairs it tries, imprints nothing, classifies
//! every face outside, and its clean-up, which visits only the intersection's edges and
//! vertices, does nothing. What it gives is the second operand's topology copied into the first,
//! then the union's whole-body tail. [`union_apart`] builds that without the intersection work.
//!
//! **The proof.** Each shell of a solid region of a B-rep gets a box that holds every face of the
//! shell within its tolerances ([`face_box`]): for a face on a plane, the vertices and the
//! certified boxes of its edges' curves over intervals inside their curves' natural ones (a plane
//! region lies in the hull of its boundary); for a curved face, [`proved_bounding_box`] (the
//! certified box of its net over the face's stored UV domain, narrowed by its trims, which
//! assumes that domain holds the face, as the boolean's own face pairing does), grown by the
//! certified bound of the definition the net approximates, if it keeps one. Each box is grown by
//! the zone tolerance of twice the largest tolerance of the face, its edges and its vertices. A
//! B-rep with a face that has no such box (a curve on a surface, an offset, a blend patch), or
//! with a face no solid region's shell uses, is proved apart from nothing. Two B-reps are proved
//! apart when every shell box of one lies farther than the larger of the two B-reps'
//! intersection tolerances from every shell box of the other. A solid region's material lies
//! within its outer shell's box, so neither solid then holds a point of the other, and the margin
//! is at least four times the intersection tolerance a boolean of the two works at (half the
//! smaller B-rep tolerance at most, `MergeState::with_breps`). The boxes of the shells' vertices
//! lie inside the shell boxes, so where they come within the margin the B-reps are not apart,
//! and no face box is computed.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`proved_apart`] | (none) |
//! | [`union_apart`] | `SmBrep::MergeBrep` (as `SmMerge::BooleanLists` merges a list operand), with `SmApiBooleanUnion`'s operand preparation and tail |
//!
//! Deviations: not upstream.

use crate::error::Result;
use crate::geom::extent3d::Extent3d;
use crate::geom::tol::{brep_xsect_tol_3d, zone_tol3d, SmallTopoType};
use crate::kernel::Kernel;
use crate::query::face::p1::proved_bounding_box;
use crate::surface::surface::SurfaceKind;
use crate::topology::face::p2 as fp2;
use crate::topology::shell::ShellType;
use crate::topology::{edge, topology_traverser, vertex, Brep, FaceId};

const OP: &str = "boolean::apart";

/// Whether solids `a` and `b` are proved apart (the module's proof), so that their union is
/// [`union_apart`]'s: both at the same B-rep tolerance, both closed manifold solids (what a
/// boolean takes its manifold path for), and every shell box of each farther from every shell
/// box of the other than the larger of their intersection tolerances. Read from the B-reps as
/// they stand; nothing is written on them (missing UV trims are made on copies).
///
/// # Errors
/// A cancellation; those of the manifold test.
pub fn proved_apart(k: &Kernel, a: &Brep, b: &Brep) -> Result<bool> {
    let manifold = crate::topology::brep::p2::is_manifold_solid;
    // one B-rep tolerance for the two: the side by side union keeps the first's, which an
    // entity of a finer second operand would sit below (a boolean reconciles them)
    if a.tolerance().partial_cmp(&b.tolerance()) != Some(core::cmp::Ordering::Equal)
        || !manifold(a)?
        || !manifold(b)?
    {
        return Ok(false);
    }
    let margin = brep_xsect_tol_3d(k, a, SmallTopoType::NormalSize).max(brep_xsect_tol_3d(
        k,
        b,
        SmallTopoType::NormalSize,
    ));
    // every vertex lies in its shell's box: vertex boxes within the margin rule it out cheaply
    let (Some(ours), Some(theirs)) = (shell_vertex_boxes(k, a)?, shell_vertex_boxes(k, b)?) else {
        return Ok(false);
    };
    if !all_apart(&ours, &theirs, margin) {
        return Ok(false);
    }
    let (Some(ours), Some(theirs)) = (shell_boxes(k, a)?, shell_boxes(k, b)?) else {
        return Ok(false);
    };
    Ok(all_apart(&ours, &theirs, margin))
}

/// Whether every box of `ours` lies farther than `margin` from every box of `theirs`: the boxes
/// swept together along x, so only boxes whose x ranges come within `margin` are compared.
fn all_apart(ours: &[Extent3d], theirs: &[Extent3d], margin: f64) -> bool {
    // each box with its side (`theirs` or not), by least x
    let mut order: Vec<(bool, &Extent3d)> = ours
        .iter()
        .map(|x| (false, x))
        .chain(theirs.iter().map(|x| (true, x)))
        .collect();
    order.sort_by(|p, q| p.1.min().x.total_cmp(&q.1.min().x));
    for (n, &(side, a)) in order.iter().enumerate() {
        for &(other, b) in order.iter().skip(n + 1) {
            // sorted by least x: no later box comes within `margin` of `a`
            if b.min().x > a.max().x + margin {
                break;
            }
            if side != other && !a.are_disjoint(b, margin) {
                return false;
            }
        }
    }
    true
}

/// The faces of each face shell of every solid region of `b`, by shell (the infinite region's
/// shells hold the same faces from outside, all lumps at once, so they are left out). `None`
/// when a solid region has a shell of another kind or a face use without face, a shell has no
/// face, or some face of `b` is in none of these shells (a closed shell whose inside is no
/// region).
///
/// # Errors
/// A cancellation; those of the topology walk.
fn solid_shell_faces(k: &Kernel, b: &Brep) -> Result<Option<Vec<Vec<FaceId>>>> {
    let infinite = b.infinite_region();
    let mut shells = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (id, region) in b.regions().iter() {
        if Some(id) == infinite {
            continue;
        }
        for s in region.get_shells(None, |_, _| Ok(false))? {
            k.check_cancelled(OP)?;
            let shell = b.shell(s)?;
            if !matches!(shell.shell_type, ShellType::Faceuse) {
                return Ok(None);
            }
            let mut faces = Vec::new();
            for fu in shell.get_faceuses()? {
                let Some(f) = b.faceuse(fu)?.face else {
                    return Ok(None);
                };
                seen.insert(f);
                faces.push(f);
            }
            if faces.is_empty() {
                return Ok(None);
            }
            shells.push(faces);
        }
    }
    let all = b.faces().iter().count();
    Ok((!shells.is_empty() && seen.len() == all).then_some(shells))
}

/// The box of each solid region shell's vertices ([`solid_shell_faces`]), grown by its
/// vertices' tolerances, for the shells that have vertices; it lies inside that shell's box
/// ([`shell_boxes`]). `None` where [`solid_shell_faces`] gives none.
///
/// # Errors
/// A cancellation; those of the topology walk.
fn shell_vertex_boxes(k: &Kernel, b: &Brep) -> Result<Option<Vec<Extent3d>>> {
    let Some(shells) = solid_shell_faces(k, b)? else {
        return Ok(None);
    };
    let mut boxes = Vec::new();
    for faces in shells {
        let mut held = Extent3d::new();
        for f in faces {
            for v in crate::query::face::p2::face_vertices(b, f, false, None)? {
                let mut at = Extent3d::from_point(vertex::point(b, v)?);
                at.expand_absolute(vertex::tolerance(b, v)?);
                held = held.union(&at);
            }
        }
        // a shell without vertices (a whole sphere) gives no box here, and no early answer
        if held.is_defined() {
            boxes.push(held);
        }
    }
    Ok(Some(boxes))
}

/// The box of each face shell of every solid region of `b` ([`solid_shell_faces`]): the
/// union of its faces' boxes ([`face_box`]). `None` when a face has no box, or the shells
/// leave a face out.
///
/// # Errors
/// A cancellation; those of the topology walk.
fn shell_boxes(k: &Kernel, b: &Brep) -> Result<Option<Vec<Extent3d>>> {
    let Some(shells) = solid_shell_faces(k, b)? else {
        return Ok(None);
    };
    let mut scratch = None;
    let mut boxes = Vec::new();
    for faces in shells {
        let mut held = Extent3d::new();
        for f in faces {
            k.check_cancelled(OP)?;
            let Some(x) = face_box(k, b, f, &mut scratch)? else {
                return Ok(None);
            };
            held = held.union(&x);
        }
        boxes.push(held);
    }
    Ok(Some(boxes))
}

/// A box that holds every point of face `f` of `b` within the tolerances of the face, its edges
/// and its vertices, or `None` when none is proved. A face on a plane: its vertices and the
/// certified boxes ([`crate::curve::curve::Curve::certified_bounding_box`]) of its edges'
/// curves over their intervals, grown by the zone tolerance of twice the largest of those
/// tolerances, as [`proved_bounding_box`] grows its box (a region of a plane lies in the hull of
/// its boundary); any other face: [`proved_bounding_box`]'s. `scratch` is that function's copy
/// of `b`.
///
/// # Errors
/// A cancellation. Any other failure gives `None`.
fn face_box(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    scratch: &mut Option<Brep>,
) -> Result<Option<Extent3d>> {
    fn probe<T>(r: Result<T>) -> Result<Option<T>> {
        match r {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.is_cancelled() => Err(e),
            Err(_) => Ok(None),
        }
    }
    let Some(Some(surface)) = probe(fp2::surface(b, f))? else {
        return Ok(None);
    };
    if !matches!(surface.kind(), SurfaceKind::Plane(_)) {
        // the net's box, grown by how far the definition it approximates may lie from it
        let bound = surface.procedural_definition().map_or(0.0, |d| d.bound());
        if !(bound.is_finite() && bound >= 0.0) {
            return Ok(None);
        }
        return Ok(proved_bounding_box(k, b, f, scratch)?.map(|mut x| {
            x.expand_absolute(bound);
            x
        }));
    }
    let Some(mut tol) = probe(fp2::tolerance(b, f))? else {
        return Ok(None);
    };
    let mut held = Extent3d::new();
    let Some(vertices) = probe(crate::query::face::p2::face_vertices(b, f, false, None))? else {
        return Ok(None);
    };
    for v in vertices {
        let (Some(p), Some(t)) = (probe(vertex::point(b, v))?, probe(vertex::tolerance(b, v))?)
        else {
            return Ok(None);
        };
        held.add_point(p);
        tol = tol.max(t);
    }
    let Some(edges) = probe(topology_traverser::face_edges(b, f))? else {
        return Ok(None);
    };
    for e in edges {
        let (Some(Some(curve)), Some(interval), Some(t)) = (
            probe(edge::curve(b, e))?,
            probe(edge::interval(b, e))?,
            probe(edge::tolerance(b, e))?,
        ) else {
            return Ok(None);
        };
        // (a box of an interval past the curve's own is of the clamped interval)
        if !interval.is_contained_by(&curve.natural_interval(), 0.0) {
            return Ok(None);
        }
        let Some(Some(x)) = probe(curve.certified_bounding_box(k, &interval))? else {
            return Ok(None);
        };
        held = held.union(&x);
        tol = tol.max(t);
    }
    if !held.is_defined() {
        return Ok(None);
    }
    held.expand_absolute(zone_tol3d(k, 2.0 * tol));
    Ok(Some(held))
}

/// The union of solids `a` and `b` that [`proved_apart`] proved apart, as a boolean union gives
/// it, without the intersection work: each operand's self-touching edge copies glued as the
/// boolean glues them first, `b`'s topology copied into `a` (`MergeBrep`, as a list boolean
/// merges its list operand: its regions but the infinite one, which becomes `a`'s, its shells,
/// faces, edges and vertices with their geometry and attributes), then the union's tail: solid
/// and void regions set again by nesting, each region's outer shell first, then the zero-area
/// inner loops where solids touch and the faces of no area between coinciding edges removed,
/// the attributes kept on the objects left. `b` is consumed. Counted in [`crate::kernel::WorkCounters::apart_unions`], not as a
/// boolean.
///
/// # Errors
/// Those of the gluing, the merge and the tail.
pub fn union_apart(k: &mut Kernel, mut a: Brep, mut b: Brep) -> Result<Brep> {
    k.check_cancelled(OP)?;
    k.work_counters.add_apart_union();
    crate::boolean::api_brep::glue_self_touching_edges(k, &mut a)?;
    crate::boolean::api_brep::glue_self_touching_edges(k, &mut b)?;
    crate::boolean::merge::p3::combine_brep(k, &mut a, &b, None, None, true)?;
    // as a union's merge ends (`put_outer_shells_first`): solid and void regions set again by
    // nesting, each region's outer shell first
    let regions = crate::topology::brep::p1::regions(&a, None)?;
    if regions.len() > 2
        && regions.first() == a.infinite_region().as_ref()
        && crate::topology::brep::p2::is_manifold_solid(&a)?
    {
        crate::topology::brep::p1::set_region_is_void_flags_for_nested_solids(k, &mut a)?;
    }
    let editing = a.editing_enabled();
    a.set_editing_enabled(true);
    let mut state = crate::topology::brep::p2::ArenaNotifyState::with_attributes_of(&mut a);
    let tail = {
        let mut env = state.env(None);
        crate::topology::brep::p3::remove_slit_loops_where_solids_touch(k, &mut a, &mut env)
            .and_then(|_| {
                crate::topology::brep::p3::collapse_zero_area_two_edge_faces(k, &mut a, &mut env)
            })
    };
    let restored = state.restore_attributes(&mut a);
    a.set_editing_enabled(editing);
    tail?;
    restored?;
    a.refresh_cached_topology();
    Ok(a)
}
