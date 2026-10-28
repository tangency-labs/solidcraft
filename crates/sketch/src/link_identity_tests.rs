use super::*;
use crate::{ConstraintKind, solve};

fn square(size: f64) -> Vec<LinkGeom> {
    let pts = [Vec2::new(-size, -size), Vec2::new(size, -size), Vec2::new(size, size), Vec2::new(-size, size)];
    (0..4).map(|i| LinkGeom::Line(pts[i], pts[(i + 1) % 4])).collect()
}

fn face() -> LinkSource {
    LinkSource::Face { body: "Case".into(), at: Vec3::ZERO }
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

/// #5388: a circle the user drew on a projected hole's centre survives when the face changes
/// shape and its projection is rebuilt (a second hole added to the face).
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
    let kept = sk.curves.iter().find(|c| c.id == "c_mine").expect("the user's circle survives the rebuild");
    assert!(kept.link.is_none());
    match kept.kind {
        CurveKind::Circle { c, r } => {
            assert_eq!(r, 5.25);
            let p = &sk.points[c];
            assert!(p.link.is_none(), "it sits on a point of its own, not a projected one");
            assert!(p.pos.len() < 1e-9, "still at the hole's centre: {:?}", p.pos);
        }
        ref k => panic!("not a circle: {k:?}"),
    }
    assert_eq!(sk.link_curves(&link).len(), 4 + 3, "the projection is the new outline and holes");
}
