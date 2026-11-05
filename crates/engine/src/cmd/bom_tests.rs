use serde_json::{Value, json};

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
    let undo = s.history.len();
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
    assert_eq!(s.history.len(), undo, "the bill of materials only reads");
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
    let undo = s.history.len();
    let r = run(&mut s, "component.metadata.set", json!({"component": "Pin", "revision": "C", "properties": {"finish": null, "material": "1.4301"}}));
    assert_eq!(r["changed"], json!(["revision", "properties.finish", "properties.material"]));
    assert_eq!(s.history.len(), undo + 1, "one undo step");
    // Setting the same values again changes nothing.
    let r = run(&mut s, "component.metadata.set", json!({"component": "Pin", "revision": "C"}));
    assert_eq!(r["changed"], json!([]));
    assert_eq!(s.history.len(), undo + 1);
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
fn inserted_design_carries_its_part_properties_into_the_bom() {
    let dir = scratch("insert-design");
    let source = dir.join("plate.solidcraft");
    let metadata = json!({
        "part_number": "PL-6624", "description": "Mounting plate", "revision": "B",
        "author": "Example author", "license": "CC0-1.0", "com.example.catalog": {"stock": 12}
    });
    let mut part = Session::default();
    run(&mut part, "solid.box", json!({"length": 40, "width": 20, "height": 3}));
    run(&mut part, "document.metadata.set", json!({"values": metadata}));
    run(&mut part, "file.save", json!({"path": source}));

    for reuse in [false, true] {
        let mut s = Session::default();
        run(&mut s, "document.metadata.set", json!({"values": {"part_number": "ASM-1", "description": "Assembly"}}));
        let own_metadata = s.doc.metadata.clone();
        let undo = s.history.len();
        let inserted = run(&mut s, "component.insert_design", json!({"path": source, "name": "Plate", "reuse": reuse}));
        let component = inserted["component"].as_u64().unwrap();
        assert_eq!(run(&mut s, "component.metadata", json!({"component": component}))["metadata"], metadata);
        assert_eq!(s.doc.metadata, own_metadata, "the assembly keeps its own properties");
        let bom = run(&mut s, "assembly.bom", json!({}));
        assert_eq!(bom["rows"][0]["part_number"], "PL-6624");
        assert_eq!(bom["rows"][0]["description"], "Mounting plate");
        assert_eq!(bom["rows"][0]["revision"], "B");
        assert_eq!(bom["rows"][0]["count"], 1);
        assert_eq!(s.history.len(), undo + 1, "insertion including properties is one undo step");
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc.components.is_empty());
        assert!(s.doc.links.is_empty());
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(run(&mut s, "component.metadata", json!({"component": component}))["metadata"], metadata);

        for (file, format) in [("assembly.json", "json"), ("assembly.solidcraft", "container")] {
            let path = dir.join(file);
            run(&mut s, "file.save_as", json!({"path": path, "format": format}));
            let mut back = Session::default();
            run(&mut back, "doc.open", json!({"path": path}));
            assert_eq!(run(&mut back, "component.metadata", json!({"component": component}))["metadata"], metadata, "{format}");
            assert_eq!(run(&mut back, "assembly.bom", json!({}))["rows"], bom["rows"], "{format}");
        }

        // Reusing the link shares its definition, including locally edited part properties.
        run(&mut s, "component.metadata.set", json!({"component": component, "description": "Assembly-specific description"}));
        run(&mut s, "component.insert_design", json!({"path": source, "reuse": true, "translate": [50, 0, 0]}));
        let bom = run(&mut s, "assembly.bom", json!({}));
        assert_eq!(bom["rows"].as_array().unwrap().len(), 1);
        assert_eq!(bom["rows"][0]["count"], 2);
        assert_eq!(bom["rows"][0]["part_number"], "PL-6624");
        assert_eq!(bom["rows"][0]["description"], "Assembly-specific description");
    }
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
    let undo = s.history.len();
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
    assert_eq!(s.history.len(), undo);
    let pin = run(&mut s, "component.metadata", json!({"component": "Pin"}));
    assert_eq!(pin["part_number"], "PN-040", "all or nothing");
    for p in [json!({"levels": "deep"}), json!({"group_by": ["colour"]}), json!({"group_by": 3}), json!({"component": "Nope"})] {
        assert!(s.execute("assembly.bom", &p).is_err(), "{p}");
    }
    // The root reads as the design's own properties.
    run(&mut s, "document.metadata.set", json!({"key": "part_number", "value": "ASM-1"}));
    assert_eq!(run(&mut s, "component.metadata", json!({"component": 0}))["part_number"], "ASM-1");
}

#[test]
fn a_components_refusals_speak_of_part_properties() {
    let mut s = assembly();
    let undo = s.history.len();
    let refusal = |s: &mut Session, p: Value| s.execute("component.metadata.set", &p).unwrap_err().to_string();
    let bogus = refusal(&mut s, json!({"component": "Pin", "values": {"bogus": "x"}}));
    assert!(
        bogus.ends_with(
            "`bogus` is not a part property: SolidCraft's are project, part_number, revision, author, organization, description, \
             license, bom_type; another tool's data goes under a reverse-DNS key such as `com.example.tool`"
        ),
        "{bogus}"
    );
    assert!(!bogus.contains("design property"), "{bogus}");
    assert!(refusal(&mut s, json!({"component": "Pin", "values": {"": "x"}})).contains("`` is not a part property"));
    // Nothing to change: the same list as before the table.
    assert!(refusal(&mut s, json!({"component": "Pin"})).ends_with(
        "give part_number, description, revision, bom_type, values or properties (project, part_number, revision, author, \
         organization, description, license, bom_type)"
    ));
    // Only the bill of materials' keys are parameters of their own; the others go in `values`.
    assert!(refusal(&mut s, json!({"component": "Pin", "author": "Jane"})).contains("give part_number, description, revision"));
    let long = refusal(&mut s, json!({"component": "Pin", "properties": {"finish": "x".repeat(5000)}}));
    assert!(long.ends_with("property `finish` is longer than 4096 bytes"), "{long}");
    let bell = refusal(&mut s, json!({"component": "Pin", "properties": {"finish": "a\u{7}"}}));
    assert!(bell.ends_with("property `finish` has a control character"), "{bell}");
    assert_eq!(s.history.len(), undo, "nothing changed");
    // The design keeps its own words.
    let design = s.execute("document.metadata.set", &json!({"key": "bogus", "value": "x"})).unwrap_err().to_string();
    assert!(design.contains("`bogus` is not a design property"), "{design}");
}

#[test]
fn every_key_a_component_took_before_is_still_taken() {
    let mut s = assembly();
    let keys = ["project", "part_number", "revision", "author", "organization", "description", "license"];
    let mut values: serde_json::Map<String, Value> = keys.iter().map(|k| (k.to_string(), json!(format!("{k} text")))).collect();
    values.insert("com.acme.shop".into(), json!({"id": 7}));
    let r = run(&mut s, "component.metadata.set", json!({"component": "Plate", "values": values}));
    assert_eq!(r["changed"].as_array().map(Vec::len), Some(keys.len() + 1), "{r}");
    let plate = run(&mut s, "component.metadata", json!({"component": "Plate"}));
    for k in keys {
        assert_eq!(plate["metadata"][k], format!("{k} text"), "{k}");
    }
    for k in ["part_number", "description", "revision"] {
        assert_eq!(plate[k], format!("{k} text"), "the bill of materials' field {k}: {plate}");
    }
    assert!(plate.get("author").is_none(), "the others stay in metadata: {plate}");
    // The shortcuts and the root read the same fields.
    run(&mut s, "component.metadata.set", json!({"component": "Pin", "part_number": "", "description": "Pin", "revision": "D"}));
    let pin = run(&mut s, "component.metadata", json!({"component": "Pin"}));
    assert_eq!((&pin["part_number"], &pin["description"], &pin["revision"]), (&Value::Null, &json!("Pin"), &json!("D")), "{pin}");
    let root = run(&mut s, "component.metadata", json!({"component": 0}));
    assert_eq!((&root["component"], &root["part_number"], &root["properties"]), (&json!(0), &Value::Null, &json!({})), "{root}");
}

#[test]
fn a_component_refused_past_its_limits_says_part_properties() {
    let mut s = assembly();
    // The pin holds a part number and a description: room for this many more keys.
    let room = solidcraft_doc::metadata::MAX_KEYS - 2;
    let values: serde_json::Map<String, Value> = (0..room).map(|i| (format!("org.k{i}"), json!({}))).collect();
    run(&mut s, "component.metadata.set", json!({"component": "Pin", "values": values}));
    let over = s.execute("component.metadata.set", &json!({"component": "Pin", "values": {"org.one_more": {}}})).unwrap_err().to_string();
    assert!(over.ends_with("part properties are limited to 256 keys and 256 KiB"), "{over}");
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
    for s in [assembly(), bolts()] {
        let st = s.model.state();
        let each: std::collections::HashSet<u64> = st.bodies.iter().map(|b| s.doc.model_body_component(b)).collect();
        assert_eq!(s.doc.body_owners(&st), each);
        assert!(each.len() > 1);
        // The same answers body by body, in the state's order.
        let in_order: Vec<u64> = st.bodies.iter().map(|b| s.doc.model_body_component(b)).collect();
        assert_eq!(s.doc.model_body_components(&st), in_order);
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
fn group_children_keeps_an_inseparable_parts_purchased_children_in_its_row() {
    let mut s = Session::default();
    for (name, parent, part_number, bom_type) in
        [("Sub", "root", "SUB-1", "inseparable"), ("Screw", "Sub", "M2x10", "purchased"), ("Pin", "Sub", "PIN-1", "normal")]
    {
        run(&mut s, "component.create", json!({"name": name, "parent": parent, "activate": false}));
        run(&mut s, "component.metadata.set", json!({"component": name, "part_number": part_number, "bom_type": bom_type}));
    }
    run(&mut s, "occurrence.copy", json!({"component": "Screw", "parent": "Sub"}));
    run(&mut s, "occurrence.copy", json!({"component": "Sub"}));
    let plain = run(&mut s, "assembly.bom", json!({}));
    assert_eq!(typed(&plain), [row("M2x10", "purchased", 4), row("SUB-1", "inseparable", 2)]);
    for levels in ["flat", "top"] {
        for group in ["part_number", "definition"] {
            let r = run(&mut s, "assembly.bom", json!({"levels": levels, "group_by": [group], "group_children": true}));
            assert_eq!(typed(&r), [row("SUB-1", "inseparable", 2)], "{r}");
            assert_eq!(r["total_occurrences"], plain["total_occurrences"]);
            let children = r["rows"][0]["children"].as_array().unwrap();
            assert_eq!(children.len(), 1, "the made Pin stays inside the inseparable part: {r}");
            assert_eq!(children[0]["item"], "1.1");
            assert_eq!(children[0]["part_number"], "M2x10");
            assert_eq!(children[0]["bom_type"], "purchased");
            assert_eq!(children[0]["each"], 2);
            assert_eq!(children[0]["count"], 4);
        }
    }
    let path = scratch("inseparable-kit").join("bom.csv");
    run(&mut s, "assembly.bom", json!({"group_children": true, "path": path}));
    let text = std::fs::read_to_string(path).unwrap();
    let rows: Vec<&str> = text.lines().skip(1).collect();
    assert_eq!(rows, ["\"1\",\"SUB-1\",\"\",\"\",\"2\",\"Sub\"", "\"1.1\",\"M2x10\",\"\",\"\",\"4\",\"Screw\""]);

    // In its own context the component still lists all its parts, regardless of its type.
    let own = run(&mut s, "assembly.bom", json!({"component": "Sub", "group_children": true}));
    assert_eq!(typed(&own), [row("M2x10", "purchased", 2), row("PIN-1", "normal", 1)]);
    // Only numbered parts fold; turning the option off or removing the number keeps the flat list.
    run(&mut s, "component.metadata.set", json!({"component": "Sub", "part_number": ""}));
    assert_eq!(run(&mut s, "assembly.bom", json!({"group_children": true}))["rows"], run(&mut s, "assembly.bom", json!({}))["rows"]);
}

#[test]
fn an_inseparable_kit_finds_nested_purchased_parts_without_opening_purchased_or_reference_parts() {
    let mut s = Session::default();
    for (name, parent, bom_type) in [
        ("Sub", "root", "inseparable"),
        ("Carrier", "Sub", "normal"),
        ("Nested", "Carrier", "inseparable"),
        ("Bolt", "Nested", "purchased"),
        ("Phantom", "Sub", "phantom"),
        ("Reference", "Sub", "reference"),
        ("Bought", "Sub", "purchased"),
    ] {
        run(&mut s, "component.create", json!({"name": name, "parent": parent, "activate": false}));
        run(&mut s, "component.metadata.set", json!({"component": name, "part_number": name, "bom_type": bom_type}));
    }
    for parent in ["Phantom", "Reference", "Bought"] {
        run(&mut s, "occurrence.copy", json!({"component": "Bolt", "parent": parent}));
    }
    let plain = run(&mut s, "assembly.bom", json!({}));
    assert_eq!(typed(&plain), [row("Bolt", "purchased", 2), row("Bought", "purchased", 1), row("Sub", "inseparable", 1)]);
    let folded = run(&mut s, "assembly.bom", json!({"group_children": true}));
    assert_eq!(typed(&folded), [row("Sub", "inseparable", 1)]);
    assert_eq!(folded["total_occurrences"], plain["total_occurrences"]);
    let children = &folded["rows"][0]["children"];
    assert_eq!(children.as_array().unwrap().len(), 2, "{folded}");
    assert_eq!(children[0]["part_number"], "Bolt");
    assert_eq!(children[0]["each"], 2);
    assert_eq!(children[1]["part_number"], "Bought");
    assert_eq!(children[1]["each"], 1);

    // Two definitions with the same number but different purchased contents stay separate kits.
    run(&mut s, "component.create", json!({"name": "Other", "activate": false}));
    run(&mut s, "component.metadata.set", json!({"component": "Other", "part_number": "Sub", "bom_type": "inseparable"}));
    run(&mut s, "occurrence.copy", json!({"component": "Bolt", "parent": "Other"}));
    let different = run(&mut s, "assembly.bom", json!({"group_children": true}));
    assert_eq!(typed(&different), [row("Sub", "inseparable", 1), row("Sub", "inseparable", 1)]);
    assert_eq!(different["total_occurrences"], 6);
    // Buying the whole parent makes one purchased row, with no children listed.
    set_type(&mut s, "Sub", "purchased");
    let bought = run(&mut s, "assembly.bom", json!({"group_children": true}));
    let sub = bought["rows"].as_array().unwrap().iter().find(|r| r["definition"] == "Sub").unwrap();
    assert!(sub.get("children").is_none(), "{bought}");
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
    let undo = s.history.len();
    s.evaluations = 0;
    let r = run(&mut s, "component.metadata.set", json!({"component": "Gearbox", "bom_type": "inseparable"}));
    assert_eq!(r["changed"], json!(["bom_type"]));
    assert_eq!(s.history.len(), undo + 1);
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
    let undo = s.history.len();
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
    assert_eq!(s.history.len(), undo, "nothing changed");
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
