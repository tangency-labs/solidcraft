//! No English where a translation exists (#2544): the text a frame actually paints, in languages
//! that between them translate every text any language translates. A painter call that skips the
//! label lookup (`ui.painter().text(.., c.label, ..)`) paints the English text even though the
//! language has a translation; this sweep catches it wherever it happens on the surfaces it opens.

use crate::i18n::LocaleNames as _;
use std::collections::{BTreeMap, BTreeSet};

use solidcraft_engine::Session;

use crate::context_menu::Target;
use crate::scenario::Harness;
use crate::{Services, SolveApp};

/// English text → the catalog ids that carry it.
fn english_ids() -> BTreeMap<String, Vec<String>> {
    const EN: &str = include_str!(concat!(env!("OUT_DIR"), "/locales/en.json"));
    let messages: BTreeMap<String, String> = serde_json::from_str(EN).unwrap();
    let mut ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, text) in messages {
        ids.entry(text).or_default().push(id);
    }
    ids
}

/// Every piece of text one more frame paints: whole texts, and the parts of a composed one
/// ("SKETCH › CREATE", "R   SKETCH › CREATE", a tab's "Untitled •").
fn painted(h: &mut Harness) -> BTreeSet<String> {
    fn walk(shape: &egui::Shape, out: &mut BTreeSet<String>) {
        match shape {
            egui::Shape::Text(t) => {
                let text = t.galley.job.text.trim();
                out.insert(text.to_string());
                // Split composed labels, but only on the spaced middle dot: the bare one is
                // Catalan's l·l inside a word ("Cancel·la"), not a separator.
                for part in text.split(['›', '•']).flat_map(|p| p.split(" · ")).flat_map(|p| p.split("   ")) {
                    out.insert(part.trim().to_string());
                }
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let mut out = h.ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
        h.app.logic(ui.ctx());
        h.app.ui(ui);
    });
    out.textures_delta.clear();
    let mut texts = BTreeSet::new();
    out.shapes.iter().for_each(|s| walk(&s.shape, &mut texts));
    texts
}

/// Names the design holds (its own name, features, bodies, components, occurrences), and the
/// keyboard shortcuts' key names. They are shown as typed or as printed on the key in every
/// language, even when one matches a label ("Boss", "Holes", the Delete key).
fn design_names(app: &SolveApp) -> BTreeSet<String> {
    let doc = &app.session.doc;
    let mut names: BTreeSet<String> = [doc.name.clone()].into();
    names.extend(doc.features.iter().map(|f| f.name.clone()));
    names.extend(doc.components.iter().map(|c| c.name.clone()));
    names.extend(doc.occurrences.iter().map(|o| o.name.clone()));
    names.extend(app.session.model.state().bodies.iter().map(|b| b.name.clone()));
    names.extend(solidcraft_engine::command_specs().iter().filter_map(|c| crate::keymap::effective(app, c.id)));
    names
}

/// Texts painted in English although every catalog entry with that English text is translated
/// in `language`. (When one entry with the text is still English there, English is right.)
/// `names` are the design's own names, which are not UI text, except the default names the
/// language translates (a new design's `Untitled`, `Sketch1`): those must show translated.
/// A text the language gives another message is its own word, not English: Romanian's Aspect
/// (Appearance) is also the English Aspect of the Render Frame stage.
fn english_leaks(language: &str, texts: &BTreeSet<String>, english: &BTreeMap<String, Vec<String>>, names: &BTreeSet<String>) -> Vec<String> {
    let locale = solidcraft_ui_text::Locale::for_language(language);
    let own: BTreeSet<&str> = english
        .iter()
        .flat_map(|(text, ids)| ids.iter().map(move |id| (text.as_str(), locale.text(id))))
        .filter(|(text, shown)| text != shown)
        .map(|(_, shown)| shown)
        .collect();
    texts
        .iter()
        .filter(|text| !names.contains(*text) || locale.given_name(text) != text.as_str())
        .filter(|text| !own.contains(text.as_str()))
        .filter(|text| english.get(*text).is_some_and(|ids| ids.iter().all(|id| locale.text(id) != text.as_str())))
        .cloned()
        .collect()
}

/// The languages the sweep paints in. A painter call that skips the lookup paints its English
/// in every language, so it is a leak in each language that translates that text; between them,
/// these languages translate every English text that any language translates, so they see every
/// leak that sweeping all of them would. Each is chosen as the one adding the most texts not yet
/// covered, until none is left.
fn covering_languages(english: &BTreeMap<String, Vec<String>>) -> Vec<&'static str> {
    let all: BTreeSet<String> = english.keys().cloned().collect();
    let detects: Vec<(&'static str, BTreeSet<String>)> = crate::i18n::LOCALES
        .iter()
        .filter(|c| c.id != "en")
        .map(|c| (c.id, english_leaks(c.id, &all, english, &BTreeSet::new()).into_iter().collect()))
        .collect();
    let mut covered = BTreeSet::new();
    let mut chosen = Vec::new();
    loop {
        let best = detects.iter().max_by_key(|(_, texts)| texts.difference(&covered).count());
        match best {
            Some((id, texts)) if !texts.is_subset(&covered) => {
                chosen.push(*id);
                covered.extend(texts.iter().cloned());
            }
            _ => return chosen,
        }
    }
}

fn sample() -> Harness {
    let mut h = Harness::new();
    let mut session = Session::default();
    session.run_script(&solidcraft_engine::sample::script()).unwrap();
    h.app = SolveApp::new(session, Services::default());
    h.app.home.open = false;
    h.frames(3);
    h
}

/// The surfaces the sweep opens, each in every language of [`covering_languages`]: the window with the sample design; the
/// S command search with nothing typed (the first rows of every command, by name) and with two
/// queries; every Preferences page; and the right-click menus of the viewport, a body, the
/// toolbar and the workspace.
fn sweep(h: &mut Harness, language: &str) -> BTreeMap<&'static str, BTreeSet<String>> {
    h.app.preferences.language = language.into();
    let mut seen = BTreeMap::new();
    h.app.ui.palette_open = false;
    h.frames(2);
    seen.insert("window", painted(h));
    for (name, query) in [("S box", ""), ("S box: rect", "rect"), ("S box: sketch", "sketch")] {
        crate::shortcut_box::open(&mut h.app, None);
        h.app.sbox.text = query.into();
        h.frames(2);
        seen.insert(name, painted(h));
    }
    h.app.ui.palette_open = false;
    let title = solidcraft_ui_text::Locale::for_language(language).text("ui.preferences").to_string();
    for page in crate::prefs::PAGES {
        h.call("ui.prefs", serde_json::json!({"open": page}));
        h.frames(2);
        let texts = painted(h);
        assert!(texts.contains(&title), "{language}: the {page} page isn't showing");
        seen.insert(page, texts);
    }
    h.call("ui.prefs", serde_json::json!({"button": "cancel"}));
    h.frames(1);
    let body = h.app.session.model.state().bodies.first().map(|b| b.name.clone()).unwrap_or_default();
    let menus = [
        ("viewport menu", Target::Viewport),
        ("body menu", Target::Body { name: body }),
        ("toolbar menu", Target::Toolbar),
        ("workspace menu", Target::Workspace),
    ];
    for (name, target) in menus {
        crate::context_menu::open_for(&mut h.app, egui::pos2(700.0, 400.0), target);
        h.frames(2);
        let texts = painted(h);
        let new = texts.difference(&seen["window"]).count();
        assert!(new >= 2, "{language}: the {name} isn't showing ({new} new texts)");
        seen.insert(name, texts);
        crate::context_menu::close(&mut h.app);
        h.frames(1);
    }
    seen
}

#[test]
fn no_english_is_painted_where_the_language_has_a_translation() {
    crate::i18n::install();
    let english = english_ids();
    let mut h = sample();
    let names = design_names(&h.app);
    assert!(names.contains("Boss") && names.len() > 5, "{names:?}");
    let languages = covering_languages(&english);
    assert!(languages.len() >= 2 && languages.len() < crate::i18n::LOCALES.len(), "{languages:?}");
    let mut leaks = Vec::new();
    for language in languages {
        for (surface, texts) in sweep(&mut h, language) {
            assert!(texts.len() > 20, "{language} {surface}: only {} texts painted", texts.len());
            for text in english_leaks(language, &texts, &english, &names) {
                leaks.push(format!("{language} {surface}: {text:?}"));
            }
        }
    }
    assert!(leaks.is_empty(), "{} English text(s) painted where the language has a translation:\n{}", leaks.len(), leaks.join("\n"));
}

/// The sweep can see a leak: English painted by a plain painter call is reported, the same text
/// in the language is not, English that the language leaves untranslated is not either, and nor
/// is a word the language gives another message.
#[test]
fn the_sweep_reports_english_only_where_a_translation_exists() {
    crate::i18n::install();
    let english = english_ids();
    let locale = solidcraft_ui_text::Locale::for_language("ja");
    let translated = locale.text("cmd.sketch_rectangle_center").to_string();
    assert_ne!(translated, "Center Rectangle");
    let texts: BTreeSet<String> =
        ["Center Rectangle".to_string(), translated, "mm".to_string(), "Untranslated Text 2544".to_string(), "Fillet".to_string()].into();
    // "Fillet" here is a feature someone named so: a design name, not a label.
    let names: BTreeSet<String> = ["Fillet".to_string()].into();
    assert_eq!(english_leaks("ja", &texts, &english, &names), vec!["Center Rectangle".to_string()]);
    // A word the language gives another message is its own, though English has a message with
    // the same letters: Romanian's Aspect is Appearance, English's Aspect is the Frame stage's.
    let ro = solidcraft_ui_text::Locale::for_language("ro");
    assert_eq!(ro.text("cmd.appearance_assign"), "Aspect");
    assert_ne!(ro.text("render.frame_aspect"), "Aspect");
    assert!(english_leaks("ro", &["Aspect".to_string()].into(), &english, &BTreeSet::new()).is_empty());
}
