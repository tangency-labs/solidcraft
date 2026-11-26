//! A character none of SolidCraft's faces has (bundled, chosen, or found for a known script in
//! [`super`]) is drawn with an installed font that has it, not as an empty box (#20005).
//!
//! The installed fonts are listed, and the characters each one draws read from it, once on a
//! background thread ([`fontdb`]). Only then is [`SystemFallback`] added to the context, so the
//! window never waits for it: adding a provider makes egui ask again for the characters it
//! could not draw, and repaint. egui asks [`SystemFallback::font_for`] at most once per family
//! and character, and the font it gets is appended to that family's fallback chain, so the rest
//! of the script follows from it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use egui::{FallbackRequest, FontData, FontFamily, FontInsert, FontPriority, InsertFontFamily};

/// An installed face egui can draw, and the characters it has, as sorted, disjoint ranges.
struct Face {
    id: fontdb::ID,
    chars: Box<[(u32, u32)]>,
}

impl Face {
    fn has(&self, c: char) -> bool {
        let c = u32::from(c);
        let i = self.chars.partition_point(|&(_, last)| last < c);
        self.chars.get(i).is_some_and(|&(first, _)| first <= c)
    }
}

/// An [`egui::FontProvider`] that finds a character in the installed fonts.
pub(crate) struct SystemFallback {
    db: fontdb::Database,
    faces: Vec<Face>,
    /// The face found for a character, for a monospaced family or not.
    found: Mutex<HashMap<(char, bool), Option<fontdb::ID>>>,
}

impl SystemFallback {
    /// Over the faces of `db`, reading the characters each one has. Slow: not on the UI thread.
    fn over(db: fontdb::Database) -> Self {
        let faces = db.faces().filter_map(|f| Some(Face { id: f.id, chars: chars_of(&db, f.id)? })).collect();
        Self { db, faces, found: Mutex::new(HashMap::new()) }
    }

    /// The installed face that draws `c` best: one of the family's kind (monospaced or not),
    /// upright, normal width, nearest the regular weight, a Noto face before others, then by name
    /// so the choice does not depend on the order the folders were read in.
    fn best_face(&self, c: char, monospace: bool) -> Option<&fontdb::FaceInfo> {
        let faces = self.faces.iter().filter(|f| f.has(c)).filter_map(|f| self.db.face(f.id));
        faces.min_by_key(|f| {
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
}

impl egui::FontProvider for SystemFallback {
    fn font_for(&self, request: &FallbackRequest<'_>) -> Option<FontInsert> {
        let c = request.cluster.chars().next()?;
        let monospace = matches!(request.family, FontFamily::Monospace);
        let id = *self
            .found
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry((c, monospace))
            .or_insert_with(|| self.best_face(c, monospace).map(|f| f.id));
        let face = self.db.face(id?)?;
        let bytes = self.db.with_face_data(face.id, |data, _| data.to_vec())?;
        let name = format!("system:{}:{}", face.post_script_name, face.index);
        log::debug!("{name} draws {c:?}");
        let mut data = FontData::from_owned(bytes);
        data.index = face.index;
        Some(FontInsert::new(&name, data, vec![InsertFontFamily { family: request.family.clone(), priority: FontPriority::Lowest }]))
    }
}

/// The characters face `id` has a glyph for, as sorted, disjoint ranges; `None` when it has none
/// or egui cannot draw it (no outline, colour glyph or bitmap).
fn chars_of(db: &fontdb::Database, id: fontdb::ID) -> Option<Box<[(u32, u32)]>> {
    let mut chars = db.with_face_data(id, |data, index| {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let t = face.tables();
        let drawable = t.glyf.is_some() || t.cff.is_some() || t.cff2.is_some() || t.colr.is_some() || t.sbix.is_some() || t.cbdt.is_some();
        if !drawable {
            return None;
        }
        let mut chars = Vec::new();
        for table in t.cmap?.subtables.into_iter().filter(|s| s.is_unicode()) {
            table.codepoints(|c| {
                if table.glyph_index(c).is_some_and(|g| g.0 != 0) {
                    chars.push(c);
                }
            });
        }
        Some(chars)
    })??;
    chars.sort_unstable();
    chars.dedup();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for c in chars {
        match ranges.last_mut() {
            Some((_, last)) if *last + 1 == c => *last = c,
            _ => ranges.push((c, c)),
        }
    }
    (!ranges.is_empty()).then(|| ranges.into_boxed_slice())
}

/// Asks the installed fonts for any character the context's fonts lack, once they are listed.
/// Once per context.
pub(crate) fn install(ctx: &egui::Context) {
    let id = egui::Id::unique("solidcraft_system_font_fallback");
    if ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id, true));
    let ctx = ctx.clone();
    let listing = std::thread::Builder::new().name("font-fallback".into()).spawn(move || {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        // Repaints, and egui asks again for the characters it missed.
        ctx.add_font_provider(Arc::new(SystemFallback::over(db)));
    });
    if let Err(error) = listing {
        log::warn!("no installed fonts for missing characters: {error}");
    }
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

    fn family_of(p: &SystemFallback, c: char, monospace: bool) -> Option<String> {
        p.best_face(c, monospace).and_then(|f| f.families.first()).map(|(n, _)| n.clone())
    }

    #[test]
    fn a_family_takes_a_face_of_its_kind_and_only_one_that_has_the_character() {
        let p = SystemFallback::over(defaults());
        assert_eq!(p.faces.len(), 2);
        assert_eq!(family_of(&p, 'a', true).as_deref(), Some("Hack"));
        assert_eq!(family_of(&p, 'a', false).as_deref(), Some("Ubuntu"));
        // Only Ubuntu has it, so even a monospaced family takes it.
        assert_eq!(family_of(&p, SCHWA, true).as_deref(), Some("Ubuntu"));
        assert_eq!(family_of(&p, '한', false), None);
    }

    #[test]
    fn a_face_has_the_characters_of_its_ranges_only() {
        let face = Face { id: fontdb::ID::dummy(), chars: Box::new([(0x41, 0x5a), (0x61, 0x7a), (0x259, 0x259)]) };
        for c in ['A', 'Z', 'a', 'm', 'z', 'ə'] {
            assert!(face.has(c), "{c:?}");
        }
        for c in ['\0', '@', '[', '`', '{', 'Ə', '한', char::MAX] {
            assert!(!face.has(c), "{c:?}");
        }
    }

    #[test]
    fn a_character_is_looked_up_once_per_kind_of_family() {
        let p = SystemFallback::over(defaults());
        for _ in 0..2 {
            assert!(p.font_for(&FallbackRequest { cluster: "Ə", family: &FontFamily::Monospace }).is_some());
            assert!(p.font_for(&FallbackRequest { cluster: "한", family: &FontFamily::Proportional }).is_none());
        }
        let found = p.found.lock().unwrap_or_else(PoisonError::into_inner);
        assert_eq!(found.len(), 2);
        assert!(found[&(SCHWA, true)].is_some());
        assert_eq!(found[&('한', false)], None);
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
        let p = SystemFallback::over(fontdb::Database::new());
        assert!(p.font_for(&FallbackRequest { cluster: "한", family: &FontFamily::Proportional }).is_none());
        assert!(SystemFallback::over(defaults()).font_for(&FallbackRequest { cluster: "", family: &FontFamily::Proportional }).is_none());
    }
}
