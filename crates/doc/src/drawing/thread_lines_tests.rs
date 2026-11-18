use super::*;
use crate::thread_forms::{ThreadForm, ThreadHand};
use solidcore_record::Vec2;
use solidcraft_geom::drawing_view::{NamedView, Scale, ViewFrame, basis_looking};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// A small deterministic generator for seeded test data.
struct Lcg(u64);
impl Lcg {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

const EPS: f64 = 1e-9;

/// An M8 tapped hole 12 mm deep from `at`, down: the drilled Ø6.8 its crest, Ø8 its root.
fn tapped(at: Vec3, limits: [bool; 2]) -> Thread {
    let down = UnitVec3::new(Vec3::new(0.0, 0.0, -1.0)).unwrap();
    Thread { axis_point: at, axis: down, start: 0.0, end: 12.0, crest_radius: 3.4, root_radius: 4.0, internal: true, limits }
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).len() < EPS
}

fn ends(seg: &Seg2) -> (Vec2, Vec2) {
    (seg.start(), seg.end())
}

/// Seen end on, an M8 tapped hole draws the Ø6.8 circle and a Ø8 arc of three quarters, its gap in
/// the upper right quadrant, centred on the axis: from above and from below alike.
#[test]
fn an_m8_tapped_hole_seen_end_on_is_a_circle_and_a_three_quarter_arc() {
    for (view, centre) in [(NamedView::Top, Vec2::new(20.0, 5.0)), (NamedView::Bottom, Vec2::new(20.0, -5.0))] {
        let lines = thread_lines(&view.basis(), &tapped(Vec3::new(20.0, 5.0, 0.0), [false, true])).unwrap();
        assert_eq!(lines.crest, vec![Seg2::Arc { center: centre, radius: 3.4, start: 0.0, sweep: TAU }], "{view:?}");
        assert_eq!(lines.root, vec![Seg2::Arc { center: centre, radius: 4.0, start: FRAC_PI_2, sweep: 3.0 * FRAC_PI_2 }], "{view:?}");
        // From 90° round to 360°: the gap is the upper right quadrant.
        assert!(close(lines.root[0].start(), centre + Vec2::new(0.0, 4.0)) && close(lines.root[0].end(), centre + Vec2::new(4.0, 0.0)));
        assert!(close(lines.root[0].point_at(0.5), centre + Vec2::from_angle(5.0 * PI / 4.0) * 4.0), "through the lower left");
        assert!(lines.limits.is_empty(), "end on, a limit line is a point");
    }
}

/// Seen from the side, an M8 tapped hole 12 mm deep draws its root lines 8 apart and its crest lines
/// 6.8 apart, each as long as the thread, and one limit line 8 long at its inner end: to 1e-6 mm.
#[test]
fn an_m8_tapped_hole_seen_from_the_side_is_drawn_to_its_sizes() {
    let lines = thread_lines(&NamedView::Front.basis(), &tapped(Vec3::new(20.0, 5.0, 0.0), [false, true])).unwrap();
    let apart = |pair: &[Seg2]| (pair[0].start() - pair[1].start()).len();
    assert!((apart(&lines.root) - 8.0).abs() < 1e-6 && (apart(&lines.crest) - 6.8).abs() < 1e-6);
    for seg in lines.root.iter().chain(&lines.crest) {
        assert!((seg.length() - 12.0).abs() < 1e-6);
        let (a, b) = ends(seg);
        assert!((a.y - 0.0).abs() < 1e-6 && (b.y + 12.0).abs() < 1e-6, "from the face down: {seg:?}");
    }
    assert_eq!(lines.limits.len(), 1);
    let (a, b) = ends(&lines.limits[0]);
    assert!((a.dist(b) - 8.0).abs() < 1e-6 && (a.y + 12.0).abs() < 1e-6 && (b.y + 12.0).abs() < 1e-6 && close((a + b) * 0.5, Vec2::new(20.0, -12.0)));
    let none = thread_lines(&NamedView::Front.basis(), &tapped(Vec3::new(20.0, 5.0, 0.0), [false, false])).unwrap();
    assert!(none.limits.is_empty());
    let start = thread_lines(&NamedView::Front.basis(), &tapped(Vec3::new(20.0, 5.0, 0.0), [true, false])).unwrap();
    assert!(close((start.limits[0].start() + start.limits[0].end()) * 0.5, Vec2::new(20.0, 0.0)), "the limit at the start");
}

/// An external M8 (a screw): its crest is the major diameter, the root inside it, the limit line
/// across the crest.
#[test]
fn an_external_thread_has_its_crest_outside() {
    let screw = Thread { crest_radius: 4.0, root_radius: 3.3, internal: false, ..tapped(Vec3::ZERO, [false, true]) };
    let lines = thread_lines(&NamedView::Front.basis(), &screw).unwrap();
    let apart = |pair: &[Seg2]| (pair[0].start() - pair[1].start()).len();
    assert!((apart(&lines.crest) - 8.0).abs() < 1e-12 && (apart(&lines.root) - 6.6).abs() < 1e-12);
    assert!((lines.limits[0].length() - 8.0).abs() < 1e-12);
    let end_on = thread_lines(&NamedView::Top.basis(), &screw).unwrap();
    assert!(matches!(&end_on.root[..], [Seg2::Arc { radius, .. }] if *radius == 3.3));
}

/// A thread off the origin, in a slanted view, lies where its axis is seen: each line's ends on
/// paper are the model points it stands for, seen through the view.
#[test]
fn a_thread_in_a_slanted_view_lies_where_its_axis_is_seen() {
    let basis = basis_looking(Vec3::new(1.0, -2.0, -0.5), Vec3::Z, Vec3::new(4.0, -3.0, 2.0)).unwrap();
    let frame = ViewFrame::new(basis, Scale::new(1.0, 2.0).unwrap(), Vec2::new(3.0, 1.0), Vec2::new(150.0, 100.0)).unwrap();
    let axis = UnitVec3::new(basis.x * 0.6 + basis.y * 0.8).unwrap();
    let thread = Thread {
        axis_point: Vec3::new(30.0, 12.0, -7.0),
        axis,
        start: 2.0,
        end: 17.0,
        crest_radius: 5.0,
        root_radius: 4.2,
        internal: false,
        limits: [true, true],
    };
    let lines = thread_lines(&basis, &thread).unwrap();
    let across = axis.vec().cross(basis.x.cross(basis.y));
    let paper = |p: Vec3| frame.to_paper(p);
    for (pair, radius) in [(&lines.crest, 5.0), (&lines.root, 4.2)] {
        let wanted: Vec<(Vec2, Vec2)> = [-1.0, 1.0]
            .iter()
            .map(|side| {
                let off = across * (side * radius);
                (paper(thread.axis_point + axis.vec() * 2.0 + off), paper(thread.axis_point + axis.vec() * 17.0 + off))
            })
            .collect();
        for seg in pair {
            let (a, b) = ends(&frame.seg_to_paper(seg));
            assert!(wanted.iter().any(|(wa, wb)| close(a, *wa) && close(b, *wb)), "{a:?} {b:?} in {wanted:?}");
        }
    }
    let limit = frame.seg_to_paper(&lines.limits[1]);
    assert!(close((limit.start() + limit.end()) * 0.5, paper(thread.axis_point + axis.vec() * 17.0)), "the limit at the end");
}

/// End on and side on are each within `PARALLEL` (1e-9): 1e-10 off still counts, 1e-8 off is a
/// slant and draws nothing, as is 1° off.
#[test]
fn a_thread_at_a_slant_draws_nothing() {
    let off = |tilt: f64| tapped(Vec3::ZERO, [false, true]).with_axis(Vec3::new(tilt, 0.0, -1.0));
    let top = NamedView::Top.basis();
    assert!(matches!(thread_lines(&top, &off(1e-10)), Some(l) if l.limits.is_empty()));
    assert_eq!(thread_lines(&top, &off(1e-8)), None);
    assert_eq!(thread_lines(&top, &off(1f64.to_radians().tan())), None);
    let side = |tilt: f64| tapped(Vec3::ZERO, [false, true]).with_axis(Vec3::new(0.0, tilt, -1.0));
    let front = NamedView::Front.basis();
    assert!(matches!(thread_lines(&front, &side(1e-10)), Some(l) if l.limits.len() == 1));
    assert_eq!(thread_lines(&front, &side(1e-8)), None);
    assert_eq!(thread_lines(&front, &side(1f64.to_radians().tan())), None);
}

impl Thread {
    fn with_axis(self, axis: Vec3) -> Thread {
        Thread { axis: UnitVec3::new(axis).unwrap(), ..self }
    }
}

/// A thread that can't be drawn draws nothing, whatever is wrong with it, and nothing panics.
#[test]
fn hostile_threads_draw_nothing() {
    let good = tapped(Vec3::new(1.0, 2.0, 3.0), [true, true]);
    let front = NamedView::Front.basis();
    assert!(thread_lines(&front, &good).is_some());
    let mut bad = Vec::new();
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, -1.0000001e12] {
        bad.push(Thread { axis_point: Vec3::new(v, 0.0, 0.0), ..good });
        bad.push(Thread { start: v, ..good });
        bad.push(Thread { end: v, ..good });
        bad.push(Thread { crest_radius: v, ..good });
        bad.push(Thread { root_radius: v, ..good });
    }
    bad.extend([
        Thread { start: 12.0, ..good },
        Thread { start: 13.0, ..good },
        Thread { crest_radius: 0.0, ..good },
        Thread { root_radius: -4.0, ..good },
        // The root on the wrong side of the crest, for each kind, and equal to it.
        Thread { crest_radius: 4.0, root_radius: 3.4, ..good },
        Thread { crest_radius: 4.0, root_radius: 4.0, ..good },
        Thread { internal: false, ..good },
    ]);
    for thread in &bad {
        assert_eq!(thread_lines(&front, thread), None, "{thread:?}");
    }
    let skewed = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::X };
    assert_eq!(thread_lines(&skewed, &good), None);
    let huge = Plane { origin: Vec3::new(f64::NAN, 0.0, 0.0), ..front };
    assert_eq!(thread_lines(&huge, &good), None);
}

/// Seeded threads in seeded views: nothing panics; every line is finite; side on, every line runs
/// along the seen axis and the limit lines across it.
#[test]
fn seeded_threads_draw_finite_lines_along_their_axis() {
    let mut rng = Lcg(0x7d3);
    let mut drawn = [0usize; 2];
    for _ in 0..5000 {
        let unit = |rng: &mut Lcg| Vec3::new(rng.unit() - 0.5, rng.unit() - 0.5, rng.unit() - 0.5);
        let Ok(basis) = basis_looking(unit(&mut rng), unit(&mut rng), unit(&mut rng) * 100.0) else { continue };
        let normal = basis.x.cross(basis.y);
        let pick = rng.unit();
        let axis = if pick < 0.4 {
            basis.x * (rng.unit() - 0.5) + basis.y * (rng.unit() - 0.5)
        } else if pick < 0.8 {
            normal * (rng.unit() - 0.5)
        } else {
            unit(&mut rng)
        };
        let Some(axis) = UnitVec3::new(axis) else { continue };
        let scale = 10f64.powf(rng.unit() * 6.0 - 2.0);
        let (crest, root) = (scale * (0.1 + rng.unit()), scale * (0.1 + rng.unit()));
        let thread = Thread {
            axis_point: unit(&mut rng) * scale * 10.0,
            axis,
            start: (rng.unit() - 0.5) * scale,
            end: rng.unit() * scale,
            crest_radius: crest,
            root_radius: root,
            internal: root > crest,
            limits: [rng.unit() < 0.5, rng.unit() < 0.5],
        };
        let Some(lines) = thread_lines(&basis, &thread) else { continue };
        let all: Vec<&Seg2> = lines.crest.iter().chain(&lines.root).chain(&lines.limits).collect();
        assert!(all.iter().all(|s| s.start().is_finite() && s.end().is_finite()), "{thread:?}");
        if lines.crest.len() == 2 {
            drawn[1] += 1;
            let seen = basis.to_local(thread.axis_point + axis.vec()) - basis.to_local(thread.axis_point);
            for seg in lines.crest.iter().chain(&lines.root) {
                assert!((seg.end() - seg.start()).cross(seen).abs() < 1e-9 * seg.length().max(1.0), "{thread:?}");
            }
            for seg in &lines.limits {
                assert!((seg.end() - seg.start()).dot(seen).abs() < 1e-9 * seg.length().max(1.0), "{thread:?}");
            }
        } else {
            drawn[0] += 1;
        }
    }
    assert!(drawn.iter().all(|n| *n > 500), "{drawn:?}");
}

// ---- Each thread line seen where its crest is seen (`on_paper`).

/// How near a drawn line must lie to a crest to be it, on paper: these tests' chords are exact.
const NEAR: f64 = 0.02;

/// A view at 1:1 whose paper coordinates are its plane's.
fn view(named: NamedView) -> ViewFrame {
    ViewFrame::new(named.basis(), Scale::new(1.0, 1.0).unwrap(), Vec2::ZERO, Vec2::ZERO).unwrap()
}

fn line(a: (f64, f64), b: (f64, f64)) -> Seg2 {
    Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) }
}

/// A vertical line at `x` from `y0` down to `y1` in steps of `step`, as a mesh draws one in chords.
fn chords(x: f64, y0: f64, y1: f64, step: f64) -> Vec<Seg2> {
    let n = ((y0 - y1) / step).round() as usize;
    (0..n).map(|k| line((x, y0 - k as f64 * step), (x, y0 - (k + 1) as f64 * step))).collect()
}

/// The M8 tapped hole at (20, 5, 0), seen from the front: its crest lines are x = 16.6 and 23.4 on
/// paper, from y = 0 (the face) down; its root lines x = 16 and 24, 12 long.
fn hole() -> Thread {
    tapped(Vec3::new(20.0, 5.0, 0.0), [false, false])
}

/// Each piece of `stroke` lying along x = `x`, as (top, bottom, visibility), top first.
fn along(pieces: &[ThreadPiece], stroke: ThreadStroke, x: f64) -> Vec<(f64, f64, Visibility)> {
    let mut out: Vec<(f64, f64, Visibility)> = pieces
        .iter()
        .filter(|p| p.stroke == stroke)
        .filter_map(|p| {
            let (a, b) = ends(&p.seg);
            ((a.x - x).abs() < 1e-9 && (b.x - x).abs() < 1e-9).then(|| (a.y.max(b.y), a.y.min(b.y), p.visibility))
        })
        .collect();
    out.sort_by(|p, q| q.0.total_cmp(&p.0));
    out
}

fn same(got: &[(f64, f64, Visibility)], wanted: &[(f64, f64, Visibility)]) -> bool {
    got.len() == wanted.len() && got.iter().zip(wanted).all(|(g, w)| (g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-9 && g.2 == w.2)
}

use Visibility::{Hidden, Visible};

/// From the side, each root line takes its own side's crest's visibility stretch by stretch, the
/// crest drawn in chords or in one line alike; the limit line at 12 deep takes the crest's there.
#[test]
fn a_root_line_is_seen_where_its_crest_line_is_seen() {
    let left_seen = chords(16.6, 0.0, -6.0, 1.0);
    let left_hidden = [line((16.6, -6.0), (16.6, -16.0))];
    let right = [line((23.4, 0.0), (23.4, -16.0))];
    let drawn = [
        Drawn { visibility: Visible, path: &left_seen },
        Drawn { visibility: Hidden, path: &left_hidden },
        Drawn { visibility: Hidden, path: &right },
    ];
    let pieces = on_paper(&view(NamedView::Front), &hole(), &drawn, NEAR);
    assert!(same(&along(&pieces, ThreadStroke::Root, 16.0), &[(0.0, -6.0, Visible), (-6.0, -12.0, Hidden)]), "{pieces:?}");
    assert!(same(&along(&pieces, ThreadStroke::Root, 24.0), &[(0.0, -12.0, Hidden)]), "{pieces:?}");
    let limits: Vec<&ThreadPiece> = pieces.iter().filter(|p| p.stroke == ThreadStroke::Limit).collect();
    assert_eq!(limits.len(), 1, "{pieces:?}");
    let (a, b) = ends(&limits[0].seg);
    assert!((a.dist(b) - 8.0).abs() < 1e-9 && (a.y + 12.0).abs() < 1e-9 && close((a + b) * 0.5, Vec2::new(20.0, -12.0)), "{limits:?}");
    assert_eq!(limits[0].visibility, Hidden, "both crests are hidden at 12 deep");
}

/// A crest drawn on one side never draws the other side's root line, and a line that isn't a crest
/// draws nothing.
#[test]
fn a_crest_on_one_side_draws_only_that_sides_root_line() {
    let right = [line((23.4, 0.0), (23.4, -16.0))];
    let other = [line((10.0, 0.0), (10.0, -16.0)), line((16.6, -20.0), (19.0, -20.0))];
    let drawn = [Drawn { visibility: Visible, path: &right }, Drawn { visibility: Visible, path: &other }];
    let pieces = on_paper(&view(NamedView::Front), &hole(), &drawn, NEAR);
    assert!(same(&along(&pieces, ThreadStroke::Root, 24.0), &[(0.0, -12.0, Visible)]), "{pieces:?}");
    assert!(along(&pieces, ThreadStroke::Root, 16.0).is_empty(), "{pieces:?}");
    assert_eq!(pieces.iter().filter(|p| p.stroke == ThreadStroke::Root).count(), 1, "{pieces:?}");
}

/// Where the view drew no crest (another body hides it, the boundary clips it), the root line has
/// a gap too.
#[test]
fn a_gap_in_the_crest_is_a_gap_in_the_root_line() {
    let left = [line((16.6, 0.0), (16.6, -4.0)), line((16.6, -8.0), (16.6, -16.0))];
    let pieces = on_paper(&view(NamedView::Front), &hole(), &[Drawn { visibility: Visible, path: &left }], NEAR);
    assert!(same(&along(&pieces, ThreadStroke::Root, 16.0), &[(0.0, -4.0, Visible), (-8.0, -12.0, Visible)]), "{pieces:?}");
}

/// End on, the root arc takes the crest circle's visibility by angle: drawn as two half circles (one
/// turning back) or in chords.
#[test]
fn end_on_the_root_arc_takes_its_circles_visibility_by_angle() {
    let centre = Vec2::new(20.0, 5.0);
    let seen = [Seg2::Arc { center: centre, radius: 3.4, start: 0.0, sweep: PI }];
    let hidden = [Seg2::Arc { center: centre, radius: 3.4, start: TAU, sweep: -PI }];
    let drawn = [Drawn { visibility: Visible, path: &seen }, Drawn { visibility: Hidden, path: &hidden }];
    let pieces = on_paper(&view(NamedView::Top), &hole(), &drawn, NEAR);
    let arc = |start: f64, sweep: f64, visibility| ThreadPiece {
        stroke: ThreadStroke::Root,
        visibility,
        seg: Seg2::Arc { center: centre, radius: 4.0, start, sweep },
    };
    let near_arc = |got: &ThreadPiece, wanted: &ThreadPiece| match (got.seg, wanted.seg) {
        (Seg2::Arc { center: c, radius: r, start: s, sweep: w }, Seg2::Arc { center, radius, start, sweep }) => {
            got.stroke == wanted.stroke
                && got.visibility == wanted.visibility
                && close(c, center)
                && (r - radius).abs() < 1e-9
                && (s - start).abs() < 1e-9
                && (w - sweep).abs() < 1e-9
        }
        _ => false,
    };
    let wanted = [arc(FRAC_PI_2, FRAC_PI_2, Visible), arc(PI, PI, Hidden)];
    assert!(pieces.len() == 2 && pieces.iter().zip(&wanted).all(|(g, w)| near_arc(g, w)), "{pieces:?}");
    let ring: Vec<Seg2> = (0..64).map(|k| line_on(centre, 3.4, k, 64)).collect();
    let pieces = on_paper(&view(NamedView::Top), &hole(), &[Drawn { visibility: Visible, path: &ring }], NEAR);
    assert!(pieces.len() == 1 && near_arc(&pieces[0], &arc(FRAC_PI_2, 3.0 * FRAC_PI_2, Visible)), "{pieces:?}");
}

/// The `k`th of `n` chords round the circle about `centre` of `radius`.
fn line_on(centre: Vec2, radius: f64, k: usize, n: usize) -> Seg2 {
    let at = |k: usize| centre + Vec2::from_angle(TAU * k as f64 / n as f64) * radius;
    Seg2::Line { a: at(k), b: at(k + 1) }
}

/// A limit line goes only where the thread stops while its face runs on: none where the thread
/// fills its face, one at its inner end in a deeper hole, both where the face runs on either way.
#[test]
fn limit_lines_go_only_where_the_thread_stops_inside_its_face() {
    let limits = |top: f64, bottom: f64| {
        let crests = [line((16.6, top), (16.6, bottom)), line((23.4, top), (23.4, bottom))];
        let pieces = on_paper(&view(NamedView::Front), &hole(), &[Drawn { visibility: Visible, path: &crests }], NEAR);
        let mut at: Vec<f64> = pieces.iter().filter(|p| p.stroke == ThreadStroke::Limit).map(|p| 0.5 * (p.seg.start().y + p.seg.end().y)).collect();
        at.sort_by(|a, b| b.total_cmp(a));
        at
    };
    assert!(limits(0.0, -12.0).is_empty());
    assert!(limits(0.005, -12.005).is_empty(), "within the tolerance it is the thread's own end");
    let deep = limits(0.0, -16.0);
    assert!(deep.len() == 1 && (deep[0] + 12.0).abs() < 1e-9, "{deep:?}");
    let both = limits(2.0, -16.0);
    assert!(both.len() == 2 && both[0].abs() < 1e-9 && (both[1] + 12.0).abs() < 1e-9, "{both:?}");
}

/// A tolerance that isn't a finite size, a drawn line that isn't finite, or a thread that can't be
/// drawn: nothing wrong is drawn, and nothing panics.
#[test]
fn hostile_lines_and_tolerances_draw_nothing_wrong() {
    let crests = [line((16.6, 0.0), (16.6, -16.0)), line((23.4, 0.0), (23.4, -16.0))];
    let good = [Drawn { visibility: Visible, path: &crests }];
    let front = view(NamedView::Front);
    assert!(!on_paper(&front, &hole(), &good, NEAR).is_empty());
    for near in [f64::NAN, f64::INFINITY, -1.0] {
        assert!(on_paper(&front, &hole(), &good, near).is_empty(), "{near}");
    }
    let mut bad = Vec::new();
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300] {
        bad.extend([line((v, 0.0), (16.6, -16.0)), line((16.6, v), (16.6, -16.0))]);
        bad.extend([Seg2::Arc { center: Vec2::new(v, 5.0), radius: 3.4, start: 0.0, sweep: v }]);
        bad.push(Seg2::Arc { center: Vec2::new(20.0, 5.0), radius: 3.4, start: v, sweep: v });
    }
    for named in [NamedView::Front, NamedView::Top] {
        let pieces = on_paper(&view(named), &hole(), &[Drawn { visibility: Visible, path: &bad }], NEAR);
        assert!(pieces.iter().all(|p| p.seg.start().is_finite() && p.seg.end().is_finite()), "{pieces:?}");
    }
    assert!(on_paper(&front, &Thread { root_radius: 3.0, ..hole() }, &good, NEAR).is_empty());
}

/// Seeded views, threads and drawn lines, some along the crests: nothing panics, every piece is
/// finite and takes the drawn visibility, and every root piece lies on a root line within the
/// thread's span.
#[test]
fn seeded_lines_pair_onto_root_lines_only() {
    let mut rng = Lcg(0x13050);
    let mut paired = 0usize;
    for _ in 0..3000 {
        let named = if rng.unit() < 0.5 { NamedView::Front } else { NamedView::Top };
        let thread = Thread { end: 1.0 + rng.unit() * 20.0, ..hole() };
        let Some(lines) = thread_lines(&named.basis(), &thread) else { continue };
        let mut path = Vec::new();
        for _ in 0..(rng.unit() * 12.0) as usize {
            let pick = rng.unit();
            let crest = lines.crest.get((rng.unit() * lines.crest.len() as f64) as usize).copied();
            let seg = match crest {
                Some(Seg2::Line { a, b }) if pick < 0.6 => {
                    let (s, t) = (rng.unit() * 1.4 - 0.2, rng.unit() * 1.4 - 0.2);
                    Seg2::Line { a: a.lerp(b, s), b: a.lerp(b, t) }
                }
                Some(Seg2::Arc { center, radius, .. }) if pick < 0.6 => {
                    Seg2::Arc { center, radius, start: rng.unit() * 7.0 - 0.5, sweep: rng.unit() * 8.0 - 4.0 }
                }
                _ => {
                    let mut at = || (rng.unit() - 0.5) * 60.0;
                    line((at(), at()), (at(), at()))
                }
            };
            path.push(seg);
        }
        let visibility = if rng.unit() < 0.5 { Visible } else { Hidden };
        let pieces = on_paper(&view(named), &thread, &[Drawn { visibility, path: &path }], NEAR);
        for p in &pieces {
            assert!(p.seg.start().is_finite() && p.seg.end().is_finite() && p.visibility == visibility, "{p:?}");
            if p.stroke != ThreadStroke::Root {
                continue;
            }
            paired += 1;
            let on_root = lines.root.iter().any(|root| match (*root, p.seg) {
                (Seg2::Line { a, b }, Seg2::Line { a: pa, b: pb }) => {
                    let dir = (b - a).normalized().unwrap();
                    let fits = |q: Vec2| (q - a).cross(dir).abs() < 1e-9 && (q - a).dot(dir) > -1e-9 && (q - a).dot(dir) < (b - a).len() + 1e-9;
                    fits(pa) && fits(pb)
                }
                (Seg2::Arc { center, radius, start, sweep }, Seg2::Arc { center: c, radius: r, start: s, sweep: w }) => {
                    close(center, c) && radius == r && s >= start - 1e-9 && s + w <= start + sweep + 1e-9 && w > 0.0
                }
                _ => false,
            });
            assert!(on_root, "{p:?} on {lines:?}");
        }
    }
    assert!(paired > 1000, "{paired}");
}

/// A cosmetic thread's record as a view draws it: an M8 screw's root at the minor radius its
/// modelled thread is cut to (4 - 5H/8, H = 1.25 √3 / 2), an M8 tapped hole's at the major radius,
/// both crests at the face's own radius; a taper thread draws none.
#[test]
fn a_cosmetic_thread_is_drawn_from_its_record() {
    let info = |internal: bool, radius: f64, form: ThreadForm| ThreadInfo {
        feature: 1,
        body: "Body".into(),
        designation: "M8".into(),
        major_diameter: 8.0,
        radius,
        pitch: 1.25,
        internal,
        axis_point: Vec3::new(1.0, 2.0, 3.0),
        axis: UnitVec3::new(Vec3::Z).unwrap(),
        start: 0.0,
        end: 10.0,
        form,
        hand: ThreadHand::default(),
        starts: 1,
    };
    let screw = Thread::of(&info(false, 4.0, ThreadForm::Metric)).unwrap();
    let minor = 4.0 - 5.0 * (1.25 * 3f64.sqrt() / 2.0) / 8.0;
    assert!((screw.root_radius - minor).abs() < 1e-12 && screw.crest_radius == 4.0 && !screw.internal, "{screw:?}");
    assert_eq!((screw.axis_point, screw.start, screw.end, screw.limits), (Vec3::new(1.0, 2.0, 3.0), 0.0, 10.0, [false, false]));
    let tapped = Thread::of(&info(true, 3.4, ThreadForm::Metric)).unwrap();
    assert_eq!((tapped.root_radius, tapped.crest_radius, tapped.internal), (4.0, 3.4, true));
    for form in [ThreadForm::Bspt, ThreadForm::Npt] {
        assert_eq!(Thread::of(&info(false, 4.0, form)), None, "{form:?}");
    }
    // A record saved without its face's radius (0) can't be drawn.
    assert_eq!(thread_lines(&NamedView::Front.basis(), &Thread::of(&info(true, 0.0, ThreadForm::Metric)).unwrap()), None);
}

/// Where crests of two visibilities overlap (two faces' lines on one silhouette), the most seen
/// wins: the root line is seen where any of its crest is.
#[test]
fn where_crests_overlap_the_most_seen_wins() {
    let seen = [line((16.6, 0.0), (16.6, -8.0))];
    let hidden = [line((16.6, -4.0), (16.6, -16.0))];
    let drawn = [Drawn { visibility: Hidden, path: &hidden }, Drawn { visibility: Visible, path: &seen }];
    let pieces = on_paper(&view(NamedView::Front), &hole(), &drawn, NEAR);
    assert!(same(&along(&pieces, ThreadStroke::Root, 16.0), &[(0.0, -8.0, Visible), (-8.0, -12.0, Hidden)]), "{pieces:?}");
}
