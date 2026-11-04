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
    assert_eq!(crate::i18n::tr("ui.preferences"), "Preferences", "the render scope must restore English");

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
    assert_eq!(crate::i18n::tr("ui.preferences"), "Preferences");

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
    for (lang, browser, filter) in [
        ("zh", "浏览器", "选择筛选"),
        ("ja", "ブラウザ", "選択フィルター"),
        ("de", "Browser", "Auswahlfilter"),
        ("ko", "브라우저", "선택 필터"),
        ("ru", "Браузер", "Фильтр выделения"),
        ("fr", "Navigateur", "Filtre de sélection"),
        ("it", "Struttura", "Filtro di selezione"),
        ("uk", "Браузер", "Фільтр виділення"),
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
        let locale = crate::i18n::Locale::for_language(catalog.id);
        for id in ["home.new_mm", "home.new_inches", "home.open", "home.import"] {
            let label = locale.text(id);
            let (_, text) = texts.iter().find(|(t, _)| t == label).unwrap_or_else(|| panic!("{}: {label:?} is not painted", catalog.id));
            let button = rects
                .iter()
                .filter(|r| (r.height() - crate::theme::Tokens::get().file_tile_height).abs() < 1.0 && r.contains(text.left_center()))
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
    let hover = crate::theme::Tokens::get().hover;
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
    let fr = crate::i18n::Locale::for_language("fr");
    assert_eq!(fr.format("dialog.bodies_count", &[("count", "3")]), "3 corps");
    assert_eq!(fr.text("dialog.compute_again"), "Recalculer");
}

/// The continuity legend's Gap (an opening between faces) and the sheet-metal hem's gap are
/// different words in some languages, so each has its own message.
#[test]
fn continuity_gap_and_sheet_metal_gap_are_separate_messages() {
    for (lang, legend, sheet) in [("fr", "Écart", "Jeu"), ("de", "Lücke", "Spalt")] {
        let locale = crate::i18n::Locale::for_language(lang);
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
