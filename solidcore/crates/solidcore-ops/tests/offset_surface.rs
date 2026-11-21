//! Offset surfaces (tangency-labs/solidcore#3138): faces copied out at a distance, each on its
//! surface's exact offset, joined where they meet; the cases of SolidCraft's
//! `surface_offset.rs`, on closed forms measured on the exact B-rep.
#![allow(
    clippy::unwrap_used,
    clippy::disallowed_methods // the closed forms' oracle, not ops geometry
)]

use std::f64::consts::PI;

use solidcore_ops::history::{recorded_into, Change, FaceOrigin, Made};
use solidcore_ops::{make_box, make_cylinder, make_sphere, offset_surface, session, Body, Order};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-9)
}

fn face_at(b: &Body, o: &Order, p: [f64; 3]) -> usize {
    b.face_at(o, p, 1e-6).unwrap().unwrap()
}

fn edges(b: &Body) -> usize {
    b.to_record().unwrap().1.edges.len()
}

/// Planes move exactly: a 10 cube's top 5 up is a 10 × 10 square at z = 15, the offset of the
/// top. Two faces across a sharp edge come apart (8 edges, none shared); a zero distance copies.
#[test]
fn planes_move_and_sharp_edges_come_apart() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, o) = b.to_record().unwrap();
    let top = face_at(&b, &o, [5.0, 5.0, 10.0]);
    let (up, h) = recorded_into(&b, &o, Made::Offset, || {
        offset_surface(&b, &o, &[top], 5.0, 0.01).map(|(r, _)| r)
    })
    .unwrap();
    assert!(rel(up.area(1e-8).unwrap(), 100.0) < 1e-9);
    let (lo, hi) = up.bounds().unwrap();
    assert!(
        (lo[2] - 15.0).abs() < 1e-9 && (hi[2] - 15.0).abs() < 1e-9,
        "{lo:?} {hi:?}"
    );
    assert_eq!(h.faces.len(), 1);
    assert_eq!(h.faces[0].change, Change::Generated);
    assert_eq!(h.faces[0].origins, vec![FaceOrigin::OffsetOf(top)]);
    let side = face_at(&b, &o, [5.0, 0.0, 5.0]);
    let (two, closed) = offset_surface(&b, &o, &[top, side], 1.0, 0.01).unwrap();
    assert!(!closed);
    assert_eq!(
        (two.to_record().unwrap().1.faces.len(), edges(&two)),
        (2, 8)
    );
    let (copy, _) = offset_surface(&b, &o, &[top, side], 0.0, 0.01).unwrap();
    assert!(rel(copy.area(1e-8).unwrap(), 200.0) < 1e-9);
    // the body itself is unchanged
    assert!(rel(b.volume(1e-8).unwrap(), 1000.0) < 1e-9);
}

/// A cylinder's side (r 10, h 20, one face closed round its seam) moved out by 2 keeps its seam
/// joined, with radius 12: area 2π·12·20. Moved in past its axis: refused.
#[test]
fn a_cylinder_side_changes_radius() {
    all_fixes();
    let c = make_cylinder([0.0; 3], 10.0, 20.0).unwrap();
    let (_, o) = c.to_record().unwrap();
    let d = 10.0 / 2f64.sqrt();
    let side = face_at(&c, &o, [d, d, 10.0]);
    assert_eq!(
        face_at(&c, &o, [-d, -d, 10.0]),
        side,
        "the side is one face"
    );
    let (out, _) = offset_surface(&c, &o, &[side], 2.0, 0.01).unwrap();
    assert!(rel(out.area(1e-8).unwrap(), 2.0 * PI * 12.0 * 20.0) < 1e-7);
    let (joined, _) = offset_surface(&c, &o, &[side], 0.0, 0.01).unwrap();
    assert_eq!(
        edges(&out),
        edges(&joined),
        "the side still closes on its seam"
    );
    assert!(offset_surface(&c, &o, &[side], -10.5, 0.01).is_err());
}

/// A ball (r 10) moved out by 2: its faces meet smoothly, so their offsets share every edge and
/// close into the ball of radius 12 (area 4π·144, volume 4π·1728/3); moved in to its centre:
/// refused.
#[test]
fn a_ball_closes_and_is_refused_at_its_centre() {
    all_fixes();
    let s = make_sphere([0.0; 3], 10.0).unwrap();
    let (_, o) = s.to_record().unwrap();
    let all: Vec<usize> = (0..o.faces.len()).collect();
    let (out, closed) = offset_surface(&s, &o, &all, 2.0, 0.01).unwrap();
    assert!(closed, "the offset ball closes into a solid");
    assert_eq!(edges(&out), edges(&s));
    assert!(rel(out.area(1e-8).unwrap(), 4.0 * PI * 144.0) < 1e-7);
    assert!(rel(out.volume(1e-8).unwrap(), 4.0 / 3.0 * PI * 1728.0) < 1e-7);
    assert!(offset_surface(&s, &o, &all, -10.0, 0.01).is_err());
}

/// Arguments: no face, a face twice, an index past the faces, a tolerance of 0.
#[test]
fn bad_arguments_are_refused() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, o) = b.to_record().unwrap();
    assert!(offset_surface(&b, &o, &[], 1.0, 0.01).is_err());
    assert!(offset_surface(&b, &o, &[0, 0], 1.0, 0.01).is_err());
    assert!(offset_surface(&b, &o, &[6], 1.0, 0.01).is_err());
    assert!(offset_surface(&b, &o, &[0], 1.0, 0.0).is_err());
}
