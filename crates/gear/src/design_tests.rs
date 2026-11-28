use std::time::Instant;

use super::*;
use crate::mesh2d::{Verdict, check_mesh, worst};
use crate::plan::{Plan, plan};
use crate::strength::Material;

fn fact(p: &Plan, label: &str) -> f64 {
    p.facts.iter().find(|f| f.label == label).unwrap_or_else(|| panic!("no fact {label}")).value
}

fn room(s: &Suggestion) -> f64 {
    s.center_distance + (s.outer_diameter + s.outer_diameter2) / 2.0
}

fn standard(m: f64) -> bool {
    FINE_MODULES.iter().chain(&ISO54_SERIES_1).chain(&ISO54_SERIES_2).any(|v| *v == m)
}

/// A suggestion planned as a pair: the plan's numbers are the suggestion's, the teeth are
/// sound and the pair meshes.
fn proves(goal: &Goal, s: &Suggestion) -> Plan {
    let p = plan(&s.spec(goal)).unwrap_or_else(|e| panic!("{s:?}: {e}"));
    assert!((fact(&p, "centre distance") - s.center_distance).abs() < 1e-9, "{s:?}");
    assert!((fact(&p, "ratio") - s.ratio).abs() < 1e-12);
    assert!((fact(&p, "gear 1 tip diameter") - s.outer_diameter).abs() < 1e-6, "{} {s:?}", fact(&p, "gear 1 tip diameter"));
    assert!((fact(&p, "gear 2 tip diameter") - s.outer_diameter2).abs() < 1e-6);
    // The plan measures the contact ratio on the flanks as generated.
    assert!((fact(&p, "contact ratio") - s.contact_ratio).abs() < 0.02, "{} {s:?}", fact(&p, "contact ratio"));
    assert!(
        !p.warnings.iter().any(|w| w.contains("undercut")
            || w.contains("thin tooth")
            || w.contains("point")
            || w.contains("contact ratio")
            || w.contains("clearance")),
        "{s:?}: {:?}",
        p.warnings
    );
    assert_eq!(worst(&check_mesh(&p)), Some(Verdict::Ok), "{s:?}");
    p
}

#[test]
fn modules_come_from_the_standard_series() {
    assert_eq!(standard_modules(1.0, 4.0, false), [1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0]);
    assert_eq!(standard_modules(1.0, 2.0, true), [1.0, 1.125, 1.25, 1.375, 1.5, 1.75, 2.0]);
    assert_eq!(standard_modules(0.45, 1.0, false), [0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 1.0]);
    assert!(standard_modules(60.0, 70.0, true).is_empty());
    let all = standard_modules(0.0, 100.0, true);
    assert!(all.windows(2).all(|w| w[0] < w[1]) && all.len() == 45);
}

#[test]
fn a_ratio_within_a_size_gives_ranked_pairs() {
    let goal = Goal { ratio: 3.0, max_outer_diameter: 100.0, ..Goal::default() };
    let out = suggest(&goal);
    assert_eq!(out.len(), goal.limit);
    for s in &out {
        assert!(standard(s.module) && (goal.min_module..=goal.max_module).contains(&s.module), "{s:?}");
        assert!(s.ratio_error.abs() <= goal.ratio_tolerance + 1e-12 && (s.ratio - f64::from(s.teeth2) / f64::from(s.teeth)).abs() < 1e-12);
        assert!(s.outer_diameter.max(s.outer_diameter2) <= 100.0 + 1e-9);
        // No shift allowed: no undercut, so 18 teeth or more at 20°.
        assert!(s.teeth >= 18 && s.shift == 0.0 && s.shift2 == 0.0, "{s:?}");
        assert!(s.contact_ratio > 1.2 && s.total_contact_ratio == s.contact_ratio);
        assert!(s.safety_bending.is_none() && s.safety_contact.is_none() && s.strong_enough.is_none());
        assert_eq!(s.hunting, gcd(s.teeth, s.teeth2) == 1);
    }
    assert!(out.windows(2).all(|w| w[0].score <= w[1].score));
    // The smallest: the finest module with the fewest teeth, exact.
    assert_eq!((out[0].teeth, out[0].teeth2, out[0].module), (18, 54, 1.0));
    assert!(out.iter().all(|s| room(s) >= room(&out[0]) - 1e-9));
    for s in &out[..4] {
        proves(&goal, s);
    }
    // Nothing fits in a thimble; a nonsense goal gives nothing rather than a panic.
    assert!(suggest(&Goal { max_outer_diameter: 10.0, ..goal.clone() }).is_empty());
    assert!(suggest(&Goal { ratio: -1.0, ..goal.clone() }).is_empty());
    assert!(suggest(&Goal { ratio: f64::NAN, ..goal.clone() }).is_empty());
    assert!(check(&Goal { min_module: 3.0, max_module: 2.0, ..goal.clone() }).unwrap_err().contains("module"));
    // A step-up ratio: gear 2 is the small one.
    let up = suggest(&Goal { ratio: 0.4, ..goal.clone() });
    assert!(!up.is_empty() && up.iter().all(|s| s.teeth2 >= 18 && s.teeth > s.teeth2 && s.ratio_error.abs() <= 0.01));
    proves(&Goal { ratio: 0.4, ..goal.clone() }, &up[0]);
    // An exact ratio.
    let exact = suggest(&Goal { ratio: 2.5, ratio_tolerance: 0.0, ..goal.clone() });
    assert!(!exact.is_empty() && exact.iter().all(|s| 2 * s.teeth2 == 5 * s.teeth));
}

#[test]
fn a_fixed_centre_distance_is_met_by_teeth_or_by_shift() {
    // 50 mm at 1.5: 20/30 at module 2 (and its like at other modules) without any shift.
    let goal = Goal { ratio: 1.5, center_distance: 50.0, ..Goal::default() };
    let out = suggest(&goal);
    assert!(out.iter().any(|s| (s.teeth, s.teeth2, s.module) == (20, 30, 2.0)), "{out:?}");
    for s in &out {
        assert!((s.center_distance - 50.0).abs() < 1e-6 && s.shift == 0.0 && s.shift2 == 0.0, "{s:?}");
        proves(&goal, s);
    }
    // 51 mm: no tooth counts give it at these modules and this ratio.
    let goal = Goal { center_distance: 51.0, ratio_tolerance: 0.0, ..goal };
    assert!(suggest(&goal).is_empty());
    // With shift they do: 20/30 at module 2 pushed apart by a shift sum of about half a module.
    let goal = Goal { allow_shift: true, ..goal };
    let out = suggest(&goal);
    assert!(!out.is_empty());
    let s = out.iter().find(|s| (s.teeth, s.teeth2, s.module) == (20, 30, 2.0)).expect("20/30");
    assert!(s.shift > s.shift2 && s.shift2 > 0.0 && (s.shift + s.shift2 - 0.53).abs() < 0.03, "{s:?}");
    for s in &out {
        assert!((s.center_distance - 51.0).abs() < 1e-6, "{s:?}");
        let p = proves(&goal, s);
        assert!((fact(&p, "centre distance") - 51.0).abs() < 1e-6);
    }
    // Pulled together a little (49.6 mm): a negative sum, most of it on the larger gear.
    let goal = Goal { center_distance: 49.6, ..goal };
    let out = suggest(&goal);
    let s = out.iter().find(|s| (s.teeth, s.teeth2, s.module) == (20, 30, 2.0)).expect("20/30");
    assert!(s.shift + s.shift2 < 0.0 && s.shift2 < s.shift, "{s:?}");
    proves(&goal, s);
    // Helical pairs keep the centre distance too.
    let goal = Goal { center_distance: 60.0, helix: 20.0, ratio: 2.0, ratio_tolerance: 0.02, ..goal };
    let out = suggest(&goal);
    assert!(!out.is_empty());
    for s in &out[..out.len().min(3)] {
        assert!((s.center_distance - 60.0).abs() < 1e-6 && s.total_contact_ratio > s.contact_ratio);
        proves(&goal, s);
    }
}

#[test]
fn shift_lets_small_pinions_in_without_undercut() {
    let plain = Goal { ratio: 4.0, max_outer_diameter: 130.0, min_module: 1.5, max_module: 2.0, ..Goal::default() };
    let shifted = Goal { allow_shift: true, ..plain.clone() };
    let (a, b) = (suggest(&plain), suggest(&shifted));
    assert!(a.iter().all(|s| s.teeth >= 18));
    // Fewer teeth, held up by a positive shift, in less room.
    assert!(b[0].teeth < 14 && b[0].shift > 0.1 && b[0].shift2 == 0.0, "{:?}", b[0]);
    assert!(room(&b[0]) < room(&a[0]));
    for s in &b[..5] {
        proves(&shifted, s);
    }
    // Asking for a minimum holds with shift allowed, and can't buy undercut without it.
    assert!(suggest(&Goal { min_teeth: 12, ..shifted.clone() }).iter().all(|s| s.teeth >= 12));
    assert!(suggest(&Goal { min_teeth: 10, ..plain.clone() }).iter().all(|s| s.teeth >= 18));
    assert!(suggest(&Goal { min_teeth: 25, ..plain.clone() }).iter().all(|s| s.teeth >= 25));
    // A 25° pressure angle needs fewer teeth to begin with.
    assert!(suggest(&Goal { pressure_angle: 25.0, ..plain }).iter().any(|s| s.teeth == 12));
}

#[test]
fn preferences_and_loads_rank_differently() {
    let load =
        Load { torque: 8.0, speed: 600.0, material: Material::SteelThroughHardened, material2: Material::SteelThroughHardened, ..Load::default() };
    let base = Goal { ratio: 3.0, max_outer_diameter: 160.0, max_module: 3.0, face_width: 12.0, load: load.clone(), ..Goal::default() };
    let pick = |prefer: Prefer| suggest(&Goal { prefer, ..base.clone() });
    let (small, quiet, strong) = (pick(Prefer::Smallest), pick(Prefer::Quietest), pick(Prefer::Strongest));
    for (name, out) in [("smallest", &small), ("quietest", &quiet), ("strongest", &strong)] {
        let s = &out[0];
        eprintln!(
            "{name:<9} z {}/{} m {} a {:.2} da2 {:.1} eps {:.2} SF {:.2} SH {:.2}",
            s.teeth,
            s.teeth2,
            s.module,
            s.center_distance,
            s.outer_diameter2,
            s.contact_ratio,
            s.safety_bending.unwrap(),
            s.safety_contact.unwrap()
        );
        assert!(out.windows(2).all(|w| w[0].score <= w[1].score));
    }
    let margin = |s: &Suggestion| (s.safety_bending.unwrap() / SF_MIN).min(s.safety_contact.unwrap() / SH_MIN);
    // Each is best at its own game.
    assert!(room(&small[0]) < room(&quiet[0]) && room(&small[0]) < room(&strong[0]));
    assert!(quiet[0].contact_ratio > small[0].contact_ratio && quiet[0].contact_ratio >= strong[0].contact_ratio);
    assert!(margin(&strong[0]) > margin(&small[0]) && margin(&strong[0]) >= margin(&quiet[0]));
    // The smallest is the smallest that carries the load: every choice offered is strong enough
    // (weaker ones only fill up the list after them), though smaller pairs exist.
    assert_eq!(small[0].strong_enough, Some(true));
    let first_weak = small.iter().position(|s| s.strong_enough == Some(false)).unwrap_or(small.len());
    assert!(first_weak >= 6 && small[first_weak..].iter().all(|s| s.strong_enough == Some(false)), "{small:?}");
    let unloaded = suggest(&Goal { load: Load::default(), ..base.clone() });
    assert!(room(&unloaded[0]) < room(&small[0]));
    let p = plan(&unloaded[0].spec(&base)).unwrap();
    assert!(fact(&p, "minimum contact safety SH") < SH_MIN || fact(&p, "minimum bending safety SF") < SF_MIN);
    // A load nothing in the range carries: the least weak are offered, marked as such.
    let hopeless = suggest(&Goal { load: Load { torque: 5000.0, ..base.load.clone() }, ..base.clone() });
    assert!(hopeless.iter().all(|s| s.strong_enough == Some(false)));
    // The safety factors are the plan's own rating of that pair.
    for s in [&small[0], &quiet[0], &strong[0], &small[3]] {
        let p = proves(&base, s);
        assert!((fact(&p, "minimum bending safety SF") / s.safety_bending.unwrap() - 1.0).abs() < 1e-9);
        assert!((fact(&p, "minimum contact safety SH") / s.safety_contact.unwrap() - 1.0).abs() < 1e-9);
    }
    // Without a load, "strongest" still ranks (bigger teeth, bigger gears) but reports no safety.
    let unrated = suggest(&Goal { load: Load::default(), prefer: Prefer::Strongest, ..base.clone() });
    assert!(unrated[0].safety_bending.is_none() && unrated[0].strong_enough.is_none() && unrated[0].outer_diameter2 > 150.0, "{:?}", unrated[0]);
    // Printed gears under the same load need far more gear.
    let pla = suggest(&Goal { load: Load { material: Material::Pla, material2: Material::Pla, ..load }, ..base.clone() });
    assert!(pla.is_empty() || room(&pla[0]) > room(&small[0]));
    assert_eq!(Prefer::parse("quietest"), Some(Prefer::Quietest));
    assert!(Prefer::ALL.iter().all(|p| Prefer::parse(p.name()) == Some(*p)));
}

#[test]
fn suggesting_is_quick() {
    // The widest search: every module, no size limit, rated.
    let goal = Goal {
        ratio: 3.7,
        ratio_tolerance: 0.02,
        min_module: 0.2,
        max_module: 50.0,
        second_choice_modules: true,
        allow_shift: true,
        max_teeth: 400,
        load: Load { torque: 5.0, speed: 1000.0, ..Load::default() },
        prefer: Prefer::Strongest,
        ..Goal::default()
    };
    let t = Instant::now();
    let out = suggest(&goal);
    let ms = t.elapsed().as_secs_f64() * 1e3;
    eprintln!("suggest, widest search: {ms:.1} ms, {} results", out.len());
    assert_eq!(out.len(), goal.limit);
    if !cfg!(debug_assertions) {
        crate::test_util::assert_quick("suggest, widest search", ms, 1000.0);
    }
}

#[test]
fn ratios_split_into_even_hunting_stages() {
    let l = StageLimits::default();
    for (total, n) in [(20.0, 2), (100.0, 3), (5.0, 1), (12.0, 2), (64.0, 3), (7.3, 2), (300.0, 4), (1.0, 1), (2.0, 3)] {
        let out = split_ratio(total, n, &l);
        assert!(!out.is_empty(), "{total} in {n}");
        let even = total.powf(1.0 / f64::from(n));
        let s = &out[0];
        eprintln!(
            "{total} in {n}: {:?} = {:.4} ({:+.3} %)",
            s.stages.iter().map(|st| (st.teeth, st.teeth2)).collect::<Vec<_>>(),
            s.ratio,
            s.error * 100.0
        );
        assert_eq!(s.stages.len(), n as usize);
        let product: f64 = s.stages.iter().map(|st| st.ratio).product();
        assert!((product - s.ratio).abs() < 1e-9 * total && (s.error - (s.ratio / total - 1.0)).abs() < 1e-12);
        assert!(s.error.abs() <= l.tolerance, "{total} in {n}: {s:?}");
        for st in &s.stages {
            assert!(st.teeth >= l.min_teeth && st.teeth2 <= l.max_teeth && st.teeth2 >= st.teeth && st.ratio <= l.max_stage_ratio + 1e-9);
            assert!((st.ratio - f64::from(st.teeth2) / f64::from(st.teeth)).abs() < 1e-12 && st.hunting == (gcd(st.teeth, st.teeth2) == 1));
            // Near-equal stages.
            assert!(st.ratio / even < 1.3 && even / st.ratio < 1.3, "{total} in {n}: {s:?}");
        }
        assert!(s.stages.windows(2).all(|w| w[0].ratio <= w[1].ratio));
        // Hunting teeth where the ratio leaves a choice (not a 1:1 stage).
        if even > 1.2 && n > 1 {
            assert!(s.stages.iter().all(|st| st.hunting), "{total} in {n}: {s:?}");
        }
        // The alternatives are different gearboxes.
        for (i, a) in out.iter().enumerate() {
            assert!(out[i + 1..].iter().all(|b| b.stages != a.stages));
        }
    }
    // A speed increaser is the reducer run backwards.
    let up = &split_ratio(0.05, 2, &l)[0];
    assert!((up.ratio - 0.05).abs() < 0.05 * l.tolerance && up.stages.iter().all(|st| st.teeth > st.teeth2 && st.ratio < 1.0));
    // Out of reach: 1000:1 in one stage, or in two of at most 6:1.
    assert!(split_ratio(1000.0, 1, &l).is_empty());
    assert!(split_ratio(1000.0, 2, &l).is_empty());
    assert!(!split_ratio(1000.0, 4, &l).is_empty());
    assert!(split_ratio(10.0, 0, &l).is_empty() && split_ratio(f64::NAN, 2, &l).is_empty() && split_ratio(-3.0, 2, &l).is_empty());
    // Tighter limits are kept.
    let tight = StageLimits { min_teeth: 12, max_teeth: 60, max_stage_ratio: 4.0, tolerance: 0.01 };
    let s = &split_ratio(30.0, 3, &tight)[0];
    assert!(s.error.abs() <= 0.01 && s.stages.iter().all(|st| st.teeth >= 12 && st.teeth2 <= 60 && st.ratio <= 4.0));
}
