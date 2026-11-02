//! Text on paper (#4890 B1b): laid out by the rule sketch text measures with, in the drawing's
//! font, as runs the paper scene takes. Expected numbers come from the font's own tables
//! (Liberation Sans: 2048 units per em, a cap height of 1409) or from `TextMetrics`. Named families
//! are looked up in the repository's own fonts, never the computer's.

use std::path::PathBuf;

use sha2::{Digest, Sha256};
use solidcraft_geom::Vec2;
use solidcraft_geom::paper::text::{Embed, Font, TextRun};
use solidcraft_geom::paper::{Group, Item, ObjectKind, Scene};

use crate::fonts::fallback::{Han, Query};
use crate::fonts::font_data;
use crate::tests::v;
use crate::*;

const AT: Vec2 = Vec2 { x: 100.0, y: 50.0 };

fn set(text: &str, style: &TextStyle) -> (Vec<TextRun>, Font) {
    one(paper_text(text, None, "", AT, 5.0, 0.0, style).unwrap().unwrap())
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
    assert_eq!(fonts.len(), 1, "one font");
    (runs, fonts.remove(0))
}

/// The runs and fonts of a text that falls back, each font holding exactly the glyphs its runs place,
/// and a scene that takes them.
fn mixed((runs, fonts): (Vec<TextRun>, Vec<Font>)) -> (Vec<TextRun>, Vec<Font>) {
    assert!(fonts.windows(2).all(|w| w[0].id < w[1].id), "once each, in order of id");
    for font in &fonts {
        let placed: std::collections::BTreeSet<u16> = runs.iter().filter(|r| r.font == font.id).flat_map(|r| r.glyphs.iter().map(|g| g.id)).collect();
        assert!(font.glyphs.keys().copied().eq(placed), "{} holds exactly its runs' glyphs", font.postscript_name);
    }
    let items = runs.iter().cloned().map(Item::Text).collect();
    let mut scene = Scene::new(v(420.0, 297.0), vec![Group { object: 1, kind: ObjectKind::TitleBlock, items }]);
    scene.add_fonts(fonts.iter().cloned());
    scene.validate().unwrap();
    (runs, fonts)
}

fn set_mixed(text: &str, family: Option<&str>, language: &str) -> (Vec<TextRun>, Vec<Font>) {
    mixed(paper_text_in(&repo_fonts(), text, family, language, AT, 5.0, 0.0, &TextStyle::default()).unwrap().unwrap())
}

/// A folder of its own for a search, emptied when dropped, holding copies of BIZ UDPGothic.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str, copies: &[&str]) -> Folder {
        let folder = std::env::temp_dir().join(format!("solidcraft-fallback-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        for copy in copies {
            std::fs::write(folder.join(copy), gothic()).unwrap();
        }
        Folder(folder)
    }

    fn dirs(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }

    /// The file the search finds for `ch`, by its name.
    fn found(&self, han: Han, ch: char) -> Option<String> {
        let answer = crate::fonts::fallback::answers(&[Query::new(&self.dirs(), han, false, false, ch)], true).unwrap().remove(0);
        answer.map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn gothic() -> Vec<u8> {
    std::fs::read(repo_fonts()[0].join("biz-udp-gothic/BIZUDPGothic-Regular.ttf")).unwrap()
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
    paper_text_in(&repo_fonts(), text, family, "", AT, 5.0, 0.0, style).unwrap_err().to_string()
}

/// The repository's fonts as the only installed ones: Liberation Sans, IBM Plex Mono, BIZ
/// UDPGothic and IBM Plex Sans (a variable face).
fn repo_fonts() -> Vec<PathBuf> {
    vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts")]
}

fn set_in(text: &str, family: &str) -> (Vec<TextRun>, Font) {
    one(paper_text_in(&repo_fonts(), text, Some(family), "", AT, 5.0, 0.0, &TextStyle::default()).unwrap().unwrap())
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
    assert_eq!((font.metrics.italic_angle, font.metrics.flags, font.embed), (0.0, Default::default(), Embed::Subset));
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
    let (runs, font) = one(paper_text("AB\nC", None, "", AT, 5.0, angle, &style).unwrap().unwrap());
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
fn a_character_no_font_has_is_refused_by_name() {
    let e = err("Size \u{d55c}", None, &TextStyle::default());
    assert!(e.contains("no glyph for '\u{d55c}'") && !e.contains("the web"), "{e}");
    let e = err("a\tb", None, &TextStyle::default());
    assert!(e.contains("'\\t'"), "{e}");
}

#[test]
fn a_line_falls_back_per_character_at_the_same_em_on_its_baseline() {
    let (runs, fonts) = set_mixed("Size \u{30c6}\u{30b9}\u{30c8} 12 A", None, "");
    assert_eq!(fonts.len(), 2, "{runs:?}");
    let own = fonts.iter().find(|f| f.postscript_name.starts_with("Liberation")).unwrap();
    let other = fonts.iter().find(|f| f.id != own.id).unwrap();
    let texts: Vec<(&str, bool)> = runs.iter().map(|r| (r.text.as_str(), r.font == own.id)).collect();
    // A space and digits stay with the run they follow, when its font has them.
    assert_eq!(texts, [("Size ", true), ("\u{30c6}\u{30b9}\u{30c8} 12 ", false), ("A", true)]);
    let k = 5.0 / f64::from(own.metrics.cap_height);
    assert!((runs[0].scale - k).abs() < 1e-12);
    assert!((runs[1].scale - k * f64::from(own.units_per_em) / f64::from(other.units_per_em)).abs() < 1e-12);
    assert!(runs.iter().all(|r| r.origin == AT), "every run starts where the line does");
    // Along the whole line, one run after another.
    let end = |r: &TextRun| r.glyphs.last().map(|g| g.x + g.advance).unwrap();
    assert!((runs[1].glyphs[0].x - end(&runs[0])).abs() < 1e-9);
    assert!((runs[2].glyphs[0].x - end(&runs[1])).abs() < 1e-9);
    assert_eq!(runs.iter().map(|r| r.text.len()).sum::<usize>(), "Size \u{30c6}\u{30b9}\u{30c8} 12 A".len());
}

/// Fonts of different units per em: the same em on paper.
#[test]
fn a_fallback_takes_the_em_of_the_text_font() {
    let (runs, fonts) = set_mixed("A\u{30c6}", Some("IBM Plex Mono"), "");
    let em = |run: &TextRun, units: u16| run.scale * f64::from(fonts.iter().find(|f| f.units_per_em == units).unwrap().units_per_em);
    assert!((em(&runs[0], 1000) - em(&runs[1], 2048)).abs() < 1e-12, "{runs:?}");
}

/// Beside a fallback the text's own font stays as it is alone, as sketch text sets it: no pair
/// closed up by a shaper's kerning.
#[test]
fn the_own_font_beside_a_fallback_is_set_as_it_is_alone() {
    let (alone, _) = set("AVAV", &TextStyle::default());
    let (runs, _) = set_mixed("AVAV\u{30c6}", None, "");
    assert_eq!(runs[0].glyphs, alone[0].glyphs);
}

#[test]
fn letter_spacing_goes_between_the_runs_too() {
    let style = TextStyle { letter_spacing: 0.5, ..TextStyle::default() };
    let (runs, _) = mixed(paper_text_in(&repo_fonts(), "A\u{30c6}B", None, "", AT, 5.0, 0.0, &style).unwrap().unwrap());
    assert_eq!(runs.len(), 3);
    let end = |r: &TextRun| r.glyphs.last().map(|g| g.x + g.advance).unwrap();
    assert!((runs[1].glyphs[0].x - end(&runs[0]) - 0.5).abs() < 1e-9);
    assert!((runs[2].glyphs[0].x - end(&runs[1]) - 0.5).abs() < 1e-9);
}

#[test]
fn alignment_counts_every_run_of_the_line() {
    // Spaced, and ending in the text's own font: the width stops at the last glyph's advance.
    let spaced = TextStyle { letter_spacing: 0.5, ..TextStyle::default() };
    let left = mixed(paper_text_in(&repo_fonts(), "\u{30c6}A", None, "", AT, 5.0, 0.0, &spaced).unwrap().unwrap()).0;
    let width = left[1].glyphs.last().map(|g| g.x + g.advance).unwrap();
    let style = TextStyle { align: TextAlign::Right, ..spaced };
    let (runs, _) = mixed(paper_text_in(&repo_fonts(), "\u{30c6}A", None, "", AT, 5.0, 0.0, &style).unwrap().unwrap());
    assert!((runs[0].origin.x - (AT.x - width)).abs() < 1e-9, "{:?}", runs[0].origin);
}

/// Pieces of one right-to-left stretch in different fonts stand in display order: the last first.
#[test]
fn fonts_within_a_right_to_left_stretch_stand_in_display_order() {
    let (runs, fonts) = set_mixed("\u{5d0}\u{30fb}\u{5d1}", Some("BIZ UDPGothic"), "");
    assert_eq!(fonts.len(), 2);
    let texts: Vec<&str> = runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, ["\u{5d1}", "\u{30fb}", "\u{5d0}"]);
    assert!(runs.iter().all(|r| r.rtl), "{runs:?}");
}

/// The text's own font is set as sketch text sets it, glyph by glyph, beside a run that falls back.
#[test]
fn the_own_fonts_run_stays_unshaped_beside_a_fallback() {
    let (alone, _) = set("office", &TextStyle::default());
    let (runs, _) = set_mixed("office\u{30c6}", None, "");
    assert_eq!(runs[0].glyphs, alone[0].glyphs);
}

/// A text wholly in fonts fallen back to lists only those.
#[test]
fn a_font_no_run_uses_isnt_listed() {
    let (_, fonts) = set_mixed("\u{30c6}", None, "");
    assert_eq!(fonts.len(), 1);
    assert!(fonts[0].postscript_name.starts_with("BIZ"), "{}", fonts[0].postscript_name);
}

#[test]
fn each_script_has_its_list() {
    let han = Han::Korean;
    for (ch, script) in [
        ('\u{5d0}', "Hebrew"),
        ('\u{fb1d}', "Hebrew"),
        ('\u{628}', "Arabic"),
        ('\u{fefc}', "Arabic"),
        ('\u{e01}', "Thai"),
        ('\u{1100}', "Hangul"),
        ('\u{d55c}', "Hangul"),
        ('\u{3042}', "Kana"),
        ('\u{ff66}', "Kana"),
        ('\u{4e2d}', "Han"),
        ('\u{3001}', "Han"),
        ('\u{20000}', "Han"),
        ('\u{416}', "Other text"),
    ] {
        assert_eq!(crate::fonts::fallback::script_name(ch, han).0, script, "{ch}");
    }
    assert_eq!(crate::fonts::fallback::script_name('\u{4e2d}', han).1, Some("Korean"));
    assert_eq!(crate::fonts::fallback::script_name('\u{3042}', han).1, None);
}

/// A right-to-left line in a font with no Hebrew: its letters come from one that has them, shaped,
/// with their points, and the runs stand in display order.
#[test]
fn a_right_to_left_run_falls_back_shaped_and_in_display_order() {
    let line = "ab \u{5e9}\u{5c1}\u{5b8}\u{5dc}\u{5d5}\u{5b9}\u{5dd} cd";
    let (runs, fonts) = set_mixed(line, Some("IBM Plex Mono"), "");
    assert_eq!(fonts.len(), 2);
    let hebrew = runs.iter().find(|r| r.text.contains('\u{5e9}')).unwrap();
    assert!(hebrew.rtl);
    assert!(hebrew.clusters.len() < hebrew.text.chars().count(), "marks in their letters' clusters: {:?}", hebrew.clusters);
    let xs: Vec<f64> = runs.iter().map(|r| r.glyphs[0].x).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "left to right along the line: {xs:?}");
    assert_eq!(runs.first().unwrap().text, "ab ");
    assert_eq!(runs.last().unwrap().text, "cd");
}

/// A line its own font has all of is set as it always was, even beside one that falls back.
#[test]
fn a_line_with_nothing_lacking_is_set_as_before() {
    let (alone, _) = set("Size", &TextStyle::default());
    let (runs, _) = set_mixed("Size\n\u{30c6}", None, "");
    assert_eq!(runs[0].glyphs, alone[0].glyphs);
    assert_eq!(runs[0].clusters, alone[0].clusters);
}

/// A mark stays in its letter's font: one that font lacks is refused, not set apart.
#[test]
fn a_mark_never_leaves_its_letter() {
    let e = paper_text_in(&repo_fonts(), "\u{30c6}\u{5b8}", None, "", AT, 5.0, 0.0, &TextStyle::default()).unwrap_err().to_string();
    assert!(e.contains("no glyph for '\\u{5b8}'"), "{e}");
}

#[test]
fn han_takes_the_drawings_language_then_what_the_text_shows() {
    for (language, text, han) in [
        ("ja", "\u{4e2d}", Han::Japanese),
        ("ja-JP", "\u{4e2d}", Han::Japanese),
        ("ko", "\u{4e2d}", Han::Korean),
        ("zh", "\u{4e2d}", Han::SimplifiedChinese),
        ("zh-Hans", "\u{4e2d}", Han::SimplifiedChinese),
        ("zh-TW", "\u{4e2d}", Han::TraditionalChinese),
        ("zh-Hant-HK", "\u{4e2d}", Han::TraditionalChinese),
        ("zh_hk", "\u{4e2d}", Han::TraditionalChinese),
        ("en", "\u{4e2d}\u{30c6}", Han::Japanese),
        ("", "\u{4e2d}\u{d55c}", Han::Korean),
        ("de", "\u{4e2d}", Han::SimplifiedChinese),
        // The language wins over what the text shows.
        ("zh", "\u{4e2d}\u{30c6}", Han::SimplifiedChinese),
    ] {
        assert_eq!(Han::of(language, text), han, "{language} {text}");
    }
}

/// The script's list comes before the rest, in the list's order and by the language; then the first
/// file in sorted order.
#[test]
fn the_list_is_tried_first_then_the_first_file_in_order() {
    let both = Folder::new("list", &["aaa.ttf", "msyh.ttc", "msgothic.ttc", "msjh.ttc"]);
    assert_eq!(both.found(Han::Japanese, '\u{4e2d}').as_deref(), Some("msgothic.ttc"));
    assert_eq!(both.found(Han::SimplifiedChinese, '\u{4e2d}').as_deref(), Some("msyh.ttc"));
    assert_eq!(both.found(Han::TraditionalChinese, '\u{4e2d}').as_deref(), Some("msjh.ttc"));
    assert_eq!(both.found(Han::SimplifiedChinese, '\u{30c6}').as_deref(), Some("msgothic.ttc"), "kana is Japanese whatever the language");
    assert_eq!(both.found(Han::Japanese, 'A').as_deref(), Some("aaa.ttf"), "a script with no list goes straight to the rest");
    let lacking = Folder::new("lacking", &["malgun.ttf"]);
    assert_eq!(lacking.found(Han::Korean, '\u{d55c}'), None, "a known file without the character is no answer");
    let rest = Folder::new("rest", &["c.ttf", "b.ttf"]);
    assert_eq!(rest.found(Han::Japanese, '\u{30c6}').as_deref(), Some("b.ttf"));
}

/// A known file that can't be read as a font doesn't end the list.
#[test]
fn a_broken_known_file_is_passed_over() {
    let folder = Folder::new("broken", &["aaa.ttf", "msgothic.ttc"]);
    std::fs::write(folder.0.join("meiryo.ttc"), b"not a font").unwrap();
    assert_eq!(folder.found(Han::Japanese, '\u{30c6}').as_deref(), Some("msgothic.ttc"));
}

/// A file with a character it can't draw isn't an answer: a bitmap face, as an emoji font is.
#[test]
fn a_face_with_no_outlines_is_passed_over() {
    let folder = Folder::new("bitmap", &["b.ttf"]);
    let mut bitmap = gothic();
    without(&mut bitmap, b"glyf");
    std::fs::write(folder.0.join("a.ttf"), bitmap).unwrap();
    assert_eq!(folder.found(Han::Japanese, '\u{30c6}').as_deref(), Some("b.ttf"));
}

/// "None has it" is kept for the process, as an answer is: no second search of every font.
#[test]
fn a_miss_is_kept() {
    let folder = Folder::new("miss", &[]);
    assert_eq!(folder.found(Han::Japanese, '\u{30c6}'), None);
    std::fs::write(folder.0.join("a.ttf"), gothic()).unwrap();
    assert_eq!(folder.found(Han::Japanese, '\u{30c6}'), None);
    assert_eq!(folder.found(Han::Japanese, '\u{30b9}').as_deref(), Some("a.ttf"), "another character is another search");
}

/// The window's way in never searches: it says the fonts are being looked for until they are found.
#[test]
fn the_windows_way_in_waits_for_no_search() {
    let folder = Folder::new("thread", &["a.ttf"]);
    let dirs = folder.dirs();
    let plain = TextStyle::default();
    let now = || paper_text_now(&dirs, "A\u{30c6}", None, "", AT, 5.0, 0.0, &plain);
    assert!(matches!(now(), Err(SketchError::FontPending)));
    // Nothing to fall back to is never pending.
    assert!(paper_text_now(&dirs, "A", None, "", AT, 5.0, 0.0, &plain).unwrap().is_some());
    let answer = loop {
        match now() {
            Err(SketchError::FontPending) => std::thread::yield_now(),
            other => break other,
        }
    };
    let (runs, fonts) = mixed(answer.unwrap().unwrap());
    assert_eq!((runs.len(), fonts.len()), (2, 2));
}

#[test]
fn a_font_fallen_back_to_takes_its_nearest_style() {
    // BIZ UDPGothic has no italic and no bold: it leans, and bold isn't refused for it.
    let style = TextStyle { italic: true, ..TextStyle::default() };
    let (runs, _) = mixed(paper_text_in(&repo_fonts(), "A\u{30c6}", None, "", AT, 5.0, 0.0, &style).unwrap().unwrap());
    assert!(runs.iter().all(|r| r.slant > 0.0), "{runs:?}");
    // Each font's own: a chosen face that is italic doesn't lean, the one fallen back to does.
    let leaning = Folder::new("italic", &["a.ttf"]);
    let mut mono = std::fs::read(repo_fonts()[0].join("ibm-plex-mono/IBMPlexMono-Regular.ttf")).unwrap();
    os2(&mut mono, 62, 0x01);
    std::fs::write(leaning.0.join("mono.ttf"), mono).unwrap();
    let (runs, _) = mixed(paper_text_in(&leaning.dirs(), "A\u{30c6}", Some("IBM Plex Mono"), "", AT, 5.0, 0.0, &style).unwrap().unwrap());
    assert_eq!(runs[0].slant, 0.0);
    assert!(runs[1].slant > 0.0);
    let bold = TextStyle { bold: true, ..TextStyle::default() };
    // A bold face to choose: IBM Plex Mono, marked bold in its OS/2 fsSelection.
    let folder = Folder::new("bold", &["a.ttf"]);
    let mut mono = std::fs::read(repo_fonts()[0].join("ibm-plex-mono/IBMPlexMono-Regular.ttf")).unwrap();
    os2(&mut mono, 62, 0x20);
    std::fs::write(folder.0.join("mono.ttf"), mono).unwrap();
    let (runs, fonts) = mixed(paper_text_in(&folder.dirs(), "A\u{30c6}", Some("IBM Plex Mono"), "", AT, 5.0, 0.0, &bold).unwrap().unwrap());
    assert_eq!((runs.len(), fonts.len()), (2, 2));
}

#[test]
fn the_status_names_what_fell_back_and_looks_for_nothing() {
    let folder = Folder::new("notes", &["a.ttf"]);
    let dirs = folder.dirs();
    // Nothing searched yet: nothing to say, and nothing was looked for.
    assert!(paper_fallback_notes(&dirs, "", "ja", ["\u{4e2d}\u{30c6}"]).is_empty());
    assert_eq!(folder.found(Han::Japanese, '\u{4e2d}').as_deref(), Some("a.ttf"));
    assert_eq!(folder.found(Han::Japanese, '\u{30c6}').as_deref(), Some("a.ttf"));
    assert_eq!(
        paper_fallback_notes(&dirs, "", "ja", ["A\u{4e2d}\u{30c6}", "B"]),
        ["Han set in BIZ UDPGothic for Japanese", "Kana set in BIZ UDPGothic"]
    );
}

/// No font folders, as in the browser: Japanese waits for the font the web app fetches, then is
/// set by it, and what it lacks is refused by name. The one test, as the font's state is the process's.
#[test]
fn on_the_web_japanese_waits_for_the_fetched_font() {
    use crate::fonts::fallback::{provide_web_font, web_font_failed, web_font_wanted};
    let plain = TextStyle::default();
    let set = |text: &str| paper_text_in(&[], text, None, "", AT, 5.0, 0.0, &plain);
    // Nothing that font could set doesn't ask for it.
    let e = set("\u{628}").unwrap_err().to_string();
    assert!(e.contains("no glyph for") && e.contains("the web covers Japanese for now"), "{e}");
    assert!(!web_font_wanted());
    assert!(set("A").unwrap().is_some());
    assert!(!web_font_wanted(), "text with its own font asks for nothing");
    // Waiting, whichever way it's asked.
    assert!(matches!(set("A\u{30c6}"), Err(SketchError::FontPending)));
    assert!(web_font_wanted());
    assert!(matches!(paper_text_now(&[], "\u{4e2d}", None, "", AT, 5.0, 0.0, &plain), Err(SketchError::FontPending)));
    assert!(paper_fallback_notes(&[], "", "", ["\u{30c6}"]).is_empty(), "nothing to say yet");
    // The fetch failed: refused by name, and it stays wanted no more.
    web_font_failed();
    assert!(!web_font_wanted());
    let e = set("\u{30c6}").unwrap_err().to_string();
    assert!(e.contains("the web covers Japanese for now"), "{e}");
    // It came (a later try): set, in that font, under the name a face of it has.
    provide_web_font(gothic());
    assert!(!web_font_wanted());
    let (runs, fonts) = mixed(set("A\u{30c6}").unwrap().unwrap());
    assert_eq!((runs.len(), fonts.len()), (2, 2));
    assert!(fonts.iter().any(|f| f.postscript_name.starts_with("BIZ")));
    assert_eq!(paper_fallback_notes(&[], "", "", ["\u{30c6}"]), ["Kana set in BIZ UDPGothic"]);
    // What it lacks: Hangul and Arabic, refused by name, with no font asked for.
    let e = set("\u{d55c}").unwrap_err().to_string();
    assert!(e.contains("no glyph for") && e.contains("the web covers Japanese for now"), "{e}");
}

/// The same text and fonts give the same runs and fonts, however the answers came.
#[test]
fn the_same_text_sets_the_same() {
    let text = "Size \u{4e2d}\u{6587} \u{5e9}\u{5dc}\u{5d5}\u{5dd} 12";
    let first = set_mixed(text, Some("IBM Plex Mono"), "ja");
    let again = set_mixed(text, Some("IBM Plex Mono"), "ja");
    assert_eq!(first.0, again.0);
    assert_eq!(
        first.1.iter().map(|f| (f.id, f.glyphs.len())).collect::<Vec<_>>(),
        again.1.iter().map(|f| (f.id, f.glyphs.len())).collect::<Vec<_>>()
    );
}

/// Random lines mixing what each font has: set into runs a scene accepts, every byte in a run, or
/// refused by name, never a panic.
#[test]
fn fuzz_fallback_is_a_scene_or_a_refusal() {
    let chars: Vec<char> = "Ab9 .(-\n\u{5e9}\u{5dc}\u{5b8}\u{5c1}\u{301}\u{200d}\u{30c6}\u{30b9}\u{4e2d}\u{3001}\u{d55c}\t".chars().collect();
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    for _ in 0..300 {
        let text: String = (0..next(24)).map(|_| chars[next(chars.len())]).collect();
        let family = [None, Some("IBM Plex Mono"), Some("BIZ UDPGothic")][next(3)];
        let style = TextStyle { italic: next(2) == 0, letter_spacing: [0.0, 0.5, -0.3][next(3)], ..TextStyle::default() };
        match paper_text_in(&repo_fonts(), &text, family, ["", "ja", "zh-TW"][next(3)], AT, 2.5 + next(4) as f64, 0.5, &style) {
            Ok(Some(set)) => {
                let (runs, _) = mixed(set);
                let bytes = text.bytes().filter(|b| *b != b'\n').count();
                assert_eq!(runs.iter().map(|r| r.text.len()).sum::<usize>(), bytes, "{text:?}");
            }
            Ok(None) => assert!(text.chars().all(|c| c == '\n'), "{text:?}"),
            Err(SketchError::Invalid(why)) => assert!(why.contains("no glyph for"), "{text:?}: {why}"),
            Err(e) => panic!("{text:?}: {e}"),
        }
    }
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
        ("Liberation Serif", "Liberation Serif isn't on this computer: set in Liberation Sans"),
    ] {
        assert_eq!(set_in("Title", family), builtin, "{family}");
        assert_eq!(paper_font_note(&repo_fonts(), family).as_deref(), Some(note));
    }
    for family in ["", " ", "Liberation Sans", "liberation-sans"] {
        assert_eq!(paper_font_note(&repo_fonts(), family), None, "{family:?}");
    }
}

/// IBM Plex Mono's file with `patch` applied, alone in a folder of its own, removed at the end of
/// the test.
struct Patched(PathBuf);

impl Patched {
    fn new(name: &str, patch: impl FnOnce(&mut Vec<u8>)) -> Patched {
        let mut bytes = std::fs::read(repo_fonts()[0].join("ibm-plex-mono/IBMPlexMono-Regular.ttf")).unwrap();
        patch(&mut bytes);
        let dir = std::env::temp_dir().join(format!("solidcraft-paper-fonts-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("patched.ttf"), bytes).unwrap();
        Patched(dir)
    }

    /// The face admitted for paper, or why it isn't.
    fn admitted(&self) -> std::result::Result<Font, String> {
        super::admit(&font_data(Some(&self.0.join("patched.ttf").to_string_lossy())).unwrap())
    }
}

impl Drop for Patched {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Where table `tag`'s record is in a font file's table directory.
fn record(bytes: &[u8], tag: &[u8; 4]) -> usize {
    let tables = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
    (0..tables).map(|i| 12 + 16 * i).find(|&r| &bytes[r..r + 4] == tag).unwrap()
}

/// Where table `tag` starts in a font file.
fn table(bytes: &[u8], tag: &[u8; 4]) -> usize {
    let r = record(bytes, tag);
    u32::from_be_bytes(bytes[r + 8..r + 12].try_into().unwrap()) as usize
}

/// The font file without table `tag`: its record renamed, so no reader finds it.
fn without(bytes: &mut [u8], tag: &[u8; 4]) {
    let r = record(bytes, tag);
    bytes[r..r + 4].copy_from_slice(b"zzzz");
}

/// The font file with its OS/2 `field` (a 16-bit field at that byte) set to `value`.
fn os2(bytes: &mut [u8], field: usize, value: u16) {
    let at = table(bytes, b"OS/2") + field;
    bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

/// The font file with table `tag` in the place of its `post` table, which is long enough.
fn with_table(bytes: &mut [u8], tag: &[u8; 4], data: &[u8]) {
    let (r, at) = (record(bytes, b"post"), table(bytes, b"post"));
    bytes[at..at + data.len()].copy_from_slice(data);
    bytes[r + 12..r + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
    bytes[r..r + 4].copy_from_slice(tag);
}

/// The font file with its first `count` glyphs advancing `width`.
fn advances(bytes: &mut [u8], count: usize, width: u16) {
    let at = table(bytes, b"hmtx");
    (0..count).for_each(|g| bytes[at + 4 * g..at + 4 * g + 2].copy_from_slice(&width.to_be_bytes()));
}

/// A variable face is set at its default instance, which is what a PDF draws.
#[test]
fn a_variable_face_is_set_at_its_default_instance() {
    let (runs, font) = set_in("Title 12", "IBM Plex Sans");
    let face = ttf_parser::Face::parse(&font.bytes, 0).unwrap();
    assert!(face.is_variable() && font.postscript_name.starts_with("IBMPlexSans"), "{}", font.postscript_name);
    let t = face.glyph_index('T').unwrap();
    assert_eq!(font.glyphs[&t.0].advance, face.glyph_hor_advance(t).unwrap(), "the default instance's advance");
    scene(&runs, &font).validate().unwrap();
    assert_eq!(paper_font_note(&repo_fonts(), "IBM Plex Sans"), None);
}

/// A face whose tables lack what a PDF's font descriptor asks for is admitted with it made up for:
/// a cap height as sketch text scales by, no italic angle and a fixed pitch from the advances, and a
/// PostScript name from its family and style or its id.
#[test]
fn what_a_face_lacks_is_made_up_for() {
    let plain = Patched::new("plain", |_| {}).admitted().unwrap();
    assert_eq!((plain.postscript_name.as_str(), plain.embed, plain.metrics.flags.fixed_pitch), ("IBMPlexMono-Regular", Embed::Subset, true));
    // The cap height in OS/2 (`sCapHeight`, at byte 88) when it has one; version 1 has none: the
    // top of `H`.
    let face = ttf_parser::Face::parse(&plain.bytes, 0).unwrap();
    let h = face.glyph_bounding_box(face.glyph_index('H').unwrap()).unwrap().y_max;
    assert_ne!(h, 600);
    assert_eq!(Patched::new("cap-600", |b| os2(b, 88, 600)).admitted().unwrap().metrics.cap_height, 600);
    let v1 = Patched::new("os2-v1", |b| {
        os2(b, 88, 600);
        os2(b, 0, 1);
    });
    assert_eq!(v1.admitted().unwrap().metrics.cap_height, h);
    let no_post = Patched::new("no-post", |b| without(b, b"post"));
    let font = no_post.admitted().unwrap();
    assert_eq!((font.metrics.italic_angle, font.metrics.flags.fixed_pitch), (0.0, true));
    // Without `post`, a glyph of another width means no fixed pitch, and so does nothing advancing.
    let glyphs = usize::from(font.num_glyphs);
    let pitch = |name: &str, count, width| {
        let patched = Patched::new(name, |b| {
            without(b, b"post");
            advances(b, count, width);
        });
        patched.admitted().unwrap().metrics.flags.fixed_pitch
    };
    assert!(!pitch("one-wider", 1, 900));
    assert!(!pitch("no-advances", glyphs, 0));
    // Text set in either is text the scene takes.
    for patched in [&v1, &no_post] {
        let set = paper_text_in(std::slice::from_ref(&patched.0), "Title 12", Some("IBM Plex Mono"), "", AT, 5.0, 0.0, &TextStyle::default());
        let (runs, font) = one(set.unwrap().unwrap());
        scene(&runs, &font).validate().unwrap();
    }
    // No PostScript name or style: the family's name; no names at all: the id.
    let unnamed = Patched::new("no-postscript-name", |b| {
        let at = table(b, b"name");
        for i in 0..usize::from(u16::from_be_bytes([b[at + 2], b[at + 3]])) {
            let id = at + 6 + 12 * i + 6;
            if [2, 6, 17].contains(&u16::from_be_bytes([b[id], b[id + 1]])) {
                b[id..id + 2].copy_from_slice(&255u16.to_be_bytes());
            }
        }
    });
    assert_eq!(unnamed.admitted().unwrap().postscript_name, "IBMPlexMono");
    let nameless = Patched::new("no-names", |b| without(b, b"name")).admitted().unwrap();
    let hex: String = nameless.id[..4].iter().map(|b| format!("{b:02X}")).collect();
    assert_eq!(nameless.postscript_name, format!("Font-{hex}"));
    // A name longer than a PDF's: the whole of the name table's text, read as the PostScript name.
    let long = Patched::new("long-name", |b| {
        let at = table(b, b"name");
        let postscript = (0..usize::from(u16::from_be_bytes([b[at + 2], b[at + 3]])))
            .map(|i| at + 6 + 12 * i + 6)
            .find(|&id| u16::from_be_bytes([b[id], b[id + 1]]) == 6)
            .unwrap();
        b[postscript + 2..postscript + 4].copy_from_slice(&1000u16.to_be_bytes());
        b[postscript + 4..postscript + 6].copy_from_slice(&0u16.to_be_bytes());
    });
    assert_eq!(long.admitted().unwrap().postscript_name.len(), 127);
    // No `l` or `I` to measure: a fifth of the weight class per thousand units of em.
    let no_stem = Patched::new("no-stem", |b| {
        without(b, b"cmap");
        os2(b, 4, 700);
        let head = table(b, b"head");
        b[head + 18..head + 20].copy_from_slice(&2000u16.to_be_bytes());
    });
    assert_eq!(no_stem.admitted().unwrap().metrics.stem_v, 280);
}

/// Outlines needn't be TrueType ones, and a PDF can't hold CFF2's. No font in the repository has
/// either, so these are the smallest tables a reader takes: one glyph, empty.
#[test]
fn cff_outlines_are_outlines_and_a_pdf_cant_hold_cff2() {
    // Header; name, top dictionary (its charstrings at byte 23), string and subroutine indexes;
    // the charstrings.
    let cff = [1, 0, 4, 1, 0, 1, 1, 1, 2, b'A', 0, 1, 1, 1, 5, 28, 0, 23, 17, 0, 0, 0, 0, 0, 1, 1, 1, 2, 14];
    // Header; top dictionary (its charstrings at byte 13); subroutine index; the charstrings.
    let cff2 = [2, 0, 5, 0, 4, 28, 0, 13, 17, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 2, 14];
    for (name, tag, data, embed) in [("cff", b"CFF ", &cff[..], Embed::Subset), ("cff2", b"CFF2", &cff2[..], Embed::Outlines)] {
        let patched = Patched::new(name, |b| {
            without(b, b"glyf");
            with_table(b, tag, data);
        });
        assert_eq!(patched.admitted().unwrap().embed, embed, "{name}");
    }
}

/// How a PDF may carry a face comes from its embedding permissions (OS/2 `fsType`, at byte 8).
#[test]
fn a_faces_licence_says_how_a_pdf_carries_it() {
    for (fs_type, embed) in [
        (0x0000, Embed::Subset),
        (0x0008, Embed::Subset),
        (0x0002, Embed::Outlines),
        (0x0200, Embed::Outlines),
        (0x0100, Embed::Whole),
        (0x0102, Embed::Outlines),
    ] {
        let font = Patched::new(&format!("fs-type-{fs_type:x}"), |b| os2(b, 8, fs_type)).admitted().unwrap();
        assert_eq!(font.embed, embed, "{fs_type:#06x}");
    }
}

/// A face with no outlines to fill (a bitmap or colour-only font) is refused, and its family is
/// set in the built-in font, named.
#[test]
fn a_face_with_no_outlines_is_refused() {
    let bitmap = Patched::new("no-outlines", |b| without(b, b"glyf"));
    assert_eq!(bitmap.admitted().unwrap_err(), "it has no outlines: a bitmap or colour font");
    let note = paper_font_note(std::slice::from_ref(&bitmap.0), "IBM Plex Mono");
    assert_eq!(note.as_deref(), Some("IBM Plex Mono can't be set on paper (it has no outlines: a bitmap or colour font): set in Liberation Sans"));
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
    let (runs, _) =
        paper_text("A", Some(" liberation sans "), "", AT, 5.0, 0.0, &TextStyle { italic: true, ..TextStyle::default() }).unwrap().unwrap();
    assert_eq!(runs[0].slant, 12f64.to_radians().tan(), "a made-up italic leans as sketch text's");
    let e = err("A", None, &TextStyle { path: Some("c1".into()), ..TextStyle::default() });
    assert!(e.contains("follows no path"), "{e}");
}

#[test]
fn empty_lines_set_nothing() {
    for text in ["", "\n", "\n\n"] {
        assert!(paper_text(text, None, "", AT, 5.0, 0.0, &TextStyle::default()).unwrap().is_none(), "{text:?}");
    }
    let (runs, _) = set("A\n\nB", &TextStyle::default());
    assert_eq!(runs.len(), 2);
}

#[test]
fn hostile_numbers_and_sizes_are_refused() {
    let plain = TextStyle::default();
    for (at, height, angle) in [(AT, f64::NAN, 0.0), (AT, 0.0, 0.0), (AT, 1e7, 0.0), (v(f64::NAN, 0.0), 5.0, 0.0), (AT, 5.0, f64::INFINITY)] {
        assert!(paper_text("A", None, "", at, height, angle, &plain).is_err(), "{at:?} {height} {angle}");
    }
    assert!(matches!(paper_text(&"A".repeat(MAX_TEXT_CHARS + 1), None, "", AT, 5.0, 0.0, &plain), Err(SketchError::TooLarge)));
    assert!(paper_text("A", None, "", AT, 5.0, 0.0, &TextStyle { letter_spacing: f64::NAN, ..TextStyle::default() }).is_err());
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
        match paper_text(&text, None, "", AT, 2.5 + next(4) as f64, angle, &style) {
            Ok(Some(set)) => {
                let (runs, font) = one(set);
                scene(&runs, &font).validate().unwrap_or_else(|e| panic!("{text:?} {style:?}: {e:?}"));
            }
            Ok(None) => assert!(text.chars().all(|c| c == '\n'), "{text:?}"),
            Err(e) => panic!("{text:?}: {e}"),
        }
    }
}
