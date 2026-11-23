//! The assembly a plan describes: members on their shafts and the links between them, whose
//! ratios must be the ones the mesh checkers and the 2D preview turn the set by.

use crate::clearance::Clearance;
use crate::plan::{AssemblyLink, BevelPlanetMode, Frame, Kind, Plan, Spec, plan};

use crate::test_util::close_rel as close;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Every type's defaults, and the other shapes some types take.
fn variants() -> Vec<(String, Spec)> {
    let mut v: Vec<(String, Spec)> = Kind::ALL.iter().map(|k| (k.name().to_string(), Spec::defaults_for(*k))).collect();
    let bp = |mode, sigma, zs, zp, n| Spec {
        bevel_planet_mode: mode,
        shaft_angle: sigma,
        teeth: zs,
        teeth2: zp,
        planets: n,
        ..Spec::defaults_for(Kind::BevelPlanetary)
    };
    v.push(("sprocket pair".into(), Spec { teeth2: 34, ..Spec::defaults_for(Kind::Sprocket) }));
    v.push(("pulley pair".into(), Spec { teeth2: 40, ..Spec::defaults_for(Kind::Pulley) }));
    v.push(("helical pair".into(), Spec { helix: 15.0, ..Spec::defaults_for(Kind::Pair) }));
    v.push((
        "planetary of 4 with a ring of 60".into(),
        Spec { planets: 4, teeth: 20, teeth2: 20, teeth3: 60, ..Spec::defaults_for(Kind::Planetary) },
    ));
    v.push(("bevel at 70°".into(), Spec { shaft_angle: 70.0, ..Spec::defaults_for(Kind::Bevel) }));
    v.push(("bevel planetary single".into(), Spec { bevel_planet_mode: BevelPlanetMode::Single, ..Spec::defaults_for(Kind::BevelPlanetary) }));
    v.push(("bevel planetary ring".into(), bp(BevelPlanetMode::Ring, 45.0, 20, 10, 3)));
    v.push(("left-hand worm".into(), Spec { helix: -1.0, ..Spec::defaults_for(Kind::Worm) }));
    v.push(("2-start worm".into(), Spec { starts: 2, ..Spec::defaults_for(Kind::Worm) }));
    v.push((
        "placed pair".into(),
        Spec { placement: Some(Frame { origin: [10.0, 20.0, 30.0], x: [0.0, 1.0, 0.0], z: [1.0, 0.0, 0.0] }), ..Spec::defaults_for(Kind::Pair) },
    ));
    v
}

fn planned(label: &str, s: &Spec) -> Plan {
    plan(s).unwrap_or_else(|e| panic!("{label}: {e}"))
}

/// +1 when the member turns about its body's frame z, -1 when about the reverse.
fn sense(p: &Plan, i: usize) -> f64 {
    let m = &p.assembly.members[i];
    dot(m.axis.dir, p.bodies[m.body].frame.z).signum()
}

#[test]
fn every_set_is_an_assembly_of_its_bodies() {
    for (label, s) in variants() {
        let p = planned(&label, &s);
        let a = &p.assembly;
        assert_eq!(a.members.len(), p.bodies.len(), "{label}: one member per body");
        let mut driven = vec![false; a.members.len()];
        for (i, m) in a.members.iter().enumerate() {
            assert_eq!(m.body, i, "{label}");
            assert!(close(dot(m.axis.dir, m.axis.dir), 1.0), "{label}: member {i}'s axis isn't a unit direction");
            assert!(m.carrier.is_none_or(|c| c < a.members.len() && c != i), "{label}: member {i}'s carrier");
        }
        for l in &a.links {
            let (x, y) = l.members();
            assert!(x < driven.len() && y < driven.len() && x != y, "{label}: {l:?}");
            assert!(!driven[y], "{label}: member {y} is driven twice");
            driven[y] = true;
            assert!(l.ratio().is_finite() && l.ratio() != 0.0, "{label}: {l:?}");
            if let AssemblyLink::Gear { ra, rb, .. } = *l {
                assert!(ra > 0.0 && rb > 0.0, "{label}: {l:?}");
            }
        }
        // A set of two or more meshing members is linked into one train.
        // (A differential has two degrees of freedom: its case is driven and side gear A is free,
        // the pinions and side gear B following it relative to the case.)
        if s.kind.is_set() && s.kind != Kind::CycloidalDrive && s.kind != Kind::Differential && p.bodies.len() >= 2 {
            // (A carrier is driven on its own: the planets ride on it.)
            for i in (1..a.members.len()).filter(|i| !a.members.iter().any(|m| m.carrier == Some(*i))) {
                assert!(a.ratio(0, i).is_some(), "{label}: member {i} is not linked to member 1");
            }
        }
    }
}

/// The links turn the set as `mesh3d` checks it (sets whose axes aren't parallel) and as the 2D
/// preview animates it (parallel ones).
#[test]
fn links_turn_the_set_by_the_ratios_the_mesh_checkers_use() {
    let mut checked = 0;
    for (label, s) in variants() {
        let p = planned(&label, &s);
        let a = &p.assembly;
        // The 3D meshes: each pair's ratio about the bodies' frames.
        for m in &p.meshes3d {
            let index = |name: &str| p.bodies.iter().position(|b| b.name == name).unwrap_or_else(|| panic!("{label}: no body {name}"));
            let (i, j) = match &m.bodies {
                Some([x, y]) => (index(&x.name), index(&y.name)),
                None => (0, 1),
            };
            let got = a.ratio(i, j).unwrap_or_else(|| panic!("{label}: members {i} and {j} aren't linked")) * sense(&p, i) * sense(&p, j);
            assert!(close(got, m.ratio), "{label}: {} turns {got} per turn of {}, mesh3d says {}", p.bodies[j].name, p.bodies[i].name, m.ratio);
            checked += 1;
        }
        if !p.meshes3d.is_empty() {
            continue;
        }
        // The 2D preview: each turning gear's speed against the first's, matched to its body by
        // centre (and by name where two share one).
        let turning: Vec<_> = p.preview.iter().filter(|g| g.speed != 0.0 && g.slide == 0.0 && g.orbit == 0.0).collect();
        let speed_of = |i: usize| -> Option<f64> {
            let b = &p.bodies[i];
            let o = b.frame.origin;
            let at: Vec<_> = turning.iter().filter(|g| (g.center.x - o[0]).abs() < 1e-6 && (g.center.y - o[1]).abs() < 1e-6).collect();
            let named: Vec<_> = at.iter().filter(|g| b.name.to_lowercase().starts_with(&g.name.to_lowercase())).collect();
            match (at.as_slice(), named.as_slice()) {
                (_, [g]) => Some(g.speed),
                ([g, rest @ ..], _) if rest.iter().all(|h| h.speed == g.speed) => Some(g.speed),
                _ => None,
            }
        };
        let parallel = p.bodies.iter().all(|b| close(b.frame.z[2], 1.0)) && s.placement.is_none();
        if !parallel || p.bodies.len() < 2 || s.kind == Kind::Elliptical {
            continue;
        }
        let Some(first) = speed_of(0) else { continue };
        for j in 1..p.bodies.len() {
            let Some(sj) = speed_of(j) else { continue };
            let got = a.ratio(0, j).unwrap_or_else(|| panic!("{label}: member {j} isn't linked")) * sense(&p, 0) * sense(&p, j);
            assert!(
                close(got, sj / first),
                "{label}: {} turns {got} per turn of {}, the preview turns it {}",
                p.bodies[j].name,
                p.bodies[0].name,
                sj / first
            );
            checked += 1;
        }
    }
    assert!(checked >= 30, "only {checked} ratios checked");
}

#[test]
fn special_members_are_held_slid_or_turned_round() {
    // A rack slides along its length.
    let p = planned("rack", &Spec::defaults_for(Kind::Rack));
    assert_eq!(p.assembly.members[0].axis.dir, p.bodies[0].frame.x);
    // A cycloidal drive's pin ring is held; its disc turns.
    let p = planned("cycloidal drive", &Spec::defaults_for(Kind::CycloidalDrive));
    assert_eq!(p.assembly.members.iter().map(|m| m.grounded).collect::<Vec<_>>(), vec![false, true]);
    // A face gear turns the other way from its pinion's frame turn: the pinion's contact runs
    // below its axis at +x, so the face gear turns +z_p/z_f per turn about the pinion's frame z.
    let s = Spec::defaults_for(Kind::FaceGear);
    let p = planned("face gear", &s);
    let got = p.assembly.ratio(1, 0).unwrap() * sense(&p, 1) * sense(&p, 0);
    assert!(close(got, f64::from(s.teeth) / f64::from(s.teeth2)), "{got}");
    // An elliptical pair is linked at its mean ratio.
    let p = planned("elliptical", &Spec::defaults_for(Kind::Elliptical));
    assert!(close(p.assembly.ratio(0, 1).unwrap(), -1.0));
    // A spline's shaft and hub turn as one.
    let p = planned("spline", &Spec::defaults_for(Kind::Spline));
    assert_eq!(p.assembly.links, vec![AssemblyLink::Rigid { a: 0, b: 1 }]);
    // A chain turns both sprockets the same way, by their teeth.
    let p = planned("sprockets", &Spec { teeth2: 34, ..Spec::defaults_for(Kind::Sprocket) });
    assert!(close(p.assembly.ratio(0, 1).unwrap(), 0.5));
    // A worm wheel turns starts/teeth per worm turn, either hand.
    for helix in [1.0, -1.0] {
        let s = Spec { helix, starts: 2, ..Spec::defaults_for(Kind::Worm) };
        let p = planned("worm", &s);
        assert!(close(p.assembly.ratio(1, 0).unwrap(), -2.0 / 30.0), "{helix}");
    }
    // A planetary ring turns -zs/zr with the carrier held.
    let p = planned("planetary", &Spec::defaults_for(Kind::Planetary));
    let n = p.bodies.len();
    assert!(close(p.assembly.ratio(0, n - 1).unwrap(), -18.0 / 54.0));
    // A member built alone is linked to nothing.
    let p = planned("gear 2 of a pair", &Spec { only: 2, ..Spec::defaults_for(Kind::Pair) });
    assert_eq!((p.assembly.members.len(), p.assembly.links.len()), (1, 0));
}

/// The empty space every set needs can be made for every set in its assembly: the clearance
/// body, which the engine adds as a grounded keep-out component.
#[test]
fn every_set_can_have_its_keep_out_body() {
    for (label, s) in variants() {
        let travel = if s.kind == Kind::Rack { 60.0 } else { 0.0 };
        let s = Spec { clearance_body: Some(Clearance { travel, ..Clearance::default() }), ..s };
        let p = planned(&label, &s);
        let c = p.clearance.as_ref().unwrap_or_else(|| panic!("{label}: no keep-out body"));
        assert!(!c.steps.is_empty(), "{label}");
        assert_eq!(p.assembly.members.len(), p.bodies.len(), "{label}: the keep-out body is not a member");
    }
}
