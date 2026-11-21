//! An imported file becomes a component named after it (#1386): a STEP part is one component
//! with its body, a STEP assembly nested components that mirror the file with a repeated part's
//! placements as occurrences of one component, held together as one rigid group until it is
//! unlocked; a mesh file one component. Undo takes the whole import away, and designs whose
//! imports sit loose in a component open and rebuild as they did.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;
use solidcraft_kernel::{ExportBody, ExportProduct, StepHeader, box_solid, step_export_products};

use super::run;
use crate::*;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-import-component-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn at(x: f64, y: f64, z: f64) -> [[f64; 4]; 4] {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, y, z, 1.0]]
}

/// A quarter turn about Z, then moved to (x, y, z).
fn turned(x: f64, y: f64, z: f64) -> [[f64; 4]; 4] {
    [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, y, z, 1.0]]
}

/// `Rig`: a base plate, two `Arm` sub-assemblies (each a 20 × 4 × 4 beam and a `Pin`) and a third
/// `Pin` on its own; one arm is turned a quarter turn. Pins: 2 × 2 × 10. Six bodies in the
/// file: four products, one of them a sub-assembly placed twice.
fn rig_file(dir: &std::path::Path) -> String {
    let plate = box_solid(Vec3::ZERO, Vec3::new(100.0, 60.0, 5.0)).unwrap();
    let beam = box_solid(Vec3::ZERO, Vec3::new(20.0, 4.0, 4.0)).unwrap();
    let pin = box_solid(Vec3::ZERO, Vec3::new(2.0, 2.0, 10.0)).unwrap();
    let part = |name: &str, body| ExportProduct {
        name: name.into(),
        bodies: vec![ExportBody { name: name.into(), body, color: None }],
        children: Vec::new(),
    };
    let products = vec![
        ExportProduct {
            name: "Rig".into(),
            bodies: Vec::new(),
            children: vec![
                (1, at(0.0, 0.0, 0.0), "Base-1".into()),
                (2, at(10.0, 10.0, 5.0), "Arm-1".into()),
                (2, turned(60.0, 10.0, 5.0), "Arm-2".into()),
                (4, at(90.0, 50.0, 5.0), "Pin-3".into()),
            ],
        },
        part("Base", &plate),
        ExportProduct {
            name: "Arm".into(),
            bodies: Vec::new(),
            children: vec![(3, at(0.0, 0.0, 0.0), "Beam-1".into()), (4, at(18.0, 1.0, 4.0), "Pin-1".into())],
        },
        part("Beam", &beam),
        part("Pin", &pin),
    ];
    let text = step_export_products(&products, 0, &StepHeader::default()).unwrap();
    let path = dir.join("rig.step");
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().to_string()
}

/// Where the file itself puts its bodies: their bounding boxes, sorted.
fn file_boxes(path: &str) -> Vec<[i64; 6]> {
    let imp = solidcraft_kernel::step_import(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut out: Vec<[i64; 6]> = imp.bodies.iter().map(|b| key(solidcraft_kernel::measure(&b.body).unwrap().bbox)).collect();
    out.sort();
    out
}

fn key(b: solidcraft_geom::Aabb3) -> [i64; 6] {
    let r = |v: f64| (v * 1000.0).round() as i64;
    [r(b.min.x), r(b.min.y), r(b.min.z), r(b.max.x), r(b.max.y), r(b.max.z)]
}

/// Where the design shows its bodies, sorted.
fn shown_boxes(s: &Session) -> Vec<[i64; 6]> {
    let mut out: Vec<[i64; 6]> = s.world_state().unwrap().bodies.iter().map(|b| key(solidcraft_kernel::measure(&b.body).unwrap().bbox)).collect();
    out.sort();
    out
}

fn components(s: &mut Session) -> Vec<Value> {
    run(s, "component.list", json!({}))["components"].as_array().unwrap().clone()
}

fn component<'a>(list: &'a [Value], name: &str) -> &'a Value {
    list.iter().find(|c| c["name"] == name).unwrap_or_else(|| panic!("no component {name}: {list:?}"))
}

fn occurrence_named(s: &Session, name: &str) -> u64 {
    s.doc.occurrences.iter().find(|o| o.name == name).unwrap_or_else(|| panic!("no occurrence {name}")).id
}

/// The world corner of the bodies of one occurrence (by name) nearest -∞.
fn corner_of(s: &Session, occurrence: &str) -> Vec3 {
    let occ = occurrence_named(s, occurrence);
    let path = {
        let mut path = vec![occ];
        let mut parent = s.doc.occurrences.iter().find(|o| o.id == occ).unwrap().parent;
        while parent != 0 {
            let o = s.doc.occurrence_of(parent).unwrap();
            path.insert(0, o.id);
            parent = o.parent;
        }
        path
    };
    let names = s.occurrence_bodies(&path, true).unwrap();
    assert!(!names.is_empty());
    let world = s.world_state().unwrap();
    world.bodies.iter().filter(|b| names.contains(&b.name)).fold(Aabb3::EMPTY, |a, b| a.union(&solidcraft_kernel::measure(&b.body).unwrap().bbox)).min
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

/// A box exported as `name.step` in `dir`.
fn part_file(dir: &std::path::Path, name: &str) -> String {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 10}));
    let path = dir.join(format!("{name}.step")).to_string_lossy().to_string();
    run(&mut s, "file.export", json!({"path": path}));
    path
}

#[test]
fn a_step_part_comes_in_as_one_component_named_after_the_file() {
    let dir = temp_dir("part");
    let path = part_file(&dir, "bracket");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [100, 0, 0]}));
    let r = run(&mut s, "file.insert_step", json!({"path": path}));
    let list = components(&mut s);
    assert_eq!(list.len(), 2, "the design and the import: {list:?}");
    let bracket = component(&list, "bracket");
    assert_eq!((r["component"].clone(), r["component_name"].clone()), (bracket["id"].clone(), json!("bracket")), "{r}");
    assert_eq!(bracket["bodies"].as_array().unwrap().len(), 1, "{bracket}");
    assert_eq!(component(&list, &s.doc.name.clone())["bodies"].as_array().unwrap().len(), 1, "my own box stays where it was");
    let feature = r["feature"].as_u64().unwrap();
    assert_eq!(s.doc.feature(feature).unwrap().component, r["component"].as_u64().unwrap());
    // One part: nothing to hold together.
    assert!(s.doc.assembly.joints.is_empty() && r.get("subcomponents").is_none(), "{r}");
    // The active component is still the one I was working in.
    assert_eq!(s.active_component, 0);

    // Opening the file as a design makes the component too, so the browser shows its name.
    let r = run(&mut s, "doc.open", json!({"path": path}));
    assert_eq!(r["component_name"], "bracket", "{r}");
    assert_eq!(s.doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["bracket"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_step_assembly_comes_in_as_nested_components_with_its_instances() {
    let dir = temp_dir("assembly");
    let path = rig_file(&dir);
    let mut s = Session::default();
    let r = run(&mut s, "file.insert_step", json!({"path": path}));
    // rig ⊃ Base, Arm (twice; Beam, Pin inside), Pin (once more, directly).
    let list = components(&mut s);
    let names: Vec<&str> = list.iter().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(names.len(), 6, "{names:?}");
    for n in ["rig", "Base", "Arm", "Beam", "Pin"] {
        assert!(names.contains(&n), "{n} in {names:?}");
    }
    let id = |n: &str| component(&list, n)["id"].as_u64().unwrap();
    let parent = |n: &str| component(&list, n)["parent"].as_u64().unwrap();
    assert_eq!(parent("rig"), 0);
    assert_eq!((parent("Base"), parent("Arm")), (id("rig"), id("rig")));
    assert_eq!((parent("Beam"), parent("Pin")), (id("Arm"), id("Arm")));
    assert_eq!(r["component"], id("rig"), "{r}");
    assert_eq!(r["subcomponents"], 4, "{r}");
    // A repeated part is one component placed again: Arm twice, Pin in the arm and on its own.
    let placements = |c: u64| s.doc.occurrences.iter().filter(|o| o.component == c).count();
    assert_eq!((placements(id("Arm")), placements(id("Pin")), placements(id("Beam"))), (2, 2, 1));
    // Each component holds its own part's body; the file's later instances are not built again.
    let own = |n: &str| component(&list, n)["bodies"].as_array().unwrap().len();
    assert_eq!((own("rig"), own("Base"), own("Arm"), own("Beam"), own("Pin")), (0, 1, 0, 1, 1));
    assert_eq!(s.model.state().bodies.len(), 3, "base, beam and pin, once each");
    // And every body shows where the file puts it.
    assert_eq!(shown_boxes(&s), file_boxes(&path));

    // Saved and opened again, it is the same.
    let design = dir.join("rig.solidcraft").to_string_lossy().to_string();
    run(&mut s, "file.save_as", json!({"path": design}));
    let mut again = Session::default();
    run(&mut again, "doc.open", json!({"path": design}));
    assert_eq!(shown_boxes(&again), file_boxes(&path));
    assert_eq!(again.doc.components.len(), 5);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The import's bodies by name, sorted.
fn body_names(s: &Session) -> Vec<String> {
    let mut names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    names.sort();
    names
}

/// `flatten` brings an assembly in as bodies of the import's one component: no subcomponents,
/// occurrences or joints, a part placed twice one body per placement where the file places it,
/// and each body named after the part it belongs to, an exporter's placeholder name (Open
/// CASCADE names its sub-shapes `Open CASCADE STEP translator …`) giving way to its part's. The
/// design opens, undoes, redoes and rebuilds to the same bodies.
#[test]
fn a_flattened_step_assembly_is_one_component_of_bodies_where_the_file_places_them() {
    let dir = temp_dir("flatten");
    let path = rig_file(&dir);
    let mut s = Session::default();
    let r = run(&mut s, "file.insert_step", json!({"path": path, "flatten": true, "folder": true}));
    assert!(r.get("subcomponents").is_none() && r.get("rigid_members").is_none() && r.get("group").is_none(), "{r}");
    assert_eq!(s.doc.components.len(), 1, "only the import's own component");
    assert_eq!(s.doc.occurrences.len(), 1, "and its one occurrence");
    assert!(s.doc.assembly.joints.is_empty() && s.doc.assembly.rigid_groups.is_empty(), "no joints and no rigid group");
    let list = components(&mut s);
    assert_eq!(component(&list, "rig")["bodies"].as_array().unwrap().len(), 6, "{list:?}");
    // Every placement is a body, where the file puts it.
    assert_eq!(shown_boxes(&s), file_boxes(&path));
    let names = ["Arm: Beam", "Arm: Beam (2)", "Arm: Pin", "Arm: Pin (2)", "Base", "Pin"];
    assert_eq!(body_names(&s), names);

    let again = |s: &Session| (body_names(s), shown_boxes(s));
    let before = again(&s);
    run(&mut s, "timeline.compute_all", json!({}));
    assert_eq!(again(&s), before, "rebuilt");
    run(&mut s, "edit.undo", json!({}));
    assert!(s.model.state().bodies.is_empty() && s.doc.components.is_empty(), "undo takes it away");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(again(&s), before, "redone");
    let design = dir.join("flat.sc").to_string_lossy().to_string();
    run(&mut s, "file.save_as", json!({"path": design}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": design}));
    assert_eq!(again(&opened), before, "opened");
    assert_eq!((opened.doc.components.len(), opened.doc.occurrences.len()), (1, 1));

    // Saved by a reader that left out a body a nested import skips (a later placement of a
    // part, #13382), the flat import still builds it: a nested one's skip is not carried over.
    let nested = {
        let mut n = Session::default();
        run(&mut n, "file.insert_step", json!({"path": path}));
        match &n.doc.features[0].kind {
            solidcraft_doc::FeatureKind::Import { skip, .. } => skip.clone(),
            _ => panic!("no Import"),
        }
    };
    let gone = *nested.last().expect("the rig's later placements are skipped nested") as usize;
    let mut older = (*s.doc).clone();
    let f = older.features.iter_mut().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Import { .. })).unwrap();
    f.body_names.remove(gone);
    fn renumber(nodes: &mut [solidcraft_doc::kernel::StepNode], gone: usize) {
        for n in nodes {
            n.bodies = n.bodies.iter().filter(|b| **b != gone).map(|b| if *b > gone { b - 1 } else { *b }).collect();
            renumber(&mut n.children, gone);
        }
    }
    if let solidcraft_doc::FeatureKind::Import { components, skip, .. } = &mut f.kind {
        renumber(components, gone);
        assert!(skip.is_empty(), "a flat import skips nothing: {skip:?}");
    }
    let older = Session::opened(older, None);
    assert_eq!(shown_boxes(&older), file_boxes(&path), "every placement is built");

    // A sub-shape the exporter names for itself, in a part placed twice, is named after the part.
    let pin = box_solid(Vec3::ZERO, Vec3::new(2.0, 2.0, 10.0)).unwrap();
    let products = vec![
        ExportProduct {
            name: "Board".into(),
            bodies: Vec::new(),
            children: vec![(1, at(0.0, 0.0, 0.0), "U1".into()), (1, at(5.0, 0.0, 0.0), "U2".into())],
        },
        ExportProduct { name: "U1".into(), bodies: Vec::new(), children: vec![(2, at(0.0, 0.0, 0.0), "Shape".into())] },
        ExportProduct {
            name: "Open CASCADE STEP translator 7.5 1.1".into(),
            bodies: vec![ExportBody { name: String::new(), body: &pin, color: None }],
            children: Vec::new(),
        },
    ];
    let board = dir.join("board.step");
    std::fs::write(&board, step_export_products(&products, 0, &StepHeader::default()).unwrap()).unwrap();
    let mut s = Session::default();
    let plain = run(&mut s, "file.insert_step", json!({"path": board.to_string_lossy()}));
    assert!(plain["bodies"].to_string().contains("translator"), "nested, the reader's name stays: {plain}");
    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": board.to_string_lossy(), "flatten": true}));
    assert_eq!(body_names(&s), ["U1", "U1 (2)"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `flatten` with `reader: "cadmpeg"`: the records route's product tree is flattened the same
/// way, every placement a body of the import's one component where the file places it, named
/// after its part as with SolidCraft's reader, and it rebuilds to the same bodies.
#[test]
fn a_flattened_step_assembly_read_by_cadmpeg_is_the_same_bodies() {
    let dir = temp_dir("flatten-cadmpeg");
    let path = rig_file(&dir);
    let mut ours = Session::default();
    run(&mut ours, "file.insert_step", json!({"path": path, "flatten": true}));
    let mut s = Session::default();
    let r = run(&mut s, "file.insert_step", json!({"path": path, "flatten": true, "reader": "cadmpeg"}));
    assert!(r.get("subcomponents").is_none() && r.get("rigid_joints").is_none(), "{r}");
    assert_eq!((s.doc.components.len(), s.doc.occurrences.len()), (1, 1), "only the import's own component");
    assert!(s.doc.assembly.joints.is_empty());
    assert!(matches!(&s.doc.features[0].kind, solidcraft_doc::FeatureKind::Import { skip, .. } if skip.is_empty()), "a flat import skips nothing");
    assert_eq!(shown_boxes(&s), file_boxes(&path));
    assert_eq!(body_names(&s), body_names(&ours));
    let before = (body_names(&s), shown_boxes(&s));
    run(&mut s, "timeline.compute_all", json!({}));
    assert_eq!((body_names(&s), shown_boxes(&s)), before, "rebuilt");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Each occurrence's degrees of freedom, as `joint.list` gives them.
fn dofs(s: &mut Session) -> (std::collections::BTreeMap<u64, u64>, Value) {
    let r = run(s, "joint.list", json!({}));
    let dof = r["dof"].as_array().unwrap().iter().map(|d| (d["occurrence"].as_u64().unwrap(), d["dof"].as_u64().unwrap())).collect();
    (dof, r)
}

#[test]
fn an_imported_assembly_moves_as_one_until_it_is_unlocked() {
    let dir = temp_dir("rigid");
    let path = rig_file(&dir);
    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": path}));
    let rig = occurrence_named(&s, "rig:1");
    let inside: Vec<u64> = s.doc.occurrences.iter().filter(|o| o.id != rig).map(|o| o.id).collect();
    assert_eq!(inside.len(), 6);
    let pin = corner_of(&s, "Pin:2");
    let arm = corner_of(&s, "Arm:1");

    // The whole import is one rigid body: no part of it moves against the others. One rigid
    // group holds it, not a joint per placement.
    let (dof, r) = dofs(&mut s);
    assert!(r["conflicts"].as_array().unwrap().is_empty(), "{r}");
    assert!(s.doc.assembly.joints.is_empty(), "{r}");
    assert_eq!(
        r["rigid_groups"],
        json!([{"id": s.doc.assembly.rigid_groups[0].id, "name": "rig", "component": s.doc.components.iter().find(|c| c.name == "rig").unwrap().id, "members": 6}]),
        "{r}"
    );
    for o in &inside {
        assert_eq!(dof.get(o), dof.get(&rig), "occurrence {o} moves only with the import: {r}");
    }
    // The design's first part is grounded when it comes in; free, it moves.
    run(&mut s, "occurrence.ground", json!({"occurrence": "rig:1", "grounded": false}));

    // Moving the import's component moves everything in it.
    run(&mut s, "occurrence.move", json!({"occurrence": "rig:1", "translate": [0, 0, 50]}));
    assert!(close(corner_of(&s, "Pin:2"), pin + Vec3::new(0.0, 0.0, 50.0)), "{:?}", corner_of(&s, "Pin:2"));
    assert!(close(corner_of(&s, "Arm:1"), arm + Vec3::new(0.0, 0.0, 50.0)));
    // A part inside doesn't move on its own; the refusal says what to move instead.
    for part in ["Pin:2", "Pin:1"] {
        let e = s.execute("occurrence.move", &json!({"occurrence": part, "translate": [30, 0, 0]})).unwrap_err().to_string();
        assert!(e.contains("move rig"), "{e}");
    }
    assert!(close(corner_of(&s, "Pin:2"), pin + Vec3::new(0.0, 0.0, 50.0)));

    // Next to a grounded part of mine the import is still one rigid body.
    run(&mut s, "component.create", json!({"name": "Mine"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [-50, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.ground", json!({"occurrence": "Mine:1", "grounded": true}));
    let (dof, r) = dofs(&mut s);
    for o in &inside {
        assert_eq!(dof.get(o), dof.get(&rig), "{r}");
    }
    assert!(r["conflicts"].as_array().unwrap().is_empty(), "{r}");

    // A joint from my part to a part inside the import moves the whole import there.
    let arm_now = corner_of(&s, "Arm:1");
    let pin_now = corner_of(&s, "Pin:2");
    let mine = occurrence_named(&s, "Mine:1");
    let pin1 = occurrence_named(&s, "Pin:1");
    let slider = run(&mut s, "joint.as_built", json!({"type": "slider", "a": mine, "b": pin1, "name": "Slide"}));
    run(&mut s, "joint.drive", json!({"joint": slider["joint"], "value": 7}));
    let shift = corner_of(&s, "Pin:2") - pin_now;
    assert!(shift.dist(Vec3::ZERO) > 1e-6, "the import moved");
    assert!(close(corner_of(&s, "Arm:1"), arm_now + shift), "the arm moved with the pin: {:?} {:?}", corner_of(&s, "Arm:1"), arm_now + shift);
    run(&mut s, "joint.delete", json!({"joint": slider["joint"]}));
    let pin = corner_of(&s, "Pin:2") - Vec3::new(0.0, 0.0, 50.0);
    let arm = corner_of(&s, "Arm:1") - Vec3::new(0.0, 0.0, 50.0);

    // A component inside the import is held by the import's group: unlocking it says so.
    let e = s.execute("component.unlock", &json!({"component": "Pin"})).unwrap_err().to_string();
    assert!(e.contains("`rig`"), "{e}");

    // Unlocked (one undo step), the parts move one by one.
    let undo = s.undo.len();
    let r = run(&mut s, "component.unlock", json!({"component": "rig"}));
    assert!(r["rigid_group"].is_u64() && s.doc.assembly.rigid_groups.is_empty(), "{r}");
    assert_eq!(s.undo.len(), undo + 1);
    run(&mut s, "occurrence.move", json!({"occurrence": "Pin:2", "translate": [30, 0, 0]}));
    assert!(close(corner_of(&s, "Pin:2"), pin + Vec3::new(30.0, 0.0, 50.0)), "{:?}", corner_of(&s, "Pin:2"));
    assert!(close(corner_of(&s, "Arm:1"), arm + Vec3::new(0.0, 0.0, 50.0)), "the others stay");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.assembly.rigid_groups.len(), 1, "undo locks it again");
    assert!(s.execute("component.unlock", &json!({"component": "Mine"})).is_err(), "nothing holds my own component");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The box around boxes.
fn union(boxes: &[[i64; 6]]) -> [i64; 6] {
    let mut u = [i64::MAX, i64::MAX, i64::MAX, i64::MIN, i64::MIN, i64::MIN];
    for b in boxes {
        for i in 0..3 {
            u[i] = u[i].min(b[i]);
            u[i + 3] = u[i + 3].max(b[i + 3]);
        }
    }
    u
}

/// The world box of the body `name`, and that of every other body.
fn envelope_and_parts(s: &Session, name: &str) -> ([i64; 6], [i64; 6]) {
    let world = s.world_state().unwrap();
    let boxed = |b: &solidcraft_doc::ModelBody| key(solidcraft_kernel::measure(&b.body).unwrap().bbox);
    let parts: Vec<[i64; 6]> = world.bodies.iter().filter(|b| b.name != name).map(boxed).collect();
    (boxed(world.body(name).unwrap()), union(&parts))
}

/// `envelope: "box"` keeps a hidden box around every body the import placed, the turned arm and
/// each repeated part included, in the import's component: it moves with the import, and is
/// built again around a part moved once the import is unlocked.
#[test]
fn an_import_with_an_envelope_keeps_a_hidden_box_around_its_parts() {
    let dir = temp_dir("envelope");
    let path = rig_file(&dir);
    let mut s = Session::default();
    let r = run(&mut s, "file.insert_step", json!({"path": path, "envelope": "box"}));
    assert_eq!(r["envelope"], "rig Envelope", "{r}");
    assert!(s.visibility.hidden_bodies.iter().any(|b| b == "rig Envelope"), "{:?}", s.visibility.hidden_bodies);
    let rig = s.doc.find_component("rig").unwrap();
    assert_eq!(s.doc.features.last().map(|f| f.component), Some(rig));
    let (envelope, parts) = envelope_and_parts(&s, "rig Envelope");
    assert_eq!(envelope, union(&file_boxes(&path)));
    assert_eq!(envelope, parts);

    run(&mut s, "occurrence.ground", json!({"occurrence": "rig:1", "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": "rig:1", "translate": [0, 0, 50]}));
    let (moved, parts) = envelope_and_parts(&s, "rig Envelope");
    assert_eq!(moved, parts);
    assert_eq!(moved[2], envelope[2] + 50_000, "{moved:?}");

    run(&mut s, "component.unlock", json!({"component": "rig"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Pin:2", "translate": [0, 0, 30]}));
    let (grown, parts) = envelope_and_parts(&s, "rig Envelope");
    assert_eq!(grown, parts);
    assert_ne!(grown, moved, "the pin left the old box");
    let _ = std::fs::remove_dir_all(&dir);
}

/// With `flatten`, the envelope is a box around the bodies of the import's one component, every
/// placement included, and it is that component's only addition: no subcomponent holds it.
#[test]
fn a_flattened_import_with_an_envelope_keeps_a_box_around_its_bodies() {
    let dir = temp_dir("flat-envelope");
    let path = rig_file(&dir);
    let mut s = Session::default();
    let r = run(&mut s, "file.insert_step", json!({"path": path, "flatten": true, "envelope": "box"}));
    assert_eq!(r["envelope"], "rig Envelope", "{r}");
    assert!(r.get("subcomponents").is_none(), "{r}");
    assert!(s.visibility.hidden_bodies.iter().any(|b| b == "rig Envelope"), "{:?}", s.visibility.hidden_bodies);
    assert_eq!((s.doc.components.len(), s.doc.occurrences.len()), (1, 1), "still one component");
    let rig = s.doc.find_component("rig").unwrap();
    assert_eq!(s.doc.features.last().map(|f| f.component), Some(rig));
    let (envelope, parts) = envelope_and_parts(&s, "rig Envelope");
    assert_eq!(envelope, union(&file_boxes(&path)));
    assert_eq!(envelope, parts);

    run(&mut s, "occurrence.ground", json!({"occurrence": "rig:1", "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": "rig:1", "translate": [0, 0, 50]}));
    let (moved, parts) = envelope_and_parts(&s, "rig Envelope");
    assert_eq!(moved, parts);
    assert_eq!(moved[2], envelope[2] + 50_000, "{moved:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An import's rigid group holds the parts the import placed, and only those: it survives save
/// and reopen when it is the design's only assembly record, a part placed inside the import later
/// moves on its own, and an assembly imported inside another is released by its own unlock.
#[test]
fn an_imports_rigid_group_holds_the_parts_it_placed() {
    let dir = temp_dir("rigid-members");
    let path = rig_file(&dir);
    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": path}));
    let [group] = s.doc.assembly.rigid_groups.as_slice() else { panic!("{:?}", s.doc.assembly.rigid_groups) };
    assert_eq!(group.members.len(), 6);
    assert!(s.doc.assembly.joints.is_empty(), "the group is the only assembly record");

    // Saved and opened again, the group is there.
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.assembly.rigid_groups, s.doc.assembly.rigid_groups);

    // A part placed inside the import afterwards is not held.
    run(&mut s, "component.activate", json!({"component": "rig"}));
    run(&mut s, "component.create", json!({"name": "Mine"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [-50, 0, 0]}));
    let mine = occurrence_named(&s, "Mine:1");
    assert!(s.held_by_rigid_group(mine).is_none(), "my own part moves on its own");
    assert!(s.held_by_rigid_group(occurrence_named(&s, "Pin:2")).is_some(), "the import's parts are still held");

    // A second assembly imported inside the first: unlocking it releases its parts, not the first's.
    run(&mut s, "component.activate", json!({"component": "rig"}));
    let inner = run(&mut s, "file.insert_step", json!({"path": path, "name": "Inner"}))["component"].as_u64().unwrap();
    assert_eq!(s.doc.assembly.rigid_groups.len(), 2);
    run(&mut s, "component.unlock", json!({"component": inner}));
    let [left] = s.doc.assembly.rigid_groups.as_slice() else { panic!("{:?}", s.doc.assembly.rigid_groups) };
    assert_eq!(left.name, "rig");
    let inner_part = s.doc.occurrences.iter().find(|o| o.parent != 0 && s.doc.component_within(o.parent, inner)).unwrap().id;
    assert!(s.held_by_rigid_group(inner_part).is_none(), "the inner import's parts are free");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Paste New of a later placement of an imported assembly copies its rigid group too: the
/// copy's parts are held, as a copy of the first placement's would be.
#[test]
fn paste_new_of_a_later_placement_of_an_import_copies_its_rigid_group() {
    let dir = temp_dir("rigid-paste");
    let path = rig_file(&dir);
    let mut s = Session::default();
    let rig = run(&mut s, "file.insert_step", json!({"path": path}))["component"].as_u64().unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"component": rig, "translate": [200, 0, 0]}))["occurrence"].as_u64().unwrap();
    let copied = run(&mut s, "component.paste_new", json!({"component": rig, "occurrence": second, "translate": [0, 200, 0]}));
    let copy = copied["component"].as_u64().unwrap();
    assert_ne!(copy, rig);
    let group = s.doc.rigid_group_of(copy).unwrap_or_else(|| panic!("no group for the copy: {:?}", s.doc.assembly.rigid_groups));
    assert_eq!(s.doc.rigid_group_members(group).len(), 6, "{group:?}");
    let original = s.doc.rigid_group_of(rig).unwrap();
    assert!(group.members.iter().all(|m| !original.members.contains(m)), "the copy holds its own parts");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_mesh_file_comes_in_as_one_component() {
    let dir = temp_dir("stl");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let stl = dir.join("cube.stl").to_string_lossy().to_string();
    run(&mut s, "file.save_mesh", json!({"path": stl}));
    let mut s = Session::default();
    let r = run(&mut s, "file.insert_mesh", json!({"path": stl}));
    assert_eq!(r["component_name"], "cube", "{r}");
    let list = components(&mut s);
    assert_eq!(list.len(), 2);
    assert_eq!(component(&list, "cube")["bodies"].as_array().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A 10 x 20 x 30 box with its low corner at (100, 50, 7), saved as `block.stl` in `dir`.
fn off_origin_stl(dir: &std::path::Path) -> String {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30, "corner": [100, 50, 7]}));
    let stl = dir.join("block.stl").to_string_lossy().to_string();
    run(&mut s, "file.save_mesh", json!({"path": stl}));
    stl
}

/// The one box the design shows.
fn only_box(s: &Session) -> [i64; 6] {
    let boxes = shown_boxes(s);
    assert_eq!(boxes.len(), 1, "{boxes:?}");
    boxes[0]
}

/// Insert Mesh's `origin` (#12543): off, the mesh stays where its file puts it; on, its box is
/// centred on X and Y with its base on Z = 0, by moving the import's occurrence, and the reply
/// says by how much. A unit scales the mesh first, so the scaled box is the one centred.
#[test]
fn a_mesh_import_moves_to_the_origin_only_when_asked() {
    let dir = temp_dir("origin");
    let stl = off_origin_stl(&dir);
    let mut s = Session::default();
    let r = run(&mut s, "file.insert_mesh", json!({"path": stl}));
    assert!(r.get("moved_to_origin").is_none(), "{r}");
    assert_eq!(only_box(&s), [100_000, 50_000, 7_000, 110_000, 70_000, 37_000], "the file's coordinates by default");

    let mut s = Session::default();
    let r = run(&mut s, "file.insert_mesh", json!({"path": stl, "origin": true}));
    assert_eq!(only_box(&s), [-5_000, -10_000, 0, 5_000, 10_000, 30_000], "centred on X and Y, base on Z = 0");
    assert_eq!(r["moved_to_origin"], json!([-105.0, -60.0, -7.0]), "{r}");
    let occ = r["occurrence"].as_u64().unwrap();
    let placed = s.doc.occurrences.iter().find(|o| o.id == occ).unwrap().transform;
    assert_eq!(placed[3], [-105.0, -60.0, -7.0, 1.0], "the occurrence carries the move");
    // One undo step takes the whole import, move and all, away.
    run(&mut s, "edit.undo", json!({}));
    assert!(s.world_state().unwrap().bodies.is_empty());

    let mut s = Session::default();
    run(&mut s, "file.insert_mesh", json!({"path": stl, "origin": true, "units": "cm"}));
    assert_eq!(only_box(&s), [-50_000, -100_000, 0, 50_000, 100_000, 300_000], "scaled to millimetres, then centred");

    let mut s = Session::default();
    let e = s.execute("file.insert_mesh", &json!({"path": stl, "origin": "yes"})).unwrap_err().to_string();
    assert!(e.contains("`origin` must be true"), "{e}");
    assert!(s.doc.features.is_empty(), "a refused import adds nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `origin` is the origin of the component the mesh is inserted into, wherever that component
/// is placed; and opening a mesh file as a design takes it too.
#[test]
fn a_mesh_moved_to_the_origin_sits_on_the_active_components_origin() {
    let dir = temp_dir("origin-active");
    let stl = off_origin_stl(&dir);
    let mut s = Session::default();
    let holder = run(&mut s, "component.create", json!({"name": "Holder", "grounded": false}));
    // Made active as it is made.
    run(&mut s, "occurrence.move", json!({"occurrence": holder["occurrence"], "translate": [0, 0, 40]}));
    assert_ne!(s.active_component, 0);
    run(&mut s, "file.insert_mesh", json!({"path": stl, "origin": true}));
    assert_eq!(only_box(&s), [-5_000, -10_000, 40_000, 5_000, 10_000, 70_000], "on Holder's origin, 40 mm up");

    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": stl, "origin": true}));
    assert_eq!(r["moved_to_origin"], json!([-105.0, -60.0, -7.0]), "{r}");
    assert_eq!(only_box(&s), [-5_000, -10_000, 0, 5_000, 10_000, 30_000]);
    let e = s.execute("doc.open", &json!({"path": dir.join("x.sc").to_string_lossy(), "origin": true})).unwrap_err().to_string();
    assert!(e.contains("`origin` applies to"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Moving to the origin measures the mesh in the active component's own frame, so a component
/// turned 45 degrees takes exactly the file box's shift (not a larger box around the turned
/// one's corners); and a component made active without an occurrence path is taken at its first
/// placement, as everywhere else.
#[test]
fn a_mesh_moves_to_the_origin_of_a_turned_or_pathless_active_component() {
    let dir = temp_dir("origin-turned");
    let stl = off_origin_stl(&dir);
    let shift = |r: &Value| -> Vec3 {
        let v = r["moved_to_origin"].as_array().unwrap_or_else(|| panic!("not moved: {r}"));
        Vec3::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap())
    };

    let mut s = Session::default();
    let holder = run(&mut s, "component.create", json!({"name": "Holder", "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": holder["occurrence"], "axis": [1, 0, 0], "angle": "45 deg", "translate": [0, 0, 40]}));
    assert_ne!(s.active_component, 0);
    let r = run(&mut s, "file.insert_mesh", json!({"path": stl, "origin": true}));
    assert!(close(shift(&r), Vec3::new(-105.0, -60.0, -7.0)), "in Holder's frame: {r}");

    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Holder", "grounded": false}));
    assert_ne!(s.active_component, 0);
    s.active_occurrence_path = None;
    let r = run(&mut s, "file.insert_mesh", json!({"path": stl, "origin": true}));
    assert!(close(shift(&r), Vec3::new(-105.0, -60.0, -7.0)), "at Holder's first placement: {r}");
    assert_eq!(only_box(&s), [-5_000, -10_000, 0, 5_000, 10_000, 30_000]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undo_takes_the_whole_import_away() {
    let dir = temp_dir("undo");
    let path = rig_file(&dir);
    let mut s = Session::default();
    let undo = s.undo.len();
    run(&mut s, "file.insert_step", json!({"path": path}));
    assert_eq!(s.undo.len(), undo + 1, "one step");
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.features.is_empty() && s.doc.components.is_empty() && s.doc.occurrences.is_empty(), "nothing left");
    assert!(s.doc.assembly.joints.is_empty() && s.doc.body_components.is_empty() && s.doc.body_offsets.is_empty());
    assert!(s.world_state().unwrap().bodies.is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(shown_boxes(&s), file_boxes(&path));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A design saved before imports made components has its Import loose in the design: it opens
/// as it is (no component is made for it) and builds every body of the file where it was.
#[test]
fn a_design_with_a_loose_import_rebuilds_unchanged() {
    let dir = temp_dir("loose");
    let path = rig_file(&dir);
    let sf = solidcraft_io::step_import_feature(&std::fs::read(&path).unwrap(), &path).unwrap();
    let mut doc = solidcraft_doc::Document::new("Old");
    let id = doc.add_feature(sf.kind, Some(&sf.name)).unwrap();
    doc.feature_mut(id).unwrap().body_names = sf.body_names;
    // As a format 21 file (0.12.0) has it.
    let mut json: Value = serde_json::from_str(&doc.to_json()).unwrap();
    json["format"] = json!("solidcraft/21");
    assert!(json["features"][0]["kind"].get("skip").is_none());
    let file = dir.join("old.solidcraft");
    std::fs::write(&file, json.to_string()).unwrap();
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": file.to_string_lossy()}));
    assert!(s.doc.components.is_empty() && s.doc.occurrences.is_empty(), "not migrated");
    assert_eq!(s.doc.features[0].component, 0);
    let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names, ["Base", "Beam", "Pin", "Beam (2)", "Pin (2)", "Pin (3)"]);
    assert_eq!(shown_boxes(&s), file_boxes(&path));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A design whose `Outer` component holds a plate and an `Inner` component placed twice (the
/// first placement turned and moved), with `Outer` itself turned about X and lifted.
fn nested_file(dir: &std::path::Path) -> String {
    let mut s = Session::default();
    let outer = run(&mut s, "component.create", json!({"name": "Outer", "grounded": false}))["occurrence"].clone();
    run(&mut s, "solid.box", json!({"length": 30, "width": 30, "height": 5, "body_name": "Plate"}));
    let inner = run(&mut s, "component.create", json!({"name": "Inner"}))["occurrence"].clone();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": "Outer"}));
    run(&mut s, "occurrence.move", json!({"occurrence": inner, "axis": [0, 0, 1], "angle": "90 deg", "translate": [100, 0, 0]}));
    run(&mut s, "occurrence.copy", json!({"component": "Inner", "translate": [0, 0, 30]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": outer, "space": "world", "axis": [1, 0, 0], "angle": "90 deg", "translate": [0, 0, 50]}));
    let path = dir.join("nested.step").to_string_lossy().to_string();
    run(&mut s, "file.export", json!({"path": path}));
    path
}

#[test]
fn an_inserted_assembly_exports_its_nested_parts_where_they_are() {
    let dir = temp_dir("reexport");
    let path = nested_file(&dir);
    let placed = file_boxes(&path);
    assert_eq!(placed.len(), 3, "a plate and two blocks: {placed:?}");
    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": path}));
    assert_eq!(shown_boxes(&s), placed);
    // The whole design as an assembly: every part is placed once, by its occurrences.
    let again = dir.join("again.step").to_string_lossy().to_string();
    run(&mut s, "file.export", json!({"path": again}));
    assert_eq!(file_boxes(&again), placed);
    // And inserted again, it shows as the first file did.
    let mut back = Session::default();
    run(&mut back, "file.insert_step", json!({"path": again}));
    assert_eq!(shown_boxes(&back), placed);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file from the STEP corpus (`tests/step-conformance`).
fn corpus(name: &str) -> String {
    format!("{}/../../tests/step-conformance/{name}.step", env!("CARGO_MANIFEST_DIR"))
}

/// With `folder: true`, an assembly's file (one cylinder placed twice, an assembly) gets a
/// history folder of its own, `Group #N` with the first free number, holding its Import feature,
/// made by the same command and undone with it; a file with one object gets none. Inserted
/// between the features of a folder, the import's folder nests in it.
#[test]
fn a_multi_object_step_insert_is_one_history_folder() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [100, 0, 0], "name": "Mine"}));
    let r = run(&mut s, "file.insert_step", json!({"path": corpus("cylinder"), "folder": true}));
    assert!(r.get("group").is_none() && s.doc.timeline_groups.is_empty(), "one object: no folder: {r}");

    let undo = s.undo.len();
    let r = run(&mut s, "file.insert_step", json!({"path": corpus("cylinder_twice"), "folder": true}));
    assert_eq!(s.undo.len(), undo + 1, "one undo step");
    let feature = r["feature"].as_u64().unwrap();
    let [folder] = s.doc.timeline_groups.as_slice() else { panic!("one folder: {:?}", s.doc.timeline_groups) };
    assert_eq!(
        (json!(folder.id), folder.name.as_str(), folder.features.as_slice(), folder.parent),
        (r["group"].clone(), "Group #1", &[feature][..], None),
        "{r}"
    );
    assert!(folder.joints.is_empty() && s.doc.assembly.rigid_groups.len() == 1, "a rigid group holds the parts: {folder:?}");
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.timeline_groups.is_empty() && s.doc.feature(feature).is_none(), "undo takes the folder with the import");

    // An assembly, inserted with the marker inside a folder: its folder nests in that one.
    let dir = temp_dir("folder");
    let path = rig_file(&dir);
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [200, 0, 0], "operation": "new_body", "name": "Later"}));
    let all: Vec<u64> = s.doc.features.iter().map(|f| f.id).collect();
    let outer = run(&mut s, "timeline.group", json!({"name": "Everything", "features": all}))["group"].as_u64().unwrap();
    run(&mut s, "timeline.roll_to", json!({"feature": "Mine"}));
    let r = run(&mut s, "file.insert_step", json!({"path": path, "folder": true}));
    let folder = s.doc.timeline_groups.iter().find(|f| json!(f.id) == r["group"]).unwrap();
    assert_eq!((folder.name.as_str(), folder.parent), ("Group #1", Some(outer)), "{r}");
    assert_eq!(folder.features, [r["feature"].as_u64().unwrap()]);
    // The next one takes the next free number.
    let r = run(&mut s, "file.insert_step", json!({"path": path, "folder": true}));
    let folder = s.doc.timeline_groups.iter().find(|f| json!(f.id) == r["group"]).unwrap();
    assert_eq!(folder.name, "Group #2", "{r}");
    assert!(solidcraft_doc::non_contiguous_timeline_group(&s.doc.features, &s.doc.timeline_groups).is_none());
    // Opening a file as a design folds it the same way.
    let r = run(&mut s, "doc.open", json!({"path": corpus("cylinder_twice"), "folder": true}));
    assert_eq!(s.doc.timeline_groups.len(), 1, "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Without `folder`, as before the option, and with `folder: false` (the app's "Fold the History
/// bar into groups" turned off), a multi-object import goes into no folder, inserted or opened as
/// a design; a value that isn't true or false is refused.
#[test]
fn a_step_import_told_no_folder_makes_none() {
    let mut s = Session::default();
    let r = run(&mut s, "file.insert_step", json!({"path": corpus("cylinder_twice"), "folder": false}));
    assert!(r.get("group").is_none() && s.doc.timeline_groups.is_empty(), "no folder: {r}");
    assert_eq!(s.doc.assembly.rigid_groups.len(), 1, "its rigid group is there all the same");
    let r = run(&mut s, "doc.open", json!({"path": corpus("cylinder_twice"), "folder": false}));
    assert!(s.doc.timeline_groups.is_empty(), "opened as a design, no folder either: {r}");
    let e = s.execute("file.insert_step", &json!({"path": corpus("cylinder_twice"), "folder": "no"})).unwrap_err();
    assert!(e.to_string().contains("`folder` must be true"), "{e}");
    // Left out or null, as before the option: no folder either.
    let r = run(&mut s, "doc.open", json!({"path": corpus("cylinder_twice")}));
    assert!(s.doc.timeline_groups.is_empty(), "opened without `folder`, no folder: {r}");
    let r = run(&mut s, "file.insert_step", json!({"path": corpus("cylinder_twice"), "folder": null}));
    assert!(r.get("group").is_none() && s.doc.timeline_groups.is_empty(), "inserted with `folder: null`, no folder: {r}");
}

/// Grouping is refused, with a reason a menu can show, when the features aren't one run of the
/// history or would split another folder; one or several neighbours group.
#[test]
fn group_refusal_says_why_a_selection_cannot_be_grouped() {
    let mut s = Session::default();
    for (i, name) in ["A", "B", "C", "D"].iter().enumerate() {
        let corner = [20.0 * i as f64, 0.0, 0.0];
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": corner, "operation": "new_body", "name": name}));
    }
    let id = |s: &Session, n: &str| s.doc.find_feature(n).unwrap().id;
    let (a, b, c, d) = (id(&s, "A"), id(&s, "B"), id(&s, "C"), id(&s, "D"));
    assert_eq!(s.group_refusal(&[b], None), None);
    assert_eq!(s.group_refusal(&[b, c], None), None);
    let apart = s.group_refusal(&[a, c], None).unwrap();
    assert!(apart.contains("next to each other"), "{apart}");
    assert!(s.execute("timeline.group", &json!({"features": [a, c]})).is_err(), "the command refuses it too");
    run(&mut s, "timeline.group", json!({"name": "Run", "features": [a, b, c]}));
    let split = s.group_refusal(&[b], None).unwrap();
    assert!(split.contains("Run"), "taking the middle out splits the folder: {split}");
    let run_id = s.doc.timeline_groups.first().unwrap().id;
    assert_eq!(s.group_refusal(&[b], Some(run_id)), None, "inside the folder it nests");
    assert_eq!(s.group_refusal(&[c, d], None), None, "a folder's last feature leaves it for the new folder");
    assert!(s.group_refusal(&[], None).is_some());
}

/// The parts of one STEP import share their face names, and each keeps its body where the file
/// has it, placed in its component by an offset. A joint on one part's hole takes that part's
/// hole, not the other part's hole of that name, though the pick (in the part's own coordinates)
/// lies nearer that one as the bodies lie.
#[test]
fn a_joint_on_an_imported_part_takes_its_own_hole() {
    // Two 40 × 30 × 5 plates with a 6.2 mm hole through each: A's at (28, 20), B's at (10, 10);
    // B placed 60 mm along X, so its hole is at (70, 10) in the world.
    let plate = |x: f64, y: f64| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 5}));
        run(&mut s, "solid.hole", json!({"position": [x, y, 5], "diameter": 6.2, "depth": 5}));
        s.model.state().bodies[0].body.clone()
    };
    let (a, b) = (plate(28.0, 20.0), plate(10.0, 10.0));
    let part = |name: &str, body| ExportProduct {
        name: name.into(),
        bodies: vec![ExportBody { name: name.into(), body, color: None }],
        children: Vec::new(),
    };
    let products = vec![
        ExportProduct {
            name: "Pair".into(),
            bodies: Vec::new(),
            children: vec![(1, at(0.0, 0.0, 0.0), "A-1".into()), (2, at(60.0, 0.0, 0.0), "B-1".into())],
        },
        part("A", &a),
        part("B", &b),
    ];
    let dir = temp_dir("own-hole");
    let path = dir.join("pair.step");
    std::fs::write(&path, step_export_products(&products, 0, &StepHeader::default()).unwrap()).unwrap();
    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);

    // A pin jointed into B's hole, picked on its wall.
    let on_b = occurrence_named(&s, "B:1");
    let pin = run(&mut s, "component.create", json!({"name": "Pin"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 10, "base": [0, -40, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let a = json!({"occurrence": pin, "point": [0, -40, 0]});
    run(&mut s, "joint.create", json!({"type": "rigid", "name": "Into B", "a": a, "b": {"occurrence": on_b, "circle": [73.1, 10, 2.5]}}));
    let o = s.doc.assembly.joints.iter().find(|j| j.name == "Into B").unwrap().b.clone();
    let st = s.model.state();
    // A's hole has the very name B's has (the case this is about).
    let solidcraft_doc::joints::Snap::CircleCenter { face: Some(name), .. } = &o.snap else { panic!("{:?}", o.snap) };
    let a_comp = s.doc.occurrences.iter().find(|x| x.id == occurrence_named(&s, "A:1")).unwrap().component;
    let a_body = st.bodies.iter().find(|x| s.doc.model_body_component(x) == a_comp).unwrap();
    assert!(solidcraft_doc::naming::face_names(a_body).iter().any(|n| n == name), "{name}");
    let f = solidcraft_doc::joints::resolve_origin(&s.doc, &st, &o).unwrap();
    let at = solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(&s.doc, on_b), Vec3::new(f[3][0], f[3][1], f[3][2]));
    assert!(Vec3::new(at.x, at.y, 0.0).dist(Vec3::new(70.0, 10.0, 0.0)) < 1e-6, "B's hole is at (70, 10), the joint at {at:?}");
    // And the pin stands in it.
    let pin_at = solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(&s.doc, pin), Vec3::new(0.0, -40.0, 0.0));
    assert!(Vec3::new(pin_at.x, pin_at.y, 0.0).dist(Vec3::new(70.0, 10.0, 0.0)) < 1e-6, "the pin's base is at {pin_at:?}");
}

/// Each component's bodies, by component name.
fn bodies_by_component(s: &mut Session) -> Vec<(String, Vec<String>)> {
    components(s)
        .iter()
        .map(|c| (c["name"].as_str().unwrap().to_string(), c["bodies"].as_array().unwrap().iter().map(|b| b.as_str().unwrap().to_string()).collect()))
        .collect()
}

/// Renaming one body of an imported assembly renames that body and nothing else (#2175). An
/// Import keeps its bodies' names by their place in the file, and builds no body for the file's
/// later instances of a repeated part: the rename stored the names by their place in the model,
/// so once the design was rebuilt the bodies after those instances took the wrong names and left
/// their components.
#[test]
fn renaming_an_imported_body_after_a_repeated_part_keeps_every_body_in_its_component() {
    let dir = temp_dir("rename-after-repeat");
    // Bracket and Pin placed twice, then Plate and Knob: the file's bodies are Bracket, Pin,
    // Bracket, Pin, Plate, Knob, and the Import builds Bracket, Pin, Plate, Knob.
    let mut a = Session::default();
    run(&mut a, "solid.box", json!({"length": 20, "width": 10, "height": 10, "body_name": "Bracket"}));
    run(&mut a, "solid.box", json!({"length": 4, "width": 4, "height": 16, "corner": [8, 3, 10], "operation": "new_body", "body_name": "Pin"}));
    run(&mut a, "component.from_bodies", json!({"bodies": ["Bracket", "Pin"]}));
    for part in ["Bracket", "Pin"] {
        run(&mut a, "occurrence.copy", json!({"component": part, "translate": [0, 25, 0]}));
    }
    run(
        &mut a,
        "solid.box",
        json!({"length": 60, "width": 50, "height": 3, "corner": [-20, -10, -3], "operation": "new_body", "body_name": "Plate"}),
    );
    run(&mut a, "solid.box", json!({"length": 6, "width": 6, "height": 6, "corner": [30, 30, 0], "operation": "new_body", "body_name": "Knob"}));
    run(&mut a, "component.from_bodies", json!({"bodies": ["Plate", "Knob"]}));
    let path = dir.join("fixture.step").to_string_lossy().to_string();
    run(&mut a, "file.export", json!({"path": path}));

    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": path}));
    let f = &s.doc.features[0];
    assert!(matches!(&f.kind, solidcraft_doc::FeatureKind::Import { skip, .. } if *skip == [2, 3]), "{:?}", f.body_names);
    let names = |s: &Session| s.model.state().bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&s), ["Bracket", "Pin", "Plate", "Knob"]);
    let mut expected = bodies_by_component(&mut s);
    let boxes = shown_boxes(&s);

    let mut rename = |s: &mut Session, old: &str, new: &str| {
        run(s, "body.rename", json!({"body": old, "name": new}));
        for (_, bodies) in &mut expected {
            for b in bodies.iter_mut().filter(|b| *b == old) {
                *b = new.to_string();
            }
        }
        // Rebuilt from the stored names, the design is the same.
        run(s, "timeline.compute_all", json!({}));
        assert_eq!(bodies_by_component(s), expected);
        assert_eq!(shown_boxes(s), boxes, "every body is where it was");
    };
    rename(&mut s, "Plate", "Base plate");
    rename(&mut s, "Pin", "Peg");
    rename(&mut s, "Knob", "Handle");
    assert_eq!(names(&s), ["Bracket", "Peg", "Base plate", "Handle"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Deleting a component of an imported assembly deletes its bodies too (#17578). The Import
/// feature that builds them sits in the assembly's own component, so deleting the part's
/// component only dropped the bodies' component assignment, and they stayed in the design.
/// The bodies left keep their names and places, also when the deleted body had their name first.
#[test]
fn deleting_a_component_of_an_imported_assembly_deletes_its_bodies() {
    let dir = temp_dir("delete-subcomponent");
    // `Plate` (a 20 × 20 × 10 box) and `Pin` (a 10 × 10 × 20 box at x = 40), each with a body
    // named "Part": the import names them "Part" and "Part (2)".
    let plate = box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 10.0)).unwrap();
    let pin = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 20.0)).unwrap();
    let part = |name: &str, body| ExportProduct {
        name: name.into(),
        bodies: vec![ExportBody { name: "Part".into(), body, color: None }],
        children: Vec::new(),
    };
    let products = vec![
        ExportProduct {
            name: "TwoParts".into(),
            bodies: Vec::new(),
            children: vec![(1, at(0.0, 0.0, 0.0), "Plate-1".into()), (2, at(40.0, 0.0, 0.0), "Pin-1".into())],
        },
        part("Plate", &plate),
        part("Pin", &pin),
    ];
    let path = dir.join("two_parts.step").to_string_lossy().to_string();
    std::fs::write(&path, step_export_products(&products, 0, &StepHeader::default()).unwrap()).unwrap();
    let volumes = |s: &mut Session| -> Vec<(String, i64)> {
        let m = run(s, "inspect.measure", json!({}));
        let mut out: Vec<(String, i64)> = m["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| (b["name"].as_str().unwrap().to_string(), b["volume_mm3"].as_f64().unwrap().round() as i64))
            .collect();
        out.sort();
        out
    };
    let holding = |s: &mut Session| -> Vec<(String, Vec<String>)> { bodies_by_component(s).into_iter().filter(|(_, b)| !b.is_empty()).collect() };

    // The issue's steps: the pin goes with its component, the plate stays.
    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": path}));
    assert_eq!(volumes(&mut s), [("Part".to_string(), 4000), ("Part (2)".to_string(), 2000)]);
    run(&mut s, "component.delete", json!({"component": "Pin"}));
    assert_eq!(volumes(&mut s), [("Part".to_string(), 4000)]);
    assert_eq!(holding(&mut s), [("Plate".to_string(), vec!["Part".to_string()])]);
    // Undo brings the pin back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(volumes(&mut s).len(), 2);

    // The plate goes: the pin keeps its name, though the plate's "Part" is free now, and its place.
    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": path}));
    let pin_box = shown_boxes(&s).into_iter().find(|b| b[0] == 40_000).unwrap();
    run(&mut s, "component.delete", json!({"component": "Plate"}));
    let expected = [("Pin".to_string(), vec!["Part (2)".to_string()])];
    assert_eq!(holding(&mut s), expected);
    run(&mut s, "timeline.compute_all", json!({}));
    assert_eq!(holding(&mut s), expected, "rebuilt");
    assert_eq!(shown_boxes(&s), [pin_box]);
    // Saved and opened again, it is the same.
    let design = dir.join("pin.solidcraft").to_string_lossy().to_string();
    run(&mut s, "file.save_as", json!({"path": design}));
    let mut again = Session::default();
    run(&mut again, "doc.open", json!({"path": design}));
    assert_eq!(holding(&mut again), expected, "reopened");
    assert_eq!(shown_boxes(&again), [pin_box]);
    let _ = std::fs::remove_dir_all(&dir);
}
