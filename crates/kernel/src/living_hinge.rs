//! Living hinge calls (a kerf-bending pattern of staggered slots over a sketch area in a flat
//! plate): solidcore's planner and the plate's outline ([`solidcore_ops::living_hinge`]), each
//! inside [`guard`], with its refusals as [`KernelError::Invalid`]. Computes nothing itself.

use solidcore_ops::living_hinge as ops;
use solidcore_record::living_hinge::{HingeSlot, LivingHingePlan, LivingHingeSpec};
use solidcore_record::{Loop2, Plane, Region2, Vec2};

pub use solidcore_ops::living_hinge::{MAX_SLOTS, PlateFace, PlateOutline};

use crate::body::Body;
use crate::{KernelError, Result, guard};

/// What the guard and the messages name.
const WHAT: &str = "living hinge";

/// The pattern a new living hinge starts from (the kernel's defaults).
#[must_use]
pub fn default_spec() -> LivingHingeSpec {
    ops::default_spec()
}

/// The slots of a living hinge over the area `field` in the plate's outline `plate` (closed loops
/// of the sketch plane), along `angle` (radians from the sketch's X axis), centred on the area or
/// anchored at `anchor`. The plan is in the hinge frame.
///
/// # Errors
/// [`KernelError::Invalid`] for a refused parameter, area or plate, or too many slots;
/// [`KernelError::Internal`] for a kernel panic.
pub fn plan(field: &[Loop2], plate: &[Loop2], spec: &LivingHingeSpec, angle: f64, anchor: Option<Vec2>) -> Result<LivingHingePlan> {
    guard(WHAT, || ops::plan_living_hinge(field, plate, spec, angle, anchor).map_err(|e| KernelError::Invalid(e.to_string())))
}

/// Whether the area `field` and the plate's outline `plate` overlap (touching is not meeting);
/// `false` when either cannot be followed by chords.
///
/// # Errors
/// [`KernelError::Internal`] for a kernel panic.
pub fn meets(field: &[Loop2], plate: &[Loop2]) -> Result<bool> {
    guard(WHAT, || Ok(ops::meets(field, plate)))
}

/// The region of `slot` in the hinge frame: a stadium `slot_width` wide around its centre line.
///
/// # Errors
/// [`KernelError::Invalid`] for a width that is not finite and positive;
/// [`KernelError::Internal`] for a kernel panic.
pub fn slot_region(slot: &HingeSlot, slot_width: f64) -> Result<Region2> {
    if !(slot_width.is_finite() && slot_width > 0.0) {
        return Err(KernelError::Invalid(format!("{WHAT}: the slot width must be finite and positive")));
    }
    guard(WHAT, || Ok(ops::slot_region(slot, slot_width)))
}

/// The outline of the plate `body` seen along `plane`'s normal: its flat faces parallel to the
/// plane, and the union of those facing along the normal (empty when there is none).
///
/// # Errors
/// [`KernelError::Invalid`] for a mesh body, a body without size or a face not read exactly;
/// [`KernelError::Failed`] for a body the kernel cannot take or a failed union;
/// [`KernelError::Internal`] for a kernel panic.
pub fn plate_outline(body: &Body, plane: &Plane) -> Result<PlateOutline> {
    if body.is_mesh() {
        return Err(KernelError::Invalid(format!("{WHAT}: the plate is a mesh body, not a solid")));
    }
    let (ops_body, _) = body.ops_body().ok_or_else(|| KernelError::Failed(format!("{WHAT}: the kernel could not take the plate")))?;
    guard(WHAT, || {
        ops::plate_outline(ops_body, plane).map_err(|e| match e {
            solidcore_ops::Error::Empty => KernelError::Invalid(format!("{WHAT}: the plate has no size")),
            solidcore_ops::Error::Unsupported(m) => KernelError::Invalid(format!("{WHAT}: the plate: {m}")),
            solidcore_ops::Error::Internal(m) => KernelError::Internal(format!("{WHAT}: {m}")),
            e => KernelError::Failed(format!("{WHAT}: the plate: {e}")),
        })
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use solidcore_record::{Seg2, Vec3};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
        let c = [Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)];
        Loop2 { segs: (0..4).map(|i| Seg2::Line { a: c[i], b: c[(i + 1) % 4] }).collect() }
    }

    #[test]
    fn a_field_inside_a_plate_meets_it_and_plans_slots() {
        let plate = [rect(0.0, 0.0, 100.0, 60.0)];
        let field = [rect(20.0, 10.0, 80.0, 50.0)];
        assert!(meets(&field, &plate).unwrap());
        let p = plan(&field, &plate, &default_spec(), 0.0, None).unwrap();
        assert!(!p.slots.is_empty());
        let r = slot_region(&p.slots[0], 0.5).unwrap();
        assert!(r.holes.is_empty());
    }

    #[test]
    fn a_field_beside_the_plate_does_not_meet_it() {
        assert!(!meets(&[rect(200.0, 0.0, 260.0, 40.0)], &[rect(0.0, 0.0, 100.0, 60.0)]).unwrap());
    }

    #[test]
    fn refusals_are_invalid_input() {
        let plate = [rect(0.0, 0.0, 100.0, 60.0)];
        let spec = LivingHingeSpec { slot_length: -1.0, ..default_spec() };
        let e = plan(&[rect(20.0, 10.0, 80.0, 50.0)], &plate, &spec, 0.0, None).unwrap_err();
        assert!(matches!(e, KernelError::Invalid(ref m) if m.starts_with("living hinge")), "{e}");
        let p = plan(&[rect(20.0, 10.0, 80.0, 50.0)], &plate, &default_spec(), 0.0, None).unwrap();
        assert!(matches!(slot_region(&p.slots[0], 0.0), Err(KernelError::Invalid(_))));
    }

    #[test]
    fn a_box_plate_outline_is_its_top() {
        let b = crate::box_solid(Vec3::new(0.0, 0.0, -2.0), Vec3::new(40.0, 20.0, 0.0)).unwrap();
        let o = plate_outline(&b, &Plane::XY).unwrap();
        assert_eq!((o.walls, o.other, o.faces.len(), o.regions.len()), (4, 0, 2, 1));
        assert_eq!(o.faces.iter().filter(|f| f.up && f.in_plane).count(), 1);
    }
}
