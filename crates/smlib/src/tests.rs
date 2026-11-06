use super::*;

#[test]
fn box_end_to_end() {
    let b = Brep::make_box([1.0, 2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
    assert!((b.volume(1e-7).unwrap() - 6000.0).abs() < 1e-6);
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
    let c = b.try_clone().unwrap();
    drop(b);
    assert!((c.volume(1e-7).unwrap() - 6000.0).abs() < 1e-6);
}

#[test]
fn bad_arguments_are_errors() {
    assert_eq!(Brep::make_box([0.0; 3], [1.0, -1.0, 1.0]).unwrap_err(), Error::Argument);
    assert_eq!(Brep::make_box([f64::NAN, 0.0, 0.0], [1.0; 3]).unwrap_err(), Error::Argument);
    for size in [[1.0, -1.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, f64::NAN]] {
        assert_eq!(core::make_box([0.0; 3], size).err(), Some(Error::Argument), "{size:?}");
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
        core::file::write(cylinder.core(), &path, ascii).unwrap();
        let back = core::file::read(&path, ascii).map(Brep::made_core).unwrap();
        let v = core::volume(back.core(), 1e-7).unwrap();
        assert!((v - PI * 4.0 * 5.0).abs() < 1e-5, "ascii {ascii}: {v}");
        assert_eq!(core::counts(back.core()).unwrap(), core::counts(cylinder.core()).unwrap(), "ascii {ascii}");
    }
    let body = cylinder.core();
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

/// The dump folder is process-wide: the tests that set it take turns.
static DUMP_DIR_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Entry points that ask no kernel, so have nothing to say here.
const NO_KERNEL: [&str; 14] = [
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
    "word",
    "words",
    "in_call",
    "hold_a_kernel",
];

const ANSWERING: [&str; 64] = [
    "set_dump_dir",
    "current_call",
    "crash_test",
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
    "screw_sweep_with_options",
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
            assert!(ANSWERING.contains(&name) || NO_KERNEL.contains(&name), "`{name}` is in neither list");
        }
    }
}

/// Each entry point answers, with its result or its own refusal, on a body of the right kind or the
/// wrong one: none panics.
#[test]
fn every_entry_point_answers() {
    use crate::sweeps::{Affine, Curve, DraftCorner, Ends, SweepFrame};
    use crate::trim::{Continuation, Holes};
    use crate::{Blend, BooleanOp, ChamferSecond, CheckLevel, Turn};
    fn says<T>(_name: &str, r: Result<T>) -> Option<T> {
        r.ok()
    }
    let dir = std::env::temp_dir().join(format!("solidcraft-smlib-guard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    {
        let _dumps = DUMP_DIR_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        set_dump_dir(None);
    }
    let _ = current_call();
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
    says("blend_dropping", a.blend_dropping(Blend::Round, Turn::Mitre, &edge, &[1.0], 0.1));
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
    says(
        "screw_sweep_with_options",
        Brep::screw_sweep_with_options(
            &lines,
            p(30.0, 0.0, 0.0),
            p(0.0, 0.0, 1.0),
            12.0,
            1.0,
            crate::sweeps::ScrewSweepOptions { law: None, join_spline_walls: true, tolerance: 0.01 },
        ),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// While a dump folder is set, a call that answers serialises none of its inputs, and one that
/// fails serialises each once: the rig's timings, taken with a folder set, are the kernel's.
#[test]
fn only_a_failing_call_serialises_its_inputs() {
    let _dumps = DUMP_DIR_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = std::env::temp_dir().join(format!("solidcraft-smlib-core-dump-lazy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    core::dump::set_dir(Some(dir.clone()));
    let serialised = || core::dump::SERIALISED.with(std::cell::Cell::get);
    let a = core::make_box([0.0; 3], [10.0; 3]).unwrap();
    let b = core::make_box([5.0; 3], [10.0; 3]).unwrap();
    let before = serialised();
    let v = core::volume(&a, 1e-8);
    let u = core::boolean::boolean(&a, &b, BooleanOp::Union);
    let answered = serialised() - before;
    let r: Result<()> = core::with_body("smlib_test_failure", &a, |_, _| Err(solidcore::SolidError::invalid_input("test", "refused")));
    let failed = serialised() - before - answered;
    core::dump::set_dir(None);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(v.is_ok() && matches!(u, Ok(Some(_))) && r.is_err());
    assert_eq!((answered, failed), (0, 1));
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
    assert!(seen.lock().unwrap().as_deref().is_some_and(|c| c.contains("smlib_test_failure")), "{seen:?}");
    assert!(!core::dump::current().is_some_and(|c| c.contains("smlib_test_failure")), "no longer in flight");
    let got: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let text = got.iter().find(|n| n.ends_with("-smlib_test_failure.txt")).unwrap_or_else(|| panic!("{got:?}"));
    assert!(std::fs::read_to_string(dir.join(text)).unwrap().contains("refused"));
    let smb = got.iter().find(|n| n.ends_with("-smlib_test_failure-brep.smb")).unwrap_or_else(|| panic!("{got:?}"));
    let back = core::file::read(&dir.join(smb), false).unwrap();
    assert!((core::volume(&back, 1e-7).unwrap() - 1000.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(&dir);
}

/// SMLib may end an edge at a parameter of its own (a knot its vertex lies at, where its drop onto
/// the patch ends): within the edge's tolerance as SMLib held it, on its curve, the pcurve is
/// mapped onto our interval exactly; farther is SMLib trimming the edge, refused with how far. An
/// edge whose gap from its surface raised its tolerance (two of Poppy Humanoid's left thigh, SMLib
/// ending them 0.1 um short) is held at that tolerance.
#[test]
fn a_pcurve_smlib_ends_within_the_edges_tolerance_is_mapped_onto_our_interval() {
    use solidcore_record::brep::{Curve, Curve2};
    use solidcraft_geom::brep::pcurve::TrimEdge;
    let line = Curve::Line { start: [0.0, 0.0, 0.0], end: [2.0, 0.0, 0.0] };
    let edge = TrimEdge { curve: &line, nurbs: None, interval: [0.0, 1.0], vertices: [0, 1], tolerance: 1e-5 };
    let theirs = [0.0, 1.0 - 2e-8];
    let pc = Curve2::Spline { knots: vec![0.0, 0.0, theirs[1], theirs[1]], points: vec![[0.0, 0.0], [1.0, 0.0]] };
    let mapped = crate::pcurves::on_our_interval(pc.clone(), &edge, theirs, edge.tolerance).unwrap();
    assert_eq!(mapped.interval(), Some([0.0, 1.0]));
    assert_eq!(solidcraft_geom::brep::pcurve::eval(&mapped, 1.0), Some([1.0, 0.0]));
    // 0.02 mm along the edge is past its 1e-5 mm.
    let err = crate::pcurves::on_our_interval(pc, &edge, [0.0, 0.99], edge.tolerance).unwrap_err();
    assert!(err.contains("SMLib moved an edge's interval") && err.contains("tolerance"), "{err}");
    // Held at 0.03 mm, as an edge 0.01 mm off its surface is, the same end is within it.
    let short = Curve2::Spline { knots: vec![0.0, 0.0, 0.99, 0.99], points: vec![[0.0, 0.0], [1.0, 0.0]] };
    let held = crate::pcurves::on_our_interval(short, &edge, [0.0, 0.99], 0.03).unwrap();
    assert_eq!(held.interval(), Some([0.0, 1.0]));
    assert_eq!(solidcraft_geom::brep::pcurve::eval(&held, 1.0), Some([1.0, 0.0]));
}

/// An edge whose curve runs 2.5 um past its vertex and its face's patch (as Poppy Humanoid's
/// Congé19 has them), with a tolerance of 0.5 um: SMLib trims such an edge back to the patch
/// when it makes its trim, unless the edge's tolerance covers its gap. The gap is measured
/// (SMLib's own, over the patch) and covered, so the face's pcurves come back on the edge's
/// own interval, ending at the patch's corner.
#[test]
fn an_edge_a_few_um_past_its_patch_keeps_its_interval() {
    use solidcore_record::brep::{Curve, Surface};
    use solidcraft_geom::brep::pcurve::{TrimEdge, TrimFace, TrimUse, Trims};
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
    let [u, v] = solidcraft_geom::brep::pcurve::eval(pc, 1.0).unwrap();
    assert!((u - 1.0).abs() < 1e-9 && v.abs() < 1e-9, "{u} {v}");
}

/// A solidcore call blocked on a channel until `release`, on its own thread: a call that runs long
/// or never returns, as one design's slow operation does.
fn call_held_open(body: &str) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<Result<()>>) {
    let call: &'static str = Box::leak(body.to_owned().into_boxed_str());
    let (started, is_started) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let held = std::thread::spawn(move || {
        core::run(call, |_| {
            let _ = started.send(());
            let _ = released.recv();
            Ok(())
        })
    });
    is_started.recv().unwrap();
    (release, held)
}

/// Runs `f` on another thread and gives its answer; a failure here rather than a test that hangs
/// when `f` waits for a call that does not end. The limit is no measure of speed: `f` is free to
/// take all of it, and only a wait with no end fails.
fn answered<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(120)).expect("this call waited for a call on another thread that has not ended")
}

/// One design's long kernel call must never hold up another's: the kernel call of a design closed
/// or still open, blocked here on a channel, while another makes and measures a body of its own.
#[test]
fn a_kernel_call_that_does_not_end_holds_up_no_other_call() {
    let (release, held) = call_held_open("held_open");
    let volume = answered(|| {
        let a = Brep::make_box([0.0; 3], [10.0; 3]).unwrap();
        let b = Brep::make_box([5.0, 0.0, 0.0], [10.0; 3]).unwrap();
        a.boolean(&b, BooleanOp::Union).unwrap().unwrap().volume(1e-7).unwrap()
    });
    assert!((volume - 1500.0).abs() < 1e-6, "{volume}");
    assert!(current_call().is_some_and(|c| c.contains("held_open")), "the call still runs");
    // And one holds up no other held open beside it.
    let (release_too, held_too) = call_held_open("held_open_too");
    assert!(current_call().is_some_and(|c| c.contains("held_open") && c.contains("held_open_too")));
    for (release, held) in [(release, held), (release_too, held_too)] {
        release.send(()).unwrap();
        held.join().unwrap().unwrap();
    }
    assert!(!current_call().is_some_and(|c| c.contains("held_open")), "neither is in flight any more");
}

/// Bodies are made and used on whichever kernel is free, at the same time on several threads, and
/// each answer is the one a single kernel gives: a body made while another call held the first
/// kernel is used once it is free, and two bodies from different kernels go into one boolean.
#[test]
fn bodies_move_between_kernels_and_calls_run_side_by_side() {
    let (release, held) = call_held_open("held_open");
    let on_second = answered(|| Brep::make_box([5.0, 0.0, 0.0], [10.0; 3]).unwrap());
    release.send(()).unwrap();
    held.join().unwrap().unwrap();
    let first = Brep::make_box([0.0; 3], [10.0; 3]).unwrap();
    let v = first.boolean(&on_second, BooleanOp::Subtract).unwrap().unwrap().volume(1e-7).unwrap();
    assert!((v - 500.0).abs() < 1e-6, "{v}");

    let shared = std::sync::Arc::new(Brep::make_box([0.0; 3], [10.0; 3]).unwrap());
    let workers: Vec<_> = (0..8)
        .map(|n| {
            let shared = shared.clone();
            std::thread::spawn(move || {
                for i in 0..20 {
                    let side = 2.0 + f64::from((n + i) % 5);
                    let tool = Brep::make_box([10.0 - side; 3], [side + 1.0; 3]).unwrap();
                    let cut = shared.boolean(&tool, BooleanOp::Subtract).unwrap().unwrap();
                    let rounded = Brep::make_box([0.0; 3], [10.0; 3]).unwrap().fillet(&[[5.0, 0.0, 0.0]], 1e-6, 1.0).unwrap();
                    let lost = (1.0 - std::f64::consts::FRAC_PI_4) * 10.0;
                    assert!((cut.volume(1e-7).unwrap() - (1000.0 - side.powi(3))).abs() < 1e-5);
                    assert!((rounded.volume(1e-7).unwrap() - (1000.0 - lost)).abs() < 1e-4);
                }
            })
        })
        .collect();
    for worker in workers {
        answered(move || worker.join().unwrap());
    }
}

/// Each kernel has its own range of blend serials: a body keeps the serials of the kernel that
/// made it, and another kernel's memo of solved stations must not take them for its own.
#[test]
fn kernels_never_issue_the_same_blend_serial() {
    let range = core::BLEND_SERIALS_PER_KERNEL as usize;
    let ranges = core::run("outer", |outer| {
        let inner = core::run("inner", |inner| Ok(inner.blend_last_serial.get() as usize)).unwrap_or_default();
        Ok([outer.blend_last_serial.get() as usize / range, inner / range])
    });
    let [outer, inner] = ranges.unwrap();
    assert_ne!(outer, inner);
}

/// A panic in a call running beside another costs neither: the panicking call's kernel is thrown
/// away and the other call and later ones go on.
#[test]
fn a_panic_beside_a_running_call_costs_only_its_own_kernel() {
    let (release, held) = call_held_open("held_open");
    let r: Result<()> = core::run("panicking", |_| panic!("on purpose"));
    assert!(matches!(r, Err(Error::Internal(_))), "{r:?}");
    let v = answered(|| Brep::make_box([0.0; 3], [2.0; 3]).unwrap().volume(1e-7).unwrap());
    assert!((v - 8.0).abs() < 1e-6, "{v}");
    release.send(()).unwrap();
    held.join().unwrap().unwrap();
}

/// A stuck call keeps its kernel and nothing more: the pool never holds more than its limit of
/// idle kernels however many calls ran side by side, and once as many calls as it allows are
/// stuck, a new call is refused rather than growing it or waiting.
#[test]
fn the_kernel_pool_stays_bounded_when_calls_stick() {
    let mut pool = core::Pool { idle: Vec::new(), running: 0, most_idle: 1, most_running: 3 };
    let mut made = 0;
    let mut make = || {
        made += 1;
        made
    };
    let (a, b, c) = (pool.take(&mut make), pool.take(&mut make), pool.take(&mut make));
    assert_eq!((a, b, c), (Some(1), Some(2), Some(3)), "side by side, each on its own");
    assert_eq!(pool.take(&mut make), None, "refused, not waiting, not one more");
    pool.put(a);
    assert_eq!(pool.take(&mut make), Some(1), "room again, its kernel used again");
    // The other two never come back here: they still count, and hold no idle slot.
    pool.put(Some(1));
    pool.put(b);
    assert_eq!(pool.idle, [1], "no more idle than allowed");
    assert_eq!(pool.running, 1, "one still stuck");
    // A stopped call's kernel is not used again.
    pool.put(None);
    assert_eq!(pool.running, 0);
    assert_eq!(pool.idle, [1]);
}

/// A stop reaches a running call at solidcore's next check and later calls before they start;
/// the kernel it ran on is not used again; calls of other work and answers from a body go on.
#[test]
fn a_stopped_call_ends_at_its_next_check_and_holds_up_nothing() {
    let stop = stop::Stop::new();
    let (started, is_started) = std::sync::mpsc::channel();
    let running = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            stop.within(|| {
                let first: Result<()> = core::run("stopped_call", |k| {
                    let _ = started.send(());
                    // A long solidcore loop, until it is stopped.
                    loop {
                        k.check_cancelled("stopped_call")?;
                        std::thread::yield_now();
                    }
                });
                let next = Brep::make_box([0.0; 3], [1.0; 3]);
                (first, next.map(|_| ()))
            })
        })
    };
    is_started.recv().unwrap();
    let body = answered(|| Brep::make_box([0.0; 3], [10.0; 3]).unwrap());
    stop.stop();
    let (first, next) = answered(move || running.join().unwrap());
    assert!(first.is_err_and(|e| e.to_string().contains("cancel")));
    assert!(next.is_err_and(|e| e.to_string().contains("cancelled")), "refused before it starts");
    // Under a stopped token, a body still answers, and is as it was.
    let v = stop.within(|| body.volume(1e-7)).unwrap();
    assert!((v - 1000.0).abs() < 1e-6, "{v}");
    assert!(stop.within(|| body.boolean(&body, BooleanOp::Union)).is_err(), "an operation is refused");
    // What fills a shared cell is not stopped.
    assert!(stop.within(|| stop::unstoppable(|| Brep::make_box([0.0; 3], [1.0; 3]))).is_ok());
    // And nothing else is affected.
    let v = body.boolean(&body, BooleanOp::Union).unwrap().unwrap().volume(1e-7).unwrap();
    assert!((v - 1000.0).abs() < 1e-6, "{v}");
}

/// Every call starts with the same draws of random rays, whatever its kernel did before: what
/// depends on them answers the same on any kernel of the pool.
#[test]
fn every_call_starts_clean_on_any_kernel() {
    let draw = |k: &mut solidcore::kernel::Kernel| Ok(k.rng.next_f64());
    let first = core::run("draw", draw).unwrap();
    assert_eq!(core::run("draw", draw).unwrap(), first, "the same kernel, later");
    let held = hold_a_kernel();
    assert_eq!(core::run("draw", draw).unwrap(), first, "another kernel");
    drop(held);
}
