//! The sweeps against their closed forms (moved from SolidCraft's `crates/smlib` sweep tests):
//! each case's volume (relative 1e-9, or within the tolerance its walls are approximated to),
//! a refused draft corner's reason, a planar sheet's one face. Then the arcs of a profile on one
//! circle: the queries and the join an application's one-wall rule is built on, and their
//! refusals. They run under the rig's fix set
//! (the corpus's `fixes.json`), as SolidCraft's rig ran them: with every fix off, a quarter
//! torus's volume is 1.3e-9 off its closed form.
// TODO(d0b44726f): the cases with arcs (extrude_circle*, sweep_circle_*, loft_two_circles,
// planar_faces_with_a_hole, the joins) take the kernel's arcs as exact circles, at bound 0 by
// their label until the kernel proves it.
// The closed forms use square roots and tangents; the tests compute them, not ops. The case
// table's helpers index and panic as tests do; a sheet's area is kept in its case though ops
// measures no area yet.
#![allow(
    clippy::disallowed_methods,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    dead_code
)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_ops::sweep::{
    arcs_on_one_circle, curves_meet, extrude, face_section, join_arcs, loft, loft_to_point,
    planar_faces, project_curves, revolve, screw_sweep, sweep_placed, sweep_stations, Affine,
    CornerRefusal, Curve, DraftCorner, Ends, Join, PathCheck, SweepFrame, SweepRoute, Verdict,
};
use solidcore_ops::{make_box, Body, Error};

const Z: [f64; 3] = [0.0, 0.0, 1.0];

fn line(a: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::Line { start: a, end: b }
}

/// The closed polygon through `pts`.
fn polygon(pts: &[[f64; 3]]) -> Vec<Curve> {
    (0..pts.len())
        .map(|i| line(pts[i], pts[(i + 1) % pts.len()]))
        .collect()
}

/// The square of side `s` from `(x, y)` in the plane z = `z`.
fn square(x: f64, y: f64, s: f64, z: f64) -> Vec<Curve> {
    polygon(&[[x, y, z], [x + s, y, z], [x + s, y + s, z], [x, y + s, z]])
}

/// The circle of `radius` about `axis` through `center`, from `reference`.
fn circle(center: [f64; 3], axis: [f64; 3], reference: [f64; 3], radius: f64) -> Curve {
    Curve::Arc {
        center,
        axis,
        reference,
        radius,
        start: 0.0,
        end: TAU,
    }
}

/// An arc of radius 5 about the z axis from `reference`, from `start` to `end`.
fn arc5(reference: [f64; 3], start: f64, end: f64) -> Curve {
    Curve::Arc {
        center: [0.0; 3],
        axis: Z,
        reference,
        radius: 5.0,
        start,
        end,
    }
}

/// The circle of radius 5 about the z axis as two half arcs in one frame.
fn halves() -> Vec<Curve> {
    vec![
        arc5([1.0, 0.0, 0.0], 0.0, PI),
        arc5([1.0, 0.0, 0.0], PI, TAU),
    ]
}

/// The circle of radius 5 about the z axis as a quarter from +x and three quarters from +y.
fn two_frames() -> Vec<Curve> {
    vec![
        arc5([1.0, 0.0, 0.0], 0.0, FRAC_PI_2),
        arc5([0.0, 1.0, 0.0], 0.0, 3.0 * FRAC_PI_2),
    ]
}

/// The upper half disc of radius 5 about the z axis, its arc in two quarters.
fn half_disc() -> Vec<Curve> {
    vec![
        arc5([1.0, 0.0, 0.0], 0.0, FRAC_PI_2),
        arc5([1.0, 0.0, 0.0], FRAC_PI_2, PI),
        line([-5.0, 0.0, 0.0], [5.0, 0.0, 0.0]),
    ]
}

/// The rectangle from x = 5 to 8 and z = 0 to 4 in the plane y = 0: area 12, centroid at x 6.5.
fn ring_section() -> Vec<Curve> {
    polygon(&[
        [5.0, 0.0, 0.0],
        [8.0, 0.0, 0.0],
        [8.0, 0.0, 4.0],
        [5.0, 0.0, 4.0],
    ])
}

fn translate(t: [f64; 3]) -> Affine {
    Affine {
        m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        t,
    }
}

fn scale_up(s: f64, z: f64) -> Affine {
    Affine {
        m: [[s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, 1.0]],
        t: [0.0, 0.0, z],
    }
}

#[derive(Clone, Debug)]
enum Op {
    PlanarFaces(Vec<Curve>),
    Extrude(Vec<Curve>, [f64; 3], f64, f64, DraftCorner, bool),
    Revolve(Vec<Curve>, [f64; 3], [f64; 3], f64, bool),
    Sweep(Vec<Curve>, Vec<Curve>, SweepFrame, bool),
    Loft(Vec<Vec<Curve>>, u32, bool),
    LoftToPoint(Vec<Curve>, [f64; 3]),
    Stations(Vec<Curve>, Vec<Affine>, Ends),
    Screw(Vec<Curve>, [f64; 3], [f64; 3], f64, f64, f64),
    /// A screw sweep with a radial law.
    ScrewRadial(
        Vec<Curve>,
        [f64; 3],
        [f64; 3],
        f64,
        f64,
        Vec<(f64, f64)>,
        f64,
    ),
}

/// What a case must give.
#[derive(Clone, Debug)]
enum Expect {
    /// A solid of this volume (relative 1e-9).
    Volume(f64),
    /// A solid of this volume within this relative error (walls approximated within a tolerance).
    About(f64, f64),
    /// A sheet of this area (relative 1e-9).
    Sheet(f64),
    /// A drafted corner refused for this reason, wherever.
    Corner(CornerRefusal),
}

struct Case {
    op: Op,
    expect: Expect,
}

/// The frustum of a pyramid or cone: h/3 (A1 + A2 + sqrt(A1 A2)).
fn frustum(a1: f64, a2: f64, h: f64) -> f64 {
    h / 3.0 * (a1 + a2 + (a1 * a2).sqrt())
}

fn case(name: &str) -> Case {
    let c = |op, expect| Case { op, expect };
    let rect = polygon(&[
        [0.0, 0.0, 0.0],
        [30.0, 0.0, 0.0],
        [30.0, 20.0, 0.0],
        [0.0, 20.0, 0.0],
    ]);
    let theta = 5f64.to_radians();
    let disc = || vec![circle([0.0; 3], Z, [1.0, 0.0, 0.0], 5.0)];
    match name {
        // A square of 10 with a hole of radius 2: 100 - 4π.
        "planar_faces_with_a_hole" => {
            let mut loops = square(0.0, 0.0, 10.0, 0.0);
            loops.push(circle([5.0, 5.0, 0.0], Z, [1.0, 0.0, 0.0], 2.0));
            c(Op::PlanarFaces(loops), Expect::Sheet(100.0 - 4.0 * PI))
        }
        // V = A h.
        "extrude_rectangle" => c(
            Op::Extrude(rect, Z, 10.0, 0.0, DraftCorner::default(), true),
            Expect::Volume(6000.0),
        ),
        // V = π r² h.
        "extrude_circle" => c(
            Op::Extrude(disc(), Z, 10.0, 0.0, DraftCorner::default(), true),
            Expect::Volume(250.0 * PI),
        ),
        // A circle drawn in two halves (split where another sketch curve ends on it), the
        // pieces in the circle's own frame. Ops sweeps the arcs as given, a wall each; joining
        // them into one wall is the application's rule (see `a_circle_in_pieces_joins_...`).
        "extrude_circle_in_two_halves" => c(
            Op::Extrude(halves(), Z, 10.0, 0.0, DraftCorner::default(), true),
            Expect::Volume(250.0 * PI),
        ),
        // The same circle as a quarter and three quarters each in its own frame, swept as given:
        // two walls.
        "extrude_circle_in_two_frames" => c(
            Op::Extrude(two_frames(), Z, 10.0, 0.0, DraftCorner::default(), true),
            Expect::Volume(250.0 * PI),
        ),
        // A half disc whose arc is drawn in two quarters, closed by its diameter, swept as given.
        // V = π r² h / 2.
        "extrude_half_disc_split_arc" => c(
            Op::Extrude(half_disc(), Z, 10.0, 0.0, DraftCorner::default(), true),
            Expect::Volume(125.0 * PI),
        ),
        // Leaning in by θ, the walls' offsets overlap at the corners and are trimmed where they
        // cross, under every rule: the frustum from side 20 to 20 - 2 h tanθ.
        "extrude_square_drafted_round" => {
            let top = 20.0 - 2.0 * 10.0 * theta.tan();
            c(
                Op::Extrude(
                    square(0.0, 0.0, 20.0, 0.0),
                    Z,
                    10.0,
                    theta,
                    DraftCorner::Round,
                    true,
                ),
                Expect::Volume(frustum(400.0, top * top, 10.0)),
            )
        }
        "extrude_square_drafted_curvature" => {
            let top = 20.0 - 2.0 * 10.0 * theta.tan();
            c(
                Op::Extrude(
                    square(0.0, 0.0, 20.0, 0.0),
                    Z,
                    10.0,
                    theta,
                    DraftCorner::Curvature,
                    true,
                ),
                Expect::Volume(frustum(400.0, top * top, 10.0)),
            )
        }
        // No corner: the cone frustum from radius 5 to 5 - h tanθ.
        "extrude_circle_drafted" => {
            let r2 = 5.0 - 10.0 * theta.tan();
            c(
                Op::Extrude(disc(), Z, 10.0, theta, DraftCorner::default(), true),
                Expect::Volume(frustum(PI * 25.0, PI * r2 * r2, 10.0)),
            )
        }
        // A strip 2 wide leaning in 10° over 10: the long walls meet across the short ones
        // (10 tan 10° > 1) before the far end.
        "extrude_draft_wall_closes" => c(
            Op::Extrude(
                polygon(&[
                    [0.0, 0.0, 0.0],
                    [20.0, 0.0, 0.0],
                    [20.0, 2.0, 0.0],
                    [0.0, 2.0, 0.0],
                ]),
                Z,
                10.0,
                10f64.to_radians(),
                DraftCorner::Curvature,
                true,
            ),
            Expect::Corner(CornerRefusal::WallCloses),
        ),
        // Pappus: A · θ x̄ = 12 · θ · 6.5.
        "revolve_rectangle_full" => c(
            Op::Revolve(ring_section(), [0.0; 3], Z, TAU, true),
            Expect::Volume(12.0 * TAU * 6.5),
        ),
        "revolve_rectangle_quarter" => c(
            Op::Revolve(ring_section(), [0.0; 3], Z, FRAC_PI_2, true),
            Expect::Volume(12.0 * FRAC_PI_2 * 6.5),
        ),
        // π r² L, along two lines end to end.
        "sweep_circle_straight" => c(
            Op::Sweep(
                vec![circle([0.0; 3], Z, [1.0, 0.0, 0.0], 2.0)],
                vec![
                    line([0.0; 3], [0.0, 0.0, 6.0]),
                    line([0.0, 0.0, 6.0], [0.0, 0.0, 10.0]),
                ],
                SweepFrame::Follow,
                true,
            ),
            Expect::Volume(PI * 4.0 * 10.0),
        ),
        // A quarter of a torus: π r² · R θ.
        "sweep_circle_along_an_arc" => c(
            Op::Sweep(
                vec![circle(
                    [10.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [1.0, 0.0, 0.0],
                    1.0,
                )],
                vec![Curve::Arc {
                    center: [0.0; 3],
                    axis: Z,
                    reference: [1.0, 0.0, 0.0],
                    radius: 10.0,
                    start: 0.0,
                    end: FRAC_PI_2,
                }],
                SweepFrame::Follow,
                true,
            ),
            Expect::Volume(PI * 10.0 * FRAC_PI_2),
        ),
        // A bent path (a line, then a tangent quarter arc): the exact sweep piece by piece, a
        // cylinder into a quarter torus, π r² times the path's length, 10 + 10 π/2.
        "sweep_along_a_bent_path" => c(
            Op::Sweep(
                vec![circle([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0)],
                vec![
                    line([0.0; 3], [10.0, 0.0, 0.0]),
                    Curve::Arc {
                        center: [10.0, 10.0, 0.0],
                        axis: Z,
                        reference: [0.0, -1.0, 0.0],
                        radius: 10.0,
                        start: 0.0,
                        end: FRAC_PI_2,
                    },
                ],
                SweepFrame::Follow,
                true,
            ),
            Expect::Volume(PI * (10.0 + 10.0 * FRAC_PI_2)),
        ),
        // A 4 x 3 rectangle round a closed 60 x 40 rounded rectangle (corner radius 8), from the
        // middle of its bottom line: no caps, the walls closing on each other. Pappus: A L, with
        // A = 12 and L = 2 (44 + 24) + 4 (π/2) 8.
        "sweep_round_a_closed_path" => {
            let arc = |c: [f64; 3], r: [f64; 3]| Curve::Arc {
                center: c,
                axis: Z,
                reference: r,
                radius: 8.0,
                start: 0.0,
                end: FRAC_PI_2,
            };
            let path = vec![
                line([0.0, -20.0, 0.0], [22.0, -20.0, 0.0]),
                arc([22.0, -12.0, 0.0], [0.0, -1.0, 0.0]),
                line([30.0, -12.0, 0.0], [30.0, 12.0, 0.0]),
                arc([22.0, 12.0, 0.0], [1.0, 0.0, 0.0]),
                line([22.0, 20.0, 0.0], [-22.0, 20.0, 0.0]),
                arc([-22.0, 12.0, 0.0], [0.0, 1.0, 0.0]),
                line([-30.0, 12.0, 0.0], [-30.0, -12.0, 0.0]),
                arc([-22.0, -12.0, 0.0], [-1.0, 0.0, 0.0]),
                line([-22.0, -20.0, 0.0], [0.0, -20.0, 0.0]),
            ];
            c(
                Op::Sweep(
                    polygon(&[
                        [0.0, -22.0, -1.5],
                        [0.0, -18.0, -1.5],
                        [0.0, -18.0, 1.5],
                        [0.0, -22.0, 1.5],
                    ]),
                    path,
                    SweepFrame::Follow,
                    true,
                ),
                Expect::About(12.0 * (2.0 * (44.0 + 24.0) + 16.0 * PI), 1e-4),
            )
        }
        // The frustum of a square pyramid: side 20 to 10 over 10.
        "loft_two_squares" => c(
            Op::Loft(
                vec![square(0.0, 0.0, 20.0, 0.0), square(5.0, 5.0, 10.0, 10.0)],
                1,
                true,
            ),
            Expect::Volume(frustum(400.0, 100.0, 10.0)),
        ),
        // The cone frustum: radius 5 to 3 over 10.
        "loft_two_circles" => c(
            Op::Loft(
                vec![
                    disc(),
                    vec![circle([0.0, 0.0, 10.0], Z, [1.0, 0.0, 0.0], 3.0)],
                ],
                1,
                true,
            ),
            Expect::Volume(frustum(25.0 * PI, 9.0 * PI, 10.0)),
        ),
        // A h / 3.
        "loft_to_point_pyramid" => c(
            Op::LoftToPoint(square(0.0, 0.0, 10.0, 0.0), [5.0, 5.0, 6.0]),
            Expect::Volume(200.0),
        ),
        // A square of 2 carried up 10: 40.
        "sweep_stations_prism" => c(
            Op::Stations(
                square(-1.0, -1.0, 2.0, 0.0),
                vec![
                    translate([0.0; 3]),
                    translate([0.0, 0.0, 5.0]),
                    translate([0.0, 0.0, 10.0]),
                ],
                Ends::Capped,
            ),
            Expect::Volume(40.0),
        ),
        // Scaled by s = 1 + z/10 on the way: ∫ (2 s)² dz = 4 · 10 · (2³ - 1) / 3.
        "sweep_stations_scaled" => c(
            Op::Stations(
                square(-1.0, -1.0, 2.0, 0.0),
                vec![scale_up(1.0, 0.0), scale_up(1.5, 5.0), scale_up(2.0, 10.0)],
                Ends::Capped,
            ),
            Expect::Volume(4.0 * 10.0 * 7.0 / 3.0),
        ),
        // A square thread 1 x 1 from radius 5 to 6, pitch 2, two turns: Pappus along the screw
        // motion (its rise lies in the profile's plane), A · 2π x̄ · turns, the walls within the
        // tolerance of the screw surfaces.
        "screw_sweep_square_thread" => c(
            Op::Screw(
                polygon(&[
                    [5.0, 0.0, 0.0],
                    [6.0, 0.0, 0.0],
                    [6.0, 0.0, 1.0],
                    [5.0, 0.0, 1.0],
                ]),
                [0.0; 3],
                Z,
                2.0,
                2.0,
                1e-5,
            ),
            Expect::About(TAU * 5.5 * 2.0, 1e-5),
        ),
        // The same thread moved out by 0 to 1 over its two turns, linearly: Pappus with the
        // centroid at 5.5 + t/2, A · 2π · turns · (5.5 + 1/2).
        "screw_sweep_radial_taper" => c(
            Op::ScrewRadial(
                polygon(&[
                    [5.0, 0.0, 0.0],
                    [6.0, 0.0, 0.0],
                    [6.0, 0.0, 1.0],
                    [5.0, 0.0, 1.0],
                ]),
                [0.0; 3],
                Z,
                2.0,
                2.0,
                vec![(0.0, 0.0), (2.0, 1.0)],
                1e-5,
            ),
            Expect::About(TAU * 2.0 * 6.0, 1e-5),
        ),
        _ => panic!("{name}"),
    }
}

fn run(op: &Op) -> solidcore_ops::Result<Body> {
    match op {
        Op::PlanarFaces(l) => planar_faces(l),
        Op::Extrude(p, d, dist, draft, corner, cap) => extrude(p, *d, *dist, *draft, *corner, *cap),
        Op::Revolve(p, o, a, angle, cap) => revolve(p, *o, *a, *angle, *cap),
        Op::Sweep(p, path, frame, cap) => sweep_placed(p, path, *frame, *cap).map(|(b, _, _)| b),
        Op::Loft(s, d, cap) => loft(s, *d, *cap),
        Op::LoftToPoint(s, apex) => loft_to_point(s, *apex),
        Op::Stations(p, s, e) => sweep_stations(p, s, *e),
        Op::Screw(p, o, a, pitch, turns, tol) => screw_sweep(p, *o, *a, *pitch, *turns, None, *tol),
        Op::ScrewRadial(p, o, a, pitch, turns, law, tol) => {
            screw_sweep(p, *o, *a, *pitch, *turns, Some(law), *tol)
        }
    }
}

/// The rig's fix set, the corpus's `fixes.json`, on the calling thread's kernel. The setting
/// is thread-local, so every test thread sets it: a `Once` would give the rig's set to the first
/// thread only.
fn rig_fixes() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/kernel_cases/fixes.json"
    );
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    // A flat JSON array of flag names: every other piece between quotes.
    let names: Vec<&str> = text.split('"').skip(1).step_by(2).collect();
    assert!(!names.is_empty(), "{path}: no fixes");
    solidcore_ops::session::set_thread_fixes(names).unwrap();
}

fn check(name: &str) {
    rig_fixes();
    let c = case(name);
    let got = run(&c.op);
    match (&c.expect, got) {
        (Expect::Volume(v), Ok(b)) => {
            let got = b.volume(1e-8).unwrap();
            assert!(rel(got, *v) < 1e-9, "{name}: volume {got} vs {v}");
        }
        (Expect::About(v, e), Ok(b)) => {
            let got = b.volume(1e-8).unwrap();
            assert!(
                rel(got, *v) < *e,
                "{name}: volume {got} vs {v} (within {e})"
            );
        }
        (Expect::Sheet(_), Ok(b)) => {
            assert_eq!(b.counts().unwrap()[0], 1, "{name}: one planar face");
        }
        (Expect::Corner(why), Err(Error::DraftCornerRefused { why: got, at })) => {
            assert_eq!(&got, why, "{name}");
            assert!(at.is_some(), "{name}: the corner's point");
        }
        (want, got) => panic!("{name}: want {want:?}, got {got:?}"),
    }
}

macro_rules! cases {
    ($($m:ident),* $(,)?) => {$(
        #[test]
        fn $m() {
            check(stringify!($m));
        }
    )*};
}

cases!(
    planar_faces_with_a_hole,
    extrude_rectangle,
    extrude_circle,
    extrude_circle_in_two_halves,
    extrude_circle_in_two_frames,
    extrude_half_disc_split_arc,
    extrude_square_drafted_round,
    extrude_square_drafted_curvature,
    extrude_circle_drafted,
    extrude_draft_wall_closes,
    revolve_rectangle_full,
    revolve_rectangle_quarter,
    sweep_circle_straight,
    sweep_circle_along_an_arc,
    sweep_along_a_bent_path,
    sweep_round_a_closed_path,
    loft_two_squares,
    loft_two_circles,
    loft_to_point_pyramid,
    sweep_stations_prism,
    sweep_stations_scaled,
    screw_sweep_square_thread,
    screw_sweep_radial_taper,
);

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

/// The faces of a body.
fn faces(b: &Body) -> usize {
    b.counts().unwrap()[0]
}

/// The tolerance arcs are joined to here.
const JOIN: f64 = 1e-6;

#[test]
fn a_circle_in_pieces_joins_into_one_circle_that_sweeps_one_wall() {
    rig_fixes();
    // Two halves in one frame, closed at the frame's start: the full circle from angle 0.
    let joined = join_arcs(&halves(), Join::ClosedAtFrameStart, JOIN).unwrap();
    let Curve::Arc {
        start, end, radius, ..
    } = joined
    else {
        panic!("{joined:?}")
    };
    assert_eq!((start, end, radius), (0.0, TAU, 5.0));
    let b = extrude(&[joined], Z, 10.0, 0.0, DraftCorner::default(), true).unwrap();
    assert_eq!(faces(&b), 3, "one wall and two caps");
    assert!(rel(b.volume(1e-8).unwrap(), 250.0 * PI) < 1e-9);
    // A quarter and three quarters in two frames, the seam in the middle of the longer: on the
    // first's frame, a quarter turn plus half of three quarters.
    let joined = join_arcs(&two_frames(), Join::ClosedAtMiddleOfLongest, JOIN).unwrap();
    let Curve::Arc {
        start,
        end,
        reference,
        ..
    } = joined
    else {
        panic!("{joined:?}")
    };
    assert_eq!(reference, [1.0, 0.0, 0.0]);
    assert!(
        (start - 5.0 * PI / 4.0).abs() < 1e-15 && (end - start - TAU).abs() < 1e-15,
        "{start} {end}"
    );
    let b = extrude(&[joined], Z, 10.0, 0.0, DraftCorner::default(), true).unwrap();
    assert_eq!(faces(&b), 3);
    // Swept as given, the two frames make two walls.
    let given = extrude(&two_frames(), Z, 10.0, 0.0, DraftCorner::default(), true).unwrap();
    assert_eq!(faces(&given), 4);
}

#[test]
fn an_open_run_of_arcs_joins_into_one_arc() {
    rig_fixes();
    let d = half_disc();
    let joined = join_arcs(&d[..2], Join::Open, JOIN).unwrap();
    let Curve::Arc { start, end, .. } = joined else {
        panic!("{joined:?}")
    };
    assert_eq!(start, 0.0);
    assert!((end - PI).abs() < 1e-15);
    let b = extrude(
        &[joined, d[2].clone()],
        Z,
        10.0,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap();
    assert_eq!(faces(&b), 4, "the curved wall, the flat one and two caps");
    assert!(rel(b.volume(1e-8).unwrap(), 125.0 * PI) < 1e-9);
}

#[test]
fn arcs_on_one_circle_and_curves_meeting_are_answered_certified() {
    rig_fixes();
    let [a, b] = <[Curve; 2]>::try_from(halves()).unwrap();
    let one = arcs_on_one_circle(&a, &b, JOIN).unwrap().unwrap();
    assert_eq!((one.circle, one.frame), (Verdict::Yes, Verdict::Yes));
    let [p, q] = <[Curve; 2]>::try_from(two_frames()).unwrap();
    let other = arcs_on_one_circle(&p, &q, JOIN).unwrap().unwrap();
    assert_eq!((other.circle, other.frame), (Verdict::Yes, Verdict::No));
    // A line is no arc.
    assert_eq!(
        arcs_on_one_circle(&a, &line([0.0; 3], [1.0, 0.0, 0.0]), JOIN).unwrap(),
        None
    );
    assert_eq!(curves_meet(&a, &b, JOIN).unwrap(), Verdict::Yes);
    assert_eq!(curves_meet(&b, &b, JOIN).unwrap(), Verdict::No);
}

#[test]
fn a_moved_arc_is_refused() {
    rig_fixes();
    // The second half moved off the circle by 1e-3: on another circle, and not joined.
    let moved = Curve::Arc {
        center: [1e-3, 0.0, 0.0],
        axis: Z,
        reference: [1.0, 0.0, 0.0],
        radius: 5.0,
        start: PI,
        end: TAU,
    };
    let [a, _] = <[Curve; 2]>::try_from(halves()).unwrap();
    let off = arcs_on_one_circle(&a, &moved, JOIN).unwrap().unwrap();
    assert_eq!(off.circle, Verdict::No);
    assert_eq!(
        join_arcs(&[a.clone(), moved], Join::ClosedAtFrameStart, JOIN),
        Err(Error::Argument)
    );
    // Four halves meet round the run but turn twice: no closed join drops a turn.
    let twice = [
        halves(),
        vec![
            arc5([1.0, 0.0, 0.0], TAU, 3.0 * PI),
            arc5([1.0, 0.0, 0.0], 3.0 * PI, 2.0 * TAU),
        ],
    ]
    .concat();
    assert_eq!(
        join_arcs(&twice, Join::ClosedAtFrameStart, JOIN),
        Err(Error::Argument)
    );
    // A line in the run.
    assert_eq!(
        join_arcs(&[a, line([0.0; 3], [1.0, 0.0, 0.0])], Join::Open, JOIN),
        Err(Error::Argument)
    );
}

/// A disc of radius 1 about the x axis through the origin.
fn rod() -> Vec<Curve> {
    vec![circle([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0)]
}

#[test]
fn a_path_left_undecided_is_refused_naming_its_piece() {
    rig_fixes();
    // Two lines whose ends are exactly the path gap (1e-9) apart: the kernel cannot decide
    // whether they meet, so it cannot prove the path straight, round or end to end. With
    // `Follow` that is refused, not fitted by the curve sweep.
    let path = vec![
        line([0.0; 3], [5.0, 0.0, 0.0]),
        line([5.0, 1e-9, 0.0], [10.0, 1e-9, 0.0]),
    ];
    let got = sweep_placed(&rod(), &path, SweepFrame::Follow, true);
    assert_eq!(
        got.err(),
        Some(Error::PathUndecided {
            piece: 1,
            check: PathCheck::Gap
        })
    );
}

#[test]
fn a_path_whose_pieces_do_not_meet_is_refused_before_the_curve_sweep() {
    rig_fixes();
    // A line, then a quarter arc starting 1e-4 off its end: no exact shape, and the curve
    // sweep would join across the gap. Refused at join 0, in both frames.
    let path = vec![
        line([0.0; 3], [10.0, 0.0, 0.0]),
        Curve::Arc {
            center: [10.0, 10.0 + 1e-4, 0.0],
            axis: Z,
            reference: [0.0, -1.0, 0.0],
            radius: 10.0,
            start: 0.0,
            end: FRAC_PI_2,
        },
    ];
    for frame in [SweepFrame::Follow, SweepFrame::Translate] {
        let got = sweep_placed(&rod(), &path, frame, true);
        assert_eq!(got.err(), Some(Error::PathGap { join: 0 }), "{frame:?}");
    }
}

#[test]
fn the_route_taken_is_reported() {
    rig_fixes();
    let route = |path: &[Curve], frame| sweep_placed(&rod(), path, frame, true).unwrap().2;
    let straight = [
        line([0.0; 3], [4.0, 0.0, 0.0]),
        line([4.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
    ];
    assert_eq!(route(&straight, SweepFrame::Follow), SweepRoute::Line);
    let quarter = [Curve::Arc {
        center: [0.0, 10.0, 0.0],
        axis: Z,
        reference: [0.0, -1.0, 0.0],
        radius: 10.0,
        start: 0.0,
        end: FRAC_PI_2,
    }];
    assert_eq!(route(&quarter, SweepFrame::Follow), SweepRoute::Arc);
    let bent = [
        line([0.0; 3], [10.0, 0.0, 0.0]),
        Curve::Arc {
            center: [10.0, 10.0, 0.0],
            axis: Z,
            reference: [0.0, -1.0, 0.0],
            radius: 10.0,
            start: 0.0,
            end: FRAC_PI_2,
        },
    ];
    assert_eq!(route(&bent, SweepFrame::Follow), SweepRoute::Segments);
    // A quadratic B-spline path: the curve sweep, in either frame.
    let spline = [Curve::Nurbs {
        degree: 2,
        points: vec![
            [0.0, 0.0, 0.0, 1.0],
            [5.0, 0.0, 0.0, 1.0],
            [10.0, 3.0, 0.0, 1.0],
        ],
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    }];
    assert_eq!(
        route(&spline, SweepFrame::Follow),
        SweepRoute::Curve(SweepFrame::Follow)
    );
    assert_eq!(
        route(&spline, SweepFrame::Translate),
        SweepRoute::Curve(SweepFrame::Translate)
    );
}

#[test]
fn a_closed_path_whose_ends_are_almost_meeting_is_refused() {
    rig_fixes();
    // sweep_round_a_closed_path with its closing join opened by 1e-7: neither closed to the path
    // gap nor apart by more than 1e-6, so not stitched shut.
    let Op::Sweep(profile, mut path, frame, cap) = case("sweep_round_a_closed_path").op else {
        panic!()
    };
    let n = path.len();
    path[n - 1] = line([-22.0, -20.0, 0.0], [-1e-7, -20.0, 0.0]);
    assert_eq!(
        sweep_placed(&profile, &path, frame, cap).err(),
        Some(Error::PathGap { join: n - 1 })
    );
}

/// The faces of a 10 mm box named by record index through its `Order`: the plane z = 5 (its
/// normal at length 2) sections the four side faces in one curve each, at z = 5, and misses the
/// top and bottom; an index past the record's faces and a normal of no length are refused.
#[test]
fn a_box_s_record_faces_section_at_their_index() {
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (record, order) = b.to_record().unwrap();
    assert_eq!(record.faces.len(), 6);
    let mut cut = 0;
    for i in 0..6 {
        match face_section(&b, &order, i, [0.0, 0.0, 5.0], [0.0, 0.0, 2.0], 1e-6) {
            Ok(curves) => {
                assert_eq!(curves.len(), 1, "face {i}");
                let Curve::Nurbs { points, .. } = &curves[0].0 else {
                    panic!("face {i}: {:?}", curves[0].0)
                };
                assert!(points.iter().all(|p| (p[2] / p[3] - 5.0).abs() < 1e-9));
                cut += 1;
            }
            Err(e) => assert_eq!(e, Error::Empty, "face {i}"),
        }
    }
    assert_eq!(cut, 4);
    let at = |i, n| face_section(&b, &order, i, [0.0, 0.0, 5.0], n, 1e-6);
    assert_eq!(at(6, Z), Err(Error::Argument), "an index past the faces");
    assert_eq!(
        at(0, [0.0; 3]),
        Err(Error::Argument),
        "a normal of no length"
    );
}

/// A segment over a 10 mm box projected down (the direction at length 3) lands on two record
/// faces, the top at z = 10 and the bottom at z = 0: a parallel projection onto a face is onto
/// that face's own surface (`SmBrep::CreateParallelProjectionCurves`), whatever lies between;
/// the side faces take none of it. An index past the record's faces and a direction of no length
/// are refused.
#[test]
fn a_segment_projects_onto_a_box_s_record_face_at_its_index() {
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, order) = b.to_record().unwrap();
    let seg = [line([2.0, 3.0, 20.0], [8.0, 3.0, 20.0])];
    let landed: Vec<_> = (0..6)
        .filter_map(|i| project_curves(&b, &order, i, &seg, [0.0, 0.0, -3.0], 1e-6).ok())
        .collect();
    assert_eq!(landed.len(), 2, "the top and the bottom");
    let mut heights: Vec<f64> = landed
        .iter()
        .map(|on_face| {
            let Curve::Nurbs { points, .. } = &on_face[0].0 else {
                panic!("{:?}", on_face[0].0)
            };
            let z = points[0][2] / points[0][3];
            assert!(points.iter().all(|p| (p[2] / p[3] - z).abs() < 1e-9));
            z
        })
        .collect();
    heights.sort_by(f64::total_cmp);
    assert!(
        (heights[0] - 0.0).abs() < 1e-9 && (heights[1] - 10.0).abs() < 1e-9,
        "{heights:?}"
    );
    let at = |i, d| project_curves(&b, &order, i, &seg, d, 1e-6);
    assert_eq!(at(6, Z), Err(Error::Argument), "an index past the faces");
    assert_eq!(
        at(0, [0.0; 3]),
        Err(Error::Argument),
        "a direction of no length"
    );
}

/// Two thread grooves as SolidCraft's Thread feature sweeps them (an M12 in a 10.2 mm hole, an
/// M10x1.5 over 10.23 turns) cross the record boundary and back: the record holds an edge whose
/// interval runs past its UV curve's end by the rounding of the kernel's parameter, and an end
/// cap whose plane was built from rounded corners with its axes a few units in the last place
/// off square. Each record is taken back by the kernel and agrees with its topology.
#[test]
fn a_thread_grooves_screw_sweep_crosses_the_record_and_back() {
    let p = |y: f64, z: f64| [0.0, y, z];
    let m12 = polygon(&[
        p(4.877784714610771, -2.5072862971081844),
        p(6.0, -1.8593750000000004),
        p(6.0, -1.6406249999999996),
        p(4.877784714610771, -0.9927137028918156),
    ]);
    let m10 = polygon(&[
        p(4.188101183952089, -1.6875),
        p(5.075, -2.199551270189222),
        p(5.075, -0.800448729810778),
        p(4.188101183952089, -1.3125),
    ]);
    for (profile, origin, pitch, turns) in [
        (&m12, [0.0, 0.0, -1.75], 1.75, 13.428571428571429),
        (&m10, [0.0, 0.0, -1.5], 1.5, 10.230000000000002),
    ] {
        let tolerance = pitch * 1e-4;
        let made = screw_sweep(profile, origin, Z, pitch, turns, None, tolerance).unwrap();
        let (record, _) = made.to_record().unwrap();
        solidcore_ops::record_check::validate_built(&record).unwrap();
        solidcore_ops::record_check::agreement(&record).unwrap();
        let (back, _) = Body::from_record(&record, true).unwrap();
        let (a, b) = (made.volume(1e-8).unwrap(), back.volume(1e-8).unwrap());
        assert!(
            (a - b).abs() <= 1e-9 * a,
            "{turns} turns: {a} made, {b} read back"
        );
    }
}

/// The two grooves of a two-start M10 thread (lead 3 mm, each a turn's half further round)
/// start and end in one axial plane, on opposite sides of the axis: their caps' bounding planes
/// coincide, so the boolean intersects the walls' exact boundary curves there. The grooves do
/// not meet, and their union is both of them: each copy of a boundary curve on a screw-swept
/// wall is that curve, not refused for being no B-spline.
#[test]
fn the_grooves_of_a_two_start_thread_unite_as_both() {
    let groove = |side: f64| {
        let p = |y: f64, z: f64| [0.0, side * y, z];
        polygon(&[
            p(4.188101183952089, -1.6875),
            p(5.075, -2.199551270189222),
            p(5.075, -0.800448729810778),
            p(4.188101183952089, -1.3125),
        ])
    };
    let made =
        |side| screw_sweep(&groove(side), [0.0, 0.0, -1.5], Z, 3.0, 3.0, None, 3e-4).unwrap();
    let (a, b) = (made(1.0), made(-1.0));
    let (va, vb) = (a.volume(1e-8).unwrap(), b.volume(1e-8).unwrap());
    let both = solidcore_ops::boolean::union_all(vec![a, b]).unwrap();
    let v = both.volume(1e-8).unwrap();
    assert!(
        (v - (va + vb)).abs() <= 1e-6 * v,
        "{v}, the grooves {va} + {vb}"
    );
}

/// A thread cut both ways (SolidCraft's `hand: both`): the right-handed groove and the
/// left-handed one (the same turn about the reversed axis, rising by a negative pitch, its
/// profile wound the other way so its walls face out) cross each other twice a turn. The
/// boolean copies and trims the walls' exact boundary curves where they cross, and the union
/// and the intersection hold their volumes: V(a ∪ b) + V(a ∩ b) = V(a) + V(b). Where the walls
/// meet along their boundaries the union's edges are the walls' iso curves; the record takes
/// each as the curve on its screw along its UV line, so the union goes to the record and comes
/// back the same solid (SolidCraft's export of the thread), not refused as a curve with no
/// B-spline form.
#[test]
fn the_grooves_of_a_thread_cut_both_ways_unite() {
    use solidcore_ops::boolean::{boolean, union_all, BooleanOp};
    let p = |y: f64, z: f64| [0.0, y, z];
    let mut pts = vec![
        p(4.188101183952089, -1.6875),
        p(5.075, -2.199551270189222),
        p(5.075, -0.800448729810778),
        p(4.188101183952089, -1.3125),
    ];
    let made = |pts: &[[f64; 3]], axis, pitch| {
        screw_sweep(
            &polygon(pts),
            [0.0, 0.0, -1.5],
            axis,
            pitch,
            2.0,
            None,
            1.5e-4,
        )
        .unwrap()
    };
    let right = made(&pts, Z, 1.5);
    pts.reverse();
    let left = made(&pts, [0.0, 0.0, -1.0], -1.5);
    let (vr, vl) = (right.volume(1e-8).unwrap(), left.volume(1e-8).unwrap());
    let both = boolean(
        right.copy().unwrap(),
        left.copy().unwrap(),
        BooleanOp::Intersect,
    )
    .unwrap()
    .unwrap();
    let either = union_all(vec![right, left]).unwrap();
    let (vu, vi) = (either.volume(1e-8).unwrap(), both.volume(1e-8).unwrap());
    assert!(
        vi > 0.0 && vu < vr + vl,
        "they cross: {vu} of {vr} + {vl}, {vi} in both"
    );
    assert!(
        (vu + vi - (vr + vl)).abs() <= 1e-6 * (vr + vl),
        "{vu} + {vi} vs {vr} + {vl}"
    );
    let (record, _) = either.to_record().unwrap();
    let (back, _) = Body::from_record(&record, true).unwrap();
    let vb = back.volume(1e-8).unwrap();
    assert_eq!(back.counts().unwrap(), either.counts().unwrap());
    assert!((vb - vu).abs() <= 1e-8 * vu, "{vb} back, {vu} made");
}

/// The grooves of a 22-turn M10 thread cut both ways (SolidCraft's 33 mm bolt in both hands): the
/// union splits each groove's two flanks into a face wherever the other groove crosses them,
/// 176 faces on 4 walls. Each face goes to the record as its own turns of its wall: the
/// stations its box crosses and the net over them, in the same parameters, under the bound
/// certified for that piece, never more than the whole wall's. With every face carrying its
/// whole wall the record took 6,010,780 items, past an outside record's budget, so reading it
/// back was refused; now it grows with the turns, not with the faces times the turns, and
/// comes back the same solid.
#[test]
fn the_grooves_of_a_long_thread_cut_both_ways_go_to_the_record_within_its_budget() {
    use solidcore::surface::procedural_sweep::ProceduralSweep;
    use solidcore_record::brep::sweep::ProceduralSweepRecord;
    use solidcore_record::brep::{Surface, MAX_ITEMS};
    let p = |y: f64, z: f64| [0.0, y, z];
    let mut pts = vec![
        p(4.188101183952089, -1.6875),
        p(5.075, -2.199551270189222),
        p(5.075, -0.800448729810778),
        p(4.188101183952089, -1.3125),
    ];
    let turns = 22.0;
    let made = |pts: &[[f64; 3]], axis, pitch| {
        screw_sweep(
            &polygon(pts),
            [0.0, 0.0, -1.5],
            axis,
            pitch,
            turns,
            None,
            1.5e-4,
        )
        .unwrap()
    };
    let right = made(&pts, Z, 1.5);
    pts.reverse();
    let left = made(&pts, [0.0, 0.0, -1.0], -1.5);
    let either = solidcore_ops::boolean::union_all(vec![right, left]).unwrap();
    let (record, _) = either.to_record().unwrap();
    let items = record.items().unwrap();
    assert!(items < MAX_ITEMS, "{items} items of {MAX_ITEMS}");
    record.validate().unwrap();
    // Each wall's face carries its own spans: its stations are the ones its box crosses.
    let mut split = 0;
    for f in &record.faces {
        let Surface::ProceduralSweep(s) = &f.surface else {
            continue;
        };
        let [lo, hi] = if s.swapped { f.domain[0] } else { f.domain[1] };
        let st = &s.stations;
        let (first, last) = (st[0], st[st.len() - 1]);
        assert!(
            first <= lo && lo < st[1] && st[st.len() - 2] < hi && hi <= last,
            "the face over [{lo}, {hi}] keeps the stations {first} to {last}"
        );
        split += 1;
    }
    assert_eq!(split, 176, "the walls' faces");
    // Each piece's bound is its own certificate, at most its whole wall's re-certified one:
    // the record's shared owners, which the wall's edges name, are the whole walls.
    let proved = |s: &ProceduralSweepRecord| {
        let mut s = s.clone();
        s.bound = f64::MIN_POSITIVE;
        let s = ProceduralSweep::from_record(&solidcore_convert::procedural_sweep(&s)).unwrap();
        s.procedural_sweep().unwrap().definition().bound()
    };
    let walls: Vec<_> = record
        .screw_owners
        .iter()
        .map(|wall| (wall, proved(wall)))
        .collect();
    for f in &record.faces {
        let Surface::ProceduralSweep(piece) = &f.surface else {
            continue;
        };
        let (_, whole) = walls
            .iter()
            .find(|(wall, _)| {
                wall.definition == piece.definition
                    && wall
                        .stations
                        .windows(piece.stations.len())
                        .any(|w| w == piece.stations)
            })
            .expect("the piece's wall is an owner");
        assert!(piece.bound <= *whole, "{} of {whole}", piece.bound);
    }
    let (back, _) = Body::from_record(&record, true).unwrap();
    let (vb, vu) = (back.volume(1e-8).unwrap(), either.volume(1e-8).unwrap());
    assert_eq!(back.counts().unwrap(), either.counts().unwrap());
    assert!((vb - vu).abs() <= 1e-8 * vu, "{vb} back, {vu} made");
}

/// An M10 groove with a runout (its law moves the profile out 0.887 mm over the last two of four
/// turns), cut from a 5 mm cylinder, at the tolerance SolidCraft sweeps a 1.5 mm pitch to. The
/// law makes the groove's axial wall the extrusion of a spiral, and its net the extrusion of the
/// net's spiral, about 1e-5 off it: the boolean copies that wall as its net with its definition,
/// not as the surface of extrusion recognised from the net, so the flanks' edges on it, on the
/// definition, stay within the body's tolerance of it, and the result passes the check.
#[test]
fn a_runout_groove_cut_from_a_cylinder_keeps_its_axial_wall_exact() {
    use solidcore_ops::boolean::{boolean, BooleanOp};
    use solidcore_record::brep::Surface;
    let p = |y: f64, z: f64| [0.0, y, z];
    let groove = polygon(&[
        p(4.188101183952089, -1.6875),
        p(5.075, -2.199551270189222),
        p(5.075, -0.800448729810778),
        p(4.188101183952089, -1.3125),
    ]);
    let turns = 4.0;
    let law = [(turns - 2.0, 0.0), (turns, 0.887)];
    let cut = screw_sweep(&groove, [0.0, 0.0, -1.5], Z, 1.5, turns, Some(&law), 1.5e-4).unwrap();
    let rod = solidcore_ops::make_cylinder([0.0; 3], 5.0, 1.5 * turns - 3.0).unwrap();
    let threaded = boolean(rod, cut, BooleanOp::Subtract).unwrap().unwrap();
    let report = threaded.check(solidcore_ops::CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    let (record, _) = threaded.to_record().unwrap();
    // The rod's side, ends and the groove's walls: each wall still its definition.
    let lost = record
        .faces
        .iter()
        .filter(|f| {
            matches!(
                f.surface,
                Surface::Spline { .. } | Surface::RationalSpline { .. }
            )
        })
        .count();
    assert_eq!(lost, 0, "a swept wall lost its definition");
}
