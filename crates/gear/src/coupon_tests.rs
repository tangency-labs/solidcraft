//! Print test coupons: a few real teeth of each member at the true centre distance, as outlines
//! the kernel can build, that mesh as the whole set does.

use std::f64::consts::PI;

use crate::geom::{Outline, Seg, V2};
use crate::hub::{Bore, BoreKind};
use crate::mesh2d::{Verdict, check_mesh, worst};
use crate::plan::{Kind, Op, Plan, Spec, Step, plan};
use crate::sweep_tests::{Rng, check_outline, check_region, random_spec};

fn fact(p: &Plan, label: &str) -> f64 {
    p.facts
        .iter()
        .find(|f| f.label == label)
        .unwrap_or_else(|| panic!("no fact {label}: {:?}", p.facts.iter().map(|f| &f.label).collect::<Vec<_>>()))
        .value
}

fn coupon(s: Spec) -> Spec {
    Spec { coupon: true, ..s }
}

/// Every outline of every step and of the preview is one the kernel can sketch.
fn check_plan(p: &Plan) -> Result<(), String> {
    for b in &p.bodies {
        for (i, st) in b.steps.iter().enumerate() {
            // (Crowned teeth are a loft through a few sections.)
            if let Step::Loft { sections, .. } = st {
                if sections.len() < 2 {
                    return Err(format!("{}: step {i} lofts fewer than two sections", b.name));
                }
                for (o, _) in sections {
                    check_outline(o).map_err(|e| format!("{} step {i}: {e}", b.name))?;
                }
                continue;
            }
            let Step::Prism { region, height, .. } = st else { return Err(format!("{}: step {i} is not a prism or a loft", b.name)) };
            if !(*height > 0.0) || region.outer.is_empty() {
                return Err(format!("{}: step {i} is empty", b.name));
            }
            for o in region.outer.iter().chain(&region.holes) {
                check_outline(o).map_err(|e| format!("{} step {i}: {e}", b.name))?;
            }
        }
    }
    for g in &p.preview {
        check_region(&g.region).map_err(|e| format!("preview {}: {e}", g.name))?;
    }
    Ok(())
}

fn bounds(o: &Outline) -> (V2, V2) {
    let p = o.polyline(1e-3);
    p.iter().fold((p[0], p[0]), |(lo, hi), q| (V2::new(lo.x.min(q.x), lo.y.min(q.y)), V2::new(hi.x.max(q.x), hi.y.max(q.y))))
}

fn splines(o: &Outline) -> Vec<&Vec<V2>> {
    o.segs.iter().filter_map(|s| if let Seg::Spline(p) = s { Some(p) } else { None }).collect()
}

#[test]
fn a_pair_coupon_is_two_small_pieces_on_a_base() {
    let s = Spec::defaults_for(Kind::Pair);
    let full = plan(&s).unwrap();
    let p = plan(&coupon(s.clone())).unwrap();
    check_plan(&p).unwrap();
    assert_eq!(p.bodies.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["Coupon gear 1 z20", "Coupon gear 2 z32", "Coupon base"]);
    assert_eq!(p.preview.len(), 2);
    let a = fact(&full, "centre distance");
    // The pieces sit at the true centre distance, each with a pin hole on its gear's centre.
    assert_eq!(p.bodies[0].frame.origin, [0.0; 3]);
    assert!((p.bodies[1].frame.origin[0] - a).abs() < 1e-12);
    assert!((fact(&p, "coupon: gear 1 centre to gear 2 centre") - a).abs() < 1e-9);
    assert_eq!(fact(&p, "coupon gear 1: centre hole (inscribed diameter)"), 3.0);
    assert!(fact(&p, "coupon base: gear 1 peg diameter") < 3.0);
    for (k, g) in p.preview.iter().enumerate() {
        let (piece, gear) = (&g.region.outer[0], &full.preview[k].region.outer[0]);
        // A few teeth: four or five, a fraction of the gear.
        let n = fact(&p, &format!("coupon {}: teeth", g.name));
        assert!(n == 4.0 || n == 5.0);
        assert!(piece.area() > 0.0 && piece.area() < 0.45 * gear.area(), "{} of {}", piece.area(), gear.area());
        // The teeth are the gear's own, where they are on the gear: every flank of the piece
        // is a flank of the whole gear's outline.
        let (mine, theirs) = (splines(piece), splines(gear));
        assert_eq!(mine.len(), n as usize * 4);
        assert!(mine.iter().all(|sp| theirs.contains(sp)));
        // And they face the mate (a tooth or a space on the line of centres).
        let (lo, hi) = bounds(piece);
        let (reach, z) = (gear.max_radius(), f64::from(if k == 0 { s.teeth } else { s.teeth2 }));
        if k == 0 {
            assert!(hi.x > reach * (PI / z).cos() - 0.02 && lo.x > -0.5 * reach);
        } else {
            assert!(lo.x < -reach * (PI / z).cos() + 0.02 && hi.x < 0.5 * reach);
        }
        assert!((hi.y + lo.y).abs() < 1e-6, "centred on the line of centres: {lo:?} {hi:?}");
        assert_eq!(g.region.holes.len(), 1);
        assert_eq!((g.center, g.speed), (full.preview[k].center, full.preview[k].speed));
    }
    // They mesh as the whole gears do.
    let r = check_mesh(&p);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].verdict, Verdict::Ok, "{}", r[0].message);
    // The whole set's numbers are still there.
    assert_eq!(fact(&p, "contact ratio"), fact(&full, "contact ratio"));
    // The base: a plate below z = 0 with two pegs up through the face width.
    let base = &p.bodies[2];
    assert!(matches!(&base.steps[0], Step::Prism { z0, height, op: Op::New, .. } if *z0 < 0.0 && (z0 + height).abs() < 1e-12));
    assert!(
        matches!(&base.steps[1], Step::Prism { region, z0, height, op: Op::Join, .. } if region.outer.len() == 2 && *z0 == 0.0 && *height == s.face_width)
    );
    assert_eq!(base.steps.len(), 2);
}

#[test]
fn a_bore_becomes_the_coupons_centre_hole() {
    let s = Spec {
        bore: Bore { kind: BoreKind::Keyway, diameter: 8.0, ..Bore::default() },
        bore2: Bore { kind: BoreKind::Hex, diameter: 10.0, ..Bore::default() },
        ..Spec::defaults_for(Kind::Pair)
    };
    let p = plan(&coupon(s.clone())).unwrap();
    check_plan(&p).unwrap();
    assert_eq!(p.preview[0].region.holes[0], s.bore.outline().unwrap());
    assert_eq!(p.preview[1].region.holes[0], s.bore2.outline().unwrap());
    // The pegs are round, inside the bores.
    assert!((fact(&p, "coupon gear 1: centre hole (inscribed diameter)") - 8.0).abs() < 0.01);
    assert!(fact(&p, "coupon base: gear 2 peg diameter") < 10.0);
}

#[test]
fn a_large_gear_gets_a_block_with_a_flat_reference_edge() {
    let s = Spec { teeth: 18, teeth2: 120, ..Spec::defaults_for(Kind::Pair) };
    let full = plan(&s).unwrap();
    let p = plan(&coupon(s.clone())).unwrap();
    check_plan(&p).unwrap();
    let a = fact(&full, "centre distance");
    let at = fact(&p, "coupon gear 2: flat edge to the gear's centre");
    let root = fact(&full, "gear 2 root diameter") / 2.0;
    // A few modules below the roots.
    assert!(at < root - 3.0 * s.module + 1e-9 && at > root - 9.0 * s.module, "{at} against {root}");
    // The flat is a straight edge square to the line of centres, at that distance, on the far side
    // of the gear's teeth from its mate (gear 2's mate is towards −x).
    let block = &p.preview[1].region.outer[0];
    let flat = block.segs.iter().find(|s| matches!(s, Seg::Line(a, b) if (a.x - b.x).abs() < 1e-9)).expect("a flat");
    assert!((flat.start().x + at).abs() < 1e-9);
    let (lo, hi) = bounds(block);
    assert!((hi.x + at).abs() < 1e-6 && lo.x < -full.preview[1].region.outer[0].max_radius() * (PI / 120.0).cos() + 0.02);
    assert!(p.preview[1].region.holes.is_empty());
    // The size to set the pieces by: gear 1's pin to gear 2's flat.
    assert!((fact(&p, "coupon: gear 1 centre to gear 2 flat") - (a - at)).abs() < 1e-9);
    // The base has a peg for gear 1 and a fence behind gear 2's flat.
    let base = p.bodies.last().unwrap();
    assert_eq!(base.steps.len(), 3);
    let Step::Prism { region, .. } = &base.steps[2] else { panic!() };
    let (flo, fhi) = bounds(&region.outer[0]);
    assert!((flo.x - (a - at)).abs() < 1e-9 && (fhi.x - flo.x - 2.5 * s.module).abs() < 1e-9);
    assert_eq!(worst(&check_mesh(&p)), Some(Verdict::Ok));
    // Two large gears: across the flats.
    let p = plan(&coupon(Spec { teeth: 80, teeth2: 120, ..Spec::defaults_for(Kind::Pair) })).unwrap();
    check_plan(&p).unwrap();
    let across = fact(&p, "coupon: gear 1 flat to gear 2 flat");
    let want = fact(&p, "centre distance")
        - fact(&p, "coupon gear 1: flat edge to the gear's centre")
        - fact(&p, "coupon gear 2: flat edge to the gear's centre");
    assert!((across - want).abs() < 1e-9);
    assert_eq!(worst(&check_mesh(&p)), Some(Verdict::Ok));
}

#[test]
fn a_planetary_coupon_has_sun_planet_and_ring_pieces() {
    let s = Spec::defaults_for(Kind::Planetary);
    let full = plan(&s).unwrap();
    let p = plan(&coupon(s.clone())).unwrap();
    check_plan(&p).unwrap();
    assert_eq!(
        p.bodies.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
        ["Coupon sun z18", "Coupon planet 1 z18", "Coupon ring z54", "Coupon base"]
    );
    assert_eq!(p.preview.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["sun", "planet 1", "ring"]);
    assert_eq!(p.meshes.iter().map(|m| (m.a, m.b, m.internal)).collect::<Vec<_>>(), [(0, 1, false), (1, 2, true)]);
    // The planet shows teeth to the sun and to the ring.
    let planet = &p.preview[1].region.outer[0];
    let (lo, hi) = bounds(planet);
    let reach = full.preview[1].region.outer[0].max_radius();
    let near = reach * (PI / 18.0).cos() - 0.02;
    assert!(hi.x > near && lo.x < -near && hi.y < 0.8 * reach);
    assert_eq!(splines(planet).len() as f64, 2.0 * 4.0 * fact(&p, "coupon planet 1: teeth"));
    // The ring is a block outside its teeth with a flat back.
    let ring = &p.preview[2].region.outer[0];
    let at = fact(&p, "coupon ring: flat edge to the gear's centre");
    let (lo, hi) = bounds(ring);
    assert!((hi.x - at).abs() < 1e-9 && lo.x > 0.8 * fact(&full, "planet centre radius"));
    assert!((at - (fact(&p, "coupon: planet 1 centre to ring flat") + fact(&p, "planet centre radius"))).abs() < 1e-9);
    assert!(ring.area() < 0.2 * PI * at * at);
    let r = check_mesh(&p);
    assert_eq!(r.len(), 2);
    assert!(r.iter().all(|x| x.verdict == Verdict::Ok), "{r:?}");
}

#[test]
fn helical_coupons_are_cut_from_the_twisted_gear() {
    for s in [
        Spec { helix: 20.0, ..Spec::defaults_for(Kind::Pair) },
        Spec { helix: 20.0, teeth2: 90, ..Spec::defaults_for(Kind::Pair) },
        Spec { helix: 15.0, ..Spec::defaults_for(Kind::Planetary) },
    ] {
        let p = plan(&coupon(s.clone())).unwrap();
        check_plan(&p).unwrap();
        for b in &p.bodies[..p.bodies.len() - 1] {
            // The whole twisted gear, then the piece's shape kept, straight through.
            assert!(matches!(&b.steps[0], Step::Prism { twist, op: Op::New, .. } if *twist != 0.0), "{}", b.name);
            let masks: Vec<&Step> = b.steps.iter().filter(|st| matches!(st, Step::Prism { op: Op::Intersect, .. })).collect();
            assert_eq!(masks.len(), 1, "{}", b.name);
            assert!(matches!(masks[0], Step::Prism { twist, z0, height, .. } if *twist == 0.0 && *z0 < 0.0 && z0 + height > s.face_width));
        }
        assert_eq!(worst(&check_mesh(&p)), Some(Verdict::Ok));
    }
    // Spur coupons are plain prisms of their outline.
    let p = plan(&coupon(Spec::defaults_for(Kind::Pair))).unwrap();
    assert!(p.bodies.iter().flat_map(|b| &b.steps).all(|st| matches!(st, Step::Prism { twist, op, .. } if *twist == 0.0 && *op != Op::Intersect)));
}

#[test]
fn coupon_choices() {
    let s = coupon(Spec::defaults_for(Kind::Pair));
    // One piece at a time, without the base.
    let p = plan(&Spec { only: 2, ..s.clone() }).unwrap();
    assert_eq!(p.bodies.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["Coupon gear 2 z32"]);
    assert_eq!(p.preview.len(), 2);
    assert!(plan(&Spec { only: 3, ..s.clone() }).is_err());
    // More teeth, a named set.
    let p = plan(&Spec { coupon_teeth: 7, name: "Drive".into(), ..s.clone() }).unwrap();
    assert!(fact(&p, "coupon gear 1: teeth") >= 7.0);
    assert_eq!(p.bodies[0].name, "Drive coupon gear 1 z20");
    assert_eq!(p.bodies[2].name, "Drive coupon base");
    for n in [0, 2, 41] {
        assert!(plan(&Spec { coupon_teeth: n, ..s.clone() }).unwrap_err().contains("coupon teeth"));
    }
    // Hubs and lightening holes are left off; a set that can't be built says why as before.
    let p = plan(&Spec { hub_diameter: 12.0, hub_length: 5.0, holes: 4, ..s.clone() }).unwrap();
    assert_eq!(p.bodies[0].steps.len(), 2);
    assert!(plan(&Spec { teeth: 3, shift: -1.0, ..s.clone() }).is_err());
    // Tiny gears: the coupon is the gears themselves, with pin holes.
    let p = plan(&Spec { teeth: 5, teeth2: 6, ..s.clone() }).unwrap();
    check_plan(&p).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("whole set")), "{:?}", p.warnings);
    assert_eq!(fact(&p, "coupon gear 1: the whole gear, teeth"), 5.0);
    // Other kinds have no coupon: the set as usual, and a note.
    let p = plan(&coupon(Spec::defaults_for(Kind::Worm))).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("test coupon")));
    assert_eq!(p.bodies.len(), plan(&Spec::defaults_for(Kind::Worm)).unwrap().bodies.len());
    // Off by default.
    assert!(!Spec::default().coupon && Spec::default().coupon_teeth == 4);
}

#[test]
fn coupons_of_random_sets_are_buildable_and_mesh_like_the_set() {
    let mut r = Rng(0xC0FFEE);
    let (mut made, mut refused, mut compared) = (0, 0, 0);
    for i in 0..240 {
        let kind = if i % 2 == 0 { Kind::Pair } else { Kind::Planetary };
        let mut s = random_spec(&mut r, kind);
        s.coupon = true;
        s.coupon_teeth = r.int(3, 9);
        s.only = 0;
        let full = plan(&Spec { coupon: false, holes: 0, hub_diameter: 0.0, hub_length: 0.0, ..s.clone() });
        let p = plan(&s);
        assert_eq!(full.is_ok(), p.is_ok(), "{s:?}");
        let (Ok(full), Ok(p)) = (full, p) else {
            refused += 1;
            continue;
        };
        made += 1;
        check_plan(&p).unwrap_or_else(|e| panic!("case {i}: {e}\n{s:?}"));
        assert_eq!(p.preview.len(), if kind == Kind::Pair { 2 } else { 3 });
        // Where the whole set meshes cleanly, so does a coupon with teeth enough to carry the
        // contact through a pitch.
        let whole: Vec<Verdict> = check_mesh(&full).iter().take(2).map(|x| x.verdict).collect();
        if s.coupon_teeth >= 5 && whole.iter().all(|v| *v == Verdict::Ok) {
            compared += 1;
            let got = check_mesh(&p);
            assert!(got.iter().all(|x| x.verdict == Verdict::Ok), "case {i}: {got:?}\n{s:?}");
        }
    }
    eprintln!("coupons: {made} made, {refused} refused, {compared} mesh-checked against the whole set");
    assert!(made > 100 && compared > 40);
}
