use solidcore_record::Seg2;
use solidcraft_geom::paper::Scene;
use solidcraft_geom::paper::text::TextRun;
use solidcraft_geom::prelude::*;

use super::*;

const LINE_MM: f64 = 0.25;

fn tag(at: [f64; 2], size_mm: f64) -> Tag {
    Tag { id: 7, row: 8, sheet: 9, at, size_mm, text: None, lost: None }
}

/// `tag` showing `shows`, in the built-in font, in English.
fn drawn(tag: &Tag, shows: &str) -> OnPaper {
    tag_group(tag, shows, LINE_MM, &PaperFonts::new("", "en", true))
}

/// The outline's corners in order, and the text's runs.
fn parts(group: &Group) -> (Vec<Vec2>, Vec<&TextRun>) {
    let (mut corners, mut runs) = (Vec::new(), Vec::new());
    for item in &group.items {
        match item {
            Item::Stroke { path, closed: true, width_mm, .. } => {
                assert_eq!(*width_mm, LINE_MM);
                corners = path.iter().map(Seg2::start).collect();
            }
            Item::Text(run) => runs.push(run),
            other => panic!("a tag draws its outline and its text: {other:?}"),
        }
    }
    (corners, runs)
}

/// How far `p` is inside the triangle `corners` (counter-clockwise): its distance to the nearest edge.
fn inside(corners: &[Vec2], p: Vec2) -> f64 {
    let edge = |i: usize| (corners[i], corners[(i + 1) % 3]);
    (0..3).map(edge).map(|(a, b)| (b - a).perp().normalized().unwrap().dot(p - a)).fold(f64::INFINITY, f64::min)
}

/// How far the text's box is inside the outline: its nearest corner's distance to an edge.
fn clearance(on: &OnPaper) -> f64 {
    let (corners, _) = parts(&on.group);
    let [text] = on.layout.text_boxes[..] else { panic!("one text box: {:?}", on.layout) };
    text.iter().map(|p| inside(&corners, *p)).fold(f64::INFINITY, f64::min)
}

/// The text's cap height: its box's height, for one line.
fn cap(on: &OnPaper) -> f64 {
    let [text] = on.layout.text_boxes[..] else { panic!("one text box: {:?}", on.layout) };
    text[3].y - text[0].y
}

fn valid(on: &OnPaper) {
    let mut scene = Scene::new(Vec2::new(420.0, 297.0), vec![on.group.clone()]);
    scene.add_fonts(on.fonts.iter().cloned());
    scene.validate().unwrap();
}

/// A 10 mm tag at (100, 50) by hand: its apex 10/√3 above its place and its base corners 5 to each
/// side, 10/(2√3) below, so its place is its centroid. Its letter sits a gap inside every edge, level,
/// across its middle, and its run is as wide as its box.
#[test]
fn a_tag_is_a_point_up_triangle_round_its_revision() {
    let on = drawn(&tag([100.0, 50.0], 10.0), "A");
    assert_eq!((on.group.object, on.group.kind, on.why.as_deref(), on.pending), (7, ObjectKind::RevisionTag, None, false));
    valid(&on);
    let (corners, runs) = parts(&on.group);
    let r3 = 3f64.sqrt();
    let by_hand = [Vec2::new(100.0, 50.0 + 10.0 / r3), Vec2::new(95.0, 50.0 - 5.0 / r3), Vec2::new(105.0, 50.0 - 5.0 / r3)];
    for (corner, expected) in corners.iter().zip(by_hand) {
        assert!(corner.dist(expected) < 1e-12, "{corners:?}");
    }
    assert_eq!(corners.len(), 3);
    let centroid = (corners[0] + corners[1] + corners[2]) * (1.0 / 3.0);
    assert!(centroid.dist(Vec2::new(100.0, 50.0)) < 1e-12);
    assert!(clearance(&on) >= GAP_MM - 1e-9, "{}", clearance(&on));
    let [run] = runs[..] else { panic!("one run: {runs:?}") };
    let [text] = on.layout.text_boxes[..] else { panic!() };
    assert_eq!(run.angle, 0.0);
    assert!(((text[0].x + text[1].x) * 0.5 - 100.0).abs() < 1e-9, "centred across: {text:?}");
    assert!((run.reach() - (text[1].x - text[0].x)).abs() < 1e-9 && (run.origin.x - text[0].x).abs() < 1e-9);
    assert!(cap(&on) > 0.0 && cap(&on) <= TEXT_MM);
}

/// The letters are as large as fit: a wider revision gets smaller ones, and either sits exactly a
/// gap from the edges it is nearest, which is no larger letters fit.
#[test]
fn a_wider_revision_gets_smaller_letters_as_large_as_fit() {
    let (a, ab) = (drawn(&tag([0.0, 0.0], 8.0), "A"), drawn(&tag([0.0, 0.0], 8.0), "AB"));
    assert!(cap(&ab) < cap(&a), "{} {}", cap(&ab), cap(&a));
    for on in [&a, &ab] {
        assert!(cap(on) < TEXT_MM);
        assert!((clearance(on) - GAP_MM).abs() < 1e-9, "{}", clearance(on));
        valid(on);
    }
}

/// A large tag keeps a dimension's cap height and centres its letters on its place.
#[test]
fn a_large_tag_keeps_the_usual_letters_on_its_centroid() {
    let on = drawn(&tag([200.0, 100.0], 40.0), "B");
    assert!((cap(&on) - TEXT_MM).abs() < 1e-9, "{}", cap(&on));
    let [text] = on.layout.text_boxes[..] else { panic!() };
    let middle = (text[0] + text[2]) * 0.5;
    assert!(middle.dist(Vec2::new(200.0, 100.0)) < 1e-9, "{middle:?}");
    assert!(clearance(&on) > GAP_MM);
}

/// A tag too small for its revision keeps its outline, leaves its text out, and says the side that
/// holds it; a tag of that side holds it. An empty revision draws the outline alone.
#[test]
fn a_tag_too_small_for_its_revision_says_the_side_that_holds_it() {
    let small = drawn(&tag([50.0, 50.0], 1.0), "C");
    assert_eq!(parts(&small.group).0.len(), 3);
    assert!(parts(&small.group).1.is_empty() && small.layout.text_boxes.is_empty() && small.fonts.is_empty());
    let why = small.why.as_deref().unwrap();
    let side: f64 = why.rsplit("at least ").next().and_then(|rest| rest.strip_suffix(" mm")).unwrap().parse().unwrap();
    assert!(why.starts_with("its side is too short for its text"), "{why}");
    let fits = drawn(&tag([50.0, 50.0], side), "C");
    assert!(fits.why.is_none() && !parts(&fits.group).1.is_empty(), "{side}: {:?}", fits.why);
    assert!(cap(&fits) >= solidcraft_doc::drawing::note::CAP_MM.start() - 1e-9);
    assert!(drawn(&tag([50.0, 50.0], side - 0.1), "C").why.is_some(), "{side} is the least that holds it");
    let empty = drawn(&tag([50.0, 50.0], 5.0), "");
    assert!(empty.why.is_none() && parts(&empty.group).1.is_empty() && parts(&empty.group).0.len() == 3);
    // An unassigned code point: no font anywhere has a glyph for it.
    let unset = drawn(&tag([50.0, 50.0], 10.0), "\u{378}");
    assert!(unset.why.as_deref().is_some_and(|why| why.contains("no glyph")), "{:?}", unset.why);
}

/// Every side the record allows gives three distinct corners and a valid group, and any place, at
/// the values that break a layout, gives a group without a panic.
#[test]
fn every_side_and_hostile_places_draw_without_a_panic() {
    for tenth in 10..=500 {
        let on = drawn(&tag([210.0, 148.5], f64::from(tenth) / 10.0), "A1");
        let (corners, _) = parts(&on.group);
        assert!(corners[0].dist(corners[1]) > 0.5 && corners[1].dist(corners[2]) > 0.5 && corners[2].dist(corners[0]) > 0.5);
        valid(&on);
    }
    let values = [0.0, -0.0, 1e-300, 1.0, -1.0, 1e5, 1e12, f64::MAX, f64::MIN, f64::NAN, f64::INFINITY];
    for x in values {
        for size in values {
            let on = drawn(&tag([x, x], size), "A");
            assert_eq!(on.group.kind, ObjectKind::RevisionTag);
            let mut scene = Scene::new(Vec2::new(420.0, 297.0), vec![on.group]);
            scene.add_fonts(on.fonts);
            let _ = scene.validate();
        }
    }
}
