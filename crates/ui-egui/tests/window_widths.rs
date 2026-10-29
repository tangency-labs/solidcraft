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
            // Workspace hosts are not command windows; Shots has wide expanded and small collapsed forms.
            .filter(|l| {
                l.order == egui::Order::Middle
                    && ![egui::Id::new("sc_browser_card"), egui::Id::new("sc_render_shots_strip"), egui::Id::new("sc_render_shots_pill")]
                        .contains(&l.id)
            })
            .filter_map(|l| m.area_rect(l.id).map(|r| (l.id, r.width())))
            .collect()
    })
}

fn kit_table_limit(id: egui::Id) -> Option<f32> {
    // The accepted Preferences board has a page list and one control/preview pair at 880 pt.
    if id == egui::Id::new("sc_prefs") {
        return Some(solidcraft_ui_egui::theme::Tokens::get().form_card_width * 10.0 / 3.0);
    }
    [
        egui::Id::new("sc_bom"),
        egui::Id::new("sc_gesture_editor"),
        egui::Id::new("sc_params_dialog"),
        egui::Id::new("sc_material_card_review"),
        egui::Id::new(("sc_dialog", "CONFIGURATIONS")),
        egui::Id::new(("sc_dialog", "MOTION STUDY")),
        egui::Id::new(("sc_dialog", "EDIT MOTION STUDY")),
    ]
    .contains(&id)
    .then(|| solidcraft_ui_egui::theme::Tokens::get().form_card_width * 3.0)
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
    let strip = egui::Id::new("sc_render_shots_strip");
    let width = h.ctx.memory(|m| m.area_rect(strip)).expect("visible Shots strip").width();
    assert!(width > DIALOG_MAX_W, "the workspace strip should span the canvas: {width}");
    let windows = window_widths(&h);
    assert!(!windows.iter().any(|(id, _)| *id == strip));
    assert!(windows.iter().any(|(id, _)| *id == egui::Id::new("sc_about")), "real About window is still measured: {windows:?}");
    assert_eq!(h.call("engine.execute", json!({"command":"view.shots_strip","params":{"visible":false}}))["ok"], true);
    h.frames(6);
    let pill = egui::Id::new("sc_render_shots_pill");
    let width = h.ctx.memory(|m| m.area_rect(pill)).expect("visible Shots restore control").width();
    assert!(width < DIALOG_MIN_W, "the restore control is intentionally smaller than a dialog: {width}");
    let windows = window_widths(&h);
    assert!(!windows.iter().any(|(id, _)| *id == pill));
    assert!(windows.iter().any(|(id, _)| *id == egui::Id::new("sc_about")), "real window remains measured with Shots collapsed: {windows:?}");
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
            let limit = kit_table_limit(layer).unwrap_or(if h.app.dialog.is_some() {
                DIALOG_MAX_W
            } else {
                solidcraft_ui_egui::theme::Tokens::get().form_card_width * 3.0
            });
            if w > limit + slack {
                bad.push(format!("{} ({layer:?}): {w} px", spec.id));
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
        h.app.bom = None;
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
    h.app.prefs_window.open = true;
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
        let limit = kit_table_limit(layer).unwrap_or(solidcraft_ui_egui::theme::Tokens::get().form_card_width * 3.0);
        assert!(w <= limit + 4.0, "{layer:?} is {w} px wide");
    }
}
