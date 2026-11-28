//! How a plan becomes solids ([`crate::plan::solid`]): rigid members, their assembly, the tries a
//! step gets.

use crate::V2;
use crate::plan::solid::{NUDGES, assembly, attempts, members, mirrored_half};
use crate::plan::{AssemblyLink, Kind, Op, Spec, Step};
use crate::simple::Simple;
use crate::sketch_rules::{Circle, Drawn, Reading, read, train_spec};

fn circle(x: f64, y: f64, r: f64) -> Circle {
    Circle { c: V2::new(x, y), r }
}

/// A compound train: 1 drives 2; 2 and 3 share a shaft; 3 drives 4.
fn compound() -> crate::plan::Plan {
    let d = Drawn {
        points: vec![],
        circles: vec![circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0), circle(50.0, 0.0, 15.0), circle(50.0, 35.0, 20.0)],
        lines: vec![],
    };
    let Reading::Train { circles: picked } = read(Some(Kind::Spur), &d).unwrap_or_else(|e| panic!("{e}")) else { panic!("not read as a train") };
    let cs: Vec<Circle> = picked.iter().map(|i| d.circles[*i]).collect();
    let settings = Simple { module: Some(2.0), ..Simple::default() };
    let (spec, _) = train_spec(&cs, Kind::Spur, &settings, &Spec::defaults_for(Kind::Spur)).unwrap_or_else(|e| panic!("{e}"));
    crate::plan(&spec).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_cluster_is_one_rigid_member_and_one_assembly_member() {
    let p = compound();
    let rigid = members(&p);
    // Three parts turn: gear 1, the cluster 2+3, gear 4.
    assert_eq!(rigid.len(), 3, "{rigid:?}");
    let every: Vec<usize> = rigid.iter().flat_map(|r| r.bodies.clone()).collect();
    let mut sorted = every.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), every.len(), "a body in two members");
    assert_eq!(sorted, (0..p.bodies.len()).collect::<Vec<_>>(), "every body in a member");
    let a = assembly(&p, &rigid);
    assert_eq!(a.members.len(), 3);
    assert!(a.members.iter().enumerate().all(|(k, m)| m.body == k), "members are the rigid members in order");
    assert!(!a.links.iter().any(|l| matches!(l, AssemblyLink::Rigid { .. })), "no rigid link inside one body: {:?}", a.links);
    // Gear 4 turns by the teeth: (-20/30) * (-15/20) of gear 1.
    let mut speed = [0.0; 3];
    speed[0] = 1.0;
    for l in &a.links {
        let (x, y) = l.members();
        speed[y] = speed[x] * l.ratio();
    }
    assert!((speed[2] - (20.0 / 30.0) * (15.0 / 20.0)).abs() < 1e-9, "{speed:?}");
}

#[test]
fn a_set_without_clusters_is_a_member_per_body() {
    let p = crate::plan(&Spec::defaults_for(Kind::Pair)).unwrap_or_else(|e| panic!("{e}"));
    let rigid = members(&p);
    assert_eq!(rigid.iter().map(|r| r.bodies.clone()).collect::<Vec<_>>(), (0..p.bodies.len()).map(|b| vec![b]).collect::<Vec<_>>());
    assert_eq!(assembly(&p, &rigid), p.assembly, "nothing to collapse");
}

#[test]
fn trims_are_nudged_and_short_lofts_thinned() {
    let blank = Step::Revolve { profile: vec![V2::new(0.0, 0.0), V2::new(1.0, 0.0), V2::new(1.0, 1.0)], op: Op::Intersect };
    assert_eq!(attempts(&blank).iter().map(|(_, n)| *n).collect::<Vec<_>>(), NUDGES.to_vec());
    let o = crate::geom::Outline::circle(V2::ZERO, 5.0);
    let loft = Step::Loft { sections: (0..5).map(|k| (o.clone(), f64::from(k))).collect(), op: Op::New };
    let tries: Vec<usize> = attempts(&loft).iter().map(|(s, _)| if let Step::Loft { sections, .. } = s { sections.len() } else { 0 }).collect();
    assert_eq!(tries, vec![5, 3, 2]);
}

#[test]
fn only_a_herringbones_second_half_is_a_mirror() {
    let p = crate::plan(&Spec::defaults_for(Kind::Herringbone)).unwrap_or_else(|e| panic!("{e}"));
    let b = &p.bodies[0];
    assert!(mirrored_half(b, 1, &b.steps[1]).is_some(), "{:?}", b.steps.iter().map(std::mem::discriminant).collect::<Vec<_>>());
    assert!(mirrored_half(b, 0, &b.steps[0]).is_none());
}

#[test]
fn holes_go_with_the_outline_round_them() {
    use crate::geom::{Outline, Region};
    use crate::plan::solid::region_parts;
    let r = Region {
        outer: vec![Outline::circle(V2::ZERO, 10.0), Outline::circle(V2::new(50.0, 0.0), 10.0)],
        holes: vec![Outline::circle(V2::new(50.0, 0.0), 3.0), Outline::circle(V2::ZERO, 3.0)],
    };
    assert_eq!(region_parts(&r), vec![(0, vec![1]), (1, vec![0])]);
}

/// A train drawn in 3D: each circle (x, y, r) on the sketch plane at `z`, with its face sides
/// (module 2, 10 mm faces).
fn sided(circles: &[(f64, f64, f64, f64, Option<crate::plan::SidePreset>)]) -> Spec {
    use crate::sketch_rules::{TrainCircle, size_train};
    let cs: Vec<TrainCircle> =
        circles.iter().map(|&(x, y, r, z, sides)| TrainCircle { z: Some(z), sides, ..TrainCircle::of(circle(x, y, r)) }).collect();
    let settings = Simple { module: Some(2.0), face_width: Some(10.0), ..Simple::default() };
    let sized = size_train(&cs, Kind::Spur, &settings, &Spec::defaults_for(Kind::Spur));
    sized.spec.unwrap_or_else(|| panic!("{:?}", sized.error))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn face_sides_set_where_each_face_runs() {
    use crate::plan::SidePreset::{AOnly, BOnly, Custom, Symmetric};
    // (preset, side A, side B): the face runs from z − b to z + a about the sketch plane at 5.
    for (preset, a, b) in [
        (None, 10.0, 0.0),
        (Some(AOnly), 10.0, 0.0),
        (Some(Symmetric), 5.0, 5.0),
        (Some(BOnly), 0.0, 10.0),
        (Some(Custom { a: 3.0, b: 4.0 }), 3.0, 4.0),
    ] {
        let spec = sided(&[(0.0, 0.0, 20.0, 5.0, preset), (50.0, 0.0, 30.0, 5.0, preset)]);
        let t = spec.train.as_ref().unwrap();
        assert!(t.gears.iter().all(|g| close(g.z, 5.0) && g.span_of(&spec) == (5.0 - b, 5.0 + a)), "{preset:?}: {:?}", t.gears);
        let p = crate::plan(&spec).unwrap_or_else(|e| panic!("{preset:?}: {e}"));
        let oz = spec.origin[2];
        for (i, m) in p.members.iter().enumerate() {
            assert!(close(m.face_a, a) && close(m.face_b, b) && close(m.face_width, a + b), "{preset:?}: {m:?}");
            let body = &p.bodies[m.body.unwrap()];
            assert!(close(body.frame.origin[2] - oz, 5.0 - b), "{preset:?} gear {}: body starts at {:?}", i + 1, body.frame.origin);
            let shape = p.shapes.iter().find(|s| s.member == i).unwrap();
            assert!(close(shape.center[2] - oz, 5.0 - b) && close(shape.face, a + b), "{preset:?}: {shape:?}");
        }
        let c = &p.contacts[0];
        assert!(close(c.from[2] - oz, 5.0 - b) && close(c.to[2] - oz, 5.0 + a), "{preset:?}: {c:?}");
        assert!(!p.warnings.iter().any(|w| w.contains("share only")), "{preset:?}: {:?}", p.warnings);
    }
}

#[test]
fn a_compound_gear_with_a_b_side_gear_and_an_a_side_gear_is_one_body() {
    use crate::plan::SidePreset::{AOnly, BOnly};
    // 1 drives 2 on the plane at 0, both on side B; 3 shares 2's shaft on the plane at 12 and
    // drives 4 there, both on side A.
    let spec = sided(&[
        (0.0, 0.0, 20.0, 0.0, Some(BOnly)),
        (50.0, 0.0, 30.0, 0.0, Some(BOnly)),
        (50.0, 0.0, 15.0, 12.0, Some(AOnly)),
        (50.0, 35.0, 20.0, 12.0, Some(AOnly)),
    ]);
    let p = crate::plan(&spec).unwrap_or_else(|e| panic!("{e}"));
    let oz = spec.origin[2];
    assert_eq!(p.clusters, vec![vec![1, 2]]);
    let (m2, m3) = (&p.members[1], &p.members[2]);
    assert!(close(m2.face_a, 0.0) && close(m2.face_b, 10.0) && close(m3.face_a, 10.0) && close(m3.face_b, 0.0), "{m2:?} {m3:?}");
    // One body for the cluster, starting at gear 2's back face (−10); gear 3's teeth from 12.
    assert_eq!(m2.body, m3.body);
    let cluster = &p.bodies[m2.body.unwrap()];
    assert!(close(cluster.frame.origin[2] - oz, -10.0), "{:?}", cluster.frame);
    assert!(close(p.bodies[p.members[3].body.unwrap()].frame.origin[2] - oz, 12.0));
    let rigid = members(&p);
    assert_eq!(rigid.len(), 3, "{rigid:?}");
    assert_eq!(assembly(&p, &rigid).members.len(), 3);
    // Each mesh's contact runs across its own plane's faces.
    let along = |a: usize, b: usize| p.contacts.iter().find(|c| c.a == a && c.b == b).map(|c| (c.from[2] - oz, c.to[2] - oz)).unwrap();
    let (f, t) = along(0, 1);
    assert!(close(f, -10.0) && close(t, 0.0), "{f} {t}");
    let (f, t) = along(2, 3);
    assert!(close(f, 12.0) && close(t, 22.0), "{f} {t}");
}

#[test]
fn meshing_gears_whose_faces_dont_meet_are_refused() {
    use crate::plan::SidePreset::{AOnly, BOnly, Custom};
    // Gear 1 on side A (0 to 10), gear 2 on side B (−10 to 0): they only touch at the plane.
    let spec = sided(&[(0.0, 0.0, 20.0, 0.0, Some(AOnly)), (50.0, 0.0, 30.0, 0.0, Some(BOnly))]);
    let e = crate::plan(&spec).unwrap_err();
    assert!(e.contains("gears 1 and 2 mesh but their faces don't overlap") && e.contains("face sides"), "{e}");
    // Sharing 8 of 10 mm: built, with a warning.
    let spec = sided(&[(0.0, 0.0, 20.0, 0.0, Some(Custom { a: 8.0, b: 2.0 })), (50.0, 0.0, 30.0, 0.0, Some(AOnly))]);
    let p = crate::plan(&spec).unwrap_or_else(|e| panic!("{e}"));
    assert!(p.warnings.iter().any(|w| w.contains("gears 1 and 2 share only 8 mm of face")), "{:?}", p.warnings);
}

#[test]
fn gears_on_one_shaft_whose_faces_overlap_are_refused() {
    use crate::plan::SidePreset::{AOnly, BOnly};
    // Gear 2 reaches 0 to 10 on side A, gear 3 (on its shaft at 6) back to −4 on side B.
    let spec = sided(&[
        (0.0, 0.0, 20.0, 0.0, None),
        (50.0, 0.0, 30.0, 0.0, Some(AOnly)),
        (50.0, 0.0, 15.0, 6.0, Some(BOnly)),
        (50.0, 35.0, 20.0, 6.0, Some(BOnly)),
    ]);
    let e = crate::plan(&spec).unwrap_err();
    assert!(e.contains("gears 2 and 3 share a shaft but their faces overlap along it by 6 mm"), "{e}");
}
