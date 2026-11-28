//! Input is hostile: every kind of set, planned with each number in its spec set to a value a
//! person (or a file) could give it, returns a plan or an error, never panics, and answers quickly
//! (the whole sweep is a few thousand plans).

use crate::{Kind, Spec, plan};
use std::panic::{AssertUnwindSafe, catch_unwind};

type SetF = (&'static str, fn(&mut Spec, f64));
type SetU = (&'static str, fn(&mut Spec, u32));

macro_rules! fields {
    ($t:ty; $($($f:ident).+),* $(,)?) => {
        &[$((stringify!($($f).+), |s: &mut Spec, v: $t| s.$($f).+ = v)),*]
    };
}

const REALS: &[SetF] = fields!(f64;
    module, pressure_angle, helix, shift, shift2, backlash, face_width, center_distance,
    shaft_angle, spiral_angle, offset, worm_diameter, worm_length, rim, rack_height, pin_circle,
    pin_diameter, eccentricity, hub_diameter, hub_length, hole_diameter, chain_pitch,
    roller_diameter, roller_width, strand_pitch, spline_diameter, inner_diameter,
    ellipse_eccentricity, tip_round, xy_compensation, elephant_foot, elephant_foot_height, nozzle,
    clearance, tip_shortening, tip_diameter, tip_diameter2, tip_relief, tip_relief_height,
    crowning, face_width2, face_offset2, bore_chamfer, hub_chamfer, first_azimuth,
    bore.diameter, bore.key_width, bore.key_depth, bore2.diameter,
);

const COUNTS: &[SetU] = fields!(u32;
    teeth, teeth2, teeth3, planets, starts, rack_teeth, pins, holes, strands, coupon_teeth,
);

const BAD_REALS: &[f64] = &[f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 1e-12, 1e9];
/// Large enough to be absurd, small enough that a refusal is the only quick answer.
const BAD_COUNTS: &[u32] = &[0, 1, 2, 100_000, u32::MAX];

fn panics(s: &Spec) -> Option<String> {
    let r = catch_unwind(AssertUnwindSafe(|| {
        let _ = plan(s);
    }));
    r.err().map(|e| e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|m| (*m).to_string())).unwrap_or_default())
}

/// One hostile value: the field it goes in and the value, as words, and how to set it.
type Case = (String, Box<dyn Fn(&mut Spec) + Sync>);

fn cases() -> Vec<Case> {
    let reals = REALS.iter().flat_map(|&(name, set)| {
        BAD_REALS.iter().map(move |&v| (format!("{name} = {v}"), Box::new(move |s: &mut Spec| set(s, v)) as Box<dyn Fn(&mut Spec) + Sync>))
    });
    let counts = COUNTS.iter().flat_map(|&(name, set)| {
        BAD_COUNTS.iter().map(move |&v| (format!("{name} = {v}"), Box::new(move |s: &mut Spec| set(s, v)) as Box<dyn Fn(&mut Spec) + Sync>))
    });
    reals.chain(counts).collect()
}

#[test]
fn hostile_spec_values_never_panic() {
    let cases = cases();
    let bases: Vec<Spec> = Kind::ALL.iter().map(|&k| Spec::defaults_for(k)).collect();
    // Every kind with every case, handed to the crate's own threads one plan at a time (the
    // differential's plans are the slow ones).
    let n = cases.len();
    let failed: Vec<String> = crate::par::map(bases.len() * n, |i| {
        let (base, (what, set)) = (bases.get(i / n)?, cases.get(i % n)?);
        let mut s = base.clone();
        set(&mut s);
        panics(&s).map(|m| format!("{:?} {what}: {m}", s.kind))
    })
    .into_iter()
    .flatten()
    .collect();
    assert!(
        failed.is_empty(),
        "{} panics:
{}",
        failed.len(),
        failed.join(
            "
"
        )
    );
}
