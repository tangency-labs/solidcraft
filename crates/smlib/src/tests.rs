use super::*;

#[cfg(feature = "native")]
#[test]
fn box_end_to_end() {
    let b = Brep::make_box([1.0, 2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
    assert!((b.volume(1e-7).unwrap() - 6000.0).abs() < 1e-6);
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
    let c = b.try_clone().unwrap();
    drop(b);
    assert!((c.volume(1e-7).unwrap() - 6000.0).abs() < 1e-6);
    let mut buf = [0u8; 512];
    assert_eq!(ffi::current_call(&mut buf), 0, "no C call in flight between calls");
}

#[cfg(feature = "native")]
#[test]
fn bad_arguments_are_errors() {
    assert_eq!(Brep::make_box([0.0; 3], [1.0, -1.0, 1.0]).unwrap_err(), Error::Argument);
    assert_eq!(Brep::make_box([f64::NAN, 0.0, 0.0], [1.0; 3]).unwrap_err(), Error::Argument);
    // each kernel refuses a bad box the same way, whichever one is in force
    init().unwrap();
    for size in [[1.0, -1.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, f64::NAN]] {
        assert_eq!(core::make_box([0.0; 3], size).err(), Some(Error::Argument), "solidcore: {size:?}");
        assert_eq!(ffi::make_box(&[0.0; 3], &size).map_err(error).err(), Some(Error::Argument), "C++: {size:?}");
    }
    let b = Brep::make_box([0.0; 3], [1.0; 3]).unwrap();
    assert_eq!(b.volume(f64::INFINITY).unwrap_err(), Error::Argument);
}

/// `.smb` files on solidcore, in both forms and with a curved body, and a missing or corrupt file
/// is an error, not a panic.
#[test]
fn solidcore_reads_back_the_files_it_writes() {
    use std::f64::consts::PI;
    let dir = std::env::temp_dir().join(format!("solidcraft-smlib-core-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cylinder = core::make_cylinder([1.0, 2.0, 3.0], 2.0, 5.0).map(Brep::made_core).unwrap();
    for ascii in [false, true] {
        let path = dir.join(format!("cylinder-{ascii}.smb"));
        core::file::write(cylinder.core().unwrap(), &path, ascii).unwrap();
        let back = core::file::read(&path, ascii).map(Brep::made_core).unwrap();
        let v = core::volume(back.core().unwrap(), 1e-7).unwrap();
        assert!((v - PI * 4.0 * 5.0).abs() < 1e-5, "ascii {ascii}: {v}");
        assert_eq!(core::counts(back.core().unwrap()).unwrap(), core::counts(cylinder.core().unwrap()).unwrap(), "ascii {ascii}");
    }
    let body = cylinder.core().unwrap();
    assert!(core::file::write(body, &dir.join("missing/x.smb"), false).is_err());
    assert!(core::file::read(&dir.join("missing.smb"), false).is_err());
    let junk = dir.join("junk.smb");
    std::fs::write(&junk, b"not a B-rep file").unwrap();
    assert!(core::file::read(&junk, false).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// ASCII that is no B-rep file is refused like binary. It comes back as an empty body today:
/// default-solidcore/bugs/ascii-junk-reads-as-empty.md.
#[test]
#[ignore = "solidcore: an ASCII file of junk reads as an empty B-rep instead of an error"]
fn solidcore_refuses_an_ascii_file_of_junk() {
    let dir = std::env::temp_dir().join(format!("solidcraft-smlib-core-junk-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let junk = dir.join("junk.smb");
    std::fs::write(&junk, b"not a B-rep file").unwrap();
    let r = core::file::read(&junk, true);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(r.is_err());
}

/// A file either kernel wrote reads on the other, with the same measure.
#[cfg(feature = "native")]
#[test]
fn each_kernel_reads_the_files_of_the_other() {
    use std::f64::consts::PI;
    let dir = std::env::temp_dir().join(format!("solidcraft-smlib-cross-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let want = PI * 4.0 * 5.0;
    for ascii in [false, true] {
        let from_core = dir.join(format!("core-{ascii}.smb"));
        let body = core::make_cylinder([0.0; 3], 2.0, 5.0).unwrap();
        core::file::write(&body, &from_core, ascii).unwrap();
        crate::init().unwrap();
        let on_cpp = ffi::read(&c_path(&from_core).unwrap(), ascii).map(Brep::made).unwrap();
        let v = ffi::volume(on_cpp.cpp().unwrap(), 1e-7).unwrap();
        assert!((v - want).abs() < 1e-5, "C++ on solidcore's file, ascii {ascii}: {v}");
        let from_cpp = dir.join(format!("cpp-{ascii}.smb"));
        let cpp = ffi::make_cylinder(&[0.0; 3], 2.0, 5.0).map(Brep::made).unwrap();
        crate::check(ffi::write(cpp.cpp().unwrap(), &c_path(&from_cpp).unwrap(), ascii)).unwrap();
        let on_core = core::file::read(&from_cpp, ascii).unwrap();
        let v = core::volume(&on_core, 1e-7).unwrap();
        assert!((v - want).abs() < 1e-5, "solidcore on C++'s file, ascii {ascii}: {v}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "native")]
#[test]
fn native_files_round_trip_and_failures_dump_their_inputs() {
    let _dumps = DUMP_DIR_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _cpp = route::on_cpp();
    let dir = std::env::temp_dir().join(format!("solidcraft-smlib-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let b = Brep::make_box([0.0; 3], [2.0, 3.0, 4.0]).unwrap();
    let path = dir.join("box.smb");
    b.write(&path, false).unwrap();
    let back = Brep::read(&path, false).unwrap();
    assert!((back.volume(1e-7).unwrap() - 24.0).abs() < 1e-9);

    let dumps = dir.join("dumps");
    std::fs::create_dir_all(&dumps).unwrap();
    set_dump_dir(Some(&dumps)).unwrap();
    assert!(b.write(&dir.join("missing/x.smb"), false).is_err());
    set_dump_dir(None).unwrap();
    let names: Vec<String> =
        std::fs::read_dir(&dumps).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().any(|n| n.ends_with("-smlib_brep_write.txt")), "{names:?}");
    let smb = names.iter().find(|n| n.ends_with("-smlib_brep_write-brep.smb")).unwrap();
    let replayed = Brep::read(&dumps.join(smb), false).unwrap();
    assert!((replayed.volume(1e-7).unwrap() - 24.0).abs() < 1e-9);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "native")]
#[test]
fn calls_from_many_threads_are_serialised() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            std::thread::spawn(move || {
                let s = 1.0 + i as f64;
                let b = Brep::make_box([0.0; 3], [s; 3]).unwrap();
                (b.volume(1e-7).unwrap() - s * s * s).abs() < 1e-9 * s * s * s
            })
        })
        .collect();
    assert!(handles.into_iter().all(|h| h.join().unwrap()));
}

#[cfg(not(feature = "native"))]
#[test]
fn without_native_every_call_says_so() {
    assert_eq!(init(), Err(Error::NotBuilt));
    // the C API's own box, whichever kernel is in force
    assert_eq!(ffi::make_box(&[0.0; 3], &[1.0; 3]).map_err(error).err(), Some(Error::NotBuilt));
    // solidcore builds a box without SMLib; asking the C++ kernel about it says there is none
    let b = core::make_box([0.0; 3], [1.0; 3]).map(Brep::made_core).unwrap();
    assert_eq!(b.cpp().err(), Some(Error::NotBuilt));
}

/// The dump folder is process-wide: the tests that set it take turns.
static DUMP_DIR_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Which public entry points work with no C++ kernel: all of them but [`CPP_ONLY`], which are the
/// C++ kernel's own arrays and start-up, reached only across kernels and by tests. A new entry point
/// fails [`every_entry_point_is_in_the_guard`] until it is called in the test below or listed.
const CPP_ONLY: [&str; 8] = ["init", "new", "get_i64", "get_f64", "set_i64", "set_f64", "import", "export"];

/// Entry points that ask no kernel, so have nothing to say here.
const NO_KERNEL: [&str; 10] = [
    "inertia_tensor",
    "is_valid",
    "failed_tests",
    "native",
    "arrays_keeping_splines",
    "from_arrays",
    "from_record_keeping_splines",
    "to_native_report",
    "to_native_ordered",
    "blends",
];

const ANSWERING: [&str; 62] = [
    "set_dump_dir",
    "current_call",
    "make_box",
    "read",
    "write",
    "try_clone",
    "transformed",
    "volume",
    "counts",
    "boolean",
    "union_all",
    "boolean2d",
    "imprint",
    "solid_count",
    "fillet",
    "chamfer",
    "blend",
    "blend_dropping",
    "blend_setback",
    "blend_setback_open",
    "fillet_variable",
    "chamfer_sides",
    "shell",
    "offset_faces",
    "thicken",
    "merge_faces",
    "draft",
    "split_faces",
    "split_body",
    "split_body_by_sheet",
    "untrim_face",
    "extend_sheet",
    "face_points",
    "stitch",
    "offset_sheet",
    "classify_point",
    "mass_properties",
    "bounds",
    "edge_face_distance",
    "area",
    "is_manifold_solid",
    "tagged",
    "history",
    "check",
    "make_cylinder",
    "make_sphere",
    "make_cone",
    "make_torus",
    "arrays",
    "to_native",
    "planar_faces",
    "extrude",
    "revolve",
    "sweep",
    "sweep_placed",
    "project_curves",
    "face_section",
    "loft",
    "sweep_stations",
    "loft_to_point",
    "screw_sweep",
    "screw_sweep_radial",
];

/// Every `pub fn` of the files that hold the crate's kernel entry points is accounted for.
#[test]
fn every_entry_point_is_in_the_guard() {
    let files = [
        include_str!("lib.rs"),
        include_str!("ops.rs"),
        include_str!("split.rs"),
        include_str!("trim.rs"),
        include_str!("sheets.rs"),
        include_str!("measure.rs"),
        include_str!("flat.rs"),
        include_str!("history.rs"),
        include_str!("check.rs"),
        include_str!("sweeps.rs"),
        include_str!("export.rs"),
    ];
    for file in files {
        for line in file.lines() {
            let Some(rest) = line.trim_start().strip_prefix("pub fn ") else { continue };
            let name = rest.split(['(', '<']).next().unwrap_or(rest);
            assert!(ANSWERING.contains(&name) || CPP_ONLY.contains(&name) || NO_KERNEL.contains(&name), "`{name}` is in neither list");
        }
    }
}

/// With no C++ kernel, each entry point answers with something other than "not built": its result
/// or its own refusal, on a body of the right kind or the wrong one. Built with the C++ kernel, the
/// same calls with nothing chosen never touch a C++ body: nothing on the default path reaches it.
#[test]
fn without_the_cpp_kernel_every_entry_point_answers() {
    use crate::sweeps::{Affine, Curve, DraftCorner, Ends, SweepFrame};
    use crate::trim::{Continuation, Holes};
    use crate::{Blend, BooleanOp, ChamferSecond, CheckLevel};
    if route::backend() != route::Backend::Solidcore {
        return;
    }
    fn says<T>(name: &str, r: Result<T>) -> Option<T> {
        match r {
            Err(Error::NotBuilt) => panic!("{name} needs the C++ kernel"),
            r => r.ok(),
        }
    }
    let dir = std::env::temp_dir().join(format!("solidcraft-smlib-guard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    {
        let _dumps = DUMP_DIR_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        says("set_dump_dir", set_dump_dir(None));
    }
    let _ = current_call();
    route::take_reached();
    let a = says("make_box", Brep::make_box([0.0; 3], [10.0; 3])).unwrap();
    let t = says("make_box", Brep::make_box([5.0; 3], [10.0; 3])).unwrap();
    let p = |x: f64, y: f64, z: f64| [x, y, z];
    let edge = [p(5.0, 10.0, 10.0)];
    let top = [p(5.0, 5.0, 10.0)];
    let path = dir.join("a.smb");
    says("write", a.write(&path, false));
    says("read", Brep::read(&path, false));
    says("try_clone", a.try_clone());
    says("transformed", a.transformed(&[[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]));
    says("volume", a.volume(1e-6));
    says("counts", a.counts());
    says("boolean", a.boolean(&t, BooleanOp::Union));
    says("union_all", Brep::union_all(&[&a, &t]));
    says("boolean2d", a.boolean2d(&t, BooleanOp::Union));
    says("imprint", a.imprint(&t));
    says("solid_count", a.solid_count());
    says("fillet", a.fillet(&edge, 0.1, 1.0));
    says("chamfer", a.chamfer(&edge, 0.1, 1.0));
    says("blend", a.blend(Blend::Round, &edge, &[1.0], 0.1));
    says("blend_dropping", a.blend_dropping(Blend::Round, &edge, &[1.0], 0.1));
    let corner = [p(10.0, 10.0, 10.0)];
    let three = [p(5.0, 10.0, 10.0), p(10.0, 5.0, 10.0), p(10.0, 10.0, 5.0)];
    says("blend_setback", a.blend_setback(Blend::Round, &three, &[1.0; 3], &corner, &[2.0], 0.1));
    says("blend_setback_open", a.blend_setback_open(Blend::Round, &three, &[1.0; 3], &corner, &[2.0], 0.1));
    says("fillet_variable", a.fillet_variable(edge[0], 0.1, p(0.0, 10.0, 10.0), 1, &[0.0, 0.0, 1.0, 1.0], &[2.0, 1.0]));
    says("chamfer_sides", a.chamfer_sides(&edge, 0.1, 1.0, ChamferSecond::Distance(2.0), p(0.0, 0.0, 1.0)));
    says("shell", a.shell(&top, 0.1, 1.0));
    says("offset_faces", a.offset_faces(&top, 0.1, 1.0));
    says("thicken", a.thicken(1.0, false));
    says("merge_faces", a.merge_faces(&top, 0.1));
    says("draft", a.draft(&[p(5.0, 10.0, 5.0)], 0.1, [0.0; 3], p(0.0, 0.0, 1.0), p(0.0, 0.0, 1.0), 0.1));
    let tool =
        says("Surface::plane", ssi::Surface::plane(p(0.0, 0.0, 5.0), p(1.0, 0.0, 0.0), p(0.0, 1.0, 0.0), [-50.0, 50.0], [-50.0, 50.0])).unwrap();
    says("split_faces", a.split_faces(&[0], &[&tool]));
    says("split_body", a.split_body(&tool));
    let square = [p(0.0, 0.0, 5.0), p(10.0, 0.0, 5.0), p(10.0, 10.0, 5.0), p(0.0, 10.0, 5.0)];
    let lines: Vec<Curve> = (0..4).map(|i| Curve::Line { start: square[i], end: square[(i + 1) % 4] }).collect();
    let sheet = says("planar_faces", Brep::planar_faces(&lines)).unwrap();
    says("split_body_by_sheet", a.split_body_by_sheet(&sheet, 0));
    says("untrim_face", sheet.untrim_face(p(5.0, 5.0, 5.0), 0.1, Holes::All, true, false));
    says("extend_sheet", sheet.extend_sheet(p(5.0, 5.0, 5.0), 0.1, 1.0, Continuation::Natural));
    says("face_points", a.face_points());
    says("stitch", sheet.stitch(0.1));
    says("offset_sheet", sheet.offset_sheet(1.0));
    says("classify_point", a.classify_point([1.0; 3]));
    says("mass_properties", a.mass_properties(1e-6));
    says("bounds", a.bounds());
    says("edge_face_distance", a.edge_face_distance(0, 0));
    says("area", a.area(1e-6));
    says("is_manifold_solid", a.is_manifold_solid());
    says("history", says("tagged", a.tagged(0)).unwrap().history());
    says("check", a.check(CheckLevel::All));
    says("make_cylinder", Brep::make_cylinder([0.0; 3], 1.0, 1.0));
    says("make_sphere", Brep::make_sphere([0.0; 3], 1.0));
    says("make_cone", Brep::make_cone([0.0; 3], 2.0, 1.0, 1.0));
    says("make_torus", Brep::make_torus([0.0; 3], 2.0, 1.0));
    says("arrays", a.arrays());
    says("to_native", a.to_native().map_err(|_| Error::Argument));
    says("extrude", Brep::extrude(&lines, p(0.0, 0.0, 1.0), 1.0, 0.0, DraftCorner::Curvature, true));
    says("revolve", Brep::revolve(&lines, p(20.0, 0.0, 0.0), p(0.0, 1.0, 0.0), 1.0, true));
    let run = [Curve::Line { start: p(5.0, 5.0, 5.0), end: p(5.0, 5.0, 15.0) }];
    says("sweep", Brep::sweep(&lines, &run, SweepFrame::Follow, true));
    says("sweep_placed", Brep::sweep_placed(&lines, &run, SweepFrame::Follow, true));
    says("project_curves", a.project_curves(top[0], 0.1, &lines, p(0.0, 0.0, -1.0), 0.01));
    says("face_section", a.face_section(top[0], 0.1, top[0], p(1.0, 0.0, 0.0), 0.01));
    let lifted: Vec<Curve> = (0..4)
        .map(|i| Curve::Line { start: [square[i][0], square[i][1], 9.0], end: [square[(i + 1) % 4][0], square[(i + 1) % 4][1], 9.0] })
        .collect();
    says("loft", Brep::loft(&[lines.clone(), lifted], 1, true));
    let i3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    says("sweep_stations", Brep::sweep_stations(&lines, &[Affine { m: i3, t: [0.0; 3] }, Affine { m: i3, t: [0.0, 0.0, 2.0] }], Ends::Capped));
    says("loft_to_point", Brep::loft_to_point(&lines, p(5.0, 5.0, 12.0)));
    says("screw_sweep", Brep::screw_sweep(&lines, p(30.0, 0.0, 0.0), p(0.0, 0.0, 1.0), 12.0, 1.0, 0.01));
    says("screw_sweep_radial", Brep::screw_sweep_radial(&lines, p(30.0, 0.0, 0.0), p(0.0, 0.0, 1.0), 12.0, 1.0, &[(0.0, 0.0), (1.0, 0.0)], 0.01));
    let (cpp, core) = route::take_reached();
    assert!(cpp == 0 && core > 0, "the default path touched a C++ body {cpp} times ({core} solidcore calls)");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A failing solidcore call leaves its inputs and what failed, and an answered one leaves nothing;
/// the call in flight is named, and gone after. A folder is process-wide, so this test is the one
/// that sets it.
#[test]
fn a_failing_solidcore_call_leaves_a_dump_and_names_itself() {
    let _dumps = DUMP_DIR_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = std::env::temp_dir().join(format!("solidcraft-smlib-core-dump-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    core::dump::set_dir(Some(dir.clone()));
    let a = core::make_box([0.0; 3], [10.0; 3]).unwrap();
    let seen: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let r: Result<()> = core::with_body("smlib_test_failure", &a, |_, _| {
        *seen.lock().unwrap() = core::dump::current();
        Err(solidcore::SolidError::invalid_input("test", "refused"))
    });
    core::dump::set_dir(None);
    assert!(r.is_err());
    assert_eq!(seen.lock().unwrap().as_deref(), Some("smlib_test_failure"));
    {
        // Under the kernel's lock no solidcore call is in flight.
        let _kernel = core::KERNEL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(core::dump::current(), None);
    }
    let got: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let text = got.iter().find(|n| n.ends_with("-smlib_test_failure.txt")).unwrap_or_else(|| panic!("{got:?}"));
    assert!(std::fs::read_to_string(dir.join(text)).unwrap().contains("refused"));
    let smb = got.iter().find(|n| n.ends_with("-smlib_test_failure-brep.smb")).unwrap_or_else(|| panic!("{got:?}"));
    let back = core::file::read(&dir.join(smb), false).unwrap();
    assert!((core::volume(&back, 1e-7).unwrap() - 1000.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn call_lines_give_the_function_and_the_c_api_time() {
    let (name, took) = call_time("smlib_boolean(op=1, a=#3 (12 faces), b=#4) -> 0 (12.345 ms)").unwrap();
    assert_eq!((name, took), ("smlib_boolean", std::time::Duration::from_micros(12_345)));
    assert_eq!(call_time("smlib_make_box(origin=(0,0,0)) -> -1002 (0.001 ms)").map(|c| c.0), Some("smlib_make_box"));
    for bad in ["SMLib warning at x.cpp:12: tolerance (in smlib_boolean(...))", "smlib_x() -> 0 (nan ms)", "smlib_x() -> 0 (-1 ms)", "", "smlib_x"] {
        assert_eq!(call_time(bad), None, "{bad}");
    }
}

#[cfg(feature = "native")]
#[test]
fn every_call_is_a_profiler_span_timed_by_the_c_api() {
    let _cpp = route::on_cpp();
    solidcraft_profile::set_enabled(true);
    let mark = solidcraft_profile::mark();
    // the C API's own box, whichever kernel is in force
    crate::init().unwrap();
    let b = ffi::make_box(&[0.0; 3], &[1.0; 3]).map(Brep::made).unwrap();
    let _ = b.volume(1e-7).unwrap();
    let r = mark.report();
    let names: Vec<&str> = r.rows.iter().filter(|r| r.category == "smlib").map(|r| r.name).collect();
    assert!(names.contains(&"smlib_make_box") && names.contains(&"smlib_brep_volume"), "{r:?}");
}

/// SMLib may end an edge at a parameter of its own (a knot its vertex lies at, where its drop onto
/// the patch ends): within the edge's tolerance as SMLib held it, on its curve, the pcurve is
/// mapped onto our interval exactly; farther is SMLib trimming the edge, refused with how far. An
/// edge whose gap from its surface raised its tolerance (two of Poppy Humanoid's left thigh, SMLib
/// ending them 0.1 um short) is held at that tolerance.
#[test]
fn a_pcurve_smlib_ends_within_the_edges_tolerance_is_mapped_onto_our_interval() {
    use solidcraft_geom::brep::pcurve::TrimEdge;
    use solidcraft_geom::brep::{Curve, Curve2};
    let line = Curve::Line { start: [0.0, 0.0, 0.0], end: [2.0, 0.0, 0.0] };
    let edge = TrimEdge { curve: &line, nurbs: None, interval: [0.0, 1.0], vertices: [0, 1], tolerance: 1e-5 };
    let theirs = [0.0, 1.0 - 2e-8];
    let pc = Curve2::Spline { knots: vec![0.0, 0.0, theirs[1], theirs[1]], points: vec![[0.0, 0.0], [1.0, 0.0]] };
    let mapped = crate::pcurves::on_our_interval(pc.clone(), &edge, theirs, edge.tolerance).unwrap();
    assert_eq!(mapped.interval(), Some([0.0, 1.0]));
    assert_eq!(mapped.eval(1.0), Some([1.0, 0.0]));
    // 0.02 mm along the edge is past its 1e-5 mm.
    let err = crate::pcurves::on_our_interval(pc, &edge, [0.0, 0.99], edge.tolerance).unwrap_err();
    assert!(err.contains("SMLib moved an edge's interval") && err.contains("tolerance"), "{err}");
    // Held at 0.03 mm, as an edge 0.01 mm off its surface is, the same end is within it.
    let short = Curve2::Spline { knots: vec![0.0, 0.0, 0.99, 0.99], points: vec![[0.0, 0.0], [1.0, 0.0]] };
    let held = crate::pcurves::on_our_interval(short, &edge, [0.0, 0.99], 0.03).unwrap();
    assert_eq!(held.interval(), Some([0.0, 1.0]));
    assert_eq!(held.eval(1.0), Some([1.0, 0.0]));
}

/// An edge whose curve runs 2.5 um past its vertex and its face's patch (as Poppy Humanoid's
/// Congé19 has them), with a tolerance of 0.5 um: SMLib trims such an edge back to the patch
/// when it makes its trim, unless the edge's tolerance covers its gap. The gap is measured
/// (SMLib's own, over the patch) and covered, so the face's pcurves come back on the edge's
/// own interval, ending at the patch's corner.
#[cfg(feature = "native")]
#[test]
fn an_edge_a_few_um_past_its_patch_keeps_its_interval() {
    use solidcraft_geom::brep::pcurve::{TrimEdge, TrimFace, TrimUse, Trims};
    use solidcraft_geom::brep::{Curve, Surface};
    // S(u, v) = (10u, 10v, 2v^2), bicubic
    let g = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let z = [0.0, 0.0, 2.0 / 3.0, 2.0];
    let bezier = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let points: Vec<Vec<[f64; 3]>> = (0..4).map(|i| (0..4).map(|j| [10.0 * g[i], 10.0 * g[j], z[j]]).collect()).collect();
    let surface = Surface::Spline { u_knots: bezier.clone(), v_knots: bezier.clone(), points: points.clone() };
    let past = 2.5e-3;
    let spline = |p: Vec<[f64; 3]>| Curve::Spline { knots: bezier.clone(), points: p };
    let curves = [
        spline((0..4).map(|i| [(10.0 + past) * g[i], 0.0, 0.0]).collect()),
        spline((0..4).map(|j| points[3][j]).collect()),
        spline((0..4).map(|i| points[3 - i][3]).collect()),
        spline((0..4).map(|j| points[0][3 - j]).collect()),
    ];
    let ends = [[0, 1], [1, 2], [2, 3], [3, 0]];
    let edges = (0..4)
        .map(|k| TrimEdge { curve: &curves[k], nurbs: None, interval: [0.0, 1.0], vertices: ends[k], tolerance: if k == 0 { 1e-3 } else { 1e-5 } })
        .collect();
    let face = TrimFace {
        surface: &surface,
        nurbs: None,
        domain: [[0.0, 1.0], [0.0, 1.0]],
        edges,
        vertices: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 2.0], [0.0, 10.0, 2.0]],
        loops: vec![(0..4).map(|k| TrimUse { edge: k, forward: true, pcurve: None }).collect()],
        tolerance: 1e-5,
    };
    let pcs = crate::pcurves::SmlibTrims.trims(&face).unwrap();
    let pc = &pcs[0][0];
    assert_eq!(pc.interval(), Some([0.0, 1.0]));
    let [u, v] = pc.eval(1.0).unwrap();
    assert!((u - 1.0).abs() < 1e-9 && v.abs() < 1e-9, "{u} {v}");
}
