#![allow(clippy::unwrap_used, clippy::panic)]
use super::*;
use crate::find_profiles;

fn rounded(radius: f64) -> (Sketch, Vec<(String, bool)>) {
    let mut sk = Sketch::new();
    let p = positions(radius);
    let nodes: Vec<_> = p.iter().enumerate().map(|(i, p)| sk.add_point(*p, Some(&format!("source-p{i}"))).unwrap()).collect();
    let centres = [Vec2::new(37., -32.), Vec2::new(37., 32.), Vec2::new(-37., 32.), Vec2::new(-37., -32.)];
    let mut sources = Vec::new();
    for i in 0..8 {
        let id = format!("source-edge{i}");
        if i % 2 == 0 {
            sk.add_line_pts(nodes[i], nodes[(i + 1) % 8], Some(&id)).unwrap();
        } else {
            sk.add_arc(centres[i / 2], p[i], p[(i + 1) % 8], [None, Some(nodes[i]), Some(nodes[(i + 1) % 8])], Some(&id)).unwrap();
        }
        sources.push((id, true));
    }
    for point in &mut sk.points {
        point.fixed = true;
    }
    (sk, sources)
}
fn positions(r: f64) -> [Vec2; 8] {
    [
        Vec2::new(-37., -32. - r),
        Vec2::new(37., -32. - r),
        Vec2::new(37. + r, -32.),
        Vec2::new(37. + r, 32.),
        Vec2::new(37., 32. + r),
        Vec2::new(-37., 32. + r),
        Vec2::new(-37. - r, 32.),
        Vec2::new(-37. - r, -32.),
    ]
}
fn resize(sk: &mut Sketch, r: f64) {
    for (i, value) in positions(r).into_iter().enumerate() {
        sk.points.iter_mut().find(|p| p.id == format!("source-p{i}")).unwrap().pos = value;
    }
}
fn json(sk: &Sketch) -> String {
    serde_json::to_string(sk).unwrap()
}
fn area(sk: &Sketch, ids: &[String]) -> f64 {
    let mut only = sk.clone();
    only.curves.retain(|c| ids.contains(&c.id));
    let profiles = find_profiles(&only);
    assert_eq!(profiles.len(), 1);
    profiles[0].area
}

#[test]
fn convex_extinction_material_ring_and_identity_resurrection() {
    let (mut sk, sources) = rounded(1.);
    let source_points = sk.points.clone();
    let outer = sk.add_convex_offset(sources.clone(), 0.2).unwrap();
    let inner = sk.add_convex_offset(sources, 1.4).unwrap();
    assert_eq!(inner.curves.len(), 4);
    assert!((area(&sk, &inner.curves) - 4626.24).abs() < 1e-8);
    let ring_area = area(&sk, &outer.curves) - area(&sk, &inner.curves);
    assert!((ring_area - 332.570619298295).abs() < 1e-8);
    assert_eq!(&sk.points.as_slice()[..source_points.len()], source_points.as_slice());
    let topology = sk.offsets[1].topology.clone().unwrap();
    let nodes: Vec<_> = topology.roles.iter().map(|r| (r.start.clone(), r.end.clone())).collect();
    let reserved: Vec<_> = topology.roles.iter().map(|r| r.output.clone()).collect();
    assert_eq!(sk.offset_boundary_for_reserved_curves(&reserved).unwrap(), Some(inner.association.clone()));
    let saved = json(&sk);
    sk = serde_json::from_str(&saved).unwrap();
    sk.validate_offset_topologies().unwrap();
    resize(&mut sk, 2.);
    sk.refresh_offsets().unwrap();
    assert_eq!(sk.offset_boundary(&inner.association).unwrap(), reserved);
    assert_eq!(sk.offsets[1].topology.as_ref().unwrap().roles.iter().map(|r| (r.start.clone(), r.end.clone())).collect::<Vec<_>>(), nodes);
    assert!(sk.offset_boundary_for_reserved_curves(&inner.curves).unwrap().is_none());
    assert!((area(&sk, &reserved) - (4. * 37.6 * 32.6 - (4. - std::f64::consts::PI) * 0.6 * 0.6)).abs() < 1e-8);
    resize(&mut sk, 1.);
    sk.refresh_offsets().unwrap();
    assert_eq!(sk.offset_boundary(&inner.association).unwrap(), inner.curves);
}

#[test]
fn convex_legacy_upgrade_preserves_all_reserved_curve_ids() {
    let (mut sk, sources) = rounded(2.);
    let made = sk.add_convex_offset(sources, 1.4).unwrap();
    let nodes = sk.points.clone();
    sk.offsets[0].topology = None;
    resize(&mut sk, 1.);
    sk.refresh_offsets().unwrap();
    let topology = sk.offsets[0].topology.as_ref().unwrap();
    assert_eq!(topology.roles.iter().map(|r| r.output.clone()).collect::<Vec<_>>(), made.curves);
    assert_eq!(sk.points.len(), nodes.len());
    assert_eq!(sk.offsets[0].outputs.len(), 4);
    resize(&mut sk, 2.);
    sk.refresh_offsets().unwrap();
    assert_eq!(sk.offsets[0].outputs, made.curves);
}

#[test]
fn convex_break_detaches_arc_centres_and_preserves_released_geometry() {
    let (mut sk, sources) = rounded(2.);
    let made = sk.add_convex_offset(sources, 1.4).unwrap();
    sk.break_offset_topology(&made.association).unwrap();
    let output_points: BTreeSet<_> = sk.curves.iter().filter(|c| made.curves.contains(&c.id)).flat_map(|c| c.kind.point_ids()).collect();
    let frozen: Vec<_> = output_points.iter().map(|&p| (p, sk.points[p].pos)).collect();
    let source_points: BTreeSet<_> = sk.curves.iter().filter(|c| c.id.starts_with("source-edge")).flat_map(|c| c.kind.point_ids()).collect();
    assert!(output_points.is_disjoint(&source_points));
    for p in source_points {
        sk.points[p].pos += Vec2::new(7., -3.);
    }
    sk.refresh_offsets().unwrap();
    for (p, pos) in frozen {
        assert_eq!(sk.points[p].pos, pos);
    }
    assert!((area(&sk, &made.curves) - (4. * 37.6 * 32.6 - (4. - std::f64::consts::PI) * 0.36)).abs() < 1e-8);
}

#[test]
fn convex_clockwise_and_exact_radius_threshold_are_real_closed_regions() {
    let (mut sk, mut sources) = rounded(1.4);
    sources.reverse();
    for (_, forward) in &mut sources {
        *forward = false;
    }
    let made = sk.add_convex_offset(sources, -1.4).unwrap();
    assert_eq!(made.curves.len(), 4);
    assert!((area(&sk, &made.curves) - 4. * 37. * 32.).abs() < 1e-8);
}

#[test]
fn convex_retained_arc_dependencies_refuse_atomically() {
    let (mut sk, sources) = rounded(2.);
    let made = sk.add_convex_offset(sources, 1.4).unwrap();
    let arc = made.curves.iter().find(|id| matches!(sk.curves[sk.curve_index(id).unwrap()].kind, CurveKind::Arc { .. })).unwrap();
    let index = sk.curve_index(arc).unwrap();
    sk.add_constraint(crate::ConstraintKind::Radius { c: index, value: 0.6 }, None).unwrap();
    resize(&mut sk, 1.);
    let before = json(&sk);
    assert!(sk.refresh_offsets().unwrap_err().to_string().contains("constraint"));
    assert_eq!(json(&sk), before);
}

#[test]
fn convex_reserved_identity_and_counter_bounds_are_enforced() {
    let (mut sk, sources) = rounded(1.);
    sk.add_convex_offset(sources, 1.4).unwrap();
    let retired = sk.offsets[0].topology.as_ref().unwrap().roles.iter().find(|r| r.retired).unwrap().output.clone();
    assert!(sk.add_line(Vec2::new(1., 2.), Vec2::new(2., 3.), None, None, Some(&retired)).is_err());
    let mut stored = serde_json::to_value(&sk).unwrap();
    stored["counters"]["o"] = serde_json::json!(u64::MAX);
    sk = serde_json::from_value(stored).unwrap();
    let before = json(&sk);
    assert!(sk.fresh("o").is_err());
    assert_eq!(json(&sk), before);
    let mut invalid = sk.clone();
    invalid.offsets[0].topology.as_mut().unwrap().roles[0].start = "source-p0".into();
    assert!(invalid.validate_offset_topologies().is_err());
    let mut wide = Sketch::new();
    for i in 1..=2048 {
        wide.add_point(Vec2::new(i as f64, 1.), Some(&format!("o{i}"))).unwrap();
    }
    assert_eq!(wide.fresh("o").unwrap(), "o2049");
}

#[test]
fn convex_retired_arc_flags_survive_reopen_and_resurrection() {
    let (mut sk, sources) = rounded(2.);
    let made = sk.add_convex_offset(sources, 1.4).unwrap();
    let arc = made.curves.iter().find(|id| matches!(sk.curves[sk.curve_index(id).unwrap()].kind, CurveKind::Arc { .. })).unwrap().clone();
    let index = sk.curve_index(&arc).unwrap();
    let curve = &mut sk.curves[index];
    curve.construction = true;
    curve.centerline = true;
    curve.reversed = true;
    curve.fixed = true;
    resize(&mut sk, 1.);
    sk.refresh_offsets().unwrap();
    sk = serde_json::from_str(&json(&sk)).unwrap();
    sk.validate_offset_topologies().unwrap();
    resize(&mut sk, 2.);
    sk.refresh_offsets().unwrap();
    let curve = &sk.curves[sk.curve_index(&arc).unwrap()];
    assert!(curve.construction && curve.centerline && curve.reversed && curve.fixed);
}

#[test]
fn convex_dependency_preflight_and_disappearing_region_are_atomic() {
    let (mut sk, sources) = rounded(1.);
    let before = json(&sk);
    assert!(sk.add_convex_offset(sources.clone(), 40.).is_err());
    assert_eq!(json(&sk), before);
    sk.add_convex_offset(sources, 1.4).unwrap();
    sk.offsets.push(OffsetChain {
        sources: vec![("unrelated".into(), true); MAX_WORK],
        outputs: Vec::new(),
        distance: 1.,
        closed: false,
        topology: None,
    });
    assert!(sk.validate_offset_topologies().unwrap_err().to_string().contains("work limit"));
}

#[test]
fn convex_rotated_translated_support_is_independent_of_world_origin() {
    let (mut sk, sources) = rounded(1.);
    let angle: f64 = 0.317;
    let offset = Vec2::new(1e6, -2e6);
    let map = |p: Vec2| Vec2::new(p.x * angle.cos() - p.y * angle.sin(), p.x * angle.sin() + p.y * angle.cos()) + offset;
    for p in &mut sk.points {
        p.pos = map(p.pos);
    }
    let made = sk.add_convex_offset(sources, 1.4).unwrap();
    for curve in sk.curves.iter().filter(|c| made.curves.contains(&c.id)) {
        let CurveKind::Line { a, b } = curve.kind else {
            panic!("collapsed boundary should be straight");
        };
        for i in 0..=10 {
            let world = sk.points[a].pos * (1. - i as f64 / 10.) + sk.points[b].pos * (i as f64 / 10.);
            let q = world - offset;
            let local = Vec2::new(q.x * angle.cos() + q.y * angle.sin(), -q.x * angle.sin() + q.y * angle.cos());
            assert!(local.x.abs() <= 36.6 + 1e-8 && local.y.abs() <= 31.6 + 1e-8);
        }
    }
}
