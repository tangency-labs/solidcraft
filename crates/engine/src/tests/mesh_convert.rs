use super::*;
use solidcore_record::Vec3;
use solidcraft_doc::{Document, FeatureKind, MeshData};

fn imported_box() -> Session {
    imported_mesh(&solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap(), 0.01)
}

/// A session holding `body` imported as a mesh, tessellated to `chord` mm.
fn imported_mesh(body: &solidcraft_kernel::Body, chord: f64) -> Session {
    let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(chord)).unwrap();
    let mut doc = Document::default();
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "public-safe-box.stl".into(),
            meshes: vec![MeshData {
                name: "Mesh".into(),
                positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                triangles: mesh.triangles.iter().flatten().copied().collect(),
                color: Some([0.2, 0.5, 0.8]),
            }],
        },
        Some("Imported mesh"),
    )
    .unwrap();
    Session::new(doc)
}

/// An edit keeps the design as it was for undo, and an imported mesh's triangles are shared
/// with that copy, not copied (a scan's are hundreds of megabytes): after an edit and its undo
/// the document holds the same arrays. They still save as plain arrays.
#[test]
fn edits_share_an_imported_meshs_triangles_with_undo() {
    let mut s = imported_box();
    let arrays = |s: &Session| match &s.doc.features[0].kind {
        FeatureKind::MeshImport { meshes, .. } => (meshes[0].positions.clone(), meshes[0].triangles.clone()),
        other => panic!("{other:?}"),
    };
    let (positions, triangles) = arrays(&s);
    run(&mut s, "parameters.add", json!({"name":"size","expression":"2","unit":""}));
    let (p, t) = arrays(&s);
    assert!(std::sync::Arc::ptr_eq(&p, &positions) && std::sync::Arc::ptr_eq(&t, &triangles), "shared after an edit");
    run(&mut s, "edit.undo", json!({}));
    let (p, t) = arrays(&s);
    assert!(std::sync::Arc::ptr_eq(&p, &positions) && std::sync::Arc::ptr_eq(&t, &triangles), "shared after undo");
    let saved =
        serde_json::to_value(MeshData { name: "M".into(), positions: vec![1.5, 2.0, 3.0].into(), triangles: vec![0, 0, 0].into(), color: None })
            .unwrap();
    assert_eq!(saved, json!({"name":"M","positions":[1.5,2.0,3.0],"triangles":[0,0,0]}));
    let back: MeshData = serde_json::from_value(saved).unwrap();
    assert_eq!(&back.positions[..], &[1.5, 2.0, 3.0]);
}

#[test]
fn mesh_conversion_is_parametric_undoable_and_editable_after_cold_reopen() {
    let mut s = imported_box();
    run(&mut s, "parameters.add", json!({"name":"size","expression":"1","unit":""}));
    run(&mut s, "solid.scale", json!({"bodies":["Mesh"],"factor":"size","name":"Mesh scale"}));
    run(&mut s, "mesh.convert", json!({"body":"Mesh","tolerance":"0.01 mm","name":"Reconstructed prism"}));
    let check = |s: &Session| {
        let state = s.model.state();
        let body = &state.body("Mesh").unwrap().body;
        assert!(!body.is_mesh());
        assert_eq!(body.faces(0.01).unwrap().len(), 6);
        assert_eq!(body.color(), Some([0.2, 0.5, 0.8]));
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    };
    check(&s);
    assert!((volume(&mut s) - 6000.).abs() < 1e-6);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.model.state().body("Mesh").unwrap().body.is_mesh());
    run(&mut s, "edit.redo", json!({}));
    check(&s);
    run(&mut s, "parameters.change", json!({"name":"size","expression":"2"}));
    check(&s);
    assert!((volume(&mut s) - 48000.).abs() < 1e-5);
    let mut cold = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    check(&cold);
    assert!((volume(&mut cold) - 48000.).abs() < 1e-5);
    run(&mut cold, "solid.fillet", json!({"body":"Mesh","edges":[[0,20,60]],"radius":2}));
    assert!(volume(&mut cold) < 48000.);
    assert!(cold.model.results.iter().all(|r| r.error.is_none()));
}

#[test]
fn failed_mesh_conversions_leave_the_design_unchanged() {
    let mut s = imported_box();
    let before = s.doc.to_json();
    for params in [
        json!({}),
        json!({"body":"Missing"}),
        json!({"body":"Mesh","tolerance":0}),
        json!({"body":"Mesh","tolerance":"unknown"}),
        json!({"body":"Mesh","tolerance":"2 mm"}),
    ] {
        assert!(s.execute("mesh.convert", &params).is_err());
        assert_eq!(s.doc.to_json(), before);
        assert!(s.model.state().body("Mesh").unwrap().body.is_mesh());
    }
}

#[test]
fn convert_uses_the_selected_mesh_body_when_body_is_omitted() {
    let mut s = imported_box();
    s.selection = vec![crate::Sel::Body { name: "Mesh".into() }];
    run(&mut s, "mesh.convert", json!({"tolerance":"0.01 mm"}));
    let state = s.model.state();
    let body = &state.body("Mesh").unwrap().body;
    assert!(!body.is_mesh());
    assert_eq!(body.faces(0.01).unwrap().len(), 6);
}

#[test]
fn convert_without_body_or_selection_is_refused() {
    let mut s = imported_box();
    let before = s.doc.to_json();
    assert!(s.execute("mesh.convert", &json!({"tolerance":"0.01 mm"})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert!(s.model.state().body("Mesh").unwrap().body.is_mesh());
}

#[test]
fn an_explicit_body_wins_over_the_selection() {
    let mut s = imported_box();
    s.selection = vec![crate::Sel::Body { name: "Missing".into() }];
    run(&mut s, "mesh.convert", json!({"body":"Mesh","tolerance":"0.01 mm"}));
    assert!(!s.model.state().body("Mesh").unwrap().body.is_mesh());
}

#[test]
fn exported_binary_stl_with_a_bore_reconstructs_analytic_faces() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Section"}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[40,0],[40,20],[20,20],[20,30],[0,30]],"closed":true}));
    run(&mut s, "sketch.circle.center", json!({"center":[10,10],"radius":4}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Section","profiles":[{"point":[30,10]}],"distance":15,"body_name":"Bracket"}));
    let path = std::env::temp_dir().join(format!("solidcraft-mesh-convert-{}.stl", std::process::id()));
    run(&mut s, "file.save_mesh", json!({"path":path}));
    run(&mut s, "doc.open", json!({"path":path}));
    let name = s.model.state().bodies[0].name.clone();
    run(&mut s, "mesh.convert", json!({"body":name,"tolerance":"0.05 mm"}));
    let state = s.model.state();
    let body = &state.bodies[0].body;
    assert!(!body.is_mesh());
    assert!(body.validity().is_empty());
    assert_eq!(solidcraft_kernel::measure(body).unwrap().merged.face_types.get("cylinder"), Some(&1));
    // The L's six sides, the two caps and the bore, one full-turn wall.
    assert_eq!(body.faces(0.01).unwrap().len(), 9);
    std::fs::remove_file(path).unwrap();
}

/// #784: STL is closed as written where faces meet (a fillet, a bored hole): every edge of the
/// file, keyed by its exact f32 ends, is shared by exactly two triangles, as in 3MF.
#[test]
fn stl_export_is_closed_where_faces_meet() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 10, "operation": "new", "body_name": "BoxFillet"}));
    run(&mut s, "solid.fillet", json!({"body": "BoxFillet", "edges": [[15, 0, 10]], "radius": 2}));
    run(&mut s, "solid.cylinder", json!({"diameter": 30, "height": 12, "base": [60, 0, 0], "operation": "new", "body_name": "CylHole"}));
    run(&mut s, "solid.hole", json!({"position": [60, 0, 12], "diameter": 6}));
    let state = s.model.state();
    for name in ["BoxFillet", "CylHole"] {
        let body = &state.bodies.iter().find(|b| b.name == name).unwrap().body;
        let expected = solidcraft_kernel::measure(body).unwrap().volume;
        for format in [solidcraft_io::Format::StlBinary, solidcraft_io::Format::StlAscii] {
            let bytes = solidcraft_io::export(&state, &[name.to_string()], format, name).unwrap();
            let tris = solidcraft_io::read_stl(&bytes).unwrap();
            let key = |p: [f32; 3]| p.map(f32::to_bits);
            let mut edges = std::collections::HashMap::new();
            for t in &tris {
                for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    let (a, b) = (key(a), key(b));
                    *edges.entry(if a < b { (a, b) } else { (b, a) }).or_insert(0) += 1;
                }
            }
            let open = edges.values().filter(|n| **n != 2).count();
            assert_eq!(open, 0, "{name} {format:?}: {open} edges not shared by exactly two triangles");
            let p = |v: [f32; 3]| Vec3::new(v[0] as f64, v[1] as f64, v[2] as f64);
            let v: f64 = tris.iter().map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0).sum();
            assert!((v - expected).abs() <= expected * 1e-3, "{name} {format:?}: STL volume {v}, body {expected}");
            let mf = solidcraft_io::export(&state, &[name.to_string()], solidcraft_io::Format::ThreeMf, name).unwrap();
            let (objs, _) = solidcraft_io::read_3mf(&mf).unwrap();
            assert_eq!(tris.len(), objs.iter().map(|o| o.triangles.len()).sum::<usize>(), "{name} {format:?}: STL and 3MF triangles");
        }
    }
}

#[test]
fn save_mesh_rejects_unknown_format_and_slicer_without_writing() {
    let mut s = imported_box();
    // Its own file name: `save_mesh_writes_stl_and_3mf` writes `solvecraft-save-mesh-<pid>.stl`, and
    // sharing it made this one fail when the two ran together.
    let path = std::env::temp_dir().join(format!("solvecraft-save-mesh-rejected-{}.stl", std::process::id()));
    let _ = std::fs::remove_file(&path);
    for params in
        [json!({"path":path,"format":"nope"}), json!({"path":path,"slicer":"no such slicer anywhere"}), json!({"path":path,"slicer":"/bin/sh"})]
    {
        assert!(s.execute("file.save_mesh", &params).is_err());
        assert!(!path.exists());
    }
}

#[test]
fn save_mesh_writes_stl_and_3mf() {
    let mut s = imported_box();
    let dir = std::env::temp_dir();
    for (ext, format) in [("stl", None), ("3mf", None), ("dat", Some("3mf"))] {
        let path = dir.join(format!("solvecraft-save-mesh-{}.{ext}", std::process::id()));
        let mut params = json!({"path": path});
        if let Some(f) = format {
            params["format"] = json!(f);
        }
        s.execute("file.save_mesh", &params).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // A 3MF file is a zip archive; a binary STL starts with its 80-byte header.
        assert_eq!(bytes.starts_with(b"PK"), ext == "3mf" || format == Some("3mf"), "{ext}");
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn installed_slicers_are_listed() {
    let mut s = imported_box();
    let r = s.execute("file.slicers", &json!({})).unwrap();
    assert!(r["slicers"].is_array());
}

#[test]
fn send_to_slicer_needs_a_slicer_and_bodies() {
    let mut s = imported_box();
    assert!(s.execute("file.send_to_slicer", &json!({"bodies":["Mesh"]})).is_err());
    assert!(s.execute("file.send_to_slicer", &json!({"slicer":"no such slicer anywhere","bodies":["Mesh"]})).is_err());
    // Nothing selected and nothing named.
    assert!(s.execute("file.send_to_slicer", &json!({"slicer":"no such slicer anywhere"})).is_err());
}

/// The chosen-programs list is process-wide: tests that set it take turns.
#[cfg(unix)]
static CHOSEN_PROGRAMS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `file.save_mesh` with a `slicer` writes where the caller said and hands that file over, by the
/// same steps as `file.send_to_slicer`.
#[cfg(unix)]
#[test]
fn save_mesh_with_a_chosen_program_writes_the_file_where_asked_and_sends_it_there() {
    use std::os::unix::fs::PermissionsExt;
    let _turn = CHOSEN_PROGRAMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = std::env::temp_dir().join(format!("solidcraft-save-mesh-send-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (script, seen, out) = (dir.join("fake-slicer"), dir.join("seen"), dir.join("part.stl"));
    std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" >> {}\n", seen.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let spec = script.to_string_lossy().into_owned();
    crate::slicers::set_chosen_programs(std::slice::from_ref(&spec));
    let mut s = imported_box();
    let r = s.execute("file.save_mesh", &json!({"path": out, "slicer": spec, "single_instance": false}));
    crate::slicers::set_chosen_programs(&[]);
    let r = r.unwrap();
    assert_eq!(r["path"].as_str(), out.to_str());
    assert!(out.exists(), "written where it was asked");
    assert!(r.get("slicer").is_some() && r.get("slicer_error").is_none(), "{r}");
    for _ in 0..100 {
        if seen.exists() && std::fs::metadata(&seen).unwrap().len() > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert_eq!(std::fs::read_to_string(&seen).unwrap().trim(), out.to_str().unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

/// Puts back what the tests of the send commands make up, even when one fails.
#[cfg(unix)]
struct MadeUp;

#[cfg(unix)]
impl Drop for MadeUp {
    fn drop(&mut self) {
        *crate::slicers::TEST_DETECTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *crate::slicers::TEST_BUSCTL.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        crate::slicers::set_chosen_programs(&[]);
    }
}

/// The entry point of the first reported failure: a path that `file.slicers` lists is accepted by
/// `file.save_mesh`, the file is written where asked, the result keeps its contract, and a path that
/// is not listed, or that several entries share, is refused without writing anything.
#[cfg(unix)]
#[test]
fn save_mesh_accepts_the_path_of_a_listed_appimage_and_refuses_the_others() {
    use crate::slicers::{Slicer, TEST_DETECTED, test_support::fake_program};
    let _turn = CHOSEN_PROGRAMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _reset = MadeUp;
    let dir = std::env::temp_dir().join(format!("solidcraft-save-mesh-listed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let seen = dir.join("seen");
    let image = fake_program(&dir, "PrusaSlicer-2.9.AppImage", &format!("echo \"$@\" >> {}", seen.display()));
    let other = fake_program(&dir, "other", "exit 0");
    let launcher = fake_program(&dir, "shared-launcher", "exit 0");
    let entry = |name: &str, command: Vec<String>| Slicer { id: name.into(), name: name.into(), command, icon: None, prusa_major: None };
    *TEST_DETECTED.lock().unwrap() = Some(vec![
        entry("prusa", vec![image.clone()]),
        entry("one", vec![launcher.clone(), "run".into(), "--branch=a".into()]),
        entry("two", vec![launcher.clone(), "run".into(), "--branch=b".into()]),
    ]);
    let mut s = imported_box();
    let plain = dir.join("plain.stl");
    let plain_result = s.execute("file.save_mesh", &json!({"path": plain})).unwrap();
    // Listed: accepted, written where asked, the same export as without a slicer, then handed over.
    let out = dir.join("part.stl");
    let r = s.execute("file.save_mesh", &json!({"path": out, "slicer": image, "single_instance": false})).unwrap();
    assert_eq!((&r["bytes"], &r["format"]), (&plain_result["bytes"], &plain_result["format"]), "the export's result is as before");
    assert_eq!(r["path"].as_str(), out.to_str());
    // The same STL; its 80-byte header carries the file's name, which differs.
    assert_eq!(std::fs::read(&out).unwrap()[80..], std::fs::read(&plain).unwrap()[80..]);
    assert!(r.get("slicer").is_some() && r.get("slicer_error").is_none(), "{r}");
    for _ in 0..100 {
        if std::fs::metadata(&seen).map(|m| m.len() > 0).unwrap_or(false) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert!(std::fs::read_to_string(&seen).unwrap().trim().ends_with(out.to_str().unwrap()));
    // Not listed, and shared by two entries: refused, and nothing is written.
    for (spec, name) in [(other.as_str(), "other.stl"), (launcher.as_str(), "ambiguous.stl"), ("/bin/sh", "sh.stl")] {
        let path = dir.join(name);
        assert!(s.execute("file.save_mesh", &json!({"path": path, "slicer": spec})).is_err(), "{spec}");
        assert!(!path.exists(), "{spec}: nothing written");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// The entry point of the second: a PrusaSlicer 3 that is not running is started on its own, waited
/// for, and only then sent the file `file.save_mesh` wrote.
#[cfg(unix)]
#[test]
fn save_mesh_with_a_cold_prusaslicer_3_waits_for_it_before_sending() {
    use crate::slicers::{Slicer, TEST_BUSCTL, TEST_DETECTED, test_support::fake_program};
    let _turn = CHOSEN_PROGRAMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _reset = MadeUp;
    let dir = std::env::temp_dir().join(format!("solidcraft-save-mesh-cold-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let launcher_dir = dir.join("slic3r-app-launcher");
    std::fs::create_dir_all(&launcher_dir).unwrap();
    let log = dir.join("log");
    let launcher = fake_program(&launcher_dir, "PrusaSlicer", &format!("echo \"$@\" >> {}", log.display()));
    let count = dir.join("count");
    // Not on the bus the first two looks, there after that.
    let busctl = fake_program(
        &dir,
        "busctl",
        &format!(
            "n=$(cat {c} 2>/dev/null || echo 0); echo $((n+1)) > {c}; echo ':1.5 1 x'; [ $n -ge 2 ] && echo 'com.prusa3d.prusaslicer.BroadcastListener.ab 2 y'; exit 0",
            c = count.display()
        ),
    );
    *TEST_BUSCTL.lock().unwrap() = Some(busctl);
    *TEST_DETECTED.lock().unwrap() =
        Some(vec![Slicer { id: "prusa".into(), name: "PrusaSlicer".into(), command: vec![launcher], icon: None, prusa_major: Some(3) }]);
    let mut s = imported_box();
    let out = dir.join("part.stl");
    let r = s.execute("file.save_mesh", &json!({"path": out, "slicer": "prusa", "single_instance": false})).unwrap();
    assert!(out.exists());
    assert!(r.get("slicer").is_some() && r.get("slicer_timeout").is_none(), "{r}");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let lines: Vec<String> = std::fs::read_to_string(&log).unwrap().lines().map(String::from).collect();
    // Started with no file, then sent the file, both with the flag (the preference could not take it away).
    assert_eq!(lines, ["--single-instance".to_string(), format!("--single-instance {}", out.display())]);
    assert!(std::fs::read_to_string(&count).unwrap().trim().parse::<u32>().unwrap() >= 3, "it looked until it was up");
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn unsaved_designs_of_one_name_send_to_different_files_and_one_design_to_one() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("solidcraft-open-in-slicer-uid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake-slicer");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let spec = script.to_string_lossy().into_owned();
    let _turn = CHOSEN_PROGRAMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut a, mut b) = (imported_box(), imported_box());
    assert_eq!((a.path.is_none(), a.doc.name == b.doc.name), (true, true));
    // Chosen list is process-wide: set it for this test, clear it before leaving.
    crate::slicers::set_chosen_programs(std::slice::from_ref(&spec));
    let send = |s: &mut Session| {
        s.execute("file.send_to_slicer", &json!({"slicer": spec, "bodies": ["Mesh"]})).map(|r| r["path"].as_str().unwrap().to_string())
    };
    let (a1, a2, b1) = (send(&mut a), send(&mut a), send(&mut b));
    crate::slicers::set_chosen_programs(&[]);
    let (a1, a2, b1) = (a1.unwrap(), a2.unwrap(), b1.unwrap());
    assert_eq!(a1, a2, "sending again from one document keeps one file");
    assert_ne!(a1, b1, "two unsaved documents with the same name and bodies get their own files");
    std::fs::remove_dir_all(std::path::Path::new(&a1).parent().unwrap()).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

/// The first Save As gives the design a path, and with it a new file name, once. Later sends keep
/// the name that comes from the saved path.
#[cfg(unix)]
#[test]
fn the_first_save_as_changes_the_file_name_once() {
    use std::os::unix::fs::PermissionsExt;
    let _turn = CHOSEN_PROGRAMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = std::env::temp_dir().join(format!("solidcraft-open-in-slicer-saveas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake-slicer");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let spec = script.to_string_lossy().into_owned();
    let mut s = imported_box();
    crate::slicers::set_chosen_programs(std::slice::from_ref(&spec));
    let send = |s: &mut Session| {
        s.execute("file.send_to_slicer", &json!({"slicer": spec, "bodies": ["Mesh"]})).map(|r| r["path"].as_str().unwrap().to_string())
    };
    let before = [send(&mut s), send(&mut s)];
    // What Save As does to the session: the design now has a path.
    s.path = Some("/designs/box.solidcraft".into());
    let after = [send(&mut s), send(&mut s), send(&mut s)];
    crate::slicers::set_chosen_programs(&[]);
    let (before, after) = (before.map(Result::unwrap), after.map(Result::unwrap));
    assert_eq!(before[0], before[1], "unsaved: one file");
    assert_ne!(before[0], after[0], "Save As: the name changes");
    assert!(after.iter().all(|p| *p == after[0]), "saved: the same name every time");
    std::fs::remove_dir_all(std::path::Path::new(&after[0]).parent().unwrap()).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn send_to_slicer_writes_3mf_and_starts_the_program_with_it() {
    use std::os::unix::fs::PermissionsExt;
    let _turn = CHOSEN_PROGRAMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = imported_box();
    let dir = std::env::temp_dir().join(format!("solidcraft-open-in-slicer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (script, seen) = (dir.join("fake-slicer"), dir.join("seen"));
    // Publish the acknowledgement only after writing it: cleanup must wait for every launch.
    let writing = dir.join("writing");
    std::fs::write(&script, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nmv '{}' '{}'\n", writing.display(), writing.display(), seen.display()))
        .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    run(&mut s, "select.set", json!({"items":[{"type":"body","name":"Mesh"}]}));
    // A program nobody chose is refused; once it is among the user's chosen ones it starts.
    assert!(s.execute("file.send_to_slicer", &json!({"slicer": script})).is_err());
    crate::slicers::set_chosen_programs(&[script.to_string_lossy().into_owned()]);
    let r = s.execute("file.send_to_slicer", &json!({"slicer": script}));
    // Never leave the chosen list set for other tests in this process.
    if r.is_err() {
        crate::slicers::set_chosen_programs(&[]);
    }
    let r = r.unwrap();
    assert_eq!(r["format"], "3mf");
    let file = r["path"].as_str().unwrap().to_string();
    assert!(std::fs::read(&file).unwrap().starts_with(b"PK"));
    let await_launch = |expected: &str| {
        for _ in 0..100 {
            if seen.exists() {
                assert_eq!(std::fs::read_to_string(&seen).unwrap().trim(), expected);
                std::fs::remove_file(&seen).unwrap();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("fake slicer did not acknowledge {expected}");
    };
    await_launch(&file);
    // Sending the same bodies again replaces the file; the same name in the same private folder.
    let again = s.execute("file.send_to_slicer", &json!({"slicer": script})).unwrap();
    assert_eq!((again["path"].as_str(), again["numbered"].as_bool()), (Some(file.as_str()), Some(false)));
    await_launch(&file);
    let stl = s.execute("file.send_to_slicer", &json!({"slicer": script, "format": "stl"})).unwrap();
    assert!(stl["path"].as_str().unwrap().ends_with(".stl"));
    assert_ne!(stl["path"], r["path"]);
    await_launch(stl["path"].as_str().unwrap());
    crate::slicers::set_chosen_programs(&[]);
    std::fs::remove_dir_all(std::path::Path::new(&file).parent().unwrap()).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

type Uploads = std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

/// A session whose "host" records what it is handed and answers `answer`.
fn easyprint_host(s: &mut Session, answer: std::result::Result<(), String>) -> Uploads {
    let seen = Uploads::default();
    let log = seen.clone();
    s.easyprint_upload = Some(std::sync::Arc::new(move |name, bytes| {
        log.lock().unwrap().push((name.to_string(), bytes));
        answer.clone()
    }));
    seen
}

#[test]
fn open_in_easyprint_hands_the_host_a_3mf_or_an_stl_that_reads_back() {
    let mut s = imported_box();
    let seen = easyprint_host(&mut s, Ok(()));
    let r = s.execute("file.open_in_easyprint", &json!({"consent": true, "bodies": ["Mesh"]})).unwrap();
    assert_eq!(r["uploading"], true);
    s.execute("file.open_in_easyprint", &json!({"consent": true, "bodies": ["Mesh"], "format": "stl"})).unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!((seen[0].0.ends_with(".3mf"), seen[1].0.ends_with(".stl")), (true, true), "{:?}", seen.iter().map(|u| &u.0).collect::<Vec<_>>());
    assert!(seen[0].1.starts_with(b"PK"), "a 3MF is a zip");
    // The STL reads back as the imported mesh: its triangles, closed (every side shared by two
    // triangles, each way once), enclosing the 10 × 20 × 30 box's 6000 mm³.
    let imported = match &s.doc.features[0].kind {
        FeatureKind::MeshImport { meshes, .. } => meshes[0].triangles.len() / 3,
        other => panic!("{other:?}"),
    };
    let tris = solidcraft_io::read_stl(&seen[1].1).unwrap();
    assert_eq!(tris.len(), imported);
    let key = |p: [f32; 3]| p.map(f32::to_bits);
    let mut sides = std::collections::HashMap::<_, i32>::new();
    for t in &tris {
        for i in 0..3 {
            let (a, b) = (key(t[i]), key(t[(i + 1) % 3]));
            *sides.entry(if a < b { (a, b) } else { (b, a) }).or_default() += if a < b { 1 } else { -1 };
        }
    }
    assert!(sides.values().all(|n| *n == 0), "an open or inconsistently wound STL");
    let at = |p: [f32; 3]| Vec3::new(p[0].into(), p[1].into(), p[2].into());
    let volume: f64 = tris.iter().map(|t| at(t[0]).dot(at(t[1]).cross(at(t[2]))) / 6.0).sum();
    assert!((volume - 6000.0).abs() < 1e-6, "{volume}");
    let (lo, hi) = tris
        .iter()
        .flatten()
        .fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| (std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i]))));
    assert_eq!((lo, hi), ([0.0; 3], [10.0, 20.0, 30.0]));
}

#[test]
fn open_in_easyprint_without_consent_a_host_or_bodies_uploads_nothing() {
    let mut s = imported_box();
    let e = s.execute("file.open_in_easyprint", &json!({"consent": true, "bodies": ["Mesh"]})).unwrap_err().to_string();
    assert!(e.contains("not supported yet"), "{e}");
    let seen = easyprint_host(&mut s, Ok(()));
    assert!(s.execute("file.open_in_easyprint", &json!({"bodies": ["Mesh"]})).is_err());
    assert!(s.execute("file.open_in_easyprint", &json!({"consent": false, "bodies": ["Mesh"]})).is_err());
    assert!(s.execute("file.open_in_easyprint", &json!({"consent": true})).is_err(), "nothing selected, nothing named");
    assert!(s.execute("file.open_in_easyprint", &json!({"consent": true, "bodies": ["Mesh"], "format": "step"})).is_err());
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn open_in_easyprint_reports_a_host_that_cannot_start_and_a_model_over_the_limit() {
    let mut s = imported_box();
    let seen = easyprint_host(&mut s, Err("no network".into()));
    let e = s.execute("file.open_in_easyprint", &json!({"consent": true, "bodies": ["Mesh"]})).unwrap_err().to_string();
    assert!(e.contains("no network"), "{e}");
    // An STL is 50 bytes a triangle: 2.1 million of them are over 100 MB.
    let mut doc = Document::default();
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "big.stl".into(),
            meshes: vec![MeshData {
                name: "Big".into(),
                positions: vec![0., 0., 0., 1., 0., 0., 0., 1., 0.].into(),
                triangles: [0u32, 1, 2].repeat(2_100_000).into(),
                color: None,
            }],
        },
        Some("Imported mesh"),
    )
    .unwrap();
    let mut big = Session::new(doc);
    let seen_big = easyprint_host(&mut big, Ok(()));
    let e = big.execute("file.open_in_easyprint", &json!({"consent": true, "bodies": ["Big"], "format": "stl"})).unwrap_err().to_string();
    assert!(e.contains("100 MB at most"), "{e}");
    assert!(seen_big.lock().unwrap().is_empty());
    assert_eq!(seen.lock().unwrap().len(), 1);
}

fn insert_box_mesh(s: &mut Session, suffix: &str) -> (String, u64) {
    let mut source = imported_box();
    let path = std::env::temp_dir().join(format!("solidcraft-convert-component-{}-{suffix}.stl", std::process::id()));
    run(&mut source, "file.save_mesh", json!({"path":path}));
    let result = run(s, "file.insert_mesh", json!({"path":path,"name":"Mesh 1"}));
    std::fs::remove_file(path).unwrap();
    let component = result["component"].as_u64().unwrap();
    let state = s.model.state();
    let body = state.bodies.iter().find(|body| s.doc.model_body_component(body) == component).unwrap().name.clone();
    (body, component)
}

#[test]
fn inserted_mesh_converts_in_its_owner_from_an_active_ancestor() {
    for nested in [false, true] {
        let mut s = Session::default();
        if nested {
            run(&mut s, "component.create", json!({"name":"Assembly"}));
        }
        let context = (s.active_component, s.active_occurrence_path.clone());
        let (body, owner) = insert_box_mesh(&mut s, if nested { "nested" } else { "root" });
        let before = s.doc.to_json();
        assert!(s.execute("mesh.convert", &json!({"body":body,"tolerance":"2 mm"})).is_err());
        assert_eq!(s.doc.to_json(), before);
        assert_eq!((s.active_component, s.active_occurrence_path.clone()), context);
        run(&mut s, "mesh.convert", json!({"body":body,"tolerance":"0.01 mm"}));
        assert_eq!((s.active_component, s.active_occurrence_path.clone()), context);
        let check = |s: &Session| {
            let state = s.model.state();
            let converted = state.body(&body).unwrap();
            assert!(!converted.body.is_mesh());
            assert_eq!(converted.body.faces(0.01).unwrap().len(), 6);
            assert_eq!(s.doc.model_body_component(converted), owner);
            assert_eq!(s.doc.feature(converted.feature).unwrap().component, owner);
            assert!(s.model.results.iter().all(|r| r.error.is_none()));
        };
        check(&s);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "edit.redo", json!({}));
        check(&s);
        check(&Session::new(Document::from_json(&s.doc.to_json()).unwrap()));
    }
}

/// `mesh.recognize` reads a mesh as the surfaces it stands for, and changes nothing: a box comes
/// back as six planes square to three directions, each face a wall for two of them and a cap for
/// the third, with the whole area accounted for (2 * (10*20 + 10*30 + 20*30) = 2200 mm^2).
#[test]
fn recognizing_a_box_mesh_names_six_planes_and_changes_nothing() {
    let mut s = imported_box();
    let before = s.doc.to_json();
    let reply = run(&mut s, "mesh.recognize", json!({"body":"Mesh"}));
    assert_eq!(reply["closed"], json!(true), "{reply}");
    assert_eq!(reply["freeform"].as_f64(), Some(0.), "{reply}");
    let patches = reply["patches"].as_array().unwrap();
    assert_eq!(patches.len(), 6, "{reply}");
    assert!(patches.iter().all(|p| p["surface"] == "plane" && p["deviation"].as_f64().unwrap() < 1e-9), "{reply}");
    assert!((reply["area"].as_f64().unwrap() - 2200.).abs() < 1e-6, "{reply}");
    let axes = reply["axes"].as_array().unwrap();
    assert_eq!(axes.len(), 3, "{reply}");
    for axis in axes {
        let (along, across) = (axis["along"].as_f64().unwrap(), axis["across"].as_f64().unwrap());
        assert!((along + across - 2200.).abs() < 1e-6, "every face is a wall or a cap: {axis}");
        assert!(across > 0. && along > 0., "{axis}");
    }
    assert_eq!(s.doc.to_json(), before, "recognition changes nothing");
    assert!(s.model.state().body("Mesh").unwrap().body.is_mesh(), "the mesh body is still a mesh");
}

/// A tessellated cylinder of radius 5 and height 12 comes back as one cylinder between two discs,
/// with its radius, its axis and how far it runs. The discs are the end caps of that axis:
/// 2 * pi * 25 = 157.08 mm^2 between them, a little less once faceted.
#[test]
fn recognizing_a_cylinder_mesh_gives_its_radius_axis_and_run() {
    let body = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 5., 12.).unwrap();
    let mut s = imported_mesh(&body, 0.01);
    let reply = run(&mut s, "mesh.recognize", json!({"body":"Mesh","tolerance":"0.05 mm"}));
    let patches = reply["patches"].as_array().unwrap();
    let round = patches.iter().find(|p| p["surface"] == "cylinder").unwrap_or_else(|| panic!("{reply}"));
    // The facets sit inside the true radius by the chord error, so the fitted radius is a little
    // under 5: it is the mesh that is faceted, not the fit that is wrong.
    assert!((round["radius"].as_f64().unwrap() - 5.).abs() < 0.02, "{round}");
    assert_eq!(round["axis"].as_array().unwrap()[2].as_f64().unwrap().abs(), 1., "the axis is Z: {round}");
    let along = round["along"].as_array().unwrap();
    assert!((along[1].as_f64().unwrap() - along[0].as_f64().unwrap() - 12.).abs() < 1e-6, "{round}");
    assert_eq!(patches.iter().filter(|p| p["surface"] == "plane").count(), 2, "{reply}");
    let axis = reply["axes"].as_array().unwrap().first().unwrap();
    assert!((axis["across"].as_f64().unwrap() - 2. * std::f64::consts::PI * 25.).abs() < 1., "the two discs: {axis}");
}

/// A proposed grouping decides only what is tried together: a group per triangle gives a patch
/// per triangle, and every one of them is still a certified plane. What cannot be read is refused.
#[test]
fn recognition_takes_a_proposed_grouping_and_refuses_what_it_cannot_read() {
    let mut s = imported_box();
    let triangles = run(&mut s, "mesh.recognize", json!({"body":"Mesh"}))["triangles"].as_u64().unwrap() as usize;
    let groups: Vec<u32> = (0..triangles as u32).collect();
    let reply = run(&mut s, "mesh.recognize", json!({"body":"Mesh","groups":groups}));
    assert_eq!(reply["found"].as_u64(), Some(triangles as u64), "{reply}");
    let patches = reply["patches"].as_array().unwrap();
    assert_eq!(patches.len(), triangles.min(256), "{reply}");
    assert!(patches.iter().all(|p| p["surface"] == "plane"), "a wrong grouping cannot invent a surface: {reply}");
    let fewer = run(&mut s, "mesh.recognize", json!({"body":"Mesh","groups":groups,"max_patches":3}));
    assert_eq!((fewer["patches"].as_array().unwrap().len(), fewer["found"].as_u64()), (3, Some(triangles as u64)), "{fewer}");
    for (params, expected) in [
        (json!({"body":"Mesh","groups":[1,2,3]}), "one per triangle"),
        (json!({"body":"Mesh","groups":[-1]}), "none of them negative"),
        (json!({"body":"Mesh","groups":"all"}), "one per triangle"),
        (json!({"body":"Mesh","max_patches":0}), "between 1 and 4096"),
        (json!({"body":"Mesh","tolerance":"5 mm"}), "between 0.000001 and 1 mm"),
        (json!({"body":"Nope"}), "no body `Nope`"),
    ] {
        let error = s.execute("mesh.recognize", &params).unwrap_err().to_string();
        assert!(error.contains(expected), "{params}: {error}");
    }
    // What conversion leaves behind is a solid, not a mesh body; undoing gives the mesh back and
    // it reads as the same six planes.
    run(&mut s, "mesh.convert", json!({"body":"Mesh","tolerance":"0.01 mm"}));
    let error = s.execute("mesh.recognize", &json!({"body":"Mesh"})).unwrap_err().to_string();
    assert!(error.contains("select a mesh body"), "{error}");
    run(&mut s, "edit.undo", json!({}));
    let back = run(&mut s, "mesh.recognize", json!({"body":"Mesh"}));
    assert_eq!(back["patches"].as_array().unwrap().len(), 6, "{back}");
}

#[test]
fn conversion_from_another_branch_names_the_component_to_activate() {
    let mut s = Session::default();
    let (body, _) = insert_box_mesh(&mut s, "other-branch");
    run(&mut s, "component.create", json!({"name":"Other"}));
    let before = s.doc.to_json();
    let context = (s.active_component, s.active_occurrence_path.clone());
    let error = s.execute("mesh.convert", &json!({"body":body})).unwrap_err().to_string();
    assert!(error.contains("activate component 'Mesh 1' first"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    assert_eq!((s.active_component, s.active_occurrence_path.clone()), context);
}

/// A plate 40 x 30 x 10 with two holes drilled through it, as an imported mesh: the shape whose
/// answer the rebuild has to come back with.
fn imported_plate() -> Session {
    let plate = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
    let mut body = plate;
    for (at, radius) in [(Vec3::new(10., 10., 0.), 3.), (Vec3::new(30., 20., 0.), 2.5)] {
        let tool = solidcraft_kernel::cylinder(at - Vec3::Z, Vec3::Z, radius, 12.).unwrap();
        body = solidcraft_kernel::boolean(&body, &tool, solidcraft_kernel::BoolOp::Cut).unwrap().unwrap();
    }
    imported_mesh(&body, 0.01)
}

#[test]
fn a_mesh_is_rebuilt_as_a_sketch_an_extrusion_and_its_holes_which_the_parameters_then_drive() {
    let mut s = imported_plate();
    let frozen = volume(&mut s);
    let out = run(&mut s, "mesh.rebuild", json!({"body": "Mesh", "tolerance": "0.05 mm", "prefix": "plate"}));
    assert_eq!(out["outline"], "polygon");
    assert_eq!(out["corners"].as_array().map(Vec::len), Some(4), "{}", out["corners"]);
    assert_eq!(out["bores"].as_array().map(Vec::len), Some(2), "{}", out["bores"]);
    assert!(out["bores"].as_array().is_some_and(|b| b.iter().all(|b| b["through"] == true)), "{}", out["bores"]);
    assert!((out["thickness"].as_f64().unwrap_or_default() - 10.).abs() < 0.05, "{}", out["thickness"]);
    assert!(out["accounted"].as_f64().unwrap_or_default() > 0.99, "{} with {}", out["accounted"], out["unused"]);
    // The mesh body is left exactly as it was; the rebuild stands beside it.
    assert!(out["features"].as_array().is_some_and(|f| !f.is_empty()), "{}", out["features"]);
    let built = volume(&mut s) - frozen;
    let expect = 40. * 30. * 10. - PI * (9. + 6.25) * 10.;
    assert!(rel(built, expect) < 0.01, "{built} against {expect}");

    // Every size is a parameter the features follow, which is the whole point of rebuilding
    // rather than converting: the plate is 20 thick when its parameter says so.
    let named: Vec<String> =
        out["parameters"].as_array().map(|p| p.iter().filter_map(|p| p["name"].as_str().map(str::to_owned)).collect()).unwrap_or_default();
    assert!(named.contains(&"plate_thickness".to_owned()), "{named:?}");
    assert!(named.iter().any(|n| n.starts_with("plate_bore_1")), "{named:?}");
    run(&mut s, "parameters.change", json!({"changes": {"plate_thickness": "20 mm"}}));
    let taller = volume(&mut s) - frozen;
    assert!(rel(taller, 40. * 30. * 20. - PI * (9. + 6.25) * 20.) < 0.01, "{taller}");

    // The whole rebuild is one step of Undo, not one per command it ran.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(rel(volume(&mut s), frozen) < 1e-9, "{} against {frozen}", volume(&mut s));
    assert_eq!(s.doc.features.len(), 1, "{:?}", s.doc.features.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
}

#[test]
fn a_plan_only_rebuild_reports_the_commands_and_changes_nothing() {
    let mut s = imported_plate();
    let before = s.doc.to_json();
    let out = run(&mut s, "mesh.rebuild", json!({"body": "Mesh", "plan_only": true, "prefix": "plate"}));
    assert_eq!(s.doc.to_json(), before, "plan_only changed the design");
    assert!(out["features"].as_array().is_some_and(Vec::is_empty), "{}", out["features"]);
    let steps: Vec<&str> = out["steps"].as_array().map(|s| s.iter().filter_map(|s| s["command"].as_str()).collect()).unwrap_or_default();
    assert!(steps.contains(&"sketch.create") && steps.contains(&"solid.extrude") && steps.contains(&"solid.hole"), "{steps:?}");
    assert!(steps.iter().filter(|c| **c == "parameters.add").count() >= 3, "{steps:?}");
    // The reported steps are the ones it would run: sent as a batch they build the same part.
    let script: Vec<Value> =
        out["steps"].as_array().map(|s| s.iter().map(|s| json!({"command": s["command"], "params": s["params"]})).collect()).unwrap_or_default();
    s.run_script(&json!(script)).unwrap_or_else(|err| panic!("the reported steps did not run: {err}"));
    assert!(volume(&mut s) > 0.);
}

#[test]
fn a_rebuild_refuses_what_it_cannot_do_and_hostile_input_never_panics() {
    let mut s = imported_plate();
    for bad in [json!({"body": "Mesh", "align": 0}), json!({"body": "Mesh", "align": 90}), json!({"body": "Mesh", "axis": [0, 0, 0]})] {
        assert!(s.execute("mesh.rebuild", &bad).is_err(), "{bad}");
    }
    // A plate is prismatic along its width as well, and rebuilding it that way is a real answer:
    // 30 by 10 extruded 40. The holes then run across the extrusion instead of through it, so
    // they are not built, and the reply says so rather than quietly leaving them out.
    let sideways = run(&mut s, "mesh.rebuild", json!({"body": "Mesh", "axis": [1, 0, 0], "plan_only": true}));
    assert_eq!(sideways["axis"], json!([1.0, 0.0, 0.0]));
    assert!((sideways["thickness"].as_f64().unwrap_or_default() - 40.).abs() < 0.05, "{}", sideways["thickness"]);
    assert!(sideways["bores"].as_array().is_some_and(Vec::is_empty), "{}", sideways["bores"]);
    assert!(sideways["accounted"].as_f64().unwrap_or_default() < 0.95, "{}", sideways["accounted"]);
    let unused = sideways["unused"].to_string();
    assert!(unused.contains("does not turn about the build axis"), "{unused}");
    assert!(s.execute("mesh.rebuild", &json!({"body": "nope"})).is_err());
    assert!(s.execute("mesh.rebuild", &json!({"body": "Mesh", "groups": [1, 2]})).is_err());
    // Nothing above left the design changed.
    assert_eq!(s.doc.features.len(), 1);
}
