//! The #2252 pipe-on-plate G2 round (r 2, order 2, w 0) by WR-685's G^n section on
//! SolidCraft's flag profile (tangency-labs/solidcraft#8526
//! at c494a18, `crates/smlib/src/core.rs` `fixes()`, 292 flags), reported on #2260.
//!
//! With `query_statuses_pass_on` (tangency-labs/smlib 8311f8e) `sm_RefineCCSolutions`
//! passed on `Inversion`'s `SM_ERR` for a curve/curve range solution's end refined
//! 2e-15 past the range (12.149466823226815 against the range's max 12.149466823226813,
//! while the round's insertion classifies a curve on a face), so the fillet failed with
//! 1001. The body the boolean builds with `keep_operand_uv_trims` and the offset extension
//! flags is what puts an end there. The fork's later 8ea6a20 takes such a parameter as
//! the answer it is and leaves an end alone only for a range with no fraction; solidcore
//! now does that under the same flag (#2264).
//!
//! Expected volume: the tangency-labs/smlib 729f893 build run on the same construction by
//! `SmApiBrepComputeVolume`, 40267.709270590589 at accuracy 1e-7 (3bc54a4 and c596548
//! give the same), checked to the 1e-7 accuracy the volume is integrated to.
//!
//! The round is the plate's and the pipe's section swept along x, so its volume is the
//! union's, `24000 + 60 (100 pi - (100 acos 0.6 - 48)) = 40165.784613529`, plus 60 times the
//! section's area. The closed form G^n section (n 2, w 0, ball 2; as `gn_section` in
//! `fork_ea59e9c_section_orders.rs`), integrated over its plane, gives:
//! - with the jets since smlib#79's 3bc54a4 (WR-685: the order-n jet is the normal part,
//!   0 on the plate and `-|c1|^2 / (2 R)` radially on the pipe), area 1.698743952036 and
//!   volume 40267.7092506513, which the fork's value matches to 5e-10 and solidcore's to
//!   3e-9;
//! - with the feet's Taylor jets before it, area 1.545983045903 and volume 40258.5435962833.
//!   That was the d01d7b0 build's 40258.543608517233, the value this test used to expect.
//!
//! Both sections run from the ball's feet (31.3137, 10) to (29.4281, 12.6667) and meet
//! the plate and the pipe G2 (end curvatures 0 and 1/R); they differ by a tangential
//! term at order 2.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::Kernel;

/// The flags SolidCraft's `fixes()` turns on at tangency-labs/solidcraft c494a18.
const SOLIDCRAFT_C494A18: &[&str] = &[
    "analytic_copy_keeps_parameterization",
    "classify_edge_interval",
    "curve_contacts_at_curve_ends",
    "curve_loop_normal_from_area",
    "face_tight_box_searches_trim",
    "refine_cc_range_ends",
    "simple_coincidence_crossing_is_point",
];

/// The volume 729f893 gives the rounded body at accuracy 1e-7.
const FORK_VOLUME: f64 = 40_267.709_270_590_589;

fn kernel(names: &[&str]) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::default();
    for name in names {
        *k.fixes
            .flag_mut(name)
            .unwrap_or_else(|| panic!("no flag {name}")) = true;
    }
    k
}

/// Unites the pipe and the plate, rounds the root line at (30, 28, 10) and returns the
/// result with the body.
fn round_pipe_root(k: &mut Kernel) -> (solidcore::Result<()>, solidcore::Brep) {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let mut pipe = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0).unwrap();
    let pl = Axis2Placement::new(
        Point3::new(0.0, 20.0, 16.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    solidcore::topology::brep::p5::transform(k, &mut pipe, &pl, None, false).unwrap();
    let mut body = boolean_union(k, plate, pipe).unwrap();
    let q = Point3::new(30.0, 28.0, 10.0);
    let mut best = (0.1_f64, None);
    for &e in body.edge_list() {
        if let Ok(c) = edge_closest_point(k, &body, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    let e = best.1.expect("root edge");
    let r = fillet_edges(k, &mut body, &[e], 2.0, 2, 2, 0.0);
    (r, body)
}

fn check_round(names: &[&str]) {
    let mut k = kernel(names);
    let (r, mut body) = round_pipe_root(&mut k);
    assert!(r.is_ok(), "pipe G2 round failed: {r:?}");
    let report = check_brep(&mut k, &mut body, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(report.is_empty(), "{report:?}");
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut body, 1e-7, &attrs).unwrap();
    eprintln!("pipe root volume {v:.17e}");
    assert!(
        (v - FORK_VOLUME).abs() <= 1e-7 * FORK_VOLUME,
        "{v} want {FORK_VOLUME}"
    );
}

/// SolidCraft's whole profile plus the G^n section: the round completes with the fork's
/// volume.
#[test]
fn solidcraft_profile_pipe_gn_round_completes() {
    check_round(SOLIDCRAFT_C494A18);
}

/// The four flags the failure needed together (`keep_operand_uv_trims`,
/// `query_statuses_pass_on`, `offset_extension_from_base`, `offset_extension_as_itself`).
#[test]
fn reduced_profile_pipe_gn_round_completes() {
    check_round(&[]);
}

/// Without `query_statuses_pass_on` the status is dropped, as upstream, and the round
/// completes with the same volume.
#[test]
fn without_pass_on_pipe_gn_round_completes() {
    check_round(&[]);
}
