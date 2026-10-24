use serde_json::{Value, json};
use solidcraft_doc::bom::{self, Bom, Group, Holds, Mass, Request, Tree, Unknown, View};

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
        "\"item\",\"part_number\",\"description\",\"revision\",\"count\",\"definition\"\n\
         \"1\",\"PL-100\",\"Plate, 3 mm\",\"B\",\"3\",\"Plate\"\n\
         \"2\",\"PN-040\",\"Pin \"\"short\"\", 4 mm\",\"\",\"2\",\"Pin\"\n"
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
    // Weighing measures every part, so it is asked for; materials come without it.
    let plain = run(&mut s, "assembly.bom", json!({}));
    assert_eq!((&plain["mass"], &plain["rows"][1]["material"]), (&json!(false), &json!("Aluminum; Steel")));
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
    let _ = std::fs::remove_dir_all(&dir);
}

/// The bill of materials `request` lists for the design `s` holds, as typed rows.
fn list(s: &Session, request: Request) -> Bom {
    let state = s.model.state();
    bom::list(&mut Tree::new(&s.doc, &state), &request).unwrap()
}

/// `assembly.bom`'s JSON presents the typed rows: every field of every row and child, for every
/// option.
#[test]
fn assembly_bom_presents_the_typed_rows() {
    let base = Request::default();
    let options = [
        (json!({}), base),
        (json!({"levels": "top"}), Request { view: View::FirstLevel, ..base }),
        (json!({"group_children": true}), Request { fold: true, ..base }),
        (json!({"levels": "top", "group_children": true}), Request { view: View::FirstLevel, fold: true, ..base }),
        (json!({"group_by": ["definition"], "group_children": true}), Request { group: Group::Definition, fold: true, ..base }),
        (json!({"mass": true}), Request { mass: true, ..base }),
        (json!({"levels": "top", "mass": true}), Request { view: View::FirstLevel, mass: true, ..base }),
        (json!({"group_children": true, "mass": true}), Request { fold: true, mass: true, ..base }),
    ];
    let mut numbered_frame = frame();
    run(&mut numbered_frame, "component.metadata.set", json!({"component": "Frame", "part_number": "FR-1"}));
    for (name, mut s) in [("assembly", assembly()), ("bolts", bolts()), ("machine", machine()), ("frame", numbered_frame)] {
        for (params, request) in &options {
            let shown = run(&mut s, "assembly.bom", params.clone());
            let listed = list(&s, *request);
            assert_eq!(shown["total_occurrences"], listed.total.occurrences, "{name} {params}");
            let lines: Vec<&Value> = shown["rows"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|r| std::iter::once(r).chain(r["children"].as_array().into_iter().flatten()))
                .collect();
            let deep = listed.rows_deep();
            assert_eq!(lines.len(), deep.len(), "{name} {params}");
            for (line, (item, row)) in lines.into_iter().zip(deep) {
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

/// A row says what one of its parts stands for when weighed, and which placements it counts.
#[test]
fn a_row_says_what_it_stands_for_and_which_placements_it_counts() {
    let mut s = frame();
    set_type(&mut s, "Frame", "inseparable");
    set_type(&mut s, "Bracket", "purchased");
    let id = |name: &str| s.doc.occurrences.iter().find(|o| o.name == name).unwrap().id;
    let mut brackets = vec![vec![id("Frame:1"), id("Bracket:1")], vec![id("Bracket:2")]];
    brackets.sort();
    let want = [
        ("Bracket", Holds::All, brackets),
        ("Frame", Holds::AllButPurchased, vec![vec![id("Frame:1")]]),
        ("Grease", Holds::Own, vec![vec![id("Grease:1")]]),
        ("Spacer", Holds::Own, vec![vec![id("Spacer:1")]]),
    ];
    let flat = list(&s, Request::default());
    let rows: Vec<(&str, Holds, Vec<Vec<u64>>)> = flat.rows.iter().map(|r| (r.names[0].as_str(), r.stands_for, r.members.clone())).collect();
    assert_eq!(rows, want, "the blocks are the inseparable frame's, the brackets bought beside it");
    // At the first level each row stands for all its part holds.
    assert!(list(&s, Request { view: View::FirstLevel, ..Request::default() }).rows.iter().all(|r| r.stands_for == Holds::All));
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
