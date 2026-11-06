use solidcore_record::Vec3;
use solidcraft_doc::drawing::dimension::{Member, ModelEdge, ModelFace, Silhouette, Status};
use solidcraft_geom::Aabb2;
use solidcraft_geom::drawing_measure::{Measured, Reading, SilhouetteSide};
use solidcraft_geom::drawing_view::{NamedView, Scale};
use solidcraft_geom::paper::Scene;
use solidcraft_geom::paper::line::LineGroup;
use solidcraft_sketch::{TextMetrics, font_data};

use super::*;

/// The front view at `scale`: X to the right on paper and Z up, centred on the origin of paper
/// with the point (0, 0) of its plane there.
fn view_at(scale: &str) -> ViewFrame {
    let scale: Scale = scale.parse().unwrap();
    ViewFrame::new(NamedView::Front.basis(), scale, Vec2::ZERO, Vec2::ZERO).unwrap()
}

fn dimension(kind: DimensionKind, offset_mm: f64, text: TextPlace) -> Dimension {
    Dimension { id: 7, view: 8, kind, offset_mm, text, decimals: 2, override_text: None, last_good: None }
}

fn edge() -> ModelEdge {
    ModelEdge { body: "B".into(), origin_feature: 1, name: "e".into(), path: Vec::new() }
}

fn face() -> ModelFace {
    ModelFace { body: "B".into(), origin_feature: 1, name: "f".into(), path: Vec::new() }
}

fn read(points: &[[f64; 2]], text: &str) -> Resolved {
    Resolved {
        status: Status::Ok,
        measured: None,
        text: Some(text.into()),
        members: Vec::new(),
        exact: true,
        // Front view: x along X and y along Z.
        points: points.iter().map(|[x, y]| Vec3::new(*x, 0.0, *y)).collect(),
    }
}

fn length(direction: LinearDirection) -> DimensionKind {
    DimensionKind::Length { edge: edge(), direction, reading: Reading::Projected }
}

/// `dimension` on paper in the built-in font, in English.
fn laid(dimension: &Dimension, resolved: &Resolved, frame: &ViewFrame) -> Option<OnPaper> {
    paper_group(dimension, resolved, frame, LineGroup::DEFAULT, "", "en", true)
}

fn drawn(kind: DimensionKind, offset: f64, points: &[[f64; 2]], text: &str, scale: &str) -> Group {
    laid(&dimension(kind, offset, TextPlace::Centred), &read(points, text), &view_at(scale)).unwrap().group
}

/// A group's text, run by run.
fn runs(group: &Group) -> Vec<&TextRun> {
    group
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Text(run) => Some(run),
            _ => None,
        })
        .collect()
}

/// A line of `text`'s width in the built-in font, measured by the rule sketch text is drawn with.
fn measured(text: &str) -> f64 {
    TextMetrics::new(&font_data(None).unwrap(), &TextStyle::default()).unwrap().line_width(text, TEXT_MM).unwrap()
}

/// A dimension's lines are its line group's thin line: 0.25 mm in the default group, 0.35 mm in
/// the 0.7 mm one.
#[test]
fn a_dimensions_lines_are_its_groups_thin_line() {
    let widths = |group| {
        let d = dimension(length(LinearDirection::Horizontal), 10.0, TextPlace::Centred);
        let drawn = paper_group(&d, &read(&[[0.0, 0.0], [40.0, 0.0]], "40.00"), &view_at("1:1"), group, "", "en", true).unwrap().group;
        let strokes = drawn.items.iter().filter_map(|i| match i {
            Item::Stroke { width_mm, .. } => Some(*width_mm),
            _ => None,
        });
        strokes.collect::<Vec<f64>>()
    };
    let (default, wide) = (widths(LineGroup::DEFAULT), widths(LineGroup::Mm0_7));
    assert!(!default.is_empty() && default.len() == wide.len());
    assert!(default.iter().all(|w| *w == 0.25) && wide.iter().all(|w| *w == 0.35), "{default:?} {wide:?}");
}

/// A group's lines, each point by point, and its arrowheads' tips.
fn parts(group: &Group) -> (Vec<Vec<Vec2>>, Vec<Vec2>) {
    let (mut lines, mut tips) = (Vec::new(), Vec::new());
    for item in &group.items {
        match item {
            Item::Stroke { path, .. } => {
                let mut line = vec![path[0].start()];
                line.extend(path.iter().map(|seg| seg.end()));
                lines.push(line);
            }
            Item::Fill { region } => tips.push(region.outer.segs[0].start()),
            Item::Text(_) | Item::Picture(_) => {}
        }
    }
    (lines, tips)
}

/// The box round a fill's corners.
fn fill_box(item: &Item) -> Aabb2 {
    let Item::Fill { region } = item else { panic!("not a fill") };
    region.outer.segs.iter().fold(Aabb2::EMPTY, |all, seg| all.union(&seg.bounds()))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// The same edge at three scales: its arrows' tips are its length apart at the view's scale, and
/// its arrowheads, its gap, its overshoot and its offset are the same on paper.
#[test]
fn a_dimension_is_the_same_size_on_paper_at_any_scale() {
    for (scale, factor) in [("1:1", 1.0), ("1:2", 0.5), ("1:5", 0.2)] {
        let group = drawn(length(LinearDirection::Aligned), 10.0, &[[0.0, 30.0], [80.0, 30.0]], "80.00", scale);
        assert_eq!((group.object, group.kind), (7, ObjectKind::Dimension));
        let (lines, tips) = parts(&group);
        assert_eq!(tips.len(), 2, "{scale}");
        let top = 30.0 * factor;
        assert!(tips.iter().all(|t| close(t.y, top + 10.0)), "{scale}: {tips:?}");
        assert!(close(tips[1].dist(tips[0]), 80.0 * factor), "{scale}: {tips:?}");
        // The two extension lines: 1 clear of the edge, 2 past the dimension line.
        let upright: Vec<&Vec<Vec2>> = lines.iter().filter(|l| close(l[0].x, l[1].x)).collect();
        assert_eq!(upright.len(), 2, "{scale}: {lines:?}");
        for line in upright {
            assert!(close(line[0].y, top + 1.0) && close(line[1].y, top + 12.0), "{scale}: {line:?}");
        }
        // Each arrowhead is the arrow's own size: as long as it is, and as wide as its point opens there.
        let Terminator::Arrow { length_mm, angle, .. } = ARROW else { panic!("a dimension's arrowhead is an arrow") };
        let wide = 2.0 * length_mm * (angle / 2.0).tan();
        for item in group.items.iter().filter(|item| matches!(item, Item::Fill { .. })) {
            let size = fill_box(item);
            assert!(close(size.max.x - size.min.x, length_mm) && close(size.max.y - size.min.y, wide), "{scale}: {size:?}");
        }
    }
}

/// The arrows sit inside when the span holds them and the text, and outside pointing in when it
/// doesn't. Inside, the line stops at their bases; the tips stay the span apart either way.
#[test]
fn arrows_go_inside_or_outside_by_the_span() {
    let kind = || length(LinearDirection::Horizontal);
    let wide = drawn(kind(), 10.0, &[[0.0, 0.0], [40.0, 0.0]], "40.00", "1:1");
    let (lines, tips) = parts(&wide);
    let line = lines.iter().find(|l| close(l[0].y, l[1].y)).unwrap();
    assert!(close(line[0].x, 3.0) && close(line[1].x, 37.0), "stops at the bases: {line:?}");
    assert!(close(tips[0].x, 0.0) && close(tips[1].x, 40.0), "{tips:?}");

    let tight = drawn(kind(), 10.0, &[[0.0, 0.0], [5.0, 0.0]], "5.00", "1:1");
    let (lines, tips) = parts(&tight);
    let line = lines.iter().find(|l| close(l[0].y, l[1].y)).unwrap();
    assert!(line[0].x < 0.0 && line[1].x > 5.0, "tails outside: {line:?}");
    assert!(close(tips[0].x, 0.0) && close(tips[1].x, 5.0), "{tips:?}");
}

/// Offsets: zero is on what it measures, with no extension lines; the sign is the side, whichever
/// way round the edge's ends are.
#[test]
fn an_offset_says_which_side_whichever_way_round_the_points_are() {
    let line_y = |offset: f64, a: [f64; 2], b: [f64; 2]| parts(&drawn(length(LinearDirection::Aligned), offset, &[a, b], "1", "1:1")).1[0].y;
    let (a, b) = ([0.0, 0.0], [50.0, 0.0]);
    for (offset, y) in [(10.0, 10.0), (-10.0, -10.0)] {
        assert!(close(line_y(offset, a, b), y) && close(line_y(offset, b, a), y), "{offset}");
    }
    // Upright: positive is to the left.
    let upright = parts(&drawn(length(LinearDirection::Aligned), 10.0, &[[0.0, 50.0], [0.0, 0.0]], "1", "1:1")).1;
    assert!(upright.iter().all(|t| close(t.x, -10.0)), "{upright:?}");
    let flat = drawn(length(LinearDirection::Aligned), 0.0, &[a, b], "1", "1:1");
    assert_eq!(parts(&flat).0.len(), 1, "only the dimension line");
    // An offset smaller than the gap: the extension lines stay whole, the dimension line still stops
    // at the arrows' bases.
    let (lines, _) = parts(&drawn(length(LinearDirection::Aligned), 0.5, &[a, b], "1", "1:1"));
    assert!(lines.iter().any(|l| close(l[0].x, 3.0) && close(l[1].x, 47.0)), "{lines:?}");
    assert_eq!(lines.iter().filter(|l| close(l[0].x, l[1].x)).count(), 2, "{lines:?}");
}

/// A horizontal dimension of a slanted edge, and one along a direction of the view.
#[test]
fn a_line_runs_in_its_dimensions_direction() {
    let slant = [[0.0, 0.0], [30.0, 40.0]];
    let tips = parts(&drawn(length(LinearDirection::Horizontal), 5.0, &slant, "1", "1:1")).1;
    assert!(close(tips[0].y, tips[1].y) && close(tips[1].x - tips[0].x, 30.0), "{tips:?}");
    let tips = parts(&drawn(length(LinearDirection::Vertical), 5.0, &slant, "1", "1:1")).1;
    assert!(close(tips[0].x, tips[1].x) && close((tips[1].y - tips[0].y).abs(), 40.0), "{tips:?}");
    let along = LinearDirection::Along(Vec2::new(0.0, 2.0));
    let tips = parts(&drawn(length(along), 5.0, &slant, "1", "1:1")).1;
    assert!(close(tips[0].x, tips[1].x), "{tips:?}");
    // A direction of no length, and an aligned line whose ends are one point, have no place.
    let none = |kind, points: &[[f64; 2]]| laid(&dimension(kind, 5.0, TextPlace::Centred), &read(points, "1"), &view_at("1:1"));
    assert!(none(length(LinearDirection::Along(Vec2::ZERO)), &slant).is_none());
    assert!(none(length(LinearDirection::Aligned), &[[1.0, 1.0], [1.0, 1.0]]).is_none());
}

/// An angle's two lines meet where they cross, even when they stop short of it; its arrows are the
/// offset from there, on the rays either side of the smaller angle, or with `other_side` the larger.
#[test]
fn an_angle_is_drawn_at_the_point_its_lines_meet() {
    let kind = |other_side| DimensionKind::Angle { a: edge(), b: edge(), reading: Reading::Projected, other_side };
    let square = [[10.0, 0.0], [50.0, 0.0], [0.0, 10.0], [0.0, 50.0]];
    let tips = parts(&drawn(kind(false), 20.0, &square, "90", "1:1")).1;
    let mut seen: Vec<(i64, i64)> = tips.iter().map(|t| (t.x.round() as i64, t.y.round() as i64)).collect();
    seen.sort();
    assert_eq!(seen, vec![(0, 20), (20, 0)]);

    // Rays 60 degrees apart; the larger angle takes one of them round to 120.
    let (c, s) = (0.5, 3f64.sqrt() / 2.0);
    let sixty = [[0.0, 0.0], [50.0, 0.0], [0.0, 0.0], [50.0 * c, 50.0 * s]];
    for (other_side, degrees) in [(false, 60.0_f64), (true, 120.0)] {
        let tips = parts(&drawn(kind(other_side), 20.0, &sixty, "1", "1:1")).1;
        assert!(tips.iter().all(|t| close(t.len(), 20.0)), "{tips:?}");
        let between = (tips[0].dot(tips[1]) / 400.0).clamp(-1.0, 1.0).acos();
        assert!((between - degrees.to_radians()).abs() < 1e-9, "{other_side}: {tips:?}");
    }

    // Parallel lines never meet; nearly parallel ones meet far away, and still lay out.
    let parallel = [[0.0, 0.0], [50.0, 0.0], [0.0, 10.0], [50.0, 10.0]];
    assert!(laid(&dimension(kind(false), 20.0, TextPlace::Centred), &read(&parallel, "0"), &view_at("1:1")).is_none());
    let nearly = [[0.0, 0.0], [50.0, 0.0], [0.0, 10.0], [50.0, 10.0 + 1e-7]];
    let far = drawn(kind(false), 20.0, &nearly, "0", "1:1");
    assert!(!far.items.is_empty());
}

/// A hole's diameter has an arrow on each side of its rim; a radius one, on the arc's middle.
#[test]
fn a_radial_dimensions_arrows_sit_on_its_rim() {
    let hole = DimensionKind::Radial { edge: edge(), diameter: true };
    let circle = [[0.0, 0.0], [6.0, 0.0], [-6.0, 0.0], [6.0, 0.0]];
    let tips = parts(&drawn(hole, 10.0, &circle, "Ø12.00", "1:2")).1;
    assert_eq!(tips.len(), 2);
    assert!(tips.iter().all(|t| close(t.len(), 3.0)), "at 1:2: {tips:?}");
    assert!(close((tips[0] + tips[1]).len(), 0.0), "opposite: {tips:?}");

    // A quarter arc from east to north: its arrow is on the arc's middle.
    let fillet = DimensionKind::Radial { edge: edge(), diameter: false };
    let (r, m) = (5.0, 5.0 * FRAC_1_SQRT_2);
    let tips = parts(&drawn(fillet.clone(), 10.0, &[[0.0, 0.0], [r, 0.0], [m, m], [0.0, r]], "R5.00", "1:1")).1;
    assert_eq!(tips.len(), 1);
    assert!(close(tips[0].x, m) && close(tips[0].y, m), "{tips:?}");
    // The same arc read the other way round.
    let back = parts(&drawn(fillet, 10.0, &[[0.0, 0.0], [0.0, r], [m, m], [r, 0.0]], "R5.00", "1:1")).1;
    assert!(close(back[0].x, m) && close(back[0].y, m), "{back:?}");
}

/// A chain is one line of frames at one offset: a shared extension line is drawn once, whichever
/// point is further from the line.
#[test]
fn a_chain_is_one_line_with_each_extension_line_once() {
    let kind = DimensionKind::Chain { points: Vec::new(), direction: LinearDirection::Horizontal };
    let member = |text: &str| Member { measured: Measured { value: 1.0, foreshortened: false }, text: text.into() };
    let mut r = read(&[[0.0, 0.0], [10.0, 5.0], [30.0, 0.0], [60.0, 8.0]], "60");
    r.text = None;
    r.members = vec![member("10.00"), member("20.00"), member("30.00")];
    let group = laid(&dimension(kind, 10.0, TextPlace::Centred), &r, &view_at("1:1")).unwrap().group;
    let texts: Vec<&str> = runs(&group).iter().map(|run| run.text.as_str()).collect();
    assert_eq!(texts, ["10.00", "20.00", "30.00"], "each member's own");
    let (lines, tips) = parts(&group);
    let line_y = 4.0 + 10.0;
    assert!(tips.iter().all(|t| close(t.y, line_y)), "{tips:?}");
    assert_eq!(tips.len(), 6);
    let upright = lines.iter().filter(|l| close(l[0].x, l[1].x)).count();
    assert_eq!(upright, 4, "one for each point: {lines:?}");
}

/// A chain as long as a chain goes, with tight spans between its points, stays in one group.
#[test]
fn a_chain_of_the_most_points_lays_out() {
    let kind = DimensionKind::Chain { points: Vec::new(), direction: LinearDirection::Aligned };
    let points: Vec<[f64; 2]> = (0..64).map(|i| [i as f64 * 1.5, 0.0]).collect();
    let mut r = read(&points, "");
    r.text = None;
    let group = laid(&dimension(kind, 10.0, TextPlace::Centred), &r, &view_at("1:1")).unwrap().group;
    assert_eq!(parts(&group).1.len(), 2 * 63);
    let scene = Scene::new(Vec2::new(420.0, 297.0), vec![group]);
    assert!(scene.validate().is_ok());
}

/// The arrows go inside at the text's measured length and outside just short of it: a span of the
/// arrows, a gap each side and the text. Texts of five characters flip at their own widths.
#[test]
fn arrows_flip_at_the_measured_length() {
    let tails = |text: &str, span: f64| {
        let (lines, tips) = parts(&drawn(length(LinearDirection::Horizontal), 10.0, &[[0.0, 0.0], [span, 0.0]], text, "1:1"));
        assert_eq!(tips.len(), 2);
        lines.iter().flatten().any(|p| p.x < -1.0 || p.x > span + 1.0)
    };
    for text in ["80.00", "11111", "WWWWW"] {
        let fits = measured(text) + 2.0 * SIZES.gap + 2.0 * SIZES.arrow;
        assert!(!tails(text, fits + 0.01), "{text}: inside just past {fits}");
        assert!(tails(text, fits - 0.01), "{text}: outside just short of {fits}");
    }
    assert!(measured("WWWWW") > measured("11111") + 1.0, "they differ");
}

/// Set along its line at 0, 45, 90 and 135 degrees: the text reads along the line from the bottom of
/// the sheet, or from the right when it is upright, its middle over the line's and a gap clear of
/// it, and it is as wide as sketch text measures it.
#[test]
fn text_runs_along_its_line_and_reads_from_the_bottom_or_the_right() {
    let width = measured("80.00");
    let s = FRAC_1_SQRT_2;
    let lines = [
        ([[0.0, 0.0], [80.0, 0.0]], Vec2::X),
        ([[0.0, 0.0], [80.0, 80.0]], Vec2::new(s, s)),
        ([[0.0, 0.0], [0.0, 80.0]], Vec2::new(0.0, 1.0)),
        ([[80.0, 0.0], [0.0, 80.0]], Vec2::new(s, -s)),
    ];
    for (points, dir) in lines {
        let group = drawn(length(LinearDirection::Aligned), 10.0, &points, "80.00", "1:1");
        let texts = runs(&group);
        assert_eq!(texts.len(), 1);
        let run = texts[0];
        assert_eq!(run.text, "80.00");
        assert!((run.reach() - width).abs() < 1e-9, "{}", run.reach());
        assert!(close(run.angle, dir.angle()), "{} for {dir:?}", run.angle);
        let middle = Vec2::new(points[0][0] + points[1][0], points[0][1] + points[1][1]) * 0.5;
        let start = middle + dir.perp() * (10.0 + SIZES.gap) - dir * (width * 0.5);
        assert!(run.origin.dist(start) < 1e-9, "{:?} for {start:?}", run.origin);
    }
    // Upright, its start is its lower end, so it reads from the right.
    let group = drawn(length(LinearDirection::Aligned), 10.0, &[[0.0, 80.0], [0.0, 0.0]], "80.00", "1:1");
    assert!(close(runs(&group)[0].angle, std::f64::consts::FRAC_PI_2) && runs(&group)[0].origin.y < 40.0, "{:?}", runs(&group));
}

/// A text of two lines stands above the line with its last line's baseline a gap clear of it.
#[test]
fn a_text_of_two_lines_stands_clear_of_its_line() {
    let group = drawn(length(LinearDirection::Horizontal), 10.0, &[[0.0, 0.0], [80.0, 0.0]], "80\n00", "1:1");
    let texts = runs(&group);
    assert_eq!(texts.len(), 2);
    assert!(texts[0].origin.y > texts[1].origin.y, "{texts:?}");
    assert!(close(texts[1].origin.y, 10.0 + SIZES.gap), "{texts:?}");
}

/// The marks a dimension needs are in the built-in font: its Ø, its ° and its R are set as they read,
/// and the fonts it names hold each glyph. A character no font has is said and keeps the lines.
#[test]
fn the_marks_are_found_in_the_font_and_a_missing_glyph_is_reported() {
    let circle = DimensionKind::Radial { edge: edge(), diameter: true };
    let points = [[0.0, 0.0], [6.0, 0.0], [-6.0, 0.0], [6.0, 0.0]];
    let angle = DimensionKind::Angle { a: edge(), b: edge(), reading: Reading::Projected, other_side: false };
    let lines = [[0.0, 0.0], [30.0, 0.0], [0.0, 0.0], [20.0, 20.0]];
    let radius = DimensionKind::Radial { edge: edge(), diameter: false };
    for (kind, points, text) in [(circle.clone(), &points, "\u{d8}12.00"), (angle, &lines, "45.00\u{b0}"), (radius, &points, "R6.00")] {
        let on = laid(&dimension(kind, 10.0, TextPlace::Centred), &read(points, text), &view_at("1:1")).unwrap();
        assert_eq!(on.why, None, "{text}");
        assert_eq!(runs(&on.group)[0].text, text);
        let mut scene = Scene::new(Vec2::new(420.0, 297.0), vec![on.group]);
        scene.add_fonts(on.fonts);
        scene.validate().unwrap();
    }
    // An unassigned code point: no font anywhere has a glyph for it.
    let on = laid(&dimension(circle, 10.0, TextPlace::Centred), &read(&points, "\u{378}"), &view_at("1:1")).unwrap();
    assert!(on.why.as_deref().is_some_and(|why| why.contains("no glyph")), "{:?}", on.why);
    assert!(runs(&on.group).is_empty() && on.fonts.is_empty());
    assert_eq!(parts(&on.group).1.len(), 2, "its arrows");
}

/// A cylinder's dimension runs straight across its silhouettes, beyond the end on its offset's side.
#[test]
fn a_cylinder_is_dimensioned_across_its_silhouettes() {
    let cylinder = || DimensionKind::Cylinder { face: face() };
    // A shaft along X, radius 5, from x = 0 to 40: the end on a positive offset's side is the one
    // furthest along the line's normal.
    let silhouettes = [[0.0, 5.0], [40.0, 5.0], [0.0, -5.0], [40.0, -5.0]];
    let tips = parts(&drawn(cylinder(), 10.0, &silhouettes, "Ø10.00", "1:1")).1;
    assert_eq!(tips.len(), 2);
    assert!(close((tips[0].y - tips[1].y).abs(), 10.0) && close(tips[0].x, tips[1].x), "{tips:?}");
    // Along X the line's normal is -x: a positive offset is left of the shaft's left end.
    let out = |offset: f64| parts(&drawn(cylinder(), offset, &silhouettes, "Ø10.00", "1:1")).1[0].x;
    assert!(close(out(10.0), -10.0) && close(out(-10.0), 50.0), "{} {}", out(10.0), out(-10.0));

    let side = || Silhouette { face: face(), side: SilhouetteSide::Near };
    let between = || DimensionKind::Silhouettes { a: side(), b: side(), direction: LinearDirection::Aligned };
    let two = [[0.0, 20.0], [40.0, 20.0], [0.0, 12.0], [40.0, 12.0]];
    let tips = parts(&drawn(between(), 10.0, &two, "8.00", "1:1")).1;
    assert!(close((tips[0].y - tips[1].y).abs(), 8.0), "{tips:?}");
    let faces = DimensionKind::Faces { a: face(), b: face(), direction: LinearDirection::Aligned };
    let tips = parts(&drawn(faces, 10.0, &two, "8.00", "1:1")).1;
    assert!(close((tips[0].y - tips[1].y).abs(), 8.0), "{tips:?}");
}

/// What isn't found has no lines; a kind with the wrong number of points never lays out.
#[test]
fn a_dimension_with_no_points_has_no_lines() {
    let mut lost = read(&[], "");
    lost.status = Status::Lost("gone".into(), Vec::new());
    for kind in [length(LinearDirection::Aligned), DimensionKind::Radial { edge: edge(), diameter: false }, DimensionKind::Cylinder { face: face() }]
    {
        assert!(laid(&dimension(kind, 10.0, TextPlace::Centred), &lost, &view_at("1:1")).is_none());
    }
    let chain = DimensionKind::Chain { points: Vec::new(), direction: LinearDirection::Aligned };
    assert!(laid(&dimension(chain, 10.0, TextPlace::Centred), &lost, &view_at("1:1")).is_none());
    let three = read(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]], "1");
    assert!(laid(&dimension(length(LinearDirection::Aligned), 1.0, TextPlace::Centred), &three, &view_at("1:1")).is_none());
}

/// A text's own place, far off the sheet's middle, moves the text and the arc it sits on, never the
/// measured span.
#[test]
fn a_texts_place_keeps_the_span() {
    let at = TextPlace::At([1000.0, -500.0]);
    let group =
        laid(&dimension(length(LinearDirection::Horizontal), 10.0, at), &read(&[[0.0, 0.0], [40.0, 0.0]], "40.00"), &view_at("1:1")).unwrap().group;
    let tips = parts(&group).1;
    assert!(close((tips[1].x - tips[0].x).abs(), 40.0) && close(tips[0].y, 10.0), "{tips:?}");
}

/// Any measured points and offsets, at the sizes and shapes that break layouts (zero spans, equal
/// points, reversed or parallel lines, huge and tiny values), lay out or say no without a panic; a
/// scene that takes the group either holds it or says why.
#[test]
fn hostile_points_and_offsets_never_panic() {
    let mut state = 0x4896_9193_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let values = [0.0, -0.0, 1e-300, 1e-9, -1e-9, 1.0, -1.0, 3.0, 6.0, 1e5, -1e5, 1e12, f64::MAX, f64::MIN, f64::NAN, f64::INFINITY];
    let pick = |next: &mut dyn FnMut() -> u64| values[(next() % values.len() as u64) as usize];
    for _ in 0..4000 {
        let count = (next() % 7) as usize;
        let points: Vec<[f64; 2]> = (0..count).map(|_| [pick(&mut next), pick(&mut next)]).collect();
        let offset = pick(&mut next);
        let text = if next() % 2 == 0 { TextPlace::Centred } else { TextPlace::At([pick(&mut next), pick(&mut next)]) };
        let kinds = [
            length(LinearDirection::Aligned),
            length(LinearDirection::Horizontal),
            length(LinearDirection::Along(Vec2::new(pick(&mut next), pick(&mut next)))),
            DimensionKind::Angle { a: edge(), b: edge(), reading: Reading::Projected, other_side: next() % 2 == 0 },
            DimensionKind::Radial { edge: edge(), diameter: next() % 2 == 0 },
            DimensionKind::Chain { points: Vec::new(), direction: LinearDirection::Aligned },
            DimensionKind::Cylinder { face: face() },
        ];
        for kind in kinds {
            let d = dimension(kind, offset, text);
            if let Some(on) = laid(&d, &read(&points, "1"), &view_at("1:5")) {
                let mut scene = Scene::new(Vec2::new(420.0, 297.0), vec![on.group]);
                scene.add_fonts(on.fonts);
                let _ = scene.validate();
            }
        }
    }
}
