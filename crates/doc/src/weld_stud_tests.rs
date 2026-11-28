//! Stud welds: the proposed collars against ISO 13918's table and the proportions; the collar
//! against its revolved volume; the same collar from the foot edge, the side face and a point; a
//! foot within and beyond the tolerance off the face, a leaning stud; the parts unchanged.

use std::f64::consts::PI;

use solidcore_record::Vec3;
use solidcraft_kernel::{SmlibCheck, box_solid, cylinder, validity};

use super::*;
use crate::weld_stake::WELD_TOLERANCE;

fn volume(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

fn check(b: &Body, what: &str) {
    let v = validity(b);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "{what}: {v:?}");
}

fn close(a: f64, b: f64, rel: f64, what: &str) {
    assert!((a - b).abs() <= rel * b.abs(), "{what}: {a} against {b}");
}

/// The collar's volume by Pappus, its outside the circle `stud_weld` draws, integrated finely.
fn collar_volume(d: f64, c: StudCollar) -> f64 {
    let (r, ro, h) = (d / 2.0, c.outer / 2.0, c.height);
    let w = ro - r;
    // z at radius rho on the arc
    let z = |rho: f64| -> f64 {
        if w < h {
            let cc = (ro * ro - r * r - h * h) / (2.0 * w);
            let rad = ro - cc;
            (rad * rad - (rho - cc).powi(2)).max(0.0).sqrt()
        } else {
            let k = (h * h - w * w) / (2.0 * h);
            let rad = h - k;
            k + (rad * rad - (rho - r).powi(2)).max(0.0).sqrt()
        }
    };
    let n = 200_000;
    let step = w / f64::from(n);
    // Simpson's rule on 2 pi rho z(rho)
    let f = |i: u32| {
        let rho = r + step * f64::from(i);
        2.0 * PI * rho * z(rho)
    };
    let mut sum = f(0) + f(n);
    for i in 1..n {
        sum += f(i) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * step / 3.0
}

#[test]
fn the_proposed_collar_follows_iso_13918_for_drawn_arc_studs() {
    let c = |d: f64| proposed_collar(d, StudProcess::DrawnArc);
    assert_eq!(c(10.0), StudCollar { outer: 13.0, height: 4.0 });
    assert_eq!(c(14.6), StudCollar { outer: 18.5, height: 6.0 });
    let mid = c(11.0);
    close(mid.outer, 14.5, 1e-12, "between 10 and 12");
    close(mid.height, 4.5, 1e-12, "between 10 and 12");
    let big = c(20.0);
    close(big.outer, 21.0 * 20.0 / 16.0, 1e-12, "past the table");
    close(big.height, 7.0 * 20.0 / 16.0, 1e-12, "past the table");
    let small = c(3.0);
    close(small.outer, 4.25, 1e-12, "below the table");
    close(small.height, 2.0, 1e-12, "below the table");
    assert_eq!(proposed_collar(10.0, StudProcess::ShortCycle), StudCollar { outer: 1.15 * 10.0, height: 0.15 * 10.0 });
    assert_eq!(proposed_collar(10.0, StudProcess::TipIgnition), StudCollar { outer: 1.05 * 10.0, height: 0.05 * 10.0 });
}

#[test]
fn a_collar_is_its_profile_revolved() {
    for (d, process) in
        [(10.0, StudProcess::DrawnArc), (6.0, StudProcess::DrawnArc), (16.0, StudProcess::ShortCycle), (8.0, StudProcess::TipIgnition)]
    {
        let collar = proposed_collar(d, process);
        let body = stud_weld(&StudWeld { at: [1.0, 2.0, 3.0], axis: [0.0, 0.0, 1.0], diameter: d, collar }).unwrap();
        check(&body, "collar");
        close(volume(&body), collar_volume(d, collar), 1e-7, "collar volume");
        let bb = body.exact_bounds().unwrap();
        close(bb.max.x - bb.min.x, collar.outer, 1e-9, "collar width");
        close(bb.max.z - bb.min.z, collar.height, 1e-9, "collar height");
    }
    // a wide flat collar: its arc centred on the stud's side line
    let collar = StudCollar { outer: 20.0, height: 1.0 };
    let body = stud_weld(&StudWeld { at: [0.0; 3], axis: [0.0, 0.0, 1.0], diameter: 10.0, collar }).unwrap();
    check(&body, "flat collar");
    close(volume(&body), collar_volume(10.0, collar), 1e-7, "flat collar volume");
}

#[test]
fn a_collar_no_wider_than_the_stud_is_refused() {
    let w = StudWeld { at: [0.0; 3], axis: [0.0, 0.0, 1.0], diameter: 10.0, collar: StudCollar { outer: 10.0, height: 1.0 } };
    assert!(matches!(stud_weld(&w), Err(StudRefusal::Invalid(_))));
}

/// A plate's top at z 0 and a stud of diameter 10 standing at `foot` (its base's height) on it.
fn scene(foot: f64) -> (Body, Body) {
    let plate = box_solid(Vec3::new(-15.0, -15.0, -4.0), Vec3::new(15.0, 15.0, 0.0)).unwrap();
    let stud = cylinder(Vec3::new(0.0, 0.0, foot), Vec3::new(0.0, 0.0, 1.0), 5.0, 14.0).unwrap();
    (plate, stud)
}

fn top(b: &Body) -> usize {
    b.face_hits_by_ray([10.0, 10.0, 100.0], [0.0, 0.0, -1.0]).unwrap()[0].face
}

fn foot_edge(stud: &Body, z: f64) -> usize {
    stud.edges(0.01).unwrap().iter().find(|e| (e.mid.z - z).abs() < 1e-6).unwrap().index
}

fn side(stud: &Body) -> usize {
    stud.face_hits_by_ray([20.0, 0.0, 7.0], [-1.0, 0.0, 0.0]).unwrap()[0].face
}

#[test]
fn the_collar_is_the_same_from_the_foot_edge_the_side_face_and_a_point() {
    let (plate, stud) = scene(0.0);
    let (rp, rs) = (plate.record().clone(), stud.record().clone());
    let want = collar_volume(10.0, StudCollar { outer: 13.0, height: 4.0 });
    let face = top(&plate);
    let from_edge = stud_site_from_edge(&stud, foot_edge(&stud, 0.0), &plate, face, StudProcess::DrawnArc, None, WELD_TOLERANCE).unwrap();
    let from_side = stud_site_from_side(&stud, side(&stud), &plate, face, StudProcess::DrawnArc, None, WELD_TOLERANCE).unwrap();
    let at_point = stud_site_at_point(&plate, face, [0.0, 0.0, 0.0], 10.0, StudProcess::DrawnArc, None, WELD_TOLERANCE).unwrap();
    for (s, what) in [(&from_edge, "edge"), (&from_side, "side"), (&at_point, "point")] {
        check(&s.body, what);
        close(volume(&s.body), want, 1e-7, what);
        assert!(s.notices.is_empty(), "{what}: {:?}", s.notices);
        let bb = s.body.exact_bounds().unwrap();
        assert!(bb.min.z.abs() < 1e-9 && (bb.max.z - 4.0).abs() < 1e-9, "{what}: {bb:?}");
    }
    assert_eq!(plate.record(), &rp, "plate changed");
    assert_eq!(stud.record(), &rs, "stud changed");
}

#[test]
fn a_foot_off_the_face_builds_within_the_tolerance_and_is_refused_beyond_it() {
    let (plate, stud) = scene(0.05);
    let s = stud_site_from_edge(&stud, foot_edge(&stud, 0.05), &plate, top(&plate), StudProcess::DrawnArc, None, WELD_TOLERANCE).unwrap();
    assert!(s.notices.is_empty(), "{:?}", s.notices);
    let (plate, stud) = scene(0.6);
    let got = stud_site_from_edge(&stud, foot_edge(&stud, 0.6), &plate, top(&plate), StudProcess::DrawnArc, None, WELD_TOLERANCE);
    assert!(matches!(got, Err(StudRefusal::OffFace { gap }) if (gap - 0.6).abs() < 1e-9), "{got:?}");
}

#[test]
fn a_leaning_stud_builds_round_its_axis_with_a_notice() {
    let plate = box_solid(Vec3::new(-15.0, -15.0, -4.0), Vec3::new(15.0, 15.0, 0.0)).unwrap();
    let lean = 2.0_f64.to_radians();
    let stud = cylinder(Vec3::new(0.0, 0.0, 0.0), Vec3::new(lean.sin(), 0.0, lean.cos()), 5.0, 14.0).unwrap();
    let foot = stud.edges(0.01).unwrap().iter().find(|e| e.mid.z.abs() < 0.5).unwrap().index;
    let s = stud_site_from_edge(&stud, foot, &plate, top(&plate), StudProcess::DrawnArc, None, WELD_TOLERANCE).unwrap();
    check(&s.body, "leaning");
    assert!(matches!(s.notices[..], [StudNotice::Tilted { by }] if (by - lean).abs() < 1e-9), "{:?}", s.notices);
}

#[test]
fn a_face_that_is_not_round_is_refused() {
    let (plate, stud) = scene(0.0);
    let face = top(&plate);
    assert!(matches!(stud_site_from_side(&plate, face, &plate, face, StudProcess::DrawnArc, None, WELD_TOLERANCE), Err(StudRefusal::NotRound(_))));
    let straight = stud.edges(0.01).unwrap().iter().find(|e| (e.mid.z - 7.0).abs() < 1e-6).map(|e| e.index);
    if let Some(e) = straight {
        assert!(matches!(stud_site_from_edge(&stud, e, &plate, face, StudProcess::DrawnArc, None, WELD_TOLERANCE), Err(StudRefusal::NotRound(_))));
    }
}
