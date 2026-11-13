//! tangency-labs/solidcore#2621: a modelled thread's groove cut down to a filleted shoulder.
//!
//! SolidCraft cuts an external ISO metric thread into a boss as a screw sweep of the
//! groove's section, run past the plane where the boss's cylinder ends (`z = 21`, where a
//! fillet of radius 1 meets it) and intersected with the half-space above that plane,
//! then subtracted from the body. The fillet's top circle lies in the plane, so the
//! groove's end face meets it there, and each groove flank meets the circle at about 5
//! degrees: along the circle, the point where the flank, the end face and the circle meet
//! is known only to an edge's accuracy over the angle's sine. The imprint split the
//! body's circle where it meets the flank, and the tool's edge between end face and flank
//! where it meets the circle, each at its own intersection's parameter: 2.9e-5 apart for
//! SolidCraft's M16. The merge joins the two vertices at the body's, so the tool edge
//! ended 2.9e-5 from its vertex, over half the vertex's tolerance, and SMLib's check
//! refused the body (`SmBrep` tests #0 and #2, `SmLoopuse` test #5), though the edge's
//! curve passes 2.7e-6 from the vertex. `MergeIntoObject` now splits an edge where its
//! curve passes nearest the classified point (WR-751).
//!
//! The bodies are SolidCraft's, the operands of its Cut (`tests/data/issue_2621_*`): a
//! 60 x 40 x 20 box with a boss of the thread's radius `R` about `(15, 15)` from `z = 20`
//! to `30`, its foot rounded with radius 1. The grooves are SolidCraft's (its
//! `thread_grooves`): the section's root at `d/2 - 5/8 H`, `P/8` wide each side, out to
//! `d/2 + P/20` at 30 degrees a flank, from `0.566 P` below the shoulder plane to `P`
//! above the top, swept to SolidCraft's tolerance `P * 1e-4`, here by solidcore's screw
//! sweep (M16's gives SolidCraft's gap exactly), and trimmed by a box above the shoulder.
//!
//! The volume is checked against the closed form: the box, the boss's cylinder, the
//! fillet's ring (by Pappus: `2 pi` times the first moment about the axis of the unit
//! square at the foot less its quarter disc), less the groove inside the cylinder. Every
//! slice of a screw sweep across its axis is the same, so between the shoulder plane and
//! the top, where the groove covers every slice, that is the height times `2 pi / P`
//! times the first moment `int r dA` of the section's part inside `R` (Pappus again).
//! SolidCraft measures the same cuts, its fallback forced off, within 0.01 mm^3 of it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::AttributeStore;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::sweep::screw::screw_sweep;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::topology_ref;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// SolidCraft's fix list (`tests/kernel_cases/fixes.json`).
fn kernel() -> Kernel {
    let names: Vec<String> = serde_json::from_str(include_str!("kernel_cases/fixes.json")).unwrap();
    let mut f = UpstreamFixes::default();
    for name in &names {
        if let Some(flag) = f.flag_mut(name) {
            *flag = true;
        }
    }
    Kernel {
        fixes: f,
        ..Kernel::default()
    }
}

/// The boss's axis, the shoulder plane (the fillet's top) and the boss's top.
const AXIS: (f64, f64) = (15.0, 15.0);
const SHOULDER: f64 = 21.0;
const TOP: f64 = 30.0;

/// An ISO metric external thread of major diameter `d` and pitch `p`.
struct Thread {
    d: f64,
    p: f64,
}

impl Thread {
    /// SolidCraft's body before the cut, read from its binary `.smb`.
    fn body(&self, k: &mut Kernel) -> Brep {
        let path = format!(
            "{}/tests/data/issue_2621_solidcraft_m{}_filleted_boss.smb",
            env!("CARGO_MANIFEST_DIR"),
            self.d
        );
        let bytes = std::fs::read(path).unwrap();
        let mut attributes = AttributeStore::new();
        let mut records = NotifyRecords::new();
        let mut env = TopologyEnv {
            all_attributes: &[],
            attributes: &mut attributes,
            key_of: &|e| topology_ref(0, e),
            records: &mut records,
            next_key: 1 << 50,
        };
        let mut b = read_brep_from_file(k, Some(bytes), false, false, &mut env).unwrap();
        b.refresh_cached_topology();
        b
    }

    /// The section's corners: (radius, axial offset), root first.
    fn section(&self) -> [(f64, f64); 4] {
        let h = 3f64.sqrt() / 2.0 * self.p;
        let root = self.d / 2.0 - 5.0 / 8.0 * h;
        let outer = self.d / 2.0 + self.p / 20.0;
        let half_root = self.p / 8.0;
        let half_outer = half_root + (outer - root) * 30f64.to_radians().tan();
        [
            (root, -half_root),
            (outer, -half_outer),
            (outer, half_outer),
            (root, half_root),
        ]
    }

    /// The groove: the section in the meridian half-plane `x = 15, y > 15` screw-swept
    /// about the boss's axis, right-handed, from `0.566 P` below the shoulder plane to `P`
    /// above the top, to SolidCraft's tolerance; `phi` turns the half-plane about the axis
    /// (the screw's phase: SolidCraft's is 0).
    fn groove(&self, k: &mut Kernel, phi: f64) -> Brep {
        const BELOW: f64 = 0.566_367_513_459_481_3;
        let base = SHOULDER - BELOW * self.p;
        let turns = (TOP + self.p - base) / self.p;
        let (s, c) = phi.sin_cos();
        let corners: Vec<Point3> = self
            .section()
            .iter()
            .map(|&(r, z)| Point3::new(AXIS.0 - r * s, AXIS.1 + r * c, base + z))
            .collect();
        let profile: Vec<Curve> = (0..corners.len())
            .map(|i| {
                let line = create_line_segment(corners[i], corners[(i + 1) % corners.len()]);
                Curve::from(line.unwrap())
            })
            .collect();
        let origin = Point3::new(AXIS.0, AXIS.1, base);
        let tolerance = (self.p * 1e-4).clamp(1e-6, 1e-2);
        screw_sweep(k, &profile, origin, Vec3::Z, self.p, turns, tolerance).unwrap()
    }

    /// The body less the groove trimmed to the half-space above the shoulder plane.
    fn cut(&self, k: &mut Kernel, phi: f64) -> Brep {
        let body = self.body(k);
        let groove = self.groove(k, phi);
        let above = create_box(
            k,
            Point3::new(AXIS.0 - 50.0, AXIS.1 - 50.0, SHOULDER),
            100.0,
            100.0,
            50.0,
        )
        .unwrap();
        let trimmed = boolean_intersection(k, groove, above).unwrap();
        boolean_difference(k, body, trimmed).unwrap()
    }

    /// The cut's volume in closed form.
    fn cut_volume(&self) -> f64 {
        let r = self.d / 2.0;
        let block = 60.0 * 40.0 * 20.0;
        let cylinder = PI * r * r * (TOP - 20.0);
        // the unit square at the boss's foot, centroid r + 1/2, less the quarter disc
        // about (r + 1, 21), centroid 4 / (3 pi) in from its centre
        let ring = 2.0 * PI * ((r + 0.5) - (r + 1.0 - 4.0 / (3.0 * PI)) * PI / 4.0);
        // the section's half-width w(s) = a + b s from the root out to the radius
        let [(root, half_root), (outer, half_outer), ..] = self.section();
        let (half_root, half_outer) = (-half_root, -half_outer);
        let b = (half_outer - half_root) / (outer - root);
        let a = half_root - b * root;
        // int_root^r s 2 w(s) ds
        let moment = |s: f64| 2.0 * (a * s * s / 2.0 + b * s * s * s / 3.0);
        let groove = (TOP - SHOULDER) * 2.0 * PI / self.p * (moment(r) - moment(root));
        block + cylinder + ring - groove
    }
}

/// The cut passes SMLib's check and has the closed-form volume.
fn cut_is_valid_with_its_volume(t: &Thread) {
    let mut k = kernel();
    let mut cut = t.cut(&mut k, 0.0);
    let lines = check_brep(&mut k, &mut cut, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "M{}: {lines:#?}", t.d);
    let volume = brep_volume_default(&k, &mut cut, 1e-8).unwrap();
    let want = t.cut_volume();
    // The groove's walls lie within the sweep's tolerance P * 1e-4 of the true screw
    // surfaces, over at most 4 (2 pi R / P) mm^2 of wall a millimetre of height.
    let bound = t.p * 1e-4 * 4.0 * 2.0 * PI * t.d / 2.0 / t.p * (TOP - SHOULDER);
    assert!(
        (volume - want).abs() < bound,
        "M{}: volume {volume}, closed form {want}, bound {bound}",
        t.d
    );
}

#[test]
fn issue_2621_m16_groove_cut_down_to_a_filleted_shoulder() {
    cut_is_valid_with_its_volume(&Thread { d: 16.0, p: 2.0 });
}

#[test]
fn issue_2621_m10_m12_m20_grooves_cut_down_to_a_filleted_shoulder() {
    for (d, p) in [(10.0, 1.5), (12.0, 1.75), (20.0, 2.5)] {
        cut_is_valid_with_its_volume(&Thread { d, p });
    }
}

/// SMLib's check of the M16 cut with the groove at phase `i / 32` of a turn.
fn m16_phase_check(i: u32) -> Vec<String> {
    let t = Thread { d: 16.0, p: 2.0 };
    let mut k = kernel();
    let mut cut = t.cut(&mut k, 2.0 * PI * f64::from(i) / 32.0);
    check_brep(&mut k, &mut cut, CheckLevel::Level2)
        .unwrap()
        .lines()
}

/// The M16 groove at every 32nd of a turn of phase: where the flank meets the fillet's
/// circle moves along it, and the vertex there lies inside the tool edge's span (phases
/// 0 to 8 and 24 to 31) or past its end (9 to 23). Every cut passes SMLib's check.
/// Splitting each edge at its own intersection's parameter, 30 of the 32 were refused
/// with gaps from 2.15e-5 to 3.06e-5 (only 3 and 27 passed). An optimised build runs all
/// 32; a debug build every fourth (0, 4, ..., 28: both kinds of vertex, all refused before),
/// to keep the test's time down.
#[test]
fn issue_2621_m16_groove_at_every_phase_passes_the_check() {
    let step = if cfg!(debug_assertions) { 4 } else { 1 };
    let refused: Vec<(u32, Vec<String>)> = (0..32)
        .step_by(step)
        .map(|i| (i, m16_phase_check(i)))
        .filter(|(_, lines)| !lines.is_empty())
        .collect();
    assert!(refused.is_empty(), "{refused:#?}");
}
