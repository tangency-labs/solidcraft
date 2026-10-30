//! Text on paper (#4890 B1b): laid out by the rule sketch text measures with, in the drawing's
//! font, as runs the paper scene takes. Expected numbers come from the font's own tables
//! (Liberation Sans: 2048 units per em, a cap height of 1409) or from `TextMetrics`. Named families
//! are looked up in the repository's own fonts, never the computer's.

use std::path::PathBuf;

use sha2::{Digest, Sha256};
use solidcraft_geom::Vec2;
use solidcraft_geom::paper::text::{Font, TextRun};
use solidcraft_geom::paper::{Group, Item, ObjectKind, Scene};

use crate::fonts::font_data;
use crate::tests::v;
use crate::*;

const AT: Vec2 = Vec2 { x: 100.0, y: 50.0 };

fn set(text: &str, style: &TextStyle) -> (Vec<TextRun>, Font) {
    one(paper_text(text, None, AT, 5.0, 0.0, style).unwrap().unwrap())
}

/// The runs and their one font: the fonts come back once each, in order of id, each run's among them
/// and each holding exactly the glyphs its runs place.
fn one((runs, mut fonts): (Vec<TextRun>, Vec<Font>)) -> (Vec<TextRun>, Font) {
    assert!(fonts.windows(2).all(|w| w[0].id < w[1].id), "once each, in order of id");
    for font in &fonts {
        let placed: std::collections::BTreeSet<u16> = runs.iter().filter(|r| r.font == font.id).flat_map(|r| r.glyphs.iter().map(|g| g.id)).collect();
        assert!(font.glyphs.keys().copied().eq(placed), "{} holds exactly its runs' glyphs", font.postscript_name);
    }
    assert!(runs.iter().all(|r| fonts.iter().any(|f| f.id == r.font)), "each run's font is listed");
    assert_eq!(fonts.len(), 1, "one font a text so far");
    (runs, fonts.remove(0))
}

/// The runs on an A3 sheet as one group, with their font: what `Scene::validate` holds them to.
fn scene(runs: &[TextRun], font: &Font) -> Scene {
    let items = runs.iter().cloned().map(Item::Text).collect();
    let mut scene = Scene::new(v(420.0, 297.0), vec![Group { object: 1, kind: ObjectKind::TitleBlock, items }]);
    scene.add_fonts([font.clone()]);
    scene
}

fn width(line: &str) -> f64 {
    TextMetrics::new(&font_data(None).unwrap(), &TextStyle::default()).unwrap().line_width(line, 5.0).unwrap()
}

fn err(text: &str, family: Option<&str>, style: &TextStyle) -> String {
    paper_text_in(&repo_fonts(), text, family, AT, 5.0, 0.0, style).unwrap_err().to_string()
}

/// The repository's fonts as the only installed ones: Liberation Sans, IBM Plex Mono, BIZ
/// UDPGothic and IBM Plex Sans (a variable face, which paper refuses).
fn repo_fonts() -> Vec<PathBuf> {
    vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts")]
}

fn set_in(text: &str, family: &str) -> (Vec<TextRun>, Font) {
    one(paper_text_in(&repo_fonts(), text, Some(family), AT, 5.0, 0.0, &TextStyle::default()).unwrap().unwrap())
}

#[test]
fn a_line_is_one_run_as_long_as_sketch_text_measures_it() {
    let (runs, font) = set("Title 12", &TextStyle::default());
    let [run] = runs.as_slice() else { panic!("one run: {runs:?}") };
    assert_eq!((run.text.as_str(), run.font, run.origin, run.angle, run.slant, run.rtl), ("Title 12", font.id, AT, 0.0, 0.0, false));
    assert!((run.scale * 1409.0 - 5.0).abs() < 1e-12, "a cap height of 5 mm");
    assert_eq!(run.glyphs.len(), 8);
    let last = run.glyphs.last().unwrap();
    assert!((last.x + last.advance - width("Title 12")).abs() < 1e-12);
    // One character, one glyph, one cluster each.
    for (i, c) in run.clusters.iter().enumerate() {
        assert_eq!((c.glyphs.clone(), c.text.clone()), (i as u32..i as u32 + 1, i as u32..i as u32 + 1));
    }
    let ids: std::collections::BTreeSet<u16> = run.glyphs.iter().map(|g| g.id).collect();
    assert!(font.glyphs.keys().copied().eq(ids), "the font holds exactly the glyphs placed");
    scene(&runs, &font).validate().unwrap();
}

#[test]
fn the_drawing_font_is_admitted_with_what_a_pdf_needs() {
    let (_, font) = set("l", &TextStyle::default());
    let data = font_data(None).unwrap();
    assert_eq!((font.face, font.postscript_name.as_str()), (0, "LiberationSans"));
    assert_eq!((font.units_per_em, font.num_glyphs, font.metrics.cap_height), (2048, 2620, 1409));
    assert_eq!(font.metrics.bbox, [-1114, -621, 2666, 2007]);
    assert!(font.metrics.ascent > 1409 && font.metrics.descent < 0);
    assert_eq!((font.metrics.italic_angle, font.metrics.flags), (0.0, Default::default()));
    assert_eq!(&*font.bytes, data.bytes());
    let id: [u8; 32] = Sha256::new().chain_update(data.bytes()).chain_update(0u32.to_le_bytes()).finalize().into();
    assert_eq!(font.id, id);
    // Liberation Sans' `l` is one upright stem: its stem width is its bounding box's.
    let face = ttf_parser::Face::parse(data.bytes(), 0).unwrap();
    let l = face.glyph_index('l').unwrap();
    let b = face.glyph_bounding_box(l).unwrap();
    assert_eq!(i32::from(font.metrics.stem_v), i32::from(b.x_max) - i32::from(b.x_min));
    let glyph = &font.glyphs[&l.0];
    assert_eq!(glyph.advance, face.glyph_hor_advance(l).unwrap());
    assert!(!glyph.outline.is_empty() && glyph.outline.iter().all(|lp| lp.is_closed(1e-9)));
    // A second text is set in the same font, admitted once.
    assert_eq!(set("T", &TextStyle::default()).1.id, font.id);
}

#[test]
fn a_space_has_its_advance_and_no_outline() {
    let (runs, font) = set("A B", &TextStyle::default());
    let space = runs[0].glyphs[1].id;
    assert!(font.glyphs[&space].outline.is_empty() && font.glyphs[&space].advance > 0);
    let outlines: Vec<_> = runs[0].outlines(&font).collect();
    assert!(outlines.len() >= 2, "A and B are drawn");
    scene(&runs, &font).validate().unwrap();
}

#[test]
fn lines_break_a_pitch_apart_and_align_as_sketch_text_does() {
    let style = TextStyle { align: TextAlign::Center, ..TextStyle::default() };
    let angle = std::f64::consts::FRAC_PI_2;
    let (runs, font) = one(paper_text("AB\nC", None, AT, 5.0, angle, &style).unwrap().unwrap());
    let pitch = TextMetrics::new(&font_data(None).unwrap(), &style).unwrap().line_pitch(5.0).unwrap();
    // Turned a quarter: along the baseline is up the sheet, down a line is to the right.
    for (run, along, down) in [(&runs[0], -width("AB") / 2.0, 0.0), (&runs[1], -width("C") / 2.0, pitch)] {
        let want = AT + v(down, along);
        assert!(run.origin.dist(want) < 1e-9, "{:?} {want:?}", run.origin);
        assert_eq!(run.angle, angle);
    }
    scene(&runs, &font).validate().unwrap();
}

#[test]
fn a_right_to_left_line_is_a_run_per_direction_with_marks_in_their_letters_clusters() {
    let line = "Plan \u{5e9}\u{5c1}\u{5b8}\u{5dc}\u{5d5}\u{5b9}\u{5dd} 12";
    let (runs, font) = set(line, &TextStyle::default());
    assert!(runs.iter().any(|r| r.rtl) && runs.iter().any(|r| !r.rtl), "{runs:?}");
    assert_eq!(runs.iter().map(|r| r.text.len()).sum::<usize>(), line.len(), "every byte in one run");
    let hebrew = runs.iter().find(|r| r.text.contains('\u{5e9}')).unwrap();
    assert!(hebrew.rtl);
    // Four letters carry three points: fewer clusters than characters.
    assert!(hebrew.clusters.len() < hebrew.text.chars().count(), "{:?}", hebrew.clusters);
    scene(&runs, &font).validate().unwrap();
}

#[test]
fn a_character_the_font_lacks_is_refused_by_name() {
    let e = err("Size \u{4e2d}", None, &TextStyle::default());
    assert!(e.contains("no glyph for '\u{4e2d}'"), "{e}");
    let e = err("a\tb", None, &TextStyle::default());
    assert!(e.contains("'\\t'"), "{e}");
}

#[test]
fn a_family_is_found_by_name_among_the_installed_fonts() {
    let (runs, font) = set_in("Title 12", "ibm plex mono");
    let file = std::fs::read(repo_fonts()[0].join("ibm-plex-mono/IBMPlexMono-Regular.ttf")).unwrap();
    let id: [u8; 32] = Sha256::new().chain_update(&file).chain_update(0u32.to_le_bytes()).finalize().into();
    assert_eq!((font.postscript_name.as_str(), font.id), ("IBMPlexMono-Regular", id));
    assert!(*font.bytes == *file, "the file's own bytes");
    assert!(font.metrics.flags.fixed_pitch && !font.metrics.flags.serif);
    scene(&runs, &font).validate().unwrap();
    assert_eq!(paper_font_note(&repo_fonts(), "IBM Plex Mono"), None);
    // Japanese in a Japanese family; the built-in font has no glyph for it.
    let (runs, font) = set_in("図面 A3", "BIZ UDPGothic");
    assert_eq!(font.postscript_name, "BIZUDPGothic-Regular");
    scene(&runs, &font).validate().unwrap();
    // The same text in the same font is the same runs and font, every time.
    assert_eq!(set_in("図面 A3", "BIZ UDPGothic"), (runs, font));
}

#[test]
fn a_family_paper_cant_use_is_set_in_the_built_in_font_and_named() {
    let builtin = set("Title", &TextStyle::default());
    for (family, note) in [
        ("Futura", "Futura isn't on this computer: set in Liberation Sans"),
        ("IBM Plex Sans", "IBM Plex Sans can't be set on paper (it is a variable face): set in Liberation Sans"),
    ] {
        assert_eq!(set_in("Title", family), builtin, "{family}");
        assert_eq!(paper_font_note(&repo_fonts(), family).as_deref(), Some(note));
    }
    for family in ["", " ", "Liberation Sans", "liberation-sans"] {
        assert_eq!(paper_font_note(&repo_fonts(), family), None, "{family:?}");
    }
}

#[test]
fn a_font_file_or_a_name_on_several_lines_is_refused() {
    for family in ["C:/Fonts/arial.ttf", "fonts\\a", "Futura.otf", "~/f.ttc"] {
        let e = err("A", Some(family), &TextStyle::default());
        assert!(e.contains("drawings name fonts by family, so they travel"), "{family}: {e}");
    }
    assert!(check_family("Futura\nBold").is_err() && check_family(&"F".repeat(1025)).is_err());
    assert!(check_family("").is_ok() && check_family("Noto Sans CJK JP").is_ok());
}

#[test]
fn bold_takes_a_bold_face_and_italic_leans_without_one() {
    let bold = TextStyle { bold: true, ..TextStyle::default() };
    let e = err("A", None, &bold);
    assert!(e.contains("the font Liberation Sans has no bold face"), "{e}");
    let e = err("A", Some("IBM Plex Mono"), &bold);
    assert!(e.contains("the font IBM Plex Mono has no bold face"), "{e}");
    let (runs, _) = paper_text("A", Some(" liberation sans "), AT, 5.0, 0.0, &TextStyle { italic: true, ..TextStyle::default() }).unwrap().unwrap();
    assert_eq!(runs[0].slant, 12f64.to_radians().tan(), "a made-up italic leans as sketch text's");
    let e = err("A", None, &TextStyle { path: Some("c1".into()), ..TextStyle::default() });
    assert!(e.contains("follows no path"), "{e}");
}

#[test]
fn empty_lines_set_nothing() {
    for text in ["", "\n", "\n\n"] {
        assert!(paper_text(text, None, AT, 5.0, 0.0, &TextStyle::default()).unwrap().is_none(), "{text:?}");
    }
    let (runs, _) = set("A\n\nB", &TextStyle::default());
    assert_eq!(runs.len(), 2);
}

#[test]
fn hostile_numbers_and_sizes_are_refused() {
    let plain = TextStyle::default();
    for (at, height, angle) in [(AT, f64::NAN, 0.0), (AT, 0.0, 0.0), (AT, 1e7, 0.0), (v(f64::NAN, 0.0), 5.0, 0.0), (AT, 5.0, f64::INFINITY)] {
        assert!(paper_text("A", None, at, height, angle, &plain).is_err(), "{at:?} {height} {angle}");
    }
    assert!(matches!(paper_text(&"A".repeat(MAX_TEXT_CHARS + 1), None, AT, 5.0, 0.0, &plain), Err(SketchError::TooLarge)));
    assert!(paper_text("A", None, AT, 5.0, 0.0, &TextStyle { letter_spacing: f64::NAN, ..TextStyle::default() }).is_err());
}

/// Random lines of Latin, Hebrew with its points, digits, spaces and breaks, in random styles:
/// each is set into runs the scene accepts, or refused, never a panic.
#[test]
fn fuzz_whatever_is_set_is_a_scene() {
    let chars: Vec<char> = "Ab9 .-\n\u{5e9}\u{5dc}\u{5d5}\u{5dd}\u{5b8}\u{5c1}\u{5bc}\u{301}".chars().collect();
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    for _ in 0..300 {
        let text: String = (0..next(24)).map(|_| chars[next(chars.len())]).collect();
        let style = TextStyle {
            align: [TextAlign::Left, TextAlign::Center, TextAlign::Right][next(3)],
            italic: next(2) == 0,
            letter_spacing: [0.0, 0.5, -0.3][next(3)],
            ..TextStyle::default()
        };
        let angle = next(8) as f64 * 0.7;
        match paper_text(&text, None, AT, 2.5 + next(4) as f64, angle, &style) {
            Ok(Some(set)) => {
                let (runs, font) = one(set);
                scene(&runs, &font).validate().unwrap_or_else(|e| panic!("{text:?} {style:?}: {e:?}"));
            }
            Ok(None) => assert!(text.chars().all(|c| c == '\n'), "{text:?}"),
            Err(e) => panic!("{text:?}: {e}"),
        }
    }
}
