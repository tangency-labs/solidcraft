//! #2307 regression (SolidCraft's `report-peg-front-chamfer-d1-then-concave-chamfer-d2`): a
//! bar 40 x 16 x 10 whose far end is a half disc R8 about (40, 8), with a disc R8 about the
//! same centre extruded 10 up on its top; its front top edge chamfered 1, then its concave
//! arc chamfered 2. The second chamfer's chain runs on down the first chamfer's end on the
//! cylinder, which leaves the front face tangentially: a mixed-convexity End
//! (`fillet_blend_corner`, WR-697, the fork's 3e26f76 `CalcRailIntEndSurface`).
//!
//! The inputs are the bodies SolidCraft passed to each chamfer (binary SMB, written by
//! solidcore main c002debb5). The chamfer is SolidCraft's: the nearest edge, radius
//! `d sqrt((1 - c) / (1 + c))` from the binormals at the edge's middle, a linear section
//! on a constant radius solver with that second radius, every corner a bevel, then
//! `StitchAndOrient`.
//!
//! Expected values: the fork (tangency-labs/smlib b5bb747 and 18e96f1 agree), run locally
//! through `SmFilletExecutive` the same way: chamfer 1 alone 9396.981201977540 (8 faces, 18
//! edges, 12 vertices); both 9432.066057685654 (9/21/14); chamfer 2 on the dumped input 2
//! 9432.066057685648. With the flag off, solidcore main c002debb5's 9432.066068032667
//! (SolidCraft's run), which the pinned SMLib's End gives. No closed form.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::edgeuse::p3::{evaluate_binormal_uv, BinormalOptions};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, loopuse, EdgeId};
use solidcore::Brep;

const INPUT_1: &[u8] = include_bytes!("data/issue_2307_solidcraft_peg_before_chamfer1.smb");
const INPUT_2: &[u8] = include_bytes!("data/issue_2307_solidcraft_peg_before_chamfer2.smb");

/// Chamfer 1: the front top edge, d 1.
const FRONT_TOP: [f64; 3] = [20.0, 0.0, 10.0];
/// Chamfer 2: the concave arc, d 2.
const CONCAVE_ARC: [f64; 3] = [32.0, 8.0, 10.0];

/// The fork's chamfer 1 alone.
const FORK_ONE: f64 = 9_396.981_201_977_540;
/// The fork's both chamfers.
const FORK_BOTH: f64 = 9_432.066_057_685_654;
/// The fork's chamfer 2 on the dumped input 2.
const FORK_TWO_ON_DUMP: f64 = 9_432.066_057_685_648;
/// solidcore main c002debb5 (the pinned End), both chamfers.
const BASE_BOTH: f64 = 9_432.066_068_032_667;

/// Every opt-in fix on (SolidCraft's pin-16 set), with `fillet_blend_corner` as given.
fn kernel(blend_corner: bool) -> Kernel {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    k.fixes.fillet_blend_corner = blend_corner;
    k
}

/// A SolidCraft file, read under the notification naming as SolidCraft reads it.
fn read(k: &mut Kernel, bytes: &[u8]) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes.to_vec()), false, false, &mut env).unwrap();
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

/// The edge nearest `p`.
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
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

/// SolidCraft's chamfer of the edge nearest `p` at distance `d`, the body checked sound.
fn chamfer(k: &mut Kernel, b: &mut Brep, p: [f64; 3], d: f64) {
    let e = pick(k, b, p);
    let u0 = edge::blend_edgeuse(k, b, e).unwrap().unwrap();
    let u1 = eu1::radial(b, u0).unwrap().unwrap();
    let t = edge::interval(b, e).unwrap().evaluate(0.5);
    let options = BinormalOptions::default();
    let b0 = evaluate_binormal_uv(k, b, u0, t, false, &options).unwrap();
    let b1 = evaluate_binormal_uv(k, b, u1, t, false, &options).unwrap();
    let c = b0.eval.binormal.dot(b1.eval.binormal).clamp(-1.0, 1.0);
    let r = d * ((1.0 - c) / (1.0 + c)).sqrt();
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius_with_second(
            k,
            &[e],
            &[r],
            &[r],
            &[FilletSurfaceGeneratorType::Linear],
            1,
            1.0,
        )
        .unwrap();
        s.create_corners(k, None).unwrap();
        for corner in s.fillet_corners().unwrap() {
            s.set_corner_bevel(corner, true).unwrap();
        }
        s.do_filleting(k, None).unwrap();
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    let check = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
}

/// Volume and face, edge and vertex counts.
fn measure(k: &Kernel, b: &mut Brep) -> (f64, [usize; 3]) {
    let counts = [
        b.face_list().len(),
        b.edge_list().len(),
        b.vertex_list().len(),
    ];
    (volume(k, b), counts)
}

fn assert_rel(v: f64, want: f64, what: &str) {
    assert!(
        (v - want).abs() <= 1e-9 * v.abs().max(want.abs()),
        "{what}: {v} want {want}"
    );
}

/// Both chamfers from input 1: the fork's value with the flag on, the base's with it off.
#[test]
fn both_chamfers_end_as_the_fork_s() {
    for blend_corner in [true, false] {
        let mut k = kernel(blend_corner);
        let mut b = read(&mut k, INPUT_1);
        chamfer(&mut k, &mut b, FRONT_TOP, 1.0);
        let (one, counts) = measure(&k, &mut b);
        assert_rel(one, FORK_ONE, "chamfer 1");
        assert_eq!(counts, [8, 18, 12]);
        chamfer(&mut k, &mut b, CONCAVE_ARC, 2.0);
        let (both, counts) = measure(&k, &mut b);
        eprintln!("blend_corner {blend_corner}: both {both:.12} {counts:?}");
        assert_eq!(counts, [9, 21, 14], "blend_corner {blend_corner}");
        let want = if blend_corner { FORK_BOTH } else { BASE_BOTH };
        assert_rel(both, want, &format!("both, blend_corner {blend_corner}"));
    }
}

/// Chamfer 2 on SolidCraft's input 2, flag on: the fork's value on the same input.
#[test]
fn the_concave_arc_s_chamfer_on_solidcraft_s_input() {
    let mut k = kernel(true);
    let mut b = read(&mut k, INPUT_2);
    chamfer(&mut k, &mut b, CONCAVE_ARC, 2.0);
    let (v, counts) = measure(&k, &mut b);
    eprintln!("on input 2: {v:.12} {counts:?}");
    assert_eq!(counts, [9, 21, 14]);
    assert_rel(v, FORK_TWO_ON_DUMP, "chamfer 2 on input 2");
}
