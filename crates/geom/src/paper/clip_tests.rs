use std::f64::consts::{FRAC_PI_2, PI};

use super::super::{Clip, Dashes, Group, Item, ObjectKind, Scene};
use crate::bspline::tests::Lcg;
use crate::{Seg2, Vec2};

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn line(a: Vec2, b: Vec2) -> Seg2 {
    Seg2::Line { a, b }
}

fn arc(start: f64, sweep: f64) -> Seg2 {
    Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start, sweep }
}

fn stroke(path: Vec<Seg2>, closed: bool, pattern: Option<Dashes>) -> Item {
    Item::Stroke { path, closed, width_mm: 0.35, pattern }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Clip {
    Clip::Rect { min: p(x0, y0), max: p(x1, y1) }
}

/// What object 1's `items` become cut to `clip` on an A3 sheet.
fn cut(items: Vec<Item>, clip: Clip) -> Vec<Item> {
    let scene = Scene { size_mm: p(420.0, 297.0), groups: vec![Group { object: 1, kind: ObjectKind::View, items }] };
    scene.prepare_clipped(&[(1, clip)]).unwrap().groups()[0].items.clone()
}

/// A piece as numbers: a line's ends, an arc's centre, radius, start and sweep.
fn numbers(seg: &Seg2) -> Vec<f64> {
    match *seg {
        Seg2::Line { a, b } => vec![a.x, a.y, b.x, b.y],
        Seg2::Arc { center, radius, start, sweep } => vec![center.x, center.y, radius, start, sweep],
        _ => panic!("lines and arcs"),
    }
}

/// `a` and `b` the same strokes, each number within 1e-9.
fn same(a: &[Item], b: &[Item]) -> bool {
    let close = |x: &[f64], y: &[f64]| x.len() == y.len() && x.iter().zip(y).all(|(m, n)| (m - n).abs() < 1e-9);
    let dashes = |d: &Option<Dashes>| d.as_ref().map(|d| [d.lengths_mm.clone(), vec![d.phase_mm]].concat());
    a.len() == b.len()
        && a.iter().zip(b).all(|pair| match pair {
            (Item::Stroke { path: p, closed: c, width_mm: w, pattern: d }, Item::Stroke { path: q, closed: e, width_mm: v, pattern: f }) => {
                c == e
                    && w == v
                    && p.len() == q.len()
                    && p.iter().zip(q).all(|(s, t)| close(&numbers(s), &numbers(t)))
                    && match (dashes(d), dashes(f)) {
                        (Some(x), Some(y)) => close(&x, &y),
                        (x, y) => x.is_none() && y.is_none(),
                    }
            }
            _ => false,
        })
}

/// A line keeps what lies inside: cut once by a rectangle, twice by one and by a circle, and kept
/// along a rectangle's edge; one that only touches a circle is dropped.
#[test]
fn a_line_keeps_what_lies_inside() {
    let rule = |a, b| vec![stroke(vec![line(a, b)], false, None)];
    let cases = [
        (rule(p(0.0, 0.0), p(20.0, 0.0)), rect(5.0, -1.0, 30.0, 1.0), rule(p(5.0, 0.0), p(20.0, 0.0))),
        (rule(p(-10.0, 0.0), p(40.0, 0.0)), rect(5.0, -1.0, 30.0, 1.0), rule(p(5.0, 0.0), p(30.0, 0.0))),
        (rule(p(0.0, 0.0), p(20.0, 0.0)), Clip::Circle { center: p(10.0, 0.0), radius: 4.0 }, rule(p(6.0, 0.0), p(14.0, 0.0))),
        (rule(p(0.0, 1.0), p(20.0, 1.0)), rect(5.0, -1.0, 30.0, 1.0), rule(p(5.0, 1.0), p(20.0, 1.0))),
        (rule(p(0.0, 4.0), p(20.0, 4.0)), Clip::Circle { center: p(10.0, 0.0), radius: 4.0 }, vec![]),
    ];
    for (items, clip, expected) in cases {
        let got = cut(items, clip);
        assert!(same(&got, &expected), "{clip:?}: {got:?}");
    }
}

/// A closed square cut by x >= 5: from its corner at the origin, outside, it is one open stroke
/// from (5, 0) round to (5, 10); from (10, 5), inside, the parts either side of the seam rejoin
/// into one, from (5, 0) through the seam to (5, 10). Wholly inside it stays closed and whole;
/// wholly outside it is gone.
#[test]
fn a_closed_stroke_cut_open_rejoins_across_its_seam() {
    let ring = |corners: &[Vec2]| (0..corners.len()).map(|i| line(corners[i], corners[(i + 1) % corners.len()])).collect::<Vec<_>>();
    let clip = rect(5.0, -5.0, 20.0, 20.0);
    let from_origin = ring(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]);
    let expected = [line(p(5.0, 0.0), p(10.0, 0.0)), line(p(10.0, 0.0), p(10.0, 10.0)), line(p(10.0, 10.0), p(5.0, 10.0))];
    assert!(same(&cut(vec![stroke(from_origin, true, None)], clip), &[stroke(expected.to_vec(), false, None)]));
    let mid_side = [
        line(p(10.0, 5.0), p(10.0, 10.0)),
        line(p(10.0, 10.0), p(0.0, 10.0)),
        line(p(0.0, 10.0), p(0.0, 0.0)),
        line(p(0.0, 0.0), p(10.0, 0.0)),
        line(p(10.0, 0.0), p(10.0, 5.0)),
    ];
    let rejoined =
        [line(p(5.0, 0.0), p(10.0, 0.0)), line(p(10.0, 0.0), p(10.0, 5.0)), line(p(10.0, 5.0), p(10.0, 10.0)), line(p(10.0, 10.0), p(5.0, 10.0))];
    assert!(same(&cut(vec![stroke(mid_side.to_vec(), true, None)], clip), &[stroke(rejoined.to_vec(), false, None)]));
    let inside = stroke(ring(&[p(6.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)]), true, None);
    assert_eq!(cut(vec![inside.clone()], clip), vec![inside]);
    assert_eq!(cut(vec![stroke(ring(&[p(0.0, 0.0), p(4.0, 0.0), p(4.0, 4.0)]), true, None)], clip), vec![]);
}

/// Arcs are cut where their circle crosses the edge: a circle of radius 10 (two half turns) by
/// x >= 0 keeps its right half, from 3/2 pi round through the seam at 0 to 1/2 pi; by the circle of
/// radius 10 round (10, 0), which it crosses at -pi/3 and pi/3, it keeps from 5/3 pi to 1/3 pi. A
/// half turn clockwise from the top, cut by y >= 0, keeps its first quarter.
#[test]
fn arcs_are_cut_where_they_cross_the_edge() {
    let circle = || vec![stroke(vec![arc(0.0, PI), arc(PI, PI)], true, None)];
    let got = cut(circle(), rect(0.0, -20.0, 20.0, 20.0));
    assert!(same(&got, &[stroke(vec![arc(1.5 * PI, FRAC_PI_2), arc(0.0, FRAC_PI_2)], false, None)]), "{got:?}");
    let got = cut(circle(), Clip::Circle { center: p(10.0, 0.0), radius: 10.0 });
    assert!(same(&got, &[stroke(vec![arc(5.0 * PI / 3.0, PI / 3.0), arc(0.0, PI / 3.0)], false, None)]), "{got:?}");
    let got = cut(vec![stroke(vec![arc(FRAC_PI_2, -PI)], false, None)], rect(-20.0, 0.0, 20.0, 20.0));
    assert!(same(&got, &[stroke(vec![arc(FRAC_PI_2, -FRAC_PI_2)], false, None)]), "{got:?}");
}

/// A dashed stroke keeps its pattern: a line from 0 to 20 dashed 2-1 from 0.5 into the pattern,
/// cut at x = 4.25, starts 4.75 into it, so 1.75 into a pattern. The closed square from (10, 5),
/// 40 mm round, rejoins across its seam when dashed 2-2 (40 is ten patterns), its part from
/// along 30 then 31 into the pattern, so 3; dashed 2-1, 40 is a third of a pattern over, so its
/// parts stay apart, the first from its start and the second 31 in, so 1.
#[test]
fn a_dashed_stroke_keeps_its_pattern() {
    let dashes = |lengths: &[f64], phase_mm: f64| Some(Dashes { lengths_mm: lengths.to_vec(), phase_mm });
    let got = cut(vec![stroke(vec![line(p(0.0, 0.0), p(20.0, 0.0))], false, dashes(&[2.0, 1.0], 0.5))], rect(4.25, -1.0, 30.0, 1.0));
    assert!(same(&got, &[stroke(vec![line(p(4.25, 0.0), p(20.0, 0.0))], false, dashes(&[2.0, 1.0], 1.75))]), "{got:?}");
    let square = vec![
        line(p(10.0, 5.0), p(10.0, 10.0)),
        line(p(10.0, 10.0), p(0.0, 10.0)),
        line(p(0.0, 10.0), p(0.0, 0.0)),
        line(p(0.0, 0.0), p(10.0, 0.0)),
        line(p(10.0, 0.0), p(10.0, 5.0)),
    ];
    let clip = rect(5.0, -5.0, 20.0, 20.0);
    let (first, second) =
        ([line(p(10.0, 5.0), p(10.0, 10.0)), line(p(10.0, 10.0), p(5.0, 10.0))], [line(p(5.0, 0.0), p(10.0, 0.0)), line(p(10.0, 0.0), p(10.0, 5.0))]);
    let got = cut(vec![stroke(square.clone(), true, dashes(&[2.0, 2.0], 1.0))], clip);
    assert!(same(&got, &[stroke([second.to_vec(), first.to_vec()].concat(), false, dashes(&[2.0, 2.0], 3.0))]), "{got:?}");
    let got = cut(vec![stroke(square, true, dashes(&[2.0, 1.0], 1.0))], clip);
    let expected = [stroke(first.to_vec(), false, dashes(&[2.0, 1.0], 1.0)), stroke(second.to_vec(), false, dashes(&[2.0, 1.0], 1.0))];
    assert!(same(&got, &expected), "{got:?}");
}

/// Without clips the scene is borrowed as it is; with them only the objects named are cut, and
/// one named that the scene doesn't have changes nothing.
#[test]
fn only_the_objects_named_are_cut() {
    let group = |object| Group { object, kind: ObjectKind::View, items: vec![stroke(vec![line(p(0.0, 0.0), p(20.0, 0.0))], false, None)] };
    let scene = Scene { size_mm: p(420.0, 297.0), groups: vec![group(1), group(2), group(1)] };
    let plain = scene.prepare().unwrap();
    assert!(std::ptr::eq(plain.groups().as_ptr(), scene.groups.as_ptr()));
    let clipped = scene.prepare_clipped(&[(1, rect(5.0, -1.0, 30.0, 1.0)), (9, rect(0.0, 0.0, 1.0, 1.0))]).unwrap();
    let kept = [stroke(vec![line(p(5.0, 0.0), p(20.0, 0.0))], false, None)];
    assert!(same(&clipped.groups()[0].items, &kept) && same(&clipped.groups()[2].items, &kept));
    assert_eq!(clipped.groups()[1], group(2));
}

/// Clips outside the module's notes, a second clip for one object and an object with what can't be
/// cut yet are refused; a clip at the coordinate limit is taken.
#[test]
fn what_cant_be_clipped_is_refused() {
    let scene = |items| Scene { size_mm: p(420.0, 297.0), groups: vec![Group { object: 1, kind: ObjectKind::View, items }] };
    let plain = scene(vec![stroke(vec![line(p(0.0, 0.0), p(20.0, 0.0))], false, None)]);
    let bad = "a clip is a rectangle from its lower-left to its upper-right corner, or a circle with a radius above 0, within 1e5 mm";
    for clip in [
        rect(5.0, 0.0, 5.0, 1.0),
        rect(5.0, 1.0, 6.0, 0.0),
        rect(f64::NAN, 0.0, 6.0, 1.0),
        rect(0.0, 0.0, 1e5 + 1.0, 1.0),
        Clip::Circle { center: p(0.0, 0.0), radius: 0.0 },
        Clip::Circle { center: p(0.0, 0.0), radius: -1.0 },
        Clip::Circle { center: p(0.0, 0.0), radius: f64::NAN },
        Clip::Circle { center: p(f64::INFINITY, 0.0), radius: 1.0 },
        Clip::Circle { center: p(0.0, 0.0), radius: 1e5 + 1.0 },
    ] {
        assert_eq!(plain.prepare_clipped(&[(1, clip)]).err().map(|e| e.0), Some(bad), "{clip:?}");
    }
    plain.prepare_clipped(&[(1, rect(-1e5, -1e5, 1e5, 1e5)), (2, Clip::Circle { center: p(1e5, -1e5), radius: 1e5 })]).unwrap();
    let twice = plain.prepare_clipped(&[(1, rect(0.0, 0.0, 1.0, 1.0)), (1, rect(0.0, 0.0, 2.0, 2.0))]);
    assert_eq!(twice.err().map(|e| e.0), Some("an object has one clip"));
    let cubic = Seg2::Cubic { p0: p(0.0, 0.0), p1: p(1.0, 1.0), p2: p(2.0, 1.0), p3: p(3.0, 0.0) };
    let fill = Item::Fill { region: crate::Region2 { outer: crate::Loop2::polygon(&[p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0)]), holes: vec![] } };
    for items in [vec![stroke(vec![cubic], false, None)], vec![fill]] {
        let clipped = scene(items.clone());
        let refused = clipped.prepare_clipped(&[(1, rect(0.0, 0.0, 1.0, 1.0))]).err().map(|e| e.0);
        assert_eq!(refused, Some("a clipped object's items are strokes of lines and arcs"));
        scene(items).prepare_clipped(&[(2, rect(0.0, 0.0, 1.0, 1.0))]).unwrap();
    }
}

/// How far `q` is inside `clip`: below 0 outside.
fn depth(clip: &Clip, q: Vec2) -> f64 {
    match *clip {
        Clip::Rect { min, max } => (q.x - min.x).min(max.x - q.x).min(q.y - min.y).min(max.y - q.y),
        Clip::Circle { center, radius } => radius - q.dist(center),
    }
}

/// Random strokes of lines and arcs, either way round, open and closed, cut by random rectangles
/// and circles: what comes out passes the scene's checks, every piece of it lies inside the clip,
/// every point of the stroke well inside it is drawn, and nothing is longer than it was.
#[test]
fn fuzzed_clips_keep_exactly_what_lies_inside() {
    let mut rng = Lcg(0xa8a);
    let mut r = |lo: f64, hi: f64| lo + (hi - lo) * rng.unit();
    for case in 0..500 {
        let mut path = Vec::new();
        let mut at = p(r(0.0, 100.0), r(0.0, 100.0));
        for _ in 0..1 + case % 6 {
            let seg = if r(0.0, 1.0) < 0.5 {
                line(at, p(r(0.0, 100.0), r(0.0, 100.0)))
            } else {
                let (radius, start, sweep) = (r(1.0, 40.0), r(-PI, PI), r(-2.0 * PI, 2.0 * PI));
                Seg2::Arc { center: at - Vec2::from_angle(start) * radius, radius, start, sweep }
            };
            at = seg.end();
            path.push(seg);
        }
        let closed = path.len() > 1 && r(0.0, 1.0) < 0.3;
        if closed && let (Some(first), Some(last)) = (path.first().map(Seg2::start), path.last().map(Seg2::end)) {
            path.push(line(last, first));
        }
        path.retain(|seg| seg.length() > 1e-6);
        if path.is_empty() {
            continue;
        }
        let clip = if case % 2 == 0 {
            let (x, y) = (r(0.0, 100.0), r(0.0, 100.0));
            rect(x, y, x + r(1.0, 80.0), y + r(1.0, 80.0))
        } else {
            Clip::Circle { center: p(r(0.0, 100.0), r(0.0, 100.0)), radius: r(1.0, 60.0) }
        };
        let length: f64 = path.iter().map(Seg2::length).sum();
        let scene = Scene {
            size_mm: p(420.0, 297.0),
            groups: vec![Group { object: 3, kind: ObjectKind::View, items: vec![stroke(path.clone(), closed, None)] }],
        };
        let Ok(prepared) = scene.prepare_clipped(&[(3, clip)]) else { panic!("case {case}: refused") };
        let out: Vec<&Seg2> = prepared.groups()[0]
            .items
            .iter()
            .flat_map(|item| match item {
                Item::Stroke { path, .. } => path.iter(),
                Item::Fill { .. } => panic!("strokes"),
            })
            .collect();
        for seg in &out {
            assert!(depth(&clip, seg.point_at(0.5)) > -1e-6, "case {case}: {seg:?} outside {clip:?}");
        }
        for seg in &path {
            for k in 0..=16 {
                let q = seg.point_at(k as f64 / 16.0);
                if depth(&clip, q) > 1e-6 {
                    let drawn = out.iter().any(|s| s.point_at(s.closest_param(q)).dist(q) < 1e-6);
                    assert!(drawn, "case {case}: {q:?} inside {clip:?} isn't drawn");
                }
            }
        }
        assert!(out.iter().map(|s| s.length()).sum::<f64>() <= length + 1e-9, "case {case}");
    }
}

/// What the cuts make is checked again: a zigzag of 60,000 lines between y = -1 and 1 is 60,002
/// records, and cut to |y| <= 0.5 each line's middle is its own stroke, 120,001 in all, more than
/// a scene holds, so it is refused rather than handed to a writer.
#[test]
fn cuts_that_make_too_many_records_are_refused() {
    let n = 60_000;
    let corner = |i: usize| p(i as f64 * 1e-3, if i.is_multiple_of(2) { -1.0 } else { 1.0 });
    let path: Vec<Seg2> = (0..n).map(|i| line(corner(i), corner(i + 1))).collect();
    let scene = Scene { size_mm: p(420.0, 297.0), groups: vec![Group { object: 1, kind: ObjectKind::View, items: vec![stroke(path, false, None)] }] };
    scene.prepare().unwrap();
    let refused = scene.prepare_clipped(&[(1, rect(-1.0, -0.5, 100.0, 0.5))]);
    assert_eq!(refused.err().map(|e| e.0), Some("more than 100,000 groups, items, loops and pieces"));
}
