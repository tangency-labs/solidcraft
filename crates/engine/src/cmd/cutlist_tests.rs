use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn board(s: &mut Session, name: &str, size: [f64; 3]) {
    let [length, width, height] = size;
    run(s, "solid.box", json!({"length": length, "width": width, "height": height, "body_name": name}));
}

fn sizes(r: &Value) -> Vec<(u64, [f64; 3])> {
    r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["count"].as_u64().unwrap(), ["length", "width", "thickness"].map(|k| r["stock"][k].as_f64().unwrap())))
        .collect()
}

#[test]
fn bench_has_four_rows_eight_pieces_and_hand_sized_stock() {
    let mut s = Session::default();
    for (prefix, n, dims) in
        [("Top", 2, [1828.8, 762.0, 18.0]), ("End", 3, [878.4, 591.6, 18.0]), ("Rear", 1, [1676.4, 778.4, 18.0]), ("Shelf", 2, [811.2, 591.6, 18.0])]
    {
        for i in 0..n {
            board(&mut s, &format!("{prefix} {i}"), dims);
        }
    }
    let undo = s.undo.len();
    let revision = s.revision;
    let r = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(sizes(&r), vec![(3, [878.4, 591.6, 18.0]), (1, [1676.4, 778.4, 18.0]), (2, [811.2, 591.6, 18.0]), (2, [1828.8, 762.0, 18.0])]);
    assert_eq!(r["total_occurrences"], 8);
    assert_eq!(r["totals"], json!([{"material":"", "count":8}]));
    assert_eq!((s.undo.len(), s.revision), (undo, revision));
    assert!(r["rows"].as_array().unwrap().iter().all(|r| r["stock"]["exact"] == true && r["grain"] == ""));
}

#[test]
fn a_rotated_brace_keeps_its_true_length_and_translation_changes_nothing() {
    let mut s = Session::default();
    board(&mut s, "Brace", [600.0, 70.0, 35.0]);
    run(&mut s, "solid.move", json!({"bodies":["Brace"], "axis":[0,1,0], "angle":"45 deg"}));
    let first = run(&mut s, "cutlist.parts", json!({}));
    let dims = sizes(&first);
    assert_eq!(dims[0].0, 1);
    for (got, want) in dims[0].1.iter().zip([600.0, 70.0, 35.0]) {
        assert!((got - want).abs() < 1e-7, "{first}");
    }
    run(&mut s, "solid.move", json!({"bodies":["Brace"], "translate":[1000,-300,250]}));
    let second = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(first, second);
}

#[test]
fn component_instances_count_and_reposing_does_not_change_rows() {
    let mut s = Session::default();
    let made = run(&mut s, "component.create", json!({"name":"Rail"}));
    board(&mut s, "Board", [360.0, 45.0, 18.0]);
    run(&mut s, "component.activate", json!({"component":0}));
    let seed = s.doc.occurrences.iter().find(|o| o.component == made["component"].as_u64().unwrap()).unwrap().id;
    for y in [100, 200, 300] {
        run(&mut s, "occurrence.copy", json!({"occurrence":seed, "translate":[0, y, 0]}));
    }
    let turned = s.doc.occurrences.iter().filter(|o| o.component == made["component"].as_u64().unwrap()).map(|o| o.id).max().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":turned, "axis":[0,0,1], "angle":"90 deg", "move_grounded":true}));
    let before = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(sizes(&before), [(4, [360.0, 45.0, 18.0])]);
    assert_eq!(before["rows"][0]["name"], "Rail");
    run(&mut s, "occurrence.move", json!({"occurrence":seed, "translate":[200,300,400], "axis":[1,1,1], "angle":"31 deg", "move_grounded":true}));
    assert_eq!(run(&mut s, "cutlist.parts", json!({})), before);
}

#[test]
fn body_patterns_and_mirrors_group_with_their_original() {
    let mut s = Session::default();
    board(&mut s, "Rail", [360.0, 45.0, 18.0]);
    run(&mut s, "solid.pattern.rectangular", json!({"bodies":["Rail"], "dir1":[0,1,0], "count1":3, "spacing1":100}));
    run(&mut s, "solid.mirror", json!({"bodies":["Rail"], "plane": {"origin":[-50,0,0],"normal":[1,0,0]}}));
    let r = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(sizes(&r), [(4, [360.0, 45.0, 18.0])]);
}

#[test]
fn types_filter_subtrees_and_phantom_and_glued_units_pass_boards_through() {
    let mut s = Session::default();
    for (name, kind) in [("Bought", "purchased"), ("Jig", "reference"), ("Group", "phantom"), ("Glueup", "inseparable")] {
        run(&mut s, "component.create", json!({"name":name}));
        run(&mut s, "component.metadata.set", json!({"component":name,"bom_type":kind}));
        run(&mut s, "component.create", json!({"name":format!("{name} board"),"parent":name}));
        board(&mut s, &format!("{name} body"), [100.0, 50.0, 12.0]);
        run(&mut s, "component.activate", json!({"component":0}));
    }
    run(&mut s, "occurrence.copy", json!({"component":"Group", "translate":[1000,0,0]}));
    let r = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(sizes(&r), [(3, [100.0, 50.0, 12.0])]);
    assert_eq!(r["excluded"], json!([{"body":"Bought body","reason":"purchased"},{"body":"Jig body","reason":"reference"}]));
    assert_eq!(sizes(&run(&mut s, "cutlist.parts", json!({"component":"Group"}))), [(1, [100.0, 50.0, 12.0])]);
}

#[test]
fn close_stock_and_different_materials_stay_separate() {
    let mut s = Session::default();
    for (name, thickness, material) in [("Birch", 12.7, "Baltic birch"), ("Economy", 12.7, "Economy plywood"), ("Metric", 12.0, "Baltic birch")] {
        run(&mut s, "component.create", json!({"name":name}));
        board(&mut s, name, [500.0, 200.0, thickness]);
        run(&mut s, "component.metadata.set", json!({"component":name,"properties":{"material":material}}));
        run(&mut s, "component.activate", json!({"component":0}));
    }
    let r = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(r["rows"].as_array().unwrap().len(), 3);
    assert_eq!(r["totals"], json!([{"material":"Baltic birch","count":2},{"material":"Economy plywood","count":1}]));
    assert_eq!(sizes(&r), [(1, [500.0, 200.0, 12.7]), (1, [500.0, 200.0, 12.0]), (1, [500.0, 200.0, 12.7])]);
}

#[test]
fn frame_members_report_two_section_sizes_and_their_path_lengths() {
    let mut s = Session::default();
    for (name, width, height) in [("Small", 40, 20), ("Large", 60, 30)] {
        run(&mut s, "sketch.create", json!({"plane":"XY","name":name}));
        run(&mut s, "sketch.line", json!({"points":[[0,0],[300,400]]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "frame.member", json!({"sketch":name,"profile":{"kind":"bar","width":width,"height":height},"body_name":name}));
    }
    let r = run(&mut s, "cutlist.parts", json!({}));
    let dims = sizes(&r);
    assert_eq!(dims.len(), 2);
    for ((count, got), want) in dims.iter().zip([[500.0, 60.0, 30.0], [500.0, 40.0, 20.0]]) {
        assert_eq!(*count, 1);
        for (a, b) in got.iter().zip(want) {
            assert!((a - b).abs() < 1e-7, "{r}");
        }
    }
}

#[test]
fn csv_is_exact_quoted_safe_and_always_in_mm() {
    let mut s = Session::default();
    board(&mut s, "Rail", [360.0, 45.0, 18.0]);
    s.doc_mut().units = "in".into();
    let r = run(&mut s, "cutlist.parts", json!({}));
    let list: crate::schema::cutlist::CutList = serde_json::from_value(r).unwrap();
    assert_eq!(
        super::csv(&list.rows),
        "\"item\",\"name\",\"count\",\"material\",\"length\",\"width\",\"thickness\",\"grain\",\"exact\"\n\"1\",\"Rail\",\"1\",\"\",\"360\",\"45\",\"18\",\"\",\"true\"\n"
    );
    let mut row = list.rows[0].clone();
    row.name = "=SUM(1,2)".into();
    assert!(super::csv(&[row]).contains("\"'=SUM(1,2)\""));
}

#[test]
fn invalid_options_fail_and_component_mode_omits_loose_bodies() {
    let mut s = Session::default();
    board(&mut s, "Loose", [100.0, 50.0, 20.0]);
    assert_eq!(run(&mut s, "cutlist.parts", json!({"include":"components"}))["rows"], json!([]));
    for p in
        [json!({"include":"mesh"}), json!({"component":"Missing"}), json!({"group_by":["material"]}), json!({"format":"pdf"}), json!({"path":42})]
    {
        assert!(s.execute("cutlist.parts", &p).is_err());
    }
}

#[test]
fn csv_and_json_exports_write_the_same_mm_rows() {
    let mut s = Session::default();
    board(&mut s, "Rail", [360.0, 45.0, 18.0]);
    let dir = std::env::temp_dir().join(format!("solidcraft-cutlist-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("parts.csv");
    let r = run(&mut s, "cutlist.parts", json!({"path":csv}));
    let list: crate::schema::cutlist::CutList = serde_json::from_value(r).unwrap();
    assert_eq!(std::fs::read_to_string(&csv).unwrap(), super::csv(&list.rows));
    let path = dir.join("parts.json");
    let r = run(&mut s, "cutlist.parts", json!({"path":path}));
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["rows"], r["rows"]);
    assert_eq!(saved["total_occurrences"], 1);
    std::fs::remove_file(csv).unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn a_plain_extruded_board_uses_its_part_directions() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[360,18]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":45,"body_name":"Board"}));
    run(&mut s, "solid.move", json!({"bodies":["Board"],"axis":[1,1,1],"angle":"31 deg"}));
    assert_eq!(sizes(&run(&mut s, "cutlist.parts", json!({}))), [(1, [360.0, 45.0, 18.0])]);
}

#[test]
fn loose_boards_use_the_assigned_physical_material() {
    let mut s = Session::default();
    for (name, material) in [("A", "Steel"), ("B", "Aluminum")] {
        board(&mut s, name, [100.0, 50.0, 20.0]);
        run(&mut s, "material.assign", json!({"bodies":[name],"material":material}));
    }
    let r = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(r["rows"].as_array().unwrap().len(), 2);
    assert_eq!(r["totals"], json!([{"material":"Aluminum","count":1},{"material":"Steel","count":1}]));
}

#[test]
fn assigned_material_keeps_boards_distinct_despite_a_shared_component_label() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Panels"}));
    run(&mut s, "component.metadata.set", json!({"component":"Panels","properties":{"material":"Oak"}}));
    for (name, material) in [("A", "Steel"), ("B", "Aluminum")] {
        board(&mut s, name, [100.0, 50.0, 20.0]);
        run(&mut s, "material.assign", json!({"bodies":[name],"material":material}));
    }
    let r = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(r["rows"].as_array().unwrap().len(), 2);
    assert_eq!(r["totals"], json!([{"material":"Aluminum","count":1},{"material":"Steel","count":1}]));
}

#[test]
fn a_bought_assembly_excludes_only_its_placement_of_a_shared_board() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Rail"}));
    board(&mut s, "Board", [360.0, 45.0, 18.0]);
    run(&mut s, "component.create", json!({"name":"Bought","parent":0,"activate":false}));
    run(&mut s, "component.metadata.set", json!({"component":"Bought","bom_type":"purchased"}));
    run(&mut s, "occurrence.copy", json!({"component":"Rail","parent":"Bought"}));
    let r = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(sizes(&r), [(1, [360.0, 45.0, 18.0])]);
    assert_eq!(r["excluded"], json!([]));
}

#[test]
fn boards_in_a_multibody_component_keep_their_own_names() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Bench"}));
    board(&mut s, "Rail A", [300.0, 40.0, 20.0]);
    board(&mut s, "Rail B", [300.0, 40.0, 20.0]);
    board(&mut s, "Seat", [100.0, 80.0, 18.0]);
    let r = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(sizes(&r), [(2, [300.0, 40.0, 20.0]), (1, [100.0, 80.0, 18.0])]);
    assert_eq!(r["rows"][0]["name"], "Rail A; Rail B");
    assert_eq!(r["rows"][1]["name"], "Seat");
    let csv = super::csv(&serde_json::from_value::<Vec<crate::schema::cutlist::Row>>(r["rows"].clone()).unwrap());
    assert!(csv.lines().nth(1).unwrap().contains("\"Rail A; Rail B\""), "{csv}");
}

#[test]
fn a_line_counts_a_name_more_than_one_piece_holds() {
    let mut s = Session::default();
    let made = run(&mut s, "component.create", json!({"name":"Leg"}));
    board(&mut s, "Board", [400.0, 40.0, 40.0]);
    run(&mut s, "component.activate", json!({"component":0}));
    let seed = s.doc.occurrences.iter().find(|o| o.component == made["component"].as_u64().unwrap()).unwrap().id;
    run(&mut s, "occurrence.copy", json!({"occurrence":seed, "translate":[0, 100, 0]}));
    board(&mut s, "Post", [400.0, 40.0, 40.0]);
    let r = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(sizes(&r), [(3, [400.0, 40.0, 40.0])]);
    assert_eq!(r["rows"][0]["name"], "Leg (2); Post");
}

#[test]
fn a_body_the_stock_measure_refuses_is_left_out_and_the_rest_still_list() {
    // A 71-sided prism (odd, so no two sides are parallel) has 72 face directions, over the stock measure's limit of 64:
    // it is listed as left out with the reason, and the board beside it is still in the cut list.
    let mut s = Session::default();
    board(&mut s, "Board", [600.0, 90.0, 20.0]);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
    run(&mut s, "sketch.polygon.inscribed", json!({"center": [2000, 0], "radius": 100, "sides": 71}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 50, "body_name": "Many sides"}));
    let r = run(&mut s, "cutlist.parts", json!({}));
    assert_eq!(sizes(&r), vec![(1, [600.0, 90.0, 20.0])]);
    let excluded = r["excluded"].as_array().unwrap();
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded[0]["body"], "Many sides");
    assert!(excluded[0]["reason"].as_str().unwrap().starts_with("stock size could not be measured"));
}
