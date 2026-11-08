//! Translated interface text: message catalogs, lookup by stable id, label resolution, `{name}`
//! substitution, the language choice and the per-thread language. Front-end neutral: it holds no
//! app types and draws nothing.
//!
//! The host installs its catalogs once at startup with [`install`], before the first message is
//! read. Every lookup searches the installed list; English is the installed catalog with the id
//! `en` and is the fallback for a message a translation lacks. With nothing installed (or no `en`
//! catalog among them) every id reads as itself, the language is `en` and no other language is
//! supported.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub mod bidi;

/// A catalog's messages: stable id → text.
pub type Messages = BTreeMap<String, String>;

/// The id of the fallback catalog.
const ENGLISH_ID: &str = "en";

/// One language's messages, parsed from JSON (`{"id": "text", …}`) the first time they are read.
pub struct Catalog {
    pub id: &'static str,
    pub name: &'static str,
    /// The catalog as JSON.
    pub source: &'static str,
    messages: OnceLock<Messages>,
    /// The messages in display order, for a catalog with right-to-left text (see [`bidi`]).
    shown: OnceLock<Option<Messages>>,
    /// English text → the ids with that text, for resolving a source label (see [`Locale::label`]).
    label_ids: OnceLock<BTreeMap<&'static str, Vec<&'static str>>>,
}

impl Catalog {
    pub const fn new(id: &'static str, name: &'static str, source: &'static str) -> Self {
        Self { id, name, source, messages: OnceLock::new(), shown: OnceLock::new(), label_ids: OnceLock::new() }
    }

    /// The messages in reading order, as the catalog holds them. An invalid catalog is logged and
    /// reads as empty.
    pub fn messages(&'static self) -> &'static Messages {
        self.messages.get_or_init(|| match serde_json::from_str(self.source) {
            Ok(messages) => messages,
            Err(error) => {
                log::error!("invalid embedded {} message catalog: {error}", self.id);
                Messages::new()
            }
        })
    }

    /// The messages as egui must be given them: right-to-left lines in display order, since egui
    /// has no bidirectional layout (see [`bidi`]). The same map as [`Self::messages`] for every
    /// other catalog.
    fn shown(&'static self) -> &'static Messages {
        let shown = self.shown.get_or_init(|| {
            let messages = self.messages();
            messages
                .values()
                .any(|text| bidi::has_rtl(text))
                .then(|| messages.iter().map(|(id, text)| (id.clone(), bidi::visual_template(text).into_owned())).collect())
        });
        match shown {
            Some(shown) => shown,
            None => self.messages(),
        }
    }

    /// Each message text without a placeholder → the ids that have it, in id order.
    fn label_ids(&'static self) -> &'static BTreeMap<&'static str, Vec<&'static str>> {
        self.label_ids.get_or_init(|| {
            let mut ids: BTreeMap<_, Vec<_>> = BTreeMap::new();
            for (id, text) in self.messages().iter().filter(|(_, text)| !text.contains('{')) {
                ids.entry(text.as_str()).or_default().push(id.as_str());
            }
            ids
        })
    }

    /// The language's own name as the language list draws it.
    pub fn display_name(&self) -> Cow<'static, str> {
        bidi::visual(self.name)
    }
}

/// The installed catalogs, and the English one among them.
struct Installed {
    locales: &'static [&'static Catalog],
    english: &'static Catalog,
}

static INSTALLED: OnceLock<Installed> = OnceLock::new();

/// English when none is installed: no messages, so every id reads as itself.
static NO_MESSAGES: Catalog = Catalog::new(ENGLISH_ID, "English", "{}");

/// Install the host's catalogs, once per process, before the first message is read. English (the
/// catalog with the id `en`) is the fallback; list it first. Installing the same list again does
/// nothing; a different list is ignored and logged.
pub fn install(locales: &'static [&'static Catalog]) {
    let english = match locales.iter().copied().find(|catalog| catalog.id == ENGLISH_ID) {
        Some(english) => english,
        None => {
            log::warn!("the installed message catalogs have no `{ENGLISH_ID}` catalog: untranslated ids read as themselves");
            &NO_MESSAGES
        }
    };
    if let Err(ignored) = INSTALLED.set(Installed { locales, english })
        && let Some(installed) = INSTALLED.get()
        && !same_catalogs(installed.locales, ignored.locales)
    {
        log::warn!("message catalogs are already installed: a second, different list is ignored");
    }
}

fn same_catalogs(a: &[&Catalog], b: &[&Catalog]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| std::ptr::eq(*a, *b))
}

/// The installed catalogs, in the host's order (none before [`install`]).
pub fn locales() -> &'static [&'static Catalog] {
    INSTALLED.get().map_or(&[], |installed| installed.locales)
}

fn english() -> &'static Catalog {
    INSTALLED.get().map_or(&NO_MESSAGES, |installed| installed.english)
}

pub fn is_supported(id: &str) -> bool {
    locales().iter().any(|catalog| catalog.id == id)
}

/// The `language` preference value that follows the system (#10138). Not a catalog id: it
/// resolves through [`system_language`] wherever a catalog is needed.
pub const SYSTEM: &str = "system";

/// Match the system's preferred locale tags against the catalogs: the exact tag first
/// (`zh-TW`, `pt-BR`, `en-GB`), then the tag's base language (`zh-CN` → `zh`, `de-AT` → `de`),
/// then English. A Chinese tag goes by its script or region instead ([`chinese_catalog`]).
/// Tags come in the system's order of preference; the first one that matches wins.
pub fn match_system_tags(tags: &[String]) -> &'static str {
    for tag in tags {
        let tag = tag.replace('_', "-");
        if let Some(catalog) = chinese_catalog(&tag) {
            return catalog;
        }
        if let Some(catalog) = locales().iter().find(|catalog| catalog.id.eq_ignore_ascii_case(&tag)) {
            return catalog.id;
        }
        let base = tag.split('-').next().unwrap_or(&tag);
        if let Some(catalog) = locales().iter().find(|catalog| catalog.id.eq_ignore_ascii_case(base)) {
            return catalog.id;
        }
    }
    ENGLISH_ID
}

/// The catalog for a Chinese tag (#10306): by its script when it has one (`Hant` → `zh-TW`,
/// `Hans` → `zh`), else by its region (`TW`, `HK` and `MO` use Traditional characters, so
/// `zh-TW`; any other region, or none, `zh`). macOS lists its languages with a script
/// (`zh-Hant-TW`), which no catalog matches exactly, and the base `zh` is Simplified.
fn chinese_catalog(tag: &str) -> Option<&'static str> {
    let mut parts = tag.split('-');
    if !parts.next().is_some_and(|base| base.eq_ignore_ascii_case("zh")) {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    let traditional = match rest.iter().find(|part| part.len() == 4) {
        Some(script) => script.eq_ignore_ascii_case("Hant"),
        None => rest.iter().any(|part| ["TW", "HK", "MO"].iter().any(|region| part.eq_ignore_ascii_case(region))),
    };
    let id = if traditional { "zh-TW" } else { "zh" };
    locales().iter().find(|catalog| catalog.id == id).map(|catalog| catalog.id)
}

/// The catalog id the system asks for, read once per process after [`install`]: the OS locale on
/// desktop (Windows, macOS, Linux/BSD) and `navigator.languages` in the browser. English before
/// anything is installed.
pub fn system_language() -> &'static str {
    if INSTALLED.get().is_none() {
        return ENGLISH_ID;
    }
    SYSTEM_LANGUAGE.get_or_init(|| {
        // Tests read the same words on every PC, whatever its language.
        if cfg!(test) { ENGLISH_ID } else { match_system_tags(&sys_locale::get_locales().collect::<Vec<_>>()) }
    })
}

static SYSTEM_LANGUAGE: OnceLock<&'static str> = OnceLock::new();

/// `cfg!(test)` for the tests outside this crate, which link it without that: their harness
/// calls this first, so the system language reads as English on every PC.
pub fn pin_system_language() {
    let _ = SYSTEM_LANGUAGE.set(ENGLISH_ID);
}

/// The catalog id a `language` preference resolves to: the saved choice, or the system's for
/// [`SYSTEM`]. For callers that need the id as text (font fallbacks), not a [`Locale`].
pub fn effective_language(preference: &str) -> &'static str {
    Locale::for_language(preference).0.id
}

/// Immutable locale reference. Catalogs parse once; lookups are logarithmic.
#[derive(Clone, Copy)]
pub struct Locale(&'static Catalog);

impl Locale {
    /// A locale reading `catalog`, with English as its fallback.
    pub const fn new(catalog: &'static Catalog) -> Self {
        Self(catalog)
    }

    pub fn for_language(id: &str) -> Self {
        let id = if id == SYSTEM { system_language() } else { id };
        Self(locales().iter().copied().find(|catalog| catalog.id == id).unwrap_or_else(english))
    }

    /// The catalog this locale reads.
    pub fn catalog(self) -> &'static Catalog {
        self.0
    }

    pub fn name(self) -> &'static str {
        self.0.name
    }

    /// The language's own name as the language list draws it.
    pub fn display_name(self) -> Cow<'static, str> {
        self.0.display_name()
    }

    /// Stable message ID → translation → English → ID, in display order (see [`bidi`]).
    pub fn text(self, id: &str) -> &str {
        self.0.shown().get(id).or_else(|| english().messages().get(id)).map_or(id, String::as_str)
    }

    /// Translate source labels from engine/UI tables, preserving unknown labels.
    /// Also accepts stable IDs while table helpers migrate away from source text.
    pub fn label(self, label: &str) -> &str {
        let english = english();
        // Stable IDs are authoritative. An untranslated legacy ID identical to its
        // English caption can still use another translated ID for that source label.
        if english.messages().get(label).is_some_and(|text| text != label || self.0.messages().contains_key(label)) {
            return self.text(label);
        }
        if let Some(candidates) = english.label_ids().get(label) {
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
    // `None` is English.
    static CURRENT: Cell<Option<Locale>> = const { Cell::new(None) };
    /// Theme-authored language maps are already resolved and are not catalog source labels.
    static RESOLVED_LABELS: Cell<bool> = const { Cell::new(false) };
}

/// This thread's locale (English outside any [`scope`]).
pub fn locale() -> Locale {
    CURRENT.with(Cell::get).unwrap_or_else(|| Locale(english()))
}

#[must_use]
pub struct ResolvedLabels {
    previous: bool,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

/// Shared controls can render supplied translations verbatim while retaining catalog messages.
pub fn resolved_labels() -> ResolvedLabels {
    let previous = RESOLVED_LABELS.with(|labels| labels.replace(true));
    ResolvedLabels { previous, _thread: std::marker::PhantomData }
}

impl Drop for ResolvedLabels {
    fn drop(&mut self) {
        RESOLVED_LABELS.with(|labels| labels.set(self.previous));
    }
}

#[must_use]
pub struct LanguageScope {
    previous: Option<Locale>,
    // Drop on the thread where the scope was entered.
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

pub fn scope(id: &str) -> LanguageScope {
    let previous = CURRENT.with(|current| current.replace(Some(Locale::for_language(id))));
    LanguageScope { previous, _thread: std::marker::PhantomData }
}

impl Drop for LanguageScope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.set(self.previous));
    }
}

/// This thread's language, to carry into a worker thread with [`scope`].
pub fn language() -> &'static str {
    locale().0.id
}

pub fn tr(id: &str) -> &str {
    locale().text(id)
}

pub fn tr_label(label: &str) -> &str {
    if RESOLVED_LABELS.with(Cell::get) { label } else { locale().label(label) }
}

/// Every English catalog entry `(id, text)`, for Preferences search to match a translation
/// back to the words it was written in.
pub fn each_english_message(mut f: impl FnMut(&str, &str)) {
    for (id, text) in english().messages() {
        f(id, text);
    }
}

pub fn format(id: &str, args: &[(&str, &str)]) -> String {
    locale().format(id, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENGLISH: Catalog = Catalog::new(
        "en",
        "English",
        r#"{"ui.cancel":"Cancel","ui.open":"Open","Pitch":"Pitch","coupling.pitch":"Pitch","ui.profile":"Profile",
            "dialog.selected_count":"↖ {count} selected","ui.only_english":"Only in English"}"#,
    );
    static GERMAN: Catalog =
        Catalog::new("de", "Deutsch", r#"{"ui.cancel":"Abbrechen","ui.profile":"Profil","dialog.selected_count":"{count} ausgewählt"}"#);
    static JAPANESE: Catalog = Catalog::new("ja", "日本語", r#"{"document.save_changes":"{name} への変更を保存しますか？"}"#);
    static HEBREW: Catalog = Catalog::new("he", "עברית", r#"{"ui.cancel":"ביטול חלון"}"#);
    static CHINESE: Catalog = Catalog::new("zh", "中文", "{}");
    static CHINESE_TRADITIONAL: Catalog = Catalog::new("zh-TW", "繁體中文", "{}");
    static LOCALES: &[&Catalog] = &[&ENGLISH, &GERMAN, &JAPANESE, &HEBREW, &CHINESE, &CHINESE_TRADITIONAL];

    fn installed() {
        install(LOCALES);
    }

    #[test]
    fn installed_catalogs_are_looked_up_by_id() {
        installed();
        assert_eq!(locales().len(), LOCALES.len());
        assert!(is_supported("de"));
        assert!(!is_supported("fr"));
        assert!(!is_supported(SYSTEM));
        assert_eq!(Locale::for_language("de").name(), "Deutsch");
        assert_eq!(Locale::for_language("de").text("ui.cancel"), "Abbrechen");
        assert_eq!(Locale::for_language("ja").name(), "日本語");
        assert_eq!(effective_language("de"), "de");
        assert_eq!(effective_language(SYSTEM), "en", "tests take English as the system language");
        // Installing the same list again changes nothing.
        installed();
        assert_eq!(Locale::for_language("de").text("ui.cancel"), "Abbrechen");
    }

    #[test]
    fn a_missing_translation_falls_back_to_english_and_an_unknown_language_reads_english() {
        installed();
        assert_eq!(Locale::for_language("de").text("ui.open"), "Open");
        assert_eq!(Locale::for_language("fr").name(), "English");
        assert_eq!(Locale::for_language("fr").text("ui.cancel"), "Cancel");
    }

    #[test]
    fn a_missing_id_reads_as_itself() {
        installed();
        assert_eq!(Locale::for_language("de").text("ui.no_such_message"), "ui.no_such_message");
        assert_eq!(Locale::for_language("de").label("original unknown caption"), "original unknown caption");
        assert_eq!(tr("ui.no_such_message"), "ui.no_such_message");
    }

    #[test]
    fn templates_allow_reordering_and_literal_braces_in_values() {
        installed();
        assert_eq!(interpolate("{second} then {first}", &[("first", "{second}"), ("second", "B")]), "B then {second}");
        assert_eq!(interpolate("{missing} / {", &[]), "{missing} / {");
        assert_eq!(Locale::for_language("ja").format("document.save_changes", &[("name", "My {design}")]), "My {design} への変更を保存しますか？");
        assert_eq!(Locale::for_language("en").format("dialog.selected_count", &[("count", "2")]), "↖ 2 selected");
    }

    #[test]
    fn labels_resolve_by_english_text_or_id() {
        installed();
        let de = Locale::for_language("de");
        assert_eq!(de.label("Profile"), "Profil", "by its English text");
        assert_eq!(de.label("ui.profile"), "Profil", "by its id");
        static LEGACY: Catalog = Catalog::new("test-legacy", "Test", r#"{"Pitch":"legacy pitch"}"#);
        static ALIAS: Catalog = Catalog::new("test-alias", "Test", r#"{"coupling.pitch":"mechanical pitch"}"#);
        let legacy = Locale::new(&LEGACY);
        assert_eq!(legacy.label("Pitch"), "legacy pitch");
        assert_eq!(legacy.label("coupling.pitch"), "Pitch");
        let alias = Locale::new(&ALIAS);
        assert_eq!(alias.label("Pitch"), "mechanical pitch");
        assert_eq!(alias.label("coupling.pitch"), "mechanical pitch");
    }

    #[test]
    fn scopes_restore_the_previous_language_and_resolved_labels_are_verbatim() {
        installed();
        assert_eq!(language(), "en");
        {
            let _de = scope("de");
            assert_eq!(language(), "de");
            assert_eq!(tr("ui.cancel"), "Abbrechen");
            assert_eq!(tr_label("Profile"), "Profil");
            {
                let _labels = resolved_labels();
                assert_eq!(tr_label("Profile"), "Profile");
                assert_eq!(tr("ui.cancel"), "Abbrechen", "stable catalog messages remain translated");
            }
            assert_eq!(tr_label("Profile"), "Profil");
            {
                let _ja = scope("ja");
                assert_eq!(language(), "ja");
            }
            assert_eq!(language(), "de");
            assert_eq!(format("dialog.selected_count", &[("count", "3")]), "3 ausgewählt");
        }
        assert_eq!(language(), "en");
        assert_eq!(tr("ui.cancel"), "Cancel");
    }

    #[test]
    fn right_to_left_messages_come_back_in_display_order() {
        installed();
        assert_eq!(Locale::for_language("he").text("ui.cancel"), bidi::visual_template("ביטול חלון"));
        assert_ne!(Locale::for_language("he").text("ui.cancel"), "ביטול חלון");
    }

    #[test]
    fn system_tags_match_the_installed_catalogs() {
        installed();
        let tags = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(match_system_tags(&tags(&["de-AT"])), "de");
        assert_eq!(match_system_tags(&tags(&["zh-Hant-TW"])), "zh-TW");
        assert_eq!(match_system_tags(&tags(&["zh-CN"])), "zh");
        assert_eq!(match_system_tags(&tags(&["xx-YY", "ja"])), "ja");
        assert_eq!(match_system_tags(&tags(&["fr-FR"])), "en", "no catalog: English");
    }

    #[test]
    fn english_messages_are_listed() {
        installed();
        let mut ids = Vec::new();
        each_english_message(|id, _| ids.push(id.to_string()));
        assert!(ids.iter().any(|id| id == "ui.cancel"));
        assert_eq!(ids.len(), ENGLISH.messages().len());
    }
}
