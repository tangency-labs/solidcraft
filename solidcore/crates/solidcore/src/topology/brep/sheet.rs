//! A one-face sheet on a surface's whole natural box ([`sheet_on`]), the kernel's one
//! arena-notify `CreateFaceFromSurface` wrapper, a face on a box of a surface's parameters
//! ([`face_on`]), a sheet with its faces merged where they are one face ([`merged_faces`]),
//! a sheet turned over ([`turn_over_shell`], [`turn_over_sheet`]), and a planar face rebuilt
//! as the Coons spline of its edges ([`face_spline_sheet`]).
//!
//! Not upstream: the composition an operation that rebuilds a face on a new surface (a sheet
//! extended past a side) makes of `new_brep` and `SmBrep::CreateFaceFromSurface`, kept in the
//! kernel so the modelling layer holds no notification plumbing; and the turn-over of every
//! face of an open shell ([`crate::topology::face::p1::turn_over`]); and a planar face's spline
//! sheet, so the control-net edits of the modelling layer have a net on a flat face.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::collections::BTreeSet;

use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::USE_DEFAULT;
use crate::kernel::Kernel;
use crate::surface::surface::Surface;
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use crate::topology::face::p1::turn_over;
use crate::topology::topology_traverser::Marks;
use crate::topology::{Brep, FaceId, ShellId};
use crate::EntityRef;

use crate::curve::curve::CurveKind;
use crate::geom::tol::{context_zone_tol_3d, SmallTopoType};
use crate::math::Point3;
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::primitive::api_curve_ops::make_curves_compatible;
use crate::surface::surface::SurfaceKind;
use crate::topology::edge::create_trimmed_nurbs_curve;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as fp2;
use crate::topology::loopuse;

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
    if fp2::loops(b, f, false, None)?.len() != 1 {
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
    let (sheet, _) = sheet_on(k, patch, b.tolerance())?;
    Ok(Some(sheet))
}

#[cfg(test)]
mod merged_tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::primitive::api_curves::{create_arc, create_line_segment};
    use crate::sweep::api_surfaces::create_skin_primitive;
    use crate::topology::EdgeId;

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
        let faces = crate::topology::brep::p1::faces(&m.brep, None).unwrap();
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
