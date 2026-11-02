//! #2370: with `fillet_keep_features` (WR-703) on, a round whose band never reaches a boss on
//! its face was refused (`SM_ERR_INVALID_INPUT`). A 30 x 20 x 8 block with a boss r 6 about
//! (10, 10) standing to z 20, its front top edge (y = 0, z = 8) rounded r 3: the band on the
//! top runs y 0..3, the boss's footprint y 4..16.
//!
//! The boss's walls are swept into the fillet sheet and deleted again when no crease is found.
//! Those edits re-owned the sheet's geometry under the notification naming
//! (`loopuse::notify_ref`); a target named the same way (SolidCraft reads and converts its
//! bodies under that naming) owns a surface under the same key, and the merge refused the
//! ambiguous owner. The sheet the merge takes is now the one built before the sweep.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges_per_edge;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{loopuse, EdgeId, EdgeuseId, FaceId};
use solidcore::{Brep, Kernel};

/// Default fixes, or every fix on but WR-707 (SolidCraft's profile at its ws/pin-16, whose
/// `HELD_BACK` is `fillet_keep_features` and `fillet_refuse_rounds_breaking_through`), with
/// `fillet_keep_features` as given.
fn kernel(solidcraft: bool, keep: bool) -> Kernel {
    let mut k = Kernel::default();
    if solidcraft {
        k.fixes = UpstreamFixes::all();
        k.fixes.fillet_refuse_rounds_breaking_through = false;
    }
    k.fixes.fillet_keep_features = keep;
    k
}

/// `bytes`, a binary SMB file, read under the notification naming, as SolidCraft's
/// solidcore backend reads a file (its `key_of` is `loopuse::notify_ref`).
fn read_notify_named(k: &mut Kernel, bytes: Vec<u8>) -> Brep {
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

/// SolidCraft's joined block and boss (its `box_solid` and `cylinder`, extruded from sketch
/// regions with the circle's seam at 0.6131 rad, then `boolean` union), written as binary SMB
/// by SolidCraft's solidcore backend (SolidCraft ws/pin-16, solidcore 97ed188db).
fn solidcraft_body(k: &mut Kernel) -> Brep {
    let bytes = include_bytes!("data/issue_2370_solidcraft_block_boss.smb").to_vec();
    read_notify_named(k, bytes)
}

/// The same part from solidcore's primitives, with a through hole r 2.5 about (20, 10) when
/// `hole`, given the notification naming by a binary SMB round trip.
fn primitive_body(k: &mut Kernel, hole: bool) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 8.0).unwrap();
    let boss = create_cylinder(k, Point3::new(10.0, 10.0, 0.0), 6.0, 20.0).unwrap();
    let mut b = boolean_union(k, block, boss).unwrap();
    if hole {
        let bore = create_cylinder(k, Point3::new(20.0, 10.0, -1.0), 2.5, 10.0).unwrap();
        b = boolean_difference(k, b, bore).unwrap();
    }
    let store = AttributeStore::default();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &loopuse::notify_ref,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let bytes = write_brep_to_file(k, Some(&b), &env, false).unwrap();
    read_notify_named(k, bytes)
}

/// The edge nearest the point, within 0.1.
fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.expect("an edge there")
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The block less the round's spandrel along its 30 long edge, plus the boss's 12 above the
/// top, less the hole's 8 through the block.
fn closed_form(hole: bool) -> f64 {
    let hole = if hole { PI * 6.25 * 8.0 } else { 0.0 };
    4800.0 - (1.0 - PI / 4.0) * 9.0 * 30.0 + PI * 36.0 * 12.0 - hole
}

/// The front top edge rounded r 3 as SolidCraft rounds it (`SmApiFilletEdgesPerEdge`'s session
/// steps, a circular section): one sound solid of the closed form's volume. Returns the volume.
fn round_front_top(mut k: Kernel, mut b: Brep, hole: bool) -> f64 {
    let e = pick(&k, &b, Point3::new(15.0, 0.0, 8.0));
    fillet_edges_per_edge(
        &mut k,
        &mut b,
        &[e],
        &[3.0],
        &[FilletSurfaceGeneratorType::Circular],
        1,
        1.0,
    )
    .unwrap();
    let v = volume_of(&k, &mut b);
    let want = closed_form(hole);
    println!("volume {v:.12}, want {want:.12}");
    assert!((v - want).abs() <= 1e-9 * want, "volume {v}, want {want}");
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    v
}

/// With keep on the round is the one keep off builds, to the bit: no feature is kept.
fn assert_as_without_keep(solidcraft: bool, body: impl Fn(&mut Kernel) -> Brep, hole: bool) {
    let mut on = kernel(solidcraft, true);
    let b = body(&mut on);
    let v_on = round_front_top(on, b, hole);
    let mut off = kernel(solidcraft, false);
    let b = body(&mut off);
    let v_off = round_front_top(off, b, hole);
    assert_eq!(
        v_on.to_bits(),
        v_off.to_bits(),
        "keep on {v_on}, off {v_off}"
    );
}

#[test]
fn solidcraft_body_rounds_clear_of_the_boss() {
    assert_as_without_keep(true, solidcraft_body, false);
}

#[test]
fn solidcraft_body_rounds_clear_of_the_boss_default_fixes() {
    assert_as_without_keep(false, solidcraft_body, false);
}

#[test]
fn primitive_body_rounds_clear_of_the_boss() {
    assert_as_without_keep(true, |k| primitive_body(k, false), false);
}

#[test]
fn primitive_body_with_a_hole_rounds_clear_of_the_boss() {
    assert_as_without_keep(true, |k| primitive_body(k, true), true);
}
