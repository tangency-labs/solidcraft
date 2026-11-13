//! The gallery draws every page without panicking, in every built-in theme's base, headless; its
//! pages name only components the kit has; the Theme panel's export is a theme file the kit reads.

use super::*;

fn context() -> egui::Context {
    install_tables();
    let ctx = egui::Context::default();
    solidcraft_look::fonts::install(&ctx);
    ctx
}

/// One frame of the gallery at 1280×720, with `events`.
fn frame(ctx: &egui::Context, gallery: &mut Gallery, events: Vec<egui::Event>) -> egui::FullOutput {
    let input =
        egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 720.0))), events, focused: true, ..Default::default() };
    ctx.run_ui(input, |ui| gallery.show(ui))
}

#[test]
fn every_page_draws_in_light_and_dark_with_the_theme_panel_open() {
    let ctx = context();
    let mut gallery = Gallery::new(&ctx);
    for dark in [false, true] {
        gallery.theme.choose_tone(dark);
        gallery.theme_open = dark;
        for (i, page) in PAGES.iter().enumerate() {
            gallery.page = i;
            // Twice: the first pass measures, the second lays out with what it measured.
            for _ in 0..2 {
                let output = frame(&ctx, &mut gallery, vec![egui::Event::PointerMoved(egui::pos2(640.0, 300.0))]);
                assert!(!output.shapes.is_empty(), "{}: drew nothing", page.id);
            }
        }
    }
}

#[test]
fn every_built_in_theme_draws_a_page() {
    let ctx = context();
    let mut gallery = Gallery::new(&ctx);
    assert!(gallery.theme.themes.len() > 2, "the look's built-in themes are listed besides Light and Dark");
    gallery.theme_open = true;
    for i in 0..gallery.theme.themes.len() {
        gallery.theme.choose(i);
        frame(&ctx, &mut gallery, Vec::new());
    }
}

#[test]
fn pages_are_unique_and_grouped() {
    let mut ids = std::collections::BTreeSet::new();
    for page in PAGES {
        assert!(ids.insert(page.id), "{} twice", page.id);
        assert!(!page.what.is_empty() && !page.usage.trim().is_empty(), "{} needs what it is for and how to use it", page.id);
        assert!(!page.dos.is_empty() && !page.donts.is_empty(), "{} needs do's and don'ts", page.id);
    }
    for group in Group::ALL {
        assert!(PAGES.iter().any(|p| p.group == group), "{} has no page", group.title());
    }
}

/// A page shows only what the kit has: every item it names is a public item in the kit's sources
/// (`module::name` in that module's file).
#[test]
fn pages_name_only_public_kit_items() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/ui-kit/src");
    let mut sources = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") && !path.to_string_lossy().ends_with("_tests.rs") {
                sources.push((path.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let declares = |source: &str, name: &str| {
        ["fn", "struct", "enum", "const", "trait", "type", "static"].iter().any(|kind| {
            source
                .match_indices(&format!("pub {kind} {name}"))
                .any(|(at, m)| source[at + m.len()..].chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_')))
        })
    };
    for page in PAGES {
        for item in page.items {
            let found = match item.split_once("::") {
                Some((module, name)) => sources.iter().any(|(file, source)| file == module && declares(source, name)),
                None => sources.iter().any(|(_, source)| declares(source, item)),
            };
            assert!(found, "{}: `{item}` is not a public item of solidcraft-ui-kit", page.id);
        }
    }
}

#[test]
fn an_edited_accent_reaches_the_examples_and_exports_as_a_theme_file() {
    let ctx = context();
    let mut gallery = Gallery::new(&ctx);
    let accent = egui::Color32::from_rgb(0xe0, 0x40, 0x90);
    gallery.theme.tokens.accent = accent;
    assert!(gallery.theme.edited());
    frame(&ctx, &mut gallery, Vec::new());
    solidcraft_theme::set_dark(gallery.theme.dark);
    {
        // As the gallery's frame does around every example.
        let _tokens = solidcraft_theme::scoped_tokens(gallery.theme.tokens);
        assert_eq!(Tokens::get().accent, accent, "the examples draw with the edited tokens");
    }
    assert_ne!(Tokens::get().accent, accent, "the override ends with the frame");
    let file = solidcraft_ui_kit::theme_files::parse(&gallery.theme.export()).expect("the export is a theme file");
    assert_eq!(solidcraft_ui_kit::theme_files::resolve(&file).accent, accent);
    gallery.theme.tokens = gallery.theme.shipped();
    assert!(!gallery.theme.edited(), "Reset gives back the theme as it ships");
}

#[test]
fn an_edited_colour_carries_the_tokens_that_share_it() {
    let ctx = context();
    let gallery = Gallery::new(&ctx);
    let shipped = gallery.theme.shipped();
    assert_eq!(shipped.form_primary, shipped.accent, "the dark theme's primary buttons are its accent");
    let accent = egui::Color32::from_rgb(0xe0, 0x40, 0x90);
    let mut edited = shipped;
    edited.accent = accent;
    edited.control_height += 4.0;
    let edited = gallery.theme.recolour(edited, shipped.accent, accent);
    assert_eq!((edited.accent, edited.form_primary), (accent, accent));
    assert_eq!(edited.control_height, shipped.control_height + 4.0, "sizes edited before stay");
    assert_eq!(edited.text, shipped.text, "other colours stay");
}
