use super::*;
use solidcore_record::Vec3;
use solidcraft_doc::{Document, FeatureKind, MeshData};

/// A design whose only feature imports a 10 x 20 x 30 mm box as the mesh body `Mesh`. The
/// schema tests (`schema/mesh_tests.rs`) convert it too.
pub(crate) fn imported_box() -> Session {
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
                corner_normals: Default::default(),
            }],
        },
        Some("Imported mesh"),
    )
    .unwrap();
    Session::new(doc)
}

/// A closed box with planar grids dense enough to have interior collapsible edges.
pub(crate) fn subdivided_box() -> Session {
    let mut s = imported_box();
    let FeatureKind::MeshImport { meshes, .. } = &mut s.doc_mut().features[0].kind else { panic!("mesh") };
    let mesh = &mut meshes[0];
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    let faces = [
        ([0., 0., 0.], [0., 3., 0.], [3., 0., 0.]),
        ([0., 0., 12.], [3., 0., 0.], [0., 3., 0.]),
        ([0., 0., 0.], [3., 0., 0.], [0., 0., 3.]),
        ([0., 12., 0.], [0., 0., 3.], [3., 0., 0.]),
        ([0., 0., 0.], [0., 0., 3.], [0., 3., 0.]),
        ([12., 0., 0.], [0., 3., 0.], [0., 0., 3.]),
    ];
    for (origin, u, v) in faces {
        let base = (positions.len() / 3) as u32;
        for y in 0..=4 {
            for x in 0..=4 {
                positions.extend((0..3).map(|i| origin[i] + u[i] * f64::from(x) + v[i] * f64::from(y)));
            }
        }
        for y in 0..4 {
            for x in 0..4 {
                let a = base + y * 5 + x;
                triangles.extend([a, a + 1, a + 6, a, a + 6, a + 5]);
            }
        }
    }
    mesh.positions = positions.into();
    mesh.triangles = triangles.into();
    s.refresh();
    s
}

#[test]
fn reduction_persists_a_separate_mesh_for_conversion_and_undo() {
    let mut s = subdivided_box();
    s.selection = vec![crate::Sel::Body { name: "Mesh".into() }];
    let recomputed_before = s.model.recomputed;
    let reply = run(&mut s, "mesh.reduce", json!({"percentage": 40, "name": "Reduce"}));
    assert_eq!(s.model.recomputed - recomputed_before, 1, "only the new reduction should recompute geometry");
    let name = reply["created_bodies"][0].as_str().unwrap().to_owned();
    let original = reply["original_triangles"].as_u64().unwrap();
    let target = reply["target_triangles"].as_u64().unwrap();
    let achieved = reply["achieved_triangles"].as_u64().unwrap();
    assert_eq!(target, (original as f64 * 0.4).floor() as u64);
    assert!(achieved < original);
    assert_eq!(reply["warning"].is_string(), achieved > target);
    let check = |s: &Session| {
        let state = s.model.state();
        assert_eq!(state.body("Mesh").unwrap().body.triangle_mesh().unwrap().1.len() as u64, original);
        let reduced = &state.body(&name).unwrap().body;
        assert_eq!(reduced.triangle_mesh().unwrap().1.len() as u64, achieved);
        assert!(reduced.is_closed_mesh());
        assert_eq!(reduced.color(), Some([0.2, 0.5, 0.8]));
    };
    check(&s);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.model.state().body(&name).is_none());
    run(&mut s, "edit.redo", json!({}));
    check(&s);
    let mut cold = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    check(&cold);
    run(&mut cold, "mesh.convert", json!({"body": name}));
    assert!(cold.model.state().body("Mesh").unwrap().body.is_mesh());
    assert!(!cold.model.state().body(&name).unwrap().body.is_mesh());
}

#[test]
fn reduction_rejects_invalid_and_unreachable_requests_atomically() {
    let mut s = subdivided_box();
    let before = s.doc.to_json();
    for params in [
        json!({"body":"Mesh"}),
        json!({"body":"Mesh","triangle_count":0}),
        json!({"body":"Mesh","triangle_count":1.5}),
        json!({"body":"Mesh","percentage":0}),
        json!({"body":"Mesh","percentage":100}),
        json!({"body":"Mesh","percentage":-1}),
        json!({"body":"Mesh","percentage":50,"triangle_count":10}),
        json!({"body":"Mesh","triangle_count":100000}),
        json!({"body":"Missing","triangle_count":1}),
    ] {
        assert!(s.execute("mesh.reduce", &params).is_err(), "{params}");
        assert_eq!(s.doc.to_json(), before);
    }
}

#[test]
fn reduction_reports_actual_count_for_a_noisy_mesh() {
    let mut s = subdivided_box();
    let FeatureKind::MeshImport { meshes, .. } = &mut s.doc_mut().features[0].kind else { panic!("mesh") };
    let mut positions = meshes[0].positions.to_vec();
    // Perturb the interior of the top face, retaining the watertight boundary.
    for y in 1..4 {
        for x in 1..4 {
            positions[(25 + y * 5 + x) * 3 + 2] += (x * y) as f64 * 0.013;
        }
    }
    meshes[0].positions = positions.into();
    s.refresh();
    let before = s.model.state().body("Mesh").unwrap().body.triangle_mesh().unwrap().1.len();
    let reply = run(&mut s, "mesh.reduce", json!({"body":"Mesh","percentage":50}));
    let name = reply["created_bodies"][0].as_str().unwrap();
    let state = s.model.state();
    assert_eq!(state.body("Mesh").unwrap().body.triangle_mesh().unwrap().1.len(), before);
    let achieved = state.body(name).unwrap().body.triangle_mesh().unwrap().1.len();
    assert_eq!(reply["achieved_triangles"], achieved);
    assert!(achieved < before);
}

#[test]
fn reduction_uses_source_component_and_its_placements() {
    let path = std::env::temp_dir().join(format!("solidcraft-reduce-placed-{}.stl", std::process::id()));
    let mut source = subdivided_box();
    run(&mut source, "file.save_mesh", json!({"path": path}));
    let mut s = Session::default();
    let inserted = run(&mut s, "file.insert_mesh", json!({"path": path, "name": "Scan"}));
    let owner = inserted["component"].as_u64().unwrap();
    run(&mut s, "occurrence.copy", json!({"component": "Scan", "translate": [0, 40, 0]}));
    let definition = s.model.state().bodies[0].name.clone();
    s.doc_mut().body_offsets.insert(definition.clone(), solidcraft_doc::rigid(Vec3::new(7.0, 3.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    s.refresh();
    let second = s.world_state().unwrap().bodies.iter().find(|b| b.name != definition).unwrap().name.clone();
    let context = (s.active_component, s.active_occurrence_path.clone());
    let reply = run(&mut s, "mesh.reduce", json!({"body": second, "triangle_count": 1}));
    assert_eq!((s.active_component, s.active_occurrence_path.clone()), context);
    let name = reply["created_bodies"][0].as_str().unwrap();
    assert_eq!(s.doc.model_body_component(s.model.state().body(name).unwrap()), owner);
    let check = |s: &Session, source_x: f64, derived_x: f64| {
        let world = s.world_state().unwrap();
        let bounds = |definition: &str| {
            let mut boxes: Vec<_> = world.bodies.iter().filter(|b| b.definition_name() == definition).map(|b| b.mesh().bounds()).collect();
            boxes.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
            boxes
        };
        let source = bounds(&definition);
        let reduced = bounds(name);
        assert_eq!(source.len(), 2);
        assert_eq!(reduced.len(), 2);
        for (original, result) in source.iter().zip(&reduced) {
            assert!((original.min.x - source_x).abs() < 1e-8);
            assert!((result.min.x - derived_x).abs() < 1e-8);
            assert!((original.min.y - result.min.y).abs() < 1e-8);
            assert!((original.max.y - result.max.y).abs() < 1e-8);
            assert!((original.min.z - result.min.z).abs() < 1e-8);
            assert!((original.max.z - result.max.z).abs() < 1e-8);
        }
    };
    check(&s, 7.0, 7.0);
    let cold = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    check(&cold, 7.0, 7.0);
    // A warm evaluation must follow subsequent changes to the source offset.
    s.doc_mut().body_offsets.insert(definition.clone(), solidcraft_doc::rigid(Vec3::new(9.0, 3.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    s.refresh();
    check(&s, 9.0, 9.0);
    // A separately stored derived-body placement wins, rather than being composed twice.
    s.doc_mut().body_offsets.insert(name.to_string(), solidcraft_doc::rigid(Vec3::new(20.0, 3.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    s.refresh();
    check(&s, 9.0, 20.0);
    let cold = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    check(&cold, 9.0, 20.0);
    std::fs::remove_file(path).unwrap();
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
    let saved = serde_json::to_value(MeshData {
        name: "M".into(),
        positions: vec![1.5, 2.0, 3.0].into(),
        triangles: vec![0, 0, 0].into(),
        color: None,
        corner_normals: Default::default(),
    })
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
    // Its own file name: `save_mesh_writes_stl_and_3mf` writes `solidcraft-save-mesh-<pid>.stl`, and
    // sharing it made this one fail when the two ran together.
    let path = std::env::temp_dir().join(format!("solidcraft-save-mesh-rejected-{}.stl", std::process::id()));
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
        let path = dir.join(format!("solidcraft-save-mesh-{}.{ext}", std::process::id()));
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

/// The chosen-programs list and the made-up slicers are process-wide: tests that set them take
/// turns.
pub(crate) static CHOSEN_PROGRAMS: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

/// Puts back what the tests of the slicer commands make up, even when one fails.
pub(crate) struct MadeUp;

impl Drop for MadeUp {
    fn drop(&mut self) {
        *crate::slicers::TEST_DETECTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *crate::slicers::TEST_BUSCTL.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *crate::slicers::TEST_READY_LIMIT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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
    save_mesh_to_a_cold_prusaslicer_3("cold", 0.0, 0.0);
}

/// The same on a slow machine: the slicer takes a second to start and to take the file, and each
/// look at the bus takes a second. Whether the file waits must not depend on how fast that is.
#[cfg(unix)]
#[test]
fn save_mesh_with_a_slow_cold_prusaslicer_3_still_waits_for_it_before_sending() {
    save_mesh_to_a_cold_prusaslicer_3("slow", 1.0, 1.0);
}

/// The same where the temporary folder's name has a space and a quote in it: the made-up
/// slicer and bus still write inside the test's own folder.
#[cfg(unix)]
#[test]
fn save_mesh_with_a_cold_prusaslicer_3_in_a_folder_with_a_space_waits_for_it_before_sending() {
    save_mesh_to_a_cold_prusaslicer_3("in a folder with a space and a ' quote", 0.0, 0.0);
}

/// `file.save_mesh` to a made-up PrusaSlicer 3 that is not running: its launcher takes `start`
/// seconds before it logs what it was started with, and each look at the bus takes `look` seconds.
///
/// Nothing here depends on how fast the machine is. The slicer is on the bus only once it has
/// started (it has logged its start) and has been looked for twice, so the file can only follow the
/// start; the log is read once both lines are in; and the slicer is waited for longer than
/// [`crate::slicers::READY_LIMIT`], which is there for a slicer that hangs, not one that is slow.
/// The limits below are for a hang only.
#[cfg(unix)]
fn save_mesh_to_a_cold_prusaslicer_3(tag: &str, start: f64, look: f64) {
    use crate::slicers::test_support::{fake_program, sh_quote};
    use crate::slicers::{Slicer, TEST_BUSCTL, TEST_DETECTED, TEST_READY_LIMIT};
    use std::time::{Duration, Instant};
    let _turn = CHOSEN_PROGRAMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _reset = MadeUp;
    let dir = std::env::temp_dir().join(format!("solidcraft-save-mesh-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let launcher_dir = dir.join("slic3r-app-launcher");
    std::fs::create_dir_all(&launcher_dir).unwrap();
    let (log, up, early) = (dir.join("log"), dir.join("up"), dir.join("sent-early"));
    let pause = |seconds: f64| if seconds > 0. { format!("sleep {seconds}; ") } else { String::new() };
    // Given a file before the bus said the slicer was up: noted when it is started, before its own delay.
    let launcher = fake_program(
        &launcher_dir,
        "PrusaSlicer",
        &format!(
            "[ $# -gt 1 ] && [ ! -e {u} ] && : > {e}; {p}echo \"$@\" >> {l}",
            u = sh_quote(&up),
            e = sh_quote(&early),
            p = pause(start),
            l = sh_quote(&log)
        ),
    );
    let count = dir.join("count");
    // Not on the bus the first two looks, nor before it has started; there after that.
    let busctl = fake_program(
        &dir,
        "busctl",
        &format!(
            "n=$(cat {c} 2>/dev/null || echo 0); echo $((n+1)) > {c}; {p}echo ':1.5 1 x'; if [ $n -ge 2 ] && [ -s {l} ]; then : > {u}; echo 'com.prusa3d.prusaslicer.BroadcastListener.ab 2 y'; fi; exit 0",
            c = sh_quote(&count),
            p = pause(look),
            l = sh_quote(&log),
            u = sh_quote(&up)
        ),
    );
    *TEST_BUSCTL.lock().unwrap() = Some(busctl);
    *TEST_READY_LIMIT.lock().unwrap() = Some(Duration::from_secs(120));
    *TEST_DETECTED.lock().unwrap() =
        Some(vec![Slicer { id: "prusa".into(), name: "PrusaSlicer".into(), command: vec![launcher], icon: None, prusa_major: Some(3) }]);
    let mut s = imported_box();
    let out = dir.join("part.stl");
    let r = s.execute("file.save_mesh", &json!({"path": out, "slicer": "prusa", "single_instance": false})).unwrap();
    assert!(out.exists());
    assert!(r.get("slicer").is_some() && r.get("slicer_timeout").is_none(), "{r}");
    // The launcher it was sent to runs on its own: read the log once it has written its line.
    let read = || std::fs::read_to_string(&log).unwrap_or_default().lines().map(String::from).collect::<Vec<_>>();
    let hang = Instant::now() + Duration::from_secs(60);
    while read().len() < 2 && Instant::now() < hang {
        std::thread::sleep(Duration::from_millis(20));
    }
    let lines = read();
    // Started with no file, then sent the file, both with the flag (the preference could not take it away).
    assert_eq!(lines, ["--single-instance".to_string(), format!("--single-instance {}", out.display())]);
    assert!(std::fs::read_to_string(&count).unwrap().trim().parse::<u32>().unwrap() >= 3, "it looked until it was up");
    assert!(!early.exists(), "the file was sent before the slicer was on the bus");
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
                corner_normals: Default::default(),
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

/// A mesh body of a component placed twice converts under its second placement's name, the one
/// the selection and the Browser hold: the conversion edits the component, so both placements
/// then show the solid. Given as `body` and taken from the selection.
#[test]
fn a_second_placements_mesh_body_converts_by_the_name_the_world_shows() {
    let dir = std::env::temp_dir().join(format!("solidcraft-mesh-convert-placed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("cube.stl").to_string_lossy().to_string();
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "file.save_mesh", json!({"path": stl}));
    for by_selection in [false, true] {
        let mut s = Session::default();
        run(&mut s, "file.insert_mesh", json!({"path": stl}));
        run(&mut s, "occurrence.copy", json!({"component": "cube", "translate": [0, 40, 0]}));
        let definition = s.model.state().bodies[0].name.clone();
        let world = s.world_state().unwrap();
        let second = world.bodies.iter().map(|b| b.name.clone()).find(|n| *n != definition).expect("a second placement");
        assert!(world.bodies.len() == 2 && world.bodies.iter().all(|b| b.body.is_mesh()), "two placed mesh bodies");
        if by_selection {
            s.selection = vec![Sel::Body { name: second.clone() }];
            run(&mut s, "mesh.convert", json!({"tolerance": "0.05 mm"}));
        } else {
            run(&mut s, "mesh.convert", json!({"body": second, "tolerance": "0.05 mm"}));
        }
        let world = s.world_state().unwrap();
        assert_eq!(world.bodies.len(), 2);
        assert!(world.bodies.iter().all(|b| !b.body.is_mesh()), "by selection {by_selection}: both placements show the solid");
        match &s.doc.features.last().unwrap().kind {
            FeatureKind::MeshConvert { body, .. } => assert_eq!(body.to_string(), definition, "the feature names the component's body"),
            other => panic!("{other:?}"),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// #18465: a file of another CAD program (a zip as a `.f3d`, text as a `.x_t`) is not a design;
/// Open says it does not open that kind of file, not what the design reader made of it. A design
/// under its own extension keeps the reader's words, and a design under an odd name still opens.
#[test]
fn open_names_an_unsupported_file_instead_of_the_design_readers_complaint() {
    let dir = std::env::temp_dir().join(format!("solidcraft-open-unsupported-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    };
    let mut s = Session::default();
    for (name, bytes) in [("part.f3d", &b"PK\x03\x04not a design"[..]), ("part.x_t", b"**ABCDEFGHIJKLMNOPQRSTUVWXYZ\n"), ("part.xyz123", b"{")] {
        let error = s.execute("doc.open", &json!({"path": write(name, bytes)})).unwrap_err().to_string();
        let ext = name.rsplit('.').next().unwrap();
        assert!(error.contains(&format!("can't open .{ext} files")), "{error}");
        assert!(error.contains("export STEP") && error.contains("importer plugin") && error.contains("Preferences › Plugins"), "{error}");
    }
    let broken = s.execute("doc.open", &json!({"path": write("broken.sc", b"PK\x03\x04not a design")})).unwrap_err().to_string();
    assert!(!broken.contains("can't open"), "{broken}");
    // A design that was copied under another name opens as it did.
    let mut saved = Session::default();
    let design = dir.join("design.sc");
    saved.execute("file.save_as", &json!({"path": design.to_string_lossy()})).unwrap();
    let copy = dir.join("design.sc.bak");
    std::fs::copy(&design, &copy).unwrap();
    assert!(s.execute("doc.open", &json!({"path": copy.to_string_lossy()})).is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}
