//! Cancel overlapping planar offset patches with opposite orientations.
//!
//! The two signed sheets contribute nothing where they overlap. Exact planar
//! differences retain their non-overlapping regions before the solid merge sees
//! coincident edges. Original face pairs are visited once; their evolving sheet
//! remainders stay in private B-reps until every cancellation is constructed.
//! Upstream: none (regularized inner offsets).
//! Deviations: no recursion or retries; each planar boolean runs the existing
//! kernel pipeline.

use std::collections::BTreeSet;

use super::attributes;
use super::p1::OffsetExecutive;
use super::p2::OffsetNotifications;
use crate::blend::feature_executive::copy_faces_carrying;
use crate::boolean::api_brep::{boolean_2d, Boolean2d, Boolean2dOutcome};
use crate::error::offset::OffsetFailure;
use crate::error::{EntityRef, Failure, Result, SolidError};
use crate::geom::tol::USE_DEFAULT;
use crate::kernel::Kernel;
use crate::math::{max2, Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::gamma;
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::{p1 as bp1, p3 as bp3, p5 as bp5};
use crate::topology::edge::{NotificationStores, NotifyState};
use crate::topology::face::p2 as face2;
use crate::topology::shell::ShellMembers;
use crate::topology::{loopuse, Brep, FaceId, FaceuseId};

const OP: &str = "offset::offset_executive::cancel";

/// A defect of the cancellation, naming the step that failed and, where there is
/// one, the face it failed on.
#[track_caller]
pub(super) fn defect(why: &'static str, face: Option<FaceId>) -> SolidError {
    let failure = Failure::new(OP, OffsetFailure::Defect { why });
    match face {
        Some(face) => failure.at(EntityRef::Face(face)),
        None => failure,
    }
    .into_error()
}

/// Runs `f` with the kernel context's callbacks lent out, so that no application sees
/// the edits of work on private B-reps whose result may still be refused.
pub(super) fn privately<T>(k: &mut Kernel, f: impl FnOnce(&mut Kernel) -> Result<T>) -> Result<T> {
    let lease = k.context.take_notify_lease()?;
    let result = f(k);
    k.context.restore_notify_lease(lease)?;
    result
}

#[derive(Clone, Copy)]
struct PlanarFace {
    id: FaceId,
    origin: Point3,
    normal: Vec3,
}

/// The construction-side use of an independent offset sheet. The upward use is
/// always `Same`; the native shell head retains the orientation given to `MakeFace`.
pub(super) fn signed_sheet_faceuse(b: &Brep, f: FaceId) -> Result<Option<FaceuseId>> {
    let Some(up) = face2::upward_faceuse(b, f)? else {
        return Ok(None);
    };
    let Some(shell) = b.faceuse(up)?.shell else {
        return Ok(None);
    };
    let ShellMembers::Faceuses(members) = &b.shell(shell)?.members else {
        return Ok(None);
    };
    let [first, second] = members.as_slice() else {
        return Ok(None);
    };
    let a = b.faceuse(*first)?;
    let c = b.faceuse(*second)?;
    Ok((a.face == Some(f)
        && c.face == Some(f)
        && a.shell == Some(shell)
        && c.shell == Some(shell)
        && a.mate == Some(*second)
        && c.mate == Some(*first)
        && matches!(
            (a.orientation, c.orientation),
            (OrientType::Same, OrientType::Opposite) | (OrientType::Opposite, OrientType::Same)
        ))
    .then_some(*first))
}

fn planar_face(b: &Brep, id: FaceId) -> Result<Option<PlanarFace>> {
    let Some(surface) = b.face(id)?.surface.as_ref() else {
        return Ok(None);
    };
    let SurfaceKind::Plane(p) = surface.kind() else {
        return Ok(None);
    };
    let Some(fu) = signed_sheet_faceuse(b, id)? else {
        return Ok(None);
    };
    let sign = if b.faceuse(fu)?.orientation == OrientType::Same {
        1.0
    } else {
        -1.0
    };
    Ok(Some(PlanarFace {
        id,
        origin: p.position().origin(),
        normal: p.evaluate_normal().unitize()? * sign,
    }))
}

/// Opposite planes the planar Boolean accepts as coincident: within roundoff, and
/// closer than twice the sheets' tolerance, the bound `boolean_2d_keyed` applies.
fn opposite(a: PlanarFace, b: PlanarFace, tolerance: f64) -> bool {
    let gap = a.normal.dot(b.origin - a.origin).abs();
    let magnitude = a.normal.x.abs() * (a.origin.x.abs() + b.origin.x.abs())
        + a.normal.y.abs() * (a.origin.y.abs() + b.origin.y.abs())
        + a.normal.z.abs() * (a.origin.z.abs() + b.origin.z.abs());
    // Resolve independently computed descriptions of the same support plane
    // within their subtraction/dot-product roundoff, not a geometric distance.
    magnitude.is_finite()
        && a.normal == -b.normal
        && gap <= gamma(32) * magnitude
        && gap < 2.0 * tolerance
}

/// Whether offset faces `a` and `b` stand for a common original face. Such faces are
/// pieces of one source face (a face rebuilt where its loops crossed, with an island
/// of its own), not opposing walls: their opposite construction sides are bookkeeping,
/// and cancelling them would delete the island.
fn share_an_original(exec: &OffsetExecutive, a: FaceId, b: FaceId) -> bool {
    let of_a = exec.faceuse_to_faces.firsts(a);
    exec.faceuse_to_faces
        .firsts(b)
        .iter()
        .any(|original| of_a.contains(original))
}

struct Sheet {
    source: PlanarFace,
    brep: Brep,
    changed: bool,
}

fn copy_sheet(k: &mut Kernel, source: &mut Brep, face: PlanarFace) -> Result<Sheet> {
    let mut brep = bp3::new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    brep.set_tolerance(source.tolerance());
    brep.set_editing_enabled(true);
    let (_, copied) = copy_faces_carrying(k, source, &[face.id], &mut brep, true)?;
    if !copied {
        return Err(defect(
            "copying an offset face into its own sheet produced no face, though \
             the face is a live planar sheet of the offset",
            Some(face.id),
        ));
    }
    brep.refresh_cached_topology();
    keep_orientation(&mut brep, face.normal)?;
    Ok(Sheet {
        source: face,
        brep,
        changed: false,
    })
}

fn planar_boolean(k: &mut Kernel, a: Brep, b: Brep, operation: Boolean2d) -> Result<Brep> {
    match privately(k, |k| boolean_2d(k, a, b, operation))? {
        Boolean2dOutcome::Result(result) => Ok(result),
        Boolean2dOutcome::Unassigned { .. } => Err(defect(
            "the planar Boolean of two offset sheets left its result unassigned, though \
             both operands are planar sheets of one plane",
            None,
        )),
    }
}

fn keep_orientation(b: &mut Brep, normal: Vec3) -> Result<()> {
    for f in bp1::faces(b, None)? {
        let Some(p) = planar_face(b, f)? else {
            return Err(defect(
                "a fragment of a planar sheet is not a planar sheet face, though a \
                 planar Boolean keeps the plane of its operands",
                Some(f),
            ));
        };
        if p.normal == -normal {
            // CopyFaces and Boolean2D can choose the other circular-list head.
            // Retain the known construction side of this independent sheet.
            let fu = signed_sheet_faceuse(b, f)?.ok_or_else(|| {
                defect(
                    "a sheet fragment is not a two-sided lamina face, though a planar \
                     Boolean of two sheets leaves laminas",
                    Some(f),
                )
            })?;
            let shell = b
                .faceuse(fu)?
                .shell
                .ok_or_else(|| defect("a sheet fragment's face use has no shell", Some(f)))?;
            let ShellMembers::Faceuses(members) = &mut b.shell_mut(shell)?.members else {
                return Err(defect(
                    "a sheet fragment's shell is not a list of face uses",
                    Some(f),
                ));
            };
            // The first member moves to the end, as `rotate_left(1)` would, without its panic on
            // an empty list.
            let rotated: Vec<_> = members
                .iter()
                .skip(1)
                .chain(members.iter().take(1))
                .copied()
                .collect();
            *members = rotated;
        } else if p.normal != normal {
            return Err(defect(
                "a sheet fragment has a normal that is neither side of its source's \
                 plane, though a planar Boolean keeps the plane of its operands",
                Some(f),
            ));
        }
    }
    Ok(())
}

fn cancel_pair(k: &mut Kernel, a: &mut Sheet, b: &mut Sheet) -> Result<()> {
    let tolerance = max2(a.brep.tolerance(), b.brep.tolerance());
    if !opposite(a.source, b.source, tolerance)
        || bp1::faces(&a.brep, None)?.is_empty()
        || bp1::faces(&b.brep, None)?.is_empty()
    {
        return Ok(());
    }
    let intersection = planar_boolean(
        k,
        a.brep.clone_standing_for_self()?,
        b.brep.clone_standing_for_self()?,
        Boolean2d::Intersection,
    )?;
    if bp1::faces(&intersection, None)?.is_empty() {
        return Ok(());
    }
    // Both differences use the same pre-cancellation sheets. Updating A first
    // must not make B retain the very overlap that has just been removed from A.
    let mut new_a = planar_boolean(
        k,
        a.brep.clone_standing_for_self()?,
        b.brep.clone_standing_for_self()?,
        Boolean2d::Difference,
    )?;
    let mut new_b = planar_boolean(
        k,
        b.brep.clone_standing_for_self()?,
        a.brep.clone_standing_for_self()?,
        Boolean2d::Difference,
    )?;
    keep_orientation(&mut new_a, a.source.normal)?;
    keep_orientation(&mut new_b, b.source.normal)?;
    a.brep = new_a;
    b.brep = new_b;
    a.changed = true;
    b.changed = true;
    Ok(())
}

fn take_sheet(sheets: &mut [Option<Sheet>], index: usize) -> Result<Sheet> {
    sheets
        .get_mut(index)
        .and_then(Option::take)
        .ok_or_else(|| defect("a sheet of the cancellation list is missing", None))
}

fn put_sheet(sheets: &mut [Option<Sheet>], index: usize, sheet: Sheet) -> Result<()> {
    let slot = sheets
        .get_mut(index)
        .ok_or_else(|| defect("a sheet slot of the cancellation list is missing", None))?;
    *slot = Some(sheet);
    Ok(())
}

/// Remove only the shared regions of coincident, oppositely oriented planes.
/// Copied patch attributes travel through the planar booleans; replacement and
/// deletion notify under the offset result's caller-owned identities.
///
/// The executive's offset correspondence follows each replaced face to the fragments
/// that took its place, so they keep its role and its merge exclusions.
pub(super) fn coincident_opposites(
    exec: &mut OffsetExecutive,
    k: &mut Kernel,
    b: &mut Brep,
    notifications: &mut OffsetNotifications<'_>,
) -> Result<()> {
    super::spurs::repair(exec, k, b, notifications)?;
    super::empty::remove(k, b, notifications)?;
    let mut planes = Vec::new();
    for f in bp1::faces(b, None)? {
        if let Some(p) = planar_face(b, f)? {
            planes.push(p);
        }
    }
    let tolerance = b.tolerance();
    let mut selected = Vec::new();
    for &p in &planes {
        k.check_cancelled(OP)?;
        if planes.iter().any(|&q| {
            p.id != q.id && opposite(p, q, tolerance) && !share_an_original(exec, p.id, q.id)
        }) {
            selected.push(p);
        }
    }
    if selected.is_empty() {
        return Ok(());
    }
    attributes::write_back_attributes(b, notifications.result_key_of, notifications.attributes)?;
    let mut sheets = Vec::new();
    for face in selected {
        k.check_cancelled(OP)?;
        sheets.push(Some(copy_sheet(k, b, face)?));
    }
    // Original source-face pairs form a finite work list. New boolean fragments
    // remain in their source's sheet and never create another pair to revisit.
    let count = sheets.len();
    for i in 0..count {
        let mut a = take_sheet(&mut sheets, i)?;
        for j in i.saturating_add(1)..count {
            k.check_cancelled(OP)?;
            let mut other = take_sheet(&mut sheets, j)?;
            if !share_an_original(exec, a.source.id, other.source.id) {
                cancel_pair(k, &mut a, &mut other)?;
            }
            put_sheet(&mut sheets, j, other)?;
        }
        put_sheet(&mut sheets, i, a)?;
    }
    let replacements: Vec<_> = sheets.into_iter().flatten().filter(|s| s.changed).collect();
    if replacements.is_empty() {
        return Ok(());
    }
    let removed: Vec<_> = replacements.iter().map(|s| s.source.id).collect();
    let mut stores = NotificationStores {
        records: &mut *notifications.records,
        attributes: &mut *notifications.attributes,
        key_of: notifications.result_key_of,
    };
    NotifyState::from_kernel_stores(&mut stores).with_kernel_env(k, |k, env| {
        for sheet in &replacements {
            if bp1::faces(&sheet.brep, None)?.is_empty() {
                exec.replace_face_relations(sheet.source.id, &[])?;
            } else {
                let before: BTreeSet<FaceId> = bp1::faces(b, None)?.into_iter().collect();
                bp3::merge_brep(
                    k,
                    env,
                    b,
                    &sheet.brep,
                    &loopuse::notify_ref,
                    None,
                    None,
                    false,
                )?;
                let fragments: Vec<FaceId> = bp1::faces(b, None)?
                    .into_iter()
                    .filter(|f| !before.contains(f))
                    .collect();
                exec.replace_face_relations(sheet.source.id, &fragments)?;
            }
        }
        let editing = b.editing_enabled();
        bp5::remove_faces_notified(k, b, editing, &removed, env)
    })?;
    b.refresh_cached_topology();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(id: u32, x: f64, normal_x: f64) -> PlanarFace {
        PlanarFace {
            id: FaceId::from_index(id),
            origin: Point3::new(x, 0.0, 0.0),
            normal: Vec3::new(normal_x, 0.0, 0.0),
        }
    }

    #[test]
    fn opposite_planes_cancel_only_within_the_booleans_bound() {
        let near = 2f64.powi(30);
        let gap = 2f64.powi(-20);
        let tolerance = 1e-7;
        // Within the roundoff cutoff at this magnitude, but beyond twice the tolerance.
        assert!(!opposite(
            plane(1, near, 1.0),
            plane(2, near + gap, -1.0),
            tolerance
        ));
        assert!(opposite(
            plane(1, near, 1.0),
            plane(2, near, -1.0),
            tolerance
        ));
        assert!(!opposite(plane(1, 0.0, 1.0), plane(2, 1.0, 1.0), tolerance));
    }
}
