//! `intersect_faces_read_only`: where a face of one body meets a face of another, both bodies
//! only read. A plate standing on a plate (T joint), a plate lapped on a plate and a rod
//! standing on a plate, each two separate bodies: the curve is the joint line, and both bodies
//! are the same after the call, in their file bytes and in memory (on the rod, `intersect_faces`
//! itself leaves the plate changed in memory).

#![allow(clippy::unwrap_used, clippy::panic)] // test

use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::intersect::api_intersectors::intersect_faces_read_only;
use solidcore::kernel::Kernel;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::write_brep_to_file;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::edge;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::Point3;

fn k() -> Kernel {
    Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..Kernel::default()
    }
}

fn block(k: &mut Kernel, at: [f64; 3], size: [f64; 3]) -> Brep {
    create_box(
        k,
        Point3::new(at[0], at[1], at[2]),
        size[0],
        size[1],
        size[2],
    )
    .unwrap()
}

/// The edge of `b` nearest `p`.
fn edge_at(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// The face of `b` that the edges nearest `p` and `q` share.
fn face_at(k: &Kernel, b: &Brep, p: [f64; 3], q: [f64; 3]) -> FaceId {
    let a = edge::faces(b, edge_at(k, b, p), None).unwrap();
    let c = edge::faces(b, edge_at(k, b, q), None).unwrap();
    let shared: Vec<FaceId> = a.into_iter().filter(|f| c.contains(f)).collect();
    assert_eq!(shared.len(), 1, "{p:?} {q:?}");
    *shared.first().unwrap()
}

fn smb(k: &Kernel, b: &Brep) -> Vec<u8> {
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &|e| b.owner_identity(e),
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, true).unwrap()
}

/// Both bodies as file bytes and as they are in memory.
fn state(k: &Kernel, a: &Brep, b: &Brep) -> (Vec<u8>, Vec<u8>, String, String) {
    (smb(k, a), smb(k, b), format!("{a:?}"), format!("{b:?}"))
}

fn near(p: Point3, q: [f64; 3]) -> bool {
    p.distance(Point3::new(q[0], q[1], q[2])) <= 1e-6
}

/// The ends of the one curve where `fa` of `a` meets `fb` of `b`, both bodies left as they were.
fn one_line(k: &mut Kernel, a: &Brep, fa: FaceId, b: &Brep, fb: FaceId) -> (Point3, Point3) {
    let before = state(k, a, b);
    let got = intersect_faces_read_only(k, a, fa, b, fb, false).unwrap();
    assert!(state(k, a, b) == before, "a body changed");
    assert_eq!(got.curves.len(), 1, "{got:?}");
    let ends = got.curves.first().unwrap().ends(k).unwrap();
    (ends.start, ends.end)
}

#[test]
fn a_plate_standing_on_a_plate_meets_it_along_its_foot() {
    let mut k = k();
    let base = block(&mut k, [0.0; 3], [100.0, 100.0, 10.0]);
    let web = block(&mut k, [45.0, 0.0, 10.0], [10.0, 100.0, 50.0]);
    let top = face_at(&k, &base, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let side = face_at(&k, &web, [45.0, 50.0, 10.0], [45.0, 0.0, 35.0]);
    let (s, e) = one_line(&mut k, &web, side, &base, top);
    let (p, q) = ([45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    assert!(
        (near(s, p) && near(e, q)) || (near(s, q) && near(e, p)),
        "{s:?} {e:?}"
    );
}

#[test]
fn a_plate_lapped_on_a_plate_meets_it_along_its_edge() {
    let mut k = k();
    let lower = block(&mut k, [0.0; 3], [100.0, 50.0, 5.0]);
    let upper = block(&mut k, [0.0, 40.0, 5.0], [100.0, 50.0, 5.0]);
    let top = face_at(&k, &lower, [50.0, 0.0, 5.0], [0.0, 25.0, 5.0]);
    let front = face_at(&k, &upper, [50.0, 40.0, 5.0], [0.0, 40.0, 7.5]);
    let (s, e) = one_line(&mut k, &upper, front, &lower, top);
    let (p, q) = ([0.0, 40.0, 5.0], [100.0, 40.0, 5.0]);
    assert!(
        (near(s, p) && near(e, q)) || (near(s, q) && near(e, p)),
        "{s:?} {e:?}"
    );
}

#[test]
fn a_rod_standing_on_a_plate_meets_it_round_its_foot_and_leaves_the_plate() {
    let mut k = k();
    let plate = block(&mut k, [0.0; 3], [100.0, 100.0, 10.0]);
    let rod = create_cylinder(&mut k, Point3::new(50.0, 50.0, 10.0), 15.0, 50.0).unwrap();
    let top = face_at(&k, &plate, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let side = face_at(&k, &rod, [65.0, 50.0, 10.0], [65.0, 50.0, 60.0]);
    let before = state(&k, &rod, &plate);
    let got = intersect_faces_read_only(&mut k, &rod, side, &plate, top, false).unwrap();
    assert!(state(&k, &rod, &plate) == before, "a body changed");
    assert!(!got.curves.is_empty());
    // every piece runs round the rod's foot, radius 15 about (50, 50) at z = 10, at its ends
    // and inside; together they are the whole circle, not part of it
    let mut length = 0.0;
    for c in &got.curves {
        let span = c.natural_interval();
        length += c.length(&k, &span, 1e-9).unwrap();
        for i in 0..=8 {
            let t = span.min() + (span.max() - span.min()) * f64::from(i) / 8.0;
            let p = c.evaluate_point(&k, t).unwrap();
            let r = ((p.x - 50.0).powi(2) + (p.y - 50.0).powi(2)).sqrt();
            assert!(
                (r - 15.0).abs() <= 1e-6 && (p.z - 10.0).abs() <= 1e-9,
                "{p:?}"
            );
        }
    }
    let round = 2.0 * std::f64::consts::PI * 15.0;
    assert!(
        (length - round).abs() <= 1e-6 * round,
        "{length} against {round}"
    );
}

#[test]
fn a_face_not_of_its_body_is_refused() {
    let mut k = k();
    let plate = block(&mut k, [0.0; 3], [100.0, 100.0, 10.0]);
    let rod = create_cylinder(&mut k, Point3::new(50.0, 50.0, 10.0), 15.0, 50.0).unwrap();
    let top = face_at(&k, &plate, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    // the box has six faces, the rod three: some face id of the box is none of the rod's
    let stranger = *plate
        .face_list()
        .iter()
        .find(|f| !rod.face_list().contains(f))
        .unwrap();
    let got = intersect_faces_read_only(&mut k, &rod, stranger, &plate, top, false);
    assert!(
        matches!(got, Err(SolidError::InvalidInput { .. })),
        "{got:?}"
    );
}
