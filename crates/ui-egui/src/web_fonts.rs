//! The web app's CJK faces, fetched one language at a time.
//!
//! The browser build ships only the Latin faces. A CJK interface language needs a font of its own,
//! so the shell fetches exactly the face of the language the user picks and nothing else:
//! Japanese (`ja`) BIZ UDPGothic, Simplified Chinese (`zh`) Noto Sans SC, Traditional Chinese
//! (`zh-TW`) Noto Sans TC and Korean (`ko`) Noto Sans KR. Each face goes into the fallback slot its
//! script uses (see [`WebFace::slot`] and `fonts::fallback_order`).
//!
//! Everything here is plain data, so the whole life of a font download can be tested off the web:
//! [`CjkFontLoader`] decides what to fetch and when, [`WebFaces`] holds what arrived, and
//! [`definitions`] builds the egui font definitions for a language from them. The shell
//! (`apps/solidcraft-web`) only does the fetching.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::fonts::{CHINESE_FALLBACK, CJK_FALLBACK, HANGUL_FALLBACK};

/// A downloadable face, one per CJK interface language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WebFace {
    Japanese,
    SimplifiedChinese,
    TraditionalChinese,
    Korean,
}

impl WebFace {
    pub const ALL: [WebFace; 4] = [WebFace::Japanese, WebFace::SimplifiedChinese, WebFace::TraditionalChinese, WebFace::Korean];

    /// The face a UI language needs; `None` for the languages the bundled Latin faces draw.
    pub fn for_language(language: &str) -> Option<WebFace> {
        match language {
            "ja" => Some(WebFace::Japanese),
            "zh" => Some(WebFace::SimplifiedChinese),
            "zh-TW" => Some(WebFace::TraditionalChinese),
            "ko" => Some(WebFace::Korean),
            _ => None,
        }
    }

    /// Where the web build serves the font, relative to the page (`index.html` copies it there).
    pub fn path(self) -> &'static str {
        match self {
            WebFace::Japanese => "./fonts/BIZUDPGothic-Regular.ttf",
            WebFace::SimplifiedChinese => "./fonts/CraftUI-SC.otf",
            WebFace::TraditionalChinese => "./fonts/CraftUI-TC.otf",
            WebFace::Korean => "./fonts/noto-sans-kr/NotoSansKR-Regular.otf",
        }
    }

    /// The fallback entry the face is installed under. Both Chinese faces share one slot: the
    /// language picks which of them is in it.
    pub fn slot(self) -> &'static str {
        match self {
            WebFace::Japanese => CJK_FALLBACK,
            WebFace::SimplifiedChinese | WebFace::TraditionalChinese => CHINESE_FALLBACK,
            WebFace::Korean => HANGUL_FALLBACK,
        }
    }
}

/// Where one face's download stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontStatus {
    #[default]
    Unloaded,
    Loading,
    Installed,
    Failed,
}

/// Session state of the on-demand fonts: which faces were asked for and how that went. The shell
/// does the fetch when [`CjkFontLoader::tick`] or [`CjkFontLoader::text_needs_font`] names a face.
#[derive(Debug, Default)]
pub struct CjkFontLoader {
    status: BTreeMap<WebFace, FontStatus>,
    /// The saved language while this session falls back to English after a failed load.
    saved_language: Option<String>,
}

impl CjkFontLoader {
    pub fn status(&self, face: WebFace) -> FontStatus {
        self.status.get(&face).copied().unwrap_or_default()
    }

    fn set(&mut self, face: WebFace, status: FontStatus) {
        self.status.insert(face, status);
    }

    /// Called every frame with the live language. Returns the face to fetch now: only the face of
    /// a CJK language, only once per face. After a failed load the language falls back to English
    /// for this session; the saved preference is kept (see [`Self::persisted_language`]).
    pub fn tick(&mut self, language: &mut String) -> Option<WebFace> {
        if self.saved_language.is_some() && language != "en" {
            // The user picked a language themselves since the fallback: that is the preference now.
            self.saved_language = None;
        }
        let face = WebFace::for_language(language)?;
        match self.status(face) {
            FontStatus::Unloaded => {
                self.set(face, FontStatus::Loading);
                Some(face)
            }
            FontStatus::Failed => {
                self.fall_back(language);
                None
            }
            FontStatus::Loading | FontStatus::Installed => None,
        }
    }

    /// CJK text is on screen (whatever the UI language). Returns the face to fetch now: the
    /// Japanese one, once, and only while no face was asked for yet (the UI language stays as it is
    /// after a failure).
    pub fn text_needs_font(&mut self) -> Option<WebFace> {
        if !self.watching_text() {
            return None;
        }
        self.set(WebFace::Japanese, FontStatus::Loading);
        Some(WebFace::Japanese)
    }

    /// Whether the session still watches painted text for CJK characters: until some face is on
    /// its way, since any of them draws most CJK text.
    pub fn watching_text(&self) -> bool {
        WebFace::ALL.into_iter().all(|face| self.status(face) == FontStatus::Unloaded)
    }

    /// Record the outcome of fetching and installing `face`. A failure sends the UI to English
    /// only when it is the face the current language needs.
    pub fn finished(&mut self, face: WebFace, installed: bool, language: &mut String) {
        if installed {
            self.set(face, FontStatus::Installed);
        } else {
            self.set(face, FontStatus::Failed);
            if WebFace::for_language(language) == Some(face) {
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

/// The faces that arrived. Cloning shares the font bytes.
#[derive(Clone, Default)]
pub struct WebFaces {
    faces: BTreeMap<WebFace, Arc<egui::FontData>>,
}

impl WebFaces {
    /// Keep `bytes` as the font of `face`. `false` (nothing kept) when they are not a font.
    pub fn insert(&mut self, face: WebFace, bytes: Vec<u8>) -> bool {
        if ttf_parser::Face::parse(&bytes, 0).is_err() {
            return false;
        }
        self.faces.insert(face, Arc::new(egui::FontData::from_owned(bytes)));
        true
    }

    pub fn contains(&self, face: WebFace) -> bool {
        self.faces.contains_key(&face)
    }

    /// The font installed in fallback entry `slot` for `language`: the language's own face when it
    /// arrived, otherwise the first arrived face of that slot (a Japanese UI with only the
    /// Simplified Chinese face still draws Chinese text in a design).
    #[cfg(any(test, target_arch = "wasm32"))]
    fn slot_data(&self, slot: &str, language: &str) -> Option<Arc<egui::FontData>> {
        let wanted = WebFace::for_language(language);
        self.faces.iter().filter(|(face, _)| face.slot() == slot).min_by_key(|(face, _)| Some(**face) != wanted).map(|(_, data)| data.clone())
    }
}

fn faces_id() -> egui::Id {
    egui::Id::new("solidcraft_web_faces")
}

/// The faces kept so far in `ctx`.
pub fn stored_faces(ctx: &egui::Context) -> WebFaces {
    ctx.data(|d| d.get_temp::<WebFaces>(faces_id())).unwrap_or_default()
}

/// Keep a fetched face in `ctx` for [`crate::fonts::setup_fonts_for`] to install. `false` when
/// `bytes` are not a font.
pub fn add_face(ctx: &egui::Context, face: WebFace, bytes: Vec<u8>) -> bool {
    let mut faces = stored_faces(ctx);
    if !faces.insert(face, bytes) {
        return false;
    }
    ctx.data_mut(|d| d.insert_temp(faces_id(), faces));
    true
}

/// The interface fonts for `language` on the web: the bundled faces, the interface font a
/// preference names, then each arrived face in its slot in the language's fallback order. A CJK UI
/// leads with its own face so its digits and punctuation share the CJK baseline; any other UI keeps
/// the faces behind Latin (they draw CJK text in a design, and a CJK face sets Cyrillic too wide).
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn definitions(ui_font: &str, language: &str, faces: &WebFaces) -> egui::FontDefinitions {
    use crate::fonts;
    let mut defs = fonts::base_font_definitions();
    if let Some(font) = fonts::ui_font(ui_font) {
        defs = fonts::with_ui_font(defs, font);
    }
    let order = fonts::fallback_order(language);
    for name in order {
        if let Some(data) = faces.slot_data(name, language) {
            defs.font_data.insert(name.to_owned(), data);
            for family in fonts::all_families() {
                defs.families.entry(family).or_default().push(name.to_owned());
            }
        }
    }
    if fonts::needs_cjk_font(language)
        && let Some(primary) = order.into_iter().find(|name| *name != fonts::THAI_FALLBACK && defs.font_data.contains_key(*name))
    {
        fonts::promote_font_to_front(&mut defs, primary);
    }
    fonts::with_symbol_fallback(defs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{SYMBOL_FALLBACK, THAI_FALLBACK, UI_FONT};

    fn fetch(loader: &mut CjkFontLoader, language: &str) -> Option<WebFace> {
        loader.tick(&mut language.to_owned())
    }

    fn asset(path: &str) -> Vec<u8> {
        std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts").join(path)).unwrap()
    }

    fn face_file(face: WebFace) -> &'static str {
        match face {
            WebFace::Japanese => "biz-udp-gothic/BIZUDPGothic-Regular.ttf",
            WebFace::SimplifiedChinese => "craft-ui-sc/CraftUI-SC.otf",
            WebFace::TraditionalChinese => "craft-ui-tc/CraftUI-TC.otf",
            WebFace::Korean => "noto-sans-kr/NotoSansKR-Regular.otf",
        }
    }

    /// A face other than the real ones, for tests that only look at how fonts are ordered.
    fn latin_bytes() -> Vec<u8> {
        let defaults = crate::fonts::base_font_definitions();
        let name = &defaults.families[&egui::FontFamily::Proportional][0];
        defaults.font_data[name].font.to_vec()
    }

    fn proportional(fonts: &egui::FontDefinitions) -> Vec<String> {
        fonts.families[&egui::FontFamily::Proportional].clone()
    }

    #[test]
    fn each_cjk_language_names_its_own_face_and_others_none() {
        for (language, face) in
            [("ja", WebFace::Japanese), ("zh", WebFace::SimplifiedChinese), ("zh-TW", WebFace::TraditionalChinese), ("ko", WebFace::Korean)]
        {
            assert_eq!(WebFace::for_language(language), Some(face), "{language}");
        }
        for language in ["en", "en-GB", "de", "uk", "ru", "pt-BR", ""] {
            assert_eq!(WebFace::for_language(language), None, "{language} is drawn by the bundled faces");
        }
        // The shared list of CJK languages and the faces agree.
        for language in crate::fonts::CJK_LANGUAGES {
            assert!(WebFace::for_language(language).is_some(), "{language}");
        }
        for catalog in crate::i18n::LOCALES {
            assert_eq!(crate::fonts::needs_cjk_font(catalog.id), WebFace::for_language(catalog.id).is_some(), "{}", catalog.id);
        }
    }

    #[test]
    fn every_face_has_its_own_served_file() {
        let mut paths: Vec<_> = WebFace::ALL.into_iter().map(WebFace::path).collect();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), WebFace::ALL.len(), "no two faces share a file");
        let page = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/solidcraft-web/index.html")).unwrap();
        for face in WebFace::ALL {
            let source = face_file(face);
            assert!(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts").join(source).is_file(),
                "{source} is in assets/fonts"
            );
            let name = source.rsplit('/').next().unwrap();
            let link = page.lines().find(|line| line.contains("copy-file") && line.contains(source)).expect("index.html copies the face");
            let target = link.split("data-target-path=\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap();
            assert_eq!(format!("./{target}/{name}"), face.path(), "index.html serves {source} where the app fetches it");
        }
    }

    #[test]
    fn a_cjk_language_fetches_only_its_own_face_once() {
        for (language, face) in
            [("ja", WebFace::Japanese), ("zh", WebFace::SimplifiedChinese), ("zh-TW", WebFace::TraditionalChinese), ("ko", WebFace::Korean)]
        {
            let mut loader = CjkFontLoader::default();
            assert_eq!(fetch(&mut loader, language), Some(face), "{language} starts its fetch");
            assert_eq!(fetch(&mut loader, language), None, "{language} fetches once");
            for other in WebFace::ALL.into_iter().filter(|other| *other != face) {
                assert_eq!(loader.status(other), FontStatus::Unloaded, "{language} leaves {other:?} alone");
            }
            let mut live = language.to_owned();
            loader.finished(face, true, &mut live);
            assert_eq!(loader.status(face), FontStatus::Installed);
            assert_eq!(live, language);
        }
    }

    #[test]
    fn languages_without_a_cjk_face_fetch_nothing() {
        let mut loader = CjkFontLoader::default();
        for language in ["en", "de", "fr", "uk"] {
            assert_eq!(fetch(&mut loader, language), None, "{language}");
        }
        assert!(loader.watching_text(), "nothing is on its way");
        assert!(WebFace::ALL.into_iter().all(|face| loader.status(face) == FontStatus::Unloaded));
    }

    /// Walking through the languages one by one fetches each face once, and only the faces of the
    /// languages the user settled on.
    #[test]
    fn switching_languages_fetches_each_face_once() {
        let mut loader = CjkFontLoader::default();
        let mut language = "en".to_owned();
        assert_eq!(loader.tick(&mut language), None);
        language = "ko".to_owned();
        assert_eq!(loader.tick(&mut language), Some(WebFace::Korean));
        loader.finished(WebFace::Korean, true, &mut language);
        language = "zh-TW".to_owned();
        assert_eq!(loader.tick(&mut language), Some(WebFace::TraditionalChinese), "a new language fetches its own face");
        language = "ko".to_owned();
        assert_eq!(loader.tick(&mut language), None, "back to a face that arrived: no second fetch");
        language = "zh".to_owned();
        assert_eq!(loader.tick(&mut language), Some(WebFace::SimplifiedChinese), "the two Chinese languages need their own faces");
        assert_eq!(loader.status(WebFace::Japanese), FontStatus::Unloaded, "Japanese was never asked for");
    }

    #[test]
    fn a_failed_face_sends_only_its_own_language_to_english() {
        let mut loader = CjkFontLoader::default();
        let mut language = "ko".to_owned();
        assert_eq!(loader.tick(&mut language), Some(WebFace::Korean));
        loader.finished(WebFace::Korean, false, &mut language);
        assert_eq!(language, "en", "this session shows English instead of tofu");
        assert_eq!(loader.persisted_language(&language), "ko", "the saved preference stays Korean");
        assert_eq!(loader.tick(&mut language), None);
        assert_eq!(loader.persisted_language(&language), "ko");

        // Another language's face is still fetched, and choosing it makes it the preference.
        language = "ja".to_owned();
        assert_eq!(loader.tick(&mut language), Some(WebFace::Japanese));
        assert_eq!(loader.persisted_language(&language), "ja");
        loader.finished(WebFace::Japanese, true, &mut language);
        assert_eq!(language, "ja");

        // Asking for the failed one again falls back again; the failure is remembered.
        language = "ko".to_owned();
        assert_eq!(loader.tick(&mut language), None);
        assert_eq!(language, "en");
        assert_eq!(loader.persisted_language(&language), "ko");
    }

    #[test]
    fn a_failure_of_another_face_leaves_the_language_alone() {
        let mut loader = CjkFontLoader::default();
        assert_eq!(loader.text_needs_font(), Some(WebFace::Japanese));
        let mut language = "zh".to_owned();
        assert_eq!(loader.tick(&mut language), Some(WebFace::SimplifiedChinese), "the Chinese UI starts its own fetch");
        loader.finished(WebFace::Japanese, false, &mut language);
        assert_eq!(language, "zh", "the Japanese text face failing does not take the Chinese UI away");
        assert_eq!(loader.persisted_language(&language), "zh");
    }

    /// An English UI fetches the Japanese face once CJK text is painted (#720), and a failed fetch
    /// leaves the language alone.
    #[test]
    fn cjk_text_fetches_a_face_with_an_english_ui() {
        let mut loader = CjkFontLoader::default();
        let mut language = "en".to_owned();
        assert_eq!(loader.tick(&mut language), None);
        assert!(loader.watching_text());
        assert_eq!(loader.text_needs_font(), Some(WebFace::Japanese), "CJK text starts the fetch");
        assert_eq!(loader.text_needs_font(), None, "the fetch starts once");
        assert_eq!(loader.tick(&mut language), None, "no second fetch from the language");
        assert!(!loader.watching_text());
        loader.finished(WebFace::Japanese, false, &mut language);
        assert_eq!(loader.status(WebFace::Japanese), FontStatus::Failed);
        assert_eq!(language, "en");
        assert_eq!(loader.text_needs_font(), None, "no retry loop after a failure");

        let mut loader = CjkFontLoader::default();
        assert_eq!(loader.text_needs_font(), Some(WebFace::Japanese));
        loader.finished(WebFace::Japanese, true, &mut language);
        assert_eq!(loader.status(WebFace::Japanese), FontStatus::Installed);
        // Switching to Japanese afterwards needs no second fetch.
        let mut ja = "ja".to_owned();
        assert_eq!(loader.tick(&mut ja), None);
        // A language of another face still fetches that one.
        let mut ko = "ko".to_owned();
        assert_eq!(loader.tick(&mut ko), Some(WebFace::Korean));
    }

    #[test]
    fn bytes_that_are_not_a_font_are_not_kept() {
        let mut faces = WebFaces::default();
        assert!(!faces.insert(WebFace::Korean, b"not a font".to_vec()));
        assert!(!faces.contains(WebFace::Korean));
        let ctx = egui::Context::default();
        assert!(!add_face(&ctx, WebFace::Korean, Vec::new()));
        assert!(!stored_faces(&ctx).contains(WebFace::Korean));
        assert!(add_face(&ctx, WebFace::Korean, latin_bytes()));
        assert!(stored_faces(&ctx).contains(WebFace::Korean));
    }

    /// Each face is a fallback in the slot of its script, behind the Latin face, with the symbol
    /// fallback last.
    #[test]
    fn an_arrived_face_is_installed_in_its_slot() {
        for (language, face, slot) in [
            ("ja", WebFace::Japanese, CJK_FALLBACK),
            ("zh", WebFace::SimplifiedChinese, CHINESE_FALLBACK),
            ("zh-TW", WebFace::TraditionalChinese, CHINESE_FALLBACK),
            ("ko", WebFace::Korean, HANGUL_FALLBACK),
        ] {
            let mut faces = WebFaces::default();
            assert!(faces.insert(face, latin_bytes()));
            let fonts = definitions("", language, &faces);
            assert!(fonts.font_data.contains_key(slot), "{language}: {slot} is installed");
            for other in [CJK_FALLBACK, CHINESE_FALLBACK, HANGUL_FALLBACK].into_iter().filter(|other| *other != slot) {
                assert!(!fonts.font_data.contains_key(other), "{language}: nothing else is installed");
            }
            let list = proportional(&fonts);
            assert_eq!(list.first().map(String::as_str), Some(slot), "{language}: a CJK UI leads with its face: {list:?}");
            assert_eq!(list.last().map(String::as_str), Some(SYMBOL_FALLBACK), "{language}: symbols stay last: {list:?}");
            assert_eq!(list.iter().filter(|name| *name == slot).count(), 1, "{language}: once: {list:?}");
        }
    }

    #[test]
    fn a_non_cjk_ui_keeps_every_arrived_face_behind_the_latin_one() {
        let mut faces = WebFaces::default();
        for face in WebFace::ALL {
            assert!(faces.insert(face, latin_bytes()));
        }
        let latin = proportional(&crate::fonts::base_font_definitions())[0].clone();
        for language in ["en", "de", "uk"] {
            let list = proportional(&definitions("", language, &faces));
            assert_eq!(list.first(), Some(&latin), "{language} keeps the Latin face first: {list:?}");
            assert_eq!(list.last().map(String::as_str), Some(SYMBOL_FALLBACK), "{language}: {list:?}");
            let at = |name: &str| list.iter().position(|entry| entry == name).unwrap();
            assert!(at(CJK_FALLBACK) > 0 && at(CHINESE_FALLBACK) > 0 && at(HANGUL_FALLBACK) > 0, "{language}: {list:?}");
            assert!(at(CJK_FALLBACK) < at(CHINESE_FALLBACK) && at(CHINESE_FALLBACK) < at(HANGUL_FALLBACK), "{language}: Japanese first: {list:?}");
        }
    }

    #[test]
    fn the_chinese_slot_holds_the_face_of_the_chinese_language() {
        let mut faces = WebFaces::default();
        assert!(faces.insert(WebFace::SimplifiedChinese, latin_bytes()));
        let mut traditional = latin_bytes();
        traditional.push(0);
        assert!(faces.insert(WebFace::TraditionalChinese, traditional.clone()));
        let in_slot = |language: &str| definitions("", language, &faces).font_data[CHINESE_FALLBACK].font.to_vec();
        assert_eq!(in_slot("zh-TW"), traditional, "Traditional Chinese draws with its own face");
        assert_eq!(in_slot("zh"), latin_bytes(), "Simplified Chinese draws with its own face");
        assert_eq!(in_slot("ja"), latin_bytes(), "another UI takes the first arrived Chinese face");
        let mut only_traditional = WebFaces::default();
        assert!(only_traditional.insert(WebFace::TraditionalChinese, traditional.clone()));
        assert_eq!(definitions("", "zh", &only_traditional).font_data[CHINESE_FALLBACK].font.to_vec(), traditional, "no better face arrived");
    }

    #[test]
    fn the_chosen_interface_font_stays_ahead_of_the_cjk_face() {
        let mut faces = WebFaces::default();
        assert!(faces.insert(WebFace::Japanese, latin_bytes()));
        let chosen = solidcraft_engine::sketch::DEFAULT_FONT_FAMILY;
        let fonts = definitions(chosen, "ja", &faces);
        let list = &fonts.families[&egui::FontFamily::Proportional];
        assert_eq!(list[0], UI_FONT);
        assert_eq!(list[1], CJK_FALLBACK);
        // A face that arrives after a preference change, or a preference change after a face
        // arrived, ends in the same fonts.
        let ctx = egui::Context::default();
        assert!(add_face(&ctx, WebFace::Japanese, latin_bytes()));
        let again = definitions(chosen, "ja", &stored_faces(&ctx));
        assert_eq!(again.families[&egui::FontFamily::Proportional], fonts.families[&egui::FontFamily::Proportional]);
        let reset = definitions("", "ja", &stored_faces(&ctx));
        let list = proportional(&reset);
        assert_eq!(list.first().map(String::as_str), Some(CJK_FALLBACK));
        assert!(!list.iter().any(|name| name == UI_FONT));
        assert!(!proportional(&definitions("", "en", &stored_faces(&ctx))).iter().any(|name| name == THAI_FALLBACK));
    }

    /// The real files: every face draws its language's menu name and a sentence of interface text,
    /// through egui's font stack, as the language it is installed for.
    #[test]
    fn each_real_face_draws_its_language() {
        let japanese = "日本語テスト設定";
        let samples = [
            (WebFace::Japanese, "ja", japanese),
            // Characters the Japanese face lacks, drawn by the Chinese ones.
            (WebFace::SimplifiedChinese, "zh", "简体中文设置检查产说"),
            (WebFace::TraditionalChinese, "zh-TW", "繁體中文檢查閱內產說"),
            (WebFace::Korean, "ko", "한국어 설정 열기"),
        ];
        for (face, language, text) in samples {
            let ctx = egui::Context::default();
            assert!(add_face(&ctx, face, asset(face_file(face))), "{face:?} is a font");
            ctx.set_fonts(definitions("", language, &stored_faces(&ctx)));
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            for font in [crate::fonts::semibold(14.0), egui::FontId::proportional(14.0)] {
                assert!(ctx.fonts_mut(|f| f.has_glyphs(&font, text)), "{face:?} draws {text:?}");
            }
            // Laid out and painted, not only covered.
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.label(text);
            });
            out.textures_delta.clear();
        }
    }

    #[test]
    fn the_japanese_face_alone_leaves_chinese_and_korean_boxes() {
        let ctx = egui::Context::default();
        assert!(add_face(&ctx, WebFace::Japanese, asset(face_file(WebFace::Japanese))));
        ctx.set_fonts(definitions("", "ja", &stored_faces(&ctx)));
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        assert!(!ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(14.0), "한국어")), "this is why Korean needs its own face");
    }
}
