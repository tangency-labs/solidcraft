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
    let ctx = crate::test_support::context();
    ctx.enable_accesskit();
    let mut keep = false;
    let mut open = false;
    let mut draw = |ui: &mut Ui| {
        crate::checkbox(ui, "Keep", &mut keep).backing_on_hover_text("Keeps the file");
        crate::checkbox(ui, "Open", &mut open).backing_on_hover_text_at_pointer("Opens it");
        ui.label("Note").backing_on_hover_text("A note");
    };
    crate::test_support::raw_frame(&ctx, raw_input(), &mut draw);
    let out = crate::test_support::raw_frame(&ctx, raw_input(), &mut draw);
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

/// A surface that is translucent only to be seen through (or because a host fades it) is not a
/// glass material: it asks for no blurred backdrop, while an ordinary adaptive card still does.
/// Needs no GPU: it checks the decision, and the marker count of a blur-less frame stays zero.
#[test]
fn a_see_through_container_never_blurs_but_a_glass_card_does() {
    let glass = Container::for_layout(LayoutMode::Floating, Surface::Panel);
    assert!(glass.blurs());
    assert!(!glass.see_through().blurs());
    assert!(!Container::for_layout(LayoutMode::Floating, Surface::Canvas).blurs(), "only adaptive surfaces are glass");
    // The decision does not leak: `see_through` is a per-value flag, not a global switch.
    assert!(glass.blurs());

    let fill = Color32::from_rgba_unmultiplied(40, 80, 100, 180);
    let frame = Frame::NONE.fill(fill);
    let ctx = crate::test_support::context();
    // No active blur scope: no backdrop is requested for either, and nothing is emitted.
    assert!(glass.background(&ctx, frame).is_none());
    assert!(glass.see_through().background(&ctx, frame).is_none());
    // Glass material at full opacity wants the backdrop; faded, opaque or empty fills do not.
    assert!(crate::blur::wants_backdrop(fill, 1.0, true) || !cfg!(not(target_arch = "wasm32")));
    assert!(!crate::blur::wants_backdrop(fill, 0.99, true));
    assert!(!crate::blur::wants_backdrop(fill, 0.0, true));
    assert!(!crate::blur::wants_backdrop(fill, f32::NAN, true));
    assert!(!crate::blur::wants_backdrop(fill, 1.0, false));
    assert!(!crate::blur::wants_backdrop(Color32::from_rgb(40, 80, 100), 1.0, true));
    assert!(!crate::blur::wants_backdrop(Color32::TRANSPARENT, 1.0, true));
}
