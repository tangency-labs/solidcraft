//! #2307 regression (SolidCraft's `fillet_tabbed_plate_outer_loop_and_partial_selection_after_hole`):
//! the tabbed plate (R25 disc with a tab, 2 thick) with its hole r 1.5 at (0, 262/9) and the
//! hole's top rim already rounded r 0.5, read from the binary SMB SolidCraft wrote. The tab's
//! run on the top face (its two sides, its two R2 corners and its top, tangent through the
//! corners, ending at the sharp concave corners at x = +-4) is rounded r 0.5 together.
//!
//! Each side round ends at a mixed-convexity End on the R25 wall. With
//! `fillet_blend_corner` (WR-697, the fork's 3e26f76 `CalcRailIntEndSurface`), the end rib
//! is the round's intersection with the wall, traced from the top rail's end. At x > 0
//! the trace's last span was shorter than a tenth of the one before, and `FlushCurve`
//! merged it into that span without testing the merged span: the rib's UV curve on the
//! round left its 3D curve by 4.1077e-5 against the edge's 4e-5 (`check_brep` `Bad Gap`;
//! the fork at b5bb747 and 18e96f1 gives the same offset). The merge now keeps the short
//! span when the merged one fails the trace's own span test.
//!
//! Checks (no closed form for the whole volume): `check_brep` level 2 clean, 15 faces, and
//! each end rib on both its supports: its 3D curve within the tolerance of the wall
//! `x^2 + y^2 = 25^2` and of the round's cylinder (radius 0.5 about the line x = +-3.5,
//! z = 1.5), and each edge use's UV curve, mapped onto its face, within the edge's
//! tolerance of the 3D curve. The volume is the pinned (flag off) one,
//! 4039.146522158091, within the volume a rib may move: the ribs' curves differ by at
//! most the approximation tolerance 4e-5 across each end's spandrel, area
//! (1 - pi/4) r^2 = 0.0537, so 2 * 4e-5 * 0.0537 = 4.3e-6.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point2;
use solidcore::math::Point3;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, loopuse, vertex, EdgeId};
use solidcore::Brep;

const RADIUS: f64 = 0.5;

/// The SolidCraft points on the five edges (the README of the SolidCraft repro).
const PICKS: [[f64; 3]; 5] = [
    [3.999_999_997_492_012, 27.894_518_234_675_537, 2.0],
    [3.414_213_559_875_010_5, 32.525_324_672_801_695, 2.0],
    [-2.480_541_994_032_137e-9, 33.111_111_110_466_27, 2.0],
    [-3.414_213_564_872_065_3, 32.525_324_672_877_03, 2.0],
    [-4.000_000_002_508_774_5, 27.894_518_234_296_79, 2.0],
];

/// Every opt-in fix on (SolidCraft's pin-16 set), with `fillet_blend_corner` as given.
fn kernel(blend_corner: bool) -> Kernel {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    k.fixes.fillet_blend_corner = blend_corner;
    k
}

/// The SolidCraft file, read under the notification naming as SolidCraft reads it.
fn body(k: &mut Kernel) -> Brep {
    let bytes = include_bytes!("data/issue_2307_solidcraft_plate_hole_rim.smb").to_vec();
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The edge nearest `p`, within 1e-6.
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (1e-6, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.expect("an edge at the pick")
}

/// The pinned (flag off) volume.
const PINNED: f64 = 4_039.146_522_158_091;
/// What a rib may move the volume by (the file header).
const RIB_BOUND: f64 = 4.3e-6;

/// The end ribs: the edges from the top (z = 2) to the tab's side (|x| = 4) near the wall.
fn end_ribs(b: &Brep) -> Vec<EdgeId> {
    let near = |p: Point3| (p.x.abs() - 3.75).abs() < 0.3 && p.y > 24.0 && p.y < 25.0;
    b.edge_list()
        .iter()
        .copied()
        .filter(|&e| {
            let (Some(v0), Some(v1)) = (
                edge::start_vertex(b, e).unwrap(),
                edge::end_vertex(b, e).unwrap(),
            ) else {
                return false;
            };
            let (p0, p1) = (vertex::point(b, v0).unwrap(), vertex::point(b, v1).unwrap());
            near(p0) && near(p1) && p0.z.min(p1.z) > 1.4 && (p0.z - p1.z).abs() > 0.4
        })
        .collect()
}

/// Rib `e` on its supports (the file header), at 200 steps.
fn assert_rib_on_its_supports(k: &Kernel, b: &Brep, e: EdgeId) {
    let tol = edge::tolerance(b, e).unwrap();
    let c = edge::curve(b, e).unwrap().unwrap();
    let iv = edge::interval(b, e).unwrap();
    let side = c.evaluate_point(k, iv.min()).unwrap().x.signum();
    for i in 0..=200 {
        let t = iv.evaluate(f64::from(i) / 200.0);
        let p = c.evaluate_point(k, t).unwrap();
        let wall = ((p.x * p.x + p.y * p.y).sqrt() - 25.0).abs();
        let dx = p.x - 3.5 * side;
        let dz = p.z - 1.5;
        let round = ((dx * dx + dz * dz).sqrt() - RADIUS).abs();
        assert!(
            wall <= tol && round <= tol,
            "{e:?} t {t}: wall {wall:e} round {round:e}"
        );
        for eu in edge::edgeuses(b, e, None, None).unwrap() {
            let face = eu1::face(b, eu).unwrap().unwrap();
            let s = b.face(face).unwrap().surface.as_ref().unwrap();
            let uv = eu1::uv_trim_curve(b, eu).unwrap().0.unwrap();
            let uv = uv.evaluate_point(k, t).unwrap();
            let q = s.evaluate_point(k, Point2::new(uv.x, uv.y)).unwrap();
            let gap = p.distance(q);
            assert!(
                gap <= tol,
                "{e:?} edge use {eu:?} t {t}: gap {gap:e} > {tol:e}"
            );
        }
    }
}

/// The rounded body checked as the file header says; its volume.
fn rounded(blend_corner: bool) -> f64 {
    let mut k = kernel(blend_corner);
    let mut b = body(&mut k);
    let es: Vec<EdgeId> = PICKS.iter().map(|p| pick(&k, &b, *p)).collect();
    fillet_edges(&mut k, &mut b, &es, RADIUS, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(
        check.is_valid(),
        "blend_corner {blend_corner}: {:?}",
        check.lines()
    );
    assert_eq!(b.face_list().len(), 15, "blend_corner {blend_corner}");
    let ribs = end_ribs(&b);
    assert_eq!(ribs.len(), 2, "blend_corner {blend_corner}");
    for e in ribs {
        assert_rib_on_its_supports(&k, &b, e);
    }
    volume(&k, &mut b)
}

#[test]
fn the_input_is_solidcraft_s_plate() {
    let mut k = kernel(true);
    let mut b = body(&mut k);
    assert_eq!(b.face_list().len(), 10);
    let v = volume(&k, &mut b);
    assert!((v - 4_040.367_787_504).abs() <= 1e-6, "{v}");
}

/// The issue's case, with the blend corner: sound, its ribs on their supports.
#[test]
fn the_tab_s_run_rounds_to_a_sound_body() {
    let v = rounded(true);
    assert!((v - PINNED).abs() <= RIB_BOUND, "{v} vs pinned {PINNED}");
}

/// Without the blend corner: the pinned result.
#[test]
fn without_the_blend_corner_the_pinned_result() {
    let v = rounded(false);
    assert!((v - PINNED).abs() <= 1e-9 * PINNED, "{v}");
}
