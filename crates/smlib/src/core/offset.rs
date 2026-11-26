//! Shells, face offsets, thickening, sheet offsets and drafts on solidcore: the C API's logic
//! round SMLib's offset executive, ported from the fork's `capi/src/smlib_ops.cpp`
//! (`smlib_shell`, `smlib_offset_faces`, `smlib_thicken`, `smlib_draft`, `leaves_cavity`,
//! `FaceSet`, `outer_faceuse`, `move_faces`, `draft_faces`), `capi/src/smlib_sheets.cpp`
//! (`smlib_offset_sheet`) and `capi/src/smlib_pick.h` (`pick_faces`).
//!
//! Shells, face offsets and sheet offsets are solidcore-ops' own ([`solidcore_ops::offset`]),
//! their faces picked by point through [`solidcore_ops::pick::faces`]. Thickening and drafts are
//! still composed here: solidcore-ops' versions differ from these in their results (a draft's
//! refusals, a thickened sheet), so they move once those match.
//!
//! As there, every operation works on a copy of its body (`SmApiBrepCopy`) and picks faces by
//! point; the shell and the offsets are `SmApiShellBrepFull` and `SmApiOffsetBrepFull`, with
//! convex edges extended and the results merged. A sheet offset takes faces that close into a
//! solid as the capi does since tangency-labs/smlib#17.
//!
//! A draft tilts planes and, since tangency-labs/smlib#27, turns a cylinder about the pull into
//! a cone, and since smlib#53 a cone about the pull into the cone its generators turned make: the
//! surface solidcore's draft gives (`draft_surface`), with its draft definition. A draft's tilted faces go in through solidcore's
//! `SmBrep::LocalOperation` ([`local_operation`]). The capi's check that it moved some face
//! (`moved_faces`) marks the replacement surfaces; solidcore names a surface by its owning face, so
//! the mark goes on the picked faces, which it stays with through the operation (solidcore#1875).

use solidcore::EntityRef;
use solidcore::SolidError;
use solidcore::geom::attribute::{Attribute, Behavior, ids};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::object::Shape;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::offset::draft::{Draft, DraftRefusal, draft_surface};
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions, local_operation_simple, offset_brep_simple};
use solidcore::query::api_queries::{face_closest_point_in_shape, face_internal_point};
use solidcore::query::brep::{BrepIdentity, BrepQuery, LiveBrepObjects};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::{p1 as brep_p1, p2 as brep_p2};
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::{Brep, BrepId, FaceId, FaceuseId};

use super::blend::{Miss, on_copy, p3, pick, points_ok, status as blend_status, tolerance_ok};
use super::call::ops_as;
use super::{Body, answer};
use crate::{Error, Result};

/// The capi's statuses as SolidCraft errors: an operation's own result.
type Out<T> = std::result::Result<T, Error>;

/// `SM_ERR_INVALID_INPUT`.
const INVALID_INPUT: i32 = 1007;

/// The offset's typed refusal and complete diagnostic come through solidcore-ops.
/// Other failures pass through the blend mapper, retaining that diagnostic wherever
/// it would otherwise give only a numeric status.
fn status(e: SolidError) -> Error {
    let diagnostic = solidcore_ops::offset::diagnostic(&e);
    match diagnostic.refusal {
        Some(error) => refused(error, diagnostic.detail),
        None => match blend_status(e) {
            Error::Status(_) => Error::Solidcore(diagnostic.detail),
            other => other,
        },
    }
}

/// Application errors for ops' offset refusals: specialised wall answers keep
/// their fields; other typed refusals keep the kernel diagnostic alongside them.
pub(super) fn refused(error: solidcore_ops::Error, diagnostic: String) -> Error {
    use solidcore_ops::Error as Ops;
    // this crate words a named solid test as the bare NotSolid it always was
    if error.not_solid_test().is_some() {
        return Error::RefusedWithDiagnostic { refusal: Ops::NotSolid, diagnostic };
    }
    match error {
        Ops::WallsTouch => Error::WallsTouch,
        Ops::WallFolds { largest } => Error::WallFolds { largest },
        Ops::Unsupported(_) => Error::Unsupported,
        error @ Ops::OffsetRefused { why: solidcore_ops::OffsetRefusal::Phrase { .. }, .. } => Error::Refused(error),
        refusal => Error::RefusedWithDiagnostic { refusal, diagnostic },
    }
}

/// `std::fabs(x) > 1e6` and the rest of the capi's size checks: finite, not zero, at most 1e6.
fn distance_ok(x: f64) -> bool {
    x.is_finite() && x != 0.0 && x.abs() <= 1e6
}

/// The B-rep id every shape and face key in this module names the one body by.
const ONE: BrepId = BrepId::from_index(0);

/// The live view of `b` the closest-point solves read, under `identity`: `query`'s effective
/// identity ([`BrepQuery::effective_identity`]).
fn live<'a, 'i>(b: &'a mut Brep, identity: &'a BrepIdentity<'i>) -> solidcore::Result<LiveBrepObjects<'a, 'i>> {
    LiveBrepObjects::new(b, identity, None)
}

/// `pick_faces` (`face_distance`: `SmApiFaceClosestPoint`; a face whose query returns a status
/// other than success is skipped, [`answer`]).
pub(super) fn pick_faces(k: &mut Kernel, b: &mut Brep, points: &[[f64; 3]], tol: f64) -> solidcore::Result<std::result::Result<Vec<FaceId>, Miss>> {
    let all = brep_p1::faces(b, None)?;
    let model_tol = b.tolerance();
    let mut shapes = Vec::with_capacity(all.len());
    for &f in &all {
        shapes.push(answer(Shape::new(k, &(ONE, &*b), &[], &[(ONE, f)], &[], &[]))?);
    }
    let query = BrepQuery::of_brep(b);
    let identity = query.effective_identity();
    let objects = live(b, &identity)?;
    let k: &Kernel = k;
    let indices: Vec<usize> = (0..all.len()).collect();
    let picked = pick(&indices, points, tol, model_tol, |i, p| {
        let (Some(&f), Some(Some(shape))) = (all.get(i), shapes.get(i)) else { return Ok(None) };
        Ok(answer(face_closest_point_in_shape(k, shape, &objects, (ONE, f), p))?.map(|c| c.distance))
    })?;
    Ok(picked.map(|(p, _)| p.iter().filter_map(|&i| all.get(i).copied()).collect()))
}

/// A face pick's failure as `pick_error` (ops.rs) reads it.
pub(super) fn face_miss(m: Miss) -> Error {
    match m {
        Miss::None(point) => Error::NoMatch { point },
        Miss::Ambiguous(point) => Error::Ambiguous { point },
    }
}

/// `outer_faceuse`: the faceuse of `f` on the outside of its body, the one bounding the infinite
/// region (for a sheet, both do; then the upward one).
fn outer_faceuse(b: &Brep, f: FaceId) -> solidcore::Result<Option<FaceuseId>> {
    let Some(fu) = face_p2::upward_faceuse(b, f)? else { return Ok(None) };
    let use_ = b.faceuse(fu)?;
    let outside = match use_.shell {
        Some(s) => b.shell(s)?.region.is_some() && b.shell(s)?.region == b.infinite_region(),
        None => false,
    };
    Ok(if outside { Some(fu) } else { use_.mate })
}

/// `SmApiShellBrepFull(b, distance, TRUE, TRUE, TRUE, open, r)`: convex edges extended, results
/// merged, side walls made at a sheet's edges, and (as the fork does, 5448b9a) a thickened
/// sheet's coplanar side walls one face.
pub(super) fn shell_brep_full(k: &mut Kernel, b: &Brep, distance: f64, open: &[FaceId]) -> solidcore::Result<Brep> {
    let options = ShellOptions {
        offset: OffsetOptions { distance, extend_convex_edges: true, merge_results: true },
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    };
    // solidcore's shell, a feature narrower than twice an inward wall left solid
    solidcore::offset::thin_features::shell_brep(k, b, options, open)
}

/// `SmApiOffsetBrepFull(b, distance, extend, TRUE, r)`: the convex edges extended until the
/// moved faces meet (`extend`, as the capi always does), or rounded by the distance.
fn offset_brep_full(k: &mut Kernel, b: &mut Brep, distance: f64, extend: bool) -> solidcore::Result<Brep> {
    offset_brep_simple(k, b, OffsetOptions { distance, extend_convex_edges: extend, merge_results: true })
}

/// `finish`: the result must have faces, and a solid input must give a closed manifold solid.
fn finish(b: Brep, solid_in: bool) -> solidcore::Result<Out<Brep>> {
    if b.face_list().is_empty() {
        return Ok(Err(Error::Empty));
    }
    if solid_in && !brep_p2::is_manifold_solid(&b)? {
        return Ok(Err(Error::NotSolid));
    }
    Ok(Ok(b))
}

/// An SMLib call's own status passed on (`status(...)` in the capi), else the value.
fn called<T>(r: solidcore::Result<T>) -> solidcore::Result<Out<T>> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => Ok(Err(status(e))),
    }
}

/// solidcore-ops' refusal of an offset as this crate's error: the moves and walls that have
/// kinds of their own as those kinds, a kernel status as the kernel's words for it, a result
/// that is not a solid as [`Error::NotSolid`] (with the kernel's words where the kernel said
/// why), and the rest as [`refused`] words them.
fn offset_error(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    use solidcore_ops::Error as Ops;
    if e.not_solid_test().is_some() && detail.is_none() {
        return Error::NotSolid;
    }
    match e {
        Ops::Argument => Error::Argument,
        Ops::Empty => Error::Empty,
        Ops::NoRoom => Error::NoRoom,
        Ops::NoEffect => Error::NoEffect,
        Ops::NotExact => Error::NotExact,
        Ops::NotSolid => Error::NotSolid,
        Ops::Cancelled => Error::Cancelled(call),
        Ops::Internal(what) => Error::Internal(what),
        Ops::BlendsNotFollowed { exact, why } => Error::BlendsNotFollowed { exact, why },
        Ops::DraftCutsThrough { at, depth } => Error::DraftCutsThrough { at, depth },
        Ops::Status(code) => Error::Solidcore(detail.unwrap_or_else(|| format!("{call}: status {code}"))),
        e => refused(e, detail.unwrap_or_default()),
    }
}

/// The faces of `body` nearest `points` (each within `tolerance`), each once, in the order its
/// first point picked it, as indices of the body's own list; a point that picks none, or is as
/// near two faces, refused ([`face_miss`]).
fn picked_faces(call: &'static str, body: &Body, points: &[[f64; 3]], tolerance: f64) -> Result<Vec<usize>> {
    let each = ops_as(call, &[("brep", body)], offset_error, || solidcore_ops::pick::faces(body, points, tolerance))?;
    let each = each.map_err(|m| {
        let e = face_miss(match m {
            solidcore_ops::pick::Miss::None(point) => Miss::None(point),
            solidcore_ops::pick::Miss::Ambiguous(point) => Miss::Ambiguous(point),
        });
        super::dump::failed_ops(call, &e.to_string(), &[("brep", body)]);
        e
    })?;
    let mut once: Vec<usize> = Vec::with_capacity(each.len());
    for f in each {
        if !once.contains(&f) {
            once.push(f);
        }
    }
    Ok(once)
}

/// A copy of `body` for an operation to work on (`SmApiBrepCopy`): solidcore-ops' offsets
/// change the body they are given.
fn working_copy(call: &'static str, body: &Body) -> Result<Body> {
    if body.kernel_left_unfinished() {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    ops_as(call, &[("brep", body)], offset_error, || body.copy())
}

/// `smlib_shell`: the faces nearest `points` (each within `tolerance`) opened, the rest offset
/// by `thickness` (negative: inward) into a wall; none gives a closed hollow body. An inward
/// wall of a solid that leaves no cavity is [`Error::NoRoom`], refused before the shell.
pub(crate) fn shell(body: &Body, points: &[[f64; 3]], tolerance: f64, thickness: f64) -> Result<Body> {
    if !points_ok(points, true) || !tolerance_ok(tolerance) || !distance_ok(thickness) {
        return Err(Error::Argument);
    }
    const CALL: &str = "smlib_shell";
    let open = picked_faces(CALL, body, points, tolerance)?;
    let w = working_copy(CALL, body)?;
    let order = w.own_order();
    match ops_as(CALL, &[("brep", body)], offset_error, || solidcore_ops::offset::shell(&w, &order, &open, thickness)) {
        Ok(()) => Ok(w),
        // (#18568) said why where a face's outline touches itself: the copy was put back as it
        // came in, so its faces are the body's
        Err(e) if is_bare_failure(&e) => match ops_as(CALL, &[], offset_error, || w.outline_touch())? {
            Some(at) => Err(Error::OutlineTouches { at, detail: e.to_string() }),
            None => Err(e),
        },
        Err(e) => Err(e),
    }
}

/// (#18568) A failure that does not say why: a bare status, solidcore's text, a result that is
/// not a solid, or an offset refused as unsound.
fn is_bare_failure(e: &Error) -> bool {
    matches!(
        e,
        Error::Status(_) | Error::Solidcore(_) | Error::NotSolid | Error::Exception | Error::Refused(solidcore_ops::Error::OffsetRefused { .. })
    )
}

/// `smlib_offset_faces`: the faces nearest `points` (each within `tolerance`) of a solid moved
/// `distance` along their outward normals; faces moved in so far that the body leaves no
/// material behind them are [`Error::NoRoom`].
pub(crate) fn offset_faces(body: &Body, points: &[[f64; 3]], tolerance: f64, distance: f64) -> Result<Body> {
    if !points_ok(points, false) || !tolerance_ok(tolerance) || !distance_ok(distance) {
        return Err(Error::Argument);
    }
    const CALL: &str = "smlib_offset_faces";
    let picked = picked_faces(CALL, body, points, tolerance)?;
    let w = working_copy(CALL, body)?;
    let order = w.own_order();
    ops_as(CALL, &[("brep", body)], offset_error, || solidcore_ops::offset::offset_faces(&w, &order, &picked, distance))?;
    Ok(w)
}

/// `smlib_thicken`: a sheet thickened by `thickness` along its normals, or (symmetric) moved back
/// by half first; a closed solid is [`Error::Argument`].
pub(crate) fn thicken(body: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    if !distance_ok(thickness) {
        return Err(Error::Argument);
    }
    on_copy("smlib_thicken", body, |k, mut w| {
        if brep_p2::is_manifold_solid(&w)? {
            return Ok(Err(Error::Argument));
        }
        if symmetric {
            w = match called(offset_brep_full(k, &mut w, -0.5 * thickness, true))? {
                Ok(moved) => moved,
                Err(e) => return Ok(Err(e)),
            };
        }
        match called(shell_brep_full(k, &w, thickness, &[]))? {
            Ok(r) => finish(r, true),
            Err(e) => Ok(Err(e)),
        }
    })
}

/// `smlib_offset_sheet`: every face of a sheet moved onto its surface's offset by `distance`
/// along its normal; faces that close into a solid (a sphere's one face, a block's six) give the
/// offset solid (tangency-labs/smlib#17, which dropped the capi's refusal of a closed solid).
pub(crate) fn offset_sheet(body: &Body, distance: f64) -> Result<Body> {
    if !distance_ok(distance) {
        return Err(Error::Argument);
    }
    const CALL: &str = "smlib_offset_sheet";
    let w = working_copy(CALL, body)?;
    ops_as(CALL, &[("brep", body)], offset_error, || solidcore_ops::offset::offset_sheet(&w, distance))?;
    Ok(w)
}

/// The closed solid `body` grown by `distance` along its faces' outward normals (negative: shrunk):
/// the convex edges extended until the moved faces meet, or (`round`) rounded with `distance` as
/// the radius, so every point of the result's surface is `distance` from the body's. A body that is
/// not a closed manifold solid is [`Error::Argument`]; a result that is not one is
/// [`Error::NotSolid`].
pub(crate) fn offset_solid(body: &Body, distance: f64, round: bool) -> Result<Body> {
    if !distance_ok(distance) {
        return Err(Error::Argument);
    }
    on_copy("smlib_offset_solid", body, |k, mut w| {
        if !brep_p2::is_manifold_solid(&w)? {
            return Ok(Err(Error::Argument));
        }
        match called(offset_brep_full(k, &mut w, distance, !round))? {
            Ok(r) => finish(r, true),
            Err(e) => Ok(Err(e)),
        }
    })
}

/// A unit vector from `v`, as `smlib_draft`'s `unit`; `None` for a vector too short or not
/// finite.
fn unit(v: [f64; 3]) -> Option<Vec3> {
    if !v.iter().all(|x| x.is_finite()) {
        return None;
    }
    let u = p3(v);
    let len = u.length();
    (len > 1e-12 && len.is_finite()).then(|| u * (1.0 / len))
}

/// A face the draft cannot tilt, refused for what about it the draft cannot handle (#18541), in
/// the words `solidcore_ops::offset::draft` gives the same refusal where they fit this check.
fn cannot_draft(why: &'static str) -> Error {
    Error::Refused(solidcore_ops::Error::Unsupported(why))
}

/// solidcore's refusal to draft a cone or cylinder, in the words the app's draft has used: an axis
/// in the neutral plane is one parallel to it; the rest as solidcore-ops words them (a neutral
/// plane at or past a cone's apex among them, which the kernel tells apart).
fn cone_refused(r: DraftRefusal) -> Error {
    match r {
        DraftRefusal::AxisInNeutralPlane => cannot_draft("a drafted cone or cylinder's axis is parallel to the neutral plane"),
        r => Error::Refused(solidcore_ops::offset::draft_refused(r)),
    }
}

/// The capi's `Before` mark on what `SmBrep::LocalOperation` is to move: above the history's tag.
const MOVED_MARK: u64 = ids::USER_1 + 8;

/// `Before::mark` on each replacement surface. A surface has no identity apart from its owning
/// face here, and `faces[i]`'s offset face takes `surfaces[i]`, so the mark goes on the picked face
/// and travels as the surface's does (Copy behaviour; solidcore#1875). The one difference, recorded
/// in solidcore's `topology.brep_local_operation--1875-face-mark-for-surface-mark` finding: a merge
/// keeping an unpicked face over a picked one hands that survivor the mark, where SMLib's survivor
/// keeps its own unmarked surface.
pub(super) fn mark_moved(w: &mut Brep, picked: &[FaceId]) -> solidcore::Result<()> {
    for &f in picked {
        w.add_attribute(EntityRef::Face(f), Attribute::long(MOVED_MARK, 0, Behavior::Copy))?;
    }
    Ok(())
}

/// `moved_faces`'s reading (`read_marks`): whether any face of `w` carries the mark, every mark
/// removed.
pub(super) fn moved_faces(w: &mut Brep) -> solidcore::Result<bool> {
    let mut moved = false;
    for f in w.face_list().to_vec() {
        let e = EntityRef::Face(f);
        let marks: Vec<_> = w.entity_attributes(e).into_iter().filter(|(_, a)| a.attribute_id() == MOVED_MARK).map(|(h, _)| h).collect();
        moved = moved || !marks.is_empty();
        for h in marks {
            w.remove_attribute(e, h)?;
        }
    }
    Ok(moved)
}

/// `smlib_draft`: its checks, the solid check, the pick, then `draft_faces`: each picked face (a
/// plane parallel to the pull) gets a copy of its plane turned by `angle` about its line on the
/// neutral plane, and a cylinder or cone about the pull the cone solidcore's `draft_surface` gives
/// (tangency-labs/smlib#27); `SmBrep::LocalOperation` extends or trims the faces around them to
/// meet the tilted surfaces.
pub(crate) fn draft(
    body: &Body,
    points: &[[f64; 3]],
    tolerance: f64,
    neutral_origin: [f64; 3],
    neutral_normal: [f64; 3],
    pull: [f64; 3],
    angle: f64,
) -> Result<Body> {
    let (Some(normal), Some(along)) = (unit(neutral_normal), unit(pull)) else { return Err(Error::Argument) };
    if !points_ok(points, false)
        || !tolerance_ok(tolerance)
        || !neutral_origin.iter().all(|x| x.is_finite() && x.abs() <= 1e9)
        || !angle.is_finite()
        || angle == 0.0
        || angle.abs() >= 1.5697963267948966
    {
        return Err(Error::Argument);
    }
    on_copy("smlib_draft", body, |k, mut w| {
        if !brep_p2::is_manifold_solid(&w)? {
            return Ok(Err(Error::Argument));
        }
        let picked = match pick_faces(k, &mut w, points, tolerance)? {
            Ok(p) => p,
            Err(m) => return Ok(Err(face_miss(m))),
        };
        // `draft_faces`.
        let mut surfaces = Vec::with_capacity(picked.len());
        for &f in &picked {
            let fu = outer_faceuse(&w, f)?;
            let face = w.face(f)?;
            let (Some(surf), Some(fu)) = (face.surface.as_ref(), fu) else {
                return Ok(Err(Error::Status(INVALID_INPUT)));
            };
            // A cylinder or cone about the pull drafted becomes the cone solidcore's draft gives
            // (`draft_surface`: its generators turned about its circle on the neutral plane),
            // carrying its draft definition, so that the local operation knows it for a draft.
            if let SurfaceKind::Cone(_) = surf.kind_with_nurb(k)? {
                let Ok(the_draft) = Draft::new(p3(neutral_origin), normal, along, angle) else {
                    return Ok(Err(Error::Argument));
                };
                match answer(draft_surface(k, &w, f, &the_draft))? {
                    Some(Ok(cone)) => surfaces.push(cone),
                    Some(Err(r)) => return Ok(Err(cone_refused(r))),
                    None => return Ok(Err(Error::Unsupported)),
                }
                continue;
            }
            let SurfaceKind::Plane(plane) = surf.kind_with_nurb(k)? else {
                return Ok(Err(cannot_draft("a drafted face is neither a plane nor a cone or cylinder")));
            };
            // The face's outward normal: the plane's, turned with the outer faceuse.
            let mut n = plane.position().z_axis();
            if w.faceuse(fu)?.orientation != OrientType::Same {
                n = -n;
            }
            if n.dot(along).abs() > 1e-6 {
                return Ok(Err(cannot_draft("a drafted plane is not parallel to the pull")));
            }
            let Some(inside) = answer(face_internal_point(k, &mut w, f))? else {
                return Ok(Err(Error::Status(INVALID_INPUT)));
            };
            let toward = along.dot(normal);
            if toward.abs() < 1e-12 {
                return Ok(Err(Error::Argument));
            }
            let hinge = inside + along * ((p3(neutral_origin) - inside).dot(normal) / toward);
            // Turning n about n x pull by `angle` gives n cos(angle) + pull sin(angle).
            let axis = n.cross(along);
            let len = axis.length();
            if !(len > 0.5) {
                return Ok(Err(Error::Unsupported));
            }
            let axis = axis * (1.0 / len);
            let Some(surf) = w.face(f)?.surface.as_ref() else {
                return Ok(Err(Error::Status(INVALID_INPUT)));
            };
            let Some(mut tilted) = answer(surf.copy(k))? else {
                return Ok(Err(Error::Status(INVALID_INPUT)));
            };
            let mut turn = Axis2Placement::IDENTITY;
            turn.rotate_about_axis_at_point(angle, hinge, axis);
            if answer(tilted.transform(k, &turn, None))?.is_none() {
                return Ok(Err(Error::Status(INVALID_INPUT)));
            }
            surfaces.push(tilted);
        }
        // A drafted cone that would reach a cavity's wall closes the wall: refused with the largest
        // angle that keeps it (#20767), before anything changes.
        let (n, p) = ([normal.x, normal.y, normal.z], [along.x, along.y, along.z]);
        if let Err(e) = called(solidcore_ops::offset::refuse_draft_closing_wall(k, &w, &picked, neutral_origin, n, p, angle))? {
            return Ok(Err(e));
        }
        // `moved_faces(b, status(b->LocalOperation(faces, surfaces)))`, its marks on the picked
        // faces (`mark_moved`).
        // A face the drafted faces would cut through (a hole's wall within the draft of the side,
        // #19686): read before the kernel edits the copy, named if the kernel then refuses. Best
        // effort: a check that fails itself only leaves the kernel's own refusal, and never
        // refuses a draft the kernel builds.
        let through = solidcore_ops::offset::drafted_through(k, &mut w, &picked, &surfaces).ok().flatten();
        mark_moved(&mut w, &picked)?;
        if let Err(e) = called(local_operation_simple(k, &mut w, &picked, surfaces))? {
            return Ok(Err(match through {
                Some(t) => Error::DraftCutsThrough { at: [t.at.x, t.at.y, t.at.z], depth: t.depth },
                None => e,
            }));
        }
        if !moved_faces(&mut w)? {
            return Ok(Err(Error::NoEffect));
        }
        finish(w, true)
    })
}
