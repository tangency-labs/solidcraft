use super::*;
use egui::Ui;

fn raw_input() -> egui::RawInput {
    egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 400.0))), focused: true, ..Default::default() }
}

/// A caller converted from egui's `on_hover_text` keeps what that gave screen readers: the
/// tip is the widget's description, and its name when it had none. A label is named by its
/// text (its accesskit value), so it only gains the description.
#[test]
fn backing_hover_text_describes_the_widget_for_accessibility() {
    let ctx = crate::fonts::test_context();
    ctx.enable_accesskit();
    let mut keep = false;
    let mut open = false;
    let mut draw = |ui: &mut Ui| {
        crate::ui_kit::checkbox(ui, "Keep", &mut keep).backing_on_hover_text("Keeps the file");
        crate::ui_kit::checkbox(ui, "Open", &mut open).backing_on_hover_text_at_pointer("Opens it");
        ui.label("Note").backing_on_hover_text("A note");
    };
    crate::test_support::frame(&ctx, raw_input(), &mut draw);
    let out = crate::test_support::frame(&ctx, raw_input(), &mut draw);
    let tree = out.platform_output.accesskit_update.expect("accesskit is on");
    // A checkbox is named by its label, a label by its value (its text run is a child node).
    let described = |role: egui::accesskit::Role, name: &str| {
        tree.nodes
            .iter()
            .find(|(_, n)| n.role() == role && (n.label() == Some(name) || n.value() == Some(name)))
            .map(|(_, n)| n.description().map(str::to_owned))
    };
    assert_eq!(described(egui::accesskit::Role::CheckBox, "Keep"), Some(Some("Keeps the file".into())));
    assert_eq!(described(egui::accesskit::Role::CheckBox, "Open"), Some(Some("Opens it".into())));
    assert_eq!(described(egui::accesskit::Role::Label, "Note"), Some(Some("A note".into())));
}
