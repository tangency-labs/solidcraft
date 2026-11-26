use solidcraft_geom::prelude::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_doc::canvas::base64_decode;
use solidcraft_geom::Aabb2;
use solidcraft_geom::paper::{Clip, Dashes, Group, Item, NoPictures, ObjectKind, Picture, Pictures, Scene};
use solidcraft_geom::pixel_grid::GridWindow;

use super::*;
use crate::paper::png::tests::{Image, Provider, read};
use crate::paper::{PngColour, PngOptions, paper_png};
use crate::{Geom2, read_svg};

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn line(a: Vec2, b: Vec2) -> Seg2 {
    Seg2::Line { a, b }
}

fn stroke(path: Vec<Seg2>, closed: bool, width_mm: f64) -> Item {
    Item::Stroke { path, closed, width_mm, pattern: None }
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
    let (bytes, report) = paper_svg(&scene.prepare().unwrap(), &NoPictures, 300.0).unwrap();
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

/// Cubics as `C`, parabolas as `Q`; ellipses, hyperbolas and higher Béziers as cubics (A3b).
#[test]
fn cubics_and_parabolas_are_written_and_other_curves_as_cubics() {
    let cubic = Seg2::Cubic { p0: p(0.0, 0.0), p1: p(10.0, 20.0), p2: p(30.0, 20.0), p3: p(40.0, 0.0) };
    assert_eq!(first_d(&sheet(vec![stroke(vec![cubic], false, 0.25)])), "M0 297C10 277 30 277 40 297");
    let parabola = Seg2::Conic { a: p(0.0, 0.0), apex: p(10.0, 10.0), b: p(20.0, 0.0), w: 1.0 };
    assert_eq!(first_d(&sheet(vec![stroke(vec![parabola], false, 0.25)])), "M0 297Q10 287 20 297");
    let mut controls = [p(0.0, 0.0); solidcore_record::MAX_BEZIER_CTRL];
    controls[..5].copy_from_slice(&[p(0.0, 0.0), p(5.0, 10.0), p(10.0, 0.0), p(15.0, 10.0), p(20.0, 0.0)]);
    let quartic = Seg2::Bezier { n: 4, p: controls };
    for curve in [0.5, 2.0].map(|w| Seg2::Conic { a: p(0.0, 0.0), apex: p(10.0, 10.0), b: p(20.0, 0.0), w }).into_iter().chain([quartic]) {
        let d = first_d(&sheet(vec![stroke(vec![curve], false, 0.25)]));
        assert!(d.starts_with("M0 297C") && d.ends_with(" 20 297") && !d.contains('Q') && !d.contains('A'), "{curve:?}: {d}");
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

/// Text is its glyph outlines, one path a run, filled black with SVG's default nonzero rule.
#[test]
fn text_is_its_outlines_filled_black_nonzero() {
    use solidcraft_geom::paper::text::{Cluster, Embed, Font, FontFlags, FontGlyph, FontMetrics, PlacedGlyph, TextRun};
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
        family: "Made Up".into(),
        embed: Embed::Subset,
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
    let outlines: Vec<Loop2> = run.outlines(&font).collect();
    let scene = Scene { fonts: vec![font], ..sheet(vec![Item::Text(run)]) };
    let (text, _) = svg(&scene);
    assert_eq!(text.matches("<path fill=\"#000\" stroke=\"none\" d=\"").count(), 1, "{text}");
    // The same path a fill of the placed outlines gets, without its even-odd rule.
    let (outer, holes) = outlines.split_first().unwrap();
    let filled = sheet(vec![Item::Fill { region: Region2 { outer: outer.clone(), holes: holes.to_vec() } }]);
    assert_eq!(first_d(&scene), first_d(&filled));
}

/// Real text in the drawing font: a dimension's symbols, and a line read right to left, each run one
/// path holding a closed loop for each of its glyph loops.
#[test]
fn symbols_and_a_right_to_left_line_are_written() {
    use solidcraft_sketch::{TextStyle, paper_text};
    for line in ["Ø12 ±0.1 30°", "קו 12"] {
        let (runs, fonts) = paper_text(line, None, "", p(20.0, 30.0), 3.5, 0.0, &TextStyle::default()).unwrap().unwrap();
        let loops: usize = runs.iter().map(|r| r.outlines(&fonts[0]).count()).sum();
        let count = runs.len();
        let scene = Scene { fonts, ..sheet(runs.into_iter().map(Item::Text).collect()) };
        let (text, _) = svg(&scene);
        assert_eq!(text.matches("<path fill=\"#000\" stroke=\"none\" d=\"").count(), count, "{line}");
        assert!(loops > 0 && text.matches('M').count() == loops && text.matches('Z').count() == loops, "{line}: {text}");
    }
}

fn picture(source: u64, min: Vec2, max: Vec2, opacity: f64) -> Item {
    Item::Picture(Picture { source, rect: Aabb2 { min, max }, opacity })
}

fn pictured(scene: &Scene, dpi: f64, pictures: &Provider) -> String {
    String::from_utf8(paper_svg(&scene.prepare().unwrap(), pictures, dpi).unwrap().0).unwrap()
}

/// Source 1's picture (all red) in `rect`, its object cut to `clip`, on a 30 × 20 mm sheet when
/// `small`, or on the A3 sheet.
fn clipped_picture(clip: Clip, rect: Aabb2, small: bool) -> Scene {
    let size = if small { p(30.0, 20.0) } else { p(420.0, 297.0) };
    let group = Group { object: 1, kind: ObjectKind::View, items: vec![picture(1, rect.min, rect.max, 1.0)] };
    Scene { clips: vec![(1, clip)], ..Scene::new(size, vec![group]) }
}

/// A picture crossing its clip holds only the pixels that meet the clip's box and is cut by a
/// `<clipPath>` on the lines' grid, y flipped as they are; each has its own id, and a picture wholly
/// inside needs none. 254 dpi: 10 pixels a millimetre.
#[test]
fn a_picture_crossing_its_clip_is_cut_by_a_clip_path() {
    let rect = Aabb2 { min: p(10.0, 10.0), max: p(30.0, 30.0) };
    let cut = Clip::Rect { min: p(15.0, 12.0), max: p(25.0, 18.0) };
    let provider = Provider::default();
    let text = pictured(&clipped_picture(cut, rect, false), 254.0, &provider);
    assert!(text.contains("<clipPath id=\"clip1\"><rect x=\"15\" y=\"279\" width=\"10\" height=\"6\"/></clipPath>\n<image "), "{text:.400}");
    assert_eq!(images(&text), ["<image x=\"15\" y=\"279\" width=\"10\" height=\"6\" preserveAspectRatio=\"none\" clip-path=\"url(#clip1)\""]);
    assert_eq!(provider.asked.borrow()[0].2, GridWindow { x0: 50, y0: 120, x1: 150, y1: 180 });

    let round = Clip::Circle { center: p(20.0, 20.0), radius: 5.0 };
    let mut two = clipped_picture(round, rect, false);
    two.groups.push(Group { object: 2, kind: ObjectKind::View, items: vec![picture(1, p(100.0, 10.0), p(120.0, 30.0), 1.0)] });
    two.groups.push(Group { object: 3, kind: ObjectKind::View, items: vec![picture(1, p(200.0, 10.0), p(220.0, 30.0), 1.0)] });
    two.clips.push((2, Clip::Circle { center: p(110.0, 20.0), radius: 5.5 }));
    two.clips.push((3, Clip::Rect { min: p(190.0, 0.0), max: p(230.0, 40.0) }));
    let text = pictured(&two, 254.0, &Provider::default());
    assert!(text.contains("<clipPath id=\"clip1\"><circle cx=\"20\" cy=\"277\" r=\"5\"/></clipPath>"), "{text:.400}");
    assert!(text.contains("<clipPath id=\"clip2\"><circle cx=\"110\" cy=\"277\" r=\"5.5\"/></clipPath>"), "{text:.400}");
    assert_eq!(text.matches("<clipPath").count(), 2, "the third lies wholly inside");
    assert_eq!(images(&text).iter().filter(|image| image.contains("clip-path")).count(), 2);
}

/// How far `q` is inside `clip`, worked out apart from the writers: below 0 outside.
fn depth(clip: &Clip, q: Vec2) -> f64 {
    match *clip {
        Clip::Rect { min, max } => (q.x - min.x).min(max.x - q.x).min(q.y - min.y).min(max.y - q.y),
        Clip::Circle { center, radius } => radius - q.dist(center),
    }
}

/// Pixels a millimetre in [`check_pixels`].
const PER_MM: f64 = 5.0;

/// A sheet PNG's pixels: each with its middle inside both the picture and the clip is the picture,
/// and each outside either is paper, one pixel's slack at the picture's edge as its grid's own
/// pixels may reach past it.
fn check_pixels(case: usize, scene: &Scene, clip: &Clip, rect: Aabb2) {
    let provider = Provider::default();
    let mut out = Vec::new();
    paper_png(&scene.prepare().unwrap(), PngOptions { dpi: PER_MM * 25.4, colour: PngColour::Rgb }, &provider, &mut out).unwrap();
    let image = read(&out);
    let (width, height) = (image.info.width as usize, image.info.height as usize);
    for y in 0..height {
        for x in 0..width {
            let q = p((x as f64 + 0.5) / PER_MM, scene.size_mm.y - (y as f64 + 0.5) / PER_MM);
            let in_picture = (q.x - rect.min.x).min(rect.max.x - q.x).min(q.y - rect.min.y).min(rect.max.y - q.y);
            let pixel = image.at(x, y);
            if depth(clip, q) > 1e-6 && in_picture > 0.0 {
                assert_eq!(pixel, [255, 0, 0], "case {case}: ({x}, {y}) is inside {clip:?} and {rect:?}");
            } else if depth(clip, q) < -1e-6 || in_picture < -1.0 / PER_MM {
                assert_eq!(pixel, [255, 255, 255], "case {case}: ({x}, {y}) is outside {clip:?} or {rect:?}");
            }
        }
    }
    check_asked(case, &provider, clip);
}

/// Every pixel `provider` was asked for meets the clip's box: a small cut of a large picture costs
/// only that small cut.
fn check_asked(case: usize, provider: &Provider, clip: &Clip) {
    let bounds = clip.bounds();
    for (_, grid, w) in provider.asked.borrow().iter() {
        let (left, right) = (grid.column(w.x0).0, grid.column(w.x1 - 1).1);
        let (low, high) = (grid.row(w.y1 - 1).0, grid.row(w.y0).1);
        let pitch = grid.column(0).1 - grid.column(0).0;
        let reach = pitch + 1e-6;
        assert!(
            left >= bounds.min.x - reach && right <= bounds.max.x + reach && low >= bounds.min.y - reach && high <= bounds.max.y + reach,
            "case {case}: {w:?} past {clip:?}"
        );
    }
}

/// Clips of every shape over a picture, through both writers: rectangles and circles at random, tiny
/// ones, ones reaching off the sheet, ones round the whole picture, ones only touching it and ones
/// clear of it. The PNG's pixels are exactly the picture inside the clip, and the SVG has a
/// `<clipPath>` only when the clip crosses the picture, no image when it misses, and no more pixels
/// than the clip's box needs.
#[test]
fn fuzzed_clips_cut_pictures_in_both_writers() {
    let mut seed = 0xa8c_u64;
    let mut r = |lo: f64, hi: f64| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        lo + (hi - lo) * ((seed >> 11) as f64 / (1u64 << 53) as f64)
    };
    for case in 0..240 {
        let (x, y) = (r(0.0, 20.0), r(0.0, 12.0));
        let rect = Aabb2 { min: p(x, y), max: p(x + r(1.0, 10.0), y + r(1.0, 8.0)) };
        let (w, h) = (rect.max.x - rect.min.x, rect.max.y - rect.min.y);
        let (cx, cy) = (r(rect.min.x, rect.max.x), r(rect.min.y, rect.max.y));
        let clip = match case % 8 {
            0 => Clip::Rect { min: p(cx - r(0.1, 8.0), cy - r(0.1, 8.0)), max: p(cx + r(0.1, 8.0), cy + r(0.1, 8.0)) },
            1 => Clip::Circle { center: p(cx, cy), radius: r(0.1, 8.0) },
            2 => Clip::Rect { min: p(cx, cy), max: p(cx + 1e-3, cy + 1e-3) },
            3 => Clip::Circle { center: p(cx, cy), radius: 1e-3 },
            // Reaching off the sheet, past its left and bottom.
            4 => Clip::Circle { center: p(-1.0, -1.0), radius: r(2.0, 25.0) },
            // Round the whole picture.
            5 => Clip::Circle { center: rect.center(), radius: (w + h) * r(1.0, 2.0) },
            // Touching the picture's right edge from outside.
            6 => Clip::Rect { min: p(rect.max.x, rect.min.y), max: p(rect.max.x + 3.0, rect.max.y) },
            // Clear of it, though a circle's box may overlap: the corner's distance plus a little.
            _ => {
                let radius = r(0.5, 3.0);
                let centre = rect.max + Vec2::new(radius, radius) * r(0.71, 0.99);
                Clip::Circle { center: centre, radius }
            }
        };
        let scene = clipped_picture(clip, rect, true);
        check_pixels(case, &scene, &clip, rect);

        let provider = Provider::default();
        let text = pictured(&scene, 300.0, &provider);
        check_asked(case, &provider, &clip);
        let corners = [rect.min, p(rect.max.x, rect.min.y), rect.max, p(rect.min.x, rect.max.y)];
        let whole = corners.iter().all(|corner| depth(&clip, *corner) >= 0.0);
        let clipped_paths = text.matches("<clipPath").count();
        assert_eq!(clipped_paths, images(&text).iter().filter(|image| image.contains("clip-path")).count(), "case {case}");
        assert!(clipped_paths <= 1, "case {case}");
        if whole {
            assert_eq!(clipped_paths, 0, "case {case}: {clip:?} holds all of {rect:?}");
        } else if case % 8 == 0 || case % 8 == 1 {
            // Their middle lies on the picture, so an image shows it unless it's under one pixel.
            assert_eq!(clipped_paths, 1, "case {case}: {clip:?} crosses {rect:?}");
        }
        let nearest = p(clip.bounds().center().x.clamp(rect.min.x, rect.max.x), clip.bounds().center().y.clamp(rect.min.y, rect.max.y));
        if depth(&clip, nearest) < -1e-6 {
            assert!(images(&text).is_empty() && provider.asked.borrow().is_empty(), "case {case}: {clip:?} misses {rect:?}");
        }
    }
}

/// The cut follows the sheet's own pixels: at 2400 dpi a picture is asked for at half that, and the
/// clip's edge still falls between neighbouring sheet pixels. Paper stays paper in every colour: a
/// transparent sheet keeps no alpha outside.
#[test]
fn a_clip_cuts_at_the_sheets_pixels_in_every_colour() {
    let rect = Aabb2 { min: p(0.0, 0.0), max: p(4.0, 4.0) };
    // A 94.488 px/mm grid: the edge at x = 1.005 lies in column 94's second half, so 94 is out and 95 is in.
    let clip = Clip::Rect { min: p(1.005, 1.0), max: p(3.0, 3.0) };
    let group = Group { object: 1, kind: ObjectKind::View, items: vec![picture(1, rect.min, rect.max, 1.0)] };
    let scene = Scene { clips: vec![(1, clip)], ..Scene::new(p(4.0, 4.0), vec![group]) };
    let write = |dpi: f64, colour: PngColour, provider: &Provider| {
        let mut out = Vec::new();
        paper_png(&scene.prepare().unwrap(), PngOptions { dpi, colour }, provider, &mut out).unwrap();
        read(&out)
    };
    let row = 189;
    let provider = Provider::default();
    let fine = write(2400.0, PngColour::Rgb, &provider);
    assert_eq!((fine.at(94, row), fine.at(95, row)), (&[255, 255, 255][..], &[255, 0, 0][..]));
    assert!(provider.asked.borrow().iter().all(|(_, grid, _)| (grid.column(0).1 - grid.column(0).0 - 25.4 / 1200.0).abs() < 1e-9));
    for (colour, paper, red) in [
        (PngColour::Rgb, vec![255, 255, 255], vec![255, 0, 0]),
        (PngColour::Transparent, vec![0, 0, 0, 0], vec![255, 0, 0, 255]),
        (PngColour::Grey, vec![255], vec![54]),
    ] {
        let image = write(254.0, colour, &Provider::default());
        assert_eq!((image.at(5, 20), image.at(20, 20)), (&paper[..], &red[..]), "{colour:?}");
    }
    let image = write(254.0, PngColour::BlackWhite, &Provider::default());
    assert_eq!(image.at(5, 20), [255], "dithering never touches a pixel outside");
}

/// The `<image …` elements of `text`, each up to its `href`.
fn images(text: &str) -> Vec<&str> {
    text.match_indices("<image ").map(|(i, _)| &text[i..i + text[i..].find(" href=").unwrap()]).collect()
}

/// The PNG inside the document's first `<image>`.
fn embedded(text: &str) -> Image {
    let start = text.find("href=\"data:image/png;base64,").unwrap() + 28;
    read(&base64_decode(text[start..].split('"').next().unwrap()).unwrap())
}

/// A 100 × 50 mm picture at 300 dpi is a PNG of its own grid, 1182 × 591 px (100.076 × 50.038 mm),
/// anchored at its lower-left corner (20.3, 30.7), so its top is at 297 − 80.738 = 216.262 on the
/// page: straight-alpha RGBA, sRGB, 300 dpi recorded, the provider's pixels as they were given.
#[test]
fn a_picture_is_a_png_of_its_own_grid() {
    let rect = Aabb2 { min: p(20.3, 30.7), max: p(120.3, 80.7) };
    let provider = Provider::default();
    let text = pictured(&sheet(vec![picture(0, rect.min, rect.max, 1.0)]), 300.0, &provider);
    assert_eq!(images(&text), ["<image x=\"20.3\" y=\"216.262\" width=\"100.076\" height=\"50.038\" preserveAspectRatio=\"none\""]);
    let grid = Picture { source: 0, rect, opacity: 1.0 }.grid(300.0).unwrap();
    assert_eq!(provider.asked.borrow().as_slice(), [(0, grid, GridWindow { x0: 0, y0: 0, x1: 1182, y1: 591 })]);
    let image = embedded(&text);
    assert_eq!((image.info.width, image.info.height, image.info.color_type), (1182, 591, png::ColorType::Rgba));
    assert!(image.info.srgb.is_some());
    assert_eq!(image.info.pixel_dims.map(|d| d.xppu), Some(11811), "300 dpi");
    for (x, y) in [(0, 0), (1, 0), (1181, 590), (600, 301)] {
        assert_eq!(image.at(x, y), if (x + y) % 2 == 0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }, "({x}, {y})");
    }
}

/// A line along a picture's edge is written with the image's own number: they register.
#[test]
fn a_line_on_a_pictures_edge_has_the_images_number() {
    // 100.076 mm wide at 300 dpi: the right edge is at 120.376.
    let edge = stroke(vec![line(p(120.376, 30.7), p(120.376, 80.738))], false, 0.35);
    let text = pictured(&sheet(vec![picture(1, p(20.3, 30.7), p(120.3, 80.7), 1.0), edge]), 300.0, &Provider::default());
    assert!(images(&text)[0].contains("x=\"20.3\" y=\"216.262\" width=\"100.076\""), "{:?}", images(&text));
    assert!(text.contains("d=\"M120.376 266.3L120.376 216.262\""), "the line ends on the image's top and right edges");
}

/// An image goes where the scene orders it, between the paths before and after it; opacity below 1
/// is written, in millionths as lengths are, and 1 is left out.
#[test]
fn an_image_keeps_its_place_and_its_opacity() {
    let fill = Item::Fill { region: Region2 { outer: rectangle(p(3.0, 5.0), p(7.0, 8.0)), holes: vec![] } };
    let edge = stroke(vec![line(p(0.0, 0.0), p(10.0, 0.0))], false, 0.35);
    let text = pictured(&sheet(vec![fill, picture(1, p(4.0, 6.0), p(6.0, 7.0), 0.3333333), edge]), 254.0, &Provider::default());
    let (filled, image, stroked) = (text.find("fill-rule").unwrap(), text.find("<image").unwrap(), text.find("stroke-width").unwrap());
    assert!(filled < image && image < stroked, "{:?}", (filled, image, stroked));
    assert!(images(&text)[0].ends_with(" opacity=\"0.333333\""), "{:?}", images(&text));
    let text = pictured(&sheet(vec![picture(1, p(4.0, 6.0), p(6.0, 7.0), 1.0)]), 254.0, &Provider::default());
    assert!(!text.contains("opacity"));
}

/// Only the pixels on the sheet are embedded and asked for: a picture over the A3 sheet's top-right
/// corner is cut to its 100 × 170 px there at 254 dpi; one off the sheet is left out unasked.
#[test]
fn only_the_pixels_on_the_sheet_are_embedded() {
    let provider = Provider::default();
    let text = pictured(&sheet(vec![picture(1, p(410.0, 280.0), p(430.0, 300.0), 1.0)]), 254.0, &provider);
    assert_eq!(images(&text), ["<image x=\"410\" y=\"0\" width=\"10\" height=\"17\" preserveAspectRatio=\"none\""]);
    assert_eq!(provider.asked.borrow()[0].2, GridWindow { x0: 0, y0: 30, x1: 100, y1: 200 });
    let image = embedded(&text);
    assert_eq!((image.info.width, image.info.height), (100, 170));
    assert_eq!(image.info.pixel_dims.map(|d| d.xppu), Some(10_000), "254 dpi");
    let provider = Provider::default();
    let text = pictured(&sheet(vec![picture(1, p(500.0, 500.0), p(510.0, 510.0), 1.0)]), 254.0, &provider);
    assert!(!text.contains("<image") && provider.asked.borrow().is_empty());
}

/// A large picture is asked for a band of rows at a time: the bands follow each other and cover
/// its window, and the PNG holds every row.
#[test]
fn a_large_picture_is_asked_for_in_bands() {
    // 4200 × 1100 px at 254 dpi, over 16 MiB of RGBA.
    let provider = Provider::default();
    let text = pictured(&sheet(vec![picture(1, p(0.0, 0.0), p(420.0, 110.0), 1.0)]), 254.0, &provider);
    let asked = provider.asked.borrow();
    assert!(asked.len() > 1, "{} bands", asked.len());
    let mut top = 0;
    for (_, _, w) in asked.iter() {
        assert_eq!((w.x0, w.x1, w.y0), (0, 4200, top));
        top = w.y1;
    }
    assert_eq!(top, 1100);
    assert_eq!(embedded(&text).info.height, 1100);
}

/// What can't be embedded is refused with its limit or its source named, never shrunk: the
/// picture resolution outside 25 to 1200 dpi, a side over 65,536 px, a PNG past the room left in
/// the file, a provider's short rows or its error, and no provider at all.
#[test]
fn what_cannot_be_embedded_is_refused() {
    let blank = sheet(vec![]);
    let empty = blank.prepare().unwrap();
    for dpi in [24.9, 1200.1, f64::NAN] {
        let e = paper_svg(&empty, &NoPictures, dpi).unwrap_err().to_string();
        assert!(e.contains("25 to 1200 dpi"), "{dpi}: {e}");
    }
    for dpi in [25.0, 1200.0] {
        assert!(paper_svg(&empty, &NoPictures, dpi).is_ok(), "{dpi}");
    }
    let refused = |s: &Scene, dpi: f64, provider: &dyn Pictures| paper_svg(&s.prepare().unwrap(), provider, dpi).unwrap_err().to_string();
    let wide = sheet(vec![picture(1, p(0.0, 0.0), p(1400.0, 10.0), 1.0)]);
    assert!(refused(&wide, 1200.0, &Provider::default()).contains("over the limit of 65536 px"));
    let small = sheet(vec![picture(1, p(4.0, 6.0), p(6.0, 7.0), 1.0)]);
    let short = refused(&small, 254.0, &Provider { short: true, ..Provider::default() });
    assert!(short.contains("picture 1: 799 bytes where 800 were asked for"), "{short}");
    assert!(refused(&sheet(vec![picture(666, p(4.0, 6.0), p(6.0, 7.0), 1.0)]), 254.0, &Provider::default()).contains("picture 666: boom"));
    assert!(refused(&small, 254.0, &NoPictures).contains("no pixels for picture 1"));
    // 40 × 40 px of noise is over 6 KB as a PNG: refused at 1000 bytes, written with room.
    let grid = Picture { source: 4, rect: Aabb2 { min: p(0.0, 0.0), max: p(4.0, 4.0) }, opacity: 1.0 }.grid(254.0).unwrap();
    let window = GridWindow { x0: 0, y0: 0, x1: 40, y1: 40 };
    let e = crate::paper::png::picture_png(4, &grid, window, &Provider::default(), 1000).unwrap_err().to_string();
    assert!(e.contains("picture 4: its PNG would be over 1000 bytes"), "{e}");
    assert!(crate::paper::png::picture_png(4, &grid, window, &Provider::default(), 1 << 20).unwrap().len() > 6000);
}

/// A dashed stroke carries its pattern in millimetres, its offset only when its phase isn't 0.
#[test]
fn a_dashed_stroke_has_its_dasharray_and_offset() {
    let path = |phase_mm| {
        let pattern = Some(Dashes { lengths_mm: vec![4.2, 1.05], phase_mm });
        let scene = sheet(vec![Item::Stroke { path: vec![line(p(5.0, 10.0), p(105.0, 10.0))], closed: false, width_mm: 1.0, pattern }]);
        let (text, _) = svg(&scene);
        text.lines().find(|l| l.starts_with("<path")).unwrap().to_string()
    };
    assert_eq!(path(0.0), "<path stroke-width=\"1\" stroke-dasharray=\"4.2 1.05\" d=\"M5 287L105 287\"/>");
    assert_eq!(path(0.5), "<path stroke-width=\"1\" stroke-dasharray=\"4.2 1.05\" stroke-dashoffset=\"0.5\" d=\"M5 287L105 287\"/>");
}

#[test]
fn weld_identification_keeps_the_shared_svg_dash_pattern() {
    use solidcraft_geom::weld::{Convention, IdentificationLine, Kind, Style, Weld, WeldSide};
    let weld = Weld {
        arrow_side: Some(WeldSide { kind: Kind::Fillet, size: None, length_mm: None, intermittent: None, contour: None }),
        other_side: None,
        all_round: false,
        field: false,
        tail: None,
    };
    let drawing = weld
        .draw(
            Convention::Iso2553_2019A { identification: IdentificationLine::Above },
            Style { height_mm: 1.0, width_mm: 0.2 },
            |value| Ok((value.to_string(), value)),
            |text, h| text.len() as f64 * h,
        )
        .unwrap();
    let carrier = drawing.items.into_iter().find(|item| matches!(item, Item::Stroke { pattern: Some(_), .. })).unwrap();
    let (text, _) = svg(&sheet(vec![carrier]));
    assert!(text.contains("stroke-dasharray=\"2.4 0.6\""), "{text}");
    assert!(text.contains("stroke-dashoffset=\"0.7\""), "{text}");
    assert!(text.contains("M0 295L7 295"), "{text}");
}
