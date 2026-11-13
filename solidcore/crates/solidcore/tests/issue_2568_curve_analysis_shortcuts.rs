//! #2568: `check_brep`'s B-spline curve analyses (`SmBSplineCurve::AssertValid` tests
//! 11-14) skip a search that cannot report (a speed or radius bound above the test's
//! tolerance) and keep each search's result in the kernel by the curve's exact content.
//! Neither may change a report: every check here is run with the shortcuts on (twice,
//! the second time from the kept results) and off (`Kernel::memoize_curve_caches`
//! false, every search in full), and the report lines must be the same.
//!
//! The bodies: every `.smb` under `crates/solidcore/tests/data` and the
//! `topology.assert_valid` and `topology.brep_p10` fixtures, and extruded B-spline
//! circles, at check levels 1 and 2, with the default fixes and with the curve check's
//! opt-in ones (`issue_1945_fork_1207_step4` compares a fillet result the same way).
//! The ignored `bench_*` tests time the check of 100 extruded circles and of a plate
//! with 400 holes, with and without the shortcuts.
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
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};
use std::path::{Path, PathBuf};
use std::time::Instant;

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

/// The report lines of a check at `level` of a body `make` builds afresh (a copy of a
/// B-rep would lose its geometry owners), the shortcuts on or off (the error's text
/// where the check fails).
fn lines(k: &mut Kernel, make: &dyn Fn() -> Brep, level: CheckLevel) -> Vec<String> {
    let mut b = make();
    match check_brep(k, &mut b, level) {
        Ok(check) => check.lines(),
        Err(e) => vec![format!("error: {e}")],
    }
}

/// A kernel with the shortcuts `on` or off, with the default fixes or (`checks`) with
/// the curve check's opt-in fixes, which read the radius search's results and measure
/// UV trims on their surfaces.
fn kernel(on: bool, checks: bool) -> Kernel {
    let mut k = Kernel {
        memoize_curve_caches: on,
        ..Kernel::default()
    };
    if checks {
        k.fixes.bspline_check_min_radius = true;
        k.fixes.uv_trim_checks_on_surface = true;
    }
    k
}

/// Checks the body `make` builds with the shortcuts on (twice) and off at levels 1 and
/// 2, with and without the curve check's opt-in fixes: the same lines.
fn assert_same(name: &str, make: &dyn Fn() -> Brep) {
    for checks in [false, true] {
        for level in [CheckLevel::Level1, CheckLevel::Level2] {
            let full = lines(&mut kernel(false, checks), make, level);
            let mut on = kernel(true, checks);
            let first = lines(&mut on, make, level);
            let kept = lines(&mut on, make, level);
            let at = format!("{name} at {level:?}, opt-in checks {checks}");
            assert_eq!(first, full, "{at}: shortcuts on");
            assert_eq!(kept, full, "{at}: from the kept results");
        }
    }
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

#[test]
fn every_vendored_body_checks_the_same_with_and_without_the_shortcuts() {
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
            assert_same(&path.display().to_string(), &|| {
                read_smb(bytes.clone()).unwrap()
            });
        }
    }
    assert!(read >= 10, "only {read} bodies read");
}

/// A cylinder of radius `r` and height 10 about `(x, y)`, extruded from a B-spline circle
/// (`SmApiCreateCircle`), as a circle extrude makes it.
fn extruded_circle(k: &mut Kernel, x: f64, y: f64, r: f64) -> Brep {
    let circle = Curve::from(create_circle(Point3::new(x, y, 0.0), r).unwrap());
    create_linear_sweep(k, &[Some(&circle)], Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

#[test]
fn extruded_circles_check_the_same_with_and_without_the_shortcuts() {
    for r in [1e-3, 0.5, 50.0, 1e4] {
        assert_same(&format!("circle r {r}"), &|| {
            extruded_circle(&mut Kernel::default(), 3.0, -2.0, r)
        });
    }
}

/// The checks of the bodies at level 2, twice each (a body checked again after an edit
/// elsewhere, as SolidCraft re-checks), timed, with the shortcuts on or off and the
/// curve check's opt-in fixes (SolidCraft sets them).
fn time_checks(bodies: &mut [Brep], shortcuts: bool) -> f64 {
    let mut k = kernel(shortcuts, true);
    let start = Instant::now();
    for _ in 0..2 {
        for b in bodies.iter_mut() {
            check_brep(&mut k, b, CheckLevel::Level2).unwrap();
        }
    }
    start.elapsed().as_secs_f64()
}

#[test]
#[ignore = "bench: prints the check times of 100 extruded circles"]
fn bench_100_extruded_circles() {
    let mut k = Kernel::default();
    let mut bodies: Vec<Brep> = (0..100)
        .map(|i| {
            extruded_circle(
                &mut k,
                f64::from(i % 10) * 30.0,
                f64::from(i / 10) * 30.0,
                10.0,
            )
        })
        .collect();
    let off = time_checks(&mut bodies, false);
    let on = time_checks(&mut bodies, true);
    println!("BENCH 100 circles: off {off:.3} s, on {on:.3} s");
}

/// A 400 by 400 by 10 plate with 400 holes of radius 3 on a 20 grid, swept from a
/// square of lines and 400 B-spline circles; `None` where the sweep refuses it.
fn plate_with_400_holes(k: &mut Kernel) -> Option<Brep> {
    let corners = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(400.0, 0.0, 0.0),
        Point3::new(400.0, 400.0, 0.0),
        Point3::new(0.0, 400.0, 0.0),
    ];
    let mut curves: Vec<Curve> = (0..4)
        .map(|i| Curve::from(create_line_segment(corners[i], corners[(i + 1) % 4]).unwrap()))
        .collect();
    for i in 0..400 {
        let c = Point3::new(
            10.0 + f64::from(i % 20) * 20.0,
            10.0 + f64::from(i / 20) * 20.0,
            0.0,
        );
        curves.push(Curve::from(create_circle(c, 3.0).unwrap()));
    }
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    match create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true) {
        Ok(b) => Some(b),
        Err(e) => {
            println!("BENCH plate: not built ({e})");
            None
        }
    }
}

#[test]
#[ignore = "bench: prints the check times of a plate with 400 holes"]
fn bench_plate_with_400_holes() {
    let Some(plate) = plate_with_400_holes(&mut Kernel::default()) else {
        return;
    };
    let mut bodies = [plate];
    let off = time_checks(&mut bodies, false);
    let on = time_checks(&mut bodies, true);
    println!("BENCH plate with 400 holes: off {off:.3} s, on {on:.3} s");
}

/// The plate's checks with the shortcuts on only, to profile what they leave.
#[test]
#[ignore = "bench: prints the check time of a plate with 400 holes, shortcuts on"]
fn bench_plate_with_400_holes_shortcuts_on() {
    let Some(plate) = plate_with_400_holes(&mut Kernel::default()) else {
        return;
    };
    let mut bodies = [plate];
    let on = time_checks(&mut bodies, true);
    println!("BENCH plate with 400 holes: on {on:.3} s");
}
