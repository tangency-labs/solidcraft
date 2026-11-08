//! tangency-labs/solidcore#3140: the multi-section blend, against closed forms.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::surface::surface::Surface;
use solidcore::sweep::section_blend::{section_blend, EndOrder, SectionFrame};
use solidcore::Kernel;

fn frame(p: Point3) -> SectionFrame {
    SectionFrame {
        p,
        leave: Vec3::ZERO,
        bend: Vec3::ZERO,
        bend_change: Vec3::ZERO,
    }
}

fn close(a: Point3, b: Point3) -> bool {
    a.distance(b) <= 1e-12 * (1.0 + b.length())
}

/// Derivatives up to `du` in u and `dv` in v at `(u, v)`: `d[i][j]`.
fn derivs(k: &Kernel, s: &Surface, u: f64, v: f64, du: usize, dv: usize) -> Vec<Vec<Vec3>> {
    let flags = EvaluateFlags {
        non_zero_tangents: false,
        ..EvaluateFlags::default()
    };
    s.evaluate(k, Point2::new(u, v), du, dv, flags).unwrap()
}

/// Two sections held at G0 are the ruled surface between them: `(1 - u) A(v) + u B(v)`, A and
/// B the lines through their stations.
#[test]
fn two_sections_at_g0_are_the_ruled_surface() {
    let k = Kernel::default();
    let a = [Point3::new(0.0, 0.0, 0.0), Point3::new(4.0, 0.0, 1.0)];
    let b = [Point3::new(0.0, 3.0, 2.0), Point3::new(4.0, 5.0, 0.0)];
    let sections = vec![a.map(frame).to_vec(), b.map(frame).to_vec()];
    let s =
        Surface::from(section_blend(&sections, [EndOrder::G0, EndOrder::G0], [0.0, 0.0]).unwrap());
    for i in 0..=5 {
        for j in 0..=5 {
            let (u, v) = (f64::from(i) / 5.0, f64::from(j) / 5.0);
            let av = a[0] * (1.0 - v) + a[1] * v;
            let bv = b[0] * (1.0 - v) + b[1] * v;
            let want = av * (1.0 - u) + bv * u;
            let got = s.evaluate_point(&k, Point2::new(u, v)).unwrap();
            assert!(close(got, want), "({u}, {v}): {got:?} vs {want:?}");
        }
    }
}

/// At G1 the surface leaves its first section along `leave` at the speed the definition gives
/// it, a third of the station's chord over the span's interval times `1 + crown`, and arrives at
/// the last against its `leave`: exactly.
#[test]
fn g1_ends_leave_along_their_frames_at_the_definition_s_speed() {
    let k = Kernel::default();
    let lift = Vec3::new(0.0, 0.0, 1.0);
    let back = Vec3::new(0.0, 1.0, -1.0);
    let a: Vec<SectionFrame> = [0.0, 2.0]
        .iter()
        .map(|&x| SectionFrame {
            leave: lift,
            ..frame(Point3::new(x, 0.0, 0.0))
        })
        .collect();
    let b: Vec<SectionFrame> = [0.0, 2.0]
        .iter()
        .map(|&x| SectionFrame {
            leave: back,
            ..frame(Point3::new(x, 3.0, 1.0))
        })
        .collect();
    let crown = [0.5, -0.25];
    let s = Surface::from(
        section_blend(&[a.clone(), b.clone()], [EndOrder::G1, EndOrder::G1], crown).unwrap(),
    );
    for (j, v) in [0.0, 1.0].into_iter().enumerate() {
        let chord = a[j].p.distance(b[j].p);
        let d0 = derivs(&k, &s, 0.0, v, 1, 0)[1][0];
        let want0 = lift * (chord / 3.0 * (1.0 + crown[0]));
        assert!(close(d0, want0), "{d0:?} vs {want0:?}");
        let d1 = derivs(&k, &s, 1.0, v, 1, 0)[1][0];
        let want1 = back.unitize().unwrap() * (-(chord / 3.0 * (1.0 + crown[1])));
        assert!(close(d1, want1), "{d1:?} vs {want1:?}");
    }
}

/// Five sections of the parabolic cylinder `(2 v, t, t^2)` at `t = 0, 1/4, .., 1`, its ends'
/// frames the cylinder's own derivatives at G2 (the crown chosen so the definition's speed is the
/// parameter's): the interior conditions are finite differences, exact on a quadratic, and every
/// span is the polynomial its end rows fix, so the blend is the cylinder itself.
#[test]
fn a_quadratic_s_sections_and_frames_blend_to_the_quadratic() {
    let k = Kernel::default();
    let run = 5;
    let step = 1.0 / f64::from(run - 1);
    let at = |t: f64, v: f64| Point3::new(2.0 * v, t, t * t);
    let d1 = |t: f64| Vec3::new(0.0, 1.0, 2.0 * t);
    let d2 = Vec3::new(0.0, 0.0, 2.0);
    let mut sections = Vec::new();
    let mut crown = [0.0; 2];
    for i in 0..run {
        let t = f64::from(i) * step;
        let mut section = Vec::new();
        for v in [0.0, 0.5, 1.0] {
            let mut f = frame(at(t, v));
            if i == 0 || i == run - 1 {
                // the definition's speed: a third of the chord to the neighbour, (1 + crown),
                // over the step; the frame's leave the unit tangent, its bend d2 / speed^2
                let (other, sign) = if i == 0 {
                    (at(step, v), 1.0)
                } else {
                    (at(1.0 - step, v), -1.0)
                };
                let speed = d1(t).length();
                let c = speed * 3.0 * step / at(t, v).distance(other) - 1.0;
                crown[usize::from(i != 0)] = c;
                f.leave = d1(t) * (sign / speed);
                f.bend = d2 * (1.0 / (speed * speed));
            }
            section.push(f);
        }
        sections.push(section);
    }
    let s = Surface::from(section_blend(&sections, [EndOrder::G2, EndOrder::G2], crown).unwrap());
    for i in 0..=8 {
        for j in 0..=4 {
            let (t, v) = (f64::from(i) / 8.0, f64::from(j) / 4.0);
            let got = s.evaluate_point(&k, Point2::new(t, v)).unwrap();
            assert!(
                close(got, at(t, v)),
                "({t}, {v}): {got:?} vs {:?}",
                at(t, v)
            );
        }
    }
}

/// With four or more sections an interior joint is C3 in u; with three, C2.
#[test]
fn interior_joints_are_c3_from_four_sections_and_c2_at_three() {
    let k = Kernel::default();
    let section = |x: f64, y: f64| {
        vec![
            frame(Point3::new(x, y, 0.0)),
            frame(Point3::new(x + 0.3, y, 2.0)),
        ]
    };
    let five = [(0.0, 0.0), (1.0, 0.5), (2.0, 1.7), (3.0, 1.1), (4.0, 2.4)];
    for (run, order) in [(5u32, 3usize), (3, 2)] {
        let sections: Vec<_> = five
            .iter()
            .take(run as usize)
            .map(|&(x, y)| section(x, y))
            .collect();
        let s = Surface::from(
            section_blend(&sections, [EndOrder::G0, EndOrder::G0], [0.0, 0.0]).unwrap(),
        );
        let joint = 1.0 / f64::from(run - 1);
        for v in [0.0, 0.4, 1.0] {
            let left = derivs(&k, &s, joint, v, order, 0);
            let flags = EvaluateFlags {
                non_zero_tangents: false,
                u_side: solidcore::nurbs::knots::Side::Right,
                ..EvaluateFlags::default()
            };
            let right = s
                .evaluate(&k, Point2::new(joint, v), order, 0, flags)
                .unwrap();
            for o in 1..=order {
                let (l, r) = (left[o][0], right[o][0]);
                assert!(
                    l.distance(r) <= 1e-9 * (1.0 + l.length()),
                    "run {run}, order {o}, v {v}: {l:?} vs {r:?}"
                );
            }
        }
    }
}

/// The refusals the definition names.
#[test]
fn malformed_blends_are_refused() {
    let line = |y: f64| {
        vec![
            frame(Point3::new(0.0, y, 0.0)),
            frame(Point3::new(1.0, y, 0.0)),
        ]
    };
    let g0 = [EndOrder::G0, EndOrder::G0];
    assert!(section_blend(&[line(0.0)], g0, [0.0, 0.0]).is_err());
    assert!(section_blend(&[line(0.0), line(0.0)], g0, [0.0, 0.0]).is_err());
    assert!(section_blend(&[line(0.0), line(1.0)], g0, [4.0, 0.0]).is_err());
    assert!(section_blend(&[line(0.0), vec![frame(Point3::ZERO)]], g0, [0.0, 0.0]).is_err());
    // G1 with no leave direction
    assert!(section_blend(
        &[line(0.0), line(1.0)],
        [EndOrder::G1, EndOrder::G0],
        [0.0, 0.0]
    )
    .is_err());
    let mut nan = line(1.0);
    nan[0].p.x = f64::NAN;
    assert!(section_blend(&[line(0.0), nan], g0, [0.0, 0.0]).is_err());
}
