use super::*;
use crate::schema::cutlist::{Row, Stock};

fn bench() -> CutList {
    let mut list = CutList::default();
    for (item, (name, length, width, count)) in
        [("Top", 1828.8, 762.0, 2), ("End", 878.4, 591.6, 3), ("Rear", 1676.4, 778.4, 1), ("Shelf", 811.2, 591.6, 2)].into_iter().enumerate()
    {
        list.rows.push(Row {
            item: item + 1,
            name: name.into(),
            count,
            material: "Plywood".into(),
            stock: Stock { kind: "panel".into(), length, width, thickness: 18.0, exact: true },
            grain: String::new(),
            bodies: vec![],
        });
        list.total_occurrences += count;
    }
    list
}

#[test]
fn cutting_plan_marks_conserve_pieces_and_match_labels_and_svg() {
    let library = library(None).unwrap();
    let list = bench();
    let plan = build(&list, &library, None, 3.2, 10.0).unwrap();
    assert_eq!(plan.sheets.len(), 4);
    assert_eq!(plan.labels.len(), 8);
    assert!(plan.unplaced.is_empty());
    assert_eq!(plan.to_buy[0].count, 4);
    let marks: std::collections::BTreeSet<_> = plan.labels.iter().map(|l| l.mark.as_str()).collect();
    let placed: std::collections::BTreeSet<_> = plan.sheets.iter().flat_map(|s| &s.placements).map(|p| p.mark.as_str()).collect();
    assert_eq!(marks, placed);
    let diagram = solidcraft_io::workshop::svg(&plan_pages(&plan)).unwrap();
    let labels = solidcraft_io::workshop::svg(&label_pages(&plan)).unwrap();
    assert_eq!(labels.matches("id=\"page-").count(), 8);
    assert_eq!(mark(26, 1), "Z-1");
    assert_eq!(mark(27, 1), "AA-1");
    if let Ok(dir) = std::env::var("SOLIDCRAFT_CUTTING_PLAN_EVIDENCE") {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("bench-plan.svg"), &diagram).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("bench-labels.svg"), &labels).unwrap();
    }
    for mark in marks {
        assert!(diagram.contains(mark) && labels.contains(mark));
    }
    let mut locked = list.clone();
    for row in &mut locked.rows {
        row.grain = "length".into();
    }
    let plan = build(&locked, &library, None, 3.2, 10.0).unwrap();
    assert_eq!(plan.sheets.len(), 5);
    assert!(solidcraft_io::workshop::svg(&label_pages(&plan)).unwrap().contains('→'));
}

#[test]
fn cutting_plan_command_schemas_exports_and_hostile_input() {
    use crate::schema::ParamCheck;
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("solid.box", &json!({"length":500,"width":200,"height":18,"body_name":"Shelf"})).unwrap();
    s.execute("wood.grain", &json!({"body":"Shelf","direction":"length"})).unwrap();
    let undo = s.undo.len();
    let revision = s.revision;
    let value = s.execute(ID, &json!({"stock":["plywood-18-2440x1220"]})).unwrap();
    assert_eq!((s.undo.len(), s.revision), (undo, revision));
    assert_eq!(value["labels"][0]["grain"], "length");
    assert_eq!(value["sheets"][0]["placements"][0]["mark"], "A-1");
    for params in [
        json!({"kerf":0}),
        json!({"kerf":-1}),
        json!({"trim":-1}),
        json!({"kerf":"3 mm"}),
        json!({"stock":["missing"]}),
        json!({"mode":"free"}),
        json!({"path":"out.pdf"}),
        json!({"labels_path":"labels.pdf"}),
        json!({"unknown":1}),
    ] {
        assert!(s.execute(ID, &params).is_err(), "{params}");
    }
    let empty = Session::default().execute(ID, &json!({})).unwrap();
    assert_eq!(empty["labels"], json!([]));
    let dir = std::env::temp_dir().join(format!("solidcraft-cutting-plan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for ext in ["svg", "dxf", "json", "csv"] {
        let path = dir.join(format!("plan.{ext}"));
        let labels = dir.join("labels.svg");
        s.execute(ID, &json!({"path":path,"labels_path":labels})).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("A-1"));
        assert_eq!(std::fs::read_to_string(&labels).unwrap().matches("id=\"page-").count(), 1);
    }
    let custom = dir.join("stock.json");
    let mut lib = library(None).unwrap();
    lib.stock.retain(|s| s.kind == "panel" && s.thickness == 18.0);
    lib.stock[0].length = 100.0;
    std::fs::write(&custom, serde_json::to_vec(&lib).unwrap()).unwrap();
    let no_fit = s.execute(ID, &json!({"library":custom})).unwrap();
    assert_eq!(no_fit["unplaced"].as_array().unwrap().len(), 1);
    assert_eq!(no_fit["labels"].as_array().unwrap().len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cutting_plan_short_blocks_share_the_rafters_cross_section() {
    let mut s = Session::default();
    for (prefix, length, count) in [("Rafter", 1966.0, 3), ("Block", 300.0, 2)] {
        for i in 0..count {
            s.execute("solid.box", &json!({"length":length,"width":152.4,"height":152.4,"body_name":format!("{prefix} {i}")})).unwrap();
        }
    }
    let result = s.execute(ID, &json!({"stock":["timber-6x6-2400"],"kerf":3,"trim":5})).unwrap();
    let plan: Layout = serde_json::from_value(result).unwrap();
    assert_eq!(plan.sheets.len(), 3);
    assert_eq!(plan.labels.len(), 5);
    assert!(plan.unplaced.is_empty());
    assert_eq!(plan.sheets.iter().flat_map(|s| &s.offcuts).map(|r| r.length).collect::<Vec<_>>(), [118.0, 118.0, 421.0]);
}

#[test]
fn cutting_plan_timber_presets_keep_their_cross_sections_including_panel_shaped_boards() {
    for (id, width, thickness) in [
        ("timber-6x6-2400", 152.4, 152.4),
        ("timber-6x10-2400", 254.0, 152.4),
        ("timber-4x4-2400", 101.6, 101.6),
        ("timber-3x4-2400", 101.6, 76.2),
        ("timber-3x3-2400", 76.2, 76.2),
        ("timber-2x4-2400", 101.6, 50.8),
        ("timber-2x3-2400", 76.2, 50.8),
        ("timber-1x4-2400", 101.6, 25.4),
    ] {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":1966,"width":width,"height":thickness,"body_name":"Board"})).unwrap();
        for grain in [false, true] {
            if grain {
                s.execute("wood.grain", &json!({"body":"Board","direction":"length"})).unwrap();
            }
            let value = s.execute(ID, &json!({"stock":[id],"kerf":3,"trim":5})).unwrap();
            let plan: Layout = serde_json::from_value(value).unwrap();
            assert!(plan.unplaced.is_empty(), "{id}, grain={grain}");
            assert_eq!(plan.labels.len(), 1);
            assert_eq!(plan.sheets.len(), 1);
            let sheet = &plan.sheets[0];
            assert!(sheet.linear);
            assert_eq!(sheet.stock, id);
            assert_eq!((sheet.width, sheet.thickness), (width, thickness));
            assert_eq!(sheet.placements[0].length, 1966.0);
            assert_eq!(sheet.offcuts.len(), 1);
            assert_eq!((sheet.offcuts[0].length, sheet.offcuts[0].width), (421.0, width));
            assert!((sheet.yield_ratio - 1966.0 / 2400.0).abs() < 1e-12);
        }
    }
}
