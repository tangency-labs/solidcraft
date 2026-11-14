//! History of the offset family (tangency-labs/solidcore#3138): a face the offset made as a
//! copy is that face's offset because the kernel says so, never by where it lies; a wall
//! made along a sheet's edge stands on that edge; a moved face is modified.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods // the closed form's oracle, not ops geometry
)]

use std::f64::consts::PI;

use solidcore_ops::boolean::boolean;
use solidcore_ops::history::{recorded, Change, FaceOrigin, History, Made};
use solidcore_ops::sweep::{planar_faces, Curve};
use solidcore_ops::{
    make_box, make_cylinder, offset_faces, offset_sheet, session, shell, thicken, Body, BooleanOp,
    Order,
};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

/// The record face whose centroid is `at`.
fn face_at(b: &Body, o: &Order, at: [f64; 3]) -> usize {
    (0..o.faces.len())
        .find(|&f| {
            let c = b.face_centroid(o, f, 1e-8).unwrap().centroid;
            c.iter().zip(at).all(|(x, y)| (x - y).abs() < 1e-9)
        })
        .unwrap()
}

fn origins(h: &History, change: Change) -> Vec<FaceOrigin> {
    let mut v: Vec<FaceOrigin> = h
        .faces
        .iter()
        .filter(|f| f.change == change)
        .flat_map(|f| f.origins.iter().copied())
        .collect();
    v.sort_unstable();
    v
}

/// A 10 cube shelled 1 inward, open at its top: volume 1000 − 8·8·9 = 424. The five inner
/// walls are the offsets of the five faces left closed, which are kept; the top is kept as its
/// rim; every edge of the cube is carried somewhere.
#[test]
fn a_shelled_box_s_inner_walls_are_its_faces_offsets() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, o) = b.to_record().unwrap();
    let top = face_at(&b, &o, [5.0, 5.0, 10.0]);
    let h = recorded(&b, &o, Made::Offset, || shell(&b, &o, &[top], -1.0)).unwrap();
    assert!(near(b.volume(1e-8).unwrap(), 424.0, 1e-9));
    assert_eq!(h.faces.len(), 11);
    let closed: Vec<usize> = (0..6).filter(|&f| f != top).collect();
    let offsets: Vec<FaceOrigin> = closed.iter().map(|&f| FaceOrigin::OffsetOf(f)).collect();
    assert_eq!(origins(&h, Change::Generated), offsets, "{h:?}");
    let kept: Vec<FaceOrigin> = (0..6).map(FaceOrigin::Face).collect();
    assert_eq!(origins(&h, Change::Kept), kept, "{h:?}");
    let mut carried: Vec<usize> = h.edges.iter().flatten().copied().collect();
    carried.sort_unstable();
    carried.dedup();
    assert_eq!(carried, (0..12).collect::<Vec<_>>(), "{h:?}");
    // the tags and roles are gone: read again as an offset, nothing is an offset copy
    let (_, o2) = b.to_record().unwrap();
    let again = recorded(&b, &o2, Made::Offset, || Ok(())).unwrap();
    assert!(
        again.faces.iter().all(|f| f.change == Change::Kept),
        "{again:?}"
    );
}

/// A 10 × 10 square sheet thickened 1: volume 100. The sheet stays, its copy is its offset,
/// and each of the four walls stands on its own edge of the square.
#[test]
fn a_thickened_sheet_s_walls_stand_on_its_edges() {
    all_fixes();
    let p = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ];
    let sides: Vec<Curve> = (0..4)
        .map(|i| Curve::Line {
            start: p[i],
            end: p[(i + 1) % 4],
        })
        .collect();
    let s = planar_faces(&sides).unwrap();
    let (_, o) = s.to_record().unwrap();
    assert_eq!((o.faces.len(), o.edges.len()), (1, 4));
    let h = recorded(&s, &o, Made::Offset, || thicken(&s, 1.0, false)).unwrap();
    assert!(near(s.volume(1e-8).unwrap(), 100.0, 1e-9));
    assert_eq!(
        origins(&h, Change::Kept),
        vec![FaceOrigin::Face(0)],
        "{h:?}"
    );
    let made = origins(&h, Change::Generated);
    let mut want = vec![FaceOrigin::OffsetOf(0)];
    want.extend((0..4).map(FaceOrigin::AlongEdge));
    want.sort_unstable();
    assert_eq!(made, want, "{h:?}");
}

/// A 10 cube's top moved up 2: volume 1200; the top is modified, the other five kept.
#[test]
fn a_moved_face_is_modified() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, o) = b.to_record().unwrap();
    let top = face_at(&b, &o, [5.0, 5.0, 10.0]);
    let h = recorded(&b, &o, Made::Local { moved: &[top] }, || {
        offset_faces(&b, &o, &[top], 2.0)
    })
    .unwrap();
    assert!(near(b.volume(1e-8).unwrap(), 1200.0, 1e-9));
    assert_eq!(
        origins(&h, Change::Modified),
        vec![FaceOrigin::Face(top)],
        "{h:?}"
    );
    assert_eq!(origins(&h, Change::Kept).len(), 5, "{h:?}");
}

/// #865's plate (30 × 20 × 6 less an r 1.5 hole 1 from its front) offset 1.5 inward: the
/// hole's offset (r 3) crosses the front's, so the kernel rebuilds the top and bottom offsets
/// whose loops cross and relates the rebuilt faces in place of the old ones. The volume is
/// (27 · 17 − (πr² − segment)) · 3. Every face is an offset copy, the rebuilt top's of the top
/// too: the roles are read where the relations are final.
#[test]
fn an_offset_whose_loops_cross_keeps_the_rebuilt_faces_roles() {
    all_fixes();
    let plate = make_box([0.0; 3], [30.0, 20.0, 6.0]).unwrap();
    let hole = make_cylinder([15.0, 2.5, -1.0], 1.5, 8.0).unwrap();
    let b = boolean(plate, hole, BooleanOp::Subtract).unwrap().unwrap();
    let (_, o) = b.to_record().unwrap();
    let top = face_at_z(&b, &o, 6.0);
    let h = recorded(&b, &o, Made::Offset, || offset_sheet(&b, -1.5)).unwrap();
    let (t, r) = (1.5_f64, 3.0_f64);
    let d = 2.5 - t;
    let segment = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    let want = (27.0 * 17.0 - (PI * r * r - segment)) * 3.0;
    assert!(near(b.volume(1e-8).unwrap(), want, 1e-6), "{}", want);
    assert!(
        h.faces.iter().all(|f| f.change == Change::Generated
            && !f.origins.is_empty()
            && f.origins
                .iter()
                .all(|o| matches!(o, FaceOrigin::OffsetOf(_)))),
        "{h:?}"
    );
    let (_, after) = b.to_record().unwrap();
    let new_top = face_at_z(&b, &after, 4.5);
    assert_eq!(
        h.faces[new_top].origins,
        vec![FaceOrigin::OffsetOf(top)],
        "{h:?}"
    );
}

/// The one planar face at height `z` facing +z.
fn face_at_z(b: &Body, o: &Order, z: f64) -> usize {
    (0..o.faces.len())
        .find(|&f| {
            b.face_plane_normal(o, f)
                .unwrap()
                .is_some_and(|n| n.value[2] > 0.5)
                && (b.face_centroid(o, f, 1e-8).unwrap().centroid[2] - z).abs() < 1e-9
        })
        .unwrap()
}
