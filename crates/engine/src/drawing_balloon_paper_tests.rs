use solidcraft_doc::drawing::dimension::ModelEdge;
use solidcraft_geom::paper::Scene;
use solidcraft_geom::paper::annotate::Terminator;
use solidcraft_geom::paper::text::TextRun;

use super::*;

const LINE_MM: f64 = 0.25;

fn balloon(at: [f64; 2]) -> Balloon {
    Balloon { id: 7, view: 8, edge: ModelEdge { body: "B".into(), origin_feature: 1, name: "e".into(), path: vec![1] }, at, parts_list: None }
}

/// `balloon` showing `item`, in the built-in font, in English.
fn drawn(balloon: &Balloon, item: Option<&str>, tip: Option<Vec2>) -> OnPaper {
    balloon_group(balloon, item, tip, LINE_MM, &PaperFonts::new("", "en", true))
}

fn arrow_mm() -> f64 {
    let Terminator::Arrow { length_mm, .. } = ARROW else { panic!("a balloon's leader ends in an arrowhead") };
    length_mm
}

fn close(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-9
}

/// The balloon's circles, its leader's points, its arrowhead's tip and its number's run.
fn parts(group: &Group) -> (Vec<&Item>, Vec<Vec2>, Option<Vec2>, Option<&TextRun>) {
    let (mut circles, mut line, mut tip, mut run) = (Vec::new(), Vec::new(), None, None);
    for item in &group.items {
        match item {
            Item::Stroke { closed: true, .. } => circles.push(item),
            Item::Stroke { path, .. } => line = path.iter().flat_map(|seg| [seg.start(), seg.end()]).collect(),
            Item::Fill { region } => tip = Some(region.outer.segs[0].start()),
            Item::Text(text) => run = Some(text),
            _ => {}
        }
    }
    (circles, line, tip, run)
}

fn circle_of(group: &Group) -> Vec<solidcore_record::Seg2> {
    let (circles, ..) = parts(group);
    assert_eq!(circles.len(), 1, "one circle: {group:?}");
    let Item::Stroke { path, width_mm, .. } = circles[0] else { panic!("a circle") };
    assert_eq!(*width_mm, LINE_MM);
    path.clone()
}

fn valid(on: &OnPaper) {
    let mut scene = Scene::new(Vec2::new(420.0, 297.0), vec![on.group.clone()]);
    scene.add_fonts(on.fonts.iter().cloned());
    scene.validate().unwrap();
}

/// A number in a circle of 5 at its place, level and in its middle, and a leader from the arrow's end
/// to the rim, along the line to the centre: by hand, (-40, 0) to (60, 40) leaves the rim 5 short.
#[test]
fn a_balloon_is_a_circle_round_its_number_with_a_leader_to_its_arrow() {
    let on = drawn(&balloon([60.0, 40.0]), Some("12"), Some(Vec2::new(-40.0, 0.0)));
    assert_eq!((on.group.object, on.group.kind, on.why.as_deref(), on.pending), (7, ObjectKind::Balloon, None, false));
    valid(&on);
    assert_eq!(circle_of(&on.group), Loop2::circle(Vec2::new(60.0, 40.0), RADIUS_MM).segs);
    let (_, line, tip, run) = parts(&on.group);
    assert!(close(tip.unwrap(), Vec2::new(-40.0, 0.0)), "{tip:?}");
    let way = Vec2::new(100.0, 40.0).normalized().unwrap();
    assert!(close(line[0], Vec2::new(-40.0, 0.0) + way * arrow_mm()), "stops at the arrowhead's base: {line:?}");
    assert!(close(*line.last().unwrap(), Vec2::new(60.0, 40.0) - way * RADIUS_MM), "{line:?}");
    let run = run.unwrap();
    assert_eq!((run.text.as_str(), run.angle), ("12", 0.0));
    assert!((run.origin.x + run.reach() * 0.5 - 60.0).abs() < 1e-9, "centred across: {:?}", run.origin);
}

/// A number too wide for the least circle gets a larger one, a gap clear of its corners.
#[test]
fn a_wide_number_gets_a_wider_circle() {
    let on = drawn(&balloon([0.0, 0.0]), Some("12.34.56"), None);
    let run = parts(&on.group).3.unwrap();
    let text = Text::set(Some("12.34.56"), TEXT_MM, &PaperFonts::new("", "en", true));
    assert!((text.half.x * 2.0 - run.reach()).abs() < 1e-9);
    let radius = text.half.len() + GAP_MM;
    assert!(radius > RADIUS_MM, "{radius}");
    assert_eq!(circle_of(&on.group), Loop2::circle(Vec2::ZERO, radius).segs);
}

/// A lost balloon keeps its circle and number with no leader; one with no row keeps an empty circle
/// and its leader; an arrow inside or just outside the circle draws no leader and says so; and a
/// number the font can't set keeps the rest.
#[test]
fn what_it_cant_draw_leaves_the_rest_and_says_why() {
    let lost = drawn(&balloon([0.0, 0.0]), Some("3"), None);
    assert!(parts(&lost.group).1.is_empty() && parts(&lost.group).3.is_some() && lost.why.is_none());
    let empty = drawn(&balloon([100.0, 100.0]), None, Some(Vec2::new(130.0, 100.0)));
    assert!(parts(&empty.group).3.is_none() && parts(&empty.group).2.is_some() && empty.why.is_none());
    assert_eq!(circle_of(&empty.group), Loop2::circle(Vec2::new(100.0, 100.0), RADIUS_MM).segs);
    valid(&empty);
    let at_rim = RADIUS_MM + arrow_mm();
    for tip in [Vec2::ZERO, Vec2::new(0.1, 0.0), Vec2::new(2.0, 0.0), Vec2::new(RADIUS_MM, 0.0), Vec2::new(at_rim, 0.0)] {
        let near = drawn(&balloon([0.0, 0.0]), Some("3"), Some(tip));
        assert!(parts(&near.group).2.is_none(), "{tip:?}");
        assert_eq!(near.why.as_deref(), Some("its arrow ends too near its circle to draw"), "{tip:?}");
        assert_eq!(circle_of(&near.group).len(), Loop2::circle(Vec2::ZERO, RADIUS_MM).segs.len());
    }
    assert!(parts(&drawn(&balloon([0.0, 0.0]), Some("3"), Some(Vec2::new(at_rim + 0.01, 0.0))).group).2.is_some());
    // An unassigned code point: no font anywhere has a glyph for it.
    let unset = drawn(&balloon([0.0, 0.0]), Some("\u{378}"), Some(Vec2::new(30.0, 0.0)));
    assert!(unset.why.as_deref().is_some_and(|why| why.contains("no glyph")), "{:?}", unset.why);
    assert!(parts(&unset.group).3.is_none() && parts(&unset.group).2.is_some() && unset.fonts.is_empty());
}

/// Any place and any arrow, at the values that break a layout, gives a group, without a panic.
#[test]
fn hostile_places_and_arrows_never_panic() {
    let values = [0.0, -0.0, 1e-300, 1e-9, 1.0, -1.0, 5.0, 8.0, 1e5, -1e5, 1e12, f64::MAX, f64::MIN, f64::NAN, f64::INFINITY];
    for x in values {
        for y in values {
            for tip in [Vec2::ZERO, Vec2::new(x, y), Vec2::new(y, 1.0)] {
                let on = drawn(&balloon([x, y]), Some("1"), Some(tip));
                assert_eq!(on.group.kind, ObjectKind::Balloon);
                let mut scene = Scene::new(Vec2::new(420.0, 297.0), vec![on.group]);
                scene.add_fonts(on.fonts);
                let _ = scene.validate();
            }
        }
    }
}
