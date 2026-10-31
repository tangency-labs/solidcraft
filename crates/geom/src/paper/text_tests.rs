use std::f64::consts::{FRAC_PI_2, TAU};

use serde_json::json;

use super::*;
use crate::paper::{Group, Item, ObjectKind};

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// A made-up font of 3 glyphs at 1000 units per em, cap height 700: glyph 1 a square 500 wide and
/// 700 tall, glyph 2 a triangle with a curved side; both of the run's glyphs and nothing else.
fn font() -> Font {
    let square = Loop2::polygon(&[p(0.0, 0.0), p(500.0, 0.0), p(500.0, 700.0), p(0.0, 700.0)]);
    let curve = Seg2::Conic { a: p(0.0, 700.0), apex: p(-100.0, 350.0), b: p(0.0, 0.0), w: 1.0 };
    let triangle = Loop2 { segs: vec![Seg2::Line { a: p(0.0, 0.0), b: p(400.0, 0.0) }, Seg2::Line { a: p(400.0, 0.0), b: p(0.0, 700.0) }, curve] };
    let metrics = FontMetrics {
        ascent: 900,
        descent: -200,
        cap_height: 700,
        bbox: [-100, -200, 1000, 900],
        italic_angle: 0.0,
        flags: FontFlags::default(),
        stem_v: 80,
    };
    Font {
        id: [7; 32],
        bytes: Arc::from(vec![1u8, 2, 3]),
        face: 0,
        postscript_name: "MadeUp-Regular".into(),
        num_glyphs: 3,
        units_per_em: 1000,
        metrics,
        glyphs: BTreeMap::from([(1, FontGlyph { advance: 600, outline: vec![square] }), (2, FontGlyph { advance: 500, outline: vec![triangle] })]),
    }
}

/// "AB" in 3.5 mm capitals (0.005 mm a unit) from (20, 30): one glyph a character.
fn run() -> TextRun {
    TextRun {
        font: [7; 32],
        scale: 0.005,
        origin: p(20.0, 30.0),
        angle: 0.0,
        slant: 0.0,
        glyphs: vec![PlacedGlyph { id: 1, x: 0.0, rise: 0.0, advance: 3.0 }, PlacedGlyph { id: 2, x: 3.0, rise: 0.0, advance: 2.5 }],
        text: "AB".into(),
        clusters: vec![Cluster { glyphs: 0..1, text: 0..1 }, Cluster { glyphs: 1..2, text: 1..2 }],
        rtl: false,
    }
}

fn scene(runs: Vec<TextRun>, fonts: Vec<Font>) -> Scene {
    let items = runs.into_iter().map(Item::Text).collect();
    Scene { fonts, ..Scene::new(p(420.0, 297.0), vec![Group { object: 1, kind: ObjectKind::TitleBlock, items }]) }
}

/// A closed loop of `n` lines round a circle 100 units across: a glyph outline of `n` pieces.
fn ring(n: usize) -> Loop2 {
    let at = |i: usize| {
        let turn = TAU * (i % n) as f64 / n as f64;
        p(50.0 * turn.cos(), 50.0 * turn.sin())
    };
    Loop2 { segs: (0..n).map(|i| Seg2::Line { a: at(i), b: at(i + 1) }).collect() }
}

/// An edit of the run and its font, for a list of them.
fn e(edit: impl FnOnce(&mut TextRun, &mut Font) + 'static) -> Box<dyn FnOnce(&mut TextRun, &mut Font)> {
    Box::new(edit)
}

fn refused(edit: impl FnOnce(&mut TextRun, &mut Font)) -> String {
    let (mut run, mut font) = (run(), font());
    edit(&mut run, &mut font);
    scene(vec![run], vec![font]).validate().map_or_else(|e| e.0.to_string(), |()| "valid".into())
}

#[test]
fn a_run_and_its_font_pass_and_count_toward_the_records() {
    let scene = scene(vec![run()], vec![font()]);
    scene.validate().unwrap();
    // The group, its item, two glyphs and two clusters; the font, two glyphs, a loop each, 4 + 3 pieces.
    assert_eq!(scene.groups[0].records(), 6);
    assert_eq!(font().records(), 1 + (1 + 1 + 4) + (1 + 1 + 3));
    assert!(scene.prepare().unwrap().font(&run()).is_some_and(|f| f.id == [7; 32]));
}

/// A scene without text writes no `fonts`, so every scene today keeps its bytes. A font's bytes are
/// never written, so a scene read back is refused until they come from the drawing font.
#[test]
fn font_bytes_never_go_through_json() {
    let plain = Scene::new(p(420.0, 297.0), vec![]);
    assert_eq!(serde_json::to_value(&plain).unwrap(), json!({"size_mm": [420.0, 297.0], "groups": []}));
    let scene = scene(vec![run()], vec![font()]);
    let value = serde_json::to_value(&scene).unwrap();
    assert!(value["fonts"][0].get("bytes").is_none() && value["fonts"][0]["id"] == json!(vec![7; 32]), "{value}");
    assert_eq!(value["groups"][0]["items"][0]["kind"], json!("text"));
    let mut read: Scene = serde_json::from_value(value.clone()).unwrap();
    assert!(read.fonts[0].bytes.is_empty());
    let refusal = read.validate().unwrap_err().0;
    assert!(refusal.starts_with("a font has its bytes"), "{refusal}");
    read.fonts[0].bytes = font().bytes;
    assert_eq!(read, scene);
    // Bytes given in the JSON are an unknown field, never read.
    let mut given = value;
    given["fonts"][0]["bytes"] = json!([0, 1, 0, 0]);
    assert!(serde_json::from_value::<Scene>(given).is_err());
}

#[test]
fn each_rule_is_refused() {
    let cases: Vec<(&str, Box<dyn FnOnce(&mut TextRun, &mut Font)>)> = vec![
        ("a run's font is in the scene's table", e(|r, _| r.font = [4; 32])),
        ("every glyph id is below its font's glyph count", e(|r, f| f.num_glyphs = r.glyphs[1].id)),
        ("a font's table holds exactly the glyphs its runs place", e(|_, f| drop(f.glyphs.remove(&2)))),
        ("a font's table holds exactly the glyphs its runs place", e(|_, f| drop(f.glyphs.insert(0, f.glyphs[&1].clone())))),
        ("a font has its bytes", e(|_, f| f.bytes = Arc::from(vec![]))),
        ("a font's PostScript name", e(|_, f| f.postscript_name = "Made Up".into())),
        ("a font's PostScript name", e(|_, f| f.postscript_name = "M".repeat(128))),
        ("a font's PostScript name", e(|_, f| f.postscript_name = String::new())),
        ("a font has glyphs, and 16 to 16384 units per em", e(|_, f| f.units_per_em = 15)),
        ("a font's cap height is above 0", e(|_, f| f.metrics.cap_height = 0)),
        ("a font's cap height is above 0", e(|_, f| f.metrics.italic_angle = f64::NAN)),
        (
            "a glyph's outline is lines and Bézier pieces",
            e(|_, f| {
                let arc = Seg2::Arc { center: p(0.0, 0.0), radius: 10.0, start: 0.0, sweep: TAU };
                f.glyphs.get_mut(&1).unwrap().outline = vec![Loop2 { segs: vec![arc] }];
            }),
        ),
        ("each loop of a glyph's outline is closed", e(|_, f| f.glyphs.get_mut(&1).unwrap().outline[0].segs.truncate(3))),
        ("each loop of a glyph's outline is closed", e(|_, f| f.glyphs.get_mut(&1).unwrap().outline[0].segs.clear())),
        ("a glyph's outline has at most 4096 pieces", e(|_, f| f.glyphs.get_mut(&1).unwrap().outline.push(ring(MAX_GLYPH_PIECES - 3)))),
        ("a run has 1 to 4000 glyphs", e(|r, _| r.glyphs.clear())),
        ("a run has 1 to 4000 glyphs", e(|r, _| r.text = "A".repeat(MAX_RUN_TEXT_BYTES + 1))),
        ("a run's cap height", e(|r, _| r.scale = 0.0)),
        ("a run's cap height", e(|r, _| r.scale = f64::NAN)),
        ("a run's cap height", e(|r, _| r.scale = 1e4)),
        ("a run starts within the paper's coordinates", e(|r, _| r.origin.x = 1e5 + 1.0)),
        ("a run turns at most a full turn", e(|r, _| r.angle = f64::INFINITY)),
        ("a run turns at most a full turn", e(|r, _| r.slant = 1.5)),
        ("a glyph's place and advance", e(|r, _| r.glyphs[1].rise = f64::NAN)),
        ("a run's clusters take up its glyphs in order", e(|r, _| r.clusters[1].glyphs = 2..2)),
        ("a run's clusters take up its glyphs in order", e(|r, _| r.clusters[1].text = 1..1)),
        ("a run's clusters take up its text in order", e(|r, _| (r.clusters[0].text, r.clusters[1].text) = (1..2, 0..1))),
        ("a run's clusters take up its text in order", e(|r, _| r.rtl = true)),
        ("a run's clusters cover all its glyphs and all its text", e(|r, _| r.clusters.truncate(1))),
        ("a run's clusters cover all its glyphs and all its text", e(|r, _| r.text = "ABC".into())),
    ];
    for (rule, edit) in cases {
        let refusal = refused(edit);
        assert!(refusal.starts_with(rule), "{rule}: {refusal}");
    }
    // At the bounds: 4096 pieces in all, 127-byte names, 16 units per em, a turn either way, a lean of 1.
    let at_bounds: Vec<Box<dyn FnOnce(&mut TextRun, &mut Font)>> = vec![
        e(|_, f| f.glyphs.get_mut(&1).unwrap().outline.push(ring(MAX_GLYPH_PIECES - 4))),
        e(|_, f| f.postscript_name = "M".repeat(127)),
        e(|_, f| f.units_per_em = 16),
        e(|r, _| (r.angle, r.slant) = (-TAU, -1.0)),
    ];
    for edit in at_bounds {
        assert_eq!(refused(edit), "valid");
    }
}

/// Fonts by id, once each, every one used; a run's characters whole, and right to left read
/// backwards along its glyphs.
#[test]
fn fonts_are_listed_once_and_used_and_text_runs_either_way() {
    let mut other = font();
    other.id = [1; 32];
    let refusal = |fonts| scene(vec![run()], fonts).validate().unwrap_err().0;
    assert_eq!(refusal(vec![font(), other.clone()]), "fonts are listed by id, each once");
    assert_eq!(refusal(vec![font(), font()]), "fonts are listed by id, each once");
    assert_eq!(refusal(vec![other, font()]), "every font in the table is used by a run");
    // "é" is two bytes: a cluster can't end between them.
    let accented =
        TextRun { text: "éB".into(), clusters: vec![Cluster { glyphs: 0..1, text: 0..2 }, Cluster { glyphs: 1..2, text: 2..3 }], ..run() };
    scene(vec![accented.clone()], vec![font()]).validate().unwrap();
    let split = TextRun { clusters: vec![Cluster { glyphs: 0..1, text: 0..1 }, Cluster { glyphs: 1..2, text: 1..3 }], ..accented };
    assert_eq!(scene(vec![split], vec![font()]).validate().unwrap_err().0, "a cluster's text is whole characters");
    // Right to left: the first glyph along the baseline carries the last character.
    let rtl = TextRun { rtl: true, clusters: vec![Cluster { glyphs: 0..1, text: 1..2 }, Cluster { glyphs: 1..2, text: 0..1 }], ..run() };
    scene(vec![rtl], vec![font()]).validate().unwrap();
    // One glyph for two characters (a ligature), and one character in two glyphs (a base and a mark).
    let ligature = TextRun { glyphs: vec![run().glyphs[0]], clusters: vec![Cluster { glyphs: 0..1, text: 0..2 }], ..run() };
    let mut only_one = font();
    only_one.glyphs.remove(&2);
    scene(vec![ligature], vec![only_one]).validate().unwrap();
    let marked = TextRun { text: "A".into(), clusters: vec![Cluster { glyphs: 0..2, text: 0..1 }], ..run() };
    scene(vec![marked], vec![font()]).validate().unwrap();
}

/// The outlines placed by hand: scale, then slant, then the glyph's place, then the turn about the
/// origin. Glyph 1's top right corner (500, 700) at 0.005 mm a unit is (2.5, 3.5); leaned by 0.2 it
/// moves 0.7 along; the second glyph starts 3 along and 1 up; turned a quarter about (20, 30).
#[test]
fn outlines_are_placed_on_the_sheet() {
    let run = TextRun {
        angle: FRAC_PI_2,
        slant: 0.2,
        glyphs: vec![PlacedGlyph { id: 1, x: 3.0, rise: 1.0, advance: 3.0 }],
        text: "A".into(),
        clusters: vec![Cluster { glyphs: 0..1, text: 0..1 }],
        ..run()
    };
    let font = font();
    let loops: Vec<Loop2> = run.outlines(&font).collect();
    assert_eq!(loops.len(), 1);
    // Along 3 + 2.5 + 0.7 = 6.2, up 1 + 3.5 = 4.5; a quarter turn takes (6.2, 4.5) to (-4.5, 6.2).
    let corner = loops[0].segs[2].start();
    assert!(corner.dist(p(20.0 - 4.5, 30.0 + 6.2)) < 1e-12, "{corner:?}");
    // The curved side keeps its kind and weight.
    let upright: Vec<Loop2> = self::run().outlines(&font).collect();
    assert!(matches!(upright[1].segs[2], Seg2::Conic { w, .. } if w == 1.0));
    assert!(upright[1].segs[2].start().dist(p(20.0 + 3.0, 30.0 + 3.5)) < 1e-12);
}

/// Groups laid out apart join their tables: each font once, with the glyphs of both, in order of
/// id whichever group came first.
#[test]
fn groups_fonts_are_joined_by_id() {
    let (mut a, mut b) = (font(), font());
    a.glyphs.remove(&2);
    b.glyphs.remove(&1);
    let mut other = font();
    other.id = [9; 32];
    let mut scene = Scene::new(p(420.0, 297.0), vec![]);
    scene.add_fonts([other, a]);
    scene.add_fonts([b]);
    assert_eq!(scene.fonts.iter().map(|f| f.id[0]).collect::<Vec<_>>(), [7, 9]);
    assert_eq!(scene.fonts[0], font());
}

/// A scene's fonts count toward its records with its groups: the run's scene is 6 records of group
/// and 12 of font, and a stroke beside it takes the rest up to the limit exactly.
#[test]
fn fonts_count_toward_the_records() {
    let filled = |total: usize| {
        let mut scene = scene(vec![run()], vec![font()]);
        let path = (0..total - 18 - 2).map(|i| Seg2::Line { a: p(i as f64 * 1e-3, 0.0), b: p((i + 1) as f64 * 1e-3, 0.0) }).collect();
        scene.groups.push(Group { object: 2, kind: ObjectKind::View, items: vec![Item::Stroke { path, closed: false, width_mm: 0.5 }] });
        scene.validate()
    };
    filled(crate::paper::MAX_RECORDS).unwrap();
    assert_eq!(filled(crate::paper::MAX_RECORDS + 1).unwrap_err().0, "more than 100,000 groups, items, loops and pieces");
}
