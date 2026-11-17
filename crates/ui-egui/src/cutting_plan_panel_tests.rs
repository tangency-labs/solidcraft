use crate::scenario::Harness;
use serde_json::json;

#[test]
fn cutting_plan_opens_from_cutlist_and_runs_and_invalidates_on_changes() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":800,"width":400,"height":18,"body_name":"Shelf"})).unwrap();
    h.step(&json!({"start":"cutlist.parts"})).unwrap();
    h.frames(2);
    h.step(&json!({"widget":"Cutting Plan"})).unwrap();
    h.frames(3);
    assert!(h.app.cutting_plan.as_ref().unwrap().plan.is_some());
    assert!(h.widget("Export labels SVG…").is_some());
    assert!(h.widget("A-1: 800 × 400 mm").is_some());
    h.step(&json!({"widget":"6x6 timber, 2400 mm"})).unwrap();
    h.frames(2);
    assert!(h.app.cutting_plan.as_ref().unwrap().plan.is_none());
    assert!(!h.app.cutting_plan.as_ref().unwrap().selected.iter().any(|id| id == "timber-6x6-2400"));
    h.step(&json!({"widget":"Run cutting plan"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.cutting_plan.as_ref().unwrap().plan.as_ref().unwrap().labels.len(), 1);
    h.app.run("solid.box", json!({"length":500,"width":200,"height":18,"body_name":"Second"})).unwrap();
    h.frames(2);
    assert!(h.app.cutting_plan.as_ref().unwrap().plan.is_none());
    h.step(&json!({"widget":"Run cutting plan"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.cutting_plan.as_ref().unwrap().plan.as_ref().unwrap().labels.len(), 2);
}

#[test]
fn cutting_plan_sheet_preview_can_be_captured() {
    let mut h = Harness::new();
    for (name, length, width, n) in [("Top", 1828.8, 762.0, 2), ("End", 878.4, 591.6, 3), ("Shelf", 811.2, 591.6, 2), ("Rear", 1676.4, 778.4, 1)] {
        for i in 0..n {
            h.app.run("solid.box", json!({"length":length,"width":width,"height":18,"body_name":format!("{name} {i}")})).unwrap();
        }
    }
    super::open(&mut h.app);
    let ctx = crate::fonts::test_context();
    let mut textures = std::collections::HashMap::new();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0));
    for frame in 0..6 {
        let output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), ..Default::default() }, |ui| {
            super::shared_content(&mut h.app, ui)
        });
        crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
        if frame == 5 {
            assert_eq!(h.app.cutting_plan.as_ref().unwrap().plan.as_ref().unwrap().sheets.len(), 4);
            for number in ["1", "2"] {
                assert!(crate::test_support::text_shapes(&output).any(|text| text.galley.text() == number));
            }
            if let Ok(dir) = std::env::var("SOLIDCRAFT_CUTTING_PLAN_EVIDENCE") {
                let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir);
                std::fs::create_dir_all(&dir).unwrap();
                crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures)
                    .save(std::path::Path::new(&dir).join("cutting-plan.png"))
                    .unwrap();
            }
        }
    }
}

#[test]
fn a_failed_first_run_keeps_every_stock_size_for_the_next_run() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":800,"width":400,"height":18,"body_name":"Shelf"})).unwrap();
    super::open(&mut h.app);
    h.app.cutting_plan.as_mut().unwrap().kerf = "0".into();
    h.frames(2);
    let panel = h.app.cutting_plan.as_ref().unwrap();
    assert!(panel.plan.is_none() && panel.error.is_some());
    h.app.cutting_plan.as_mut().unwrap().kerf = "3.2".into();
    h.step(&json!({"widget":"Run cutting plan"})).unwrap();
    h.frames(2);
    let panel = h.app.cutting_plan.as_ref().unwrap();
    assert!(!panel.selected.is_empty());
    let plan = panel.plan.as_ref().unwrap();
    assert_eq!((plan.labels.len(), plan.unplaced.len()), (1, 0));
}
