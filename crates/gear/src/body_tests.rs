//! Shafts, chamfers, hub hardware, and the tooth tuning of `crate::body`: every feature plans
//! into steps the kernel can build (the sweep's `check_plan`), sits where it should, and leaves
//! the teeth meshing.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::geom::{Outline, Seg, V2};
use crate::hub::{BEARINGS, Bore, BoreKind, Chamfer, Hardware, Shaft, ShaftEnd, bearing};
use crate::mesh2d::{Member, run};
use crate::plan::{Body, Frame, Kind, Op, Plan, Spec, Step, plan};
use crate::sweep_tests::{check_outline, check_plan};

fn planned(s: &Spec) -> Plan {
    let p = plan(s).unwrap_or_else(|e| panic!("{:?}: {e}", s.kind));
    check_plan(&p).unwrap_or_else(|e| panic!("{:?}: {e}\n{:?}", s.kind, p.warnings));
    p
}

fn fact(p: &Plan, label: &str) -> f64 {
    p.facts
        .iter()
        .find(|f| f.label == label)
        .unwrap_or_else(|| panic!("no fact {label}: {:?}", p.facts.iter().map(|f| &f.label).collect::<Vec<_>>()))
        .value
}

fn warned(p: &Plan, word: &str) -> bool {
    p.warnings.iter().any(|w| w.contains(word))
}

/// The heights a body's solid steps (new and joined prisms and lofts) run between. A bevel's
/// taper trimmed by its back and front cones runs from its back face to its front face (the
/// last corner of the trimming blank, on the axis).
fn extent(b: &Body) -> (f64, f64) {
    let mut z = (f64::INFINITY, f64::NEG_INFINITY);
    let front = b.steps.iter().find_map(|st| match st {
        Step::Revolve { profile, op: Op::Intersect } => profile.last().map(|q| q.y),
        _ => None,
    });
    for st in &b.steps {
        match st {
            Step::Taper { z0, height, .. } => z = (z.0.min(*z0), z.1.max(front.unwrap_or(z0 + height))),
            Step::Prism { z0, height, op: Op::New | Op::Join, .. } => z = (z.0.min(*z0), z.1.max(z0 + height)),
            Step::Loft { sections, .. } => {
                for (_, h) in sections {
                    z = (z.0.min(*h), z.1.max(*h));
                }
            }
            _ => {}
        }
    }
    z
}

fn revolves(b: &Body) -> Vec<&Vec<V2>> {
    b.steps.iter().filter_map(|st| if let Step::Revolve { profile, op: Op::Cut } = st { Some(profile) } else { None }).collect()
}

fn placed(b: &Body) -> Vec<(&Frame, &Step)> {
    b.steps.iter().filter_map(|st| if let Step::Placed { frame, step } = st { Some((frame, step.as_ref())) } else { None }).collect()
}

fn in_triangle(q: V2, t: &[V2]) -> bool {
    let s = |a: V2, b: V2| (b - a).cross(q - a);
    let (a, b, c) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
    (a > 0.0 && b > 0.0 && c > 0.0) || (a < 0.0 && b < 0.0 && c < 0.0)
}

fn dist_to(poly: &[V2], q: V2) -> f64 {
    poly.windows(2)
        .map(|w| {
            let d = w[1] - w[0];
            let t = if d.dot(d) > 0.0 { ((q - w[0]).dot(d) / d.dot(d)).clamp(0.0, 1.0) } else { 0.0 };
            q.dist(w[0] + d * t)
        })
        .fold(f64::INFINITY, f64::min)
}

fn first_outline(b: &Body) -> &Outline {
    match &b.steps[0] {
        Step::Prism { region, .. } => &region.outer[0],
        Step::Loft { sections, .. } => &sections[0].0,
        _ => panic!("no outline"),
    }
}

fn fine(o: &Outline) -> Vec<V2> {
    let mut out = Vec::new();
    for s in &o.segs {
        match s {
            Seg::Spline(p) => crate::geom::natural_spline(p, 24, &mut out),
            other => other.sample(1e-5, &mut out),
        }
    }
    out
}

#[test]
fn nothing_is_added_by_default() {
    let p = planned(&Spec::default());
    assert_eq!(p.bodies[0].steps.len(), 1);
    assert!(matches!(&p.bodies[0].steps[0], Step::Prism { z0, twist, op: Op::New, .. } if *z0 == 0.0 && *twist == 0.0));
    for k in Kind::ALL.iter().copied() {
        let p = planned(&Spec::defaults_for(k));
        for b in &p.bodies {
            // (A sprocket's own ISO 606 side chamfer is a revolve, and a hypoid's teeth are trimmed
            // to their toe and heel cones by one: on by default.)
            let revolve_ok = matches!(k, Kind::Sprocket | Kind::Hypoid);
            // (Nor a bevel's back and front cones, which trim its teeth.)
            let trim_ok = matches!(k, Kind::Bevel | Kind::SpiralBevel | Kind::BevelPlanetary);
            // (A differential's case is a revolve with its pin holes and windows in frames of their own.)
            if k == Kind::Differential {
                continue;
            }
            // (Nor a bevel planetary set's carrier, with a pin placed along every planet's axis.)
            if b.name.contains(crate::plan::bevel_planet::CARRIER) {
                continue;
            }
            assert!(
                !b.steps.iter().any(|st| matches!(st, Step::Placed { .. })
                    || (!revolve_ok && matches!(st, Step::Revolve { .. }) && !(trim_ok && matches!(st, Step::Revolve { op: Op::Intersect, .. })))),
                "{k:?}"
            );
        }
    }
}

#[test]
fn a_gear_carries_a_shaft_on_each_side() {
    let s = Spec {
        shaft_front: Shaft {
            diameter: 8.0,
            length: 20.0,
            step_diameter: 6.0,
            step_length: 10.0,
            end: ShaftEnd::DFlat,
            chamfer: 0.5,
            ..Shaft::default()
        },
        shaft_back: Shaft { diameter: 10.0, length: 15.0, end: ShaftEnd::Hex, end_size: 8.0, end_length: 6.0, chamfer: 1.0, ..Shaft::default() },
        ..Spec::default()
    };
    let p = planned(&s);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let b = &p.bodies[0];
    assert_eq!(extent(b), (-15.0, s.face_width + 30.0));
    // Front: Ø8 from the face, then Ø6; back: Ø10 down from z = 0.
    let discs: Vec<(f64, f64, f64)> = b.steps[1..]
        .iter()
        .filter_map(|st| match st {
            Step::Prism { region, z0, height, op: Op::Join, .. } => match region.outer[0].segs[0] {
                Seg::Circle(_, r) => Some((r, *z0, *height)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(discs, vec![(5.0, -15.0, 15.0), (4.0, 10.0, 20.0), (3.0, 30.0, 10.0)]);
    // An end chamfer each (the triangle holds the edge it takes off), and an end cut each,
    // inside the end's length.
    let rv = revolves(b);
    assert_eq!(rv.len(), 2);
    assert!(rv.iter().any(|t| in_triangle(V2::new(5.0, -15.0), t)) && rv.iter().any(|t| in_triangle(V2::new(3.0, 40.0), t)));
    let cuts: Vec<(f64, f64)> =
        b.steps.iter().filter_map(|st| if let Step::Prism { z0, height, op: Op::Cut, .. } = st { Some((*z0, z0 + height)) } else { None }).collect();
    assert_eq!(cuts.len(), 2);
    assert!(cuts.contains(&(-16.0, -9.0)), "{cuts:?}");
    assert!(cuts.iter().any(|c| (c.0 - 31.0).abs() < 1e-9 && c.1 == 41.0), "{cuts:?}");
}

#[test]
fn every_shaft_end_is_cut_on_both_sides() {
    for (end, _) in ShaftEnd::ALL {
        for size in [0.0, if matches!(end, ShaftEnd::DFlat | ShaftEnd::Hex) { 8.0 } else { 3.0 }] {
            let sh = Shaft { diameter: 10.0, length: 12.0, end, end_size: size, ..Shaft::default() };
            let s = Spec { shaft_front: sh, shaft_back: sh, ..Spec::default() };
            let p = planned(&s);
            assert!(p.warnings.is_empty(), "{end:?}: {:?}", p.warnings);
            let cuts = p.bodies[0].steps.iter().filter(|st| matches!(st, Step::Prism { op: Op::Cut, .. })).count();
            assert_eq!(cuts, if end == ShaftEnd::Plain { 0 } else { 2 }, "{end:?}");
            assert_eq!(ShaftEnd::parse(end.name()), Some(end));
        }
    }
    // Ends that don't fit are left plain, with a note.
    for (end, size) in [(ShaftEnd::DFlat, 12.0), (ShaftEnd::Hex, 11.0), (ShaftEnd::Cross, 9.5), (ShaftEnd::Keyway, 8.0), (ShaftEnd::CentreHole, 9.0)]
    {
        let s = Spec { shaft_front: Shaft { diameter: 10.0, length: 12.0, end, end_size: size, ..Shaft::default() }, ..Spec::default() };
        let p = planned(&s);
        assert!(warned(&p, "left plain"), "{end:?}: {:?}", p.warnings);
        assert!(!p.bodies[0].steps.iter().any(|st| matches!(st, Step::Prism { op: Op::Cut, .. })));
    }
}

#[test]
fn shafts_and_bores_are_told_apart() {
    // A bore through a shaft: a hollow shaft, the bore as long as the whole part.
    let mut s = Spec {
        bore: Bore { kind: BoreKind::Round, diameter: 4.0, ..Bore::default() },
        shaft_front: Shaft { diameter: 10.0, length: 20.0, ..Shaft::default() },
        shaft_back: Shaft { diameter: 10.0, length: 5.0, ..Shaft::default() },
        bore_chamfer: 0.5,
        ..Spec::default()
    };
    let p = planned(&s);
    assert!(warned(&p, "hollow shaft"));
    let b = &p.bodies[0];
    let bore =
        b.steps.iter().find_map(|st| if let Step::Prism { z0, height, op: Op::Cut, .. } = st { Some((*z0, z0 + height)) } else { None }).unwrap();
    assert_eq!(bore, (-6.0, 31.0));
    // Its chamfers are at the shafts' ends.
    let rv = revolves(b);
    assert_eq!(rv.len(), 2);
    assert!(rv.iter().any(|t| in_triangle(V2::new(2.0, -5.0), t)) && rv.iter().any(|t| in_triangle(V2::new(2.0, 30.0), t)));
    // A bore as big as the shaft: no shaft.
    s.bore.diameter = 9.5;
    let p = planned(&s);
    assert!(warned(&p, "left out"), "{:?}", p.warnings);
    assert_eq!(extent(&p.bodies[0]), (0.0, s.face_width));
    // A shaft bigger than the root circle is built, and said.
    let s = Spec { shaft_back: Shaft { diameter: 40.0, length: 5.0, ..Shaft::default() }, ..Spec::default() };
    let p = planned(&s);
    assert!(warned(&p, "bigger than the root circle"), "{:?}", p.warnings);
    assert_eq!(extent(&p.bodies[0]).0, -5.0);
    // A front shaft no longer than the hub is hidden in it.
    let mut s = Spec::default();
    (s.hub_diameter, s.hub_length) = (16.0, 8.0);
    s.shaft_front = Shaft { diameter: 10.0, length: 6.0, end: ShaftEnd::DFlat, ..Shaft::default() };
    let p = planned(&s);
    assert!(warned(&p, "hidden inside the hub") && warned(&p, "left plain"), "{:?}", p.warnings);
}

#[test]
fn each_member_of_a_set_gets_its_own_shafts() {
    let one = Shaft { diameter: 6.0, length: 10.0, ..Shaft::default() };
    let two = Shaft { diameter: 5.0, length: 7.0, end: ShaftEnd::Cross, ..Shaft::default() };
    // Pair: gear 1 front, gear 2 both.
    let s = Spec { shaft_front: one, shaft2_front: two, shaft2_back: two, ..Spec::defaults_for(Kind::Pair) };
    let p = planned(&s);
    assert_eq!(extent(&p.bodies[0]), (0.0, 20.0));
    assert_eq!(extent(&p.bodies[1]), (-7.0, 17.0));
    // Planetary: the sun and every planet; the ring none.
    let s = Spec { shaft_back: one, shaft2_front: two, ..Spec::defaults_for(Kind::Planetary) };
    let p = planned(&s);
    assert_eq!(extent(&p.bodies[0]), (-10.0, 10.0));
    for b in &p.bodies[1..4] {
        assert_eq!(extent(b), (0.0, 17.0), "{}", b.name);
    }
    assert_eq!(extent(&p.bodies[4]), (0.0, 10.0));
    // Worm set: the wheel (centred on its mid plane) and the worm, whose shafts run on from its ends.
    let s = Spec { shaft_front: one, shaft2_front: two, shaft2_back: one, ..Spec::defaults_for(Kind::Worm) };
    let p = planned(&s);
    let wheel = extent(&p.bodies[0]);
    assert!((wheel.1 - wheel.0 - (-2.0 * wheel.0 + 10.0)).abs() < 1e-9 && wheel.0 < 0.0, "{wheel:?}");
    let length = fact(&p, "worm length");
    let worm = extent(&p.bodies[1]);
    assert!((worm.0 + length / 2.0 + 10.0).abs() < 1e-9 && (worm.1 - length / 2.0 - 7.0).abs() < 1e-9, "{worm:?} {length}");
    // Crossed helical and cycloidal gears, and bevels (back shaft from the large end, front
    // from the small end).
    for k in [Kind::CrossedHelical, Kind::Cycloidal, Kind::Bevel, Kind::SpiralBevel] {
        let s = Spec {
            shaft_front: Shaft { diameter: 3.0, length: 9.0, ..Shaft::default() },
            shaft_back: one,
            shaft2_back: two,
            ..Spec::defaults_for(k)
        };
        let p = planned(&s);
        let (a, b) = (extent(&p.bodies[0]), extent(&p.bodies[1]));
        let (a0, b0) = {
            let q = planned(&Spec::defaults_for(k));
            (extent(&q.bodies[0]), extent(&q.bodies[1]))
        };
        assert!((a.0 - (a0.0 - 10.0)).abs() < 1e-9 && (a.1 - (a0.1 + 9.0)).abs() < 1e-9, "{k:?} {a:?} {a0:?}");
        assert!((b.0 - (b0.0 - 7.0)).abs() < 1e-9 && b.1 == b0.1, "{k:?} {b:?} {b0:?}");
    }
}

#[test]
fn teeth_are_chamfered_on_each_face() {
    let front = Chamfer { size: 1.0, angle: 45.0 };
    let back = Chamfer { size: 0.6, angle: 30.0 };
    for k in [Kind::Spur, Kind::Helical, Kind::Herringbone, Kind::Pair, Kind::Planetary, Kind::Worm, Kind::CrossedHelical, Kind::Internal] {
        let s = Spec { tooth_chamfer_front: front, tooth_chamfer_back: back, ..Spec::defaults_for(k) };
        let p = planned(&s);
        assert!(p.warnings.iter().all(|w| !w.contains("chamfer")), "{k:?}: {:?}", p.warnings);
        for b in &p.bodies {
            let rv = revolves(b);
            if b.name.starts_with("Worm 1") {
                assert!(rv.is_empty());
                continue;
            }
            assert_eq!(rv.len(), 2, "{k:?} {}", b.name);
            let (z0, z1) = extent(b);
            let ring = b.name.starts_with("Ring");
            // The tip circle's edge on each face is inside its triangle, which cuts no deeper
            // than asked and stays near the tip circle.
            let tip = if ring {
                let Step::Prism { region, .. } = &b.steps[0] else { panic!() };
                region.holes[0].polyline(1e-3).iter().map(|q| q.len()).fold(f64::INFINITY, f64::min)
            } else {
                first_outline(b).max_radius()
            };
            let (tb, tf) = (rv[0], rv[1]);
            assert!(in_triangle(V2::new(tip, z0), tb) && in_triangle(V2::new(tip, z1), tf), "{k:?} {} tip {tip} {z0}..{z1} {tb:?} {tf:?}", b.name);
            for (t, c, z, dz) in [(tb, &back, z0, 1.0), (tf, &front, z1, -1.0)] {
                assert_eq!(t.len(), 3);
                let depth = c.depth();
                for q in t.iter() {
                    assert!(q.x > 0.0 && (q.x - tip).abs() <= 1.2 * c.size, "{q:?}");
                    assert!((q.y - z) * dz <= 1.2 * depth && (q.y - z) * dz >= -0.2 * depth, "{q:?}");
                }
                // The two points where the chamfer meets the face and the tip cylinder are on
                // the triangle's long side.
                let inward = if ring { 1.0 } else { -1.0 };
                let (a, b2) = (V2::new(tip + inward * c.size, z), V2::new(tip, z + dz * depth));
                let on = |q: V2| (0..3).any(|i| dist_to(&[t[i], t[(i + 1) % 3]], q) < 1e-9);
                assert!(on(a) && on(b2), "{k:?} {t:?} {a:?} {b2:?}");
            }
        }
    }
    // Chamfers too big for the face are cut down, and said.
    let s =
        Spec { tooth_chamfer_front: Chamfer { size: 3.0, angle: 80.0 }, tooth_chamfer_back: Chamfer { size: 3.0, angle: 80.0 }, ..Spec::default() };
    let p = planned(&s);
    assert!(warned(&p, "cut down"), "{:?}", p.warnings);
    for t in revolves(&p.bodies[0]) {
        assert!(t.iter().all(|q| q.y > -1.0 && q.y < s.face_width + 1.0));
    }
    // Bevels say they have none.
    let p = planned(&Spec { tooth_chamfer_front: front, ..Spec::defaults_for(Kind::Bevel) });
    assert!(warned(&p, "not built on bevel"));
}

#[test]
fn bores_and_hubs_are_chamfered() {
    let mut s = Spec { bore: Bore { kind: BoreKind::Keyway, diameter: 8.0, ..Bore::default() }, ..Spec::default() };
    (s.hub_diameter, s.hub_length) = (18.0, 6.0);
    (s.bore_chamfer, s.hub_chamfer) = (0.5, 1.0);
    let p = planned(&s);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let rv = revolves(&p.bodies[0]);
    assert_eq!(rv.len(), 3);
    // The hub's outer edge, and the bore's edge on the back face and on the hub's end.
    assert!(in_triangle(V2::new(9.0, 16.0), rv[0]));
    assert!(in_triangle(V2::new(4.0, 0.0), rv[1]) && in_triangle(V2::new(4.0, 16.0), rv[2]));
    for t in &rv {
        assert!(t.iter().all(|q| q.x > 0.0));
    }
    // No bore, no hub: nothing to chamfer.
    let p = planned(&Spec { bore_chamfer: 0.5, hub_chamfer: 1.0, ..Spec::default() });
    assert!(revolves(&p.bodies[0]).is_empty());
}

fn tooth_spec() -> Spec {
    Spec { teeth: 24, ..Spec::default() }
}

#[test]
fn tips_are_rounded_tangent_to_flank_and_tip_circle() {
    let plain = planned(&tooth_spec());
    for round in [0.05, 0.15, 0.3] {
        let s = Spec { tip_round: round, ..tooth_spec() };
        let p = planned(&s);
        assert!(p.warnings.is_empty(), "{round}: {:?}", p.warnings);
        let o = first_outline(&p.bodies[0]);
        check_outline(o).unwrap();
        let ra = fact(&p, "tip diameter") / 2.0;
        assert!((o.max_radius() - ra).abs() < 1e-9);
        let area = |o: &Outline| fine(o).windows(2).map(|w| w[0].cross(w[1])).sum::<f64>() / 2.0;
        let off = area(first_outline(&plain.bodies[0])) - area(o);
        // A corner rounded off between lines 90° + the pressure angle at the tip apart, twice a tooth.
        assert!(off > 0.0 && off < f64::from(s.teeth) * 2.0 * (round * s.module).powi(2), "{round}: {off}");
        // Two more arcs a tooth, of the radius asked for, each leaving its neighbours without
        // a corner.
        let rho = round * s.module;
        let segs = &o.segs;
        let small: Vec<usize> = (0..segs.len()).filter(|i| matches!(&segs[*i], Seg::Arc { c, .. } if c.len() > 1.0)).collect();
        assert_eq!(small.len(), 2 * s.teeth as usize, "{round}");
        for &i in &small {
            let Seg::Arc { c, a, b, ccw } = &segs[i] else { panic!() };
            assert!(*ccw && (c.dist(*a) - rho).abs() < 1e-6 && (c.dist(*b) - rho).abs() < 1e-6, "{round}: radius {} for {rho}", c.dist(*a));
            let tangent = |q: V2| V2::new(-(q - *c).y, (q - *c).x).norm();
            let (prev, next) = (&segs[(i + segs.len() - 1) % segs.len()], &segs[(i + 1) % segs.len()]);
            let dir_out = |sg: &Seg| -> V2 {
                let mut pts = Vec::new();
                match sg {
                    Seg::Spline(p) => crate::geom::natural_spline(p, 200, &mut pts),
                    other => other.sample(1e-7, &mut pts),
                }
                (pts[pts.len() - 1] - pts[pts.len() - 2]).norm()
            };
            let dir_in = |sg: &Seg| -> V2 {
                let mut pts = Vec::new();
                match sg {
                    Seg::Spline(p) => crate::geom::natural_spline(p, 200, &mut pts),
                    other => other.sample(1e-7, &mut pts),
                }
                (pts[1] - pts[0]).norm()
            };
            let turn = |u: V2, v: V2| u.cross(v).atan2(u.dot(v)).abs().to_degrees();
            assert!(turn(dir_out(prev), tangent(*a)) < 1.5, "{round}: corner of {}° into the rounding", turn(dir_out(prev), tangent(*a)));
            assert!(turn(tangent(*b), dir_in(next)) < 1.5, "{round}: corner of {}° out of the rounding", turn(tangent(*b), dir_in(next)));
        }
        // The reported land is the tooth's own, as before the rounding.
        assert_eq!(fact(&p, "tip land"), fact(&plain, "tip land"));
    }
    // A radius the tip has no room for is cut down, and said.
    let p = planned(&Spec { tip_round: 0.5, teeth: 12, shift: 0.5, ..Spec::default() });
    assert!(warned(&p, "too thin"), "{:?}", p.warnings);
    check_outline(first_outline(&p.bodies[0])).unwrap();
}

#[test]
fn xy_compensation_moves_the_whole_outline_in_by_its_amount() {
    let d = 0.08;
    let s0 = tooth_spec();
    let s = Spec { xy_compensation: d, ..s0.clone() };
    let (p0, p) = (planned(&s0), planned(&s));
    assert!((fact(&p0, "tip diameter") - fact(&p, "tip diameter") - 2.0 * d).abs() < 1e-9);
    assert!((fact(&p0, "root diameter") - fact(&p, "root diameter") - 2.0 * d).abs() < 1e-9);
    let at = s.pressure_angle.to_radians();
    assert!((fact(&p0, "tooth thickness (pitch)") - fact(&p, "tooth thickness (pitch)") - 2.0 * d / at.cos()).abs() < 1e-9);
    // Every point of the compensated outline is its amount inside the plain one: exactly on
    // the flanks, tips and root circle, and within a few percent round the root fillets (which
    // the deeper cutter generates rather than offsets).
    let outer = fine(first_outline(&p0.bodies[0]));
    let inner = fine(first_outline(&p.bodies[0]));
    let (rb, rf) = (fact(&p, "base diameter") / 2.0, fact(&p0, "root diameter") / 2.0);
    let (mut worst_flank, mut worst_any): (f64, f64) = (0.0, 0.0);
    for q in inner.iter().step_by(7) {
        let gap = dist_to(&outer, *q);
        worst_any = worst_any.max((gap - d).abs());
        if q.len() > rb.max(rf) + 0.6 * s.module {
            worst_flank = worst_flank.max((gap - d).abs());
        }
    }
    assert!(worst_flank < 2e-3, "flanks off by {worst_flank}");
    assert!(worst_any < 0.25 * d, "outline off by {worst_any}");
    // Helical gears thin by the transverse angle's cosine.
    let s0 = Spec { helix: 25.0, ..Spec::defaults_for(Kind::Helical) };
    let (p0, p) = (planned(&s0), planned(&Spec { xy_compensation: d, ..s0.clone() }));
    let at = (s0.pressure_angle.to_radians().tan() / 25f64.to_radians().cos()).atan();
    assert!((fact(&p0, "tooth thickness (pitch)") - fact(&p, "tooth thickness (pitch)") - 2.0 * d / at.cos()).abs() < 1e-9);
    // Rings are compensated the same way: thinner teeth, tips out, roots out.
    let s0 = Spec::defaults_for(Kind::Internal);
    let (p0, p) = (planned(&s0), planned(&Spec { xy_compensation: d, ..s0.clone() }));
    assert!((fact(&p, "tip diameter (inner)") - fact(&p0, "tip diameter (inner)") - 2.0 * d).abs() < 1e-9);
    assert!((fact(&p, "root diameter") - fact(&p0, "root diameter") - 2.0 * d).abs() < 1e-9);
}

/// The 2D mesh of a pair's preview over two pitches: (deepest overlap, least gap, largest gap).
fn mesh(s: &Spec) -> (f64, f64, f64) {
    let p = planned(s);
    let m = s.module;
    let (a, b) = (Member::of(&p.preview[0], 0.2 * m, 2e-5 * m), Member::of(&p.preview[1], 0.2 * m, 2e-5 * m));
    let st = run(&a, &b, 0.0, 2.0 * TAU / f64::from(s.teeth), 40);
    (st.depth, st.gap_min, st.gap_max)
}

#[test]
fn tuned_teeth_still_mesh() {
    let base = Spec { teeth: 16, teeth2: 27, ..Spec::defaults_for(Kind::Pair) };
    let m = base.module;
    let allow = (2.0 * crate::involute::PROFILE_TOLERANCE + 3e-4) * m;
    let (d0, _, g0) = mesh(&base);
    assert!(d0 <= allow && g0 < 2e-3 * m);
    // Rounded and relieved tips take nothing from the flanks that carry the mesh.
    for s in [
        Spec { tip_round: 0.2, ..base.clone() },
        Spec { tip_relief: 0.02, ..base.clone() },
        Spec { tip_round: 0.15, tip_relief: 0.03, tip_relief_height: 0.6, helix: 20.0, ..base.clone() },
        Spec { tip_shortening: 0.3, clearance: 0.4, ..base.clone() },
        Spec { tip_diameter: 34.5, tip_diameter2: 57.0, ..base.clone() },
    ] {
        let (depth, _, gap) = mesh(&s);
        assert!(depth <= allow, "{s:?}: flanks cross by {depth}");
        assert!(gap < 0.05 * m, "{s:?}: flanks {gap} apart");
    }
    // The compensation opens the mesh by its amount on each flank (two gears: twice), along the
    // line of action; a gear 2 backlash adds to gear 1's.
    let d = 0.06;
    let (depth, lo, hi) = mesh(&Spec { xy_compensation: d, ..base.clone() });
    assert!(depth == 0.0 && (lo - 2.0 * d).abs() < 0.1 * d + 2e-3 * m && hi < 2.0 * d + 0.03 * m, "{depth} {lo} {hi}");
    let aw = 20f64.to_radians().cos();
    let (depth, lo, _) = mesh(&Spec { backlash: 0.05, backlash2: Some(0.15), ..base.clone() });
    // (The checker's gap is a flank's: half the mesh's backlash with the teeth centred.)
    assert!(depth == 0.0 && lo > 0.8 * 0.1 * aw - 2e-3 * m && lo < 0.1 * aw + 2e-3 * m, "{lo}");
    let p = planned(&Spec { backlash: 0.05, backlash2: Some(0.15), ..base.clone() });
    assert!((fact(&p, "backlash (both gears, pitch circle)") - 0.2).abs() < 1e-12);
    assert!((fact(&p, "gear 1 tooth thickness (pitch)") - fact(&p, "gear 2 tooth thickness (pitch)") - 0.1).abs() < 1e-9);
    // Planetary sets too.
    let s = Spec { tip_round: 0.15, xy_compensation: 0.05, backlash2: Some(0.02), ..Spec::defaults_for(Kind::Planetary) };
    let p = planned(&s);
    let ms: Vec<Member> = p.preview.iter().map(|g| Member::of(g, 0.2 * m, 2e-5 * m)).collect();
    let ring = ms.len() - 1;
    for k in 1..ring {
        for (a, b) in [(0, k), (k, ring)] {
            let st = run(&ms[a], &ms[b], 0.0, 2.0 * TAU / f64::from(s.teeth), 40);
            assert!(st.depth == 0.0 && st.gap_min > 0.05, "{a}-{b}: {} {}", st.depth, st.gap_min);
        }
    }
}

#[test]
fn clearance_tip_shortening_and_tip_diameter_are_their_own_settings() {
    let s0 = tooth_spec();
    let p0 = planned(&s0);
    let (m, z) = (s0.module, f64::from(s0.teeth));
    let p = planned(&Spec { clearance: 0.4, ..s0.clone() });
    assert!((fact(&p, "root diameter") - m * (z - 2.0 * 1.4)).abs() < 1e-9);
    assert_eq!(Spec { clearance: 0.4, ..s0.clone() }.clearance_eff(), 0.4);
    assert_eq!(s0.clearance_eff(), 0.25);
    let p = planned(&Spec { tip_shortening: 0.25, ..s0.clone() });
    assert!((fact(&p0, "tip diameter") - fact(&p, "tip diameter") - 0.5).abs() < 1e-9);
    let p = planned(&Spec { tip_diameter: 51.0, ..s0.clone() });
    assert!((fact(&p, "tip diameter") - 51.0).abs() < 1e-9);
    // Only ever lowers the tips.
    let p = planned(&Spec { tip_diameter: 60.0, ..s0.clone() });
    assert!(warned(&p, "left as it is") && fact(&p, "tip diameter") == fact(&p0, "tip diameter"));
    let p = planned(&Spec { tip_diameter: 40.0, ..s0.clone() });
    assert!(warned(&p, "no addendum"));
    // Out of range values are refused by name.
    for (s, word) in [
        (Spec { clearance: 1.5, ..s0.clone() }, "clearance"),
        (Spec { tip_round: 0.8, ..s0.clone() }, "rounding"),
        (Spec { xy_compensation: 0.9, ..s0.clone() }, "compensation"),
        (Spec { tip_shortening: -0.1, ..s0.clone() }, "tip shortening"),
        (Spec { crowning: 0.5, ..s0.clone() }, "crowning"),
        (Spec { tooth_chamfer_back: Chamfer { size: 1.0, angle: 89.0 }, ..s0.clone() }, "angle"),
        (Spec { shaft_back: Shaft { diameter: -1.0, ..Shaft::default() }, ..s0.clone() }, "back shaft diameter"),
        (Spec { hardware: Hardware { set_screws: 9, ..Hardware::default() }, ..s0.clone() }, "set screws"),
        (Spec { backlash2: Some(f64::NAN), ..s0.clone() }, "gear 2 backlash"),
        (Spec { pressure_angle: 30.0, clearance: 0.36, ..s0.clone() }, "pointed"),
    ] {
        let e = plan(&s).err().unwrap_or_else(|| panic!("{word} should be refused"));
        assert!(e.contains(word), "{e}");
    }
}

#[test]
fn tip_relief_thins_the_tip_and_nothing_below_its_start() {
    let s0 = tooth_spec();
    let s = Spec { tip_relief: 0.03, tip_relief_height: 0.8, ..s0.clone() };
    let (p0, p) = (planned(&s0), planned(&s));
    let (o0, o) = (fine(first_outline(&p0.bodies[0])), fine(first_outline(&p.bodies[0])));
    let ra = fact(&p, "tip diameter") / 2.0;
    let rb = fact(&p, "base diameter") / 2.0;
    let start = ra - 0.8;
    let mut at_tip: f64 = 0.0;
    for q in o.iter().step_by(3) {
        let gap = dist_to(&o0, *q);
        if q.len() < start - 0.05 {
            assert!(gap < 1.5e-3, "relief below its start: {gap} at radius {}", q.len());
        } else if q.len() < ra - 0.04 {
            // (Nearer the tip the plain tooth's tip circle is the closest thing.)
            // Linear in the radius, measured along the base tangent (the flank's normal).
            let want = 0.03 * (q.len() - start).max(0.0) / 0.8;
            assert!((gap - want).abs() < 2.5e-3, "at radius {}: {gap} for {want}", q.len());
            at_tip = at_tip.max(gap);
        }
    }
    assert!(at_tip > 0.025, "{at_tip}");
    // The outline's land at the tip is thinner by the relief on both flanks.
    let land = |o: &Outline| {
        o.segs
            .iter()
            .find_map(|sg| if let Seg::Arc { c, a, b, .. } = sg { (c.len() < 1e-9 && (a.len() - ra).abs() < 1e-9).then(|| a.dist(*b)) } else { None })
            .unwrap()
    };
    let (l0, l) = (land(first_outline(&p0.bodies[0])), land(first_outline(&p.bodies[0])));
    assert!((l0 - l - 2.0 * 0.03 * ra / rb).abs() < 2e-3, "{l0} {l}");
}

#[test]
fn elephant_foot_relief_insets_the_first_layers() {
    for (k, helix) in [(Kind::Spur, 0.0), (Kind::Helical, 20.0), (Kind::Herringbone, 25.0)] {
        let s0 = Spec { helix, ..Spec::defaults_for(k) };
        let s = Spec {
            elephant_foot: 0.1,
            elephant_foot_height: 0.3,
            bore: Bore { kind: BoreKind::Round, diameter: 6.0, ..Bore::default() },
            ..s0.clone()
        };
        let (p0, p) = (planned(&s0), planned(&s));
        let (b0, b) = (&p0.bodies[0], &p.bodies[0]);
        let stack = |b: &Body| -> Vec<(Outline, f64, f64, f64)> {
            b.steps
                .iter()
                .filter_map(|st| {
                    if let Step::Prism { region, z0, height, twist, op: Op::New | Op::Join } = st {
                        Some((region.outer[0].clone(), *z0, *height, *twist))
                    } else {
                        None
                    }
                })
                .collect()
        };
        let (st0, st) = (stack(b0), stack(b));
        assert_eq!(st.len(), st0.len() + 1, "{k:?}");
        assert_eq!((st[0].1, st[0].2), (0.0, 0.3));
        assert!((st[1].1 - 0.3).abs() < 1e-12);
        // The stack is unbroken, and turns as much as the plain one in all.
        for w in st.windows(2) {
            assert!((w[0].1 + w[0].2 - w[1].1).abs() < 1e-12);
        }
        let net = |v: &[(Outline, f64, f64, f64)]| v.iter().map(|x| x.3.abs()).sum::<f64>();
        assert!((net(&st) - net(&st0)).abs() < 1e-12, "{k:?}");
        // The foot is inside the tooth above it (which is the plain gear's, turned to where
        // the helix has brought it).
        let (foot, above) = (fine(&st[0].0), fine(&st[1].0.rotated(-st[0].3)));
        let plain = fine(&st0[0].0);
        for q in foot.iter().step_by(11) {
            let gap = dist_to(&plain, *q);
            assert!(gap > 0.07 && gap < 0.13, "{k:?}: foot {gap} inside");
        }
        for q in above.iter().step_by(11) {
            assert!(dist_to(&plain, *q) < 1e-6, "{k:?}: the tooth above the foot moved");
        }
    }
    // The height defaults to 0.2 mm and never takes over the tooth.
    let p = planned(&Spec { elephant_foot: 0.1, face_width: 0.6, ..Spec::default() });
    assert!(matches!(&p.bodies[0].steps[0], Step::Prism { height, .. } if (*height - 0.15).abs() < 1e-12));
    let p = planned(&Spec { elephant_foot: 0.1, ..Spec::default() });
    assert!(matches!(&p.bodies[0].steps[0], Step::Prism { height, .. } if *height == 0.2));
}

#[test]
fn crowned_teeth_are_a_loft_through_three_sections() {
    let s = Spec { crowning: 0.03, ..tooth_spec() };
    let p = planned(&s);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let Step::Loft { sections, op: Op::New } = &p.bodies[0].steps[0] else { panic!("{:?}", p.bodies[0].steps[0]) };
    assert_eq!(sections.iter().map(|x| x.1).collect::<Vec<_>>(), vec![0.0, 5.0, 10.0]);
    assert_eq!(sections[0].0, sections[2].0);
    // The end sections are the middle one with each flank 0.03 mm (along its normal) inside.
    let (end, mid) = (fine(&sections[0].0), fine(&sections[1].0));
    let (ra, rb, rf) = (fact(&p, "tip diameter") / 2.0, fact(&p, "base diameter") / 2.0, fact(&p, "root diameter") / 2.0);
    let mut seen = 0;
    for q in end.iter().step_by(5) {
        if q.len() > rb.max(rf) + 0.8 && q.len() < ra - 0.2 {
            let gap = dist_to(&mid, *q);
            assert!((gap - 0.03).abs() < 2e-3, "{gap} at radius {}", q.len());
            seen += 1;
        }
    }
    assert!(seen > 100);
    for (o, _) in sections {
        assert!((o.max_radius() - ra).abs() < 1e-9);
    }
    // With rounded tips as well; helical teeth are left uncrowned, and say so.
    planned(&Spec { crowning: 0.03, tip_round: 0.15, tip_relief: 0.02, ..tooth_spec() });
    let p = planned(&Spec { crowning: 0.03, helix: 15.0, ..Spec::defaults_for(Kind::Helical) });
    assert!(warned(&p, "straight teeth") && matches!(&p.bodies[0].steps[0], Step::Prism { .. }));
}

#[test]
fn gear_2_can_be_wider_and_offset() {
    let s0 = Spec { helix: 15.0, ..Spec::defaults_for(Kind::Pair) };
    let p0 = planned(&s0);
    let s = Spec { face_width2: 16.0, face_offset2: 1.0, ..s0.clone() };
    let p = planned(&s);
    let (b0, b) = (&p0.bodies[1], &p.bodies[1]);
    assert_eq!(extent(b), (0.0, 16.0));
    assert!((b.frame.origin[2] - (-3.0 + 1.0)).abs() < 1e-12 && b.frame.origin[0] == b0.frame.origin[0]);
    // The wider gear's teeth pass through where the plain gear's are: its section at gear 1's
    // back face (2 mm up its own axis) is the plain gear's there.
    let (Step::Prism { region: r0, .. }, Step::Prism { region: r, twist, height, .. }) = (&b0.steps[0], &b.steps[0]) else { panic!() };
    let (plain, wide) = (fine(&r0.outer[0]), fine(&r.outer[0].rotated(twist / height * 2.0)));
    for q in wide.iter().step_by(13) {
        assert!(dist_to(&plain, *q) < 1e-6);
    }
    // The overlap ratio counts the shared width only.
    let narrow = planned(&Spec { face_width2: 4.0, ..s0.clone() });
    assert!((fact(&narrow, "overlap ratio") / fact(&p0, "overlap ratio") - 0.4).abs() < 1e-9);
    assert!((fact(&p, "overlap ratio") - fact(&p0, "overlap ratio")).abs() < 1e-9);
    // Faces that miss each other are refused.
    let e = plan(&Spec { face_offset2: 30.0, ..s0.clone() }).unwrap_err();
    assert!(e.contains("overlap"), "{e}");
}

fn hub_spec() -> Spec {
    Spec {
        teeth: 30,
        hub_diameter: 24.0,
        hub_length: 10.0,
        bore: Bore { kind: BoreKind::Round, diameter: 8.0, ..Bore::default() },
        ..Spec::default()
    }
}

#[test]
fn hubs_take_set_screws_nut_traps_and_inserts() {
    let s = Spec {
        hardware: Hardware {
            set_screw: 3.0,
            set_screws: 2,
            nut_width: 5.5,
            nut_thickness: 2.4,
            insert_diameter: 4.0,
            insert_depth: 4.0,
            ..Hardware::default()
        },
        ..hub_spec()
    };
    let p = planned(&s);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let b = &p.bodies[0];
    let pl = placed(b);
    // Two screws: a hole, an insert pocket and a nut trap each.
    assert_eq!(pl.len(), 6);
    let world = |f: &Frame, q: [f64; 3]| b.frame.then(f).world(q);
    for (f, st) in &pl {
        // Each frame looks out from the axis, square to it, at the hub's mid length.
        assert_eq!(f.origin, [0.0, 0.0, 15.0]);
        assert!(f.z[2] == 0.0 && (f.z[0].hypot(f.z[1]) - 1.0).abs() < 1e-12);
        let y = f.y();
        assert!((y[2] - 1.0).abs() < 1e-12, "y runs along the gear's axis: {y:?}");
        let Step::Prism { z0, height, op: Op::Cut, .. } = st else { panic!() };
        // Out through the hub's wall, not past the far side.
        assert!(*z0 >= 0.0 && z0 + height <= 12.0 + 1.0 + 1e-9);
    }
    let dirs: Vec<[f64; 3]> = pl.iter().map(|(f, _)| f.z).collect();
    assert!(dirs.iter().any(|d| (d[1] - 1.0).abs() < 1e-12) && dirs.iter().any(|d| (d[0] + 1.0).abs() < 1e-12), "{dirs:?}");
    // The screw hole runs from the axis out of the hub; the nut sits in the wall.
    let (f, st) = pl.iter().find(|(f, st)| (f.z[1] - 1.0).abs() < 1e-12 && matches!(st, Step::Prism { z0, .. } if *z0 == 0.0)).unwrap();
    let Step::Prism { z0, height, .. } = st else { panic!() };
    let (a, e) = (world(f, [0.0, 0.0, *z0]), world(f, [0.0, 0.0, z0 + height]));
    assert!(a[1].abs() < 1e-12 && (e[1] - 13.0).abs() < 1e-12 && (a[2] - 15.0).abs() < 1e-12 && (e[2] - 15.0).abs() < 1e-12, "{a:?} {e:?}");
    let nut = pl
        .iter()
        .find_map(|(_, st)| {
            if let Step::Prism { region, z0, height, .. } = st { (region.outer[0].segs.len() == 4).then_some((region, *z0, *height)) } else { None }
        })
        .unwrap();
    assert!((nut.1 - (8.0 - 1.2)).abs() < 1e-12 && nut.2 == 2.4);
    // The trap is open to the hub's end (local y is the gear's +z; the hub ends 5 above).
    let top = nut.0.outer[0].polyline(0.1).iter().map(|q| q.y).fold(f64::NEG_INFINITY, f64::max);
    assert!(top > 5.0);
    // Without a hub the hardware is left out, and said.
    let p = planned(&Spec { hub_diameter: 0.0, ..s.clone() });
    assert!(warned(&p, "go in a hub") && placed(&p.bodies[0]).is_empty());
    // Hardware that doesn't fit the hub is left out piece by piece.
    let p = planned(&Spec {
        hardware: Hardware { set_screw: 9.5, nut_width: 5.5, nut_thickness: 7.0, insert_diameter: 11.0, ..Hardware::default() },
        ..hub_spec()
    });
    assert!(placed(&p.bodies[0]).is_empty() && p.warnings.len() == 3, "{:?}", p.warnings);
    // Gear 2 has its own.
    let s = Spec { hardware2: Hardware { set_screw: 3.0, ..Hardware::default() }, bore2: hub_spec().bore, ..Spec { kind: Kind::Pair, ..hub_spec() } };
    let p = planned(&s);
    assert!(placed(&p.bodies[0]).is_empty() && placed(&p.bodies[1]).len() == 1);
}

#[test]
fn clamping_hubs_are_slit_and_cross_drilled() {
    let s = Spec { hardware: Hardware { clamp_slit: 1.5, clamp_screw: 3.0, ..Hardware::default() }, ..hub_spec() };
    let p = planned(&s);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let b = &p.bodies[0];
    // The slit: over the hub only, from the bore out through the wall on −y.
    let slit = b
        .steps
        .iter()
        .find_map(|st| {
            if let Step::Prism { region, z0, height, op: Op::Cut, .. } = st {
                (region.outer[0].segs.len() == 4).then_some((region.outer[0].polyline(0.1), *z0, *height))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!((slit.1, slit.2), (10.0, 11.0));
    assert!(slit.0.iter().all(|q| q.x.abs() <= 0.75 + 1e-12 && q.y <= 1e-12) && slit.0.iter().any(|q| q.y < -12.0));
    // The clamp screw: across the slit, through the wall's middle, the whole hub wide.
    let pl = placed(b);
    assert_eq!(pl.len(), 1);
    let (f, Step::Prism { z0, height, .. }) = pl[0] else { panic!() };
    assert_eq!(f.z, [1.0, 0.0, 0.0]);
    let (a, e) = (f.world([0.0, 0.0, *z0]), f.world([0.0, 0.0, z0 + height]));
    assert!(a[0] < -12.0 && e[0] > 12.0 && (a[1] + 8.0).abs() < 1e-12 && a[1] == e[1] && a[2] == 15.0 && e[2] == 15.0, "{a:?} {e:?}");
    // No bore, no slit.
    let p = planned(&Spec { bore: Bore::default(), ..s.clone() });
    assert!(warned(&p, "clamp slit needs a bore"));
}

#[test]
fn bearing_pockets_come_from_the_standard_table() {
    assert_eq!(bearing("608"), Some((8.0, 22.0, 7.0)));
    assert_eq!(bearing("625"), Some((5.0, 16.0, 5.0)));
    assert_eq!(bearing("6001"), Some((12.0, 28.0, 8.0)));
    assert_eq!(bearing(" mr105 "), Some((5.0, 10.0, 4.0)));
    assert_eq!(bearing("6004"), Some((20.0, 42.0, 12.0)));
    assert_eq!(bearing("nope"), None);
    for (name, d, od, w) in BEARINGS {
        assert!(*d > 0.0 && od > d && *w > 0.0 && *w < *od, "{name}");
        assert_eq!(BEARINGS.iter().filter(|b| b.0 == *name).count(), 1, "{name}");
    }
    let mut h = Hardware::default();
    assert!(h.set_bearing("608") && !h.set_bearing("9999"));
    h.bearing_fit = 0.2;
    let s =
        Spec { teeth: 30, face_width: 12.0, hardware: h, bore: Bore { kind: BoreKind::Round, diameter: 9.0, ..Bore::default() }, ..Spec::default() };
    let p = planned(&s);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    // The pocket: Ø22.2, 7 deep into the back face, cut before the bore.
    let cuts: Vec<(f64, f64, f64)> = p.bodies[0]
        .steps
        .iter()
        .filter_map(|st| match st {
            Step::Prism { region, z0, height, op: Op::Cut, .. } => match region.outer[0].segs[0] {
                Seg::Circle(_, r) => Some((r, *z0, z0 + height)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(cuts.len(), 2);
    assert!((cuts[0].0 - 11.1).abs() < 1e-12 && cuts[0].1 == -1.0 && cuts[0].2 == 7.0, "{cuts:?}");
    assert_eq!(cuts[1].0, 4.5);
    // Planets carry them too (gear 2's hardware).
    let s = Spec {
        hardware2: Hardware { bearing_od: 16.0, bearing_width: 5.0, ..Hardware::default() },
        teeth2: 24,
        teeth: 12,
        teeth3: 60,
        ..Spec::defaults_for(Kind::Planetary)
    };
    let p = planned(&s);
    assert!(p.warnings.iter().all(|w| !w.contains("bearing")), "{:?}", p.warnings);
    assert!(p.bodies[1..4].iter().all(|b| b.steps.len() == 2) && p.bodies[0].steps.len() == 1);
    // A bearing bigger than the gear, or where a shaft is, is left out with the reason.
    let p = planned(&Spec { hardware: Hardware { bearing_od: 60.0, bearing_width: 7.0, ..Hardware::default() }, ..Spec::default() });
    assert!(warned(&p, "doesn't fit") && p.bodies[0].steps.len() == 1);
    let p = planned(&Spec { hardware: h, teeth: 30, shaft_back: Shaft { diameter: 6.0, length: 5.0, ..Shaft::default() }, ..Spec::default() });
    assert!(warned(&p, "back shaft"));
}

#[test]
fn print_defaults_plan_for_every_kind_and_size() {
    for k in Kind::ALL.iter().copied() {
        for module in [0.5, 1.0, 2.0, 5.0] {
            let mut s = Spec::defaults_for(k);
            if k == Kind::Hypoid {
                // The offset and face width scale with the ring gear.
                let k = module / s.module;
                s.offset *= k;
                s.face_width *= k;
                s.bore.diameter *= k;
                s.bore2.diameter *= k;
            }
            s.module = module;
            s.apply_print_defaults(0.4, 0.2);
            let p = plan(&s).unwrap_or_else(|e| panic!("{k:?} m{module}: {e}"));
            check_plan(&p).unwrap_or_else(|e| panic!("{k:?} m{module}: {e}"));
        }
    }
    let mut s = Spec::defaults_for(Kind::Pair);
    s.apply_print_defaults(0.4, 0.2);
    assert_eq!(s.rack, crate::involute::BasicRack::standard("print_fdm").unwrap());
    assert_eq!((s.pressure_angle, s.nozzle, s.elephant_foot_height), (25.0, 0.4, 0.2));
    assert!(
        (s.backlash - 0.15).abs() < 1e-12
            && (s.xy_compensation - 0.05).abs() < 1e-12
            && (s.elephant_foot - 0.1).abs() < 1e-12
            && (s.tip_round - 0.1).abs() < 1e-12
    );
    // Bad numbers fall back to the usual nozzle and layer.
    let mut t = Spec::defaults_for(Kind::Pair);
    t.apply_print_defaults(f64::NAN, -1.0);
    assert_eq!(t, s);
    // The nozzle check: fine teeth have tips too thin for a 0.4 mm nozzle.
    let p = planned(&s);
    assert!(!warned(&p, "nozzle"), "{:?}", p.warnings);
    let mut fine_teeth = Spec { module: 0.8, ..Spec::defaults_for(Kind::Pair) };
    fine_teeth.apply_print_defaults(0.4, 0.2);
    let p = planned(&fine_teeth);
    assert!(warned(&p, "two nozzle widths"), "{:?}", p.warnings);
}

#[test]
fn frames_compose() {
    let body = Frame { origin: [10.0, 0.0, 5.0], x: [0.0, 1.0, 0.0], z: [1.0, 0.0, 0.0] };
    let local = Frame { origin: [0.0, 0.0, 2.0], x: [1.0, 0.0, 0.0], z: [0.0, 1.0, 0.0] };
    let both = body.then(&local);
    for q in [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [-4.0, 0.5, 7.0]] {
        let (a, b) = (both.world(q), body.world(local.world(q)));
        assert!((0..3).all(|i| (a[i] - b[i]).abs() < 1e-12), "{a:?} {b:?}");
    }
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    assert!(dot(both.x, both.z).abs() < 1e-12 && (dot(both.x, both.x) - 1.0).abs() < 1e-12);
    assert_eq!(Frame::at([1.0, 2.0, 3.0]).then(&local).origin, [1.0, 2.0, 5.0]);
    let _ = (FRAC_PI_2, PI);
}

/// The extras together, on every kind that takes them, through the kernel-readiness checks.
#[test]
fn everything_at_once_plans() {
    let sh =
        |d: f64, end: ShaftEnd| Shaft { diameter: d, length: 12.0, step_diameter: 0.7 * d, step_length: 8.0, end, chamfer: 0.4, ..Shaft::default() };
    for k in Kind::ALL.iter().copied() {
        let mut s = Spec::defaults_for(k);
        s.apply_print_defaults(0.4, 0.2);
        s.tooth_chamfer_front = Chamfer { size: 0.8, angle: 45.0 };
        s.tooth_chamfer_back = Chamfer { size: 0.5, angle: 30.0 };
        (s.bore_chamfer, s.hub_chamfer) = (0.4, 0.6);
        s.bore = Bore { kind: BoreKind::DFlat, diameter: 3.0, ..Bore::default() };
        s.bore2 = Bore { kind: BoreKind::Round, diameter: 3.0, ..Bore::default() };
        // (A spline's hub diameter is its hub part's outside diameter: left at its default.)
        if k != Kind::Spline {
            (s.hub_diameter, s.hub_length) = (14.0, 8.0);
        }
        s.shaft_back = sh(8.0, ShaftEnd::Hex);
        s.shaft2_front = sh(8.0, ShaftEnd::Keyway);
        s.shaft2_back = sh(7.0, ShaftEnd::DFlat);
        s.hardware = Hardware {
            set_screw: 3.0,
            set_screws: 2,
            insert_diameter: 4.0,
            nut_width: 5.5,
            clamp_slit: 1.0,
            clamp_screw: 2.5,
            ..Hardware::default()
        };
        s.hardware2 = Hardware { bearing_od: 10.0, bearing_width: 4.0, ..Hardware::default() };
        (s.tip_relief, s.backlash2) = (0.02, Some(0.1));
        let p = plan(&s).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        check_plan(&p).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        for w in &p.warnings {
            eprintln!("{k:?}: {w}");
        }
    }
}

/// Sprockets, pulleys, splines, face gears and elliptical pairs: what each takes of the body
/// features, and a note (only) for what it leaves out.
#[test]
fn extras_kinds_take_what_fits_and_say_what_they_leave_out() {
    let shaft = Shaft { diameter: 8.0, length: 10.0, end: ShaftEnd::DFlat, chamfer: 0.5, ..Shaft::default() };
    let screw = Hardware { set_screw: 3.0, ..Hardware::default() };
    let notes = |p: &Plan| p.warnings.iter().filter(|w| w.contains("left out (") || w.contains("don't apply")).count();
    for k in [Kind::Sprocket, Kind::Pulley, Kind::Spline, Kind::FaceGear, Kind::Elliptical] {
        // Nothing set: nothing said.
        let p = planned(&Spec::defaults_for(k));
        assert_eq!(notes(&p), 0, "{k:?}: {:?}", p.warnings);
        // The print preset sets only what the type takes.
        let mut s = Spec::defaults_for(k);
        s.apply_print_defaults(0.4, 0.2);
        let p = planned(&s);
        assert_eq!(notes(&p), 0, "{k:?}: {:?}", p.warnings);
        assert!(s.tip_round == 0.0 && s.xy_compensation == 0.0 && s.elephant_foot == 0.0 && s.nozzle == 0.0, "{k:?}");
        // Tooth tuning is said to be left out.
        let p = planned(&Spec { tip_round: 0.2, crowning: 0.05, ..Spec::defaults_for(k) });
        assert!(p.warnings.iter().any(|w| w.contains("tip rounding, crowning don't apply")), "{k:?}: {:?}", p.warnings);
    }
    // Sprockets and pulleys (both members): shafts, hub hardware and chamfers.
    for k in [Kind::Sprocket, Kind::Pulley] {
        let base = Spec { teeth: 20, teeth2: 30, hub_diameter: 20.0, hub_length: 8.0, ..Spec::defaults_for(k) };
        let plain = planned(&base);
        let s = Spec { shaft_back: shaft, shaft2_back: shaft, hardware2: screw, bore_chamfer: 0.4, hub_chamfer: 0.5, ..base.clone() };
        let p = planned(&s);
        assert_eq!(notes(&p), 0, "{k:?}: {:?}", p.warnings);
        assert_eq!(p.bodies.len(), 2);
        for (a, b) in plain.bodies.iter().zip(&p.bodies) {
            assert!(b.steps.len() > a.steps.len() && extent(b).0 < extent(a).0 - 9.0, "{k:?} {}", b.name);
        }
        // Gear 2's hardware goes on gear 2 only.
        assert!(placed(&p.bodies[0]).is_empty() && !placed(&p.bodies[1]).is_empty(), "{k:?}");
    }
    // Face gear: the pinion takes gear 2's shafts and hardware; the face gear none.
    let base = Spec { hub_diameter: 8.0, hub_length: 4.0, ..Spec::defaults_for(Kind::FaceGear) };
    let plain = planned(&base);
    let p = planned(&Spec { shaft_front: shaft, shaft2_back: Shaft { diameter: 3.0, length: 6.0, ..Shaft::default() }, ..base.clone() });
    assert_eq!(p.bodies[0].steps.len(), plain.bodies[0].steps.len());
    assert!(p.bodies[1].steps.len() > plain.bodies[1].steps.len());
    assert!(warned(&p, "front shaft left out (the face gear itself"), "{:?}", p.warnings);
    // Elliptical pair: hub, shafts and hardware on both, no tooth chamfers.
    let base = Spec {
        hub_diameter: 12.0,
        hub_length: 6.0,
        bore: Bore { kind: BoreKind::Round, diameter: 4.0, ..Bore::default() },
        ..Spec::defaults_for(Kind::Elliptical)
    };
    let plain = planned(&base);
    let p = planned(&Spec {
        shaft_back: shaft,
        hardware: screw,
        tooth_chamfer_front: Chamfer { size: 0.5, angle: 45.0 },
        bore_chamfer: 0.3,
        ..base.clone()
    });
    assert!(p.bodies[0].steps.len() > plain.bodies[0].steps.len() + 2 && !placed(&p.bodies[0]).is_empty());
    assert!(warned(&p, "tooth chamfers left out (the tips are no circle)"), "{:?}", p.warnings);
    assert!(extent(&plain.bodies[0]).1 > Spec::defaults_for(Kind::Elliptical).face_width + 5.0, "the hub is built");
    // Spline: none of it.
    let base = Spec::defaults_for(Kind::Spline);
    let plain = planned(&base);
    let p = planned(&Spec { shaft_front: shaft, hardware2: screw, bore_chamfer: 0.3, ..base.clone() });
    assert_eq!(p.bodies.iter().map(|b| b.steps.len()).collect::<Vec<_>>(), plain.bodies.iter().map(|b| b.steps.len()).collect::<Vec<_>>());
    assert!(
        warned(&p, "front shaft, hub hardware and bearing pocket, bore chamfer left out (the shaft and the hub are the spline itself)"),
        "{:?}",
        p.warnings
    );
}

/// Every type, every option of `plan::left_out`: one said to be left out builds the same bodies
/// and is named in exactly one new warning; one said to be used changes a body, adds a warning
/// of the planner's own or is refused.
#[test]
fn every_type_uses_or_names_every_option() {
    use crate::plan::left_out::{Opt, uses, why_left_out};
    let clear = |k: Kind| Spec {
        bore: Bore::default(),
        bore2: Bore::default(),
        hub_diameter: 0.0,
        hub_length: 0.0,
        holes: 0,
        hole_diameter: 0.0,
        ..Spec::defaults_for(k)
    };
    let bore = Bore { kind: BoreKind::Round, diameter: 2.0, ..Bore::default() };
    let shaft = Shaft { diameter: 3.0, length: 5.0, ..Shaft::default() };
    let screw = Hardware { set_screw: 2.0, ..Hardware::default() };
    let set = |o: Opt, s: &mut Spec| match o {
        Opt::TipRound => s.tip_round = 0.2,
        Opt::XyCompensation => s.xy_compensation = 0.05,
        Opt::ElephantFoot => s.elephant_foot = 0.05,
        Opt::Nozzle => s.nozzle = 2.0,
        Opt::Clearance => s.clearance = 0.3,
        Opt::TipShortening => s.tip_shortening = 0.1,
        Opt::TipDiameter => s.tip_diameter = 1.0,
        Opt::TipDiameter2 => s.tip_diameter2 = 1.0,
        Opt::TipRelief => s.tip_relief = 0.05,
        Opt::Crowning => s.crowning = 0.05,
        Opt::Backlash2 => s.backlash2 = Some(0.05),
        Opt::Face2 => s.face_width2 = 0.8 * s.face_width,
        Opt::Bore => s.bore = bore,
        Opt::HubDiameter => s.hub_diameter = 0.5 * s.module * f64::from(s.teeth.max(8)),
        Opt::HubLength => s.hub_length = 4.0,
        Opt::Holes => s.holes = 3,
        Opt::FrontShaft => s.shaft_front = shaft,
        Opt::BackShaft => s.shaft_back = shaft,
        Opt::Hardware => s.hardware = screw,
        Opt::Bore2 => s.bore2 = bore,
        Opt::FrontShaft2 => s.shaft2_front = shaft,
        Opt::BackShaft2 => s.shaft2_back = shaft,
        Opt::Hardware2 => s.hardware2 = screw,
        Opt::ToothChamfers => s.tooth_chamfer_front = Chamfer { size: 0.3, angle: 45.0 },
        Opt::BoreChamfer => s.bore_chamfer = 0.3,
        Opt::HubChamfer => s.hub_chamfer = 0.3,
    };
    for k in Kind::ALL.iter().copied() {
        for o in Opt::ALL.iter().copied() {
            // What the option needs to show: a hub for the hub's fields and hardware, a bore for
            // its chamfer (where the type takes them).
            let mut base = clear(k);
            let hub = uses(&base, Opt::HubDiameter) && uses(&base, Opt::HubLength);
            match o {
                Opt::HubDiameter if hub => set(Opt::HubLength, &mut base),
                Opt::HubLength if hub => set(Opt::HubDiameter, &mut base),
                Opt::Hardware | Opt::Hardware2 | Opt::HubChamfer if hub => {
                    set(Opt::HubDiameter, &mut base);
                    set(Opt::HubLength, &mut base);
                }
                Opt::BoreChamfer => {
                    // (Both bores: a face gear's is chamfered on the pinion only.)
                    if uses(&base, Opt::Bore) {
                        set(Opt::Bore, &mut base);
                    }
                    if uses(&base, Opt::Bore2) {
                        set(Opt::Bore2, &mut base);
                    }
                }
                _ => {}
            }
            let mut with = base.clone();
            set(o, &mut with);
            assert!(o.is_set(&with), "{k:?} {o:?}");
            let p0 = plan(&base).unwrap_or_else(|e| panic!("{k:?} {o:?} base: {e}"));
            let fresh = |p: &Plan| p.warnings.iter().filter(|w| !p0.warnings.contains(w)).cloned().collect::<Vec<_>>();
            match why_left_out(&with, o) {
                Some(_) => {
                    let p = plan(&with).unwrap_or_else(|e| panic!("{k:?} {o:?}: {e}"));
                    assert_eq!(format!("{:?}", p.bodies), format!("{:?}", p0.bodies), "{k:?} {o:?} is said to be left out but changes the bodies");
                    let new = fresh(&p);
                    assert!(new.len() == 1 && new[0].contains(o.label()) && new[0].contains("left out"), "{k:?} {o:?}: {new:?}");
                }
                None => match plan(&with) {
                    Err(_) => {}
                    Ok(p) => assert!(
                        format!("{:?}", p.bodies) != format!("{:?}", p0.bodies) || !fresh(&p).is_empty(),
                        "{k:?} {o:?} is said to be used but changes nothing and says nothing"
                    ),
                },
            }
        }
    }
}

/// The user's rack: everything a rack can't take is named, in two warnings.
#[test]
fn a_rack_names_what_it_leaves_out() {
    let mut s = Spec {
        bore: Bore { kind: BoreKind::Round, diameter: 6.0, ..Bore::default() },
        hub_diameter: 14.0,
        hub_length: 5.0,
        holes: 3,
        tooth_chamfer_front: Chamfer { size: 0.5, angle: 45.0 },
        tip_round: 0.2,
        crowning: 0.05,
        ..Spec::defaults_for(Kind::Rack)
    };
    s.hardware.set_bearing("608");
    let plain = planned(&Spec::defaults_for(Kind::Rack));
    let p = planned(&s);
    assert_eq!(format!("{:?}", p.bodies), format!("{:?}", plain.bodies));
    assert!(warned(&p, "rack: tip rounding, crowning don't apply to its teeth"), "{:?}", p.warnings);
    assert!(
        warned(
            &p,
            "rack: bore, hub diameter, hub length, lightening holes, hub hardware and bearing pocket, tooth chamfers left out (a rack is a plain toothed bar)"
        ),
        "{:?}",
        p.warnings
    );
    assert_eq!(p.warnings.len(), 2, "{:?}", p.warnings);
    // What a rack does take still reaches it.
    let p = planned(&Spec { xy_compensation: 0.05, ..Spec::defaults_for(Kind::Rack) });
    assert_ne!(format!("{:?}", p.bodies), format!("{:?}", plain.bodies));
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}

/// The print preset sets only what each type uses: it never brings a "left out" warning.
#[test]
fn the_print_preset_sets_nothing_a_type_leaves_out() {
    for k in Kind::ALL.iter().copied() {
        let mut s = Spec::defaults_for(k);
        s.apply_print_defaults(0.4, 0.2);
        let Ok(p) = plan(&s) else { continue };
        assert!(!p.warnings.iter().any(|w| w.contains("left out (") || w.contains("don't apply")), "{k:?}: {:?}", p.warnings);
    }
}
