//! The kit's type API: interface weights, the family and fallback names a host installs its faces
//! under, the script checks and the on-demand CJK font state. It holds no font data and loads
//! nothing: the host embeds and installs the faces. A host that installs none binds the weight
//! families with [`with_weight_families`] and the kit draws with egui's default faces.

use egui::{FontDefinitions, FontFamily, FontId, RichText};

/// Name of the medium (500) interface family, and of its face in a host's definitions.
pub const MEDIUM_FAMILY: &str = "ui-medium";

/// Name of the semibold (600) interface family, and of its face in a host's definitions.
pub const SEMIBOLD_FAMILY: &str = "ui-semibold";

/// Name of the bold (700) interface family, and of its face in a host's definitions.
pub const BOLD_FAMILY: &str = "ui-bold";

fn medium_family() -> FontFamily {
    FontFamily::Name(MEDIUM_FAMILY.into())
}

fn semibold_family() -> FontFamily {
    FontFamily::Name(SEMIBOLD_FAMILY.into())
}

fn bold_family() -> FontFamily {
    FontFamily::Name(BOLD_FAMILY.into())
}

/// The interface families, regular first: Proportional, then medium, semibold and bold.
pub fn sans_families() -> [FontFamily; 4] {
    [FontFamily::Proportional, medium_family(), semibold_family(), bold_family()]
}

/// `fonts` with each weight family it lacks bound to its Proportional list, so the kit's weights
/// draw with whatever regular faces the definitions have. A family already bound is kept.
pub fn with_weight_families(mut fonts: FontDefinitions) -> FontDefinitions {
    let regular = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for family in [medium_family(), semibold_family(), bold_family()] {
        fonts.families.entry(family).or_insert_with(|| regular.clone());
    }
    fonts
}

/// Medium (500) interface text, retaining the language fallbacks and user override.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, medium_family())
}

/// Semibold (600) interface text, retaining the language fallbacks and user override.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, semibold_family())
}

/// Bold (700) interface text, retaining the language fallbacks and user override.
pub fn bold(size: f32) -> FontId {
    FontId::new(size, bold_family())
}

/// Select a real interface weight without changing the text's size or colour.
pub trait RichTextExt {
    fn medium(self) -> Self;
    fn semibold(self) -> Self;
    fn bold_face(self) -> Self;
}

impl RichTextExt for RichText {
    fn medium(self) -> Self {
        self.family(medium_family())
    }
    fn semibold(self) -> Self {
        self.family(semibold_family())
    }
    fn bold_face(self) -> Self {
        self.family(bold_family())
    }
}

/// Name of the CJK fallback entry in [`egui::FontDefinitions`].
pub const CJK_FALLBACK: &str = "cjk_fallback";

/// Name of the Chinese fallback entry in [`egui::FontDefinitions`], after [`CJK_FALLBACK`].
/// A Japanese-only CJK font (Meiryo, Hiragino Gothic) lacks simplified characters, so
/// Chinese falls through to this one per glyph.
pub const CHINESE_FALLBACK: &str = "chinese_fallback";

/// Name of the chosen interface font in [`egui::FontDefinitions`], first in every sans
/// family when one is set (#1290).
pub const UI_FONT: &str = "ui_font";

/// Name of the Hangul fallback entry in [`egui::FontDefinitions`], after the CJK ones.
pub const HANGUL_FALLBACK: &str = "hangul_fallback";

/// Name of the Thai fallback entry in [`egui::FontDefinitions`], after the CJK ones (#1902).
pub const THAI_FALLBACK: &str = "thai_fallback";

/// Name of the Hebrew fallback entry in [`egui::FontDefinitions`], last (#3457).
pub const HEBREW_FALLBACK: &str = "hebrew_fallback";

/// Name of the Arabic fallback entry in [`egui::FontDefinitions`]: first of the fallbacks for an
/// Arabic UI, last otherwise. It stays after the Latin interface font, which keeps drawing spaces,
/// digits and Latin text, so each Arabic word is one shaped run (see [`solidcraft_ui_text::bidi`]).
pub const ARABIC_FALLBACK: &str = "arabic_fallback";

/// egui's built-in monospace face, also the last fallback of every sans family (#2955).
pub const SYMBOL_FALLBACK: &str = "Hack";

/// UI languages whose text needs a CJK script font on the web.
pub const CJK_LANGUAGES: &[&str] = &["ja", "zh", "zh-TW", "ko"];

/// Whether the UI language needs a CJK font (the web app fetches one only then).
pub fn needs_cjk_font(language: &str) -> bool {
    CJK_LANGUAGES.contains(&language)
}

/// Whether `text` has characters only the CJK fallback draws: kana, CJK ideographs and their
/// punctuation, and full-width forms. Hangul is left out: the web app's font has none.
pub fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(u32::from(c),
            0x3000..=0x30FF // CJK punctuation, Hiragana, Katakana
            | 0x31F0..=0x31FF // Katakana phonetic extensions
            | 0x3400..=0x4DBF // CJK Extension A
            | 0x4E00..=0x9FFF // CJK Unified Ideographs
            | 0xF900..=0xFAFF // CJK Compatibility Ideographs
            | 0xFF00..=0xFFEF // Half-width and full-width forms
        )
    })
}

/// Whether any text painted so far this frame has CJK characters (see [`has_cjk`]). The web app
/// calls it after drawing the UI, until its CJK font is loaded, so a Japanese design name, file
/// name or typed text fetches the font whatever the UI language (#720).
pub fn painted_cjk(ctx: &egui::Context) -> bool {
    let mut layers: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().collect());
    layers.push(egui::LayerId::background());
    ctx.graphics(|g| layers.iter().filter_map(|l| g.get(*l)).any(|list| list.all_entries().any(|c| shape_has_cjk(&c.shape))))
}

fn shape_has_cjk(shape: &egui::Shape) -> bool {
    match shape {
        egui::Shape::Text(t) => has_cjk(&t.galley.job.text),
        egui::Shape::Vec(shapes) => shapes.iter().any(shape_has_cjk),
        _ => false,
    }
}

/// Session state of the web app's on-demand script fonts. Pure state so it can be tested off the
/// web: the shell does the fetch when [`CjkFontLoader::tick`] asks for it.
#[derive(Debug, Default)]
pub struct CjkFontLoader {
    status: CjkFontStatus,
    /// The saved language while this session falls back to English after a failed load.
    saved_language: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CjkFontStatus {
    #[default]
    Unloaded,
    Loading,
    Installed,
    Failed,
}

impl CjkFontLoader {
    pub fn status(&self) -> CjkFontStatus {
        self.status
    }

    /// Called every frame with the live language (a `system` preference resolves to its catalog
    /// via [`solidcraft_ui_text::effective_language`]). `ready` is true when the shell already has the
    /// on-demand face that language needs. Returns true when a fetch should start now. After a
    /// failed load a CJK language falls back to English for this session; the saved preference is
    /// kept (see [`Self::persisted_language`]). Switching to another CJK language that still needs
    /// a face starts another fetch (#8894).
    pub fn tick(&mut self, language: &mut String, ready: bool) -> bool {
        if self.saved_language.is_some() && language != "en" {
            // The user picked a language themselves since the fallback: that is the preference now.
            self.saved_language = None;
            if self.status == CjkFontStatus::Failed {
                self.status = CjkFontStatus::Unloaded;
            }
        }
        if !needs_cjk_font(solidcraft_ui_text::effective_language(language)) {
            return false;
        }
        if ready {
            self.status = CjkFontStatus::Installed;
            return false;
        }
        match self.status {
            CjkFontStatus::Unloaded | CjkFontStatus::Installed => {
                self.status = CjkFontStatus::Loading;
                true
            }
            CjkFontStatus::Failed => {
                self.fall_back(language);
                false
            }
            CjkFontStatus::Loading => false,
        }
    }

    /// CJK text is on screen (whatever the UI language). Returns true when the Japanese font fetch
    /// should start now: once, and never again after a failed load (the UI language stays as it is).
    pub fn text_needs_font(&mut self) -> bool {
        if self.status == CjkFontStatus::Unloaded {
            self.status = CjkFontStatus::Loading;
            true
        } else {
            false
        }
    }

    /// Whether the session still watches painted text for CJK characters.
    pub fn watching_text(&self) -> bool {
        self.status == CjkFontStatus::Unloaded
    }

    /// Record the outcome of the fetch and install. A `system` preference resolves to its
    /// catalog first ([`Self::tick`]).
    pub fn finished(&mut self, installed: bool, language: &mut String) {
        if installed {
            self.status = CjkFontStatus::Installed;
        } else {
            self.status = CjkFontStatus::Failed;
            if needs_cjk_font(solidcraft_ui_text::effective_language(language)) {
                self.fall_back(language);
            }
        }
    }

    fn fall_back(&mut self, language: &mut String) {
        self.saved_language = Some(std::mem::replace(language, "en".to_owned()));
    }

    /// The language to write to storage: the user's own choice, not this session's fallback.
    pub fn persisted_language<'a>(&'a self, live: &'a str) -> &'a str {
        self.saved_language.as_deref().unwrap_or(live)
    }
}

/// The fallback entries in the order they join each family for a UI language.
///
/// Chinese goes first for `zh`: a Japanese font (Meiryo, Yu Gothic) lacks the simplified
/// characters, so the rest of the word falls through to the Chinese font per glyph — and
/// epaint centres each face's own line height in the row, so glyphs from two faces of very
/// different height (Meiryo ≈ 2.0 em, Microsoft YaHei ≈ 1.4 em) sit at different heights
/// inside one word. One face for the whole word keeps them on one baseline.
/// Thai, Hebrew and Arabic come last whatever the language: no other face draws them. Arabic
/// goes first for `ar` instead: a CJK font such as Arial Unicode also has Arabic letters, without
/// the shaping data that joins them.
pub fn fallback_order(language: &str) -> [&'static str; 6] {
    match language {
        "zh" | "zh-TW" => [CHINESE_FALLBACK, CJK_FALLBACK, HANGUL_FALLBACK, THAI_FALLBACK, HEBREW_FALLBACK, ARABIC_FALLBACK],
        "ko" => [HANGUL_FALLBACK, CJK_FALLBACK, CHINESE_FALLBACK, THAI_FALLBACK, HEBREW_FALLBACK, ARABIC_FALLBACK],
        "ar" => [ARABIC_FALLBACK, CJK_FALLBACK, CHINESE_FALLBACK, HANGUL_FALLBACK, THAI_FALLBACK, HEBREW_FALLBACK],
        _ => [CJK_FALLBACK, CHINESE_FALLBACK, HANGUL_FALLBACK, THAI_FALLBACK, HEBREW_FALLBACK, ARABIC_FALLBACK],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kit sets every weight on a bare context with no host faces: binding the weight
    /// families to egui's own faces is enough, and the weights keep size and colour.
    #[test]
    fn the_kit_sets_type_with_egui_faces_when_no_host_fonts_are_installed() {
        let ctx = egui::Context::default();
        ctx.set_fonts(with_weight_families(egui::FontDefinitions::default()));
        let ink = egui::Color32::from_rgb(10, 20, 30);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for (text, expected) in [
                (egui::RichText::new("Medium").color(ink).size(16.0).medium(), medium(16.0)),
                (egui::RichText::new("Semibold").color(ink).size(16.0).semibold(), semibold(16.0)),
                (egui::RichText::new("Bold").color(ink).size(16.0).bold_face(), bold(16.0)),
            ] {
                ui.label(text.clone());
                let galley = egui::WidgetText::from(text).into_galley(ui, None, f32::INFINITY, egui::TextStyle::Body);
                assert_eq!(galley.job.sections[0].format.font_id, expected);
                assert_eq!(galley.job.sections[0].format.color, ink);
            }
        });
        output.textures_delta.clear();
        let defaults = egui::FontDefinitions::default();
        let installed = ctx.fonts(|f| f.definitions().families.clone());
        for family in sans_families() {
            assert_eq!(installed[&family], defaults.families[&egui::FontFamily::Proportional], "{family:?} draws with egui's faces");
        }
        let width = |font: egui::FontId| ctx.fonts_mut(|f| f.layout_no_wrap("Chamfer".into(), font, ink).size().x);
        assert_eq!(width(semibold(16.0)), width(egui::FontId::proportional(16.0)), "no host face: every weight is egui's regular face");
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&bold(14.0), "Chamfer")));
    }

    /// A family the host already bound keeps its own list.
    #[test]
    fn weight_families_keep_what_the_host_bound() {
        let mut fonts = egui::FontDefinitions::default();
        fonts.families.insert(semibold(1.0).family, vec!["host-semibold".to_owned()]);
        let bound = with_weight_families(fonts);
        assert_eq!(bound.families[&semibold(1.0).family], ["host-semibold"]);
        assert_eq!(bound.families[&medium(1.0).family], bound.families[&egui::FontFamily::Proportional]);
    }

    #[test]
    fn web_font_is_fetched_only_for_a_cjk_language() {
        let mut loader = CjkFontLoader::default();
        for lang in ["en", "de"] {
            let mut language = lang.to_owned();
            assert!(!loader.tick(&mut language, false), "no CJK font fetch for {lang}");
        }
        let mut language = "ja".to_owned();
        assert!(loader.tick(&mut language, false), "Japanese starts the fetch");
        assert!(!loader.tick(&mut language, false), "the fetch starts once");
        loader.finished(true, &mut language);
        assert_eq!(loader.status(), CjkFontStatus::Installed);
        assert_eq!(language, "ja");
        assert!(!loader.tick(&mut language, true), "ready face needs no fetch");
    }

    #[test]
    fn chinese_ui_fetches_again_after_japanese_is_installed() {
        let mut loader = CjkFontLoader::default();
        let mut language = "ja".to_owned();
        assert!(loader.tick(&mut language, false));
        loader.finished(true, &mut language);
        language = "zh".to_owned();
        assert!(loader.tick(&mut language, false), "Chinese needs its own face (#8894)");
        loader.finished(true, &mut language);
        assert!(!loader.tick(&mut language, true));
    }

    #[test]
    fn cjk_text_is_recognised() {
        for cjk in ["テスト", "ひらがな", "部品", "ﾃｽﾄ", "Bracket（改）", "Plate 板"] {
            assert!(has_cjk(cjk), "{cjk} has CJK characters");
        }
        for other in ["", "Bracket", "Ø12 × 3 mm", "Größe", "Привет", "한국어", "émoji ✓"] {
            assert!(!has_cjk(other), "{other} has no CJK characters the fallback draws");
        }
    }

    /// An English UI fetches the font once CJK text is painted (#720), and a failed fetch leaves
    /// the language alone.
    #[test]
    fn cjk_text_fetches_the_font_with_an_english_ui() {
        let mut loader = CjkFontLoader::default();
        let mut language = "en".to_owned();
        assert!(!loader.tick(&mut language, false));
        assert!(loader.watching_text());
        assert!(loader.text_needs_font(), "CJK text starts the fetch");
        assert!(!loader.text_needs_font(), "the fetch starts once");
        assert!(!loader.tick(&mut language, false), "no second fetch from the language");
        assert!(!loader.watching_text());
        loader.finished(false, &mut language);
        assert_eq!(loader.status(), CjkFontStatus::Failed);
        assert_eq!(language, "en");
        assert!(!loader.text_needs_font(), "no retry loop after a failure");

        let mut loader = CjkFontLoader::default();
        assert!(loader.text_needs_font());
        loader.finished(true, &mut language);
        assert_eq!(loader.status(), CjkFontStatus::Installed);
        // Switching to Japanese afterwards needs no second fetch when the face is ready.
        let mut ja = "ja".to_owned();
        assert!(!loader.tick(&mut ja, true));
    }

    /// The painted-text check finds CJK in the frame's labels and nothing else.
    #[test]
    fn painted_cjk_sees_labels_in_the_frame() {
        let ctx = egui::Context::default();
        let run = |text: &'static str| {
            let mut seen = false;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.label(text);
                seen = painted_cjk(ui.ctx());
            });
            out.textures_delta.clear();
            seen
        };
        assert!(!run("Untitled"));
        assert!(run("テスト"));
    }

    #[test]
    fn failed_font_fetch_falls_back_for_the_session_without_changing_the_saved_language() {
        let mut loader = CjkFontLoader::default();
        let mut language = "ja".to_owned();
        assert!(loader.tick(&mut language, false));
        loader.finished(false, &mut language);
        assert_eq!(language, "en", "this session shows English instead of tofu");
        assert_eq!(loader.persisted_language(&language), "ja", "the saved preference stays Japanese");
        assert!(!loader.tick(&mut language, false));
        assert_eq!(loader.persisted_language(&language), "ja");

        // Choosing a language afterwards makes that the saved preference.
        language = "de".to_owned();
        assert!(!loader.tick(&mut language, false));
        assert_eq!(loader.persisted_language(&language), "de");
    }
}
