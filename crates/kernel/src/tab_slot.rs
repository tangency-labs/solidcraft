//! Tab and Slot calls (tabs on one flat plate T that fit slots cut in another flat plate S where
//! T meets S square): solidcore's plan and its cut ([`solidcore_ops::plate_contact`]), each inside
//! [`guard`], with the bodies made back into this crate's [`Body`]. Computes nothing itself.

pub use solidcore::plate_contact::{PairSkip, Plane};
pub use solidcore::tab_slot::Layout;
pub use solidcore_ops::plate_contact::{StretchPlan, TabPlan, TabSlotPlan, TabSlotSpec};

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
        solidcore_ops::Error::Internal(m) => KernelError::Internal(format!("{WHAT}: {m}")),
        e => KernelError::Failed(format!("{WHAT}: {e}")),
    }
}

/// Both plates as solidcore-ops' bodies: refused for a mesh body or one the kernel cannot take.
fn ops_pair<'a>(tab: &'a Body, slot: &'a Body) -> Result<(&'a solidcore_ops::Body, &'a solidcore_ops::Body)> {
    let one = |b: &'a Body, name: &str| {
        if b.is_mesh() {
            return Err(KernelError::Invalid(format!("{WHAT}: the {name} plate is a mesh body, not a solid")));
        }
        b.ops_body().map(|(o, _)| o).ok_or_else(|| KernelError::Failed(format!("{WHAT}: the kernel could not take the {name} plate")))
    };
    Ok((one(tab, "tab")?, one(slot, "slot")?))
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
}
