use super::*;
use crate::plan::plan;
use std::time::Instant;

fn fact(p: &crate::Plan, label: &str) -> f64 {
    p.facts
        .iter()
        .find(|f| f.label == label)
        .unwrap_or_else(|| panic!("no fact `{label}` in {:?}", p.facts.iter().map(|f| &f.label).collect::<Vec<_>>()))
        .value
}

fn row(s: &Solved, label: &str) -> f64 {
    s.chosen.iter().find(|f| f.label == label).unwrap_or_else(|| panic!("no row `{label}` in {:?}", s.chosen)).value
}

fn of(kind: Kind) -> Simple {
    Simple { kind: Some(kind), ..Simple::default() }
}

fn solved(x: &Simple, base: &Spec) -> Solved {
    let s = solve(x, base);
    assert!(s.error.is_none(), "{:?}: {:?}\nnotes {:?}\nspec {:?}", x, s.error, s.notes, s.spec);
    s
}

fn close(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= tol)
}

#[test]
fn every_kind_solves_from_defaults_and_plans() {
    for k in Kind::ALL.iter().copied() {
        let base = Spec::defaults_for(k);
        let s = solved(&of(k), &base);
        assert_eq!(s.kind, k);
        assert!(!s.chosen.is_empty(), "{k:?}: nothing chosen");
        plan(&s.spec).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        // From another family's base too (the Advanced settings carried over).
        let s2 = solved(&of(k), &Spec::defaults_for(Kind::Pair));
        plan(&s2.spec).unwrap_or_else(|e| panic!("{k:?} from a pair: {e}"));
        // Printable.
        let s3 = solved(&Simple { printable: true, ..of(k) }, &base);
        if k != Kind::Hypoid {
            plan(&s3.spec).unwrap_or_else(|e| panic!("{k:?} printable: {e}"));
        }
    }
}

#[test]
fn defaults_solve_to_themselves() {
    // Advanced → Simple → solve gives the set back.
    for k in Kind::ALL.iter().copied() {
        let d = Spec::defaults_for(k);
        let back = solved(&outputs_of(&d), &d).spec;
        assert_eq!((back.module, back.teeth, back.teeth2, back.face_width), (d.module, d.teeth, d.teeth2, d.face_width), "{k:?}");
        assert_eq!((back.shift, back.shift2, back.center_distance), (d.shift, d.shift2, d.center_distance), "{k:?}");
    }
}

#[test]
fn pair_hits_centre_distance_exactly() {
    let base = Spec::defaults_for(Kind::Pair);
    for (ratio, a, helix) in
        [(1.6, 52.0, 0.0), (3.0, 80.0, 0.0), (2.0, 47.3, 0.0), (1.0, 33.33, 0.0), (4.5, 120.0, 20.0), (2.7, 61.0, 15.0), (7.0, 200.0, 0.0)]
    {
        let x = Simple { ratio: Some(ratio), center_distance: Some(a), helix: Some(helix), ..of(Kind::Pair) };
        let s = solved(&x, &base);
        let p = plan(&s.spec).unwrap();
        assert!((fact(&p, "centre distance") - a).abs() < 1e-6, "{ratio} {a}: {}", fact(&p, "centre distance"));
        let got = f64::from(s.spec.teeth2) / f64::from(s.spec.teeth);
        assert!((got / ratio - 1.0).abs() <= 0.03, "{ratio} {a}: ratio {got}");
        assert!((row(&s, "ratio error %") / 100.0 - (got / ratio - 1.0)).abs() < 1e-9);
        assert!(design::standard_modules(0.0, 100.0, true).contains(&s.spec.module), "module {}", s.spec.module);
        assert!(!p.warnings.iter().any(|w| w.contains("undercut")), "{ratio} {a}: {:?}", p.warnings);
    }
}

#[test]
fn drawn_pitch_diameters() {
    // Two circles touching: both pitch Ø, their centre distance kept exactly.
    let base = Spec::defaults_for(Kind::Pair);
    let x = Simple { pitch_diameter: Some(41.0), pitch_diameter2: Some(97.0), ..of(Kind::Pair) };
    let s = solved(&x, &base);
    let p = plan(&s.spec).unwrap();
    assert!((fact(&p, "centre distance") - 69.0).abs() < 1e-6);
    let (d1, d2) = (fact(&p, "gear 1 pitch diameter"), fact(&p, "gear 2 pitch diameter"));
    assert!((d1 - 41.0).abs() <= s.spec.module && (d2 - 97.0).abs() <= s.spec.module, "{d1} {d2} m {}", s.spec.module);
    // Exact: a non-standard module that draws them as drawn.
    let e = solved(&Simple { exact: true, ..x.clone() }, &base);
    let p = plan(&e.spec).unwrap();
    let (d1, d2) = (fact(&p, "gear 1 pitch diameter"), fact(&p, "gear 2 pitch diameter"));
    assert!((d1 + d2 - 138.0).abs() < 1e-9 && (d2 / d1 - 97.0 / 41.0).abs() < 0.03, "{d1} {d2}");
    // One circle and a ratio.
    let s = solved(&Simple { pitch_diameter: Some(36.0), ratio: Some(2.5), ..of(Kind::Pair) }, &base);
    let p = plan(&s.spec).unwrap();
    assert!((fact(&p, "gear 1 pitch diameter") - 36.0).abs() <= s.spec.module);
    // One gear, one circle.
    for d in [12.0, 25.0, 44.0, 45.0, 100.0, 333.0] {
        let s = solved(&Simple { pitch_diameter: Some(d), ..of(Kind::Spur) }, &Spec::defaults_for(Kind::Spur));
        let p = plan(&s.spec).unwrap();
        assert!((fact(&p, "pitch diameter") - d).abs() <= s.spec.module / 2.0 + 1e-9, "{d}: {}", fact(&p, "pitch diameter"));
        let e = solved(&Simple { pitch_diameter: Some(d), exact: true, ..of(Kind::Spur) }, &Spec::defaults_for(Kind::Spur));
        assert!((2.0 * e.spec.involute(e.spec.teeth, 0.0, 0.0).r() - d).abs() < 1e-9, "{d} exact");
    }
    // Outer diameter.
    let s = solved(&Simple { outer_diameter: Some(60.0), ..of(Kind::Helical) }, &Spec::defaults_for(Kind::Helical));
    let p = plan(&s.spec).unwrap();
    assert!((fact(&p, "tip diameter") - 60.0).abs() <= s.spec.module, "{}", fact(&p, "tip diameter"));
}

#[test]
fn pair_fits_inside_an_outer_diameter() {
    let base = Spec::defaults_for(Kind::Pair);
    let s = solved(&Simple { outer_diameter: Some(80.0), ratio: Some(3.0), ..of(Kind::Pair) }, &base);
    let p = plan(&s.spec).unwrap();
    let big = fact(&p, "gear 1 tip diameter").max(fact(&p, "gear 2 tip diameter"));
    assert!(big <= 80.0 + 1e-9 && big > 80.0 - 4.0 * s.spec.module, "{big}");
}

#[test]
fn internal_and_rack_companions() {
    // A circle inside a bigger one: the ring and its pinion.
    let s = solved(&Simple { ring_diameter: Some(120.0), pitch_diameter: Some(40.0), ..of(Kind::Internal) }, &Spec::defaults_for(Kind::Internal));
    let p = plan(&s.spec).unwrap();
    assert!((fact(&p, "pitch diameter") - 120.0).abs() <= s.spec.module);
    let pin = s.companion.clone().expect("pinion");
    let pp = plan(&pin).unwrap();
    assert!((fact(&pp, "pitch diameter") - 40.0).abs() <= s.spec.module);
    let a = row(&s, "pinion centre offset");
    assert!((pin.origin[0] - a).abs() < 1e-9 && (a - (fact(&p, "pitch diameter") - fact(&pp, "pitch diameter")) / 2.0).abs() < 1e-9);
    // Circle + tangent line: rack and pinion.
    let s = solved(&Simple { pitch_diameter: Some(30.0), length: Some(200.0), ..of(Kind::Rack) }, &Spec::defaults_for(Kind::Rack));
    let p = plan(&s.spec).unwrap();
    assert!((fact(&p, "length") - 200.0).abs() <= PI * s.spec.module / 2.0 + 1e-9);
    let pin = s.companion.clone().unwrap();
    assert!((plan(&pin).map(|p| fact(&p, "pitch diameter")).unwrap() - 30.0).abs() <= s.spec.module);
    assert!((pin.origin[1] - pin.module * f64::from(pin.teeth) / 2.0).abs() < 1e-9);
}

#[test]
fn planetary_from_circles_and_ratio() {
    let base = Spec::defaults_for(Kind::Planetary);
    for (ratio, n, ring) in [(4.0, 3, 108.0), (5.0, 4, 150.0), (7.0, 3, 200.0), (3.2, 5, 90.0)] {
        let s = solved(&Simple { ratio: Some(ratio), planets: Some(n), ring_diameter: Some(ring), ..of(Kind::Planetary) }, &base);
        let p = plan(&s.spec).unwrap();
        assert!(!p.warnings.iter().any(|w| w.contains("collide") || w.contains("unevenly") || w.contains("sit at")), "{ratio} {n}: {:?}", p.warnings);
        let got = fact(&p, "ratio (ring fixed, sun in, carrier out)");
        assert!((got / ratio - 1.0).abs() < 0.05, "{ratio}: {got}");
        let zr = fact(&p, "ring teeth");
        assert!((s.spec.module * zr - ring).abs() <= 2.0 * s.spec.module, "{ring}: {}", s.spec.module * zr);
    }
    // Drawn sun, planet and ring that don't close: snapped, and said so.
    let s = solved(
        &Simple { pitch_diameter: Some(30.0), pitch_diameter2: Some(31.0), ring_diameter: Some(90.0), planets: Some(3), ..of(Kind::Planetary) },
        &base,
    );
    assert!(s.notes.iter().any(|n| n.contains("don't close")), "{:?}", s.notes);
    plan(&s.spec).unwrap();
}

#[test]
fn bevels_from_angle_ratio_and_one_size() {
    let base = Spec::defaults_for(Kind::Bevel);
    for (sigma, ratio) in [(90.0, 1.5), (60.0, 2.0), (120.0, 1.0), (30.0, 3.0), (150.0, 2.0)] {
        for size in [
            Simple { pitch_diameter: Some(40.0), ..Simple::default() },
            Simple { pitch_diameter2: Some(90.0), ..Simple::default() },
            Simple { cone_distance: Some(60.0), ..Simple::default() },
        ] {
            let x = Simple { kind: Some(Kind::Bevel), shaft_angle: Some(sigma), ratio: Some(ratio), ..size.clone() };
            let s = solved(&x, &base);
            let p = plan(&s.spec).unwrap_or_else(|e| panic!("{sigma} {ratio}: {e}"));
            if let Some(d) = size.pitch_diameter {
                assert!((fact(&p, "gear 1 outer pitch diameter") - d).abs() <= s.spec.module);
            }
            if let Some(d) = size.pitch_diameter2 {
                assert!(
                    (fact(&p, "gear 2 outer pitch diameter") - d).abs() <= ratio * s.spec.module + 1e-9,
                    "{sigma} {ratio}: {}",
                    fact(&p, "gear 2 outer pitch diameter")
                );
            }
            if let Some(re) = size.cone_distance {
                assert!((fact(&p, "gear 1 cone distance") - re).abs() <= 2.0 * s.spec.module);
            }
            // The default face: 30 % of the cone distance (the base's own while its module stays).
            let face = s.spec.face_width;
            if s.spec.module != base.module {
                assert!((face - (0.3 * fact(&p, "gear 1 cone distance") * 2.0).round() / 2.0).abs() < 1e-9, "face {face}");
            }
        }
    }
}

#[test]
fn skew_sets_from_offsets() {
    // Crossed helical: pitch cylinders touching across the offset, exactly.
    for (sigma, e, ratio) in [(90.0, 40.0, 1.0), (90.0, 55.0, 2.0), (60.0, 30.0, 1.5)] {
        let s = solved(
            &Simple { kind: Some(Kind::CrossedHelical), shaft_angle: Some(sigma), offset: Some(e), ratio: Some(ratio), ..Simple::default() },
            &Spec::defaults_for(Kind::CrossedHelical),
        );
        let p = plan(&s.spec).unwrap();
        assert!((fact(&p, "centre distance") - e).abs() < 1e-6, "{e}: {}", fact(&p, "centre distance"));
        assert!((fact(&p, "gear 1 helix angle") + fact(&p, "gear 2 helix angle") - sigma).abs() < 1e-9);
    }
    // Worm: the centre distance exact (the worm's diameter takes it up).
    for (ratio, a) in [(30.0, 40.0), (20.0, 60.0), (40.0, 35.0)] {
        let s = solved(&Simple { ratio: Some(ratio), center_distance: Some(a), ..of(Kind::Worm) }, &Spec::defaults_for(Kind::Worm));
        let p = plan(&s.spec).unwrap();
        assert!((fact(&p, "centre distance") - a).abs() < 1e-6);
        assert!((fact(&p, "ratio") / ratio - 1.0).abs() < 0.05);
    }
    // Worm from a wheel circle and a tangent line (its pitch line): the line's distance.
    let s = solved(
        &Simple { pitch_diameter2: Some(60.0), center_distance: Some(40.0), ratio: Some(30.0), ..of(Kind::Worm) },
        &Spec::defaults_for(Kind::Worm),
    );
    let p = plan(&s.spec).unwrap();
    assert!((fact(&p, "centre distance") - 40.0).abs() < 1e-6 && (fact(&p, "wheel pitch diameter") - 60.0).abs() <= 2.0 * s.spec.module);
    // Hypoid: ring gear Ø, ratio, offset.
    let s = solved(
        &Simple {
            kind: Some(Kind::Hypoid),
            pitch_diameter2: Some(150.0),
            ratio: Some(3.0),
            offset: Some(20.0),
            shaft_angle: Some(90.0),
            ..Simple::default()
        },
        &Spec::defaults_for(Kind::Hypoid),
    );
    let p = plan(&s.spec).unwrap();
    assert!(
        (fact(&p, "gear outer pitch diameter") - 150.0).abs() < 1e-9
            && (fact(&p, "offset") - 20.0).abs() < 1e-9
            && (fact(&p, "ratio") - 3.0).abs() < 1e-9
    );
}

#[test]
fn auto_kind_sweeps() {
    let first = |rel: Relation, x: &Simple| auto_kind(&rel, x)[0].0;
    // Square shafts, a 100 mm gear, a 2:1 ratio: hypoid up to a quarter of the gear, crossed
    // helical where the pitch cylinders touch (75 mm).
    let x = Simple { ratio: Some(2.0), pitch_diameter2: Some(100.0), ..Simple::default() };
    assert_eq!(first(relation_of(90.0, 0.0), &x), Kind::Bevel);
    let mut kinds = Vec::new();
    for e in [0.5, 5.0, 15.0, 25.0, 35.0, 60.0, 75.0, 90.0] {
        kinds.push(first(relation_of(90.0, e), &x));
    }
    assert_eq!(
        kinds,
        [Kind::Hypoid, Kind::Hypoid, Kind::Hypoid, Kind::Hypoid, Kind::Hypoid, Kind::CrossedHelical, Kind::CrossedHelical, Kind::CrossedHelical],
        "{kinds:?}"
    );
    // A 30:1 ratio: hypoid near, worm far (wheel 100 mm: 50 mm and more).
    let x = Simple { ratio: Some(30.0), pitch_diameter2: Some(100.0), ..Simple::default() };
    let w: Vec<Kind> = [5.0, 60.0, 70.0].iter().map(|e| first(relation_of(90.0, *e), &x)).collect();
    assert_eq!(w[1..], [Kind::Worm, Kind::Worm], "{w:?}");
    // Not square: no worm.
    assert!(auto_kind(&relation_of(80.0, 60.0), &x).iter().all(|c| c.0 != Kind::Worm || c.2 == 0.0));
    // Scores change smoothly with the offset.
    let x = Simple { ratio: Some(2.0), pitch_diameter2: Some(100.0), ..Simple::default() };
    let score = |e: f64, k: Kind| auto_kind(&relation_of(90.0, e), &x).iter().find(|c| c.0 == k).map_or(0.0, |c| c.2);
    for k in [Kind::Hypoid, Kind::CrossedHelical] {
        for i in 1..400 {
            let (a, b) = (score(f64::from(i) * 0.25, k), score(f64::from(i + 1) * 0.25, k));
            assert!((a - b).abs() < 0.05, "{k:?} jumps at {}", f64::from(i) * 0.25);
        }
    }
    // Angle sweep with no offset.
    let x = Simple { ratio: Some(2.0), ..Simple::default() };
    assert_eq!(first(relation_of(0.0, 0.0), &Simple::default()), Kind::Spur);
    assert_eq!(first(relation_of(0.0, 0.0), &Simple { helix: Some(20.0), ..Simple::default() }), Kind::Helical);
    assert_eq!(first(relation_of(0.0, 0.0), &Simple { ratio: Some(5.0), ..Simple::default() }), Kind::Planetary);
    assert_eq!(first(relation_of(0.0, 0.0), &Simple { ratio: Some(30.0), ..Simple::default() }), Kind::CycloidalDrive);
    for a in [30.0, 60.0, 90.0, 120.0, 150.0] {
        assert_eq!(first(relation_of(a, 0.0), &x), Kind::Bevel, "{a}");
    }
    assert_eq!(first(relation_of(90.0, 0.0), &Simple { spiral_angle: Some(35.0), ..x.clone() }), Kind::SpiralBevel);
    // Parallel: a pair, or a belt when the gears drawn don't reach.
    assert_eq!(first(relation_of(0.0, 60.0), &x), Kind::Pair);
    assert_eq!(first(relation_of(0.0, 60.0), &Simple { pitch_diameter: Some(40.0), ..x.clone() }), Kind::Pair);
    assert_eq!(first(relation_of(0.0, 300.0), &Simple { pitch_diameter: Some(40.0), ..x.clone() }), Kind::Pulley);
    // Every kind it chooses solves and plans.
    for a in [0.0, 30.0, 90.0, 150.0] {
        for e in [0.0, 5.0, 25.0, 60.0, 90.0] {
            for r in [1.0, 2.0, 3.0, 30.0] {
                // (Parallel shafts closer than a pair of the smallest sensible gears allows at
                // this ratio can't be met by any gears.)
                if a == 0.0 && e > 0.0 && e < 0.5 * 17.0 * (1.0 + r) / 2.0 {
                    continue;
                }
                // (No bevel cone is that slim: nothing fits, and the ranking says so.)
                if a > 0.0 && e == 0.0 && r == 30.0 {
                    assert!(
                        auto_kind(&relation_of(a, e), &Simple { ratio: Some(r), ..Simple::default() })
                            .iter()
                            .all(|c| c.2 == 0.0 || c.0 == Kind::FaceGear)
                    );
                    continue;
                }
                let x = Simple { shaft_angle: Some(a), offset: Some(e), ratio: Some(r), pitch_diameter2: Some(100.0), ..Simple::default() };
                // Nothing fits (every score 0): the window says so; no set is promised.
                if auto_kind(&x.relation(), &x)[0].2 < 0.05 {
                    continue;
                }
                let s = solve(&x, &Spec::default());
                let p = s.error.clone().map_or_else(|| plan(&s.spec).err(), Some);
                assert!(p.is_none(), "{a}° {e} mm {r}:1 → {:?}: {p:?} {:?}", s.kind, s.notes);
            }
        }
    }
}

#[test]
fn sketch_rows() {
    // One circle: a single gear (helical by the helix field).
    let one = Simple { pitch_diameter: Some(50.0), ..Simple::default() };
    assert_eq!(kind_of(&one), Kind::Spur);
    assert_eq!(kind_of(&Simple { helix: Some(20.0), ..one.clone() }), Kind::Helical);
    // Two circles touching outside: a pair at their centre distance.
    let two = Simple { pitch_diameter: Some(40.0), pitch_diameter2: Some(80.0), offset: Some(60.0), ..Simple::default() };
    let s = solved(&two, &Spec::default());
    assert_eq!(s.kind, Kind::Pair);
    assert!((pair_gears(&s.spec).2 - 60.0).abs() < 1e-6);
    // Circle + line through its centre: a 90° bevel from the ratio.
    let bevel = Simple { pitch_diameter2: Some(90.0), ratio: Some(2.0), shaft_angle: Some(90.0), offset: Some(0.0), ..Simple::default() };
    assert_eq!(solved(&bevel, &Spec::default()).kind, Kind::Bevel);
    // Circle (the ring gear) + a line crossing it off centre: a hypoid; missing it: crossed helical.
    let line = |e: f64| Simple { pitch_diameter2: Some(100.0), ratio: Some(2.0), shaft_angle: Some(90.0), offset: Some(e), ..Simple::default() };
    for e in [5.0, 20.0, 40.0] {
        let s = solved(&line(e), &Spec::default());
        assert_eq!(s.kind, Kind::Hypoid, "{e}");
        plan(&s.spec).unwrap_or_else(|err| panic!("{e}: {err}"));
    }
    for e in [60.0, 75.0] {
        let s = solved(&line(e), &Spec::default());
        assert_eq!(s.kind, Kind::CrossedHelical, "{e}");
        let p = plan(&s.spec).unwrap();
        assert!((fact(&p, "centre distance") - e).abs() < 1e-6);
    }
    // ... a high ratio and a line beyond the wheel's radius: worm and wheel.
    let s = solved(&Simple { ratio: Some(30.0), ..line(60.0) }, &Spec::default());
    assert_eq!(s.kind, Kind::Worm);
    let p = plan(&s.spec).unwrap();
    assert!((fact(&p, "centre distance") - 60.0).abs() < 1e-6 && (fact(&p, "wheel pitch diameter") - 100.0).abs() <= 2.0 * s.spec.module);
}

#[test]
fn round_trips() {
    let cases = vec![
        Simple { outer_diameter: Some(45.0), ..of(Kind::Spur) },
        Simple { pitch_diameter: Some(60.0), helix: Some(20.0), left_hand: Some(true), ..of(Kind::Helical) },
        Simple { ratio: Some(3.0), center_distance: Some(80.0), ..of(Kind::Pair) },
        Simple { ratio: Some(2.0), center_distance: Some(47.3), helix: Some(15.0), ..of(Kind::Pair) },
        Simple { ratio: Some(5.0), planets: Some(4), ring_diameter: Some(150.0), ..of(Kind::Planetary) },
        Simple { shaft_angle: Some(90.0), ratio: Some(2.0), pitch_diameter: Some(40.0), ..of(Kind::Bevel) },
        Simple { shaft_angle: Some(90.0), ratio: Some(3.0), offset: Some(20.0), pitch_diameter2: Some(150.0), ..of(Kind::Hypoid) },
        Simple { shaft_angle: Some(90.0), center_distance: Some(55.0), ratio: Some(2.0), ..of(Kind::CrossedHelical) },
        Simple { ratio: Some(30.0), center_distance: Some(40.0), ..of(Kind::Worm) },
        Simple { ratio: Some(7.5), center_distance: Some(40.0), ..of(Kind::Cycloidal) },
        Simple { ratio: Some(15.0), outer_diameter: Some(80.0), ..of(Kind::CycloidalDrive) },
        Simple { teeth: Some(19), ratio: Some(2.0), center_distance: Some(400.0), ..of(Kind::Sprocket) },
        Simple { pitch_diameter: Some(25.0), ..of(Kind::Spline) },
        Simple { center_distance: Some(70.0), eccentricity: Some(0.3), ..of(Kind::Elliptical) },
        Simple { length: Some(150.0), pitch_diameter: Some(30.0), ..of(Kind::Rack) },
    ];
    for x in cases {
        let s = solved(&x, &Spec::defaults_for(x.kind.unwrap()));
        let y = measures(&s.spec);
        let m = s.spec.module.max(1.0);
        for f in [
            Field::ShaftAngle,
            Field::Offset,
            Field::CenterDistance,
            Field::PitchDiameter,
            Field::PitchDiameter2,
            Field::RingDiameter,
            Field::OuterDiameter,
            Field::Length,
            Field::Helix,
            Field::Planets,
            Field::Eccentricity,
            Field::Teeth,
        ] {
            if let Some(want) = x.get(f) {
                let got = y.get(f).unwrap_or_else(|| panic!("{:?} {f:?} missing", x.kind));
                let tol = match f {
                    Field::CenterDistance
                    | Field::Offset
                    | Field::ShaftAngle
                    | Field::Helix
                    | Field::Planets
                    | Field::Eccentricity
                    | Field::Teeth => 1e-6,
                    Field::Length => PI * m / 2.0 + 1e-9,
                    _ => 2.0 * m,
                };
                // A chain's centre distance is what was asked (it sits at whole links).
                assert!((got - want).abs() <= tol, "{:?} {f:?}: {want} asked, {got}", x.kind);
            }
        }
        if let Some(r) = x.ratio {
            assert!((y.ratio.unwrap() / r - 1.0).abs() <= 0.05, "{:?} ratio {r}: {:?}", x.kind, y.ratio);
        }
        // And the driving outputs solve to the same set.
        let again = solved(&outputs_of(&s.spec), &s.spec).spec;
        assert_eq!(
            (again.module, again.teeth, again.teeth2, again.shift, again.shift2),
            (s.spec.module, s.spec.teeth, s.spec.teeth2, s.spec.shift, s.spec.shift2),
            "{:?}",
            x.kind
        );
    }
}

#[test]
fn advanced_settings_survive() {
    let mut base = Spec::defaults_for(Kind::Pair);
    base.pressure_angle = 25.0;
    base.backlash = 0.1;
    base.bore_chamfer = 0.5;
    base.name = "drive".into();
    let s = solved(&Simple { ratio: Some(3.0), center_distance: Some(90.0), ..of(Kind::Pair) }, &base);
    assert_eq!((s.spec.pressure_angle, s.spec.backlash, s.spec.bore_chamfer, s.spec.name.as_str()), (25.0, 0.1, 0.5, "drive"));
    let b = solved(&of(Kind::Bevel), &base);
    assert_eq!((b.spec.pressure_angle, b.spec.backlash, b.spec.name.as_str()), (25.0, 0.1, "drive"));
}

/// Variants of each kind's defaults for the frame checks.
fn variants() -> Vec<Spec> {
    let mut v: Vec<Spec> = Kind::ALL.iter().map(|k| Spec::defaults_for(*k)).collect();
    let place = Frame { origin: [10.0, -5.0, 3.0], x: [0.0, 1.0, 0.0], z: [1.0, 0.0, 0.0] };
    v.push(Spec { only: 2, ..Spec::defaults_for(Kind::Pair) });
    v.push(Spec { placement: Some(place), origin: [1.0, 2.0, 3.0], ..Spec::defaults_for(Kind::Bevel) });
    v.push(Spec { placement: Some(place), ..Spec::defaults_for(Kind::Worm) });
    v.push(Spec { left_hand: false, ..Spec::defaults_for(Kind::Hypoid) });
    v.push(Spec { placement: Some(place), origin: [0.0, 4.0, 0.0], ..Spec::defaults_for(Kind::Hypoid) });
    v.push(Spec { shaft_angle: 60.0, ..Spec::defaults_for(Kind::Bevel) });
    v.push(Spec { shaft_angle: 70.0, helix: 30.0, ..Spec::defaults_for(Kind::CrossedHelical) });
    v.push(Spec { teeth2: 40, center_distance: 300.0, ..Spec::defaults_for(Kind::Sprocket) });
    v.push(Spec { teeth2: 36, center_distance: 120.0, ..Spec::defaults_for(Kind::Pulley) });
    v.push(Spec { center_distance: 55.0, ..Spec::defaults_for(Kind::Pair) });
    v.push(Spec { planets: 4, teeth: 20, teeth2: 16, teeth3: 52, only: 3, ..Spec::defaults_for(Kind::Planetary) });
    v.push(Spec { only: 2, ..Spec::defaults_for(Kind::FaceGear) });
    v.push(Spec { only: 2, ..Spec::defaults_for(Kind::Spline) });
    let bp = Spec::defaults_for(Kind::BevelPlanetary);
    v.push(Spec { placement: Some(place), origin: [1.0, 2.0, 3.0], first_azimuth: 20.0, ..bp.clone() });
    v.push(Spec { bevel_planet_mode: crate::plan::BevelPlanetMode::Ring, shaft_angle: 45.0, teeth: 20, teeth2: 10, planets: 3, ..bp.clone() });
    v.push(Spec { bevel_planet_mode: crate::plan::BevelPlanetMode::Single, planets: 3, only: 2, ..bp });
    v
}

#[test]
fn schematic_frames_match_the_plan() {
    for s in variants() {
        let p = plan(&s).unwrap_or_else(|e| panic!("{:?}: {e}", s.kind));
        let sch = schematic(&s);
        assert!(sch.note.is_none(), "{:?}: {:?}", s.kind, sch.note);
        // Members are the preview's, in its order.
        let preview: Vec<&str> = p.preview.iter().map(|g| g.name.as_str()).collect();
        if s.kind == Kind::Differential {
            // Its schematic is every body (case, ring, pinions...); its preview one spoke or section.
        } else if s.kind == Kind::BevelPlanetary {
            // Its preview is one spoke: the planets after the first take planet 1's place.
            let names: Vec<&str> = sch.members.iter().map(|m| m.name.as_str()).filter(|n| preview.contains(n)).collect();
            assert_eq!(names, preview, "{:?}", s.kind);
            for m in &sch.members {
                let at = if m.name.starts_with("planet ") { "planet 1" } else { m.name.as_str() };
                assert_eq!(preview.get(m.color), Some(&at), "{}", m.name);
            }
        } else {
            let names: Vec<&str> = sch.members.iter().map(|m| m.name.as_str()).collect();
            assert_eq!(names, preview, "{:?}", s.kind);
            for (i, m) in sch.members.iter().enumerate() {
                assert_eq!(m.color, i);
            }
        }
        let bodies: Vec<usize> = sch.members.iter().filter_map(|m| m.body).collect();
        // (A bevel planetary set's carrier is a support, not a gear: the schematic leaves it out.)
        let gears = p.bodies.iter().filter(|b| !b.name.contains(crate::plan::bevel_planet::CARRIER)).count();
        assert_eq!(bodies.len(), gears, "{:?} only {}", s.kind, s.only);
        for m in &sch.members {
            let Some(bi) = m.body else { continue };
            let f = p.bodies[bi].frame;
            assert!(
                close(m.frame.origin, f.origin, 1e-6) && close(m.frame.x, f.x, 1e-9) && close(m.frame.z, f.z, 1e-9),
                "{:?} {}: {:?} against the plan's {:?}",
                s.kind,
                m.name,
                m.frame,
                f
            );
        }
    }
}

#[test]
fn schematic_diameters_match_the_plan() {
    let pitch_of = |sch: &Schematic, name: &str| sch.members.iter().find(|m| m.name == name).unwrap().pitch_diameter;
    let tip_of = |sch: &Schematic, name: &str| {
        sch.members.iter().find(|m| m.name == name).unwrap().parts.iter().find_map(|p| match (&p.role, &p.shape) {
            (Role::Tip, Shape::Cylinder { r, .. }) => Some(2.0 * r),
            _ => None,
        })
    };
    let cases: Vec<(Spec, Vec<(&str, &str, Option<&str>)>)> = vec![
        (Spec::defaults_for(Kind::Spur), vec![("gear", "pitch diameter", Some("tip diameter"))]),
        (Spec::defaults_for(Kind::Helical), vec![("gear", "pitch diameter", Some("tip diameter"))]),
        (
            Spec { center_distance: 55.0, ..Spec::defaults_for(Kind::Pair) },
            vec![("gear 1", "gear 1 pitch diameter", Some("gear 1 tip diameter")), ("gear 2", "gear 2 pitch diameter", Some("gear 2 tip diameter"))],
        ),
        (Spec::defaults_for(Kind::Internal), vec![("ring", "pitch diameter", None)]),
        (Spec::defaults_for(Kind::Bevel), vec![("gear 1", "gear 1 outer pitch diameter", None), ("gear 2", "gear 2 outer pitch diameter", None)]),
        (
            Spec::defaults_for(Kind::CrossedHelical),
            vec![("gear 1", "gear 1 pitch diameter", Some("gear 1 tip diameter")), ("gear 2", "gear 2 pitch diameter", Some("gear 2 tip diameter"))],
        ),
        (Spec::defaults_for(Kind::Worm), vec![("wheel", "wheel pitch diameter", None), ("worm", "worm pitch diameter", Some("worm tip diameter"))]),
        (Spec::defaults_for(Kind::Cycloidal), vec![("gear 1", "gear 1 pitch diameter", None), ("gear 2", "gear 2 pitch diameter", None)]),
        (Spec::defaults_for(Kind::Sprocket), vec![("sprocket", "pitch diameter", None)]),
        (
            Spec { teeth2: 36, center_distance: 120.0, ..Spec::defaults_for(Kind::Pulley) },
            vec![("pulley 1", "pulley 1 pitch diameter", None), ("pulley 2", "pulley 2 pitch diameter", None)],
        ),
        (Spec::defaults_for(Kind::Spline), vec![("shaft", "pitch diameter", None)]),
        (Spec::defaults_for(Kind::FaceGear), vec![("pinion", "pinion pitch diameter", None), ("face gear", "face gear pitch diameter (m·z₂)", None)]),
        (Spec::defaults_for(Kind::Hypoid), vec![("gear", "gear outer pitch diameter", None), ("pinion", "pinion mean pitch diameter", None)]),
    ];
    for (s, rows) in cases {
        let p = plan(&s).unwrap();
        let sch = schematic(&s);
        for (member, pitch, tip) in rows {
            assert!(
                (pitch_of(&sch, member) - fact(&p, pitch)).abs() < 1e-6,
                "{:?} {member}: {} against {}",
                s.kind,
                pitch_of(&sch, member),
                fact(&p, pitch)
            );
            if let Some(t) = tip {
                let got = tip_of(&sch, member).unwrap();
                assert!((got - fact(&p, t)).abs() < 1e-6, "{:?} {member} tip: {got} against {}", s.kind, fact(&p, t));
            }
        }
        // Centre distances.
        if let Some(d) = sch.dims.iter().find(|d| d.kind == DimKind::CenterDistance)
            && let Some(f) = p.facts.iter().find(|f| f.label.starts_with("centre distance"))
        {
            assert!((d.value - f.value).abs() < 1e-6, "{:?}: {} against {}", s.kind, d.value, f.value);
            let gap = v_sub(d.points[1], d.points[0]);
            assert!((v_dot(gap, gap).sqrt() - d.value).abs() < 1e-6, "{:?}: drawn {:?}", s.kind, d.points);
        }
    }
    // A hypoid's offset dimension spans its axes' common perpendicular.
    let s = Spec::defaults_for(Kind::Hypoid);
    let sch = schematic(&s);
    let off = sch.dims.iter().find(|d| d.kind == DimKind::Offset).unwrap();
    let gap = v_sub(off.points[1], off.points[0]);
    assert!((v_dot(gap, gap).sqrt() - s.offset).abs() < 1e-6, "{:?}", off.points);
    let ang = sch.dims.iter().find(|d| d.kind == DimKind::ShaftAngle).unwrap();
    let (a1, a2) = (v_sub(ang.points[1], ang.points[0]), v_sub(ang.points[2], ang.points[0]));
    let cos = v_dot(a1, a2) / (v_dot(a1, a1) * v_dot(a2, a2)).sqrt();
    assert!((cos.acos().to_degrees() - 90.0).abs() < 1e-6);
}

#[test]
fn shafts_only_when_unsolvable() {
    let x = Simple {
        kind: Some(Kind::Worm),
        shaft_angle: Some(90.0),
        offset: Some(5.0),
        pitch_diameter2: Some(100.0),
        ratio: Some(30.0),
        ..Simple::default()
    };
    let (s, sch) = schematic_of(&x, &Spec::default());
    assert!(s.error.is_some());
    assert_eq!(sch.members.len(), 2);
    assert!(sch.dims.iter().any(|d| d.kind == DimKind::ShaftAngle) && sch.dims.iter().any(|d| d.kind == DimKind::Offset));
}

#[test]
fn fast_enough() {
    // Defaults, every family: solve under 5 ms, schematic under 1 ms (a hypoid's schematic is
    // cached after its first call).
    for k in Kind::ALL.iter().copied() {
        let base = Spec::defaults_for(k);
        let x = outputs_of(&base);
        let n = 20;
        let t = Instant::now();
        for _ in 0..n {
            std::hint::black_box(solve(&x, &base));
        }
        let per_solve = t.elapsed().as_secs_f64() / f64::from(n);
        let _ = schematic(&base);
        let t = Instant::now();
        for _ in 0..n {
            std::hint::black_box(schematic(&base));
        }
        let per_sch = t.elapsed().as_secs_f64() / f64::from(n);
        println!("{:<16} solve {:>8.3} ms   schematic {:>8.3} ms", k.name(), 1e3 * per_solve, 1e3 * per_sch);
        assert!(per_solve < 5e-3, "{k:?} solve {} ms", 1e3 * per_solve);
        assert!(per_sch < 1e-3, "{k:?} schematic {} ms", 1e3 * per_sch);
    }
    // And the searches that do the most: a pair across a distance, a planetary set by size.
    let t = Instant::now();
    let x = Simple { ratio: Some(3.7), center_distance: Some(123.0), ..of(Kind::Pair) };
    for _ in 0..20 {
        std::hint::black_box(solve(&x, &Spec::defaults_for(Kind::Pair)));
    }
    let pair = t.elapsed().as_secs_f64() / 20.0;
    let t = Instant::now();
    let x = Simple { ratio: Some(5.3), ring_diameter: Some(180.0), planets: Some(3), ..of(Kind::Planetary) };
    for _ in 0..20 {
        std::hint::black_box(solve(&x, &Spec::defaults_for(Kind::Planetary)));
    }
    let planet = t.elapsed().as_secs_f64() / 20.0;
    println!("pair across 123 mm {:.3} ms, planetary by ring {:.3} ms", 1e3 * pair, 1e3 * planet);
    // A hypoid not seen before: its pitch cone design (cached after).
    let t = Instant::now();
    let x = Simple { offset: Some(23.7), ratio: Some(3.4), pitch_diameter2: Some(170.0), ..of(Kind::Hypoid) };
    let s = solve(&x, &Spec::defaults_for(Kind::Hypoid));
    let first = t.elapsed().as_secs_f64();
    let t = Instant::now();
    std::hint::black_box(schematic(&s.spec));
    println!("hypoid first solve {:.3} ms, then its schematic {:.3} ms", 1e3 * first, 1e3 * t.elapsed().as_secs_f64());
    assert!(pair < 5e-3 && planet < 5e-3);
}

#[test]
fn bevel_planetary_sets_by_their_outputs() {
    use crate::plan::BevelPlanetMode::{Differential, Ring, Single};
    let base = Spec::defaults_for(Kind::BevelPlanetary);
    // Meeting shafts with planets asked for: a bevel planetary set first.
    let meet = relation_of(90.0, 0.0);
    assert_eq!(auto_kind(&meet, &Simple { planets: Some(4), ..Simple::default() })[0].0, Kind::BevelPlanetary);
    assert_ne!(auto_kind(&meet, &Simple::default())[0].0, Kind::BevelPlanetary);
    // A differential at 90°: equal side gears, sized by the sun.
    let d = solved(
        &Simple {
            bevel_planet_mode: Some(Differential),
            shaft_angle: Some(90.0),
            ratio: Some(0.625),
            planets: Some(4),
            pitch_diameter: Some(48.0),
            ..of(Kind::BevelPlanetary)
        },
        &base,
    );
    assert_eq!(d.spec.teeth3, d.spec.teeth, "{:?}", d.chosen);
    assert!((d.spec.module * f64::from(d.spec.teeth) - 48.0).abs() < 0.05 * 48.0);
    assert_eq!((d.spec.teeth + d.spec.teeth3) % 4, 0);
    plan(&d.spec).unwrap();
    // A bevel epicyclic at 45°: the counts keep the cone angle near the one asked.
    let r = solved(
        &Simple {
            bevel_planet_mode: Some(Ring),
            shaft_angle: Some(45.0),
            ratio: Some(0.5),
            planets: Some(3),
            cone_distance: Some(40.0),
            ..of(Kind::BevelPlanetary)
        },
        &base,
    );
    assert!((row(&r, "cone angle") - 45.0).abs() < 2.0, "{:?}", r.chosen);
    assert!(r.spec.teeth3 > r.spec.teeth + r.spec.teeth2, "{:?}", r.spec);
    plan(&r.spec).unwrap();
    // A single sun, by the planet's size.
    let s = solved(
        &Simple {
            bevel_planet_mode: Some(Single),
            shaft_angle: Some(70.0),
            ratio: Some(0.6),
            planets: Some(3),
            pitch_diameter2: Some(24.0),
            ..of(Kind::BevelPlanetary)
        },
        &base,
    );
    assert_eq!(s.spec.teeth3, 0);
    plan(&s.spec).unwrap();
    // Switched to Simple, each solves to itself.
    for x in [&d.spec, &r.spec, &s.spec] {
        let again = solved(&outputs_of(x), x).spec;
        assert_eq!(
            (again.module, again.teeth, again.teeth2, again.teeth3, again.shaft_angle.round(), again.planets, again.bevel_planet_mode),
            (x.module, x.teeth, x.teeth2, x.teeth3, x.shaft_angle.round(), x.planets, x.bevel_planet_mode)
        );
    }
    // Switched to a ring from the 90° differential default, a workable cone angle is taken.
    let sw = solved(&Simple { bevel_planet_mode: Some(Ring), ..of(Kind::BevelPlanetary) }, &base);
    assert!(sw.spec.shaft_angle < 85.0 && sw.spec.teeth3 > 0, "{:?}", sw.chosen);
    assert!(sw.notes.iter().any(|n| n.contains("45°")), "{:?}", sw.notes);
    plan(&sw.spec).unwrap();
    // A ring is refused where the far side faces the sun's way, with why.
    let bad = solve(&Simple { bevel_planet_mode: Some(Ring), shaft_angle: Some(120.0), ratio: Some(0.6), ..of(Kind::BevelPlanetary) }, &base);
    assert!(bad.error.is_some());
}
