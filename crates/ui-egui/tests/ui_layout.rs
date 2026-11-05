//! Layout checks over every toolbar command, headless (see `solidcraft_ui_egui::scenario`):
//! each command's dialog opens docked at the viewport's right within its width limits (or in
//! the left column, when the preferences put it there), and every window it opens fits on screen.

use serde_json::{Value, json};
use solidcraft_ui_egui::scenario::{Harness, check, rect_of};

/// A box body (its sketch finished) to give feature dialogs something to work on.
fn harness() -> Harness {
    let mut h = Harness::new();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 10})),
    ] {
        h.call("engine.execute", json!({"command": id, "params": p}));
    }
    h.frames(3);
    h
}

/// These tests and the scenarios name English words, whatever the PC's language (#10297).
#[test]
fn the_harness_takes_english_as_the_system_language() {
    let h = Harness::new();
    assert_eq!(h.app.preferences.language, solidcraft_ui_egui::i18n::SYSTEM);
    assert_eq!(solidcraft_ui_egui::i18n::effective_language(&h.app.preferences.language), "en");
}

#[test]
fn every_dialog_opens_docked_within_its_width_limits() {
    let mut bad = Vec::new();
    let mut seen = 0;
    for spec in solidcraft_engine::command_specs() {
        if spec.tab.is_empty() || spec.tab == "SKETCH" {
            continue;
        }
        let mut h = harness();
        h.call("ui.start", json!({"command": spec.id}));
        h.frames(4);
        if h.app.dialog.is_some() {
            seen += 1;
            // Wide cards use the kit's bounded table/timeline width.
            let w = rect_of("dialog:outer").map_or(0.0, |r| r.width());
            let limits = if matches!(spec.id, "config.table" | "config.configure" | "motion.study") {
                let width = solidcraft_ui_egui::theme::Tokens::get().form_card_width * 3.0;
                json!({"min":width - 4.0,"max":width + 2.0})
            } else if w > 400.0 {
                json!({"min":480,"max":562})
            } else {
                json!({"min":260,"max":382})
            };
            if let Err(e) = check(&mut h, &json!({ "dialog_docked": limits })) {
                bad.push(format!("{}: {e}", spec.id));
            }
        }
        if let Err(e) = check(&mut h, &json!({"windows_fit": {"max_width": 900}})) {
            bad.push(format!("{}: {e}", spec.id));
        }
    }
    assert!(seen > 20, "only {seen} commands opened a dialog");
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.join("\n"));
}

/// Dialogs › In the left column (#6707): every command's dialog opens in the browser's place, at
/// least as wide as the browser and its own card, and fits on screen.
#[test]
fn every_dialog_opens_in_the_left_column_when_asked() {
    let mut bad = Vec::new();
    let mut seen = 0;
    for spec in solidcraft_engine::command_specs() {
        if spec.tab.is_empty() || spec.tab == "SKETCH" {
            continue;
        }
        let mut h = harness();
        h.call("ui.prefs", json!({"dialog_place": "left_column"}));
        h.frames(2);
        let Some(browser) = rect_of("browser:card") else {
            bad.push(format!("{}: no browser before the dialog", spec.id));
            continue;
        };
        h.call("ui.start", json!({"command": spec.id}));
        h.frames(4);
        let (Some(dialog), Some(column)) = (rect_of("dialog:outer").filter(|_| h.app.dialog.is_some()), rect_of("dialog:column")) else {
            continue;
        };
        seen += 1;
        let screen = h.ctx.content_rect();
        if (dialog.left() - browser.left()).abs() > 1.0 || (dialog.top() - browser.top()).abs() > 1.0 {
            bad.push(format!("{}: dialog {dialog:?} is not in the browser's place {browser:?}", spec.id));
        }
        if dialog.width() < browser.width().max(260.0) - 2.5 || (dialog.width() - column.width()).abs() > 2.5 {
            bad.push(format!("{}: dialog width {:.0}, column {:.0}, browser {:.0}", spec.id, dialog.width(), column.width(), browser.width()));
        }
        if !screen.expand(1.0).contains_rect(dialog) || rect_of("browser:card").is_some() {
            bad.push(format!("{}: dialog {dialog:?} off screen or beside a browser", spec.id));
        }
    }
    assert!(seen > 20, "only {seen} commands opened a dialog in the column");
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.join("\n"));
}

/// Windows that are not command dialogs: each fits on screen.
#[test]
fn other_windows_fit_on_screen() {
    let mut bad = Vec::new();
    let opens: [(&str, Value); 3] = [("ui.help", json!({"item": "about"})), ("ui.key", json!({"key": "S"})), ("ui.prefs", json!({"open": true}))];
    for (m, p) in opens {
        let mut h = harness();
        h.call(m, p.clone());
        h.frames(4);
        if let Err(e) = check(&mut h, &json!({"windows_fit": {"max_width": 900}})) {
            bad.push(format!("{m} {p}: {e}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The docked ribbon reserves the same space in both themes and at narrow widths. The
/// existing panel menus and S box remain usable through the new painted hit targets.
#[test]
fn docked_ribbon_geometry_and_search() {
    use solidcraft_ui_egui::{scenario::handle_at, toolbar};
    for theme in ["light", "dark"] {
        let mut h = harness();
        h.call("ui.set", json!({"layout":"docked"}));
        h.app.preferences.theme = theme.into();
        for width in [1600.0, 640.0, 320.0] {
            h.call("ui.resize", json!({"width":width,"height":1000}));
            h.frames(4);
            let ribbon = rect_of("toolbar").unwrap();
            let workspace = rect_of("ribbon:workspace").unwrap();
            let search = rect_of("ribbon:search").unwrap();
            assert_eq!(ribbon.top(), toolbar::APP_BAR_HEIGHT);
            assert_eq!(ribbon.height(), 117.0);
            assert_eq!(ribbon.width(), width);
            assert_eq!(workspace.width(), 88.0);
            assert_eq!(workspace.top(), ribbon.top());
            assert_eq!(workspace.bottom(), ribbon.bottom() - 1.0);
            assert_eq!(toolbar::TAB_ROW_HEIGHT, 30.0);
            assert_eq!(search.height(), 22.0);
            assert_eq!(search.top(), ribbon.top() + 4.0);
            assert_eq!(search.right(), ribbon.right() - 4.0);
            assert_eq!(search.center().y, ribbon.top() + 15.0);
            let t = solidcraft_ui_egui::theme::Tokens::get();
            let (hint_width, key_width) = h.ctx.fonts_mut(|fonts| {
                (
                    fonts
                        .layout_no_wrap(
                            solidcraft_ui_egui::i18n::tr_label("Search commands").to_owned(),
                            egui::FontId::proportional(t.font_button),
                            t.form_dim,
                        )
                        .size()
                        .x,
                    fonts.layout_no_wrap("S".to_owned(), egui::FontId::monospace(t.font_small), t.form_dim).size().x,
                )
            });
            // Two outer paddings, two item gaps, and total keycap padding.
            assert_eq!(search.width(), (hint_width + key_width + t.space_16 + t.space_4 * 5.0 + t.space_2).ceil());
            assert!(ribbon.contains_rect(search));
            assert!(search.left() > workspace.right());
            assert!(h.app.viewport.rect.unwrap().top() >= toolbar::APP_BAR_HEIGHT + toolbar::RIBBON_HEIGHT);
            h.step(&json!({"click":{"handle":"ribbon:search"}})).unwrap();
            assert!(h.app.ui.palette_open);
            h.frames(10);
            let field = rect_of("ribbon:search").unwrap();
            assert_eq!(field.width(), 520.0_f32.min(width - 8.0));
            assert_eq!(field.height(), 40.0);
            assert_eq!(field.right(), search.right());
            assert_eq!(Some(field), rect_of("shortcut:search"));
            assert_eq!(Some(field), rect_of("palette:search"));
            let popup = rect_of("ribbon:search_popup").unwrap();
            assert_eq!(popup.top(), field.bottom() + solidcraft_ui_egui::theme::Tokens::get().space_6);
            assert_eq!(popup.x_range(), field.x_range());
            assert!(popup.top() >= search.bottom());
            assert!(popup.left() <= search.left());
            h.step(&json!({"key":"Escape"})).unwrap();
            assert!(!h.app.ui.palette_open);
            h.frames(10);
            assert_eq!(rect_of("ribbon:search").unwrap(), search);
            h.step(&json!({"key":"S"})).unwrap();
            assert!(h.app.ui.palette_open);
            h.frames(10);
            assert_eq!(rect_of("ribbon:search_popup").unwrap().min, popup.min);
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(10);
        }
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        h.frames(4);
        let button = rect_of("button:solid.extrude").unwrap();
        assert_eq!(button.height(), 60.0);
        assert!(button.width() >= 54.0);
        let section = rect_of("ribbon:section:MODEL:CREATE").unwrap();
        assert_eq!(section.height(), 20.0);
        assert!(section.top() > button.bottom());
        assert_eq!(section.bottom(), rect_of("toolbar").unwrap().bottom() - 4.0);
        assert_eq!(handle_at("panel:MODEL:CREATE"), handle_at("ribbon:section:MODEL:CREATE"));
        h.step(&json!({"click":{"handle":"ribbon:section:MODEL:CREATE"}})).unwrap();
        assert!(rect_of("toolbar_dropdown").is_some());
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"click":{"handle":"ribbon:workspace"}})).unwrap();
        assert!(h.app.viewport.context_menu.is_some());
    }
}

#[test]
fn finish_sketch_stays_at_the_right_when_groups_collapse() {
    use solidcraft_ui_egui::scenario::{count_of, handle_at};
    for layout in ["docked", "floating"] {
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout":layout}));
        h.call("engine.execute", json!({"command":"sketch.create","params":{"plane":"XY"}}));
        for width in [1600.0, 640.0, 320.0] {
            h.call("ui.resize", json!({"width":width,"height":1000}));
            h.frames(5);
            let ribbon = rect_of("toolbar").unwrap();
            let exit = rect_of("ribbon:section:SKETCH:FINISH SKETCH").unwrap();
            let finish = rect_of("button:sketch.finish").unwrap();
            // Finish follows the last sketch panel; once panels spill into the » menu it sits at
            // the right end, after it.
            if count_of("toolbar_more").unwrap_or(0.0) > 0.0 {
                assert_eq!(exit.right(), ribbon.right() - 4.0, "{layout} at {width}");
            } else {
                assert!(exit.right() <= ribbon.right() - 4.0, "{layout} at {width}");
                let last = rect_of("panel:SKETCH:CONFIGURE").or_else(|| rect_of("panel:SKETCH:REFERENCE")).unwrap();
                let group = rect_of("panel:SKETCH:FINISH SKETCH").unwrap();
                assert!(group.left() - last.right() < 16.0, "{layout} at {width}: Finish follows the last panel");
            }
            assert!(ribbon.contains_rect(finish));
            assert!(finish.bottom() < exit.top());
            assert!(handle_at("tab:SKETCH").is_some(), "the selected contextual tab scrolls into view");
        }
        h.step(&json!({"click":{"handle":"button:sketch.finish"}})).unwrap();
        assert!(h.app.session.active_sketch.is_none());
    }
}
