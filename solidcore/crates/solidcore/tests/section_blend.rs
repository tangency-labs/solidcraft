//! tangency-labs/solidcore#3140: the multi-section blend, against closed forms.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::surface::procedural_blend::convert::{
    section_blend_from_record, section_blend_rebuild, section_blend_to_record,
};
use solidcore::surface::procedural_blend::record::SectionBlendRecord;
use solidcore::surface::surface::Surface;
use solidcore::sweep::section_blend::{
    section_blend, section_blend_surface, EndOrder, SectionFrame,
};
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
    // three sections: the chord is to the neighbouring section and the speed is over the span's
    // interval 1/2, `d_u = leave * chord / 3 * (1 + crown) * (n - 1)`
    let mid: Vec<SectionFrame> = [0.0, 2.0]
        .iter()
        .map(|&x| frame(Point3::new(x, 1.5, 0.3)))
        .collect();
    let s = Surface::from(
        section_blend(
            &[a.clone(), mid.clone(), b.clone()],
            [EndOrder::G1, EndOrder::G1],
            crown,
        )
        .unwrap(),
    );
    for (j, v) in [0.0, 1.0].into_iter().enumerate() {
        let d0 = derivs(&k, &s, 0.0, v, 1, 0)[1][0];
        let want0 = lift * (a[j].p.distance(mid[j].p) / 3.0 * (1.0 + crown[0]) * 2.0);
        assert!(close(d0, want0), "{d0:?} vs {want0:?}");
        let d1 = derivs(&k, &s, 1.0, v, 1, 0)[1][0];
        let want1 =
            back.unitize().unwrap() * (-(b[j].p.distance(mid[j].p) / 3.0 * (1.0 + crown[1]) * 2.0));
        assert!(close(d1, want1), "{d1:?} vs {want1:?}");
    }
}

/// Five sections of five stations each of the parabolic cylinder `(2 v, t, t^2)` at
/// `t = 0, 1/4, .., 1`, `v = 0, 1/4, .., 1` (so v is interpolated at degree 3), its ends'
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
        for v in [0.0, 0.25, 0.5, 0.75, 1.0] {
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
    // (a grid of half the stations' spacing, between them as well as on them)
    for i in 0..=8 {
        for j in 0..=8 {
            let (t, v) = (f64::from(i) / 8.0, f64::from(j) / 8.0);
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
        for (joint, v) in (1..run - 1)
            .flat_map(|i| [0.0, 0.4, 1.0].map(|v| (f64::from(i) / f64::from(run - 1), v)))
        {
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
                    "run {run}, joint {joint}, order {o}, v {v}: {l:?} vs {r:?}"
                );
            }
        }
    }
}

/// Five sections of the cubic cylinder `(2 v, t, t^3)` at `t = 0, 1/4, .., 1`, held at G0: every
/// interior section holds the third difference, exact on a cubic (central at the middle section,
/// forward at the second, backward at the last-but-one), so the surface's `d_uuu` there is
/// `(0, 0, 6)` at every station.
#[test]
fn interior_sections_hold_a_cubic_s_third_derivative() {
    let k = Kernel::default();
    let sections: Vec<_> = (0..5)
        .map(|i| {
            let t = f64::from(i) / 4.0;
            [0.0, 0.5, 1.0]
                .map(|v| frame(Point3::new(2.0 * v, t, t * t * t)))
                .to_vec()
        })
        .collect();
    let s =
        Surface::from(section_blend(&sections, [EndOrder::G0, EndOrder::G0], [0.0, 0.0]).unwrap());
    // each stencil at its own section: the forward one at the second (u = 1/4), the central one
    // at the middle (1/2), and the backward one at the last-but-one (3/4), where SolidCraft's sign
    // gave (0, 0, -6)
    let stencils = [(0.25, "forward"), (0.5, "central"), (0.75, "backward")];
    for (u, stencil) in stencils {
        for v in [0.0, 0.5, 1.0] {
            let d = derivs(&k, &s, u, v, 3, 0)[3][0];
            // (a third derivative of degree-7 spans over a quarter: rounding grows by about
            // m (m - 1) (m - 2) / step^3, so 1e-9, not the points' 1e-12)
            assert!(
                d.distance(Vec3::new(0.0, 0.0, 6.0)) <= 1e-9 * 6.0,
                "the {stencil} difference at ({u}, {v}): {d:?}"
            );
        }
    }
    // the last-but-one section by itself, its sign
    let back = derivs(&k, &s, 0.75, 0.5, 3, 0)[3][0];
    assert!(back.z > 0.0, "the backward difference's sign: {back:?}");
}

/// The helix `(r cos a, r sin a, h a)` at `a`: its point, unit tangent, curvature vector and the
/// arc-length derivative of its curvature vector (`h = 0`: a circle).
fn helix(r: f64, h: f64, a: f64) -> (Point3, Vec3, Vec3, Vec3) {
    let c = (r * r + h * h).sqrt();
    let (sin, cos) = a.sin_cos();
    (
        Point3::new(r * cos, r * sin, h * a),
        Vec3::new(-r * sin, r * cos, h) * (1.0 / c),
        Vec3::new(-r * cos, -r * sin, 0.0) * (1.0 / (c * c)),
        Vec3::new(r * sin, -r * cos, 0.0) * (1.0 / (c * c * c)),
    )
}

/// `v`'s part square to the unit `t`.
fn square_to(v: Vec3, t: Vec3) -> Vec3 {
    v - t * v.dot(t)
}

/// G3 at both ends, the frames a circle's (`bend = n / r`, `bend_change` 0) and a helix's (whose
/// curvature vector turns with its torsion, so `bend_change` has a part square to the tangent),
/// five stations on helices of growing radius, from `a = 0.3` to `a = 1.5`. At every station
/// the surface leaves the first section with `d_u = t s`, `d_uu = kappa s^2` and the part of
/// `d_uuu` square to `t` equal to `s^3 (dkappa/ds)`'s: the helix's own derivatives at the constant
/// speed `s` the definition gives (a third of the chord, times `1 + crown`), whose derivatives in
/// u are those exactly. At the last section the frames point back into the blend (`leave = -t`,
/// `bend_change = -dkappa/ds`) and the surface arrives with the helix's forward derivatives.
#[test]
fn g3_ends_hold_a_circle_s_and_a_helix_s_frames() {
    let k = Kernel::default();
    let (a0, a1) = (0.3, 1.5);
    let crown = [0.2, -0.1];
    for (h, circle) in [(0.0, true), (0.6, false)] {
        let radius = |j: u32| 1.0 + 0.25 * f64::from(j);
        let first: Vec<SectionFrame> = (0..5)
            .map(|j| {
                let (p, t, kappa, dk) = helix(radius(j), h, a0);
                SectionFrame {
                    p,
                    leave: t,
                    bend: kappa,
                    bend_change: if circle { Vec3::ZERO } else { dk },
                }
            })
            .collect();
        let last: Vec<SectionFrame> = (0..5)
            .map(|j| {
                let (p, t, kappa, dk) = helix(radius(j), h, a1);
                SectionFrame {
                    p,
                    leave: -t,
                    bend: kappa,
                    bend_change: if circle { Vec3::ZERO } else { -dk },
                }
            })
            .collect();
        let s = Surface::from(
            section_blend(
                &[first.clone(), last.clone()],
                [EndOrder::G3, EndOrder::G3],
                crown,
            )
            .unwrap(),
        );
        for j in 0..5u32 {
            let v = f64::from(j) / 4.0;
            let chord = first[j as usize].p.distance(last[j as usize].p);
            for (u, a, end) in [(0.0, a0, 0usize), (1.0, a1, 1)] {
                let (_, t, kappa, dk) = helix(radius(j), h, a);
                let speed = chord / 3.0 * (1.0 + crown[end]);
                let d = derivs(&k, &s, u, v, 3, 0);
                let want = [
                    t * speed,
                    kappa * (speed * speed),
                    square_to(dk, t) * (speed * speed * speed),
                ];
                if circle {
                    assert!(want[2].length() <= 1e-15, "a circle's dkappa/ds is along t");
                }
                let scale = speed.max(speed * speed * speed);
                for (o, w) in want.into_iter().enumerate() {
                    let got = if o == 2 {
                        square_to(d[3][0], t)
                    } else {
                        d[o + 1][0]
                    };
                    assert!(
                        got.distance(w) <= 1e-9 * (scale + w.length()),
                        "h {h}, station {j}, u {u}, order {}: {got:?} vs {w:?}",
                        o + 1
                    );
                }
            }
        }
    }
}

/// Refused with this reason (the first words of it), as an `InvalidInput`.
fn refused<T>(r: solidcore::Result<T>, why: &str) {
    match r {
        Err(solidcore::SolidError::InvalidInput { reason, .. }) => {
            assert!(reason.starts_with(why), "{reason:?} is not {why:?}");
        }
        Err(e) => panic!("{e:?} is not the refusal {why:?}"),
        Ok(_) => panic!("built; expected the refusal {why:?}"),
    }
}

/// The refusals the definition names, each by its reason, and each bound passing at its limit.
#[test]
fn malformed_blends_are_refused() {
    let line = |y: f64| {
        vec![
            frame(Point3::new(0.0, y, 0.0)),
            frame(Point3::new(1.0, y, 0.0)),
        ]
    };
    let stations = |y: f64, n: u32| -> Vec<SectionFrame> {
        (0..n)
            .map(|j| frame(Point3::new(f64::from(j), y, 0.0)))
            .collect()
    };
    let g0 = [EndOrder::G0, EndOrder::G0];
    let flat = [0.0, 0.0];
    // the counts, at and past each bound
    let run = |n: u32| (0..n).map(|i| line(f64::from(i))).collect::<Vec<_>>();
    refused(
        section_blend(&run(1), g0, flat),
        "a blend runs through 2 to 50",
    );
    assert!(section_blend(&run(50), g0, flat).is_ok());
    refused(
        section_blend(&run(51), g0, flat),
        "a blend runs through 2 to 50",
    );
    refused(
        section_blend(&[stations(0.0, 1), stations(1.0, 1)], g0, flat),
        "a blend's cross-sections take 2 to 257",
    );
    assert!(section_blend(&[stations(0.0, 257), stations(1.0, 257)], g0, flat).is_ok());
    refused(
        section_blend(&[stations(0.0, 258), stations(1.0, 258)], g0, flat),
        "a blend's cross-sections take 2 to 257",
    );
    refused(
        section_blend(&[line(0.0), stations(1.0, 3)], g0, flat),
        "every cross-section of a blend has the same stations",
    );
    // the crown, at and past each end of its range
    for (c, ok) in [
        (-0.9, true),
        (-0.900_000_1, false),
        (3.0, true),
        (3.000_000_1, false),
    ] {
        let r = section_blend(&[line(0.0), line(1.0)], g0, [c, 0.0]);
        if ok {
            assert!(r.is_ok(), "crown {c}");
        } else {
            refused(r, "a blend's crown is between");
        }
    }
    refused(
        section_blend(&[line(0.0), line(0.0)], g0, flat),
        "two neighbouring cross-sections meet",
    );
    let mut nan = line(1.0);
    nan[0].p.x = f64::NAN;
    refused(
        section_blend(&[line(0.0), nan], g0, flat),
        "a blend's frame is not finite",
    );
    // an end that uses its tangent with none: G1, and G3
    for order in [EndOrder::G1, EndOrder::G3] {
        refused(
            section_blend(&[line(0.0), line(1.0)], [order, EndOrder::G0], flat),
            "a blend's leave direction has no length",
        );
    }
    // a curvature too large for its speed: d2 = bend speed^2 overflows
    let mut steep = line(0.0);
    for f in &mut steep {
        f.leave = Vec3::new(0.0, 1.0, 0.0);
        f.bend = Vec3::new(0.0, 0.0, 1e308);
    }
    refused(
        section_blend(&[steep, line(9.0)], [EndOrder::G2, EndOrder::G0], flat),
        "a section's derivatives along the run are not finite",
    );
}

/// Four sections of three stations, the ends with frames that use every condition.
fn kept_sections() -> Vec<Vec<SectionFrame>> {
    (0..4)
        .map(|i| {
            let x = f64::from(i);
            (0..3)
                .map(|j| {
                    let y = f64::from(j);
                    SectionFrame {
                        p: Point3::new(x + 0.1 * y * y, y, 0.3 * x * x - 0.2 * y),
                        leave: Vec3::new(1.0, 0.0, 0.2),
                        bend: Vec3::new(0.0, 0.0, 0.1),
                        bend_change: Vec3::new(0.0, 0.05, 0.0),
                    }
                })
                .collect()
        })
        .collect()
}

/// The blend's surface moved twice: a turn with a uniform scale, then a move.
fn kept_and_moved(k: &Kernel) -> Surface {
    let mut s =
        section_blend_surface(kept_sections(), [EndOrder::G2, EndOrder::G3], [0.2, -0.1]).unwrap();
    let mut turn = Axis2Placement::IDENTITY;
    turn.rotate_about_axis(0.7, Vec3::new(1.0, 2.0, 2.0) * (1.0 / 3.0));
    turn.translate(Vec3::new(5.0, -2.0, 1.0));
    s.transform(k, &turn, Some(Vec3::new(2.5, 2.5, 2.5)))
        .unwrap();
    let mut shift = Axis2Placement::IDENTITY;
    shift.translate(Vec3::new(-0.3, 0.7, 11.0));
    s.transform(k, &shift, None).unwrap();
    s
}

/// The surface keeps its definition, re-certified exactly (bound 0) as made and after two
/// motions; its record, through JSON and back, rebuilds the same surface (the same record,
/// the same points bit for bit) and re-certifies.
#[test]
fn a_record_round_trip_keeps_and_recertifies_the_blend() {
    let k = Kernel::default();
    let made =
        section_blend_surface(kept_sections(), [EndOrder::G2, EndOrder::G3], [0.2, -0.1]).unwrap();
    let blend = made.procedural_blend().unwrap();
    assert!(blend.section_blend().is_some());
    assert_eq!(blend.bound(), 0.0);
    assert_eq!(blend.recertify(&k).unwrap(), 0.0);

    let s = kept_and_moved(&k);
    let blend = s.procedural_blend().unwrap();
    assert_eq!(blend.section_blend().unwrap().motions().len(), 2);
    assert_eq!(blend.recertify(&k).unwrap(), 0.0);

    let rec = section_blend_to_record(&s).unwrap();
    let json = serde_json::to_string(&rec).unwrap();
    let back: SectionBlendRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back, rec);
    let rebuilt = section_blend_rebuild(&back).unwrap();
    assert_eq!(section_blend_to_record(&rebuilt).unwrap(), rec);
    assert_eq!(
        rebuilt.procedural_blend().unwrap().recertify(&k).unwrap(),
        0.0
    );
    for (u, v) in [(0.0, 0.0), (0.3, 0.6), (0.5, 1.0), (1.0, 0.25)] {
        let (a, b) = (
            s.evaluate_point(&k, Point2::new(u, v)).unwrap(),
            rebuilt.evaluate_point(&k, Point2::new(u, v)).unwrap(),
        );
        assert_eq!(
            [a.x, a.y, a.z].map(f64::to_bits),
            [b.x, b.y, b.z].map(f64::to_bits)
        );
    }
}

/// A net one ulp off its definition's is refused, as is a record that drops a motion or moves
/// a station: the definition no longer rebuilds the net exactly.
#[test]
fn a_tampered_net_is_refused() {
    let k = Kernel::default();
    let rec = section_blend_to_record(&kept_and_moved(&k)).unwrap();
    assert!(section_blend_rebuild(&rec).is_ok());

    let mut off = rec.clone();
    let y = &mut off.net.points[4][1];
    *y = f64::from_bits(y.to_bits() + 1);
    // (the record is well formed: only the re-certification refuses it)
    assert!(section_blend_from_record(&off).is_ok());
    assert!(section_blend_rebuild(&off).is_err());

    let mut unmoved = rec.clone();
    unmoved.motions.pop();
    assert!(section_blend_rebuild(&unmoved).is_err());

    let mut shifted = rec;
    shifted.sections[1][1].p[2] += 1e-6;
    assert!(section_blend_rebuild(&shifted).is_err());
}
