use super::*;
use crate::scenario::Harness;
use serde_json::json;

/// The default floating layout's bands: a 34-point app bar, the ribbon at 117, 93 or 31, a
/// 44-point History along the bottom and 7-point gaps.
fn floating(history: Option<f32>) -> BandHeights {
    BandHeights { bar: 34.0, labelled: 117.0, icons: 93.0, tabs: 31.0, history, gap: 7.0 }
}

const NO_LABELS: Bands = Bands { labels: false, panels: true, history: true };
const FOLDED: Bands = Bands { labels: false, panels: false, history: true };
const BAREST: Bands = Bands { labels: false, panels: false, history: false };

#[test]
fn the_bands_keep_the_share_of_the_height_they_take_at_the_full_layout() {
    let heights = floating(Some(44.0));
    // 34 + 2·7 + 117 + 7 + 44 = 216 points of 600.
    let share = band_share(&heights);
    assert_eq!(share, 216.0 / 600.0);
    // Each form keeps its room down to the height where it takes that share: 192 / share for
    // the ribbon without labels, 130 / share folded, and below that the History goes.
    let steps = [(600.0, Bands::FULL), (192.0 / share, NO_LABELS), (130.0 / share, FOLDED)];
    for (at, form) in steps {
        assert_eq!(bands(at, &heights, share), form, "at {at}");
    }
    assert_eq!(bands(599.9, &heights, share), NO_LABELS);
    assert_eq!(bands(192.0 / share - 0.01, &heights, share), FOLDED);
    assert_eq!(bands(130.0 / share - 0.01, &heights, share), BAREST);
    assert_eq!(bands(240.0, &heights, share), BAREST, "the smallest window");
    assert_eq!(bands(0.0, &heights, share), BAREST);
    assert_eq!(bands(-10.0, &heights, share), BAREST);
}

#[test]
fn without_a_history_along_the_bottom_the_ribbon_keeps_its_labels_longer() {
    let share = band_share(&floating(Some(44.0)));
    let heights = floating(None);
    // 34 + 2·7 + 117 + 7 = 172 points.
    assert_eq!(bands(172.0 / share, &heights, share), Bands::FULL);
    assert_eq!(bands(172.0 / share - 0.01, &heights, share), Bands { history: true, ..NO_LABELS });
}

#[test]
fn a_window_at_least_the_full_layout_keeps_every_band() {
    // Even bands taller than the default (a status bar's worth) keep their form there: nothing
    // changes in a window the old minimum size allowed.
    let heights = BandHeights { bar: 58.0, ..floating(Some(44.0)) };
    let share = band_share(&floating(Some(44.0)));
    for height in [600.0, 601.0, 2000.0, f32::INFINITY, f32::NAN] {
        assert_eq!(bands(height, &heights, share), Bands::FULL, "{height}");
    }
}

#[test]
fn side_columns_go_widest_first_until_the_rest_take_half_the_width() {
    // Nothing goes at the full layout's width and above, however wide the columns.
    assert!(hidden_columns(960.0, &[700.0, 700.0], 7.0).is_empty());
    assert!(hidden_columns(f32::NAN, &[700.0], 7.0).is_empty());
    // A 250-point Browser stays while it and its gap take at most half: 2 · 257 = 514.
    assert!(hidden_columns(514.0, &[250.0], 7.0).is_empty());
    assert_eq!(hidden_columns(513.9, &[250.0], 7.0), vec![0]);
    // The widest goes first; among equals the earlier (left) one.
    assert_eq!(hidden_columns(900.0, &[250.0, 300.0], 7.0), vec![1]);
    assert_eq!(hidden_columns(900.0, &[250.0, 250.0], 7.0), vec![0]);
    assert_eq!(hidden_columns(500.0, &[250.0, 250.0], 7.0), vec![0, 1]);
    assert_eq!(hidden_columns(320.0, &[56.0, 250.0, 240.0], 7.0), vec![1, 2]);
    // Hostile widths still end.
    assert_eq!(hidden_columns(320.0, &[f32::INFINITY, f32::NAN], 7.0).len(), 2);
    assert!(hidden_columns(-1.0, &[], 7.0).is_empty());
}

#[test]
fn a_card_wider_than_half_a_small_windows_view_goes_along_the_bottom() {
    assert!(card_at_bottom(959.0, 527.0, 264.0));
    assert!(!card_at_bottom(959.0, 528.0, 264.0));
    assert!(!card_at_bottom(960.0, 300.0, 264.0), "the full layout keeps its cards at the side");
    assert!(!card_at_bottom(f32::NAN, 300.0, 264.0));
}

#[test]
fn the_navigator_takes_at_most_a_third_of_a_small_windows_view() {
    assert!(navigator_shown(vec2(960.0, 600.0), vec2(100.0, 100.0), 116.0), "the full layout keeps it");
    assert!(navigator_shown(vec2(800.0, 500.0), vec2(600.0, 348.0), 116.0));
    assert!(!navigator_shown(vec2(800.0, 500.0), vec2(600.0, 347.0), 116.0));
    assert!(!navigator_shown(vec2(320.0, 240.0), vec2(f32::NAN, 100.0), 116.0));
}

/// The sample design in a window `width` × `height`, settled.
fn sample(width: f32, height: f32) -> Harness {
    let mut h = Harness::new();
    h.app = crate::test_support::sample_app();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width": width, "height": height}));
    h.frames(6);
    h
}

fn resize(h: &mut Harness, width: f32, height: f32) {
    h.call("ui.resize", json!({"width": width, "height": height}));
    h.frames(6);
}

fn click(h: &mut Harness, handle: &str) {
    h.step(&json!({"click": {"handle": handle}})).unwrap();
    h.frames(4);
}

#[test]
fn a_short_window_gives_up_the_ribbons_labels_its_panels_then_the_history() {
    let mut h = sample(1280.0, 720.0);
    let prefs = h.app.prefs();
    let card = || crate::scenario::rect_of("ribbon:card").unwrap().height();
    let labelled = card();
    assert!(h.app.shell.history.is_some());
    resize(&mut h, 1280.0, 560.0);
    assert_eq!(h.app.shell.small_window.bands, NO_LABELS);
    assert!(card() < labelled, "the ribbon's buttons lose their labels");
    resize(&mut h, 1280.0, 420.0);
    assert_eq!(h.app.shell.small_window.bands, FOLDED);
    assert!(crate::toolbar::tabs_only(&h.app), "the ribbon folds to its tab row");
    assert!(h.app.shell.history.is_some());
    resize(&mut h, 1280.0, 300.0);
    assert_eq!(h.app.shell.small_window.bands, BAREST);
    assert!(h.app.shell.history.is_none(), "the History goes");
    // Its button shows it again, and a second click hides it again.
    click(&mut h, "small_window:history");
    assert!(h.app.shell.history.is_some(), "shown again");
    click(&mut h, "small_window:history");
    assert!(h.app.shell.history.is_none(), "hidden again");
    // A full-size window has every part back, and nothing of this was saved.
    resize(&mut h, 1280.0, 720.0);
    assert_eq!(h.app.shell.small_window.bands, Bands::FULL);
    assert!(h.app.shell.history.is_some());
    assert_eq!(card(), labelled);
    assert!(crate::scenario::rect_of("small_window:panels").is_none(), "no buttons");
    assert_eq!(h.app.prefs(), prefs, "the preferences and the panel layout are untouched");
}

#[test]
fn a_narrow_window_hides_the_browser_and_its_button_shows_it_again() {
    let mut h = sample(1280.0, 720.0);
    let prefs = h.app.prefs();
    assert!(h.app.shell.browser.is_some());
    resize(&mut h, 480.0, 720.0);
    assert!(h.app.shell.browser.is_none(), "a 250-point Browser would take more than half of 480");
    let buttons = crate::scenario::rect_of("small_window:panels").expect("a button for the Browser");
    let view = h.app.viewport.rect.unwrap();
    assert!(view.contains_rect(buttons), "{buttons:?} in {view:?}");
    click(&mut h, "small_window:browser");
    assert!(h.app.shell.browser.is_some(), "shown again");
    click(&mut h, "small_window:browser");
    assert!(h.app.shell.browser.is_none(), "hidden again");
    resize(&mut h, 1280.0, 720.0);
    assert!(h.app.shell.browser.is_some());
    assert_eq!(h.app.prefs(), prefs, "the preferences and the panel layout are untouched");
}

#[test]
fn a_feature_card_too_wide_for_a_small_view_lies_along_its_bottom() {
    let mut h = sample(480.0, 600.0);
    h.app.start("solid.extrude");
    h.frames(8);
    let card = h.app.viewport.dialog_rect.expect("the Extrude card");
    let window = h.ctx.content_rect();
    assert!(window.contains_rect(card), "{card:?} in {window:?}");
    assert!(card.width() > window.width() * 0.8, "as wide as the view: {card:?}");
    assert!(card.top() >= window.center().y - 1.0, "in the lower half: {card:?}");
    // Its fields have room: all of them show, or the card takes the whole lower half and its
    // body scrolls (it used to keep only its header and footer, #19180 review).
    let body = crate::scenario::rect_of("dialog:body:viewport").expect("the card's body");
    let content = crate::scenario::count_of("dialog:body:content_height").expect("the body's height") as f32;
    let view = h.app.viewport.rect.unwrap();
    assert!(body.height() > 0.0 && (body.height() + 0.5 >= content || card.top() <= view.center().y + 1.0), "{body:?} for {content}: {card:?}");
}

#[test]
fn reveal_buttons_take_as_few_columns_as_fit() {
    // A pitch of 36 and an inset of 4: a column of four buttons is 40 wide and 148 tall.
    let fits = |height: f32| move |size: Vec2| (size.y <= height).then_some(size);
    assert_eq!(button_grid(4, 36.0, 4.0, fits(148.0)), Some((4, vec2(40.0, 148.0))));
    assert_eq!(button_grid(4, 36.0, 4.0, fits(147.0)), Some((2, vec2(76.0, 76.0))));
    assert_eq!(button_grid(3, 36.0, 4.0, fits(100.0)), Some((2, vec2(76.0, 76.0))));
    assert_eq!(button_grid(4, 36.0, 4.0, fits(40.0)), Some((1, vec2(148.0, 40.0))));
    assert_eq!(button_grid(4, 36.0, 4.0, fits(39.0)), None);
    assert_eq!(button_grid(0, 36.0, 4.0, fits(1000.0)), None);
}

/// The smallest window with the status bar on, every side panel and the History hidden: the view
/// is too short for a column of four buttons, and each panel still has its button, which shows it
/// and hides it again (#19180 review).
#[test]
fn the_smallest_window_has_a_button_for_every_hidden_panel() {
    let mut h = sample(1280.0, 720.0);
    h.call("ui.prefs", json!({"status_bar": true}));
    h.call("ui.agent_chat", json!({"open": true, "picker": false}));
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    resize(&mut h, 320.0, 240.0);
    let hidden: Vec<&'static str> = h.app.shell.small_window.hidden.iter().map(|panel| panel.id).collect();
    assert_eq!(hidden.len(), 4, "{hidden:?}");
    let buttons = crate::scenario::rect_of("small_window:panels").expect("the buttons");
    let view = h.app.viewport.rect.unwrap();
    assert!(view.contains_rect(buttons), "{buttons:?} in {view:?}");
    // A panel shown again leaves no room beside it: the buttons go over its corner, and hide it again.
    for id in hidden {
        click(&mut h, &format!("small_window:{id}"));
        assert!(h.app.shell.small_window.revealed.contains(id), "{id} shown again");
        click(&mut h, &format!("small_window:{id}"));
        assert!(!h.app.shell.small_window.revealed.contains(id), "{id} hidden again");
    }
}

#[test]
fn every_part_stays_in_the_window_at_every_size() {
    let mut h = sample(1280.0, 720.0);
    for (width, height) in [(960.0, 600.0), (800.0, 500.0), (640.0, 400.0), (480.0, 320.0), (320.0, 240.0), (320.0, 900.0), (1600.0, 240.0)] {
        resize(&mut h, width, height);
        let window = h.ctx.content_rect();
        let view = h.app.viewport.rect.unwrap_or(egui::Rect::NOTHING);
        assert!(view.is_positive(), "{width}×{height}: a view");
        for handle in ["ribbon:card", "small_window:panels", "axis_view:outer"] {
            if let Some(rect) = crate::scenario::rect_of(handle) {
                assert!(window.expand(0.5).contains_rect(rect), "{width}×{height}: {handle} {rect:?} outside {window:?}");
            }
        }
    }
}

/// With SOLIDCRAFT_SMALL_WINDOW_EVIDENCE set to a folder: pictures of the sample design at sizes
/// from the full layout down to the smallest window.
#[test]
fn pictures_of_the_window_at_every_size() {
    let Some(dir) = std::env::var_os("SOLIDCRAFT_SMALL_WINDOW_EVIDENCE") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let save = |h: &Harness, name: &str| h.picture().save(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
    for (width, height) in [(1280.0, 720.0), (960.0, 600.0), (800.0, 500.0), (640.0, 400.0), (480.0, 320.0), (320.0, 240.0)] {
        save(&sample(width, height), &format!("{width}x{height}"));
    }
    // The Browser shown again with its button, and the Extrude card along the bottom.
    let mut h = sample(480.0, 600.0);
    click(&mut h, "small_window:browser");
    save(&h, "480x600-browser-shown");
    click(&mut h, "small_window:browser");
    h.app.start("solid.extrude");
    h.frames(8);
    save(&h, "480x600-extrude");
}
