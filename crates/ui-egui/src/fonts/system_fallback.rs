//! A character none of SolidCraft's faces has (bundled, chosen, or found for a known script in
//! [`super`]) is drawn with an installed font that has it, not as an empty box (#20005).
//!
//! The installed fonts are listed once on a background thread ([`fontdb`]); egui asks
//! [`SystemFallback::font_for`] at most once per family and character, and the font it gets is
//! appended to that family's fallback chain, so the rest of the script follows from it.

use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use egui::{FallbackRequest, FontData, FontFamily, FontInsert, FontPriority, InsertFontFamily};

/// The installed fonts: being listed, listed, or (without a thread) none.
enum Fonts {
    Listing(JoinHandle<fontdb::Database>),
    Listed(fontdb::Database),
}

/// An [`egui::FontProvider`] that finds a character in the installed fonts.
pub(crate) struct SystemFallback {
    fonts: Mutex<Option<Fonts>>,
}

impl SystemFallback {
    /// Starts listing the installed fonts on a thread of its own.
    pub(crate) fn new() -> Self {
        let fonts = std::thread::Builder::new()
            .name("font-fallback".into())
            .spawn(|| {
                let mut db = fontdb::Database::new();
                db.load_system_fonts();
                db
            })
            .ok()
            .map(Fonts::Listing);
        Self { fonts: Mutex::new(fonts) }
    }

    /// A provider over `db` (tests).
    #[cfg(test)]
    fn over(db: fontdb::Database) -> Self {
        Self { fonts: Mutex::new(Some(Fonts::Listed(db))) }
    }
}

impl egui::FontProvider for SystemFallback {
    fn font_for(&self, request: &FallbackRequest<'_>) -> Option<FontInsert> {
        let c = request.cluster.chars().next()?;
        let mut fonts = self.fonts.lock().unwrap_or_else(PoisonError::into_inner);
        // Waits only when the first missing character comes before the listing is done.
        if let Some(Fonts::Listing(_)) = &*fonts
            && let Some(Fonts::Listing(thread)) = fonts.take()
        {
            *fonts = thread.join().ok().map(Fonts::Listed);
        }
        let Some(Fonts::Listed(db)) = &*fonts else { return None };
        let monospace = matches!(request.family, FontFamily::Monospace);
        let face = best_face(db, c, monospace)?;
        let bytes = db.with_face_data(face.id, |data, _| data.to_vec())?;
        let name = format!("system:{}:{}", face.post_script_name, face.index);
        log::debug!("{name} draws {c:?}");
        let mut data = FontData::from_owned(bytes);
        data.index = face.index;
        Some(FontInsert::new(&name, data, vec![InsertFontFamily { family: request.family.clone(), priority: FontPriority::Lowest }]))
    }
}

/// The installed face that draws `c` best: one of the family's kind (monospaced or not), upright,
/// normal width, nearest the regular weight, a Noto face before others, then by name so the
/// choice does not depend on the order the folders were read in.
fn best_face(db: &fontdb::Database, c: char, monospace: bool) -> Option<&fontdb::FaceInfo> {
    db.faces().filter(|f| draws(db, f.id, c)).min_by_key(|f| {
        let name = f.families.first().map(|(n, _)| n.as_str()).unwrap_or_default();
        (
            f.monospaced != monospace,
            f.style != fontdb::Style::Normal,
            f.stretch != fontdb::Stretch::Normal,
            f.weight.0.abs_diff(fontdb::Weight::NORMAL.0),
            !name.starts_with("Noto"),
            name.to_owned(),
            f.index,
        )
    })
}

/// Whether face `id` has a glyph for `c` that egui can draw: an outline, a colour glyph or a
/// bitmap.
fn draws(db: &fontdb::Database, id: fontdb::ID, c: char) -> bool {
    db.with_face_data(id, |data, index| {
        let Ok(face) = ttf_parser::Face::parse(data, index) else { return false };
        let t = face.tables();
        let drawable = t.glyf.is_some() || t.cff.is_some() || t.cff2.is_some() || t.colr.is_some() || t.sbix.is_some() || t.cbdt.is_some();
        drawable && face.glyph_index(c).is_some_and(|g| g.0 != 0)
    })
    .unwrap_or(false)
}

/// Asks the installed fonts for any character the context's fonts lack. Once per context.
pub(crate) fn install(ctx: &egui::Context) {
    let id = egui::Id::unique("solidcraft_system_font_fallback");
    if ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id, true));
    ctx.add_font_provider(Arc::new(SystemFallback::new()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::FontProvider as _;

    /// Latin capital letter schwa: in Ubuntu Light, not in Hack.
    const SCHWA: char = 'Ə';

    /// egui's two default faces, Hack (monospaced, regular) and Ubuntu Light (proportional, light).
    fn defaults() -> fontdb::Database {
        let mut db = fontdb::Database::new();
        let defs = egui::FontDefinitions::default();
        for name in ["Hack", "Ubuntu-Light"] {
            if let Some(font) = defs.font_data.get(name) {
                db.load_font_data(font.bytes().to_vec());
            }
        }
        db
    }

    fn family_of(db: &fontdb::Database, c: char, monospace: bool) -> Option<String> {
        best_face(db, c, monospace).and_then(|f| f.families.first()).map(|(n, _)| n.clone())
    }

    #[test]
    fn a_family_takes_a_face_of_its_kind_and_only_one_that_has_the_character() {
        let db = defaults();
        assert_eq!(db.len(), 2);
        assert_eq!(family_of(&db, 'a', true).as_deref(), Some("Hack"));
        assert_eq!(family_of(&db, 'a', false).as_deref(), Some("Ubuntu"));
        // Only Ubuntu has it, so even a monospaced family takes it.
        assert_eq!(family_of(&db, SCHWA, true).as_deref(), Some("Ubuntu"));
        assert_eq!(family_of(&db, '한', false), None);
    }

    #[test]
    fn a_context_without_the_character_draws_it_with_the_found_face() {
        let mut only_hack = egui::FontDefinitions::empty();
        let defs = egui::FontDefinitions::default();
        let Some(hack) = defs.font_data.get("Hack") else { panic!("egui's default fonts") };
        only_hack.font_data.insert("Hack".into(), hack.clone());
        only_hack.families.insert(FontFamily::Proportional, vec!["Hack".into()]);
        only_hack.families.insert(FontFamily::Monospace, vec!["Hack".into()]);
        let ctx = egui::Context::default();
        ctx.set_fonts(only_hack);
        ctx.add_font_provider(Arc::new(SystemFallback::over(defaults())));
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        assert!(ctx.fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(14.0), SCHWA)));
        assert!(!ctx.fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(14.0), '한')));
    }

    #[test]
    fn a_request_without_fonts_finds_none() {
        let p = SystemFallback { fonts: Mutex::new(None) };
        assert!(p.font_for(&FallbackRequest { cluster: "한", family: &FontFamily::Proportional }).is_none());
        assert!(SystemFallback::over(defaults()).font_for(&FallbackRequest { cluster: "", family: &FontFamily::Proportional }).is_none());
    }
}
