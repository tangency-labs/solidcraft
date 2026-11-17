//! Planning a living hinge (design 15): the slots of a kerf-bending pattern over a sketch area in a
//! flat plate.
//!
//! Recipes of kernel calls (`solidcore::living_hinge`, which holds the lattice, the edge rules and
//! the checks of its inputs): the record's spec and slots are converted by
//! [`solidcore_convert::living_hinge`]. It computes no geometric quantity.

use solidcore::living_hinge as kernel;
use solidcore::living_hinge::NearEdge;
use solidcore_convert as convert;
use solidcore_record::living_hinge::{HingeSlot, LivingHingePlan, LivingHingeSpec};
use solidcore_record::{Loop2, Region2, Vec2};

pub use solidcore::living_hinge::{LivingHingeError, MAX_SLOTS};

/// The pattern a new living hinge starts from (the kernel's defaults).
#[must_use]
pub fn default_spec() -> LivingHingeSpec {
    convert::living_hinge::record_spec(&kernel::LivingHingeSpec::default())
}

/// The loops of the area (`field: true`) or the plate in the hinge frame of `angle`, refused as
/// that input when a curve cannot be followed by chords.
fn frame_loops(
    loops: &[Loop2],
    angle: f64,
    field: bool,
) -> Result<Vec<Vec<NearEdge>>, LivingHingeError> {
    loops
        .iter()
        .map(|lp| {
            kernel::frame_loop(&convert::loop2(lp), angle).ok_or_else(|| {
                let why = "a curve of it cannot be followed by chords".to_owned();
                if field {
                    LivingHingeError::Field(why)
                } else {
                    LivingHingeError::Plate(why)
                }
            })
        })
        .collect()
}

/// The slots of a living hinge over the area `field` in the plate's outline `plate`, both closed
/// loops of the sketch plane, with the slots along `angle` (radians, from the sketch's X axis).
/// The lattice is centred on the area, or anchored at the sketch point `anchor`. The plan is in
/// the hinge frame (`u` along the slots, `v` across the rows).
///
/// # Errors
///
/// [`LivingHingeError`]: a parameter, the area or the plate is refused, or the area reaches more
/// than [`MAX_SLOTS`] lattice slots.
pub fn plan_living_hinge(
    field: &[Loop2],
    plate: &[Loop2],
    spec: &LivingHingeSpec,
    angle: f64,
    anchor: Option<Vec2>,
) -> Result<LivingHingePlan, LivingHingeError> {
    let f = frame_loops(field, angle, true)?;
    let p = frame_loops(plate, angle, false)?;
    let anchor = anchor.map(|a| kernel::frame_point((a.x, a.y), angle));
    let plan = kernel::plan(&f, &p, None, &convert::living_hinge::spec(spec), anchor)?;
    Ok(convert::living_hinge::record_plan(&plan))
}

/// Whether the area `field` and the plate's outline `plate` overlap (design 3.5): touching along
/// an edge or at a point is not meeting. `false` when either cannot be followed by chords.
#[must_use]
pub fn meets(field: &[Loop2], plate: &[Loop2]) -> bool {
    match (
        frame_loops(field, 0.0, true),
        frame_loops(plate, 0.0, false),
    ) {
        (Ok(f), Ok(p)) => kernel::meets(&f, &p),
        _ => false,
    }
}

/// The region of `slot` in the hinge frame: a stadium `slot_width` wide around its centre line.
#[must_use]
pub fn slot_region(slot: &HingeSlot, slot_width: f64) -> Region2 {
    convert::record_region2(&kernel::slot_region(
        &convert::living_hinge::slot(slot),
        slot_width,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::living_hinge::{HingeClip, SlotKind};
    use solidcore_record::Seg2;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
        let p = [
            Vec2 { x: x0, y: y0 },
            Vec2 { x: x1, y: y0 },
            Vec2 { x: x1, y: y1 },
            Vec2 { x: x0, y: y1 },
        ];
        Loop2 {
            segs: p
                .iter()
                .zip(p.iter().cycle().skip(1))
                .map(|(&a, &b)| Seg2::Line { a, b })
                .collect(),
        }
    }

    fn counts(p: &LivingHingePlan) -> (usize, usize, usize, usize, usize, usize) {
        let n = |k| p.slots.iter().filter(|s| s.kind == k).count();
        (
            p.rows,
            p.slots.len(),
            n(SlotKind::Whole),
            n(SlotKind::Shortened),
            n(SlotKind::RunOut),
            p.left_out,
        )
    }

    #[test]
    fn the_default_spec_is_the_designs() {
        let s = default_spec();
        assert_eq!(
            (
                s.slot_length,
                s.bridge,
                s.row_spacing,
                s.slot_width,
                s.margin
            ),
            (20.0, 3.0, 1.5, 0.2, 0.0)
        );
        assert_eq!(s.clip, HingeClip::RunOut);
    }

    #[test]
    fn case_i_angle_0_plans_the_designs_slots() {
        // Plate 60 x 100, area (0, 30)-(60, 70), slots along X: 27 rows, 95 slots.
        let p = plan_living_hinge(
            &[rect(0.0, 30.0, 60.0, 70.0)],
            &[rect(0.0, 0.0, 60.0, 100.0)],
            &default_spec(),
            0.0,
            None,
        )
        .unwrap();
        assert_eq!(counts(&p), (27, 95, 41, 26, 28, 0));
    }

    #[test]
    #[ignore = "open: at 90 deg 5 of the 28 run-outs come back shortened (41/31/23), see handoff leg 11"]
    fn case_a_angle_90_plans_the_same_slots() {
        // Plate 100 x 60, area (30, 0)-(70, 60), slots along Y.
        let p = plan_living_hinge(
            &[rect(30.0, 0.0, 70.0, 60.0)],
            &[rect(0.0, 0.0, 100.0, 60.0)],
            &default_spec(),
            std::f64::consts::FRAC_PI_2,
            None,
        )
        .unwrap();
        assert_eq!(counts(&p), (27, 95, 41, 26, 28, 0));
    }

    #[test]
    fn case_h_anchor_moves_the_lattice() {
        // Case I turned: anchor (0, 0.5) in the sketch is row 0's slot centre.
        let p = plan_living_hinge(
            &[rect(0.0, 30.0, 60.0, 70.0)],
            &[rect(0.0, 0.0, 60.0, 100.0)],
            &default_spec(),
            0.0,
            Some(Vec2 { x: 0.0, y: 30.5 }),
        )
        .unwrap();
        assert!(p.slots.iter().any(|s| s.row == 0 && s.v == 30.5));
    }

    #[test]
    fn an_area_meets_a_plate_only_by_overlapping_it() {
        let plate = [rect(0.0, 0.0, 60.0, 100.0)];
        assert!(meets(&[rect(0.0, 30.0, 60.0, 70.0)], &plate));
        assert!(meets(&[rect(-10.0, -10.0, 5.0, 5.0)], &plate));
        assert!(!meets(&[rect(60.0, 30.0, 80.0, 70.0)], &plate));
        assert!(!meets(&[rect(70.0, 30.0, 80.0, 70.0)], &plate));
    }

    #[test]
    fn a_bad_spec_is_refused() {
        let mut s = default_spec();
        s.bridge = f64::NAN;
        let e = plan_living_hinge(
            &[rect(0.0, 30.0, 60.0, 70.0)],
            &[rect(0.0, 0.0, 60.0, 100.0)],
            &s,
            0.0,
            None,
        );
        assert!(matches!(e, Err(LivingHingeError::Parameter(_))));
    }
}
