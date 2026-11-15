//! Localization must not change scenario handles or leave the next app in another locale.

use crate::scenario::{self, Harness};
use serde_json::json;

#[test]
fn japanese_dialog_keeps_english_scenario_handles() {
    let mut h = Harness::new();
    h.app.preferences.language = "ja".into();
    h.app.home.open = false;
    h.call("ui.start", json!({"command":"sketch.create"}));
    h.frames(3);
    assert!(h.widget("平面").is_some(), "the input label must be translated");
    assert!(h.widget("キャンセル").is_some(), "the close button must be translated");
    assert!(!scenario::handles("dialog:Plane").is_empty(), "scenario handles must retain source labels");
    assert_eq!(solidcraft_ui_text::tr("ui.preferences"), "Preferences", "the render scope must restore English");

    h.app.preferences.language = "en".into();
    h.frames(3);
    assert!(h.widget("Plane").is_some());
    assert!(h.widget("Cancel").is_some());
}

#[test]
fn italian_dialog_keeps_english_scenario_handles() {
    let mut h = Harness::new();
    h.app.preferences.language = "it".into();
    h.app.home.open = false;
    h.call("ui.start", json!({"command":"sketch.create"}));
    h.frames(3);
    assert!(h.widget("Piano").is_some());
    assert!(h.widget("Annulla").is_some());
    assert!(!scenario::handles("dialog:Plane").is_empty());
    assert_eq!(solidcraft_ui_text::tr("ui.preferences"), "Preferences");

    h.app.preferences.language = "en".into();
    h.frames(3);
    assert!(h.widget("Plane").is_some());
    assert!(h.widget("Cancel").is_some());
}

#[test]
fn sketch_palette_profile_and_points_rows_show_their_own_translation() {
    // The palette uses the shared profile/points IDs. Check the text actually on screen:
    // a broken ID could silently fall back to English.
    for (lang, profile, points) in [("de", "Profil", "Punkte"), ("ja", "プロファイル", "点"), ("ko", "프로파일", "점"), ("uk", "Профіль", "Точки")]
    {
        let mut h = Harness::new();
        h.app.preferences.language = lang.into();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(3);
        let at = scenario::handle_at("sketch_palette:options_fold").unwrap();
        h.call("ui.click", json!({"x":at.x,"y":at.y}));
        assert!(h.widget(profile).is_some(), "{lang}: the Profile row must show {profile:?}");
        assert!(h.widget(points).is_some(), "{lang}: the Points row must show {points:?}");
        assert!(h.widget("Profile").is_none(), "{lang}: must not silently fall back to English");
        assert!(h.widget("Points").is_none(), "{lang}: must not silently fall back to English");
    }
}

#[test]
fn language_preference_cancel_restores_previous_locale() {
    let mut h = Harness::new();
    h.app.preferences.language = "en".into();
    h.app.home.open = false;
    h.call("ui.prefs", json!({"open":"General"}));
    h.call("ui.prefs", json!({"language":"ja"}));
    assert!(h.widget("言語").is_some());
    assert_eq!(h.app.preferences.language, "ja");
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(h.app.preferences.language, "en");
    assert_eq!(h.app.tr("ui.preferences"), "Preferences");
}

/// A first start (no saved language) shows `System` as the interface language choice (#10138).
#[test]
fn language_dropdown_defaults_to_system() {
    let mut h = Harness::new();
    h.app.home.open = false;
    assert_eq!(h.app.preferences.language, "system");
    h.call("ui.prefs", json!({"open":"General"}));
    let system = h.app.tr("prefs.language_system").to_owned();
    assert!(h.widget(&system).is_some(), "the language dropdown reads {system}");
}

/// The chrome a user sees before touching anything: the browser heading and the SELECT panel.
/// Both were English literals in the drawing code, and a broken message id falls back to
/// English silently, so check the word actually rendered in each language.
#[test]
fn window_chrome_renders_in_the_active_language() {
    for (lang, browser, filter, faces) in [
        ("zh", "浏览器", "选择筛选", "面"),
        ("ja", "ブラウザ", "選択フィルター", "面"),
        ("de", "Browser", "Auswahlfilter", "Flächen"),
        ("ko", "브라우저", "선택 필터", "면"),
        ("ru", "Браузер", "Фильтр выделения", "Грани"),
        ("fr", "Navigateur", "Filtre de sélection", "Faces"),
        ("it", "Struttura", "Filtro di selezione", "Facce"),
        ("uk", "Браузер", "Фільтр виділення", "Грані"),
    ] {
        let mut h = Harness::new();
        h.app.preferences.language = lang.into();
        h.app.home.open = false;
        h.frames(2);
        // The browser heading is on screen from the start; the selection filter lives in the
        // SELECT panel, which is not the one shown by default.
        assert!(h.widget(browser).is_some(), "{lang}: the browser heading must show {browser:?}");
        h.step(&json!({"click":{"handle":"tab:VIEW"}})).unwrap();
        let select = scenario::handle_at("panel:VIEW:SELECT").expect("the selection filter is on VIEW");
        h.step(&json!({"click": [select.x, select.y]})).unwrap();
        h.frames(2);
        assert!(h.widget(filter).is_some(), "{lang}: the selection filter must show {filter:?}");
        assert!(h.widget(faces).is_some(), "{lang}: the Faces checkbox must show {faces:?}");
    }
}

/// File-view action tiles must keep translated labels inside their bounds. A translation longer than the English
/// label ran past the edge ("Nouvelle conception"). Every registered language's label must stay
/// inside its button.
#[test]
fn home_page_buttons_fit_their_translated_labels() {
    fn collect(shape: &egui::Shape, rects: &mut Vec<egui::Rect>, texts: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Rect(r) => rects.push(r.rect),
            egui::Shape::Text(t) => texts.push((t.galley.job.text.clone(), egui::Rect::from_min_size(t.pos, t.galley.size()))),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, rects, texts)),
            _ => {}
        }
    }
    for catalog in crate::i18n::LOCALES {
        let mut h = Harness::new();
        h.app.preferences.language = catalog.id.into();
        let rev = h.app.session.revision;
        h.app.home.show(rev);
        h.frames(2);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let out = crate::test_support::frame(&h.ctx, egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| h.app.ui(ui));
        let (mut rects, mut texts) = (Vec::new(), Vec::new());
        out.shapes.iter().for_each(|shape| collect(&shape.shape, &mut rects, &mut texts));
        let locale = solidcraft_ui_text::Locale::for_language(catalog.id);
        for id in ["home.new_mm", "home.new_inches", "home.open", "home.import"] {
            let label = locale.text(id);
            let (_, text) = texts.iter().find(|(t, _)| t == label).unwrap_or_else(|| panic!("{}: {label:?} is not painted", catalog.id));
            let button = rects
                .iter()
                .filter(|r| (r.height() - solidcraft_theme::Tokens::get().file_tile_height).abs() < 1.0 && r.contains(text.left_center()))
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .unwrap_or_else(|| panic!("{}: no button around {label:?}", catalog.id));
            assert!(text.right() <= button.right(), "{}: {label:?} ends at {} past its button's edge {}", catalog.id, text.right(), button.right());
        }
    }
}

/// The start page is the first thing a user sees. Its buttons, links, headings and cards were
/// English literals, mostly painted rather than laid out as widgets, so check the text actually
/// painted in each complete language: a broken message id falls back to English silently.
#[test]
fn home_page_renders_in_the_active_language() {
    fn painted(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| painted(s, out)),
            _ => {}
        }
    }
    let cases = [
        (
            "it",
            ["Nuovo progetto", "Apri…", "Scorciatoie da tastiera", "Impostazioni", "Recenti", "Esempi", "Piastra di esempio"],
            "non presente\n2 h fa",
        ),
        (
            "ja",
            ["新しいデザイン", "開く…", "キーボードショートカット", "設定", "最近使用した項目", "サンプル", "サンプル プレート"],
            "見つかりません\n2 時間前",
        ),
        ("zh", ["新建设计", "打开…", "键盘快捷键", "设置", "最近使用", "示例", "示例板"], "文件不存在\n2 小时前"),
    ];
    for (lang, words, missing_recent) in cases {
        let mut h = Harness::new();
        h.app.preferences.language = lang.into();
        let gone = h.dir.join("gone.solidcraft").to_string_lossy().into_owned();
        h.app.home.add_recent(&gone);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        h.app.home.opened_at.insert(gone, now - 2 * 3600);
        let rev = h.app.session.revision;
        h.app.home.show(rev);
        h.frames(2);
        // The recent file counts as there until the background check says it is not.
        h.app.home.wait_for_existence_checks();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let out = crate::test_support::frame(&h.ctx, egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| h.app.ui(ui));
        let mut texts = Vec::new();
        out.shapes.iter().for_each(|shape| painted(&shape.shape, &mut texts));
        for word in words {
            assert!(texts.iter().any(|t| t == word), "{lang}: {word:?} must be painted, got {texts:?}");
        }
        let (missing, opened) = missing_recent.split_once('\n').unwrap();
        assert!(texts.iter().any(|text| text.starts_with(&format!("{missing} · "))), "{lang}: missing-file status must remain visible");
        assert!(texts.iter().any(|text| text == opened), "{lang}: the recent timestamp must remain visible");
        for english in ["New Design", "Open…", "Documentation", "Report an Issue", "What's new", "Recent", "Samples", "Sample Plate"] {
            assert!(!texts.iter().any(|t| t == english), "{lang}: {english:?} is still painted in English");
        }
    }
}

#[test]
fn translated_dialog_buttons_do_not_wrap() {
    fn cancel_rows(shape: &egui::Shape) -> Option<usize> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == "キャンセル" => Some(text.galley.rows.len()),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(cancel_rows),
            _ => None,
        }
    }
    let mut h = Harness::new();
    crate::fonts::setup_fonts(&h.ctx);
    h.app.preferences.language = "ja".into();
    h.app.home.open = false;
    h.call("ui.start", json!({"command":"solid.extrude"}));
    // A few frames: the card sizes itself from its content over the first passes.
    let mut out = egui::FullOutput::default();
    for _ in 0..5 {
        out = h.ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() },
            |ui| h.app.ui(ui),
        );
    }
    assert_eq!(out.shapes.iter().find_map(|shape| cancel_rows(&shape.shape)), Some(1));
    out.textures_delta.clear();
}

#[test]
fn finish_exit_label_and_its_button_are_centred() {
    fn label_rows(shape: &egui::Shape) -> Option<Vec<egui::Rect>> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == "Beenden" => {
                Some(text.galley.rows.iter().map(|row| row.rect().translate(text.pos.to_vec2())).collect())
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(label_rows),
            _ => None,
        }
    }
    fn hover_rect(shape: &egui::Shape, hover: egui::Color32, at: egui::Pos2) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Rect(r) if r.fill == hover && r.rect.contains(at) => Some(r.rect),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| hover_rect(s, hover, at)),
            _ => None,
        }
    }
    let mut h = Harness::new();
    h.app.preferences.language = "de".into();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"xy"})).unwrap();
    h.frames(2);
    let label = scenario::handles("panel:SKETCH:FINISH SKETCH")[0].1;
    // Finish and Cancel Sketch (#3838) sit side by side, the pair centred over the label.
    let finish = scenario::handles("button:sketch.finish")[0].1;
    let cancel = scenario::handles("button:sketch.cancel")[0].1;
    assert!(finish.x < label.x && label.x < cancel.x, "the label is centred under the pair, Finish first: {label:?} {finish:?} {cancel:?}");

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let mut out =
        h.ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events: vec![egui::Event::PointerMoved(label)], ..Default::default() }, |ui| {
            h.app.ui(ui)
        });
    out.textures_delta.clear();
    let mut out = h.ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| h.app.ui(ui));
    let rows = out.shapes.iter().find_map(|shape| label_rows(&shape.shape)).expect("the Finish Sketch panel label is drawn");
    assert_eq!(rows.len(), 1, "the translated Exit section label fits one row: {rows:?}");
    assert!((rows[0].center().x - label.x).abs() < 1.0, "the label is centred: {rows:?}");
    let hover = solidcraft_theme::Tokens::get().hover;
    let lr = out.shapes.iter().find_map(|shape| hover_rect(&shape.shape, hover, label)).expect("the hovered label is highlighted");
    assert!(rows.iter().all(|row| lr.contains_rect(*row)), "the highlight holds the label: {lr:?} {rows:?}");
    out.textures_delta.clear();
}

/// The Interference dialog's title, input and Compute button come from the catalogs: they were
/// English in every language.
#[test]
fn interference_dialog_renders_in_the_active_language() {
    let mut h = Harness::new();
    h.app.preferences.language = "fr".into();
    h.app.home.open = false;
    h.call("ui.start", json!({"command":"inspect.interference"}));
    h.frames(3);
    for word in ["INTERFÉRENCE", "Corps (tous si aucun)", "Calculer"] {
        assert!(h.widget(word).is_some(), "{word:?} must be shown");
    }
    for english in ["INTERFERENCE", "Bodies (all if none)", "Compute"] {
        assert!(h.widget(english).is_none(), "{english:?} is still shown in English");
    }
    let fr = solidcraft_ui_text::Locale::for_language("fr");
    assert_eq!(fr.format("dialog.bodies_count", &[("count", "3")]), "3 corps");
    assert_eq!(fr.text("dialog.compute_again"), "Recalculer");
}

/// The continuity legend's Gap (an opening between faces) and the sheet-metal hem's gap are
/// different words in some languages, so each has its own message.
#[test]
fn continuity_gap_and_sheet_metal_gap_are_separate_messages() {
    crate::i18n::install();
    for (lang, legend, sheet) in [("fr", "Écart", "Jeu"), ("de", "Lücke", "Spalt")] {
        let locale = solidcraft_ui_text::Locale::for_language(lang);
        assert_eq!(locale.text("ui.continuity_gap"), legend, "{lang}");
        assert_eq!(locale.text("ui.sheet_gap"), sheet, "{lang}");
    }
}

/// The reference labels of the cataloguing fix: a label passed as text (`Cylindrical face`) and an
/// id (`thread.iso_metric`).
#[test]
fn newly_catalogued_dialog_labels_show_in_german() {
    let mut h = Harness::new();
    h.app.preferences.language = "de".into();
    h.app.home.open = false;
    h.call("ui.start", json!({"command":"solid.thread"}));
    h.frames(3);
    assert!(h.widget("Zylindrische Fläche").is_some(), "the Cylindrical face row must be translated");
    assert!(h.widget("ISO metrisch").is_some(), "the thread.iso_metric label must be translated");
    assert!(h.widget("ISO metric").is_none(), "the label must not fall back to English");
}

/// The kit's widgets in SolidCraft's languages: they translate labels once, show options in the
/// user's language and fit German text.
mod kit_translation {
    use crate::scenario;
    use crate::ui_kit::{ContentWidth, row, scope, segmented, test_support, theme_option_notice};
    use egui::{Rect, vec2};
    use solidcraft_theme::Tokens;

    /// `label_width` and `sized_row` share `indented_label`, so a label is translated once. German
    /// `Profiles` is `Profile`, which is itself an English key that would translate again to
    /// `Profil`; the row must show `Profile`, not the second translation.
    #[test]
    fn a_label_is_translated_once() {
        crate::i18n::install();
        let _de = solidcraft_ui_text::scope("de");
        let ctx = crate::fonts::test_context();
        let shown = |label: &str| {
            let output = test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| scope(ui, |ui| row(ui, label, |_| ())));
            let mut texts = Vec::new();
            for clipped in &output.shapes {
                test_support::each_shape(&clipped.shape, &mut |shape| {
                    if let egui::Shape::Text(text) = shape {
                        texts.push(text.galley.job.text.clone());
                    }
                });
            }
            texts
        };
        assert_eq!(shown("Profiles"), ["Profile"]);
        assert_eq!(shown("  Profiles"), ["  Profile"]);
        assert_eq!(shown("Profile"), ["Profil"]);
    }

    /// A segment shows its option in the user's language, as the dropdown it falls back to does, and
    /// keeps the option as passed in its handle.
    #[test]
    fn segments_show_their_options_translated() {
        let ctx = crate::fonts::test_context();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 120.0));
        let _japanese = solidcraft_ui_text::scope("ja");
        let mut selected = 0;
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
            row(ui, "Section", |ui| segmented(ui, &mut selected, &["Circular", "Square"], Default::default()));
        });
        output.textures_delta.clear();
        let painted = |text: &str| output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text == text));
        for (option, shown) in [("Circular", "円形"), ("Square", "正方形")] {
            assert!(painted(shown) && !painted(option), "{option} shows as {shown}");
            assert!(scenario::handle_at(&format!("dialog:Section:{option}")).is_some(), "{option} keeps its handle");
        }
    }

    #[test]
    fn theme_notice_actions_fit_wrap_and_accept_native_pointer_clicks_in_both_themes() {
        const NOTICE: &str = "A theme option changed. Undo restores the previous value; dismiss keeps the current value and closes this notice.";

        fn frame(ctx: &egui::Context, width: f32, can_undo: bool, events: Vec<egui::Event>) -> (bool, bool) {
            scenario::clear_handles();
            let mut clicked = (false, false);
            crate::test_support::frame(
                ctx,
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 1600.0))),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    let mut content = Rect::NOTHING;
                    let allocated = ui.allocate_ui_with_layout(vec2(width, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(width);
                        content = ui.max_rect().intersect(ui.clip_rect());
                        ui.set_clip_rect(content);
                        scope(ui, |ui| {
                            clicked = theme_option_notice(ui, "guard", NOTICE, solidcraft_ui_text::tr("prefs.theme_notice_dismiss"), can_undo);
                        });
                    });
                    for (target, present) in [("prefs:theme_notice_undo:guard", can_undo), ("prefs:theme_notice_dismiss:guard", true)] {
                        let rect = scenario::rect_of(target);
                        if present {
                            let rect = rect.expect("notice action");
                            assert!(content.contains_rect(rect), "{target} clips at width {width}: {rect:?} outside {content:?}");
                            assert!(allocated.response.rect.contains_rect(rect), "{target} must be inside the allocated notice content");
                            assert!(rect.is_positive());
                            assert_eq!(scenario::handle_at(target), Some(rect.center()));
                        } else {
                            assert!(rect.is_none(), "Undo must be absent when unavailable");
                        }
                    }
                },
            );
            clicked
        }

        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            for language in ["en", "de"] {
                let _language = solidcraft_ui_text::scope(language);
                let t = Tokens::get();
                let mut undo_width = 0.0;
                let mut dismiss_width = 0.0;
                let mut label_wrap_width = 0.0;
                test_support::frame(&crate::fonts::test_context(), vec![], |ui| {
                    undo_width = ContentWidth::button(ui, solidcraft_ui_text::tr("ui.undo"));
                    dismiss_width = ContentWidth::button(ui, solidcraft_ui_text::tr("prefs.theme_notice_dismiss"));
                    label_wrap_width = (ContentWidth::label(ui, solidcraft_ui_text::tr("prefs.theme_notice_dismiss")) / 3.0).ceil();
                });
                let actions_width = undo_width + dismiss_width + t.space_8 * 2.0;
                let narrow_width = undo_width.max(dismiss_width);
                for (width, stacked, label_wrap) in
                    [(actions_width + t.form_card_width, false, false), (narrow_width, true, false), (label_wrap_width, true, true)]
                {
                    for can_undo in [true, false] {
                        let ctx = crate::fonts::test_context();
                        assert_eq!(frame(&ctx, width, can_undo, vec![]), (false, false));
                        let text = scenario::rect_of("dialog:hint:guard").expect("notice text");
                        let dismiss = scenario::rect_of("prefs:theme_notice_dismiss:guard").expect("Dismiss");
                        let undo = scenario::rect_of("prefs:theme_notice_undo:guard");
                        if stacked {
                            assert!(dismiss.top() >= text.bottom(), "Dismiss must move below the text");
                            if let Some(undo) = undo {
                                assert!(undo.top() >= text.bottom(), "Undo must move below the text");
                                assert!(dismiss.top() >= undo.bottom(), "actions must wrap onto separate rows at width {width}");
                            }
                        } else {
                            assert_eq!(dismiss.top(), text.top(), "wide notices retain the inline layout");
                            if let Some(undo) = undo {
                                assert_eq!(undo.top(), text.top());
                                assert!(dismiss.left() >= undo.right());
                            }
                            // The wide notice retains the original action budget and text allocation.
                            let expected = width - dismiss_width - t.space_8 - if can_undo { undo_width + t.space_8 } else { 0.0 };
                            assert_eq!(text.width(), expected);
                        }
                        if label_wrap {
                            assert!(dismiss.height() > t.control_height, "an individual long action label must wrap");
                        }
                        for (target, expected) in
                            [("prefs:theme_notice_undo:guard", (true, false)), ("prefs:theme_notice_dismiss:guard", (false, true))]
                        {
                            if expected.0 && !can_undo {
                                continue;
                            }
                            let at = if expected.0 { undo.expect("Undo").center() } else { dismiss.center() };
                            assert_eq!(frame(&ctx, width, can_undo, test_support::pointer(at, true)), (false, false), "{target} on press");
                            assert_eq!(frame(&ctx, width, can_undo, test_support::pointer(at, false)), expected, "{target} on release");
                            assert_eq!(frame(&ctx, width, can_undo, vec![]), (false, false), "{target} must not repeat");
                        }
                    }
                }
            }
        }
        solidcraft_theme::set_dark(previous);
    }
}
