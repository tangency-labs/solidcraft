//! Shared by the test programs: `mod common;` at the top of a test file.

#![allow(dead_code)]

use crate::error::teeth::{TeethFailure, Why};
use crate::teeth::outline::rotated;
use crate::teeth::{Gaps, Outline, Pair, Piece, Point};
use crate::SolidError;

/// The distance between two points.
pub fn dist(a: Point, b: Point) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// An outline's own promise, checked from outside: the pieces of the tooth join, the tooth closes
/// onto the next one, the points are finite, and the whole runs counter-clockwise around an area.
pub fn check_outline(name: &str, outline: &Outline) {
    let tooth = outline.tooth();
    for pair in tooth.windows(2) {
        let (end, start) = (
            pair[0].end().expect("an end"),
            pair[1].start().expect("a start"),
        );
        assert!(
            dist(end, start) < 1e-9,
            "{name}: pieces {:?} and {:?} do not join",
            pair[0].role(),
            pair[1].role()
        );
    }
    let first = tooth.first().and_then(Piece::start).expect("a first point");
    let last = tooth.last().and_then(Piece::end).expect("a last point");
    assert!(
        dist(last, rotated(first, outline.pitch_angle())) < 1e-9,
        "{name}: the tooth does not close onto the next"
    );
    let points = outline.points();
    assert!(
        points.len() >= 3 && points.iter().flatten().all(|v| v.is_finite()),
        "{name}: points"
    );
    let doubled: f64 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum();
    assert!(
        doubled > 0.0,
        "{name}: the outline does not run counter-clockwise"
    );
}

/// The smallest gap found while a pair was turned, as a number (the parts must have come within
/// reach).
pub fn least(gaps: &Gaps) -> f64 {
    gaps.least
        .expect("the parts never came within reach of each other")
        .distance
}

/// The largest of the steps' smallest gaps (the parts must have been within reach at every step).
pub fn most(gaps: &Gaps) -> f64 {
    gaps.most
        .expect("at some step the parts were not within reach of each other")
}

/// Asserts that a pair keeps the gap `expected` at every one of `steps` steps of a cycle, within
/// `within`.
pub fn assert_gap(
    name: &str,
    pair: &Pair,
    steps: u32,
    reach: f64,
    expected: f64,
    within: f64,
) -> crate::Result<()> {
    let gaps = pair.through_a_cycle(steps, reach)?;
    let (least, most) = (least(&gaps), most(&gaps));
    assert!(
        (least - expected).abs() < within,
        "{name}: least gap {least}, expected {expected} (at turn {})",
        gaps.least_at
    );
    assert!(
        (most - expected).abs() < within,
        "{name}: largest gap {most}, expected {expected}"
    );
    Ok(())
}

/// The angle between two directions, 0 to pi.
pub fn angle_between(a: Point, b: Point) -> f64 {
    (a[0] * b[1] - a[1] * b[0])
        .atan2(a[0] * b[0] + a[1] * b[1])
        .abs()
}

/// A small generator of the same numbers every run, for sweeps.
pub struct Dice(pub u64);

impl Dice {
    pub fn roll(&mut self) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize
    }
    pub fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[self.roll() % from.len()]
    }
}

/// Numbers no function may panic on.
pub const HOSTILE: [f64; 30] = [
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::MAX,
    f64::MIN,
    f64::MIN_POSITIVE,
    f64::EPSILON,
    0.0,
    -0.0,
    1e-300,
    1e-12,
    1e-6,
    0.01,
    0.25,
    0.3,
    0.5,
    0.85,
    1.0,
    1.25,
    2.0,
    3.0,
    7.0,
    20.0,
    30.0,
    45.0,
    1e3,
    1e9,
    1e300,
    -1.0,
    -1e-9,
];

/// Counts no function may panic on.
pub const HOSTILE_COUNTS: [u32; 12] = [0, 1, 2, 3, 4, 5, 7, 20, 100, 2000, 2001, u32::MAX];

/// What every accepted outline must be; whether one was accepted.
pub fn sound(outline: crate::Result<Outline>) -> bool {
    let Ok(outline) = outline else { return false };
    for piece in outline.tooth() {
        let (start, end) = (
            piece.start().expect("a start"),
            piece.end().expect("an end"),
        );
        assert!(start.iter().chain(&end).all(|v| v.is_finite()), "{piece:?}");
    }
    let points = outline.points();
    assert!(points.len() >= 3 && points.len() <= crate::teeth::MAX_OUTLINE_POINTS);
    assert!(points.iter().flatten().all(|v| v.is_finite()));
    let (low, high) = outline.radii();
    assert!(low.is_finite() && high.is_finite() && low <= high);
    true
}

/// The tooth failure of a refused public call (None for Ok or for another kind of error).
pub fn failure<T>(r: &crate::Result<T>) -> Option<TeethFailure> {
    r.as_ref()
        .err()
        .and_then(SolidError::teeth_failure)
        .copied()
}

/// The reason of a refused public call, for an Invalid.
pub fn why<T>(r: &crate::Result<T>) -> Option<Why> {
    failure(r).as_ref().and_then(TeethFailure::why)
}

/// The `what` of an `OutOfRange` refusal.
pub fn out_of_range<T>(r: &crate::Result<T>) -> Option<&'static str> {
    match failure(r) {
        Some(TeethFailure::OutOfRange { what, .. }) => Some(what),
        _ => None,
    }
}

/// The `what` of a `NotProved` refusal.
pub fn not_proved<T>(r: &crate::Result<T>) -> Option<&'static str> {
    match failure(r) {
        Some(TeethFailure::NotProved { what, .. }) => Some(what),
        _ => None,
    }
}

/// The operation path of a refused public call.
pub fn operation<T>(r: &crate::Result<T>) -> Option<&'static str> {
    r.as_ref().err().map(SolidError::operation)
}

/// A crate-private result as a public one, for `?` in tests: operation `"teeth::tests"`.
pub fn lift<T>(r: Result<T, TeethFailure>) -> crate::Result<T> {
    r.map_err(|e| SolidError::teeth("teeth::tests", e))
}

/// The reason of a crate-private result.
pub fn typed_why<T>(r: &Result<T, TeethFailure>) -> Option<Why> {
    r.as_ref().err().and_then(TeethFailure::why)
}
