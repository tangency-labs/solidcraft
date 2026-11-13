use solidcraft_doc::drawing::note::Anchor;
use solidcraft_geom::cell::{Content, Part};
use solidcraft_geom::paper::text::TextRun;
use solidcraft_sketch::{TextMetrics, TextStyle, font_data};

use super::*;

const LINE_MM: f64 = 0.25;

fn note(at: [f64; 2], cap_mm: Option<f64>) -> Note {
    Note { id: 9, anchor: Anchor::Sheet(1), at, content: Content(vec![Part::Text("ROUND".into())]), cap_mm, branches: Vec::new() }
}

/// `note` reading `text`, in the built-in font, in English.
fn drawn(note: &Note, text: &str, tip: Option<Vec2>) -> OnPaper {
    note_group(note, text, tip.as_slice(), LINE_MM, &PaperFonts::new("", "en", true))
}

fn runs(group: &Group) -> Vec<&TextRun> {
    group.items.iter().filter_map(|item| if let Item::Text(run) = item { Some(run) } else { None }).collect()
}

/// The leader's line point by point, and its arrowhead's tip.
fn leader_of(group: &Group) -> Option<(Vec<Vec2>, Vec2)> {
    let line = group.items.iter().find_map(|item| match item {
        Item::Stroke { path, .. } => Some(std::iter::once(path[0].start()).chain(path.iter().map(|seg| seg.end())).collect()),
        _ => None,
    })?;
    let tip = group.items.iter().find_map(|item| if let Item::Fill { region } = item { Some(region.outer.segs[0].start()) } else { None })?;
    Some((line, tip))
}

fn width(text: &str, cap_mm: f64) -> f64 {
    TextMetrics::new(&font_data(None).unwrap(), &TextStyle::default()).unwrap().line_width(text, cap_mm).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// A note's text is centred on its place, at its own capital height or a dimension's, in a group of
/// its own kind.
#[test]
fn a_notes_text_is_centred_on_its_place() {
    for (cap_mm, at) in [(None, [20.0, 10.0]), (Some(5.0), [120.0, 60.0])] {
        let on = drawn(&note(at, cap_mm), "ROUND", None);
        assert_eq!((on.group.object, on.group.kind, on.why.as_deref()), (9, ObjectKind::Note, None));
        let cap = cap_mm.unwrap_or(TEXT_MM);
        let texts = runs(&on.group);
        assert_eq!(texts.len(), 1);
        assert!(close(texts[0].origin.x + width("ROUND", cap) / 2.0, at[0]), "{:?}", texts[0].origin);
        assert!(leader_of(&on.group).is_none());
    }
}

/// From beside the text, a leader ends with a landing up to the text's gap, on the side it comes
/// from; its arrowhead's tip is where it points.
#[test]
fn a_leader_lands_on_the_side_it_comes_from() {
    let half = width("ROUND", TEXT_MM) / 2.0 + GAP_MM;
    let landing = LANDING_PER_LINE_WIDTH * LINE_MM;
    for (tip, side) in [(Vec2::new(-40.0, -30.0), -1.0), (Vec2::new(60.0, 25.0), 1.0)] {
        let on = drawn(&note([0.0, 0.0], None), "ROUND", Some(tip));
        let (line, end) = leader_of(&on.group).unwrap();
        assert_eq!(end, tip);
        let last = line.last().copied().unwrap();
        let before = line[line.len() - 2];
        assert!(close(last.x, side * half) && close(last.y, 0.0), "{line:?}");
        assert!(close(before.x, side * (half + landing)) && close(before.y, 0.0), "{line:?}");
    }
}

/// Each leader's line point by point, and the arrowheads' tips.
fn leaders_of(group: &Group) -> (Vec<Vec<Vec2>>, Vec<Vec2>) {
    let lines = group.items.iter().filter_map(|item| match item {
        Item::Stroke { path, .. } => Some(std::iter::once(path[0].start()).chain(path.iter().map(|seg| seg.end())).collect()),
        _ => None,
    });
    let tips = group.items.iter().filter_map(|item| if let Item::Fill { region } = item { Some(region.outer.segs[0].start()) } else { None });
    (lines.collect(), tips.collect())
}

/// A note with several leaders draws each, its arrowhead where it points. The landing on a side is
/// drawn once, by the first leader from that side; the next from that side ends where it starts, so
/// they meet there. A leader that can't be drawn leaves the landing to the next one.
#[test]
fn leaders_from_one_side_meet_at_its_landing() {
    let half = width("ROUND", TEXT_MM) / 2.0 + GAP_MM;
    let landing = LANDING_PER_LINE_WIDTH * LINE_MM;
    let tips = [Vec2::new(-40.0, -30.0), Vec2::new(-50.0, 10.0), Vec2::new(60.0, 25.0)];
    let on = note_group(&note([0.0, 0.0], None), "ROUND", &tips, LINE_MM, &PaperFonts::new("", "en", true));
    let (lines, ends) = leaders_of(&on.group);
    assert_eq!(ends, tips);
    let knee = |side: f64| Vec2::new(side * (half + landing), 0.0);
    let text = |side: f64| Vec2::new(side * half, 0.0);
    // Each line starts at its arrowhead's base, an arrowhead's length from the tip towards the knee.
    let solidcraft_geom::paper::annotate::Terminator::Arrow { length_mm: arrow, .. } = ARROW else { panic!("a dimension's arrowhead") };
    let base = |tip: Vec2, to: Vec2| tip + (to - tip) * (arrow / tip.dist(to));
    let expected = [
        vec![base(tips[0], knee(-1.0)), knee(-1.0), text(-1.0)],
        vec![base(tips[1], knee(-1.0)), knee(-1.0)],
        vec![base(tips[2], knee(1.0)), knee(1.0), text(1.0)],
    ];
    assert_eq!(lines.len(), 3, "{lines:?}");
    for (line, want) in lines.iter().zip(&expected) {
        assert!(line.len() == want.len() && line.iter().zip(want).all(|(a, b)| a.dist(*b) < 1e-9), "{line:?} != {want:?}");
    }
    // The first from the left is too short to draw: the second draws the landing.
    let short = Vec2::new(-(half + landing + 0.1), 0.0);
    let on = note_group(&note([0.0, 0.0], None), "ROUND", &[short, tips[0]], LINE_MM, &PaperFonts::new("", "en", true));
    let (lines, ends) = leaders_of(&on.group);
    assert_eq!((ends, on.why.as_deref()), (vec![tips[0]], Some("its leader ends too near its text to draw")));
    assert!(lines.len() == 1 && lines[0].len() == 3 && lines[0][2].dist(text(-1.0)) < 1e-9, "{lines:?}");
}

/// From over or under the text, where a landing would run back through it, the leader runs straight
/// to the nearer side, with no landing.
#[test]
fn a_leader_from_above_runs_straight_to_the_nearer_side() {
    let half = width("ROUND", TEXT_MM) / 2.0 + GAP_MM;
    for (tip, side) in [(Vec2::new(-1.0, 30.0), -1.0), (Vec2::new(1.0, -30.0), 1.0)] {
        let on = drawn(&note([0.0, 0.0], None), "ROUND", Some(tip));
        let (line, _) = leader_of(&on.group).unwrap();
        assert_eq!(line.len(), 2, "{line:?}");
        assert!(close(line[1].x, side * half) && close(line[1].y, 0.0), "{line:?}");
    }
}

/// A leader too short for its arrowhead is left out and said why; the text stays.
#[test]
fn a_leader_too_short_for_its_arrowhead_is_left_out() {
    let half = width("ROUND", TEXT_MM) / 2.0 + GAP_MM;
    for tip in [Vec2::new(half, 0.0), Vec2::new(half + 0.5, 0.0)] {
        let on = drawn(&note([0.0, 0.0], None), "ROUND", Some(tip));
        assert!(leader_of(&on.group).is_none(), "{tip:?}");
        assert_eq!(on.why.as_deref(), Some("its leader ends too near its text to draw"));
        assert_eq!(runs(&on.group).len(), 1);
    }
}

/// Hostile places and tips never panic: each is a group, with or without its leader.
#[test]
fn hostile_places_never_panic() {
    let odd = [f64::NAN, f64::INFINITY, -f64::INFINITY, 1e300, -1e300, 0.0, f64::MIN_POSITIVE];
    for x in odd {
        for y in odd {
            let on = drawn(&note([x, y], Some(x.abs())), "ROUND", Some(Vec2::new(x, y)));
            assert_eq!(on.group.kind, ObjectKind::Note);
        }
    }
    let empty = drawn(&note([0.0, 0.0], None), "", Some(Vec2::new(-50.0, 0.0)));
    assert!(runs(&empty.group).is_empty());
    let many: Vec<Vec2> = odd.iter().flat_map(|x| odd.map(|y| Vec2::new(*x, y))).collect();
    assert_eq!(drawn(&note([0.0, 0.0], None), "ROUND", None).group.kind, ObjectKind::Note);
    let all = note_group(&note([0.0, 0.0], None), "ROUND", &many, LINE_MM, &PaperFonts::new("", "en", true));
    assert_eq!(all.group.kind, ObjectKind::Note);
}
