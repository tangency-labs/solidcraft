//! A shallow circular cut after a pocket (#1765): repeated empty numerical answers must not
//! stop the later geometric strategies. Inject the numerical failure on a real small part;
//! the exact rounding and ray classification of the original design are not available.

use super::*;
use crate::{box_solid, cylinder, fillet, measure, mm, sphere, step_export, step_import};
use std::cell::Cell;
use std::f64::consts::PI;

#[test]
fn shallow_cut_after_a_pocket_recovers_from_empty_numerical_attempts() {
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let plate = fillet(&plate, &[Vec3::new(20.0, 0.0, 10.0)], mm(2.0)).unwrap();
    let pocket = box_solid(Vec3::new(5.0, 15.0, 9.25), Vec3::new(35.0, 25.0, 10.25)).unwrap();
    let target = boolean(&plate, &pocket, BoolOp::Cut).unwrap().unwrap();
    let tool = cylinder(Vec3::new(12.0, 15.0, 8.5), Vec3::Z, 5.0, 1.75).unwrap();
    let attempts = Cell::new(0);
    let empty = |_: &Body, _: &Body, _: BoolOp, _: Option<mt::Vector3>, _: f64| {
        attempts.set(attempts.get() + 1);
        Ok((Solid::new_unchecked(Vec::new()), None))
    };
    let (result, diagnostics) =
        capture_boolean_diagnostics(|| boolean_via(&target, &tool, BoolOp::Cut, &|a, b, op| boolean_whole_with(a, b, op, false, &empty)));
    assert!(attempts.get() >= 2, "the first two numerical tries must really have failed");
    let cut = result.unwrap().expect("a shallow cut leaves the plate");
    assert!(cut.validity().is_empty(), "{:?}", cut.validity());
    assert_eq!(cut.lumps().unwrap().len(), 1);
    // Half of the circle overlaps the existing 0.75 mm pocket.
    let expected = 40.0 * 30.0 * 10.0 - (4.0 - PI) * 40.0 - 30.0 * 10.0 * 0.75 - PI * 25.0 * (1.5 - 0.75 / 2.0);
    let actual = measure(&cut).unwrap().volume;
    assert!((actual - expected).abs() < 0.02, "{actual} vs {expected}");
    assert!(diagnostics.events.iter().all(|e| e.strategy != BooleanStrategy::RefinedTolerance));
}

#[test]
fn step_seam_cut_recovers_from_empty_numerical_attempts() {
    let pin = cylinder(Vec3::new(-5.0, 0.0, 0.0), Vec3::X, 4.5, 10.0).unwrap();
    let target = step_import(&step_export(&[&pin], "seam regression").unwrap()).unwrap().bodies.remove(0).body;
    let tool = sphere(Vec3::new(-5.0, 0.0, 0.0), 6.0).unwrap();
    assert!(crate::seam_faces::needed(&target, &tool));
    let attempts = Cell::new(0);
    let shifted_attempts = Cell::new(0);
    let empty = |_: &Body, _: &Body, _: BoolOp, shift: Option<mt::Vector3>, _: f64| {
        attempts.set(attempts.get() + 1);
        if shift.is_some() {
            shifted_attempts.set(shifted_attempts.get() + 1);
        }
        Ok((Solid::new_unchecked(Vec::new()), None))
    };
    let (result, diagnostics) =
        capture_boolean_diagnostics(|| boolean_via(&target, &tool, BoolOp::Cut, &|a, b, op| boolean_whole_with(a, b, op, false, &empty)));
    assert!(attempts.get() >= 2, "the initial numerical tries must really have failed");
    assert_eq!(shifted_attempts.get(), 0, "seam recovery must precede displaced retries");
    let cut = result.unwrap().expect("the spherical cut leaves the end of the pin");
    assert!(cut.validity().is_empty(), "{:?}", cut.validity());
    assert_eq!(cut.lumps().unwrap().len(), 1);
    assert!(cut.history().is_some());
    // Along the common axis, integrate min(pi*4.5^2, pi*(6^2-x^2)) from 0 to 6.
    let full_radius_end = (6.0_f64.powi(2) - 4.5_f64.powi(2)).sqrt();
    let removed = PI * (4.5_f64.powi(2) * full_radius_end + 36.0 * (6.0 - full_radius_end) - (216.0 - full_radius_end.powi(3)) / 3.0);
    let expected = PI * 4.5_f64.powi(2) * 10.0 - removed;
    let actual = measure(&cut).unwrap().volume;
    assert!((actual - expected).abs() < expected * 1e-3, "{actual} vs {expected}");
    assert!(diagnostics.warning().is_none(), "{:?}", diagnostics.warning());
}

#[test]
fn unrecovered_empty_attempts_keep_refusal_without_displacing_tool() {
    // These spheres keep fine_tols empty, isolating retries after the exact local strategies.
    let target = sphere(Vec3::ZERO, 10.0).unwrap();
    let tool = sphere(Vec3::new(8.0, 0.0, 0.0), 5.0).unwrap();
    let shifted_attempts = Cell::new(0);
    let empty = |_: &Body, _: &Body, _: BoolOp, shift: Option<mt::Vector3>, _: f64| {
        if shift.is_some() {
            shifted_attempts.set(shifted_attempts.get() + 1);
        }
        Ok((Solid::new_unchecked(Vec::new()), None))
    };
    let result = boolean_via(&target, &tool, BoolOp::Cut, &|a, b, op| boolean_whole_with(a, b, op, false, &empty));
    assert_eq!(shifted_attempts.get(), 0, "unrecovered empty attempts must not enable displaced retries");
    let error = result.unwrap_err();
    assert!(error.to_string().contains("it came out empty"), "{error}");
}
