//! The navigation bar's style menu: a visual style is picked from the list rather than cycled
//! to, and Translucent draws every body see-through.

use crate::viewport::{LAST_VISUAL_STYLE, TRANSLUCENT, VISUAL_STYLES};
use serde_json::json;

fn a_box(h: &mut crate::scenario::Harness) {
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"body_name":"Block"})).unwrap();
    h.frames(3);
}

#[test]
fn the_menu_offers_every_style_and_picks_translucent() {
    let mut h = crate::scenario::Harness::new();
    a_box(&mut h);
    assert_eq!(h.app.ui.visual_style, 0, "shaded with edges to begin with");
    h.step(&json!({"click": {"handle": "nav:style"}})).unwrap();
    h.frames(2);
    for style in VISUAL_STYLES {
        assert!(h.widget(style).is_some(), "the menu offers {style}");
    }
    h.step(&json!({"click": {"handle": "nav:style:Translucent"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.ui.visual_style, TRANSLUCENT);
}

/// The button opened a menu instead of stepping to the next style, so one click no longer
/// changes what is drawn.
#[test]
fn the_button_opens_the_menu_without_changing_the_style() {
    let mut h = crate::scenario::Harness::new();
    a_box(&mut h);
    h.step(&json!({"click": {"handle": "nav:style"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.ui.visual_style, 0, "opening the menu picks nothing");
    assert!(h.widget("Translucent").is_some(), "the menu is open");
}

#[test]
fn a_saved_style_comes_back_and_a_wild_one_does_not() {
    let mut h = crate::scenario::Harness::new();
    h.app.ui.visual_style = TRANSLUCENT;
    let saved = h.app.prefs();
    h.app.ui.visual_style = 0;
    h.app.load_prefs(&saved);
    assert_eq!(h.app.ui.visual_style, TRANSLUCENT, "translucent survives a restart");
    h.app.load_prefs(&json!({"visual_style": 99}).to_string());
    assert_eq!(h.app.ui.visual_style, LAST_VISUAL_STYLE, "a style this build does not know is clamped");
}
