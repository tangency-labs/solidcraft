//! Every command dialog and shared window stays within its content-width limits: started
//! through the control channel (the toolbar's way) and measured as drawn.

use serde_json::json;
use solidcraft_ui_egui::frame::{DIALOG_MAX_W, DIALOG_MIN_W};

use solidcraft_ui_egui::scenario::Harness;

/// Widths of the windows on screen (their areas in the middle layer).
fn window_widths(h: &Harness) -> Vec<(egui::Id, f32)> {
    h.ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.order == egui::Order::Middle)
            .filter_map(|l| m.area_rect(l.id).map(|r| (l.id, r.width())))
            .collect()
    })
}

fn kit_table_limit(h: &Harness, id: egui::Id) -> Option<f32> {
    // The accepted Preferences board has a page list and one control/preview pair at 880 pt.
    if id == egui::Id::unique("sc_prefs") {
        return Some(solidcraft_theme::Tokens::get().form_card_width * 10.0 / 3.0);
    }
    if id == egui::Id::unique("sc_params_dialog") {
        use solidcraft_ui_egui::ui_kit::{PageSwitch, icon_label_width};
        let t = solidcraft_theme::Tokens::get();
        let ui = egui::Ui::new(h.ctx.clone(), egui::Id::unique("parameter_width_probe"), egui::UiBuilder::new().max_rect(h.ctx.content_rect()));
        let labels = [solidcraft_ui_text::tr("parameters.redesign.named"), solidcraft_ui_text::tr("parameters.redesign.dimensions")];
        let counts = ["params:named:count", "params:dimensions:count"].map(|key| solidcraft_ui_egui::scenario::count_of(key).unwrap() as usize);
        let navigation = PageSwitch::new("parameter_width_probe", &labels).counts(&counts).width(&ui);
        let transfers: f32 = ["parameters.redesign.import", "parameters.redesign.export_csv", "parameters.redesign.export_json"]
            .into_iter()
            .map(|key| {
                icon_label_width(&ui, solidcraft_ui_text::tr(key), Some(if key.ends_with("import") { "import_tray" } else { "export_tray" }), false)
            })
            .sum();
        // Named/Dimensions adds counted navigation to the labelled transfer actions, overflow
        // button and 200-point search field. Bound the card by that intrinsic content plus its
        // six header gaps and card insets, rather than the old three-card cap or a larger constant.
        let search = t.space_24 * 8.0 + t.space_8;
        let header = navigation + transfers + t.form_close_size + search + t.space_6 * 6.0;
        return Some((t.form_card_width * 3.0).max(header + t.space_12 * 2.0 + t.space_16));
    }
    [
        egui::Id::unique("sc_bom"),
        egui::Id::unique("sc_gesture_editor"),
        egui::Id::unique("sc_material_card_review"),
        egui::Id::unique(("sc_dialog", "CONFIGURATIONS")),
        egui::Id::unique(("sc_dialog", "MOTION STUDY")),
        egui::Id::unique(("sc_dialog", "EDIT MOTION STUDY")),
    ]
    .contains(&id)
    .then(|| solidcraft_theme::Tokens::get().form_card_width * 3.0)
}

fn assert_parameter_header_fits(h: &Harness) {
    let card = h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_params_dialog"))).unwrap();
    assert!(h.ctx.content_rect().contains_rect(card), "Parameters stays inside the window: {card:?}");
    let mut previous_right = card.left();
    for key in [
        "params:view:named",
        "params:view:dimensions",
        "params:header:actions",
        "params:import",
        "params:export_csv",
        "params:export_json",
        "dialog:Filter",
    ] {
        let control = solidcraft_ui_egui::scenario::rect_of(key).unwrap();
        assert!(card.contains_rect(control), "{key} is clipped: {control:?} in {card:?}");
        assert!(control.left() >= previous_right, "{key} overlaps the preceding header control");
        previous_right = control.right();
    }
}

fn fresh() -> Harness {
    let mut h = Harness::new();
    h.call("engine.execute", json!({"command": "solid.box", "params": {"length": 40, "width": 30, "height": 20}}));
    h.frames(3);
    h
}

#[test]
fn render_strip_is_excluded_while_real_windows_are_still_measured() {
    let mut h = fresh();
    assert_eq!(h.call("ui.workspace", json!({"workspace":"render"}))["ok"], true);
    h.app.help.about = true;
    h.frames(6);
    let shots_width = |h: &Harness| {
        h.app
            .universal_panel_frame()
            .unwrap()
            .layout
            .sections
            .iter()
            .find(|section| section.instance.0 == "render-shots")
            .map(|section| section.outer.size.width)
    };
    let width = shots_width(&h).expect("visible Shots strip");
    assert!(width > DIALOG_MAX_W, "the workspace strip should span the canvas: {width}");
    let windows = window_widths(&h);
    assert!(windows.iter().all(|(_, window_width)| *window_width < width), "the strip is not a dialog: {windows:?}");
    assert!(windows.iter().any(|(id, _)| *id == egui::Id::unique("sc_about")), "real About window is still measured: {windows:?}");
    assert_eq!(h.call("engine.execute", json!({"command":"view.shots_strip","params":{"visible":false}}))["ok"], true);
    h.frames(6);
    assert!(shots_width(&h).is_none(), "the shared strip is hidden");
    let windows = window_widths(&h);
    assert!(windows.iter().any(|(id, _)| *id == egui::Id::unique("sc_about")), "real window remains measured with Shots hidden: {windows:?}");
    assert_eq!(h.call("ui.start", json!({"command":"file.export_view_dxf"}))["ok"], true);
    h.frames(6);
    let dialog = solidcraft_ui_egui::scenario::rect_of("dialog:outer").expect("DXF dialog has an outer rectangle");
    assert!(dialog.width() >= DIALOG_MIN_W - 4.0, "actual DXF dialog remains at least the dialog minimum: {dialog:?}");
}

#[test]
fn command_dialogs_shrink_to_their_content_within_limits() {
    let mut h = fresh();
    let mut bad = Vec::new();
    // The frame border and shadow add a little to the content limits.
    let slack = 4.0;
    for spec in solidcraft_engine::command_specs() {
        h.call("ui.start", json!({"command": spec.id}));
        h.frames(4);
        for (layer, w) in window_widths(&h) {
            // Floating windows a command opened (Parameters…) have their own limit.
            let limit = kit_table_limit(&h, layer).unwrap_or(if h.app.dialog.is_some() {
                DIALOG_MAX_W
            } else {
                solidcraft_theme::Tokens::get().form_card_width * 3.0
            });
            if w > limit + slack {
                bad.push(format!("{} ({layer:?}): {w} px", spec.id));
            }
            if layer == egui::Id::unique("sc_params_dialog") {
                assert_parameter_header_fits(&h);
            }
        }
        // The docked dialog shrinks to its content: most dialogs stay well under the maximum.
        if let Some(rect) = solidcraft_ui_egui::scenario::rect_of("dialog:outer").filter(|_| h.app.dialog.is_some()) {
            let w = rect.width();
            if w < DIALOG_MIN_W - slack {
                bad.push(format!("{}: active dialog {w} px is narrower than {DIALOG_MIN_W}", spec.id));
            }
        }
        h.app.dialog = None;
        h.app.params = None;
        h.app.design_properties = None;
        h.app.sketch_report = None;
        h.app.design_report = None;
        h.app.bom = None;
        // Properties (Inspect → Measure) opens the Properties card.
        h.app.menu.props = None;
        if spec.id == "material.properties.validate" {
            h.step(&json!({"click":{"handle":"dialog:close"}})).unwrap();
        }
        h.app.tool = None;
        if h.app.session.active_sketch.is_some() {
            h.app.finish_sketch();
        }
        h.frames(2);
    }
    assert!(bad.is_empty(), "windows too wide or too narrow:\n{}", bad.join("\n"));
}

#[test]
fn common_dialogs_are_not_stretched() {
    // Simple dialogs have room to spare: they must not be pushed out to the maximum.
    let mut h = fresh();
    for id in ["solid.extrude", "solid.fillet", "solid.shell", "solid.move", "solid.hole"] {
        h.call("ui.start", json!({"command": id}));
        h.frames(6);
        let w: f32 = window_widths(&h).iter().map(|x| x.1).fold(0.0, f32::max);
        assert!(w < DIALOG_MAX_W - 20.0, "{id} is stretched to {w} px");
        h.app.dialog = None;
        h.frames(2);
    }
}

#[test]
fn floating_windows_stay_within_limits() {
    let mut h = fresh();
    h.app.prefs_window.shell.open = true;
    h.app.keymap.open = true;
    h.app.help.about = true;
    h.app.ui.palette_open = true;
    solidcraft_ui_egui::params_dialog::open(&mut h.app);
    solidcraft_ui_egui::design_properties::open(&mut h.app);
    h.frames(6);
    let ws = window_widths(&h);
    // Keyboard Shortcuts is a page of Preferences (one window, #36).
    assert!(ws.len() >= 3, "windows on screen: {ws:?}");
    for (layer, w) in ws {
        let limit = kit_table_limit(&h, layer).unwrap_or(solidcraft_theme::Tokens::get().form_card_width * 3.0);
        assert!(w <= limit + 4.0, "{layer:?} is {w} px wide");
        if layer == egui::Id::unique("sc_params_dialog") {
            assert_parameter_header_fits(&h);
        }
    }
}
