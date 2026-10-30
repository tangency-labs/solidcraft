//! A box end to end, the primitives and the error paths.
//!
//! Source: tangency-labs/smlib `capi/tests/smoke.c` (Apache-2.0, Copyright SolidCraft
//! contributors), replay/1.0.1 at f8fd83e. The `NULL` argument checks have no Rust form (a reference is
//! never null) and are left out.

#![allow(clippy::unwrap_used)] // test helpers

use std::f64::consts::PI;
use std::sync::{Arc, Mutex};

use solidcore_smlib_api::{Smlib, Status, ABI_VERSION};

#[test]
fn box_end_to_end() {
    assert_eq!(Smlib::abi_version(), ABI_VERSION);
    let mut api = Smlib::default();
    assert_eq!(api.init(), Ok(()));

    let origin = [1.0, 2.0, 3.0];
    let size = [10.0, 20.0, 30.0];
    let b = api.make_box(origin, size).unwrap();
    assert!((api.brep_volume(&b, 1e-7).unwrap() - 6000.0).abs() < 1e-6);
    assert_eq!(api.brep_counts(&b).unwrap(), [6, 12, 8]);

    let copy = api.brep_copy(&b).unwrap();
    drop(b);
    assert!((api.brep_volume(&copy, 1e-7).unwrap() - 6000.0).abs() < 1e-6);
}

#[test]
fn rejected_arguments() {
    let mut api = Smlib::default();
    let origin = [1.0, 2.0, 3.0];
    assert_eq!(
        api.make_box(origin, [10.0, -1.0, 30.0]).err(),
        Some(Status::ERR_ARG)
    );
    assert_eq!(
        api.make_box(origin, [f64::NAN, 1.0, 1.0]).err(),
        Some(Status::ERR_ARG)
    );
}

/// Boxes SMLib cannot make, so far from the origin for their size that their faces come out
/// degenerate, fail with no body (tangency-labs/solidcore#377).
#[test]
fn far_boxes_fail() {
    let mut api = Smlib::default();
    let far = [
        ([0.0, 0.0, 1e200], [1.0, 1.0, 1.0]),
        ([1e95, 0.0, 0.0], [0.5, 0.5, 0.5]),
        ([1e294, 1e294, 1e294], [1e-3, 1e-3, 1e-3]),
    ];
    for (origin, size) in far {
        assert!(api.make_box(origin, size).is_err(), "{origin:?} {size:?}");
    }
}

/// A box far smaller than the tolerance (`smlib_fuzz`'s leak-000bffce): its top face closes
/// up on the bottom one and SMLib's test of the closed shell fails (`SM_ERR`, 1001).
#[test]
fn tiny_box_is_sm_err() {
    let mut api = Smlib::default();
    let origin = [
        f64::from_bits(0x3e3e_3e3e_2a29_2902),
        f64::from_bits(0x3e3e_3e3e_3e3e_3e3e),
        f64::from_bits(0x3e3e_3e3e_3e3e_3e3e),
    ];
    let size = [
        f64::from_bits(0x3e3e_3e3e_3e3e_3e3c),
        f64::from_bits(0x3eff_e0f5_053e_3e3e),
        f64::from_bits(0x3e3e_3f3e_3e3e_3e3e),
    ];
    assert_eq!(
        api.make_box(origin, size).err().map(Status::code),
        Some(1001)
    );
}

/// One log line per call, with the call and its status.
#[test]
fn log_lines() {
    let mut api = Smlib::default();
    let lines: Arc<Mutex<Vec<(i32, String)>>> = Arc::default();
    let sink = Arc::clone(&lines);
    api.set_log(Some(Box::new(move |level, line| {
        sink.lock().unwrap().push((level, line.to_string()));
    })));
    api.make_box([1.0, 2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
    api.set_log(None);
    let lines = lines.lock().unwrap();
    assert_eq!(lines.len(), 1);
    let (level, line) = &lines[0];
    assert_eq!(*level, 0);
    assert!(
        line.starts_with("smlib_make_box(origin=[1,2,3], size=[10,20,30])"),
        "{line}"
    );
    assert_eq!(api.current_call(), "");
}

/// A failing call dumps its line and its B-rep inputs; the dumped B-rep reads back.
#[test]
fn failing_call_dumps_inputs() {
    let dir = std::env::temp_dir().join(format!("smlib-api-dump-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.to_str().unwrap().to_string();
    let mut api = Smlib::default();
    let b = api.make_box([1.0, 2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
    assert_eq!(api.set_dump_dir(Some(&dir)), Ok(()));
    assert!(api.brep_write(&b, "/nonexistent-dir/x.smb", false).is_err());
    let replayed = api
        .brep_read(&format!("{dir}/1-smlib_brep_write-brep.smb"), true)
        .unwrap();
    assert!((api.brep_volume(&replayed, 1e-7).unwrap() - 6000.0).abs() < 1e-6);
    assert_eq!(api.set_dump_dir(None), Ok(()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn primitives() {
    let mut api = Smlib::default();
    let at = [0.0; 3];
    let b = api.make_cylinder(at, 5.0, 10.0).unwrap();
    assert!((api.brep_volume(&b, 1e-7).unwrap() - PI * 250.0).abs() < 1e-4);
    let b = api.make_sphere(at, 5.0).unwrap();
    assert!((api.brep_volume(&b, 1e-7).unwrap() - 4.0 / 3.0 * PI * 125.0).abs() < 1e-3);
    let b = api.make_cone(at, 5.0, 2.0, 10.0).unwrap();
    assert!((api.brep_volume(&b, 1e-7).unwrap() - PI * 10.0 / 3.0 * 39.0).abs() < 1e-3);
    let b = api.make_torus(at, 10.0, 3.0).unwrap();
    assert!((api.brep_volume(&b, 1e-7).unwrap() - 2.0 * PI * PI * 10.0 * 9.0).abs() < 1e-2);

    assert_eq!(
        api.make_cylinder(at, -1.0, 10.0).err(),
        Some(Status::ERR_ARG)
    );
    assert_eq!(api.make_sphere(at, f64::NAN).err(), Some(Status::ERR_ARG));
    assert_eq!(
        api.make_cone(at, 5.0, 5.0, 10.0).err(),
        Some(Status::ERR_ARG)
    );
    assert_eq!(
        api.make_cone(at, 5.0, -1.0, 10.0).err(),
        Some(Status::ERR_ARG)
    );
    assert_eq!(api.make_torus(at, 3.0, 3.0).err(), Some(Status::ERR_ARG));
}
