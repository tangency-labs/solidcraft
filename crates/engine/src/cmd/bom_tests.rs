use serde_json::{Value, json};
use solidcraft_doc::bom::{self, Bom, Group, Mass, Request, Tree, Unknown, View};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// A folder of its own for one test's files.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-bom-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A plate placed three times, and a hinge subassembly (no bodies of its own) placed twice
/// that holds one pin: the plate has 3 occurrences, the pin 2 (one in each hinge).
fn assembly() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 3}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Hinge", "activate": false}));
    run(&mut s, "component.create", json!({"name": "Pin", "parent": "Hinge"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 4, "height": 30}));
    run(&mut s, "component.activate", json!({"component": 0}));
    for x in [50, 100] {
        run(&mut s, "occurrence.copy", json!({"component": "Plate", "translate": [x, 0, 0]}));
    }
    run(&mut s, "occurrence.copy", json!({"component": "Hinge", "translate": [0, 50, 0]}));
    run(&mut s, "component.metadata.set", json!({"component": "Plate", "part_number": "PL-100", "description": "Plate, 3 mm", "revision": "B"}));
    run(
        &mut s,
        "component.metadata.set",
        json!({"component": "Pin", "part_number": "PN-040", "description": "Pin 4 x 30", "properties": {"finish": "zinc"}}),
    );
    s
}

fn summary(r: &Value) -> Vec<(String, String, u64)> {
    r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["part_number"].as_str().unwrap().to_string(), r["definition"].as_str().unwrap().to_string(), r["count"].as_u64().unwrap()))
        .collect()
}

#[test]
fn flat_counts_parts_through_subassemblies_and_top_counts_children() {
    let mut s = assembly();
    let undo = s.undo.len();
    let flat = run(&mut s, "assembly.bom", json!({}));
    assert_eq!(summary(&flat), [("PL-100".into(), "Plate".into(), 3), ("PN-040".into(), "Pin".into(), 2)], "{flat}");
    assert_eq!(flat["total_occurrences"], 5);
    assert_eq!(flat["rows"][0]["item"], 1);
    assert_eq!(flat["rows"][0]["description"], "Plate, 3 mm");
    assert_eq!(flat["rows"][0]["revision"], "B");
    assert_eq!(flat["rows"][1]["properties"], json!({"finish": "zinc"}));

    let top = run(&mut s, "assembly.bom", json!({"levels": "top"}));
    assert_eq!(summary(&top), [("PL-100".into(), "Plate".into(), 3), (String::new(), "Hinge".into(), 2)], "{top}");
    assert_eq!(top["total_occurrences"], 5);

    // One subassembly's own bill of materials.
    let hinge = run(&mut s, "assembly.bom", json!({"component": "Hinge"}));
    assert_eq!(summary(&hinge), [("PN-040".into(), "Pin".into(), 1)]);
    assert_eq!(s.undo.len(), undo, "the bill of materials only reads");
}

#[test]
fn equal_part_numbers_share_a_row_unless_grouped_by_definition() {
    let mut s = assembly();
    // An independent copy keeps the part properties, so it is the same part.
    run(&mut s, "component.paste_new", json!({"component": "Plate", "translate": [0, -60, 0]}));
    let r = run(&mut s, "assembly.bom", json!({}));
    let plate = &r["rows"][0];
    assert_eq!((plate["part_number"].as_str(), plate["count"].as_u64()), (Some("PL-100"), Some(4)), "{r}");
    assert_eq!(plate["components"].as_array().map(Vec::len), Some(2));
    let by_def = run(&mut s, "assembly.bom", json!({"group_by": ["definition"]}));
    let counts: Vec<u64> =
        by_def["rows"].as_array().unwrap().iter().filter(|r| r["part_number"] == "PL-100").map(|r| r["count"].as_u64().unwrap()).collect();
    assert_eq!(counts, [3, 1], "{by_def}");
    // Without a part number every definition is its own row, after the numbered ones.
    run(&mut s, "component.create", json!({"name": "Spacer"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let r = run(&mut s, "assembly.bom", json!({}));
    let last = r["rows"].as_array().unwrap().last().cloned().unwrap();
    assert_eq!((last["part_number"].as_str(), last["definition"].as_str()), (Some(""), Some("Spacer")));
}

#[test]
fn part_properties_survive_save_and_open_and_undo_in_one_step() {
    let dir = scratch("roundtrip");
    let mut s = assembly();
    let undo = s.undo.len();
    let r = run(&mut s, "component.metadata.set", json!({"component": "Pin", "revision": "C", "properties": {"finish": null, "material": "1.4301"}}));
    assert_eq!(r["changed"], json!(["revision", "properties.finish", "properties.material"]));
    assert_eq!(s.undo.len(), undo + 1, "one undo step");
    // Setting the same values again changes nothing.
    let r = run(&mut s, "component.metadata.set", json!({"component": "Pin", "revision": "C"}));
    assert_eq!(r["changed"], json!([]));
    assert_eq!(s.undo.len(), undo + 1);
    for (file, format) in [("bom.json", "json"), ("bom.solidcraft", "container")] {
        let path = dir.join(file);
        run(&mut s, "file.save_as", json!({"path": path, "format": format}));
        let mut back = Session::default();
        run(&mut back, "doc.open", json!({"path": path}));
        let pin = run(&mut back, "component.metadata", json!({"component": "Pin"}));
        assert_eq!((pin["part_number"].as_str(), pin["revision"].as_str()), (Some("PN-040"), Some("C")), "{format}: {pin}");
        assert_eq!(pin["properties"], json!({"material": "1.4301"}), "{format}");
        assert_eq!(summary(&run(&mut back, "assembly.bom", json!({}))), summary(&run(&mut s, "assembly.bom", json!({}))), "{format}");
    }
    run(&mut s, "edit.undo", json!({}));
    let pin = run(&mut s, "component.metadata", json!({"component": "Pin"}));
    assert_eq!((pin["revision"].as_str(), &pin["properties"]), (None, &json!({"finish": "zinc"})));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn csv_quotes_every_field_and_json_writes_the_rows() {
    let dir = scratch("export");
    let mut s = assembly();
    run(&mut s, "component.metadata.set", json!({"component": "Pin", "description": "Pin \"short\", 4 mm"}));
    let csv = dir.join("bom.csv");
    let r = run(&mut s, "assembly.bom", json!({"path": csv}));
    assert_eq!(r["format"], "csv");
    let text = std::fs::read_to_string(&csv).unwrap();
    assert_eq!(
        text,
        "\"item\",\"part_number\",\"description\",\"revision\",\"count\",\"definition\",\"material\"\n\
         \"1\",\"PL-100\",\"Plate, 3 mm\",\"B\",\"3\",\"Plate\",\"Default\"\n\
         \"2\",\"PN-040\",\"Pin \"\"short\"\", 4 mm\",\"\",\"2\",\"Pin\",\"Default\"\n"
    );
    let json_path = dir.join("bom.json");
    run(&mut s, "assembly.bom", json!({"path": json_path, "levels": "top"}));
    let back: Value = serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
    assert_eq!(back["total_occurrences"], 5);
    assert_eq!(back["rows"][1]["definition"], "Hinge");
    assert!(s.execute("assembly.bom", &json!({"path": dir.join("bom.txt")})).is_err(), "unknown extension");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn standard_parts_carry_a_part_number_that_follows_a_resize() {
    let mut s = Session::default();
    let a = run(&mut s, "parts.insert", json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20}));
    run(&mut s, "parts.insert", json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20, "point": [30, 0, 0]}));
    run(&mut s, "parts.insert", json!({"family": "bearing", "size": "608", "point": [60, 0, 0]}));
    let r = run(&mut s, "assembly.bom", json!({}));
    let rows: Vec<(String, String, u64)> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["part_number"].as_str().unwrap().into(), r["description"].as_str().unwrap().into(), r["count"].as_u64().unwrap()))
        .collect();
    assert_eq!(
        rows,
        [("608".into(), "Deep groove ball bearing (envelope) 608".into(), 1), ("ISO 4762 M6x20".into(), "Socket head cap screw M6x20".into(), 2)]
    );
    let screws = r["rows"].as_array().unwrap().iter().find(|r| r["count"] == 2).cloned().unwrap();
    assert_eq!(screws["definition"], "SHCS M6x20", "equal names are named once");
    assert_eq!(screws["components"].as_array().map(Vec::len), Some(2));
    // A resize renumbers the part; a description someone wrote stays theirs.
    let comp = a["component"].as_u64().unwrap();
    run(&mut s, "component.metadata.set", json!({"component": comp, "description": "Cap screw, A2"}));
    run(&mut s, "parts.resize", json!({"component": comp, "size": "M8", "length": 25}));
    let m = run(&mut s, "component.metadata", json!({"component": comp}));
    assert_eq!((m["part_number"].as_str(), m["description"].as_str()), (Some("ISO 4762 M8x25"), Some("Cap screw, A2")));
}

#[test]
fn hostile_part_properties_are_refused_without_a_change() {
    let mut s = assembly();
    let undo = s.undo.len();
    for p in [
        json!({"component": "Pin"}),
        json!({"component": 0, "part_number": "X"}),
        json!({"component": "Nope", "part_number": "X"}),
        json!({"component": "Pin", "part_number": 7}),
        json!({"component": "Pin", "part_number": "ok", "description": "bell\u{7}"}),
        json!({"component": "Pin", "properties": {"": "x"}}),
        json!({"component": "Pin", "properties": {"finish": ["x"]}}),
        json!({"component": "Pin", "properties": {"finish": "x".repeat(5000)}}),
        json!({"component": "Pin", "values": {"bogus": "x"}}),
        json!({"component": "Pin", "values": "x"}),
    ] {
        assert!(s.execute("component.metadata.set", &p).is_err(), "{p}");
    }
    assert_eq!(s.undo.len(), undo);
    let pin = run(&mut s, "component.metadata", json!({"component": "Pin"}));
    assert_eq!(pin["part_number"], "PN-040", "all or nothing");
    for p in [
        json!({"levels": "deep"}),
        json!({"group_by": ["colour"]}),
        json!({"group_by": 3}),
        json!({"component": "Nope"}),
        json!({"mass": "yes"}),
        json!({"mass": 1}),
    ] {
        assert!(s.execute("assembly.bom", &p).is_err(), "{p}");
    }
    // The root reads as the design's own properties.
    run(&mut s, "document.metadata.set", json!({"key": "part_number", "value": "ASM-1"}));
    assert_eq!(run(&mut s, "component.metadata", json!({"component": 0}))["part_number"], "ASM-1");
}

/// Two hex bolts, each with a washer and a nut placed inside it, and a third bolt alone.
fn bolts() -> Session {
    let mut s = Session::default();
    for x in [0, 30] {
        let bolt = run(&mut s, "parts.insert", json!({"family": "hex_bolt", "size": "M6", "length": 30, "point": [x, 0, 0], "direction": [0, 0, 1]}));
        run(&mut s, "component.activate", json!({"component": bolt["component"]}));
        run(&mut s, "parts.insert", json!({"family": "washer", "size": "M6", "point": [x, 0, -2], "direction": [0, 0, -1]}));
        run(&mut s, "parts.insert", json!({"family": "hex_nut", "size": "M6", "point": [x, 0, -4], "direction": [0, 0, -1]}));
        run(&mut s, "component.activate", json!({"component": 0}));
    }
    run(&mut s, "parts.insert", json!({"family": "hex_bolt", "size": "M6", "length": 30, "point": [60, 0, 0], "direction": [0, 0, 1]}));
    s
}

#[test]
fn group_children_keeps_a_bolts_washer_and_nut_in_its_row() {
    let mut s = bolts();
    let plain = run(&mut s, "assembly.bom", json!({}));
    let numbers: Vec<(String, u64)> = summary(&plain).into_iter().map(|(pn, _, n)| (pn, n)).collect();
    assert_eq!(numbers, [("ISO 4017 M6x30".into(), 3), ("ISO 4032 M6".into(), 2), ("ISO 7089 M6".into(), 2)], "{plain}");

    let r = run(&mut s, "assembly.bom", json!({"group_children": true}));
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "the bolts with a nut and the bolt alone: {r}");
    let kit = &rows[0];
    assert_eq!((kit["part_number"].as_str(), kit["count"].as_u64()), (Some("ISO 4017 M6x30"), Some(2)), "{r}");
    let children: Vec<(&str, &str, u64, u64)> = kit["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["item"].as_str().unwrap(), c["part_number"].as_str().unwrap(), c["each"].as_u64().unwrap(), c["count"].as_u64().unwrap()))
        .collect();
    assert_eq!(children, [("1.1", "ISO 4032 M6", 1, 2), ("1.2", "ISO 7089 M6", 1, 2)]);
    assert_eq!((rows[1]["count"].as_u64(), rows[1].get("children")), (Some(1), None));
    assert_eq!(r["total_occurrences"], plain["total_occurrences"], "the same parts, folded");

    let dir = scratch("kit");
    let path = dir.join("bom.csv");
    run(&mut s, "assembly.bom", json!({"group_children": true, "path": path.to_str().unwrap()}));
    let text = std::fs::read_to_string(&path).unwrap();
    let items: Vec<&str> = text.lines().skip(1).map(|l| l.split(',').next().unwrap()).collect();
    assert_eq!(items, ["\"1\"", "\"1.1\"", "\"1.2\"", "\"2\""], "{text}");
    assert!(s.execute("assembly.bom", &json!({"group_children": "yes"})).is_err());
}

#[test]
fn changing_part_properties_does_not_evaluate_the_model_again() {
    let mut s = bolts();
    s.evaluations = 0;
    run(&mut s, "component.metadata.set", json!({"component": 1, "description": "Bolt", "properties": {"finish": "zinc"}}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.evaluations, 0, "part properties change no geometry");
    assert_eq!(run(&mut s, "component.metadata", json!({"component": 1}))["description"], "Bolt");
    // A geometric edit after it finds every other feature's earlier result.
    let first = run(&mut s, "assembly.bom", json!({}))["rows"][1]["components"][0].clone();
    run(&mut s, "parts.resize", json!({"component": first, "size": "M8"}));
    run(&mut s, "parts.resize", json!({"component": first, "size": "M6"}));
    // Anything else still evaluates.
    run(&mut s, "occurrence.copy", json!({"component": 1, "translate": [0, 40, 0]}));
    assert!(s.evaluations > 0);
}

#[test]
fn body_owners_match_each_bodys_component() {
    // A body made in the design and moved into a component belongs to it.
    let mut moved = assembly();
    let target = run(&mut moved, "component.create", json!({"name": "Target"}))["component"].as_u64().unwrap();
    run(&mut moved, "component.activate", json!({"component": 0}));
    run(&mut moved, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [0, -30, 0], "body_name": "Moved"}));
    run(&mut moved, "component.move_bodies", json!({"bodies": ["Moved"], "component": target}));
    let st = moved.model.state();
    let owner = st.bodies.iter().position(|b| b.name == "Moved").map(|i| moved.doc.body_owners(&st)[i]);
    assert_eq!(owner, Some(target));
    for s in [assembly(), bolts(), moved] {
        let st = s.model.state();
        let each: Vec<u64> = st.bodies.iter().map(|b| s.doc.model_body_component(b)).collect();
        assert_eq!(s.doc.body_owners(&st), each);
        assert!(each.iter().collect::<std::collections::HashSet<_>>().len() > 1);
    }
}

/// A machine: a plate, and a gearbox (no bodies of its own) that holds a shaft and a bearing.
/// Each is placed once.
fn machine() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Gearbox", "activate": false}));
    for (name, diameter, height) in [("Shaft", 10, 80), ("Bearing", 22, 7)] {
        run(&mut s, "component.create", json!({"name": name, "parent": "Gearbox"}));
        run(&mut s, "solid.cylinder", json!({"diameter": diameter, "height": height}));
        run(&mut s, "component.activate", json!({"component": 0}));
    }
    for (c, pn) in [("Plate", "PL-1"), ("Gearbox", "GB-1"), ("Shaft", "SH-1"), ("Bearing", "BR-1")] {
        run(&mut s, "component.metadata.set", json!({"component": c, "part_number": pn}));
    }
    s
}

/// Each row's part number, bill of materials type and count.
fn typed(r: &Value) -> Vec<(String, String, u64)> {
    r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["part_number"].as_str().unwrap().to_string(), r["bom_type"].as_str().unwrap().to_string(), r["count"].as_u64().unwrap()))
        .collect()
}

fn row(part_number: &str, bom_type: &str, count: u64) -> (String, String, u64) {
    (part_number.to_string(), bom_type.to_string(), count)
}

fn set_type(s: &mut Session, component: &str, bom_type: &str) {
    run(s, "component.metadata.set", json!({"component": component, "bom_type": bom_type}));
}

#[test]
fn bom_types_decide_what_is_a_row_and_what_counts_inside_it() {
    let mut s = machine();
    let top = |s: &mut Session| typed(&run(s, "assembly.bom", json!({"levels": "top"})));
    let flat = |s: &mut Session| typed(&run(s, "assembly.bom", json!({})));
    let own = |s: &mut Session| typed(&run(s, "assembly.bom", json!({"component": "Gearbox"})));
    // Normal: the gearbox is a row at the top and is listed through its parts in parts only.
    assert_eq!(top(&mut s), [row("GB-1", "normal", 1), row("PL-1", "normal", 1)]);
    assert_eq!(flat(&mut s), [row("BR-1", "normal", 1), row("PL-1", "normal", 1), row("SH-1", "normal", 1)]);

    // Purchased: one row, with nothing inside it listed; its own bill of materials still lists it.
    set_type(&mut s, "Gearbox", "purchased");
    assert_eq!(top(&mut s), [row("GB-1", "purchased", 1), row("PL-1", "normal", 1)]);
    assert_eq!(flat(&mut s), [row("GB-1", "purchased", 1), row("PL-1", "normal", 1)]);
    assert_eq!(own(&mut s), [row("BR-1", "normal", 1), row("SH-1", "normal", 1)]);

    // Inseparable: one row; of what it holds only purchased parts reach the parts-only list.
    set_type(&mut s, "Gearbox", "inseparable");
    assert_eq!(flat(&mut s), [row("GB-1", "inseparable", 1), row("PL-1", "normal", 1)]);
    set_type(&mut s, "Bearing", "purchased");
    assert_eq!(top(&mut s), [row("GB-1", "inseparable", 1), row("PL-1", "normal", 1)]);
    assert_eq!(flat(&mut s), [row("BR-1", "purchased", 1), row("GB-1", "inseparable", 1), row("PL-1", "normal", 1)]);
    assert_eq!(own(&mut s), [row("BR-1", "purchased", 1), row("SH-1", "normal", 1)], "in its own context an ordinary assembly");

    // Reference: counted nowhere, and nothing inside it is counted, purchased parts included.
    set_type(&mut s, "Gearbox", "reference");
    assert_eq!(top(&mut s), [row("PL-1", "normal", 1)]);
    assert_eq!(flat(&mut s), [row("PL-1", "normal", 1)]);
    assert_eq!(run(&mut s, "assembly.bom", json!({}))["total_occurrences"], 1);

    // Phantom: no row; what it holds counts where it is placed, at the top level too.
    set_type(&mut s, "Gearbox", "phantom");
    assert_eq!(top(&mut s), [row("BR-1", "purchased", 1), row("PL-1", "normal", 1), row("SH-1", "normal", 1)]);
    assert_eq!(flat(&mut s), [row("BR-1", "purchased", 1), row("PL-1", "normal", 1), row("SH-1", "normal", 1)]);

    // Back to normal (empty text removes the key) gives the first lists again.
    set_type(&mut s, "Gearbox", "");
    set_type(&mut s, "Bearing", "normal");
    assert_eq!(top(&mut s), [row("GB-1", "normal", 1), row("PL-1", "normal", 1)]);
    assert_eq!(flat(&mut s), [row("BR-1", "normal", 1), row("PL-1", "normal", 1), row("SH-1", "normal", 1)]);
    assert_eq!(run(&mut s, "component.metadata", json!({"component": "Gearbox"}))["bom_type"], "normal");
}

#[test]
fn a_phantom_counts_what_it_holds_in_its_parent_merged_with_the_same_parts() {
    // A fastener set (no bodies) of two screws, placed three times, and one more screw placed directly.
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Set", "activate": false}));
    run(&mut s, "component.create", json!({"name": "Screw", "parent": "Set"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 6, "height": 20}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.copy", json!({"component": "Screw", "parent": "Set", "translate": [10, 0, 0]}));
    for y in [50, 100] {
        run(&mut s, "occurrence.copy", json!({"component": "Set", "translate": [0, y, 0]}));
    }
    run(&mut s, "occurrence.copy", json!({"component": "Screw", "parent": 0, "translate": [0, -50, 0]}));
    run(&mut s, "component.metadata.set", json!({"component": "Screw", "part_number": "SC-6"}));
    run(&mut s, "component.metadata.set", json!({"component": "Set", "part_number": "SET-1"}));
    assert_eq!(typed(&run(&mut s, "assembly.bom", json!({"levels": "top"}))), [row("SC-6", "normal", 1), row("SET-1", "normal", 3)]);
    assert_eq!(typed(&run(&mut s, "assembly.bom", json!({}))), [row("SC-6", "normal", 7)], "2 x 3 + 1");

    set_type(&mut s, "Set", "phantom");
    assert_eq!(typed(&run(&mut s, "assembly.bom", json!({"levels": "top"}))), [row("SC-6", "normal", 7)], "promoted and merged");
    let flat = run(&mut s, "assembly.bom", json!({}));
    assert_eq!(typed(&flat), [row("SC-6", "normal", 7)]);
    assert_eq!(flat["total_occurrences"], 7);

    // A phantom inside a phantom promotes through both.
    run(&mut s, "component.create", json!({"name": "Kit", "activate": false}));
    run(&mut s, "occurrence.copy", json!({"component": "Set", "parent": "Kit"}));
    run(&mut s, "occurrence.copy", json!({"component": "Kit", "translate": [0, 200, 0]}));
    set_type(&mut s, "Kit", "phantom");
    // The Kit created at the root holds one Set; its copy holds the same: 2 kits x 2 screws more.
    assert_eq!(typed(&run(&mut s, "assembly.bom", json!({"levels": "top"}))), [row("SC-6", "normal", 11)]);
}

/// Kits with one part number fold into one row only when what they hold is the same, its bill of
/// materials types included: a kit holding a bought bolt isn't the kit holding a made one.
#[test]
fn kits_with_one_part_number_fold_together_only_when_their_contents_types_match() {
    let mut s = Session::default();
    for (kit, bolt) in [("KitA", "BoltA"), ("KitB", "BoltB")] {
        run(&mut s, "component.create", json!({"name": kit, "activate": false}));
        run(&mut s, "component.create", json!({"name": bolt, "parent": kit}));
        run(&mut s, "solid.cylinder", json!({"diameter": 6, "height": 20}));
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "component.metadata.set", json!({"component": kit, "part_number": "KIT"}));
        run(&mut s, "component.metadata.set", json!({"component": bolt, "part_number": "BOLT"}));
    }
    // Each kit row: its count, and each child's definition, count in one kit and count in all.
    let kits = |s: &mut Session| -> Vec<(u64, Vec<(String, u64, u64)>)> {
        let r = run(s, "assembly.bom", json!({"group_children": true}));
        let child = |c: &Value| (c["definition"].as_str().unwrap().to_string(), c["each"].as_u64().unwrap(), c["count"].as_u64().unwrap());
        r["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| (k["count"].as_u64().unwrap(), k["children"].as_array().unwrap().iter().map(child).collect()))
            .collect()
    };
    let same = kits(&mut s);
    assert_eq!((same.len(), same[0].0, same[0].1.len(), same[0].1[0].1, same[0].1[0].2), (1, 2, 1, 1, 2), "{same:?}");
    set_type(&mut s, "BoltB", "purchased");
    assert_eq!(kits(&mut s), [(1, vec![("BoltA".into(), 1, 1)]), (1, vec![("BoltB".into(), 1, 1)])]);
}

#[test]
fn a_purchased_part_keeps_what_is_placed_inside_it_out_of_the_list() {
    let mut s = bolts();
    // Each inserted bolt is a definition of its own; buy them all.
    let bolts = run(&mut s, "assembly.bom", json!({}))["rows"][0]["components"].clone();
    assert_eq!(bolts.as_array().map(Vec::len), Some(3));
    for bolt in bolts.as_array().unwrap() {
        run(&mut s, "component.metadata.set", json!({"component": bolt, "bom_type": "purchased"}));
    }
    // The washers and nuts placed inside two of them are no longer listed.
    let plain = run(&mut s, "assembly.bom", json!({}));
    assert_eq!(typed(&plain), [row("ISO 4017 M6x30", "purchased", 3)], "{plain}");
    assert_eq!(plain["total_occurrences"], 3);
    // Folding has nothing to fold: a purchased part is one row already.
    let folded = run(&mut s, "assembly.bom", json!({"group_children": true}));
    assert_eq!(typed(&folded), [row("ISO 4017 M6x30", "purchased", 3)]);
    assert!(folded["rows"][0].get("children").is_none(), "{folded}");
    // One bolt bought and two made gives two rows for the one part number.
    run(&mut s, "component.metadata.set", json!({"component": bolts[0], "bom_type": null}));
    run(&mut s, "component.metadata.set", json!({"component": bolts[1], "bom_type": null}));
    let mixed = typed(&run(&mut s, "assembly.bom", json!({})));
    assert!(mixed.contains(&row("ISO 4017 M6x30", "normal", 2)) && mixed.contains(&row("ISO 4017 M6x30", "purchased", 1)), "{mixed:?}");
}

#[test]
fn a_bom_type_is_one_undo_step_survives_save_and_open_and_evaluates_nothing() {
    let dir = scratch("bom-type");
    let mut s = machine();
    let undo = s.undo.len();
    s.evaluations = 0;
    let r = run(&mut s, "component.metadata.set", json!({"component": "Gearbox", "bom_type": "inseparable"}));
    assert_eq!(r["changed"], json!(["bom_type"]));
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(s.evaluations, 0, "a part property changes no geometry");
    for (file, format) in [("machine.json", "json"), ("machine.solidcraft", "container")] {
        let path = dir.join(file);
        run(&mut s, "file.save_as", json!({"path": path, "format": format}));
        let mut back = Session::default();
        run(&mut back, "doc.open", json!({"path": path}));
        assert_eq!(run(&mut back, "component.metadata", json!({"component": "Gearbox"}))["bom_type"], "inseparable", "{format}");
        assert_eq!(typed(&run(&mut back, "assembly.bom", json!({}))), typed(&run(&mut s, "assembly.bom", json!({}))), "{format}");
    }
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "component.metadata", json!({"component": "Gearbox"}))["bom_type"], "normal");
    // Through `values` too, and the same word again changes nothing.
    run(&mut s, "component.metadata.set", json!({"component": "Gearbox", "values": {"bom_type": "phantom"}}));
    let again = run(&mut s, "component.metadata.set", json!({"component": "Gearbox", "bom_type": "phantom"}));
    assert_eq!(again["changed"], json!([]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_bom_types_are_refused_without_a_change() {
    let mut s = machine();
    let undo = s.undo.len();
    for p in [
        json!({"component": "Gearbox", "bom_type": "virtual"}),
        json!({"component": "Gearbox", "bom_type": "Phantom"}),
        json!({"component": "Gearbox", "bom_type": 3}),
        json!({"component": "Gearbox", "bom_type": ["phantom"]}),
        json!({"component": "Gearbox", "bom_type": "x".repeat(5000)}),
        json!({"component": "Gearbox", "values": {"bom_type": "phantom\u{7}"}}),
        json!({"component": 0, "bom_type": "phantom"}),
        json!({"component": "Gearbox", "part_number": "GB-2", "bom_type": "nope"}),
    ] {
        assert!(s.execute("component.metadata.set", &p).is_err(), "{p}");
    }
    assert!(s.execute("document.metadata.set", &json!({"key": "bom_type", "value": "phantom"})).is_err(), "not a design property");
    assert!(s.execute("document.metadata.set", &json!({"values": {"bom_type": "purchased"}})).is_err());
    assert_eq!(s.undo.len(), undo, "nothing changed");
    let gb = run(&mut s, "component.metadata", json!({"component": "Gearbox"}));
    assert_eq!((gb["part_number"].as_str(), gb["bom_type"].as_str()), (Some("GB-1"), Some("normal")), "all or nothing");
    // A word another build wrote is kept and reads as normal.
    let id = s.doc.find_component("Gearbox").unwrap();
    if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == id) {
        c.metadata.0.insert("bom_type".into(), json!("kit"));
    }
    assert_eq!(run(&mut s, "component.metadata", json!({"component": "Gearbox"}))["bom_type"], "normal");
    assert_eq!(typed(&run(&mut s, "assembly.bom", json!({"levels": "top"}))), [row("GB-1", "normal", 1), row("PL-1", "normal", 1)]);
}

/// Four brackets, BR-10, placed in the design: Bracket:1 … Bracket:4.
fn brackets() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Bracket"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 3}));
    run(&mut s, "component.activate", json!({"component": 0}));
    for x in [50, 100, 150] {
        run(&mut s, "occurrence.copy", json!({"component": "Bracket", "translate": [x, 0, 0]}));
    }
    run(&mut s, "component.metadata.set", json!({"component": "Bracket", "part_number": "BR-10"}));
    s
}

fn set_occurrence(s: &mut Session, occurrence: &str, bom_type: Value) -> Value {
    run(s, "occurrence.bom_type", json!({"occurrence": occurrence, "bom_type": bom_type}))
}

fn occurrence_type(s: &Session, name: &str) -> Option<solidcraft_doc::BomType> {
    s.doc.occurrences.iter().find(|o| o.name == name).and_then(|o| o.bom_type)
}

#[test]
fn one_occurrence_counts_as_its_own_type_in_rows_of_its_own() {
    let mut s = brackets();
    let flat = |s: &mut Session| typed(&run(s, "assembly.bom", json!({})));
    assert_eq!(flat(&mut s), [row("BR-10", "normal", 4)]);
    // One bracket kept only for context: three are counted.
    let r = set_occurrence(&mut s, "Bracket:2", json!("reference"));
    assert_eq!((r["bom_type"].as_str(), r["effective"].as_str(), r["changed"].as_bool()), (Some("reference"), Some("reference"), Some(true)));
    assert_eq!(flat(&mut s), [row("BR-10", "normal", 3)]);
    assert_eq!(run(&mut s, "assembly.bom", json!({}))["total_occurrences"], 3);
    // One bought instead of made: a row of its own, in both views.
    set_occurrence(&mut s, "Bracket:3", json!("purchased"));
    assert_eq!(flat(&mut s), [row("BR-10", "normal", 2), row("BR-10", "purchased", 1)]);
    assert_eq!(typed(&run(&mut s, "assembly.bom", json!({"levels": "top"}))), [row("BR-10", "normal", 2), row("BR-10", "purchased", 1)]);
    // The component bought: occurrences without a type of their own follow it, the others keep theirs.
    set_type(&mut s, "Bracket", "purchased");
    assert_eq!(flat(&mut s), [row("BR-10", "purchased", 3)]);
    assert_eq!(set_occurrence(&mut s, "Bracket:4", json!("normal"))["effective"], "normal");
    assert_eq!(flat(&mut s), [row("BR-10", "normal", 1), row("BR-10", "purchased", 2)]);
    // As its component again, by the word or by null.
    let r = set_occurrence(&mut s, "Bracket:4", json!("same_as_component"));
    assert_eq!((r["bom_type"].as_str(), r["effective"].as_str()), (Some("same_as_component"), Some("purchased")));
    set_occurrence(&mut s, "Bracket:2", Value::Null);
    assert_eq!(flat(&mut s), [row("BR-10", "purchased", 4)]);
    assert_eq!(set_occurrence(&mut s, "Bracket:1", Value::Null)["changed"], false, "nothing to clear");
}

#[test]
fn an_occurrence_type_inside_a_subassembly_holds_wherever_it_is_placed() {
    let mut s = machine();
    run(&mut s, "occurrence.copy", json!({"component": "Gearbox", "translate": [0, 100, 0]}));
    let top = |s: &mut Session| typed(&run(s, "assembly.bom", json!({"levels": "top"})));
    let flat = |s: &mut Session| typed(&run(s, "assembly.bom", json!({})));
    assert_eq!(top(&mut s), [row("GB-1", "normal", 2), row("PL-1", "normal", 1)]);
    assert_eq!(flat(&mut s), [row("BR-1", "normal", 2), row("PL-1", "normal", 1), row("SH-1", "normal", 2)]);
    // The second gearbox only groups its parts: they count at the top level, the first stays a row.
    set_occurrence(&mut s, "Gearbox:2", json!("phantom"));
    assert_eq!(top(&mut s), [row("BR-1", "normal", 1), row("GB-1", "normal", 1), row("PL-1", "normal", 1), row("SH-1", "normal", 1)]);
    assert_eq!(flat(&mut s), [row("BR-1", "normal", 2), row("PL-1", "normal", 1), row("SH-1", "normal", 2)], "parts only is the same");
    // Bought whole instead: its own row, with nothing inside it listed.
    set_occurrence(&mut s, "Gearbox:2", json!("purchased"));
    assert_eq!(top(&mut s), [row("GB-1", "normal", 1), row("GB-1", "purchased", 1), row("PL-1", "normal", 1)]);
    assert_eq!(flat(&mut s), [row("BR-1", "normal", 1), row("GB-1", "purchased", 1), row("PL-1", "normal", 1), row("SH-1", "normal", 1)]);
    // The bearing's occurrence belongs to the gearbox's definition: kept for reference, it leaves every
    // gearbox, and the gearbox's own list.
    set_occurrence(&mut s, "Bearing:1", json!("reference"));
    assert_eq!(flat(&mut s), [row("GB-1", "purchased", 1), row("PL-1", "normal", 1), row("SH-1", "normal", 1)]);
    assert_eq!(typed(&run(&mut s, "assembly.bom", json!({"component": "Gearbox"}))), [row("SH-1", "normal", 1)]);
}

#[test]
fn an_occurrence_type_is_one_undo_step_is_saved_and_evaluates_nothing() {
    let dir = scratch("occurrence-type");
    let mut s = machine();
    let undo = s.undo.len();
    s.evaluations = 0;
    assert_eq!(set_occurrence(&mut s, "Gearbox:1", json!("purchased"))["changed"], true);
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(set_occurrence(&mut s, "Gearbox:1", json!("purchased"))["changed"], false);
    assert_eq!(s.undo.len(), undo + 1, "the same type again is no step");
    assert_eq!(s.evaluations, 0, "a bill of materials type changes no geometry");
    let list = run(&mut s, "component.list", json!({"compact": true}));
    assert!(list.to_string().contains(r#""bom_type":"purchased""#), "{list}");
    for (file, format) in [("machine.json", "json"), ("machine.solidcraft", "container")] {
        let path = dir.join(file);
        run(&mut s, "file.save_as", json!({"path": path, "format": format}));
        let mut back = Session::default();
        run(&mut back, "doc.open", json!({"path": path}));
        assert_eq!(occurrence_type(&back, "Gearbox:1"), Some(solidcraft_doc::BomType::Purchased), "{format}");
        assert_eq!(typed(&run(&mut back, "assembly.bom", json!({}))), typed(&run(&mut s, "assembly.bom", json!({}))), "{format}");
    }
    // In the file: on that occurrence only, with the change listed as pending.
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("machine.json")).unwrap()).unwrap();
    let occurrences = saved["occurrences"].as_array().unwrap();
    let with_type: Vec<&str> = occurrences.iter().filter(|o| o.get("bom_type").is_some()).map(|o| o["name"].as_str().unwrap()).collect();
    assert_eq!(with_type, ["Gearbox:1"]);
    assert!(saved["format_pending"].as_array().unwrap().contains(&json!("occurrence-bom-type")), "{}", saved["format_pending"]);
    // A damaged word is refused as the design opens, never read as another type.
    let mut damaged = saved.clone();
    let i = occurrences.iter().position(|o| o.get("bom_type").is_some()).unwrap();
    damaged["occurrences"][i]["bom_type"] = json!("kit");
    let path = dir.join("damaged.json");
    std::fs::write(&path, damaged.to_string()).unwrap();
    assert!(Session::default().execute("doc.open", &json!({"path": path})).is_err());
    // Saving evaluates once, whatever changed; undo and redo of the type evaluate nothing.
    s.evaluations = 0;
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(occurrence_type(&s, "Gearbox:1"), None);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(occurrence_type(&s, "Gearbox:1"), Some(solidcraft_doc::BomType::Purchased));
    assert_eq!(s.evaluations, 0, "nor do undo and redo");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_purchased_occurrence_of_a_kit_part_is_listed_without_its_kit() {
    let mut s = bolts();
    // The first bolt (with its washer and nut) placed twice; the second placement bought whole.
    let first = run(&mut s, "assembly.bom", json!({"group_children": true}))["rows"][0]["components"][0].as_u64().unwrap();
    let copy = run(&mut s, "occurrence.copy", json!({"component": first, "translate": [0, 50, 0]}));
    run(&mut s, "occurrence.bom_type", json!({"occurrence": copy["occurrence"], "bom_type": "purchased"}));
    let r = run(&mut s, "assembly.bom", json!({"group_children": true}));
    let rows: Vec<(&str, &str, u64, usize)> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let kit = r.get("children").and_then(Value::as_array).map_or(0, Vec::len);
            (r["part_number"].as_str().unwrap(), r["bom_type"].as_str().unwrap(), r["count"].as_u64().unwrap(), kit)
        })
        .collect();
    assert!(rows.contains(&("ISO 4017 M6x30", "normal", 2, 2)), "the two made bolts with their nut and washer: {rows:?}");
    assert!(rows.contains(&("ISO 4017 M6x30", "purchased", 1, 0)), "the bought one alone: {rows:?}");
}

#[test]
fn paste_new_keeps_occurrence_types_and_paste_instance_counts_as_the_component() {
    let mut s = machine();
    set_occurrence(&mut s, "Gearbox:1", json!("purchased"));
    set_occurrence(&mut s, "Bearing:1", json!("reference"));
    // Another placement of the same gearbox counts as its component.
    let copy = run(&mut s, "occurrence.copy", json!({"occurrence": "Gearbox:1", "translate": [0, 100, 0]}));
    let placed = s.doc.occurrences.iter().find(|o| Some(o.id) == copy["occurrence"].as_u64()).unwrap();
    assert_eq!(placed.bom_type, None);
    // An independent copy keeps its source occurrence's type, and the types inside it.
    let before: std::collections::HashSet<u64> = s.doc.occurrences.iter().map(|o| o.id).collect();
    run(&mut s, "component.paste_new", json!({"component": "Gearbox", "occurrence": "Gearbox:1", "translate": [0, -100, 0]}));
    let mut added: Vec<Option<solidcraft_doc::BomType>> = s.doc.occurrences.iter().filter(|o| !before.contains(&o.id)).map(|o| o.bom_type).collect();
    added.sort();
    use solidcraft_doc::BomType::{Purchased, Reference};
    assert_eq!(added, [None, Some(Reference), Some(Purchased)], "the shaft, the bearing and the gearbox copied");
}

#[test]
fn hostile_occurrence_types_are_refused_without_a_change() {
    let mut s = machine();
    let undo = s.undo.len();
    for p in [
        json!({"occurrence": "Gearbox:1", "bom_type": "virtual"}),
        json!({"occurrence": "Gearbox:1", "bom_type": "Purchased"}),
        json!({"occurrence": "Gearbox:1", "bom_type": "Same_As_Component"}),
        json!({"occurrence": "Gearbox:1", "bom_type": ""}),
        json!({"occurrence": "Gearbox:1", "bom_type": 3}),
        json!({"occurrence": "Gearbox:1", "bom_type": ["purchased"]}),
        json!({"occurrence": "Gearbox:1", "bom_type": {"type": "purchased"}}),
        json!({"occurrence": "Gearbox:1"}),
        json!({"occurrence": "Gearbox:9", "bom_type": "purchased"}),
        json!({"occurrence": 999_999, "bom_type": "purchased"}),
        json!({"occurrence": -1, "bom_type": "purchased"}),
        json!({"occurrence": true, "bom_type": "purchased"}),
        json!({"bom_type": "purchased"}),
        json!({"occurrence": "Gearbox:1", "path": [], "bom_type": "purchased"}),
        json!({"occurrence": "Gearbox:1", "path": "x", "bom_type": "purchased"}),
        json!({"occurrence": "Gearbox:1", "path": [1.5], "bom_type": "purchased"}),
        json!({"occurrence": "Gearbox:1", "path": [999_999], "bom_type": "purchased"}),
    ] {
        assert!(s.execute("occurrence.bom_type", &p).is_err(), "{p}");
    }
    let long = s.execute("occurrence.bom_type", &json!({"occurrence": "Gearbox:1", "bom_type": "x".repeat(5000)}));
    let message = long.unwrap_err().to_string();
    assert!(message.len() < 300, "the word isn't echoed back: {} bytes", message.len());
    assert_eq!(s.undo.len(), undo, "nothing changed");
    assert!(s.doc.occurrences.iter().all(|o| o.bom_type.is_none()));
    // The exact placement, when given, is accepted.
    let id = s.doc.occurrences.iter().find(|o| o.name == "Gearbox:1").unwrap().id;
    run(&mut s, "occurrence.bom_type", json!({"occurrence": id, "path": [id], "bom_type": "inseparable"}));
    assert_eq!(occurrence_type(&s, "Gearbox:1"), Some(solidcraft_doc::BomType::Inseparable));
}

/// A frame (no bodies of its own) holding two steel blocks, 100 x 50 x 10 (50,000 mm³ at
/// 7.85 g/cm³: 392.5 g each), and a bracket: an aluminium plate, 40 x 20 x 3 (2,400 mm³ at
/// 2.70: 6.48 g), and a steel cube of 10 (7.85 g), 14.33 g in all; 799.33 g together. A second
/// bracket, a spacer with no material assigned (a cube of 10 at the default 1.29: 1.29 g) and
/// grease (no bodies) are placed in the design.
fn frame() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Frame", "activate": false}));
    run(&mut s, "component.create", json!({"name": "Block", "parent": "Frame"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 10, "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.copy", json!({"component": "Block", "parent": "Frame", "translate": [0, 60, 0]}));
    run(&mut s, "component.create", json!({"name": "Bracket", "parent": "Frame"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 3, "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "operation": "new", "body_name": "Cube"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.copy", json!({"component": "Bracket", "parent": 0, "translate": [0, -60, 0]}));
    run(&mut s, "component.create", json!({"name": "Spacer"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Spacer"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Grease", "activate": false}));
    for (body, material) in [("Block", "Steel"), ("Plate", "Aluminum"), ("Cube", "Steel")] {
        run(&mut s, "material.assign", json!({"bodies": [body], "material": material}));
    }
    s
}

/// A row's mass, to the microgram (`None`: null).
fn grams(row: &Value) -> Option<f64> {
    row["mass_g"].as_f64().map(|g| (g * 1e6).round() / 1e6)
}

/// Each row's definition, material and mass.
fn weights(r: &Value) -> Vec<(String, String, Option<f64>)> {
    let rows = r["rows"].as_array().or_else(|| r.as_array()).unwrap();
    rows.iter().map(|r| (r["definition"].as_str().unwrap().into(), r["material"].as_str().unwrap().into(), grams(r))).collect()
}

fn weight(definition: &str, material: &str, grams: Option<f64>) -> (String, String, Option<f64>) {
    (definition.into(), material.into(), grams)
}

#[test]
fn rows_name_their_materials_and_with_mass_weigh_one_part() {
    let mut s = frame();
    let flat = run(&mut s, "assembly.bom", json!({"mass": true}));
    assert_eq!(
        weights(&flat),
        [
            weight("Block", "Steel", Some(392.5)),
            weight("Bracket", "Aluminum; Steel", Some(14.33)),
            weight("Grease", "", None),
            weight("Spacer", "Default", Some(1.29)),
        ],
        "one of each, whatever the count: {flat}"
    );
    // The frame as one row stands for its steel and aluminium parts: no one material.
    let top = run(&mut s, "assembly.bom", json!({"levels": "top", "mass": true}));
    assert_eq!(weights(&top)[1], weight("Frame", "", Some(799.33)), "{top}");
    // The assembly as a whole: the frame, the second bracket and the spacer, weighed from the design.
    let total = |r: &Value| r["total_mass_g"].as_f64().map(|g| (g * 1e6).round() / 1e6);
    assert_eq!((total(&flat), total(&top)), (Some(814.95), Some(814.95)), "{top}");
    // Weighing measures every part, so it is asked for; materials come without it.
    let plain = run(&mut s, "assembly.bom", json!({}));
    assert_eq!((&plain["mass"], &plain["rows"][1]["material"]), (&json!(false), &json!("Aluminum; Steel")));
    assert!(plain.get("total_mass_g").is_none(), "{plain}");
    assert!(plain["rows"].as_array().unwrap().iter().all(|r| r.get("mass_g").is_none()), "{plain}");
    // Parts folded into a numbered frame's row carry theirs too.
    run(&mut s, "component.metadata.set", json!({"component": "Frame", "part_number": "FR-1"}));
    let kit = run(&mut s, "assembly.bom", json!({"group_children": true, "mass": true}));
    let frame = &kit["rows"][0];
    assert_eq!((frame["part_number"].as_str(), grams(frame)), (Some("FR-1"), Some(799.33)), "{kit}");
    assert_eq!(weights(&frame["children"]), [weight("Block", "Steel", Some(392.5)), weight("Bracket", "Aluminum; Steel", Some(14.33))]);
}

#[test]
fn a_reference_placement_adds_nothing_to_what_its_assembly_weighs() {
    let mut s = frame();
    let frame = |s: &mut Session| {
        let top = run(s, "assembly.bom", json!({"levels": "top", "mass": true}));
        top["rows"].as_array().unwrap().iter().find(|r| r["definition"] == "Frame").and_then(grams)
    };
    assert_eq!(frame(&mut s), Some(799.33));
    // One block kept only for context.
    set_occurrence(&mut s, "Block:2", json!("reference"));
    assert_eq!(frame(&mut s), Some(406.83));
    // Phantom, purchased and inseparable parts weigh what they weigh, wherever they are listed.
    for bom_type in ["phantom", "purchased", "inseparable"] {
        set_type(&mut s, "Bracket", bom_type);
        assert_eq!(frame(&mut s), Some(406.83), "{bom_type}");
    }
    set_type(&mut s, "Bracket", "reference");
    assert_eq!(frame(&mut s), Some(392.5));
    // Holding only reference parts, it has nothing to weigh.
    set_occurrence(&mut s, "Block:1", json!("reference"));
    assert_eq!(frame(&mut s), None);
}

/// Every list's rows add up to the assembly once: in parts only, what a part holds is listed
/// beside it, so it isn't weighed again with the part.
#[test]
fn the_rows_of_every_list_add_up_to_the_assembly() {
    let (flat, top, folded) = (json!({"mass": true}), json!({"levels": "top", "mass": true}), json!({"group_children": true, "mass": true}));
    let check = |s: &mut Session, what: &str, lists: &[&Value]| {
        let assembly = run(s, "component.properties", json!({"component": "root"}))["total"]["mass_g"].as_f64().unwrap();
        for &p in lists {
            let r = run(s, "assembly.bom", p.clone());
            let rows = r["rows"].as_array().unwrap();
            let listed: f64 = rows.iter().map(|row| row["count"].as_f64().unwrap() * row["mass_g"].as_f64().unwrap_or(0.0)).sum();
            assert!((listed - assembly).abs() < 1e-6 * assembly, "{what} {p}: {listed} g listed, {assembly} g in all: {r}");
        }
    };
    // Bolts with a washer and a nut placed inside them, and one alone. In parts only the bolts'
    // row weighs one bolt, and both kinds agree; folded, a bolt's row weighs the kit.
    let mut s = bolts();
    check(&mut s, "bolts", &[&flat, &folded]);
    let bolt = run(&mut s, "assembly.bom", flat.clone())["rows"][0]["mass_g"].as_f64().unwrap();
    let kit = run(&mut s, "assembly.bom", folded.clone())["rows"][0]["mass_g"].as_f64().unwrap();
    assert!(kit > bolt, "a bolt with its washer and nut ({kit} g) weighs more than the bolt ({bolt} g)");
    // At the top level one row holds the bolts with and without a washer and nut: they weigh
    // differently, so the row has no one mass.
    assert!(run(&mut s, "assembly.bom", top.clone())["rows"][0]["mass_g"].is_null());
    // An inseparable frame holding purchased brackets: in parts only, the frame weighs all but
    // the brackets, which are listed beside it.
    let mut s = frame();
    check(&mut s, "frame", &[&flat, &top, &folded]);
    set_type(&mut s, "Frame", "inseparable");
    set_type(&mut s, "Bracket", "purchased");
    check(&mut s, "an inseparable frame with purchased brackets", &[&flat, &top, &folded]);
    let frame = run(&mut s, "assembly.bom", json!({"mass": true}));
    // Its steel blocks: the aluminium brackets are rows beside it.
    assert_eq!(weights(&frame)[1], weight("Frame", "Steel", Some(785.0)), "{frame}");
    // A placement kept for reference adds nothing to the assembly either.
    set_occurrence(&mut s, "Block:2", json!("reference"));
    check(&mut s, "a block kept for reference", &[&flat, &top, &folded]);
}

/// A phantom's own bodies are a row of their own, its type's, as a subassembly's are in parts
/// only: they are counted in no other row, so without it the rows wouldn't add up.
#[test]
fn a_phantoms_own_bodies_are_a_row_of_their_own() {
    let mut s = phantom_kit("");
    let assembly = run(&mut s, "component.properties", json!({"component": "root"}))["total"]["mass_g"].as_f64().unwrap();
    for levels in ["flat", "top", "all"] {
        let r = run(&mut s, "assembly.bom", json!({"levels": levels, "mass": true}));
        assert_eq!(typed(&r), [row("", "phantom", 1), row("", "normal", 1)], "{levels}: {r}");
        assert_eq!(weights(&r), [weight("Kit", "Steel", Some(7.85)), weight("Part", "Aluminum", Some(6.48))], "{levels}");
        let listed: f64 = r["rows"].as_array().unwrap().iter().map(|row| row["mass_g"].as_f64().unwrap()).sum();
        assert!((listed - assembly).abs() < 1e-6 * assembly, "{levels}: {listed} g listed, {assembly} g in all");
    }
}

/// A phantom kit with a steel cube of 10 of its own (7.85 g), holding a part: an aluminium plate,
/// 40 x 20 x 3 (6.48 g). Placed in `parent`, an assembly made for it, or the design when empty.
fn phantom_kit(parent: &str) -> Session {
    let mut s = Session::default();
    let mut kit = json!({"name": "Kit"});
    if !parent.is_empty() {
        run(&mut s, "component.create", json!({"name": parent, "activate": false}));
        kit["parent"] = json!(parent);
    }
    run(&mut s, "component.create", kit);
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    run(&mut s, "component.create", json!({"name": "Part", "parent": "Kit"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 3, "body_name": "Plate"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    for (body, material) in [("Cube", "Steel"), ("Plate", "Aluminum")] {
        run(&mut s, "material.assign", json!({"bodies": [body], "material": material}));
    }
    set_type(&mut s, "Kit", "phantom");
    s
}

/// Each row's definition, material, mass and static values, the rows under it next.
fn statics(r: &Value) -> Vec<(String, String, Option<f64>, Value)> {
    let rows = r["rows"].as_array().unwrap().iter().flat_map(|r| std::iter::once(r).chain(r["children"].as_array().into_iter().flatten()));
    rows.map(|r| {
        (r["definition"].as_str().unwrap().into(), r["material"].as_str().unwrap().into(), grams(r), r.get("static").cloned().unwrap_or_default())
    })
    .collect()
}

fn typed_in(definition: &str, material: &str, grams: Option<f64>) -> (String, String, Option<f64>, Value) {
    (definition.into(), material.into(), grams, json!(["material"]))
}

fn worked_out(definition: &str, material: &str, grams: Option<f64>) -> (String, String, Option<f64>, Value) {
    (definition.into(), material.into(), grams, Value::Null)
}

/// A static material is what a component's rows say in every view and at every level, over its
/// bodies' materials or an assembly's blank; it doesn't change what anything weighs, and clearing it
/// brings the worked-out material back.
#[test]
fn a_static_material_is_what_every_list_says_until_it_is_cleared() {
    let mut s = frame();
    run(&mut s, "component.metadata.set", json!({"component": "Frame", "part_number": "FR-1"}));
    let undo = s.undo.len();
    s.evaluations = 0;
    let r = run(&mut s, "component.metadata.set", json!({"component": "Block", "material": "S355 flat bar"}));
    assert_eq!(r["changed"], json!(["material"]));
    assert_eq!((s.undo.len(), s.evaluations), (undo + 1, 0), "one undo step, no geometry");
    run(&mut s, "component.metadata.set", json!({"component": "Frame", "values": {"material": "Welded frame"}}));
    assert_eq!(run(&mut s, "component.metadata", json!({"component": "Frame"}))["material"], "Welded frame");
    let bom = |s: &mut Session, p: Value| statics(&run(s, "assembly.bom", p));
    // Parts only: the block over its own steel; the frame isn't a row.
    let block = typed_in("Block", "S355 flat bar", Some(392.5));
    let bracket = worked_out("Bracket", "Aluminum; Steel", Some(14.33));
    let spacer = worked_out("Spacer", "Default", Some(1.29));
    let grease = worked_out("Grease", "", None);
    assert_eq!(bom(&mut s, json!({"mass": true})), [block.clone(), bracket.clone(), grease.clone(), spacer.clone()]);
    // Structured: over the frame's blank (steel and aluminium); folded, the block under it too.
    let frame = typed_in("Frame", "Welded frame", Some(799.33));
    assert_eq!(bom(&mut s, json!({"levels": "top", "mass": true})), [frame.clone(), bracket.clone(), grease.clone(), spacer.clone()]);
    let folded = bom(&mut s, json!({"group_children": true, "mass": true}));
    assert_eq!(folded, [frame.clone(), block.clone(), bracket.clone(), bracket.clone(), grease.clone(), spacer.clone()]);
    // All levels: the frame, and under it the block.
    let all = bom(&mut s, json!({"levels": "all", "mass": true}));
    assert_eq!(all, [frame.clone(), block.clone(), bracket.clone(), bracket.clone(), grease.clone(), spacer.clone()]);
    // The JSON and CSV files show what the rows show.
    let dir = scratch("static-material");
    let path = dir.join("bom.json");
    run(&mut s, "assembly.bom", json!({"levels": "all", "mass": true, "path": path}));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(statics(&file), all);
    let path = dir.join("bom.csv");
    run(&mut s, "assembly.bom", json!({"levels": "all", "path": path}));
    let csv = std::fs::read_to_string(&path).unwrap();
    let column: Vec<&str> = csv.lines().skip(1).filter_map(|l| l.rsplit(",\"").next()).map(|m| m.trim_end_matches('"')).collect();
    assert_eq!(column, ["Welded frame", "S355 flat bar", "Aluminum; Steel", "Aluminum; Steel", "", "Default"], "{csv}");
    // Saved with the component and read back.
    let path = dir.join("frame.solidcraft");
    run(&mut s, "file.save_as", json!({"path": path, "format": "container"}));
    let mut back = Session::default();
    run(&mut back, "doc.open", json!({"path": path}));
    assert_eq!(bom(&mut back, json!({"levels": "all", "mass": true})), all);
    // Cleared, by empty text or null: the worked-out materials again.
    run(&mut s, "component.metadata.set", json!({"component": "Block", "material": ""}));
    run(&mut s, "component.metadata.set", json!({"component": "Frame", "material": null}));
    assert_eq!(bom(&mut s, json!({"levels": "top"}))[0], worked_out("Frame", "", None));
    assert_eq!(bom(&mut s, json!({}))[0], worked_out("Block", "Steel", None));
    // A static material of the longest text a property takes.
    let longest = "m".repeat(solidcraft_doc::metadata::MAX_TEXT_BYTES);
    run(&mut s, "component.metadata.set", json!({"component": "Spacer", "material": longest}));
    assert_eq!(bom(&mut s, json!({}))[3], typed_in("Spacer", &longest, None));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A phantom's static material is its own bodies' row's (it has no other); what it holds keeps its
/// own. A mesh part takes one too, and still weighs (or can't be weighed) as its mesh does.
#[test]
fn a_static_material_names_a_phantoms_own_row_and_a_mesh_part() {
    let mut s = phantom_kit("");
    run(&mut s, "component.metadata.set", json!({"component": "Kit", "material": "Cast iron"}));
    for levels in ["flat", "top", "all"] {
        let r = run(&mut s, "assembly.bom", json!({"levels": levels, "mass": true}));
        assert_eq!(statics(&r), [typed_in("Kit", "Cast iron", Some(7.85)), worked_out("Part", "Aluminum", Some(6.48))], "{levels}");
    }
    run(&mut s, "component.metadata.set", json!({"component": "Part", "material": "6082-T6"}));
    assert_eq!(statics(&run(&mut s, "assembly.bom", json!({"mass": true})))[1], typed_in("Part", "6082-T6", Some(6.48)));

    let dir = scratch("static-mesh");
    // A 10 mm square at z = 10 facing down: open, it measures inside out, so its mass is unknown.
    let facet = |t: [[i32; 2]; 3]| {
        let v: String = t.iter().map(|[x, y]| format!("vertex {x} {y} 10\n")).collect();
        format!("facet normal 0 0 0\nouter loop\n{v}endloop\nendfacet\n")
    };
    let stl = format!("solid square\n{}{}endsolid square\n", facet([[0, 0], [0, 10], [10, 10]]), facet([[0, 0], [10, 10], [10, 0]]));
    let path = dir.join("Mesh.stl");
    std::fs::write(&path, stl).unwrap();
    let mut s = Session::default();
    run(&mut s, "file.insert_mesh", json!({"path": path}));
    run(&mut s, "component.metadata.set", json!({"component": "Mesh", "material": "PA12"}));
    assert_eq!(statics(&run(&mut s, "assembly.bom", json!({"mass": true}))), [typed_in("Mesh", "PA12", None)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A static material stands for its part in what an assembly around it shares, so the bill of
/// materials says one thing about the part everywhere; a phantom's names its own bodies only.
#[test]
fn a_static_material_stands_for_its_part_in_what_an_assembly_shares() {
    let top = |s: &mut Session, definition: &str| -> String {
        let r = run(s, "assembly.bom", json!({"levels": "top"}));
        let row = r["rows"].as_array().unwrap().iter().find(|r| r["definition"] == definition).cloned();
        row.and_then(|r| r["material"].as_str().map(str::to_string)).unwrap_or_else(|| panic!("no {definition} row: {r}"))
    };
    let mut s = frame();
    // Steel blocks and a bracket of aluminium and steel: no one material.
    assert_eq!(top(&mut s, "Frame"), "");
    for part in ["Block", "Bracket"] {
        run(&mut s, "component.metadata.set", json!({"component": part, "material": "S355"}));
    }
    assert_eq!(top(&mut s, "Frame"), "S355", "both parts say S355");
    // A bracket of plain steel beside S355 blocks: two materials.
    run(&mut s, "component.metadata.set", json!({"component": "Bracket", "material": null}));
    run(&mut s, "material.assign", json!({"bodies": ["Plate"], "material": "Steel"}));
    assert_eq!(top(&mut s, "Frame"), "");
    run(&mut s, "component.metadata.set", json!({"component": "Block", "material": null}));
    assert_eq!(top(&mut s, "Frame"), "Steel", "the worked-out materials again");

    // A crate holding a phantom kit (its own steel cube, holding an aluminium plate).
    let mut s = phantom_kit("Crate");
    run(&mut s, "component.metadata.set", json!({"component": "Kit", "material": "Cast iron"}));
    assert_eq!(top(&mut s, "Crate"), "", "the kit's cube is cast iron, its plate still aluminium");
    run(&mut s, "component.metadata.set", json!({"component": "Part", "material": "Cast iron"}));
    assert_eq!(top(&mut s, "Crate"), "Cast iron");

    // A frame of a steel block and an aluminium plate, inside a pallet: its static material stands
    // for all it holds.
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Pallet", "activate": false}));
    run(&mut s, "component.create", json!({"name": "Frame", "parent": "Pallet", "activate": false}));
    for (part, length) in [("Block", 100), ("Plate", 40)] {
        run(&mut s, "component.create", json!({"name": part, "parent": "Frame"}));
        run(&mut s, "solid.box", json!({"length": length, "width": 20, "height": 3, "body_name": part}));
        run(&mut s, "component.activate", json!({"component": 0}));
    }
    for (body, material) in [("Block", "Steel"), ("Plate", "Aluminum")] {
        run(&mut s, "material.assign", json!({"bodies": [body], "material": material}));
    }
    assert_eq!(top(&mut s, "Pallet"), "");
    run(&mut s, "component.metadata.set", json!({"component": "Frame", "material": "Welded frame"}));
    assert_eq!(top(&mut s, "Pallet"), "Welded frame");
}

#[test]
fn hostile_static_materials_are_refused_without_a_change() {
    let mut s = frame();
    run(&mut s, "component.metadata.set", json!({"component": "Block", "material": "Steel bar"}));
    let undo = s.undo.len();
    let control = format!("Steel{}", char::from(7));
    for p in [
        json!({"component": "Block", "material": "x".repeat(solidcraft_doc::metadata::MAX_TEXT_BYTES + 1)}),
        json!({"component": "Block", "material": 7.85}),
        json!({"component": "Block", "material": {"name": "Steel"}}),
        json!({"component": "Block", "material": ["Steel"]}),
        json!({"component": "Block", "material": true}),
        json!({"component": "Block", "values": {"material": control}}),
        json!({"component": 0, "material": "Steel"}),
        json!({"component": "Block", "part_number": "BL-1", "material": 1}),
    ] {
        assert!(s.execute("component.metadata.set", &p).is_err(), "{p}");
    }
    assert!(s.execute("document.metadata.set", &json!({"key": "material", "value": "Steel"})).is_err(), "not a design property");
    assert!(s.execute("document.metadata.set", &json!({"values": {"material": "Steel"}})).is_err());
    assert_eq!(s.undo.len(), undo, "nothing changed");
    let block = run(&mut s, "component.metadata", json!({"component": "Block"}));
    assert_eq!((block["part_number"].as_str(), block["material"].as_str()), (None, Some("Steel bar")), "all or nothing");
    // Another tool's number is kept, and the row reads its bodies' material.
    let id = s.doc.find_component("Block").unwrap();
    if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == id) {
        c.metadata.0.insert("material".into(), json!(42));
    }
    assert_eq!(run(&mut s, "component.metadata", json!({"component": "Block"}))["material"], Value::Null);
    assert_eq!(statics(&run(&mut s, "assembly.bom", json!({})))[0], worked_out("Block", "Steel", None));
}

/// A row gathering several definitions (one part number) says the static material of the first
/// that has one, as it does their description.
#[test]
fn a_row_of_several_definitions_takes_the_first_static_material() {
    let mut s = frame();
    run(&mut s, "component.metadata.set", json!({"component": "Spacer", "part_number": "SP-1"}));
    run(&mut s, "component.paste_new", json!({"component": "Spacer", "translate": [0, -30, 0]}));
    let copy = s.doc.components.iter().map(|c| c.id).max().unwrap();
    let spacer = |s: &mut Session| {
        let r = run(s, "assembly.bom", json!({}));
        let row = r["rows"].as_array().unwrap().iter().find(|r| r["part_number"] == "SP-1").cloned().unwrap();
        assert_eq!(row["components"].as_array().map(Vec::len), Some(2), "{row}");
        (row["material"].as_str().unwrap().to_string(), row.get("static").cloned())
    };
    run(&mut s, "component.metadata.set", json!({"component": copy, "material": "Nylon"}));
    assert_eq!(spacer(&mut s), ("Nylon".into(), Some(json!(["material"]))), "the copy's, the only one");
    run(&mut s, "component.metadata.set", json!({"component": "Spacer", "material": "PTFE"}));
    assert_eq!(spacer(&mut s), ("PTFE".into(), Some(json!(["material"]))), "the first definition's");
}

/// The bill of materials and the physical properties weigh a body from one measurement, kept with
/// the evaluated model: the same grams to the bit, and asking again measures nothing new.
#[test]
fn the_bill_of_materials_and_the_physical_properties_weigh_one_measurement() {
    let mut s = frame();
    let bom = run(&mut s, "assembly.bom", json!({"mass": true}));
    let block = bom["rows"][0]["mass_g"].as_f64().unwrap();
    let properties = run(&mut s, "component.properties", json!({"component": "Block"}))["total"]["mass_g"].as_f64().unwrap();
    assert_eq!(block.to_bits(), properties.to_bits(), "{block} g listed, {properties} g measured");
    // A pin placed far from the origin, where measuring the placed body again would move the
    // last bits: its placement weighs what its row does.
    run(&mut s, "component.create", json!({"name": "Pin"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 7, "height": 31, "body_name": "Pin"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let far = run(&mut s, "occurrence.copy", json!({"component": "Pin", "parent": 0, "translate": [1000, 700, 300]}))["occurrence"].clone();
    let bom = run(&mut s, "assembly.bom", json!({"mass": true}));
    let pin = bom["rows"].as_array().unwrap().iter().find(|r| r["definition"] == "Pin").and_then(|r| r["mass_g"].as_f64()).unwrap();
    let placed = run(&mut s, "occurrence.properties", json!({"occurrence": far}))["total"]["mass_g"].as_f64().unwrap();
    assert_eq!(pin.to_bits(), placed.to_bits(), "{pin} g listed, {placed} g measured where it is placed");
    let state = s.model.state();
    let body = state.bodies.iter().find(|b| b.name == "Block").unwrap();
    assert_eq!(body.volume().map(f64::to_bits), body.clone().volume().map(f64::to_bits), "a clone shares the measurement");
}

/// One pin modelled two ways, as a cylinder and as a revolved rectangle: tessellated differently,
/// measured a little apart, still one part with one mass. A pin 0.1% wider is another part.
#[test]
fn equal_parts_modelled_apart_have_one_mass() {
    let mut s = Session::default();
    let pin = |s: &mut Session, name: &str, revolved: bool, diameter: f64| {
        run(s, "component.create", json!({"name": name}));
        if revolved {
            let profile = format!("{name} profile");
            run(s, "sketch.create", json!({"plane": "XZ", "name": profile}));
            run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [diameter / 2.0, 31]}));
            run(s, "sketch.finish", json!({}));
            run(s, "solid.revolve", json!({"sketch": profile, "axis": {"origin": [0, 0, 0], "dir": [0, 0, 1]}, "body_name": name}));
        } else {
            run(s, "solid.cylinder", json!({"diameter": diameter, "height": 31, "body_name": name}));
        }
        run(s, "component.activate", json!({"component": 0}));
        run(s, "component.metadata.set", json!({"component": name, "part_number": "PIN"}));
        run(s, "component.properties", json!({"component": name}))["total"]["mass_g"].as_f64().unwrap()
    };
    let (cylinder, revolved) = (pin(&mut s, "Pin A", false, 7.0), pin(&mut s, "Pin B", true, 7.0));
    let apart = (cylinder - revolved).abs() / cylinder;
    assert!(apart > 0.0 && apart <= bom::MEASURE_ACCURACY, "measured {apart} apart");
    let r = run(&mut s, "assembly.bom", json!({"mass": true}));
    assert_eq!(r["rows"].as_array().map(Vec::len), Some(1), "{r}");
    assert!(r["rows"][0]["mass_g"].as_f64().is_some(), "one mass for both: {r}");
    // 0.2% heavier under the same part number: the row's definitions differ.
    pin(&mut s, "Pin C", false, 7.007);
    let r = run(&mut s, "assembly.bom", json!({"mass": true}));
    assert!(r["rows"][0]["mass_g"].is_null(), "{r}");
}

#[test]
fn one_part_number_has_one_mass_only_when_its_definitions_agree() {
    let mut s = frame();
    run(&mut s, "component.metadata.set", json!({"component": "Block", "part_number": "BL-1"}));
    // An independent copy keeps the part properties and the material: the same part, the same mass.
    run(&mut s, "component.paste_new", json!({"component": "Block", "translate": [0, 200, 0]}));
    let r = run(&mut s, "assembly.bom", json!({"mass": true}));
    let block = &r["rows"][0];
    assert_eq!((block["components"].as_array().map(Vec::len), grams(block)), (Some(2), Some(392.5)), "{r}");
    // The spacer given the same number weighs differently: no one mass for the row.
    run(&mut s, "component.metadata.set", json!({"component": "Spacer", "part_number": "BL-1"}));
    let r = run(&mut s, "assembly.bom", json!({"mass": true}));
    let block = &r["rows"][0];
    assert_eq!((block["components"].as_array().map(Vec::len), grams(block)), (Some(3), None), "{r}");
    assert_eq!(block["material"], "Steel; Default");
}

/// A row that stands for an assembly names a material only when everything it stands for shares it
/// (Cameron, 2026-10-23), whatever kind of row it is; a part lists its own bodies' materials.
#[test]
fn an_assembly_row_names_the_one_material_all_it_stands_for_shares() {
    let mut s = frame();
    let material = |s: &mut Session, p: Value, definition: &str| -> String {
        let r = run(s, "assembly.bom", p);
        let row = r["rows"].as_array().unwrap().iter().find(|r| r["definition"] == definition).cloned();
        row.and_then(|r| r["material"].as_str().map(str::to_string)).unwrap_or_else(|| panic!("no {definition} row: {r}"))
    };
    let (flat, top, folded) = (json!({}), json!({"levels": "top"}), json!({"group_children": true}));
    // Steel blocks and a bracket of aluminium and steel; the bracket, a part, lists both.
    assert_eq!(material(&mut s, top.clone(), "Frame"), "");
    assert_eq!(material(&mut s, flat.clone(), "Bracket"), "Aluminum; Steel");
    // Kept for reference, the brackets aren't part of it: the frame is its steel blocks.
    set_type(&mut s, "Bracket", "reference");
    assert_eq!(material(&mut s, top.clone(), "Frame"), "Steel");
    // Bought as one: the same rule.
    set_type(&mut s, "Bracket", "normal");
    set_type(&mut s, "Frame", "purchased");
    assert_eq!(material(&mut s, flat.clone(), "Frame"), "");
    // Made as one, in parts only: its purchased brackets are listed beside it, each with its own.
    set_type(&mut s, "Frame", "inseparable");
    set_type(&mut s, "Bracket", "purchased");
    assert_eq!(material(&mut s, flat.clone(), "Frame"), "Steel");
    assert_eq!(material(&mut s, flat.clone(), "Bracket"), "Aluminum; Steel", "a bought part with nothing inside lists its own");
    assert_eq!(material(&mut s, top.clone(), "Frame"), "", "at the top level it stands for its brackets too");
    // Folded with a part number: the same rule, and its children list their own.
    set_type(&mut s, "Frame", "normal");
    set_type(&mut s, "Bracket", "normal");
    run(&mut s, "component.metadata.set", json!({"component": "Frame", "part_number": "FR-1"}));
    assert_eq!(material(&mut s, folded.clone(), "Frame"), "");
    // A physical material given to the whole frame is every body's in it, so the row shows it.
    run(&mut s, "material.assign", json!({"bodies": ["Block", "Plate", "Cube"], "material": "Aluminum"}));
    for p in [top, folded] {
        assert_eq!(material(&mut s, p.clone(), "Frame"), "Aluminum", "{p}");
    }
}

/// A row names the material its mass is worked out with, as the library or the design spells it:
/// a damaged file's material that doesn't exist, or whose density can't be used, weighs and reads
/// as the default, and a hand-edited spelling reads as the material's own.
#[test]
fn a_row_names_the_material_its_mass_is_worked_out_with() {
    let mut s = frame();
    let row = |s: &mut Session, definition: &str| {
        let r = run(s, "assembly.bom", json!({"mass": true}));
        weights(&r).into_iter().find(|(d, ..)| d == definition)
    };
    // The spacer, a cube of 10 (1000 mm³), in a design's own material.
    run(&mut s, "material.create", json!({"name": "Walnut", "density_g_cm3": 0.65}));
    run(&mut s, "material.assign", json!({"bodies": ["Spacer"], "material": "Walnut"}));
    assert_eq!(row(&mut s, "Spacer"), Some(weight("Spacer", "Walnut", Some(0.65))));
    s.doc_mut().materials.insert("Spacer".into(), "Unobtainium".into());
    assert_eq!(row(&mut s, "Spacer"), Some(weight("Spacer", "Default", Some(1.29))), "a material the design doesn't have");
    s.doc_mut().user_materials.push(solidcraft_doc::UserMaterial { name: "Broken".into(), density: -3.0, appearance: None });
    s.doc_mut().materials.insert("Spacer".into(), "Broken".into());
    assert_eq!(row(&mut s, "Spacer"), Some(weight("Spacer", "Default", Some(1.29))), "a design material with an unusable density");
    // The bracket's plate stored as "steel" beside its cube in "Steel": one material, and both
    // weigh as steel (2,400 + 1,000 mm³ at 7.85 g/cm³).
    s.doc_mut().materials.insert("Plate".into(), "steel".into());
    assert_eq!(row(&mut s, "Bracket"), Some(weight("Bracket", "Steel", Some(26.69))));
}

/// A mesh's measured volume is signed: an open one facing the origin measures less than nothing,
/// and that is unknown, never a negative mass. A flat one through the origin measures exactly
/// nothing, and weighs 0 g.
#[test]
fn a_body_that_measures_inside_out_has_no_mass() {
    let dir = scratch("inside-out");
    // A 10 mm square at z = 10 facing down (the cone to the origin: -333.3 mm³), and one at z = 0.
    let square = |z: i32, corners: [[i32; 2]; 4]| {
        let facet = |t: [usize; 3]| {
            let v: String = t.iter().map(|&i| format!("vertex {} {} {z}\n", corners[i][0], corners[i][1])).collect();
            format!("facet normal 0 0 0\nouter loop\n{v}endloop\nendfacet\n")
        };
        format!("solid square\n{}{}endsolid square\n", facet([0, 1, 2]), facet([0, 2, 3]))
    };
    let mut s = Session::default();
    for (name, z, corners) in [("Facing down", 10, [[0, 0], [0, 10], [10, 10], [10, 0]]), ("Flat", 0, [[0, 0], [10, 0], [10, 10], [0, 10]])] {
        let path = dir.join(format!("{name}.stl"));
        std::fs::write(&path, square(z, corners)).unwrap();
        run(&mut s, "file.insert_mesh", json!({"path": path}));
    }
    let r = run(&mut s, "assembly.bom", json!({"mass": true}));
    assert_eq!(weights(&r), [weight("Facing down", "Default", None), weight("Flat", "Default", Some(0.0))], "{r}");
    // The flat one beside a definition with nothing to weigh, under one part number: weighing
    // nothing and having nothing to weigh aren't one mass.
    run(&mut s, "component.create", json!({"name": "Empty", "activate": false}));
    for c in ["Flat", "Empty"] {
        run(&mut s, "component.metadata.set", json!({"component": c, "part_number": "FL-1"}));
    }
    let r = run(&mut s, "assembly.bom", json!({"mass": true}));
    let row = r["rows"].as_array().unwrap().iter().find(|r| r["part_number"] == "FL-1").cloned().unwrap();
    assert_eq!((row["components"].as_array().map(Vec::len), &row["mass_g"]), (Some(2), &Value::Null), "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The bill of materials `request` lists for the design `s` holds, as typed rows.
fn list(s: &Session, request: Request) -> Bom {
    let state = s.model.state();
    bom::list(&mut Tree::new(&s.doc, &state), &request).unwrap()
}

/// Three levels (A holds B, B holds C and a purchased gearbox with a gear inside): all levels
/// lists rows two and three deep, and the gearbox's gear when purchased contents are shown.
fn three_levels() -> Session {
    let mut s = Session::default();
    subassembly(&mut s, "A", "A-1", "");
    subassembly(&mut s, "B", "B-1", "A");
    part(&mut s, "C", "C-1", "B");
    place(&mut s, "C", "B", 1);
    subassembly(&mut s, "Gearbox", "GB-1", "B");
    part(&mut s, "Gear", "G-1", "Gearbox");
    set_type(&mut s, "Gearbox", "purchased");
    place(&mut s, "B", "A", 1);
    s
}

/// `assembly.bom`'s JSON presents the typed rows: every field of every row and child, for every
/// option, at every depth all levels lists (its rows one after another).
#[test]
fn assembly_bom_presents_the_typed_rows() {
    let base = Request::default();
    let all = Request { view: View::AllLevels, ..base };
    let options = [
        (json!({}), base),
        (json!({"levels": "top"}), Request { view: View::FirstLevel, ..base }),
        (json!({"group_children": true}), Request { fold: true, ..base }),
        (json!({"levels": "top", "group_children": true}), Request { view: View::FirstLevel, fold: true, ..base }),
        (json!({"group_by": ["definition"], "group_children": true}), Request { group: Group::Definition, fold: true, ..base }),
        (json!({"mass": true}), Request { mass: true, ..base }),
        (json!({"levels": "top", "mass": true}), Request { view: View::FirstLevel, mass: true, ..base }),
        (json!({"group_children": true, "mass": true}), Request { fold: true, mass: true, ..base }),
        (json!({"levels": "all"}), all),
        (json!({"levels": "all", "mass": true}), Request { mass: true, ..all }),
        (json!({"levels": "all", "show_purchased_contents": true}), Request { purchased_contents: true, ..all }),
        (json!({"levels": "all", "show_purchased_contents": true, "mass": true}), Request { purchased_contents: true, mass: true, ..all }),
        (json!({"levels": "all", "group_by": ["definition"]}), Request { group: Group::Definition, ..all }),
    ];
    let mut numbered_frame = frame();
    run(&mut numbered_frame, "component.metadata.set", json!({"component": "Frame", "part_number": "FR-1"}));
    let mut typed_in = three_levels();
    for c in ["B", "Gear"] {
        run(&mut typed_in, "component.metadata.set", json!({"component": c, "material": "Typed in"}));
    }
    let mut deepest = 0;
    for (name, mut s) in [
        ("assembly", assembly()),
        ("bolts", bolts()),
        ("machine", machine()),
        ("frame", numbered_frame),
        ("three levels", three_levels()),
        ("static materials", typed_in),
    ] {
        for (params, request) in &options {
            let shown = run(&mut s, "assembly.bom", params.clone());
            let listed = list(&s, *request);
            assert_eq!(shown["total_occurrences"], listed.total.occurrences, "{name} {params}");
            match listed.total.mass {
                Mass::NotAsked => assert!(shown.get("total_mass_g").is_none(), "{name} {params}"),
                Mass::Grams(grams) => assert_eq!(shown["total_mass_g"], json!(grams), "{name} {params}"),
                Mass::Unknown(_) => assert!(shown["total_mass_g"].is_null(), "{name} {params}"),
            }
            let lines: Vec<&Value> = shown["rows"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|r| std::iter::once(r).chain(r["children"].as_array().into_iter().flatten()))
                .collect();
            let deep = listed.rows_deep();
            assert_eq!(lines.len(), deep.len(), "{name} {params}");
            for (line, (item, row)) in lines.into_iter().zip(deep) {
                deepest = deepest.max(item.len());
                let top = item.len() == 1;
                assert_eq!(line["item"], if top { json!(row.item) } else { json!(bom::item_text(&item)) }, "{name} {params}");
                let fields = [
                    &line["part_number"],
                    &line["description"],
                    &line["revision"],
                    &line["bom_type"],
                    &line["count"],
                    &line["definition"],
                    &line["material"],
                ];
                let want = [
                    json!(row.part_number),
                    json!(row.description),
                    json!(row.revision),
                    json!(row.bom_type.as_str()),
                    json!(row.count),
                    json!(row.names.join("; ")),
                    json!(row.materials.join("; ")),
                ];
                assert_eq!(fields, want.each_ref(), "{name} {params}");
                let statics: Vec<&str> = row.statics.iter().map(|s| s.key()).collect();
                assert_eq!(line.get("static"), (!statics.is_empty()).then(|| json!(statics)).as_ref(), "{name} {params}");
                if top {
                    assert_eq!((&line["components"], &line["properties"]), (&json!(row.components), &json!(row.properties)), "{name} {params}");
                } else {
                    assert_eq!(line["each"], row.each, "{name} {params}");
                }
                match row.mass {
                    Mass::NotAsked => assert!(line.get("mass_g").is_none(), "{name} {params}"),
                    Mass::Grams(grams) => assert_eq!(line["mass_g"], json!(grams), "{name} {params}"),
                    Mass::Unknown(_) => assert!(line["mass_g"].is_null(), "{name} {params}"),
                }
            }
        }
    }
    assert!(deepest >= 4, "rows as deep as 1.1.1.1 compared: {deepest}");
}

/// A row's mass is known, or says why not; it is weighed only when asked.
#[test]
fn a_rows_mass_says_why_it_is_unknown() {
    let s = frame();
    let flat = list(&s, Request { mass: true, ..Request::default() });
    let masses: Vec<(&str, Option<f64>)> = flat
        .rows
        .iter()
        .map(|r| {
            let grams = match r.mass {
                Mass::Grams(g) => Some((g * 1e6).round() / 1e6),
                Mass::NotAsked | Mass::Unknown(_) => None,
            };
            (r.names[0].as_str(), grams)
        })
        .collect();
    assert_eq!(masses, [("Block", Some(392.5)), ("Bracket", Some(14.33)), ("Grease", None), ("Spacer", Some(1.29))]);
    assert_eq!(flat.rows[2].mass, Mass::Unknown(Unknown::NothingToWeigh), "the grease has no bodies");
    let plain = list(&s, Request::default());
    assert!(plain.rows.iter().all(|r| r.mass == Mass::NotAsked) && plain.total.mass == Mass::NotAsked);
    // At the top level the bolts with and without a washer and nut share a row, and weigh differently.
    let top = list(&bolts(), Request { view: View::FirstLevel, mass: true, ..Request::default() });
    assert_eq!(top.rows[0].mass, Mass::Unknown(Unknown::Differs));
}

/// The assembly's mass is weighed from the design, never added up from rows: a row whose
/// definitions weigh differently has none, and the assembly still has one. When every row is
/// known, the rows of every list add up to it.
#[test]
fn the_total_is_weighed_from_the_assembly_not_added_up_from_rows() {
    let properties = |s: &mut Session| run(s, "component.properties", json!({"component": "root"}))["total"]["mass_g"].as_f64().unwrap();
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-6 * b.abs();
    let mut s = bolts();
    let top = list(&s, Request { view: View::FirstLevel, mass: true, ..Request::default() });
    let Mass::Grams(total) = top.total.mass else { panic!("{:?}", top.total) };
    assert!(close(total, properties(&mut s)), "{total} g");
    let mut s = frame();
    run(&mut s, "component.metadata.set", json!({"component": "Frame", "part_number": "FR-1"}));
    for reference in [false, true] {
        if reference {
            set_occurrence(&mut s, "Block:2", json!("reference"));
        }
        let assembly = properties(&mut s);
        for view in View::ALL {
            for fold in [false, true] {
                let b = list(&s, Request { view, fold, mass: true, ..Request::default() });
                let Mass::Grams(total) = b.total.mass else { panic!("{:?}", b.total) };
                let listed: f64 = b.rows.iter().map(|r| if let Mass::Grams(g) = r.mass { r.count as f64 * g } else { 0.0 }).sum();
                assert!(
                    close(total, assembly) && close(listed, assembly),
                    "{view:?} {fold} {reference}: {listed} g listed, {total} g, {assembly} g in all"
                );
            }
        }
    }
}

/// A compact assembly that places one block 2^64 times (a level placed once, each level below it
/// placed twice in the one above, 64 deep): one of its rows holds more bodies than a count does.
/// Weighing it must never overflow; parts only, which walks every placement, refuses it.
#[test]
fn a_compact_assembly_of_more_bodies_than_a_count_holds_is_weighed() {
    let mut s = Session::default();
    let block = run(&mut s, "component.create", json!({"name": "Block"}))["component"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let doc = s.doc_mut();
    let mut parent = doc.add_component(Some("Level 1"), 0).unwrap();
    for level in 2..=64 {
        let below = doc.add_component(Some(format!("Level {level}").as_str()), parent).unwrap();
        doc.add_occurrence(below, parent, solidcraft_doc::IDENTITY).unwrap();
        parent = below;
    }
    for _ in 0..2 {
        doc.add_occurrence(block, parent, solidcraft_doc::IDENTITY).unwrap();
    }
    doc.occurrences.retain(|o| !(o.component == block && o.parent == 0));
    let top = run(&mut s, "assembly.bom", json!({"levels": "top", "mass": true}));
    assert!(top["rows"][0]["mass_g"].as_f64().is_some_and(f64::is_finite), "{top}");
    assert!(s.execute("assembly.bom", &json!({})).is_err(), "more placements than a list walks");
}

/// A part with a body, its part number `pn`, placed once in `parent` (0 for the design).
fn part(s: &mut Session, name: &str, pn: &str, parent: &str) {
    let mut p = json!({"name": name});
    if !parent.is_empty() {
        p["parent"] = json!(parent);
    }
    run(s, "component.create", p);
    run(s, "solid.cylinder", json!({"diameter": 6, "height": 20}));
    run(s, "component.activate", json!({"component": 0}));
    run(s, "component.metadata.set", json!({"component": name, "part_number": pn}));
}

/// An assembly placed once in `parent`, its part number `pn`, empty until something is placed in it.
fn subassembly(s: &mut Session, name: &str, pn: &str, parent: &str) {
    let mut p = json!({"name": name, "activate": false});
    if !parent.is_empty() {
        p["parent"] = json!(parent);
    }
    run(s, "component.create", p);
    run(s, "component.metadata.set", json!({"component": name, "part_number": pn}));
}

/// `n` more placements of `component` in `parent`.
fn place(s: &mut Session, component: &str, parent: &str, n: usize) {
    let parent = if parent.is_empty() { json!(0) } else { json!(parent) };
    for i in 0..n {
        run(s, "occurrence.copy", json!({"component": component, "parent": parent, "translate": [10 * (i + 1), 0, 0]}));
    }
}

/// Every row, children included, as (item, part number, bill of materials type, each, count).
fn deep(r: &Value) -> Vec<(String, String, String, u64, u64)> {
    fn walk(rows: &Value, out: &mut Vec<(String, String, String, u64, u64)>) {
        for r in rows.as_array().into_iter().flatten() {
            let item = r["item"].as_str().map_or_else(|| r["item"].to_string(), str::to_string);
            let each = r["each"].as_u64().unwrap_or(r["count"].as_u64().unwrap());
            out.push((item, r["part_number"].as_str().unwrap().into(), r["bom_type"].as_str().unwrap().into(), each, r["count"].as_u64().unwrap()));
            walk(&r["children"], out);
        }
    }
    let mut out = Vec::new();
    walk(&r["rows"], &mut out);
    out
}

fn deep_row(item: &str, pn: &str, bom_type: &str, each: u64, count: u64) -> (String, String, String, u64, u64) {
    (item.into(), pn.into(), bom_type.into(), each, count)
}

/// Four bolts and a two-bolt subassembly placed twice (the study's case, 0xlillith on #3874): parts
/// only counts 8 bolts; all levels lists the subassembly twice with its bolts under it, 2 in each
/// and 4 in all, beside the 4 bolts of the top.
fn bolts_and_subassemblies() -> Session {
    let mut s = Session::default();
    part(&mut s, "Bolt", "BOLT", "");
    place(&mut s, "Bolt", "", 3);
    subassembly(&mut s, "Sub", "SUB", "");
    place(&mut s, "Bolt", "Sub", 2);
    place(&mut s, "Sub", "", 1);
    s
}

#[test]
fn all_levels_lists_each_subassembly_with_its_own_rows_under_it() {
    let mut s = bolts_and_subassemblies();
    assert_eq!(typed(&run(&mut s, "assembly.bom", json!({}))), [row("BOLT", "normal", 8)]);
    let all = run(&mut s, "assembly.bom", json!({"levels": "all"}));
    assert_eq!(deep(&all), [deep_row("1", "BOLT", "normal", 4, 4), deep_row("2", "SUB", "normal", 2, 2), deep_row("2.1", "BOLT", "normal", 2, 4)]);
    assert_eq!(all["total_occurrences"], 10, "4 + 2 + 4 placements");
    assert_eq!(all["levels"], "all");
    // Folding changes nothing here: every row of all levels holds what it holds.
    let folded = run(&mut s, "assembly.bom", json!({"levels": "all", "group_children": true}));
    assert_eq!(deep(&folded), deep(&all));
}

/// Three levels: items 1, 1.1, 1.1.1, the counts multiplied through (2 x 3 x 2), and the CSV in
/// that order.
#[test]
fn all_levels_numbers_and_counts_three_levels_deep() {
    let mut s = Session::default();
    subassembly(&mut s, "A", "A-1", "");
    subassembly(&mut s, "B", "B-1", "A");
    part(&mut s, "C", "C-1", "B");
    place(&mut s, "C", "B", 1);
    place(&mut s, "B", "A", 2);
    place(&mut s, "A", "", 1);
    let all = run(&mut s, "assembly.bom", json!({"levels": "all"}));
    assert_eq!(deep(&all), [deep_row("1", "A-1", "normal", 2, 2), deep_row("1.1", "B-1", "normal", 3, 6), deep_row("1.1.1", "C-1", "normal", 2, 12)]);
    assert_eq!(all["total_occurrences"], 20);
    let dir = std::env::temp_dir().join(format!("solidcraft-bom-all-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("all.csv");
    run(&mut s, "assembly.bom", json!({"levels": "all", "path": path.to_str().unwrap()}));
    let csv = std::fs::read_to_string(&path).unwrap();
    let items: Vec<&str> = csv.lines().skip(1).map(|l| l.split(',').next().unwrap()).collect();
    assert_eq!(items, ["\"1\"", "\"1.1\"", "\"1.1.1\""]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Each type in all levels: a phantom's contents join the level it sits in, merged with the same
/// part there; a purchased part is a row with nothing under it unless its contents are asked for;
/// an inseparable part is a row with nothing under it; a reference is in no row, nor anything in it.
#[test]
fn all_levels_follows_each_bill_of_materials_type() {
    let mut s = bolts_and_subassemblies();
    subassembly(&mut s, "Group", "", "Sub");
    place(&mut s, "Bolt", "Group", 1);
    set_type(&mut s, "Group", "phantom");
    let all = |s: &mut Session, p: Value| deep(&run(s, "assembly.bom", p));
    assert_eq!(
        all(&mut s, json!({"levels": "all"})),
        [deep_row("1", "BOLT", "normal", 4, 4), deep_row("2", "SUB", "normal", 2, 2), deep_row("2.1", "BOLT", "normal", 3, 6)],
        "the phantom's bolt joins the subassembly's two"
    );
    set_type(&mut s, "Sub", "purchased");
    assert_eq!(all(&mut s, json!({"levels": "all"})), [deep_row("1", "BOLT", "normal", 4, 4), deep_row("2", "SUB", "purchased", 2, 2)]);
    let opened = run(&mut s, "assembly.bom", json!({"levels": "all", "show_purchased_contents": true}));
    assert_eq!(
        deep(&opened),
        [deep_row("1", "BOLT", "normal", 4, 4), deep_row("2", "SUB", "purchased", 2, 2), deep_row("2.1", "BOLT", "normal", 3, 6)]
    );
    assert_eq!(opened["total_occurrences"], 12);
    assert_eq!(
        all(&mut s, json!({"levels": "top", "show_purchased_contents": true})),
        [deep_row("1", "BOLT", "normal", 4, 4), deep_row("2", "SUB", "purchased", 2, 2)]
    );
    set_type(&mut s, "Sub", "inseparable");
    assert_eq!(
        all(&mut s, json!({"levels": "all", "show_purchased_contents": true})),
        [deep_row("1", "BOLT", "normal", 4, 4), deep_row("2", "SUB", "inseparable", 2, 2)]
    );
    set_type(&mut s, "Sub", "reference");
    assert_eq!(all(&mut s, json!({"levels": "all"})), [deep_row("1", "BOLT", "normal", 4, 4)]);
}

/// Two subassemblies with one part number and different contents are two rows, each with its own
/// (one part number's rows by name: Three before Two).
#[test]
fn all_levels_keeps_subassemblies_with_different_contents_apart() {
    let mut s = Session::default();
    part(&mut s, "Bolt", "BOLT", "");
    subassembly(&mut s, "Two", "SUB", "");
    place(&mut s, "Bolt", "Two", 2);
    subassembly(&mut s, "Three", "SUB", "");
    place(&mut s, "Bolt", "Three", 3);
    let all = deep(&run(&mut s, "assembly.bom", json!({"levels": "all"})));
    assert_eq!(
        all,
        [
            deep_row("1", "BOLT", "normal", 1, 1),
            deep_row("2", "SUB", "normal", 1, 1),
            deep_row("2.1", "BOLT", "normal", 3, 3),
            deep_row("3", "SUB", "normal", 1, 1),
            deep_row("3.1", "BOLT", "normal", 2, 2),
        ]
    );
}

/// What all levels refuses: a word it doesn't know, a flag that isn't one, and an assembly larger
/// than the walk's budget (17 definitions placed twice each: over 100,000 placements).
#[test]
fn all_levels_refuses_what_it_cant_list() {
    let mut s = bolts_and_subassemblies();
    for (p, says) in
        [(json!({"levels": "deep"}), "`levels` is flat or top or all"), (json!({"levels": "all", "show_purchased_contents": "yes"}), "true or false")]
    {
        let e = s.execute("assembly.bom", &p).unwrap_err().to_string();
        assert!(e.contains(says), "{p}: {e}");
    }
    let mut s = Session::default();
    for _ in 0..17 {
        let parent = s.active_component;
        let component = s.doc_mut().add_component(None, parent).unwrap();
        s.doc_mut().add_occurrence(component, parent, solidcraft_doc::IDENTITY).unwrap();
        s.active_component = component;
    }
    s.active_component = 0;
    let e = s.execute("assembly.bom", &json!({"levels": "all"})).unwrap_err().to_string();
    assert!(e.contains("more than 100000 placements"), "{e}");
}

/// 64 levels in all levels: the rows come one after another, so a JSON reader takes the list back
/// (nested, it would be 129 containers deep, past serde_json's 128).
#[test]
fn all_levels_64_deep_reads_back_as_json() {
    let mut s = Session::default();
    let mut parent = 0;
    for _ in 0..64 {
        parent = s.doc_mut().add_component(None, parent).unwrap();
    }
    let r = run(&mut s, "assembly.bom", json!({"levels": "all"}));
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 64);
    assert!(rows.iter().all(|row| row.get("children").is_none()));
    assert_eq!(rows.last().unwrap()["item"], vec!["1"; 64].join("."));
    let back: Value = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(back, r);
    let dir = scratch("deep");
    let path = dir.join("deep.json");
    run(&mut s, "assembly.bom", json!({"levels": "all", "path": path.to_str().unwrap()}));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(file["rows"].as_array().map(Vec::len), Some(64));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// All levels gathers the rows under a row as it gathers the top: by part number two definitions
/// of one part number under a subassembly are one row, by definition two.
#[test]
fn all_levels_gathers_every_level_as_the_first() {
    let mut s = Session::default();
    subassembly(&mut s, "Sub", "SUB", "");
    part(&mut s, "BoltA", "BOLT", "Sub");
    part(&mut s, "BoltB", "BOLT", "Sub");
    let by_number = deep(&run(&mut s, "assembly.bom", json!({"levels": "all"})));
    assert_eq!(by_number, [deep_row("1", "SUB", "normal", 1, 1), deep_row("1.1", "BOLT", "normal", 2, 2)]);
    let by_definition = deep(&run(&mut s, "assembly.bom", json!({"levels": "all", "group_by": ["definition"]})));
    assert_eq!(
        by_definition,
        [deep_row("1", "SUB", "normal", 1, 1), deep_row("1.1", "BOLT", "normal", 1, 1), deep_row("1.2", "BOLT", "normal", 1, 1)]
    );
}

/// Each row of a reply as `assembly.bom` gives it: item, part number, and its numbering flags.
fn numbers(r: &Value) -> Vec<(String, String, String)> {
    r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let flags: String =
                [("provisional", 'p'), ("locked", 'l'), ("duplicate", 'd')].iter().filter(|(k, _)| row[*k] == true).map(|(_, c)| *c).collect();
            (row["item"].to_string().trim_matches('"').to_string(), row["part_number"].as_str().unwrap().to_string(), flags)
        })
        .collect()
}

fn numbered_row(item: &str, part_number: &str, flags: &str) -> (String, String, String) {
    (item.into(), part_number.into(), flags.into())
}

#[test]
fn a_new_rows_item_number_is_saved_in_the_changes_undo_step() {
    let dir = scratch("numbers");
    let mut s = assembly();
    let unnumbered = run(&mut s, "assembly.bom", json!({}));
    assert_eq!(unnumbered["numbered"], false);
    assert!(unnumbered["rows"].as_array().unwrap().iter().all(|r| r.get("provisional").is_none()), "a view never numbered reads as before");
    let undo = s.undo.len();
    let r = run(&mut s, "assembly.bom.renumber", json!({"start": 10, "step": 10}));
    assert_eq!((r["numbered"].as_bool(), numbers(&r)), (Some(true), vec![numbered_row("10", "PL-100", ""), numbered_row("20", "PN-040", "")]));
    assert_eq!(s.undo.len(), undo + 1, "one undo step");
    // A new part: its number is saved with it, in its own undo step.
    run(&mut s, "component.create", json!({"name": "Bracket", "activate": false}));
    run(&mut s, "component.metadata.set", json!({"component": "Bracket", "part_number": "BR-001"}));
    assert_eq!(s.undo.len(), undo + 3);
    let after = run(&mut s, "assembly.bom", json!({}));
    assert_eq!(after["numbered"], true);
    assert_eq!(numbers(&after), [numbered_row("10", "PL-100", ""), numbered_row("20", "PN-040", ""), numbered_row("21", "BR-001", "")]);
    assert_eq!(s.doc.assembly.item_numbers[0].items.len(), 3);
    // Undone, the part and its number go together; redone, both come back.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((s.doc.assembly.item_numbers[0].items.len(), numbers(&run(&mut s, "assembly.bom", json!({}))).len()), (2, 2));
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(numbers(&run(&mut s, "assembly.bom", json!({}))), numbers(&after));
    // Saved and opened again, in both formats; the CSV lists by item number.
    for (file, format) in [("numbers.json", "json"), ("numbers.solidcraft", "container")] {
        let path = dir.join(file);
        run(&mut s, "file.save_as", json!({"path": path, "format": format}));
        let mut back = Session::default();
        run(&mut back, "doc.open", json!({"path": path}));
        assert_eq!(numbers(&run(&mut back, "assembly.bom", json!({}))), numbers(&after), "{format}");
        assert_eq!(back.doc.assembly.item_numbers, s.doc.assembly.item_numbers, "{format}");
    }
    let csv = dir.join("numbers.csv");
    run(&mut s, "assembly.bom", json!({"path": csv}));
    let items: Vec<String> =
        std::fs::read_to_string(&csv).unwrap().lines().skip(1).map(|l| l.split(',').take(2).collect::<Vec<_>>().join(",")).collect();
    assert_eq!(items, ["\"10\",\"PL-100\"", "\"20\",\"PN-040\"", "\"21\",\"BR-001\""]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn item_set_marks_duplicates_and_locks_and_a_renumber_keeps_locks() {
    let mut s = assembly();
    run(&mut s, "component.create", json!({"name": "Bracket", "activate": false}));
    run(&mut s, "component.metadata.set", json!({"component": "Bracket", "part_number": "BR-001"}));
    let undo = s.undo.len();
    // The view isn't numbered yet: the others keep the numbers they show.
    let r = run(&mut s, "assembly.bom.item.set", json!({"row": "3", "item": 1, "locked": true}));
    assert_eq!(numbers(&r), [numbered_row("1", "BR-001", "d"), numbered_row("1", "PN-040", "ld"), numbered_row("2", "PL-100", "")]);
    assert_eq!(r["duplicates"], json!([1]));
    assert_eq!(s.undo.len(), undo + 1);
    let r = run(&mut s, "assembly.bom.renumber", json!({}));
    assert_eq!(numbers(&r), [numbered_row("1", "PN-040", "l"), numbered_row("2", "BR-001", ""), numbered_row("3", "PL-100", "")]);
    // A row under another, by its item as shown: the pin in the hinge.
    let r = run(&mut s, "assembly.bom.item.set", json!({"levels": "all", "row": "3.1", "locked": true}));
    let pin = r["rows"].as_array().unwrap().iter().find(|row| row["part_number"] == "PN-040").unwrap();
    assert_eq!((&pin["item"], &pin["locked"]), (&json!("3.1"), &json!(true)), "{r}");
    assert_eq!(r["duplicates"], json!([]));
    assert_eq!(s.doc.assembly.item_numbers.len(), 2, "each view numbered on its own");
}

#[test]
fn numbering_evaluates_nothing() {
    let mut s = bolts();
    s.evaluations = 0;
    run(&mut s, "assembly.bom.renumber", json!({"levels": "top"}));
    run(&mut s, "assembly.bom.item.set", json!({"levels": "top", "row": 1, "item": 5}));
    run(&mut s, "component.metadata.set", json!({"component": 1, "description": "Bolt"}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.evaluations, 0, "item numbers change no geometry");
    // A placement and a new part added to a numbered design are evaluated as in one never
    // numbered; the new part's row takes its number without another evaluation.
    let mut plain = bolts();
    plain.evaluations = 0;
    s.evaluations = 0;
    for session in [&mut s, &mut plain] {
        run(session, "occurrence.copy", json!({"component": 1, "translate": [0, 40, 0]}));
        run(session, "parts.insert", json!({"family": "hex_nut", "size": "M6", "point": [90, 0, 0], "direction": [0, 0, 1]}));
    }
    assert_eq!(s.evaluations, plain.evaluations);
    assert!(run(&mut s, "assembly.bom", json!({"levels": "top"}))["rows"].as_array().unwrap().iter().all(|r| r.get("provisional").is_none()));
}

#[test]
fn hostile_numbering_is_refused_without_a_change() {
    let mut s = assembly();
    let undo = s.undo.len();
    for p in [
        json!({"start": 0}),
        json!({"start": -1}),
        json!({"start": 1.5}),
        json!({"start": "1"}),
        json!({"start": 1_000_000}),
        json!({"step": 0}),
        json!({"start": 999_999, "step": 1}),
        json!({"order": "up"}),
        json!({"order": 1}),
        json!({"levels": "deep"}),
        json!({"component": "Nope"}),
    ] {
        assert!(s.execute("assembly.bom.renumber", &p).is_err(), "{p}");
    }
    for p in [
        json!({}),
        json!({"item": 1}),
        json!({"row": "1"}),
        json!({"row": "x", "item": 1}),
        json!({"row": "1..2", "item": 1}),
        json!({"row": "", "item": 1}),
        json!({"row": -1, "item": 1}),
        json!({"row": [1], "item": 1}),
        json!({"row": "9", "item": 1}),
        json!({"row": "1.1", "item": 1}),
        json!({"row": "1".repeat(2000), "item": 1}),
        json!({"row": "1", "item": 0}),
        json!({"row": "1", "item": "2"}),
        json!({"row": "1", "locked": "yes"}),
    ] {
        assert!(s.execute("assembly.bom.item.set", &p).is_err(), "{p}");
    }
    assert_eq!(s.undo.len(), undo);
    assert!(s.doc.assembly.item_numbers.is_empty());
}
