use crate::scenario::Harness;
use serde_json::json;

#[test]
fn cut_list_opens_sorts_refreshes_and_uses_display_units() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":254,"width":50.8,"height":25.4,"body_name":"Rail"})).unwrap();
    h.app.run("solid.box", json!({"length":127,"width":50.8,"height":25.4,"body_name":"Short"})).unwrap();
    h.app.session.selection.clear();
    h.app.session.doc_mut().units = "in".into();
    h.app.preferences.length_decimals = 3;
    h.step(&json!({"start":"cutlist.parts"})).unwrap();
    h.frames(4);
    h.app
        .run_panel_command(
            "panel.resize",
            &json!({"target":{"type":"instance","id":"cutlist"},"size":{"width":720,"height":400},"thickness_only":true}),
        )
        .unwrap();
    h.frames(4);
    assert!(h.widget("Export CSV…").is_some());
    assert!(h.widget("Total for Unassigned: 2").is_some());
    assert!(h.widget("10.000 in").unwrap()[1] < h.widget("5.000 in").unwrap()[1]);
    h.step(&json!({"widget":"Length"})).unwrap();
    assert!(h.widget("5.000 in").unwrap()[1] < h.widget("10.000 in").unwrap()[1]);
    h.step(&json!({"widget":"Length"})).unwrap();
    assert!(h.widget("10.000 in").unwrap()[1] < h.widget("5.000 in").unwrap()[1]);
    h.app.run("solid.box", json!({"length":"254 mm","width":"50.8 mm","height":"25.4 mm","body_name":"Copy"})).unwrap();
    h.app.session.selection.clear();
    h.frames(2);
    assert!(h.widget("Total for Unassigned: 3").is_some());
    assert_eq!(h.app.session.execute("cutlist.parts", &json!({})).unwrap()["rows"][0]["stock"]["length"], 254.0);
}

#[test]
fn cut_list_panel_can_be_captured_without_a_desktop_window() {
    let mut h = Harness::new();
    for (name, length, width, count) in [("Top", 1828.8, 762.0, 2), ("End", 878.4, 591.6, 3), ("Shelf", 811.2, 591.6, 2), ("Rear", 1676.4, 778.4, 1)]
    {
        for i in 0..count {
            let name = format!("{name} {i}");
            h.app.run("solid.box", json!({"length":length,"width":width,"height":18,"body_name":name})).unwrap();
        }
    }
    super::open(&mut h.app);
    let ctx = crate::fonts::test_context();
    let mut textures = std::collections::HashMap::new();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
    for frame in 0..6 {
        let output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), ..Default::default() }, |ui| {
            super::shared_content(&mut h.app, ui)
        });
        crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
        if frame == 5 {
            assert!(crate::test_support::text_shapes(&output).any(|t| t.galley.text() == "Total for Unassigned: 8"));
            for text in crate::test_support::text_shapes(&output) {
                assert!(!text.galley.elided, "cut list text is truncated: {}", text.galley.text());
            }
            if let Ok(dir) = std::env::var("SOLIDCRAFT_CUTLIST_EVIDENCE") {
                std::fs::create_dir_all(&dir).unwrap();
                crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures)
                    .save(std::path::Path::new(&dir).join("cut-list.png"))
                    .unwrap();
            }
        }
    }
}

#[test]
fn a_part_the_stock_measure_refuses_shows_why() {
    let reason = super::exclusion_reason("stock size could not be measured: 72 face directions, over the limit of 64");
    assert_eq!(reason, "Its stock size could not be measured");
    assert_eq!(super::exclusion_reason("stock size requires a solid B-rep body"), "A solid B-rep is required");
    assert_eq!(super::exclusion_reason("purchased"), solidcraft_ui_text::tr("cutlist.purchased"));
}

#[test]
fn cut_list_uses_the_bom_ribbon_section_and_a_shared_panel_for_its_document() {
    let command = solidcraft_engine::find_command("cutlist.parts").unwrap();
    let bom = solidcraft_engine::find_command("assembly.bom").unwrap();
    let destination = crate::workspace::destination(command);
    assert!(destination.is_some(), "the cut list has a ribbon entry");
    assert_eq!(destination, crate::workspace::destination(bom));
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":100,"width":50,"height":20})).unwrap();
    h.step(&json!({"start":"cutlist.parts"})).unwrap();
    h.frames(4);
    assert!(super::available(&h.app));
    crate::documents::new_tab(&mut h.app);
    h.frames(4);
    assert!(!super::available(&h.app));
    crate::documents::switch(&mut h.app, 0);
    h.frames(4);
    assert!(super::available(&h.app));
    h.app.run("file.new", json!({})).unwrap();
    h.frames(4);
    assert!(!super::available(&h.app));
}
