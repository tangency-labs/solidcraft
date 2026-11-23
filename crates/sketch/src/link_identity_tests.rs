use super::*;
use crate::{ConstraintKind, solve};

fn square(size: f64) -> Vec<LinkGeom> {
    let pts = [Vec2::new(-size, -size), Vec2::new(size, -size), Vec2::new(size, size), Vec2::new(-size, size)];
    (0..4).map(|i| LinkGeom::Line(pts[i], pts[(i + 1) % 4])).collect()
}

fn face() -> LinkSource {
    LinkSource::Face { body: "Case".into(), at: Vec3::ZERO }
}

#[test]
fn named_projection_kind_changes_keep_ids_and_offset_direction_only_for_the_same_whole_edge() {
    for reverse in [false, true] {
        for change in ["same", "other edge", "split", "trimmed", "unnamed"] {
            let mut sk = Sketch::new();
            let mut points = vec![Vec2::new(1., 0.), Vec2::new(1., 1.), Vec2::new(0., 1.)];
            if reverse {
                points.reverse();
            }
            let saved = [LinkGeom::Spline {
                pts: points,
                control: true,
                degree: 2,
                periodic: false,
                nurbs: Some(crate::Nurbs { knots: vec![0., 0., 0., 1., 1., 1.], weights: vec![1., std::f64::consts::FRAC_1_SQRT_2, 1.] }),
            }];
            let link = sk.add_link(LinkKind::Project, LinkSource::Edge { body: "Cap".into(), at: Vec3::ZERO }, &saved).unwrap();
            if change != "unnamed" {
                sk.links[0].edge_name = Some("cap-edge".into());
            }
            let source = sk.curve_index("s1").unwrap();
            let made = sk.offset_freeform_chain(&[(source, true)], 0.2).unwrap();
            sk.record_freeform_chain_offset(&[(source, true)], &made, 0.2).unwrap();
            let outputs = sk.freeform_chain_offsets[0].outputs.clone();
            // A point of the sketch's own held on the saved spline (#20327 review).
            let held = sk.add_point(Vec2::new(std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2), None).unwrap();
            let held_id = sk.points[held].id.clone();
            sk.add_constraint(ConstraintKind::PointOnCurve { p: held, c: source }, None).unwrap();
            let arc = LinkGeom::Arc { c: Vec2::ZERO, a: Vec2::new(1., 0.), b: Vec2::new(0., 1.) };
            let (fresh, names) = match change {
                "split" => (
                    vec![
                        LinkGeom::Arc {
                            c: Vec2::ZERO,
                            a: Vec2::new(1., 0.),
                            b: Vec2::new(std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2),
                        },
                        LinkGeom::Arc {
                            c: Vec2::ZERO,
                            a: Vec2::new(std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2),
                            b: Vec2::new(0., 1.),
                        },
                    ],
                    vec!["cap-edge".into(), "cap-edge".into()],
                ),
                "trimmed" => (
                    vec![LinkGeom::Arc {
                        c: Vec2::ZERO,
                        a: Vec2::new(1., 0.),
                        b: Vec2::new(std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2),
                    }],
                    vec!["cap-edge".into()],
                ),
                "other edge" => (vec![arc], vec!["different-edge".into()]),
                _ => (vec![arc], vec!["cap-edge".into()]),
            };
            sk.update_link_named(&link, &fresh, &names).unwrap();
            assert_eq!(sk.freeform_chain_offsets.len(), 1, "a rebuild never silently breaks an offset");
            assert_eq!(sk.freeform_chain_offsets[0].outputs, outputs);
            if change == "same" {
                assert!(matches!(sk.curves[sk.curve_index("s1").unwrap()].kind, CurveKind::Arc { .. }));
                assert_eq!(sk.freeform_chain_offsets[0].sources, vec![("s1".into(), !reverse)]);
                let (held, s1) = (sk.point_index(&held_id).unwrap(), sk.curve_index("s1").unwrap());
                assert!(
                    sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::PointOnCurve { p, c } if p == held && c == s1)),
                    "a point held on the spline stays held on the arc it is again"
                );
                sk.refresh_offsets().unwrap();
                assert_eq!(sk.freeform_chain_offsets[0].outputs, outputs);
            } else {
                assert!(sk.curve_index("s1").is_none(), "{change} cannot claim the saved source");
                assert!(sk.refresh_offsets().unwrap_err().to_string().contains("source vanished"));
            }
        }
    }
}

fn ring(outer_first: bool, outer: f64, inner: f64) -> Vec<LinkGeom> {
    let (first, second) = if outer_first { (outer, inner) } else { (inner, outer) };
    square(first).into_iter().chain(square(second)).collect()
}

#[test]
fn saved_face_link_keeps_inner_ids_when_boundary_order_changes() {
    let mut sk = Sketch::new();
    // An older saved face may have enumerated its inner boundary first.
    let link = sk.add_link(LinkKind::Project, face(), &ring(false, 10.0, 8.0)).unwrap();
    let inner = sk.link_curves(&link)[0];
    let corner = match sk.curves[inner].kind {
        CurveKind::Line { a, .. } => a,
        _ => panic!(),
    };
    let free = sk.add_point(Vec2::new(-8.0, -8.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: free, q: corner }, None).unwrap();
    let serialized = serde_json::to_string(&sk).unwrap();
    let mut sk: Sketch = serde_json::from_str(&serialized).unwrap();
    let before = sk.clone();
    assert!(!sk.update_link(&link, &ring(true, 10.0, 8.0)).unwrap());
    assert_eq!(sk, before, "canonical outer-first enumeration must not reinterpret saved ids");
    assert!(solve(&mut sk).ok());
    assert_eq!(sk.points[free].pos, Vec2::new(-8.0, -8.0));
}

#[test]
fn resized_and_reordered_face_keeps_constraints_on_the_same_boundary() {
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &ring(false, 10.0, 8.0)).unwrap();
    let points = sk.link_points(&link);
    let original: Vec<Vec2> = points.iter().map(|p| sk.points[*p].pos).collect();
    let ids: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    let free = sk.add_point(Vec2::new(-8.0, -8.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: free, q: points[0] }, None).unwrap();
    let fresh: Vec<LinkGeom> = ring(true, 10.0, 8.0)
        .into_iter()
        .map(|g| match g {
            LinkGeom::Line(a, b) => LinkGeom::Line(Vec2::new(b.x * 2.0, b.y * 3.0), Vec2::new(a.x * 2.0, a.y * 3.0)),
            _ => panic!(),
        })
        .collect();
    assert!(!sk.update_link(&link, &fresh).unwrap());
    for (p, old) in points.iter().zip(original) {
        assert_eq!(sk.points[*p].pos, Vec2::new(old.x * 2.0, old.y * 3.0));
    }
    assert_eq!(ids, sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect::<Vec<_>>());
    assert!(solve(&mut sk).ok());
    assert!(sk.points[free].pos.dist(Vec2::new(-16.0, -24.0)) < 1e-7);
}

#[test]
fn ambiguous_face_refresh_preserves_last_geometry_and_marks_link_lost() {
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &ring(true, 10.0, 9.9)).unwrap();
    let before = sk.clone();
    assert!(sk.update_link(&link, &ring(false, 10.0, 2.0)).is_err());
    assert_eq!(sk.points, before.points);
    assert_eq!(sk.curves, before.curves);
    assert!(sk.link(&link).unwrap().lost);
}

#[test]
fn large_face_permutation_is_bounded_and_preserves_geometry() {
    let mut sk = Sketch::new();
    let geom: Vec<LinkGeom> = (1..=129).flat_map(|i| square(i as f64)).collect();
    let link = sk.add_link(LinkKind::Project, face(), &geom).unwrap();
    assert!(!sk.update_link(&link, &geom).unwrap(), "ordered unchanged geometry needs no permutation scan");
    let before = sk.clone();
    let reversed: Vec<LinkGeom> = (1..=129).rev().flat_map(|i| square(i as f64)).collect();
    assert!(sk.update_link(&link, &reversed).is_err());
    assert_eq!(sk.points, before.points);
    assert_eq!(sk.curves, before.curves);
}

#[test]
fn a_small_ordered_move_of_a_large_face_updates_points_without_losing_identity() {
    let mut sk = Sketch::new();
    let geom: Vec<_> = (1..=129).flat_map(|i| square(f64::from(i))).collect();
    let link = sk.add_link(LinkKind::Project, face(), &geom).unwrap();
    let points = sk.link_points(&link);
    let ids: Vec<_> = sk.link_curves(&link).iter().map(|i| sk.curves[*i].id.clone()).collect();
    let old: Vec<_> = points.iter().map(|i| sk.points[*i].pos).collect();
    let free = sk.add_point(Vec2::new(-129.0, -129.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: free, q: points[0] }, None).unwrap();
    let constraint = sk.constraints[0].clone();
    let shift = Vec2::new(1e-8, -1e-8);
    let fresh: Vec<_> = geom
        .iter()
        .map(|g| match g {
            LinkGeom::Line(a, b) => LinkGeom::Line(*a + shift, *b + shift),
            _ => panic!("line fixture"),
        })
        .collect();
    assert!(!sk.update_link(&link, &fresh).unwrap());
    assert!(!sk.link(&link).unwrap().lost);
    assert_eq!(points, sk.link_points(&link));
    assert_eq!(ids, sk.link_curves(&link).iter().map(|i| sk.curves[*i].id.clone()).collect::<Vec<_>>());
    assert_eq!(sk.constraints, vec![constraint]);
    for (i, p) in points.iter().zip(&old) {
        assert_eq!(sk.points[*i].pos, *p + shift, "the change must be applied, not ignored");
    }
    let moved = sk.clone();
    let large_move: Vec<_> = fresh
        .iter()
        .map(|g| match g {
            LinkGeom::Line(a, b) => LinkGeom::Line(*a + Vec2::X, *b + Vec2::X),
            _ => panic!("line fixture"),
        })
        .collect();
    assert!(sk.update_link(&link, &large_move).is_err());
    assert_eq!(sk.points, moved.points);
    assert_eq!(sk.curves, moved.curves);
}

#[test]
fn small_moves_of_large_faces_use_relative_scale_and_apply_the_edit() {
    for (offset, delta) in [(0.0, 1e-8), (1000.0, 1e-5), (100_000.0, 1e-3)] {
        let origin = Vec2::new(offset, offset);
        let geom: Vec<_> = (1..=129)
            .flat_map(|i| square(f64::from(i)))
            .map(|g| match g {
                LinkGeom::Line(a, b) => LinkGeom::Line(a + origin, b + origin),
                _ => panic!("line fixture"),
            })
            .collect();
        let mut sk = Sketch::new();
        let link = sk.add_link(LinkKind::Project, face(), &geom).unwrap();
        let points = sk.link_points(&link);
        let ids: Vec<_> = sk.link_curves(&link).iter().map(|i| sk.curves[*i].id.clone()).collect();
        let old: Vec<_> = points.iter().map(|i| sk.points[*i].pos).collect();
        let free = sk.add_point(origin, None).unwrap();
        sk.add_constraint(ConstraintKind::Coincident { p: free, q: points[0] }, None).unwrap();
        let constraint = sk.constraints[0].clone();
        let shift = Vec2::new(delta, -delta);
        let fresh: Vec<_> = geom
            .into_iter()
            .map(|g| match g {
                LinkGeom::Line(a, b) => LinkGeom::Line(a + shift, b + shift),
                _ => panic!("line fixture"),
            })
            .collect();
        assert!(!sk.update_link(&link, &fresh).unwrap(), "offset={offset}");
        assert!(!sk.link(&link).unwrap().lost);
        assert_eq!(points, sk.link_points(&link));
        assert_eq!(ids, sk.link_curves(&link).iter().map(|i| sk.curves[*i].id.clone()).collect::<Vec<_>>());
        assert_eq!(sk.constraints, vec![constraint]);
        for (id, old) in points.iter().zip(old) {
            assert_eq!(sk.points[*id].pos, old + shift, "offset={offset}: apply the geometry change");
        }
    }
}

#[test]
fn geometric_refresh_updates_conic_and_ellipse_scalar_values() {
    let mut sk = Sketch::new();
    let geom = |rho, minor| {
        vec![
            LinkGeom::Conic { a: Vec2::new(1.0, 1.0), apex: Vec2::new(2.0, 3.0), b: Vec2::new(4.0, 1.0), rho },
            LinkGeom::Ellipse { c: Vec2::new(10.0, 10.0), major: Vec2::new(14.0, 10.0), minor },
        ]
    };
    let link = sk.add_link(LinkKind::Project, LinkSource::Origin, &geom(0.4, 2.0)).unwrap();
    assert!(!sk.update_link(&link, &geom(0.6, 3.0)).unwrap());
    assert!(matches!(sk.curves[0].kind, CurveKind::Conic { rho, .. } if rho == 0.6));
    assert!(matches!(sk.curves[1].kind, CurveKind::Ellipse { r, .. } if r == 3.0));
}

#[test]
fn many_curves_sharing_few_points_do_not_enter_permutation_matching() {
    let mut sk = Sketch::new();
    let arc = |c: Vec2| LinkGeom::Arc { c, a: c + Vec2::X, b: c + Vec2::Y };
    let geom = vec![arc(Vec2::new(1.0, 1.0)); 513];
    let link = sk.add_link(LinkKind::Project, face(), &geom).unwrap();
    assert_eq!(sk.link_points(&link).len(), 3);
    let before = sk.clone();
    let shifted = vec![arc(Vec2::new(3.0, 3.0)); 513];
    assert!(sk.update_link(&link, &shifted).is_err());
    assert_eq!(sk.points, before.points);
    assert_eq!(sk.curves, before.curves);
    assert!(sk.link(&link).unwrap().lost);
}

#[test]
fn concentric_face_resize_keeps_inner_outer_circle_identity() {
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &[LinkGeom::Circle(Vec2::ZERO, 8.0), LinkGeom::Circle(Vec2::ZERO, 10.0)]).unwrap();
    let ids: Vec<String> = sk.link_curves(&link).iter().map(|i| sk.curves[*i].id.clone()).collect();
    assert!(!sk.update_link(&link, &[LinkGeom::Circle(Vec2::ZERO, 20.0), LinkGeom::Circle(Vec2::ZERO, 16.0)]).unwrap());
    assert!(matches!(sk.curves[0].kind, CurveKind::Circle { r, .. } if r == 16.0));
    assert!(matches!(sk.curves[1].kind, CurveKind::Circle { r, .. } if r == 20.0));
    assert_eq!(ids, sk.link_curves(&link).iter().map(|i| sk.curves[*i].id.clone()).collect::<Vec<_>>());
    // Coincident circles do not have a unique boundary identity.
    let before = sk.clone();
    assert!(sk.update_link(&link, &[LinkGeom::Circle(Vec2::ZERO, 18.0), LinkGeom::Circle(Vec2::ZERO, 18.0)]).is_err());
    assert_eq!(sk.points, before.points);
    assert_eq!(sk.curves, before.curves);
}

#[test]
fn resized_dense_rim_matches_arc_centres_by_role_before_proximity() {
    let mut sk = Sketch::new();
    let geometry = |half_width: f64, x: f64| {
        let mut out = square(half_width);
        let c = Vec2::new(x, -16.5);
        let left = c - Vec2::X * 3.0;
        let right = c + Vec2::X * 3.0;
        out.push(LinkGeom::Arc { c, a: right, b: left });
        out.push(LinkGeom::Arc { c, a: left, b: right });
        out
    };
    let link = sk.add_link(LinkKind::Project, face(), &geometry(38.0, -26.5)).unwrap();
    let arcs: Vec<usize> = sk.link_curves(&link).into_iter().filter(|i| matches!(sk.curves[*i].kind, CurveKind::Arc { .. })).collect();
    let ids: Vec<String> = arcs.iter().map(|i| sk.curves[*i].id.clone()).collect();
    // The old centre is spatially closer in normalized space to a fresh endpoint.
    // Both semicircles must still keep the same shared centre and logical endpoints.
    assert!(!sk.update_link(&link, &geometry(43.0, -31.5)).unwrap());
    for i in &arcs {
        let CurveKind::Arc { c, a, b } = sk.curves[*i].kind else { panic!() };
        assert_eq!(sk.points[c].pos, Vec2::new(-31.5, -16.5));
        assert!((sk.points[a].pos.dist(sk.points[c].pos) - 3.0).abs() < 1e-9);
        assert!((sk.points[b].pos.dist(sk.points[c].pos) - 3.0).abs() < 1e-9);
    }
    assert_eq!(ids, arcs.iter().map(|i| sk.curves[*i].id.clone()).collect::<Vec<_>>());
}

#[test]
fn nested_face_resize_matches_each_loop_before_nearby_outer_vertices() {
    let rectangle = |x: f64, y: f64| {
        let p = [Vec2::new(-x, -y), Vec2::new(x, -y), Vec2::new(x, y), Vec2::new(-x, y)];
        (0..4).map(|i| LinkGeom::Line(p[i], p[(i + 1) % 4])).collect::<Vec<_>>()
    };
    let mut sk = Sketch::new();
    let old: Vec<_> = rectangle(38.0, 28.0).into_iter().chain(rectangle(40.0, 30.0)).collect();
    let link = sk.add_link(LinkKind::Project, face(), &old).unwrap();
    let ids: Vec<_> = sk.link_curves(&link).iter().map(|i| sk.curves[*i].id.clone()).collect();
    let points = sk.link_points(&link);
    let before: Vec<_> = points.iter().map(|i| sk.points[*i].pos).collect();
    let fresh: Vec<_> = rectangle(41.0, 36.0).into_iter().chain(rectangle(38.0, 33.0)).collect();
    assert!(!sk.update_link(&link, &fresh).unwrap());
    assert!(!sk.link(&link).unwrap().lost);
    for (i, p) in points.iter().zip(before) {
        let expected = if p.x.abs() == 38.0 { Vec2::new(p.x, p.y.signum() * 33.0) } else { Vec2::new(p.x.signum() * 41.0, p.y.signum() * 36.0) };
        assert_eq!(sk.points[*i].pos, expected);
    }
    assert_eq!(ids, sk.link_curves(&link).iter().map(|i| sk.curves[*i].id.clone()).collect::<Vec<_>>());
}

#[test]
fn growing_face_retains_stationary_hole_arcs_and_shared_centre() {
    let geometry = |width: f64| {
        let p = [Vec2::ZERO, Vec2::new(width, 0.0), Vec2::new(width, 30.0), Vec2::new(0.0, 30.0)];
        let mut out: Vec<_> = (0..4).map(|i| LinkGeom::Line(p[i], p[(i + 1) % 4])).collect();
        let c = Vec2::new(20.0, 15.0);
        out.extend([
            LinkGeom::Arc { c, a: c + Vec2::X * 5.0, b: c - Vec2::X * 5.0 },
            LinkGeom::Arc { c, a: c - Vec2::X * 5.0, b: c + Vec2::X * 5.0 },
        ]);
        out
    };
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &geometry(40.0)).unwrap();
    let saved = sk.clone();
    assert!(!sk.update_link(&link, &geometry(50.0)).unwrap());
    assert!(!sk.link(&link).unwrap().lost);
    for i in sk.link_curves(&link) {
        assert_eq!(sk.curves[i].id, saved.curves[i].id);
        if matches!(sk.curves[i].kind, CurveKind::Arc { .. }) {
            assert_eq!(sk.curves[i], saved.curves[i]);
            for p in sk.curves[i].kind.point_ids() {
                assert_eq!(sk.points[p], saved.points[p]);
            }
        }
    }
}

#[test]
fn touching_or_crossed_face_boundaries_refuse_without_retargeting_ids() {
    for x in [9.0, 11.0] {
        let mut sk = Sketch::new();
        let link = sk.add_link(LinkKind::Project, face(), &[LinkGeom::Circle(Vec2::ZERO, 10.0), LinkGeom::Circle(Vec2::ZERO, 1.0)]).unwrap();
        let saved = sk.clone();
        assert!(sk.update_link(&link, &[LinkGeom::Circle(Vec2::ZERO, 10.0), LinkGeom::Circle(Vec2::new(x, 0.0), 1.0)]).is_err());
        assert_eq!(sk.points, saved.points);
        assert_eq!(sk.curves, saved.curves);
        assert!(sk.link(&link).unwrap().lost);
    }
}

#[test]
fn face_with_two_inner_loops_meeting_at_one_corner_projects_and_refreshes() {
    // A plate with a block and a pad joined on it: the pad's corner meets the block's corner
    // (#20619). Both footprints are inner loops that share one vertex.
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        let p = [Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)];
        (0..4).map(|i| LinkGeom::Line(p[i], p[(i + 1) % 4])).collect::<Vec<_>>()
    };
    let touching = |pad_x: f64| -> Vec<LinkGeom> {
        let mut g = rect(0.0, 0.0, 50.0, 40.0);
        g.extend(rect(15.0, 10.0, 30.0, 25.0));
        g.extend(rect(pad_x, 25.0, pad_x + 6.0, 31.0));
        g
    };
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &touching(30.0)).unwrap();
    assert_eq!(sk.link_curves(&link).len(), 12);
    // The corner is one shared point: 11 points, not 12.
    assert_eq!(sk.link_points(&link).len(), 11);
    // Re-reading the same face is not refused and keeps the link current.
    assert!(!sk.update_link(&link, &touching(30.0)).unwrap());
    assert!(!sk.link(&link).unwrap().lost);
    // A pad that overlaps the block instead of touching it still refuses.
    let saved = sk.clone();
    assert!(sk.update_link(&link, &touching(29.0)).is_err());
    assert_eq!(sk.points, saved.points);
    assert_eq!(sk.curves, saved.curves);
    assert!(sk.link(&link).unwrap().lost);
}

#[test]
fn resized_rounded_rim_retains_inner_outer_roles_with_shared_arc_centres() {
    let rounded = |x: f64, y: f64, r: f64| {
        let centres = [Vec2::new(x - r, -y + r), Vec2::new(x - r, y - r), Vec2::new(-x + r, y - r), Vec2::new(-x + r, -y + r)];
        let mut out = Vec::new();
        for i in 0..4 {
            let start = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::FRAC_PI_2;
            let previous = centres[(i + 3) % 4] + Vec2::from_angle(start) * r;
            let a = centres[i] + Vec2::from_angle(start) * r;
            let b = centres[i] + Vec2::from_angle(start + std::f64::consts::FRAC_PI_2) * r;
            out.push(LinkGeom::Line(previous, a));
            out.push(LinkGeom::Arc { c: centres[i], a, b });
        }
        out
    };
    let mut sk = Sketch::new();
    let old: Vec<_> = rounded(38.0, 28.0, 2.0).into_iter().chain(rounded(40.0, 30.0, 4.0)).collect();
    let link = sk.add_link(LinkKind::Project, face(), &old).unwrap();
    let curves = sk.link_curves(&link);
    let ids: Vec<_> = curves.iter().map(|i| sk.curves[*i].id.clone()).collect();
    let fresh: Vec<_> = rounded(41.0, 36.0, 4.0).into_iter().chain(rounded(38.0, 33.0, 1.0)).collect();
    assert!(!sk.update_link(&link, &fresh).unwrap());
    assert!(!sk.link(&link).unwrap().lost);
    assert_eq!(ids, sk.link_curves(&link).iter().map(|i| sk.curves[*i].id.clone()).collect::<Vec<_>>());
    for (old, fresh) in curves.iter().zip(rounded(38.0, 33.0, 1.0).into_iter().chain(rounded(41.0, 36.0, 4.0))) {
        match (&sk.curves[*old].kind, fresh) {
            (CurveKind::Line { a, b }, LinkGeom::Line(p, q)) => {
                assert!(sk.points[*a].pos.dist(p) < 1e-10);
                assert!(sk.points[*b].pos.dist(q) < 1e-10);
            }
            (CurveKind::Arc { c, a, b }, LinkGeom::Arc { c: p, a: q, b: r }) => {
                assert!(sk.points[*c].pos.dist(p) < 1e-10);
                assert!(sk.points[*a].pos.dist(q) < 1e-10);
                assert!(sk.points[*b].pos.dist(r) < 1e-10);
            }
            _ => panic!(),
        }
    }
}

#[test]
fn far_translated_small_hole_crossing_long_edge_is_not_certified_separate() {
    let mut sk = Sketch::new();
    let p = [Vec2::new(-1e8, 1.0), Vec2::new(1e8, 1.0), Vec2::new(1e8, 10.0), Vec2::new(-1e8, 10.0)];
    let mut old: Vec<_> = (0..4).map(|i| LinkGeom::Line(p[i], p[(i + 1) % 4])).collect();
    old.push(LinkGeom::Circle(Vec2::new(1e6, 5.0), 2.0));
    let link = sk.add_link(LinkKind::Project, face(), &old).unwrap();
    let saved = sk.clone();
    let mut fresh: Vec<_> = (0..4).map(|i| LinkGeom::Line(p[i], p[(i + 1) % 4])).collect();
    fresh.push(LinkGeom::Circle(Vec2::new(1e6, 0.0), 2.0));
    assert!(sk.update_link(&link, &fresh).is_err());
    assert_eq!(sk.points, saved.points);
    assert_eq!(sk.curves, saved.curves);
    assert!(sk.link(&link).unwrap().lost);
}

#[test]
fn equal_nested_sibling_candidates_remain_ambiguous_after_loop_classification() {
    let mut sk = Sketch::new();
    let link = sk
        .add_link(
            LinkKind::Project,
            face(),
            &[LinkGeom::Circle(Vec2::ZERO, 10.0), LinkGeom::Circle(Vec2::new(-2.0, 0.0), 1.0), LinkGeom::Circle(Vec2::new(2.0, 0.0), 1.0)],
        )
        .unwrap();
    let saved = sk.clone();
    assert!(
        sk.update_link(
            &link,
            &[LinkGeom::Circle(Vec2::ZERO, 10.0), LinkGeom::Circle(Vec2::new(0.0, -2.0), 1.0), LinkGeom::Circle(Vec2::new(0.0, 2.0), 1.0)]
        )
        .is_err()
    );
    assert_eq!(sk.points, saved.points);
    assert_eq!(sk.curves, saved.curves);
    assert!(sk.link(&link).unwrap().lost);
}

#[test]
fn tiny_circle_crossing_huge_round_boundary_is_not_certified_separate() {
    let mut sk = Sketch::new();
    let direction = Vec2::new(std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2);
    let link = sk.add_link(LinkKind::Project, face(), &[LinkGeom::Circle(Vec2::ZERO, 1e8), LinkGeom::Circle(direction * (1e8 + 10.0), 2.0)]).unwrap();
    let saved = sk.clone();
    assert!(sk.update_link(&link, &[LinkGeom::Circle(Vec2::ZERO, 1e8), LinkGeom::Circle(direction * (1e8 + 1.0), 2.0)]).is_err());
    assert_eq!(sk.points, saved.points);
    assert_eq!(sk.curves, saved.curves);
    assert!(sk.link(&link).unwrap().lost);
}

#[test]
fn huge_loop_does_not_make_nearby_bowtie_edges_topologically_adjacent() {
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &square(10.0)).unwrap();
    let saved = sk.clone();
    let p = [
        Vec2::ZERO,
        Vec2::new(0.04, 0.04),
        Vec2::new(0.0, 0.04),
        Vec2::new(0.04, 0.0),
        Vec2::new(9e8, 0.0),
        Vec2::new(9e8, 10.0),
        Vec2::new(0.0, 10.0),
    ];
    let crossed: Vec<_> = (0..p.len()).map(|i| LinkGeom::Line(p[i], p[(i + 1) % p.len()])).collect();
    assert!(sk.update_link(&link, &crossed).is_err());
    assert_eq!(sk.points, saved.points);
    assert_eq!(sk.curves, saved.curves);
    assert!(sk.link(&link).unwrap().lost);
}

#[test]
fn edge_on_face_lays_out_as_one_silhouette_line_and_stays_current() {
    let mut sk = Sketch::new();
    // A vertical wall seen from above: its top and bottom edges project onto the same segment,
    // its vertical edges onto points.
    let (a, b) = (Vec2::new(40.0, 0.0), Vec2::new(40.0, 30.0));
    let wall = vec![LinkGeom::Line(a, b), LinkGeom::Line(b, b), LinkGeom::Line(b, a), LinkGeom::Line(a, a)];
    let link = sk.add_link(LinkKind::Project, face(), &wall).unwrap();
    assert_eq!(sk.link_curves(&link).len(), 1);
    assert!(!sk.update_link(&link, &wall).unwrap(), "the same silhouette keeps its curve");
    assert!(!sk.link(&link).unwrap().lost);
    // A notched wall seen edge-on: overlapping pieces along one line, still one line end to end.
    let notched = [(0.0, 12.0), (8.0, 20.0), (18.0, 30.0), (5.0, 25.0)]
        .into_iter()
        .flat_map(|(y0, y1)| [LinkGeom::Line(Vec2::new(40.0, y0), Vec2::new(40.0, y1)), LinkGeom::Line(Vec2::new(40.0, y1), Vec2::new(40.0, y0))])
        .collect::<Vec<_>>();
    assert!(!sk.update_link(&link, &notched).unwrap(), "the same line keeps its curve");
    let [ci] = sk.link_curves(&link)[..] else { panic!("one line") };
    let CurveKind::Line { a, b } = sk.curves[ci].kind else { panic!("a line") };
    let ends = [sk.points[a].pos, sk.points[b].pos];
    assert!(ends.contains(&Vec2::new(40.0, 0.0)) && ends.contains(&Vec2::new(40.0, 30.0)), "{ends:?}");
    assert!(!sk.link(&link).unwrap().lost);
    // Turned to face the sketch, the wall is its rectangle again.
    assert!(sk.update_link(&link, &square(10.0)).unwrap());
    assert_eq!(sk.link_curves(&link).len(), 4);
    assert!(!sk.link(&link).unwrap().lost);
}

/// A face with a hole cut through the hole by the sketch plane: its section is two pieces on
/// one line, one each side of the hole. Only a projection seen edge-on is one line end to end;
/// a section keeps the gap (#14282).
#[test]
fn intersected_face_with_a_hole_keeps_both_pieces_on_one_line() {
    // A pipe's top ring (bore 10, outside 22) cut through its axis.
    let section = vec![LinkGeom::Line(Vec2::new(-22.0, 40.0), Vec2::new(-10.0, 40.0)), LinkGeom::Line(Vec2::new(10.0, 40.0), Vec2::new(22.0, 40.0))];
    let ends = |sk: &Sketch, link: &str| -> Vec<[Vec2; 2]> {
        sk.link_curves(link)
            .iter()
            .map(|c| match sk.curves[*c].kind {
                CurveKind::Line { a, b } => [sk.points[a].pos, sk.points[b].pos],
                _ => panic!("a line"),
            })
            .collect()
    };
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Intersect, face(), &section).unwrap();
    let pieces = [[Vec2::new(-22.0, 40.0), Vec2::new(-10.0, 40.0)], [Vec2::new(10.0, 40.0), Vec2::new(22.0, 40.0)]];
    assert_eq!(ends(&sk, &link), pieces, "one line each side of the hole");
    assert!(!sk.update_link(&link, &section).unwrap(), "the same section keeps its curves");
    assert_eq!(ends(&sk, &link), pieces);
    assert!(!sk.link(&link).unwrap().lost);
    // Intersecting the same face again finds both pieces already there, so nothing is added.
    assert!(sk.link_geometry_or_points_present(&section), "the section is already linked");
    // Projected, the same ring seen edge-on is still one line end to end.
    let projected = sk.add_link(LinkKind::Project, face(), &section).unwrap();
    let [[a, b]] = ends(&sk, &projected)[..] else { panic!("one line") };
    assert!([a, b].contains(&Vec2::new(-22.0, 40.0)) && [a, b].contains(&Vec2::new(22.0, 40.0)), "{a:?} {b:?}");
}

/// A conic that comes with a link (projected geometry) takes the curve prefix `n` like a drawn
/// one, not the constraints' `k` (#6173).
#[test]
fn linked_conic_curves_take_the_n_prefix_not_the_constraints_k() {
    let mut sk = Sketch::new();
    let geom = vec![
        LinkGeom::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),
        LinkGeom::Conic { a: Vec2::new(10.0, 0.0), apex: Vec2::new(15.0, 4.0), b: Vec2::new(20.0, 0.0), rho: 0.5 },
    ];
    let link = sk.add_link(LinkKind::Project, LinkSource::Origin, &geom).unwrap();
    let ids: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(ids, vec!["l1".to_string(), "n1".to_string()], "linked curve ids");
}

/// A face seen edge-on was saved as two lines on the same two points, one each way, and is one
/// line now. Re-resolving it must not give the first line a new id: an offset names it as its
/// source, and lost it ("an offset source vanished or changed shape").
#[test]
fn edge_on_face_saved_as_a_line_pair_keeps_the_line_an_offset_follows() {
    let (a, b) = (Vec2::new(5.0, 5.0), Vec2::new(15.0, 5.0));
    let mut sk = Sketch::new();
    // `add_link` makes one line of a reversed pair, so lay the pair out the way a saved file has it.
    let link = "j1".to_string();
    let (pa, pb) = (sk.add_point(a, None).unwrap(), sk.add_point(b, None).unwrap());
    for (from, to, id) in [(pa, pb, "l1"), (pb, pa, "l2")] {
        let c = sk.add_line_pts(from, to, Some(id)).unwrap();
        sk.curves[c].link = Some(link.clone());
    }
    for point in [pa, pb] {
        sk.points[point].link = Some(link.clone());
    }
    sk.links.push(Link {
        sketch_placements: Vec::new(),
        id: link.clone(),
        kind: LinkKind::Project,
        source: face(),
        face_name: None,
        edge_name: None,
        vertex_name: None,
        curve_edges: Vec::new(),
        profile_boundary: Vec::new(),
        auto_face: false,
        lost: false,
        name_dropped: false,
    });
    let ids: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(ids.len(), 2, "the saved link holds the pair");
    let (p, q) = (sk.add_point(Vec2::new(5.0, 3.0), None).unwrap(), sk.add_point(Vec2::new(15.0, 3.0), None).unwrap());
    let out = sk.add_line_pts(p, q, None).unwrap();
    let out_id = sk.curves[out].id.clone();
    sk.offsets.push(crate::offset::OffsetChain {
        sources: vec![(ids[0].clone(), true)],
        outputs: vec![out_id],
        distance: -2.0,
        closed: false,
        topology: None,
    });
    sk.refresh_offsets().unwrap();

    sk.update_link(&link, &[LinkGeom::Line(a, Vec2::new(17.0, 5.0))]).unwrap();

    let kept: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(kept, vec![ids[0].clone()], "the first line keeps its id");
    sk.refresh_offsets().unwrap();
    let end = sk.points[sk.link_points(&link)[1]].pos;
    assert_eq!(end, Vec2::new(17.0, 5.0), "the line moved to the new geometry");
}

/// #5388: a circle the user drew on a projected hole's centre survives when the face changes
/// shape and its projection is rebuilt (a second hole added to the face). It stays on that
/// hole's centre while the hole is there, and on a free point where it was once it is gone.
#[test]
fn rebuilt_face_keeps_a_users_circle_on_its_projected_centre() {
    let mut sk = Sketch::new();
    let hole = |c: Vec2| LinkGeom::Circle(c, 2.1);
    let face_with = |holes: &[Vec2]| square(15.0).into_iter().chain(holes.iter().map(|c| hole(*c))).collect::<Vec<_>>();
    let link = sk.add_link(LinkKind::Project, face(), &face_with(&[Vec2::ZERO])).unwrap();
    // The projected hole's centre point.
    let centre = sk
        .link_curves(&link)
        .into_iter()
        .find_map(|i| match sk.curves[i].kind {
            CurveKind::Circle { c, .. } => Some(c),
            _ => None,
        })
        .unwrap();
    let mine = sk.add_curve(CurveKind::Circle { c: centre, r: 5.25 }, Some("c_mine")).unwrap();
    assert!(sk.curves[mine].link.is_none());
    // The face gains two more holes: the projection's shape changes, so it is rebuilt.
    assert!(sk.update_link(&link, &face_with(&[Vec2::ZERO, Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0)])).unwrap());
    let centre_of_mine = |sk: &Sketch| {
        let kept = sk.curves.iter().find(|c| c.id == "c_mine").expect("the user's circle survives the rebuild");
        assert!(kept.link.is_none());
        match kept.kind {
            CurveKind::Circle { c, r } => {
                assert_eq!(r, 5.25);
                assert!(sk.points[c].pos.len() < 1e-9, "still at the hole's centre: {:?}", sk.points[c].pos);
                c
            }
            ref k => panic!("not a circle: {k:?}"),
        }
    };
    let c = centre_of_mine(&sk);
    assert_eq!(sk.points[c].link.as_deref(), Some(link.as_str()), "it stays on the hole's projected centre");
    assert_eq!(sk.link_curves(&link).len(), 4 + 3, "the projection is the new outline and holes");
    // The hole it was drawn on goes: the circle stays where it was, on a point of its own.
    assert!(sk.update_link(&link, &face_with(&[Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0)])).unwrap());
    let c = centre_of_mine(&sk);
    assert!(sk.points[c].link.is_none() && !sk.points[c].fixed, "it sits on a point of its own, not a projected one");
}

/// #12086: the bottom face of a holder, an arm 2 thick along a hole's ring, projected into a
/// sketch. The ring's diameter `d` drives the outline: the arm's end corners are 2 apart, at
/// (55 + s, -9 + s) and (55 + s, -7 + s), with `s` = (10 - d) / 2. Made smaller (d 10 to 8),
/// the outline's box shrinks with it, and in box-relative terms each end corner lands nearer
/// to where the other one goes. The lines leave the two corners in different directions, so they
/// are not confused: the projection follows, keeping its ids and a point drawn on a corner.
#[test]
fn a_face_with_a_thin_arm_follows_its_body_made_smaller() {
    let holder = |d: f64| -> Vec<LinkGeom> {
        let (r, s) = (d / 2.0, (10.0 - d) / 2.0);
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let hole = r - 2.0;
        vec![
            LinkGeom::Line(p(-r, 0.0), p(-r, -67.0 + s)),
            LinkGeom::Line(p(-r, -67.0 + s), p(-3.0 + s, -67.0 + s)),
            LinkGeom::Arc { c: Vec2::ZERO, a: p(r, 0.0), b: p(-r, 0.0) },
            LinkGeom::Line(p(-3.0 + s, -67.0 + s), p(-3.0 + s, -9.0 + s)),
            LinkGeom::Line(p(-3.0 + s, -9.0 + s), p(55.0 + s, -9.0 + s)),
            LinkGeom::Line(p(r, -7.0 + s), p(r, 0.0)),
            LinkGeom::Line(p(55.0 + s, -7.0 + s), p(r, -7.0 + s)),
            LinkGeom::Line(p(55.0 + s, -9.0 + s), p(55.0 + s, -7.0 + s)),
            LinkGeom::Arc { c: Vec2::ZERO, a: p(-hole, 0.0), b: p(hole, 0.0) },
            LinkGeom::Arc { c: Vec2::ZERO, a: p(hole, 0.0), b: p(-hole, 0.0) },
        ]
    };
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &holder(10.0)).unwrap();
    let ids: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    let corner = |sk: &Sketch, at: Vec2| sk.link_points(&link).into_iter().find(|p| sk.points[*p].pos.dist(at) < 1e-9).unwrap();
    let upper = corner(&sk, Vec2::new(55.0, -7.0));
    let lower = corner(&sk, Vec2::new(55.0, -9.0));
    let free = sk.add_point(Vec2::new(55.0, -7.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: free, q: upper }, None).unwrap();

    assert_eq!(sk.update_link(&link, &holder(8.0)).map_err(|e| e.to_string()), Ok(false), "the same outline, moved");
    assert!(!sk.link(&link).unwrap().lost);
    assert_eq!(ids, sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect::<Vec<_>>());
    assert!(sk.points[upper].pos.dist(Vec2::new(56.0, -6.0)) < 1e-9, "{:?}", sk.points[upper].pos);
    assert!(sk.points[lower].pos.dist(Vec2::new(56.0, -8.0)) < 1e-9, "{:?}", sk.points[lower].pos);
    assert!(solve(&mut sk).ok());
    assert!(sk.points[free].pos.dist(Vec2::new(56.0, -6.0)) < 1e-7, "{:?}", sk.points[free].pos);
}

/// #13471: a face taken with `Intersect` is the face's section with the sketch plane: an open
/// chain of segments, which no closed boundary describes. The refresh that follows closing the
/// sketcher must not read that chain as an ambiguous projected boundary and lose the link.
#[test]
fn an_intersected_face_section_keeps_its_link_across_a_refresh() {
    let section = vec![LinkGeom::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)), LinkGeom::Line(Vec2::new(10.0, 0.0), Vec2::new(10.0, 6.0))];
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Intersect, face(), &section).unwrap();
    assert_eq!(sk.update_link(&link, &section).map_err(|e| e.to_string()), Ok(false), "the same section keeps the link");
    assert!(!sk.link(&link).unwrap().lost, "the link is not lost");
}

/// #13471: the same section, bigger, as the body it comes from grows: the link follows it.
#[test]
fn an_intersected_face_section_follows_a_resized_body() {
    let section = |size: f64| -> Vec<LinkGeom> {
        vec![LinkGeom::Line(Vec2::new(0.0, 0.0), Vec2::new(size, 0.0)), LinkGeom::Line(Vec2::new(size, 0.0), Vec2::new(size, size * 0.6))]
    };
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Intersect, face(), &section(10.0)).unwrap();
    let ids: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    let corner = sk.link_points(&link).into_iter().find(|p| sk.points[*p].pos.dist(Vec2::new(10.0, 0.0)) < 1e-9).unwrap();
    assert_eq!(sk.update_link(&link, &section(12.0)).map_err(|e| e.to_string()), Ok(false), "the grown section keeps the link");
    assert!(!sk.link(&link).unwrap().lost, "the link is not lost");
    assert_eq!(ids, sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect::<Vec<_>>(), "the curves keep their ids");
    assert!(sk.points[corner].pos.dist(Vec2::new(12.0, 0.0)) < 1e-9, "{:?}", sk.points[corner].pos);
}

/// The lines at a point leave it the same ways only when each direction matches a different
/// one: two nearly parallel lines at a sharp corner don't both match one line while the other
/// line turned round.
#[test]
fn leaving_directions_match_one_to_one() {
    let (right, left, up) = (Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0));
    let near_right = Vec2::new(0.995, 0.0998);
    assert!(super::loops::same_leaving(&[right, up], &[up, right]), "the same lines, read in another order");
    assert!(super::loops::same_leaving(&[right, near_right], &[near_right, right]));
    assert!(!super::loops::same_leaving(&[right, near_right], &[right, left]), "one line turned round");
    assert!(!super::loops::same_leaving(&[right, up], &[right, up, left]));
    assert!(super::loops::same_leaving(&[], &[]));
}

/// #17732: a projected outline set to construction stays construction when the link is rebuilt
/// because the face changed shape (a notch splits its bottom edge). Were it to turn into
/// profile geometry, a circle drawn around the plate would extrude the disc minus the plate.
/// The hole's rim, saved as two halves, keeps them (#18250) and stays profile geometry.
#[test]
fn a_rebuilt_link_keeps_its_curves_set_to_construction() {
    let mut sk = Sketch::new();
    let (east, west) = (Vec2::new(5.0, 0.0), Vec2::new(-5.0, 0.0));
    let corners = [Vec2::new(-20.0, -10.0), Vec2::new(20.0, -10.0), Vec2::new(20.0, 10.0), Vec2::new(-20.0, 10.0)];
    let mut saved: Vec<LinkGeom> = (0..4).map(|k| LinkGeom::Line(corners[k], corners[(k + 1) % 4])).collect();
    saved.push(LinkGeom::Arc { c: Vec2::ZERO, a: east, b: west });
    saved.push(LinkGeom::Arc { c: Vec2::ZERO, a: west, b: east });
    let face = LinkSource::Edge { body: "Body1".into(), at: Vec3::new(5.0, 0.0, 0.0) };
    let link = sk.add_link(LinkKind::Project, face, &saved).unwrap();
    let outline: Vec<String> =
        sk.link_curves(&link).iter().filter(|c| matches!(sk.curves[**c].kind, CurveKind::Line { .. })).map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(outline.len(), 4);
    for c in sk.link_curves(&link) {
        if matches!(sk.curves[c].kind, CurveKind::Line { .. }) {
            sk.curves[c].construction = true;
        }
    }
    let notch = Vec2::new(0.0, -10.0);
    let mut now = vec![LinkGeom::Line(corners[0], notch), LinkGeom::Line(notch, corners[1])];
    now.extend((1..4).map(|k| LinkGeom::Line(corners[k], corners[(k + 1) % 4])));
    now.push(LinkGeom::Circle(Vec2::ZERO, 5.0));
    assert!(sk.update_link(&link, &now).unwrap());
    for id in &outline {
        let c = sk.curves.iter().find(|c| &c.id == id).unwrap_or_else(|| panic!("{id} is still in the sketch"));
        assert!(c.construction, "{id} is still construction after the rebuild");
    }
    let rim: Vec<usize> = sk.link_curves(&link).iter().copied().filter(|c| matches!(sk.curves[*c].kind, CurveKind::Arc { .. })).collect();
    assert_eq!(rim.len(), 2, "the rim's two halves");
    assert!(rim.iter().all(|c| !sk.curves[*c].construction));
}

/// #17172 made a fresh projection of a closed circular edge one circle, not its two halves. A
/// saved sketch's halves stay halves when the edge is read again as that circle (#18250):
/// others name each half by id. Both keep their ids, a dimension on the second half stays on
/// it, and a line drawn to a point where the halves meet keeps its end there.
#[test]
fn a_circle_saved_as_two_halves_keeps_its_halves_and_what_was_on_them() {
    let mut sk = Sketch::new();
    let (east, west) = (Vec2::new(5.0, 0.0), Vec2::new(-5.0, 0.0));
    let halves = [LinkGeom::Arc { c: Vec2::ZERO, a: east, b: west }, LinkGeom::Arc { c: Vec2::ZERO, a: west, b: east }];
    let rim = LinkSource::Edge { body: "Body1".into(), at: Vec3::new(5.0, 0.0, 0.0) };
    let link = sk.add_link(LinkKind::Project, rim, &halves).unwrap();
    let [first, second] = sk.link_curves(&link)[..] else { panic!("two halves") };
    let ids = [sk.curves[first].id.clone(), sk.curves[second].id.clone()];
    let CurveKind::Arc { b: split, .. } = sk.curves[first].kind else { panic!("an arc") };
    sk.add_constraint(ConstraintKind::Diameter { c: second, value: 10.0 }, None).unwrap();
    let start = sk.add_point(Vec2::new(-20.0, 0.0), None).unwrap();
    let line = sk.add_line_pts(start, split, None).unwrap();
    let line = sk.curves[line].id.clone();
    sk.update_link(&link, &[LinkGeom::Circle(Vec2::ZERO, 5.0)]).unwrap();
    let kept: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(kept, ids, "both halves keep their ids");
    let second = sk.curve_index(&ids[1]).unwrap();
    assert!(sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::Diameter { c, .. } if c == second)), "{:?}", sk.constraints);
    let line = sk.curve_index(&line).unwrap();
    let CurveKind::Line { b: end, .. } = sk.curves[line].kind else { panic!("a line") };
    let CurveKind::Arc { b: split, .. } = sk.curves[sk.curve_index(&ids[0]).unwrap()].kind else { panic!("an arc") };
    assert_eq!(end, split, "the line still ends where the halves meet");
    assert!(solve(&mut sk).ok());
    assert!(sk.points[end].pos.dist(west) < 1e-9);
}

/// #18250: a saved sketch with a closed offset of a projected circle's two halves. Re-read as
/// one circle (#17175), the halves would fold into it and the offset lose its sources ("an
/// offset source vanished or changed shape"). The halves stay, split where they were, and the
/// offset follows the circle when it grows.
#[test]
fn halves_of_a_projected_circle_an_offset_follows_stay_halves() {
    let mut sk = Sketch::new();
    let (east, west) = (Vec2::new(5.0, 0.0), Vec2::new(-5.0, 0.0));
    let halves = [LinkGeom::Arc { c: Vec2::ZERO, a: east, b: west }, LinkGeom::Arc { c: Vec2::ZERO, a: west, b: east }];
    let rim = LinkSource::Edge { body: "Body1".into(), at: Vec3::new(5.0, 0.0, 0.0) };
    let link = sk.add_link(LinkKind::Project, rim, &halves).unwrap();
    let ids: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(ids.len(), 2, "two halves");
    let (ie, iw) = (Vec2::new(4.0, 0.0), Vec2::new(-4.0, 0.0));
    let upper = sk.add_arc(Vec2::ZERO, ie, iw, [None; 3], None).unwrap();
    let CurveKind::Arc { c, a: e, b: w } = sk.curves[upper].kind else { panic!("an arc") };
    let lower = sk.add_arc(Vec2::ZERO, iw, ie, [Some(c), Some(w), Some(e)], None).unwrap();
    let outputs = vec![sk.curves[upper].id.clone(), sk.curves[lower].id.clone()];
    sk.offsets.push(crate::offset::OffsetChain {
        sources: vec![(ids[0].clone(), true), (ids[1].clone(), true)],
        outputs: outputs.clone(),
        distance: 1.0,
        closed: true,
        topology: None,
    });
    sk.refresh_offsets().unwrap();

    sk.update_link(&link, &[LinkGeom::Circle(Vec2::ZERO, 6.0)]).unwrap();

    let kept: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(kept, ids, "both halves keep their ids");
    sk.refresh_offsets().unwrap();
    for id in &outputs {
        let CurveKind::Arc { c, a, .. } = sk.curves[sk.curve_index(id).unwrap()].kind else { panic!("an arc") };
        assert!((sk.points[c].pos.dist(sk.points[a].pos) - 5.0).abs() < 1e-9, "the offset follows the larger circle");
    }
}

/// #18250: a face's two concentric rims saved as halves, which a feature names as profile loops
/// by curve id. Read again as two circles, each rim keeps its two halves and their ids.
#[test]
fn concentric_rims_saved_as_halves_keep_their_ids_read_again_as_circles() {
    let mut sk = Sketch::new();
    let rim = |r: f64| {
        [
            LinkGeom::Arc { c: Vec2::ZERO, a: Vec2::new(r, 0.0), b: Vec2::new(-r, 0.0) },
            LinkGeom::Arc { c: Vec2::ZERO, a: Vec2::new(-r, 0.0), b: Vec2::new(r, 0.0) },
        ]
    };
    let saved: Vec<LinkGeom> = rim(10.0).into_iter().chain(rim(4.0)).collect();
    let link = sk.add_link(LinkKind::Project, face(), &saved).unwrap();
    let ids: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(ids.len(), 4, "two rims of two halves");

    sk.update_link(&link, &[LinkGeom::Circle(Vec2::ZERO, 4.0), LinkGeom::Circle(Vec2::ZERO, 10.0)]).unwrap();

    let kept: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(kept, ids, "every half keeps its id");
    let radius = |id: &str| match sk.curves[sk.curve_index(id).unwrap()].kind {
        CurveKind::Arc { c, a, .. } => sk.points[c].pos.dist(sk.points[a].pos),
        _ => f64::NAN,
    };
    assert!((radius(&ids[0]) - 10.0).abs() < 1e-9 && (radius(&ids[1]) - 10.0).abs() < 1e-9, "the outer rim");
    assert!((radius(&ids[2]) - 4.0).abs() < 1e-9 && (radius(&ids[3]) - 4.0).abs() < 1e-9, "the inner rim");
}

#[test]
fn a_projected_circle_read_again_a_hair_away_takes_the_fresh_reading() {
    // #12920: a hole's rim saved as it was fitted, 1.3e-6 small and off centre, read again as
    // the hole's circle exactly. It is the same circle (the same curve and point, so whatever
    // is drawn on it stays), now where the model puts it.
    let mut sk = Sketch::new();
    let edge = LinkSource::Edge { body: "Body3".into(), at: Vec3::ZERO };
    let fitted = [LinkGeom::Circle(Vec2::new(25.962298020754407, 4.500001583499248), 1.6999986845754604)];
    let link = sk.add_link(LinkKind::Project, edge, &fitted).unwrap();
    let curves = sk.link_curves(&link);
    let points = sk.link_points(&link);
    let exact = Vec2::new(25.96229670532913, 4.5);
    assert!(!sk.update_link(&link, &[LinkGeom::Circle(exact, 1.7)]).unwrap());
    assert_eq!((sk.link_curves(&link), sk.link_points(&link)), (curves.clone(), points));
    let Some(CurveKind::Circle { c, r }) = curves.first().map(|ci| sk.curves[*ci].kind.clone()) else { panic!("{:?}", sk.curves) };
    assert_eq!((sk.points[c].pos, r), (exact, 1.7));
}

/// #14249: a face with two round holes, saved with each hole split where an older build split
/// it. Read again, the holes come back split at other points and the outline is read in
/// another order: the projection is rebuilt. Its outline lines are where they were, so they
/// keep their ids, though no constraint holds them: an extrude picks its profile by them.
#[test]
fn a_rebuilt_face_keeps_the_ids_of_lines_that_did_not_move() {
    // A round hole about `c`, split into two arcs at `c + to` and `c - to`.
    let hole = |c: Vec2, to: Vec2| [LinkGeom::Arc { c, a: c + to, b: c - to }, LinkGeom::Arc { c, a: c - to, b: c + to }];
    let p = [Vec2::new(-96.0, 180.0), Vec2::new(-96.0, -6.0), Vec2::new(96.0, -6.0), Vec2::new(96.0, 180.0)];
    let (big, small) = (Vec2::new(0.0, 90.0), Vec2::new(-48.44, 41.56));
    let saved: Vec<LinkGeom> = [LinkGeom::Line(p[0], p[1]), LinkGeom::Line(p[1], p[2]), LinkGeom::Line(p[3], p[0]), LinkGeom::Line(p[2], p[3])]
        .into_iter()
        .chain(hole(big, Vec2::new(-57.0, 0.0)))
        .chain(hole(small, Vec2::new(-1.6, 1.2)))
        .collect();
    let fresh: Vec<LinkGeom> = [LinkGeom::Line(p[1], p[0]), LinkGeom::Line(p[1], p[2]), LinkGeom::Line(p[0], p[3]), LinkGeom::Line(p[2], p[3])]
        .into_iter()
        .chain(hole(big, Vec2::new(57.0, 0.0)))
        .chain(hole(small, Vec2::new(2.0, 0.0)))
        .collect();
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &saved).unwrap();
    // Each curve's id, with where it lies: a line by its two ends, an arc by its centre.
    let placed = |sk: &Sketch| -> Vec<(String, Vec<Vec2>)> {
        let mut out: Vec<(String, Vec<Vec2>)> = sk
            .link_curves(&link)
            .iter()
            .map(|c| {
                let at = |p: usize| sk.points[p].pos;
                let mut ends = match sk.curves[*c].kind {
                    CurveKind::Line { a, b } => vec![at(a), at(b)],
                    CurveKind::Arc { c, .. } => vec![at(c)],
                    ref k => panic!("not a line or an arc: {k:?}"),
                };
                ends.sort_by(|p, q| p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)));
                (sk.curves[*c].id.clone(), ends)
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    };
    let before = placed(&sk);
    assert!(sk.update_link(&link, &fresh).unwrap(), "the holes are split at other points: the projection is rebuilt");
    assert!(!sk.link(&link).unwrap().lost);
    assert_eq!(placed(&sk), before, "each line keeps its id on the same two ends, each arc on the same hole");
    let picked: Vec<String> = before.iter().filter(|(_, ends)| ends.len() == 2).map(|(id, _)| id.clone()).collect();
    assert_eq!(picked.len(), 4);
    let profiles = crate::find_profiles(&sk);
    let outline = crate::profile_by_curves(&profiles, &picked).expect("the extrude's saved pick finds its profile");
    assert_eq!(profiles[outline].hole_curves.len(), 2, "with both holes in it");
}

/// #14249: matching the curves nothing holds never takes a match from one the sketch holds. A
/// face's rounded corner goes back to a sharp one. Both ends of the round are as near to the
/// new corner; a line of the sketch's own ends on one of them. That end becomes the corner, as
/// when only held points were matched; the other end, which nothing holds, goes.
#[test]
fn a_held_round_end_keeps_the_corner_its_free_neighbour_is_as_near_to() {
    let rounded = |round: bool| {
        let (o, x, y) = (Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(0.0, 100.0));
        let mut out = vec![LinkGeom::Line(o, x)];
        if round {
            let (a, b) = (Vec2::new(100.0, 95.0), Vec2::new(95.0, 100.0));
            out.extend([LinkGeom::Line(x, a), LinkGeom::Arc { c: Vec2::new(95.0, 95.0), a, b }, LinkGeom::Line(b, y)]);
        } else {
            let corner = Vec2::new(100.0, 100.0);
            out.extend([LinkGeom::Line(x, corner), LinkGeom::Line(corner, y)]);
        }
        out.push(LinkGeom::Line(y, o));
        out
    };
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &rounded(true)).unwrap();
    let end = sk.link_points(&link).into_iter().find(|p| sk.points[*p].pos == Vec2::new(100.0, 95.0)).unwrap();
    let free = sk.add_point(Vec2::new(150.0, 95.0), None).unwrap();
    sk.add_curve(CurveKind::Line { a: end, b: free }, Some("l_mine")).unwrap();
    assert!(sk.update_link(&link, &rounded(false)).unwrap());
    let mine = sk.curves.iter().find(|c| c.id == "l_mine").expect("the sketch's line survives the rebuild");
    let CurveKind::Line { a, .. } = mine.kind else { panic!("not a line") };
    assert_eq!(sk.points[a].pos, Vec2::new(100.0, 100.0), "it ends on the new corner");
    assert_eq!(sk.points[a].link.as_deref(), Some(link.as_str()), "on the projected corner, not a point of its own");
}

/// #14249: among circles about one centre that nothing holds, one that kept its radius keeps
/// its id, even when an old circle listed before it is nearest to it too.
#[test]
fn a_rebuilt_face_gives_an_unchanged_concentric_circle_its_own_id() {
    let c = Vec2::new(0.0, 0.0);
    let rings = |radii: &[f64]| square(10.0).into_iter().chain(radii.iter().map(|r| LinkGeom::Circle(c, *r))).collect::<Vec<_>>();
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &rings(&[3.0, 4.0])).unwrap();
    let id_of = |sk: &Sketch, r: f64| {
        sk.link_curves(&link)
            .into_iter()
            .find(|i| matches!(sk.curves[*i].kind, CurveKind::Circle { r: s, .. } if s == r))
            .map(|i| sk.curves[i].id.clone())
    };
    let four = id_of(&sk, 4.0).unwrap();
    assert!(sk.update_link(&link, &rings(&[1.0, 4.0, 6.0])).unwrap());
    assert_eq!(id_of(&sk, 4.0), Some(four), "the circle of radius 4 is where it was");
}

/// #18711: a face recorded with the names of its edges, read again sheared far out of its old
/// box and with its edges in another order, is the same four lines: each keeps its id and goes
/// where its edge went, and a point held on a corner follows it. Without the names the reading is
/// matched by geometry alone, as before.
#[test]
fn a_face_read_by_its_edge_names_keeps_its_curves_through_any_order() {
    let names: Vec<String> = ["F1:side:a|F1:top", "F1:side:b|F1:top", "F1:side:c|F1:top", "F1:side:d|F1:top"].map(String::from).to_vec();
    let shear = |p: Vec2| Vec2::new(p.x + 3.0 * p.y, p.y);
    let sheared: Vec<LinkGeom> = square(10.0)
        .into_iter()
        .map(|g| match g {
            LinkGeom::Line(a, b) => LinkGeom::Line(shear(a), shear(b)),
            _ => panic!(),
        })
        .collect();
    // The same edges, the first one read last.
    let order = [1, 2, 3, 0];
    let fresh: Vec<LinkGeom> = order.iter().map(|i| sheared[*i].clone()).collect();
    let fresh_names: Vec<String> = order.iter().map(|i| names[*i].clone()).collect();

    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &square(10.0)).unwrap();
    sk.set_link_curve_edges(&link, &square(10.0), &names);
    assert_eq!(sk.link(&link).unwrap().curve_edges, names);
    let ids: Vec<String> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    let corner = sk.link_points(&link).into_iter().find(|p| sk.points[*p].pos == Vec2::new(10.0, 10.0)).unwrap();
    let free = sk.add_point(Vec2::new(10.0, 10.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: free, q: corner }, None).unwrap();
    let unnamed = sk.clone();

    assert_eq!(sk.update_link_named(&link, &fresh, &fresh_names).map_err(|e| e.to_string()), Ok(false));
    assert!(!sk.link(&link).unwrap().lost);
    assert_eq!(ids, sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect::<Vec<_>>());
    assert_eq!(sk.link(&link).unwrap().curve_edges, names, "the names stay with their curves");
    assert_eq!(sk.points[corner].pos, shear(Vec2::new(10.0, 10.0)));
    assert_eq!(sk.constraints.len(), 1);
    assert!(solve(&mut sk).ok());
    assert!(sk.points[free].pos.dist(shear(Vec2::new(10.0, 10.0))) < 1e-7);

    // A link saved without the names is read as before: by geometry, and this reading is not one.
    let mut old = unnamed.clone();
    old.links.iter_mut().for_each(|l| l.curve_edges.clear());
    let mut plain = old.clone();
    let named = old.update_link_named(&link, &fresh, &fresh_names).map_err(|e| e.to_string());
    let geometric = plain.update_link(&link, &fresh).map_err(|e| e.to_string());
    assert_eq!(named, geometric);
    assert_eq!(old.points, plain.points);
    assert_eq!(old.curves, plain.curves);
    assert_eq!(old.constraints, plain.constraints);
}

/// #20280: a face's outer rim and a shelled wall's rim, concentric arcs saved on one centre point,
/// are read again with the two centres apart by more than the layout merges (5e-7) but within the
/// rebuild's matching tolerance. The old centre becomes only one of the new ones; the other arc
/// is still the same arc and keeps its id, which an offset names.
#[test]
fn concentric_arcs_read_with_centres_apart_keep_their_ids() {
    let (r3, s) = (5.0 * 3f64.sqrt(), 5.0 * std::f64::consts::FRAC_1_SQRT_2);
    let boundary = |inner: Vec2| {
        vec![
            LinkGeom::Arc { c: Vec2::ZERO, a: Vec2::new(r3, -5.), b: Vec2::new(-r3, -5.) },
            LinkGeom::Line(Vec2::new(-r3, -5.), Vec2::new(r3, -5.)),
            LinkGeom::Arc { c: inner, a: Vec2::new(s, -s), b: Vec2::new(-s, -s) },
            LinkGeom::Line(Vec2::new(-s, -s), Vec2::new(s, -s)),
        ]
    };
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, face(), &boundary(Vec2::ZERO)).unwrap();
    assert_eq!(sk.link_points(&link).len(), 5, "the saved arcs share one centre point");
    let ids: Vec<String> = sk.link_curves(&link).iter().map(|&i| sk.curves[i].id.clone()).collect();
    assert!(sk.update_link(&link, &boundary(Vec2::new(0., 5e-7))).unwrap());
    assert_eq!(sk.link_points(&link).len(), 6, "the fresh arcs have a centre each");
    for id in &ids {
        assert!(sk.curve_index(id).is_some_and(|i| sk.curves[i].link.as_deref() == Some(link.as_str())), "{id} kept");
    }
    let CurveKind::Arc { c, .. } = sk.curves[sk.curve_index(&ids[2]).unwrap()].kind else { panic!("the inner rim is an arc") };
    assert!(sk.points[c].pos.dist(Vec2::new(0., 5e-7)) < 1e-12, "the inner arc keeps its own centre");
}
