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
