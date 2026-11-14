//! Cancel overlapping planar offset patches with opposite orientations.
//!
//! The two signed sheets contribute nothing where they overlap. Exact planar
//! differences retain their non-overlapping regions before the solid merge sees
//! coincident edges. Original face pairs are visited once; their evolving sheet
//! remainders stay in private B-reps until every cancellation is constructed.
//! Upstream: none (regularized inner offsets).
//! Deviations: no recursion or retries; each planar boolean runs the existing
//! kernel pipeline.

use super::attributes;
use super::p2::OffsetNotifications;
use crate::blend::feature_executive::copy_faces_carrying;
use crate::boolean::api_brep::{boolean_2d, Boolean2d, Boolean2dOutcome};
use crate::error::{Result, SolidError};
use crate::geom::tol::USE_DEFAULT;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::gamma;
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::{p1 as bp1, p3 as bp3, p5 as bp5};
use crate::topology::edge::{NotificationStores, NotifyState};
use crate::topology::face::p2 as face2;
use crate::topology::shell::ShellMembers;
use crate::topology::{loopuse, Brep, FaceId, FaceuseId};

const OP: &str = "offset::offset_executive::cancel";

#[derive(Clone, Copy)]
struct PlanarFace {
    id: FaceId,
    origin: Point3,
    normal: Vec3,
}

/// The construction-side use of an independent offset sheet. The upward use is
/// always `Same`; the native shell head retains the orientation given to MakeFace.
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

fn opposite(a: PlanarFace, b: PlanarFace) -> bool {
    let magnitude = a.normal.x.abs() * (a.origin.x.abs() + b.origin.x.abs())
        + a.normal.y.abs() * (a.origin.y.abs() + b.origin.y.abs())
        + a.normal.z.abs() * (a.origin.z.abs() + b.origin.z.abs());
    // Resolve independently computed descriptions of the same support plane
    // within their subtraction/dot-product roundoff, not a geometric distance.
    magnitude.is_finite()
        && a.normal == -b.normal
        && a.normal.dot(b.origin - a.origin).abs() <= gamma(32) * magnitude
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
        return Err(SolidError::sm_err(OP));
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
    match boolean_2d(k, a, b, operation)? {
        Boolean2dOutcome::Result(result) => Ok(result),
        Boolean2dOutcome::Unassigned { .. } => Err(SolidError::sm_err(OP)),
    }
}

fn keep_orientation(b: &mut Brep, normal: Vec3) -> Result<()> {
    for f in bp1::faces(b, None)? {
        let Some(p) = planar_face(b, f)? else {
            return Err(SolidError::sm_err(OP));
        };
        if p.normal == -normal {
            // CopyFaces and Boolean2D can choose the other circular-list head.
            // Retain the known construction side of this independent sheet.
            let fu = signed_sheet_faceuse(b, f)?.ok_or_else(|| SolidError::sm_err(OP))?;
            let shell = b.faceuse(fu)?.shell.ok_or_else(|| SolidError::sm_err(OP))?;
            let ShellMembers::Faceuses(members) = &mut b.shell_mut(shell)?.members else {
                return Err(SolidError::sm_err(OP));
            };
            members.rotate_left(1);
        } else if p.normal != normal {
            return Err(SolidError::sm_err(OP));
        }
    }
    Ok(())
}

fn cancel_pair(k: &mut Kernel, a: &mut Sheet, b: &mut Sheet) -> Result<()> {
    if !opposite(a.source, b.source)
        || bp1::faces(&a.brep, None)?.is_empty()
        || bp1::faces(&b.brep, None)?.is_empty()
    {
        return Ok(());
    }
    if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
        eprintln!(
            "cancel trace: pair={:?}/{:?} intersection",
            a.source.id, b.source.id
        );
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
    if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
        eprintln!("cancel trace: positive intersection, differences");
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
    if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
        eprintln!("cancel trace: pair completed");
    }
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
        .ok_or_else(|| SolidError::sm_err(OP))
}

fn put_sheet(sheets: &mut [Option<Sheet>], index: usize, sheet: Sheet) -> Result<()> {
    let slot = sheets
        .get_mut(index)
        .ok_or_else(|| SolidError::sm_err(OP))?;
    *slot = Some(sheet);
    Ok(())
}

/// Remove only the shared regions of coincident, oppositely oriented planes.
/// Copied patch attributes travel through the planar booleans; replacement and
/// deletion notify under the offset result's caller-owned identities.
pub(super) fn coincident_opposites(
    k: &mut Kernel,
    b: &mut Brep,
    notifications: &mut OffsetNotifications<'_>,
) -> Result<()> {
    if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
        eprintln!("cancel trace: entered faces={}", b.face_list().len());
    }
    super::spurs::repair(k, b, notifications)?;
    super::empty::remove(k, b, notifications)?;
    let mut planes = Vec::new();
    for f in bp1::faces(b, None)? {
        if let Some(p) = planar_face(b, f)? {
            if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
                eprintln!(
                    "cancel trace: plane={f:?} origin={:?} normal={:?}",
                    p.origin, p.normal
                );
            }
            planes.push(p);
        }
    }
    let selected: Vec<_> = planes
        .iter()
        .copied()
        .filter(|&p| {
            planes
                .iter()
                .copied()
                .any(|q| p.id != q.id && opposite(p, q))
        })
        .collect();
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
            cancel_pair(k, &mut a, &mut other)?;
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
            if !bp1::faces(&sheet.brep, None)?.is_empty() {
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
            }
        }
        let editing = b.editing_enabled();
        bp5::remove_faces_notified(k, b, editing, &removed, env)
    })?;
    b.refresh_cached_topology();
    Ok(())
}
