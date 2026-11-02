//! Text styles: alignment, letter and line spacing, a made-up italic, a bold the font lacks, and
//! text that follows a curve of its sketch; and text measured (`TextMetrics`) by the rule that draws it.

use solidcraft_geom::Vec2;

use super::*;
use crate::{CurveKind, Sketch, font_data};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// Every end and control point of the outlines.
fn points(g: &[LinkGeom]) -> Vec<Vec2> {
    g.iter()
        .flat_map(|x| match x {
            LinkGeom::Line(a, b) => vec![*a, *b],
            LinkGeom::Conic { a, apex, b, .. } => vec![*a, *apex, *b],
            LinkGeom::Spline { pts, .. } => pts.clone(),
            _ => vec![],
        })
        .collect()
}

/// The box around the outlines' end points: (min, max).
fn extent(g: &[LinkGeom]) -> (Vec2, Vec2) {
    let pts: Vec<Vec2> = g
        .iter()
        .flat_map(|x| match x {
            LinkGeom::Line(a, b) | LinkGeom::Conic { a, b, .. } => vec![*a, *b],
            _ => vec![],
        })
        .collect();
    let lo = pts.iter().fold(v(f64::MAX, f64::MAX), |m, p| v(m.x.min(p.x), m.y.min(p.y)));
    let hi = pts.iter().fold(v(f64::MIN, f64::MIN), |m, p| v(m.x.max(p.x), m.y.max(p.y)));
    (lo, hi)
}

fn styled(text: &str, style: &TextStyle, path: Option<&[Vec2]>) -> Vec<LinkGeom> {
    text_geometry_styled(None, text, Vec2::ZERO, 10.0, 0.0, style, path).unwrap()
}

#[test]
fn the_plain_style_is_the_text_as_before() {
    assert!(TextStyle::default().is_plain());
    assert_eq!(styled("Ab\nC", &TextStyle::default(), None), text_geometry("Ab\nC", Vec2::ZERO, 10.0, 0.0).unwrap());
}

#[test]
fn lines_align_left_center_or_right_on_the_anchor() {
    let left = extent(&styled("HH", &TextStyle::default(), None));
    let width = left.1.x - left.0.x;
    let center = extent(&styled("HH", &TextStyle { align: TextAlign::Center, ..TextStyle::default() }, None));
    let right = extent(&styled("HH", &TextStyle { align: TextAlign::Right, ..TextStyle::default() }, None));
    // H has the same side bearing both sides: centred, the ink is about the anchor; right, it ends left of it.
    assert!((center.0.x + center.1.x).abs() < 0.05, "{center:?}");
    assert!((right.1.x - right.0.x - width).abs() < 1e-9 && right.1.x < 0.0 && right.1.x > -2.0, "{right:?}");
    assert!((left.0.y - center.0.y).abs() < 1e-12, "alignment moves lines along, not up");
}

#[test]
fn letter_spacing_adds_between_letters_and_line_spacing_scales_the_lines() {
    let plain = extent(&styled("HHH", &TextStyle::default(), None));
    let spaced = extent(&styled("HHH", &TextStyle { letter_spacing: 2.5, ..TextStyle::default() }, None));
    assert!(((spaced.1.x - spaced.0.x) - (plain.1.x - plain.0.x) - 5.0).abs() < 1e-9, "two gaps of 2.5 mm");
    let one = extent(&styled("H\nH", &TextStyle::default(), None));
    let double = extent(&styled("H\nH", &TextStyle { line_spacing: 2.0, ..TextStyle::default() }, None));
    // The second line's baseline: one line distance below the first, then two.
    let gap = -one.0.y;
    assert!((-double.0.y - 2.0 * gap).abs() < 1e-9, "{gap} {double:?}");
    for bad in [TextStyle { line_spacing: 0.0, ..TextStyle::default() }, TextStyle { letter_spacing: f64::NAN, ..TextStyle::default() }] {
        assert!(text_geometry_styled(None, "H", Vec2::ZERO, 10.0, 0.0, &bad, None).is_err());
    }
}

#[test]
fn italic_without_an_italic_face_slants_and_bold_without_a_bold_face_is_an_error() {
    // The built-in font has neither: its I leans 12 degrees, its top moved right by the height times tan 12°.
    let upright = extent(&styled("I", &TextStyle::default(), None));
    let slanted = extent(&styled("I", &TextStyle { italic: true, ..TextStyle::default() }, None));
    assert!((slanted.1.x - upright.1.x - 10.0 * 12_f64.to_radians().tan()).abs() < 1e-6, "{upright:?} {slanted:?}");
    assert!((slanted.0.x - upright.0.x).abs() < 1e-9, "the baseline stays");
    let e = text_geometry_styled(None, "I", Vec2::ZERO, 10.0, 0.0, &TextStyle { bold: true, ..TextStyle::default() }, None).unwrap_err();
    assert!(e.to_string().contains("no bold face"), "{e}");
}

/// #8211: a variable font (one file for every weight, as most web fonts ship) has no bold face of
/// its own; Bold sets its weight axis to 700, so the strokes widen at the same height.
#[test]
fn bold_in_a_variable_font_sets_its_weight_axis() {
    let file = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf");
    let font = file.to_string_lossy().into_owned();
    let set = |style: &TextStyle| extent(&text_geometry_styled(Some(&font), "l", Vec2::ZERO, 10.0, 0.0, style, None).unwrap());
    let (plain, bold) = (set(&TextStyle::default()), set(&TextStyle { bold: true, ..TextStyle::default() }));
    let width = |(lo, hi): (Vec2, Vec2)| hi.x - lo.x;
    assert!(width(bold) > 1.25 * width(plain), "the stroke widens: {plain:?} {bold:?}");
    assert!((bold.1.y - plain.1.y).abs() < 0.05 * 10.0, "the same height: {plain:?} {bold:?}");
    let (data, slanted) = crate::font_data_styled(Some(&font), true, false).unwrap();
    assert_eq!((data.weight, slanted), (Some(700.0), false));
    // A font without a weight axis still has no bold to give.
    let e = text_geometry_styled(None, "l", Vec2::ZERO, 10.0, 0.0, &TextStyle { bold: true, ..TextStyle::default() }, None).unwrap_err();
    assert!(e.to_string().contains("no bold face"), "{e}");
}

#[test]
fn text_on_a_straight_path_stands_on_it_reading_left_to_right_whichever_way_it_was_drawn() {
    let plain = styled("Hi", &TextStyle::default(), None);
    let on = |path: &[Vec2], reverse: bool| styled("Hi", &TextStyle { path: Some("l".into()), reverse, ..TextStyle::default() }, Some(path));
    let same = |a: &[LinkGeom], b: &[LinkGeom]| points(a).iter().zip(points(b)).all(|(p, q)| p.dist(q) < 1e-9);
    // Along the x axis from the origin: the text as it is at the origin.
    assert!(same(&on(&[v(0.0, 0.0), v(100.0, 0.0)], false), &plain));
    // The same line drawn right to left still reads left to right, from its left end.
    assert!(same(&on(&[v(100.0, 0.0), v(0.0, 0.0)], false), &plain));
    // Reversed: right to left from the right end, upside down below the line.
    let (lo, hi) = extent(&on(&[v(0.0, 0.0), v(100.0, 0.0)], true));
    assert!(hi.y <= 1e-9 && lo.y < -5.0 && hi.x <= 100.0 + 1e-9 && lo.x > 80.0, "{lo:?} {hi:?}");
    // Centred on the path's middle, ended at its end.
    let c = extent(&styled(
        "HH",
        &TextStyle { path: Some("l".into()), align: TextAlign::Center, ..TextStyle::default() },
        Some(&[v(0.0, 0.0), v(100.0, 0.0)]),
    ));
    assert!(((c.0.x + c.1.x) / 2.0 - 50.0).abs() < 0.05, "{c:?}");
}

#[test]
fn text_on_a_circle_stands_outside_it_centred_on_top_and_inside_reversed() {
    let r = 30.0;
    let circle: Vec<Vec2> = (0..=360).map(|i| v(r * (i as f64).to_radians().cos(), r * (i as f64).to_radians().sin())).collect();
    let style = TextStyle { path: Some("c".into()), align: TextAlign::Center, ..TextStyle::default() };
    let outside = styled("TOP", &style, Some(&circle));
    // Every outline point is outside the circle (but for the O dipping below its baseline), and the text is about the top.
    assert!(points(&outside).iter().all(|p| p.len() >= r - 0.3), "outside");
    let (lo, hi) = extent(&outside);
    // (The ink is not quite symmetric: the P has a wide right side bearing.)
    assert!((lo.x + hi.x).abs() < 2.0 && lo.y > 0.0, "{lo:?} {hi:?}");
    let inside = styled("TOP", &TextStyle { reverse: true, ..style }, Some(&circle));
    assert!(points(&inside).iter().all(|p| p.len() <= r + 0.3), "inside");
}

#[test]
fn a_sketch_sets_its_text_on_one_of_its_curves() {
    let mut sk = Sketch::new();
    let a = sk.add_point(v(0.0, 0.0), None).unwrap();
    let b = sk.add_point(v(80.0, 0.0), None).unwrap();
    let line = sk.add_curve(CurveKind::Line { a, b }, None).unwrap();
    let id = sk.curves[line].id.clone();
    let source = |path: &str| LinkSource::Text {
        text: "Hi".into(),
        at: v(5.0, 5.0),
        height: 10.0,
        angle: 0.7,
        font: None,
        style: TextStyle { path: Some(path.into()), ..TextStyle::default() },
        exprs: Default::default(),
    };
    // On the line: `at` and the angle do not place it.
    let on = sk.text_source_geometry(&source(&id)).unwrap();
    assert_eq!(on, styled("Hi", &TextStyle { path: Some("x".into()), ..TextStyle::default() }, Some(&[v(0.0, 0.0), v(80.0, 0.0)])));
    let e = sk.text_source_geometry(&source("nope")).unwrap_err().to_string();
    assert!(e.contains("not a curve of this sketch"), "{e}");
    // A text cannot follow its own (or another text's) outline.
    let text = sk.add_link(LinkKind::Text, source(&id), &on).unwrap();
    let outline = sk.curves.iter().find(|c| c.link.as_deref() == Some(text.as_str())).unwrap().id.clone();
    let e = sk.text_source_geometry(&source(&outline)).unwrap_err().to_string();
    assert!(e.contains("outline of a text"), "{e}");
}

#[test]
fn a_style_is_saved_only_when_it_is_not_plain_and_reads_back() {
    let plain = LinkSource::Text {
        text: "A".into(),
        at: Vec2::ZERO,
        height: 5.0,
        angle: 0.0,
        font: None,
        style: TextStyle::default(),
        exprs: Default::default(),
    };
    let json = serde_json::to_value(&plain).unwrap();
    assert!(json.get("style").is_none(), "{json}");
    let styled = LinkSource::Text {
        text: "A".into(),
        at: Vec2::ZERO,
        height: 5.0,
        angle: 0.0,
        font: None,
        style: TextStyle {
            bold: true,
            italic: true,
            align: TextAlign::Right,
            letter_spacing: -0.5,
            line_spacing: 1.5,
            path: Some("l3".into()),
            reverse: true,
        },
        exprs: Default::default(),
    };
    let json = serde_json::to_value(&styled).unwrap();
    assert_eq!(json["style"]["align"], "right");
    assert_eq!(serde_json::from_value::<LinkSource>(json).unwrap(), styled);
    // Saved without a style (before styles): plain.
    let mut old = serde_json::to_value(&plain).unwrap();
    old.as_object_mut().unwrap().remove("style");
    assert_eq!(serde_json::from_value::<LinkSource>(old).unwrap(), plain);
}

/// What pins an outline exactly: how many pieces it has, its first and last point, and the sum of all its points.
fn pin(g: &[LinkGeom]) -> (usize, Vec2, Vec2, Vec2) {
    let pts = points(g);
    let sum = pts.iter().fold(Vec2::ZERO, |s, p| s + *p);
    (g.len(), pts.first().copied().unwrap_or_default(), pts.last().copied().unwrap_or_default(), sum)
}

#[test]
fn the_outlines_of_styled_texts_stay_exactly_as_they_were() {
    // Pinned before `TextMetrics` took over the layout (#4813): curves and a descender; spacing, a blank line, a made-up
    // italic and an angle; a path; glyphs the font lacks (`?`, and U+FFFF, which its character map sends to `.notdef`);
    // line breaks at both ends.
    let arc: Vec<Vec2> = (0..=90).map(|i| v(40.0 * f64::from(i).to_radians().cos(), 40.0 * f64::from(i).to_radians().sin())).collect();
    let spaced = TextStyle { italic: true, align: TextAlign::Center, letter_spacing: 0.75, line_spacing: 1.5, ..TextStyle::default() };
    let on_arc = TextStyle { align: TextAlign::Right, path: Some("arc".into()), ..TextStyle::default() };
    let missing = TextStyle { align: TextAlign::Right, letter_spacing: -0.5, ..TextStyle::default() };
    let cases: [(&str, Vec2, f64, f64, &TextStyle, Option<&[Vec2]>); 5] = [
        ("Hg", v(5.0, -3.0), 10.0, 0.0, &TextStyle::default(), None),
        ("Ab\n\nC", v(1.0, 2.0), 7.0, 0.3, &spaced, None),
        ("Hi", Vec2::ZERO, 5.0, 0.0, &on_arc, Some(&arc)),
        ("x\u{5b57}y\u{ffff}", v(-2.0, 0.5), 4.0, -1.0, &missing, None),
        ("\nH\n", Vec2::ZERO, 3.5, 0.0, &TextStyle::default(), None),
    ];
    let pins: Vec<_> = cases
        .iter()
        .map(|(text, at, height, angle, style, path)| pin(&text_geometry_styled(None, text, *at, *height, *angle, style, *path).unwrap()))
        .collect();
    assert_eq!(
        pins,
        [
            (45, v(12.955997161107167, -3.0), v(21.330731014904188, 0.8396025550035482), v(2070.968772178849, 45.508161816891416)),
            (58, v(0.235951166654778, 1.7636519993743809), v(11.036554267036463, -25.010425523555526), v(777.6105726380807, -898.3264755185471)),
            (20, v(39.918286168688034, 2.884686538489657), v(39.984307910534746, 1.1283251324010486), v(1709.968158829435, 122.02595165962421)),
            (66, v(-7.205346806960301, 8.606847326667703), v(-0.7406630833657615, 5.5426375007981274), v(-709.3625374614094, 1175.3199400202661)),
            (12, v(2.784599006387509, -5.849893541518807), v(2.784599006387509, -5.849893541518807), v(44.11639460610361, -97.83108587650818)),
        ]
    );
}

#[test]
fn on_a_closed_curve_left_alignment_starts_at_the_top() {
    let r = 30.0;
    let circle: Vec<Vec2> = (0..=360).map(|i| v(r * (i as f64).to_radians().cos(), r * (i as f64).to_radians().sin())).collect();
    let (lo, hi) = extent(&styled("START", &TextStyle { path: Some("c".into()), ..TextStyle::default() }, Some(&circle)));
    // It runs clockwise from the top: to the right of it, coming down.
    assert!(lo.x > -1.0 && hi.x > 10.0 && hi.y > r, "{lo:?} {hi:?}");
}

/// The built-in font, Liberation Sans, as its own tables give it: 2048 units per em, a capital height of 1409 units,
/// ascender 1854, descender -434 and line gap 67 (2355 units from baseline to baseline); `0` and `?` advance 1139
/// units, `H` 1479, `A` and `V` 1366. At a capital height of 1409 mm a unit is a millimetre.
const CAP: f64 = 1409.0;

#[test]
fn text_is_measured_by_the_fonts_own_tables() {
    let font = font_data(None).unwrap();
    let m = TextMetrics::new(&font, &TextStyle::default()).unwrap();
    assert_eq!((m.scale(CAP).unwrap(), m.line_pitch(CAP).unwrap()), (1.0, 2355.0));
    assert_eq!((m.line_width("0", CAP).unwrap(), m.line_width("H0", CAP).unwrap()), (1139.0, 1479.0 + 1139.0));
    assert_eq!(m.line_width("", CAP).unwrap(), 0.0);
    // No kerning: the font's own pair for A and V (152 units closer) is not used, in measured or drawn text.
    assert_eq!(m.line_width("AV", CAP).unwrap(), 2.0 * 1366.0);
    // At 3.5 mm, a parts list's text: 5.850 mm between lines, 2.829 mm a digit.
    assert!((m.line_pitch(3.5).unwrap() - 5.850).abs() < 5e-4 && (m.line_width("0", 3.5).unwrap() - 2.829).abs() < 5e-4);
}

#[test]
fn letter_spacing_goes_between_letters_only_and_line_spacing_scales_the_pitch() {
    let font = font_data(None).unwrap();
    let spaced = TextMetrics::new(&font, &TextStyle { letter_spacing: 2.5, line_spacing: 1.5, ..TextStyle::default() }).unwrap();
    // One letter has no gap, three have two.
    assert_eq!(spaced.line_width("0", CAP).unwrap(), 1139.0);
    assert_eq!(spaced.line_width("000", CAP).unwrap(), 3.0 * 1139.0 + 2.0 * 2.5);
    assert_eq!(spaced.line_pitch(CAP).unwrap(), 1.5 * 2355.0);
    // Closer than the letters are wide, each starts left of the one before and the line measures less than nothing.
    let tight = TextMetrics::new(&font, &TextStyle { letter_spacing: -2000.0, ..TextStyle::default() }).unwrap();
    assert_eq!(tight.line_width("000", CAP).unwrap(), 3.0 * 1139.0 - 2.0 * 2000.0);
}

#[test]
fn the_characters_a_font_lacks_are_named_in_order_each_time() {
    let font = font_data(None).unwrap();
    let m = TextMetrics::new(&font, &TextStyle::default()).unwrap();
    // It has no 字, tab or emoji; its character map sends U+FFFF to its missing-glyph box.
    assert_eq!(m.missing("a\u{5b57}b\t\u{5b57}\u{ffff}\u{1f600}c"), ['\u{5b57}', '\t', '\u{5b57}', '\u{ffff}', '\u{1f600}']);
    assert!(m.missing("").is_empty() && m.missing("Hello, w\u{f6}rld \u{20ac} 0?").is_empty());
    // Only characters it lacks: each is set, drawn and measured as `?`, which is why they are named.
    assert_eq!(m.line_width("\u{5b57}\u{5b57}", CAP).unwrap(), 2.0 * 1139.0);
    assert_eq!(styled("\u{5b57}", &TextStyle::default(), None), styled("?", &TextStyle::default(), None));
    // The box is drawn and measured as it is (1536 units), as before.
    assert_eq!(m.line_width("\u{ffff}", CAP).unwrap(), 1536.0);
}

#[test]
fn a_line_too_long_to_draw_is_still_measured() {
    // A hundred times the characters a text may have, in one pass.
    let font = font_data(None).unwrap();
    let m = TextMetrics::new(&font, &TextStyle::default()).unwrap();
    let long = "0".repeat(100 * MAX_TEXT_CHARS);
    assert!(matches!(text_geometry(&long, Vec2::ZERO, 1.0, 0.0), Err(SketchError::TooLarge)));
    assert_eq!(m.line_width(&long, CAP).unwrap(), 100_000.0 * 1139.0);
    assert!(m.missing(&long).is_empty());
}

#[test]
fn line_breaks_at_the_ends_are_empty_lines() {
    let font = font_data(None).unwrap();
    let m = TextMetrics::new(&font, &TextStyle::default()).unwrap();
    // Three lines: the H stands on the second baseline, one measured pitch below the first.
    assert_eq!(extent(&styled("\nH\n", &TextStyle::default(), None)).0.y, -m.line_pitch(10.0).unwrap());
    assert!(styled("\n", &TextStyle::default(), None).is_empty());
}

#[test]
fn sizes_and_spacings_a_text_refuses_are_refused_by_its_measure() {
    let font = font_data(None).unwrap();
    let m = TextMetrics::new(&font, &TextStyle::default()).unwrap();
    // A capital height above a micrometre and below a kilometre.
    for h in [0.0, -1.0, 1e-6, 1e6, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(m.scale(h).is_err() && m.line_pitch(h).is_err() && m.line_width("H", h).is_err(), "{h}");
        assert!(text_geometry("H", Vec2::ZERO, h, 0.0).is_err(), "{h}");
    }
    for h in [2e-6, 999_999.0] {
        let width = m.line_width(&"W".repeat(MAX_TEXT_CHARS), h).unwrap();
        assert!(width.is_finite() && width > 0.0 && m.line_pitch(h).unwrap().is_finite(), "{h}");
    }
    // The spacing a text accepts, at its bounds and just past them.
    for (letter_spacing, line_spacing, ok) in [
        (MAX_LETTER_SPACING, LINE_SPACING_RANGE.0, true),
        (-MAX_LETTER_SPACING, LINE_SPACING_RANGE.1, true),
        (MAX_LETTER_SPACING * (1.0 + 1e-12), 1.0, false),
        (f64::NAN, 1.0, false),
        (0.0, LINE_SPACING_RANGE.0 * (1.0 - 1e-12), false),
        (0.0, LINE_SPACING_RANGE.1 * (1.0 + 1e-12), false),
        (0.0, f64::INFINITY, false),
    ] {
        let style = TextStyle { letter_spacing, line_spacing, ..TextStyle::default() };
        assert_eq!(TextMetrics::new(&font, &style).is_ok(), ok, "{style:?}");
        assert_eq!(text_geometry_styled(None, "H", Vec2::ZERO, 10.0, 0.0, &style, None).is_ok(), ok, "{style:?}");
    }
}

#[test]
fn a_line_measures_what_it_draws() {
    // Right-aligned, a line ends at the anchor: its outline is the left-aligned one moved back by its measured width
    // (centred, by half of it). The next line stands one measured pitch below.
    for style in [TextStyle::default(), TextStyle { italic: true, letter_spacing: -0.4, line_spacing: 1.3, ..TextStyle::default() }] {
        let (font, _) = font_data_styled(None, style.bold, style.italic).unwrap();
        let m = TextMetrics::new(&font, &style).unwrap();
        for line in ["Hg", "W i d e ", "x\u{5b57}y\u{ffff}?", "0"] {
            let width = m.line_width(line, 10.0).unwrap();
            let left = points(&styled(line, &style, None));
            for (align, by) in [(TextAlign::Center, -width / 2.0), (TextAlign::Right, -width)] {
                let moved = points(&styled(line, &TextStyle { align, ..style.clone() }, None));
                let same = moved.len() == left.len() && left.iter().zip(&moved).all(|(p, q)| (*p + v(by, 0.0)).dist(*q) < 1e-9);
                assert!(same, "{line:?} {align:?} {style:?}");
            }
        }
        let two = points(&styled("Hg\nHg", &style, None));
        let (first, second) = two.split_at(two.len() / 2);
        let pitch = m.line_pitch(10.0).unwrap();
        assert!(first.iter().zip(second).all(|(p, q)| (*p - v(0.0, pitch)).dist(*q) < 1e-9), "{style:?}");
    }
}

#[test]
fn hostile_lines_are_measured_as_they_are_drawn_or_refused_alike() {
    // Characters the font has and lacks (字, a tab, U+FFFF, an emoji, NUL, a carriage return), a lone combining accent,
    // sizes and spacings at and past their bounds: never a panic, the measure refuses what the text refuses, and what
    // it measures is what is drawn.
    let chars = ['H', 'g', ' ', '?', '\u{e9}', '\u{301}', '\u{5b57}', '\t', '\u{ffff}', '\u{1f600}', '\0', '\r'];
    let lacks = ['\u{5b57}', '\t', '\u{ffff}', '\u{1f600}', '\0', '\r'];
    let heights = [0.0, -1.0, 1e-6, 2e-6, 0.5, 10.0, 999_999.0, 1e6, f64::NAN, f64::INFINITY];
    let letter = [0.0, -0.5, 3.0, -1e5, 1e5, 1.5e5, f64::NAN];
    let lines = [1.0, 0.05, 100.0, 0.04, 101.0, f64::INFINITY];
    let mut rnd = crate::tests::Lcg(4813);
    let mut pick = |n: usize| ((rnd.next() * n as f64) as usize).min(n - 1);
    let font = font_data(None).unwrap();
    for _ in 0..1000 {
        let line: String = (0..pick(10)).map(|_| chars[pick(chars.len())]).collect();
        let height = heights[pick(heights.len())];
        let style = TextStyle { letter_spacing: letter[pick(letter.len())], line_spacing: lines[pick(lines.len())], ..TextStyle::default() };
        let measured = TextMetrics::new(&font, &style).and_then(|m| Ok((m.line_width(&line, height)?, m.line_pitch(height)?, m.missing(&line))));
        let right_aligned = TextStyle { align: TextAlign::Right, ..style.clone() };
        let drawn = [&style, &right_aligned].map(|s| text_geometry_styled(None, &line, Vec2::ZERO, height, 0.0, s, None));
        let (Ok((width, pitch, missing)), [Ok(left), Ok(right)]) = (&measured, &drawn) else {
            assert!(measured.is_err() && drawn.iter().all(|d| d.is_err()), "{line:?} {height} {style:?}");
            continue;
        };
        assert!(width.is_finite() && pitch.is_finite() && *pitch > 0.0, "{line:?} {height} {style:?}");
        assert_eq!(*missing, line.chars().filter(|c| lacks.contains(c)).collect::<Vec<_>>());
        let (left, right, tol) = (points(left), points(right), 1e-12 * (1.0 + width.abs() + 2.0 * height));
        assert!(
            left.len() == right.len() && left.iter().zip(&right).all(|(p, q)| (*p - v(*width, 0.0)).dist(*q) <= tol),
            "{line:?} {height} {style:?}"
        );
    }
}
