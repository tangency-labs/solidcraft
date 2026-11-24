//! The incremental check (`geom::check_memo`): a check that meets neighbourhoods an earlier
//! check found clean skips their geometric tests, and must report exactly what the full
//! check reports. Every body here is checked in full (`Kernel::memoize_curve_caches` off),
//! then twice on one kernel with the memo on, each time on a fresh body: the first check
//! fills the memo, the second skips what it can. The three must give the same lines.
//!
//! The bodies: the primitives, a boolean result, plates with grids of holes, every `.smb`
//! under `crates/solidcore/tests/data` and every fixture directory, and bodies made broken
//! after a clean check of the same shape, whose broken neighbourhoods must be tested again.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::AttributeStore;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_circle, create_line_segment};
use solidcore::primitive::api_primitives as primitives;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{
    check_brep, check_brep_full, CheckLevel, ValidityScope,
};
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::VertexId;
use solidcore::{Brep, Kernel};
use std::path::{Path, PathBuf};

/// Reads `bytes` as an ASCII `.smb` and names it by arena identities; `None` when the
/// reader refuses it.
fn read_smb(bytes: Vec<u8>) -> Option<Brep> {
    let mut k = Kernel::default();
    let mut b = Brep::new_with_infinite_region().ok()?;
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(&mut k, &mut b, bytes, FileType::Ascii, false, &mut env).ok()?;
    b.use_arena_naming().ok()?;
    Some(b)
}

/// Every `.smb` file under `dir`.
fn smb_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            out.extend(smb_files(&p));
        } else if p.extension().is_some_and(|e| e == "smb") {
            out.push(p);
        }
    }
    out
}

/// The check lines at level 2 of a fresh body from `make`, in full.
fn full(make: &dyn Fn() -> Brep) -> Vec<String> {
    let mut k = Kernel::default();
    let mut b = make();
    match check_brep_full(&mut k, &mut b, CheckLevel::Level2, ValidityScope::Body) {
        Ok(check) => check.lines(),
        Err(e) => vec![format!("error: {e}")],
    }
}

/// The check lines at level 2 of a fresh body from `make`, on `k` with its memo.
fn on(k: &mut Kernel, make: &dyn Fn() -> Brep) -> Vec<String> {
    let mut b = make();
    match check_brep(k, &mut b, CheckLevel::Level2) {
        Ok(check) => check.lines(),
        Err(e) => vec![format!("error: {e}")],
    }
}

/// Checks the body `make` builds in full and twice on one kernel with the memo, and
/// requires the same lines; says whether the body was found valid.
fn same(label: &str, make: &dyn Fn() -> Brep) -> bool {
    let want = full(make);
    let mut k = Kernel::default();
    assert_eq!(on(&mut k, make), want, "{label}: first check");
    let filled = k.check_memo.borrow().len();
    assert_eq!(on(&mut k, make), want, "{label}: second check");
    if want.is_empty() {
        assert!(filled > 0, "{label}: a valid body left nothing clean");
    }
    want.is_empty()
}

/// A `side` by `side` by 10 plate with `n` by `n` circular holes of radius 3 on a 20
/// grid, swept from a square of lines and B-spline circles.
fn plate(n: u32) -> Brep {
    let side = 20.0 * f64::from(n);
    let corners = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(side, 0.0, 0.0),
        Point3::new(side, side, 0.0),
        Point3::new(0.0, side, 0.0),
    ];
    let mut curves: Vec<Curve> = (0..4)
        .map(|i| Curve::from(create_line_segment(corners[i], corners[(i + 1) % 4]).unwrap()))
        .collect();
    for i in 0..n * n {
        let c = Point3::new(
            10.0 + f64::from(i % n) * 20.0,
            10.0 + f64::from(i / n) * 20.0,
            0.0,
        );
        curves.push(Curve::from(create_circle(c, 3.0).unwrap()));
    }
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let mut k = Kernel::default();
    create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

fn holed_block() -> Brep {
    let mut k = Kernel::default();
    let o = Point3::new(0.0, 0.0, 0.0);
    let block = primitives::create_box(&mut k, o, 4.0, 4.0, 2.0).unwrap();
    let drill = primitives::create_cylinder(&mut k, Point3::new(2.0, 2.0, -1.0), 1.0, 4.0).unwrap();
    let mut holed = boolean_difference(&mut k, block, drill).unwrap();
    holed.refresh_cached_topology();
    holed
}

#[test]
fn primitives_and_a_boolean_check_the_same_incrementally() {
    let o = Point3::new(0.0, 0.0, 0.0);
    let makers: Vec<(&str, Box<dyn Fn() -> Brep>)> = vec![
        (
            "box",
            Box::new(move || {
                primitives::create_box(&mut Kernel::default(), o, 2.0, 3.0, 4.0).unwrap()
            }),
        ),
        (
            "cylinder",
            Box::new(move || {
                primitives::create_cylinder(&mut Kernel::default(), o, 1.5, 4.0).unwrap()
            }),
        ),
        (
            "cone",
            Box::new(move || {
                primitives::create_cone(&mut Kernel::default(), o, 2.0, 0.5, 3.0).unwrap()
            }),
        ),
        (
            "sphere",
            Box::new(move || primitives::create_sphere(&mut Kernel::default(), o, 2.5).unwrap()),
        ),
        (
            "torus",
            Box::new(move || {
                primitives::create_torus(&mut Kernel::default(), o, 5.0, 1.5).unwrap()
            }),
        ),
        ("box less cylinder", Box::new(holed_block)),
    ];
    for (label, make) in &makers {
        assert!(same(label, make.as_ref()), "{label}: not valid");
    }
}

#[test]
fn plates_with_holes_check_the_same_incrementally() {
    for n in [1, 2, 4] {
        assert!(same(&format!("{n} by {n} holes"), &|| plate(n)));
    }
}

/// A body made after a clean check of another: the box the boolean cut keeps faces of, then
/// the boolean's result, on one kernel; each check gives the full check's lines.
#[test]
fn a_boolean_result_after_its_input_checks_the_same() {
    let o = Point3::new(0.0, 0.0, 0.0);
    let block = || primitives::create_box(&mut Kernel::default(), o, 4.0, 4.0, 2.0).unwrap();
    let mut k = Kernel::default();
    assert_eq!(on(&mut k, &block), full(&block));
    assert_eq!(on(&mut k, &holed_block), full(&holed_block));
    assert_eq!(on(&mut k, &holed_block), full(&holed_block));
}

/// A plate checked clean, then the same plate with one vertex moved off its edges and
/// faces: the moved vertex's neighbourhoods have other digests and are tested again, so
/// the check reports what the full check reports.
#[test]
fn a_moved_vertex_is_tested_again_after_its_shape_was_clean() {
    let moved = |dx: f64| {
        move || {
            let mut b = plate(2);
            b.vertex_mut(VertexId::from_index(0)).unwrap().point.x += dx;
            b
        }
    };
    let mut k = Kernel::default();
    assert!(on(&mut k, &|| plate(2)).is_empty());
    for dx in [1e-3, 0.5] {
        let make = moved(dx);
        let want = full(&make);
        assert!(!want.is_empty(), "moved {dx}: valid");
        assert_eq!(on(&mut k, &make), want, "moved {dx}");
    }
}

#[test]
fn every_vendored_body_checks_the_same_incrementally() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut read = 0;
    let mut valid = 0;
    for dir in [root.join("tests/data"), root.join("../../fixtures")] {
        for path in smb_files(&dir) {
            let bytes = std::fs::read(&path).unwrap();
            if read_smb(bytes.clone()).is_none() {
                continue;
            }
            read += 1;
            let make = || read_smb(bytes.clone()).unwrap();
            valid += usize::from(same(&path.display().to_string(), &make));
        }
    }
    assert!(read >= 20, "only {read} bodies read");
    assert!(valid >= 5, "only {valid} of {read} bodies valid");
}
