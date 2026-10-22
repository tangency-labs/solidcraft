use super::*;
use solidcraft_doc::{Document, FeatureKind, MeshData};
use solidcraft_geom::Vec3;

fn imported_box() -> Session {
    let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
    let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
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
    assert_eq!(body.faces(0.01).unwrap().len(), 10);
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
    std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" > {}\n", seen.display())).unwrap();
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
    for _ in 0..100 {
        if seen.exists() && std::fs::metadata(&seen).unwrap().len() > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(std::fs::read_to_string(&seen).unwrap().trim(), file);
    // Sending the same bodies again replaces the file; the same name in the same private folder.
    let again = s.execute("file.send_to_slicer", &json!({"slicer": script})).unwrap();
    assert_eq!((again["path"].as_str(), again["numbered"].as_bool()), (Some(file.as_str()), Some(false)));
    let stl = s.execute("file.send_to_slicer", &json!({"slicer": script, "format": "stl"})).unwrap();
    assert!(stl["path"].as_str().unwrap().ends_with(".stl"));
    assert_ne!(stl["path"], r["path"]);
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
    // A binary STL: 80-byte header, a count, 50 bytes a triangle, and a box has 12.
    assert_eq!(seen[1].1.len(), 84 + 50 * 12);
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
