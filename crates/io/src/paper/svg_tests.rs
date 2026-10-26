use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcraft_doc::canvas::base64_decode;
use solidcraft_geom::paper::{Group, Item, NoPictures, ObjectKind, Picture, Pictures, Scene};
use solidcraft_geom::pixel_grid::GridWindow;
use solidcraft_geom::{Aabb2, Loop2, Region2, Seg2, Vec2};

use super::*;
use crate::paper::png::tests::{Image, Provider, read};
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
    Scene { size_mm: p(420.0, 297.0), groups: vec![Group { object: 1, kind: ObjectKind::DrawingBorder, items }] }
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
        let e = paper_svg(&scene.prepare().unwrap(), &NoPictures, 300.0).err().map(|e| e.to_string()).unwrap();
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

fn picture(source: u64, min: Vec2, max: Vec2, opacity: f64) -> Item {
    Item::Picture(Picture { source, rect: Aabb2 { min, max }, opacity })
}

fn pictured(scene: &Scene, dpi: f64, pictures: &Provider) -> String {
    String::from_utf8(paper_svg(&scene.prepare().unwrap(), pictures, dpi).unwrap().0).unwrap()
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
