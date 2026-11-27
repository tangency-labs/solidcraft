use super::*;
use crate::{make_box, session};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

fn one(count: usize) -> Grid {
    Grid {
        count_u: count,
        count_v: count,
        angle: 0.0,
        staggered: false,
    }
}

/// Every face of a 100 × 60 × 10 box stands its one station at the face's centre, its normal out
/// of the box: from the record through the query, the shell's use of each face turns its normal.
#[test]
fn each_face_of_a_box_faces_out_of_it() {
    all_fixes();
    let (record, _) = make_box([0.0; 3], [100.0, 60.0, 10.0])
        .unwrap()
        .to_record()
        .unwrap();
    let q = Query::new(&record);
    let mut found = Vec::new();
    for f in 0..record.faces.len() {
        let s = grid_stations(&q, f, &one(1), 0.0).unwrap();
        let [s] = s.as_slice() else {
            panic!("face {f}: {s:?}")
        };
        // The centre of the face is the box's centre moved out along the normal by half the
        // box's size that way.
        let (p, n) = (s.point, s.normal);
        let half = [50.0, 30.0, 5.0];
        let want = [
            50.0 + n.x * half[0],
            30.0 + n.y * half[1],
            5.0 + n.z * half[2],
        ];
        assert!(
            close(p.x, want[0]) && close(p.y, want[1]) && close(p.z, want[2]),
            "face {f}: {s:?}"
        );
        found.push([n.x.round(), n.y.round(), n.z.round()]);
    }
    found.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut want = vec![
        [-1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
    ];
    want.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(found, want);
}

/// A placement on the top face's station stands the copy's anchor on it: with the anchor at the
/// origin, the translation is the station's point and the copy's +z is the face's normal.
#[test]
fn a_placement_on_a_station_is_the_kernels() {
    all_fixes();
    let (record, _) = make_box([0.0; 3], [100.0, 60.0, 10.0])
        .unwrap()
        .to_record()
        .unwrap();
    let q = Query::new(&record);
    let top = (0..record.faces.len())
        .filter_map(|f| grid_stations(&q, f, &one(1), 0.0).ok())
        .flatten()
        .find(|s| s.normal.z > 0.5)
        .unwrap();
    let m = placement(&top, Vec3::new(0.0, 0.0, 0.0), false, 1.0, 0.0, 0.0).unwrap();
    assert!(close(m[3][0], 50.0) && close(m[3][1], 30.0) && close(m[3][2], 10.0));
    assert!(close(m[2][2], 1.0));
    assert!(placement(&top, Vec3::new(0.0, 0.0, 0.0), false, -1.0, 0.0, 0.0).is_err());
}

/// The edge points and a random placement reach the kernel's through the query: four edges round
/// the top face, and as many random stations as asked, all on it.
#[test]
fn edge_points_and_random_stations_come_through() {
    all_fixes();
    let (record, _) = make_box([0.0; 3], [100.0, 60.0, 10.0])
        .unwrap()
        .to_record()
        .unwrap();
    let q = Query::new(&record);
    let top = (0..record.faces.len())
        .find(|f| {
            grid_stations(&q, *f, &one(1), 0.0)
                .is_ok_and(|s| s.first().is_some_and(|s| s.normal.z > 0.5))
        })
        .unwrap();
    let lists = edge_points(&q, top, 5.0).unwrap();
    assert_eq!(lists.len(), 4);
    assert!(lists.iter().flatten().all(|p| close(p.z, 10.0)));
    let r = Random {
        count: 25,
        seed: 1,
        noise: Noise::Blue,
    };
    let mut asked = 0;
    let s = random_stations(&q, top, &r, 1.0, &mut |_| {
        asked += 1;
        true
    })
    .unwrap();
    assert_eq!(s.len(), 25);
    assert!(asked >= 25);
    assert!(s.iter().all(|s| close(s.point.z, 10.0) && s.margin >= 1.0));
}

/// The record of a sphere of radius 10 about the origin, and its spherical face.
fn sphere_record() -> (solidcore_record::brep::Brep, usize) {
    let (record, _) = crate::make_sphere([0.0; 3], 10.0)
        .unwrap()
        .to_record()
        .unwrap();
    let q = Query::new(&record);
    let face = (0..record.faces.len())
        .find(|f| spiral_stations(&q, &[*f], 1, 0.0).is_ok_and(|s| !s.is_empty()))
        .unwrap();
    (record, face)
}

/// The nearest and farthest of the stations' distances to their nearest neighbours, measured by
/// the kernel.
fn nearest(stations: &[Station]) -> (f64, f64) {
    use solidcore::eval::vec::VecExt as _;
    let d = |a: Vec3, b: Vec3| vec3(a).dist(vec3(b));
    let each: Vec<f64> = stations
        .iter()
        .enumerate()
        .map(|(i, s)| {
            stations
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, t)| d(s.point, t.point))
                .fold(f64::INFINITY, f64::min)
        })
        .collect();
    each.iter()
        .fold((f64::INFINITY, 0.0), |(lo, hi), x| (lo.min(*x), hi.max(*x)))
}

/// On a real sphere the layouts spaced by distance reach the kernel's: the geodesic of frequency
/// 2 has its 42 vertices, poles and seam included; the spaced rows and the spiral are even, none
/// crowded at a pole; every station on the sphere, its normal out of the ball.
#[test]
fn the_layouts_spaced_by_distance_are_even_on_a_sphere() {
    all_fixes();
    let (record, face) = sphere_record();
    let q = Query::new(&record);
    // On the ball, and one along its normal 11 from the centre: the normal points out.
    let on_ball = |s: &Station| {
        use solidcore::eval::vec::VecExt as _;
        let (p, n) = (s.point, s.normal);
        let out = Vec3::new(p.x + n.x, p.y + n.y, p.z + n.z);
        close(vec3(p).len(), 10.0) && close(vec3(out).len(), 11.0)
    };
    let geodesic = geodesic_stations(&q, &[face], 2, 0.0, 0.0).unwrap();
    assert_eq!(geodesic.len(), 42);
    assert!(geodesic.iter().all(on_ball));
    let spacing = Spacing {
        spacing: 3.0,
        row_spacing: 3.0,
        staggered: true,
    };
    let rows = spaced_stations(&q, &[face], &spacing, None, 0.0).unwrap();
    let (lo, hi) = nearest(&rows);
    assert!(
        rows.iter().all(on_ball) && lo > 2.0 && hi < 3.6,
        "{} {lo} {hi}",
        rows.len()
    );
    let spiral = spiral_stations(&q, &[face], 200, 0.0).unwrap();
    let (lo, hi) = nearest(&spiral);
    assert!(
        spiral.len() == 200 && spiral.iter().all(on_ball) && lo > 1.7 && hi < 3.4,
        "{lo} {hi}"
    );
    let fill = fill_stations(&q, &[face], 100, 0.0).unwrap();
    assert!(fill.len() == 100 && fill.iter().all(on_ball));
    // A box's face is no sphere.
    let (b, _) = make_box([0.0; 3], [10.0; 3]).unwrap().to_record().unwrap();
    assert!(geodesic_stations(&Query::new(&b), &[0], 2, 0.0, 0.0).is_err());
}

/// The top and a side of a box together are one region: the edge between them is no edge of it,
/// so stations a margin wide of it on either face stay, and the spiral shares its count by area.
#[test]
fn two_faces_of_a_box_are_one_region() {
    all_fixes();
    let (record, _) = make_box([0.0; 3], [100.0, 60.0, 10.0])
        .unwrap()
        .to_record()
        .unwrap();
    let q = Query::new(&record);
    let facing = |n: [f64; 3]| {
        (0..record.faces.len())
            .find(|f| {
                grid_stations(&q, *f, &one(1), 0.0).is_ok_and(|s| {
                    s.first().is_some_and(|s| {
                        s.normal.x * n[0] + s.normal.y * n[1] + s.normal.z * n[2] > 0.5
                    })
                })
            })
            .unwrap()
    };
    let (top, front) = (facing([0.0, 0.0, 1.0]), facing([0.0, -1.0, 0.0]));
    // Ten cells along the front (10 wide) and one up it: their centres 5 below the shared edge.
    let row = Grid {
        count_u: 10,
        count_v: 1,
        angle: 0.0,
        staggered: false,
    };
    let alone = grid_stations_across(&q, &[front], &row, None, 4.0).unwrap();
    let together = grid_stations_across(&q, &[top, front], &row, None, 4.0).unwrap();
    assert!(
        alone.len() < together.len(),
        "{} {}",
        alone.len(),
        together.len()
    );
    // The top is 100 × 60, the front 100 × 10: of 70 stations, 60 on top.
    let spiral = spiral_stations(&q, &[top, front], 70, 0.0).unwrap();
    assert_eq!(spiral.iter().filter(|s| close(s.point.z, 10.0)).count(), 60);
    let fill = fill_stations(&q, &[top, front], 70, 0.0).unwrap();
    assert_eq!(fill.len(), 70);
    assert_eq!(edge_points_across(&q, &[top, front], 5.0).unwrap().len(), 6);
}

/// Fitted, a grid on the box's top runs from the fit in from its edges to the same in from the
/// other side: 3 × 3 at 10 in on the 100 × 60 top, its outer stations 10 from the edges.
#[test]
fn a_fitted_grid_reaches_the_kernels() {
    all_fixes();
    let (record, _) = make_box([0.0; 3], [100.0, 60.0, 10.0])
        .unwrap()
        .to_record()
        .unwrap();
    let q = Query::new(&record);
    let top = (0..record.faces.len())
        .find(|f| {
            grid_stations(&q, *f, &one(1), 0.0)
                .is_ok_and(|s| s.first().is_some_and(|s| s.normal.z > 0.5))
        })
        .unwrap();
    let fit = Fit { u: 10.0, v: 10.0 };
    let got = grid_stations_across(&q, &[top], &one(3), Some(&fit), 0.0).unwrap();
    assert_eq!(got.len(), 9);
    assert!(got
        .iter()
        .all(|s| close(s.margin, 10.0) || s.margin > 10.0 + 1e-6));
    assert_eq!(got.iter().filter(|s| close(s.margin, 10.0)).count(), 8);
}
