//! #2845: rounds on a stepped wall. The union of a 4 mm wall on a 7 mm base, so the base stands
//! 3 mm proud of the wall as a ledge. The wall's edge and the ledge's both end on the back face
//! y = 20, an L, and share no vertex. Their rails run on past the edges' ends until they are
//! trimmed, and the crossing check read that run-on as a rail inside the other round's strip.
//! Its own test binary: the fix set is the process kernel's.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)] // test: the closed form uses pi

mod common;

use common::{all_fixes, built, edge_at, kernel_copy, volume};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::kernel::Kernel;
use solidcore_ops::boolean::boolean;
use solidcore_ops::{blend, make_box, session, Blend, Body, BooleanOp, CheckLevel, Corners};
use solidcore_record::brep::Brep as Record;
use std::io::Read as _;

const WALL: [f64; 3] = [20.0, 20.0, -19.0];
const LEDGE: [f64; 3] = [23.0, 20.0, -45.0];

fn stepped_wall() -> Body {
    let wall = make_box([16.0, -20.0, -40.0], [4.0, 40.0, 42.0]).unwrap();
    let base = make_box([16.0, -20.0, -50.0], [7.0, 40.0, 10.0]).unwrap();
    boolean(wall, base, BooleanOp::Union).unwrap().unwrap()
}

/// The two rounds are apart, each between faces square to it: each takes (1 - pi/4) r^2 per
/// length, over 42 + 10 mm.
fn closed_form(r: f64) -> f64 {
    9520.0 - (1.0 - std::f64::consts::FRAC_PI_4) * r * r * 52.0
}

#[test]
fn both_rounds_that_fit_build_their_closed_form() {
    all_fixes();
    for r in [2.5, 3.0] {
        let b = stepped_wall();
        let edges = [edge_at(&b, WALL), edge_at(&b, LEDGE)];
        blend(&b, Blend::Round, &edges, &[r], &[], Corners::Filled).unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        let v = volume(&b);
        assert!((v - closed_form(r)).abs() <= 1e-9 * v, "r {r}: {v}");
    }
}

/// Past the wall's 4 mm the ball rolls on the face x = 20 and on the wall's far edge (x = 16,
/// y = 20): the arc runs from that edge to its rail, and the section it takes off the wall is
/// the strip less the disc's part under the rail. With every fix on: SolidCraft's set fails here
/// with a bare 1001 (the case `12342-round-wall-edge-r5-past-the-wall-refused`).
fn past_the_wall(r: f64) -> f64 {
    let near = r - 4.0;
    let height = (r * r - near * near).sqrt();
    let under = |u: f64| u / 2.0 * (r * r - u * u).max(0.0).sqrt() + r * r / 2.0 * (u / r).asin();
    9520.0 - 42.0 * (4.0 * height - (under(r) - under(near)))
}

#[test]
fn the_wall_alone_rides_its_far_edge_past_its_width() {
    all_fixes();
    for r in [4.5, 5.0, 6.0] {
        let b = stepped_wall();
        let e = edge_at(&b, WALL);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        let v = volume(&b);
        assert!((v - past_the_wall(r)).abs() <= 1e-9 * v, "r {r}: {v}");
    }
}

#[test]
fn the_same_on_the_default_kernel() {
    for r in [2.5, 3.0] {
        let body = stepped_wall();
        let edges = [edge_at(&body, WALL), edge_at(&body, LEDGE)];
        let (_, mut b) = kernel_copy(&body);
        let ids = edges.map(|i| b.edge_list()[i]);
        fillet_edges(&mut Kernel::default(), &mut b, &ids, r, 1, 1, 1.0).unwrap();
        let v = volume(&built(|_| Ok(b)));
        assert!((v - closed_form(r)).abs() <= 1e-9 * v, "r {r}: {v}");
    }
}

/// The case's record body (`fillets/12342-*`): the same L, each face's plane bounded to its
/// face (x 16..23 for the back face), as SolidCraft writes it; on the kernel rig's fix set.
fn recorded_stepped_wall() -> Body {
    let fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
    let path = format!(
        "{}/../solidcore/tests/kernel_cases/fillets/12342-round-wall-edge-r5-past-the-wall-refused.bodies.json.gz",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b1"].clone()).unwrap();
    Body::from_record(&record, true).unwrap().0
}

/// The overflow trimmed at its crease (docs/generic-blend.md section 3.4), without the
/// outline's opt-in fix: the round on the continued faces, centred on x = y = 20 - r, is
/// trimmed at its crease with the wall's inner face x = 16. Each section z in (-40, 2) loses
/// the square [16, 20] x [20 - r, 20] less the disc's part in it,
/// A(r) = 4 r - int_{r - 4}^{r} sqrt(r^2 - t^2) dt, over the wall's 42.
fn trimmed_at_its_crease(r: f64) -> f64 {
    let under = |u: f64| u / 2.0 * (r * r - u * u).max(0.0).sqrt() + r * r / 2.0 * (u / r).asin();
    9520.0 - 42.0 * (4.0 * r - (under(r) - under(r - 4.0)))
}

/// Past the wall's width on the record body: the round's End on the ledge's top caps along
/// the back face's bottom edge, past that edge's end to the rail at x = 20 - r. That cap edge
/// is where the ledge top's extension meets the back face continued as itself. The back
/// face's plane, bounded to its face, did not reach x < 16, so the intersection failed and
/// the round failed with status 1001.
#[test]
fn the_recorded_wall_s_round_past_the_wall_overflows() {
    for r in [4.5, 5.0, 6.0] {
        let b = recorded_stepped_wall();
        let e = edge_at(&b, WALL);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
            .unwrap_or_else(|e| panic!("r {r}: {e}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "r {r}: {report}");
        let v = volume(&b);
        assert!(
            (v - trimmed_at_its_crease(r)).abs() <= 1e-9 * v,
            "r {r}: {v}"
        );
    }
}
