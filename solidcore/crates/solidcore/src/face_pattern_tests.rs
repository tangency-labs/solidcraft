use std::f64::consts::FRAC_PI_2;

use super::*;
use crate::eval::query::{Edge, Face as RecordFace, Record, Use};
use crate::eval::{Curve, Curve2, Surface};

/// A loop of straight edges through `corners` (in the plane z = 0, which is also the face's
/// (u, v)), as edges and their uses, its edges numbered from `first`.
fn ring(corners: &[[f64; 2]], first: usize) -> (Vec<Edge>, Vec<Use>) {
    let mut edges = Vec::new();
    let mut uses = Vec::new();
    for (i, a) in corners.iter().enumerate() {
        let b = corners[(i + 1) % corners.len()];
        edges.push(Edge {
            curve: Some(Curve::Line {
                start: [a[0], a[1], 0.0],
                end: [b[0], b[1], 0.0],
            }),
            interval: [0.0, 1.0],
            faces: vec![0],
        });
        uses.push(Use {
            edge: first + i,
            forward: true,
            pcurve: Curve2::Spline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                points: vec![*a, b],
            },
        });
    }
    (edges, uses)
}

/// A plate 100 × 60 in the plane z = 0, its corner at the origin, (u, v) = (x, y), its shell using
/// it as `forward` says, with a hole through each of the corner lists in `holes`.
fn plate(forward: Option<bool>, holes: &[[[f64; 2]; 4]]) -> Query {
    let (mut edges, outer) = ring(&[[0.0, 0.0], [100.0, 0.0], [100.0, 60.0], [0.0, 60.0]], 0);
    let mut loops = vec![Loop::Edges(outer)];
    for h in holes {
        let (more, uses) = ring(h, edges.len());
        edges.extend(more);
        loops.push(Loop::Edges(uses));
    }
    let plane = Surface::Plane {
        origin: [0.0; 3],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
    };
    Query::new(Record {
        edges,
        faces: vec![RecordFace {
            parameters: Some(plane.clone()),
            surface: Some(plane),
            domain: [[-1000.0, 1000.0], [-1000.0, 1000.0]],
            forward,
            loops,
        }],
    })
}

fn grid(count_u: usize, count_v: usize) -> Grid {
    Grid {
        count_u,
        count_v,
        angle: 0.0,
        staggered: false,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

fn close3(a: Vec3, b: Vec3) -> bool {
    close(a.x, b.x) && close(a.y, b.y) && close(a.z, b.z)
}

fn points(stations: &[Station]) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = stations.iter().map(|s| [s.point.x, s.point.y]).collect();
    out.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    out
}

/// A 4 × 3 grid on the plate puts a station at each cell's centre, (12.5 + 25 i, 10 + 20 j), each
/// with the plate's normal (+z), its first parameter's direction (+x) and its distance to the
/// nearest edge.
#[test]
fn a_grid_stands_one_station_at_each_cell_centre() {
    let q = plate(Some(true), &[]);
    let got = grid_stations(&q, 0, &grid(4, 3), 0.0).unwrap();
    let mut want = Vec::new();
    for i in 0..4 {
        for j in 0..3 {
            want.push([12.5 + 25.0 * f64::from(i), 10.0 + 20.0 * f64::from(j)]);
        }
    }
    let got_points = points(&got);
    assert_eq!(got_points.len(), want.len());
    for (g, w) in got_points.iter().zip(&want) {
        assert!(close(g[0], w[0]) && close(g[1], w[1]), "{got_points:?}");
    }
    for s in &got {
        assert!(
            close3(s.normal, Vec3::Z) && close3(s.along, Vec3::X),
            "{s:?}"
        );
        let (x, y) = (s.point.x, s.point.y);
        assert!(
            close(s.margin, x.min(100.0 - x).min(y).min(60.0 - y)),
            "{s:?}"
        );
    }
}

/// A face its shell uses against its surface's normal faces the other way: its stations' normals
/// are -z, their directions still +x.
#[test]
fn a_face_its_shell_turns_over_faces_down() {
    let q = plate(Some(false), &[]);
    let got = grid_stations(&q, 0, &grid(2, 2), 0.0).unwrap();
    assert_eq!(got.len(), 4);
    assert!(got
        .iter()
        .all(|s| close3(s.normal, -Vec3::Z) && close3(s.along, Vec3::X)));
}

/// A margin of 11 keeps only the stations at least 11 from every edge: of the 4 × 3 grid, the
/// middle row (y = 30), whose stations are 12.5, 30, 30 and 12.5 from the nearest edge.
#[test]
fn the_margin_leaves_out_stations_near_the_boundary() {
    let q = plate(Some(true), &[]);
    let got = grid_stations(&q, 0, &grid(4, 3), 11.0).unwrap();
    assert_eq!(
        points(&got),
        vec![[12.5, 30.0], [37.5, 30.0], [62.5, 30.0], [87.5, 30.0]]
    );
}

/// A hole from (37, 20) to (63, 40) holds the two middle stations of the 4 × 3 grid, and the
/// station below it, at (37.5, 10), is 10 from the hole's lower edge as from the plate's.
#[test]
fn a_hole_holds_no_station() {
    let hole = [[37.0, 20.0], [37.0, 40.0], [63.0, 40.0], [63.0, 20.0]];
    let q = plate(Some(true), &[hole]);
    let got = grid_stations(&q, 0, &grid(4, 3), 0.0).unwrap();
    assert_eq!(got.len(), 10);
    assert!(!points(&got).contains(&[37.5, 30.0]) && !points(&got).contains(&[62.5, 30.0]));
    let below = got
        .iter()
        .find(|s| close3(s.point, Vec3::new(37.5, 10.0, 0.0)))
        .unwrap();
    assert!(close(below.margin, 10.0), "{below:?}");
    // With a margin of 8, the stations beside the hole (12.5 and 87.5 across, 24.5 from it) stay;
    // the ones at the plate's top and bottom (10 from its edges) do too.
    assert_eq!(grid_stations(&q, 0, &grid(4, 3), 8.0).unwrap().len(), 10);
    assert_eq!(grid_stations(&q, 0, &grid(4, 3), 10.5).unwrap().len(), 2);
}

/// A staggered 2 × 2 grid moves its second column half a cell along: (25, 15) and (25, 45), then
/// (75, 30); the shifted column's cell at v = 0 lies on the plate's edge and stands no copy.
#[test]
fn a_staggered_grid_moves_every_other_column_half_a_cell() {
    let q = plate(Some(true), &[]);
    let g = Grid {
        staggered: true,
        ..grid(2, 2)
    };
    let got = grid_stations(&q, 0, &g, 0.0).unwrap();
    assert_eq!(points(&got), vec![[25.0, 15.0], [25.0, 45.0], [75.0, 30.0]]);
}

/// A grid turned a quarter turn in the extent's unit square still has one station per cell: a
/// 4 × 3 grid turned 90° puts its stations at the centres of a 3 × 4 grid's cells.
#[test]
fn a_turned_grid_still_fills_the_face() {
    let q = plate(Some(true), &[]);
    let g = Grid {
        angle: FRAC_PI_2,
        ..grid(4, 3)
    };
    let got = grid_stations(&q, 0, &g, 0.0).unwrap();
    let mut want = Vec::new();
    for i in 0..3 {
        for j in 0..4 {
            want.push([
                100.0 / 6.0 * f64::from(2 * i + 1),
                7.5 * f64::from(2 * j + 1),
            ]);
        }
    }
    let got = points(&got);
    assert_eq!(got.len(), 12);
    for (g, w) in got.iter().zip(&want) {
        assert!(close(g[0], w[0]) && close(g[1], w[1]), "{got:?}");
    }
}

/// A random placement is the same for the same seed, different for another, every station inside
/// the plate and its hole left alone, as many as asked; `keep` refusing every one leaves none.
#[test]
fn a_random_placement_is_its_seeds() {
    let hole = [[37.0, 20.0], [37.0, 40.0], [63.0, 40.0], [63.0, 20.0]];
    let q = plate(Some(true), &[hole]);
    for noise in [Noise::White, Noise::Blue, Noise::Pink, Noise::Brown] {
        let r = Random {
            count: 30,
            seed: 7,
            noise,
        };
        let a = random_stations(&q, 0, &r, 2.0, &mut |_| true).unwrap();
        let b = random_stations(&q, 0, &r, 2.0, &mut |_| true).unwrap();
        let c = random_stations(&q, 0, &Random { seed: 8, ..r }, 2.0, &mut |_| true).unwrap();
        assert_eq!(a, b, "{noise:?}");
        assert_ne!(a, c, "{noise:?}");
        assert_eq!(a.len(), 30, "{noise:?}");
        for s in &a {
            let (x, y) = (s.point.x, s.point.y);
            let in_hole = (37.0..=63.0).contains(&x) && (20.0..=40.0).contains(&y);
            assert!(
                (0.0..=100.0).contains(&x) && (0.0..=60.0).contains(&y) && !in_hole,
                "{s:?}"
            );
            assert!(s.margin >= 2.0 && close3(s.normal, Vec3::Z), "{s:?}");
        }
        assert!(random_stations(&q, 0, &r, 0.0, &mut |_| false)
            .unwrap()
            .is_empty());
    }
}

/// Blue noise spreads its stations: none closer to another than white noise's closest pair, for
/// the same count and seed.
#[test]
fn blue_noise_keeps_its_stations_apart() {
    let q = plate(Some(true), &[]);
    let closest = |noise: Noise| {
        let r = Random {
            count: 40,
            seed: 3,
            noise,
        };
        let s = random_stations(&q, 0, &r, 0.0, &mut |_| true).unwrap();
        let mut best = f64::INFINITY;
        for (i, a) in s.iter().enumerate() {
            for b in s.iter().skip(i + 1) {
                best = best.min(a.point.dist(b.point));
            }
        }
        best
    };
    assert!(closest(Noise::Blue) > closest(Noise::White));
}

/// The points along the plate's edges: one list per edge from its start, its points 6.25 apart
/// along the 100 mm edges and 3.75 along the 60 mm ones at a spacing of 10 (16 pieces each), and
/// at a spacing of 2, 1.5625 (64 pieces) and 1.875 (32).
#[test]
fn edge_points_run_along_each_edge_at_the_spacing() {
    let q = plate(Some(true), &[]);
    for (spacing, gaps) in [(10.0, [6.25, 3.75]), (2.0, [1.5625, 1.875])] {
        let lists = edge_points(&q, 0, spacing).unwrap();
        assert_eq!(lists.len(), 4);
        let starts = [[0.0, 0.0], [100.0, 0.0], [100.0, 60.0], [0.0, 60.0]];
        for (k, l) in lists.iter().enumerate() {
            let first = l.first().unwrap();
            assert!(
                close3(*first, Vec3::new(starts[k][0], starts[k][1], 0.0)),
                "{l:?}"
            );
            let gap = gaps[k % 2];
            assert!(
                l.iter()
                    .zip(l.iter().skip(1))
                    .all(|(a, b)| close(a.dist(*b), gap)),
                "{l:?}"
            );
        }
    }
    assert!(edge_points(&q, 0, 0.0).is_err());
}

fn apply(m: &[[f64; 4]; 4], p: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
        m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
        m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
    )
}

/// A placement puts the copy's anchor on the station, its +z along the normal (moved out by the
/// offset), its +x along the station turned by the spin, all scaled; flipped, its +z against the
/// normal and its +y with it, a right-handed frame still.
#[test]
fn a_placement_stands_the_anchor_on_the_station() {
    let s = Station {
        point: Vec3::new(1.0, 2.0, 3.0),
        normal: Vec3::Z,
        along: Vec3::X,
        margin: 5.0,
    };
    let anchor = Vec3::new(4.0, 5.0, -1.0);
    let m = placement(&s, anchor, false, 1.0, 0.0, 0.0).unwrap();
    assert!(close3(apply(&m, anchor), s.point));
    assert!(close3(apply(&m, anchor + Vec3::X) - s.point, Vec3::X));
    // Turned a quarter turn, scaled twice and moved 3 out: x goes to +y, twice as long.
    let m = placement(&s, anchor, false, 2.0, 3.0, FRAC_PI_2).unwrap();
    assert!(close3(apply(&m, anchor), Vec3::new(1.0, 2.0, 6.0)));
    assert!(close3(
        apply(&m, anchor + Vec3::X) - apply(&m, anchor),
        Vec3::new(0.0, 2.0, 0.0)
    ));
    assert!(close3(
        apply(&m, anchor + Vec3::Z) - apply(&m, anchor),
        Vec3::new(0.0, 0.0, 2.0)
    ));
    // Flipped: +z down, +x still along, so +y is -y.
    let m = placement(&s, anchor, true, 1.0, 0.0, 0.0).unwrap();
    let at = |d: Vec3| apply(&m, anchor + d) - s.point;
    assert!(close3(at(Vec3::Z), -Vec3::Z) && close3(at(Vec3::X), Vec3::X));
    assert!(close3(
        at(Vec3::new(0.0, 1.0, 0.0)),
        Vec3::new(0.0, -1.0, 0.0)
    ));
    assert!(placement(&s, anchor, false, 0.0, 0.0, 0.0).is_err());
    assert!(placement(&s, anchor, false, 1.0, f64::NAN, 0.0).is_err());
    let bent = Station {
        along: Vec3::new(1.0, 0.0, 1.0),
        ..s
    };
    assert!(placement(&bent, anchor, false, 1.0, 0.0, 0.0).is_err());
}

/// With one set of attractors, the share runs from the near distance to the far one: 0.5 halfway
/// (both falloffs), 0.25 or smoothstep's 0.15625 a quarter of the way, 0 and 1 past the ends, 0
/// when near and far are the same. Between two sets, a copy within a hold zone takes that set's
/// value; between them, the part of the way from one zone to the other.
#[test]
fn attraction_runs_from_the_near_value_to_the_far_one() {
    use Falloff::{Linear, Smooth};
    assert!(close(attraction(5.0, None, 0.0, 10.0, Linear), 0.5));
    assert!(close(attraction(5.0, None, 0.0, 10.0, Smooth), 0.5));
    assert!(close(attraction(2.5, None, 0.0, 10.0, Linear), 0.25));
    assert!(close(attraction(2.5, None, 0.0, 10.0, Smooth), 0.15625));
    assert!(close(attraction(-1.0, None, 0.0, 10.0, Linear), 0.0));
    assert!(close(attraction(11.0, None, 0.0, 10.0, Linear), 1.0));
    assert!(close(attraction(4.0, None, 4.0, 4.0, Linear), 0.0));
    assert!(close(attraction(1.0, Some(9.0), 2.0, 2.0, Linear), 0.0));
    assert!(close(attraction(9.0, Some(1.0), 2.0, 2.0, Linear), 1.0));
    assert!(close(attraction(5.0, Some(3.0), 2.0, 2.0, Linear), 0.75));
    assert!(close(ramp(1.0, 3.0, 0.5), 2.0));
    assert!(close(ramp(1.0, 3.0, 0.0), 1.0));
}

/// Inputs out of range are refused, not panicked on.
#[test]
fn bad_inputs_are_refused() {
    let q = plate(Some(true), &[]);
    assert!(grid_stations(&q, 0, &grid(0, 3), 0.0).is_err());
    assert!(grid_stations(&q, 0, &grid(4, MAX_STATION_COUNT + 1), 0.0).is_err());
    assert!(grid_stations(&q, 0, &grid(4, 3), -1.0).is_err());
    let nan = Grid {
        angle: f64::NAN,
        ..grid(4, 3)
    };
    assert!(grid_stations(&q, 0, &nan, 0.0).is_err());
    assert!(grid_stations(&q, 3, &grid(4, 3), 0.0).is_err());
    let r = Random {
        count: MAX_STATION_COUNT + 1,
        seed: 0,
        noise: Noise::White,
    };
    assert!(random_stations(&q, 0, &r, 0.0, &mut |_| true).is_err());
    assert!(edge_points(&Query::new(Record::default()), 0, 1.0).is_err());
}
