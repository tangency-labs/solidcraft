//! A one-face sheet on a surface's whole natural box ([`sheet_on`]), the kernel's one
//! arena-notify `CreateFaceFromSurface` wrapper, a face on a box of a surface's parameters
//! ([`face_on`]), a sheet with its faces merged where they are one face ([`merged_faces`]),
//! a sheet turned over ([`turn_over_shell`], [`turn_over_sheet`]), a sheet of several faces
//! taken as the pieces of one surface and made again from it ([`one_surface`], [`sheet_of`]),
//! a planar face rebuilt as the Coons spline of its edges ([`face_spline_sheet`]), and any face
//! made a sheet with a control net ([`editable_sheet`]: an analytic face as its exact NURBS form),
//! and a sheet's loop started at a side of its box ([`start_at_side`], [`first_side`]), so a sheet
//! made again on an edited surface lists its edges as before.
//!
//! Not upstream: the composition an operation that rebuilds a face on a new surface (a sheet
//! extended past a side) makes of `new_brep` and `SmBrep::CreateFaceFromSurface`, kept in the
//! kernel so the modelling layer holds no notification plumbing; the turn-over of every
//! face of an open shell ([`crate::topology::face::p1::turn_over`]); the pieces a split
//! leaves joined back into their surface, so an operation on the surface (an extension past a
//! side) can make the pieces again, cut where they were; a planar face's spline sheet, so the
//! control-net edits of the modelling layer have a net on a flat face; and an analytic face's
//! exact-form sheet, so they have one on a cylinder, a sphere or a torus.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::collections::BTreeSet;

use crate::curve::curve::CurveKind;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::USE_DEFAULT;
use crate::geom::tol::{context_zone_tol_3d, SmallTopoType};
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::nurbs::math::CPoint;
use crate::nurbs::srf_basic::{Direction, Surface as NurbSurface};
use crate::nurbs::srf_tool::p3::join;
use crate::primitive::api_curve_ops::make_curves_compatible;
use crate::surface::side::BoxSide;
use crate::surface::surface::Surface;
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::p1::faces;
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p4::stitch_faces;
use crate::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use crate::topology::edge::create_trimmed_nurbs_curve;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p1::turn_over;
use crate::topology::face::p2 as fp2;
use crate::topology::face::p2::{loops, surface as face_surface, uv_domain};
use crate::topology::loopuse;
use crate::topology::topology_traverser::Marks;
use crate::topology::{Brep, FaceId, ShellId};
use crate::EntityRef;

/// A new body of one face on `surface` over its whole natural box, at `tolerance`, and that
/// face.
///
/// # Errors
///
/// Those of the box query, `new_brep` and `CreateFaceFromSurface`.
pub fn sheet_on(k: &mut Kernel, surface: Surface, tolerance: f64) -> Result<(Brep, FaceId)> {
    let bx = surface.natural_uv_domain_with_nurb(k)?;
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    b.set_tolerance(tolerance);
    let f = face_on(k, &mut b, surface, &bx)?;
    b.refresh_cached_topology();
    Ok((b, f))
}

/// `SmBrep::CreateFaceFromSurface(surface, bx, face)`: a new face of `b` on `surface` over the
/// box `bx` of its parameters, bounded by one loop of the box's sides, notifying through an
/// arena-only store (default records, no attributes, no context), the new face owning its
/// surface under its arena identity. The kernel's one such wrapper; the caller refreshes `b`'s
/// cached topology when it reads it next.
///
/// # Errors
///
/// Those of `CreateFaceFromSurface`.
pub fn face_on(k: &mut Kernel, b: &mut Brep, surface: Surface, bx: &Extent2d) -> Result<FaceId> {
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    create_face_from_surface(k, b, Some(surface), bx, &mut fenv)
}

const OP_SIDE: &str = "topology::brep::sheet::first_side";

/// [`sheet_on`] with its face's loop starting at the side `first` of the box
/// ([`start_at_side`]).
///
/// # Errors
///
/// Those of [`sheet_on`] and [`start_at_side`].
pub fn sheet_on_from(
    k: &mut Kernel,
    surface: Surface,
    tolerance: f64,
    first: BoxSide,
) -> Result<(Brep, FaceId)> {
    let (mut b, f) = sheet_on(k, surface, tolerance)?;
    start_at_side(k, &mut b, f, first)?;
    Ok((b, f))
}

/// Face `f`'s loop made to start at the edge on side `first` of its box, the others following
/// round the loop as before (its upward use's first loop use, and its mate from the mate of
/// that edge use): a sheet made again on an edited surface then lists its edges in the order the
/// sheet it replaces did ([`first_side`]). Unchanged where no edge lies on `first` (a side
/// collapsed to a point).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without its upward use, loop or surface; those of
/// the UV ends and of `SetEdgeuse`.
pub fn start_at_side(k: &Kernel, b: &mut Brep, f: FaceId, first: BoxSide) -> Result<()> {
    let (lu, uses) = outer_loop_uses(b, f)?;
    for eu in uses {
        if side_of_use(k, b, f, eu)? == Some(first) {
            loopuse::set_edgeuse(b, lu, Some(eu))?;
            if let (Some(mate_eu), Some(mate_lu)) = (eu1::mate(b, eu)?, b.loopuse(lu)?.mate) {
                loopuse::set_edgeuse(b, mate_lu, Some(mate_eu))?;
            }
            b.refresh_cached_topology();
            break;
        }
    }
    Ok(())
}

/// The side of face `f`'s parameter box its loop's first edge use runs along (its upward use's
/// first loop, as the loop is listed), the side both its UV ends lie on; `None` where they lie
/// on no one side (an edge inside the box).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without its upward use, loop or surface; those of the
/// UV ends.
pub fn first_side(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<Option<BoxSide>> {
    let (_, uses) = outer_loop_uses(b, f)?;
    let Some(&eu) = uses.first() else {
        return Ok(None);
    };
    side_of_use(k, b, f, eu)
}

/// The face's upward use's first loop use and its edge uses, as it lists them.
fn outer_loop_uses(
    b: &Brep,
    f: FaceId,
) -> Result<(crate::topology::LoopuseId, Vec<crate::topology::EdgeuseId>)> {
    let fu = fp2::upward_faceuse(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP_SIDE, "a face without its upward use"))?;
    let lu = eu1::faceuse_loopuses(b, fu, false, None)?
        .first()
        .copied()
        .ok_or_else(|| SolidError::invalid_input(OP_SIDE, "a face without its loop"))?;
    Ok((lu, loopuse::edgeuses(b, lu, None, None)?))
}

/// The side of face `f`'s box edge use `eu` runs along: the one side both its UV ends lie on,
/// within a millionth of the box's size.
fn side_of_use(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    eu: crate::topology::EdgeuseId,
) -> Result<Option<BoxSide>> {
    use crate::geom::extent2d::{SS_UMAX, SS_UMIN, SS_VMAX, SS_VMIN};
    let dom = face_surface(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP_SIDE, "a face without a surface"))?
        .natural_uv_domain_with_nurb(k)?;
    let (start, end) = crate::topology::trimming_tools::edge_uv_ends(k, b, eu)?;
    let size = dom.x_length().max(dom.y_length());
    let tol = crate::math::EFF_ZERO_SQRT * (1.0 + size);
    let on = dom.point_boundaries(start, tol) & dom.point_boundaries(end, tol);
    Ok(match on {
        SS_UMIN => Some(BoxSide::UMin),
        SS_UMAX => Some(BoxSide::UMax),
        SS_VMIN => Some(BoxSide::VMin),
        SS_VMAX => Some(BoxSide::VMax),
        _ => None,
    })
}

const OP_MERGED: &str = "topology::brep::sheet::merged_faces";

/// A sheet with its faces merged where they are one face ([`merged_faces`]), and where each of
/// the sheet's faces and edges went.
#[derive(Debug)]
pub struct Merged {
    /// The merged copy.
    pub brep: Brep,
    /// Each face of the sheet and the face of `brep` it is now part of.
    pub faces: Vec<(FaceId, FaceId)>,
    /// Each edge of the sheet and the edge of `brep` it is now part of, or `None` for an edge
    /// that is gone (between two faces merged into one).
    pub edges: Vec<(crate::topology::EdgeId, Option<crate::topology::EdgeId>)>,
}

/// A copy of `b` with its faces merged across every edge between two faces of one surface and
/// its edges joined at every vertex between two tangent edges
/// (`SmBrep::RemoveTopologicalEdgesAndVertices`, which keeps geometry and merges the merged
/// entities' attributes): a flat sheet the skin split into plane faces becomes one plane face.
/// Each of `b`'s edges maps to the edge it is still part of: itself where it survives, else the
/// edge through its middle (within ten of the body's tolerance; a joined piece), else `None`
/// (an edge removed between two merged faces). Each face maps to the face it was merged into:
/// the one surviving face of the faces joined across removed edges. `b` is unchanged.
///
/// # Errors
///
/// Those of the copy, the merge and the edge queries; [`SolidError::topology_failed`] for a
/// group of merged faces that does not leave exactly one face.
pub fn merged_faces(k: &mut Kernel, b: &Brep) -> Result<Merged> {
    use crate::topology::brep::p1::{edges, faces};
    use crate::topology::EdgeId;
    use std::collections::BTreeMap;

    let (mut c, map) = crate::topology::brep::p3::brep_copy_mapped(k, b)?;
    // the map holds each of `b`'s entities and its copy (source to copy)
    let copy_of = |r: EntityRef| -> Result<Option<EntityRef>> { map.get_at(Some(r)) };
    // before the merge: each edge of `b`, its copy, the copy's middle and its faces
    let mut copied_edges = Vec::new();
    for oe in edges(b, None)? {
        let Some(EntityRef::Edge(ce)) = copy_of(EntityRef::Edge(oe))? else {
            return Err(SolidError::topology_failed(
                OP_MERGED,
                "an edge without its copy",
            ));
        };
        let record = c.edge(ce)?;
        let curve = record
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::topology_missing(OP_MERGED, "an edge's curve"))?;
        let middle = curve.evaluate_point(k, record.interval.mid())?;
        let mut on = Vec::new();
        for eu in crate::topology::edge::edgeuses(&c, ce, None, None)? {
            if let Some(f) = crate::topology::edgeuse::p1::face(&c, eu)? {
                if !on.contains(&f) {
                    on.push(f);
                }
            }
        }
        copied_edges.push((oe, ce, middle, on));
    }
    let mut copied_faces = Vec::new();
    for of in faces(b, None)? {
        let Some(EntityRef::Face(cf)) = copy_of(EntityRef::Face(of))? else {
            return Err(SolidError::topology_failed(
                OP_MERGED,
                "a face without its copy",
            ));
        };
        copied_faces.push((of, cf));
    }

    crate::topology::brep::p5::remove_topological_edges_and_vertices(k, &mut c, None, None)?;
    c.refresh_cached_topology();

    let live_edges = edges(&c, None)?;
    let live_faces = faces(&c, None)?;
    let near = 10.0 * c.tolerance().max(1e-9);
    // each edge: itself, else the edge through its middle, else gone
    let mut edge_map = Vec::new();
    let mut removed: Vec<&[FaceId]> = Vec::new();
    for (oe, ce, middle, on) in &copied_edges {
        let now = if live_edges.contains(ce) {
            Some(*ce)
        } else {
            let mut best: Option<(f64, EdgeId)> = None;
            for &e in &live_edges {
                let Ok(at) = crate::query::api_queries::edge_closest_point(k, &c, e, *middle)
                else {
                    continue;
                };
                let d = at.closest.distance;
                if d <= near && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, e));
                }
            }
            best.map(|(_, e)| e)
        };
        if now.is_none() {
            removed.push(on.as_slice());
        }
        edge_map.push((*oe, now));
    }
    // the faces joined across removed edges, each group one label
    let mut label: BTreeMap<FaceId, usize> = copied_faces
        .iter()
        .enumerate()
        .map(|(i, &(_, cf))| (cf, i))
        .collect();
    for on in removed {
        let mut groups: Vec<usize> = on.iter().filter_map(|f| label.get(f).copied()).collect();
        groups.sort_unstable();
        groups.dedup();
        if let Some((&keep, rest)) = groups.split_first() {
            for v in label.values_mut() {
                if rest.contains(v) {
                    *v = keep;
                }
            }
        }
    }
    let mut face_map = Vec::new();
    for &(of, cf) in &copied_faces {
        let group = label.get(&cf).copied();
        let mut survivors = live_faces
            .iter()
            .filter(|&f| label.get(f).copied() == group);
        let (Some(&now), None) = (survivors.next(), survivors.next()) else {
            return Err(SolidError::topology_failed(
                OP_MERGED,
                "merged faces that do not leave exactly one face",
            ));
        };
        face_map.push((of, now));
    }
    Ok(Merged {
        brep: c,
        faces: face_map,
        edges: edge_map,
    })
}

const OP_TURN: &str = "topology::brep::sheet::turn_over_shell";

/// Turns the open shell `s` over: each of its faces ([`turn_over`], `SmFace::SwapUV`), so each
/// face's side is the other one, its edges as they were; the body stays valid. Turning a shell
/// over twice gives it back.
///
/// A loop of [`turn_over`], not `SmBrep::SwapUV` or `OrientSheetSurfaces(same = false)`: those
/// ignore a face's failed swap (upstream's `sm_SwapFaceUV` is void), and a turn-over that stops
/// part way must say so and leave the shell as it was to the caller's rollback.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a closed shell (one whose faces' other uses lie in another
/// shell: turned over, the body would be inside out), before anything changes;
/// [`SolidError::InvalidInput`] for a shell of no faces; those of [`turn_over`].
pub fn turn_over_shell(k: &Kernel, b: &mut Brep, s: ShellId) -> Result<()> {
    let faces = open_shell_faces(b, s)?;
    for f in faces {
        turn_over(k, b, f)?;
    }
    b.refresh_cached_topology();
    Ok(())
}

/// Turns every shell of faces of `b` over ([`turn_over_shell`]): a sheet turned over.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a body with a closed shell (a solid), before anything
/// changes; those of [`turn_over`].
pub fn turn_over_sheet(k: &Kernel, b: &mut Brep) -> Result<()> {
    let mut faces = Vec::new();
    let shells: Vec<ShellId> = b.shells().ids().collect();
    for s in shells {
        if b.shell(s)?.is_faceuse_shell() {
            faces.extend(open_shell_faces(b, s)?);
        }
    }
    for f in faces {
        turn_over(k, b, f)?;
    }
    b.refresh_cached_topology();
    Ok(())
}

/// The faces of the open shell `s`, each once, in the order of their uses.
fn open_shell_faces(b: &Brep, s: ShellId) -> Result<Vec<FaceId>> {
    let shell = b.shell(s)?;
    if !shell.is_faceuse_shell() {
        return Err(SolidError::invalid_input(OP_TURN, "a shell of no faces"));
    }
    if shell.is_closed(b, s)? {
        return Err(SolidError::Unsupported {
            operation: OP_TURN,
            reason: "a closed shell: turned over, the body would be inside out",
        });
    }
    let mut seen = BTreeSet::new();
    let mut faces = Vec::new();
    for fu in shell.get_faceuses()? {
        if let Some(f) = b.faceuse(fu)?.face {
            if seen.insert(f) {
                faces.push(f);
            }
        }
    }
    Ok(faces)
}

const OP_SPLINE: &str = "topology::brep::sheet::face_spline_sheet";

fn not_rebuilt(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP_SPLINE,
        reason,
    }
}

/// The curve that stays at `p`: degree 1 on `[0, 1]`, both control points `p` (a Coons side
/// collapsed to a corner).
fn at_point(p: Point3) -> Result<BSplineCurve> {
    let points = [p, p];
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[2, 2],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
}

/// `c` running the other way.
fn reversed(mut c: BSplineCurve) -> Result<BSplineCurve> {
    let interval = c.natural_interval();
    c.reverse_parameterization(&interval)?;
    Ok(c)
}

/// `a` and `b` made compatible, exactly (one degree, parameter range and knot vector).
fn compatible(a: BSplineCurve, b: BSplineCurve) -> Result<[BSplineCurve; 2]> {
    let mut pair = [a, b];
    make_curves_compatible(&mut pair)?;
    Ok(pair)
}

/// The bilinear Coons patch bounded by `sides`, three or four B-spline curves running end to end
/// round one loop in their order (`c0` from `P0` to `P1`, `c1` on to `P2`, `c2` on to `P3` and
/// `c3` back to `P0`; with three, `c2` runs back to `P0` and the fourth side stays at `P0`, a
/// degenerate patch). Its sides in the patch's order are `P0` to `P1` and `P3` to `P2` (in u, at
/// v min and max) and `P0` to `P3` and `P1` to `P2` (in v, at u min and max); opposite sides are
/// made compatible first, exactly (one degree, parameter range and knot vector each pair), so
/// the patch's sides are the curves themselves (`create_coons_patch` with `tol`). Not upstream:
/// the one sequence the modelling layer's boundary patch and [`face_spline_sheet`] share.
///
/// # Errors
///
/// `InvalidInput` for other than three or four curves; those of the reversal, the
/// compatibility and the patch.
pub fn coons_patch_of_loop(sides: Vec<BSplineCurve>, tol: f64) -> Result<BSplineSurface> {
    let mut sides = sides.into_iter();
    let (Some(c0), Some(c1), Some(c2)) = (sides.next(), sides.next(), sides.next()) else {
        return Err(SolidError::invalid_input(
            OP_SPLINE,
            "a Coons patch of other than three or four sides",
        ));
    };
    let c3 = match sides.next() {
        Some(c3) => c3,
        None => at_point(c0.ends()?.start)?,
    };
    if sides.next().is_some() {
        return Err(SolidError::invalid_input(
            OP_SPLINE,
            "a Coons patch of other than three or four sides",
        ));
    }
    let [u0, u1] = compatible(c0, reversed(c2)?)?;
    let [v0, v1] = compatible(reversed(c3)?, c1)?;
    let mut quad = [u0, u1, v0, v1];
    BSplineSurface::create_coons_patch(tol, true, &mut quad, &mut [])
}

/// Face `f` of `b`, when its surface is a plane, rebuilt as a one-face sheet on the bilinear
/// Coons patch of its edges: a spline surface whose whole box is the face. The edges are taken
/// round the face's one loop as its upward use runs it, each as a B-spline copy of its curve
/// over its interval (`CreateTrimmedNURBSCurve`), and bound the patch [`coons_patch_of_loop`]
/// makes (three edges: its fourth side collapsed to the corner where the third meets the
/// first). The patch's sides are the edges themselves,
/// and each of its points is an affine combination of points of the edges, so it lies in their
/// plane; its degrees are the edges' own (a skin of two cubic sections gives (3, 1), its wall).
/// `None` for a face that is not on a plane.
///
/// # Errors
///
/// `Unsupported` for a planar face with holes or with fewer than three or more than four edges,
/// an edge with no B-spline form, or a patch that folds (its normal not the face's at every
/// point of a 5 by 5 grid inside its box: a dart-shaped outline); those of the curves, the
/// compatibility, the patch and the sheet.
pub fn face_spline_sheet(k: &mut Kernel, b: &Brep, f: FaceId) -> Result<Option<Brep>> {
    let face = b.face(f)?;
    let surface = face
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP_SPLINE, "a face without a surface"))?;
    let SurfaceKind::Plane(plane) = surface.kind() else {
        return Ok(None);
    };
    let normal = plane.position().z_axis();
    if loops(b, f, false, None)?.len() != 1 {
        return Err(not_rebuilt("a planar face with holes"));
    }
    let fu = fp2::upward_faceuse(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP_SPLINE, "a face without its upward use"))?;
    let lu = eu1::faceuse_loopuses(b, fu, false, None)?
        .first()
        .copied()
        .ok_or_else(|| SolidError::invalid_input(OP_SPLINE, "a face without its loop"))?;
    let mut sides = Vec::new();
    for eu in loopuse::edgeuses(b, lu, None, None)? {
        let e = eu1::edge(b, eu)?
            .ok_or_else(|| SolidError::invalid_input(OP_SPLINE, "an edge use without its edge"))?;
        let c = create_trimmed_nurbs_curve(k, b, e)?
            .ok_or_else(|| not_rebuilt("an edge with no B-spline form"))?;
        let CurveKind::BSpline(c) = c.into_kind() else {
            return Err(not_rebuilt("an edge with no B-spline form"));
        };
        // as the loop runs
        sides.push(if eu1::orientation(b, eu)? == OrientType::Same {
            c
        } else {
            reversed(c)?
        });
    }
    if !(3..=4).contains(&sides.len()) {
        return Err(not_rebuilt(
            "a planar face of other than three or four edges",
        ));
    }
    let tol = context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
    let patch = coons_patch_of_loop(sides, tol)?;
    let patch = Surface::from(patch);
    // no fold: the patch faces the face's way inside its box
    let bx = patch.natural_uv_domain_with_nurb(k)?;
    for i in 1..=5 {
        for j in 1..=5 {
            let uv = bx.evaluate(f64::from(i) / 6.0, f64::from(j) / 6.0);
            let n = patch.evaluate_normal(k, uv, Side::Left, Side::Left)?;
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
            if !(n.dot(normal) > 0.0) {
                return Err(not_rebuilt(
                    "a planar face whose Coons patch folds (its outline is not convex enough)",
                ));
            }
        }
    }
    // the patch's v min side is the loop's first edge (`coons_patch_of_loop`): the sheet lists
    // its edges in the face's order
    let (sheet, _) = sheet_on_from(k, patch, b.tolerance(), BoxSide::VMin)?;
    Ok(Some(sheet))
}

const OP_EDITABLE: &str = "topology::brep::sheet::editable_sheet";

/// Face `f` of `b` as a one-face sheet whose surface has a control net to edit, its whole box
/// the face; `None` for a face on a B-spline surface (it has one already).
/// - A plane: [`face_spline_sheet`], the Coons spline of its edges.
/// - A cone (a cylinder too), a sphere, a torus, a surface of revolution or of extrusion: its
///   surface's exact NURBS form, the B-spline the kernel keeps for it (its circles rational
///   quadratics), on a sheet over its whole box. Only for a face that is its surface's whole box,
///   bounded by the box's sides (the natural trim of
///   [`crate::topology::face::p1::trim_boundary_rectangle`], a pole a point or an edge): a face
///   trimmed inside it would lose its trims to a whole-box sheet. A closed surface's two seam
///   sides are one edge used twice and a pole is a point (`CreateFaceFromSurface` shares
///   coincident sides and drops degenerate ones).
///
/// `b` is unchanged.
///
/// # Errors
///
/// `Unsupported` for a surface of another kind (an offset, a blend, a ball patch: no exact
/// NURBS form here) and for a face that is not its surface's whole box; those of
/// [`face_spline_sheet`], the copy, the trim query and the sheet.
pub fn editable_sheet(k: &mut Kernel, b: &Brep, f: FaceId) -> Result<Option<Brep>> {
    let surface = b
        .face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP_EDITABLE, "a face without a surface"))?;
    match surface.kind() {
        SurfaceKind::BSpline(_) => Ok(None),
        SurfaceKind::Plane(_) => face_spline_sheet(k, b, f),
        SurfaceKind::Cone(_)
        | SurfaceKind::Sphere(_)
        | SurfaceKind::Torus(_)
        | SurfaceKind::SurfOfRevolution(_)
        | SurfaceKind::SurfOfExtrusion(_) => exact_form_sheet(k, b, f, surface).map(Some),
        _ => Err(SolidError::Unsupported {
            operation: OP_EDITABLE,
            reason: "a face with no exact NURBS form",
        }),
    }
}

/// The sheet on `surface`'s exact NURBS form over its whole box, when face `f` of `b` is that
/// whole box ([`editable_sheet`]).
fn exact_form_sheet(k: &mut Kernel, b: &Brep, f: FaceId, surface: &Surface) -> Result<Brep> {
    use crate::geom::attribute::ObjectKey;
    use crate::geom::object::ObjectRef;
    use crate::topology::brep::p3::{brep_copy_mapped, with_nurb};
    use crate::topology::face::p1::trim_boundary_rectangle;

    // the trim query builds the face's trim boundary: on a copy, so `b` is only read
    let (mut c, map) = brep_copy_mapped(k, b)?;
    let Some(EntityRef::Face(cf)) = map.get_at(Some(EntityRef::Face(f)))? else {
        return Err(SolidError::invalid_input(
            OP_EDITABLE,
            "a face without its copy",
        ));
    };
    let key_of = |e: EntityRef| ObjectRef {
        type_code: 0,
        key: ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    if !trim_boundary_rectangle(k, &mut c, cf, &key_of)?.natural {
        return Err(SolidError::Unsupported {
            operation: OP_EDITABLE,
            reason: "a face that is not its surface's whole box",
        });
    }
    let form = with_nurb(k, surface)?
        .ok_or_else(|| SolidError::invalid_input(OP_EDITABLE, "a surface with no NURBS form"))?;
    let nurb = form
        .gw_nurb()
        .ok_or_else(|| SolidError::invalid_input(OP_EDITABLE, "a surface with no NURBS form"))?;
    let (sheet, _) = sheet_on(
        k,
        Surface::from(BSplineSurface::from_gw_nurb(nurb)),
        b.tolerance(),
    )?;
    Ok(sheet)
}

const OP_PIECES: &str = "topology::brep::sheet::one_surface";

/// A sheet's faces as the pieces of one surface ([`one_surface`]): the surface, its whole box,
/// each face with its box on the surface, and the parameters the pieces are cut at.
#[derive(Debug, Clone)]
pub struct Pieces {
    /// The surface the faces are the pieces of: a B-spline surface on their parameters.
    pub surface: Surface,
    /// The surface's whole box: the faces' boxes together.
    pub whole: Extent2d,
    /// Each face and its box, by u interval, then by v interval within it: the order
    /// [`sheet_of`] makes its faces in.
    pub faces: Vec<(FaceId, Extent2d)>,
    /// The u parameters between the pieces, increasing.
    pub u_cuts: Vec<f64>,
    /// The v parameters between the pieces, increasing.
    pub v_cuts: Vec<f64>,
}

impl Pieces {
    /// Whether side `side` of a face's box `bx` is on the same side of the whole box.
    #[must_use]
    pub fn on_side(&self, bx: &Extent2d, side: BoxSide) -> bool {
        side.at(bx) == side.at(&self.whole)
    }
}

/// The one surface `b`'s faces are the pieces of, as a split (`SmBrep::CreateFacesFromSurface`,
/// [`sheet_of`]) leaves them, or `None` where they are not: each face a B-spline surface's whole
/// box bounded by one loop, the boxes tiling one box on one grid of parameters, and each piece
/// continuing its neighbour exactly (the same degrees, rationality and knots across the cut, the
/// row they share the same bit for bit, their parameters meeting). The pieces are joined back
/// ([`join`], in v within each u interval, then in u), the shared row once; a knot at a cut is
/// kept where removing it would move the net at all.
///
/// # Errors
///
/// Those of the face queries and the join.
pub fn one_surface(k: &Kernel, b: &Brep) -> Result<Option<Pieces>> {
    let mut cells: Vec<(FaceId, Extent2d, NurbSurface)> = Vec::new();
    for f in faces(b, None)? {
        let Some(s) = face_surface(b, f)? else {
            return Ok(None);
        };
        let SurfaceKind::BSpline(spline) = s.kind() else {
            return Ok(None);
        };
        let Some(net) = spline.gw_nurb() else {
            return Ok(None);
        };
        let bx = uv_domain(b, f)?;
        if !s.is_whole_box(k, &bx)? || loops(b, f, false, None)?.len() != 1 {
            return Ok(None);
        }
        cells.push((f, bx, net.clone()));
    }
    // the grid: every face's box one cell of it, each cell one face's
    let breaks = |of: &dyn Fn(&Extent2d) -> [f64; 2]| -> Vec<f64> {
        let mut x: Vec<f64> = cells.iter().flat_map(|(_, bx, _)| of(bx)).collect();
        x.sort_by(f64::total_cmp);
        x.dedup();
        x
    };
    let us = breaks(&|bx: &Extent2d| [bx.min().x, bx.max().x]);
    let vs = breaks(&|bx: &Extent2d| [bx.min().y, bx.max().y]);
    let (columns, rows) = (us.len().saturating_sub(1), vs.len().saturating_sub(1));
    if columns.checked_mul(rows) != Some(cells.len()) || cells.is_empty() {
        return Ok(None);
    }
    let interval = |x: &[f64], a: f64, b: f64| {
        x.windows(2)
            .position(|w| matches!(w, [p, q] if *p == a && *q == b))
    };
    let mut grid: Vec<Option<(FaceId, Extent2d, NurbSurface)>> = vec![None; cells.len()];
    for cell in cells {
        let bx = &cell.1;
        let (Some(i), Some(j)) = (
            interval(&us, bx.min().x, bx.max().x),
            interval(&vs, bx.min().y, bx.max().y),
        ) else {
            return Ok(None);
        };
        let Some(slot) = i
            .checked_mul(rows)
            .and_then(|x| x.checked_add(j))
            .and_then(|x| grid.get_mut(x))
        else {
            return Ok(None);
        };
        if slot.is_some() {
            return Ok(None);
        }
        *slot = Some(cell);
    }
    let grid: Vec<(FaceId, Extent2d, NurbSurface)> = grid.into_iter().flatten().collect();
    // each u interval's pieces joined in v, then the columns in u
    let mut joined_columns: Vec<NurbSurface> = Vec::new();
    for i in 0..columns {
        let column = i
            .checked_mul(rows)
            .and_then(|a| Some(a..a.checked_add(rows)?))
            .and_then(|r| grid.get(r))
            .ok_or_else(|| SolidError::invalid_input(OP_PIECES, "a column past the grid"))?;
        let Some(c) = joined(column.iter().map(|(_, _, s)| s), Direction::V)? else {
            return Ok(None);
        };
        joined_columns.push(c);
    }
    let Some(whole_net) = joined(joined_columns.iter(), Direction::U)? else {
        return Ok(None);
    };
    let surface = Surface::from(BSplineSurface::from_gw_nurb(&whole_net));
    let whole = surface.natural_uv_domain_with_nurb(k)?;
    let inner = |x: &[f64]| -> Vec<f64> {
        x.iter()
            .skip(1)
            .take(x.len().saturating_sub(2))
            .copied()
            .collect()
    };
    Ok(Some(Pieces {
        surface,
        whole,
        faces: grid.into_iter().map(|(f, bx, _)| (f, bx)).collect(),
        u_cuts: inner(&us),
        v_cuts: inner(&vs),
    }))
}

/// The pieces of `pieces` joined in `dir`, each continuing the one before exactly, or `None`
/// where one does not.
fn joined<'a>(
    mut pieces: impl Iterator<Item = &'a NurbSurface>,
    dir: Direction,
) -> Result<Option<NurbSurface>> {
    let Some(first) = pieces.next() else {
        return Err(SolidError::invalid_input(OP_PIECES, "no pieces to join"));
    };
    let mut out = first.clone();
    for next in pieces {
        if !continues(&out, next, dir)? {
            return Ok(None);
        }
        // a knot removed only where the net does not move at all
        out = join(&out, next, dir, 0.0)?;
    }
    Ok(Some(out))
}

/// Whether `b` continues `a` past its end in `dir` exactly: the same degrees and rationality,
/// the same knots and net size the other way, `b`'s parameters starting where `a`'s end, and
/// `a`'s last row `b`'s first, bit for bit.
fn continues(a: &NurbSurface, b: &NurbSurface, dir: Direction) -> Result<bool> {
    let same = |x: &[f64], y: &[f64]| {
        x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits())
    };
    let bits = |c: &CPoint| [c.x.to_bits(), c.y.to_bits(), c.z.to_bits(), c.w.to_bits()];
    if a.degrees() != b.degrees() || a.is_rational() != b.is_rational() {
        return Ok(false);
    }
    let (across, along) = if dir == Direction::U {
        ((a.v_knots(), b.v_knots()), (a.u_knots(), b.u_knots()))
    } else {
        ((a.u_knots(), b.u_knots()), (a.v_knots(), b.v_knots()))
    };
    let meet = matches!(
        (along.0.last(), along.1.first()),
        (Some(x), Some(y)) if x.to_bits() == y.to_bits()
    );
    if !same(across.0, across.1) || !meet {
        return Ok(false);
    }
    let (count, a_end) = if dir == Direction::U {
        (a.m(), a.n())
    } else {
        (a.n(), a.m())
    };
    for i in 0..=count {
        let (p, q) = if dir == Direction::U {
            (a.cpt(a_end, i)?, b.cpt(0, i)?)
        } else {
            (a.cpt(i, a_end)?, b.cpt(i, 0)?)
        };
        if bits(&p) != bits(&q) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A new sheet of `surface` cut at the u parameters `u_cuts`, then each piece at the v
/// parameters `v_cuts` (each increasing, inside the surface's box), one face a piece on its
/// whole box, stitched at `tolerance`; and each face with its box, by u interval, then by v
/// interval within it (the order [`one_surface`] gives its faces in). The pieces are cut where
/// asked and nowhere else: a knot of low continuity elsewhere (an extension's joint) stays
/// inside its face, as on a sheet of one face ([`sheet_on`]).
///
/// # Errors
///
/// `InvalidInput` for cuts not increasing or not inside the box; those of the split, the face
/// construction and the stitch.
pub fn sheet_of(
    k: &mut Kernel,
    surface: Surface,
    u_cuts: &[f64],
    v_cuts: &[f64],
    tolerance: f64,
) -> Result<(Brep, Vec<(FaceId, Extent2d)>)> {
    let bx = surface.natural_uv_domain_with_nurb(k)?;
    let inside = |cuts: &[f64], lo: f64, hi: f64| {
        cuts.iter().all(|&c| lo < c && c < hi)
            && cuts.windows(2).all(|w| matches!(w, [a, b] if a < b))
    };
    if !inside(u_cuts, bx.min().x, bx.max().x) || !inside(v_cuts, bx.min().y, bx.max().y) {
        return Err(SolidError::invalid_input(
            OP_SHEET_OF,
            "cuts not increasing inside the surface's box",
        ));
    }
    let mut columns = Vec::new();
    let mut rest = surface;
    for &u in u_cuts {
        let (left, right) = rest.split_at(k, u, SurfParamType::U)?;
        columns.push(left);
        rest = right;
    }
    columns.push(rest);
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    b.set_tolerance(tolerance);
    let mut made = Vec::new();
    let mut place = |k: &mut Kernel, b: &mut Brep, s: Surface| -> Result<()> {
        let bx = s.natural_uv_domain_with_nurb(k)?;
        made.push((face_on(k, b, s, &bx)?, bx));
        Ok(())
    };
    for column in columns {
        let mut rest = column;
        for &v in v_cuts {
            let (low, high) = rest.split_at(k, v, SurfParamType::V)?;
            place(k, &mut b, low)?;
            rest = high;
        }
        place(k, &mut b, rest)?;
    }
    b.refresh_cached_topology();
    if made.len() > 1 {
        stitch_faces(k, &mut b, tolerance, None, false)?;
        b.refresh_cached_topology();
    }
    Ok((b, made))
}

const OP_SHEET_OF: &str = "topology::brep::sheet::sheet_of";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::nurbs::math::NOW;
    use crate::primitive::api_curves::{create_arc, create_line_segment};
    use crate::sweep::api_surfaces::create_skin_primitive;
    use crate::topology::EdgeId;

    /// A row of the crease's net at `x`, `z`: y from 0 to 4.
    fn row(x: f64, z: f64) -> Vec<CPoint> {
        vec![CPoint::new(x, 0.0, z, NOW), CPoint::new(x, 4.0, z, NOW)]
    }

    /// A surface of degree 1 both ways on the rows at `(x, z)`, over u knots `u`.
    fn spline(rows: &[(f64, f64)], u: &[f64]) -> NurbSurface {
        NurbSurface::new(
            rows.iter().map(|&(x, z)| row(x, z)).collect(),
            1,
            1,
            u.to_vec(),
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap()
    }

    /// The crease: flat from x = 0 to 5, then up to z = 5; its knot u = 0.5 the crease.
    fn crease() -> NurbSurface {
        spline(
            &[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)],
            &[0.0, 0.0, 0.5, 1.0, 1.0],
        )
    }

    fn of(n: &NurbSurface) -> Surface {
        Surface::from(BSplineSurface::from_gw_nurb(n))
    }

    /// The net and knots, bit for bit.
    fn bits(s: &NurbSurface) -> (Vec<u64>, Vec<u64>, Vec<[u64; 4]>) {
        let knots = |x: &[f64]| x.iter().map(|t| t.to_bits()).collect();
        let net = s
            .rows()
            .flatten()
            .map(|c| [c.x.to_bits(), c.y.to_bits(), c.z.to_bits(), c.w.to_bits()])
            .collect();
        (knots(s.u_knots()), knots(s.v_knots()), net)
    }

    #[test]
    fn a_sheet_cut_from_a_surface_gives_the_surface_back() {
        let mut k = Kernel::default();
        let s = crease();
        let (b, made) = sheet_of(&mut k, of(&s), &[0.5], &[], 1e-6).unwrap();
        assert_eq!(made.len(), 2);
        let pieces = one_surface(&k, &b).unwrap().unwrap();
        assert_eq!(pieces.u_cuts, [0.5]);
        assert!(pieces.v_cuts.is_empty());
        // the faces in the order the sheet made them
        let ids = |x: &[(FaceId, Extent2d)]| x.iter().map(|(f, _)| *f).collect::<Vec<_>>();
        assert_eq!(ids(&pieces.faces), ids(&made));
        let SurfaceKind::BSpline(back) = pieces.surface.kind() else {
            panic!("not a B-spline: {:?}", pieces.surface.kind());
        };
        assert_eq!(bits(back.gw_nurb().unwrap()), bits(&s));
        // the first piece is on the whole box's u min side and both v sides, not its u max
        let (_, first) = made[0];
        assert!(pieces.on_side(&first, BoxSide::UMin));
        assert!(!pieces.on_side(&first, BoxSide::UMax));
        assert!(pieces.on_side(&first, BoxSide::VMin) && pieces.on_side(&first, BoxSide::VMax));
    }

    #[test]
    fn faces_that_do_not_continue_each_other_are_not_one_surface() {
        let mut k = Kernel::default();
        let left = spline(&[(0.0, 0.0), (5.0, 0.0)], &[0.0, 0.0, 0.5, 0.5]);
        let sheet = |k: &mut Kernel, right: &NurbSurface| {
            let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
            for n in [&left, right] {
                let s = of(n);
                let bx = s.natural_uv_domain_with_nurb(k).unwrap();
                face_on(k, &mut b, s, &bx).unwrap();
            }
            b.refresh_cached_topology();
            b
        };
        // the right piece where the left one ends: one surface
        let meets = spline(&[(5.0, 0.0), (5.0, 5.0)], &[0.5, 0.5, 1.0, 1.0]);
        let b = sheet(&mut k, &meets);
        assert!(one_surface(&k, &b).unwrap().is_some());
        // lifted off it by 1: not
        let lifted = spline(&[(5.0, 1.0), (5.0, 6.0)], &[0.5, 0.5, 1.0, 1.0]);
        let b = sheet(&mut k, &lifted);
        assert!(one_surface(&k, &b).unwrap().is_none());
    }

    /// The flat arch: the line from (10, 0) to (0, 0) and the half circle about (5, 5) of
    /// radius 5 from (10, 5) over (5, 10) to (0, 5) (two quadratic spans at a double knot),
    /// skinned in the plane z = 0.
    fn flat_arch(k: &mut Kernel) -> Brep {
        let line = Curve::from(
            create_line_segment(Point3::new(10.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)).unwrap(),
        );
        let arc = Curve::from(create_arc(Point3::new(5.0, 5.0, 0.0), 5.0, 0.0, 180.0).unwrap());
        create_skin_primitive(k, &[1, 1], &[Some(&line), Some(&arc)], 1, false)
            .unwrap()
            .brep
    }

    /// The edge of `b` through `p`.
    fn edge_through(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
        let p = Point3::new(p[0], p[1], p[2]);
        crate::topology::brep::p1::edges(b, None)
            .unwrap()
            .into_iter()
            .find(|&e| {
                crate::query::api_queries::edge_closest_point(k, b, e, p)
                    .is_ok_and(|c| c.closest.distance < 1e-9)
            })
            .unwrap_or_else(|| panic!("no edge through {p:?}"))
    }

    fn mapped(m: &Merged, e: EdgeId) -> Option<EdgeId> {
        m.edges.iter().find(|(o, _)| *o == e).unwrap().1
    }

    #[test]
    fn the_flat_arch_merges_into_one_face_its_halves_into_one_edge() {
        let mut k = Kernel::default();
        let b = flat_arch(&mut k);
        // the skin split the wall at the arc's double knot: two faces, their seam x = 5
        assert_eq!(b.face_list().len(), 2);
        let d = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
        let halves = [
            edge_through(&k, &b, [5.0 + d, 5.0 + d, 0.0]),
            edge_through(&k, &b, [5.0 - d, 5.0 + d, 0.0]),
        ];
        let bottom = [
            edge_through(&k, &b, [7.5, 0.0, 0.0]),
            edge_through(&k, &b, [2.5, 0.0, 0.0]),
        ];
        let seam = edge_through(&k, &b, [5.0, 5.0, 0.0]);
        assert_ne!(halves[0], halves[1]);
        let (faces_before, edges_before) = (b.face_list().to_vec(), b.edge_list().to_vec());

        let m = merged_faces(&mut k, &b).unwrap();
        // one face of four edges: the bottom, the two sides and the arc
        let faces = faces(&m.brep, None).unwrap();
        assert_eq!(faces.len(), 1);
        assert_eq!(
            crate::topology::brep::p1::edges(&m.brep, None)
                .unwrap()
                .len(),
            4
        );
        // both half arcs are one edge now, and so are both halves of the bottom
        let arc = mapped(&m, halves[0]);
        assert!(arc.is_some());
        assert_eq!(mapped(&m, halves[1]), arc);
        let line = mapped(&m, bottom[0]);
        assert!(line.is_some() && line != arc);
        assert_eq!(mapped(&m, bottom[1]), line);
        // the seam is gone
        assert_eq!(mapped(&m, seam), None);
        // both faces are the merged face
        assert_eq!(m.faces.len(), 2);
        assert!(m.faces.iter().all(|&(_, f)| f == faces[0]));
        // every edge of the sheet is mapped, and the sheet is unchanged
        assert_eq!(m.edges.len(), edges_before.len());
        assert_eq!(b.face_list(), faces_before.as_slice());
        assert_eq!(b.edge_list(), edges_before.as_slice());
    }
}
