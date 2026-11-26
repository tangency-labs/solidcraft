//! Large STEP imports (#883): the file's text is held once however many undo states share
//! it, and an import cancelled from another thread stops and keeps nothing.

use std::sync::Arc;

use serde_json::json;
use solidcraft_doc::{FeatureKind, progress::Progress};

use crate::*;

fn step_file(name: &str, count: usize) -> String {
    let bodies: Vec<_> = (0..count)
        .map(|i| {
            let at = solidcore_record::Vec3::new((i % 32) as f64 * 12.0, (i / 32) as f64 * 12.0, 0.0);
            solidcraft_kernel::box_solid(at, at + solidcore_record::Vec3::new(10.0, 10.0, 10.0)).unwrap()
        })
        .collect();
    let text = solidcraft_kernel::step_export(&bodies.iter().collect::<Vec<_>>(), name).unwrap();
    let path = std::env::temp_dir().join(format!("solidcraft-883-{}-{name}.step", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().to_string()
}

fn step_text(s: &Session) -> Arc<str> {
    s.doc.features.iter().find_map(|f| if let FeatureKind::Import { step, .. } = &f.kind { Some(step.clone()) } else { None }).unwrap()
}

/// Memory budget: 30 edits after a large import keep one copy of the STEP text, not 31.
#[test]
fn edits_after_a_large_import_share_one_copy_of_its_text() {
    let path = step_file("shared", 300);
    let mut s = Session::default();
    s.execute("file.insert_step", &json!({"path": path})).unwrap();
    let text = step_text(&s);
    assert!(text.len() > 500_000, "{} bytes", text.len());
    for i in 0..30 {
        s.execute("parameters.add", &json!({"name": format!("p{i}"), "expression": format!("{i} mm")})).unwrap();
    }
    assert!(s.undo.len() >= 30);
    for snap in &s.undo {
        for f in &snap.doc.features {
            if let FeatureKind::Import { step, .. } = &f.kind {
                assert!(Arc::ptr_eq(step, &text), "an undo state holds its own copy of the STEP text");
            }
        }
    }
    assert!(Arc::ptr_eq(&step_text(&s), &text));
    // Every Document clone (preview, scratch session) shares it too.
    let copy = (*s.doc).clone();
    assert!(copy.features.iter().any(|f| matches!(&f.kind, FeatureKind::Import { step, .. } if Arc::ptr_eq(step, &text))));
    let _ = std::fs::remove_file(path);
}

#[test]
fn cancelled_import_stops_and_leaves_the_design_alone() {
    let path = step_file("cancelled", 300);
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let before = s.model.state().bodies.len();
    // Cancel from the window thread while the worker reads the file.
    let progress = Arc::new(Progress::default());
    let canceller = {
        let progress = progress.clone();
        std::thread::spawn(move || {
            while progress.work().is_none() {
                std::thread::yield_now();
            }
            progress.cancel();
        })
    };
    let r = solidcraft_doc::progress::watch(progress.clone(), || s.execute("file.insert_step", &json!({"path": path})));
    canceller.join().unwrap();
    let err = r.unwrap_err().to_string();
    assert!(err.contains("cancelled"), "{err}");
    assert_eq!(s.model.state().bodies.len(), before);
    assert!(!s.doc.features.iter().any(|f| matches!(f.kind, FeatureKind::Import { .. })));
    let (_, done, total) = progress.work().unwrap();
    assert!(done < total, "stopped part way ({done} of {total})");
    let _ = std::fs::remove_file(path);
}

/// A preview cancelled while it imports again (#7167) keeps no "cancelled" result: the scratch
/// session shares the evaluation cache with the design, which then evaluates the same inputs and
/// imports in full.
#[test]
fn a_cancelled_preview_import_leaves_no_failure_in_the_shared_cache() {
    let path = step_file("preview", 300);
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "first"})).unwrap();
    s.execute("file.insert_step", &json!({"path": path})).unwrap();
    let import = s.doc.features.iter().position(|f| matches!(f.kind, FeatureKind::Import { .. })).unwrap();
    // Changing document units changes the shared evaluation context, forcing the import to
    // evaluate again on new inputs.
    let change_units = json!({"units": "in"});
    let mut scratch = s.scratch();
    let progress = Arc::new(Progress::default());
    let canceller = {
        let progress = progress.clone();
        std::thread::spawn(move || {
            while progress.work().is_none() {
                std::thread::yield_now();
            }
            progress.cancel();
        })
    };
    let _ = solidcraft_doc::progress::watch(progress.clone(), || scratch.execute("document.units", &change_units));
    canceller.join().unwrap();
    let (_, done, total) = progress.work().unwrap();
    assert!(done < total, "the preview's import stopped part way ({done} of {total})");
    s.execute("document.units", &change_units).unwrap();
    let result = &s.model.results[import];
    assert!(result.error.is_none() && !result.skipped, "the design imports in full: {:?}", result.error);
    let _ = std::fs::remove_file(path);
}

/// The largest files read (#4086). Over the default limits a STEP file, a design file and a glTF
/// file (#17446) are refused before anything is read, saying how to raise the limit; with the
/// limits raised (the preference, or `--max-file-size` for one run, which wins) the same reads go
/// ahead. A mesh file and a point cloud file follow the limit for STEP and import files
/// (#14548).
#[test]
fn raised_file_limits_let_larger_step_and_design_files_be_read() {
    use solidcraft_io::limits::{self, FileLimits, GB, Kind, MB};
    /// Puts the defaults back however the test ends.
    struct Defaults;
    impl Drop for Defaults {
        fn drop(&mut self) {
            let _ = limits::set_override(None);
            let _ = limits::set_preferred(FileLimits::DEFAULT);
        }
    }
    let _defaults = Defaults;
    let dir = std::env::temp_dir().join(format!("solidcraft-4086-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Sparse files: nothing is written, and a refusal reads nothing either.
    let sparse = |name: &str, len: u64| {
        let path = dir.join(name);
        std::fs::File::create(&path).unwrap().set_len(len).unwrap();
        path.to_string_lossy().to_string()
    };
    let step = sparse("big.step", 512 * MB + 1);
    let design = sparse("big.solidcraft", 256 * MB + 1);
    let glb = sparse("big.glb", 512 * MB + 1);
    // A mesh file and a point cloud file are read up to the same limit (#14548). The mesh file
    // that is read is an OBJ: an STL reader takes a sparse file for the ten million triangles
    // its length promises.
    let stl = sparse("big.stl", 512 * MB + 1);
    let obj = sparse("big.obj", 512 * MB + 1);
    let cloud = sparse("big.ply", 1024 * MB + 1);
    let mut s = Session::default();
    assert_eq!(limits::effective(), FileLimits::DEFAULT);
    for path in [&step, &design, &glb] {
        let error = s.execute("doc.open", &json!({"path": path})).unwrap_err().to_string();
        assert!(error.contains("too large") && error.contains("Largest file to open") && error.contains("--max-file-size"), "{error}");
    }
    // The mesh file and the point cloud file are refused the same way: before they are read, and
    // with the message that says how to raise the limit.
    let error = s.execute("file.insert_mesh", &json!({"path": stl})).unwrap_err().to_string();
    assert!(error.contains("too large") && error.contains("Largest file to open") && error.contains("--max-file-size"), "{error}");
    let error = s.execute("pointcloud.insert", &json!({"path": cloud})).unwrap_err().to_string();
    assert!(error.contains("too large") && error.contains("Largest file to open") && error.contains("--max-file-size"), "{error}");
    // The preference raises both limits; the design is now read (and refused only for not being one).
    limits::set_preferred(FileLimits { step: GB, design: GB }).unwrap();
    assert_eq!((limits::max_step_bytes(), limits::max_design_bytes()), (1 << 30, 1 << 30));
    // The STEP parser's own cap follows, so it doesn't refuse what the size check let in (#13329).
    assert_eq!(solidcraft_kernel::step_max_bytes(), 1 << 30);
    assert!(limits::check(Kind::Step, 512 * MB + 1).is_ok());
    let error = s.execute("doc.open", &json!({"path": design})).unwrap_err().to_string();
    assert!(!error.contains("too large"), "{error}");
    // The glTF file follows the limit for STEP and import files: read, and refused only for not
    // being a glTF file.
    let error = s.execute("doc.open", &json!({"path": glb})).unwrap_err().to_string();
    assert!(!error.contains("too large"), "{error}");
    // A mesh file follows the same limit: read, and refused only for not holding a mesh.
    let error = s.execute("file.insert_mesh", &json!({"path": obj})).unwrap_err().to_string();
    assert!(!error.contains("too large"), "{error}");
    // A point cloud file follows the limit for import files too, and is never read under the
    // 1 GB it has always had (#14548).
    assert_eq!(limits::Kind::PointCloud.limit(), GB);
    // The flag overrides the preference for the run, and the ceiling holds for both.
    limits::set_override(Some(2 * GB)).unwrap();
    assert_eq!(limits::effective(), FileLimits { step: 2 * GB, design: 2 * GB });
    assert_eq!(limits::Kind::PointCloud.limit(), 2 * GB, "the flag raises the point cloud limit too");
    assert_eq!(solidcraft_kernel::step_max_bytes(), 2 << 30);
    assert!(limits::set_override(Some(16 * GB + 1)).is_err());
    assert!(limits::set_preferred(FileLimits { step: 17 * GB, design: GB }).is_err());
    assert_eq!(limits::preferred(), FileLimits { step: GB, design: GB }, "a refused value changes nothing");
    let _ = limits::set_override(None);
    let _ = limits::set_preferred(FileLimits::DEFAULT);
    assert_eq!(solidcraft_kernel::step_max_bytes(), 512 << 20, "the defaults put the parser's cap back");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #15588: a STEP file that may need more memory than the machine can spare opens all the same,
/// with a warning naming that memory; a valid file is never refused. Later rebuilds read its text
/// again without asking.
#[test]
fn a_step_file_too_large_for_the_memory_opens_with_a_warning() {
    let path = step_file("memory", 4);
    let size = std::fs::metadata(&path).unwrap().len() as usize;
    solidcraft_kernel::step_with_memory_to_spare(size, || {
        // `doc.open` without the import cache, so the file itself is read.
        for (cmd, params) in [("doc.open", json!({"path": path, "reuse_import": false})), ("file.insert_step", json!({"path": path}))] {
            let mut s = Session::default();
            let reply = s.execute(cmd, &params).unwrap();
            let warning = reply["warnings"][0].as_str().unwrap_or_default();
            assert!(warning.contains("STEP file needs about") && warning.contains("opening it anyway"), "{cmd}: {reply}");
            assert_eq!(s.model.state().bodies.len(), 4, "{cmd}");
            // A rebuild reads the Import feature's text again, and is not refused.
            assert!(solidcraft_kernel::step_import(&step_text(&s)).is_ok(), "{cmd}");
        }
    });
    // With the memory there, no warning.
    let mut s = Session::default();
    let reply = s.execute("file.insert_step", &json!({"path": path})).unwrap();
    assert!(
        !reply["warnings"].as_array().is_some_and(|w| w.iter().any(|w| w.as_str().is_some_and(|w| w.contains("STEP file needs about")))),
        "{reply}"
    );
    let _ = std::fs::remove_file(path);
}
