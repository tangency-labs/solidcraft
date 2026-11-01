//! A dimension from a link keeps its side when the link moves: the solve after the links are
//! refreshed reads sides and turns where the points were stored ([`Sketch::positions_in`]),
//! not beside a line that has already moved past them.

use super::*;
use crate::{
    Analysis, ConstraintKind, SolveReport, flips_from_stored, side_flips_from_stored, solve, solve_after_edit, solve_after_link_move,
    solve_after_value_edit,
};

fn source() -> LinkSource {
    LinkSource::SketchCurve { sketch: 1, curve: "rail".into() }
}

fn rail(height: f64) -> Vec<LinkGeom> {
    vec![LinkGeom::Line(Vec2::new(0.0, height), Vec2::new(80.0, height))]
}

/// What the evaluation does with a stored sketch whose link's source moved: the link goes
/// where the source is, then the sketch is solved from its stored state. `kept`: the solve is
/// given the stored positions.
fn moved(stored: &Sketch, link: &str, geom: &[LinkGeom], kept: bool) -> Sketch {
    let mut sk = stored.clone();
    assert!(!sk.update_link(link, geom).unwrap(), "the link keeps its points");
    let before = if kept { sk.positions_in(stored) } else { None };
    assert!(solve_after_edit(&mut sk, &stored.driving_values(), before.as_deref()).keeps_orientation());
    sk
}

fn near(at: Vec2, x: f64, y: f64) -> bool {
    at.dist(Vec2::new(x, y)) < 1e-9
}

#[test]
fn a_point_dimensioned_from_a_linked_line_keeps_its_side_when_the_line_moves_past() {
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, source(), &rail(50.0)).unwrap();
    let l = sk.link_curves(&link)[0];
    let p = sk.add_point(Vec2::new(40.0, 45.0), None).unwrap();
    sk.add_constraint(ConstraintKind::DistanceX { p: 0, q: p, value: 40.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::PointLineDistance { p, l, value: 5.0 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    // Past the point (20, 6, 44), away from it (400) and not as far as it (48).
    for height in [20.0, 6.0, 44.0, 400.0, 48.0] {
        let at = moved(&sk, &link, &rail(height), true).points[p].pos;
        assert!(near(at, 40.0, height - 5.0), "{height}: {at:?}");
    }
    // Read where the line is now, the stored point is above it: the solution on that side.
    let at = moved(&sk, &link, &rail(20.0), false).points[p].pos;
    assert!(near(at, 40.0, 25.0), "{at:?}");
}

#[test]
fn a_dimension_along_an_axis_keeps_its_side_of_a_linked_point_that_moves_past() {
    for upright in [true, false] {
        let end = |h: f64| vec![LinkGeom::Point(if upright { Vec2::new(80.0, h) } else { Vec2::new(h, 80.0) })];
        let mut sk = Sketch::new();
        let link = sk.add_link(LinkKind::Project, LinkSource::SketchPoint { sketch: 1, point: "end".into() }, &end(50.0)).unwrap();
        let q = sk.link_points(&link)[0];
        let p = sk.add_point(if upright { Vec2::new(40.0, 45.0) } else { Vec2::new(45.0, 40.0) }, None).unwrap();
        if upright {
            sk.add_constraint(ConstraintKind::DistanceX { p: 0, q: p, value: 40.0 }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceY { p: q, q: p, value: 5.0 }, None).unwrap();
        } else {
            sk.add_constraint(ConstraintKind::DistanceY { p: 0, q: p, value: 40.0 }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceX { p: q, q: p, value: 5.0 }, None).unwrap();
        }
        assert!(solve(&mut sk).ok());
        for height in [20.0, 6.0, 400.0] {
            let at = moved(&sk, &link, &end(height), true).points[p].pos;
            let (along, across) = if upright { (at.y, at.x) } else { (at.x, at.y) };
            assert!((along - (height - 5.0)).abs() < 1e-9 && (across - 40.0).abs() < 1e-9, "upright {upright}, {height}: {at:?}");
        }
        let at = moved(&sk, &link, &end(20.0), false).points[p].pos;
        assert!(((if upright { at.y } else { at.x }) - 25.0).abs() < 1e-9, "upright {upright}: {at:?}");
    }
}

/// A corner 50 from both ends of a linked line: below the line, or its mirror image above.
#[test]
fn a_corner_dimensioned_from_two_linked_points_keeps_its_turn_when_they_move_past() {
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, source(), &rail(50.0)).unwrap();
    let ends = sk.link_points(&link);
    let p = sk.add_point(Vec2::new(40.0, 20.0), None).unwrap();
    for end in ends {
        sk.add_constraint(ConstraintKind::Distance { p: end, q: p, value: 50.0 }, None).unwrap();
    }
    assert!(solve(&mut sk).ok());
    assert!(near(sk.points[p].pos, 40.0, 20.0));
    for height in [10.0, -200.0, 300.0] {
        let at = moved(&sk, &link, &rail(height), true).points[p].pos;
        assert!(near(at, 40.0, height - 30.0), "{height}: {at:?}");
    }
    let at = moved(&sk, &link, &rail(10.0), false).points[p].pos;
    assert!(near(at, 40.0, 40.0), "{at:?}");
}

#[test]
fn stored_positions_are_matched_by_point_id() {
    let mut stored = Sketch::new();
    let link = stored.add_link(LinkKind::Project, source(), &rail(50.0)).unwrap();
    let p = stored.add_point(Vec2::new(40.0, 45.0), Some("p")).unwrap();
    assert_eq!(stored.positions_in(&stored), None, "nothing moved");

    let mut sk = stored.clone();
    sk.update_link(&link, &rail(20.0)).unwrap();
    let was: Vec<Vec2> = stored.points.iter().map(|q| q.pos).collect();
    assert_eq!(sk.positions_in(&stored), Some(was));

    // A link of another shape is built anew: its points are new, and the points after it
    // change places in the list.
    let mut sk = stored.clone();
    let bent = [LinkGeom::Line(Vec2::new(0.0, 20.0), Vec2::new(40.0, 30.0)), LinkGeom::Line(Vec2::new(40.0, 30.0), Vec2::new(80.0, 20.0))];
    assert!(sk.update_link(&link, &bent).unwrap());
    let now = sk.point_index("p").unwrap();
    assert_ne!(now, p);
    assert_eq!(sk.positions_in(&stored), None, "the new points have no stored place and `p` has not moved");
    sk.points[now].pos = Vec2::new(60.0, 45.0);
    let was = sk.positions_in(&stored).unwrap();
    assert_eq!(was[now], Vec2::new(40.0, 45.0));
    for i in sk.link_points(&link) {
        assert_eq!(was[i], sk.points[i].pos);
    }
}

/// A 40 mm horizontal line tangent to a linked circle (r 3) above it, its left end 20 mm left
/// of the centre; the stored sketch with the circle where it was (`y`).
fn tangent_on_circle(y: f64) -> (Sketch, String, usize) {
    let circle = |y: f64| vec![LinkGeom::Circle(Vec2::new(0.0, y), 3.0)];
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, LinkSource::SketchCurve { sketch: 1, curve: "bend".into() }, &circle(y)).unwrap();
    let c = sk.link_curves(&link)[0];
    let centre = sk.link_points(&link)[0];
    let l = sk.add_line(Vec2::new(-20.0, y + 3.0), Vec2::new(20.0, y + 3.0), None, None, None).unwrap();
    let (a, _) = sk.curves[l].kind.ends().unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: l, b: c }, None).unwrap();
    sk.add_constraint(ConstraintKind::DistanceX { p: a, q: centre, value: 20.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Length { l, value: 40.0 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    (sk, link, l)
}

#[test]
fn a_line_tangent_to_a_linked_circle_keeps_its_side_when_the_circle_moves_far() {
    let (stored, link, l) = tangent_on_circle(0.0);
    let height = |sk: &Sketch| match sk.curves[l].kind {
        CurveKind::Line { a, .. } => sk.points[a].pos.y,
        _ => f64::NAN,
    };
    assert!((height(&stored) - 3.0).abs() < 1e-9);
    let circle = |y: f64| vec![LinkGeom::Circle(Vec2::new(0.0, y), 3.0)];
    // Moved 100 mm up, the circle passes the line. Its other tangent (y 97, a diameter below)
    // solves every constraint too: only the side it left tells it from the right one.
    let mut mirrored = stored.clone();
    mirrored.update_link(&link, &circle(100.0)).unwrap();
    for p in &mut mirrored.points {
        if p.link.is_none() && p.id != "origin" {
            p.pos.y += 94.0;
        }
    }
    assert!((height(&mirrored) - 97.0).abs() < 1e-9);
    assert!(solve(&mut mirrored).ok() && (height(&mirrored) - 97.0).abs() < 1e-9);
    assert!(!flips_from_stored(&mirrored, &stored).is_empty());
    // Walked there, the line stays above the circle, in either direction and far.
    for y in [100.0, -100.0, 2000.0] {
        let mut sk = stored.clone();
        sk.update_link(&link, &circle(y)).unwrap();
        let report = solve_after_link_move(&mut sk, &stored, &stored.driving_values(), Analysis::Full);
        assert!(report.is_some_and(|r| r.keeps_orientation()), "{y}");
        assert!((height(&sk) - (y + 3.0)).abs() < 1e-9 && flips_from_stored(&sk, &stored).is_empty(), "{y}: {}", height(&sk));
    }
    // Nothing moved: nothing to walk.
    let mut sk = stored.clone();
    assert!(solve_after_link_move(&mut sk, &stored, &stored.driving_values(), Analysis::Full).is_none());
}

/// A point held on a linked rail 3 mm from another point on it (a distance between the two):
/// the same distance back past the other point solves every constraint too, and only the side
/// it was drawn on tells them apart. The other point is held on the rail itself, or lies on it
/// as the mirror image of a point on it across a line square to it. The mirror image counts as
/// a flip, and the rail walked far (steeper, higher, the other way up) keeps the point on its side.
#[test]
fn a_point_a_distance_along_a_linked_line_keeps_its_side() {
    let slant = |y0: f64, y1: f64| vec![LinkGeom::Line(Vec2::new(0.0, y0), Vec2::new(80.0, y1))];
    for mirrored_on in [false, true] {
        let mut sk = Sketch::new();
        let link = sk.add_link(LinkKind::Project, source(), &slant(10.0, 50.0)).unwrap();
        let rail = sk.link_curves(&link)[0];
        let a = sk.add_point(Vec2::new(20.0, 20.0), None).unwrap();
        let h = sk.add_point(Vec2::new(23.0, 21.5), None).unwrap();
        if mirrored_on {
            // `a` mirrors `b` (on the rail, x 0..) across a line square to the rail at x 10.
            let b = sk.add_point(Vec2::new(0.5, 10.0), None).unwrap();
            let mh = sk.add_line(Vec2::new(10.0, 15.0), Vec2::new(5.5, 24.0), None, None, None).unwrap();
            let (m, _) = sk.curves[mh].kind.ends().unwrap();
            sk.add_constraint(ConstraintKind::PointOnCurve { p: b, c: rail }, None).unwrap();
            sk.add_constraint(ConstraintKind::PointOnCurve { p: m, c: rail }, None).unwrap();
            sk.add_constraint(ConstraintKind::Perpendicular { a: mh, b: rail }, None).unwrap();
            sk.add_constraint(ConstraintKind::Length { l: mh, value: 10.0 }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceX { p: 0, q: m, value: 10.0 }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceX { p: 0, q: b, value: 0.5 }, None).unwrap();
            sk.add_constraint(ConstraintKind::Symmetric { p: b, q: a, l: mh }, None).unwrap();
        } else {
            sk.add_constraint(ConstraintKind::PointOnCurve { p: a, c: rail }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceX { p: 0, q: a, value: 20.0 }, None).unwrap();
        }
        sk.add_constraint(ConstraintKind::PointOnCurve { p: h, c: rail }, None).unwrap();
        let span = sk.add_constraint(ConstraintKind::Distance { p: a, q: h, value: 3.0 }, None).unwrap();
        assert!(solve(&mut sk).ok(), "{mirrored_on}");
        let ahead = |sk: &Sketch| match sk.curves[rail].kind {
            CurveKind::Line { a: s, b: e } => (sk.points[e].pos - sk.points[s].pos).dot(sk.points[h].pos - sk.points[a].pos),
            _ => f64::NAN,
        };
        assert!(ahead(&sk) > 0.0 && (sk.points[h].pos.dist(sk.points[a].pos) - 3.0).abs() < 1e-9, "{mirrored_on}");
        let stored = sk.clone();
        // The same 3 mm back past `a`: solved as it is, and told apart only by the side.
        let mut back = stored.clone();
        back.points[h].pos = back.points[a].pos * 2.0 - back.points[h].pos;
        assert!(solve(&mut back).ok() && ahead(&back) < 0.0, "{mirrored_on}");
        assert!(flips_from_stored(&back, &stored).contains(&span), "{mirrored_on}: {:?}", flips_from_stored(&back, &stored));
        // Walked there, `h` stays ahead of `a` along the rail.
        for (y0, y1) in [(300.0, 260.0), (-40.0, 400.0), (2000.0, 1990.0)] {
            let mut sk = stored.clone();
            sk.update_link(&link, &slant(y0, y1)).unwrap();
            let report = solve_after_link_move(&mut sk, &stored, &stored.driving_values(), Analysis::Full);
            assert!(report.is_some_and(|r| r.keeps_orientation()), "{mirrored_on} {y0} {y1}");
            assert!(ahead(&sk) > 0.0 && flips_from_stored(&sk, &stored).is_empty(), "{mirrored_on} {y0} {y1}: {}", ahead(&sk));
        }
    }
}

/// An ellipse (12 by 10) centred where two linked walls cross (`rail` upright, `wall` level), its
/// major axis ending on a third linked line (`tan`, 12 right of `rail`); a line from its centre to
/// it parallel to `rail`; and a chord 5 from the centre square to `rail`. Drawn with the line up
/// the rail and the chord pointing left. The sketch, its links (rail, wall, tan), the ellipse's
/// centre, the line, the chord and the parallel and perpendicular constraints.
struct Bore {
    sk: Sketch,
    links: [String; 3],
    centre: usize,
    line: usize,
    chord: usize,
    parallel: String,
    square: String,
}

fn upright(x: f64) -> Vec<LinkGeom> {
    vec![LinkGeom::Line(Vec2::new(x, -50.0), Vec2::new(x, 50.0))]
}

fn level(y: f64) -> Vec<LinkGeom> {
    vec![LinkGeom::Line(Vec2::new(-50.0, y), Vec2::new(50.0, y))]
}

fn beside(x: f64) -> Vec<LinkGeom> {
    upright(x + 12.0)
}

fn bore() -> Bore {
    let mut sk = Sketch::new();
    let walls = [("rail", upright(0.0)), ("wall", level(0.0)), ("tan", beside(0.0))];
    let links = walls.map(|(name, geom)| sk.add_link(LinkKind::Project, LinkSource::SketchCurve { sketch: 1, curve: name.into() }, &geom).unwrap());
    let [r, w, t] = [0, 1, 2].map(|i| sk.link_curves(&links[i])[0]);
    let centre = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let m = sk.add_point(Vec2::new(12.0, 0.0), None).unwrap();
    let e = sk.add_curve(CurveKind::Ellipse { c: centre, m, r: 10.0 }, None).unwrap();
    for (p, c) in [(centre, r), (centre, w), (m, t), (m, w)] {
        sk.add_constraint(ConstraintKind::PointOnCurve { p, c }, None).unwrap();
    }
    sk.add_constraint(ConstraintKind::Radius { c: e, value: 10.0 }, None).unwrap();
    let line = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(0.0, 10.0), None, None, None).unwrap();
    let (l0, l1) = sk.curves[line].kind.ends().unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: l0, q: centre }, None).unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: l1, c: e }, None).unwrap();
    let parallel = sk.add_constraint(ConstraintKind::Parallel { a: line, b: r }, None).unwrap();
    let chord = sk.add_line(Vec2::new(8.0, 5.0), Vec2::new(-8.0, 5.0), None, None, None).unwrap();
    let (k0, k1) = sk.curves[chord].kind.ends().unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: k0, c: e }, None).unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: k1, c: e }, None).unwrap();
    let square = sk.add_constraint(ConstraintKind::Perpendicular { a: chord, b: r }, None).unwrap();
    sk.add_constraint(ConstraintKind::PointLineDistance { p: centre, l: chord, value: 5.0 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let bore = Bore { sk, links, centre, line, chord, parallel, square };
    assert_eq!(bore.turn(&bore.sk), (1.0, -1.0));
    bore
}

impl Bore {
    /// Which way the line points along y from the centre, and the chord along x (±1).
    fn turn(&self, sk: &Sketch) -> (f64, f64) {
        let dir = |l: usize| match sk.curves[l].kind {
            CurveKind::Line { a, b } => sk.points[b].pos - sk.points[a].pos,
            _ => Vec2::new(f64::NAN, f64::NAN),
        };
        let up = sk.points[self.centre].pos.dist(sk.points[sk.curves[self.line].kind.ends().unwrap().1].pos);
        assert!((up - 10.0).abs() < 1e-6 && (dir(self.chord).len() - 2.0 * 75.0f64.sqrt() * 1.2).abs() < 1e-6, "{up} {}", dir(self.chord).len());
        (dir(self.line).y.signum(), dir(self.chord).x.signum())
    }

    /// The walls moved: `rail` and `tan` to `x`, `wall` to `y`.
    fn moved(&self, sk: &mut Sketch, x: f64, y: f64) {
        for (link, geom) in self.links.iter().zip([upright(x), level(y), beside(x)]) {
            assert!(!sk.update_link(link, &geom).unwrap(), "the link keeps its points");
        }
    }

    /// [`follow`] the walls to (`x`, `y`).
    fn follow(&self, x: f64, y: f64) -> Sketch {
        follow(&self.sk, |sk| self.moved(sk, x, y))
    }
}

/// What a document's evaluation does with a stored sketch when `change` moves its links (and
/// may set new values): the sketch solved from the stored state and, when that solve fails or
/// turns something over, walked there; a side or turn the walk cannot keep either is reported
/// as folded (a turned sense only asks for the walk). Whether a walk was tried, and got there.
fn evaluate(stored: &Sketch, change: impl Fn(&mut Sketch)) -> (Sketch, SolveReport, Option<bool>) {
    let mut sk = stored.clone();
    change(&mut sk);
    let before = sk.positions_in(stored);
    let mut report = solve_after_edit(&mut sk, &stored.driving_values(), before.as_deref());
    let flips = if report.ok() { flips_from_stored(&sk, stored) } else { Vec::new() };
    let mut walked = None;
    if !report.keeps_orientation() || !flips.is_empty() {
        walked = Some(false);
        match solve_after_link_move(&mut sk, stored, &stored.driving_values(), Analysis::Full) {
            Some(r) => (report, walked) = (r, Some(true)),
            None => {
                let sides = if flips.is_empty() { Vec::new() } else { side_flips_from_stored(&sk, stored) };
                for id in sides {
                    if !report.failing.contains(&id) {
                        report.failing.push(id.clone());
                    }
                    if !report.folded.contains(&id) {
                        report.folded.push(id);
                    }
                }
            }
        }
    }
    (sk, report, walked)
}

/// [`evaluate`], which must get there with nothing to report, walked there if it walks.
#[track_caller]
fn follow(stored: &Sketch, change: impl Fn(&mut Sketch)) -> Sketch {
    let (sk, report, walked) = evaluate(stored, change);
    assert!(report.keeps_orientation() && report.failing.is_empty(), "{:?} failing {:?} folded {:?}", report.status, report.failing, report.folded);
    assert_ne!(walked, Some(false), "the walk did not get there");
    sk
}

/// A line from the centre of an ellipse to it, parallel to a linked line, reaches it at either end
/// of a diameter; a chord square to that line, its distance from the centre held, runs either way.
/// Turned half round the centre, both solve every constraint and every side is kept (the centre is
/// on the same side of the chord): only which way each points along the linked line tells the two
/// apart. A solve from the stored state after the walls moved far landed on the turned image and
/// reported it solved; the image counts as a flip now, and the walls walked far keep the senses.
#[test]
fn a_line_parallel_or_square_to_a_linked_line_keeps_its_sense_when_the_link_moves_far() {
    let bore = bore();
    let stored = &bore.sk;
    let mut turned = stored.clone();
    let c = turned.points[bore.centre].pos;
    for l in [bore.line, bore.chord] {
        let (a, b) = turned.curves[l].kind.ends().unwrap();
        for p in [a, b] {
            turned.points[p].pos = c * 2.0 - turned.points[p].pos;
        }
    }
    assert!(solve(&mut turned).ok() && bore.turn(&turned) == (-1.0, 1.0));
    let flips = flips_from_stored(&turned, stored);
    assert!(flips.contains(&bore.parallel) && flips.contains(&bore.square), "{flips:?}");
    // A turned sense asks for the walk; it is not a turn the walk must report when it fails.
    assert!(side_flips_from_stored(&turned, stored).is_empty());
    // Far up the rail and off to the side, one jump lands on the turned image (a little way up,
    // on the chord's other side): walked there, both senses are kept.
    for (x, y) in [(0.0, 1000.0), (30.0, 40.0), (0.0, 15.0), (-108.0, 213.7), (-240.0, 0.0), (0.0, -100.0)] {
        let sk = bore.follow(x, y);
        assert_eq!(bore.turn(&sk), (1.0, -1.0), "({x}, {y})");
        assert!(flips_from_stored(&sk, stored).is_empty(), "({x}, {y}): {:?}", flips_from_stored(&sk, stored));
    }
}

/// A value edit with the links moved too: the walk after the link move starts from the new
/// values solved where the links were stored, and keeps the senses that solve has. The bore made
/// smaller keeps its senses, also with its walls far away. A dimension lifting an ellipse's
/// centre up a linked rail takes a line from the centre to it, parallel to the rail, wherever the
/// value edit alone takes it (a value edit keeps no sense of a parallel): moved with the rail as
/// well, the sketch gets there the same way, not refused.
#[test]
fn a_value_edit_with_the_links_moved_keeps_the_senses_of_the_value_edit() {
    let bore = bore();
    let smaller = |sk: &mut Sketch| {
        for c in &mut sk.constraints {
            if let ConstraintKind::Radius { value, .. } | ConstraintKind::PointLineDistance { value, .. } = &mut c.kind {
                *value *= 0.6;
            }
        }
    };
    let stored = &bore.sk;
    for (x, y) in [(0.0, 0.0), (0.0, 1000.0), (30.0, 40.0)] {
        let sk = follow(stored, |sk| {
            smaller(sk);
            if (x, y) != (0.0, 0.0) {
                bore.moved(sk, x, y);
            }
        });
        let dir = |l: usize| match sk.curves[l].kind {
            CurveKind::Line { a, b } => sk.points[b].pos - sk.points[a].pos,
            _ => Vec2::new(f64::NAN, f64::NAN),
        };
        assert!((dir(bore.line).len() - 6.0).abs() < 1e-6, "({x}, {y}): {:?}", dir(bore.line));
        assert!(dir(bore.line).y > 0.0 && dir(bore.chord).x < 0.0, "({x}, {y}): {:?} {:?}", dir(bore.line), dir(bore.chord));
    }

    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, source(), &upright(0.0)).unwrap();
    let r = sk.link_curves(&link)[0];
    let c = sk.add_point(Vec2::new(0.0, 1.0), None).unwrap();
    let m = sk.add_point(Vec2::new(12.0, 1.0), None).unwrap();
    let e = sk.add_curve(CurveKind::Ellipse { c, m, r: 10.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: c, c: r }, None).unwrap();
    let lift = sk.add_constraint(ConstraintKind::DistanceY { p: 0, q: c, value: 1.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::HorizontalPoints { p: c, q: m }, None).unwrap();
    sk.add_constraint(ConstraintKind::DistanceX { p: c, q: m, value: 12.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Radius { c: e, value: 10.0 }, None).unwrap();
    let l = sk.add_line(Vec2::new(0.0, 1.0), Vec2::new(0.0, 11.0), Some(c), None, None).unwrap();
    let (_, l1) = sk.curves[l].kind.ends().unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: l1, c: e }, None).unwrap();
    sk.add_constraint(ConstraintKind::Parallel { a: l, b: r }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let stored = sk.clone();
    let lift = stored.constraints.iter().position(|k| k.id == lift).unwrap();
    let up = |sk: &Sketch| sk.points[l1].pos.y - sk.points[c].pos.y;
    for value in [20.0, 1000.0] {
        let lifted = |sk: &mut Sketch| sk.constraints[lift].kind.set_value(value);
        let mut alone = stored.clone();
        lifted(&mut alone);
        assert!(solve_after_value_edit(&mut alone, &stored.driving_values()).keeps_orientation());
        let sk = follow(&stored, |sk| {
            lifted(sk);
            assert!(!sk.update_link(&link, &upright(5.0)).unwrap());
        });
        assert!((sk.points[c].pos.y - value).abs() < 1e-9 && (up(&sk) - up(&alone)).abs() < 1e-6, "{value}: {} {}", up(&sk), up(&alone));
    }
}

/// A line drawn between two linked points, and a line from the centre of an ellipse to it held
/// parallel to that line. Turned by its points (the end about the start, or both swapped), the
/// line between them turns the parallel one with it, and how it does so is the source's: turned
/// up to nearly half round (one jump lands on the other sense past a quarter), it is walked
/// there and keeps its sense along the turned line; swapped, it comes back end for end, and the
/// parallel line, which keeps its place, is no flip: the sketch follows without a walk (walked,
/// the two points would pass through each other).
#[test]
fn a_line_parallel_to_a_line_between_linked_points_follows_them_turned_or_swapped() {
    let mut sk = Sketch::new();
    let ends = [("start", Vec2::new(-20.0, 0.0)), ("end", Vec2::new(-20.0, 40.0))]
        .map(|(name, at)| sk.add_link(LinkKind::Project, LinkSource::SketchPoint { sketch: 1, point: name.into() }, &[LinkGeom::Point(at)]).unwrap());
    let [p, q] = [0, 1].map(|i| sk.link_points(&ends[i])[0]);
    let g = sk.add_line(Vec2::new(-20.0, 0.0), Vec2::new(-20.0, 40.0), None, None, None).unwrap();
    let (g0, g1) = sk.curves[g].kind.ends().unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: g0, q: p }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: g1, q }, None).unwrap();
    let m = sk.add_point(Vec2::new(12.0, 0.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: m }, None).unwrap();
    let e = sk.add_curve(CurveKind::Ellipse { c: 0, m, r: 10.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Radius { c: e, value: 10.0 }, None).unwrap();
    let l = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(0.0, 10.0), Some(0), None, None).unwrap();
    let (_, l1) = sk.curves[l].kind.ends().unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: l1, c: e }, None).unwrap();
    sk.add_constraint(ConstraintKind::Parallel { a: l, b: g }, None).unwrap();
    assert!(solve(&mut sk).ok() && sk.points[l1].pos.dist(Vec2::new(0.0, 10.0)) < 1e-6);
    let stored = sk.clone();
    let sense = |sk: &Sketch| (sk.points[q].pos - sk.points[p].pos).dot(sk.points[l1].pos - sk.points[0].pos);
    for degrees in [30.0_f64, 100.0, 150.0, 177.0] {
        let turn = (90.0 + degrees).to_radians();
        let at = Vec2::new(-20.0 + 40.0 * turn.cos(), 40.0 * turn.sin());
        let sk = follow(&stored, |sk| assert!(!sk.update_link(&ends[1], &[LinkGeom::Point(at)]).unwrap()));
        assert!(sense(&sk) > 0.0, "{degrees}: {:?}", sk.points[l1].pos);
    }
    for (low, high) in [(0.0, 40.0), (1000.0, 1040.0)] {
        let swapped = |sk: &mut Sketch| {
            for (link, y) in ends.iter().zip([high, low]) {
                assert!(!sk.update_link(link, &[LinkGeom::Point(Vec2::new(-20.0, y))]).unwrap());
            }
        };
        let mut jump = stored.clone();
        swapped(&mut jump);
        let before = jump.positions_in(&stored);
        assert!(solve_after_edit(&mut jump, &stored.driving_values(), before.as_deref()).keeps_orientation());
        assert!(flips_from_stored(&jump, &stored).is_empty(), "{low}: {:?}", flips_from_stored(&jump, &stored));
        let (sk, report, walked) = evaluate(&stored, swapped);
        assert!(
            report.keeps_orientation() && walked.is_none() && sk.points[l1].pos.dist(Vec2::new(0.0, 10.0)) < 1e-6,
            "{low}: {:?}",
            sk.points[l1].pos
        );
    }
}

/// A rectangle (a right angle and two parallels, its bottom side level) whose corner `a` is held
/// on one linked point and the opposite corner `c` on another: the links alone place it. Points
/// `a`, `b`, `c`, `d` and the two links.
fn rectangle_between(p: Vec2, q: Vec2) -> (Sketch, [usize; 4], [String; 2]) {
    let mut sk = Sketch::new();
    let links = [("start", p), ("end", q)]
        .map(|(name, at)| sk.add_link(LinkKind::Project, LinkSource::SketchPoint { sketch: 1, point: name.into() }, &[LinkGeom::Point(at)]).unwrap());
    let [lp, lq] = [0, 1].map(|i| sk.link_points(&links[i])[0]);
    let corners = [p, Vec2::new(q.x, p.y), q, Vec2::new(p.x, q.y)].map(|at| sk.add_point(at, None).unwrap());
    let [a, b, c, d] = corners;
    let sides = [(a, b), (b, c), (c, d), (d, a)].map(|(x, y)| sk.add_line_pts(x, y, None).unwrap());
    sk.add_constraint(ConstraintKind::Coincident { p: a, q: lp }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: c, q: lq }, None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l: sides[0] }, None).unwrap();
    sk.add_constraint(ConstraintKind::Perpendicular { a: sides[0], b: sides[1] }, None).unwrap();
    sk.add_constraint(ConstraintKind::Parallel { a: sides[0], b: sides[2] }, None).unwrap();
    sk.add_constraint(ConstraintKind::Parallel { a: sides[1], b: sides[3] }, None).unwrap();
    assert!(solve(&mut sk).ok());
    (sk, corners, links)
}

/// The links place every corner of the rectangle, so where they cross over, its sides go through
/// zero length and come back end for end: the only solution, not a flip. Followed with nothing
/// to report, however far the corners go (also both at once), it is the rectangle on the corners
/// where the links are now.
#[test]
fn a_rectangle_between_linked_points_that_cross_follows_them() {
    let (stored, [a, b, c, d], links) = rectangle_between(Vec2::new(0.0, 0.0), Vec2::new(40.0, 20.0));
    let moves = [
        (Vec2::new(0.0, 0.0), Vec2::new(25.0, 20.0)),
        (Vec2::new(0.0, 0.0), Vec2::new(-30.0, 20.0)),
        (Vec2::new(0.0, 0.0), Vec2::new(-40.0, 20.0)),
        (Vec2::new(0.0, 0.0), Vec2::new(-500.0, 20.0)),
        (Vec2::new(0.0, 0.0), Vec2::new(-0.001, 20.0)),
        (Vec2::new(0.0, 0.0), Vec2::new(40.0, -20.0)),
        (Vec2::new(0.0, 0.0), Vec2::new(-30.0, -20.0)),
        (Vec2::new(50.0, 0.0), Vec2::new(10.0, 20.0)),
        (Vec2::new(100.0, 0.0), Vec2::new(0.0, 20.0)),
        (Vec2::new(-10.0, 0.0), Vec2::new(-60.0, 20.0)),
    ];
    for (p, q) in moves {
        let sk = follow(&stored, |sk| {
            for (link, at) in links.iter().zip([p, q]) {
                assert!(!sk.update_link(link, &[LinkGeom::Point(at)]).unwrap());
            }
        });
        for (corner, at) in [(a, p), (b, Vec2::new(q.x, p.y)), (c, q), (d, Vec2::new(p.x, q.y))] {
            assert!(sk.points[corner].pos.dist(at) < 1e-6, "{p:?} {q:?}: {:?} not {at:?}", sk.points[corner].pos);
        }
    }
}

/// A line dropped square from a linked point onto a linked edge (its foot held on the edge, its
/// other end on the point) and, in the same sketch, a line tangent to a linked circle. The point
/// crossing the edge turns the dropped line end for end through zero length (its only solution);
/// moved alone it is followed, and moved together with the circle far away (which needs the walk
/// to keep the tangent on its side), the walk gets there too.
#[test]
fn a_line_dropped_square_from_a_linked_point_follows_it_across_its_edge() {
    let circle = |y: f64| vec![LinkGeom::Circle(Vec2::new(0.0, y), 3.0)];
    let mut sk = Sketch::new();
    let round = sk.add_link(LinkKind::Project, LinkSource::SketchCurve { sketch: 1, curve: "bend".into() }, &circle(0.0)).unwrap();
    let c = sk.link_curves(&round)[0];
    let centre = sk.link_points(&round)[0];
    let l = sk.add_line(Vec2::new(-20.0, 3.0), Vec2::new(20.0, 3.0), None, None, None).unwrap();
    let (la, _) = sk.curves[l].kind.ends().unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: l, b: c }, None).unwrap();
    sk.add_constraint(ConstraintKind::DistanceX { p: la, q: centre, value: 20.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Length { l, value: 40.0 }, None).unwrap();
    let edge = sk.add_link(LinkKind::Project, source(), &[LinkGeom::Line(Vec2::new(100.0, 0.0), Vec2::new(200.0, 0.0))]).unwrap();
    let g = sk.link_curves(&edge)[0];
    let point =
        sk.add_link(LinkKind::Project, LinkSource::SketchPoint { sketch: 1, point: "r".into() }, &[LinkGeom::Point(Vec2::new(150.0, 30.0))]).unwrap();
    let r = sk.link_points(&point)[0];
    let f = sk.add_point(Vec2::new(150.0, 0.0), None).unwrap();
    let e = sk.add_point(Vec2::new(150.0, 30.0), None).unwrap();
    let dropped = sk.add_line_pts(f, e, None).unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: f, c: g }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: e, q: r }, None).unwrap();
    sk.add_constraint(ConstraintKind::Perpendicular { a: dropped, b: g }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let stored = sk.clone();
    for (y, at) in [
        (0.0, Vec2::new(150.0, -30.0)),
        (0.0, Vec2::new(170.0, -5.0)),
        (0.0, Vec2::new(120.0, -300.0)),
        (100.0, Vec2::new(150.0, -20.0)),
        (-100.0, Vec2::new(150.0, -20.0)),
        (2000.0, Vec2::new(150.0, -20.0)),
    ] {
        let moved = |sk: &mut Sketch| {
            if y != 0.0 {
                sk.update_link(&round, &circle(y)).unwrap();
            }
            assert!(!sk.update_link(&point, &[LinkGeom::Point(at)]).unwrap());
        };
        let sk = follow(&stored, moved);
        assert!(sk.points[f].pos.dist(Vec2::new(at.x, 0.0)) < 1e-6, "{y} {at:?}: foot {:?}", sk.points[f].pos);
        assert!((sk.points[la].pos.y - (y + 3.0)).abs() < 1e-6, "{y} {at:?}: tangent at {}", sk.points[la].pos.y);
        if y != 0.0 {
            let mut walked = stored.clone();
            moved(&mut walked);
            let report = solve_after_link_move(&mut walked, &stored, &stored.driving_values(), Analysis::Full);
            assert!(report.is_some_and(|r| r.keeps_orientation()), "{y} {at:?}: not walked");
        }
    }
}

/// Where the line between two linked points stops being a rotation and becomes the source's own
/// swap (the solver's `END_FOR_END`, about 2.6° short of half round): from the positions alone a
/// near swap and a rotation look the same, so the threshold decides, and these pin both sides.
/// Turned 177° about its start, it is a rotation: walked there, the parallel line keeps its sense
/// along it. Turned 178°, or swapped with a skew of a degree either way, it came end for end: the
/// parallel line keeps its place and the sketch follows without a walk.
#[test]
fn a_line_between_linked_points_turned_nearly_half_round_is_a_rotation_up_to_end_for_end() {
    let mut sk = Sketch::new();
    let ends = [("start", Vec2::new(-20.0, 0.0)), ("end", Vec2::new(-20.0, 40.0))]
        .map(|(name, at)| sk.add_link(LinkKind::Project, LinkSource::SketchPoint { sketch: 1, point: name.into() }, &[LinkGeom::Point(at)]).unwrap());
    let [p, q] = [0, 1].map(|i| sk.link_points(&ends[i])[0]);
    let g = sk.add_line(Vec2::new(-20.0, 0.0), Vec2::new(-20.0, 40.0), None, None, None).unwrap();
    let (g0, g1) = sk.curves[g].kind.ends().unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: g0, q: p }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: g1, q }, None).unwrap();
    let m = sk.add_point(Vec2::new(12.0, 0.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: m }, None).unwrap();
    let e = sk.add_curve(CurveKind::Ellipse { c: 0, m, r: 10.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Radius { c: e, value: 10.0 }, None).unwrap();
    let l = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(0.0, 10.0), Some(0), None, None).unwrap();
    let (_, l1) = sk.curves[l].kind.ends().unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: l1, c: e }, None).unwrap();
    sk.add_constraint(ConstraintKind::Parallel { a: l, b: g }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let stored = sk.clone();
    let sense = |sk: &Sketch| (sk.points[q].pos - sk.points[p].pos).dot(sk.points[l1].pos - sk.points[0].pos);
    let turned = |degrees: f64| {
        let turn = (90.0 + degrees).to_radians();
        Vec2::new(-20.0 + 40.0 * turn.cos(), 40.0 * turn.sin())
    };
    // Just short of end for end: a rotation, walked there with its sense.
    let (sk, report, walked) = evaluate(&stored, |sk| assert!(!sk.update_link(&ends[1], &[LinkGeom::Point(turned(177.0))]).unwrap()));
    assert!(report.keeps_orientation() && report.failing.is_empty() && walked == Some(true), "{:?} {walked:?}", report.status);
    assert!(sense(&sk) > 0.0, "{:?}", sk.points[l1].pos);
    // Past it, turned or swapped askew: the source's swap, followed in place without a walk.
    let centre = Vec2::new(-20.0, 20.0);
    let askew = |skew: f64| {
        let (s, c) = (180.0 + skew).to_radians().sin_cos();
        [Vec2::new(-20.0, 0.0), Vec2::new(-20.0, 40.0)].map(|at| {
            let d = at - centre;
            centre + Vec2::new(c * d.x - s * d.y, s * d.x + c * d.y)
        })
    };
    let moves: [[Vec2; 2]; 3] = [[Vec2::new(-20.0, 0.0), turned(178.0)], askew(1.0), askew(-1.0)];
    for [at_p, at_q] in moves {
        let (sk, report, walked) = evaluate(&stored, |sk| {
            for (link, at) in ends.iter().zip([at_p, at_q]) {
                assert!(!sk.update_link(link, &[LinkGeom::Point(at)]).unwrap());
            }
        });
        assert!(report.keeps_orientation() && report.failing.is_empty() && walked.is_none(), "{at_p:?} {at_q:?}: {:?} {walked:?}", report.status);
        assert!(sense(&sk) < 0.0 && sk.points[l1].pos.y > 9.0, "{at_p:?} {at_q:?}: {:?}", sk.points[l1].pos);
    }
}
