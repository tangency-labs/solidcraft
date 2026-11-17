//! Tab and Slot calls (tabs on one flat plate T that fit slots cut in another flat plate S where
//! T meets S square): solidcore's plan and its cut ([`solidcore_ops::plate_contact`]), each inside
//! [`guard`], with the bodies made back into this crate's [`Body`]. Computes nothing itself.

pub use solidcore::plate_contact::{PairSkip, Plane};
pub use solidcore::tab_slot::Layout;
pub use solidcore_ops::plate_contact::{JointSkip, NearPair, StretchPlan, TabPlan, TabSlotPlan, TabSlotSpec};

use crate::body::Body;
use crate::{KernelError, Result, guard};

/// What the guard and the messages name.
const WHAT: &str = "tab and slot";

/// The recipe applied ([`cut`]): T with its tabs joined, S with its slots cut, and the plan.
#[derive(Debug)]
pub struct TabSlotCut {
    /// The tab plate T with every made tab joined.
    pub tab_plate: Body,
    /// The slot plate S with every made tab's slot cut.
    pub slot_plate: Body,
    /// The stretches and tabs, made or skipped.
    pub plan: Vec<StretchPlan>,
}

fn kernel_error(e: solidcore_ops::Error) -> KernelError {
    match e {
        solidcore_ops::Error::Empty => KernelError::Invalid(format!("{WHAT}: a plate has no size")),
        solidcore_ops::Error::Unsupported(m) => KernelError::Invalid(format!("{WHAT}: {m}")),
        e @ (solidcore_ops::Error::Refused { .. } | solidcore_ops::Error::Argument) => KernelError::Invalid(format!("{WHAT}: {e}")),
        solidcore_ops::Error::Internal(m) => KernelError::Internal(format!("{WHAT}: {m}")),
        e => KernelError::Failed(format!("{WHAT}: {e}")),
    }
}

/// One plate as solidcore-ops' body: refused for a mesh body or one the kernel cannot take.
fn ops_of<'a>(b: &'a Body, name: &str) -> Result<&'a solidcore_ops::Body> {
    if b.is_mesh() {
        return Err(KernelError::Invalid(format!("{WHAT}: the {name} plate is a mesh body, not a solid")));
    }
    b.ops_body().map(|(o, _)| o).ok_or_else(|| KernelError::Failed(format!("{WHAT}: the kernel could not take the {name} plate")))
}

/// Both plates as solidcore-ops' bodies: refused for a mesh body or one the kernel cannot take.
fn ops_pair<'a>(tab: &'a Body, slot: &'a Body) -> Result<(&'a solidcore_ops::Body, &'a solidcore_ops::Body)> {
    Ok((ops_of(tab, "tab")?, ops_of(slot, "slot")?))
}

fn back(b: &solidcore_ops::Body) -> Result<Body> {
    let (record, _) = b.to_record().map_err(kernel_error)?;
    Body::from_record(record)
}

/// The plan of the tab plate `tab` against the slot plate `slot` meeting on `p` (S's face T
/// stands on): the strip's run and its stretches with their tabs, or why the pair has no joint.
/// Neither body changes.
///
/// # Errors
/// [`KernelError::Invalid`] for a mesh body or a refused spec or tolerance;
/// [`KernelError::Failed`] for a body the kernel cannot take; [`KernelError::Internal`] for a
/// kernel panic.
pub fn plan(tab: &Body, slot: &Body, p: Plane, spec: TabSlotSpec, tol: f64) -> Result<TabSlotPlan> {
    let (t, s) = ops_pair(tab, slot)?;
    guard(WHAT, || t.tab_slot_plan(s, p, spec, tol).map_err(kernel_error))
}

/// [`plan`] applied: every made tab joined to a copy of `tab`, its slot cut from a copy of
/// `slot`; or why the pair has no joint. Neither body changes.
///
/// # Errors
/// Those of [`plan`], and [`KernelError::Failed`] for a failed boolean or a result the record
/// cannot carry.
pub fn cut(tab: &Body, slot: &Body, p: Plane, spec: TabSlotSpec, tol: f64) -> Result<core::result::Result<TabSlotCut, PairSkip>> {
    let (t, s) = ops_pair(tab, slot)?;
    guard(WHAT, || match t.tab_slot_cut(s, p, spec, tol).map_err(kernel_error)? {
        Err(skip) => Ok(Err(skip)),
        Ok(c) => Ok(Ok(TabSlotCut { tab_plate: back(&c.tab_plate)?, slot_plate: back(&c.slot_plate)?, plan: c.plan })),
    })
}

/// The planes of `slot`'s planar faces, each facing out of its material ([`Body::planar_face`]),
/// one entry per plane: faces in one plane (normals within 1e-9, offsets within `tol`) share it.
/// In face order; empty for a mesh body or a body with no planar face.
#[must_use]
pub fn slot_planes(slot: &Body, tol: f64) -> Vec<Plane> {
    let mut out: Vec<Plane> = Vec::new();
    for i in 0..slot.face_count() {
        let Some(f) = slot.planar_face(i) else { continue };
        let (x, y) = (f.x, f.y);
        let n = solidcore::Vec3::new(x.y * y.z - x.z * y.y, x.z * y.x - x.x * y.z, x.x * y.y - x.y * y.x);
        let Ok(p) = Plane::through(solidcore::Point3::new(f.origin.x, f.origin.y, f.origin.z), n) else { continue };
        if !out.iter().any(|q| q.normal.dot(p.normal) > 1.0 - 1e-9 && (q.offset - p.offset).abs() <= tol) {
            out.push(p);
        }
    }
    out
}

/// The joint plane P of the tab plate `tab` on the slot plate `slot` (design 5.3): of `slot`'s
/// planes ([`slot_planes`]), the one `tab` stands on, and its joint; of several, the face pick
/// `near` chooses ([`solidcore_ops::Body::plate_pair_near`]), else
/// [`PairSkip::SeveralContacts`]; no joint: why. Neither body changes.
///
/// # Errors
/// [`KernelError::Invalid`] for a mesh body, a slot plate with no planar face, or a refused
/// tolerance; [`KernelError::Failed`] for a body the kernel cannot take;
/// [`KernelError::Internal`] for a kernel panic.
pub fn joint_plane(tab: &Body, slot: &Body, near: Option<solidcore_record::Vec3>, tol: f64) -> Result<NearPair> {
    let (t, s) = ops_pair(tab, slot)?;
    let planes = slot_planes(slot, tol);
    let near = near.map(|q| solidcore::Point3::new(q.x, q.y, q.z));
    guard(WHAT, || t.plate_pair_near(s, &planes, near, tol).map_err(kernel_error))
}

/// One feature's joints in pick order (design 5.7, [`solidcore_ops::plate_contact::tab_slot_tool_faces`]):
/// each `(t, s, p)` names the tab and slot plate by index into `plates` and P. The answer: per
/// joint `None` (made) or why it is skipped (the pair's skip, `meets_other_plate_joint`,
/// `other_direction`), and per plate the tool faces its boolean takes over the made joints. No
/// plate changes.
///
/// # Errors
/// [`KernelError::Invalid`] for a mesh plate, a plate index out of range or a refused spec or
/// tolerance; [`KernelError::Failed`] for a plate the kernel cannot take;
/// [`KernelError::Internal`] for a kernel panic.
pub fn joints(plates: &[&Body], joints: &[(usize, usize, Plane)], spec: TabSlotSpec, tol: f64) -> Result<(Vec<Option<JointSkip>>, Vec<usize>)> {
    let ops = plates.iter().map(|b| ops_of(b, "joint")).collect::<Result<Vec<_>>>()?;
    guard(WHAT, || {
        let owned = ops.iter().map(|b| b.copy().map_err(kernel_error)).collect::<Result<Vec<_>>>()?;
        solidcore_ops::plate_contact::tab_slot_tool_faces(&owned, joints, spec, tol).map_err(kernel_error)
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;

    const TOL: f64 = 1.0e-3;

    /// Fixture F (design 16): Bottom (0,0,0)-(160,100,6), Wall (20,30,6)-(140,36,66) on its top.
    fn fixture_f() -> (Body, Body, Plane) {
        let bottom = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(160.0, 100.0, 6.0)).unwrap();
        let wall = crate::box_solid(Vec3::new(20.0, 30.0, 6.0), Vec3::new(140.0, 36.0, 66.0)).unwrap();
        (wall, bottom, Plane { normal: solidcore::Vec3::Z, offset: 6.0 })
    }

    fn spec(layout: Layout, width: f64, end_distance: f64) -> TabSlotSpec {
        TabSlotSpec { layout, width, end_distance, clearance: 0.1, min_land: 0.5, protrusion: 0.0, overshoot: 0.5 }
    }

    fn volume(b: &Body) -> f64 {
        crate::measure::measure_mass(b).unwrap().volume
    }

    fn one_lump_checked(b: &Body) {
        assert_eq!(b.lumps().unwrap().len(), 1);
        let (o, _) = b.ops_body().unwrap();
        let r = o.check(solidcore_ops::CheckLevel::All).unwrap();
        assert!(r.is_valid(), "{r:?}");
    }

    /// 16.3, E1 laser on F with `box_solid` (count 3, W 20, e 10, c 0.1), planned and cut: the
    /// plan's three tabs are all made; Wall 45 360 and Bottom 93 745.68 mm³, 18 faces each, one
    /// lump each, solidcore's check clean. Neither input changes.
    #[test]
    fn e1_laser_planned_and_cut() {
        let (wall, bottom, p) = fixture_f();
        let s = spec(Layout::Count(3), 20.0, 10.0);
        let (_, stretches) = plan(&wall, &bottom, p, s, TOL).unwrap().unwrap();
        let tabs: Vec<&TabPlan> = stretches.iter().filter_map(|st| st.tabs.as_ref().ok()).flatten().collect();
        assert_eq!(tabs.len(), 3);
        assert!(tabs.iter().all(|t| t.skip.is_none() && t.tab.is_ok()));
        let c = cut(&wall, &bottom, p, s, TOL).unwrap().unwrap();
        assert_eq!(c.plan, stretches);
        assert!((volume(&c.tab_plate) - 45_360.0).abs() < 1e-6, "{}", volume(&c.tab_plate));
        assert!((volume(&c.slot_plate) - 93_745.68).abs() < 1e-6, "{}", volume(&c.slot_plate));
        assert_eq!((c.tab_plate.face_count(), c.slot_plate.face_count()), (18, 18));
        one_lump_checked(&c.tab_plate);
        one_lump_checked(&c.slot_plate);
        assert!((volume(&wall) - 43_200.0).abs() < 1e-6);
        assert!((volume(&bottom) - 96_000.0).abs() < 1e-6);
    }

    /// 16.3, E1b defaults on F (auto layout, W = e = 18, c 0.1): two tabs, cut; Wall 44 496 and
    /// Bottom 94 645.92 mm³, 14 faces each, one lump each, solidcore's check clean.
    #[test]
    fn e1b_defaults_planned_and_cut() {
        let (wall, bottom, p) = fixture_f();
        let s = spec(Layout::Auto, 18.0, 18.0);
        let (_, stretches) = plan(&wall, &bottom, p, s, TOL).unwrap().unwrap();
        let tabs: Vec<&TabPlan> = stretches.iter().filter_map(|st| st.tabs.as_ref().ok()).flatten().collect();
        assert_eq!(tabs.len(), 2);
        assert!(tabs.iter().all(|t| t.skip.is_none() && t.tab.is_ok()));
        let c = cut(&wall, &bottom, p, s, TOL).unwrap().unwrap();
        assert_eq!(c.plan, stretches);
        assert!((volume(&c.tab_plate) - 44_496.0).abs() < 1e-6, "{}", volume(&c.tab_plate));
        assert!((volume(&c.slot_plate) - 94_645.92).abs() < 1e-6, "{}", volume(&c.slot_plate));
        assert_eq!((c.tab_plate.face_count(), c.slot_plate.face_count()), (14, 14));
        one_lump_checked(&c.tab_plate);
        one_lump_checked(&c.slot_plate);
    }

    /// Refusals: a mesh body as either plate, and a tolerance of 0, are [`KernelError::Invalid`]
    /// for both the plan and the cut.
    #[test]
    fn mesh_body_and_zero_tolerance_are_invalid() {
        let (wall, bottom, p) = fixture_f();
        let s = spec(Layout::Count(3), 20.0, 10.0);
        let pos = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0)];
        let mesh = crate::mesh_body(&pos, &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]).unwrap();
        assert!(mesh.is_mesh());
        assert!(matches!(plan(&mesh, &bottom, p, s, TOL), Err(KernelError::Invalid(_))));
        assert!(matches!(plan(&wall, &mesh, p, s, TOL), Err(KernelError::Invalid(_))));
        assert!(matches!(cut(&mesh, &bottom, p, s, TOL), Err(KernelError::Invalid(_))));
        assert!(matches!(cut(&wall, &mesh, p, s, TOL), Err(KernelError::Invalid(_))));
        let zero = plan(&wall, &bottom, p, s, 0.0);
        assert!(matches!(zero, Err(KernelError::Invalid(_))), "{zero:?}");
        let zero = cut(&wall, &bottom, p, s, 0.0);
        assert!(matches!(zero, Err(KernelError::Invalid(_))), "{:?}", zero.map(|r| r.map(|c| c.plan)));
    }

    /// 5.3 on F: Bottom's planes are its six faces'; Wall stands on its top (z = 6), found with
    /// and without a face pick. Wall lifted 4 mm off Bottom has no joint. A mesh is refused.
    #[test]
    fn joint_plane_on_f() {
        let (wall, bottom, p) = fixture_f();
        assert_eq!(slot_planes(&bottom, TOL).len(), 6);
        let (q, _) = joint_plane(&wall, &bottom, None, TOL).unwrap().unwrap();
        assert!((q.normal.dot(p.normal) - 1.0).abs() < 1e-9 && (q.offset - p.offset).abs() < 1e-9, "{q:?}");
        let (q, _) = joint_plane(&wall, &bottom, Some(Vec3::new(80.0, 33.0, 6.0)), TOL).unwrap().unwrap();
        assert!((q.offset - p.offset).abs() < 1e-9, "{q:?}");
        let lifted = crate::box_solid(Vec3::new(20.0, 30.0, 10.0), Vec3::new(140.0, 36.0, 70.0)).unwrap();
        assert!(joint_plane(&lifted, &bottom, None, TOL).unwrap().is_err());
        let pos = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0)];
        let mesh = crate::mesh_body(&pos, &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]).unwrap();
        assert!(matches!(joint_plane(&wall, &mesh, None, TOL), Err(KernelError::Invalid(_))));
    }

    /// 5.7 on F (E1 spec): one joint is made and both plates take tool faces; the same joint asked
    /// again meets the first's slots (`meets_other_plate_joint`); an index out of range is Invalid.
    #[test]
    fn joints_in_order_on_f() {
        let (wall, bottom, p) = fixture_f();
        let s = spec(Layout::Count(3), 20.0, 10.0);
        let (skips, faces) = joints(&[&wall, &bottom], &[(0, 1, p)], s, TOL).unwrap();
        assert_eq!(skips, vec![None]);
        assert!(faces.len() == 2 && faces.iter().all(|&n| n > 0), "{faces:?}");
        let (skips, again) = joints(&[&wall, &bottom], &[(0, 1, p), (0, 1, p)], s, TOL).unwrap();
        assert_eq!(skips, vec![None, Some(JointSkip::MeetsOtherJoint(0))]);
        assert_eq!(skips[1].as_ref().unwrap().reason_id(), "meets_other_plate_joint");
        assert_eq!(again, faces);
        assert!(matches!(joints(&[&wall, &bottom], &[(0, 5, p)], s, TOL), Err(KernelError::Invalid(_))));
    }
}
