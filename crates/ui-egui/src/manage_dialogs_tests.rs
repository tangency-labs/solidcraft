//! Manage cards keep their commands, dense chrome and public interaction handles.
use crate::{
    dialogs,
    scenario::{self, Harness},
    shell::LayoutMode,
    theme::Tokens,
};
use serde_json::json;
use solidcraft_engine::Sel;

fn fixture() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width":1600,"height":1200}));
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("parameters.add", json!({"name":"width","expression":"10 mm"})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
    h
}

fn painted_frame(h: &mut Harness) -> egui::FullOutput {
    let mut output = h.ctx.run_ui(egui::RawInput { screen_rect: Some(h.ctx.content_rect()), focused: true, ..Default::default() }, |ui| {
        h.app.logic(ui.ctx());
        h.app.ui(ui);
    });
    output.textures_delta.clear();
    output
}

fn painted_text<'a>(output: &'a egui::FullOutput, text: &str, within: egui::Rect) -> &'a egui::epaint::TextShape {
    fn find<'a>(shape: &'a egui::Shape, text: &str, within: egui::Rect) -> Option<&'a egui::epaint::TextShape> {
        match shape {
            egui::Shape::Text(shape) if shape.galley.job.text == text && within.contains(shape.pos) => Some(shape),

            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, text, within)),
            _ => None,
        }
    }
    output.shapes.iter().find_map(|shape| find(&shape.shape, text, within)).unwrap_or_else(|| panic!("missing painted {text:?} in {within:?}"))
}

#[test]
fn parameter_columns_fit_controls_and_headers_share_cell_left_edges() {
    for layout in [LayoutMode::Docked, LayoutMode::Floating] {
        let mut h = fixture();
        h.call("ui.set", json!({"layout":layout}));
        h.app.start("parameters.change");
        h.frames(4);
        let output = painted_frame(&mut h);
        for label in ["Name", "Unit", "Expression", "Value", "Comment"] {
            let header = scenario::rect_of(&format!("dialog:column:user:{label}")).unwrap();
            let cell = scenario::rect_of(&format!("dialog:table:user:width:cell:{label}")).unwrap();
            assert_eq!(header.left(), cell.left(), "{label}: {layout:?}");
            assert_eq!(header.width(), cell.width());
            let text = painted_text(&output, label, header);
            assert!((text.pos.x - cell.left()).abs() <= 0.5, "{label}: painted header starts at {:?}, cell {cell:?}", text.pos);
            assert!(!text.galley.elided);
        }
        let unit = scenario::rect_of("dialog:table:user:width:cell:Unit").unwrap();
        let mm = painted_text(&output, "mm", unit);
        assert!(!mm.galley.elided, "mm must be painted without an ellipsis");
        let spacing = egui::style::Spacing::default();
        assert!(unit.width() >= mm.galley.size().x + 2.0 * Tokens::get().space_8 + spacing.icon_width + spacing.icon_spacing);
        assert!(mm.pos.x + mm.galley.size().x + Tokens::get().space_8 + spacing.icon_width + spacing.icon_spacing <= unit.right());
        let actions = scenario::rect_of("dialog:table:user:width:cell:Actions").unwrap();
        assert!(actions.contains_rect(scenario::rect_of("dialog:button:Actions:Delete").unwrap()));
        // More content widens the scroll area, never the card.
        h.call("ui.resize", json!({"width":640,"height":1000}));
        h.frames(4);
        assert!(h.ctx.content_rect().contains_rect(scenario::rect_of("dialog:header").unwrap()));
        assert_eq!(scenario::rect_of("dialog:column:user:Unit").unwrap().width(), unit.width());
    }
}

#[test]
fn configuration_columns_align_values_status_and_remove_actions() {
    let mut h = fixture();
    h.app.run("parameters.change", json!({"name":"width","expression":"80 mm"})).unwrap();
    h.app.run("config.configure", json!({"params":["width"]})).unwrap();
    h.app.start("config.table");
    h.frames(4);
    let output = painted_frame(&mut h);
    for label in ["Configuration", "width"] {
        let header = scenario::rect_of(&format!("dialog:column:configuration_table:{label}")).unwrap();
        let cell = scenario::rect_of(&format!("dialog:table:configuration_table:Default:cell:{label}")).unwrap();
        assert_eq!(header.left(), cell.left());
        assert_eq!(header.width(), cell.width());
        assert!((painted_text(&output, label, header).pos.x - header.left()).abs() <= 0.5);
    }
    let name = scenario::rect_of("dialog:table:configuration_table:Default:cell:Configuration").unwrap();
    assert!(!painted_text(&output, "Default", name).galley.elided, "strong names reserve their measured width");
    let active = painted_text(&output, "Active", name);
    assert!(!active.galley.elided);
    assert!(active.pos.x + active.galley.size().x <= name.right());
    let value = scenario::rect_of("dialog:table:configuration_table:Default:cell:width").unwrap();
    assert!(!painted_text(&output, "80 mm", value).galley.elided);
    let remove_cell = scenario::rect_of("dialog:column:configuration_table:width").unwrap();
    assert!(scenario::rect_of("dialog:button:width:×").is_none(), "remove stays hidden until header hover");
    h.step(&json!({"move":[remove_cell.center().x,remove_cell.center().y]})).unwrap();
    h.frames(2);
    let remove = scenario::rect_of("dialog:button:width:×").unwrap();
    assert!(remove_cell.contains_rect(remove));
    h.step(&json!({"click":[remove.center().x,remove.center().y]})).unwrap();
    assert!(h.app.session.doc.configs.columns.is_empty(), "the column action remains interactive");
}

fn chrome(h: &Harness) {
    for key in ["dialog:close", "dialog:ok", "dialog:cancel"] {
        assert!(scenario::handle_at(key).is_some(), "missing {key}");
    }
    assert_eq!(scenario::rect_of("dialog:header").unwrap().height(), Tokens::get().form_header_height);
    assert!(scenario::rect_of("dialog:hint:manage").is_some());
    let footer = scenario::rect_of("dialog:footer").unwrap();
    assert!(h.ctx.content_rect().contains_rect(footer));
}

fn form(command: &str, labels: &[&str]) {
    for layout in [LayoutMode::Docked, LayoutMode::Floating] {
        let mut h = fixture();
        h.call("ui.set", json!({"layout":layout}));
        h.app.start(command);
        h.frames(3);
        chrome(&h);
        for label in labels {
            assert!(scenario::rect_of(&format!("dialog:row:{label}")).is_some(), "{command}: {label}");
        }
        let before = dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        h.frames(3);
        assert_eq!(before, dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap());
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        assert!(h.app.dialog.is_none());
    }
}

#[test]
fn physical_material_is_dense_and_keeps_assignment() {
    form("material.assign", &["Bodies", "Material"]);
    let mut h = fixture();
    h.app.start("material.assign");
    h.frames(3);
    let commands = dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].0, "material.assign");
    assert_eq!(commands[0].1["bodies"], json!(["Body1"]));
    assert!(commands[0].1["material"].is_string());
}

#[test]
fn appearance_is_dense_and_preserves_color_opacity_and_targets() {
    form("appearance.assign", &["Objects", "Apply To", "Colour", "Opacity", "Library", "Search"]);
    let mut h = fixture();
    h.app.run("appearance.assign", json!({"bodies":["Body1"],"color":"#336699","opacity":0.123456})).unwrap();
    h.app.start("appearance.assign");
    let before = dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    h.frames(4);
    let after = dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(before, after);
    assert_eq!(after[0].1["opacity"], 0.123456);
}

#[test]
fn sheet_rule_manager_is_dense() {
    form("sheet.manage_rules", &["Rule", "Thickness", "K Factor", "Active rule"]);
}

#[test]
fn plastic_rule_manager_is_dense() {
    form("plastic.manage_rules", &["Rule", "Material", "Active rule"]);
}

#[test]
fn plastic_assignment_remains_dense() {
    form("plastic.assign_rule", &["Bodies", "Rule"]);
}

fn configurations(command: &str) {
    let mut h = fixture();
    h.app.run("config.configure", json!({"params":["width"]})).unwrap();
    h.app.start(command);
    h.frames(4);
    chrome(&h);
    for label in ["Configuration", "width", "Actions"] {
        assert!(scenario::rect_of(&format!("dialog:column:configuration_table:{label}")).is_some());
    }
    assert_eq!(scenario::rect_of("dialog:table:configuration_table:Default").unwrap().height(), crate::ui_kit::table_row_height());
    assert!(h.widget("Add Column").is_some());
    assert!(h.widget("Add configuration").is_some());
    h.step(&json!({"click":{"handle":"dialog:table:configuration_table:add"}})).unwrap();
    assert_eq!(h.app.session.doc.configs.rows.len(), 1, "add inserts a draft, not a saved row");
    assert_eq!(scenario::rect_of("dialog:table:configuration_table:new").unwrap().height(), crate::ui_kit::table_row_height());
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.configs.rows.len(), 2);
    h.step(&json!({"click":{"handle":"config:cell:Configuration2:param:width"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"25 mm"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_some(), "cell Enter leaves the configuration table open");
    assert_eq!(h.app.session.doc.configs.rows.iter().find(|row| row.name == "Configuration2").unwrap().cells[0], "25 mm");
    assert_eq!(h.app.session.doc.param("width").unwrap().expr, "10 mm", "editing an inactive row does not activate it");
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc.configs.rows.len(), 2);
}

#[test]
fn configuration_table_is_dense_and_adds_rows() {
    configurations("config.table");
}

#[test]
fn configure_alias_is_dense_and_adds_rows() {
    configurations("config.configure");
}

#[test]
fn parameters_are_dense_and_keep_inline_edits_order_and_undo() {
    let mut h = fixture();
    h.app.run("parameters.add", json!({"name":"height","expression":"20 mm"})).unwrap();
    h.app.start("parameters.change");
    h.frames(4);
    chrome(&h);
    for label in ["Name", "Unit", "Expression", "Value", "Comment", "Actions"] {
        assert!(scenario::rect_of(&format!("dialog:column:user:{label}")).is_some());
    }
    h.step(&json!({"click":{"handle":"param:height"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"25 mm"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.param("height").unwrap().expr, "25 mm");
    assert!(h.app.params.is_some(), "cell Enter does not close the editor");
    h.step(&json!({"click":{"handle":"param:before:height"}})).unwrap();
    let names: Vec<_> = h.app.session.doc.params.iter().filter(|p| !p.model).map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["height", "width"]);
    h.app.run("edit.undo", json!({})).unwrap();
    let names: Vec<_> = h.app.session.doc.params.iter().filter(|p| !p.model).map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["width", "height"]);
    h.step(&json!({"click":{"handle":"params:close"}})).unwrap();
    assert!(h.app.params.is_none());
}

#[test]
fn material_review_is_dense_and_cancel_keeps_history() {
    let mut h = fixture();
    let before = (h.app.session.doc.clone(), h.app.session.undo.len());
    h.app.start("material.properties.validate");
    h.frames(3);
    chrome(&h);
    assert!(h.widget("Review card").is_some());
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert!(h.app.material_card_review.is_none());
    assert!(std::sync::Arc::ptr_eq(&before.0, &h.app.session.doc));
    assert_eq!(before.1, h.app.session.undo.len());
}

#[test]
fn wide_managers_leave_the_active_sketch_palette_visible() {
    for command in ["parameters.change", "config.table", "material.properties.validate"] {
        for width in [1000, 1600] {
            let mut h = fixture();
            h.call("ui.resize", json!({"width":width,"height":1000}));
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.frames(3);
            h.app.start(command);
            h.frames(4);
            let palette = h.app.viewport.palette_rect.unwrap();
            let header = scenario::rect_of("dialog:header").unwrap();
            assert!(header.right() <= palette.left(), "{command}, {width}: {header:?} overlaps {palette:?}");
            assert!(h.ctx.content_rect().contains_rect(header));
            assert!(scenario::handle_at("sketch_palette:status_header").is_some());
        }
    }
}

#[test]
fn appearance_card_keeps_the_approved_width_with_no_preselection() {
    let mut h = fixture();
    h.app.session.selection.clear();
    h.app.start("appearance.assign");
    h.frames(4);
    let header = scenario::rect_of("dialog:header").unwrap();
    let card = h.ctx.memory(|m| m.area_rect(egui::Id::new(("sc_dialog", "APPEARANCE")))).unwrap();
    let t = Tokens::get();
    let approved = t.form_card_width + t.form_label_width + t.space_6;
    assert!((card.width() - approved).abs() <= t.space_2, "card {card:?}, header {header:?}");
}

#[test]
fn parameter_inventory_remains_reachable_with_regular_draft_rows() {
    let mut h = fixture();
    h.app.start("parameters.change");
    h.frames(4);
    for label in ["Filter", "Import…", "Export CSV…", "Export JSON…", "User Parameters", "Model Parameters"] {
        assert!(h.widget(label).is_some(), "missing inventory control {label}");
    }
    h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
    assert_eq!(scenario::rect_of("dialog:table:user:new").unwrap().height(), crate::ui_kit::table_row_height());
    for label in ["name", "unit", "expression", "comment"] {
        assert!(scenario::handle_at(&format!("param:new:{label}")).is_some());
    }
    assert_eq!(h.app.params.as_ref().unwrap().new_expr, "", "a new parameter's expression starts blank (#2919)");
    h.step(&json!({"text":"pitch"})).unwrap();
    // Enter after the name goes on to the blank expression, and Enter there adds the parameter.
    h.step(&json!({"key":"Enter"})).unwrap();
    h.step(&json!({"text":"10 mm"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.param("pitch").unwrap().expr, "10 mm");
    assert!(h.app.params.is_some());
    h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
    h.step(&json!({"click":{"handle":"param:new:unit"}})).unwrap();
    for unit in ["mm", "cm", "m", "in", "ft", "deg", "rad", "none", "um"] {
        assert!(scenario::handle_at(&format!("dialog:Unit:{unit}")).is_some(), "unit {unit}");
    }
    h.step(&json!({"click":{"handle":"dialog:Unit:mm"}})).unwrap();
    h.step(&json!({"click":{"handle":"param:new:expression"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"missing_parameter"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(scenario::count_of("param:new:valid"), Some(0.0));
    assert!(h.app.params.as_ref().unwrap().error.is_some());
    assert_eq!(h.app.session.doc.params.iter().filter(|p| !p.model).count(), 2);
    h.step(&json!({"click":{"handle":"dialog:Filter"}})).unwrap();
    assert!(!h.app.params.as_ref().unwrap().adding, "an abandoned nameless draft disappears");
    h.frames(40); // Separate the rename gesture from preceding clicks (egui also recognizes triple-clicks).
    h.step(&json!({"click":{"handle":"param:name:width"},"double":true})).unwrap();
    h.frames(2);
    assert!(h.app.params.as_ref().unwrap().renaming.is_some(), "double click enters rename mode");
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"span"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.session.doc.param("span").is_some(), "double-click rename still commits");
    h.step(&json!({"click":{"handle":"param:star:span"}})).unwrap();
    assert!(h.widget("Favorites").is_some());
    assert!(scenario::handle_at("param:favorite:span").is_some());
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert!(h.app.params.is_none());
    assert!(h.app.session.doc.param("pitch").is_some(), "Cancel keeps committed edits");
}

#[test]
fn configuration_inventory_keeps_library_rules_preview_and_inline_row_lifecycle() {
    let mut h = fixture();
    h.app.run("config.configure", json!({"params":["width"],"materials":["Body1"],"appearances":["Body1"]})).unwrap();
    h.app.start("config.table");
    h.frames(4);
    assert!(h.widget("Active").is_some());
    assert!(scenario::rect_of("dialog:menu:Library").is_some());
    h.step(&json!({"click":{"handle":"dialog:menu:Library"}})).unwrap();
    for label in ["Follow material", "ABS - Blue"] {
        assert!(h.widget(label).is_some(), "missing library option {label}");
    }
    h.step(&json!({"widget":"Follow material"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:menu:Add Column"}})).unwrap();
    assert!(h.widget("Face Appearance").is_some());
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.dialog.is_some(), "menu Escape leaves the card open");
    h.step(&json!({"click":{"handle":"dialog:table:configuration_table:add"}})).unwrap();
    h.step(&json!({"text":"Wide"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    let rows = &h.app.session.doc.configs.rows;
    let default = rows.iter().find(|row| row.name == "Default").unwrap();
    let wide = rows.iter().find(|row| row.name == "Wide").unwrap();
    assert_eq!(wide.cells, default.cells, "new rows copy the active configuration");
    h.step(&json!({"click":{"handle":"config:activate:Wide"}})).unwrap();
    assert_eq!(h.app.session.doc.configs.active.as_deref(), Some("Wide"));
    h.step(&json!({"click":{"handle":"dialog:table:configuration_table:add"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:section:Add or edit rule"}})).unwrap();
    h.frames(3);
    assert!(scenario::rect_of("dialog:table:configuration_table:new").is_none(), "abandoned empty row is dropped");
    assert!(h.widget("Enabled").is_some());
    for label in ["Rule name", "Length units", "When", "Then"] {
        assert!(scenario::rect_of(&format!("dialog:row:{label}")).is_some(), "rule inventory {label}");
    }
    assert!(scenario::handle_at("config:save_rule").is_some());
    h.step(&json!({"click":{"handle":"dialog:section:Add or edit rule"}})).unwrap();
    h.step(&json!({"click":{"handle":"config:preview_row"}})).unwrap();
    assert!(h.widget("Preview: Default").is_some());
    h.step(&json!({"click":{"handle":"dialog:collapse"}})).unwrap();
    assert!(scenario::handle_at("dialog:ok").is_some(), "fold retains footer actions");
    h.step(&json!({"click":{"handle":"dialog:collapse"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:button:Actions:Delete"}})).unwrap();
    assert_eq!(h.app.session.doc.configs.rows.len(), 1, "bin retains immediate deletion");
}

#[test]
fn draft_bins_do_not_delete_saved_rows_named_new() {
    for command in ["parameters.change", "config.table"] {
        let mut h = fixture();
        h.app.run("parameters.add", json!({"name":"new","expression":"12 mm"})).unwrap();
        if command == "config.table" {
            h.app.run("config.configure", json!({"params":["width"]})).unwrap();
            h.app.run("config.row", json!({"name":"new","from":"Default"})).unwrap();
        }
        h.app.start(command);
        h.frames(4);
        let add = if command == "config.table" { "dialog:table:configuration_table:add" } else { "dialog:table:user:add" };
        h.step(&json!({"click":{"handle":add}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:button:Actions:Delete"}})).unwrap();
        assert!(h.app.session.doc.param("new").is_some());
        if command == "config.table" {
            assert!(h.app.session.doc.configs.rows.iter().any(|row| row.name == "new"));
        } else {
            assert!(!h.app.params.as_ref().unwrap().adding);
        }
    }
}

#[test]
fn invalid_named_parameter_draft_stays_pending_until_the_expression_is_valid() {
    let mut h = fixture();
    h.app.start("parameters.change");
    h.frames(4);
    h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
    h.step(&json!({"click":{"handle":"param:new:expression"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"missing_parameter"})).unwrap();
    h.step(&json!({"click":{"handle":"param:new:name"}})).unwrap();
    h.step(&json!({"text":"pitch"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Filter"}})).unwrap();
    assert!(h.app.params.as_ref().unwrap().adding);
    assert!(h.app.session.doc.param("pitch").is_none());
    assert_eq!(scenario::count_of("param:new:valid"), Some(0.0));
    h.step(&json!({"click":{"handle":"param:new:expression"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"5 mm"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.param("pitch").unwrap().expr, "5 mm");
    assert!(h.app.params.as_ref().unwrap().adding);
    assert!(h.app.params.as_ref().unwrap().new_name.is_empty());
    assert!(scenario::handle_at("param:new:name").is_some(), "successful add focuses the next draft name");
}

fn scroll_card(h: &mut Harness, delta: f32) {
    let viewport = scenario::rect_of("dialog:body:viewport").unwrap();
    h.call("ui.scroll", json!({"x":viewport.center().x,"y":viewport.center().y,"delta":delta}));
    h.frames(40);
}

fn assert_card_top() {
    assert_eq!(scenario::count_of("dialog:body:scroll_y"), Some(0.0));
    let viewport = scenario::rect_of("dialog:body:viewport").unwrap();
    assert!(viewport.contains_rect(scenario::rect_of("dialog:hint:manage").unwrap()));
}

#[test]
fn parameters_open_at_top_retain_user_scroll_and_reveal_only_requested_drafts() {
    for layout in [LayoutMode::Docked, LayoutMode::Floating] {
        let mut h = fixture();
        h.call("ui.set", json!({"layout":layout}));
        for _ in 0..7 {
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        }
        for i in 0..18 {
            h.app.run("parameters.add", json!({"name":format!("user{i}"),"expression":"10 mm"})).unwrap();
        }
        h.app.run("parameters.favorite", json!({"name":"width","favorite":true})).unwrap();
        h.app.start("parameters.change");
        h.frames(8);
        assert_card_top();
        let viewport = scenario::rect_of("dialog:body:viewport").unwrap();
        assert!(viewport.contains_rect(scenario::rect_of("dialog:row:Filter").unwrap()));
        assert!(viewport.contains_rect(scenario::rect_of("dialog:section:Favorites").unwrap()));
        assert!(!h.app.params.as_ref().unwrap().adding);
        assert!(h.ctx.memory(|m| m.focused().is_none()));
        scroll_card(&mut h, -400.0);
        let offset = scenario::count_of("dialog:body:scroll_y").unwrap();
        assert!(offset > 0.0);
        h.frames(8);
        assert_eq!(scenario::count_of("dialog:body:scroll_y"), Some(offset));
        h.step(&json!({"click":{"handle":"params:close"}})).unwrap();
        h.app.start("parameters.change");
        h.frames(8);
        assert_card_top();
        // Put the add prompt at the bottom edge. The taller draft must be revealed only
        // after clicking it, with focus on Name even inside the horizontal table scroller.
        let prompt = scenario::rect_of("dialog:table:user:add").unwrap();
        scroll_card(&mut h, viewport.bottom() - prompt.bottom() - 1.0);
        let before = scenario::count_of("dialog:body:scroll_y").unwrap();
        h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
        h.frames(40);
        assert!(h.app.params.as_ref().unwrap().adding);
        let name = scenario::handle_at("param:new:name").unwrap();
        let focused = h.ctx.memory(|m| m.focused()).and_then(|id| h.ctx.read_response(id)).unwrap();
        assert!(focused.rect.contains(name));
        assert!(scenario::rect_of("dialog:body:viewport").unwrap().contains_rect(focused.rect));
        assert!(scenario::count_of("dialog:body:scroll_y").unwrap() > before);
        h.step(&json!({"text":"pitch"})).unwrap();
        assert_eq!(h.app.params.as_ref().unwrap().new_name, "pitch");
        h.step(&json!({"key":"Enter"})).unwrap();
        h.step(&json!({"text":"10 mm"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.session.doc.param("pitch").unwrap().expr, "10 mm");
    }
}

#[test]
fn configuration_and_appearance_open_at_top_and_retain_user_scroll() {
    for command in ["config.table", "appearance.assign"] {
        let mut h = fixture();
        h.call("ui.resize", json!({"width":1600,"height":650}));
        if command == "config.table" {
            h.app.run("config.configure", json!({"params":["width"]})).unwrap();
            for i in 0..20 {
                h.app.run("config.row", json!({"name":format!("Row{i}"),"from":"Default"})).unwrap();
            }
        }
        h.app.start(command);
        h.frames(8);
        assert_card_top();
        scroll_card(&mut h, -250.0);
        let offset = scenario::count_of("dialog:body:scroll_y").unwrap();
        assert!(offset > 0.0, "{command} fixture must overflow");
        h.frames(8);
        assert_eq!(scenario::count_of("dialog:body:scroll_y"), Some(offset));
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        h.app.start(command);
        h.frames(8);
        assert_card_top();
    }
}

#[test]
fn parameters_header_drag_retains_offset_and_bounds_the_card() {
    for layout in [LayoutMode::Docked, LayoutMode::Floating] {
        let mut h = fixture();
        h.call("ui.set", json!({"layout":layout}));
        h.app.start("parameters.change");
        h.frames(6);
        let rest = scenario::rect_of("dialog:header").unwrap();
        let title = scenario::handle_at("params:title").unwrap();
        h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":title.x-180.0,"y1":title.y+100.0,"steps":8}));
        h.frames(6);
        let moved = scenario::rect_of("dialog:header").unwrap();
        assert!((moved.min - rest.min - egui::vec2(-180.0, 100.0)).length() < 2.0, "{layout:?}: {rest:?} -> {moved:?}");
        h.frames(6);
        assert_eq!(moved, scenario::rect_of("dialog:header").unwrap(), "offset survives idle frames");
        for delta in [egui::vec2(3000.0, 3000.0), egui::vec2(-3000.0, -3000.0)] {
            let title = scenario::handle_at("params:title").unwrap();
            h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":title.x+delta.x,"y1":title.y+delta.y,"steps":8}));
            h.frames(6);
            let card = scenario::rect_of("dialog:header").unwrap().union(scenario::rect_of("dialog:footer").unwrap());
            let viewport = crate::shell::tool_drag_rect(&h.app, crate::shell::content_rect(&h.app, h.app.viewport.rect.unwrap()));
            assert!(viewport.expand(2.0).contains_rect(card), "{layout:?}: {card:?} outside {viewport:?}");
            assert!(h.app.params.is_some());
        }
    }
}
