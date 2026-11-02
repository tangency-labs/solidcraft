use super::*;
use solidcraft_doc::{Document, FeatureKind, MeshData};
use solidcraft_geom::Vec3;

fn imported_box() -> Session {
    let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
    let mesh = body.tessellate(0.01).unwrap();
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

/// The size cap of a copy to a network folder (#2304) is process-wide: a test that sends to one
/// holds this, and leaves the cap as it was however it ends.
struct NetworkFolders {
    _turn: std::sync::MutexGuard<'static, ()>,
    root: std::path::PathBuf,
}

impl NetworkFolders {
    fn new(tag: &str) -> NetworkFolders {
        let turn = crate::remote_slicers::TEST_TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = std::env::temp_dir().join(format!("solidcraft-network-folder-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("share")).unwrap();
        NetworkFolders { _turn: turn, root }
    }

    fn share(&self) -> std::path::PathBuf {
        self.root.join("share")
    }

    fn files(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.share()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }
}

impl Drop for NetworkFolders {
    fn drop(&mut self) {
        *crate::remote_slicers::TEST_MAX_BYTES.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *crate::remote_sftp::TEST_SETUP.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn network_folders_are_added_listed_tested_and_removed_by_command() {
    let net = NetworkFolders::new("commands");
    let mut s = imported_box();
    let share = net.share().to_string_lossy().into_owned();
    let added = s.execute("file.slicers.add_remote", &json!({"name": " Orca on homelab ", "folder": share})).unwrap();
    assert_eq!(added["id"], "remote:Orca on homelab");
    assert_eq!((added["kind"].as_str(), added["replaced"].as_bool()), (Some("folder"), Some(false)));
    let listed = s.execute("file.slicers", &json!({})).unwrap();
    let first = &listed["slicers"][0];
    assert_eq!(
        (first["id"].as_str(), first["remote"].as_bool(), first["folder"].as_str()),
        (Some("remote:Orca on homelab"), Some(true), Some(share.as_str()))
    );
    let again = s.execute("file.slicers.add_remote", &json!({"name": "orca on HOMELAB", "folder": share})).unwrap();
    assert_eq!(again["replaced"], true, "the same name in another case replaces it");
    assert_eq!(s.network_folders.len(), 1);

    let ok = s.execute("file.slicers.test", &json!({"name": "remote:orca on homelab"})).unwrap();
    assert_eq!((ok["ok"].as_bool(), ok["error"].is_null()), (Some(true), true));
    assert!(net.files().is_empty(), "the test file is deleted");
    let gone = net.root.join("not-mounted").to_string_lossy().into_owned();
    let missing = s.execute("file.slicers.test", &json!({"folder": gone})).unwrap();
    assert_eq!(missing["ok"], false);
    assert!(missing["error"].as_str().unwrap().contains("mounted"), "{missing}");

    for bad in [
        json!({"name": "", "folder": share}),
        json!({"name": "x".repeat(65), "folder": share}),
        json!({"name": "Relative", "folder": "some/folder"}),
        json!({"name": "No folder"}),
    ] {
        assert!(s.execute("file.slicers.add_remote", &bad).is_err(), "{bad}");
    }
    // A key the command doesn't take is kept nowhere: warned about, or refused when checks are strict.
    let extra = json!({"name": "Extra", "folder": share, "password": "never"});
    s.param_check = crate::schema::ParamCheck::Warn;
    let warned = s.execute("file.slicers.add_remote", &extra).unwrap();
    assert!(warned["param_warnings"].to_string().contains("password"), "{warned}");
    assert!(!serde_json::to_string(&s.network_folders).unwrap().contains("never"));
    s.execute("file.slicers.remove_remote", &json!({"name": "remote:Extra"})).unwrap();
    s.param_check = crate::schema::ParamCheck::Strict;
    assert!(s.execute("file.slicers.add_remote", &extra).is_err());
    assert!(crate::remote_slicers::resolve(&s.network_folders, "Extra").is_none());
    s.param_check = crate::schema::ParamCheck::Off;
    assert!(s.execute("file.slicers.test", &json!({})).is_err());
    assert!(s.execute("file.slicers.test", &json!({"name": "Orca on homelab", "folder": share})).is_err());
    assert!(s.execute("file.slicers.test", &json!({"name": "Nobody"})).is_err());

    // The user's, not the design's: they stay when another design is started.
    assert!(!s.doc.to_json().contains(&share), "never saved in the design");
    s.execute("file.new", &json!({})).unwrap();
    assert_eq!(s.network_folders.len(), 1, "kept across designs");

    let removed = s.execute("file.slicers.remove_remote", &json!({"name": "Orca on homelab"})).unwrap();
    assert_eq!(removed["removed"], true);
    assert!(s.execute("file.slicers.remove_remote", &json!({"name": "Orca on homelab"})).is_err());
    assert!(s.execute("file.slicers", &json!({})).unwrap()["slicers"].as_array().unwrap().iter().all(|t| t["remote"].is_null()));
}

#[test]
fn sending_to_a_network_folder_writes_a_readable_name_and_never_replaces_a_file() {
    let net = NetworkFolders::new("send");
    let mut s = imported_box();
    let share = net.share().to_string_lossy().into_owned();
    s.execute("file.slicers.add_remote", &json!({"name": "Orca", "folder": share})).unwrap();
    // A file of the readable name is already there, from someone else.
    std::fs::write(net.share().join("Untitled - Mesh.3mf"), b"theirs").unwrap();
    let send = |s: &mut Session, p: Value| s.execute("file.send_to_slicer", &p).unwrap();
    let first = send(&mut s, json!({"slicer": "remote:Orca", "bodies": ["Mesh"]}));
    assert_eq!((first["remote"].as_bool(), first["slicer"].as_str(), first["renamed"].as_bool()), (Some(true), Some("Orca"), Some(true)));
    assert!(first["slicer_error"].is_null(), "{first}");
    let second = send(&mut s, json!({"slicer": "orca", "bodies": ["Mesh"]}));
    assert!(second["sent_to"].as_str().unwrap().ends_with("Untitled - Mesh (3).3mf"), "{second}");
    assert_eq!(std::fs::read(net.share().join("Untitled - Mesh.3mf")).unwrap(), b"theirs");
    let sent = std::fs::read(first["sent_to"].as_str().unwrap()).unwrap();
    assert!(sent.starts_with(b"PK"), "a 3MF, as the slicer folder's file");
    assert_eq!(sent, std::fs::read(first["path"].as_str().unwrap()).unwrap(), "the same bytes as the file saved here");

    // Format and STL encoding are kept.
    let stl = send(&mut s, json!({"slicer": "Orca", "bodies": ["Mesh"], "format": "stl"}));
    let ascii = send(&mut s, json!({"slicer": "Orca", "bodies": ["Mesh"], "format": "stl", "ascii": true}));
    assert!(stl["sent_to"].as_str().unwrap().ends_with("Untitled - Mesh.stl"), "{stl}");
    assert!(ascii["sent_to"].as_str().unwrap().ends_with("Untitled - Mesh (2).stl"), "{ascii}");
    assert!(!std::fs::read(stl["sent_to"].as_str().unwrap()).unwrap().starts_with(b"solid"));
    assert!(std::fs::read(ascii["sent_to"].as_str().unwrap()).unwrap().starts_with(b"solid"));
    assert_eq!(
        net.files(),
        ["Untitled - Mesh (2).3mf", "Untitled - Mesh (3).3mf", "Untitled - Mesh (2).stl", "Untitled - Mesh.3mf", "Untitled - Mesh.stl"]
            .map(String::from)
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    );

    // file.save_mesh with a network folder keeps the file's own name.
    let path = net.root.join("bracket v2.stl");
    let saved = s.execute("file.save_mesh", &json!({"path": path, "slicer": "Orca", "bodies": ["Mesh"]})).unwrap();
    assert!(saved["sent_to"].as_str().unwrap().ends_with("bracket v2.stl"), "{saved}");
    assert!(path.exists());

    // wait: return, as the window sends: answered at once, and the copy goes on in the background.
    let r = send(&mut s, json!({"slicer": "Orca", "bodies": ["Mesh"], "format": "stl", "wait": "return"}));
    assert!(r["sent_to"].is_null() && r["remote"] == true, "{r}");
    let ticket = r["sending"].as_u64().unwrap();
    let done = (0..500)
        .find_map(|_| {
            let answer = crate::remote_slicers::poll(ticket);
            if answer.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            answer
        })
        .unwrap()
        .unwrap();
    assert_eq!(done.path, net.share().join("Untitled - Mesh (3).stl"));
}

#[test]
fn an_sftp_server_is_tested_trusted_and_sent_to_by_command() {
    use crate::remote_sftp::test_server;
    let net = NetworkFolders::new("sftp");
    // Not the user's own known_hosts or ssh-agent: a server only this test knows.
    *crate::remote_sftp::TEST_SETUP.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(crate::remote_sftp::Setup { known_hosts: None, agent: None, confirmed: None, limit: std::time::Duration::from_secs(10) });
    let key = net.root.join("id_ed25519");
    std::fs::write(&key, test_server::key(7).to_openssh(russh::keys::ssh_key::LineEnding::LF).unwrap().as_bytes()).unwrap();
    let authorized = vec![test_server::key(7).public_key().clone()];
    let running = test_server::start(1, test_server::Options { authorized, ..Default::default() }, &net.root).unwrap();
    let fp = running.host_key.public_key().fingerprint(russh::keys::HashAlg::Sha256).to_string();
    let server = format!("maker@127.0.0.1:{}", running.port);
    let (share, key) = (net.share().to_string_lossy().into_owned(), key.to_string_lossy().into_owned());
    let mut s = imported_box();

    // An unknown server is never written to: the test says its key, to check and confirm.
    let unknown = s.execute("file.slicers.test", &json!({"sftp": server, "folder": share, "key": key})).unwrap();
    assert_eq!((unknown["ok"].as_bool(), unknown["sftp"].as_str()), (Some(false), Some(server.as_str())), "{unknown}");
    assert_eq!((unknown["host_key"]["fingerprint"].as_str(), unknown["host_key"]["trusted"].as_bool()), (Some(fp.as_str()), Some(false)));
    let confirmed = s.execute("file.slicers.test", &json!({"sftp": server, "folder": share, "key": key, "confirm": fp})).unwrap();
    assert_eq!((confirmed["ok"].as_bool(), confirmed["host_key"]["trusted"].as_bool()), (Some(true), Some(true)), "{confirmed}");
    assert!(net.files().is_empty(), "the test file is deleted");

    // Added without trusting its key, sending to it saves the file here and says why it went no further.
    s.execute("file.slicers.add_remote", &json!({"name": "Orca box", "sftp": server, "folder": share, "key": key})).unwrap();
    let refused = s.execute("file.send_to_slicer", &json!({"slicer": "Orca box", "bodies": ["Mesh"]})).unwrap();
    assert!(refused["slicer_error"].as_str().unwrap().contains("not a server you trust"), "{refused}");
    assert!(net.files().is_empty() && std::path::Path::new(refused["path"].as_str().unwrap()).exists());

    let added =
        s.execute("file.slicers.add_remote", &json!({"name": "Orca box", "sftp": server, "folder": share, "key": key, "host_key": fp})).unwrap();
    assert_eq!(
        (added["kind"].as_str(), added["sftp"].as_str(), added["host_key"].as_str(), added["replaced"].as_bool()),
        (Some("sftp"), Some(server.as_str()), Some(fp.as_str()), Some(true))
    );
    let listed = s.execute("file.slicers", &json!({})).unwrap();
    assert_eq!((listed["slicers"][0]["remote"].as_bool(), listed["slicers"][0]["kind"].as_str()), (Some(true), Some("sftp")));
    let tested = s.execute("file.slicers.test", &json!({"name": "remote:Orca box"})).unwrap();
    assert_eq!(tested["ok"], true, "{tested}");

    let send =
        |s: &mut Session| s.execute("file.send_to_slicer", &json!({"slicer": "remote:Orca box", "bodies": ["Mesh"], "quality": "fine"})).unwrap();
    let (first, second) = (send(&mut s), send(&mut s));
    let url = format!("sftp://{server}{share}/Untitled - Mesh");
    assert_eq!(first["sent_to"].as_str(), Some(format!("{url}.3mf").as_str()), "{first}");
    assert_eq!(second["sent_to"].as_str(), Some(format!("{url} (2).3mf").as_str()), "{second}");
    assert_eq!(net.files(), ["Untitled - Mesh (2).3mf", "Untitled - Mesh.3mf"]);
    assert_eq!(std::fs::read(net.share().join("Untitled - Mesh.3mf")).unwrap(), std::fs::read(first["path"].as_str().unwrap()).unwrap());

    for bad in [
        json!({"name": "B", "sftp": "printbox", "folder": share}),
        json!({"name": "B", "sftp": "maker@box:0", "folder": share}),
        json!({"name": "B", "sftp": server, "folder": ""}),
        json!({"name": "B", "sftp": server, "folder": share, "key": "id_ed25519"}),
        json!({"name": "B", "sftp": server, "folder": share, "host_key": "MD5:00:11"}),
        json!({"name": "B", "folder": share, "key": key}),
    ] {
        assert!(s.execute("file.slicers.add_remote", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("file.slicers.test", &json!({"folder": share, "confirm": fp})).is_err(), "confirm goes with a server");
    assert!(s.execute("file.slicers.test", &json!({"name": "Orca box", "sftp": server})).is_err());

    // An agent limited to folders sends to the server the user added, but names no server itself.
    let scope = solidcraft_io::vfs::PathScope::new([net.root.clone()]);
    solidcraft_io::vfs::scoped(Some(scope), || {
        let other = json!({"name": "Elsewhere", "sftp": server, "folder": share, "host_key": fp});
        assert!(s.execute("file.slicers.add_remote", &other).unwrap_err().to_string().contains("--allow-path"));
        assert!(s.execute("file.slicers.test", &json!({"sftp": server, "folder": share})).is_err());
        assert_eq!(s.execute("file.slicers.test", &json!({"name": "Orca box"})).unwrap()["ok"], true);
    });
    assert!(!s.doc.to_json().contains(&server), "never saved in the design");
}

#[test]
fn quality_reaches_the_file_sent_to_a_network_folder() {
    let net = NetworkFolders::new("quality");
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"diameter": 20, "height": 10}));
    let share = net.share().to_string_lossy().into_owned();
    s.execute("file.slicers.add_remote", &json!({"name": "Orca", "folder": share})).unwrap();
    let size = |s: &mut Session, quality: &str| {
        let r = s.execute("file.send_to_slicer", &json!({"slicer": "Orca", "format": "stl", "quality": quality, "bodies": ["Body1"]})).unwrap();
        std::fs::metadata(r["sent_to"].as_str().unwrap()).unwrap().len()
    };
    let (standard, finest) = (size(&mut s, "standard"), size(&mut s, "finest"));
    assert!(finest > standard, "finest follows the curve more closely: {finest} > {standard} bytes");
    assert!(s.execute("file.send_to_slicer", &json!({"slicer": "Orca", "quality": "coarse", "bodies": ["Body1"]})).is_err());
}

#[test]
fn a_network_folder_that_cannot_take_the_file_is_reported_and_nothing_crashes() {
    let net = NetworkFolders::new("errors");
    let mut s = imported_box();
    let gone = net.root.join("not-mounted");
    s.execute("file.slicers.add_remote", &json!({"name": "Gone", "folder": gone})).unwrap();
    s.execute("file.slicers.add_remote", &json!({"name": "Small", "folder": net.share()})).unwrap();

    // Not mounted: the file is saved here, the folder is not made, and the reply says why.
    let r = s.execute("file.send_to_slicer", &json!({"slicer": "Gone", "bodies": ["Mesh"]})).unwrap();
    let e = r["slicer_error"].as_str().unwrap();
    assert!(e.contains("Gone") && e.contains("mounted"), "{e}");
    assert!(r["sent_to"].is_null() && std::path::Path::new(r["path"].as_str().unwrap()).exists());
    assert!(!gone.exists(), "a missing folder is never made");

    // Over the size cap: refused before anything is copied.
    *crate::remote_slicers::TEST_MAX_BYTES.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(100);
    let r = s.execute("file.send_to_slicer", &json!({"slicer": "Small", "bodies": ["Mesh"]})).unwrap();
    assert!(r["slicer_error"].as_str().unwrap().contains("MB"), "{r}");
    assert!(net.files().is_empty());
    *crate::remote_slicers::TEST_MAX_BYTES.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;

    // No permission to write there.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(net.share(), std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root may write anywhere; the check means nothing then.
        if std::fs::write(net.share().join("probe"), b"x").is_err() {
            let r = s.execute("file.send_to_slicer", &json!({"slicer": "Small", "bodies": ["Mesh"]})).unwrap();
            assert!(r["slicer_error"].as_str().unwrap().contains("no permission"), "{r}");
        }
        std::fs::set_permissions(net.share(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // A network folder that isn't there is refused before anything is written.
    let before = s.execute("file.slicers", &json!({})).unwrap();
    assert!(s.execute("file.send_to_slicer", &json!({"slicer": "remote:Nobody", "bodies": ["Mesh"]})).is_err());
    let path = net.root.join("unknown.stl");
    assert!(s.execute("file.save_mesh", &json!({"path": path, "slicer": "remote:Nobody"})).is_err());
    assert!(!path.exists());
    assert_eq!(s.execute("file.slicers", &json!({})).unwrap(), before);
}
