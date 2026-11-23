//! The cliff overflow of a convex blend (not upstream): a blend whose rails leave the faces
//! they roll on across cliffs (an opening's rim, the body's outline) is built as if those
//! faces went on past the cliffs, and then trimmed at its creases with the body (WR-695,
//! [`crate::blend::fillet_overflow`] and the session's `trim_at_creases`).
//!
//! Not upstream: the SolidCraft fork's `source/SMLib/src/SmFilletCliffOverflow.cpp`
//! (tangency-labs/smlib `8ba4667` and `ae2a6de`, rev `68fb9162`; `eddf0ed` and `bba9914`,
//! branch `replay/1.0.1`; WR-631, WR-638, WR-635, WR-636). The executive computes the fillet on the
//! support faces' surfaces (`m_bCliffOverflow`): its fillet B-rep is the sheet of fillet and
//! corner faces, its boundary on the surfaces of the faces the blends roll on and end on.
//! What is left here is what the trim at creases reads: the fillet cut off at the ends its
//! Nx1 corners left uncut, by the end face's surface continued ([`cut_fillet_at_open_end`]),
//! the section ends of WR-674 and WR-706, and the surface helpers. The fork's
//! `SubtractRemovalRegion` and its closing of the sheet into the region the blends remove
//! (`CloseRemovalRegion`, `sm_CloseRoundLoop`, `sm_CloseBand`, `sm_CoveringSurface`,
//! `sm_LiesOnSurface`, and WR-674's closing of a section-ended round) went with the flag
//! `fillet_overflow_trim_at_creases`: the trim replaced them, as smlib#79 step 6 deletes
//! them in the fork.
//!
//! Exactness: a planar surface continues exactly (its plane over a larger domain: a plane,
//! or a B-spline SMLib finds planar, a loft's wall; WR-635, `eddf0ed`); a cylinder, cone,
//! sphere or torus continues as itself (`db4a655`, [`continued_surface`]); any other
//! surface is used within its own domain. An end the fillet runs past is cut only by a
//! planar end face. Configurations the construction does not cover stay too large for the
//! faces (`FilletErrorType::NoRoom`, the reason in the error's comment).
//!
//! | Rust                         | Fork                                   |
//! |------------------------------|----------------------------------------|
//! | `curve_ends` (private)       | `sm_CurveEnds`                         |
//! | `add_curve` (private)        | `sm_AddCurve`                          |
//! | `edge_curve` (private)       | `sm_EdgeCurve`                         |
//! | [`distance_to_surface`]      | `sm_DistanceToSurface`                 |
//! | `samples` (private)          | `sm_Samples`                           |
//! | `same` (private)             | `sm_Same`                              |
//! | [`cut_fillet_at_open_end`]   | `SmFilletExecutive::CutFilletAtOpenEnd` |
//! | [`plane_of`]                 | `sm_PlaneOf` (`eddf0ed`)               |
//! | `face_of_loop` (private)     | the tail of `CloseRemovalRegion`'s closing face (`bba9914`) |
//! | [`continued_surface`], [`ContinuedSurface`] | `sm_ContinuedSurface` (`db4a655`) |
//! | [`section_end`], [`section_end_face`], [`section_edge`] | none (WR-674, WR-706, not the fork's) |
//!
//! Deviations:
//! - **Errors.** `GetFilletErrorInfo()->NoteError(code, comment)` followed by `SM_ERR`
//!   becomes a [`Refusal`] written to the caller's note, then a
//!   [`crate::error::blend::BlendFailure::NoRoom`] refusal for the same reason (status
//!   `SM_ERR`).
//! - **Sets.** The fork's pointer-keyed `std::set` is a `BTreeSet` keyed by ids, used for
//!   membership only; every loop follows a vector.
//! - **Curve ownership.** A face's curves are owned values handed to
//!   `MakeFaceWithCurves`; the fork's `sm_Loop` deletion on failure is the drop.
//! - **Connector line.** The fork's `new SmLine( sV, sP, 3, &m_crContext )` is the
//!   two-point line `SmLine(StartPoint, EndPoint, lDim, cpContext)` ([`Line::from_points`]).

use std::collections::BTreeSet;

use crate::blend::fillet_executive::p3::NoRoomReason;
use crate::blend::fillet_geom::p1::FilletErrorType;
use crate::blend::fillet_overflow::{PLANAR_NORMAL, PLANE_AXIS};
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::blend::{BlendFailure, OverflowDrop, OverflowInput};
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec2, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::face::p1::{point_3d_classify, Point3dClassifyOptions};
use crate::surface::offset_surface::OffsetSurface;
use crate::surface::plane::Plane;
use crate::surface::surf_of_revolution::SurfOfRevolution;
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
}

/// Write the refusal for `reason` to `note` and fail with it (`NoteError`, then `return
/// SM_ERR`): a [`BlendFailure::NoRoom`] refusal for the same reason, status `SM_ERR`.
fn refuse<T>(note: &mut Option<Refusal>, reason: NoRoomReason, comment: &'static str) -> Result<T> {
    *note = Some(Refusal::no_room(reason, comment));
    Err(SolidError::blend(OP, BlendFailure::NoRoom(reason)))
}

/// A face or edge the overflow reads lacks `input` ([`BlendFailure::OverflowInputMissing`]).
/// The helpers read the caller's body and the executive's sheet alike, so the failure names
/// no entity: the executive's refusal carries the error's entity as the target's.
#[track_caller]
fn input_missing(input: OverflowInput) -> SolidError {
    SolidError::blend(OP, BlendFailure::OverflowInputMissing(input))
}

/// A missing thing this module built ([`BlendFailure::Defect`] keeping the upstream `NER`
/// status `SM_ERR_NULL_POINTER`), `why` saying why it is always there.
#[track_caller]
fn null_defect(why: &'static str) -> SolidError {
    Failure::new(OP, BlendFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// A [`BlendFailure::Defect`] chaining the failed step's error `cause`.
#[track_caller]
fn defect_caused_by(why: &'static str, cause: SolidError) -> SolidError {
    Failure::new(OP, BlendFailure::Defect { why })
        .caused_by(cause)
        .into_error()
}

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
pub(crate) fn edge_curve(k: &Kernel, b: &Brep, e: EdgeId) -> Result<Curve> {
    let curve = edge::curve(b, e)?.ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?;
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
/// A [`BlendFailure::Defect`] (`SM_ERR`) for a zero normal or axis; those of `IsPlanar`.
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
        .map_err(|e| defect_caused_by(PLANAR_NORMAL, e))?;
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
        .map_err(|e| defect_caused_by(PLANE_AXIS, e))?;
    let y = normal.cross(x);
    Ok(Some(PlaneFrame {
        origin: planar.point,
        x,
        y,
        z: normal,
    }))
}

/// The fork's `sm_ContinuedSurface` (tangency-labs/smlib@db4a655): the surface of `f`
/// continued exactly as far as it must to hold the feet of `points` (all on it, or near
/// it), with the farthest point's distance from it (`gap`) and whether it is exact.
#[derive(Debug, Clone)]
pub struct ContinuedSurface {
    /// `rpSurface`: over its own domain (a copy) where that holds the feet; a cylinder,
    /// cone, sphere or torus continued as itself where it must be.
    pub surface: Surface,
    /// `rdGap`: the farthest point's distance from `surface` (`BIG_DOUBLE` for a failed
    /// drop, or when no foot comes off the sides).
    pub gap: f64,
    /// `rbExact`: `false` when a point farther than the tolerance has its foot on a side of
    /// a surface that does not continue exactly (any other kind).
    pub exact: bool,
}

/// The kind's surface of revolution, for a surface that continues as itself
/// ([`OffsetSurface::continues_as_itself`]).
fn revolution(surface: &Surface) -> Option<&SurfOfRevolution> {
    match surface.kind() {
        SurfaceKind::Cone(c) => Some(c.rev()),
        SurfaceKind::Sphere(s) => Some(s.base()),
        SurfaceKind::Torus(t) => Some(t.base()),
        _ => None,
    }
}

/// `GetRotateParamExtent`'s direction: 0 = u, 1 = v; and whether the surface of
/// revolution makes a full turn (`360 - SM_EFF_ZERO_SQRT` degrees or more).
fn rotation_of(rev: &SurfOfRevolution) -> (usize, bool) {
    let rot = usize::from(rev.swap_uv());
    let full = rev.end_angle_deg() - rev.start_angle_deg() >= 360.0 - EFF_ZERO_SQRT;
    (rot, full)
}

/// `sm_ContinuedSurface` (tangency-labs/smlib@db4a655): the surface of `f` continued
/// exactly as far as it must to hold the feet of `points`: over its own domain (a copy)
/// where that holds them; a cylinder, cone, sphere or torus continued as itself
/// ([`OffsetSurface::continue_as_itself`]) where a point farther than `tol` from it has its
/// foot on a side of the domain it can grow past, each such side grown by the domain's
/// size times the least power of two that takes the feet off it (32 rounds at most). Round
/// the axis a surface of revolution grows only where it does not close its turn already.
/// A plane is [`plane_of`]'s, not continued here.
///
/// # Errors
/// [`BlendFailure::OverflowInputMissing`] (`SM_ERR_NULL_POINTER`) for a face without
/// surface; those of the copies, the continuations and the drops.
pub fn continued_surface(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    points: &[Point3],
    tol: f64,
) -> Result<ContinuedSurface> {
    let base = face::p2::surface(b, f)?.ok_or_else(|| input_missing(OverflowInput::FaceSurface))?;
    let domain = base.natural_uv_domain_with_nurb(k)?;
    let size = domain.size();
    let as_itself = OffsetSurface::continues_as_itself(base);
    // the directions that can grow: both, but round the axis of a full turn
    let mut grows = [true, true];
    if as_itself {
        if let Some(rev) = revolution(base) {
            let (rot, full) = rotation_of(rev);
            if full {
                if let Some(g) = grows.get_mut(rot) {
                    *g = false;
                }
            }
        }
    }
    let (mut lo, mut hi) = (domain.min(), domain.max());
    for round in 0..32u32 {
        let surface = if round == 0 {
            base.copy(k)?
        } else {
            OffsetSurface::continue_as_itself(k, base, lo, hi)?
        };
        let have = surface.natural_uv_domain_with_nurb(k)?;
        let edge = [
            1.0e-9 * smos_max(have.size().x, 1.0),
            1.0e-9 * smos_max(have.size().y, 1.0),
        ];
        let (mut low, mut high, mut on_side) = ([false, false], [false, false], false);
        let mut gap = 0.0;
        for &p in points {
            let dropped =
                surface.drop_point::<NoObject>(k, p, &have, &DropPointOptions::default())?;
            let dist = if dropped.success {
                dropped.gap
            } else {
                BIG_DOUBLE
            };
            gap = smos_max(gap, dist);
            if !dropped.success || dist <= tol {
                continue;
            }
            let uv = [dropped.result_uv.x, dropped.result_uv.y];
            let (min, max) = ([have.min().x, have.min().y], [have.max().x, have.max().y]);
            for d in 0..2 {
                let (Some(&u), Some(&a), Some(&z), Some(&e)) =
                    (uv.get(d), min.get(d), max.get(d), edge.get(d))
                else {
                    continue;
                };
                if u <= a + e {
                    if let Some(l) = low.get_mut(d) {
                        *l = true;
                    }
                    on_side = true;
                }
                if u >= z - e {
                    if let Some(h) = high.get_mut(d) {
                        *h = true;
                    }
                    on_side = true;
                }
            }
        }
        let grow =
            on_side && ((grows[0] && (low[0] || high[0])) || (grows[1] && (low[1] || high[1])));
        if !grow || !as_itself {
            return Ok(ContinuedSurface {
                surface,
                gap,
                exact: !grow || as_itself,
            });
        }
        let factor = f64::from(1u32 << round.min(20));
        if grows[0] && low[0] {
            lo.x -= factor * size.x;
        }
        if grows[0] && high[0] {
            hi.x += factor * size.x;
        }
        if grows[1] && low[1] {
            lo.y -= factor * size.y;
        }
        if grows[1] && high[1] {
            hi.y += factor * size.y;
        }
    }
    // no foot comes off the sides: the points are not on the surface's continuation
    Ok(ContinuedSurface {
        surface: base.copy(k)?,
        gap: BIG_DOUBLE,
        exact: true,
    })
}

/// `sm_DistanceToSurface`: the distance from `point` to the surface of `f` continued, as far
/// as it continues exactly: a plane's to the whole plane, a cylinder's, cone's, sphere's or
/// torus's to itself continued ([`continued_surface`], tangency-labs/smlib@db4a655), any
/// other surface's to its own domain (`BIG_DOUBLE` when the drop fails). A planar surface
/// of any kind ([`plane_of`]; WR-635, `eddf0ed`) continues as its plane.
///
/// # Errors
/// [`BlendFailure::OverflowInputMissing`] (`SM_ERR_NULL_POINTER`) for a face without
/// surface; those of the drop.
pub fn distance_to_surface(k: &Kernel, b: &Brep, f: FaceId, point: Point3) -> Result<f64> {
    let surface =
        face::p2::surface(b, f)?.ok_or_else(|| input_missing(OverflowInput::FaceSurface))?;
    if let Some(frame) = plane_of(k, surface)? {
        return Ok((point - frame.origin).dot(frame.z).abs());
    }
    Ok(continued_surface(k, b, f, &[point], 0.0)?.gap)
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
/// The section is fitted to `fit_tol` and merged at `tol` (the fork uses `tol` for both;
/// the crease trim fits it tighter, #17673).
///
/// # Errors
/// The refusals as [`BlendFailure::NoRoom`] (`SM_ERR`);
/// [`BlendFailure::OverflowPointDoesNotDrop`] (`SM_ERR`) for a failed drop of the edge's
/// middle; those of the section, the split, the classifications and the deletion.
#[allow(clippy::too_many_arguments)]
pub fn cut_fillet_at_open_end(
    k: &mut Kernel,
    region: &mut Brep,
    faces: &mut [Option<FaceId>],
    end_surface: &Surface,
    vertex_point: Point3,
    edge_mid: Point3,
    tol: f64,
    fit_tol: f64,
    note: &mut Option<Refusal>,
) -> Result<()> {
    let planar = end_surface.is_planar(k, EFF_ZERO)?;
    if !planar.is_planar {
        return refuse(
            note,
            NoRoomReason::CliffEndFaceNotPlanar,
            "Fillet Error: a cliff overflow runs past a fillet end whose end face is not planar",
        );
    }
    let (at, normal) = (planar.point, planar.normal);
    let cut = Plane::from_normal(at, normal);

    for slot in faces.iter_mut() {
        let Some(f) = *slot else {
            continue;
        };
        let surface = face::p2::surface(region, f)?
            .ok_or_else(|| input_missing(OverflowInput::FaceSurface))?
            .clone_for_evaluation()?;
        let uv_domain = face::p2::uv_domain(region, f)?;
        let dropped = surface.drop_point::<NoObject>(
            k,
            edge_mid,
            &uv_domain,
            &DropPointOptions::default(),
        )?;
        if !dropped.success {
            return Err(SolidError::blend(
                OP,
                BlendFailure::OverflowPointDoesNotDrop(OverflowDrop::EdgeMiddleOntoFilletFace),
            ));
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
            Some(fit_tol),
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
                NoRoomReason::CliffEndNotCut,
                "Fillet Error: the end face's plane does not cut a fillet that overflows its end in two",
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
                NoRoomReason::CliffEndNotCut,
                "Fillet Error: the end face's plane does not part a fillet that overflows its end from its middle",
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

// ---- WR-674: a round ended at its sections ------------------------------------------------

/// One end of a round cut off by its section (WR-674, WR-706): the filleted
/// edge's vertex there and the section's plane through it, normal to the edge.
#[derive(Debug, Clone, Copy)]
pub struct SectionEnd {
    /// The filleted edge's vertex (of the target).
    pub vertex: VertexId,
    /// The vertex's point, on the plane.
    pub at: Point3,
    /// The plane's unit normal: the filleted edge's tangent at the vertex.
    pub normal: Vec3,
}

/// Whether `p` lies on the plane through `at` with unit normal `normal`, within `tol`.
fn on_section_plane(p: Point3, at: Point3, normal: Vec3, tol: f64) -> bool {
    (p - at).dot(normal).abs() <= tol
}

/// The face over the loop `lp` on `surface`, anticlockwise
/// by the loop's signed area in the surface's parameters, made in `region`'s infinite
/// region.
///
/// # Errors
/// [`BlendFailure::OverflowPointDoesNotDrop`] (`SM_ERR`) for a failed drop; those of the
/// samples and the face construction.
fn face_of_loop(
    k: &mut Kernel,
    region: &mut Brep,
    surface: Surface,
    mut lp: Vec<LoopCurve>,
) -> Result<FaceId> {
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
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::OverflowPointDoesNotDrop(OverflowDrop::LoopSampleOntoSurface),
                ));
            }
            uvs.push(dropped.result_uv);
        }
    }
    let mut area = 0.0;
    // each point and its successor, the last point's the first
    for (a, b) in uvs.iter().zip(uvs.iter().cycle().skip(1)) {
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
    made.face.ok_or_else(|| {
        null_defect(
            "making a section end's face from its loop succeeded without a face: \
             `MakeFaceWithCurves` hands back the face its face construction made, which \
             fails where it makes none",
        )
    })
}

/// The section's flat face of a round cut off at `end` (WR-674;
/// WR-706, the trim's): on the section's
/// plane, bounded by the lamina edge `section` of `region` (the round's section) and the lines
/// from its ends to the end's vertex (a line shorter than `on_tol` left out), added to
/// `region`.
///
/// # Errors
/// Those of the samples and the face construction.
pub fn section_end_face(
    k: &mut Kernel,
    region: &mut Brep,
    end: &SectionEnd,
    section: EdgeId,
    on_tol: f64,
) -> Result<()> {
    let line = |from: Point3, to: Point3| Curve::from(Line::from_points(from, to, 3));
    let (a, b) = edge::start_end_vertices(region, section)?;
    let a = vertex::point(
        region,
        a.ok_or_else(|| input_missing(OverflowInput::EdgeStartVertex))?,
    )?;
    let b = vertex::point(
        region,
        b.ok_or_else(|| input_missing(OverflowInput::EdgeEndVertex))?,
    )?;
    let mut lp: Vec<LoopCurve> = Vec::new();
    let section_curve = edge_curve(k, region, section)?;
    // the box starts at the section curve's first sample (`samples`' point at 0), so it is
    // never empty: adding that sample again below leaves it as it is
    let first = section_curve.evaluate_point(k, section_curve.natural_interval().evaluate(0.0))?;
    add_curve(k, &mut lp, section_curve, a, b)?;
    for (from, to) in [(b, end.at), (end.at, a)] {
        if !same(from, to, on_tol) {
            add_curve(k, &mut lp, line(from, to), from, to)?;
        }
    }
    let [_, x, y] = end.normal.make_unit_ortho_vectors(None)?;
    let in_plane = |p: Point3| {
        let d = p - end.at;
        Point2::new(d.dot(x), d.dot(y))
    };
    let mut bbox = Extent2d::from_points(in_plane(first), in_plane(first));
    for c in &lp {
        let mut points = Vec::new();
        samples(k, &c.curve, 8, &mut points)?;
        for p in points {
            bbox.add_point(in_plane(p));
        }
    }
    let margin = 0.01 * bbox.size().length() + on_tol;
    let domain = Extent2d::from_bounds(
        bbox.min().x - margin,
        bbox.min().y - margin,
        bbox.max().x + margin,
        bbox.max().y + margin,
    );
    let plane = Surface::from(Plane::new(end.at, x, y, Vec2::new(1.0, 1.0), &domain));
    face_of_loop(k, region, plane, lp)?;
    Ok(())
}

/// The section end of the round on `fil_edge` (of `target`) at its vertex `v` (WR-674): the
/// plane through the vertex normal to the filleted edge there; `None` where the edge has no
/// tangent at the vertex.
///
/// # Errors
/// Those of the evaluation.
pub fn section_end(
    k: &Kernel,
    target: &Brep,
    fil_edge: EdgeId,
    v: VertexId,
) -> Result<Option<SectionEnd>> {
    let vertex_point = vertex::point(target, v)?;
    let interval = edge::interval(target, fil_edge)?;
    let t = if edge::start_vertex(target, fil_edge)? == Some(v) {
        interval.min()
    } else {
        interval.max()
    };
    let derivs = edge::curve(target, fil_edge)?
        .ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?
        .evaluate(k, t, 1, true, true)?;
    let tangent = derivs.get(1).copied().ok_or_else(|| {
        null_defect(
            "the filleted edge's curve evaluated with one derivative gives no tangent: a \
                 curve's evaluation returns its point and each derivative asked for",
        )
    })?;
    let length = tangent.length();
    if length <= EFF_ZERO {
        return Ok(None);
    }
    Ok(Some(SectionEnd {
        vertex: v,
        at: vertex_point,
        normal: tangent * (1.0 / length),
    }))
}

/// The lamina edge of the round's faces `faces` (of `region`) that lies on the plane of
/// `end` (four samples within `on_tol`): the round's section there, once the round is cut by
/// it. `None` when no lamina edge or more than one does (WR-706).
///
/// Only the round's own faces are searched: another round of the same sheet can end at a
/// section on the same plane (a tab's two corner arcs, each running on into the tab's sides
/// at the same height, tangency-labs/solidcore#2314), and its section is not this end's.
///
/// # Errors
/// Those of the samples.
pub fn section_edge(
    k: &Kernel,
    region: &Brep,
    end: &SectionEnd,
    faces: &[FaceId],
    on_tol: f64,
) -> Result<Option<EdgeId>> {
    let mut found = None;
    let mut seen: BTreeSet<EdgeId> = BTreeSet::new();
    for &f in faces {
        for e in face::p4::edges(region, f, None, false, None)? {
            if !seen.insert(e) || !edge::is_lamina(region, e)? {
                continue;
            }
            let curve = edge_curve(k, region, e)?;
            let mut points = Vec::new();
            samples(k, &curve, 4, &mut points)?;
            if points
                .iter()
                .all(|&p| on_section_plane(p, end.at, end.normal, on_tol))
            {
                if found.is_some() {
                    return Ok(None);
                }
                found = Some(e);
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRANCHES: Refusal = Refusal::no_room(
        NoRoomReason::CliffBoundaryBranches,
        "Fillet Error: a cliff overflow's fillet boundary branches on a face",
    );

    #[test]
    fn a_refusal_is_noted_and_a_typed_failure() {
        let mut note = None;
        let error = refuse::<()>(
            &mut note,
            NoRoomReason::CliffBoundaryBranches,
            BRANCHES.comment,
        )
        .unwrap_err();
        assert_eq!(note, Some(BRANCHES));
        assert_eq!(
            error,
            SolidError::blend(
                OP,
                BlendFailure::NoRoom(NoRoomReason::CliffBoundaryBranches)
            )
        );
        assert!(error.is_refusal(), "{error:?}");
        assert_eq!(error.upstream_status(), Some(crate::error::SM_ERR));
    }
}
