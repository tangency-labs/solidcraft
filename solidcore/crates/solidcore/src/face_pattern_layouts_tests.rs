use std::f64::consts::{FRAC_PI_2, PI, TAU};

use super::*;
use crate::eval::query::{Edge, Face as RecordFace, Loop, Record, Use};
use crate::eval::{Curve, Curve2, Frame, Surface};
use crate::face_pattern::{
    edge_points_across, grid_stations, grid_stations_across, random_stations_across, Fit, Grid,
    Noise, Random,
};

const R: f64 = 10.0;

fn line2(a: [f64; 2], b: [f64; 2]) -> Curve2 {
    Curve2::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![a, b],
    }
}

fn line_edge(a: [f64; 2], b: [f64; 2], faces: Vec<usize>) -> Edge {
    Edge {
        curve: Some(Curve::Line {
            start: [a[0], a[1], 0.0],
            end: [b[0], b[1], 0.0],
        }),
        interval: [0.0, 1.0],
        faces,
    }
}

fn plane() -> Surface {
    Surface::Plane {
        origin: [0.0; 3],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
    }
}

fn plane_face(loops: Vec<Loop>) -> RecordFace {
    RecordFace {
        parameters: Some(plane()),
        surface: Some(plane()),
        domain: [[-1000.0, 1000.0], [-1000.0, 1000.0]],
        forward: Some(true),
        loops,
    }
}

/// A use of `edge` from `a` to `b` in the plane's parameters (its edge running the same way when
/// `forward`).
fn use_of(edge: usize, forward: bool, a: [f64; 2], b: [f64; 2]) -> Use {
    Use {
        edge,
        forward,
        pcurve: line2(a, b),
    }
}

/// A plate 100 × 60 in the plane z = 0, (u, v) = (x, y), as one face.
fn plate() -> Query {
    let c = [[0.0, 0.0], [100.0, 0.0], [100.0, 60.0], [0.0, 60.0]];
    let edges = (0..4)
        .map(|i| line_edge(c[i], c[(i + 1) % 4], vec![0]))
        .collect();
    let uses = (0..4)
        .map(|i| use_of(i, true, c[i], c[(i + 1) % 4]))
        .collect();
    Query::new(Record {
        edges,
        faces: vec![plane_face(vec![Loop::Edges(uses)])],
    })
}

/// The same plate as two faces, x 0 to 50 and 50 to 100, sharing the edge at x = 50 (edge 6).
fn halves() -> Query {
    let p = |x: f64, y: f64| [x, y];
    let edges = vec![
        line_edge(p(0.0, 0.0), p(50.0, 0.0), vec![0]),
        line_edge(p(50.0, 0.0), p(100.0, 0.0), vec![1]),
        line_edge(p(100.0, 0.0), p(100.0, 60.0), vec![1]),
        line_edge(p(100.0, 60.0), p(50.0, 60.0), vec![1]),
        line_edge(p(50.0, 60.0), p(0.0, 60.0), vec![0]),
        line_edge(p(0.0, 60.0), p(0.0, 0.0), vec![0]),
        line_edge(p(50.0, 0.0), p(50.0, 60.0), vec![0, 1]),
    ];
    let left = vec![
        use_of(0, true, p(0.0, 0.0), p(50.0, 0.0)),
        use_of(6, true, p(50.0, 0.0), p(50.0, 60.0)),
        use_of(4, true, p(50.0, 60.0), p(0.0, 60.0)),
        use_of(5, true, p(0.0, 60.0), p(0.0, 0.0)),
    ];
    let right = vec![
        use_of(1, true, p(50.0, 0.0), p(100.0, 0.0)),
        use_of(2, true, p(100.0, 0.0), p(100.0, 60.0)),
        use_of(3, true, p(100.0, 60.0), p(50.0, 60.0)),
        use_of(6, false, p(50.0, 60.0), p(50.0, 0.0)),
    ];
    Query::new(Record {
        edges,
        faces: vec![
            plane_face(vec![Loop::Edges(left)]),
            plane_face(vec![Loop::Edges(right)]),
        ],
    })
}

/// A whole sphere of radius 10 about the origin, its pole on z, as a sphere's record is: one
/// face, its only loop its seam (a meridian, from pole to pole) used twice, at u = 0 and u = 2π.
fn ball() -> Query {
    let s = Surface::Sphere {
        frame: Frame {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        },
        radius: R,
    };
    let (s0, n0) = (-FRAC_PI_2, FRAC_PI_2);
    let seam = Edge {
        curve: Some(Curve::Circle {
            frame: Frame {
                origin: [0.0; 3],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 0.0, 1.0],
            },
            radius: R,
        }),
        interval: [s0, n0],
        faces: vec![0, 0],
    };
    let uses = vec![
        Use {
            edge: 0,
            forward: false,
            pcurve: line2([0.0, s0], [0.0, n0]),
        },
        Use {
            edge: 0,
            forward: true,
            pcurve: line2([TAU, s0], [TAU, n0]),
        },
    ];
    Query::new(Record {
        edges: vec![seam],
        faces: vec![RecordFace {
            parameters: Some(s.clone()),
            surface: Some(s),
            domain: [[0.0, TAU], [s0, n0]],
            forward: Some(true),
            loops: vec![Loop::Edges(uses)],
        }],
    })
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

/// The distance from each station to its nearest neighbour.
fn nearest(stations: &[Station]) -> Vec<f64> {
    stations
        .iter()
        .enumerate()
        .map(|(i, s)| {
            stations
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, t)| s.point.dist(t.point))
                .fold(f64::INFINITY, f64::min)
        })
        .collect()
}

fn spread(stations: &[Station]) -> (f64, f64) {
    nearest(stations)
        .into_iter()
        .fold((f64::INFINITY, 0.0), |(lo, hi), d| (lo.min(d), hi.max(d)))
}

/// The spacing an even (hexagonal) layout of `n` on `area` has.
fn hexagonal(area: f64, n: usize) -> f64 {
    (2.0 * area / (3.0_f64.sqrt() * n as f64)).sqrt()
}

/// On the flat plate a spacing of 10 is a 10 × 6 grid of cells 10 wide, a station at each centre.
#[test]
fn spacing_on_a_plate_is_a_grid_of_cells_that_size() {
    let q = plate();
    let spacing = Spacing {
        spacing: 10.0,
        row_spacing: 10.0,
        staggered: false,
    };
    let got = spaced_stations(&q, &[0], &spacing, None, 0.0).unwrap();
    assert_eq!(got.len(), 60);
    for s in &got {
        let (x, y) = (s.point.x - 5.0, s.point.y - 5.0);
        assert!(
            close(x, (x / 10.0).round() * 10.0) && close(y, (y / 10.0).round() * 10.0),
            "{s:?}"
        );
    }
}

/// On a sphere the spaced rows are its circles of latitude, a row spacing apart along the
/// meridian, each with as many stations as fit round it: fewer towards the poles, none crowded.
#[test]
fn spacing_on_a_sphere_rings_it_with_fewer_towards_the_poles() {
    let q = ball();
    let s = 2.0;
    let spacing = Spacing {
        spacing: s,
        row_spacing: s,
        staggered: false,
    };
    let got = spaced_stations(&q, &[0], &spacing, None, 0.0).unwrap();
    // Rows a meridian's length (π R) divided into round(π R / s) apart; each row's count the
    // rounded circumference over the spacing.
    let rows = (PI * R / s).round();
    let mut want = 0.0;
    for r in 0..rows as usize {
        let lat = -FRAC_PI_2 + (r as f64 + 0.5) * PI / rows;
        want += (TAU * R * lat.cos() / s).round();
    }
    assert_eq!(got.len() as f64, want);
    assert!(got.iter().all(|t| close(t.point.len(), R)));
    // Every station's nearest neighbour is about the spacing away: none crowded at a pole.
    let (lo, hi) = spread(&got);
    assert!(lo > 0.8 * s && hi < 1.2 * s, "{lo} {hi}");
    // The plain grid of as many cells crowds them there.
    let n = (got.len() as f64).sqrt().round() as usize;
    let grid = Grid {
        count_u: n,
        count_v: n,
        angle: 0.0,
        staggered: false,
    };
    let (lo, _) = spread(&grid_stations(&q, 0, &grid, 0.0).unwrap());
    assert!(lo < 0.5 * s, "{lo}");
}

/// The spiral puts every one of its stations on a whole sphere, evenly: each one's nearest
/// neighbour within a third of the even spacing.
#[test]
fn a_spiral_spreads_a_sphere_evenly() {
    let q = ball();
    let got = spiral_stations(&q, &[0], 300, 0.0).unwrap();
    assert_eq!(got.len(), 300);
    let h = hexagonal(2.0 * TAU * R * R, 300);
    let (lo, hi) = spread(&got);
    assert!(lo > 0.65 * h && hi < 1.35 * h, "{lo} {hi} {h}");
    // Normals out of the ball, as its shell uses it.
    assert!(got.iter().all(|t| close(t.normal.dot(t.point) / R, 1.0)));
}

/// How evenly stations are spread: the nearest of the nearest-neighbour distances, and those
/// distances' spread (standard deviation over mean).
fn evenness(stations: &[Station]) -> (f64, f64) {
    let d = nearest(stations);
    let n = d.len() as f64;
    let mean = d.iter().sum::<f64>() / n;
    let sd = (d.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
    (d.iter().copied().fold(f64::INFINITY, f64::min), sd / mean)
}

/// Round a hole the spiral's rows bunch up; the even fill relaxes them: its nearest pair farther
/// apart and its spacings less spread than the spiral's, every station on the plate, none in
/// the hole.
#[test]
fn an_even_fill_spreads_round_a_hole() {
    let q = holed();
    let fill = fill_stations(&q, &[0], 60, 0.0).unwrap();
    let spiral = spiral_stations(&q, &[0], 60, 0.0).unwrap();
    assert_eq!(fill.len(), 60);
    let ((lo, spread), (spiral_lo, spiral_spread)) = (evenness(&fill), evenness(&spiral));
    assert!(
        lo > 1.5 * spiral_lo && spread < 0.5 * spiral_spread,
        "{lo} {spread} {spiral_lo} {spiral_spread}"
    );
    let (inside, hole) = (
        |s: &Station| (0.0..=100.0).contains(&s.point.x) && (0.0..=60.0).contains(&s.point.y),
        |s: &Station| (35.0..=65.0).contains(&s.point.x) && (15.0..=45.0).contains(&s.point.y),
    );
    assert!(fill.iter().all(|s| inside(s) && !hole(s)));
    // With a margin of 5 every station keeps it, round the hole too: the samples it moves among
    // keep it.
    let kept = fill_stations(&q, &[0], 60, 5.0).unwrap();
    assert!(
        kept.len() == 60 && kept.iter().all(|s| s.margin >= 5.0),
        "{}",
        kept.len()
    );
}

/// On an L the fill is more even than the spiral too.
#[test]
fn an_even_fill_spreads_an_l_evenly() {
    let q = ell();
    let (lo, spread) = evenness(&fill_stations(&q, &[0], 60, 0.0).unwrap());
    let (spiral_lo, spiral_spread) = evenness(&spiral_stations(&q, &[0], 60, 0.0).unwrap());
    assert!(
        lo > spiral_lo && spread < spiral_spread,
        "{lo} {spread} {spiral_lo} {spiral_spread}"
    );
}

/// A geodesic sphere of frequency 1 is the icosahedron: 12 stations, each 1.0515 R from its five
/// nearest; at frequency 2, 42, the poles among them with the sphere's own normal there.
#[test]
fn a_geodesic_layout_is_the_subdivided_icosahedron() {
    let q = ball();
    let got = geodesic_stations(&q, &[0], 1, 0.0, 0.0).unwrap();
    assert_eq!(got.len(), 12);
    let edge = R * 4.0 / (10.0 + 2.0 * 5.0_f64.sqrt()).sqrt();
    for d in nearest(&got) {
        assert!((d - edge).abs() < 1e-9, "{d} {edge}");
    }
    let got = geodesic_stations(&q, &[0], 2, 0.0, 0.0).unwrap();
    assert_eq!(got.len(), 42);
    let pole = got.iter().find(|s| close(s.point.z, R)).unwrap();
    assert!(
        close(pole.normal.z, 1.0) && close(pole.along.x, 1.0),
        "{pole:?}"
    );
    assert_eq!(
        geodesic_stations(&q, &[0], MAX_GEODESIC_FREQUENCY, 0.0, 0.0)
            .unwrap()
            .len(),
        10 * 81 + 2
    );
}

/// A geodesic layout needs a sphere, and a frequency from 1 to 9.
#[test]
fn a_geodesic_layout_refuses_a_plane_and_frequencies_out_of_range() {
    assert!(geodesic_stations(&plate(), &[0], 2, 0.0, 0.0).is_err());
    assert!(geodesic_stations(&ball(), &[0], 0, 0.0, 0.0).is_err());
    assert!(geodesic_stations(&ball(), &[0], 10, 0.0, 0.0).is_err());
}

/// Two faces sharing an edge are one region: its margin is measured to the plate's outline, not
/// to the shared edge, so the stations beside it stay; each face alone drops them.
#[test]
fn faces_sharing_an_edge_measure_their_margin_to_the_outline() {
    let q = halves();
    let one = Grid {
        count_u: 2,
        count_v: 1,
        angle: 0.0,
        staggered: false,
    };
    // Stations at x = 12.5 and 37.5 on the left, 62.5 and 87.5 on the right, all at y = 30.
    assert!(grid_stations(&q, 0, &one, 20.0).unwrap().is_empty());
    let got = grid_stations_across(&q, &[0, 1], &one, None, 20.0).unwrap();
    let mut xs: Vec<f64> = got.iter().map(|s| s.point.x).collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!(xs, vec![37.5, 62.5]);
    // The outline has six edges; the shared one is not among them.
    assert_eq!(edge_points_across(&q, &[0, 1], 5.0).unwrap().len(), 6);
    assert!(grid_stations_across(&q, &[0, 0], &one, None, 0.0).is_err());
    assert!(grid_stations_across(&q, &[], &one, None, 0.0).is_err());
}

/// Random, spiral and fill layouts share their count over both halves by area, and the spaced
/// one lays rows on each.
#[test]
fn layouts_span_both_faces() {
    let q = halves();
    let r = Random {
        count: 200,
        seed: 3,
        noise: Noise::White,
    };
    let got = random_stations_across(&q, &[0, 1], &r, 0.0, &mut |_| true).unwrap();
    let left = got.iter().filter(|s| s.point.x < 50.0).count();
    assert_eq!(got.len(), 200);
    assert!((70..=130).contains(&left), "{left}");
    let got = spiral_stations(&q, &[0, 1], 100, 0.0).unwrap();
    assert_eq!(got.iter().filter(|s| s.point.x < 50.0).count(), 50);
    // The fill moves across the shared edge as if it were not there.
    let got = fill_stations(&q, &[0, 1], 60, 0.0).unwrap();
    assert_eq!(got.len(), 60);
    let (lo, _) = spread(&got);
    assert!(lo > 0.75 * hexagonal(6000.0, 60), "{lo}");
    let spacing = Spacing {
        spacing: 10.0,
        row_spacing: 10.0,
        staggered: true,
    };
    // Staggered, each half's odd rows stand between the even rows' cells: 5 + 4 a row pair.
    let got = spaced_stations(&q, &[0, 1], &spacing, None, 0.0).unwrap();
    assert_eq!(got.len(), 2 * (3 * 5 + 3 * 4));
}

/// Out of range inputs are refused, never a panic.
#[test]
fn layouts_refuse_bad_input() {
    let q = plate();
    let bad = Spacing {
        spacing: 0.0,
        row_spacing: 1.0,
        staggered: false,
    };
    assert!(spaced_stations(&q, &[0], &bad, None, 0.0).is_err());
    let tiny = Spacing {
        spacing: 0.01,
        row_spacing: 0.01,
        staggered: false,
    };
    assert!(spaced_stations(&q, &[0], &tiny, None, 0.0).is_err());
    assert!(spiral_stations(&q, &[0], 1001, 0.0).is_err());
    assert!(fill_stations(&q, &[0], 10, -1.0).is_err());
    assert!(spiral_stations(&q, &[7], 10, 0.0).is_err());
    assert!(spiral_stations(&q, &[0], 0, 0.0).unwrap().is_empty());
}

/// A plate 100 × 60 with a square hole 30 wide in its middle.
fn holed() -> Query {
    let c = [[0.0, 0.0], [100.0, 0.0], [100.0, 60.0], [0.0, 60.0]];
    let h = [[35.0, 15.0], [35.0, 45.0], [65.0, 45.0], [65.0, 15.0]];
    let mut edges: Vec<Edge> = (0..4)
        .map(|i| line_edge(c[i], c[(i + 1) % 4], vec![0]))
        .collect();
    edges.extend((0..4).map(|i| line_edge(h[i], h[(i + 1) % 4], vec![0])));
    let outer = (0..4)
        .map(|i| use_of(i, true, c[i], c[(i + 1) % 4]))
        .collect();
    let inner = (0..4)
        .map(|i| use_of(4 + i, true, h[i], h[(i + 1) % 4]))
        .collect();
    Query::new(Record {
        edges,
        faces: vec![plane_face(vec![Loop::Edges(outer), Loop::Edges(inner)])],
    })
}

/// An L: 100 × 60 less its top right 60 × 30.
fn ell() -> Query {
    let c = [
        [0.0, 0.0],
        [100.0, 0.0],
        [100.0, 30.0],
        [40.0, 30.0],
        [40.0, 60.0],
        [0.0, 60.0],
    ];
    let edges = (0..6)
        .map(|i| line_edge(c[i], c[(i + 1) % 6], vec![0]))
        .collect();
    let uses = (0..6)
        .map(|i| use_of(i, true, c[i], c[(i + 1) % 6]))
        .collect();
    Query::new(Record {
        edges,
        faces: vec![plane_face(vec![Loop::Edges(uses)])],
    })
}

fn xs_and_ys(stations: &[Station]) -> (Vec<f64>, Vec<f64>) {
    let mut xs: Vec<f64> = stations
        .iter()
        .map(|s| (s.point.x * 1e6).round() / 1e6)
        .collect();
    let mut ys: Vec<f64> = stations
        .iter()
        .map(|s| (s.point.y * 1e6).round() / 1e6)
        .collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    (xs, ys)
}

/// Fitted, a grid's outer stations stand the fit in from the face's edges, the rest evenly
/// between: 5 × 3 on the plate, 5 in each way, at x = 5 … 95 and y = 5, 30, 55. One station each
/// way stands in the middle; a fit wider than the face leaves no room for any.
#[test]
fn a_fitted_grid_runs_from_edge_to_edge() {
    let q = plate();
    let g = Grid {
        count_u: 5,
        count_v: 3,
        angle: 0.0,
        staggered: false,
    };
    let fit = Fit { u: 5.0, v: 5.0 };
    let got = grid_stations_across(&q, &[0], &g, Some(&fit), 0.0).unwrap();
    assert_eq!(got.len(), 15);
    assert_eq!(
        xs_and_ys(&got),
        (vec![5.0, 27.5, 50.0, 72.5, 95.0], vec![5.0, 30.0, 55.0])
    );
    let one = Grid {
        count_u: 1,
        count_v: 1,
        ..g
    };
    let got = grid_stations_across(&q, &[0], &one, Some(&fit), 0.0).unwrap();
    assert_eq!(xs_and_ys(&got), (vec![50.0], vec![30.0]));
    let wide = Fit { u: 60.0, v: 5.0 };
    assert!(grid_stations_across(&q, &[0], &g, Some(&wide), 0.0)
        .unwrap()
        .is_empty());
    // Staggered, the odd columns sit half a step along: between the even columns' rows, one
    // fewer.
    let staggered = Grid {
        staggered: true,
        ..g
    };
    let got = grid_stations_across(&q, &[0], &staggered, Some(&fit), 0.0).unwrap();
    assert_eq!(got.len(), 3 * 3 + 2 * 2);
    assert!(grid_stations_across(&q, &[0], &g, Some(&Fit { u: -1.0, v: 0.0 }), 0.0).is_err());
}

/// Fitted, spaced rows run from the fit in from one edge to the same in from the other, as many
/// as come nearest the spacing: on the plate at 10 apart, 5 in, rows at y = 5 … 55 and stations
/// at x = 5 … 95.
#[test]
fn fitted_spacing_runs_from_edge_to_edge() {
    let q = plate();
    let spacing = Spacing {
        spacing: 10.0,
        row_spacing: 10.0,
        staggered: false,
    };
    let fit = Fit { u: 5.0, v: 5.0 };
    let got = spaced_stations(&q, &[0], &spacing, Some(&fit), 0.0).unwrap();
    let (xs, ys) = xs_and_ys(&got);
    assert_eq!(got.len(), 60);
    assert_eq!(
        (xs.first(), xs.last(), xs.len()),
        (Some(&5.0), Some(&95.0), 10)
    );
    assert_eq!(
        (ys.first(), ys.last(), ys.len()),
        (Some(&5.0), Some(&55.0), 6)
    );
    // Staggered, the odd rows stand between the even rows' stations.
    let staggered = Spacing {
        staggered: true,
        ..spacing
    };
    assert_eq!(
        spaced_stations(&q, &[0], &staggered, Some(&fit), 0.0)
            .unwrap()
            .len(),
        3 * 10 + 3 * 9
    );
}

/// A sphere's poles are no edges: fitted, its rows still keep half a row's spacing from them, and
/// its grid, round and from pole to pole, is the grid of cells.
#[test]
fn a_fit_keeps_half_a_step_from_a_pole() {
    let q = ball();
    let spacing = Spacing {
        spacing: 2.0,
        row_spacing: 2.0,
        staggered: false,
    };
    let fit = Fit { u: 1.0, v: 1.0 };
    let got = spaced_stations(&q, &[0], &spacing, Some(&fit), 0.0).unwrap();
    let top = got
        .iter()
        .map(|s| s.point.z)
        .fold(f64::NEG_INFINITY, f64::max);
    // The top row half a spacing of arc from the pole.
    assert!(close(top, R * (FRAC_PI_2 - 1.0 / R).sin()), "{top}");
    let (lo, hi) = spread(&got);
    assert!(lo > 1.6 && hi < 2.4, "{lo} {hi}");
    let g = Grid {
        count_u: 8,
        count_v: 6,
        angle: 0.0,
        staggered: false,
    };
    let fitted = grid_stations_across(&q, &[0], &g, Some(&fit), 0.0).unwrap();
    let cells = grid_stations_across(&q, &[0], &g, None, 0.0).unwrap();
    assert_eq!(fitted.len(), cells.len());
    for (a, b) in fitted.iter().zip(&cells) {
        assert!(a.point.dist(b.point) < 1e-9, "{a:?} {b:?}");
    }
}

/// Along the plate's outline every frame points off the plate, square to its edge in its plane,
/// with the plate's normal; on the two halves, the shared edge has none.
#[test]
fn edge_frames_point_off_the_faces() {
    use crate::face_pattern::edge_frames_across;
    for (q, faces) in [(plate(), vec![0]), (halves(), vec![0, 1])] {
        let lines = edge_frames_across(&q, &faces, 5.0).unwrap();
        assert_eq!(lines.len(), if faces.len() == 1 { 4 } else { 6 });
        for f in lines.iter().flatten() {
            let back = f.point - f.out;
            let ahead = f.point + f.out;
            let on = |p: Vec3| (0.0..=100.0).contains(&p.x) && (0.0..=60.0).contains(&p.y);
            assert!(
                close(f.out.len(), 1.0) && close(f.out.z, 0.0) && close(f.normal.z, 1.0),
                "{f:?}"
            );
            // Off the plate ahead, and on it a step back (a corner's step back can be off it too).
            assert!(!on(ahead), "{f:?}");
            let corner =
                (f.point.x == 0.0 || f.point.x == 100.0) && (f.point.y == 0.0 || f.point.y == 60.0);
            assert!(corner || on(back), "{f:?}");
        }
    }
}
