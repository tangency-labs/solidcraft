//! `SmFace::AssertValid` test 37 (`assert_test_face37`) with winding hulls: an inner
//! loop whose UV hull excludes another inner loop's sample point is not walked for it.
//! That may change no result: every check here runs with the shortcut on and off
//! (`Kernel::memoize_curve_caches`), on fresh bodies, and the results must be the same.
//!
//! The bodies: plates with a grid of circular holes (a face with many inner loops),
//! the same with a third of their UV trims dropped (built again by the test), and every `.smb` under `crates/solidcore/tests/data` and the `topology.assert_valid`
//! and `topology.brep_p10` fixtures, each face's test 37 compared directly.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_circle, create_line_segment};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p4::assert_test_face37;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeuseId;
use solidcore::{Brep, EntityRef, Kernel};
use std::path::{Path, PathBuf};

/// Reads `bytes` as an ASCII `.smb` and names it by arena identities, as the difftest
/// reader does; `None` when the reader refuses it.
fn read_smb(bytes: Vec<u8>) -> Option<Brep> {
    let mut k = Kernel::default();
    let mut b = Brep::new_with_infinite_region().ok()?;
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
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

/// A kernel with the shortcuts `on` or off.
fn kernel(on: bool) -> Kernel {
    Kernel {
        memoize_curve_caches: on,
        ..Kernel::default()
    }
}

/// Test 37 of every face of the body `make` builds, each face on a fresh body, as
/// debug text (the error's where it fails).
fn face37(k: &Kernel, make: &dyn Fn() -> Brep) -> Vec<String> {
    let n = make().face_list().len();
    (0..n)
        .map(|i| {
            let mut b = make();
            let f = b.face_list()[i];
            format!("{:?}", assert_test_face37(k, &mut b, f))
        })
        .collect()
}

/// The check lines at level 2 of the body `make` builds (fresh: a copy of a B-rep
/// would lose its geometry owners).
fn lines(k: &mut Kernel, make: &dyn Fn() -> Brep) -> Vec<String> {
    let mut b = make();
    match check_brep(k, &mut b, CheckLevel::Level2) {
        Ok(check) => check.lines(),
        Err(e) => vec![format!("error: {e}")],
    }
}

/// A `side` by `side` by 10 plate with `n` by `n` circular holes of radius 3 on a
/// 20 grid, swept from a square of lines and B-spline circles.
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

#[test]
fn plates_with_holes_check_the_same_with_and_without_the_winding_hulls() {
    for n in [1, 2, 6] {
        let make = || plate(n);
        let full = face37(&kernel(false), &make);
        assert_eq!(face37(&kernel(true), &make), full, "{n} by {n} holes");
        assert!(full.iter().all(|r| r == "Ok(true)"), "{full:?}");
        assert_eq!(
            lines(&mut kernel(true), &make),
            lines(&mut kernel(false), &make),
            "{n} by {n} holes"
        );
    }
}

/// [`plate`] with the UV trims of every third edge use dropped: test 37 builds them
/// again as it samples and walks, so loops start without a hull and get one later.
fn plate_missing_trims(n: u32) -> Brep {
    let mut b = plate(n);
    let mut i = 0;
    while let Ok(eu) = b.edgeuse_mut(EdgeuseId::from_index(i)) {
        if i % 3 == 0 {
            eu.uv_trim_curve = None;
        }
        i += 1;
    }
    b
}

#[test]
fn holes_with_missing_trims_check_the_same_with_and_without_the_winding_hulls() {
    for n in [2, 4] {
        let make = || plate_missing_trims(n);
        assert_eq!(
            face37(&kernel(true), &make),
            face37(&kernel(false), &make),
            "{n} by {n} holes"
        );
    }
}

#[test]
fn every_vendored_body_has_the_same_test_37_with_and_without_the_winding_hulls() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut read = 0;
    for dir in [
        root.join("tests/data"),
        root.join("../../fixtures/topology.assert_valid"),
        root.join("../../fixtures/topology.brep_p10"),
    ] {
        for path in smb_files(&dir) {
            let bytes = std::fs::read(&path).unwrap();
            if read_smb(bytes.clone()).is_none() {
                continue;
            }
            read += 1;
            let make = || read_smb(bytes.clone()).unwrap();
            assert_eq!(
                face37(&kernel(true), &make),
                face37(&kernel(false), &make),
                "{}",
                path.display()
            );
        }
    }
    assert!(read >= 10, "only {read} bodies read");
}
