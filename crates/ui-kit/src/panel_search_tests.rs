use crate::test_support::*;
use crate::*;

use egui::{Rect, Ui, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_ui_registry as registry;

const TEXT: PanelSearchText<'static> = PanelSearchText { hint: "Find", tip: "Names to find", end: "End search", clear: "Clear" };
const FILTER: PanelSearchFilter<'static> = PanelSearchFilter { icon: "find_kinds", label: "Kinds", handle: "kinds", picked: 2 };

fn bar() -> Rect {
    Rect::from_min_size(pos2(10.0, 20.0), vec2(260.0, 32.0))
}

/// Draws the `demo` box in [`bar`], with or without a filter, and says whether it ended.
fn draw(ui: &mut Ui, query: &mut String, filter: bool, focus: bool) -> bool {
    let search = PanelSearch::new("demo", TEXT, 64).focus(focus);
    let search = if filter { search.filter(FILTER) } else { search };
    search.show(ui, bar(), query).finish(ui, |_| ())
}

#[test]
fn the_box_puts_its_parts_in_order_inside_one_inset_frame() {
    let t = Tokens::get();
    for filter in [true, false] {
        let ctx = context();
        let mut query = String::new();
        frame(&ctx, vec![], |ui| {
            draw(ui, &mut query, filter, false);
        });
        let frame = registry::rect_of("demo:search:frame").unwrap();
        assert_eq!(registry::rect_of("demo:search:bar"), Some(bar()));
        assert!((frame.left() - bar().left() - t.space_6).abs() < 0.01, "inset {frame:?}");
        assert!((frame.height() - t.form_close_size).abs() < 0.01 && (frame.center().y - bar().center().y).abs() < 0.01);
        let field = registry::rect_of("demo:search").unwrap();
        let magnifier = registry::handle_at("demo:magnifier").unwrap();
        let clear = registry::handle_at("demo:search:clear").unwrap();
        assert!(magnifier.x < field.left() && clear.x > field.right(), "magnifier, field, × in order");
        for at in [magnifier, field.center(), clear] {
            assert!(frame.contains(at), "{at:?} inside {frame:?}");
        }
        let button = t.space_24;
        match registry::handle_at("demo:search:kinds") {
            Some(kinds) => {
                assert!(filter && field.right() < kinds.x && kinds.x < clear.x, "the filter sits between the field and the ×");
                assert!((field.right() - (kinds.x - button / 2.0)).abs() < 0.5, "the field reaches the filter");
                assert!(registry::handle_at("demo:search:kinds:badge").is_some(), "two picked: a badge");
            }
            None => {
                assert!(!filter);
                assert!((field.right() - (clear.x - button / 2.0)).abs() < 0.5, "without a filter the field reaches the ×");
            }
        }
    }
}

#[test]
fn the_box_takes_the_keyboard_when_asked_and_ends_on_esc() {
    let ctx = context();
    let mut query = String::new();
    let mut ended = false;
    frame(&ctx, vec![], |ui| ended |= draw(ui, &mut query, true, true));
    frame(&ctx, vec![], |ui| ended |= draw(ui, &mut query, true, false));
    assert!(ctx.memory(|m| m.has_focus(PanelSearch::id("demo"))) && !ended, "the field has the keyboard");
    frame(&ctx, vec![egui::Event::Text("bolt".into())], |ui| ended |= draw(ui, &mut query, true, false));
    assert_eq!(query, "bolt");
    frame(&ctx, key(egui::Key::Escape), |ui| ended |= draw(ui, &mut query, true, false));
    assert!(ended, "Esc in the field ends the search");
}

#[test]
fn the_magnifier_and_the_clear_button_end_the_search() {
    for handle in ["demo:magnifier", "demo:search:clear"] {
        let ctx = context();
        let mut query = "bolt".to_string();
        let mut ended = false;
        frame(&ctx, vec![], |ui| ended |= draw(ui, &mut query, false, false));
        let at = registry::handle_at(handle).unwrap();
        frame(&ctx, pointer(at, true), |ui| ended |= draw(ui, &mut query, false, false));
        frame(&ctx, pointer(at, false), |ui| ended |= draw(ui, &mut query, false, false));
        assert!(ended, "{handle} ends the search");
    }
}

#[test]
fn the_narrowest_box_holds_its_buttons() {
    assert_eq!(PanelSearch::min_width(true), 136.0);
    assert_eq!(PanelSearch::min_width(false), 108.0);
    // The id is the panel's, whatever Ui draws it.
    assert_ne!(PanelSearch::id("demo"), PanelSearch::id("other"));
}
