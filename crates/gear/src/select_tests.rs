//! Gears on selected shafts, and bearings on them.

use crate::axes::Axis;
use crate::clearance::{Clearance, Seat};
use crate::mesh2d::{Verdict, check_mesh};
use crate::plan::{AssemblyLink, Kind, Spec};
use crate::select::{Ask, Shaft, bearing_point, set_on_shafts};

fn shaft(origin: [f64; 3], dir: [f64; 3], d: Option<f64>) -> Shaft {
    Shaft { axis: Axis::new(origin, dir).unwrap(), pitch_diameter: d, teeth: None }
}

fn clean(p: &crate::Plan) {
    let r = check_mesh(p);
    assert!(!r.is_empty());
    for m in &r {
        assert_eq!(m.verdict, Verdict::Ok, "{}", m.message);
    }
}

#[test]
fn one_shaft_and_a_size_make_a_gear_on_it() {
    let s = [shaft([10.0, 0.0, 0.0], [0.0, 0.0, 1.0], Some(40.0))];
    let out = set_on_shafts(&s, &Ask { along: 5.0, face_width: Some(8.0), ..Ask::default() }, &Spec::default()).unwrap();
    assert_eq!(out.spec.kind, Kind::Spur);
    assert!((out.spec.module * f64::from(out.spec.teeth) - 40.0).abs() < 1e-9, "{} × {}", out.spec.module, out.spec.teeth);
    let p = crate::plan(&out.spec).unwrap();
    let f = p.bodies[0].frame;
    assert!((f.origin[0] - 10.0).abs() < 1e-9 && f.origin[1].abs() < 1e-9 && (f.origin[2] - 5.0).abs() < 1e-9, "{f:?}");
    assert!((f.z[2] - 1.0).abs() < 1e-12);
    // Teeth with the size: the module that makes it.
    let s = [Shaft { teeth: Some(25), ..s[0] }];
    let out = set_on_shafts(&s, &Ask::default(), &Spec::default()).unwrap();
    assert_eq!(out.spec.teeth, 25);
    assert!((out.spec.module - 1.6).abs() < 1e-12);
}

#[test]
fn two_parallel_shafts_and_one_size_make_a_pair_filling_the_spacing() {
    let s = [shaft([0.0; 3], [0.0, 0.0, 1.0], Some(40.0)), shaft([50.0, 0.0, 0.0], [0.0, 0.0, 1.0], None)];
    let out = set_on_shafts(&s, &Ask::default(), &Spec::default()).unwrap();
    assert_eq!(out.spec.kind, Kind::Pair);
    assert!((out.spec.module * f64::from(out.spec.teeth) - 40.0).abs() < 1e-9);
    let d2 = out.spec.module * f64::from(out.spec.teeth2);
    assert!((d2 - 60.0).abs() < 1e-9, "{d2}");
    let p = crate::plan(&out.spec).unwrap();
    assert!((p.bodies[1].frame.origin[0] - 50.0).abs() < 1e-6, "{:?}", p.bodies[1].frame);
    clean(&p);
    // Helical teeth asked: a helical pair.
    let out = set_on_shafts(&s, &Ask { kind: Some(Kind::Helical), ..Ask::default() }, &Spec::default()).unwrap();
    assert_eq!((out.spec.kind, out.spec.helix), (Kind::Pair, 20.0));
    // Too big for the spacing.
    let big = [shaft([0.0; 3], [0.0, 0.0, 1.0], Some(120.0)), s[1]];
    assert!(set_on_shafts(&big, &Ask::default(), &Spec::default()).unwrap_err().contains("reaches past"));
}

#[test]
fn meeting_shafts_make_bevels_and_skew_ones_a_crossed_set() {
    let s = [shaft([0.0, 0.0, -40.0], [0.0, 0.0, 1.0], None), shaft([40.0, 0.0, 0.0], [1.0, 0.0, 0.0], None)];
    let out = set_on_shafts(&s, &Ask::default(), &Spec::default()).unwrap();
    assert_eq!(out.spec.kind, Kind::Bevel);
    assert!((out.spec.shaft_angle - 90.0).abs() < 1e-9);
    crate::plan(&out.spec).unwrap();
    let out = set_on_shafts(&s, &Ask { kind: Some(Kind::Helical), ..Ask::default() }, &Spec::default()).unwrap();
    assert_eq!(out.spec.kind, Kind::SpiralBevel);
    let skew = [shaft([0.0; 3], [0.0, 0.0, 1.0], None), shaft([0.0, 40.0, 0.0], [1.0, 0.0, 0.0], None)];
    let out = set_on_shafts(&skew, &Ask::default(), &Spec::default()).unwrap();
    assert!(matches!(out.spec.kind, Kind::Worm | Kind::CrossedHelical), "{:?}", out.spec.kind);
    crate::plan(&out.spec).unwrap();
    // A family asked for, where it fits.
    let out = set_on_shafts(&skew, &Ask { kind: Some(Kind::CrossedHelical), ..Ask::default() }, &Spec::default()).unwrap();
    assert_eq!(out.spec.kind, Kind::CrossedHelical);
}

#[test]
fn three_parallel_shafts_make_a_train_on_them() {
    // Shafts along world Y: gear 1 Ø40 given, the others fill the chain.
    let s = [
        shaft([0.0; 3], [0.0, 1.0, 0.0], Some(40.0)),
        shaft([50.0, 0.0, 0.0], [0.0, 1.0, 0.0], None),
        shaft([50.0, 3.0, 45.0], [0.0, 1.0, 0.0], Some(30.0)),
    ];
    let out = set_on_shafts(&s, &Ask { along: 2.0, ..Ask::default() }, &Spec::default()).unwrap();
    let t = out.spec.train.as_ref().unwrap();
    // The automatic module: 1.5, the smallest gear (Ø30) 20 teeth.
    assert_eq!(t.gears.iter().map(|g| g.teeth).collect::<Vec<_>>(), vec![27, 40, 20]);
    let p = crate::plan(&out.spec).unwrap();
    clean(&p);
    // Each gear on its shaft, 2 mm along shaft 1's direction from shaft 1's origin.
    for (b, want) in p.bodies.iter().zip([[0.0, 2.0, 0.0], [50.0, 2.0, 0.0], [50.0, 2.0, 45.0]]) {
        assert!((0..3).all(|i| (b.frame.origin[i] - want[i]).abs() < 1e-9), "{} at {:?}", b.name, b.frame.origin);
        assert!((b.frame.z[1] - 1.0).abs() < 1e-12);
    }
    assert!(p.facts.iter().any(|f| f.label == "train ratio gear 1 → gear 3"));
    // Shafts that aren't parallel.
    let bad = [s[0], s[1], shaft([0.0, 0.0, 80.0], [1.0, 0.0, 0.0], Some(20.0))];
    assert!(set_on_shafts(&bad, &Ask::default(), &Spec::default()).unwrap_err().contains("aren't parallel"));
    // No size at all.
    let none = [shaft([0.0; 3], [0.0, 1.0, 0.0], None), s[1], shaft([50.0, 0.0, 45.0], [0.0, 1.0, 0.0], None)];
    assert!(set_on_shafts(&none, &Ask::default(), &Spec::default()).unwrap_err().contains("at least one shaft's pitch diameter"));
}

#[test]
fn bearings_on_a_selected_shaft_are_seats_and_bodies() {
    let s = [shaft([0.0; 3], [0.0, 0.0, 1.0], Some(40.0)), shaft([50.0, 0.0, 0.0], [0.0, 0.0, 1.0], None)];
    let mut out = set_on_shafts(
        &s,
        &Ask::default(),
        &Spec { bore: crate::hub::Bore { kind: crate::hub::BoreKind::Round, diameter: 8.0, ..Default::default() }, ..Spec::default() },
    )
    .unwrap();
    // Two 608s on shaft 1, 20 mm either side of the gear's middle (5 mm up: face 10).
    let seats = vec![Seat::Point(bearing_point(&s[0].axis, -15.0)), Seat::Point(bearing_point(&s[0].axis, 25.0))];
    out.spec.clearance_body = Some(Clearance { bearing: Some("608".into()), seats, bearing_bodies: true, keep_out: false, ..Clearance::default() });
    let p = crate::plan(&out.spec).unwrap();
    assert!(p.clearance.is_none());
    assert_eq!(p.bearings.len(), 2);
    assert_eq!(p.bodies.len(), 2 + 4, "{:?}", p.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    let b = &p.bearings[0];
    assert_eq!((b.member, b.name.as_str()), (0, "608"));
    assert!((b.frame.origin[2] + 15.0).abs() < 1e-9 && b.frame.origin[0].abs() < 1e-9, "{:?}", b.frame);
    assert!(p.bodies[2].name.contains("Bearing 1 608 outer ring") && p.bodies[3].name.contains("inner ring"));
    // The outer ring held, the inner ring on gear 1's shaft.
    let a = &p.assembly;
    assert_eq!(a.members.len(), p.bodies.len());
    assert!(a.members[2].grounded && !a.members[3].grounded);
    assert!(a.links.contains(&AssemblyLink::Rigid { a: 0, b: 3 }));
    // With the keep-out body: its seats too.
    out.spec.clearance_body.as_mut().unwrap().keep_out = true;
    let p = crate::plan(&out.spec).unwrap();
    assert!(p.clearance.is_some());
    assert!(p.facts.iter().any(|f| f.label.starts_with("clearance: bearing seat 2")));
}
