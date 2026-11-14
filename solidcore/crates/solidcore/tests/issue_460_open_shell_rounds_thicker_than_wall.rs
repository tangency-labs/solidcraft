//! tangency-labs/solidcore#460: a 30 x 20 x 10 box with all twelve edges rounded r3,
//! shelled 2 inward, closed and with the top face open. At the c9c5979 pin the shell
//! failed in the offset's piecewise merge at the corner patches, in the Rust port every
//! time and in the C++ depending on heap layout (WR-347, ND-261, UB-261); usd-brep v1.0.1
//! builds it in every layout, and the port follows v1.0.1. The open-top shell stays a
//! probe (`--ignored`): v1.0.1 returns success for it, but whether that body is a valid
//! open-top shell is not settled (the SMLib fork's fails the level-2 check).
//!
//! The rounded box is the core 24 x 14 x 4 grown by 3; its closed shell's cavity is
//! the same core grown by 1. Open at the top, the cavity also takes the 24 x 14 x 2
//! slab between the cavity's flat top (z = 8) and the removed top face (z = 10).
//!
//! tangency-labs/solidcraft#15108 is the same family at r5 on a 40 x 30 x 20 box: its open
//! shell failed only with the box centred on `y = 0` (the ball extension's farthest-point
//! angle, `surface::extend::ball`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use core::f64::consts::PI;

use offset_fixture::{api_shell, planar_faces_at, report, shell, volume};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

/// The core `a x b x c` grown by `r` (Minkowski sum with a ball).
fn grown_box(a: f64, b: f64, c: f64, r: f64) -> f64 {
    a * b * c
        + 2.0 * r * (a * b + b * c + a * c)
        + PI * r * r * (a + b + c)
        + 4.0 / 3.0 * PI * r * r * r
}

fn rounded_box(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let edges: Vec<_> = b.edges().ids().collect();
    fillet_edges(k, &mut b, &edges, 3.0, 1, 1, 1.0).unwrap();
    b
}

#[test]
fn issue_460_rounded_box_input() {
    let mut k = Kernel::default();
    let mut b = rounded_box(&mut k);
    let want = grown_box(24.0, 14.0, 4.0, 3.0);
    let got = volume(&k, &mut b).unwrap();
    eprintln!(
        "PROBE issue_460 input: faces={} volume={got} want={want}",
        b.face_list().len()
    );
    assert!((got - want).abs() <= 1e-5 * want, "{got} vs {want}");
}

#[test]
fn issue_460_closed_shell() {
    let mut k = Kernel::default();
    let b = rounded_box(&mut k);
    let want = grown_box(24.0, 14.0, 4.0, 3.0) - grown_box(24.0, 14.0, 4.0, 1.0);
    let result = shell(&mut k, b, -2.0, &[]);
    assert!(report(&k, "issue_460 closed", result, Some(want), 1e-5));
}

#[test]
#[ignore = "probe: the open-top shell's validity is not settled (see the module notes)"]
fn issue_460_open_top_shell() {
    let mut k = Kernel::default();
    let b = rounded_box(&mut k);
    let top = planar_faces_at(&k, &b, 2, 10.0);
    assert_eq!(top.len(), 1, "the top face");
    let want =
        grown_box(24.0, 14.0, 4.0, 3.0) - grown_box(24.0, 14.0, 4.0, 1.0) - 24.0 * 14.0 * 2.0;
    let result = shell(&mut k, b, -2.0, &top);
    assert!(report(&k, "issue_460 open top", result, Some(want), 1e-5));
}

/// Narrows the closed-shell failure: the box with one edge, the four vertical edges,
/// the four top edges and all twelve edges rounded r3, each shelled closed 2 inward
/// (reported only).
#[test]
#[ignore = "diagnostic probe, run on demand"]
fn issue_460_closed_shell_by_round_set() {
    for set in [
        "one vertical edge",
        "four vertical edges",
        "four top edges",
        "all twelve edges",
    ] {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
        let edges: Vec<_> = b
            .edges()
            .iter()
            .filter(|(_, e)| {
                let c = e.curve.as_ref().unwrap();
                let p = c.evaluate_point(&k, e.interval.min()).unwrap();
                let q = c.evaluate_point(&k, e.interval.max()).unwrap();
                let vertical = (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9;
                let top = (p.z - 10.0).abs() < 1e-9 && (q.z - 10.0).abs() < 1e-9;
                match set {
                    "one vertical edge" => vertical && p.x.abs() < 1e-9 && p.y.abs() < 1e-9,
                    "four vertical edges" => vertical,
                    "four top edges" => top,
                    _ => true,
                }
            })
            .map(|(id, _)| id)
            .collect();
        let rounded = fillet_edges(&mut k, &mut b, &edges, 3.0, 1, 1, 1.0);
        eprintln!(
            "PROBE issue_460 {set}: {} edges rounded: {rounded:?}",
            edges.len()
        );
        if rounded.is_err() {
            continue;
        }
        let result = shell(&mut k, b, -2.0, &[]);
        let _ = report(&k, &format!("issue_460 closed, {set}"), result, None, 0.0);
    }
}

/// tangency-labs/solidcraft#15108: a 40 x 30 x 20 box centred at `c`, all twelve edges
/// rounded r5 and shelled 2 inward with the top open, with the options SolidCraft's
/// `smlib_shell` passes and SolidCraft's fix set: every opt-in fix but the two its backend
/// holds back (`fixes()` less `HELD_BACK` in SolidCraft's `crates/smlib/src/core.rs`;
/// mirrored here, as solidcore cannot depend on the app). Returns the shell's volume.
fn shell_15108(c: [f64; 3]) -> f64 {
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_refuse_rounds_breaking_through = false;
    fixes.fillet_round_ends_on_outline = false;
    let mut k = Kernel {
        fixes,
        ..Kernel::default()
    };
    let corner = Point3::new(c[0] - 20.0, c[1] - 15.0, c[2] - 10.0);
    let mut b = create_box(&mut k, corner, 40.0, 30.0, 20.0).unwrap();
    let edges: Vec<_> = b.edges().ids().collect();
    fillet_edges(&mut k, &mut b, &edges, 5.0, 1, 1, 1.0).unwrap();
    let top = planar_faces_at(&k, &b, 2, c[2] + 10.0);
    assert_eq!(top.len(), 1, "the top face");
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: -2.0,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    };
    let mut r = api_shell(&mut k, &mut b, options, &top)
        .unwrap_or_else(|e| panic!("centre {c:?}: the shell fails: {e:?}"));
    solidcore::topology::integrity::check(&r).unwrap();
    assert!(brep_is_manifold_solid(&r).unwrap(), "centre {c:?}");
    // the check SolidCraft runs on every result (`CheckLevel::All`)
    let check = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
    assert!(
        check.is_valid(),
        "centre {c:?}: {:?}",
        check.failures().collect::<Vec<_>>()
    );
    volume(&k, &mut r).unwrap()
}

/// #15108: the shell builds wherever the box sits. Centred on `y = 0` its corner balls'
/// poles are normal to their third sides' planes, and the ball extension took the
/// farthest point of that side from a direction that was all rounding (a half turn,
/// refused as "not a triangle under a half turn"); shifted, it built. Each position gives
/// the closed form: the r5 rounded box less its cavity, the core 30 x 20 x 10 grown by 3,
/// less the 30 x 20 x 2 slab the open top adds to it (to the volume integrator's 1e-6),
/// and the two positions agree far tighter.
#[test]
fn issue_15108_open_shell_of_a_rounded_box_is_independent_of_position() {
    let want =
        grown_box(30.0, 20.0, 10.0, 5.0) - grown_box(30.0, 20.0, 10.0, 3.0) - 30.0 * 20.0 * 2.0;
    let shifted = shell_15108([20.0, 15.0, 10.0]);
    for c in [
        [0.0, 0.0, 0.0],
        [20.0, 0.0, 10.0],
        [0.1, 0.0, 0.0],
        [20.0, 15.0, 10.0],
    ] {
        let got = shell_15108(c);
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "centre {c:?}: {got} vs {want}"
        );
        assert!(
            (got - shifted).abs() <= 1e-9 * shifted,
            "centre {c:?}: {got} vs {shifted} at [20, 15, 10]"
        );
    }
}
