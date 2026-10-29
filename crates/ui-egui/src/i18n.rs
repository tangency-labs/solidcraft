//! Embedded offline message catalogs. UI messages use stable IDs; engine display labels
//! pass through an adapter without changing engine or scenario identifiers.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::OnceLock;

type Messages = BTreeMap<String, String>;

/// Register locales here after adding their catalogs. Missing translations use English.
pub struct Catalog {
    pub id: &'static str,
    pub name: &'static str,
    source: &'static str,
    messages: OnceLock<Messages>,
    /// The messages in display order, for a catalog with right-to-left text (see [`crate::bidi`]).
    shown: OnceLock<Option<Messages>>,
}

impl Catalog {
    const fn new(id: &'static str, name: &'static str, source: &'static str) -> Self {
        Self { id, name, source, messages: OnceLock::new(), shown: OnceLock::new() }
    }

    fn messages(&'static self) -> &'static Messages {
        self.messages.get_or_init(|| match serde_json::from_str(self.source) {
            Ok(messages) => messages,
            Err(error) => {
                log::error!("invalid embedded {} message catalog: {error}", self.id);
                Messages::new()
            }
        })
    }

    /// The messages as egui must be given them: right-to-left lines in display order, since egui
    /// has no bidirectional layout (see [`crate::bidi`]). The same map as [`Self::messages`] for
    /// every other catalog.
    fn shown(&'static self) -> &'static Messages {
        let shown = self.shown.get_or_init(|| {
            let messages = self.messages();
            messages
                .values()
                .any(|text| crate::bidi::has_rtl(text))
                .then(|| messages.iter().map(|(id, text)| (id.clone(), crate::bidi::visual_template(text).into_owned())).collect())
        });
        match shown {
            Some(shown) => shown,
            None => self.messages(),
        }
    }

    /// The language's own name as the language list draws it.
    pub fn display_name(&self) -> std::borrow::Cow<'static, str> {
        crate::bidi::visual(self.name)
    }
}

static ENGLISH: Catalog = Catalog::new("en", "English", include_str!(concat!(env!("OUT_DIR"), "/locales/en.json")));
static ARABIC: Catalog = Catalog::new("ar", "العربية", include_str!(concat!(env!("OUT_DIR"), "/locales/ar.json")));
static BOSNIAN: Catalog = Catalog::new("bs", "Bosanski", include_str!(concat!(env!("OUT_DIR"), "/locales/bs.json")));
static CZECH: Catalog = Catalog::new("cs", "Čeština", include_str!(concat!(env!("OUT_DIR"), "/locales/cs.json")));
static DANISH: Catalog = Catalog::new("da", "Dansk", include_str!(concat!(env!("OUT_DIR"), "/locales/da.json")));
static GERMAN: Catalog = Catalog::new("de", "Deutsch", include_str!(concat!(env!("OUT_DIR"), "/locales/de.json")));
/// English in UK spelling: only the messages whose spelling differs; the rest are English's.
static ENGLISH_UK: Catalog = Catalog::new("en-GB", "English (UK)", include_str!(concat!(env!("OUT_DIR"), "/locales/en-GB.json")));
static SPANISH: Catalog = Catalog::new("es", "Español (España)", include_str!(concat!(env!("OUT_DIR"), "/locales/es.json")));
static FRENCH: Catalog = Catalog::new("fr", "Français", include_str!(concat!(env!("OUT_DIR"), "/locales/fr.json")));
static GALICIAN: Catalog = Catalog::new("gl", "Galego", include_str!(concat!(env!("OUT_DIR"), "/locales/gl.json")));
static HEBREW: Catalog = Catalog::new("he", "עברית", include_str!(concat!(env!("OUT_DIR"), "/locales/he.json")));
static CROATIAN: Catalog = Catalog::new("hr", "Hrvatski", include_str!(concat!(env!("OUT_DIR"), "/locales/hr.json")));
static HUNGARIAN: Catalog = Catalog::new("hu", "Magyar", include_str!(concat!(env!("OUT_DIR"), "/locales/hu.json")));
static INDONESIAN: Catalog = Catalog::new("id", "Bahasa Indonesia", include_str!(concat!(env!("OUT_DIR"), "/locales/id.json")));
static ITALIAN: Catalog = Catalog::new("it", "Italiano", include_str!(concat!(env!("OUT_DIR"), "/locales/it.json")));
static JAPANESE: Catalog = Catalog::new("ja", "日本語", include_str!(concat!(env!("OUT_DIR"), "/locales/ja.json")));
static KOREAN: Catalog = Catalog::new("ko", "한국어", include_str!(concat!(env!("OUT_DIR"), "/locales/ko.json")));
static MALAY: Catalog = Catalog::new("ms", "Bahasa Melayu", include_str!(concat!(env!("OUT_DIR"), "/locales/ms.json")));
static NORWEGIAN: Catalog = Catalog::new("nb", "Norsk bokmål", include_str!(concat!(env!("OUT_DIR"), "/locales/nb.json")));
static DUTCH: Catalog = Catalog::new("nl", "Nederlands", include_str!(concat!(env!("OUT_DIR"), "/locales/nl.json")));
static POLISH: Catalog = Catalog::new("pl", "Polski", include_str!(concat!(env!("OUT_DIR"), "/locales/pl.json")));
static PORTUGUESE_BR: Catalog = Catalog::new("pt-BR", "Português (Brasil)", include_str!(concat!(env!("OUT_DIR"), "/locales/pt-BR.json")));
static ROMANIAN: Catalog = Catalog::new("ro", "Română", include_str!(concat!(env!("OUT_DIR"), "/locales/ro.json")));
static RUSSIAN: Catalog = Catalog::new("ru", "Русский", include_str!(concat!(env!("OUT_DIR"), "/locales/ru.json")));
static SERBIAN: Catalog = Catalog::new("sr", "Српски", include_str!(concat!(env!("OUT_DIR"), "/locales/sr.json")));
static SWEDISH: Catalog = Catalog::new("sv", "Svenska", include_str!(concat!(env!("OUT_DIR"), "/locales/sv.json")));
static TURKISH: Catalog = Catalog::new("tr", "Türkçe", include_str!(concat!(env!("OUT_DIR"), "/locales/tr.json")));
static UKRAINIAN: Catalog = Catalog::new("uk", "Українська", include_str!(concat!(env!("OUT_DIR"), "/locales/uk.json")));
static VIETNAMESE: Catalog = Catalog::new("vi", "Tiếng Việt", include_str!(concat!(env!("OUT_DIR"), "/locales/vi.json")));
static CHINESE: Catalog = Catalog::new("zh", "中文", include_str!(concat!(env!("OUT_DIR"), "/locales/zh.json")));
static CHINESE_TRADITIONAL: Catalog = Catalog::new("zh-TW", "繁體中文", include_str!(concat!(env!("OUT_DIR"), "/locales/zh-TW.json")));
/// English first (the fallback), then by language id, one per line so that adding a language
/// doesn't edit a line another addition edits.
#[rustfmt::skip]
pub const LOCALES: &[&Catalog] = &[
    &ENGLISH,
    &ARABIC,
    &BOSNIAN,
    &CZECH,
    &DANISH,
    &GERMAN,
    &ENGLISH_UK,
    &SPANISH,
    &FRENCH,
    &GALICIAN,
    &HEBREW,
    &CROATIAN,
    &HUNGARIAN,
    &INDONESIAN,
    &ITALIAN,
    &JAPANESE,
    &KOREAN,
    &MALAY,
    &NORWEGIAN,
    &DUTCH,
    &POLISH,
    &PORTUGUESE_BR,
    &ROMANIAN,
    &RUSSIAN,
    &SERBIAN,
    &SWEDISH,
    &TURKISH,
    &UKRAINIAN,
    &VIETNAMESE,
    &CHINESE,
    &CHINESE_TRADITIONAL,
];

pub fn is_supported(id: &str) -> bool {
    LOCALES.iter().any(|catalog| catalog.id == id)
}

/// Immutable locale reference. Catalogs parse once; lookups are logarithmic.
#[derive(Clone, Copy)]
pub struct Locale(&'static Catalog);

impl Locale {
    pub fn for_language(id: &str) -> Self {
        Self(LOCALES.iter().copied().find(|catalog| catalog.id == id).unwrap_or(&ENGLISH))
    }

    pub fn name(self) -> &'static str {
        self.0.name
    }

    /// The language's own name as the language list draws it.
    pub fn display_name(self) -> std::borrow::Cow<'static, str> {
        self.0.display_name()
    }

    /// Stable message ID → translation → English → ID, in display order (see [`crate::bidi`]).
    pub fn text(self, id: &str) -> &str {
        self.0.shown().get(id).or_else(|| ENGLISH.messages().get(id)).map_or(id, String::as_str)
    }

    /// Translate source labels from engine/UI tables, preserving unknown labels.
    /// Also accepts stable IDs while table helpers migrate away from source text.
    pub fn label(self, label: &str) -> &str {
        // Stable IDs are authoritative. An untranslated legacy ID identical to its
        // English caption can still use another translated ID for that source label.
        if ENGLISH.messages().get(label).is_some_and(|text| text != label || self.0.messages().contains_key(label)) {
            return self.text(label);
        }
        static LABEL_IDS: OnceLock<BTreeMap<&'static str, Vec<&'static str>>> = OnceLock::new();
        let ids = LABEL_IDS.get_or_init(|| {
            let mut ids: BTreeMap<_, Vec<_>> = BTreeMap::new();
            for (id, text) in ENGLISH.messages().iter().filter(|(_, text)| !text.contains('{')) {
                ids.entry(text.as_str()).or_default().push(id.as_str());
            }
            ids
        });
        if let Some(candidates) = ids.get(label) {
            // Preserve the old last-ID preference among available translations. A new
            // English-only alias must not hide a translation that already exists here.
            if let Some(text) = candidates.iter().rev().find_map(|id| self.0.shown().get(*id)) {
                return text;
            }
            if let Some(id) = candidates.last() {
                return self.text(id);
            }
        }
        self.text(label)
    }

    pub fn format(self, id: &str, args: &[(&str, &str)]) -> String {
        interpolate(self.text(id), args)
    }

    /// The default names SolidCraft gives (`Untitled` for a new design, `Sketch1`, `Body2`,
    /// `Component3`), translated.
    fn default_name(self, text: &str) -> Option<String> {
        if text == "Untitled" {
            return Some(self.text("ui.untitled").to_string());
        }
        for (prefix, id) in [("Sketch", "browser.sketch_name"), ("Body", "browser.body_name"), ("Component", "browser.component_name")] {
            if let Some(number) = text.strip_prefix(prefix)
                && !number.is_empty()
                && number.chars().all(|c| c.is_ascii_digit())
            {
                return Some(self.format(id, &[("number", number)]));
            }
        }
        None
    }

    /// A name someone gave (a body, sketch, component, …): shown as typed, even when it matches
    /// a UI label ("Fillet", "Boss"); only the default names are translated.
    pub fn given_name(self, text: &str) -> String {
        self.default_name(text).unwrap_or_else(|| text.to_string())
    }

    /// Evaluation errors stay English for scripts; the empty Cut/Intersect hint that names
    /// another component is translated where it is shown. Other text is returned unchanged.
    pub fn diagnostic(self, text: &str) -> String {
        const PREFIX: &str = "there is no body to cut or intersect in component '";
        if let Some((before, detail)) = text.split_once(PREFIX)
            && let Some((component, detail)) = detail.split_once("'; the tool may overlap '")
            && let Some((body, detail)) = detail.split_once("' in component '")
            && let Some((other, after)) = detail.split_once("' — activate that component or pick the body as a target")
        {
            let (component, body, other) = (self.given_name(component), self.given_name(body), self.given_name(other));
            let message = self.format("diagnostic.empty_boolean_component", &[("component", &component), ("body", &body), ("other", &other)]);
            return format!("{before}{message}{after}");
        }
        text.to_string()
    }

    /// Translate generated browser names and labels, preserving custom names otherwise.
    pub fn dynamic(self, text: &str) -> String {
        if let Some(name) = self.default_name(text) {
            return name;
        }
        for (prefix, id, arg) in [("Units: ", "browser.units", "units"), ("Precision: ", "browser.precision", "precision")] {
            if let Some(value) = text.strip_prefix(prefix) {
                return self.format(id, &[(arg, value)]);
            }
        }
        if let Some(count) = text.strip_prefix("Parameters (").and_then(|s| s.strip_suffix(')')) {
            return self.format("browser.parameters", &[("count", count)]);
        }
        if let Some(count) = text.strip_suffix(" degrees of freedom")
            && !count.is_empty()
            && count.chars().all(|c| c.is_ascii_digit())
        {
            return self.format("sketch.degrees_of_freedom", &[("count", count)]);
        }
        self.label(text).to_string()
    }
}

/// One-pass substitution: argument values are literal, even when they contain braces.
/// Missing arguments remain visible rather than crashing or silently removing text.
fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    let mut output = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some((before, after)) = remaining.split_once('{') {
        output.push_str(before);
        let Some((name, rest)) = after.split_once('}') else {
            output.push('{');
            output.push_str(after);
            return output;
        };
        if let Some((_, value)) = args.iter().find(|(key, _)| *key == name) {
            output.push_str(value);
        } else {
            output.push('{');
            output.push_str(name);
            output.push('}');
        }
        remaining = rest;
    }
    output.push_str(remaining);
    output
}

thread_local! {
    // Helpers without an app reference share a render scope. Nested renders restore
    // their caller's locale; no owned language strings or persistent global setting.
    static CURRENT: Cell<Locale> = const { Cell::new(Locale(&ENGLISH)) };
}

#[must_use]
pub struct LanguageScope {
    previous: Locale,
    // Drop on the thread where the scope was entered.
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

pub fn scope(id: &str) -> LanguageScope {
    let previous = CURRENT.with(|current| current.replace(Locale::for_language(id)));
    LanguageScope { previous, _thread: std::marker::PhantomData }
}

impl Drop for LanguageScope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.set(self.previous));
    }
}

/// This thread's language, to carry into a worker thread with [`scope`].
pub fn language() -> &'static str {
    CURRENT.with(|current| current.get().0.id)
}

pub fn tr(id: &str) -> &str {
    CURRENT.with(|current| current.get().text(id))
}
pub fn tr_label(label: &str) -> &str {
    CURRENT.with(|current| current.get().label(label))
}

/// Every English catalog entry `(id, text)`, for Preferences search to match a translation
/// back to the words it was written in.
pub(crate) fn each_english_message(mut f: impl FnMut(&str, &str)) {
    for (id, text) in ENGLISH.messages() {
        f(id, text);
    }
}
pub fn format(id: &str, args: &[(&str, &str)]) -> String {
    CURRENT.with(|current| current.get().format(id, args))
}
pub fn tr_dyn(text: &str) -> String {
    CURRENT.with(|current| current.get().dynamic(text))
}
pub fn tr_name(text: &str) -> String {
    CURRENT.with(|current| current.get().given_name(text))
}
pub fn tr_diagnostic(text: &str) -> String {
    CURRENT.with(|current| current.get().diagnostic(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strings a pull request adds under `assets/locales/pending/` work before they are folded
    /// into the catalogs (the build merges them in).
    #[test]
    fn pending_messages_are_in_the_catalogs() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/locales/pending");
        for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok).filter(|e| e.path().extension().is_some_and(|x| x == "json")) {
            let pending: BTreeMap<String, Messages> = serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap()).unwrap();
            for (locale, messages) in pending {
                for (id, text) in messages {
                    // The catalogs hand right-to-left text out in display order (`crate::bidi`);
                    // pending files hold it in reading order, as the catalogs do (#7527).
                    assert_eq!(Locale::for_language(&locale).text(&id), crate::bidi::visual_template(&text), "{locale}: {id}");
                }
            }
        }
    }

    #[test]
    fn alias_resolution_regression() {
        static LEGACY: Catalog = Catalog::new("test-legacy", "Test", r#"{"Pitch":"legacy pitch"}"#);
        static ALIAS: Catalog = Catalog::new("test-alias", "Test", r#"{"coupling.pitch":"mechanical pitch"}"#);
        assert_eq!(ENGLISH.messages().get("Pitch").map(String::as_str), Some("Pitch"));
        assert_eq!(ENGLISH.messages().get("coupling.pitch").map(String::as_str), Some("Pitch"));
        let legacy = Locale(&LEGACY);
        assert_eq!(legacy.label("Pitch"), "legacy pitch");
        assert_eq!(legacy.label("coupling.pitch"), "Pitch");
        let alias = Locale(&ALIAS);
        assert_eq!(alias.label("Pitch"), "mechanical pitch");
        assert_eq!(alias.label("coupling.pitch"), "mechanical pitch");
        assert_eq!(alias.label("original unknown caption"), "original unknown caption");
    }

    #[test]
    fn timeline_feature_hint_comes_from_the_catalog() {
        let id = "timeline.feature_hint";
        let english = "Double-click to edit · drag to reorder · right-click for more";
        assert_eq!(ENGLISH.messages().get(id).map(String::as_str), Some(english));
        for language in ["en", "ja", "zh", "de", "ko", "es"] {
            let _scope = scope(language);
            let hint = tr(id);
            assert_eq!(hint, Locale::for_language(language).text(id));
            assert_ne!(hint, id);
            if ["ja", "zh", "de", "ko"].contains(&language) {
                assert_ne!(hint, english, "{language}: the hint must be translated");
            }
        }
    }

    #[test]
    fn empty_boolean_component_diagnostic_translates_with_literal_names() {
        let message = "solid.extrude: there is no body to cut or intersect in component 'Part {1}'; the tool may overlap 'Bracket' in component 'Imported STEP' — activate that component or pick the body as a target";
        assert_eq!(Locale::for_language("en").diagnostic(message), message);
        for (language, prefix) in [("ja", "solid.extrude: コンポーネント"), ("zh", "solid.extrude: 组件")] {
            let translated = Locale::for_language(language).diagnostic(message);
            assert!(translated.starts_with(prefix), "{translated}");
            for name in ["Part {1}", "Bracket", "Imported STEP"] {
                assert!(translated.contains(name), "{translated}");
            }
        }
        let plain = "there is no body to cut or intersect";
        assert_eq!(Locale::for_language("ja").diagnostic(plain), plain);
    }

    /// English first, then by id, so adding a locale edits only its own line.
    #[test]
    fn locales_are_listed_english_first_then_by_id() {
        let ids: Vec<&str> = LOCALES.iter().map(|c| c.id).collect();
        assert_eq!(ids.first(), Some(&"en"));
        assert!(ids[1..].is_sorted(), "{ids:?}");
    }

    /// English (UK) holds only the messages it spells differently; the rest are English's.
    #[test]
    fn english_uk_shows_uk_spelling_and_english_for_the_rest() {
        let (uk, english) = (Locale::for_language("en-GB"), Locale::for_language("en"));
        assert_eq!(uk.name(), "English (UK)");
        assert_eq!(uk.label("Center X"), "Centre X");
        assert_eq!(uk.label("Customize Toolbar"), "Customise Toolbar");
        assert_eq!(uk.text("ui.design_properties.organization"), "Organisation");
        assert_eq!(uk.text("ui.preferences"), english.text("ui.preferences"));
        assert_eq!(english.label("Center X"), "Center X");
    }

    /// A new design's name, `Untitled`, is a default name like `Sketch1`: it shows translated
    /// wherever the design's name is shown (the tab, the browser's top row, the save prompt).
    #[test]
    fn a_new_designs_name_is_translated_like_the_other_default_names() {
        let ja = Locale::for_language("ja");
        assert_eq!(ja.given_name("Untitled"), ja.text("ui.untitled"));
        assert_ne!(ja.given_name("Untitled"), "Untitled");
        assert_eq!(Locale::for_language("en").given_name("Untitled"), "Untitled");
        // Only the whole default name: names people type stay as typed.
        assert_eq!(ja.given_name("Untitled bracket"), "Untitled bracket");
        assert_eq!(ja.given_name("Boss"), "Boss");
    }
    use proc_macro2::{Delimiter, TokenStream, TokenTree};
    use std::collections::BTreeSet;

    fn placeholders(text: &str) -> BTreeSet<&str> {
        let mut result = BTreeSet::new();
        let mut rest = text;
        while let Some((_, after)) = rest.split_once('{') {
            let (name, remaining) = after.split_once('}').expect("unclosed placeholder");
            assert!(!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
            result.insert(name);
            rest = remaining;
        }
        result
    }

    /// Every English text `label()` can be asked to translate, read straight from the catalogs.
    fn english_texts() -> BTreeSet<&'static str> {
        ENGLISH.messages().values().map(String::as_str).collect()
    }

    /// Catalogs delivered complete. English is the source catalog; Arabic, Czech, Italian, Japanese and Chinese
    /// are complete. German, Korean, Malay, Russian, French and Ukrainian
    /// are partial (German and Korean predate the coverage guards; Russian, French and Ukrainian
    /// are kept partial so a new label doesn't need more translations), so their gaps fall back
    /// to English and are reported by `cargo xtask i18n` without failing the build, while
    /// nothing in the complete catalogs may regress.
    const COMPLETE: &[&str] = &["ar", "he", "ja", "vi", "zh"];

    /// `text()` falls back to English silently, so a message a complete locale never received is
    /// shown in English inside an otherwise translated UI, with nothing in the build to say so.
    /// This is the guard against that: adding a key to `en.json` and forgetting a complete
    /// catalog fails here.
    #[test]
    fn every_complete_catalog_carries_every_english_message() {
        let english: Messages = serde_json::from_str(ENGLISH.source).unwrap();
        for catalog in LOCALES.iter().filter(|c| COMPLETE.contains(&c.id)) {
            let messages: Messages = serde_json::from_str(catalog.source).unwrap();
            let missing: Vec<&str> = english.keys().filter(|id| !messages.contains_key(*id)).map(String::as_str).collect();
            assert!(missing.is_empty(), "{} is missing {} English message(s): {}", catalog.id, missing.len(), missing.join(", "));
        }
    }

    /// A toolbar tab or panel reaches the UI as a bare source string (`toolbar::TABS`,
    /// `CommandSpec::tab` / `.panel`) and is resolved only by `label()`'s English-text lookup, so
    /// a heading missing from `en.json` can never translate in any locale. There are a dozen of
    /// them, so all of them are required rather than ratcheted.
    #[test]
    fn every_toolbar_tab_and_panel_is_in_the_english_catalog() {
        let texts = english_texts();
        let mut wanted: Vec<&str> = crate::toolbar::TABS.to_vec();
        wanted.push(crate::toolbar_custom::WORKSPACE);
        for section in crate::workspace::SECTIONS {
            wanted.push(section.tab);
            wanted.push(section.panel);
        }
        for spec in solidcraft_engine::command_specs() {
            wanted.push(spec.tab);
            wanted.push(spec.panel);
        }
        let mut missing: Vec<&str> = wanted.into_iter().filter(|t| !t.is_empty() && !texts.contains(t)).collect();
        missing.sort_unstable();
        missing.dedup();
        assert!(missing.is_empty(), "toolbar tabs/panels absent from en.json: {missing:?}");
    }

    /// Every command label the engine can show must exist as an English catalog message.
    ///
    /// `label()` resolves source text coming from the engine by looking that exact English string
    /// up in `en.json`, so a label the catalog does not carry stays English in *every* locale —
    /// nothing fails, the row just looks untranslated. The whole registry now has entries (their
    /// ids are `cmd.<command id>`), so this is a hard requirement rather than a budget: a new
    /// command added without a catalog entry fails here.
    #[test]
    fn every_command_label_is_in_the_english_catalog() {
        let texts = english_texts();
        let mut untranslated: Vec<&str> = solidcraft_engine::command_specs().iter().map(|c| c.label).filter(|label| !texts.contains(label)).collect();
        untranslated.sort_unstable();
        untranslated.dedup();
        assert!(
            untranslated.is_empty(),
            "{} command label(s) have no English catalog entry, so they would stay English in every locale: {untranslated:?}",
            untranslated.len()
        );
    }

    /// The start page shows each built-in sample's name and description through `label()`, so a
    /// sample added to the engine without English catalog entries would stay English everywhere.
    #[test]
    fn every_sample_name_and_description_is_in_the_english_catalog() {
        let texts = english_texts();
        let samples = solidcraft_engine::sample::samples();
        let missing: Vec<&str> = samples.iter().flat_map(|s| [s.name, s.about]).filter(|text| !texts.contains(text)).collect();
        assert!(missing.is_empty(), "sample texts absent from en.json: {missing:?}");
        assert_eq!(Locale::for_language("ja").label("Sample Plate"), "サンプル プレート");
    }

    /// The whole engine registry now reaches the catalogs. Sample command labels from areas that
    /// had no entry at all before (sheet metal, inspect, joints, motion) must resolve to their
    /// translation rather than echo the English source text — `label()` returns its argument when
    /// it finds no English message, so a dropped entry looks like a working English UI.
    #[test]
    fn newly_catalogued_command_labels_translate() {
        let cases = [
            ("cs", "Příruba", "Zebra analýza", "Studie pohybu"),
            ("da", "Flange", "Zebraanalyse", "Bevægelsesanalyse"),
            ("nb", "Flens", "Zebraanalyse", "Bevegelsesanalyse"),
            ("zh", "法兰", "斑马纹分析", "运动研究"),
            ("ja", "フランジ", "ゼブラ解析", "モーション スタディ"),
            ("de", "Flansch", "Zebra-Analyse", "Bewegungsuntersuchung"),
            ("ko", "플랜지", "얼룩말줄무늬 분석", "운동 연구"),
            ("pl", "Kołnierz", "Analiza zebry", "Badanie ruchu"),
            ("ru", "Фланец", "Зебра-анализ", "Исследование движения"),
            ("fr", "Bord tombé", "Analyse zébrée", "Étude de mouvement"),
            ("uk", "Фланець", "Зебра-аналіз", "Дослідження руху"),
        ];
        for (lang, flange, zebra, motion) in cases {
            let locale = Locale::for_language(lang);
            for (source, expected) in [("Flange", flange), ("Zebra Analysis", zebra), ("Motion Study", motion)] {
                assert_eq!(locale.label(source), expected, "{lang}: {source}");
            }
        }
    }

    #[test]
    fn catalogs_have_valid_ids_and_matching_placeholders() {
        let english: Messages = serde_json::from_str(ENGLISH.source).unwrap();
        let mut ids = BTreeSet::new();
        for catalog in LOCALES {
            assert!(ids.insert(catalog.id), "duplicate locale");
            let messages: Messages = serde_json::from_str(catalog.source).unwrap();
            assert!(!messages.is_empty());
            for (id, text) in messages {
                let original = english.get(&id).expect("unknown message ID");
                assert_eq!(placeholders(&text), placeholders(original), "{}: {id}", catalog.id);
            }
        }
    }

    #[test]
    fn direct_ui_message_ids_exist_in_english_catalog() {
        // Catch a misspelled key before it is shown verbatim in the UI. Engine-table labels
        // deliberately use the separate label adapter and fall back to their source text.
        for source in [
            include_str!("toolbar.rs"),
            include_str!("prefs.rs"),
            include_str!("documents.rs"),
            include_str!("dialogs.rs"),
            include_str!("browser.rs"),
            include_str!("sketch_palette.rs"),
            include_str!("design_properties.rs"),
            include_str!("home.rs"),
        ] {
            for call in [".tr(\"", "i18n::tr(\"", "i18n::format(\""] {
                for tail in source.split(call).skip(1) {
                    let id = tail.split('"').next().unwrap();
                    assert!(ENGLISH.messages().contains_key(id), "missing English message: {id}");
                }
            }
        }
    }

    /// `label()` resolves a literal by its English text (or, failing that, an id); `text()` only
    /// by id. A literal neither reaches is shown verbatim in every locale and nothing fails, so
    /// every label and id literal the UI passes is read from the source and checked against the
    /// English catalog. The source is tokenised with `proc-macro2`, so comments, raw strings,
    /// escapes and test modules are handled, and calls inside macros count too. UI-kit calls that
    /// translate their label themselves (`row`, imported as `form_row`, and the rest of
    /// `KIT_CALLS`) are read at the same argument positions the kit translates.
    #[test]
    fn english_only_render_aliases_do_not_hide_existing_label_translations() {
        for language in ["de", "fr"] {
            let locale = Locale::for_language(language);
            for (label, stable_id) in [
                ("Library", "Library"),
                ("Favorites", "Favorites"),
                ("Center X", "Center X"),
                ("Move up", "Move up"),
                ("Appearance", "cmd.appearance_assign"),
                ("Checkpoint", "cmd.edit_checkpoint"),
                ("Pitch", "coupling.pitch"),
            ] {
                assert_eq!(locale.label(label), locale.text(stable_id), "{language}: {label}");
                assert_ne!(locale.label(label), label, "{language}: {label} must retain its existing translation");
            }
            assert_eq!(locale.label("render.camera_pitch"), locale.text("render.camera_pitch"));
            assert_eq!(locale.label("a custom untranslated caption"), "a custom untranslated caption");
        }
    }

    /// "Quantity" is the number of instances in the pattern dialogs and the plotted value in the
    /// motion study (#6538): each meaning has its own entry, and the label is the count.
    #[test]
    fn pattern_quantity_is_the_count_and_the_motion_plot_keeps_its_own() {
        for catalog in LOCALES {
            let locale = Locale(catalog);
            assert_eq!(locale.label("Quantity"), locale.text("ui.quantity"), "{}", catalog.id);
            assert_eq!(locale.label("Quantity 2"), locale.text("ui.quantity_2"), "{}", catalog.id);
        }
        let de = Locale::for_language("de");
        assert_eq!(de.label("Quantity"), "Anzahl");
        assert_eq!(de.label("motion.plot_quantity"), "Größe");
        assert_eq!(Locale::for_language("en").label("motion.plot_quantity"), "Quantity");
    }

    #[test]
    fn every_ui_label_literal_reaches_an_english_catalog_entry() {
        let english: Messages = serde_json::from_str(ENGLISH.source).unwrap();
        let ids: BTreeSet<&str> = english.keys().map(String::as_str).collect();
        let texts: BTreeSet<&str> = english.values().filter(|text| !text.contains('{')).map(String::as_str).collect();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        source_files(&root, &mut files);
        files.sort();
        let sources: Vec<(std::path::PathBuf, String)> =
            files.into_iter().map(|path| (path.clone(), std::fs::read_to_string(&path).unwrap())).collect();
        let context = Context::new(&sources);
        // Test code like `*_tests.rs`, and the files a source declares as external test modules
        // (`#[cfg(test)] mod gallery;`). An inline `#[cfg(test)] mod tests { … }` includes no file
        // and is skipped in place by the scanner.
        let test_only: BTreeSet<std::path::PathBuf> = sources.iter().flat_map(|(path, source)| test_module_files(path, source)).collect();
        let mut missing: Vec<String> = Vec::new();
        for (path, source) in &sources {
            if test_only.contains(path) {
                continue;
            }
            let shown = path.strip_prefix(&root).unwrap_or(path).display().to_string();
            for (literal, id_only) in literals_in(&context, path, source) {
                // A literal with no letters (`×`, `↑`) has no language: there is nothing to
                // translate, and no entry to want. An empty literal is a blank spacer cell.
                if !literal.chars().any(char::is_alphabetic) {
                    continue;
                }
                if !ids.contains(literal.as_str()) && (id_only || !texts.contains(literal.as_str())) {
                    missing.push(format!("{shown}: `{literal}`"));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "{} literal(s) would stay English in every locale because no English catalog entry reaches them:\n  {}",
            missing.len(),
            missing.join("\n  ")
        );
    }

    #[test]
    fn render_shell_labels_and_planned_reasons_translate_in_required_catalogs() {
        for language in ["ja", "zh", "de", "ko"] {
            let locale = Locale::for_language(language);
            for label in [
                "APPEARANCE",
                "SCENE",
                "LIGHTS",
                "STAGE",
                "CAMERA",
                "SHOTS",
                "LENS",
                "FRAME",
                "RESULT",
                "IMAGE",
                "BATCH",
                "Shots",
                "Shots Strip",
                "Pick",
                "Component",
                "The live preview shows direct light only and no shadows.",
            ] {
                assert_ne!(locale.label(label), label, "{language}: {label}");
            }
            assert_ne!(locale.text("render.stage_preview_note"), Locale::for_language("en").text("render.stage_preview_note"));
            // "Studio" is also the German word: require the translated catalog entry,
            // rather than requiring its spelling to differ from English.
            assert!(locale.0.messages().contains_key("toolbar.render_studio"));
            assert_eq!(locale.label("STUDIO"), locale.text("toolbar.render_studio"));
            let english: Messages = serde_json::from_str(ENGLISH.source).unwrap();
            for id in english.keys().filter(|id| id.starts_with("render.inspector_")) {
                assert!(locale.0.messages().contains_key(id), "{language}: {id}");
            }
            for id in [
                "render.pick_label",
                "render.pick_body",
                "render.pick_face",
                "render.pick_component",
                "render.pick_help",
                "render.tree_browser",
                "render.tree_search",
                "render.tree_more",
                "render.shots_save",
                "render.shots_name_help",
                "render.shots_no_result",
                "render.shots_result",
                "render.studios_environments",
                "render.frame_presets",
                "render.size_presets",
                "render.frame_presets_pending",
                "render.size_presets_pending",
                "render.search_appearances",
                "render.library_applicable",
                "render.library_other",
                "render.library_apply_hint",
                "render.studio_tiles_require_gpu",
                "render.environment_tiles_require_raytrace",
            ] {
                assert_ne!(locale.text(id), Locale::for_language("en").text(id), "{language}: {id}");
            }
            let session = solidcraft_engine::Session::default();
            for command in solidcraft_engine::command_specs().into_iter().filter(|c| c.planned.is_some() && crate::workspace::render_tab(c.tab)) {
                assert_ne!(locale.label(command.label), command.label, "{language}: {}", command.id);
                let reason = command.info(&session).disabled_reason.unwrap();
                assert_ne!(locale.label(&reason), reason, "{language}: {} reason", command.id);
            }
        }
    }

    #[test]
    fn output_viewer_labels_exist_in_all_required_catalogs() {
        for language in ["en", "de", "ja", "ko", "zh"] {
            let locale = Locale::for_language(language);
            for key in [
                "cmd.render_output_size",
                "cmd.render_result_view",
                "cmd.render_result_close",
                "cmd.render_result_copy",
                "cmd.render_lights_add",
                "render.output_destination",
                "render.output_destination_help",
                "render.output_backend_ready",
                "render.output_dimensions",
                "render.output_effects",
                "render.output_advanced",
                "render.output_copy",
                "render.output_fit",
                "render.output_export",
                "render.output_view_image",
                "render.effects_gpu_only",
                "render.output_image",
                "render.output_check_summary",
                "render.output_presets_help",
                "render.size_preview",
                "render.size_full_hd",
                "render.size_4k",
                "render.size_square",
                "render.size_custom",
                "render.output_format",
                "render.output_exr_unavailable",
                "render.output_largest",
                "render.output_quality",
                "render.output_quality_waiting",
                "render.quality_draft",
                "render.quality_balanced",
                "render.quality_high",
                "render.quality_custom",
                "render.output_check",
                "render.output_check_help",
                "render.output_ready",
                "render.output_size_ok",
                "render.check_transmission",
                "render.check_opaque",
                "render.check_sorted_alpha",
                "render.state_model",
                "render.state_in_progress",
                "render.state_completed",
                "render.result_actual",
                "render.radial_assign",
                "render.radial_save",
                "render.radial_fit",
                "render.radial_compare",
                "render.wait_add_light",
            ] {
                assert!(locale.0.messages().contains_key(key), "{language}: {key}");
            }
        }
    }

    /// The mouse mapping picker shows its preset's help through `tr_label`, which the guard above
    /// can't see: the text sits in a struct field, not in a call. Each one must be the English text
    /// of its preset's catalog message, or it stays English in every locale.
    #[test]
    fn navigation_preset_help_reaches_the_catalog() {
        let english = Locale::for_language("en");
        for preset in crate::prefs::NAV_PRESETS {
            let id = format!("nav.{}.help", preset.id);
            assert_eq!(english.text(&id), preset.help, "{id}");
        }
    }

    #[test]
    fn missing_sheet_metal_guidance_uses_the_message_catalog() {
        let _scope = scope("ja");
        let message = "there is no sheet metal body: start one with Flange > Base, or turn a plate into sheet metal with Convert to Sheet Metal (pick its large flat face)";
        let error = format!("invalid parameters for `sheet.flange`: {message}");
        let translated = tr("sheet.no_body");
        assert_ne!(translated, "sheet.no_body");
        assert_ne!(translated, message);
        assert_eq!(crate::friendly_error("sheet.flange", &error), format!("{}: {translated}", tr_label("Flange")));
    }

    #[test]
    fn keys_and_engine_labels_translate_with_english_fallback() {
        let ja = Locale::for_language("ja");
        assert_eq!(ja.text("ui.preferences"), "環境設定");
        assert_eq!(ja.label("Extrude"), "押し出し");
        assert_eq!(ja.label("CREATE SKETCH"), "スケッチを作成");
        assert_eq!(ja.label("New Component"), "新規コンポーネント");
        assert_eq!(ja.label("UTILITIES"), "ユーティリティ");
        assert_eq!(ja.label("CONFIGURE"), "構成");
        assert_eq!(ja.label("Drive Joints"), "ジョイントを駆動");
        assert_eq!(ja.label("Axis Through Cylinder/Cone/Torus"), "円柱/円錐/トーラスを通る軸");
        assert_eq!(ja.label("Filter commands or keys"), "コマンドまたはキーを検索");
        assert_eq!(ja.label("Context menu"), "コンテキスト メニュー");
        assert_eq!(ja.label("NonExistentKey"), "NonExistentKey");
        assert_eq!(Locale::for_language("unknown").text("ui.preferences"), "Preferences");
        static PARTIAL: Catalog = Catalog::new("test", "Test", r#"{"ui.preferences":"Prefs"}"#);
        assert_eq!(Locale(&PARTIAL).text("ui.preferences"), "Prefs");
        assert_eq!(Locale(&PARTIAL).text("ui.extrude"), "Extrude");
    }

    #[test]
    fn italian_labels_templates_and_names_preserve_technical_values() {
        assert!(is_supported("it"));
        let it = Locale::for_language("it");
        assert_eq!(it.name(), "Italiano");
        for (source, expected) in [
            ("Sketch", "Schizzo"),
            ("Extrude", "Estrusione"),
            ("Fillet", "Raccordo"),
            ("Chamfer", "Smusso"),
            ("Body", "Corpo"),
            ("New Component", "Nuovo componente"),
            ("CONSTRAINTS", "VINCOLI"),
            ("Draft", "Sformo"),
            ("Counterbore", "Lamatura"),
            ("Countersink", "Svasatura"),
        ] {
            assert_eq!(it.label(source), expected, "{source}");
        }
        assert_eq!(it.dynamic("Sketch12"), "Schizzo12");
        assert_eq!(it.dynamic("Body2"), "Corpo2");
        assert_eq!(it.dynamic("Component3"), "Componente3");
        assert_eq!(it.dynamic("Units: mm"), "Unità: mm");
        assert_eq!(it.dynamic("4 degrees of freedom"), "4 gradi di libertà");
        assert_eq!(it.given_name("Fillet"), "Fillet");
        assert_eq!(it.label("solid.extrude"), "solid.extrude");
        assert_eq!(it.format("document.save_changes", &[("name", "Staffa {v2}")]), "Salvare le modifiche a Staffa {v2}?");
        // The bottom command-line hint retired with that surface. A live save prompt
        // still proves that a substituted value keeps literal JSON braces intact.
        assert_eq!(
            it.format("document.save_changes", &[("name", "solid.extrude {\"distance\": 20}")]),
            "Salvare le modifiche a solid.extrude {\"distance\": 20}?"
        );
        assert_eq!(it.format("ui.browser_bodies_other", &[("count", "2")]), "2 corpi");
    }

    /// Stable IDs and source labels share the same translations. A broken ID must not
    /// silently fall back to English in the sketch palette.
    #[test]
    fn id_labels_resolve_to_their_own_translation_not_an_english_fallback() {
        for (lang, profile, points) in [
            ("de", "Profil", "Punkte"),
            ("ja", "プロファイル", "点"),
            ("ko", "프로파일", "점"),
            ("ru", "Профиль", "Точки"),
            ("fr", "Profil", "Points"),
            ("uk", "Профіль", "Точки"),
        ] {
            let locale = Locale::for_language(lang);
            assert_eq!(locale.label("ui.profile"), profile, "{lang}");
            assert_eq!(locale.label("ui.points"), points, "{lang}");
        }
        // Dialog source labels still reach the same shared messages.
        assert_eq!(Locale::for_language("de").label("Profile"), Locale::for_language("de").text("ui.profile"));
        assert_eq!(Locale::for_language("de").label("Points"), Locale::for_language("de").text("ui.points"));
    }

    /// The Construction Geometries row passes its English text, like the other seven text
    /// rows. "Construction" on its own is already the line-type entry `ui.construction`, so
    /// the row's two-word text must reach its own entry rather than that one.
    #[test]
    fn the_construction_geometries_row_reaches_its_own_entry_not_the_line_type() {
        let rows = [
            ("de", "Konstruktionsgeometrien"),
            ("ja", "構築ジオメトリ"),
            ("ko", "구성 형상"),
            ("ru", "Вспомогательная геометрия"),
            ("fr", "Géométries de construction"),
            ("uk", "Конструкційна геометрія"),
        ];
        for (lang, want) in rows {
            let locale = Locale::for_language(lang);
            assert_eq!(locale.label("Construction Geometries"), want, "{lang}");
            assert_ne!(locale.label("Construction Geometries"), locale.text("ui.construction"), "{lang}");
        }
        assert_eq!(Locale::for_language("en").label("Construction Geometries"), "Construction Geometries");
    }

    #[test]
    fn templates_allow_reordering_and_literal_braces_in_values() {
        assert_eq!(interpolate("{second} then {first}", &[("first", "{second}"), ("second", "B")]), "B then {second}");
        assert_eq!(interpolate("{missing} / {", &[]), "{missing} / {");
        assert_eq!(Locale::for_language("ja").format("document.save_changes", &[("name", "My {design}")]), "My {design} への変更を保存しますか？");
        assert_eq!(Locale::for_language("en").format("dialog.selected_count", &[("count", "2")]), "↖ 2 selected");
    }

    #[test]
    fn browser_patterns_preserve_numbers_and_custom_names() {
        let ja = Locale::for_language("ja");
        assert_eq!(ja.dynamic("Sketch1"), "スケッチ1");
        assert_eq!(ja.dynamic("Body2"), "ボディ2");
        assert_eq!(ja.dynamic("Component12"), "コンポーネント12");
        assert_eq!(ja.dynamic("Units: mm"), "単位: mm");
        assert_eq!(ja.dynamic("Precision: 3"), "精度: 3");
        assert_eq!(ja.dynamic("Parameters (5)"), "パラメータ (5)");
        assert_eq!(ja.dynamic("4 degrees of freedom"), "自由度 4");
        assert_eq!(ja.dynamic("Sketch of bracket"), "Sketch of bracket");
        assert_eq!(Locale::for_language("en").dynamic("Sketch1"), "Sketch1");
    }

    #[test]
    fn given_names_are_not_translated_as_labels() {
        let de = Locale::for_language("de");
        // "Boss" and "Fillet" are also UI labels, which `dynamic` translates.
        assert_ne!(de.dynamic("Boss"), "Boss");
        assert_ne!(de.dynamic("Fillet"), "Fillet");
        assert_eq!(de.given_name("Boss"), "Boss");
        assert_eq!(de.given_name("Fillet"), "Fillet");
        assert_eq!(de.given_name("Sketch3"), de.dynamic("Sketch3"));
        assert_ne!(de.given_name("Sketch3"), "Sketch3");
    }

    #[test]
    fn spanish_catalog_is_registered_and_translates_cad_labels() {
        let es = Locale::for_language("es");
        assert_eq!(es.name(), "Español (España)");
        assert!(is_supported("es"));
        assert_eq!(es.text("ui.preferences"), "Preferencias");
        assert_eq!(es.label("Extrude"), "Extruir");
        assert_eq!(es.label("Fillet"), "Redondeo");
        assert_eq!(es.dynamic("Sketch1"), "Boceto1");
        assert_eq!(es.format("document.save_changes", &[("name", "Pieza")]), "¿Guardar los cambios de Pieza?");
    }

    #[test]
    fn korean_catalog_translates_keys_labels_and_browser_patterns() {
        let ko = Locale::for_language("ko");
        assert_eq!(ko.name(), "한국어");
        assert!(is_supported("ko"));
        assert_eq!(ko.text("ui.preferences"), "환경 설정");
        assert_eq!(ko.label("Extrude"), "돌출");
        assert_eq!(ko.label("CREATE SKETCH"), "스케치 만들기");
        assert_eq!(ko.label("NonExistentKey"), "NonExistentKey");
        assert_eq!(ko.format("document.save_changes", &[("name", "My {design}")]), "My {design}의 변경 사항을 저장하시겠습니까?");
        assert_eq!(ko.format("dialog.selected_count", &[("count", "2")]), "↖ 2개 선택됨");
        assert_eq!(ko.dynamic("Sketch1"), "스케치1");
        assert_eq!(ko.dynamic("Body2"), "바디2");
        assert_eq!(ko.dynamic("Component12"), "컴포넌트12");
        assert_eq!(ko.dynamic("Units: mm"), "단위: mm");
        assert_eq!(ko.dynamic("Precision: 3"), "정밀도: 3");
        assert_eq!(ko.dynamic("Parameters (5)"), "매개변수 (5)");
        assert_eq!(ko.dynamic("4 degrees of freedom"), "자유도 4개");
        assert_eq!(ko.dynamic("Sketch of bracket"), "Sketch of bracket");
    }

    #[test]
    fn malay_catalog_translates_keys_labels_and_browser_patterns() {
        let ms = Locale::for_language("ms");
        assert!(is_supported("ms"));
        assert_eq!(ms.name(), "Bahasa Melayu");
        assert_eq!(ms.text("ui.preferences"), "Tetapan");
        assert_eq!(ms.label("Extrude"), "Semperit");
        assert_eq!(ms.label("CREATE SKETCH"), "CIPTA LAKARAN");
        assert_eq!(ms.format("document.save_changes", &[("name", "My {design}")]), "Simpan perubahan pada My {design}?");
        assert_eq!(ms.format("dialog.selected_count", &[("count", "2")]), "↖ 2 dipilih");
        assert_eq!(ms.dynamic("Sketch1"), "Lakaran1");
        assert_eq!(ms.dynamic("Body2"), "Jasad2");
        assert_eq!(ms.dynamic("Component12"), "Komponen12");
        assert_eq!(ms.dynamic("Units: mm"), "Unit: mm");
        assert_eq!(ms.dynamic("Precision: 3"), "Ketepatan: 3");
        assert_eq!(ms.dynamic("Parameters (5)"), "Parameter (5)");
        assert_eq!(ms.dynamic("4 degrees of freedom"), "4 darjah kebebasan");
        assert_eq!(ms.given_name("Sketch of bracket"), "Sketch of bracket");
        assert_eq!(ms.label("NonExistentKey"), "NonExistentKey");
    }

    #[test]
    fn russian_catalog_translates_keys_labels_and_browser_patterns() {
        let ru = Locale::for_language("ru");
        assert_eq!(ru.name(), "Русский");
        assert!(is_supported("ru"));
        assert_eq!(ru.text("ui.preferences"), "Настройки");
        assert_eq!(ru.label("Extrude"), "Выдавливание");
        assert_eq!(ru.label("CREATE SKETCH"), "СОЗДАТЬ ЭСКИЗ");
        assert_eq!(ru.label("NonExistentKey"), "NonExistentKey");
        assert_eq!(ru.format("document.save_changes", &[("name", "My {design}")]), "Сохранить изменения в My {design}?");
        assert_eq!(ru.format("dialog.selected_count", &[("count", "2")]), "↖ Выбрано: 2");
        assert_eq!(ru.dynamic("Sketch1"), "Эскиз1");
        assert_eq!(ru.dynamic("Body2"), "Тело2");
        assert_eq!(ru.dynamic("Component12"), "Компонент12");
        assert_eq!(ru.dynamic("Units: mm"), "Единицы: mm");
        assert_eq!(ru.dynamic("Precision: 3"), "Точность: 3");
        assert_eq!(ru.dynamic("Parameters (5)"), "Параметры (5)");
        assert_eq!(ru.dynamic("4 degrees of freedom"), "Степеней свободы: 4");
        assert_eq!(ru.dynamic("Sketch of bracket"), "Sketch of bracket");
    }

    #[test]
    fn french_catalog_translates_keys_labels_and_browser_patterns() {
        let fr = Locale::for_language("fr");
        assert_eq!(fr.name(), "Français");
        assert!(is_supported("fr"));
        assert_eq!(fr.text("ui.preferences"), "Préférences");
        assert_eq!(fr.label("Extrude"), "Extrusion");
        assert_eq!(fr.label("Fillet"), "Congé");
        assert_eq!(fr.label("CREATE SKETCH"), "CRÉER UNE ESQUISSE");
        assert_eq!(fr.label("NonExistentKey"), "NonExistentKey");
        // French puts a no-break space before the two-part punctuation marks.
        assert_eq!(fr.format("document.save_changes", &[("name", "My {design}")]), "Enregistrer les modifications de My {design}\u{a0}?");
        assert_eq!(fr.format("dialog.selected_count", &[("count", "2")]), "↖ 2 sélectionné(s)");
        assert_eq!(fr.dynamic("Sketch1"), "Esquisse1");
        assert_eq!(fr.dynamic("Body2"), "Corps2");
        assert_eq!(fr.dynamic("Component12"), "Composant12");
        assert_eq!(fr.dynamic("Units: mm"), "Unités\u{a0}: mm");
        assert_eq!(fr.dynamic("Precision: 3"), "Précision\u{a0}: 3");
        assert_eq!(fr.dynamic("Parameters (5)"), "Paramètres (5)");
        assert_eq!(fr.dynamic("4 degrees of freedom"), "4 degrés de liberté");
        assert_eq!(fr.dynamic("Sketch of bracket"), "Sketch of bracket");
    }

    #[test]
    fn turkish_catalog_is_registered_and_translates_keys() {
        let tr = Locale::for_language("tr");
        assert_eq!(tr.name(), "Türkçe");
        assert!(is_supported("tr"));
        assert_eq!(tr.text("ui.preferences"), "Ayarlar");
        assert_eq!(tr.format("document.save_changes", &[("name", "My {design}")]), "My {design} içindeki değişiklikler kaydedilsin mi?");
        assert_eq!(tr.format("dialog.selected_count", &[("count", "2")]), "↖ 2 seçildi");
        assert_eq!(tr.label("NonExistentKey"), "NonExistentKey");
    }

    #[test]
    fn brazilian_portuguese_catalog_is_registered_and_translates_keys() {
        let pt = Locale::for_language("pt-BR");
        assert_eq!(pt.name(), "Português (Brasil)");
        assert!(is_supported("pt-BR"));
        assert_eq!(pt.text("ui.preferences"), "Preferências");
        assert_eq!(pt.format("document.save_changes", &[("name", "My {design}")]), "Salvar as alterações em My {design}?");
        assert_eq!(pt.format("dialog.selected_count", &[("count", "2")]), "↖ 2 selecionados");
        assert_eq!(pt.label("NonExistentKey"), "NonExistentKey");
    }

    /// Hebrew is stored in reading order; egui reverses each run of Hebrew letters but places
    /// the runs of a line left to right, so every message comes back with its words, numbers,
    /// units and placeholders in display order and brackets mirrored (#3457).
    /// Arabic goes through the same right-to-left path as Hebrew: its words, numbers, units and
    /// placeholders come back in display order, and each Arabic word's letters stay in reading
    /// order for the shaper, which joins and reverses them.
    #[test]
    fn arabic_catalog_is_registered_and_comes_back_in_display_order() {
        let ar = Locale::for_language("ar");
        assert!(is_supported("ar"));
        assert_eq!(ar.name(), "العربية");
        assert_eq!(ar.display_name(), "العربية");
        assert_eq!(ar.text("ui.preferences"), "التفضيلات");
        assert_eq!(ar.label("Extrude"), "بثق");
        // Two words: the second is drawn first, on the left.
        assert_eq!(ar.text("ui.new_design"), "جديد تصميم");
        assert_eq!(ar.dynamic("Units: mm"), "mm :الوحدات");
        assert_eq!(ar.dynamic("Sketch1"), "1 رسم");
        let saved = ar.format("document.save_changes", &[("name", "Bracket")]);
        assert!(saved.starts_with("؟Bracket "), "{saved}");
        let english: Messages = serde_json::from_str(ENGLISH.source).unwrap();
        for id in english.keys() {
            let text = ar.text(id);
            assert!(
                !text.chars().any(|c| matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')),
                "{id}: {text:?}"
            );
        }
    }

    #[test]
    fn hebrew_catalog_is_registered_and_comes_back_in_display_order() {
        let he = Locale::for_language("he");
        assert!(is_supported("he"));
        assert_eq!(he.name(), "עברית");
        assert_eq!(he.display_name(), "עברית");
        assert_eq!(he.text("ui.preferences"), "העדפות");
        assert_eq!(he.label("Extrude"), "שליפה");
        assert_eq!(he.dynamic("Sketch1"), "1סקיצה");
        assert_eq!(he.dynamic("Units: mm"), "mm :יחידות");
        assert_eq!(he.dynamic("Parameters (5)"), "(5) פרמטרים");
        assert_eq!(he.format("document.save_changes", &[("name", "My {design}")]), "?My {design}ב־ השינויים את לשמור");
        assert_eq!(he.given_name("Sketch of bracket"), "Sketch of bracket");
        assert_eq!(he.label("NonExistentKey"), "NonExistentKey");
        // No directional control characters reach egui, which would draw them as boxes.
        let english: Messages = serde_json::from_str(ENGLISH.source).unwrap();
        for id in english.keys() {
            let text = he.text(id);
            assert!(
                !text.chars().any(|c| matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')),
                "{id}: {text:?}"
            );
        }
    }

    #[test]
    fn galician_catalog_is_registered_and_translates_keys() {
        let gl = Locale::for_language("gl");
        assert_eq!(gl.name(), "Galego");
        assert!(is_supported("gl"));
        assert_eq!(gl.text("ui.preferences"), "Preferencias");
        assert_eq!(gl.format("document.save_changes", &[("name", "My {design}")]), "Gardar os cambios en My {design}?");
        assert_eq!(gl.format("dialog.selected_count", &[("count", "2")]), "↖ 2 seleccionados");
        assert_eq!(gl.label("Fillet"), "Arredondamento");
        assert_eq!(gl.label("NonExistentKey"), "NonExistentKey");
    }

    #[test]
    fn ukrainian_catalog_translates_keys_labels_and_browser_patterns() {
        let uk = Locale::for_language("uk");
        assert_eq!(uk.name(), "Українська");
        assert!(is_supported("uk"));
        assert_eq!(uk.text("ui.preferences"), "Налаштування");
        assert_eq!(uk.label("Extrude"), "Видавлювання");
        assert_eq!(uk.label("CREATE SKETCH"), "СТВОРИТИ ЕСКІЗ");
        assert_eq!(uk.label("NonExistentKey"), "NonExistentKey");
        assert_eq!(uk.format("document.save_changes", &[("name", "My {design}")]), "Зберегти зміни в My {design}?");
        assert_eq!(uk.format("dialog.selected_count", &[("count", "2")]), "↖ Вибрано: 2");
        assert_eq!(uk.dynamic("Sketch1"), "Ескіз1");
        assert_eq!(uk.dynamic("Body2"), "Тіло2");
        assert_eq!(uk.dynamic("Component12"), "Компонент12");
        assert_eq!(uk.dynamic("Units: mm"), "Одиниці: mm");
        assert_eq!(uk.dynamic("Precision: 3"), "Точність: 3");
        assert_eq!(uk.dynamic("Parameters (5)"), "Параметри (5)");
        assert_eq!(uk.dynamic("4 degrees of freedom"), "4 ступенів вільності");
        assert_eq!(uk.dynamic("Sketch of bracket"), "Sketch of bracket");
    }

    #[test]
    fn vietnamese_catalog_translates_keys_labels_and_browser_patterns() {
        let vi = Locale::for_language("vi");
        assert_eq!(vi.name(), "Tiếng Việt");
        assert!(is_supported("vi"));
        assert_eq!(vi.text("ui.preferences"), "Tùy chọn cài đặt");
        assert_eq!(vi.label("Extrude"), "Đùn khối");
        assert_eq!(vi.label("CREATE SKETCH"), "TẠO PHÁC THẢO");
        assert_eq!(vi.label("NonExistentKey"), "NonExistentKey");
        assert_eq!(vi.format("document.save_changes", &[("name", "My {design}")]), "Lưu các thay đổi đối với My {design}?");
        assert_eq!(vi.format("dialog.selected_count", &[("count", "2")]), "↖ 2 đã chọn");
        assert_eq!(vi.dynamic("Sketch1"), "Phác thảo1");
        assert_eq!(vi.dynamic("Body2"), "Thân khối2");
        assert_eq!(vi.dynamic("Component12"), "Cụm chi tiết12");
        assert_eq!(vi.dynamic("Units: mm"), "Đơn vị: mm");
        assert_eq!(vi.dynamic("Precision: 3"), "Độ chính xác: 3");
        assert_eq!(vi.dynamic("Parameters (5)"), "Bảng tham số (5)");
        assert_eq!(vi.dynamic("4 degrees of freedom"), "4 bậc tự do");
        assert_eq!(vi.dynamic("Sketch of bracket"), "Sketch of bracket");
    }

    #[test]
    fn new_languages_translate_keys_labels_and_default_names() {
        for (id, name, preferences, extrude, sketch1, save) in [
            ("bs", "Bosanski", "Postavke", "Ekstrudiraj", "Skica1", "Sačuvati promjene u My {design}?"),
            ("hr", "Hrvatski", "Postavke", "Ekstrudiraj", "Skica1", "Spremiti promjene u My {design}?"),
            ("sr", "Српски", "Подешавања", "Екструдирај", "Скица1", "Сачувати промене у My {design}?"),
            ("hu", "Magyar", "Beállítások", "Kihúzás", "Vázlat1", "Menti a(z) My {design} módosításait?"),
            ("id", "Bahasa Indonesia", "Preferensi", "Ekstrusi", "Sketsa1", "Simpan perubahan ke My {design}?"),
            ("nl", "Nederlands", "Voorkeuren", "Extruderen", "Schets1", "Wijzigingen in My {design} opslaan?"),
            ("ro", "Română", "Preferințe", "Extrudare", "Schița1", "Salvați modificările în My {design}?"),
            ("sv", "Svenska", "Inställningar", "Extrudera", "Skiss1", "Spara ändringar i My {design}?"),
            ("zh-TW", "繁體中文", "偏好設定", "擠出", "草圖1", "儲存對 My {design} 的變更？"),
        ] {
            let locale = Locale::for_language(id);
            assert_eq!(locale.name(), name);
            assert!(is_supported(id));
            assert_eq!(locale.text("ui.preferences"), preferences);
            assert_eq!(locale.label("Extrude"), extrude);
            assert_eq!(locale.label("NonExistentKey"), "NonExistentKey");
            assert_eq!(locale.format("document.save_changes", &[("name", "My {design}")]), save);
            assert_eq!(locale.dynamic("Sketch1"), sketch1);
            assert_eq!(locale.dynamic("Sketch of bracket"), "Sketch of bracket");
        }
    }

    #[test]
    fn render_scopes_restore_the_previous_language() {
        let _english = scope("en");
        {
            let _japanese = scope("ja");
            assert_eq!(tr("ui.preferences"), "環境設定");
            {
                let _nested = scope("en");
                assert_eq!(tr("ui.preferences"), "Preferences");
            }
            assert_eq!(tr("ui.preferences"), "環境設定");
        }
        assert_eq!(tr("ui.preferences"), "Preferences");
    }

    /// Every `.rs` file under `dir` except test modules (`*_tests.rs`, `tests.rs`).
    fn source_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                source_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                if name != "tests.rs" && !name.ends_with("_tests.rs") {
                    out.push(path);
                }
            }
        }
    }

    /// A UI-kit call whose label or text argument reaches `tr_label` (shown text) or `tr` (id).
    /// `args` are zero-based positions without the receiver; `trim` names the positions the kit
    /// normalises at run time (the row label, whose leading indent it keeps). Only those may be
    /// matched with their indentation removed.
    struct KitCall {
        name: &'static str,
        args: &'static [usize],
        id: bool,
        trim: &'static [usize],
    }

    /// The kit functions that translate a label or text for display or announcement, read off
    /// `crates/ui-egui/src/ui_kit/`. Not here: the painters that expect an already translated
    /// string (`caption`, `summary_row`, `list_row`, `panel::header`, `empty_box`, `checkbox`,
    /// `tool_choices`), `segmented` (it measures its options with `tr_label` but paints the raw
    /// option, so a catalog entry could not translate it — a wiring gap of its own) and the
    /// measurement helpers (`metrics::label_width`, `ContentWidth::*`). The `hint` arguments of
    /// `hint_field`/`text_field`/`password_field`/`message_field` go raw into
    /// `TextEdit::hint_text`, so they are not translated labels either.
    const KIT_CALLS: &[KitCall] = &[
        // layout.rs: `sized_row` draws `tr_label(text)`, and every row builder reaches it.
        KitCall { name: "row", args: &[1], id: false, trim: &[1] },
        KitCall { name: "status_line", args: &[1], id: false, trim: &[] },
        KitCall { name: "expr_row", args: &[1], id: false, trim: &[1] },
        KitCall { name: "hint_row", args: &[1], id: false, trim: &[1] },
        KitCall { name: "text_row", args: &[1], id: false, trim: &[1] },
        KitCall { name: "choice_row", args: &[1, 4], id: false, trim: &[1] },
        KitCall { name: "section", args: &[2], id: false, trim: &[] },
        KitCall { name: "section_count", args: &[2], id: false, trim: &[] },
        // fields.rs
        KitCall { name: "multiline_text_field", args: &[1], id: false, trim: &[1] },
        KitCall { name: "dropdown", args: &[3], id: false, trim: &[] },
        KitCall { name: "cell_editor", args: &[3], id: false, trim: &[] },
        KitCall { name: "bounded_cell_editor", args: &[3], id: false, trim: &[] },
        KitCall { name: "search_field", args: &[3], id: false, trim: &[] },
        KitCall { name: "menu_filter", args: &[3], id: false, trim: &[] },
        KitCall { name: "placement_field", args: &[3], id: false, trim: &[] },
        KitCall { name: "library_cell", args: &[3], id: false, trim: &[] },
        // actions.rs
        KitCall { name: "icon_button_at", args: &[3], id: false, trim: &[] },
        KitCall { name: "icon_button_colored", args: &[3], id: false, trim: &[] },
        KitCall { name: "footer", args: &[1, 2], id: false, trim: &[] },
        KitCall { name: "button", args: &[1], id: false, trim: &[] },
        KitCall { name: "primary_button", args: &[1], id: false, trim: &[] },
        KitCall { name: "wide_primary_button", args: &[1], id: false, trim: &[] },
        KitCall { name: "action_menu", args: &[1], id: false, trim: &[] },
        KitCall { name: "compact_button", args: &[2], id: false, trim: &[] },
        KitCall { name: "icon_label_button", args: &[1], id: false, trim: &[] },
        KitCall { name: "command_menu_button", args: &[2], id: false, trim: &[] },
        // choice.rs
        KitCall { name: "checkbox_grid", args: &[1], id: false, trim: &[] },
        KitCall { name: "options", args: &[2], id: false, trim: &[] },
        KitCall { name: "checkbox_row", args: &[2], id: false, trim: &[] },
        KitCall { name: "toggle_icon", args: &[2], id: false, trim: &[] },
        // table.rs
        KitCall { name: "Column::new", args: &[0], id: false, trim: &[] },
        KitCall { name: ".add_prompt", args: &[1, 2], id: false, trim: &[] },
        // every `Window`, `Confirm` and `Panel` action row ends in the kit footer
        KitCall { name: ".actions", args: &[0, 1, 2], id: false, trim: &[] },
        KitCall { name: "removable_header", args: &[1], id: false, trim: &[] },
        KitCall { name: "bin_button", args: &[1], id: false, trim: &[] },
        // menu.rs and search.rs; `CommandSearch::heading` passes its label to `tr`, an id.
        KitCall { name: "Menu::heading", args: &[1], id: false, trim: &[] },
        KitCall { name: "MenuItem::new", args: &[1], id: false, trim: &[] },
        KitCall { name: "CommandSearch::heading", args: &[1], id: true, trim: &[] },
    ];

    /// A struct field the kit translates when it draws the value: `Footer { apply }`
    /// (actions.rs:116), `PairButton { label }` (actions.rs:217), `PopupOption { kind }`
    /// (choice.rs:379) and `WindowPage { label }` (panel.rs:561, an id).
    struct KitField {
        owner: &'static str,
        field: &'static str,
        id: bool,
    }

    const KIT_FIELDS: &[KitField] = &[
        KitField { owner: "Footer", field: "apply", id: false },
        KitField { owner: "PairButton", field: "label", id: false },
        KitField { owner: "PopupOption", field: "kind", id: false },
        KitField { owner: "WindowPage", field: "label", id: true },
    ];

    /// A text carrier outside `ui_kit/` that hands its label to the kit: `context_menu::button`
    /// and `button_enabled` build a `MenuItem`, which draws `tr_label(label)` (menu.rs:210).
    /// Resolved by the same origin rules as the kit: a `context_menu::…`/`crate::context_menu::…`
    /// path, a `use` import, or a call inside `context_menu.rs` itself.
    struct ModuleCall {
        module: &'static str,
        name: &'static str,
        args: &'static [usize],
    }

    const MODULE_CALLS: &[ModuleCall] = &[
        ModuleCall { module: "context_menu", name: "button", args: &[1] },
        ModuleCall { module: "context_menu", name: "button_enabled", args: &[1] },
    ];

    /// The kit's modules: only these may sit between `ui_kit` and a translated function in a path.
    const KIT_MODULES: &[&str] =
        &["layout", "fields", "actions", "choice", "table", "menu", "metrics", "text", "panel", "reports", "search", "status", "radial"];

    /// One leaf or glob of a `use` tree: the path it names (already made absolute against the
    /// file's module) and, for a leaf, the local name it binds.
    enum Import {
        Leaf { path: Vec<String>, local: String, public: bool },
        Glob { path: Vec<String> },
    }

    /// How one file's imports name the kit: function bindings, module bindings (`kit::row`,
    /// `l::row`) and glob imports (`use crate::ui_kit::*`).
    #[derive(Default)]
    struct Names {
        functions: BTreeMap<String, Target>,
        modules: BTreeMap<String, Vec<String>>,
        globs: Vec<Vec<String>>,
    }

    /// What a name or path resolves to: a kit function, or a text carrier in another module.
    #[derive(Clone, Copy)]
    enum Target {
        Kit(&'static str),
        Module(&'static ModuleCall),
    }

    /// What the crate's files say about the kit: the names each file imports from it, and the
    /// public re-exports a module offers another (`dialogs::form_row` → `ui_kit::row`).
    struct Context {
        reexports: BTreeMap<Vec<String>, BTreeMap<String, Vec<String>>>,
        imports: BTreeMap<std::path::PathBuf, Vec<Import>>,
    }

    impl Context {
        fn new(files: &[(std::path::PathBuf, String)]) -> Self {
            let mut imports = BTreeMap::new();
            let mut reexports: BTreeMap<Vec<String>, BTreeMap<String, Vec<String>>> = BTreeMap::new();
            for (path, source) in files {
                let module = module_path(path);
                let file_imports = parsed_imports(source, &module);
                for import in &file_imports {
                    if let Import::Leaf { path: target, local, public: true } = import {
                        reexports.entry(module.clone()).or_default().insert(local.clone(), target.clone());
                    }
                }
                imports.insert(path.clone(), file_imports);
            }
            Self { reexports, imports }
        }

        /// How this file names the kit: bound functions, bound modules and glob imports.
        fn names(&self, path: &std::path::Path) -> Names {
            let mut names = Names::default();
            for import in self.imports.get(path).into_iter().flatten() {
                match import {
                    Import::Leaf { path: target, local, .. } => {
                        if let Some(found) = resolve_target(target, &self.reexports) {
                            names.functions.insert(local.clone(), found);
                        } else {
                            // Not a function target: the leaf may bind a module (`ui_kit as kit`).
                            names.modules.insert(local.clone(), target.clone());
                        }
                    }
                    Import::Glob { path } => names.globs.push(path.clone()),
                }
            }
            names
        }

        fn inside_kit(&self, path: &std::path::Path) -> bool {
            module_path(path).get(1).is_some_and(|module| module == "ui_kit")
        }
    }

    /// The crate module path of a source file (`…/src/ui_kit/mod.rs` → `crate::ui_kit`).
    fn module_path(path: &std::path::Path) -> Vec<String> {
        let mut parts: Vec<String> = path.components().filter_map(|part| part.as_os_str().to_str().map(str::to_string)).collect();
        let start = parts.iter().rposition(|part| part == "src").map_or(0, |at| at + 1);
        let mut relative: Vec<String> = parts.split_off(start);
        let file = relative.pop().unwrap_or_default();
        let stem = file.strip_suffix(".rs").unwrap_or(&file).to_string();
        let mut out = vec!["crate".to_string()];
        out.extend(relative);
        if !matches!(stem.as_str(), "lib" | "main" | "mod") {
            out.push(stem);
        }
        out
    }

    /// The kit function or module text carrier a path names, following a module's public
    /// re-exports (`crate::dialogs::form_row` → `crate::ui_kit::row`). Only a path through
    /// `ui_kit` (or through a listed module) counts, so a same-named function elsewhere
    /// (`ContentWidth::button`, `other::button`) stays out.
    fn resolve_target(path: &[String], reexports: &BTreeMap<Vec<String>, BTreeMap<String, Vec<String>>>) -> Option<Target> {
        let mut path = path.to_vec();
        for _ in 0..4 {
            let (name, module) = path.split_last()?;
            if let Some(at) = module.iter().position(|segment| segment == "ui_kit")
                && (module.len() == at + 1 || KIT_MODULES.contains(&module[at + 1].as_str()))
                && let Some(kit) = free_kit(name)
            {
                return Some(Target::Kit(kit));
            }
            if let Some(call) = module_call(name, module) {
                return Some(Target::Module(call));
            }
            path = reexports.get(module)?.get(name)?.clone();
        }
        None
    }

    /// The module text carrier `name` names, when the last segment of `module` is its module.
    fn module_call(name: &str, module: &[String]) -> Option<&'static ModuleCall> {
        MODULE_CALLS.iter().find(|call| call.name == name && module.last().is_some_and(|segment| segment == call.module))
    }

    /// Make a `use`/call path absolute against the module of the file it appears in:
    /// `super::row` in `ui_kit/choice.rs` → `crate::ui_kit::row`, `self::x` → the file's own
    /// module, `crate::…` unchanged.
    fn normalize(module: &[String], path: &[String]) -> Vec<String> {
        match path.first().map(String::as_str) {
            Some("crate") => path.to_vec(),
            Some("self") => {
                let mut out = module.to_vec();
                out.extend_from_slice(&path[1..]);
                out
            }
            Some("super") => {
                let supers = path.iter().take_while(|segment| segment.as_str() == "super").count();
                let mut out = module.to_vec();
                for _ in 0..supers {
                    out.pop();
                }
                out.extend_from_slice(&path[supers..]);
                out
            }
            _ => path.to_vec(),
        }
    }

    /// The target a qualified call names: through a module binding (`kit::row`, `l::row`) or a
    /// path relative to the file's module (`super::row`, `super::actions::…`).
    fn resolve_qualified(context: &Context, names: &Names, module: &[String], qualifier: &[String], name: &str) -> Option<Target> {
        if let Some(bound) = qualifier.first().and_then(|first| names.modules.get(first)) {
            let mut path = bound.clone();
            path.extend_from_slice(&qualifier[1..]);
            path.push(name.to_string());
            if let Some(target) = resolve_target(&path, &context.reexports) {
                return Some(target);
            }
        }
        let mut path = normalize(module, qualifier);
        path.push(name.to_string());
        resolve_target(&path, &context.reexports)
    }

    /// A module text carrier called inside its own module (`button` in `context_menu.rs`).
    fn module_inside(path: &std::path::Path, name: &str) -> Option<&'static ModuleCall> {
        let module = module_path(path);
        MODULE_CALLS.iter().find(|call| call.name == name && module.last().is_some_and(|segment| segment == call.module))
    }

    fn free_kit(name: &str) -> Option<&'static str> {
        KIT_CALLS.iter().find(|call| call.name == name).map(|call| call.name)
    }

    /// A standalone snippet, as if it were the file `src/live.rs`, for the scanner tests.
    fn literals(source: &str) -> Vec<(String, bool)> {
        literals_with(source, &[])
    }

    /// [`literals`] with extra files beside the snippet (a module whose re-exports it imports).
    fn literals_with(source: &str, extra: &[(&str, &str)]) -> Vec<(String, bool)> {
        let snippet = std::path::PathBuf::from("src/live.rs");
        let mut files = vec![(snippet.clone(), source.to_string())];
        files.extend(extra.iter().map(|(path, source)| (std::path::PathBuf::from(path), (*source).to_string())));
        literals_in(&Context::new(&files), &snippet, source)
    }

    /// [`literals`] for a snippet at a given crate path (for a file inside `ui_kit/`).
    fn literals_at(path: &str, source: &str) -> Vec<(String, bool)> {
        let path = std::path::PathBuf::from(path);
        literals_in(&Context::new(&[(path.clone(), source.to_string())]), &path, source)
    }

    /// Every label and id literal in `source` as `(text, id-only)`.
    fn literals_in(context: &Context, path: &std::path::Path, source: &str) -> Vec<(String, bool)> {
        // A file the tokenizer refuses would otherwise pass unchecked.
        let tokens = source.parse::<TokenStream>().expect("a UI source file tokenizes");
        // Which `tr` the file means, read from its tokens (no comments) with the spacing removed.
        let code: String = tokens.to_string().split_whitespace().collect();
        let label_file = code.contains("fntr(text:&str)->&str") || code.contains("lettr=crate::i18n::tr_label;");
        let id_file = code.contains("usecrate::i18n::tr;") || code.contains("lettr=crate::i18n::tr;") || imports_i18n_tr(&code);
        let names = context.names(path);
        let mut out = Vec::new();
        scan(tokens.into_iter().collect(), label_file, id_file, &names, context, path, &mut out);
        out
    }

    /// Walk `tokens`, recording each call's literal; recurse into groups and skip `#[cfg(test)]`.
    fn scan(
        tokens: Vec<TokenTree>,
        label_file: bool,
        id_file: bool,
        names: &Names,
        context: &Context,
        path: &std::path::Path,
        out: &mut Vec<(String, bool)>,
    ) {
        let inside_kit = context.inside_kit(path);
        let module = module_path(path);
        let mut i = 0;
        while i < tokens.len() {
            if cfg_test(&tokens, i) {
                i = skip_item(&tokens, i + 2);
                continue;
            }
            if let TokenTree::Ident(ident) = &tokens[i] {
                let written = ident.to_string();
                let previous = tokens.get(i.wrapping_sub(1));
                let method = matches!(previous, Some(TokenTree::Punct(p)) if p.as_char() == '.');
                let qualified = matches!(previous, Some(TokenTree::Punct(p)) if p.as_char() == ':');
                let definition = matches!(previous, Some(TokenTree::Ident(id)) if id == "fn");
                if !definition {
                    if method {
                        // A receiver's type is unknown here, so a method is read by its name
                        // alone: a known limit (a foreign `.actions(…)` would be read as well).
                        kit_method(&written, &tokens, i, out);
                    } else {
                        match written.as_str() {
                            // The dialogs' own `tr` wraps `tr_label`; an imported `i18n::tr` keeps its id semantics.
                            "tr" if !qualified => push(&tokens, i + 1, &[0], id_file && !label_file, &[], out),
                            "tr_label" => push(&tokens, i + 1, &[0], false, &[], out),
                            "SelInput" if is_path(&tokens, i, "new") => push(&tokens, i + 4, &[0], false, &[], out),
                            "i18n" if is_path(&tokens, i, "tr") => push(&tokens, i + 4, &[0], true, &[], out),
                            _ => {
                                // `Column::new`, `Menu::heading`, …: a type path, read by name (the
                                // same known limit as the methods).
                                kit_path(&written, &tokens, i, out);
                                // A free kit function counts from its origin only: called through
                                // `ui_kit::`, imported from `ui_kit` (or a module re-exporting it),
                                // or called inside `ui_kit/` itself.
                                let qualifier = qualifier(&tokens, i);
                                let resolved = if qualifier.is_empty() {
                                    names
                                        .functions
                                        .get(&written)
                                        .copied()
                                        .or_else(|| {
                                            // `use crate::ui_kit::*;` (or `use super::*;` in the kit)
                                            // makes the kit's functions callable unqualified.
                                            names.globs.iter().find_map(|glob| {
                                                let mut path = glob.clone();
                                                path.push(written.clone());
                                                resolve_target(&path, &context.reexports)
                                            })
                                        })
                                        .or_else(|| inside_kit.then(|| free_kit(&written)).flatten().map(Target::Kit))
                                        .or_else(|| module_inside(path, &written).map(Target::Module))
                                } else {
                                    resolve_qualified(context, names, &module, &qualifier, &written)
                                };
                                match resolved {
                                    Some(Target::Kit(kit)) => kit_free(kit, &tokens, i, out),
                                    Some(Target::Module(call)) => push(&tokens, i + 1, call.args, false, &[], out),
                                    None => {}
                                }
                            }
                        }
                    }
                    kit_fields(&written, &tokens, i, out);
                }
            }
            if let TokenTree::Group(group) = &tokens[i] {
                scan(group.stream().into_iter().collect(), label_file, id_file, names, context, path, out);
            }
            i += 1;
        }
    }

    /// Record the label literals of a method call (`.actions`, `.add_prompt`), read by name.
    fn kit_method(written: &str, tokens: &[TokenTree], i: usize, out: &mut Vec<(String, bool)>) {
        for call in KIT_CALLS {
            if let Some(name) = call.name.strip_prefix('.')
                && written == name
            {
                push(tokens, i + 1, call.args, call.id, call.trim, out);
            }
        }
    }

    /// Record the label literals of a type-path call (`Column::new`, `Menu::heading`), read by the
    /// type's name — the same known limit as the methods.
    fn kit_path(written: &str, tokens: &[TokenTree], i: usize, out: &mut Vec<(String, bool)>) {
        for call in KIT_CALLS {
            if let Some((ty, name)) = call.name.split_once("::")
                && written == ty
                && is_path(tokens, i, name)
            {
                push(tokens, i + 4, call.args, call.id, call.trim, out);
            }
        }
    }

    /// Record the label literals of a free kit call that `resolve_kit` resolved to `kit`.
    fn kit_free(kit: &'static str, tokens: &[TokenTree], i: usize, out: &mut Vec<(String, bool)>) {
        for call in KIT_CALLS.iter().filter(|call| call.name == kit) {
            push(tokens, i + 1, call.args, call.id, call.trim, out);
        }
    }

    /// The translated field of a struct literal the kit draws (`Footer { apply: Some("Apply") }`).
    fn kit_fields(written: &str, tokens: &[TokenTree], i: usize, out: &mut Vec<(String, bool)>) {
        let Some(TokenTree::Group(braces)) = tokens.get(i + 1) else { return };
        if braces.delimiter() != Delimiter::Brace {
            return;
        }
        for spec in KIT_FIELDS.iter().filter(|spec| spec.owner == written) {
            for member in arguments(braces.stream().into_iter().collect()) {
                if !matches!(member.first(), Some(TokenTree::Ident(name)) if *name == spec.field)
                    || !matches!(member.get(1), Some(TokenTree::Punct(p)) if p.as_char() == ':')
                {
                    continue;
                }
                for literal in argument_literals(&member[2..]) {
                    out.push((literal, spec.id));
                }
            }
        }
    }

    /// The module path before a call's name (`crate::ui_kit::row` → `["crate", "ui_kit"]`).
    fn qualifier(tokens: &[TokenTree], i: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut k = i;
        while k >= 3
            && matches!(&tokens[k - 1], TokenTree::Punct(p) if p.as_char() == ':')
            && matches!(&tokens[k - 2], TokenTree::Punct(p) if p.as_char() == ':')
        {
            match &tokens[k - 3] {
                TokenTree::Ident(segment) => out.push(segment.to_string()),
                _ => break,
            }
            k -= 3;
        }
        out.reverse();
        out
    }

    /// Record the string literals that are argument `args` of the parenthesised group at `group`;
    /// `trim` drops the leading indent of the positions the kit normalises at run time.
    fn push(tokens: &[TokenTree], group: usize, args: &[usize], id_only: bool, trim: &[usize], out: &mut Vec<(String, bool)>) {
        let Some(TokenTree::Group(g)) = tokens.get(group) else { return };
        if g.delimiter() != Delimiter::Parenthesis {
            return;
        }
        let arguments = arguments(g.stream().into_iter().collect());
        for &arg in args {
            for value in arguments.get(arg).map_or_else(Vec::new, |tokens| argument_literals(tokens)) {
                out.push((if trim.contains(&arg) { value.trim_start().to_string() } else { value }, id_only));
            }
        }
    }

    /// Split a call's or a struct literal's tokens at its top-level commas. Nested calls, closures,
    /// blocks and lists are single groups, so their commas stay inside the member they belong to.
    fn arguments(tokens: Vec<TokenTree>) -> Vec<Vec<TokenTree>> {
        let mut out = vec![Vec::new()];
        for token in tokens {
            if matches!(&token, TokenTree::Punct(p) if p.as_char() == ',') {
                out.push(Vec::new());
            } else if let Some(argument) = out.last_mut() {
                argument.push(token);
            }
        }
        out
    }

    /// The label literal of one argument: `"A"`, `&"A"`, `&r#"A"#`, `Some("A")`, or every entry
    /// of a list (`&["A", "B"]`, `&mut [("A", a), ("B", b)]`). Anything else (a `match`, a
    /// `format!`, a variable) has no single label to read and contributes nothing.
    fn argument_literals(tokens: &[TokenTree]) -> Vec<String> {
        let tokens: Vec<&TokenTree> = tokens
            .iter()
            .filter(|token| !matches!(token, TokenTree::Punct(p) if p.as_char() == '&') && !matches!(token, TokenTree::Ident(id) if id == "mut"))
            .collect();
        match tokens.as_slice() {
            [TokenTree::Literal(lit)] => string_literal(lit).into_iter().collect(),
            [TokenTree::Ident(some), TokenTree::Group(group)] if *some == "Some" && group.delimiter() == Delimiter::Parenthesis => {
                argument_literals(&group.stream().into_iter().collect::<Vec<_>>())
            }
            [TokenTree::Group(group)] if group.delimiter() == Delimiter::Bracket => {
                arguments(group.stream().into_iter().collect()).iter().filter_map(|entry| list_entry_literal(entry)).collect()
            }
            _ => Vec::new(),
        }
    }

    /// One entry of a label list: `"A"` or the first literal of a tuple (`("A", &mut a)`).
    fn list_entry_literal(tokens: &[TokenTree]) -> Option<String> {
        match tokens {
            [TokenTree::Literal(lit)] => string_literal(lit),
            [TokenTree::Group(group)] if group.delimiter() == Delimiter::Parenthesis => match group.stream().into_iter().next() {
                Some(TokenTree::Literal(lit)) => string_literal(&lit),
                _ => None,
            },
            _ => None,
        }
    }

    fn string_literal(lit: &proc_macro2::Literal) -> Option<String> {
        syn::parse_str::<syn::LitStr>(&lit.to_string()).ok().map(|text| text.value())
    }

    /// Every `use` declaration in `source` as its leaves and globs, with paths made absolute
    /// against the module `module` of the file.
    fn parsed_imports(source: &str, module: &[String]) -> Vec<Import> {
        let mut out = Vec::new();
        if let Ok(tokens) = source.parse::<TokenStream>() {
            collect_imports(tokens.into_iter().collect(), module, &mut out);
        }
        out
    }

    fn collect_imports(tokens: Vec<TokenTree>, module: &[String], out: &mut Vec<Import>) {
        let mut i = 0;
        while i < tokens.len() {
            if cfg_test(&tokens, i) {
                i = skip_item(&tokens, i + 2);
                continue;
            }
            if matches!(&tokens[i], TokenTree::Ident(id) if id == "use") {
                let public = matches!(tokens.get(i.wrapping_sub(1)), Some(TokenTree::Ident(id)) if id == "pub")
                    || matches!(tokens.get(i.wrapping_sub(1)), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis)
                        && matches!(tokens.get(i.wrapping_sub(2)), Some(TokenTree::Ident(id)) if id == "pub");
                let mut j = i + 1;
                let mut tree = Vec::new();
                while j < tokens.len() && !matches!(&tokens[j], TokenTree::Punct(p) if p.as_char() == ';') {
                    tree.push(tokens[j].clone());
                    j += 1;
                }
                use_tree(&tree, &[], module, public, out);
                i = j;
            }
            if let TokenTree::Group(group) = &tokens[i] {
                collect_imports(group.stream().into_iter().collect(), module, out);
            }
            i += 1;
        }
    }

    /// Solve one `use` tree: a path with nested `{…}` groups, `as` aliases and `*` globs.
    fn use_tree(tokens: &[TokenTree], prefix: &[String], module: &[String], public: bool, out: &mut Vec<Import>) {
        let mut path = prefix.to_vec();
        let mut local = None;
        let mut i = 0;
        while i < tokens.len() {
            match &tokens[i] {
                TokenTree::Ident(id) if id == "as" => {
                    if let Some(TokenTree::Ident(alias)) = tokens.get(i + 1) {
                        local = Some(alias.to_string());
                    }
                    i += 2;
                    continue;
                }
                TokenTree::Ident(id) => path.push(id.to_string()),
                TokenTree::Punct(p) if p.as_char() == '*' => {
                    out.push(Import::Glob { path: normalize(module, &path) });
                    return;
                }
                TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
                    for part in arguments(group.stream().into_iter().collect()) {
                        use_tree(&part, &path, module, public, out);
                    }
                    return;
                }
                _ => {}
            }
            i += 1;
        }
        if path.is_empty() {
            return;
        }
        let last = path.last().cloned().unwrap_or_default();
        if last == "self" {
            // `use crate::ui_kit::{self, …}` binds the module itself.
            let target = normalize(module, &path[..path.len() - 1]);
            let local = local.unwrap_or_else(|| target.last().cloned().unwrap_or_default());
            out.push(Import::Leaf { path: target, local, public });
        } else {
            let target = normalize(module, &path);
            out.push(Import::Leaf { path: target, local: local.unwrap_or(last), public });
        }
    }

    /// The files a source file includes as *external* test modules (`#[cfg(test)] mod gallery;`),
    /// resolved like rustc: `name.rs` or `name/mod.rs` in the module directory, or a `#[path]` file
    /// relative to the declaring file. An inline `#[cfg(test)] mod tests { … }` includes no file.
    fn test_module_files(declaring: &std::path::Path, source: &str) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        if let Ok(tokens) = source.parse::<TokenStream>() {
            collect_test_files(declaring, tokens.into_iter().collect(), &mut out);
        }
        out
    }

    fn collect_test_files(declaring: &std::path::Path, tokens: Vec<TokenTree>, out: &mut Vec<std::path::PathBuf>) {
        let mut i = 0;
        while i < tokens.len() {
            if cfg_test(&tokens, i) {
                // Only the annotated item counts. Further attributes and a `pub` may sit between
                // `#[cfg(test)]` and an external `mod name;`; a `fn`, a `struct` or an inline
                // `mod name { … }` includes no file (and a following `mod panel;` is production).
                let mut k = i + 2;
                let mut attribute = None;
                loop {
                    if let (Some(TokenTree::Punct(hash)), Some(TokenTree::Group(group))) = (tokens.get(k), tokens.get(k + 1))
                        && hash.as_char() == '#'
                        && group.delimiter() == Delimiter::Bracket
                    {
                        let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                        if let [TokenTree::Ident(id), TokenTree::Punct(p), TokenTree::Literal(lit)] = inner.as_slice()
                            && id == "path"
                            && p.as_char() == '='
                            && let Some(text) = string_literal(lit)
                        {
                            attribute = Some(text);
                        }
                        k += 2;
                        continue;
                    }
                    if let Some(TokenTree::Ident(vis)) = tokens.get(k)
                        && vis == "pub"
                    {
                        k += if matches!(tokens.get(k + 1), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis) {
                            2
                        } else {
                            1
                        };
                        continue;
                    }
                    break;
                }
                if matches!(tokens.get(k), Some(TokenTree::Ident(id)) if id == "mod")
                    && let Some(TokenTree::Ident(name)) = tokens.get(k + 1)
                    && matches!(tokens.get(k + 2), Some(TokenTree::Punct(p)) if p.as_char() == ';')
                {
                    out.extend(module_file(declaring, &name.to_string(), attribute.as_deref()));
                }
                i = skip_item(&tokens, i + 2);
                continue;
            }
            if let TokenTree::Group(group) = &tokens[i] {
                collect_test_files(declaring, group.stream().into_iter().collect(), out);
            }
            i += 1;
        }
    }

    /// The files a `mod name;` declaration may name, from the declaring file.
    fn module_file(declaring: &std::path::Path, name: &str, attribute: Option<&str>) -> Vec<std::path::PathBuf> {
        let dir = declaring.parent().unwrap_or(std::path::Path::new("."));
        if let Some(path) = attribute {
            // A `#[path]` on a non-inline module is relative to the declaring file's directory.
            let mut candidates = vec![dir.join(path)];
            if !path.ends_with(".rs") {
                candidates.push(dir.join(path).join("mod.rs"));
            }
            return candidates;
        }
        let file = declaring.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        let module_dir =
            if matches!(file, "mod.rs" | "lib.rs" | "main.rs") { dir.to_path_buf() } else { dir.join(declaring.file_stem().unwrap_or_default()) };
        vec![module_dir.join(format!("{name}.rs")), module_dir.join(name).join("mod.rs")]
    }

    fn is_path(tokens: &[TokenTree], i: usize, name: &str) -> bool {
        let colon = |k: usize| matches!(tokens.get(k), Some(TokenTree::Punct(p)) if p.as_char() == ':');
        colon(i + 1) && colon(i + 2) && matches!(tokens.get(i + 3), Some(TokenTree::Ident(id)) if *id == name)
    }

    /// Whether `tokens[i]` starts the attribute `#[cfg(test)]`.
    fn cfg_test(tokens: &[TokenTree], i: usize) -> bool {
        matches!(tokens.get(i), Some(TokenTree::Punct(p)) if p.as_char() == '#')
            && matches!(tokens.get(i + 1), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Bracket
                && g.stream().to_string().split_whitespace().collect::<String>() == "cfg(test)")
    }

    /// Past the item starting at `i`: its first `{…}` body or `;`. `#[cfg(test)]` on a single field,
    /// array element or expression is not modelled (the UI sources have none).
    fn skip_item(tokens: &[TokenTree], mut i: usize) -> usize {
        while i < tokens.len()
            && !matches!(&tokens[i], TokenTree::Group(g) if g.delimiter() == Delimiter::Brace)
            && !matches!(&tokens[i], TokenTree::Punct(p) if p.as_char() == ';')
        {
            i += 1;
        }
        (i + 1).min(tokens.len())
    }

    /// Whether the file imports `tr` from `crate::i18n` inside a use list, from the use list in
    /// `code` (spacing removed) under its own name.
    fn imports_i18n_tr(code: &str) -> bool {
        code.split_once("usecrate::i18n::{")
            .is_some_and(|(_, rest)| rest.split_once('}').is_some_and(|(list, _)| list.split(',').any(|name| name == "tr")))
    }

    /// The names the hand-written kit probes import, exactly as a UI file would.
    const KIT_IMPORTS: &str = concat!(
        "use crate::ui_kit::{row, status_line, expr_row, hint_row, text_row, choice_row, section, section_count, ",
        "multiline_text_field, dropdown, cell_editor, bounded_cell_editor, search_field, menu_filter, placement_field, ",
        "library_cell, icon_button_at, icon_button_colored, footer, button, primary_button, ",
        "wide_primary_button, action_menu, compact_button, icon_label_button, command_menu_button, ",
        "checkbox_grid, options, checkbox_row, toggle_icon, removable_header, bin_button, ",
        "Column, Menu, MenuItem, CommandSearch};"
    );

    /// A snippet as a UI file: the kit imports, then the call under test.
    fn kit(source: &str) -> Vec<(String, bool)> {
        literals(&format!("{KIT_IMPORTS}\nfn live() {{ {source} }}"))
    }

    fn texts(found: &[(String, bool)]) -> Vec<&str> {
        found.iter().map(|(text, _)| text.as_str()).collect()
    }

    /// One call per kit function, written with the signature it really has. The guard must read
    /// the label where the kit translates it — including the six `(ui, label, …)` actions whose
    /// label is the second argument and `choice_row`'s options.
    #[test]
    fn every_kit_call_is_read_at_its_real_signature() {
        let cases: &[(&str, &[&str])] = &[
            ("row(ui, \"row\", |ui| {})", &["row"]),
            ("status_line(ui, \"status_line\", StatusTone::Dim)", &["status_line"]),
            ("expr_row(ui, \"expr_row\", value)", &["expr_row"]),
            ("hint_row(ui, \"hint_row\", value, hint)", &["hint_row"]),
            ("text_row(ui, \"text_row\", value, hint)", &["text_row"]),
            (
                "choice_row(ui, \"choice_row\", id, selected, &[\"First option\", \"Second option\"])",
                &["choice_row", "First option", "Second option"],
            ),
            ("section(ui, id, \"section\", true, |ui| {})", &["section"]),
            ("section_count(ui, id, \"section_count\", true, Some(2), |ui| {})", &["section_count"]),
            ("multiline_text_field(ui, \"multiline_text_field\", id, value)", &["multiline_text_field"]),
            ("dropdown(ui, id, selected, &[\"dropdown option\"])", &["dropdown option"]),
            ("cell_editor(ui, id, value, \"cell_editor\", invalid)", &["cell_editor"]),
            ("bounded_cell_editor(ui, id, value, \"bounded_cell_editor\", invalid, limit)", &["bounded_cell_editor"]),
            ("search_field(ui, id, value, \"search_field\")", &["search_field"]),
            ("menu_filter(ui, id, value, \"menu_filter\")", &["menu_filter"]),
            ("placement_field(ui, id, value, \"placement_field\")", &["placement_field"]),
            ("library_cell(ui, id, value, \"library_cell\", |ui| {})", &["library_cell"]),
            ("icon_button_at(ui, rect, icon, \"icon_button_at\")", &["icon_button_at"]),
            ("icon_button_colored(ui, rect, icon, \"icon_button_colored\", None)", &["icon_button_colored"]),
            ("footer(ui, \"footer ok\", \"footer cancel\", true, Footer::default())", &["footer ok", "footer cancel"]),
            ("button(ui, \"button\", Button::Normal)", &["button"]),
            ("primary_button(ui, \"primary_button\")", &["primary_button"]),
            ("wide_primary_button(ui, \"wide_primary_button\")", &["wide_primary_button"]),
            ("action_menu(ui, \"action_menu\", |ui| {})", &["action_menu"]),
            ("compact_button(ui, glyph, \"compact_button\", true, false)", &["compact_button"]),
            ("icon_label_button(ui, \"icon_label_button\", icon, true, false)", &["icon_label_button"]),
            ("command_menu_button(ui, id, \"command_menu_button\", icon, shortcut, true)", &["command_menu_button"]),
            ("checkbox_grid(ui, &mut [(\"checkbox_grid\", value)], 1)", &["checkbox_grid"]),
            ("options(ui, id, &mut [(\"options\", value)])", &["options"]),
            ("checkbox_row(ui, key, \"checkbox_row\", true)", &["checkbox_row"]),
            ("toggle_icon(ui, row, \"toggle_icon\", selected, icon)", &["toggle_icon"]),
            ("Column::new(\"Column::new\", 1.0)", &["Column::new"]),
            ("table.add_prompt(ui, \"add_prompt\", \"add_prompt tooltip\")", &["add_prompt", "add_prompt tooltip"]),
            ("window.actions(\"actions ok\", \"actions cancel\", Some(\"actions apply\"))", &["actions ok", "actions cancel", "actions apply"]),
            ("removable_header(ui, \"removable_header\")", &["removable_header"]),
            ("bin_button(ui, \"bin_button\")", &["bin_button"]),
            ("Menu::heading(ui, \"Menu::heading\")", &["Menu::heading"]),
            ("MenuItem::new(id, \"MenuItem::new\", icon)", &["MenuItem::new"]),
        ];
        for (call, expected) in cases {
            assert_eq!(texts(&kit(call)), *expected, "{call}");
        }
        assert_eq!(kit("CommandSearch::heading(ui, \"search.commands\", 3)"), [("search.commands".to_string(), true)]);
    }

    /// `Footer { apply }` is drawn (and translated) by the kit footer (actions.rs:108–116).
    #[test]
    fn a_footer_apply_label_is_found() {
        let source = "footer(ui, \"OK\", \"Cancel\", true, Footer { apply: Some(\"Review apply\"), ..Default::default() })";
        assert_eq!(texts(&kit(source)), ["OK", "Cancel", "Review apply"]);
    }

    /// `PairButton { label }` is drawn translated by `button_pair` (actions.rs:217).
    #[test]
    fn a_pair_button_label_is_found() {
        let source = "button_pair(ui, key, [false, false], [PairButton { key: \"a\", label: \"Look At\", icon: None }, \
                      PairButton { key: \"b\", label: \"Slice\", icon: None }])";
        assert_eq!(texts(&kit(source)), ["Look At", "Slice"]);
    }

    /// `PopupOption { kind }` is drawn translated in the completion popup (choice.rs:379).
    #[test]
    fn a_popup_option_kind_is_found() {
        let source = "popup_list(ctx, id, anchor, 0, &[PopupOption { text: \"t\", detail: \"d\", kind: \"Parameter\", color }], hint)";
        assert_eq!(kit(source), [("Parameter".to_string(), false)]);
    }

    /// A preferences page's `label` reaches `tr`, an id (panel.rs:561).
    #[test]
    fn a_window_page_label_is_an_id() {
        let source = "const PAGES: [WindowPage<App>; 1] = [WindowPage { icon: \"settings\", label: \"ui.general\", scroll: true }];";
        assert_eq!(literals(source), [("ui.general".to_string(), true)]);
    }

    /// An inline `#[cfg(test)] mod … { … }` is skipped where it stands and excludes no file.
    #[test]
    fn an_inline_test_module_does_not_exclude_a_file() {
        let declaring = std::path::Path::new("/crate/src/ui_kit/mod.rs");
        assert!(test_module_files(declaring, "#[cfg(test)] mod panel { fn helper() {} }").is_empty());
        assert_eq!(
            test_module_files(declaring, "#[cfg(test)] mod gallery;"),
            [std::path::PathBuf::from("/crate/src/ui_kit/gallery.rs"), std::path::PathBuf::from("/crate/src/ui_kit/gallery/mod.rs")]
        );
    }

    /// Only the item `#[cfg(test)]` annotates counts: a following production `mod panel;` is not
    /// a test file (the old search ran past the test function).
    #[test]
    fn a_test_function_does_not_exclude_a_following_module() {
        let declaring = std::path::Path::new("/crate/src/ui_kit/mod.rs");
        assert!(test_module_files(declaring, "#[cfg(test)] fn helper() {}\nmod panel;").is_empty());
    }

    /// A test module with a `#[path]` attribute is still resolved (and excluded).
    #[test]
    fn a_test_module_with_a_path_attribute_is_still_excluded() {
        let files = test_module_files(std::path::Path::new("/crate/src/ui_kit/mod.rs"), "#[cfg(test)] #[path = \"x_tests.rs\"] mod x_tests;");
        assert_eq!(files, [std::path::PathBuf::from("/crate/src/ui_kit/x_tests.rs")]);
    }

    /// A `#[path]` attribute resolves relative to the declaring file's directory.
    #[test]
    fn a_path_attribute_resolves_beside_the_declaring_file() {
        let files = test_module_files(std::path::Path::new("/crate/src/ui_kit/layout.rs"), "#[cfg(test)] #[path = \"layout_tests.rs\"] mod tests;");
        assert_eq!(files, [std::path::PathBuf::from("/crate/src/ui_kit/layout_tests.rs")]);
    }

    /// `row` reaches the dialogs as `form_row`; the alias must not hide a label.
    #[test]
    fn a_kit_function_imported_under_an_alias_is_found() {
        let source = "use crate::ui_kit::{field, row as form_row}; fn live() { form_row(ui, \"missing\", |ui| {}); }";
        assert_eq!(literals(source), [("missing".to_string(), false)]);
    }

    /// A plain `use … as …` binds the kit function too.
    #[test]
    fn a_plain_import_alias_is_found() {
        let source = "use crate::ui_kit::row as custom_row; fn live() { custom_row(ui, \"missing\", |ui| {}); }";
        assert_eq!(literals(source), [("missing".to_string(), false)]);
    }

    /// A nested `use` group binds `layout::row` under its alias.
    #[test]
    fn a_nested_import_alias_is_found() {
        let source = "use crate::ui_kit::{layout::{row as r}, field}; fn live() { r(ui, \"missing\", |ui| {}); }";
        assert_eq!(literals(source), [("missing".to_string(), false)]);
    }

    /// An alias belongs to its file: the same name in another file is no kit call.
    #[test]
    fn an_alias_in_another_file_does_not_apply() {
        assert_eq!(
            literals("use crate::ui_kit::row as custom_row; fn live() { custom_row(ui, \"in file\", |ui| {}); }"),
            [("in file".to_string(), false)]
        );
        assert!(literals("fn live() { custom_row(ui, \"other file\", |ui| {}); }").is_empty());
    }

    /// `dialogs` re-exports `row` as `form_row`; importing that name still names the kit.
    #[test]
    fn a_reexported_kit_name_is_followed() {
        let source = "use crate::dialogs::form_row; fn live() { form_row(ui, \"missing\", |ui| {}); }";
        assert_eq!(
            literals_with(source, &[("src/dialogs.rs", "pub(crate) use crate::ui_kit::{field, row as form_row};")]),
            [("missing".to_string(), false)]
        );
    }

    /// A free function of another module is no kit call, even when it is named `row`.
    #[test]
    fn a_free_function_from_another_module_is_not_a_kit_call() {
        assert!(literals("fn live() { other::row(ui, \"Not a kit label\", |ui| {}); }").is_empty());
    }

    /// `use crate::ui_kit as kit;` keeps the module binding for qualified calls.
    #[test]
    fn a_kit_module_import_binds_qualified_calls() {
        assert_eq!(literals("use crate::ui_kit as kit; fn f() { kit::row(ui, \"Missing\", |ui| {}); }"), [("Missing".to_string(), false)]);
        assert_eq!(literals("use crate::ui_kit::layout as l; fn f() { l::row(ui, \"Missing\", |ui| {}); }"), [("Missing".to_string(), false)]);
    }

    /// A glob import of the kit makes its functions callable unqualified.
    #[test]
    fn a_kit_glob_import_binds_unqualified_calls() {
        assert_eq!(literals("use crate::ui_kit::*; fn f() { row(ui, \"Missing\", |ui| {}); }"), [("Missing".to_string(), false)]);
    }

    /// A relative alias import in a kit file resolves against the file's module path.
    #[test]
    fn a_relative_alias_import_is_resolved() {
        let source = "use super::{row as custom_row}; fn f() { custom_row(ui, \"Missing\", |ui| {}); }";
        assert_eq!(literals_at("src/ui_kit/choice.rs", source), [("Missing".to_string(), false)]);
    }

    /// A glob from another module does not make a same-named function a kit call.
    #[test]
    fn a_foreign_glob_import_is_not_a_kit_call() {
        assert!(literals("use crate::other::*; fn f() { row(ui, \"X\", |ui| {}); }").is_empty());
    }

    /// A call through the kit's own path is a kit call without an import.
    #[test]
    fn a_kit_path_through_ui_kit_is_found() {
        assert_eq!(literals("fn live() { ui_kit::row(ui, \"missing\", |ui| {}); }"), [("missing".to_string(), false)]);
    }

    /// A call inside `ui_kit/` counts without an import, as `ui_kit/choice.rs` calls `section`.
    #[test]
    fn a_call_inside_the_kit_is_found() {
        let source = "fn options(ui: &mut Ui, id: &str, items: &mut [(&str, &mut bool)]) { section(ui, id, \"missing\", true, |ui| {}); }";
        assert_eq!(literals_at("src/ui_kit/choice.rs", source), [("missing".to_string(), false)]);
    }

    /// Only the row label is normalised at run time (`sized_row`); everything else keeps its spaces.
    #[test]
    fn only_row_labels_are_trimmed() {
        assert_eq!(kit("expr_row(ui, \"  Minimum\", value)"), [("Minimum".to_string(), false)]);
        assert_eq!(literals("fn live() { tr_label(\"  Radius\"); }"), [("  Radius".to_string(), false)]);
        assert_eq!(kit("dropdown(ui, id, selected, &[\"  Option\"])"), [("  Option".to_string(), false)]);
        assert_eq!(
            kit("choice_row(ui, \"  Label\", id, selected, &[\"  Option\"])"),
            [("Label".to_string(), false), ("  Option".to_string(), false)]
        );
    }

    /// The reference cases from the review, kept as tests of the token scanner.
    #[test]
    fn ordinary_and_multiline_row_calls_are_found() {
        assert_eq!(literals("fn live() { row_label(\n ui, \n\"missing\"); }").len(), 0);
        assert_eq!(kit("row(\n ui, \n\"missing\", |ui| {});").len(), 1);
    }

    #[test]
    fn cfg_test_function_with_lifetime_preserves_production() {
        let found = literals("#[cfg(test)] fn helper<'a>(x: &'a str) { let c = '}'; } fn live() { tr_label(\"missing\"); }");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "missing");
    }

    #[test]
    fn multiline_bare_tr_is_found() {
        let found = literals("use crate::i18n::tr; fn live() { tr(\n\"missing\"); }");
        assert_eq!(found.len(), 1);
        assert!(found[0].1, "tr imported from crate::i18n is an id");
    }

    #[test]
    fn raw_label_is_found() {
        assert_eq!(literals(r##"fn live() { tr_label(r#"missing"#); }"##).len(), 1);
    }

    #[test]
    fn comments_are_ignored() {
        assert_eq!(literals("// tr_label(\"missing\")\nfn live() {}").len(), 0);
    }

    #[test]
    fn raw_string_in_test_function_preserves_production() {
        let s = r###"#[cfg(test)] fn helper() { let s = r#"" { "#; } fn live() { tr_label("missing"); }"###;
        assert_eq!(literals(s).len(), 1);
    }

    #[test]
    fn cfg_marker_in_comment_preserves_production() {
        assert_eq!(literals("// #[cfg(test)]\nfn live() { tr_label(\"missing\"); }").len(), 1);
    }

    #[test]
    fn decoded_escape_matches_catalog_text() {
        assert_eq!(literals(r#"tr_label("Say \"hello\"")"#)[0].0, "Say \"hello\"");
    }

    #[test]
    fn a_call_inside_a_macro_is_found() {
        assert_eq!(literals("fn live() { format!(\"{}\", tr_label(\"missing\")); }").len(), 1);
    }

    #[test]
    fn a_row_label_after_a_longer_first_argument_is_found() {
        assert_eq!(
            kit("row(&mut ui, \"missing\", |ui| {}); row(self.ui, \"  other\", |ui| {});"),
            [("missing".to_string(), false), ("other".to_string(), false)]
        );
    }

    /// `context_menu::button` hands its label to `MenuItem::new` → `tr_label` (menu.rs:210); all
    /// three origins count: the `crate::context_menu::…` path, a `use` import and a call inside
    /// `context_menu.rs` itself.
    #[test]
    fn a_context_menu_button_label_is_found() {
        assert_eq!(
            literals("fn live() { crate::context_menu::button(ui, \"Add to New Folder\", \"folder\"); }"),
            [("Add to New Folder".to_string(), false)]
        );
        assert_eq!(
            literals("use crate::context_menu::button; fn live() { button(ui, \"Remove from Folder\", \"folder\"); }"),
            [("Remove from Folder".to_string(), false)]
        );
        assert_eq!(
            literals_at("src/context_menu.rs", "fn f() { button(ui, \"Roll Back Here\", \"history\"); }"),
            [("Roll Back Here".to_string(), false)]
        );
    }

    /// `context_menu::button_enabled` takes the same label at the same position.
    #[test]
    fn a_context_menu_button_enabled_label_is_found() {
        assert_eq!(
            literals("fn live() { context_menu::button_enabled(ui, \"Rename Feature\", \"rename\", enabled); }"),
            [("Rename Feature".to_string(), false)]
        );
    }

    /// Another module's `button(…)` is still no text carrier, with or without a qualifier.
    #[test]
    fn a_foreign_button_is_still_not_a_text_carrier() {
        assert!(literals("fn live() { other::button(ui, \"Not a label\", icon); }").is_empty());
        assert!(literals("fn live() { button(ui, \"Not a label\", icon); }").is_empty());
    }

    /// `ui_kit::row_label(ui)` only reads the row's accessibility tag: it takes no literal, so the
    /// guard must not choke on it (the dialogs' own `row_label(ui, text)` is gone).
    #[test]
    fn reading_the_row_label_takes_no_literal() {
        assert_eq!(literals("fn live() { ui_kit::row_label(ui); row_label(ui); }").len(), 0);
    }

    #[test]
    fn a_comment_or_spacing_does_not_change_which_tr_a_file_means() {
        let id = [("Preferences".to_string(), true)];
        assert_eq!(literals("// fn tr(text: &str) -> &str\nuse crate::i18n::tr;\nfn live() { tr(\"Preferences\"); }"), id);
        assert_eq!(literals("use crate :: i18n :: tr;\nfn live() { tr(\"Preferences\"); }"), id);
        assert_eq!(literals("use crate::i18n::{format, tr};\nfn live() { tr(\"Preferences\"); }"), id);
        assert_eq!(literals("fn live() { let tr = crate::i18n::tr; tr(\"Preferences\"); }"), id);
    }
}
