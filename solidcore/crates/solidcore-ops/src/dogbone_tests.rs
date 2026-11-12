use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2};

use solidcore_record::brep::{Brep, Curve, Loop, Surface};

use super::*;
use crate::boolean::{boolean, BooleanOp};
use crate::{make_box, make_cylinder, Body};

const R: f64 = 1.5;

fn options(style: Style) -> BodyOptions {
    BodyOptions {
        relief: Options {
            radius: R,
            style,
            max_angle: FRAC_PI_2,
        },
        along: None,
    }
}

/// `a` less each of `tools`.
fn cut(a: Body, tools: Vec<Body>) -> Body {
    tools.into_iter().fold(a, |acc, t| {
        boolean(acc, t, BooleanOp::Subtract).unwrap().unwrap()
    })
}

/// A 40 x 30 x 10 plate with a 20 x 14 pocket 6 deep from its top (floor at z = 4).
fn pocketed() -> Body {
    cut(
        make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap(),
        vec![make_box([10.0, 8.0, 4.0], [20.0, 14.0, 7.0]).unwrap()],
    )
}

/// The plate with a 20 x 6 slot through it: the reliefs at the ends of each short wall don't meet.
fn slotted() -> Body {
    cut(
        make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap(),
        vec![make_box([10.0, 12.0, -1.0], [20.0, 6.0, 12.0]).unwrap()],
    )
}

fn record(b: &Body) -> Brep {
    b.to_record().unwrap().0
}

fn point(r: &Brep, v: u32) -> [f64; 3] {
    r.vertices[v as usize].point
}

/// The straight edges along z with both ends at (x, y) in `corners`.
fn vertical_edges(r: &Brep, corners: &[[f64; 2]]) -> Vec<usize> {
    let at = |p: [f64; 3]| {
        corners
            .iter()
            .any(|c| (p[0] - c[0]).abs() < 1e-9 && (p[1] - c[1]).abs() < 1e-9)
    };
    (0..r.edges.len())
        .filter(|&i| {
            let e = &r.edges[i];
            let (a, b) = (point(r, e.vertices[0]), point(r, e.vertices[1]));
            matches!(e.curve, Curve::Line { .. }) && at(a) && at(b) && (a[2] - b[2]).abs() > 1e-9
        })
        .collect()
}

/// The planar face whose every vertex lies at height `z` inside the box `lo`–`hi` (x, y).
fn flat_face(r: &Brep, z: f64, lo: [f64; 2], hi: [f64; 2]) -> usize {
    let found: Vec<usize> = (0..r.faces.len())
        .filter(|&f| {
            let face = &r.faces[f];
            if !matches!(
                face.surface,
                Surface::Plane { .. } | Surface::PlaneThrough { .. }
            ) {
                return false;
            }
            let verts: Vec<[f64; 3]> = face
                .loops
                .iter()
                .flat_map(|lp| match lp {
                    Loop::Edges { edges } => edges
                        .iter()
                        .flat_map(|oe| r.edges[oe.edge as usize].vertices)
                        .collect::<Vec<_>>(),
                    Loop::Vertex { vertex } => vec![*vertex],
                })
                .map(|v| point(r, v))
                .collect();
            verts.iter().all(|p| {
                (p[2] - z).abs() < 1e-9
                    && p[0] >= lo[0] - 1e-9
                    && p[0] <= hi[0] + 1e-9
                    && p[1] >= lo[1] - 1e-9
                    && p[1] <= hi[1] + 1e-9
            })
        })
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found[0]
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9)
}

/// The cylinders of `plan` cut from `body` (each tool's axis along ±z).
fn relieved(body: Body, plan: &Plan) -> Body {
    let tools = plan
        .tools
        .iter()
        .map(|t| {
            let c = t.cylinder;
            assert!(
                near([c.axis[0], c.axis[1], c.axis[2].abs()], [0.0, 0.0, 1.0]),
                "{c:?}"
            );
            let bottom = if c.axis[2] > 0.0 {
                c.base[2]
            } else {
                c.base[2] - c.height
            };
            make_cylinder([c.base[0], c.base[1], bottom], c.radius, c.height).unwrap()
        })
        .collect();
    cut(body, tools)
}

const POCKET: [[f64; 2]; 4] = [[10.0, 8.0], [30.0, 8.0], [30.0, 22.0], [10.0, 22.0]];

#[test]
fn a_pockets_four_corners_get_blind_reliefs_from_the_floor_up() {
    let body = pocketed();
    let r = record(&body);
    let edges = vertical_edges(&r, &POCKET);
    assert_eq!(edges.len(), 4);
    let plan = plan(&r, &edges, &[], &options(Style::Normal)).unwrap();
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    assert_eq!(plan.tools.len(), 4);
    let d = R * FRAC_1_SQRT_2;
    for t in &plan.tools {
        let c = t.cylinder;
        // Each centre sits d inside the pocket from its corner, the relief from the floor (z = 4)
        // up past the top by a radius.
        let (lo, hi) = if c.axis[2] > 0.0 {
            (c.base[2], c.base[2] + c.height)
        } else {
            (c.base[2] - c.height, c.base[2])
        };
        assert!(
            (lo - 4.0).abs() < 1e-9 && (hi - (10.0 + R)).abs() < 1e-9,
            "{c:?}"
        );
        let corner = POCKET
            .iter()
            .find(|p| (c.base[0] - p[0]).abs() < 2.0 && (c.base[1] - p[1]).abs() < 2.0)
            .unwrap();
        let inward = [
            if corner[0] < 20.0 { d } else { -d },
            if corner[1] < 15.0 { d } else { -d },
        ];
        assert!(
            near(
                [c.base[0], c.base[1], 0.0],
                [corner[0] + inward[0], corner[1] + inward[1], 0.0]
            ),
            "{c:?}"
        );
        assert_eq!(c.radius, R);
    }
    // Cut: the volume falls by four reliefs, (π/2 − 1) r² over the pocket's 6 mm depth each.
    let before = 40.0 * 30.0 * 10.0 - 20.0 * 14.0 * 6.0;
    let after = relieved(body, &plan).volume(1e-9).unwrap();
    let want = before - 4.0 * (FRAC_PI_2 - 1.0) * R * R * 6.0;
    assert!((after - want).abs() < 1e-6 * before, "{after} vs {want}");
}

#[test]
fn a_picked_floor_relieves_the_pockets_corners() {
    let body = pocketed();
    let r = record(&body);
    let floor = flat_face(&r, 4.0, [10.0, 8.0], [30.0, 22.0]);
    let corners = vertical_edges(&r, &POCKET);
    let by_face = plan(&r, &[], &[floor], &options(Style::Normal)).unwrap();
    let by_edges = plan(&r, &corners, &[], &options(Style::Normal)).unwrap();
    assert!(by_face.skipped.is_empty(), "{:?}", by_face.skipped);
    // The same four reliefs, whatever the order.
    assert_eq!(by_face.tools.len(), 4);
    assert_eq!(by_edges.tools.len(), 4);
    assert!(
        by_face.tools.iter().all(|t| by_edges.tools.contains(t)),
        "{by_face:?} vs {by_edges:?}"
    );
    // The same edge picked twice, and by its face too: one relief.
    let e = corners[0];
    let again = plan(&r, &[e, e], &[floor], &options(Style::Normal)).unwrap();
    assert_eq!(again.tools.len(), 4);
}

#[test]
fn a_slot_through_the_plate_is_relieved_past_both_faces() {
    let body = slotted();
    let r = record(&body);
    let top = flat_face(&r, 10.0, [0.0, 0.0], [40.0, 30.0]);
    let plan = plan(&r, &[], &[top], &options(Style::Normal)).unwrap();
    // The slot's four corners; the plate's four outside edges are convex.
    assert_eq!(plan.tools.len(), 4);
    assert_eq!(plan.skipped.len(), 4);
    assert!(
        plan.skipped
            .iter()
            .all(|s| s.why == Why::Corner(Skip::Convex) && s.edge.is_some()),
        "{:?}",
        plan.skipped
    );
    for t in &plan.tools {
        let c = t.cylinder;
        assert!((c.height - (10.0 + 2.0 * R)).abs() < 1e-9, "{c:?}");
    }
    let before = 40.0 * 30.0 * 10.0 - 20.0 * 6.0 * 10.0;
    let after = relieved(body, &plan).volume(1e-9).unwrap();
    let want = before - 4.0 * (FRAC_PI_2 - 1.0) * R * R * 10.0;
    assert!((after - want).abs() < 1e-6 * before, "{after} vs {want}");
}

#[test]
fn an_l_shaped_pocket_has_five_inside_corners_and_one_outside() {
    // An L: 20 x 6 along x and 6 x 16 along y from (10, 6), 5 deep.
    let plate = make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap();
    let l = boolean(
        make_box([10.0, 6.0, 5.0], [20.0, 6.0, 6.0]).unwrap(),
        make_box([10.0, 6.0, 5.0], [6.0, 16.0, 6.0]).unwrap(),
        BooleanOp::Union,
    )
    .unwrap()
    .unwrap();
    let body = cut(plate, vec![l]);
    let r = record(&body);
    let corners = [
        [10.0, 6.0],
        [30.0, 6.0],
        [30.0, 12.0],
        [16.0, 12.0],
        [16.0, 22.0],
        [10.0, 22.0],
    ];
    let edges = vertical_edges(&r, &corners);
    assert_eq!(edges.len(), 6);
    let plan = plan(&r, &edges, &[], &options(Style::Normal)).unwrap();
    assert_eq!(plan.tools.len(), 5, "{plan:?}");
    assert_eq!(plan.skipped.len(), 1);
    let s = plan.skipped[0];
    assert_eq!(s.why, Why::Corner(Skip::Convex));
    // The outside corner of the L at (16, 12).
    assert!(
        (s.at[0] - 16.0).abs() < 1e-9 && (s.at[1] - 12.0).abs() < 1e-9,
        "{s:?}"
    );
    let before = 40.0 * 30.0 * 10.0 - (20.0 * 6.0 + 6.0 * 10.0) * 5.0;
    let after = relieved(body, &plan).volume(1e-9).unwrap();
    let want = before - 5.0 * (FRAC_PI_2 - 1.0) * R * R * 5.0;
    assert!((after - want).abs() < 1e-6 * before, "{after} vs {want}");
}

#[test]
fn styles_place_the_relief_as_the_kernel_says() {
    let r = record(&pocketed());
    let edges = vertical_edges(&r, &[[10.0, 8.0]]);
    let centre = |style| {
        let p = plan(&r, &edges, &[], &options(style)).unwrap();
        let c = p.tools[0].cylinder.base;
        [c[0] - 10.0, c[1] - 8.0]
    };
    let d = R * FRAC_1_SQRT_2;
    let g = 0.25;
    let m = (R + g) * FRAC_1_SQRT_2;
    let n = centre(Style::Normal);
    assert!((n[0] - d).abs() < 1e-9 && (n[1] - d).abs() < 1e-9);
    let mi = centre(Style::Minimal { gap: g });
    assert!((mi[0] - m).abs() < 1e-9 && (mi[1] - m).abs() < 1e-9);
    // The pocket's walls at (10, 8): 20 along x, 14 along y. A T-bone bites the longer (x) one.
    let long = centre(Style::TBone { side: Side::Longer });
    assert!(
        (long[0] - R).abs() < 1e-9 && long[1].abs() < 1e-9,
        "{long:?}"
    );
    let short = centre(Style::TBone {
        side: Side::Shorter,
    });
    assert!(
        short[0].abs() < 1e-9 && (short[1] - R).abs() < 1e-9,
        "{short:?}"
    );
}

#[test]
fn edges_that_are_no_inside_corner_are_skipped_with_the_reason() {
    let r = record(&pocketed());
    // A plate's outside vertical edge: convex.
    let outside = vertical_edges(&r, &[[0.0, 0.0]]);
    let p = plan(&r, &outside, &[], &options(Style::Normal)).unwrap();
    assert_eq!(p.tools, []);
    assert_eq!(p.skipped[0].why, Why::Corner(Skip::Convex));
    assert!(near(p.skipped[0].at, [0.0, 0.0, 5.0]));
    // A pocket floor edge (along x) with the bit along z: not along.
    let floor_edge = (0..r.edges.len())
        .find(|&i| {
            let e = &r.edges[i];
            let (a, b) = (point(&r, e.vertices[0]), point(&r, e.vertices[1]));
            (a[2] - 4.0).abs() < 1e-9
                && (b[2] - 4.0).abs() < 1e-9
                && (a[1] - 8.0).abs() < 1e-9
                && (b[1] - 8.0).abs() < 1e-9
        })
        .unwrap();
    let mut o = options(Style::Normal);
    o.along = Some([0.0, 0.0, 1.0]);
    let p = plan(&r, &[floor_edge], &[], &o).unwrap();
    assert_eq!(p.skipped[0].why, Why::NotAlong);
    // Without an axis, the floor edge is an inside corner (90°) and is relieved along x.
    let p = plan(&r, &[floor_edge], &[], &options(Style::Normal)).unwrap();
    assert_eq!(p.tools.len(), 1);
    assert!(p.tools[0].cylinder.axis[0].abs() > 1.0 - 1e-12);
    // A corner more open than the largest angle.
    let mut narrow = options(Style::Normal);
    narrow.relief.max_angle = 1.0;
    let p = plan(&r, &vertical_edges(&r, &POCKET), &[], &narrow).unwrap();
    assert_eq!(p.tools, []);
    assert!(p
        .skipped
        .iter()
        .all(|s| s.why == Why::Corner(Skip::TooOpen)));
}

#[test]
fn a_curved_face_or_edge_is_skipped() {
    // A plate with a round hole: its rim edges are circles, its wall a cylinder.
    let body = cut(
        make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap(),
        vec![make_cylinder([20.0, 15.0, -1.0], 5.0, 12.0).unwrap()],
    );
    let r = record(&body);
    let rims: Vec<usize> = (0..r.edges.len())
        .filter(|&i| !matches!(r.edges[i].curve, Curve::Line { .. }))
        .collect();
    assert_ne!(rims.len(), 0);
    let p = plan(&r, &rims, &[], &options(Style::Normal)).unwrap();
    assert_eq!(p.tools, []);
    assert!(
        p.skipped.iter().all(|s| s.why == Why::NotStraight),
        "{:?}",
        p.skipped
    );
    let wall = (0..r.faces.len())
        .find(|&f| {
            !matches!(
                r.faces[f].surface,
                Surface::Plane { .. } | Surface::PlaneThrough { .. }
            )
        })
        .unwrap();
    let p = plan(&r, &[], &[wall], &options(Style::Normal)).unwrap();
    assert_eq!(p.skipped.len(), 1);
    assert_eq!(
        (p.skipped[0].face, p.skipped[0].why),
        (Some(wall), Why::NotPlanar)
    );
    // The plate's top: its four outside corners are convex, the hole has none.
    let top = flat_face(&r, 10.0, [0.0, 0.0], [40.0, 30.0]);
    let p = plan(&r, &[], &[top], &options(Style::Normal)).unwrap();
    assert_eq!(p.tools, []);
    assert_eq!(
        p.skipped
            .iter()
            .filter(|s| s.why == Why::Corner(Skip::Convex))
            .count(),
        4,
        "{:?}",
        p.skipped
    );
}

#[test]
fn hostile_plans_are_refused() {
    let r = record(&pocketed());
    assert_eq!(
        plan(&r, &[r.edges.len()], &[], &options(Style::Normal)),
        Err(Error::Argument)
    );
    assert_eq!(
        plan(&r, &[], &[r.faces.len()], &options(Style::Normal)),
        Err(Error::Argument)
    );
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut o = options(Style::Normal);
        o.relief.radius = radius;
        assert_eq!(plan(&r, &[0], &[], &o), Err(Error::Argument));
    }
    let mut o = options(Style::Normal);
    o.along = Some([f64::NAN, 0.0, 0.0]);
    assert!(plan(&r, &vertical_edges(&r, &POCKET), &[], &o).is_err());
    o.along = Some([0.0; 3]);
    assert!(plan(&r, &vertical_edges(&r, &POCKET), &[], &o).is_err());
    let many = vec![0usize; MAX_PICKS + 1];
    assert_eq!(
        plan(&r, &many, &[], &options(Style::Normal)),
        Err(Error::Argument)
    );
    // Nothing picked: nothing to do.
    assert_eq!(
        plan(&r, &[], &[], &options(Style::Normal)).unwrap(),
        Plan::default()
    );
}

/// The face of `r` on a cylinder.
fn cylinder_face(r: &Brep) -> usize {
    let found: Vec<usize> = (0..r.faces.len())
        .filter(|&f| matches!(r.faces[f].surface, Surface::Cylinder { .. }))
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found[0]
}

#[test]
fn a_pick_means_the_exact_face_nearest_it() {
    let body = pocketed();
    let (r, order) = body.to_record().unwrap();
    let floor = flat_face(&r, 4.0, [10.0, 8.0], [30.0, 22.0]);
    let at = |p: [f64; 3]| faces_at(&body, &order, &[p], 2.0).unwrap()[0];
    assert_eq!(at([20.0, 15.0, 4.0]), Some(floor));
    // 0.2 above the floor and 0.5 from the wall at x = 10: the floor; 0.5 above it and 0.2 from
    // the wall: the wall, whose every vertex lies at x = 10.
    assert_eq!(at([10.5, 15.0, 4.2]), Some(floor));
    let wall = at([10.2, 15.0, 4.5]).unwrap();
    assert_ne!(wall, floor);
    let on_wall = |p: [f64; 3]| (p[0] - 10.0).abs() < 1e-9;
    assert!(r.faces[wall].loops.iter().all(|lp| match lp {
        Loop::Edges { edges } => edges.iter().all(|oe| {
            let [a, b] = r.edges[oe.edge as usize].vertices;
            on_wall(point(&r, a)) && on_wall(point(&r, b))
        }),
        Loop::Vertex { vertex } => on_wall(point(&r, *vertex)),
    }));
    // The floor as trimmed: 0.5 over its plane but 1 past its edge at x = 10 (in the plate),
    // the point is nearer the wall.
    assert_eq!(at([9.0, 15.0, 4.5]), Some(wall));
    // Farther than the reach from every face: none.
    assert_eq!(at([100.0, 100.0, 100.0]), None);
    // Several points at once, in order.
    assert_eq!(
        faces_at(
            &body,
            &order,
            &[[20.0, 15.0, 4.0], [100.0, 0.0, 0.0], [10.5, 15.0, 4.2]],
            2.0
        )
        .unwrap(),
        vec![Some(floor), None, Some(floor)]
    );
    assert_eq!(faces_at(&body, &order, &[], 2.0).unwrap(), vec![]);
}

#[test]
fn a_pick_beside_a_round_wall_is_decided_on_the_exact_wall() {
    // A round pocket 5 in radius, floor at z = 4. A facet of a mesh of its wall would lie inside
    // the pocket, nearer its axis than the wall by up to the mesh's tolerance; on the exact wall,
    // a point 0.1 in from it is 0.1 away, so 0.08 above the floor picks the floor and 0.12 above
    // it the wall, whatever a mesh would say.
    let body = cut(
        make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap(),
        vec![make_cylinder([20.0, 15.0, 4.0], 5.0, 7.0).unwrap()],
    );
    let (r, order) = body.to_record().unwrap();
    let floor = flat_face(&r, 4.0, [15.0, 10.0], [25.0, 20.0]);
    let wall = cylinder_face(&r);
    // Unit directions from the axis, given as numbers: ops calls no trigonometry.
    for (c, s) in [
        (1.0, 0.0),
        (0.0, 1.0),
        (0.6, 0.8),
        (-0.8, 0.6),
        (-0.6, -0.8),
        (0.28, -0.96),
    ] {
        let (x, y) = (20.0 + 4.9 * c, 15.0 + 4.9 * s);
        let at = |z: f64| faces_at(&body, &order, &[[x, y, z]], 2.0).unwrap()[0];
        assert_eq!(at(4.08), Some(floor), "towards ({c}, {s})");
        assert_eq!(at(4.12), Some(wall), "towards ({c}, {s})");
    }
}

#[test]
fn hostile_face_picks_are_refused() {
    let body = pocketed();
    let (_, order) = body.to_record().unwrap();
    for reach in [-1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            faces_at(&body, &order, &[[20.0, 15.0, 4.0]], reach),
            Err(Error::Argument)
        );
    }
    for p in [[f64::NAN, 0.0, 0.0], [0.0, f64::INFINITY, 0.0]] {
        assert_eq!(faces_at(&body, &order, &[p], 1.0), Err(Error::Argument));
    }
    let many = vec![[0.0; 3]; MAX_PICKS + 1];
    assert_eq!(faces_at(&body, &order, &many, 1.0), Err(Error::Argument));
}
