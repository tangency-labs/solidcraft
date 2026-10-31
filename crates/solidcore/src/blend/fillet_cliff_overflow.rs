//! The cliff overflow of a convex blend (not upstream): a blend whose rails leave the faces
//! they roll on across cliffs (an opening's rim, the body's outline) is built as if those
//! faces went on past the cliffs, and the material it removes is subtracted from the body.
//!
//! Not upstream: the SolidCraft fork's `source/SMLib/src/SmFilletCliffOverflow.cpp`
//! (tangency-labs/smlib `8ba4667` and `ae2a6de`, rev `68fb9162`; `eddf0ed` and `bba9914`,
//! branch `replay/1.0.1`), behind `fixes.fillet_cliff_overflow` (WR-631, WR-638, WR-635,
//! WR-636). The executive computes the fillet on the
//! support faces' surfaces (`m_bCliffOverflow`): its fillet B-rep is the sheet of fillet and
//! corner faces, its boundary on the surfaces of the faces the blends roll on and end on.
//! The session's `SubtractRemovalRegion`
//!   1. cuts the fillet off at the ends its Nx1 corners left uncut, by the end face's
//!      surface continued ([`cut_fillet_at_open_end`]);
//!   2. closes the sheet into the region the blends remove ([`close_removal_region`]): on
//!      each target face the sheet's boundary runs on, a face on that face's surface,
//!      continued exactly where it needs to be, bounded by the sheet's boundary there, the
//!      edges blended, and the side edges (continued along their lines past their ends
//!      when they must be) that join the two;
//!   3. subtracts that region from the target with the manifold Boolean.
//!
//! Exactness: a planar surface continues exactly (its plane over a larger domain: a plane,
//! or a B-spline SMLib finds planar, a loft's wall; WR-635, `eddf0ed`); any other
//! surface is used within its own domain, and a needed continuation of one is refused
//! (`FilletErrorType::NotExact`). A side edge continues past its end only when it is a
//! straight line. An end the fillet runs past is cut only by a planar end face.
//! Configurations the construction does not cover stay too large for the faces
//! (`FilletErrorType::NoRoom`, the reason in the error's comment).
//!
//! | Rust                         | Fork                                   |
//! |------------------------------|----------------------------------------|
//! | `curve_ends` (private)       | `sm_CurveEnds`                         |
//! | `add_curve` (private)        | `sm_AddCurve`                          |
//! | `edge_curve` (private)       | `sm_EdgeCurve`                         |
//! | [`distance_to_surface`]      | `sm_DistanceToSurface`                 |
//! | `samples` (private)          | `sm_Samples`                           |
//! | [`covering_surface`]         | `sm_CoveringSurface`                   |
//! | `same` (private)             | `sm_Same`                              |
//! | [`cut_fillet_at_open_end`]   | `SmFilletExecutive::CutFilletAtOpenEnd` |
//! | [`close_removal_region`]     | `SmFilletExecutive::CloseRemovalRegion` |
//! | [`lamina_hosts`]             | its host test                          |
//! | [`lamina_chains`]            | its chain walk                         |
//! | [`closing_face`]             | its closing face of one chain          |
//! | [`plane_of`]                 | `sm_PlaneOf` (`eddf0ed`)               |
//! | [`lies_on_surface`]          | `sm_LiesOnSurface` (`eddf0ed`)         |
//! | `loop_area` (private)        | `sm_LoopArea` (`bba9914`)              |
//! | `reverse_loop` (private)     | `sm_Reverse` (`bba9914`)               |
//! | [`close_round_loop`]         | `sm_CloseRoundLoop` (`bba9914`)        |
//!
//! Deviations:
//! - **Errors.** `GetFilletErrorInfo()->NoteError(code, comment)` followed by `SM_ERR`
//!   becomes a [`Refusal`] written to the caller's note, then `SM_ERR`.
//! - **Sets and maps.** The fork's pointer-keyed `std::set` / `std::map` are `BTreeSet` /
//!   `BTreeMap` keyed by ids, used for membership only; every loop follows a vector (the
//!   lamina edges in the sheet's edge order, the hosts in first-seen order, the chains).
//! - **Bounded walks.** The chain walks (`for(;;)` in the fork) stop with `SM_ERR` after as
//!   many steps as there are lamina edges; the walk along the filleted edges keeps the
//!   fork's bound (the host's edge count).
//! - **Curve ownership.** The closing face's curves are owned values handed to
//!   `MakeFaceWithCurves`; the fork's `sm_Loop` deletion on failure is the drop.
//! - **Connector line.** The fork's `new SmLine( sV, sP, 3, &m_crContext )` is the
//!   two-point line `SmLine(StartPoint, EndPoint, lDim, cpContext)` ([`Line::from_points`]).

use std::collections::{BTreeMap, BTreeSet};

use crate::blend::fillet_executive::p3::NoRoomReason;
use crate::blend::fillet_geom::p1::FilletErrorType;
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec2, Vec3, BIG_DOUBLE, EFF_ZERO};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::face::p1::{point_3d_classify, Point3dClassifyOptions};
use crate::surface::plane::Plane;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::brep::p3::{delete_face, with_kernel_callbacks, DeleteFaceOptions};
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::brep::planar_section::face_planar_section_curves;
use crate::topology::topology_traverser::Marks;
use crate::topology::{edge, face, vertex, Brep, EdgeId, FaceId, VertexId};

const OP: &str = "blend::fillet_cliff_overflow";

/// `smos_Max` of two doubles.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min` of two doubles.
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// A refusal of the cliff overflow's construction: the fork's `NoteError(code, comment)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    /// The fillet error code (`SM_FILERR_NO_ROOM`, `SM_FILERR_NOT_EXACT` or
    /// `SM_FILERR_BAD_MERGE`).
    pub code: FilletErrorType,
    /// The typed reason of a `NoRoom` refusal.
    pub reason: Option<NoRoomReason>,
    /// The error's comment.
    pub comment: &'static str,
}

impl Refusal {
    const fn no_room(reason: NoRoomReason, comment: &'static str) -> Self {
        Self {
            code: FilletErrorType::NoRoom,
            reason: Some(reason),
            comment,
        }
    }

    const fn not_exact(comment: &'static str) -> Self {
        Self {
            code: FilletErrorType::NotExact,
            reason: None,
            comment,
        }
    }
}

/// Write `refusal` to `note` and fail (`NoteError`, then `return SM_ERR`).
fn refuse<T>(note: &mut Option<Refusal>, refusal: Refusal) -> Result<T> {
    *note = Some(refusal);
    Err(SolidError::sm_err(OP))
}

const BRANCHES: Refusal = Refusal::no_room(
    NoRoomReason::CliffBoundaryBranches,
    "Fillet Error: a cliff overflow's fillet boundary branches on a face",
);

// ---- helpers ------------------------------------------------------------------------------

/// `sm_CurveEnds`: the two ends of a curve over its natural interval.
fn curve_ends(k: &Kernel, curve: &Curve) -> Result<(Point3, Point3)> {
    let ivl = curve.natural_interval();
    Ok((
        curve.evaluate_point(k, ivl.min())?,
        curve.evaluate_point(k, ivl.max())?,
    ))
}

/// A curve of a closing face's loop and its orientation (`sm_LoopCurve`).
#[derive(Debug, Clone)]
struct LoopCurve {
    curve: Curve,
    orient: OrientType,
}

/// `sm_AddCurve`: adds `curve` to `lp`, oriented to run from `from` to `to`.
fn add_curve(
    k: &Kernel,
    lp: &mut Vec<LoopCurve>,
    curve: Curve,
    from: Point3,
    to: Point3,
) -> Result<()> {
    let (start, end) = curve_ends(k, &curve)?;
    let same = start.distance(from) + end.distance(to);
    let opposite = start.distance(to) + end.distance(from);
    let orient = if same <= opposite {
        OrientType::Same
    } else {
        OrientType::Opposite
    };
    lp.push(LoopCurve { curve, orient });
    Ok(())
}

/// `sm_EdgeCurve`: a copy of the edge's curve over the edge's interval.
fn edge_curve(k: &Kernel, b: &Brep, e: EdgeId) -> Result<Curve> {
    let curve = edge::curve(b, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let mut copy = curve.copy(k)?;
    let mut ivl = edge::interval(b, e)?;
    if ivl != copy.natural_interval() {
        copy.trim(k, &mut ivl, TrimMode::Notify)?;
    }
    Ok(copy)
}

/// The plane a planar surface lies on, by an origin and orthonormal axes `(x, y, z)`.
#[derive(Debug, Clone, Copy)]
pub struct PlaneFrame {
    /// The origin.
    pub origin: Point3,
    /// The first axis in the plane.
    pub x: Vec3,
    /// The second axis in the plane.
    pub y: Vec3,
    /// The unit normal.
    pub z: Vec3,
}

/// `sm_PlaneOf` (WR-635, the fork's `eddf0ed`): the plane a planar surface lies on: a
/// plane's own frame, or for any other surface SMLib finds planar (`IsPlanar(SM_EFF_ZERO)`:
/// a ruled B-spline wall between parallel lines, a loft's planar side) its plane through
/// the point `IsPlanar` gives, its first axis the normal crossed with the coordinate axis
/// it leans on least. `None` when the surface is not planar.
///
/// # Errors
/// `SM_ERR` for a zero normal or axis; those of `IsPlanar`.
pub fn plane_of(k: &Kernel, surface: &Surface) -> Result<Option<PlaneFrame>> {
    if let SurfaceKind::Plane(plane) = surface.kind() {
        let frame = plane.position();
        return Ok(Some(PlaneFrame {
            origin: frame.origin(),
            x: frame.x_axis(),
            y: frame.y_axis(),
            z: frame.z_axis(),
        }));
    }
    let planar = surface.is_planar(k, EFF_ZERO)?;
    if !planar.is_planar {
        return Ok(None);
    }
    let normal = planar
        .normal
        .unitize()
        .map_err(|_| SolidError::sm_err(OP))?;
    // an axis in the plane: the normal crossed with the coordinate axis it leans on least
    let mut axis = Vec3::new(1.0, 0.0, 0.0);
    if normal.y.abs() < normal.x.abs() && normal.y.abs() <= normal.z.abs() {
        axis = Vec3::new(0.0, 1.0, 0.0);
    } else if normal.z.abs() < normal.x.abs() {
        axis = Vec3::new(0.0, 0.0, 1.0);
    }
    let x = normal
        .cross(axis)
        .unitize()
        .map_err(|_| SolidError::sm_err(OP))?;
    let y = normal.cross(x);
    Ok(Some(PlaneFrame {
        origin: planar.point,
        x,
        y,
        z: normal,
    }))
}

/// `sm_DistanceToSurface`: the distance from `point` to the surface of `f` continued, as far
/// as it continues exactly: a plane's to the whole plane, any other surface's to its own
/// domain (`BIG_DOUBLE` when the drop fails). A planar surface of any kind ([`plane_of`];
/// WR-635, `eddf0ed`) continues as its plane.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` for a face without surface; those of the drop.
pub fn distance_to_surface(k: &Kernel, b: &Brep, f: FaceId, point: Point3) -> Result<f64> {
    let surface = face::p2::surface(b, f)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    if let Some(frame) = plane_of(k, surface)? {
        return Ok((point - frame.origin).dot(frame.z).abs());
    }
    let surface = surface.clone_for_evaluation()?;
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let dropped =
        surface.drop_point::<NoObject>(k, point, &domain, &DropPointOptions::default())?;
    Ok(if dropped.success {
        dropped.gap
    } else {
        BIG_DOUBLE
    })
}

/// `sm_Samples`: `count + 1` points along a curve's natural interval, its ends among them,
/// appended to `points`.
fn samples(k: &Kernel, curve: &Curve, count: u32, points: &mut Vec<Point3>) -> Result<()> {
    let ivl = curve.natural_interval();
    for i in 0..=count {
        points.push(curve.evaluate_point(k, ivl.evaluate(f64::from(i) / f64::from(count)))?);
    }
    Ok(())
}

/// `sm_CoveringSurface`: a surface that is `f`'s surface, whose domain holds every point of
/// `points` (all on it): a plane the same plane over a domain that covers them (a margin of
/// 1% of the box's diagonal and `10 tol`), any other surface a copy of itself when its own
/// domain does (`None`, not exact, when it does not).
///
/// # Errors
/// `SM_ERR` for no points on a plane; `SM_ERR_NULL_POINTER` for a face without surface;
/// those of the distances and the copy.
pub fn covering_surface(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    points: &[Point3],
    tol: f64,
) -> Result<Option<Surface>> {
    let surface = face::p2::surface(b, f)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    if let Some(frame) = plane_of(k, surface)? {
        // the plane through that origin along those axes, at unit scale: the same points
        let origin = frame.origin;
        let (x, y) = (frame.x, frame.y);
        let mut bbox: Option<Extent2d> = None;
        for &p in points {
            let d = p - origin;
            let uv = Point2::new(d.dot(x), d.dot(y));
            match bbox.as_mut() {
                None => bbox = Some(Extent2d::from_points(uv, uv)),
                Some(bx) => bx.add_point(uv),
            }
        }
        let Some(bbox) = bbox else {
            return Err(SolidError::sm_err(OP));
        };
        let margin = 0.01 * bbox.size().length() + 10.0 * tol;
        let domain = Extent2d::from_bounds(
            bbox.min().x - margin,
            bbox.min().y - margin,
            bbox.max().x + margin,
            bbox.max().y + margin,
        );
        return Ok(Some(Surface::from(Plane::new(
            origin,
            x,
            y,
            Vec2::new(1.0, 1.0),
            &domain,
        ))));
    }
    // Within its own domain, or not exactly.
    for &p in points {
        if distance_to_surface(k, b, f, p)? > tol {
            return Ok(None);
        }
    }
    Ok(Some(surface.copy(k)?))
}

/// `sm_LiesOnSurface` (WR-635, `eddf0ed`): whether the sheet face `sheet_face` of `region`
/// lies on the surface of the target face `f` continued: its surface, over a 5 x 5 grid
/// across the face's UV box, within `tol` of it ([`distance_to_surface`]). An end face of an
/// Nx1 corner of mixed convexity lies on the end face's surface extended.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` for a face without surface; those of the evaluations and
/// distances.
pub fn lies_on_surface(
    k: &Kernel,
    region: &Brep,
    sheet_face: FaceId,
    target: &Brep,
    f: FaceId,
    tol: f64,
) -> Result<bool> {
    let surface =
        face::p2::surface(region, sheet_face)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let bbox = face::p2::uv_domain(region, sheet_face)?;
    let size = bbox.size();
    let mut on = true;
    for i in 0..=4u32 {
        if !on {
            break;
        }
        for j in 0..=4u32 {
            if !on {
                break;
            }
            let uv = Point2::new(
                bbox.min().x + size.x * f64::from(i) / 4.0,
                bbox.min().y + size.y * f64::from(j) / 4.0,
            );
            let pnt = surface.evaluate_point(k, uv)?;
            on = distance_to_surface(k, target, f, pnt)? <= tol;
        }
    }
    Ok(on)
}

/// `sm_Same`: whether two points coincide to `tol`.
fn same(a: Point3, b: Point3, tol: f64) -> bool {
    a.distance(b) <= tol
}

// ---- CutFilletAtOpenEnd -------------------------------------------------------------------

/// The fork's `SmFilletExecutive::CutFilletAtOpenEnd`: cut the fillet off at an end its Nx1
/// corner left uncut, by the end face's surface continued.
///
/// The fillet faces of the end's solver (`faces`, one per fillet geometry, the face its
/// fillet surface is on; `None` for none) run on past the end, their rails past their side
/// edges. The end face (`end_surface`, planar) continues as its plane: the plane sections
/// each fillet face, its section nearest the end's vertex (`vertex_point`, the end rib)
/// splits it (`MergeCurvesOnSurface` with the plane as the intersection surface), and of the
/// two pieces the one beyond the end is deleted: the piece kept holds the fillet's point
/// nearest `edge_mid`, the middle of the filleted edge. A kept new piece becomes the
/// geometry's face in `faces`. Refused (`NoRoom`, in `note`) for an end face that is not
/// planar and for a rib that does not split a face in two or part it from its middle.
///
/// # Errors
/// The refusals as `SM_ERR`; `SM_ERR` for a failed drop of the edge's middle; those of the
/// section, the split, the classifications and the deletion.
#[allow(clippy::too_many_arguments)]
pub fn cut_fillet_at_open_end(
    k: &mut Kernel,
    region: &mut Brep,
    faces: &mut [Option<FaceId>],
    end_surface: &Surface,
    vertex_point: Point3,
    edge_mid: Point3,
    tol: f64,
    note: &mut Option<Refusal>,
) -> Result<()> {
    let planar = end_surface.is_planar(k, EFF_ZERO)?;
    if !planar.is_planar {
        return refuse(
            note,
            Refusal::no_room(
                NoRoomReason::CliffEndFaceNotPlanar,
                "Fillet Error: a cliff overflow runs past a fillet end whose end face is not planar",
            ),
        );
    }
    let (at, normal) = (planar.point, planar.normal);
    let cut = Plane::from_normal(at, normal);

    for slot in faces.iter_mut() {
        let Some(f) = *slot else {
            continue;
        };
        let surface = face::p2::surface(region, f)?
            .ok_or_else(|| SolidError::null_pointer(OP))?
            .clone_for_evaluation()?;
        let uv_domain = face::p2::uv_domain(region, f)?;
        let dropped = surface.drop_point::<NoObject>(
            k,
            edge_mid,
            &uv_domain,
            &DropPointOptions::default(),
        )?;
        if !dropped.success {
            return Err(SolidError::sm_err(OP));
        }
        let keep = surface.evaluate_point(k, dropped.result_uv)?;

        // section the face by the end's plane, and split it there
        let section = face_planar_section_curves(
            k,
            region,
            f,
            &surface,
            &uv_domain,
            at,
            normal,
            Some(tol),
            None,
        )?;
        if section.curves_3d.is_empty() {
            continue; // this face does not reach the end
        }
        // The end rib: the section nearest the edge's end (a fillet traced far past its end
        // can cross the plane again out there, where it is cut off anyway).
        let mut rib = 0;
        let mut nearest = BIG_DOUBLE;
        for (c, curve) in section.curves_3d.iter().enumerate() {
            let on = curve.drop_point(
                k,
                &curve.natural_interval(),
                vertex_point,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )?;
            if on.success && on.distance < nearest {
                nearest = on.distance;
                rib = c;
            }
        }
        let mut rib_3d: Vec<Curve> = section.curves_3d.get(rib).cloned().into_iter().collect();
        let rib_uv: Option<Vec<Option<Curve>>> = (section.uv_curves.len()
            == section.curves_3d.len())
        .then(|| vec![section.uv_curves.get(rib).cloned().flatten()]);
        let (new_faces, _) = with_kernel_callbacks(k, region, |k, b, env| {
            let serial = b.notify_serial();
            let curve_ref = |e: EdgeId, kind: i64| crate::geom::object::ObjectRef {
                key: match serial {
                    Some(s) => crate::topology::brep::p3::topology_ref(s, EntityRef::Edge(e)).key,
                    None => crate::topology::loopuse::notify_ref(EntityRef::Edge(e)).key,
                },
                type_code: kind,
            };
            crate::topology::brep::p10::merge_curves_on_surface_through_surface(
                k,
                b,
                env,
                &curve_ref,
                f,
                tol,
                &mut rib_3d,
                rib_uv.as_deref(),
                Some(Surface::from(cut.clone())),
            )
        })?;
        let [new_face] = new_faces[..] else {
            if new_faces.is_empty() {
                continue; // the section only touches it
            }
            return refuse(
                note,
                Refusal::no_room(
                    NoRoomReason::CliffEndNotCut,
                    "Fillet Error: the end face's plane does not cut a fillet that overflows its end in two",
                ),
            );
        };
        // keep the piece holding the fillet's middle, delete the one beyond the end
        let pieces = [f, new_face];
        let mut holds = [false, false];
        for (piece, held) in pieces.iter().zip(holds.iter_mut()) {
            let zone = face::p2::tolerance(region, *piece)?;
            let mut class = PointClassification::new(zone);
            point_3d_classify(
                k,
                region,
                *piece,
                keep,
                zone,
                true,
                &mut class,
                Point3dClassifyOptions::default(),
            )?;
            *held = class.point_class() != PointClassType::Unknown;
        }
        let [held_first, held_new] = holds;
        if held_first == held_new {
            return refuse(
                note,
                Refusal::no_room(
                    NoRoomReason::CliffEndNotCut,
                    "Fillet Error: the end face's plane does not part a fillet that overflows its end from its middle",
                ),
            );
        }
        let beyond = if held_first { new_face } else { f };
        if held_new {
            // the kept piece is the fillet surface's face from here on
            *slot = Some(new_face);
        }
        with_kernel_callbacks(k, region, |k, b, env| {
            let mut marks = Marks::default();
            delete_face(
                k,
                b,
                env,
                &mut marks,
                beyond,
                DeleteFaceOptions {
                    delete_edges_vertices: true,
                    ..DeleteFaceOptions::default()
                },
            )
        })?;
    }
    Ok(())
}

// ---- CloseRemovalRegion -------------------------------------------------------------------

/// The filleted edges of a cliff overflow and what [`close_removal_region`] reads of them.
#[derive(Debug, Clone, Default)]
pub struct Blended {
    /// `sBlended`: the filleted edges of the target.
    pub edges: BTreeSet<EdgeId>,
    /// `sBlendedVerts`: their vertices.
    pub vertices: BTreeSet<VertexId>,
    /// `vHosts`: the faces round those vertices, in first-seen order.
    pub hosts: Vec<FaceId>,
    /// `sEdgeOfFillet` (WR-636, `bba9914`): the filleted edge of each fillet face of the
    /// sheet (the face each solver's fillet geometries' surfaces are on, in solver then
    /// geometry order, a later entry replacing an earlier one).
    pub edge_of_fillet: BTreeMap<FaceId, EdgeId>,
}

impl Blended {
    /// The filleted edges `edges` (one per solver, in solver order), their vertices and the
    /// faces round them, each face once in the order first seen (`GetFaces` of each start
    /// then end vertex).
    ///
    /// # Errors
    /// `SM_ERR_NULL_POINTER` for an edge without both vertices; those of the lookups.
    pub fn of_edges(target: &Brep, edges: &[EdgeId]) -> Result<Self> {
        let mut out = Self::default();
        let mut host_set = BTreeSet::new();
        for &e in edges {
            out.edges.insert(e);
            let (start, end) = edge::start_end_vertices(target, e)?;
            for v in [start, end] {
                let v = v.ok_or_else(|| SolidError::null_pointer(OP))?;
                out.vertices.insert(v);
                for f in vertex::faces(target, v)? {
                    if host_set.insert(f) {
                        out.hosts.push(f);
                    }
                }
            }
        }
        Ok(out)
    }
}

/// The lamina edges of the sheet and their hosts (the first half of
/// `CloseRemovalRegion`).
#[derive(Debug, Clone, Default)]
pub struct LaminaHosts {
    /// `vLamina`: the sheet's lamina edges, in its edge order.
    pub lamina: Vec<EdgeId>,
    /// `sHostOf`: the target face each lies on.
    pub host_of: BTreeMap<EdgeId, FaceId>,
    /// `sLaminaAt`: the lamina edges at each sheet vertex, in [`Self::lamina`] order.
    pub at: BTreeMap<VertexId, Vec<EdgeId>>,
}

impl LaminaHosts {
    fn host(&self, e: EdgeId) -> Result<FaceId> {
        self.host_of
            .get(&e)
            .copied()
            .ok_or_else(|| SolidError::null_pointer(OP))
    }

    fn at(&self, v: VertexId) -> &[EdgeId] {
        self.at.get(&v).map_or(&[], Vec::as_slice)
    }
}

/// The host test of `CloseRemovalRegion`: each lamina edge of the sheet lies on the surface
/// (continued) of exactly one target face round the filleted edges' vertices, at five points
/// along it within `on_tol`. Refused when one lies on two (`NoRoom`) or none (`NotExact`).
///
/// # Errors
/// The refusals as `SM_ERR`; those of the curves, samples and distances.
pub fn lamina_hosts(
    k: &Kernel,
    region: &Brep,
    target: &Brep,
    hosts: &[FaceId],
    on_tol: f64,
    note: &mut Option<Refusal>,
) -> Result<LaminaHosts> {
    let mut out = LaminaHosts::default();
    for e in crate::topology::brep::p1::edges(region, None)? {
        if !edge::is_lamina(region, e)? {
            continue;
        }
        let curve = edge_curve(k, region, e)?;
        let mut points = Vec::new();
        samples(k, &curve, 4, &mut points)?;
        // the sheet's face along the edge (a lamina edge has one)
        let sheet_faces = edge::faces(region, e, None)?;
        let [sheet_face] = sheet_faces.as_slice() else {
            return Err(SolidError::sm_err(OP));
        };
        let mut host = None;
        for &f in hosts {
            // A sheet face on the face's own surface (an Nx1 corner's end face of mixed
            // convexity, on the end face's surface extended, `CreateCornerPatch`) stands in
            // for that face there: its lamina edge along a side edge lies on both faces'
            // surfaces, and the sheet's boundary passes along it onto the other face.
            if lies_on_surface(k, region, *sheet_face, target, f, on_tol)? {
                continue;
            }
            let mut on = true;
            for &p in &points {
                if !on {
                    break;
                }
                on = distance_to_surface(k, target, f, p)? <= on_tol;
            }
            if !on {
                continue;
            }
            if host.is_some() {
                return refuse(
                    note,
                    Refusal::no_room(
                        NoRoomReason::CliffBoundaryOnTwoFaces,
                        "Fillet Error: a cliff overflow's fillet boundary lies on the surfaces of two faces",
                    ),
                );
            }
            host = Some(f);
        }
        let Some(host) = host else {
            return refuse(
                note,
                Refusal::not_exact(
                    "Fillet Error: a cliff overflow's fillet boundary lies on no face's exact continuation",
                ),
            );
        };
        out.lamina.push(e);
        out.host_of.insert(e, host);
        let (start, end) = edge::start_end_vertices(region, e)?;
        for v in [start, end] {
            let v = v.ok_or_else(|| SolidError::null_pointer(OP))?;
            out.at.entry(v).or_default().push(e);
        }
    }
    Ok(out)
}

/// A chain of consecutive lamina edges on one host, from its start to its end vertex
/// (`sm_Chain`).
#[derive(Debug, Clone)]
pub struct Chain {
    /// `m_pHost`.
    pub host: FaceId,
    /// `m_vEdges`.
    pub edges: Vec<EdgeId>,
    /// `m_vVerts`: one more than the edges, the first again when closed.
    pub vertices: Vec<VertexId>,
    /// `m_bClosed` (WR-636, `bba9914`): the chain closes on its host.
    pub closed: bool,
}

/// The chain walk of `CloseRemovalRegion`: the lamina edges in chains of consecutive edges
/// on one host, each from the first edge not yet in a chain, forward from its end vertex,
/// then backward from its start vertex. Refused (`NoRoom`) where the boundary branches on a
/// face. A chain that comes back to its first edge is closed (WR-636, `bba9914`; it was
/// refused before).
///
/// # Errors
/// The refusals as `SM_ERR`; `SM_ERR` past the walk's bound (the lamina edge count);
/// those of the lookups.
pub fn lamina_chains(
    region: &Brep,
    hosts: &LaminaHosts,
    note: &mut Option<Refusal>,
) -> Result<Vec<Chain>> {
    let bound = hosts.lamina.len();
    let mut chains = Vec::new();
    let mut done: BTreeSet<EdgeId> = BTreeSet::new();
    for &first in &hosts.lamina {
        if done.contains(&first) {
            continue;
        }
        let host = hosts.host(first)?;
        // the next lamina edge on the same host at `v`, other than `from` (`None` at a
        // chain's end; `Err` at a branch)
        let next = |v: VertexId, from: EdgeId| -> Result<std::result::Result<Option<EdgeId>, ()>> {
            let mut found = None;
            for &e in hosts.at(v) {
                if e == from || hosts.host(e)? != host {
                    continue;
                }
                if found.is_some() {
                    return Ok(Err(())); // a branch of the boundary
                }
                found = Some(e);
            }
            Ok(Ok(found))
        };
        let (start, end) = edge::start_end_vertices(region, first)?;
        let start = start.ok_or_else(|| SolidError::null_pointer(OP))?;
        let end = end.ok_or_else(|| SolidError::null_pointer(OP))?;
        let mut chain = Chain {
            host,
            edges: vec![first],
            vertices: vec![start, end],
            closed: false,
        };
        done.insert(first);
        // forward from the first edge's end vertex
        let mut closed = false;
        let mut steps = 0;
        loop {
            steps += 1;
            if steps > bound {
                return Err(SolidError::sm_err(OP));
            }
            let (Some(&at), Some(&from)) = (chain.vertices.last(), chain.edges.last()) else {
                return Err(SolidError::null_pointer(OP));
            };
            let Ok(found) = next(at, from)? else {
                return refuse(note, BRANCHES);
            };
            let Some(found) = found else {
                break;
            };
            if found == first {
                closed = true;
                break;
            }
            let other = edge::other_vertex(region, found, Some(at))?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            chain.edges.push(found);
            chain.vertices.push(other);
            done.insert(found);
        }
        if closed {
            // round a loop of the host whose every edge is blended ([`close_round_loop`])
            chain.closed = true;
            chains.push(chain);
            continue;
        }
        // backward from the first edge's start vertex
        let mut at = start;
        let mut from = first;
        let mut steps = 0;
        loop {
            steps += 1;
            if steps > bound {
                return Err(SolidError::sm_err(OP));
            }
            let Ok(found) = next(at, from)? else {
                return refuse(note, BRANCHES);
            };
            let Some(found) = found else {
                break;
            };
            let other = edge::other_vertex(region, found, Some(at))?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            chain.edges.insert(0, found);
            chain.vertices.insert(0, other);
            done.insert(found);
            at = other;
            from = found;
        }
        chains.push(chain);
    }
    Ok(chains)
}

/// The side edge at one end of a chain: the filleted edges' vertex it starts at, and the
/// connector from that vertex to the chain's end (`None` when the chain ends at the vertex).
struct SideEnd {
    vertex: VertexId,
    connector: Option<Curve>,
}

/// At the chain's end `end_vertex` (of the sheet; its edge there `mine`), the side edge of
/// `host` the sheet's boundary passes along onto the next host, from a filleted edges'
/// vertex: the chain's end lies at the vertex, on the side edge, or on its line past its
/// other end (a line only).
#[allow(clippy::too_many_arguments)]
fn side_end(
    k: &Kernel,
    region: &Brep,
    target: &Brep,
    blended: &Blended,
    hosts: &LaminaHosts,
    host: FaceId,
    end_vertex: VertexId,
    mine: EdgeId,
    on_tol: f64,
    note: &mut Option<Refusal>,
) -> Result<SideEnd> {
    let mut next_host = None;
    for &e in hosts.at(end_vertex) {
        if e == mine {
            continue;
        }
        if next_host.is_some() || hosts.host(e)? == host {
            return refuse(note, BRANCHES);
        }
        next_host = Some(hosts.host(e)?);
    }
    let next_host = next_host.ok_or_else(|| SolidError::null_pointer(OP))?;
    let p = vertex::point(region, end_vertex)?;
    let mut inexact = false;
    let mut found: Option<SideEnd> = None;
    // A chain can end at a filleted edges' vertex itself (WR-635, `eddf0ed`): an Nx1
    // corner of mixed convexity extends its end face's surface past the vertex, along the
    // side edge of the filleted edge's convexity continued back (a pocket's rim edge, the
    // pocket's corner edge concave), and its end face, in the sheet, reaches the vertex.
    // The chain closes there: no side edge leads back to it.
    for v in face::p4::vertices(target, host, None, false, None)? {
        if found.is_some() {
            break;
        }
        if blended.vertices.contains(&v) && same(vertex::point(target, v)?, p, on_tol) {
            found = Some(SideEnd {
                vertex: v,
                connector: None,
            });
        }
    }
    for side in face::p4::edges(target, host, None, false, None)? {
        if found.is_some() {
            break;
        }
        if blended.edges.contains(&side) {
            continue;
        }
        if !edge::faces(target, side, None)?.contains(&next_host) {
            continue;
        }
        let (start, end) = edge::start_end_vertices(target, side)?;
        for v in [start, end] {
            let Some(v) = v else {
                continue;
            };
            if found.is_some() || !blended.vertices.contains(&v) {
                continue;
            }
            let pv = vertex::point(target, v)?;
            if same(pv, p, on_tol) {
                // the chain ends at the vertex itself: no connector
                found = Some(SideEnd {
                    vertex: v,
                    connector: None,
                });
                continue;
            }
            // on the side edge itself
            let side_curve =
                edge::curve(target, side)?.ok_or_else(|| SolidError::null_pointer(OP))?;
            let side_ivl = edge::interval(target, side)?;
            let on = side_curve.drop_point(
                k,
                &side_ivl,
                p,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )?;
            if on.success && on.distance <= on_tol {
                let on_v = side_curve.drop_point(
                    k,
                    &side_ivl,
                    pv,
                    None,
                    BIG_DOUBLE,
                    None,
                    SolverOperation::Minimize,
                )?;
                if !on_v.success || on_v.distance > on_tol {
                    return Err(SolidError::sm_err(OP));
                }
                let mut piece = side_curve.copy(k)?;
                let mut part = crate::geom::extent1d::Extent1d::from_min_max(
                    smos_min(on.parameter, on_v.parameter),
                    smos_max(on.parameter, on_v.parameter),
                );
                piece.trim(k, &mut part, TrimMode::Notify)?;
                found = Some(SideEnd {
                    vertex: v,
                    connector: Some(piece),
                });
                continue;
            }
            // on its line past its other end
            let w = edge::other_vertex(target, side, Some(v))?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let along = vertex::point(target, w)? - pv;
            let out = p - pv;
            let len = along.length();
            if len <= on_tol || out.dot(along) <= 0.0 {
                continue;
            }
            let cross = out.cross(along);
            if cross.length() / len > on_tol || out.length() <= len {
                continue;
            }
            if !side_curve.is_linear(k, EFF_ZERO, false)?.linear {
                inexact = true; // the chain's end lies along it, but it is no line
                continue;
            }
            found = Some(SideEnd {
                vertex: v,
                connector: Some(Curve::from(Line::from_points(pv, p, 3))),
            });
        }
    }
    match found {
        Some(found) => Ok(found),
        None if inexact => refuse(
            note,
            Refusal::not_exact(
                "Fillet Error: a cliff overflow needs a side edge continued past its end that is not a line",
            ),
        ),
        None => refuse(
            note,
            Refusal::no_room(
                NoRoomReason::CliffBoundaryLeavesAlongNoSideEdge,
                "Fillet Error: a cliff overflow's fillet boundary leaves a face along no side edge of the filleted edges",
            ),
        ),
    }
}

/// The closing face of one chain: on its host's surface, along the chain, back along the
/// side edge at its end to the filleted edges' vertex there, along the host's filleted
/// edges to the vertex at the chain's start, and out along that side edge to the chain's
/// start; on the host's covering surface, anticlockwise by the loop's signed area in the
/// surface's parameters; made in the sheet's infinite region.
///
/// # Errors
/// The refusals as `SM_ERR`; `SM_ERR` past the walk's bound and for a failed drop; those of
/// the curves, the covering surface and the face construction.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one fork routine
pub fn closing_face(
    k: &mut Kernel,
    region: &mut Brep,
    target: &Brep,
    blended: &Blended,
    hosts: &LaminaHosts,
    chain: &Chain,
    on_tol: f64,
    note: &mut Option<Refusal>,
) -> Result<FaceId> {
    let host = chain.host;
    let mut lp: Vec<LoopCurve> = Vec::new();

    // the chain, from its start to its end
    for (e, pair) in chain.edges.iter().zip(chain.vertices.windows(2)) {
        let [from, to] = pair else {
            return Err(SolidError::null_pointer(OP));
        };
        let curve = edge_curve(k, region, *e)?;
        add_curve(
            k,
            &mut lp,
            curve,
            vertex::point(region, *from)?,
            vertex::point(region, *to)?,
        )?;
    }

    // At each end of the chain, the side edge the sheet's boundary passes along onto the
    // next host, and the filleted edges' vertex it starts at.
    let (Some(&front_v), Some(&back_v), Some(&front_e), Some(&back_e)) = (
        chain.vertices.first(),
        chain.vertices.last(),
        chain.edges.first(),
        chain.edges.last(),
    ) else {
        return Err(SolidError::null_pointer(OP));
    };
    let start_side = side_end(
        k, region, target, blended, hosts, host, front_v, front_e, on_tol, note,
    )?;
    let end_side = side_end(
        k, region, target, blended, hosts, host, back_v, back_e, on_tol, note,
    )?;

    // from the chain's end back along its side edge to the vertex
    if let Some(connector) = end_side.connector {
        add_curve(
            k,
            &mut lp,
            connector,
            vertex::point(region, back_v)?,
            vertex::point(target, end_side.vertex)?,
        )?;
    }
    // along the filleted edges of the host from there to the other end's vertex
    let host_edges = face::p4::edges(target, host, None, false, None)?;
    let mut at = end_side.vertex;
    let mut from: Option<EdgeId> = None;
    let mut steps = 0;
    while at != start_side.vertex {
        if steps > host_edges.len() {
            return Err(SolidError::sm_err(OP));
        }
        steps += 1;
        let mut found = None;
        for &e in &host_edges {
            if Some(e) == from || !blended.edges.contains(&e) {
                continue;
            }
            let (s, t) = edge::start_end_vertices(target, e)?;
            if s != Some(at) && t != Some(at) {
                continue;
            }
            if found.is_some() {
                return refuse(
                    note,
                    Refusal::no_room(
                        NoRoomReason::CliffClosingFaceUnbounded,
                        "Fillet Error: a cliff overflow's closing face meets two filleted edges at a vertex",
                    ),
                );
            }
            found = Some(e);
        }
        let Some(found) = found else {
            return refuse(
                note,
                Refusal::no_room(
                    NoRoomReason::CliffClosingFaceUnbounded,
                    "Fillet Error: a cliff overflow's closing face is not bounded by filleted edges between its side edges",
                ),
            );
        };
        let other = edge::other_vertex(target, found, Some(at))?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let curve = edge_curve(k, target, found)?;
        add_curve(
            k,
            &mut lp,
            curve,
            vertex::point(target, at)?,
            vertex::point(target, other)?,
        )?;
        from = Some(found);
        at = other;
    }
    // out along the start's side edge to the chain's start
    if let Some(connector) = start_side.connector {
        add_curve(
            k,
            &mut lp,
            connector,
            vertex::point(target, start_side.vertex)?,
            vertex::point(region, front_v)?,
        )?;
    }

    // the host's surface over the loop, exactly
    let mut points = Vec::new();
    for c in &lp {
        samples(k, &c.curve, 8, &mut points)?;
    }
    let Some(surface) = covering_surface(k, target, host, &points, on_tol)? else {
        return refuse(
            note,
            Refusal::not_exact(
                "Fillet Error: a cliff overflow needs a face's surface continued past its domain, which is not a plane",
            ),
        );
    };

    // anticlockwise on the surface: the loop's points in its parameters, by their signed
    // area
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let mut uvs: Vec<Point2> = Vec::new();
    for c in &lp {
        let mut along = Vec::new();
        samples(k, &c.curve, 8, &mut along)?;
        if c.orient == OrientType::Opposite {
            along.reverse();
        }
        let count = along.len().saturating_sub(1);
        for &p in along.iter().take(count) {
            let dropped =
                surface.drop_point::<NoObject>(k, p, &domain, &DropPointOptions::default())?;
            if !dropped.success {
                return Err(SolidError::sm_err(OP));
            }
            uvs.push(dropped.result_uv);
        }
    }
    let mut area = 0.0;
    for (i, a) in uvs.iter().enumerate() {
        let Some(b) = uvs.get((i + 1) % uvs.len()) else {
            return Err(SolidError::null_pointer(OP));
        };
        area += a.x * b.y - b.x * a.y;
    }
    if area < 0.0 {
        lp.reverse();
        for c in &mut lp {
            c.orient = if c.orient == OrientType::Same {
                OrientType::Opposite
            } else {
                OrientType::Same
            };
        }
    }

    let input = FaceCurvesInput {
        loops: vec![lp.len()],
        curves_3d: Some(lp.iter().map(|c| Some(c.curve.clone())).collect()),
        uv_curves: None,
        orientations: lp.iter().map(|c| c.orient).collect(),
        loop_points: Vec::new(),
    };
    let made = with_kernel_callbacks(k, region, |k, b, env| {
        let region_id = b.infinite_region();
        let key_of = env.key_of;
        let face_ref = |f: FaceId| key_of(EntityRef::Face(f));
        make_face_with_curves(
            k,
            b,
            region_id,
            input,
            surface,
            &domain,
            OrientType::Same,
            None,
            &face_ref,
            env,
        )
    })?;
    made.face.ok_or_else(|| SolidError::null_pointer(OP))
}

/// The fork's `SmFilletExecutive::CloseRemovalRegion`: close `region`, the sheet of fillet
/// and corner faces, into the region the blends remove: a face on the surface of each
/// target face its boundary runs on.
///
/// Each lamina edge of the sheet lies on the surface (continued) of one target face round
/// the filleted edges' vertices: its host ([`lamina_hosts`]). Consecutive lamina edges on
/// one host form a chain ([`lamina_chains`]). Where a chain ends the sheet's boundary passes
/// onto the next host X, along the side edge s of the chain's host F that F shares with X
/// and that starts at a filleted edge's vertex v: the chain's end lies on s, or on s's line
/// past s's other end. The closing face on F's surface runs along the chain, back along s
/// to v, along the filleted edges of F from v to the other end's vertex, and out along
/// that end's side edge to the chain's start ([`closing_face`]). `tol` is the solvers'
/// largest tolerance; points lie on a surface within ten times it.
///
/// # Errors
/// The refusals as `SM_ERR`, noted in `note`; those of the steps.
pub fn close_removal_region(
    k: &mut Kernel,
    region: &mut Brep,
    target: &Brep,
    blended: &Blended,
    tol: f64,
    note: &mut Option<Refusal>,
) -> Result<()> {
    let on_tol = 10.0 * tol;
    let hosts = lamina_hosts(k, region, target, &blended.hosts, on_tol, note)?;
    let chains = lamina_chains(region, &hosts, note)?;
    for chain in &chains {
        if chain.closed {
            close_round_loop(k, region, target, blended, chain, on_tol, note)?;
            continue;
        }
        closing_face(k, region, target, blended, &hosts, chain, on_tol, note)?;
    }
    Ok(())
}

/// `sm_LoopArea` (WR-636, `bba9914`): the signed area of a loop of curves in a plane's
/// parameters (its frame's axes), from 16 samples a curve.
fn loop_area(k: &Kernel, lp: &[LoopCurve], frame: &PlaneFrame) -> Result<f64> {
    let mut uvs: Vec<Point2> = Vec::new();
    for c in lp {
        let mut along = Vec::new();
        samples(k, &c.curve, 16, &mut along)?;
        if c.orient == OrientType::Opposite {
            along.reverse();
        }
        let count = along.len().saturating_sub(1);
        for &p in along.iter().take(count) {
            let d = p - frame.origin;
            uvs.push(Point2::new(d.dot(frame.x), d.dot(frame.y)));
        }
    }
    let mut area = 0.0;
    for (i, a) in uvs.iter().enumerate() {
        let Some(b) = uvs.get((i + 1) % uvs.len()) else {
            return Err(SolidError::null_pointer(OP));
        };
        area += 0.5 * (a.x * b.y - b.x * a.y);
    }
    Ok(area)
}

/// `sm_Reverse`: reverses a loop of curves.
fn reverse_loop(lp: &mut [LoopCurve]) {
    lp.reverse();
    for c in lp {
        c.orient = if c.orient == OrientType::Same {
            OrientType::Opposite
        } else {
            OrientType::Same
        };
    }
}

/// `sm_CloseRoundLoop` (WR-636, the fork's `bba9914`): the closing face, on the host's
/// plane, of a chain of the sheet's boundary that closes on that one face: the blends of
/// every edge of one of its loops run all round it (a pocket's rim), and the face between
/// the chain and that loop is the region's there, the one loop inside the other. The loop
/// is the one whose every edge is blended and some of whose edges the chain's fillet faces
/// blend ([`Blended::edge_of_fillet`]). The larger loop (by its area on the host's plane)
/// runs anticlockwise and the other clockwise; the face is made in the sheet's infinite
/// region. Refused (`NoRoom`) when no such loop, or more than one, is found, or the host is
/// not planar.
///
/// # Errors
/// The refusals as `SM_ERR`; `SM_ERR` for a chain without one more vertex than edges or a
/// loop whose edges do not run end to end; those of the curves, the covering surface and
/// the face construction.
#[allow(clippy::too_many_lines)] // one fork routine
pub fn close_round_loop(
    k: &mut Kernel,
    region: &mut Brep,
    target: &Brep,
    blended: &Blended,
    chain: &Chain,
    on_tol: f64,
    note: &mut Option<Refusal>,
) -> Result<FaceId> {
    let host = chain.host;
    if chain.vertices.len() != chain.edges.len() + 1 {
        return Err(SolidError::sm_err(OP));
    }

    // the filleted edges whose fillet faces the chain bounds
    let mut along: BTreeSet<EdgeId> = BTreeSet::new();
    for &e in &chain.edges {
        for f in edge::faces(region, e, None)? {
            if let Some(&filleted) = blended.edge_of_fillet.get(&f) {
                along.insert(filleted);
            }
        }
    }
    // the host's loop of filleted edges they belong to
    let mut round = None;
    for lp in face::p2::loops(target, host, false, None)? {
        let edges = crate::topology::loopuse::loop_edges(target, lp)?;
        let mut all_blended = !edges.is_empty();
        let mut ours = false;
        for e in &edges {
            if !e.is_some_and(|e| blended.edges.contains(&e)) {
                all_blended = false;
            }
            if e.is_some_and(|e| along.contains(&e)) {
                ours = true;
            }
        }
        if !all_blended || !ours {
            continue;
        }
        if round.is_some() {
            return refuse(
                note,
                Refusal::no_room(
                    NoRoomReason::CliffBoundaryClosed,
                    "Fillet Error: a cliff overflow's fillet boundary closes on one face round two of its loops",
                ),
            );
        }
        round = Some(lp);
    }
    let Some(round) = round else {
        return refuse(
            note,
            Refusal::no_room(
                NoRoomReason::CliffBoundaryClosed,
                "Fillet Error: a cliff overflow's fillet boundary closes on one face round no loop of filleted edges",
            ),
        );
    };
    let host_surface =
        face::p2::surface(target, host)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let Some(frame) = plane_of(k, host_surface)? else {
        return refuse(
            note,
            Refusal::no_room(
                NoRoomReason::CliffBoundaryClosed,
                "Fillet Error: a cliff overflow's fillet boundary closes on one face that is not planar",
            ),
        );
    };

    // the chain, and the loop of filleted edges, each run end to end
    let mut loops: [Vec<LoopCurve>; 2] = [Vec::new(), Vec::new()];
    for (e, pair) in chain.edges.iter().zip(chain.vertices.windows(2)) {
        let [from, to] = pair else {
            return Err(SolidError::null_pointer(OP));
        };
        let curve = edge_curve(k, region, *e)?;
        add_curve(
            k,
            &mut loops[0],
            curve,
            vertex::point(region, *from)?,
            vertex::point(region, *to)?,
        )?;
    }
    let mut left: Vec<EdgeId> = Vec::new();
    for e in crate::topology::loopuse::loop_edges(target, round)? {
        left.push(e.ok_or_else(|| SolidError::null_pointer(OP))?);
    }
    let Some(&first) = left.first() else {
        return Err(SolidError::sm_err(OP));
    };
    let mut at = edge::start_end_vertices(target, first)?
        .0
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    while !left.is_empty() {
        let mut found = None;
        for (i, &e) in left.iter().enumerate() {
            let (s, t) = edge::start_end_vertices(target, e)?;
            if s == Some(at) || t == Some(at) {
                found = Some(i);
                break;
            }
        }
        let Some(i) = found else {
            return Err(SolidError::sm_err(OP));
        };
        let e = left.remove(i);
        let other =
            edge::other_vertex(target, e, Some(at))?.ok_or_else(|| SolidError::null_pointer(OP))?;
        let curve = edge_curve(k, target, e)?;
        add_curve(
            k,
            &mut loops[1],
            curve,
            vertex::point(target, at)?,
            vertex::point(target, other)?,
        )?;
        at = other;
    }

    // the host's plane over both, the outer loop anticlockwise on it and the inner clockwise
    let mut points = Vec::new();
    for lp in &loops {
        for c in lp {
            samples(k, &c.curve, 8, &mut points)?;
        }
    }
    let surface = covering_surface(k, target, host, &points, on_tol)?
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    let areas = [
        loop_area(k, &loops[0], &frame)?,
        loop_area(k, &loops[1], &frame)?,
    ];
    let outer = usize::from(areas[0].abs() < areas[1].abs());
    let inner = 1 - outer;
    if areas.get(outer).is_some_and(|&a| a < 0.0) {
        if let Some(lp) = loops.get_mut(outer) {
            reverse_loop(lp);
        }
    }
    if areas.get(inner).is_some_and(|&a| a > 0.0) {
        if let Some(lp) = loops.get_mut(inner) {
            reverse_loop(lp);
        }
    }
    // the plane built on the host's frame: its parameters are (x, y), as measured
    let mut counts = Vec::new();
    let mut curves = Vec::new();
    let mut orientations = Vec::new();
    for index in [outer, inner] {
        let lp = loops
            .get(index)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        counts.push(lp.len());
        for c in lp {
            curves.push(Some(c.curve.clone()));
            orientations.push(c.orient);
        }
    }
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let input = FaceCurvesInput {
        loops: counts,
        curves_3d: Some(curves),
        uv_curves: None,
        orientations,
        loop_points: Vec::new(),
    };
    let made = with_kernel_callbacks(k, region, |k, b, env| {
        let region_id = b.infinite_region();
        let key_of = env.key_of;
        let face_ref = |f: FaceId| key_of(EntityRef::Face(f));
        make_face_with_curves(
            k,
            b,
            region_id,
            input,
            surface,
            &domain,
            OrientType::Same,
            None,
            &face_ref,
            env,
        )
    })?;
    made.face.ok_or_else(|| SolidError::null_pointer(OP))
}
