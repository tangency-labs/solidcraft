use super::*;
use crate::bspline::tests::Lcg;
use crate::drawing_view::{NamedView, Scale, ViewFrame, basis_looking};
use solidcore_record::Vec2;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

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
