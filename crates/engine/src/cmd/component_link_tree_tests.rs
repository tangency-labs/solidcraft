//! A linked design's own components, kept as components of the design that links it (#5526).
//! Sizes are chosen so every expected box and volume is plain arithmetic: a plate 40 x 20 x 5
//! (4000 mm3), a pin 4 x 6 x 10 (240 mm3) and a rail 40 x 2 x 2 (160 mm3).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-link-tree-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn open(path: &Path) -> Session {
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path}));
    s
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

/// A part: one box with a corner at the origin, and a part number.
fn save_part(path: &Path, size: [f64; 3], body: &str, number: &str) {
    let mut part = Session::default();
    run(&mut part, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "body_name": body}));
    run(&mut part, "document.metadata.set", json!({"values": {"part_number": number}}));
    run(&mut part, "file.save", json!({"path": path}));
}

/// `unit.sc`, an assembly of 4640 mm3: a rail of its own, a linked plate, and a linked pin
/// placed twice, the second turned a quarter turn about Z through its corner at (30, 5).
fn save_unit(dir: &Path) -> PathBuf {
    let (plate, pin, unit) = (dir.join("plate.sc"), dir.join("pin.sc"), dir.join("unit.sc"));
    save_part(&plate, [40.0, 20.0, 5.0], "Plate", "PL-1");
    save_part(&pin, [4.0, 6.0, 10.0], "Pin", "PN-1");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 2, "height": 2, "corner": [0, 22, 0], "body_name": "Rail"}));
    run(&mut s, "component.insert_design", json!({"path": plate, "structure": true}));
    run(&mut s, "component.insert_design", json!({"path": pin, "translate": [5, 5, 5], "structure": true}));
    run(&mut s, "component.insert_design", json!({"path": pin, "reuse": true, "translate": [30, 5, 5]}));
    run(&mut s, "occurrence.move", json!({"occurrence": "pin:2", "axis": [0, 0, 1], "angle": "90 deg", "origin": [30, 5, 0]}));
    run(&mut s, "document.metadata.set", json!({"values": {"part_number": "UNIT-1", "description": "Bearing unit"}}));
    run(&mut s, "file.save", json!({"path": unit}));
    unit
}

/// The box around every body the design shows, in micrometres, sorted.
fn boxes(s: &Session) -> Vec<[i64; 6]> {
    let world = s.world_state().unwrap();
    let mut out: Vec<[i64; 6]> = world
        .bodies
        .iter()
        .map(|b| {
            let x = b.body.exact_bounds().unwrap();
            [x.min.x, x.min.y, x.min.z, x.max.x, x.max.y, x.max.z].map(|v| (v * 1e3).round() as i64)
        })
        .collect();
    out.sort_unstable();
    out
}

/// `boxes` moved by `by` mm.
fn shifted(boxes: &[[i64; 6]], by: [i64; 3]) -> Vec<[i64; 6]> {
    let mut out: Vec<[i64; 6]> = boxes.iter().map(|b| std::array::from_fn(|i| b[i] + by[i % 3] * 1000)).collect();
    out.sort_unstable();
    out
}

fn mm(boxes: &[[i64; 6]]) -> Vec<[i64; 6]> {
    let mut out: Vec<[i64; 6]> = boxes.iter().map(|b| b.map(|v| v * 1000)).collect();
    out.sort_unstable();
    out
}

/// A bill of materials as (part number, count), in its order.
fn counts(bom: &Value) -> Vec<(String, u64)> {
    bom["rows"].as_array().unwrap().iter().map(|r| (r["part_number"].as_str().unwrap().to_string(), r["count"].as_u64().unwrap())).collect()
}

fn rows(expected: &[(&str, u64)]) -> Vec<(String, u64)> {
    expected.iter().map(|(number, count)| (number.to_string(), *count)).collect()
}

fn component(s: &Session, name: &str) -> u64 {
    let found: Vec<u64> = s.doc.components.iter().filter(|c| c.name == name).map(|c| c.id).collect();
    assert_eq!(found.len(), 1, "one component named {name}: {found:?}");
    found[0]
}

/// The placements of `component` inside `parent`, in the design's order.
fn placements(s: &Session, component: u64, parent: u64) -> Vec<u64> {
    s.doc.occurrences.iter().filter(|o| o.component == component && o.parent == parent).map(|o| o.id).collect()
}

/// Inserted with `structure`, a linked assembly's own components are components inside the
/// linked one, every body is where the assembly has it, and the bill of materials counts its
/// parts as the assembly's own design does. The insert is one undo step, and a second occurrence
/// makes nothing twice.
#[test]
fn a_linked_assembly_comes_in_as_a_subassembly_and_its_parts_count_in_the_bill_of_materials() {
    let dir = scratch("nested");
    let unit = save_unit(&dir);
    let source = open(&unit);
    // The unit as it was built: the plate, the pins (the second turned about its corner) and the rail.
    let unit_boxes = boxes(&source);
    assert_eq!(unit_boxes, mm(&[[0, 0, 0, 40, 20, 5], [5, 5, 5, 9, 11, 15], [24, 5, 5, 30, 9, 15], [0, 22, 0, 40, 24, 2]]));

    let mut s = Session::default();
    let undo = s.undo.len();
    let inserted = run(&mut s, "component.insert_design", json!({"path": unit, "translate": [100, 0, 0], "structure": true}));
    let top = inserted["component"].as_u64().unwrap();
    // The unit's plate and pin are components inside it, the pin placed twice.
    assert_eq!(s.doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["unit", "plate", "pin"]);
    assert_eq!(inserted["subcomponents"], 2, "{inserted}");
    assert!(s.doc.assembly.joints.is_empty(), "nothing is joined to its parts: no joint is needed to hold them");
    let (plate, pin) = (component(&s, "plate"), component(&s, "pin"));
    assert_eq!((placements(&s, plate, top).len(), placements(&s, pin, top).len()), (1, 2));
    assert!(s.doc.components.iter().all(|c| c.id == top || s.doc.component_within(c.id, top)), "they are deleted with it");
    // Every body is where the unit has it, moved by the unit's placement.
    assert_eq!(boxes(&s), shifted(&unit_boxes, [100, 0, 0]));
    assert!((volume(&mut s) - 4640.0).abs() < 1e-6, "{}", volume(&mut s));
    // Each body belongs to its part: the rail to the unit itself.
    let state = s.model.state();
    let owner = |body: &str| s.doc.model_body_component(state.body(body).unwrap());
    assert_eq!((owner("Rail"), owner("Plate"), owner("Pin")), (top, plate, pin));
    assert_eq!(state.bodies.len(), 3, "the pin placed twice is one body");
    // The bill of materials counts the unit's parts as the unit's own design does; the unit is
    // a row too, because it has a body of its own.
    let own = run(&mut open(&unit), "assembly.bom", json!({}));
    assert_eq!(counts(&own), rows(&[("PL-1", 1), ("PN-1", 2)]));
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({"component": top}))), counts(&own));
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({}))), rows(&[("PL-1", 1), ("PN-1", 2), ("UNIT-1", 1)]));
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({"levels": "top"}))), rows(&[("UNIT-1", 1)]));
    // It is one step to undo.
    assert_eq!(s.undo.len(), undo + 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.components.is_empty() && s.doc.occurrences.is_empty() && s.doc.assembly.joints.is_empty() && s.doc.links.is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(boxes(&s), shifted(&unit_boxes, [100, 0, 0]));

    // Placed again, the unit's parts count twice and nothing is made twice.
    run(&mut s, "component.insert_design", json!({"path": unit, "reuse": true, "translate": [0, 100, 0]}));
    assert_eq!(s.doc.components.len(), 3);
    let mut both = shifted(&unit_boxes, [100, 0, 0]);
    both.extend(shifted(&unit_boxes, [0, 100, 0]));
    both.sort_unstable();
    assert_eq!(boxes(&s), both);
    assert!((volume(&mut s) - 9280.0).abs() < 1e-6, "{}", volume(&mut s));
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({}))), rows(&[("PL-1", 2), ("PN-1", 4), ("UNIT-1", 2)]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_links_follows_the_parts_a_linked_assembly_gains_moves_renames_and_loses() {
    let dir = scratch("update");
    let unit = save_unit(&dir);
    let mut s = Session::default();
    let top = run(&mut s, "component.insert_design", json!({"path": unit, "structure": true}))["component"].as_u64().unwrap();
    let (plate, pin) = (component(&s, "plate"), component(&s, "pin"));
    let pins = placements(&s, pin, top);
    let evaluated = || super::EVALUATED.with(std::cell::Cell::get);

    // Nothing changed: the unit is not read again.
    let before = evaluated();
    assert_eq!(run(&mut s, "component.update_links", json!({}))["unchanged"], json!(["unit"]));
    assert_eq!(evaluated(), before);

    // The unit gains a third pin and gives its pin another part number: the same bodies, so
    // only what it places and what it calls them changes.
    let mut source = open(&unit);
    run(&mut source, "component.insert_design", json!({"path": dir.join("pin.sc"), "reuse": true, "translate": [15, 5, 5]}));
    run(&mut source, "component.metadata.set", json!({"component": "pin", "part_number": "PN-2"}));
    run(&mut source, "file.save", json!({}));
    let updated = run(&mut s, "component.update_links", json!({}));
    assert_eq!(updated["updated"], json!(["unit"]), "{updated}");
    assert_eq!(boxes(&s), boxes(&source));
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({"component": top}))), rows(&[("PL-1", 1), ("PN-2", 3)]));
    // The parts it had are the same components and placements here.
    assert_eq!((component(&s, "plate"), component(&s, "pin")), (plate, pin));
    assert_eq!(placements(&s, pin, top)[..2], pins[..]);
    assert_eq!(run(&mut s, "component.update_links", json!({}))["unchanged"], json!(["unit"]));

    // The unit loses its second pin, moves its third and takes a thicker plate.
    save_part(&dir.join("plate.sc"), [40.0, 20.0, 8.0], "Plate", "PL-1");
    run(&mut source, "component.update_links", json!({}));
    run(&mut source, "selection.delete", json!({"occurrences": ["pin:2"]}));
    run(&mut source, "occurrence.move", json!({"occurrence": "pin:3", "translate": [0, 0, 20]}));
    run(&mut source, "component.rename", json!({"component": "pin", "name": "dowel"}));
    run(&mut source, "file.save", json!({}));
    let updated = run(&mut s, "component.update_links", json!({}));
    assert_eq!(updated["updated"], json!(["unit"]), "{updated}");
    assert_eq!(boxes(&s), boxes(&source));
    assert!((volume(&mut s) - (6400.0 + 480.0 + 160.0)).abs() < 1e-6, "{}", volume(&mut s));
    assert_eq!(component(&s, "dowel"), pin, "renamed, it is the same component");
    let now = placements(&s, pin, top);
    assert_eq!(now.len(), 2);
    assert_eq!(now[0], pins[0], "the first pin is the placement it was");
    assert!(!now.contains(&pins[1]), "the second went with the unit's");
    let names: Vec<&str> = s.doc.occurrences.iter().filter(|o| o.component == pin).map(|o| o.name.as_str()).collect();
    assert!(names.iter().all(|n| n.starts_with("dowel:")), "{names:?}");

    // One undo takes the whole update back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(placements(&s, pin, top).len(), 3);
    run(&mut s, "edit.redo", json!({}));

    // The unit loses the pin altogether: its component goes here too.
    run(&mut source, "component.delete", json!({"component": "dowel"}));
    run(&mut source, "file.save", json!({}));
    run(&mut s, "component.update_links", json!({}));
    assert_eq!(s.doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["unit", "plate"]);
    assert_eq!(boxes(&s), boxes(&source));
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({"component": top}))), rows(&[("PL-1", 1)]));
    assert!(s.doc.assembly.joints.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a link keeps is saved with the design, in both file formats: it opens again with the
/// same components, occurrences and link record, and an Update Links that finds nothing changed
/// leaves it without unsaved changes.
#[test]
fn a_saved_assembly_reopens_with_its_subassemblies() {
    let dir = scratch("reopen");
    let unit = save_unit(&dir);
    let mut s = Session::default();
    let top =
        run(&mut s, "component.insert_design", json!({"path": unit, "translate": [0, 0, 50], "structure": true}))["component"].as_u64().unwrap();
    let bom = run(&mut s, "assembly.bom", json!({}));
    for (file, format) in [("machine.json", "json"), ("machine.sc", "container")] {
        let path = dir.join(file);
        run(&mut s, "file.save_as", json!({"path": path, "format": format}));
        let mut back = open(&path);
        assert_eq!(back.doc.components, s.doc.components, "{format}");
        assert_eq!(back.doc.occurrences, s.doc.occurrences, "{format}");
        let tree = back.doc.links.get(&top).and_then(|link| link.tree.clone()).unwrap();
        assert_eq!((tree.components.len(), tree.occurrences.len()), (2, 3), "{format}");
        assert_eq!(Some(&tree), s.doc.links.get(&top).and_then(|link| link.tree.as_ref()), "{format}");
        assert_eq!(boxes(&back), boxes(&s), "{format}");
        assert_eq!(run(&mut back, "assembly.bom", json!({}))["rows"], bom["rows"], "{format}");
        assert_eq!(run(&mut back, "component.update_links", json!({}))["unchanged"], json!(["unit"]), "{format}");
        assert!(!back.is_dirty(), "{format}: an update that finds nothing changed changes nothing");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn subassemblies_nest_in_subassemblies() {
    let dir = scratch("levels");
    let unit = save_unit(&dir);
    // A machine: two units and a plate of its own.
    let machine = dir.join("machine.sc");
    let mut m = Session::default();
    run(&mut m, "component.insert_design", json!({"path": unit, "structure": true}));
    run(&mut m, "component.insert_design", json!({"path": unit, "reuse": true, "translate": [0, 60, 0]}));
    run(&mut m, "component.insert_design", json!({"path": dir.join("plate.sc"), "translate": [0, 0, 30], "structure": true}));
    run(&mut m, "file.save", json!({"path": machine}));
    assert!((volume(&mut m) - (2.0 * 4640.0 + 4000.0)).abs() < 1e-6);
    let machine_boxes = boxes(&m);
    assert_eq!(machine_boxes.len(), 9);

    // A line of two machines.
    let mut s = Session::default();
    let top = run(&mut s, "component.insert_design", json!({"path": machine, "structure": true}))["component"].as_u64().unwrap();
    run(&mut s, "component.insert_design", json!({"path": machine, "reuse": true, "translate": [200, 0, 0]}));
    let mut both = machine_boxes.clone();
    both.extend(shifted(&machine_boxes, [200, 0, 0]));
    both.sort_unstable();
    assert_eq!(boxes(&s), both);
    assert!((volume(&mut s) - 2.0 * (2.0 * 4640.0 + 4000.0)).abs() < 1e-6, "{}", volume(&mut s));
    // machine > unit > plate, pin; and machine > plate.
    let unit_here = component(&s, "unit");
    assert_eq!(placements(&s, unit_here, top).len(), 2);
    assert_eq!(s.doc.components.iter().filter(|c| c.name == "plate").count(), 2, "the unit's plate and the machine's own");
    assert_eq!(placements(&s, component(&s, "pin"), unit_here).len(), 2);
    // Each machine has 3 plates, 4 pins and 2 units.
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({}))), rows(&[("PL-1", 6), ("PN-1", 8), ("UNIT-1", 4)]));
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({"component": top}))), counts(&run(&mut m, "assembly.bom", json!({}))));
    let top_level = run(&mut s, "assembly.bom", json!({"levels": "top"}));
    assert_eq!((top_level["rows"][0]["definition"].clone(), top_level["rows"][0]["count"].clone()), (json!("machine"), json!(2)), "{top_level}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_joint_to_a_part_of_a_subassembly_moves_the_subassembly() {
    let dir = scratch("joint");
    let unit = save_unit(&dir);
    let unit_boxes = boxes(&open(&unit));
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-50, 0, 0], "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let top =
        run(&mut s, "component.insert_design", json!({"path": unit, "translate": [100, 0, 0], "structure": true}))["component"].as_u64().unwrap();
    let plate = placements(&s, component(&s, "plate"), top)[0];
    // A slider along Z between the grounded base and the plate inside the unit.
    let origin = |occurrence: Value| json!({"occurrence": occurrence, "point": [0, 0, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "slider", "name": "Lift", "flip": true, "a": origin(json!("Base")), "b": origin(json!(plate))}));
    let block = mm(&[[-50, 0, 0, -40, 10, 10]]);
    let with_block = |unit: Vec<[i64; 6]>| {
        let mut all = unit;
        all.extend(block.iter().copied());
        all.sort_unstable();
        all
    };
    assert_eq!(boxes(&s), with_block(shifted(&unit_boxes, [100, 0, 0])), "joined where it sits, nothing moves");
    // The joint is on the unit's occurrence, through the plate inside it: nothing is added to
    // hold the plate, and the pins need nothing either.
    let unit_here = s.doc.occurrences.iter().find(|o| o.component == top).map(|o| o.id).unwrap();
    assert_eq!(s.doc.assembly.joints.len(), 1, "{:?}", s.doc.assembly.joints);
    let lift = s.doc.assembly.joints[0].clone();
    assert_eq!((lift.b.occurrence, lift.b.inside.clone()), (unit_here, vec![plate]));
    let driven = run(&mut s, "joint.drive", json!({"joint": "Lift", "value": 10}));
    assert_eq!(driven["moved"], true, "{driven}");
    // The plate rises 10 mm, and the rail and the pins with it.
    assert_eq!(boxes(&s), with_block(shifted(&unit_boxes, [100, 0, 10])));
    // The pins cannot be dragged out of the unit meanwhile, held by a joint or not.
    let pin = placements(&s, component(&s, "pin"), top)[0];
    assert!(s.execute("occurrence.move", &json!({"occurrence": pin, "translate": [0, 0, 5]})).is_err());
    // Without the joint the plate needs no holding either.
    run(&mut s, "joint.delete", json!({"joint": "Lift"}));
    assert!(s.doc.assembly.joints.is_empty(), "{:?}", s.doc.assembly.joints);
    assert_eq!(boxes(&s), with_block(shifted(&unit_boxes, [100, 0, 10])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_joint_to_a_part_two_levels_down_moves_the_outer_subassembly() {
    let dir = scratch("deep-joint");
    let unit = save_unit(&dir);
    let machine = dir.join("machine.sc");
    let mut m = Session::default();
    run(&mut m, "component.insert_design", json!({"path": unit, "structure": true}));
    run(&mut m, "component.insert_design", json!({"path": unit, "reuse": true, "translate": [0, 60, 0]}));
    run(&mut m, "file.save", json!({"path": machine}));
    let machine_boxes = boxes(&m);

    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-50, 0, 0], "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let top = run(&mut s, "component.insert_design", json!({"path": machine, "structure": true}))["component"].as_u64().unwrap();
    let unit_here = component(&s, "unit");
    let pin = placements(&s, component(&s, "pin"), unit_here)[0];
    let origin = |occurrence: Value| json!({"occurrence": occurrence, "point": [0, 0, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "slider", "name": "Lift", "flip": true, "a": origin(json!("Base")), "b": origin(json!(pin))}));
    // One joint, on the machine's occurrence through the unit and the pin inside it.
    let machine_here = s.doc.occurrences.iter().find(|o| o.component == top).map(|o| o.id).unwrap();
    assert_eq!(s.doc.assembly.joints.len(), 1, "{:?}", s.doc.assembly.joints);
    let lift = s.doc.assembly.joints[0].clone();
    assert_eq!((lift.b.occurrence, lift.b.inside.clone()), (machine_here, vec![placements(&s, unit_here, top)[0], pin]));
    let driven = run(&mut s, "joint.drive", json!({"joint": "Lift", "value": 10}));
    assert_eq!(driven["moved"], true, "{driven}");
    let mut all = shifted(&machine_boxes, [0, 0, 10]);
    all.extend(mm(&[[-50, 0, 0, -40, 10, 10]]));
    all.sort_unstable();
    assert_eq!(boxes(&s), all, "both units and everything in them rise with the pin");
    // Saved and opened again, it is the same assembly with the same joint.
    let saved = dir.join("line.sc");
    run(&mut s, "file.save", json!({"path": saved}));
    let back = open(&saved);
    assert_eq!(back.doc.assembly.joints, s.doc.assembly.joints);
    assert_eq!(boxes(&back), all);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_parts_of_a_linked_assembly_stay_as_its_design_has_them_until_the_link_is_broken() {
    let dir = scratch("guards");
    let unit = save_unit(&dir);
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Cell", "activate": false}));
    let top = run(&mut s, "component.insert_design", json!({"path": unit, "parent": "Cell", "structure": true}))["component"].as_u64().unwrap();
    let pin = component(&s, "pin");
    let placed = placements(&s, pin, top);
    let before = s.doc.clone();
    let refused =
        |s: &mut Session, id: &str, p: Value| s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{id} {p} went through"));
    // A part cannot be moved, unlocked or deleted on its own: the unit's design decides.
    assert!(refused(&mut s, "occurrence.move", json!({"occurrence": placed[0], "translate": [0, 0, 5]})).contains("linked subassembly"));
    assert!(refused(&mut s, "component.unlock", json!({"component": top})).contains("Break Link"));
    assert!(refused(&mut s, "component.delete", json!({"component": pin})).contains("linked subassembly"));
    assert!(refused(&mut s, "selection.delete", json!({"occurrences": [placed[1].to_string()]})).contains("linked subassembly"));
    assert!(refused(&mut s, "selection.delete", json!({"components": [pin.to_string()]})).contains("linked subassembly"));
    // A component around the unit does not unlock the unit's parts either.
    assert!(refused(&mut s, "component.unlock", json!({"component": "Cell"})).contains("Break Link"));
    assert_eq!(s.doc, before, "nothing changed");

    // Break Link keeps the parts and frees them.
    run(&mut s, "component.break_link", json!({"component": top}));
    assert!(s.doc.links.is_empty());
    assert_eq!(s.doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Cell", "unit", "plate", "pin"]);
    assert!(refused(&mut s, "component.unlock", json!({"component": top})).contains("nothing holds"));
    run(&mut s, "occurrence.move", json!({"occurrence": placed[0], "translate": [0, 0, 5]}));
    run(&mut s, "selection.delete", json!({"occurrences": [placed[1].to_string()]}));
    assert_eq!(placements(&s, pin, top), [placed[0]]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `hinge.sc`: a base, grounded as the design's first part, and an arm on a revolute joint
/// "Hinge" about Z through (0, 10), limited to a quarter turn each way, saved at 0. The arm is
/// the box (0, 8, 5)..(30, 12, 9); a quarter turn puts it at (-2, 10, 5)..(2, 40, 9).
fn save_hinge(dir: &Path) -> PathBuf {
    let (base, arm, hinge) = (dir.join("base.sc"), dir.join("arm.sc"), dir.join("hinge.sc"));
    save_part(&base, [40.0, 20.0, 5.0], "Base", "BS-1");
    save_part(&arm, [30.0, 4.0, 4.0], "Arm", "AR-1");
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": base, "structure": true}));
    run(&mut s, "component.insert_design", json!({"path": arm, "translate": [0, 8, 5], "structure": true}));
    let origin = |part: &str| json!({"occurrence": part, "point": [0, 10, 7], "z": [0, 0, 1]});
    let joint = json!({"type": "revolute", "name": "Hinge", "flip": true, "limits": [[-90, 90]], "a": origin("base"), "b": origin("arm")});
    run(&mut s, "joint.create", joint);
    run(&mut s, "file.save", json!({"path": hinge}));
    hinge
}

/// Two lists of boxes as one, in the order `boxes` gives.
fn together(a: Vec<[i64; 6]>, b: Vec<[i64; 6]>) -> Vec<[i64; 6]> {
    let mut all = a;
    all.extend(b);
    all.sort_unstable();
    all
}

/// The joint an occurrence of a linked component has for the linked design's joint: its id and name.
fn joint_of(s: &Session, top: u64, occurrence: u64) -> (u64, String) {
    let id =
        s.doc.links[&top].tree.as_ref().and_then(|tree| tree.flexible.get(&occurrence)).and_then(|joints| joints.values().next().copied()).unwrap();
    (id, s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.name.clone()).unwrap())
}

/// Each occurrence of a linked subassembly is rigid or flexible on its own (#5526), and a
/// flexible one has its own pose (#1516): two occurrences of one hinge stand at different angles.
#[test]
fn each_occurrence_of_a_subassembly_is_rigid_or_flexible_and_has_its_own_pose() {
    let dir = scratch("flexible");
    let hinge = save_hinge(&dir);
    let saved = boxes(&open(&hinge));
    assert_eq!(saved, mm(&[[0, 0, 0, 40, 20, 5], [0, 8, 5, 30, 12, 9]]));
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "translate": [0, 0, 50], "structure": true}));
    let (top, first) = (inserted["component"].as_u64().unwrap(), inserted["occurrence"].as_u64().unwrap());
    // Rigid, as a link is made: its design's joint is not a joint here, and it moves as one.
    assert!(s.doc.assembly.joints.is_empty());
    assert!(s.execute("joint.drive", &json!({"joint": "Hinge", "value": 90})).is_err());
    assert_eq!(run(&mut s, "component.links", json!({}))["links"][0]["flexible"], json!([]));

    // Flexible: the hinge is a joint here, on this occurrence's own copy of the design's parts.
    let mode = run(&mut s, "component.link_mode", json!({"component": top, "flexible": true}));
    assert_eq!((mode["structure"].clone(), mode["flexible"].clone(), mode["joints"].clone()), (json!(true), json!([first]), json!(1)), "{mode}");
    assert_eq!(run(&mut s, "component.links", json!({}))["links"][0]["flexible"], json!([first]));
    let (base, arm) = (placements(&s, component(&s, "base"), top)[0], placements(&s, component(&s, "arm"), top)[0]);
    assert_eq!(s.doc.assembly.joints.len(), 1, "the design's joint and no other: {:?}", s.doc.assembly.joints);
    let hinge_here = s.doc.assembly.joints[0].clone();
    assert_eq!(hinge_here.name, "Hinge");
    assert_eq!((hinge_here.a.occurrence, hinge_here.a.inside.clone()), (first, vec![base]));
    assert_eq!((hinge_here.b.occurrence, hinge_here.b.inside.clone()), (first, vec![arm]));
    assert_eq!(boxes(&s), shifted(&saved, [0, 0, 50]), "made flexible, nothing moves");

    // Driven here, it takes the pose the design takes when driven itself. The pose is this
    // occurrence's: the placements kept for the design stay where the design has them.
    let kept = s.doc.occurrences.clone();
    let driven = run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    assert_eq!(driven["moved"], true, "{driven}");
    let mut source = open(&hinge);
    run(&mut source, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    let up = boxes(&source);
    assert_eq!(up, mm(&[[0, 0, 0, 40, 20, 5], [-2, 10, 5, 2, 40, 9]]));
    assert_eq!(boxes(&s), shifted(&up, [0, 0, 50]));
    assert_eq!(s.doc.occurrences, kept);
    let posed: Vec<(u64, u64)> = s.doc.assembly.poses.iter().map(|p| (p.occurrence, p.inner)).collect();
    assert_eq!(posed, [(first, arm)]);
    // Its limits are the design's.
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 120}));
    assert_eq!(boxes(&s), shifted(&up, [0, 0, 50]), "stopped at 90");

    // Another occurrence of the same subassembly is rigid: it shows the design as it was saved.
    let second =
        run(&mut s, "component.insert_design", json!({"path": hinge, "reuse": true, "translate": [0, 100, 50]}))["occurrence"].as_u64().unwrap();
    assert_eq!(s.doc.assembly.joints.len(), 1);
    assert_eq!(boxes(&s), together(shifted(&up, [0, 0, 50]), shifted(&saved, [0, 100, 50])));
    // Made flexible, it has the joint for itself under its own name, at the design's value.
    let mode = run(&mut s, "component.link_mode", json!({"component": top, "occurrence": second, "flexible": true}));
    assert_eq!(mode["flexible"], json!([first, second]), "{mode}");
    let (other_id, other) = joint_of(&s, top, second);
    assert_eq!((joint_of(&s, top, first).0, other.as_str()), (hinge_here.id, "hinge:2: Hinge"));
    assert_eq!(boxes(&s), together(shifted(&up, [0, 0, 50]), shifted(&saved, [0, 100, 50])), "made flexible, nothing moves");
    // Joined to nothing grounded it can't be driven, and the reply names it, not the part in it.
    let floating = run(&mut s, "joint.drive", json!({"joint": other, "value": -90}));
    assert_eq!((floating["moved"].clone(), floating["unconnected"].clone()), (json!(false), json!("hinge:2")), "{floating}");
    run(&mut s, "occurrence.ground", json!({"occurrence": second, "grounded": true}));
    let down = mm(&[[0, 0, 0, 40, 20, 5], [-2, -20, 5, 2, 10, 9]]);
    assert_eq!(boxes(&s), together(shifted(&up, [0, 0, 50]), shifted(&down, [0, 100, 50])), "the two stand in different poses");
    // What reads the design reads each where it stands: physical properties, and a STEP file.
    let centre = |s: &mut Session, occurrence: u64| -> Vec<i64> {
        let p = run(s, "occurrence.properties", json!({"occurrence": occurrence}));
        p["total"]["center_of_mass"].as_array().unwrap().iter().map(|v| (v.as_f64().unwrap() * 1e3).round() as i64).collect()
    };
    // The base, 4000 mm3 at (20, 10, 2.5), and the arm, 480 mm3 at (0, 25, 7) up and (0, -5, 7) down.
    assert_eq!((centre(&mut s, first), centre(&mut s, second)), (vec![17857, 11607, 52982], vec![17857, 108393, 52982]));
    let file = dir.join("line.step");
    run(&mut s, "file.export", json!({"path": file}));
    let exported = solidcraft_kernel::step_import(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let mut read: Vec<[i64; 6]> = exported
        .bodies
        .iter()
        .map(|b| {
            let x = b.body.exact_bounds().unwrap();
            [x.min.x, x.min.y, x.min.z, x.max.x, x.max.y, x.max.z].map(|v| (v * 1e3).round() as i64)
        })
        .collect();
    read.sort_unstable();
    assert_eq!(read, boxes(&s), "the STEP assembly has each hinge in its own pose");
    // Each is driven on its own.
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 0}));
    assert_eq!(boxes(&s), together(shifted(&saved, [0, 0, 50]), shifted(&down, [0, 100, 50])));
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    // The bill of materials counts the parts of both.
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({}))), rows(&[("AR-1", 2), ("BS-1", 2)]));

    // Update Links takes the design's change (wider limits, still saved at 0), and each
    // occurrence keeps its joint and the pose it has here.
    let mut design = open(&hinge);
    run(&mut design, "joint.limits", json!({"joint": "Hinge", "min": -120, "max": 120}));
    run(&mut design, "file.save", json!({}));
    assert_eq!(run(&mut s, "component.update_links", json!({}))["updated"], json!(["hinge"]));
    let both = together(shifted(&up, [0, 0, 50]), shifted(&down, [0, 100, 50]));
    assert_eq!(boxes(&s), both, "each still where it was");
    assert_eq!((joint_of(&s, top, first).0, joint_of(&s, top, second).0), (hinge_here.id, other_id), "the same joints");
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 110}));
    let at = s.doc.assembly.joints.iter().find(|j| j.name == "Hinge").map(|j| j.values[0].to_degrees()).unwrap();
    assert!((at - 110.0).abs() < 1e-6, "{at}");
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));

    // Saved and opened again, each is flexible at its pose.
    let line = dir.join("line.sc");
    run(&mut s, "file.save", json!({"path": line}));
    let back = open(&line);
    assert_eq!(back.doc.links, s.doc.links);
    assert_eq!(back.doc.assembly, s.doc.assembly);
    assert_eq!(boxes(&back), both);

    // A copy of a flexible occurrence is flexible too, in the same pose, with a joint of its own.
    let copy = run(&mut s, "occurrence.copy", json!({"occurrence": second, "translate": [0, 100, 0]}))["occurrence"].as_u64().unwrap();
    assert_eq!(boxes(&s), together(both.clone(), shifted(&down, [0, 200, 50])));
    assert_eq!(joint_of(&s, top, copy).1, "hinge:3: Hinge");
    // Deleted, its joint and its pose go with it.
    run(&mut s, "selection.delete", json!({"occurrences": [copy]}));
    assert_eq!(boxes(&s), both);
    assert_eq!(s.doc.assembly.joints.len(), 2);
    assert!(s.doc.assembly.poses.iter().all(|p| p.occurrence != copy));
    assert!(!s.doc.links[&top].tree.as_ref().unwrap().flexible.contains_key(&copy));

    // One made rigid again shows the design as saved; the other keeps its pose.
    let mode = run(&mut s, "component.link_mode", json!({"component": top, "occurrence": first, "flexible": false}));
    assert_eq!(mode["flexible"], json!([second]), "{mode}");
    assert_eq!(boxes(&s), together(shifted(&saved, [0, 0, 50]), shifted(&down, [0, 100, 50])));
    assert!(s.execute("joint.drive", &json!({"joint": "Hinge", "value": 10})).is_err(), "its joint went");
    // All rigid: no joint and no pose is left.
    let mode = run(&mut s, "component.link_mode", json!({"component": top, "flexible": false}));
    assert_eq!((mode["flexible"].clone(), mode["joints"].clone()), (json!([]), json!(0)), "{mode}");
    assert!(s.doc.assembly.joints.is_empty(), "{:?}", s.doc.assembly.joints);
    assert!(s.doc.assembly.poses.is_empty());
    assert_eq!(boxes(&s), together(shifted(&saved, [0, 0, 50]), shifted(&saved, [0, 100, 50])));
    // A link that keeps no components has no parts for joints to move.
    assert!(s.execute("component.link_mode", &json!({"component": top, "structure": false, "flexible": true})).is_err());
    assert!(s.execute("component.link_mode", &json!({"component": top, "occurrence": first})).is_err(), "an occurrence needs `flexible`");
    assert!(s.execute("component.link_mode", &json!({"component": top, "occurrence": base, "flexible": true})).is_err(), "not an occurrence of it");
    assert!(s.execute("component.insert_design", &json!({"path": hinge, "flexible": "yes", "structure": true})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A joint of this design made to a part of a flexible occurrence is on that occurrence's own
/// copy of the part: it moves the part by the design's joint, not the whole subassembly.
#[test]
fn a_joint_to_a_part_of_a_flexible_occurrence_moves_that_part() {
    let dir = scratch("flexible-joined");
    let hinge = save_hinge(&dir);
    let saved = boxes(&open(&hinge));
    let mut s = Session::default();
    // A grounded block, then the hinge, flexible, its base held to the block.
    run(&mut s, "component.create", json!({"name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-100, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let block = boxes(&s);
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "translate": [0, 0, 50], "flexible": true}));
    let top = inserted["component"].as_u64().unwrap();
    let base = placements(&s, component(&s, "base"), top)[0];
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Block", "b": base, "name": "Mount"}));
    assert_eq!(boxes(&s), together(block.clone(), shifted(&saved, [0, 0, 50])), "as built, nothing moves");
    // The hinge's own joint turns the arm, the base stays on the block.
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    let up = mm(&[[0, 0, 0, 40, 20, 5], [-2, 10, 5, 2, 40, 9]]);
    assert_eq!(boxes(&s), together(block.clone(), shifted(&up, [0, 0, 50])));
    // The placements kept for the design are as the design has them: no joint holds them.
    let names: Vec<&str> = s.doc.assembly.joints.iter().map(|j| j.name.as_str()).collect();
    assert_eq!(names, ["Hinge", "Mount"], "no joint is added to hold a part");
    // Break Link: the parts are the component's own again, as the design was saved, and the
    // joints the occurrence had from its design are gone with its pose.
    run(&mut s, "component.break_link", json!({"component": top}));
    assert_eq!(boxes(&s), together(block, shifted(&saved, [0, 0, 50])));
    assert!(s.doc.assembly.poses.is_empty());
    assert!(s.doc.assembly.joints.iter().all(|j| j.name != "Hinge"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A loop through a flexible occurrence closes by its design's joint: a lever of this design,
/// turning about the hinge's axis and fixed to the hinge's arm, takes the arm with it, and the
/// hinge's own joint follows. (A rigid occurrence would have to move as one, and could not.)
#[test]
fn a_loop_through_a_flexible_occurrence_moves_its_designs_joint() {
    let dir = scratch("flexible-loop");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-100, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Lever"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-100, 40, 50]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "translate": [0, 0, 50], "flexible": true}));
    let top = inserted["component"].as_u64().unwrap();
    let (base, arm) = (placements(&s, component(&s, "base"), top)[0], placements(&s, component(&s, "arm"), top)[0]);
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Block", "b": base, "name": "Mount"}));
    // The lever turns on the block about the hinge's axis, Z through (0, 10), and holds the arm.
    let axis = |part: &str| json!({"occurrence": part, "point": [0, 10, 57], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "revolute", "name": "Crank", "flip": true, "a": axis("Block"), "b": axis("Lever")}));
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Lever", "b": arm, "name": "Link"}));
    let before = boxes(&s);
    let list = run(&mut s, "joint.list", json!({}));
    assert_eq!(list["conflicts"], json!([]), "{list}");
    let driven = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 90}));
    assert_eq!(driven["moved"], true, "{driven}");
    // The arm went a quarter turn with the lever: the hinge's joint stands at a quarter turn.
    let angle = s.doc.assembly.joints.iter().find(|j| j.name == "Hinge").map(|j| j.values[0].to_degrees()).unwrap();
    assert!((angle.abs() - 90.0).abs() < 1e-6, "{angle}");
    let after = boxes(&s);
    assert_ne!(after, before);
    assert!(after.contains(&[-2_000, 10_000, 55_000, 2_000, 40_000, 59_000]), "the arm, turned: {after:?}");
    assert!(after.contains(&[0, 0, 50_000, 40_000, 20_000, 55_000]), "the base, where it was: {after:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A joint between two datum planes means the other way round what `flip` means between two
/// frames, and a carried joint's sides are frames: carried, it must still hold its two
/// components on the same sides of the plane.
#[test]
fn a_carried_joint_between_two_datum_planes_keeps_its_side() {
    let dir = scratch("datum");
    let (base, arm, flat) = (dir.join("base.sc"), dir.join("arm.sc"), dir.join("flat.sc"));
    save_part(&base, [40.0, 20.0, 5.0], "Base", "BS-1");
    save_part(&arm, [30.0, 4.0, 4.0], "Arm", "AR-1");
    let mut design = Session::default();
    run(&mut design, "component.insert_design", json!({"path": base, "structure": true}));
    run(&mut design, "component.insert_design", json!({"path": arm, "translate": [0, 8, 5], "structure": true}));
    let plane = |part: &str| json!({"occurrence": part, "plane": "XY"});
    run(&mut design, "joint.create", json!({"type": "planar", "name": "Flat", "a": plane("base"), "b": plane("arm")}));
    run(&mut design, "file.save", json!({"path": flat}));
    let saved = boxes(&design);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": flat, "flexible": true}));
    assert_eq!(inserted["joints"], 1, "{inserted}");
    let list = run(&mut s, "joint.list", json!({}));
    assert_eq!((list["conflicts"].clone(), list["errors"].clone()), (json!([]), json!([])), "{list}");
    assert_eq!(boxes(&s), saved, "the arm is on the side of the plane the design has it on");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A pose is kept for what an occurrence's joints move. When the linked design drops the joint,
/// Update Links puts that occurrence back where the design has it.
#[test]
fn update_links_takes_back_the_pose_of_what_the_design_no_longer_moves() {
    let dir = scratch("stale-pose");
    let hinge = save_hinge(&dir);
    let saved = boxes(&open(&hinge));
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": hinge, "flexible": true}));
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    assert_ne!(boxes(&s), saved);
    let mut design = open(&hinge);
    run(&mut design, "joint.delete", json!({"joint": "Hinge"}));
    run(&mut design, "file.save", json!({}));
    run(&mut s, "component.update_links", json!({}));
    assert!(s.doc.assembly.joints.is_empty(), "{:?}", s.doc.assembly.joints);
    assert!(s.doc.assembly.poses.is_empty(), "{:?}", s.doc.assembly.poses);
    assert_eq!(boxes(&s), saved);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a link keeps for its linked design is that design's: nothing is made inside it, copied
/// from it or grounded in it here, and the joints a flexible occurrence has from the design
/// are not deleted or changed here. Otherwise Update Links would take what was made with it.
#[test]
fn what_a_link_keeps_is_changed_in_its_design_only() {
    let dir = scratch("kept-guards");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Cell", "activate": false}));
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "parent": "Cell", "flexible": true}));
    let (top, first) = (inserted["component"].as_u64().unwrap(), inserted["occurrence"].as_u64().unwrap());
    let arm_component = component(&s, "arm");
    let arm = placements(&s, arm_component, top)[0];
    let before = s.doc.clone();
    let refused =
        |s: &mut Session, id: &str, p: Value| s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{id} {p} went through"));
    for (command, params) in [
        ("component.activate", json!({"component": arm_component})),
        ("component.create", json!({"name": "Mine", "parent": arm_component})),
        ("component.insert_design", json!({"path": hinge, "parent": arm_component, "structure": true})),
        ("occurrence.copy", json!({"occurrence": arm})),
        ("occurrence.ground", json!({"occurrence": arm, "grounded": true})),
        ("joint.delete", json!({"joint": "Hinge"})),
        ("joint.limits", json!({"joint": "Hinge", "min": -10, "max": 10})),
        ("joint.edit", json!({"joint": "Hinge", "flip": false})),
        ("selection.delete", json!({"joints": ["Hinge"]})),
    ] {
        let why = refused(&mut s, command, params);
        assert!(why.contains("linked subassembly"), "{command}: {why}");
    }
    // A component around a flexible occurrence does not go to a design file of its own.
    let why = refused(&mut s, "component.extract", json!({"component": "Cell", "path": dir.join("cell.sc")}));
    assert!(why.contains("flexible occurrence"), "{why}");
    assert_eq!(s.doc, before, "nothing changed");

    // Rigid again needs nothing of the linked design, also when its file is gone; one undo.
    std::fs::rename(&hinge, dir.join("away.sc")).unwrap();
    run(&mut s, "component.link_mode", json!({"component": top, "occurrence": first, "flexible": false}));
    assert!(s.doc.assembly.joints.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.assembly.joints, before.assembly.joints);
    assert_eq!(s.doc.links, before.links);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A record is not trusted: a link whose record names a joint that is not on its occurrence
/// (a damaged or crafted design file) does not rewrite or delete that joint. A motion link
/// made here between the joints of two occurrences is this design's, and stays.
#[test]
fn a_link_touches_only_the_joints_that_are_on_its_occurrences() {
    let dir = scratch("stray-record");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    for name in ["A", "B"] {
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [-60, 0, 0]}));
        run(&mut s, "component.activate", json!({"component": 0}));
    }
    let origin = |part: &str| json!({"occurrence": part, "point": [-60, 0, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "revolute", "name": "Mine", "flip": true, "a": origin("A"), "b": origin("B")}));
    let mine = s.doc.assembly.joints[0].clone();
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "translate": [0, 0, 50], "flexible": true}));
    let (top, first) = (inserted["component"].as_u64().unwrap(), inserted["occurrence"].as_u64().unwrap());
    let second =
        run(&mut s, "component.insert_design", json!({"path": hinge, "reuse": true, "translate": [0, 100, 50], "flexible": true}))["occurrence"]
            .as_u64()
            .unwrap();
    let (a, b) = (joint_of(&s, top, first).1, joint_of(&s, top, second).1);
    run(&mut s, "joint.motion_link", json!({"a": a, "b": b, "ratio": -1}));
    assert_eq!(s.doc.assembly.links.len(), 1);
    // The record of the first occurrence now names the joint that is not the link's.
    let mut doc = (*s.doc).clone();
    for joint in doc.links.get_mut(&top).unwrap().tree.as_mut().unwrap().flexible.get_mut(&first).unwrap().values_mut() {
        *joint = mine.id;
    }
    s.set_doc(doc);
    let untouched = |s: &Session| s.doc.assembly.joints.iter().find(|j| j.id == mine.id).cloned();
    run(&mut s, "component.link_mode", json!({"component": top, "flexible": true}));
    assert_eq!(untouched(&s), Some(mine.clone()), "written again from the design: not this one");
    run(&mut s, "component.link_mode", json!({"component": top, "flexible": false}));
    assert_eq!(untouched(&s), Some(mine.clone()), "made rigid: not removed");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.assembly.links.len(), 1, "the motion link between the two occurrences' joints is kept");
    run(&mut s, "component.break_link", json!({"component": top}));
    assert_eq!(untouched(&s), Some(mine));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A pose is where joints last put something. A design file whose pose is no placement, or
/// names occurrences the design has not got, still opens: such a pose is dropped.
#[test]
fn a_design_with_a_damaged_pose_still_opens() {
    let dir = scratch("damaged-pose");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": hinge, "flexible": true}));
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    let mut design = serde_json::to_value(&*s.doc).unwrap();
    let poses = design["assembly"]["poses"].as_array_mut().expect("the design has a pose");
    assert_eq!(poses.len(), 1);
    let mut stray = poses[0].clone();
    stray["occurrence"] = json!(987_654);
    poses[0]["transform"][0][0] = json!(2.0);
    poses.push(stray);
    let file = dir.join("damaged.json");
    std::fs::write(&file, serde_json::to_string(&design).unwrap()).unwrap();
    let back = open(&file);
    assert!(
        back.doc.assembly.poses.iter().all(|p| solidcraft_doc::is_placement(&p.transform) && p.occurrence != 987_654),
        "{:?}",
        back.doc.assembly.poses
    );
    assert_eq!(back.doc.assembly.joints.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_design_inserted_flexible_brings_its_joints_and_the_list_says_which_parts_a_link_keeps() {
    let dir = scratch("flexible-insert");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "flexible": true}));
    let top = inserted["component"].as_u64().unwrap();
    assert_eq!((inserted["subcomponents"].clone(), inserted["joints"].clone()), (json!(2), json!(1)), "{inserted}");
    assert!(s.doc.assembly.joints.iter().any(|j| j.name == "Hinge"));
    // component.list says which components the link keeps for its design.
    let list = run(&mut s, "component.list", json!({}));
    let link_of = |name: &str| list["components"].as_array().unwrap().iter().find(|c| c["name"] == name).map(|c| c["link"].clone()).unwrap();
    assert_eq!((link_of("base"), link_of("arm")), (json!(top), json!(top)), "{list}");
    assert!(link_of("hinge").is_null(), "the linked component itself is in component.links");
    // A joint of this design named as the design's keeps its name; the design's gets its
    // occurrence's in front.
    let mut other = Session::default();
    run(&mut other, "component.create", json!({"name": "A"}));
    run(&mut other, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [-60, 0, 0]}));
    run(&mut other, "component.activate", json!({"component": 0}));
    run(&mut other, "component.create", json!({"name": "B"}));
    run(&mut other, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [-80, 0, 0]}));
    run(&mut other, "component.activate", json!({"component": 0}));
    let origin = |part: &str| json!({"occurrence": part, "point": [-60, 0, 0], "z": [0, 0, 1]});
    run(&mut other, "joint.create", json!({"type": "revolute", "name": "Hinge", "flip": true, "a": origin("A"), "b": origin("B")}));
    run(&mut other, "component.insert_design", json!({"path": hinge, "flexible": true}));
    let names: Vec<&str> = other.doc.assembly.joints.iter().map(|j| j.name.as_str()).collect();
    assert_eq!(names, ["Hinge", "hinge:1: Hinge"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The default is as it was: without `structure`, a linked assembly is one component holding
/// all its bodies, and Update Links keeps it so. Relink Source with `structure` makes it a
/// subassembly or one component again, and leaves the link as it is when `structure` is left
/// out.
#[test]
fn a_link_keeps_the_assembly_in_one_component_unless_asked_and_relink_changes_that() {
    let dir = scratch("flat");
    let unit = save_unit(&dir);
    let unit_boxes = boxes(&open(&unit));
    let mut s = Session::default();
    // Untold: one component, as every design came in before links kept components.
    let inserted = run(&mut s, "component.insert_design", json!({"path": unit}));
    let top = inserted["component"].as_u64().unwrap();
    // One component holding the four bodies the unit shows, as before.
    assert_eq!(s.doc.components.len(), 1);
    assert!(inserted.get("subcomponents").is_none(), "{inserted}");
    assert_eq!(inserted["bodies"].as_array().map(Vec::len), Some(4), "{inserted}");
    assert!(s.doc.links.get(&top).is_some_and(|link| link.tree.is_none()));
    assert_eq!(boxes(&s), unit_boxes);
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({}))), rows(&[("UNIT-1", 1)]));
    // Update Links keeps it so.
    let mut source = open(&unit);
    run(&mut source, "component.insert_design", json!({"path": dir.join("pin.sc"), "reuse": true, "translate": [15, 5, 5]}));
    run(&mut source, "file.save", json!({}));
    assert_eq!(run(&mut s, "component.update_links", json!({}))["updated"], json!(["unit"]));
    assert_eq!(s.doc.components.len(), 1);
    assert_eq!(boxes(&s), boxes(&source));

    // Relink Source makes it a subassembly, and back.
    run(&mut s, "component.relink", json!({"component": top, "path": unit, "structure": true}));
    assert_eq!(s.doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["unit", "plate", "pin"]);
    assert_eq!(boxes(&s), boxes(&source));
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({}))), rows(&[("PL-1", 1), ("PN-1", 3), ("UNIT-1", 1)]));
    assert_eq!(s.model.state().bodies.len(), 3);
    run(&mut s, "component.relink", json!({"component": top, "path": unit}));
    assert_eq!(s.doc.components.len(), 3, "it stays as the link is");
    run(&mut s, "component.relink", json!({"component": top, "path": unit, "structure": false}));
    assert_eq!(s.doc.components.len(), 1);
    assert!(s.doc.occurrences.len() == 1 && s.doc.assembly.joints.is_empty() && s.doc.body_components.is_empty() && s.doc.body_offsets.is_empty());
    assert!(s.doc.links.get(&top).is_some_and(|link| link.tree.is_none()));
    assert_eq!(boxes(&s), boxes(&source));
    assert_eq!(s.model.state().bodies.len(), 5);
    assert!(s.execute("component.insert_design", &json!({"path": unit, "structure": "yes"})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link told to keep components holds to that while its design has none: the part comes in as
/// one component, and when its design gets a component of its own, Update Links brings that in
/// as a component inside it.
#[test]
fn a_part_design_links_as_one_component_and_becomes_a_subassembly_when_it_gets_components() {
    let dir = scratch("grows");
    let part = dir.join("bracket.sc");
    save_part(&part, [40.0, 20.0, 5.0], "Bracket", "BR-1");
    save_part(&dir.join("pin.sc"), [4.0, 6.0, 10.0], "Pin", "PN-1");
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": part, "structure": true}));
    let top = inserted["component"].as_u64().unwrap();
    assert_eq!(s.doc.components.len(), 1);
    assert!(inserted.get("subcomponents").is_none() && s.doc.assembly.joints.is_empty(), "{inserted}");
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({}))), rows(&[("BR-1", 1)]));
    // The bracket's design gets a pin of its own.
    let mut source = open(&part);
    run(&mut source, "component.insert_design", json!({"path": dir.join("pin.sc"), "translate": [5, 5, 5], "structure": true}));
    run(&mut source, "file.save", json!({}));
    assert_eq!(run(&mut s, "component.update_links", json!({}))["updated"], json!(["bracket"]));
    assert_eq!(s.doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["bracket", "pin"]);
    assert_eq!(boxes(&s), boxes(&source));
    assert_eq!(counts(&run(&mut s, "assembly.bom", json!({}))), rows(&[("BR-1", 1), ("PN-1", 1)]));
    assert_eq!(placements(&s, component(&s, "pin"), top).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The boxes of `boxes` that are `size` mm on every side.
fn cubes(boxes: &[[i64; 6]], size: i64) -> Vec<[i64; 6]> {
    boxes.iter().copied().filter(|b| (0..3).all(|i| b[i + 3] - b[i] == size * 1000)).collect()
}

/// A box (in micrometres) after a quarter turn about the vertical line through (`x`, `y`) mm,
/// from +X towards +Y: a point (px, py) goes to (x - (py - y), y + (px - x)).
fn quarter_turn(b: [i64; 6], x: i64, y: i64) -> [i64; 6] {
    let (x, y) = (x * 1000, y * 1000);
    [x - (b[4] - y), y + (b[0] - x), b[2], x - (b[1] - y), y + (b[3] - x), b[5]]
}

/// Two flexible occurrences of the hinge, both grounded: `hinge:1` at z 50, `hinge:2` 100 mm
/// along Y from it. Returns the session, the linked component and the two occurrences.
fn two_hinges(dir: &Path) -> (Session, u64, u64, u64) {
    let hinge = save_hinge(dir);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "translate": [0, 0, 50], "flexible": true}));
    let (top, first) = (inserted["component"].as_u64().unwrap(), inserted["occurrence"].as_u64().unwrap());
    let again = run(&mut s, "component.insert_design", json!({"path": hinge, "reuse": true, "translate": [0, 100, 50], "flexible": true}));
    let second = again["occurrence"].as_u64().unwrap();
    assert_eq!(again["joints"], 1, "{again}");
    run(&mut s, "occurrence.ground", json!({"occurrence": second, "grounded": true}));
    (s, top, first, second)
}

/// A side on a part's own origin plane is on the copy it was given on (`path`), and stays on
/// that part whatever pose the copy is in: a block joined to the arm of the second hinge turns
/// with that arm, about that hinge's axis through (0, 110), and not with the first hinge's.
#[test]
fn a_datum_plane_of_a_part_in_a_flexible_occurrence_goes_with_that_copy() {
    let dir = scratch("datum-copy");
    let (mut s, top, first, second) = two_hinges(&dir);
    let arm = placements(&s, component(&s, "arm"), top)[0];
    let (_, hinge_one) = joint_of(&s, top, first);
    let (_, hinge_two) = joint_of(&s, top, second);
    run(&mut s, "component.create", json!({"name": "Block", "activate": true}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "On arm", "flip": true,
               "a": {"occurrence": arm, "path": [second, arm], "plane": "XY"},
               "b": {"occurrence": "Block", "plane": "XY"}}),
    );
    let solved = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
    assert!(solved.errors.is_empty() && solved.conflicts.is_empty(), "{:?} {:?}", solved.errors, solved.conflicts);
    // Stored on the second hinge's own copy of the arm.
    let side = &s.doc.assembly.joints.iter().find(|j| j.name == "On arm").unwrap().a;
    assert_eq!((side.occurrence, side.inside.clone()), (second, vec![arm]));
    let at_rest = cubes(&boxes(&s), 10);
    assert_eq!(at_rest.len(), 1, "one block: {:?}", boxes(&s));
    // With a corner at the origin of the second hinge's arm, (0, 108, 55), and a face on its
    // XY plane: not at the first hinge's arm, at (0, 8, 55).
    let b = at_rest[0];
    assert!(b[0] >= -10_000 && b[3] <= 10_000 && b[1] >= 98_000 && b[4] <= 118_000 && (b[2] == 55_000 || b[5] == 55_000), "{at_rest:?}");
    // The first hinge's joint does not move it.
    run(&mut s, "joint.drive", json!({"joint": hinge_one, "value": 90}));
    assert_eq!(cubes(&boxes(&s), 10), at_rest);
    // The second hinge's joint turns it with the arm, a quarter turn about (0, 110).
    run(&mut s, "joint.drive", json!({"joint": hinge_two, "value": 90}));
    assert_eq!(cubes(&boxes(&s), 10), [quarter_turn(at_rest[0], 0, 110)]);
    // And back, and again: the side is the same on the part in every pose.
    run(&mut s, "joint.drive", json!({"joint": hinge_two, "value": 0}));
    assert_eq!(cubes(&boxes(&s), 10), at_rest);
    run(&mut s, "joint.drive", json!({"joint": hinge_two, "value": 90}));
    assert_eq!(cubes(&boxes(&s), 10), [quarter_turn(at_rest[0], 0, 110)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `joint.reach` moves the copy named by `path`: the arm of the second hinge, whose centre goes
/// from (15, 110, 57) a sixth of a turn about (0, 110). Without a path the occurrence is the
/// first hinge's, about (0, 10).
#[test]
fn reach_moves_the_copy_of_an_occurrence_that_its_path_names() {
    let dir = scratch("reach-copy");
    let (mut s, top, first, second) = two_hinges(&dir);
    let arm = placements(&s, component(&s, "arm"), top)[0];
    let (one, _) = joint_of(&s, top, first);
    let (two, _) = joint_of(&s, top, second);
    let angle = |s: &Session, joint: u64| s.doc.assembly.joints.iter().find(|j| j.id == joint).unwrap().values[0].to_degrees().round() as i64;
    // 15 mm from the axis at 60 degrees: 7.5 along X and 15 sin 60 = 12.990381 along Y.
    let reached = run(&mut s, "joint.reach", json!({"occurrence": arm, "path": [second, arm], "point": [7.5, 122.990_381_056_766_58, 57]}));
    assert_eq!(reached["reached"], true, "{reached}");
    assert_eq!((angle(&s, one), angle(&s, two)), (0, 60));
    let reached = run(&mut s, "joint.reach", json!({"occurrence": arm, "point": [7.5, -2.990_381_056_766_58, 57]}));
    assert_eq!(reached["reached"], true, "{reached}");
    assert_eq!((angle(&s, one), angle(&s, two)), (-60, 60));
    // The base is held by its hinge's occurrence, which is grounded: nothing to move.
    let base = placements(&s, component(&s, "base"), top)[0];
    let held = run(&mut s, "joint.reach", json!({"occurrence": base, "path": [second, base], "point": [0, 0, 0]}));
    assert_eq!(held["reached"], false, "{held}");
    assert!(held["reason"].as_str().is_some_and(|why| why.contains("fixed in place")), "{held}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A subassembly placed twice weighs twice what it holds: `assembly.mass_properties` lists
/// every placement of the parts inside it, each with the bodies it has there.
#[test]
fn a_subassembly_weighs_what_it_holds_in_each_of_its_occurrences() {
    let dir = scratch("mass");
    let unit = save_unit(&dir);
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": unit, "structure": true}));
    run(&mut s, "component.insert_design", json!({"path": unit, "reuse": true, "translate": [0, 0, 100]}));
    // Untold, the parts inside are listed at the first placement only, and a warning says so.
    let first = run(&mut s, "assembly.mass_properties", json!({}));
    let said = |reply: &Value| {
        reply["warnings"].as_array().is_some_and(|all| all.iter().any(|w| w.as_str().is_some_and(|w| w.contains("first placement only"))))
    };
    assert!(said(&first), "{first}");
    let mass = run(&mut s, "assembly.mass_properties", json!({"every_placement": true}));
    assert!(first["occurrences"].as_array().unwrap().len() < mass["occurrences"].as_array().unwrap().len(), "{first}");
    assert!(s.execute("assembly.mass_properties", &json!({"every_placement": "yes"})).is_err());
    let total: f64 = mass["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|o| o["bodies"].as_array().unwrap().iter())
        .map(|b| b["volume_mm3"].as_f64().unwrap_or_else(|| panic!("{b} in {mass}")))
        .sum();
    assert!((total - 9280.0).abs() < 1e-6, "two units of 4640 mm3 each: {total}");
    // Each placement is listed by its path: the plate in the first unit and in the second.
    let plate = component(&s, "plate");
    let paths: Vec<usize> =
        mass["occurrences"].as_array().unwrap().iter().filter(|o| o["component"] == plate).map(|o| o["path"].as_array().unwrap().len()).collect();
    assert_eq!(paths, [2, 2], "{mass}");
    assert!(!said(&mass), "{mass}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link that kept its design in one component and now keeps its components: a joint that was
/// on a face of the linked component's bodies stays where it is and still moves the component,
/// and the reply names it, since it no longer follows the face.
#[test]
fn a_joint_stays_where_it_is_when_its_link_starts_to_keep_components() {
    let dir = scratch("keep-joints");
    let unit = save_unit(&dir);
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base", "activate": true}));
    run(&mut s, "solid.box", json!({"length": 50, "width": 50, "height": 10, "corner": [-200, 0, 0], "body_name": "Base block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.insert_design", json!({"path": unit, "translate": [0, 0, 100], "structure": false}));
    // From the base's top face up to the plate's top face, at (20, 10, 105) in the world.
    let joint = json!({"type": "slider", "name": "Lift", "flip": true,
                       "a": {"occurrence": "Base", "point": [-175, 25, 10], "z": [0, 0, 1]},
                       "b": {"occurrence": "unit", "face": [20, 10, 105]}});
    run(&mut s, "joint.create", joint);
    // The joint has brought the unit to the base. The base block is the one box it is not.
    let base_block = [-200_000, 0, 0, -150_000, 50_000, 10_000];
    let unit_of = |all: Vec<[i64; 6]>| all.into_iter().filter(|b| *b != base_block).collect::<Vec<_>>();
    let before = boxes(&s);
    assert_eq!(unit_of(before.clone()).len(), 4, "{before:?}");
    let kept = run(&mut s, "component.link_mode", json!({"component": "unit", "structure": true}));
    assert_eq!(kept["subcomponents"], 2, "{kept}");
    let warnings = kept["warnings"].to_string();
    assert!(warnings.contains("joint Lift") && warnings.contains("stay where they are"), "{warnings}");
    assert!(!warnings.contains("joint Lift:"), "it still solves: {warnings}");
    assert_eq!(boxes(&s), before, "nothing moved");
    // It still lifts the whole unit.
    let driven = run(&mut s, "joint.drive", json!({"joint": "Lift", "value": 10}));
    assert_eq!(driven["moved"], true, "{driven}");
    let (moved, unit_before) = (unit_of(boxes(&s)), unit_of(before));
    let rise = moved[0][2] - unit_before[0][2];
    assert!(rise.abs() == 10_000, "the unit moved 10 mm along the joint: {rise}");
    assert_eq!(moved, shifted(&unit_before, [0, 0, rise / 1000]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Nothing of this design goes into a component a link keeps, whichever command would put it
/// there: the linked design decides what its components are and hold.
#[test]
fn no_command_puts_this_designs_work_into_a_component_a_link_keeps() {
    let dir = scratch("kept-guard");
    let unit = save_unit(&dir);
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": unit, "structure": true}));
    let before = serde_json::to_string(&*s.doc).unwrap();
    let refused = |s: &mut Session, id: &str, p: Value| {
        let e = s.execute(id, &p).expect_err(&format!("{id} {p}")).to_string();
        assert!(e.contains("linked subassembly unit"), "{id}: {e}");
    };
    refused(&mut s, "component.rename", json!({"component": "plate", "name": "Plate 2"}));
    refused(&mut s, "component.metadata.set", json!({"component": "plate", "description": "changed here"}));
    // A sketch on a face of the plate that would activate the plate first: its features would
    // be made in the plate.
    refused(&mut s, "sketch.create", json!({"plane": {"face": [20, 10, 5]}, "activate_part": true}));
    assert_eq!(serde_json::to_string(&*s.doc).unwrap(), before, "the design is as it was");
    assert_eq!(s.active_component, 0);
    // The same sketch made in the design itself is this design's own, on the face.
    let features = s.doc.features.len();
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 10, 5]}}));
    assert!(s.doc.features.len() == features + 1 && s.doc.features.last().is_some_and(|f| f.component == 0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A linked design that has a flexible hinge inside it, saved with the arm a quarter turn up,
/// shows that way where it is linked: the link keeps the pose the design has for it, and a STEP
/// assembly has it too.
#[test]
fn a_flexible_occurrence_inside_a_linked_design_shows_in_the_pose_that_design_has() {
    let dir = scratch("inherited-pose");
    let hinge = save_hinge(&dir);
    let mid = dir.join("mid.sc");
    let mut m = Session::default();
    run(&mut m, "component.insert_design", json!({"path": hinge, "flexible": true}));
    run(&mut m, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    let in_mid = boxes(&m);
    assert_eq!(in_mid, mm(&[[-2, 10, 5, 2, 40, 9], [0, 0, 0, 40, 20, 5]]));
    run(&mut m, "file.save", json!({"path": mid}));
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": mid, "translate": [0, 0, 50], "structure": true}));
    run(&mut s, "component.insert_design", json!({"path": mid, "reuse": true, "translate": [0, 200, 50]}));
    let expected = together(shifted(&in_mid, [0, 0, 50]), shifted(&in_mid, [0, 200, 50]));
    assert_eq!(boxes(&s), expected, "each occurrence shows the hinge as the linked design has it");
    assert_eq!(s.doc.assembly.poses.len(), 1, "the one pose, for the occurrence kept for the hinge: {:?}", s.doc.assembly.poses);
    assert!(s.doc.assembly.joints.is_empty(), "rigid here: no joint of the linked design is carried");
    // Saved and opened again, and exported.
    let top = dir.join("top.sc");
    run(&mut s, "file.save", json!({"path": top}));
    let mut back = open(&top);
    assert_eq!(boxes(&back), expected);
    let file = dir.join("top.step");
    run(&mut back, "file.export", json!({"path": file}));
    let exported = solidcraft_kernel::step_import(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let mut read: Vec<[i64; 6]> = exported
        .bodies
        .iter()
        .map(|b| {
            let x = b.body.exact_bounds().unwrap();
            [x.min.x, x.min.y, x.min.z, x.max.x, x.max.y, x.max.z].map(|v| (v * 1e3).round() as i64)
        })
        .collect();
    read.sort_unstable();
    assert_eq!(read, expected, "the STEP assembly has the hinge in that pose in both occurrences");
    // The linked design saved with the arm back down: Update Links takes the pose back.
    run(&mut m, "joint.drive", json!({"joint": "Hinge", "value": 0}));
    run(&mut m, "file.save", json!({"path": mid}));
    run(&mut back, "component.update_links", json!({}));
    let flat = mm(&[[0, 0, 0, 40, 20, 5], [0, 8, 5, 30, 12, 9]]);
    assert_eq!(boxes(&back), together(shifted(&flat, [0, 0, 50]), shifted(&flat, [0, 200, 50])));
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a design file says its links keep is not trusted: a record that names a joint of this
/// design's own, an occurrence that is no part of the link, or a pose for occurrences that are
/// not a subassembly's, is dropped when the design opens, and no command then acts on it.
#[test]
fn a_design_with_damaged_link_records_opens_without_them() {
    let dir = scratch("damaged-records");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base", "activate": true}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-100, 0, 0], "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "flexible": true}));
    let (top, first) = (inserted["component"].as_u64().unwrap(), inserted["occurrence"].as_u64().unwrap());
    let origin = |part: &str, at: [i64; 3]| json!({"occurrence": part, "point": at, "z": [0, 0, 1]});
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "name": "Mine", "flip": true, "a": origin("Base", [-95, 5, 10]), "b": origin("hinge", [0, 0, 0])}),
    );
    let mine = s.doc.assembly.joints.iter().find(|j| j.name == "Mine").unwrap().id;
    let base_occurrence = s.doc.occurrences.iter().find(|o| o.name == "Base:1").unwrap().id;
    let arm = placements(&s, component(&s, "arm"), top)[0];
    let mut design = serde_json::to_value(&*s.doc).unwrap();
    let tree = &mut design["links"][top.to_string()]["tree"];
    // The record claims this design's own joint, an occurrence outside the link, and a
    // flexible occurrence that is none of the linked component's.
    tree["flexible"][first.to_string()]["999"] = json!(mine);
    tree["flexible"][base_occurrence.to_string()] = json!({"1": mine});
    tree["occurrences"]["998"] = json!(base_occurrence);
    tree["fixed"] = json!([base_occurrence]);
    design["assembly"]["poses"] = json!([{"occurrence": base_occurrence, "inner": arm, "transform": s.doc.occurrences[0].transform},
                                         {"occurrence": first, "inner": base_occurrence, "transform": s.doc.occurrences[0].transform}]);
    let file = dir.join("damaged.json");
    std::fs::write(&file, serde_json::to_string(&design).unwrap()).unwrap();
    let mut back = open(&file);
    let tree = back.doc.links[&top].tree.clone().unwrap();
    assert!(tree.flexible.keys().eq([first].iter()), "{:?}", tree.flexible);
    assert!(tree.flexible[&first].values().all(|joint| *joint != mine), "{:?}", tree.flexible);
    assert!(tree.occurrences.values().all(|o| *o != base_occurrence) && tree.fixed.iter().all(|o| *o != base_occurrence));
    // The poses the file had are gone; the one the joint gives the arm's copy is this design's.
    assert!(back.doc.assembly.poses.iter().all(|p| p.occurrence == first && p.inner == arm), "{:?}", back.doc.assembly.poses);
    // Made rigid, the occurrence gives up the linked design's joint, and this design's stays.
    run(&mut back, "component.link_mode", json!({"component": "hinge", "flexible": false}));
    assert_eq!(back.doc.assembly.joints.iter().map(|j| j.name.as_str()).collect::<Vec<_>>(), ["Mine"]);
    run(&mut back, "joint.delete", json!({"joint": "Mine"}));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A linked design without joints has nothing to make an occurrence flexible with: every
/// occurrence stays rigid, the reply says so, and no record of a flexible occurrence is kept.
#[test]
fn a_design_without_joints_stays_rigid_when_asked_for_flexible() {
    let dir = scratch("no-joints");
    let unit = save_unit(&dir);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": unit, "flexible": true}));
    let top = inserted["component"].as_u64().unwrap();
    assert!(inserted.get("joints").is_none(), "{inserted}");
    assert!(s.doc.links[&top].tree.as_ref().unwrap().flexible.is_empty());
    let mode = run(&mut s, "component.link_mode", json!({"component": "unit", "flexible": true}));
    assert!(mode["flexible"].as_array().unwrap().is_empty() && mode["joints"] == 0, "{mode}");
    assert!(mode["warnings"].to_string().contains("no joints"), "{mode}");
    // A part, which has no components either.
    let part = run(&mut s, "component.insert_design", json!({"path": dir.join("plate.sc"), "flexible": true}));
    let part = part["component"].as_u64().unwrap();
    assert!(s.doc.links[&part].tree.as_ref().is_some_and(|tree| tree.flexible.is_empty() && tree.components.is_empty()));
    assert!(s.doc.assembly.poses.is_empty() && s.doc.assembly.joints.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two components inside one occurrence of a linked subassembly can be joined here only where
/// the linked design's joints move one against the other: not in a rigid occurrence, which
/// moves as one, and not two that those joints leave together.
#[test]
fn two_components_of_one_occurrence_are_joined_only_where_its_joints_move_them() {
    let dir = scratch("inner-sides");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "structure": true}));
    let (top, first) = (inserted["component"].as_u64().unwrap(), inserted["occurrence"].as_u64().unwrap());
    let (base, arm) = (placements(&s, component(&s, "base"), top)[0], placements(&s, component(&s, "arm"), top)[0]);
    let side = |part: u64, at: [i64; 3]| json!({"occurrence": part, "point": at, "z": [0, 0, 1]});
    let joint = json!({"type": "revolute", "name": "Mine", "flip": true, "a": side(base, [0, 10, 7]), "b": side(arm, [0, 10, 7])});
    let refused = s.execute("joint.create", &joint).unwrap_err().to_string();
    assert!(refused.contains("rigid occurrence"), "{refused}");
    run(&mut s, "component.link_mode", json!({"component": "hinge", "occurrence": first, "flexible": true}));
    // The arm turns against the base: a joint of this design between them is one more on the
    // same axis, and the two agree.
    run(&mut s, "joint.create", joint);
    let solved = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
    assert!(solved.errors.is_empty() && solved.conflicts.is_empty(), "{:?} {:?}", solved.errors, solved.conflicts);
    run(&mut s, "joint.delete", json!({"joint": "Mine"}));
    // Two origins on the base, which the hinge's joint does not move.
    let both = json!({"type": "rigid", "name": "Both", "a": side(base, [0, 0, 5]), "b": side(base, [40, 20, 5])});
    let refused = s.execute("joint.create", &both).unwrap_err().to_string();
    assert!(refused.contains("do not move"), "{refused}");
    // A motion link between two of the occurrence's own joints is the linked design's to make.
    let (own, _) = joint_of(&s, top, first);
    let refused = s.execute("joint.motion_link", &json!({"a": own, "b": own, "ratio": 1})).unwrap_err().to_string();
    assert!(refused.contains("two different joints"), "{refused}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// One occurrence relinked to another design: a joint of this design to a component inside it
/// was on a component the old link kept. It stays where it is, as a frame on the occurrence,
/// and the reply names it.
#[test]
fn relinking_one_occurrence_keeps_a_joint_to_a_component_inside_it_in_place() {
    let dir = scratch("relink-inside");
    let (mut s, top, _, second) = two_hinges(&dir);
    let other = dir.join("hinge2.sc");
    std::fs::copy(dir.join("hinge.sc"), &other).unwrap();
    let arm = placements(&s, component(&s, "arm"), top)[0];
    run(&mut s, "component.create", json!({"name": "Block", "activate": true}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "On arm", "flip": true,
               "a": {"occurrence": arm, "path": [second, arm], "point": [0, 108, 55], "z": [0, 0, 1]},
               "b": {"occurrence": "Block", "point": [0, 0, 0], "z": [0, 0, 1]}}),
    );
    let before = cubes(&boxes(&s), 10);
    assert_eq!(before.len(), 1, "{:?}", boxes(&s));
    let relinked = run(&mut s, "component.relink", json!({"component": "hinge", "occurrence": second, "path": other}));
    assert!(relinked["warnings"].to_string().contains("stays where it is"), "{relinked}");
    assert!(relinked["joint_errors"].as_array().is_none_or(Vec::is_empty), "{relinked}");
    let side = &s.doc.assembly.joints.iter().find(|j| j.name == "On arm").unwrap().a;
    assert_eq!((side.occurrence, side.inside.len()), (second, 0), "on the occurrence itself now");
    assert_eq!(cubes(&boxes(&s), 10), before, "the block stays where it was");
    // The occurrence came rigid: the joint and the pose it had for itself were its old design's.
    assert!(s.doc.assembly.poses.iter().all(|p| p.occurrence != second));
    assert_eq!(s.doc.assembly.joints.iter().filter(|j| j.a.occurrence == second && j.b.occurrence == second).count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `unit.sc`: two hinges, `hinge:1` at the origin and `hinge:2` 100 mm along Y, both flexible
/// and both held still by the unit's design (grounded), saved with their arms at 0.
fn save_unit_of_hinges(dir: &Path) -> PathBuf {
    let hinge = save_hinge(dir);
    let unit = dir.join("unit2.sc");
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": hinge, "flexible": true}));
    let second = run(&mut s, "component.insert_design", json!({"path": hinge, "reuse": true, "translate": [0, 100, 0], "flexible": true}));
    run(&mut s, "occurrence.ground", json!({"occurrence": second["occurrence"], "grounded": true}));
    run(&mut s, "file.save", json!({"path": unit}));
    unit
}

/// The boxes of a unit of two hinges whose arms stand at `first` and `second` quarter turns
/// (1 up, -1 down, 0 as saved), moved by `at` mm: the base (0, 0, 0)..(40, 20, 5) and the arm
/// (0, 8, 5)..(30, 12, 9), which a quarter turn about Z through (0, 10) puts at
/// (-2, 10, 5)..(2, 40, 9) and a quarter turn back at (-2, -20, 5)..(2, 10, 9); the second hinge
/// 100 mm along Y.
fn unit_boxes(first: i64, second: i64, at: [i64; 3]) -> Vec<[i64; 6]> {
    let arm = |turn: i64| match turn {
        1 => [-2, 10, 5, 2, 40, 9],
        -1 => [-2, -20, 5, 2, 10, 9],
        _ => [0, 8, 5, 30, 12, 9],
    };
    let base = [0, 0, 0, 40, 20, 5];
    let one = shifted(&mm(&[base, arm(first)]), at);
    let two = shifted(&mm(&[base, arm(second)]), [at[0], at[1] + 100, at[2]]);
    together(one, two)
}

/// A flexible occurrence has the joints of the flexible occurrences inside its linked design
/// too (#1516): a machine with two units of two hinges each drives all four hinges, each to
/// its own angle, and a rigid unit shows the unit as it was saved.
#[test]
fn a_flexible_occurrence_carries_the_joints_of_the_flexible_occurrences_inside_it() {
    let dir = scratch("cascade");
    let unit = save_unit_of_hinges(&dir);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": unit, "flexible": true}));
    assert_eq!(inserted["joints"], 2, "the unit has a joint for each of its hinges: {inserted}");
    let top = inserted["component"].as_u64().unwrap();
    let again = run(&mut s, "component.insert_design", json!({"path": unit, "reuse": true, "translate": [0, 0, 100], "flexible": true}));
    let second = again["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.ground", json!({"occurrence": second, "grounded": true}));
    let names: Vec<String> = s.doc.assembly.joints.iter().map(|j| j.name.clone()).collect();
    assert_eq!(names, ["Hinge", "hinge:2: Hinge", "unit2:2: Hinge", "unit2:2: hinge:2: Hinge"]);
    // As saved: every arm at 0.
    assert_eq!(boxes(&s), together(unit_boxes(0, 0, [0, 0, 0]), unit_boxes(0, 0, [0, 0, 100])));
    // Each of the four hinges to its own angle.
    for (joint, value) in [("Hinge", 90), ("hinge:2: Hinge", -90), ("unit2:2: Hinge", -90), ("unit2:2: hinge:2: Hinge", 90)] {
        let driven = run(&mut s, "joint.drive", json!({"joint": joint, "value": value}));
        assert_eq!(driven["moved"], true, "{joint}: {driven}");
    }
    let posed = together(unit_boxes(1, -1, [0, 0, 0]), unit_boxes(-1, 1, [0, 0, 100]));
    assert_eq!(boxes(&s), posed);
    // The two arms of one unit are copies of one inner occurrence, each under its own hinge.
    // The machine's own four poses, and the two the unit was saved with (kept for a rigid unit).
    let under = |n: usize| s.doc.assembly.poses.iter().filter(|p| p.through.len() == n).count();
    assert_eq!((under(1), under(0)), (4, 2), "{:?}", s.doc.assembly.poses);
    // Saved, opened again and written as a STEP assembly, it stands the same.
    let machine = dir.join("machine.sc");
    run(&mut s, "file.save", json!({"path": machine}));
    let mut back = open(&machine);
    assert_eq!(boxes(&back), posed);
    let file = dir.join("machine.step");
    run(&mut back, "file.export", json!({"path": file}));
    let exported = solidcraft_kernel::step_import(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let mut read: Vec<[i64; 6]> = exported
        .bodies
        .iter()
        .map(|b| {
            let x = b.body.exact_bounds().unwrap();
            [x.min.x, x.min.y, x.min.z, x.max.x, x.max.y, x.max.z].map(|v| (v * 1e3).round() as i64)
        })
        .collect();
    read.sort_unstable();
    assert_eq!(read, posed, "the STEP assembly has each of the four hinges in its own pose");
    // The second unit made rigid shows the unit as its design was saved; the first keeps its pose.
    let rigid = run(&mut back, "component.link_mode", json!({"component": top, "occurrence": second, "flexible": false}));
    assert_eq!(rigid["joints"], 2, "{rigid}");
    assert_eq!(boxes(&back), together(unit_boxes(1, -1, [0, 0, 0]), unit_boxes(0, 0, [0, 0, 100])));
    assert_eq!(back.doc.assembly.joints.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Three levels: a top design links the machine above, which was saved with its four hinges in
/// four poses. Rigid, it shows them so. Made flexible, it has all four joints, and the deepest
/// hinge moves from the top while the others stay.
#[test]
fn joints_three_levels_down_move_from_the_top_when_every_level_is_flexible() {
    let dir = scratch("cascade-3");
    let unit = save_unit_of_hinges(&dir);
    let machine = dir.join("machine.sc");
    let mut m = Session::default();
    run(&mut m, "component.insert_design", json!({"path": unit, "flexible": true}));
    let again = run(&mut m, "component.insert_design", json!({"path": unit, "reuse": true, "translate": [0, 0, 100], "flexible": true}));
    run(&mut m, "occurrence.ground", json!({"occurrence": again["occurrence"], "grounded": true}));
    for (joint, value) in [("Hinge", 90), ("hinge:2: Hinge", -90), ("unit2:2: Hinge", -90), ("unit2:2: hinge:2: Hinge", 90)] {
        run(&mut m, "joint.drive", json!({"joint": joint, "value": value}));
    }
    run(&mut m, "file.save", json!({"path": machine}));
    let in_machine = together(unit_boxes(1, -1, [0, 0, 0]), unit_boxes(-1, 1, [0, 0, 100]));
    assert_eq!(boxes(&m), in_machine);

    // Rigid in the top: the machine as it was saved, 300 mm along X.
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": machine, "translate": [300, 0, 0], "structure": true}));
    assert!(s.doc.assembly.joints.is_empty(), "rigid: no joint of the machine is carried");
    assert_eq!(boxes(&s), shifted(&in_machine, [300, 0, 0]));
    // Flexible: the joints the machine has for its units' hinges are joints here.
    let flexible = run(&mut s, "component.link_mode", json!({"component": inserted["component"], "flexible": true}));
    assert_eq!(flexible["joints"], 4, "{flexible}");
    assert_eq!(boxes(&s), shifted(&in_machine, [300, 0, 0]), "made flexible, it stands as it did");
    // The deepest hinge of the second unit, from the top: back to 0. The other three stay.
    let deepest = s.doc.assembly.joints.iter().find(|j| j.name == "unit2:2: hinge:2: Hinge").map(|j| j.id).unwrap();
    let driven = run(&mut s, "joint.drive", json!({"joint": deepest, "value": 0}));
    assert_eq!(driven["moved"], true, "{driven}");
    let expected = together(unit_boxes(1, -1, [300, 0, 0]), unit_boxes(-1, 0, [300, 0, 100]));
    assert_eq!(boxes(&s), expected);
    // Its pose names both flexible occurrences it is under: the unit, and the hinge in it.
    assert!(s.doc.assembly.poses.iter().any(|p| p.through.len() == 2), "{:?}", s.doc.assembly.poses);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A rigid level stops it: a unit whose hinges are rigid in it has no joints to give, so a
/// machine cannot make that unit flexible, and says so.
#[test]
fn a_rigid_occurrence_on_the_way_keeps_what_is_below_it_as_saved() {
    let dir = scratch("cascade-rigid");
    let hinge = save_hinge(&dir);
    let unit = dir.join("unit-rigid.sc");
    let mut u = Session::default();
    run(&mut u, "component.insert_design", json!({"path": hinge, "structure": true}));
    run(&mut u, "file.save", json!({"path": unit}));
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": unit, "flexible": true}));
    assert!(inserted.get("joints").is_none(), "{inserted}");
    assert!(inserted["warnings"].to_string().contains("no joints to carry"), "{inserted}");
    assert!(s.doc.assembly.joints.is_empty() && s.doc.assembly.poses.is_empty());
    assert_eq!(boxes(&s), mm(&[[0, 0, 0, 40, 20, 5], [0, 8, 5, 30, 12, 9]]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A dialog previews a motion study on a copy of the design and shows the result without
/// changing the design. An occurrence that moved is shown by a pending move; the arm of a
/// flexible hinge is no occurrence of its own, so the copy's poses are shown in place of the
/// design's (`Session::pending_poses`). The arm turns in the world, the design keeps its poses
/// and its joint's value, a copy of the session shows the design, and the world is the design's
/// again when the preview ends.
#[test]
fn a_previewed_study_shows_a_flexible_occurrences_parts_without_changing_the_design() {
    let dir = scratch("preview-poses");
    let (mut s, top, first, _) = two_hinges(&dir);
    let (hinge, name) = joint_of(&s, top, first);
    let saved = mm(&[[0, 0, 0, 40, 20, 5], [0, 8, 5, 30, 12, 9]]);
    let up = mm(&[[0, 0, 0, 40, 20, 5], [-2, 10, 5, 2, 40, 9]]);
    let at_rest = together(shifted(&saved, [0, 0, 50]), shifted(&saved, [0, 100, 50]));
    let turned = together(shifted(&up, [0, 0, 50]), shifted(&saved, [0, 100, 50]));
    assert_eq!(boxes(&s), at_rest);
    let value = |s: &Session| s.doc.assembly.joints.iter().find(|j| j.id == hinge).map(|j| j.values.clone());
    let (design, poses, stored) = (s.doc.clone(), s.doc.assembly.poses.clone(), value(&s));

    // As the Motion Study dialog does: the study is made and played on a copy.
    let mut copy = s.scratch();
    let keys = json!([{"joint": name, "points": [[0, 0], [10, 90]]}]);
    let study = run(&mut copy, "motion.study", json!({"name": "Swing", "steps": 10, "keys": keys}))["study"].clone();
    run(&mut copy, "motion.play", json!({"study": study, "step": 10}));
    assert_eq!(boxes(&copy), turned, "the copy shows the study's last step");
    // No occurrence moved, so no pending move can show it: only the arm's pose changed.
    assert_eq!(copy.doc.occurrences, s.doc.occurrences);
    assert_ne!(copy.doc.assembly.poses, poses);

    // Shown on the design: the arm turns, and the design is what it was.
    s.pending_poses = Some(copy.doc.assembly.poses.clone());
    assert_eq!(boxes(&s), turned);
    assert!(std::sync::Arc::ptr_eq(&s.doc, &design), "the design is the same document");
    assert_eq!((&s.doc.assembly.poses, value(&s)), (&poses, stored));
    assert!(s.pending_moves.is_empty() && !s.moved_uncaptured(), "a shown pose is no move to capture");
    // A copy of the session solves and shows the design, not the preview.
    assert_eq!(boxes(&s.scratch()), at_rest);

    // The preview ends: the world is the design's again.
    s.pending_poses = None;
    assert_eq!(boxes(&s), at_rest);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A feature reads every part where the design has it, and the world shows the parts of a
/// flexible occurrence where its joints stand. A reference picked on a part that stands in a
/// pose would be looked for where the part is not shown, so the command is refused and says how
/// to go on. A part that stands as its design has it is picked as before: in a rigid occurrence,
/// in a flexible one whose joint does not move it, and once the occurrence is rigid again.
#[test]
fn a_reference_picked_on_a_part_in_a_pose_is_refused_and_one_on_a_part_as_designed_is_kept() {
    let dir = scratch("posed-picks");
    let hinge = save_hinge(&dir);
    let saved = mm(&[[0, 0, 0, 40, 20, 5], [0, 8, 5, 30, 12, 9]]);
    let up = mm(&[[0, 0, 0, 40, 20, 5], [-2, 10, 5, 2, 40, 9]]);
    let mut s = Session::default();
    // The first hinge is flexible, its arm a quarter turn up; the second is rigid, as saved.
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "translate": [0, 0, 50], "structure": true}));
    let (top, first) = (inserted["component"].as_u64().unwrap(), inserted["occurrence"].as_u64().unwrap());
    run(&mut s, "component.link_mode", json!({"component": top, "flexible": true}));
    let driven = run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    assert_eq!(driven["moved"], true, "{driven}");
    run(&mut s, "component.insert_design", json!({"path": hinge, "reuse": true, "translate": [0, 100, 50]}));
    assert_eq!(boxes(&s), together(shifted(&up, [0, 0, 50]), shifted(&saved, [0, 100, 50])));

    // The first arm is shown at (-2, 10, 55)..(2, 40, 59): a top edge through (2, 25, 59), a
    // corner at (2, 40, 59), its top face around (0, 25, 59). Its design has it at
    // (0, 8, 55)..(30, 12, 59), where none of those picks is.
    let before = s.doc.clone();
    let refused =
        |s: &mut Session, id: &str, p: Value| s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{id} {p} went through"));
    let told = |command: &str, why: &str| {
        assert!(why.contains("inside a flexible occurrence of a linked subassembly"), "{command}: {why}");
        assert!(why.contains("does not stand as its design has it") && why.contains("component.link_mode"), "{command}: {why}");
    };
    for (command, params) in [
        ("construct.axis.edge", json!({"edge": {"edge": [2, 25, 59]}})),
        ("construct.point.vertex", json!({"point": {"vertex": [2, 40, 59]}})),
        ("construct.plane.offset", json!({"base": "XY", "through": {"vertex": [2, 40, 59]}})),
        ("construct.plane.offset", json!({"base": {"face": [0, 25, 59]}, "offset": 5})),
    ] {
        told(command, &refused(&mut s, command, params));
    }
    assert_eq!(s.doc, before, "nothing was made");
    // A sketch of the design projects what the design has, too.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    told("sketch.project", &refused(&mut s, "sketch.project", json!({"refs": [{"edge": [2, 25, 59]}]})));
    run(&mut s, "sketch.finish", json!({}));

    let axis = |s: &Session, name: &str| {
        let state = s.model.state();
        match state.construct.iter().find(|c| c.name == name).map(|c| c.geom.clone()) {
            Some(solidcraft_doc::construct::ConstructGeom::Axis { origin, dir }) => (origin, dir),
            other => panic!("{name}: {other:?}"),
        }
    };
    // The second hinge is rigid: its arm is where its design has it, (0, 108, 55)..(30, 112, 59).
    run(&mut s, "construct.axis.edge", json!({"edge": {"edge": [15, 112, 59]}, "name": "On rigid arm"}));
    let (origin, along) = axis(&s, "On rigid arm");
    assert!((origin.y - 112.0).abs() < 1e-6 && (origin.z - 59.0).abs() < 1e-6 && along.x.abs() > 1.0 - 1e-9, "{origin:?} {along:?}");
    // The first hinge's base is in the flexible occurrence, and its joint does not move it.
    run(&mut s, "construct.axis.edge", json!({"edge": {"edge": [20, 0, 50]}, "name": "On base"}));
    let (origin, along) = axis(&s, "On base");
    assert!(origin.y.abs() < 1e-6 && (origin.z - 50.0).abs() < 1e-6 && along.x.abs() > 1.0 - 1e-9, "{origin:?} {along:?}");

    // Made rigid, as the message says, the first hinge stands as saved and its arm is picked.
    run(&mut s, "component.link_mode", json!({"component": top, "occurrence": first, "flexible": false}));
    assert_eq!(boxes(&s), together(shifted(&saved, [0, 0, 50]), shifted(&saved, [0, 100, 50])));
    run(&mut s, "construct.axis.edge", json!({"edge": {"edge": [15, 12, 59]}, "name": "On first arm"}));
    let (origin, along) = axis(&s, "On first arm");
    assert!((origin.y - 12.0).abs() < 1e-6 && (origin.z - 59.0).abs() < 1e-6 && along.x.abs() > 1.0 - 1e-9, "{origin:?} {along:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A flexible occurrence needs the design's components, so asking for one asks for them too,
/// and asking for one without them is refused.
#[test]
fn a_flexible_occurrence_is_asked_for_with_the_designs_components() {
    let dir = scratch("implied");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    let made = run(&mut s, "component.insert_design", json!({"path": hinge, "flexible": true}));
    assert_eq!((made["subcomponents"].clone(), made["joints"].clone()), (json!(2), json!(1)), "{made}");
    assert!(s.execute("component.insert_design", &json!({"path": hinge, "structure": false, "flexible": true})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A joint origin given by a point in the world, with no occurrence named, is on the part whose
/// body the point is on. A part of a linked subassembly is built where the linked design has it,
/// and its offset takes it into its component's frame: the hinge's arm is built as the box
/// (0, 8, 5)..(30, 12, 9), its component's frame is at (0, 8, 5) in the hinge, and the hinge is
/// placed at z 50. The point (15, 10, 57) is the middle of the arm as the world shows it,
/// (0, 8, 55)..(30, 12, 59), and 2 mm above the base, (0, 0, 50)..(40, 20, 55). Taken back
/// through the arm's placement alone it is (15, 2, 2): 6 along Y and 3 along Z from the box the
/// arm is built as, sqrt(45) = 6.7 mm, further than the base's 2 mm, so the base would be picked.
#[test]
fn a_point_picked_on_a_part_of_a_linked_subassembly_is_on_that_part() {
    let dir = scratch("pick-part");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "translate": [0, 0, 50], "structure": true}));
    let top = inserted["component"].as_u64().unwrap();
    let (base, arm) = (placements(&s, component(&s, "base"), top)[0], placements(&s, component(&s, "arm"), top)[0]);
    run(&mut s, "joint.origin", json!({"name": "On arm", "point": [15, 10, 57], "z": [0, 0, 1]}));
    let made = s.doc.assembly.origins[0].origin.clone();
    assert_eq!(made.occurrence, arm, "on the arm, not on the base ({base}) below it: {made:?}");
    // In the arm's own coordinates: the middle of the box (0, 0, 0)..(30, 4, 4).
    let solidcraft_doc::joints::Snap::Point { point, .. } = &made.snap else { panic!("{made:?}") };
    assert_eq!([point.x, point.y, point.z].map(|v| (v * 1e3).round() as i64), [15_000, 2_000, 2_000]);
    // A point in the base, 3 mm below the arm, is on the base as before.
    run(&mut s, "joint.origin", json!({"name": "On base", "point": [20, 10, 52], "z": [0, 0, 1]}));
    assert_eq!(s.doc.assembly.origins[1].origin.occurrence, base);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What asks where a placement is shown (`Session::occurrence_placement`: the origin drawn for
/// a component, the frame of a pick) follows a previewed study as the world does. What reports
/// the design's own placements does not. The arm's frame is at (0, 8, 5) in the hinge, which
/// stands at z 50: (0, 8, 55). A quarter turn about Z through (0, 10) takes a point (x, y) to
/// (0 - (y - 10), 10 + (x - 0)), so (0, 8) goes to (2, 10).
#[test]
fn a_previewed_study_is_where_a_shown_placement_is_asked_for() {
    let dir = scratch("preview-placement");
    let (mut s, top, first, _) = two_hinges(&dir);
    let arm = placements(&s, component(&s, "arm"), top)[0];
    let (_, name) = joint_of(&s, top, first);
    // A placement's origin in the world, in micrometres.
    let origin = |world: solidcraft_doc::Mat| [world[3][0], world[3][1], world[3][2]].map(|v| (v * 1e3).round() as i64);
    let shown = |s: &Session| origin(s.occurrence_placement(&[first, arm]).unwrap().world);
    let designed = |s: &Session| origin(s.occurrence_placement_with(&[first, arm], &s.uncaptured_moves()).unwrap().world);
    assert_eq!((shown(&s), designed(&s)), ([0, 8_000, 55_000], [0, 8_000, 55_000]));
    // As the Motion Study dialog does: the study is played on a copy, and its poses are shown.
    let mut copy = s.scratch();
    let keys = json!([{"joint": name, "points": [[0, 0], [10, 90]]}]);
    let study = run(&mut copy, "motion.study", json!({"name": "Swing", "steps": 10, "keys": keys}))["study"].clone();
    run(&mut copy, "motion.play", json!({"study": study, "step": 10}));
    s.pending_poses = Some(copy.doc.assembly.poses.clone());
    assert_eq!(shown(&s), [2_000, 10_000, 55_000], "where the preview shows the arm");
    assert_eq!(designed(&s), [0, 8_000, 55_000], "the design has it where it was");
    // The preview ends: it is shown where the design has it again.
    s.pending_poses = None;
    assert_eq!(shown(&s), [0, 8_000, 55_000]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Asked to keep the components of a linked design that has none, `component.link_mode` says
/// so: the menu offers Keep Its Components on every link that keeps none, since what a link
/// holds does not say whether its design has components. The link is set to keep them, which
/// shows nothing until the design has some. A design that has components is kept without a
/// word about it.
#[test]
fn a_link_asked_to_keep_components_says_when_its_design_has_none() {
    let dir = scratch("no-components");
    let part = dir.join("bracket.sc");
    save_part(&part, [40.0, 20.0, 5.0], "Bracket", "BR-1");
    let unit = save_unit(&dir);
    let mut s = Session::default();
    let bracket = run(&mut s, "component.insert_design", json!({"path": part}))["component"].as_u64().unwrap();
    let kept = run(&mut s, "component.link_mode", json!({"component": bracket, "structure": true}));
    assert_eq!((kept["structure"].clone(), kept["subcomponents"].clone()), (json!(true), json!(0)), "{kept}");
    assert!(kept["warnings"].to_string().contains("no components of its own"), "{kept}");
    assert!(s.doc.links.get(&bracket).is_some_and(|link| link.tree.is_some()), "set to keep them once it has some");
    let top = run(&mut s, "component.insert_design", json!({"path": unit, "translate": [0, 100, 0]}))["component"].as_u64().unwrap();
    let kept = run(&mut s, "component.link_mode", json!({"component": top, "structure": true}));
    assert_eq!(kept["subcomponents"], 2, "{kept}");
    assert!(!kept["warnings"].to_string().contains("no components"), "{kept}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// As `a_joint_stays_where_it_is_when_its_link_starts_to_keep_components`, for a part the
/// linked design places twice. Kept in one component, the link has a body for each placement;
/// keeping components, it has one body for the part, at its first placement. A joint on the top
/// face of the second pin stays on that pin, and does not go to the same face of the first: in
/// the unit the second pin is the box (24, 5, 5)..(30, 9, 15), its top face around (27, 7, 15),
/// and the first is (5, 5, 5)..(9, 11, 15), 20 mm away.
#[test]
fn a_joint_on_the_second_placement_of_a_part_stays_there_when_its_link_starts_to_keep_components() {
    let dir = scratch("keep-joints-second");
    let unit = save_unit(&dir);
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base", "activate": true}));
    run(&mut s, "solid.box", json!({"length": 50, "width": 50, "height": 10, "corner": [-200, 0, 0], "body_name": "Base block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.insert_design", json!({"path": unit, "translate": [0, 0, 100], "structure": false}));
    // From the base's top face up to the second pin's top face, at (27, 7, 115) in the world.
    let joint = json!({"type": "slider", "name": "Lift", "flip": true,
                       "a": {"occurrence": "Base", "point": [-175, 25, 10], "z": [0, 0, 1]},
                       "b": {"occurrence": "unit", "face": [27, 7, 115]}});
    run(&mut s, "joint.create", joint);
    // The joint has brought the unit to the base. The base block is the one box it is not.
    let base_block = [-200_000, 0, 0, -150_000, 50_000, 10_000];
    let unit_of = |all: Vec<[i64; 6]>| all.into_iter().filter(|b| *b != base_block).collect::<Vec<_>>();
    let before = boxes(&s);
    assert_eq!(unit_of(before.clone()).len(), 4, "{before:?}");
    let kept = run(&mut s, "component.link_mode", json!({"component": "unit", "structure": true}));
    assert_eq!(kept["subcomponents"], 2, "{kept}");
    let warnings = kept["warnings"].to_string();
    assert!(warnings.contains("joint Lift") && warnings.contains("stay where they are"), "{warnings}");
    assert_eq!(boxes(&s), before, "nothing moved: the joint is where the second pin's face is");
    // It still lifts the whole unit, 10 mm along the joint.
    let driven = run(&mut s, "joint.drive", json!({"joint": "Lift", "value": 10}));
    assert_eq!(driven["moved"], true, "{driven}");
    let (moved, unit_before) = (unit_of(boxes(&s)), unit_of(before));
    let rise = moved[0][2] - unit_before[0][2];
    assert!(rise.abs() == 10_000, "the unit moved 10 mm along the joint: {rise}");
    assert_eq!(moved, shifted(&unit_before, [0, 0, rise / 1000]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The linked design moves a part into another of its components (Move to Component there):
/// the same placement, in another component. Update Links keeps it the placement it is here,
/// in the component kept for that one, and a joint of this design made to it stays on it. The
/// unit's first pin goes into a new component `Cell`, which sits at the unit's origin, so
/// nothing moves: the unit stands 100 mm along X before and after, and the joint still lifts
/// it 10 mm.
#[test]
fn update_links_keeps_the_placement_and_the_joint_of_a_part_its_design_moves_into_another_component() {
    let dir = scratch("moved-in");
    let unit = save_unit(&dir);
    let unit_boxes = boxes(&open(&unit));
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-50, 0, 0], "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let inserted = run(&mut s, "component.insert_design", json!({"path": unit, "translate": [100, 0, 0], "structure": true}));
    let (top, unit_here) = (inserted["component"].as_u64().unwrap(), inserted["occurrence"].as_u64().unwrap());
    let pin = component(&s, "pin");
    let pins = placements(&s, pin, top);
    // A slider along Z between the grounded base and the first pin inside the unit.
    let origin = |occurrence: Value| json!({"occurrence": occurrence, "point": [0, 0, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "slider", "name": "Lift", "flip": true, "a": origin(json!("Base")), "b": origin(json!(pins[0]))}));
    let with_block = |unit: Vec<[i64; 6]>| together(unit, mm(&[[-50, 0, 0, -40, 10, 10]]));
    assert_eq!(boxes(&s), with_block(shifted(&unit_boxes, [100, 0, 0])), "joined where it sits, nothing moves");

    let mut source = open(&unit);
    run(&mut source, "component.create", json!({"name": "Cell", "activate": false}));
    run(&mut source, "occurrence.reparent", json!({"occurrence": "pin:1", "parent": "Cell"}));
    run(&mut source, "file.save", json!({}));
    assert_eq!(boxes(&source), unit_boxes, "the pin is where it was in the unit");
    let updated = run(&mut s, "component.update_links", json!({}));
    assert_eq!(updated["updated"], json!(["unit"]), "{updated}");
    assert!(!updated["warnings"].to_string().contains("Lift"), "{updated}");
    // The same two placements of the pin, the first now in the component kept for the cell.
    let cell = component(&s, "Cell");
    let cell_here = placements(&s, cell, top);
    assert_eq!(cell_here.len(), 1);
    assert_eq!((placements(&s, pin, cell), placements(&s, pin, top)), (vec![pins[0]], vec![pins[1]]));
    // The joint reaches the pin through the cell now, and holds the unit as it did.
    let lift = s.doc.assembly.joints.iter().find(|j| j.name == "Lift").cloned().expect("the joint to the pin is kept");
    assert_eq!((lift.b.occurrence, lift.b.inside.clone()), (unit_here, vec![cell_here[0], pins[0]]));
    assert_eq!(boxes(&s), with_block(shifted(&unit_boxes, [100, 0, 0])));
    let driven = run(&mut s, "joint.drive", json!({"joint": "Lift", "value": 10}));
    assert_eq!(driven["moved"], true, "{driven}");
    assert_eq!(boxes(&s), with_block(shifted(&unit_boxes, [100, 0, 10])));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A design holds at most 10 000 joints. When the linked design replaces a joint by another,
/// the one here goes and the new one takes its place: the count does not grow, so a design
/// that is full takes the update. Here one joint of the design's own, 9 998 suppressed copies of
/// it and the hinge's one joint are 10 000. The hinge's design deletes `Hinge` and makes
/// `Pivot` in its place. After Update Links the design holds 10 000 joints again, `Pivot`
/// among them.
#[test]
fn a_design_full_of_joints_takes_the_joint_that_replaces_one_in_its_linked_design() {
    let dir = scratch("full");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    for name in ["A", "B"] {
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [-60, 0, 0]}));
        run(&mut s, "component.activate", json!({"component": 0}));
    }
    let origin = |part: &str| json!({"occurrence": part, "point": [-60, 0, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "revolute", "name": "Mine", "flip": true, "a": origin("A"), "b": origin("B")}));
    let mine = s.doc.assembly.joints[0].clone();
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "translate": [0, 0, 50], "flexible": true}));
    let (top, first) = (inserted["component"].as_u64().unwrap(), inserted["occurrence"].as_u64().unwrap());
    assert_eq!(joint_of(&s, top, first).1, "Hinge");
    let limit = solidcraft_doc::joints::MAX_JOINTS;
    let mut doc = (*s.doc).clone();
    for k in doc.assembly.joints.len()..limit {
        let id = doc.allocate_id().unwrap();
        doc.assembly.joints.push(solidcraft_doc::joints::Joint { id, name: format!("Spare {k}"), suppressed: true, ..mine.clone() });
    }
    s.set_doc(doc);
    assert_eq!(s.doc.assembly.joints.len(), limit);

    let mut design = open(&hinge);
    run(&mut design, "joint.delete", json!({"joint": "Hinge"}));
    let side = |part: &str| json!({"occurrence": part, "point": [0, 10, 7], "z": [0, 0, 1]});
    let pivot = json!({"type": "revolute", "name": "Pivot", "flip": true, "limits": [[-90, 90]], "a": side("base"), "b": side("arm")});
    run(&mut design, "joint.create", pivot);
    run(&mut design, "file.save", json!({}));
    let updated = run(&mut s, "component.update_links", json!({}));
    assert_eq!(updated["updated"], json!(["hinge"]), "{updated}");
    assert!(!updated["warnings"].to_string().contains("no room"), "{updated}");
    assert_eq!(joint_of(&s, top, first).1, "Pivot", "the occurrence has the joint that replaced its hinge");
    assert!(s.doc.assembly.joints.iter().all(|j| j.name != "Hinge"), "and the one it replaced is gone");
    assert_eq!(s.doc.assembly.joints.len(), limit);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A body transfer that is in the timeline already is left alone, until a command turns it on
/// a component a link keeps: edited to take the plate's body out of the plate, it is refused as
/// a new transfer of that body is, and the design stays as it was.
#[test]
fn an_edited_body_transfer_cannot_take_a_body_out_of_a_component_a_link_keeps() {
    let dir = scratch("kept-transfer");
    let unit = save_unit(&dir);
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": unit, "structure": true}));
    let plate = component(&s, "plate");
    // A block of this design's own, moved into a component of this design's own.
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 50], "body_name": "Block"}));
    let mine = run(&mut s, "component.create", json!({"name": "Mine", "activate": false}))["component"].as_u64().unwrap();
    let transfer = run(&mut s, "component.move_bodies", json!({"component": mine, "bodies": ["Block"]}))["features"][0].as_u64().unwrap();
    let owner = |s: &Session, body: &str| s.doc.model_body_component(s.model.state().body(body).unwrap());
    assert_eq!((owner(&s, "Block"), owner(&s, "Plate")), (mine, plate));
    let before = serde_json::to_string(&*s.doc).unwrap();
    let edit = json!({"feature": transfer, "set": {"from": plate, "bodies": ["Plate"]}});
    let refused = s.execute("timeline.edit", &edit).expect_err("the plate's body would leave the plate").to_string();
    assert!(refused.contains("linked subassembly unit"), "{refused}");
    assert_eq!(serde_json::to_string(&*s.doc).unwrap(), before, "the design is as it was");
    assert_eq!((owner(&s, "Block"), owner(&s, "Plate")), (mine, plate));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Asked for every placement, the command must have every placement to weigh. A design walks at
/// most 100 000 placements, and one whose subassemblies come to more is refused, not weighed in
/// part: 317 racks of 317 slots each are 317 + 317 x 317 = 100 806 placements, from 634
/// occurrences. Untold, the 634 are listed once each, with the design itself, as before.
#[test]
fn every_placement_is_refused_where_a_design_has_more_placements_than_can_be_listed() {
    let mut s = Session::default();
    let rack = run(&mut s, "component.create", json!({"name": "Rack", "activate": false}))["component"].as_u64().unwrap();
    let slot = run(&mut s, "component.create", json!({"name": "Slot", "parent": rack, "activate": false}))["component"].as_u64().unwrap();
    let mut doc = (*s.doc).clone();
    for k in 2..=317 {
        for (component, parent, name) in [(rack, 0, "Rack"), (slot, rack, "Slot")] {
            let id = doc.allocate_id().unwrap();
            let name = format!("{name}:{k}");
            doc.occurrences.push(solidcraft_doc::Occurrence { id, component, parent, name, transform: solidcraft_doc::IDENTITY, grounded: false });
        }
    }
    s.set_doc(doc);
    assert_eq!(s.doc.occurrences.len(), 634);
    let first = run(&mut s, "assembly.mass_properties", json!({}));
    assert_eq!(first["occurrences"].as_array().map(Vec::len), Some(635), "each occurrence once, and the design itself");
    let refused = s.execute("assembly.mass_properties", &json!({"every_placement": true})).expect_err("100 806 placements").to_string();
    assert!(refused.contains("more placements than `every_placement` lists"), "{refused}");
}

/// With `every_placement`, a further placement stands in the rigid groups as the first one
/// does. A cell that holds a flexible hinge is placed twice. In each cell the hinge's joint
/// moves the arm against the base, so the arm (30 x 4 x 4 = 480 mm3) is a group of its own, the
/// base (40 x 20 x 5 = 4000 mm3) is in its hinge's group, and neither is in the cell's. Untold,
/// the hinge and its parts are listed once, in the first cell.
#[test]
fn every_placement_keeps_what_a_joint_moves_apart_in_each_placement() {
    let dir = scratch("mass-groups");
    let hinge = save_hinge(&dir);
    let mut s = Session::default();
    let cell_one = run(&mut s, "component.create", json!({"name": "Cell", "activate": false}))["occurrence"].as_u64().unwrap();
    let inserted = run(&mut s, "component.insert_design", json!({"path": hinge, "parent": "Cell", "flexible": true}));
    let (top, hinge_here) = (inserted["component"].as_u64().unwrap(), inserted["occurrence"].as_u64().unwrap());
    let cell_two = run(&mut s, "occurrence.copy", json!({"occurrence": cell_one, "translate": [0, 100, 0]}))["occurrence"].as_u64().unwrap();
    let (base, arm) = (placements(&s, component(&s, "base"), top)[0], placements(&s, component(&s, "arm"), top)[0]);
    let listed = |mass: &Value| (mass["occurrences"].as_array().map(Vec::len), mass["groups"].as_array().map(Vec::len));

    // Untold: the design, the two cells, the hinge, its base and its arm. The base is in the
    // hinge's group, so there are five groups.
    let first = run(&mut s, "assembly.mass_properties", json!({}));
    assert_eq!(listed(&first), (Some(6), Some(5)), "{first}");

    // Every placement: the hinge, its base and its arm once more, in the second cell, and two
    // groups more for them.
    let mass = run(&mut s, "assembly.mass_properties", json!({"every_placement": true}));
    assert_eq!(listed(&mass), (Some(9), Some(7)), "{mass}");
    let entries = mass["occurrences"].as_array().unwrap();
    let at = |path: &[u64]| entries.iter().find(|o| o["path"] == json!(path)).unwrap_or_else(|| panic!("{path:?} in {mass}"));
    let group = |path: &[u64]| at(path)["group"].as_u64().unwrap() as usize;
    let kg = |group: usize| mass["groups"][group]["mass"]["mass_kg"].as_f64().unwrap();
    let solid = |path: &[u64]| at(path)["bodies"][0]["volume_mm3"].as_f64().map(f64::round);
    for cell in [cell_one, cell_two] {
        let (of_cell, of_hinge) = (group(&[cell]), group(&[cell, hinge_here]));
        let (of_base, of_arm) = (group(&[cell, hinge_here, base]), group(&[cell, hinge_here, arm]));
        assert!(of_arm != of_base && of_arm != of_cell, "the hinge's joint moves the arm: {mass}");
        assert!(of_base == of_hinge && of_hinge != of_cell, "the base stays with its hinge: {mass}");
        assert_eq!(mass["groups"][of_arm]["occurrences"], json!([arm]), "{mass}");
        assert_eq!((solid(&[cell, hinge_here, arm]), solid(&[cell, hinge_here, base])), (Some(480.0), Some(4000.0)));
        // One material throughout, so the two groups weigh as their volumes: 480 to 4000.
        assert!((kg(of_arm) / kg(of_base) - 0.12).abs() < 1e-9, "{mass}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Replace with Link and Find Identical Copies compare a component body for body with a link
/// that holds its design in one component. A link that keeps its design's components builds a
/// part the design places twice once, so a component that builds the unit's four solids (the
/// plate, the rail, and the pin at both its places) is not compared with it: both commands say
/// why and how to go on, where the one said the copy "is not the same" and the other found no
/// copy. The way they name works: with the link in one component the copy is found and
/// replaced, and the link keeps its components again, the copy now the unit's second
/// occurrence, 100 mm along Y, where it stood.
#[test]
fn replace_with_link_says_why_it_takes_no_link_that_keeps_components_and_how_to_go_on() {
    let dir = scratch("replace");
    let unit = save_unit(&dir);
    let unit_boxes = boxes(&open(&unit));
    let mut s = Session::default();
    let top = run(&mut s, "component.insert_design", json!({"path": unit, "structure": true}))["component"].as_u64().unwrap();
    // The same four solids as bodies of one component. The unit's second pin is the first
    // turned a quarter turn: the box (24, 5, 5)..(30, 9, 15), 6 x 4 x 10.
    run(&mut s, "component.create", json!({"name": "Copy"}));
    for (size, corner, body) in [
        ([40, 20, 5], [0, 0, 0], "Copy plate"),
        ([40, 2, 2], [0, 22, 0], "Copy rail"),
        ([4, 6, 10], [5, 5, 5], "Copy pin"),
        ([6, 4, 10], [24, 5, 5], "Copy pin 2"),
    ] {
        run(&mut s, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner, "body_name": body}));
    }
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Copy:1", "translate": [0, 100, 0]}));
    let both = together(unit_boxes.clone(), shifted(&unit_boxes, [0, 100, 0]));
    assert_eq!(boxes(&s), both);
    let before = serde_json::to_string(&*s.doc).unwrap();
    let asked = [("component.replace_with_link", json!({"component": "Copy", "with": top})), ("component.copies", json!({"component": top}))];
    for (command, params) in asked {
        let why = s.execute(command, &params).expect_err(command).to_string();
        assert!(why.contains("unit keeps its linked design's components") && why.contains("component.link_mode"), "{command}: {why}");
    }
    assert_eq!(serde_json::to_string(&*s.doc).unwrap(), before, "the design is as it was");
    // The way round that the message names.
    run(&mut s, "component.link_mode", json!({"component": top, "structure": false}));
    let copies = run(&mut s, "component.copies", json!({"component": top}));
    assert_eq!(copies["copies"][0]["name"], "Copy", "{copies}");
    let replaced = run(&mut s, "component.replace_with_link", json!({"component": "Copy", "with": top}));
    assert_eq!(replaced["replaced"], json!(["Copy"]), "{replaced}");
    run(&mut s, "component.link_mode", json!({"component": top, "structure": true}));
    assert_eq!(s.doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["unit", "plate", "pin"]);
    assert_eq!(placements(&s, top, 0).len(), 2, "the copy is the unit's second occurrence");
    assert_eq!(boxes(&s), both, "and shows what it showed");
    assert!((volume(&mut s) - 9280.0).abs() < 1e-6, "{}", volume(&mut s));
    let _ = std::fs::remove_dir_all(&dir);
}
