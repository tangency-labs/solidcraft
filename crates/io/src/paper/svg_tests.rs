use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcraft_geom::paper::{Group, Item, ObjectKind, Scene};
use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};

use super::*;
use crate::{Geom2, read_svg};

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn line(a: Vec2, b: Vec2) -> Seg2 {
    Seg2::Line { a, b }
}

fn stroke(path: Vec<Seg2>, closed: bool, width_mm: f64) -> Item {
    Item::Stroke { path, closed, width_mm }
}

/// Anticlockwise from `min`.
fn rectangle(min: Vec2, max: Vec2) -> Loop2 {
    Loop2::polygon(&[min, p(max.x, min.y), max, p(min.x, max.y)])
}

/// An A3 landscape sheet with one object's items.
fn sheet(items: Vec<Item>) -> Scene {
    Scene::new(p(420.0, 297.0), vec![Group { object: 1, kind: ObjectKind::DrawingBorder, items }])
}

fn svg(scene: &Scene) -> (String, SvgReport) {
    let (bytes, report) = paper_svg(&scene.prepare().unwrap()).unwrap();
    (String::from_utf8(bytes).unwrap(), report)
}

/// The `d` of the document's first path.
fn first_d(scene: &Scene) -> String {
    let (text, _) = svg(scene);
    let start = text.find(" d=\"").unwrap() + 4;
    text[start..].split('"').next().unwrap().to_string()
}

fn arc(center: Vec2, radius: f64, start: f64, sweep: f64) -> Seg2 {
    Seg2::Arc { center, radius, start, sweep }
}

/// The plan's A3 sheet, the whole document: a drawing border closed by `Z`, two trimming fills.
#[test]
fn an_a3_drawing_border_is_this_document() {
    let scene = sheet(vec![
        stroke(rectangle(p(20.0, 10.0), p(410.0, 287.0)).segs, true, 0.7),
        Item::Fill { region: Region2 { outer: rectangle(p(0.0, 0.0), p(10.0, 5.0)), holes: vec![] } },
        Item::Fill { region: Region2 { outer: rectangle(p(0.0, 0.0), p(5.0, 10.0)), holes: vec![] } },
    ]);
    let (text, report) = svg(&scene);
    let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
        <svg xmlns=\"http://www.w3.org/2000/svg\" width=\"420mm\" height=\"297mm\" viewBox=\"0 0 420 297\" fill=\"none\" stroke=\"#000\" \
        stroke-linecap=\"butt\" stroke-linejoin=\"miter\" stroke-miterlimit=\"4\">\n\
        <g data-object=\"1\" data-kind=\"drawing_border\">\n\
        <path stroke-width=\"0.7\" d=\"M20 287L410 287L410 10L20 10Z\"/>\n\
        <path fill=\"#000\" stroke=\"none\" fill-rule=\"evenodd\" d=\"M0 297L10 297L10 292L0 292Z\"/>\n\
        <path fill=\"#000\" stroke=\"none\" fill-rule=\"evenodd\" d=\"M0 297L5 297L5 287L0 287Z\"/>\n\
        </g>\n\
        </svg>\n";
    assert_eq!(text, expected);
    assert_eq!(report, SvgReport::default());
    assert_eq!(svg(&scene).0, text, "the same scene gives the same bytes");
}

/// Arcs: anticlockwise on paper is sweep flag 0 after the flip, clockwise 1; at most a quarter turn
/// a piece, so a 270° arc is three and a whole turn four, large-arc never 1.
#[test]
fn arcs_are_written_in_quarter_turns_with_the_flipped_sweep() {
    let c = p(50.0, 50.0);
    let open = |seg: Seg2| sheet(vec![stroke(vec![seg], false, 0.35)]);
    assert_eq!(first_d(&open(arc(c, 10.0, 0.0, FRAC_PI_2))), "M60 247A10 10 0 0 0 50 237");
    assert_eq!(first_d(&open(arc(c, 10.0, 0.0, 1.5 * PI))), "M60 247A10 10 0 0 0 50 237A10 10 0 0 0 40 247A10 10 0 0 0 50 257");
    assert_eq!(first_d(&open(arc(c, 10.0, FRAC_PI_2, -FRAC_PI_2))), "M50 237A10 10 0 0 1 60 247", "clockwise: sweep flag 1");
    let circle = first_d(&sheet(vec![stroke(vec![arc(c, 10.0, 0.0, TAU)], true, 0.35)]));
    assert_eq!(circle, "M60 247A10 10 0 0 0 50 237A10 10 0 0 0 40 247A10 10 0 0 0 50 257A10 10 0 0 0 60 247Z");
    let halves = first_d(&sheet(vec![stroke(Loop2::circle(c, 10.0).segs, true, 0.35)]));
    assert_eq!(halves, circle, "a circle of two halves is the same four quarters");
    // Just over a quarter is two pieces; a quarter and a hair (the 1e-9 margin) is one.
    assert_eq!(first_d(&open(arc(c, 10.0, 0.0, FRAC_PI_2 + 1e-12))).matches('A').count(), 1);
    assert_eq!(first_d(&open(arc(c, 10.0, 0.0, FRAC_PI_2 + 1e-6))).matches('A').count(), 2);
}

/// Cubics as `C`, parabolas as `Q`; ellipses, hyperbolas and higher Béziers refused until A3b.
#[test]
fn cubics_and_parabolas_are_written_and_other_conics_refused() {
    let cubic = Seg2::Cubic { p0: p(0.0, 0.0), p1: p(10.0, 20.0), p2: p(30.0, 20.0), p3: p(40.0, 0.0) };
    assert_eq!(first_d(&sheet(vec![stroke(vec![cubic], false, 0.25)])), "M0 297C10 277 30 277 40 297");
    let parabola = Seg2::Conic { a: p(0.0, 0.0), apex: p(10.0, 10.0), b: p(20.0, 0.0), w: 1.0 };
    assert_eq!(first_d(&sheet(vec![stroke(vec![parabola], false, 0.25)])), "M0 297Q10 287 20 297");
    for w in [0.5, 2.0] {
        let conic = Seg2::Conic { a: p(0.0, 0.0), apex: p(10.0, 10.0), b: p(20.0, 0.0), w };
        let scene = sheet(vec![stroke(vec![conic], false, 0.25)]);
        let e = paper_svg(&scene.prepare().unwrap()).err().map(|e| e.to_string()).unwrap();
        assert!(e.contains("not supported yet"), "{w}: {e}");
    }
}

/// Numbers are whole nanometres: no exponent, no "-0", trailing zeros trimmed; a width under half
/// a nanometre is written as one and reported (the grid's own rounding is `geom`'s test).
#[test]
fn numbers_are_whole_nanometres() {
    for (n, text) in [(0, "0"), (1, "0.000001"), (-1, "-0.000001"), (1_500_000, "1.5"), (-420_000_000, "-420"), (123_456_789, "123.456789")] {
        assert_eq!(number(n), text);
    }
    let thin = sheet(vec![stroke(vec![line(p(0.0, 0.0), p(1.0, 0.0))], false, 1e-9)]);
    let (text, report) = svg(&thin);
    assert!(text.contains("stroke-width=\"0.000001\""), "{text}");
    assert_eq!(report.widths_snapped, 1);
}

/// A closed path ends exactly on its first point, even when its end and start round apart; a
/// sliver of a piece is merged into the next and reported; an open path's last piece is kept
/// however short, since nothing follows it.
#[test]
fn joints_and_closure_are_exact_on_the_grid() {
    // The start rounds to 1 nanometre, the end (2e-10 away) to 0: the last curve ends on the start.
    let (s, e) = (p(5e-7 + 1e-10, 0.0), p(5e-7 - 1e-10, 0.0));
    let path =
        vec![line(s, p(10.0, 0.0)), line(p(10.0, 0.0), p(10.0, 10.0)), Seg2::Cubic { p0: p(10.0, 10.0), p1: p(5.0, 10.0), p2: p(0.0, 5.0), p3: e }];
    let d = first_d(&sheet(vec![stroke(path, true, 0.35)]));
    assert_eq!(d, "M0.000001 297L10 297L10 287C5 287 0 292 0.000001 297Z");
    // A 1e-9 mm line between two lines: merged, the path straight on to the next point.
    let path = vec![line(p(0.0, 0.0), p(10.0, 0.0)), line(p(10.0, 0.0), p(10.0, 1e-9)), line(p(10.0, 1e-9), p(15.0, 8.66))];
    let (text, report) = svg(&sheet(vec![stroke(path, false, 0.35)]));
    assert!(text.contains("d=\"M0 297L10 297L15 288.34\""), "{text}");
    assert_eq!(report.pieces_merged, 1);
    // A 5 nanometre last piece of an open path stays; one that rounds to no length doesn't.
    let path = vec![line(p(0.0, 0.0), p(10.0, 0.0)), line(p(10.0, 0.0), p(10.000005, 0.0))];
    let (text, report) = svg(&sheet(vec![stroke(path, false, 0.35)]));
    assert!(text.contains("d=\"M0 297L10 297L10.000005 297\""), "{text}");
    assert_eq!(report.pieces_merged, 0);
    let path = vec![line(p(0.0, 0.0), p(10.0, 0.0)), line(p(10.0, 0.0), p(10.0000001, 0.0))];
    assert_eq!(first_d(&sheet(vec![stroke(path, false, 0.35)])), "M0 297L10 297");
    // The same for an arc's last quarter: a 5 nanometre arc ending an open path is written.
    let path = vec![line(p(0.0, 0.0), p(10.0, 0.0)), arc(p(10.0, 1.0), 1.0, -FRAC_PI_2, 5e-6)];
    assert!(first_d(&sheet(vec![stroke(path, false, 0.35)])).starts_with("M0 297L10 297A1 1 0 0 0 10.000005 297"));
}

/// Our own SVG reader takes the file back: in millimetres, lines exactly (these have at most six
/// decimals), circular arcs as arcs. The reader's root transform puts the page's top at 0, so its
/// y is ours less the height.
#[test]
fn the_svg_reader_reads_it_back() {
    let scene = sheet(vec![
        stroke(vec![line(p(20.0, 10.0), p(410.0, 10.0)), line(p(410.0, 10.0), p(410.0, 287.0))], false, 0.7),
        stroke(vec![arc(p(50.0, 50.0), 10.0, 0.0, FRAC_PI_2)], false, 0.35),
    ]);
    let (text, _) = svg(&scene);
    let read = read_svg(&text).unwrap();
    let up = |v: Vec2| p(v.x, v.y + 297.0);
    let lines: Vec<(Vec2, Vec2)> = read.iter().filter_map(|g| if let Geom2::Line(a, b) = g { Some((up(*a), up(*b))) } else { None }).collect();
    assert_eq!(lines, [(p(20.0, 10.0), p(410.0, 10.0)), (p(410.0, 10.0), p(410.0, 287.0))]);
    let arcs: Vec<(Vec2, Vec2, Vec2)> =
        read.iter().filter_map(|g| if let Geom2::Arc { c, a, b } = g { Some((up(*c), up(*a), up(*b))) } else { None }).collect();
    assert_eq!(arcs.len(), 1, "{read:?}");
    let (c, a, b) = arcs[0];
    assert!(c.dist(p(50.0, 50.0)) < 3e-6 && a.dist(p(60.0, 50.0)) < 3e-6 && b.dist(p(50.0, 60.0)) < 3e-6, "{:?}", arcs[0]);
}

/// Text isn't written yet (B2): a scene with a run is refused by name, never written without it.
#[test]
fn text_is_refused_by_name_until_the_writer_learns_it() {
    use solidcraft_geom::paper::text::{Cluster, Font, FontFlags, FontGlyph, FontMetrics, PlacedGlyph, TextRun};
    let metrics = FontMetrics {
        ascent: 900,
        descent: -200,
        cap_height: 700,
        bbox: [0, 0, 500, 700],
        italic_angle: 0.0,
        flags: FontFlags::default(),
        stem_v: 80,
    };
    let glyph = FontGlyph { advance: 600, outline: vec![rectangle(p(0.0, 0.0), p(500.0, 700.0))] };
    let font = Font {
        id: [1; 32],
        bytes: std::sync::Arc::from(vec![1u8]),
        face: 0,
        postscript_name: "MadeUp".into(),
        num_glyphs: 2,
        units_per_em: 1000,
        metrics,
        glyphs: [(1, glyph)].into(),
    };
    let run = TextRun {
        font: [1; 32],
        scale: 0.005,
        origin: p(20.0, 30.0),
        angle: 0.0,
        slant: 0.0,
        glyphs: vec![PlacedGlyph { id: 1, x: 0.0, rise: 0.0, advance: 3.0 }],
        text: "A".into(),
        clusters: vec![Cluster { glyphs: 0..1, text: 0..1 }],
        rtl: false,
    };
    let scene = Scene { fonts: vec![font], ..sheet(vec![Item::Text(run)]) };
    let refusal = paper_svg(&scene.prepare().unwrap()).unwrap_err().to_string();
    assert!(refusal.contains("not supported yet: text in an SVG"), "{refusal}");
}
