//! SolidCraft's `fillet_the_edges_between_two_overlapping_cylinders` (#13538 there): a profile of
//! two crossing circles' outer arcs, extruded. The two straight edges where the walls cross are
//! concave; a round of one is the cylinder of the ball that touches both walls beside that edge.
//! Each rail's end is where the rail meets a cap edge, and the faces hold a second ball there:
//! the round of the other crossing, whose contacts lie on the same two faces. With the offsets
//! continued as themselves (whole circles), the R 13 round's rail end on the small wall was
//! solved onto that other ball, and the cap's intersection with the round, which runs between
//! the two rail ends, was refused (status 1001). The rail end is now continued from the round's
//! own section at the corner.
//!
//! The volume's closed form: the round fills the plan area between its contacts and the corner,
//! less the three circular segments, all along the extrusion's height.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)] // test

mod common;

use common::edge_at;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::{session, Body, CheckLevel};

const C1: [f64; 2] = [0.0, 0.0];
const R1: f64 = 17.140595088852663;
const C2: [f64; 2] = [10.715766135516247, 13.378055020402751];
const R2: f64 = 9.666944358807896;
/// The crossings: `P` above the line of centres, `Q` below it.
const P: [f64; 2] = [1.7728134670876865, 17.048669520256194];
const Q: [f64; 2] = [16.250324167975673, 5.452243981674495];
const HEIGHT: f64 = 10.0;

/// Resets the process-wide fixes when the test ends, a failed assertion included.
struct ResetFixes;

impl Drop for ResetFixes {
    fn drop(&mut self) {
        session::set_fixes([]).unwrap();
    }
}

fn angle(c: [f64; 2], p: [f64; 2]) -> f64 {
    (p[1] - c[1]).atan2(p[0] - c[0])
}

/// The profile, counter-clockwise: the big circle's arc outside the small one from `P` to `Q`,
/// then the small circle's from `Q` to `P`; extruded `HEIGHT` along +z.
fn body() -> Body {
    let arc = |c: [f64; 2], r: f64, from: [f64; 2], to: [f64; 2]| {
        let start = angle(c, from);
        let mut end = angle(c, to);
        if end <= start {
            end += 2.0 * std::f64::consts::PI;
        }
        Curve::Arc {
            center: [c[0], c[1], 0.0],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: r,
            start,
            end,
        }
    };
    let profile = vec![arc(C1, R1, P, Q), arc(C2, R2, Q, P)];
    extrude(
        &profile,
        [0.0, 0.0, 1.0],
        HEIGHT,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap()
}

/// The profile's area: the two circular segments on either side of the chord `PQ`, each the
/// sector less (or, past a half turn, with) the triangle on the chord.
fn profile_area() -> f64 {
    let segment = |c: [f64; 2], r: f64, from: [f64; 2], to: [f64; 2]| {
        let mut turn = angle(c, to) - angle(c, from);
        if turn <= 0.0 {
            turn += 2.0 * std::f64::consts::PI;
        }
        r * r * (turn - turn.sin()) / 2.0
    };
    segment(C1, R1, P, Q) + segment(C2, R2, Q, P)
}

/// The plan area a round of radius `r` fills at the concave corner `x`: the triangle of its
/// contacts and the corner, less the three circular segments (as SolidCraft's test).
fn round_gap(x: [f64; 2], r: f64) -> f64 {
    let (a, b) = (R1 + r, R2 + r);
    let d = (C2[0] - C1[0]).hypot(C2[1] - C1[1]);
    let u = [(C2[0] - C1[0]) / d, (C2[1] - C1[1]) / d];
    let along = (a * a - b * b + d * d) / (2.0 * d);
    let side = if (x[0] - C1[0]) * -u[1] + (x[1] - C1[1]) * u[0] < 0.0 {
        -1.0
    } else {
        1.0
    };
    let off = (a * a - along * along).sqrt() * side;
    let f = [
        C1[0] + u[0] * along - u[1] * off,
        C1[1] + u[1] * along + u[0] * off,
    ];
    let t1 = [
        C1[0] + (f[0] - C1[0]) * R1 / a,
        C1[1] + (f[1] - C1[1]) * R1 / a,
    ];
    let t2 = [
        C2[0] + (f[0] - C2[0]) * R2 / b,
        C2[1] + (f[1] - C2[1]) * R2 / b,
    ];
    let between = |c: [f64; 2], p: [f64; 2], q: [f64; 2]| {
        let (v, w) = ([p[0] - c[0], p[1] - c[1]], [q[0] - c[0], q[1] - c[1]]);
        ((v[0] * w[0] + v[1] * w[1]) / (v[0].hypot(v[1]) * w[0].hypot(w[1])))
            .clamp(-1.0, 1.0)
            .acos()
    };
    let segment = |r: f64, t: f64| r * r * (t - t.sin()) / 2.0;
    let triangle =
        ((x[0] - t1[0]) * (t2[1] - t1[1]) - (t2[0] - t1[0]) * (x[1] - t1[1])).abs() / 2.0;
    triangle
        - segment(R1, between(C1, t1, x))
        - segment(R2, between(C2, x, t2))
        - segment(r, between(f, t2, t1))
}

fn check(b: &Body, expected: f64, what: &str) {
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{what}: {report}");
    let v = b.volume(1e-9).unwrap();
    assert!(
        (v - expected).abs() < 1e-6 * expected,
        "{what}: volume {v}, closed form {expected}"
    );
}

/// Each corner alone at R 6, both at once, and the R 13 round of `Q`, whose rails end far
/// along both walls; with the offsets continued as themselves, as SolidCraft runs them. One
/// test: the fixes are one process-wide setting.
#[test]
fn a_round_between_crossing_cylinders_is_the_ball_of_its_own_crossing() {
    let _reset = ResetFixes;
    session::set_fixes([
        "linear_sweep_arc_is_cylinder",
        "offset_extension_from_base",
        "offset_extension_as_itself",
    ])
    .unwrap();
    let area = profile_area();
    check(&body(), area * HEIGHT, "the extrusion");
    let at = |b: &Body, x: [f64; 2]| edge_at(b, [x[0], x[1], HEIGHT / 2.0]);
    for (x, r) in [(P, 6.0), (Q, 6.0), (Q, 13.0)] {
        let b = body();
        let e = at(&b, x);
        let what = format!("R {r} at ({:.3}, {:.3})", x[0], x[1]);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
            .unwrap_or_else(|err| panic!("{what}: {err:?}"));
        check(&b, (area + round_gap(x, r)) * HEIGHT, &what);
    }
    let b = body();
    let edges = [at(&b, P), at(&b, Q)];
    blend(&b, Blend::Round, &edges, &[6.0, 6.0], &[], Corners::Filled)
        .unwrap_or_else(|err| panic!("both R 6: {err:?}"));
    check(
        &b,
        (area + round_gap(P, 6.0) + round_gap(Q, 6.0)) * HEIGHT,
        "both R 6",
    );
}
