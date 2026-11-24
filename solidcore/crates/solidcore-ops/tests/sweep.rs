//! The sweeps against their closed forms (moved from SolidCraft's `crates/smlib` sweep tests):
//! each case's volume (relative 1e-9, or within the tolerance its walls are approximated to),
//! a refused draft corner's reason, a planar sheet's one face. Then the arcs of a profile on one
//! circle: the queries and the join an application's one-wall rule is built on, and their
//! refusals. They run under the rig's fix set
//! (SolidCraft's preset, `solidcraft_fixes.rs`), as SolidCraft's rig ran them: with every fix off, a quarter
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

#[path = "../../solidcore/tests/support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_ops::sweep::{
    arcs_on_one_circle, curves_meet, extrude, face_section, join_arcs, loft, loft_sheet,
    loft_to_point, loft_to_point_through, planar_faces, project_curves, revolve, screw_sweep,
    sweep_placed, sweep_stations, Affine, CornerRefusal, Curve, DraftCorner, Ends, Join, PathCheck,
    SweepFrame, SweepRoute, Verdict,
};
use solidcore_ops::sweep::{loft_with_ends, LoftEnd};
use solidcore_ops::{make_box, Body, CheckLevel, Error};

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
    /// A loft from the first section through the rest (stations) to the apex, at the parameters.
    LoftToPointThrough(Vec<Vec<Curve>>, [f64; 3], Vec<f64>),
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
        // (10 tan 10° > 1) before the far end, and the section closes there along its ridge:
        // the hip roof, ∫ (a - 2tz)(b - 2tz) dz to z* = b / 2t, V = b² (3a - b) / 12t.
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
            Expect::Volume(4.0 * (3.0 * 20.0 - 2.0) / (12.0 * 10f64.to_radians().tan())),
        ),
        // The same strip leaning in so that its long walls meet just at the far end
        // (10 tan θ = 1): the far end would pinch to the ridge, so it is refused.
        "extrude_draft_ridge_on_far_end" => c(
            Op::Extrude(
                polygon(&[
                    [0.0, 0.0, 0.0],
                    [20.0, 0.0, 0.0],
                    [20.0, 2.0, 0.0],
                    [0.0, 2.0, 0.0],
                ]),
                Z,
                10.0,
                0.1f64.atan(),
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
        // Through the square halved at half height: the pyramid's own station, so the pyramid,
        // A h / 3.
        "loft_to_point_through_pyramid" => c(
            Op::LoftToPointThrough(
                vec![square(0.0, 0.0, 10.0, 0.0), square(2.5, 2.5, 5.0, 3.0)],
                [5.0, 5.0, 6.0],
                vec![0.0, 0.5, 1.0],
            ),
            Expect::Volume(200.0),
        ),
        // A circle of radius 5 through the circle of 2.5 halfway to an apex 10 up: the cone,
        // pi r^2 h / 3.
        "loft_to_point_through_cone" => c(
            Op::LoftToPointThrough(
                vec![
                    vec![circle([0.0; 3], Z, [1.0, 0.0, 0.0], 5.0)],
                    vec![circle([0.0, 0.0, 5.0], Z, [1.0, 0.0, 0.0], 2.5)],
                ],
                [0.0, 0.0, 10.0],
                vec![0.0, 0.5, 1.0],
            ),
            Expect::Volume(PI * 25.0 * 10.0 / 3.0),
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
        Op::LoftToPointThrough(s, apex, params) => loft_to_point_through(s, *apex, params),
        Op::Stations(p, s, e) => sweep_stations(p, s, *e),
        Op::Screw(p, o, a, pitch, turns, tol) => screw_sweep(p, *o, *a, *pitch, *turns, None, *tol),
        Op::ScrewRadial(p, o, a, pitch, turns, law, tol) => {
            screw_sweep(p, *o, *a, *pitch, *turns, Some(law), *tol)
        }
    }
}

/// The rig's fix set, SolidCraft's preset (`solidcraft_fixes.rs`), on the calling thread's
/// kernel. The setting is thread-local, so every test thread sets it: a `Once` would give the
/// rig's set to the first thread only.
fn rig_fixes() {
    solidcore_ops::session::set_thread_fixes(solidcraft_fixes::solidcraft_fix_names()).unwrap();
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
    extrude_draft_ridge_on_far_end,
    revolve_rectangle_full,
    revolve_rectangle_quarter,
    sweep_circle_straight,
    sweep_circle_along_an_arc,
    sweep_along_a_bent_path,
    sweep_round_a_closed_path,
    loft_two_squares,
    loft_two_circles,
    loft_to_point_pyramid,
    loft_to_point_through_pyramid,
    loft_to_point_through_cone,
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

/// An arc in the plane z = 0 about the z axis (`turn` +1) or against it (-1), from the x axis.
fn arc_about(center: [f64; 2], turn: f64, radius: f64, start: f64, end: f64) -> Curve {
    Curve::Arc {
        center: [center[0], center[1], 0.0],
        axis: [0.0, 0.0, turn],
        reference: [1.0, 0.0, 0.0],
        radius,
        start,
        end,
    }
}

/// The area a profile of lines and arcs in the plane z = 0 bounds, its holes running
/// clockwise: Green's theorem, half the integral of x dy - y dx round every curve, exactly.
fn area_bounded(profile: &[Curve]) -> f64 {
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    profile
        .iter()
        .map(|c| match c {
            Curve::Line { start, end } => 0.5 * cross([start[0], start[1]], [end[0], end[1]]),
            Curve::Arc {
                center,
                axis,
                reference,
                radius,
                start,
                end,
            } => {
                let (u, s) = (reference, axis[2]);
                let v = [-u[1] * s, u[0] * s];
                let at = |t: f64| {
                    [
                        radius * (t.cos() * u[0] + t.sin() * v[0]),
                        radius * (t.cos() * u[1] + t.sin() * v[1]),
                    ]
                };
                let (p, q) = (at(*start), at(*end));
                let chord = [q[0] - p[0], q[1] - p[1]];
                0.5 * (cross([center[0], center[1]], chord) + radius * radius * s * (end - start))
            }
            Curve::Nurbs { .. } => panic!("{c:?}"),
        })
        .sum()
}

/// #9216: three holes of a profile touch at one point, where a circle and a line are tangent
/// to the outer circle: the r 3.5 hole below the line and the two slits between the line and
/// the r 60 circle either side. The sketch solver left the r 3.5 circle 2.6e-8 off the point,
/// so the curves end there up to 2.6e-8 apart. The r 3.5 hole is one loop with the slit it
/// touches, so its wall is one full cylinder whose seam edge stands at the point, 2.6e-8 from
/// the vertex there; the slits' tips stand beside the seam on walls of their own. 0.27.0 built
/// this extrusion; face test 36 refused it.
#[test]
fn holes_touching_at_a_tangent_point_extrude_to_a_valid_solid() {
    rig_fixes();
    let p = [
        arc_about(
            [0.0, 0.0],
            1.0,
            60.0,
            -4.440892098500626e-16,
            0.8410686705679303,
        ),
        line([40.0, 44.721359549995796, 0.0], [40.0, 60.0, 0.0]),
        line([40.0, 60.0, 0.0], [39.2, 60.0, 0.0]),
        line([39.2, 60.0, 0.0], [39.2, 45.424222613050844, 0.0]),
        arc_about([0.0, 0.0], 1.0, 60.0, 0.8588172718823294, FRAC_PI_2),
        arc_about([0.0, 0.0], 1.0, 60.0, FRAC_PI_2, 2.282775381707464),
        line([-39.2, 45.424222613050844, 0.0], [-39.2, 60.0, 0.0]),
        line([-39.2, 60.0, 0.0], [-40.0, 60.0, 0.0]),
        line([-40.0, 60.0, 0.0], [-40.0, 44.721359549995796, 0.0]),
        arc_about([0.0, 0.0], 1.0, 60.0, 2.300523983021863, PI),
        arc_about([0.0, 0.0], 1.0, 60.0, PI, 4.211519448788037),
        line(
            [-28.81130658186081, -52.629921271516494, 0.0],
            [-2.0648102791373195, -67.27181102503482, 0.0],
        ),
        arc_about(
            [2.589620916414858e-08, -63.50000000057623],
            1.0,
            4.3,
            4.211519448787992,
            5.213258512796999,
        ),
        line(
            [2.0648103340060695, -67.27181102335074, 0.0],
            [28.81130662478856, -52.629921248016466, 0.0],
        ),
        arc_about([0.0, 0.0], 1.0, 60.0, 5.213258512796996, TAU),
        arc_about(
            [0.0, 0.0],
            -1.0,
            59.2,
            -10.494704755967625,
            -5.213258512796996,
        ),
        arc_about(
            [0.0, 0.0],
            -1.0,
            59.2,
            -5.213258512796996,
            -4.712388980792504,
        ),
        arc_about(
            [0.0, 0.0],
            -1.0,
            59.2,
            -4.712388980792504,
            -4.211519448788038,
        ),
        arc_about([0.0, 0.0], 1.0, 60.0, 4.375000755112054, 4.671916535172597),
        arc_about([0.0, 0.0], 1.0, 60.0, 4.671916535172597, 4.71238898038469),
        line(
            [-1.1021821192326178e-14, -60.0, 0.0],
            [-2.4979991726557813, -60.0, 0.0],
        ),
        arc_about(
            [2.589620916414858e-08, -63.50000000057623],
            1.0,
            4.3,
            2.1906671803402036,
            3.591648595026369,
        ),
        line(
            [-3.8718178779954653, -65.37056839523703, 0.0],
            [-19.861421682738087, -56.6173465356728, 0.0],
        ),
        arc_about([0.0, 0.0], 1.0, 60.0, 4.7528614264124105, 5.04977720647299),
        line(
            [19.861421728918845, -56.61734651947255, 0.0],
            [3.871817931313502, -65.3705683920792, 0.0],
        ),
        arc_about(
            [2.589620916414858e-08, -63.50000000057623],
            1.0,
            4.3,
            5.833129366558602,
            7.234110780429176,
        ),
        line(
            [2.4979992244482014, -60.0, 0.0],
            [2.589620712709006e-08, -60.00000000057623, 0.0],
        ),
        arc_about(
            [2.589620916414858e-08, -63.50000000057623],
            -1.0,
            3.5,
            -7.853981633974484,
            -5.2132585127969975,
        ),
        arc_about(
            [2.589620916414858e-08, -63.50000000057623],
            -1.0,
            3.5,
            -5.2132585127969975,
            -4.211519448787993,
        ),
        arc_about(
            [2.589620916414858e-08, -63.50000000057623],
            -1.0,
            3.5,
            -4.211519448787993,
            -1.5707963267948974,
        ),
        arc_about([0.0, 0.0], 1.0, 60.0, 4.712388980816293, 4.7528614264124105),
    ];
    let open = |run: &[usize]| {
        let pieces: Vec<Curve> = run.iter().map(|&i| p[i].clone()).collect();
        join_arcs(&pieces, Join::Open, JOIN).unwrap()
    };
    let c = |i: usize| p[i].clone();
    // The arcs on one circle joined as SolidCraft joins them (one wall per circle): the outer
    // loop, the r 59.2 hole (a whole circle), the two slits and, in the second slit's loop
    // (which runs round the r 3.5 hole from the point it touches), the r 3.5 circle.
    let profile = [
        c(1),
        c(2),
        c(3),
        open(&[4, 5]),
        c(6),
        c(7),
        c(8),
        open(&[9, 10]),
        c(11),
        c(12),
        c(13),
        open(&[14, 0]),
        join_arcs(&p[15..18], Join::ClosedAtFrameStart, JOIN).unwrap(),
        open(&[18, 19]),
        c(20),
        c(21),
        c(22),
        c(24),
        c(25),
        c(26),
        open(&[27, 28, 29]),
        open(&[30, 23]),
    ];
    let b = extrude(&profile, Z, 3.0, 0.0, DraftCorner::default(), true).unwrap();
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    // A wall per curve and two caps; an edge up each wall's join and round each cap; on each
    // cap twelve vertices round the outside, the r 59.2 circle's seam vertex, and seven round
    // the slits and the r 3.5 circle, which share the point they touch at.
    assert_eq!(b.counts().unwrap(), [24, 66, 40]);
    assert_eq!(b.to_record().unwrap().0.shells.len(), 1);
    let want = 3.0 * area_bounded(&p);
    assert!(rel(b.volume(1e-10).unwrap(), want) < 1e-9, "{want}");
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

/// The x range of record face `face`'s corners.
fn corners_x(record: &solidcore_record::brep::Brep, face: usize) -> [f64; 2] {
    let xs: Vec<f64> = record
        .edges
        .iter()
        .filter(|e| e.uses.iter().any(|u| u.face as usize == face))
        .flat_map(|e| e.vertices)
        .map(|v| record.vertices[v as usize].point[0])
        .collect();
    [
        xs.iter().copied().fold(f64::INFINITY, f64::min),
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    ]
}

/// Two lines from x = 0 to 5 and on to 10 at height `z`, the corners at `y`.
fn two_lines(y: [f64; 3], z: f64) -> Vec<Curve> {
    vec![
        line([0.0, y[0], z], [5.0, y[1], z]),
        line([5.0, y[1], z], [10.0, y[2], z]),
    ]
}

/// A sheet lofted through three sections of two lines each, whose corners move differently from
/// one section to the next (the middle one unevenly, so each column's chords differ), is one
/// skin: its two faces share the seam between them (tangency-labs/solidcraft#14048, where each
/// column skinned on its own took parameters of its own and the seams parted). Each face is
/// traced to its one column, the face between the first lines lying over x 0…5 and the other
/// over x 5…10: curves `k`, `2 + k` and `4 + k`.
#[test]
fn a_loft_sheet_s_columns_share_their_seams() {
    let sections = [
        two_lines([0.0, 0.0, 0.0], 0.0),
        two_lines([1.0, 6.0, 3.0], 5.0),
        two_lines([0.0, 2.0, 0.0], 10.0),
    ];
    let (b, sources) = loft_sheet(&sections, 2).unwrap();
    assert_eq!(b.counts().unwrap(), [2, 7, 6], "two faces sharing one seam");
    let (record, _) = b.to_record().unwrap();
    assert_eq!(record.edges.iter().filter(|e| e.uses.len() == 1).count(), 6);
    let sources = sources.expect("traced");
    assert_eq!(sources.len(), 2, "{sources:?}");
    for (f, curves) in sources.into_iter().enumerate() {
        let k = curves.first().copied().unwrap_or(9) % 2;
        assert_eq!(curves, [k, 2 + k, 4 + k], "face {f}");
        let want = [5.0 * k as f64, 5.0 * k as f64 + 5.0];
        let got = corners_x(&record, f);
        assert!(
            (got[0] - want[0]).abs() < 1e-9 && (got[1] - want[1]).abs() < 1e-9,
            "face {f}, column {k}: x {got:?}"
        );
    }
}

/// Two sections, each a line drawn as two halves of equal length, lofted as one sheet: each
/// section's halves continue each other at the same speed, so the skin has no corner there and
/// the flat strip is one face, traced to all four curves.
#[test]
fn a_loft_sheet_face_across_columns_is_traced_to_each() {
    let sections = [two_lines([0.0; 3], 0.0), two_lines([0.0; 3], 5.0)];
    let (b, sources) = loft_sheet(&sections, 1).unwrap();
    assert_eq!(b.counts().unwrap(), [1, 4, 4]);
    assert_eq!(sources, Some(vec![vec![0, 1, 2, 3]]));
}

/// Squares of side 10, 8 and 10 at z = 0, 5 and 10 lofted as one sheet: a closed band of four
/// faces, one for each side and traced to that side in every section, joined at the four corner
/// seams; only the two squares are free.
#[test]
fn a_loft_sheet_through_closed_sections_is_a_band() {
    let sections = [
        square(0.0, 0.0, 10.0, 0.0),
        square(1.0, 1.0, 8.0, 5.0),
        square(0.0, 0.0, 10.0, 10.0),
    ];
    let (b, sources) = loft_sheet(&sections, 2).unwrap();
    assert_eq!(b.counts().unwrap(), [4, 12, 8]);
    let (record, _) = b.to_record().unwrap();
    assert_eq!(record.edges.iter().filter(|e| e.uses.len() == 1).count(), 8);
    let sources = sources.expect("traced");
    let mut sides: Vec<usize> = sources
        .iter()
        .map(|curves| {
            let k = curves.first().copied().unwrap_or(99);
            assert_eq!(curves, &[k, 4 + k, 8 + k], "one side in every section");
            k
        })
        .collect();
    sides.sort_unstable();
    assert_eq!(sides, [0, 1, 2, 3], "one side a face: {sources:?}");
}

/// A square lofted to the square turned 45° (its corners at the first one's side midpoints) is a
/// band of four twisted walls, each between one side of each section; every side is used once.
#[test]
fn a_loft_sheet_to_a_turned_square_is_a_band() {
    let diamond = polygon(&[
        [5.0, 0.0, 5.0],
        [10.0, 5.0, 5.0],
        [5.0, 10.0, 5.0],
        [0.0, 5.0, 5.0],
    ]);
    let (b, sources) = loft_sheet(&[square(0.0, 0.0, 10.0, 0.0), diamond], 1).unwrap();
    assert_eq!(b.counts().unwrap(), [4, 12, 8]);
    let sources = sources.expect("traced");
    for curves in &sources {
        assert!(
            curves.len() == 2 && curves[0] < 4 && curves[1] >= 4,
            "one side of each section: {sources:?}"
        );
    }
    let mut all = sources.concat();
    all.sort_unstable();
    assert_eq!(all, (0..8).collect::<Vec<_>>());
}

/// A loft sheet needs two sections or more, of one count of curves, and a degree of 1 to 3.
#[test]
fn a_loft_sheet_refuses_bad_sections() {
    let one = two_lines([0.0; 3], 0.0);
    let other = two_lines([1.0; 3], 5.0);
    assert_eq!(
        loft_sheet(std::slice::from_ref(&one), 1).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        loft_sheet(&[one.clone(), other[..1].to_vec()], 1).err(),
        Some(Error::Argument),
        "counts differ"
    );
    assert_eq!(
        loft_sheet(&[one.clone(), other.clone()], 0).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        loft_sheet(&[one.clone(), other.clone()], 4).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        loft_sheet(&[vec![], vec![]], 1).err(),
        Some(Error::Argument)
    );
}

/// A loft's end square to its section's plane, with `weight`.
fn square_end(weight: f64) -> LoftEnd {
    LoftEnd::Square { weight }
}

/// The valid solid `loft_with_ends` makes of `sections`.
fn lofted(sections: &[Vec<Curve>], ends: [LoftEnd; 2]) -> Body {
    rig_fixes();
    let b = loft_with_ends(sections, true, ends).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    b
}

/// Two equal squares, one above the other: every point's straight way to the next section is
/// square to the planes already, so with weight 1 at both ends each row's Hermite cubic is that
/// straight way and the loft is the box, its four flat walls planes.
#[test]
fn a_loft_square_at_both_ends_between_stacked_squares_is_the_box() {
    let sections = [square(0.0, 0.0, 10.0, 0.0), square(0.0, 0.0, 10.0, 7.0)];
    let b = lofted(&sections, [square_end(1.0); 2]);
    assert!(rel(b.volume(1e-8).unwrap(), 700.0) < 1e-9);
    assert_eq!(faces(&b), 6);
    let m = b.mass_properties(1e-8).unwrap();
    assert!(rel(m.area, 2.0 * 100.0 + 4.0 * 70.0) < 1e-9, "{}", m.area);
}

/// The square of side 10 at z = 0 lofted to its copy moved 6 along x at z = 8, both ends
/// square to their planes: every row's chord is 10, so with the weights 1.2 and 0.6 a corner's
/// row is the cubic Bezier with z = 0, 4, 6, 8 and x = 0, 0, 6, 6 (the legs at the ends are
/// weight * chord / 3 along z). z rises all the way (z' = 12 - 12 v + 6 v^2 > 0), so every
/// section at a height is the square moved by x(z): the volume is the box's, 800, and the
/// centre's x is 5 + (the integral of x z' dv) / 8 with x = 6 (3 v^2 - 2 v^3), which is
/// 6 (12 - 15 + 8.4 - 2) / 8 = 2.55. A free loft is the sheared box, its centre at x = 8.
#[test]
fn a_loft_square_at_both_ends_between_shifted_squares_has_its_closed_form_centre() {
    let sections = [square(0.0, 0.0, 10.0, 0.0), square(6.0, 0.0, 10.0, 8.0)];
    let b = lofted(&sections, [square_end(1.2), square_end(0.6)]);
    let m = b.mass_properties(1e-8).unwrap();
    assert!(rel(m.volume, 800.0) < 1e-8, "{}", m.volume);
    for (got, want) in m.centroid.into_iter().zip([7.55, 5.0, 4.0]) {
        assert!((got - want).abs() < 1e-7, "{:?}", m.centroid);
    }
    // The two walls across y are flat (planes); the two along it are the curved ones.
    assert_eq!(faces(&b), 6);
    // One square end: a quadratic row, z = 0, w c / 2, 8 and x = 0, 0, 6, so with weight 0.8
    // z' = 8 and x = 6 v^2: the centre's x is 5 + 6 / 3.
    let b = lofted(&sections, [square_end(0.8), LoftEnd::Free]);
    let m = b.mass_properties(1e-8).unwrap();
    assert!(rel(m.volume, 800.0) < 1e-8, "{}", m.volume);
    assert!((m.centroid[0] - 7.0).abs() < 1e-7, "{:?}", m.centroid);
    let b = lofted(&sections, [LoftEnd::Free, square_end(0.8)]);
    let m = b.mass_properties(1e-8).unwrap();
    assert!((m.centroid[0] - 9.0).abs() < 1e-7, "{:?}", m.centroid);
}

/// Three equal circles on one axis, the first end square with weight 1: the rows through the
/// three sections at their chord-length parameters with the straight way's speed at the start
/// are the straight rows, so the loft is the cylinder.
#[test]
fn a_loft_square_at_one_end_through_three_stacked_circles_is_the_cylinder() {
    let ring = |z: f64| vec![circle([0.0, 0.0, z], Z, [1.0, 0.0, 0.0], 5.0)];
    let b = lofted(
        &[ring(0.0), ring(5.0), ring(12.0)],
        [square_end(1.0), LoftEnd::Free],
    );
    assert!(rel(b.volume(1e-8).unwrap(), 25.0 * PI * 12.0) < 1e-9);
    let b = lofted(&[ring(0.0), ring(5.0), ring(12.0)], [square_end(1.0); 2]);
    assert!(rel(b.volume(1e-8).unwrap(), 25.0 * PI * 12.0) < 1e-9);
}

/// Sections with a hole: the outline and the hole leave square to the plane alike, so two
/// equal stacked frames make the box with its passage.
#[test]
fn a_loft_square_at_both_ends_keeps_a_section_s_hole() {
    let frame = |z: f64| {
        let mut loops = square(0.0, 0.0, 10.0, z);
        loops.extend(polygon(&[
            [3.0, 3.0, z],
            [3.0, 7.0, z],
            [7.0, 7.0, z],
            [7.0, 3.0, z],
        ]));
        loops
    };
    let b = lofted(&[frame(0.0), frame(7.0)], [square_end(1.0); 2]);
    assert!(rel(b.volume(1e-8).unwrap(), (100.0 - 16.0) * 7.0) < 1e-9);
    assert_eq!(faces(&b), 10);
}

/// A larger weight carries the walls further out square to the ends before they turn: between
/// a small and a large stacked square the volume falls with the first end's weight (the walls
/// stay at the small square for longer) and grows with the last's, and each is the closed form
/// of its rows. A corner's row runs from the
/// half-side 5 to 10 over the height 12: with the weights a and b its half-side over v is the
/// cubic 5 + 5 (3 v^2 - 2 v^3) (the legs along z move no x) and its height the Bezier
/// 0, a c / 3, 12 - b c / 3, 12 with c = sqrt(12^2 + 2 * 5^2) the corners' chord; the volume is
/// the integral of (2 half-side)^2 z' dv.
#[test]
fn a_loft_s_end_weights_shape_it_as_their_closed_form() {
    let centred = |half: f64, z: f64| square(-half, -half, 2.0 * half, z);
    let sections = [centred(5.0, 0.0), centred(10.0, 12.0)];
    let c = (144.0_f64 + 50.0).sqrt();
    let closed_form = |a: f64, b: f64| {
        let (z1, z2) = (a * c / 3.0, 12.0 - b * c / 3.0);
        let n = 4000;
        let f = |v: f64| {
            let half = 5.0 + 5.0 * (3.0 * v * v - 2.0 * v * v * v);
            let dz = 3.0
                * ((1.0 - v) * (1.0 - v) * z1
                    + 2.0 * (1.0 - v) * v * (z2 - z1)
                    + v * v * (12.0 - z2));
            4.0 * half * half * dz
        };
        // Simpson's rule: the integrand is a polynomial of degree 8, far inside its reach.
        let sum: f64 = (0..=n)
            .map(|i| {
                let w = if i == 0 || i == n {
                    1.0
                } else if i % 2 == 1 {
                    4.0
                } else {
                    2.0
                };
                w * f(f64::from(i) / f64::from(n))
            })
            .sum();
        sum / (3.0 * f64::from(n))
    };
    let volume = |a: f64, b: f64| {
        lofted(&sections, [square_end(a), square_end(b)])
            .volume(1e-8)
            .unwrap()
    };
    for (a, b) in [(1.0, 1.0), (0.4, 1.0), (1.8, 1.0), (1.0, 0.4), (1.0, 1.8)] {
        assert!(rel(volume(a, b), closed_form(a, b)) < 1e-8, "{a} {b}");
    }
    assert!(volume(0.4, 1.0) > volume(1.0, 1.0) && volume(1.0, 1.0) > volume(1.8, 1.0));
    assert!(volume(1.0, 0.4) < volume(1.0, 1.0) && volume(1.0, 1.0) < volume(1.0, 1.8));
}

/// What `loft_with_ends` refuses: no square end, a weight that is not a number above 0, and an
/// end with no side to leave to (the next section in the end section's own plane).
#[test]
fn a_loft_s_hostile_ends_are_refused() {
    rig_fixes();
    let stacked = [square(0.0, 0.0, 10.0, 0.0), square(0.0, 0.0, 10.0, 7.0)];
    let refused = |sections: &[Vec<Curve>], ends| loft_with_ends(sections, true, ends).is_err();
    assert!(matches!(
        loft_with_ends(&stacked, true, [LoftEnd::Free; 2]),
        Err(Error::Argument)
    ));
    for weight in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            loft_with_ends(&stacked, true, [square_end(weight), LoftEnd::Free]),
            Err(Error::Argument)
        ));
        assert!(refused(&stacked, [LoftEnd::Free, square_end(weight)]));
    }
    assert!(refused(&stacked[..1], [square_end(1.0); 2]));
    // Side by side in one plane: neither end has a side to leave to.
    let beside = [square(0.0, 0.0, 10.0, 0.0), square(20.0, 0.0, 10.0, 0.0)];
    assert!(refused(&beside, [square_end(1.0), LoftEnd::Free]));
    assert!(refused(&beside, [LoftEnd::Free, square_end(1.0)]));
}

/// A loft to a point through a station that bulges out (the base square again, half way up):
/// a closed solid fuller than the pyramid (200) and inside the prism over the base (600). Rows of
/// different counts, an apex in the base's plane, a single row, and parameters that do not run
/// from 0 to 1 by increasing steps, one a row, are refused.
#[test]
fn loft_to_point_through_a_bulge_and_its_refusals() {
    rig_fixes();
    let base = square(0.0, 0.0, 10.0, 0.0);
    let apex = [5.0, 5.0, 6.0];
    let half = [0.0, 0.5, 1.0];
    let bulge = [base.clone(), square(0.0, 0.0, 10.0, 3.0)];
    let v = loft_to_point_through(&bulge, apex, &half)
        .unwrap()
        .volume(1e-8)
        .unwrap();
    assert!(v > 200.0 && v < 600.0, "volume {v}");
    let fewer = vec![
        line([2.5, 2.5, 3.0], [7.5, 7.5, 3.0]),
        line([7.5, 7.5, 3.0], [2.5, 2.5, 3.0]),
    ];
    let refused = |sections: &[Vec<Curve>], apex: [f64; 3], params: &[f64]| {
        loft_to_point_through(sections, apex, params).err() == Some(Error::Argument)
    };
    assert!(refused(&[base.clone(), fewer], apex, &half), "counts");
    assert!(
        refused(&bulge, [5.0, 5.0, 0.0], &half),
        "apex in the base's plane"
    );
    assert!(refused(&[base], apex, &[0.0, 1.0]), "one row");
    for params in [
        &[0.0, 1.0][..],
        &[0.0, 0.5, 0.9],
        &[0.1, 0.5, 1.0],
        &[0.0, 0.0, 1.0],
        &[0.0, f64::NAN, 1.0],
    ] {
        assert!(refused(&bulge, apex, params), "{params:?}");
    }
}

/// A square whose corners take different weights from the two lines that meet there (each line
/// weight 1 at its start and 2 at its end), through the same square halved at half height, to
/// the apex: neighbouring walls meet on the same curves because each corner keeps one ratio of
/// weights through the rows, so the walls stitch into the plain pyramid, A h / 3 = 200.
#[test]
fn loft_to_point_through_corners_of_different_weights_stitch() {
    rig_fixes();
    let weighted = |pts: [[f64; 3]; 4]| -> Vec<Curve> {
        (0..4)
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % 4]);
                Curve::Nurbs {
                    degree: 1,
                    points: vec![[a[0], a[1], a[2], 1.0], [b[0], b[1], b[2], 2.0]],
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                }
            })
            .collect()
    };
    let base = weighted([
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ]);
    let half = weighted([
        [2.5, 2.5, 3.0],
        [7.5, 2.5, 3.0],
        [7.5, 7.5, 3.0],
        [2.5, 7.5, 3.0],
    ]);
    let b = loft_to_point_through(&[base, half], [5.0, 5.0, 6.0], &[0.0, 0.5, 1.0]).unwrap();
    assert!(
        b.check(CheckLevel::All).unwrap().is_valid(),
        "a closed valid solid"
    );
    let v = b.volume(1e-8).unwrap();
    assert!(rel(v, 200.0) < 1e-9, "volume {v} vs 200");
}

/// A circle swept along a line, at a small radius and far from the origin: the record it writes
/// opens again (tangency-labs/solidcore#3377, where a cap plane was written with axes off square
/// by 1e-15 to 1e-14 and `from_record` refused it).
#[test]
fn a_swept_circle_s_record_opens_again() {
    let (s, c) = 15f64.to_radians().sin_cos();
    let mut failed = Vec::new();
    for (r, start, dir) in [
        (1.0, [0.0, 0.0, 0.0], [c, s, 0.0]),
        (0.01, [0.0, 0.0, 0.0], [c, s, 0.0]),
        (1.0, [10010.0, 9997.58, 10.0], [0.0, 1.0, 0.0]),
    ] {
        let end = [0, 1, 2].map(|i| start[i] + 10.0 * dir[i]);
        let circle = Curve::Arc {
            center: start,
            axis: dir,
            reference: [0.0, 0.0, 1.0],
            radius: r,
            start: 0.0,
            end: TAU,
        };
        let (body, _, _) =
            sweep_placed(&[circle], &[line(start, end)], SweepFrame::Follow, true).unwrap();
        let (record, _) = body.to_record().unwrap();
        let again = Body::from_record(&record, true).and_then(|(back, _)| {
            let (second, _) = back.to_record()?;
            Body::from_record(&second, true).map(|_| second)
        });
        match again {
            Ok(second) if second == record => {}
            Ok(_) => failed.push(format!("r {r} from {start:?}: the second record differs")),
            Err(e) => failed.push(format!("r {r} from {start:?}: {e:?}")),
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A swept circle saved before #21509 squared the record's plane axes: its cap planes' v axes
/// are off square by about 1.36e-14 times their size (tangency-labs/solidcraft#21752). The
/// record opens, as the same solid, and the record it writes again opens too.
#[test]
fn a_swept_circle_saved_with_its_caps_off_square_opens() {
    use solidcore_record::brep::Surface;
    let start = [10010.0, 9997.58, 10.0];
    let circle = Curve::Arc {
        center: start,
        axis: [0.0, 1.0, 0.0],
        reference: Z,
        radius: 1.0,
        start: 0.0,
        end: TAU,
    };
    let end = [start[0], start[1] + 10.0, start[2]];
    let (body, _, _) =
        sweep_placed(&[circle], &[line(start, end)], SweepFrame::Follow, true).unwrap();
    let (mut record, _) = body.to_record().unwrap();
    let mut skewed = 0;
    for face in &mut record.faces {
        if let Surface::Plane { u_axis, v_axis, .. } = &mut face.surface {
            let size = (0..3).map(|i| v_axis[i] * v_axis[i]).sum::<f64>().sqrt();
            for i in 0..3 {
                v_axis[i] += 1.36e-14 * size * u_axis[i];
            }
            skewed += 1;
        }
    }
    assert_eq!(skewed, 2, "the two caps");
    let (back, _) = Body::from_record(&record, true).unwrap();
    assert!(back.check(CheckLevel::All).unwrap().is_valid());
    let (again, _) = back.to_record().unwrap();
    assert_eq!(
        (again.faces.len(), again.edges.len(), again.vertices.len()),
        (record.faces.len(), record.edges.len(), record.vertices.len())
    );
    Body::from_record(&again, true).unwrap();
    let (v, w) = (body.volume(1e-9).unwrap(), back.volume(1e-9).unwrap());
    assert!(rel(v, w) < 1e-9, "volume {w} vs {v}");
}
