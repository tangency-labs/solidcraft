use crate::axes::{Axis, Drive, Relation, Solved, candidates, natural, relate, solve};
use crate::plan::{Kind, Spec, plan};

use crate::test_util::close;

fn axis(o: [f64; 3], d: [f64; 3]) -> Axis {
    Axis::new(o, d).unwrap()
}

fn free(ratio: f64) -> Drive {
    Drive { ratio: Some(ratio), module_free: true, teeth_free: true, teeth2_free: true, starts_free: true, ..Drive::default() }
}

fn fact(s: &Solved, label: &str) -> f64 {
    s.facts.iter().find(|f| f.label == label).unwrap_or_else(|| panic!("no fact {label}: {:?}", s.facts)).value
}

fn plan_fact(p: &crate::Plan, label: &str) -> f64 {
    p.facts.iter().find(|f| f.label == label).unwrap_or_else(|| panic!("no fact {label}")).value
}

/// The built bodies turn about the given axes, and the solver's frames say the same.
fn on_axes(s: &Solved, a1: &Axis, a2: &Axis) -> crate::Plan {
    let p = plan(&s.spec).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.bodies.len(), 2, "{:?}", p.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    for (i, (b, a)) in p.bodies.iter().zip([a1, a2]).enumerate() {
        assert!(a.holds(b.frame.origin, b.frame.z, 1e-6), "body {i} {:?} is not on {a:?}", b.frame);
        assert!(a.holds(s.frames[i].origin, s.frames[i].z, 1e-6), "frame {i}");
        for k in 0..3 {
            assert!(
                close(b.frame.origin[k], s.frames[i].origin[k], 1e-6) && close(b.frame.z[k], s.frames[i].z[k], 1e-9),
                "body {i}: {:?} vs {:?}",
                b.frame,
                s.frames[i]
            );
        }
        // A frame is a frame: unit and square.
        let (x, z) = (b.frame.x, b.frame.z);
        assert!(close(x.iter().map(|v| v * v).sum::<f64>(), 1.0, 1e-9) && close(x[0] * z[0] + x[1] * z[1] + x[2] * z[2], 0.0, 1e-9));
    }
    p
}

#[test]
fn axes_are_classified() {
    let z = Axis::Z;
    assert_eq!(relate(&z, &axis([0.0, 0.0, 5.0], [0.0, 0.0, -2.0])), Relation::Coincident);
    match relate(&z, &axis([30.0, 40.0, 7.0], [0.0, 0.0, 1.0])) {
        Relation::Parallel { distance, foot1, foot2 } => {
            assert!(close(distance, 50.0, 1e-12));
            assert_eq!(foot1, [0.0; 3]);
            assert!(close(foot2[0], 30.0, 1e-12) && close(foot2[1], 40.0, 1e-12) && close(foot2[2], 0.0, 1e-12));
        }
        r => panic!("{r:?}"),
    }
    match relate(&z, &axis([10.0, 0.0, 20.0], [1.0, 0.0, 0.0])) {
        Relation::Intersecting { angle, apex } => {
            assert!(close(angle, 90.0, 1e-9));
            assert!(close(apex[0], 0.0, 1e-9) && close(apex[2], 20.0, 1e-9));
        }
        r => panic!("{r:?}"),
    }
    match relate(&z, &axis([25.0, 3.0, 8.0], [0.0, 60f64.to_radians().sin(), 60f64.to_radians().cos()])) {
        Relation::Skew { angle, offset, foot1, foot2 } => {
            assert!(close(angle, 60.0, 1e-9) && close(offset, 25.0, 1e-9));
            assert!(close(foot1[0], 0.0, 1e-9) && close(foot1[1], 0.0, 1e-9));
            assert!(close(foot2[0], 25.0, 1e-9) && close(foot2[1], 0.0, 1e-9) && close(foot1[2], foot2[2], 1e-9));
        }
        r => panic!("{r:?}"),
    }
    assert!(Axis::new([0.0; 3], [0.0; 3]).is_none());
    assert_eq!(relate(&z, &axis([40.0, 0.0, 0.0], [0.0, 0.0, 1.0])).describe(), "parallel, 40 mm apart");
}

#[test]
fn candidates_say_what_fits_and_why_not() {
    let fits = |rel: &Relation| -> Vec<Kind> { candidates(Some(rel)).into_iter().filter(|c| c.fits && c.kind.is_set()).map(|c| c.kind).collect() };
    let par = relate(&Axis::Z, &axis([40.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    assert_eq!(fits(&par), [Kind::Pair, Kind::Cycloidal, Kind::Sprocket, Kind::Pulley, Kind::Elliptical]);
    assert_eq!(natural(Some(&par)), Kind::Pair);
    assert_eq!(fits(&Relation::Coincident), [Kind::Planetary, Kind::CycloidalDrive, Kind::Spline]);
    assert_eq!(natural(Some(&Relation::Coincident)), Kind::Planetary);
    let meet = relate(&Axis::Z, &axis([0.0; 3], [1.0, 0.0, 0.0]));
    assert_eq!(fits(&meet), [Kind::Bevel, Kind::SpiralBevel, Kind::FaceGear, Kind::BevelPlanetary]);
    assert_eq!(natural(Some(&meet)), Kind::Bevel);
    let skew90 = relate(&Axis::Z, &axis([25.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
    assert_eq!(fits(&skew90), [Kind::Hypoid, Kind::CrossedHelical, Kind::Worm]);
    assert_eq!(natural(Some(&skew90)), Kind::Worm);
    let skew60 = relate(&Axis::Z, &axis([25.0, 0.0, 0.0], [0.0, 0.866, 0.5]));
    assert_eq!(fits(&skew60), [Kind::Hypoid, Kind::CrossedHelical]);
    assert_eq!(natural(Some(&skew60)), Kind::CrossedHelical);
    let why = |rel: &Relation, k: Kind| candidates(Some(rel)).into_iter().find(|c| c.kind == k).unwrap().reason;
    assert!(why(&skew90, Kind::Bevel).contains("skew") && why(&skew90, Kind::Bevel).contains("intersecting"));
    assert!(why(&skew60, Kind::Worm).contains("90°"));
    assert!(why(&meet, Kind::Pair).contains("parallel"));
    // Single gears always fit: they use axis 1.
    assert!(candidates(Some(&skew90)).iter().filter(|c| !c.kind.is_set()).all(|c| c.fits));
    assert_eq!(natural(None), Kind::Spur);
    // Refusing a family that doesn't fit says why.
    let e = solve(&Axis::Z, Some(&axis([25.0, 0.0, 0.0], [0.0, 1.0, 0.0])), Some(Kind::Bevel), &Spec::default(), &Drive::default()).unwrap_err();
    assert!(e.contains("skew"), "{e}");
}

#[test]
fn a_parallel_pair_takes_a_standard_module_and_shifts_to_the_distance() {
    let a1 = axis([10.0, 20.0, 30.0], [0.0, 1.0, 0.0]);
    let a2 = axis([10.0, -50.0, 70.0], [0.0, -1.0, 0.0]);
    // 40 mm apart at 1.5: module 2 with 16 + 24 teeth fits exactly.
    let s = solve(&a1, Some(&a2), None, &Spec::defaults_for(Kind::Pair), &free(1.5)).unwrap();
    assert_eq!((s.spec.kind, s.spec.module, s.spec.teeth, s.spec.teeth2), (Kind::Pair, 2.0, 16, 24), "{:?}", s.notes);
    assert!(s.derived.module && s.derived.teeth && s.derived.teeth2 && s.derived.center_distance);
    let p = on_axes(&s, &a1, &a2);
    assert!(close(plan_fact(&p, "centre distance"), 40.0, 1e-9));
    // Gear 1 sits at axis 1's origin.
    assert!(close(p.bodies[0].frame.origin[1], 20.0, 1e-9));

    // 41.3 mm apart: the same gears, shifted to reach it.
    let a2 = axis([10.0, 0.0, 71.3], [0.0, 1.0, 0.0]);
    let s = solve(&a1, Some(&a2), None, &Spec::defaults_for(Kind::Pair), &free(1.5)).unwrap();
    let p = on_axes(&s, &a1, &a2);
    assert!(close(plan_fact(&p, "centre distance"), 41.3, 1e-6), "{}", plan_fact(&p, "centre distance"));
    assert!(s.spec.shift > 0.0 && close(s.spec.shift + s.spec.shift2, fact(&s, "profile shift sum"), 1e-9));
    assert!(MODULE_SET.contains(&s.spec.module), "{}", s.spec.module);
    assert!(close(f64::from(s.spec.teeth2) / f64::from(s.spec.teeth), 1.5, 0.08));

    // A fixed module and gear 1: gear 2's teeth follow from the distance.
    let base = Spec { module: 2.5, teeth: 18, ..Spec::defaults_for(Kind::Pair) };
    let s = solve(&a1, Some(&a2), Some(Kind::Pair), &base, &Drive { teeth2_free: true, ..Drive::default() }).unwrap();
    assert_eq!((s.spec.module, s.spec.teeth, s.spec.teeth2), (2.5, 18, 15));
    assert!(close(plan_fact(&on_axes(&s, &a1, &a2), "centre distance"), 41.3, 1e-6));

    // Everything fixed and far off: refused with the numbers.
    let base = Spec { module: 2.0, teeth: 20, teeth2: 30, ..Spec::defaults_for(Kind::Pair) };
    let e = solve(&a1, Some(&a2), Some(Kind::Pair), &base, &Drive::default()).unwrap_err();
    assert!(e.contains("50") && e.contains("41.3"), "{e}");
    // Fixed teeth, free module: the exact module when no standard one is near.
    let s = solve(&a1, Some(&a2), Some(Kind::Pair), &base, &Drive { module_free: true, ..Drive::default() }).unwrap();
    assert!(close(plan_fact(&on_axes(&s, &a1, &a2), "centre distance"), 41.3, 1e-6));
}

const MODULE_SET: [f64; 42] = {
    let mut all = [0.0; 42];
    let mut i = 0;
    while i < 25 {
        all[i] = crate::axes::MODULES[i];
        i += 1;
    }
    while i < 42 {
        all[i] = crate::axes::MODULES_2[i - 25];
        i += 1;
    }
    all
};

#[test]
fn helical_pairs_and_cycloidal_gears_reach_the_distance() {
    let a1 = Axis::Z;
    let a2 = axis([0.0, 63.0, 0.0], [0.0, 0.0, 1.0]);
    let base = Spec { helix: 20.0, ..Spec::defaults_for(Kind::Pair) };
    let s = solve(&a1, Some(&a2), None, &base, &free(2.0)).unwrap();
    let p = on_axes(&s, &a1, &a2);
    assert!(close(plan_fact(&p, "centre distance"), 63.0, 1e-6));
    assert!(close(f64::from(s.spec.teeth2) / f64::from(s.spec.teeth), 2.0, 0.1), "{:?}", s.notes);
    let s = solve(&a1, Some(&a2), Some(Kind::Cycloidal), &Spec::defaults_for(Kind::Cycloidal), &free(7.5)).unwrap();
    let p = on_axes(&s, &a1, &a2);
    assert!(close(plan_fact(&p, "centre distance"), 63.0, 1e-9));
    assert!(close(plan_fact(&p, "ratio"), 7.5, 1e-9));
}

#[test]
fn bevels_meet_at_the_apex() {
    // (120° at ratio 2 would need a 90° crown gear, which the builder refuses: ratio 1 there.)
    for (deg, o2, ratio) in [(90.0f64, [0.0, 0.0, 12.0], 2u32), (60.0, [0.0, 0.0, 12.0], 2), (120.0, [0.0, 0.0, 12.0], 1)] {
        let a1 = axis([0.0, 0.0, -40.0], [0.0, 0.0, 1.0]);
        let r = deg.to_radians();
        // Axis 2 leaves the apex (0, 0, 12) at `deg` from the ray gear 1 sits on (−z).
        let side2 = [r.sin() * 0.6, r.sin() * 0.8, -r.cos()];
        let a2 = axis([o2[0] + 30.0 * side2[0], o2[1] + 30.0 * side2[1], o2[2] + 30.0 * side2[2]], side2);
        for kind in [Kind::Bevel, Kind::SpiralBevel] {
            let s = solve(
                &a1,
                Some(&a2),
                Some(kind),
                &Spec::defaults_for(kind),
                &Drive { ratio: Some(f64::from(ratio)), teeth_free: true, teeth2_free: true, ..Drive::default() },
            )
            .unwrap();
            assert!(close(s.spec.shaft_angle, deg, 1e-6), "{deg}: {}", s.spec.shaft_angle);
            assert_eq!(s.spec.teeth2, ratio * s.spec.teeth);
            let p = on_axes(&s, &a1, &a2);
            // Each gear's large end is the cone distance from the apex, on its own side.
            let re = fact(&s, "outer cone distance");
            let d1 = plan_fact(&p, "gear 1 pitch cone angle").to_radians();
            let o = p.bodies[0].frame.origin;
            assert!(close(12.0 - o[2], re * d1.cos(), 1e-6) && o[2] < 12.0, "{deg}");
            let o = p.bodies[1].frame.origin;
            let from_apex = [o[0], o[1], o[2] - 12.0];
            assert!(from_apex[0] * side2[0] + from_apex[1] * side2[1] + from_apex[2] * side2[2] > 0.0, "{deg}: gear 2 is on the wrong side");
        }
    }
    // The cone distance drives the module.
    let (a1, a2) = (Axis::Z, axis([0.0; 3], [1.0, 0.0, 0.0]));
    let s = solve(&a1, Some(&a2), None, &Spec::defaults_for(Kind::Bevel), &Drive { cone_distance: Some(50.0), ..Drive::default() }).unwrap();
    let p = on_axes(&s, &a1, &a2);
    assert!(close(plan_fact(&p, "gear 1 cone distance"), 50.0, 1e-9));
    // Axes through the apex put the gears along their directions; flipping moves one across.
    assert!(p.bodies[0].frame.origin[2] > 0.0 && p.bodies[1].frame.origin[0] > 0.0);
    let s = solve(&a1, Some(&a2), None, &Spec::defaults_for(Kind::Bevel), &Drive { flip2: true, ..Drive::default() }).unwrap();
    assert!(on_axes(&s, &a1, &a2).bodies[1].frame.origin[0] < 0.0);
}

#[test]
fn skew_sets_span_the_offset() {
    let a1 = axis([5.0, 5.0, 5.0], [0.0, 0.0, 1.0]);
    for deg in [90.0f64, 60.0, 110.0] {
        let r = deg.to_radians();
        let a2 = axis([5.0 + 25.0 * 0.6, 5.0 + 25.0 * 0.8, 40.0], [-0.8 * r.sin(), 0.6 * r.sin(), r.cos()]);
        match relate(&a1, &a2) {
            Relation::Skew { offset, .. } => assert!(close(offset, 25.0, 1e-9)),
            x => panic!("{x:?}"),
        }
        let s = solve(&a1, Some(&a2), Some(Kind::CrossedHelical), &Spec::defaults_for(Kind::CrossedHelical), &free(2.0)).unwrap();
        let p = on_axes(&s, &a1, &a2);
        assert!(close(plan_fact(&p, "centre distance"), 25.0, 1e-6), "{deg}: {}", plan_fact(&p, "centre distance"));
        assert!(MODULE_SET.contains(&s.spec.module) && s.derived.helix);
        assert!(close(s.spec.shaft_angle, deg, 1e-6) || close(s.spec.shaft_angle, 180.0 - deg, 1e-6));
        // A spiral hypoid at 3:1 fits the square and the 60° crossing (by the spiral hand the
        // axes call for); at 110° its ring gear's pitch cone would pass 90°, which it refuses.
        let hyp = solve(&a1, Some(&a2), Some(Kind::Hypoid), &Spec::defaults_for(Kind::Hypoid), &free(3.0));
        if deg > 100.0 {
            let e = hyp.unwrap_err();
            assert!(e.contains("pitch cone"), "{deg}: {e}");
            continue;
        }
        let s = hyp.unwrap_or_else(|e| panic!("{deg}: {e}"));
        let p = on_axes(&s, &a1, &a2);
        assert!(close(plan_fact(&p, "offset"), 25.0, 1e-6), "{deg}: {}", plan_fact(&p, "offset"));
        assert!(p.warnings.iter().all(|w| !w.contains("offset")), "{:?}", p.warnings);
    }
    // A fixed module too coarse for the offset is refused with the largest that works.
    let a2 = axis([30.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let base = Spec { module: 4.0, ..Spec::defaults_for(Kind::CrossedHelical) };
    let e = solve(&a1, Some(&a2), Some(Kind::CrossedHelical), &base, &Drive::default()).unwrap_err();
    assert!(e.contains("at least") && e.contains("or less"), "{e}");
}

#[test]
fn a_worm_fills_what_the_wheel_leaves() {
    let a1 = axis([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let a2 = axis([7.0, 40.0, 0.0], [0.0, 0.0, 1.0]);
    let s = solve(&a1, Some(&a2), None, &Spec::defaults_for(Kind::Worm), &free(30.0)).unwrap();
    assert_eq!((s.spec.kind, s.spec.teeth, s.spec.starts), (Kind::Worm, 30, 1));
    let p = on_axes(&s, &a1, &a2);
    assert!(close(plan_fact(&p, "centre distance"), 40.0, 1e-9));
    assert!(close(plan_fact(&p, "ratio"), 30.0, 1e-9));
    assert!(close(s.spec.worm_diameter + s.spec.module * 30.0, 80.0, 1e-9));
    assert!((6.0..=20.0).contains(&(s.spec.worm_diameter / s.spec.module)));
    // A low ratio takes more starts.
    let s = solve(&a1, Some(&a2), None, &Spec::defaults_for(Kind::Worm), &free(8.0)).unwrap();
    assert_eq!((s.spec.starts, s.spec.teeth), (3, 24));
    // The worm on axis 1 instead.
    let s = solve(&a1, Some(&a2), None, &Spec::defaults_for(Kind::Worm), &Drive { swap: true, ..free(30.0) }).unwrap();
    on_axes(&s, &a2, &a1);
    // Too coarse a module leaves no worm.
    let base = Spec { module: 3.0, teeth: 30, ..Spec::defaults_for(Kind::Worm) };
    assert!(solve(&a1, Some(&a2), Some(Kind::Worm), &base, &Drive::default()).unwrap_err().contains("smaller module"));
}

#[test]
fn single_gears_and_coaxial_sets_go_on_axis_1() {
    let a1 = axis([3.0, 4.0, 5.0], [1.0, 1.0, 0.0]);
    for kind in [Kind::Spur, Kind::Helical, Kind::Internal, Kind::Planetary, Kind::CycloidalDrive] {
        let s = solve(&a1, Some(&a1), Some(kind), &Spec::defaults_for(kind), &Drive { along: 10.0, ..Drive::default() }).unwrap();
        let p = plan(&s.spec).unwrap();
        // Every member's axis is parallel to axis 1; the first is on it, 10 along.
        assert!(p.bodies.iter().all(|b| a1.holds(a1.origin, b.frame.z, 1e-9)));
        let ring = p.bodies.iter().find(|b| a1.distance(b.frame.origin) < 1e-9).unwrap();
        let along: f64 = (0..3).map(|k| (ring.frame.origin[k] - a1.origin[k]) * a1.dir[k]).sum();
        assert!(close(along, 10.0, 1e-9), "{kind:?}");
    }
    // Only one axis: a pair still builds, gear 1 on it.
    let s = solve(&a1, None, Some(Kind::Pair), &Spec::defaults_for(Kind::Pair), &Drive::default()).unwrap();
    let p = plan(&s.spec).unwrap();
    assert!(a1.holds(p.bodies[0].frame.origin, p.bodies[0].frame.z, 1e-9));
    // World Z at the origin is the world frame: nothing moves.
    let s = solve(&Axis::Z, None, Some(Kind::Spur), &Spec::default(), &Drive::default()).unwrap();
    assert_eq!(plan(&s.spec).unwrap().bodies, plan(&Spec::default()).unwrap().bodies);
}

#[test]
fn no_placement_leaves_every_set_where_it_was() {
    for k in Kind::ALL.iter().copied() {
        let s = Spec::defaults_for(k);
        assert!(s.placement.is_none());
        let at = Spec { placement: Some(crate::Frame::at([0.0; 3])), ..s.clone() };
        assert_eq!(plan(&s).unwrap().bodies, plan(&at).unwrap().bodies, "{k:?}");
    }
}

/// Chain and belt drives, elliptical pairs and face gears are placed on their axes too.
#[test]
fn extras_are_placed_on_their_axes() {
    let par = axis([60.0, 0.0, 5.0], [0.0, 0.0, 1.0]);
    for kind in [Kind::Sprocket, Kind::Pulley] {
        let s = solve(
            &Axis::Z,
            Some(&par),
            Some(kind),
            &Spec { teeth: 16, ..Spec::default() },
            &Drive { ratio: Some(2.0), teeth2_free: true, ..Drive::default() },
        )
        .unwrap();
        assert_eq!((s.spec.teeth, s.spec.teeth2), (16, 32), "{kind:?}");
        assert!(close(s.spec.center_distance, 60.0, 1e-9));
        assert!(close(s.frames[1].origin[0], 60.0, 1e-9));
        assert!(plan(&s.spec).unwrap().bodies.len() == 2, "{kind:?}");
    }
    let s = solve(&Axis::Z, Some(&par), Some(Kind::Elliptical), &Spec { teeth: 20, ..Spec::default() }, &Drive::default()).unwrap();
    let p = plan(&s.spec).unwrap();
    let a = p.facts.iter().find(|f| f.label == "centre distance").unwrap().value;
    assert!(close(a, 60.0, 1e-6), "{a}");
    // A face gear: axis 1 its own, axis 2 the pinion's, meeting at 90°.
    let meet = axis([0.0, 0.0, 30.0], [1.0, 0.0, 0.0]);
    let s = solve(&Axis::Z, Some(&meet), Some(Kind::FaceGear), &Spec { module: 1.0, teeth: 12, teeth2: 40, ..Spec::default() }, &Drive::default())
        .unwrap();
    let pl = s.spec.placement.unwrap();
    assert!(close(pl.origin[2], 30.0 - 6.0, 1e-9), "{pl:?}");
    assert!(close(s.frames[1].origin[2], 30.0, 1e-9) && close(s.frames[1].z[0].abs(), 1.0, 1e-9), "{:?}", s.frames[1]);
    assert!(solve(&Axis::Z, Some(&axis([0.0, 0.0, 30.0], [0.6, 0.0, 0.8])), Some(Kind::FaceGear), &Spec::default(), &Drive::default()).is_err());
    // A spline is coaxial: one axis.
    assert!(solve(&Axis::Z, Some(&par), Some(Kind::Spline), &Spec::default(), &Drive::default()).is_err());
    assert!(solve(&Axis::Z, None, Some(Kind::Spline), &Spec::default(), &Drive::default()).is_ok());
}

/// An 11-tooth pinion across a drawn distance takes the shift that clears its undercut, its mate
/// the rest (the distance held), when the mate keeps its own teeth whole: no undercut warning.
#[test]
fn the_shift_goes_to_the_pinion_that_would_be_undercut() {
    let a1 = axis([0.0; 3], [0.0, 0.0, 1.0]);
    for a in [25.5, 25.9] {
        let a2 = axis([a, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let base = Spec { module: 1.0, teeth: 11, teeth2: 40, ..Spec::defaults_for(Kind::Pair) };
        let s = solve(&a1, Some(&a2), Some(Kind::Pair), &base, &Drive::default()).unwrap();
        let (g1, g2) = (s.spec.involute(11, s.spec.shift, 0.0), s.spec.involute2(40, s.spec.shift2, 0.0));
        assert!(!g1.undercut() && !g2.undercut(), "{a}: shifts {} and {}", s.spec.shift, s.spec.shift2);
        let p = on_axes(&s, &a1, &a2);
        assert!(close(plan_fact(&p, "centre distance"), a, 1e-6), "{a}: {}", plan_fact(&p, "centre distance"));
        assert!(
            p.warnings.iter().all(|w| !w.contains("undercut") && !w.contains("thin tooth tips") && !w.contains("point")),
            "{a}: {:?}",
            p.warnings
        );
    }
    // A train's meshing group the same: the 11-tooth gear between two others.
    let g = |z: u32| Spec { module: 1.0, ..Spec::defaults_for(Kind::Pair) }.involute(z, 0.0, 0.0);
    let gears = [g(30), g(11), g(25)];
    let meshes = [
        crate::sketch_rules::GroupMesh { a: 0, b: 1, internal: false, dist: 20.5 },
        crate::sketch_rules::GroupMesh { a: 1, b: 2, internal: false, dist: 18.0 },
    ];
    let xs = crate::sketch_rules::group_shifts(&gears, &[false; 3], &meshes).unwrap();
    for (k, x) in xs.iter().enumerate() {
        assert!(!crate::involute::Involute { shift: *x, ..gears[k] }.undercut(), "gear {k}: shifts {xs:?}");
    }
    assert!(close(xs[0] + xs[1], 0.0, 1e-9) && close(xs[1] + xs[2], 0.0, 1e-9), "{xs:?}");
}

/// Where no split of the shift keeps both gears clear, the undercut warning says what to change
/// (the planner picks the shift to hold the distance, so it is not the user's to raise).
#[test]
fn an_unavoidable_undercut_says_what_to_change() {
    let a1 = axis([0.0; 3], [0.0, 0.0, 1.0]);
    let a2 = axis([10.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let base = Spec { module: 1.0, teeth: 10, teeth2: 10, ..Spec::defaults_for(Kind::Pair) };
    let s = solve(&a1, Some(&a2), Some(Kind::Pair), &base, &Drive::default()).unwrap();
    let p = plan(&s.spec).unwrap();
    let w: Vec<&String> = p.warnings.iter().filter(|w| w.contains("undercut")).collect();
    assert!(!w.is_empty(), "{:?}", p.warnings);
    assert!(w.iter().all(|w| w.contains("more teeth") && w.contains("pressure angle") && !w.contains("raise the profile shift")), "{w:?}");
}

/// A hypoid whose teeth twist further between sections than the kernel's matching followed is
/// built at the face width asked (the engine lofts the sections as given, piece for piece).
#[test]
fn a_wide_hypoid_keeps_the_face_width_asked() {
    let base = Spec::defaults_for(Kind::Hypoid);
    let mut cut = None;
    for k in [1.5, 2.0] {
        let s = Spec { face_width: base.face_width * k, ..base.clone() };
        let p = plan(&s).unwrap_or_else(|e| panic!("×{k}: {e}"));
        if p.warnings.iter().any(|w| w.contains("face width cut")) {
            cut = Some((k, p.warnings.clone()));
            break;
        }
    }
    assert!(cut.is_none(), "{cut:?}");
}
