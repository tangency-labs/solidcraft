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

/// The straight edges along z with both ends at one (x, y) in `corners` (a sloped rim edge from
/// one corner to the next is not one).
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
            matches!(e.curve, Curve::Line { .. })
                && at(a)
                && (a[0] - b[0]).abs() < 1e-9
                && (a[1] - b[1]).abs() < 1e-9
                && (a[2] - b[2]).abs() > 1e-9
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
fn a_picked_edge_off_the_axis_is_still_a_corner_of_a_picked_face() {
    let body = pocketed();
    let r = record(&body);
    let floor = flat_face(&r, 4.0, [10.0, 8.0], [30.0, 22.0]);
    let e = vertical_edges(&r, &POCKET)[0];
    let mut sideways = options(Style::Normal);
    sideways.along = Some([1.0, 0.0, 0.0]);
    // Alone, the corner edge is off the axis.
    let alone = plan(&r, &[e, e], &[], &sideways).unwrap();
    assert!(alone.tools.is_empty());
    assert_eq!(alone.skipped.len(), 1, "{:?}", alone.skipped);
    assert_eq!(
        (alone.skipped[0].edge, alone.skipped[0].why),
        (Some(e), Why::NotAlong)
    );
    // With the floor picked too, the floor takes it along its normal: the same plan as the
    // floor alone, with no skip.
    let by_face = plan(&r, &[], &[floor], &sideways).unwrap();
    let both = plan(&r, &[e], &[floor], &sideways).unwrap();
    assert_eq!(by_face.tools.len(), 4);
    assert!(by_face.skipped.is_empty(), "{:?}", by_face.skipped);
    assert_eq!(both, by_face);
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

/// A 40 x 30 x 30 block whose top slopes, z = 2 + 0.75 y (its outward normal (0, -0.6, 0.8):
/// tan β = 0.75 from the z axis).
fn sloped() -> Body {
    let slope = make_box([-10.0, -30.0, 0.0], [60.0, 90.0, 60.0]).unwrap();
    // A turn about x taking z to (0, -0.6, 0.8), then up 2.
    slope
        .transform(&[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.8, 0.6, 0.0],
            [0.0, -0.6, 0.8, 0.0],
            [0.0, 0.0, 2.0, 1.0],
        ])
        .unwrap();
    cut(make_box([0.0; 3], [40.0, 30.0, 30.0]).unwrap(), vec![slope])
}

#[test]
fn a_picked_face_with_no_corner_is_skipped() {
    // The sloped top: planar, but each straight edge at its corners runs up the block's sides,
    // 36.87 degrees off its normal, so none is a corner of it. (A round post's top would not do:
    // its side's seam is a straight edge along the axis at the rim's vertex.)
    let r = record(&sloped());
    let top = (0..r.faces.len())
        .find(|&f| {
            matches!(
                r.faces[f].surface,
                Surface::Plane { .. } | Surface::PlaneThrough { .. }
            ) && r.faces[f].loops.iter().any(|lp| match lp {
                Loop::Edges { edges } => {
                    edges.iter().any(|oe| {
                        r.edges[oe.edge as usize]
                            .vertices
                            .iter()
                            .any(|v| (point(&r, *v)[2] - 24.5).abs() < 1e-9)
                    }) && edges.iter().all(|oe| {
                        r.edges[oe.edge as usize]
                            .vertices
                            .iter()
                            .all(|v| point(&r, *v)[2] > 1.0)
                    })
                }
                Loop::Vertex { .. } => false,
            })
        })
        .unwrap();
    let p = plan(&r, &[], &[top], &options(Style::Normal)).unwrap();
    assert_eq!(p.tools, []);
    assert_eq!(p.skipped.len(), 1, "{:?}", p.skipped);
    assert_eq!(
        (p.skipped[0].face, p.skipped[0].why),
        (Some(top), Why::NoCorners)
    );
}

#[test]
fn an_edge_between_two_faces_of_one_plane_is_smooth() {
    // A 20 mm cube imprinted across x = 10: its top, bottom, front and back are each split
    // there, by an edge between two faces of one plane.
    crate::session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied())
        .unwrap();
    let body = crate::boolean::imprint(
        make_box([0.0; 3], [20.0, 20.0, 20.0]).unwrap(),
        make_box([10.0, -10.0, -10.0], [40.0, 40.0, 40.0]).unwrap(),
    )
    .unwrap()
    .0;
    let r = record(&body);
    let across: Vec<usize> = (0..r.edges.len())
        .filter(|&i| {
            let e = &r.edges[i];
            matches!(e.curve, Curve::Line { .. })
                && e.vertices
                    .iter()
                    .all(|v| (point(&r, *v)[0] - 10.0).abs() < 1e-9)
        })
        .collect();
    assert_eq!(across.len(), 4);
    let p = plan(&r, &across, &[], &options(Style::Normal)).unwrap();
    assert_eq!(p.tools, []);
    assert_eq!(p.skipped.len(), 4);
    assert!(
        p.skipped
            .iter()
            .all(|s| s.why == Why::Smooth && s.edge.is_some()),
        "{:?}",
        p.skipped
    );
}

#[test]
fn an_edge_not_between_two_faces_is_skipped() {
    // A pocket corner, relieved as the record is; then its uses altered.
    let r = record(&pocketed());
    let e = vertical_edges(&r, &[[10.0, 8.0]])[0];
    assert_eq!(
        plan(&r, &[e], &[], &options(Style::Normal))
            .unwrap()
            .tools
            .len(),
        1
    );
    let why = |b: &Brep| {
        let p = plan(b, &[e], &[], &options(Style::Normal)).unwrap();
        assert_eq!(p.tools, []);
        assert_eq!(p.skipped.len(), 1, "{:?}", p.skipped);
        (p.skipped[0].edge, p.skipped[0].why)
    };
    // A sheet's edge: one face uses it.
    let mut sheet = r.clone();
    sheet.edges[e].uses.truncate(1);
    assert_eq!(why(&sheet), (Some(e), Why::NotTwoFaces));
    // Three uses of it.
    let mut three = r.clone();
    let u = three.edges[e].uses[0];
    three.edges[e].uses.push(u);
    assert_eq!(why(&three), (Some(e), Why::NotTwoFaces));
    // A seam: one face uses it twice.
    let mut seam = r;
    seam.edges[e].uses[1].face = seam.edges[e].uses[0].face;
    assert_eq!(why(&seam), (Some(e), Why::NotTwoFaces));
}

/// The four corners of a 20 x 20 pocket in a 40 x 40 x 10 plate, counter-clockwise about +z.
const SQUARE: [[f64; 2]; 4] = [[10.0, 10.0], [30.0, 10.0], [30.0, 30.0], [10.0, 30.0]];

#[test]
fn a_t_bone_bites_every_corner_of_a_square_pocket_the_same_way_round() {
    // The walls at each corner are equally long by design.
    let body = cut(
        make_box([0.0; 3], [40.0, 40.0, 10.0]).unwrap(),
        vec![make_box([10.0, 10.0, 4.0], [20.0, 20.0, 7.0]).unwrap()],
    );
    let r = record(&body);
    let edges = vertical_edges(&r, &SQUARE);
    assert_eq!(edges.len(), 4);
    let p = plan(
        &r,
        &edges,
        &[],
        &options(Style::TBone { side: Side::Longer }),
    )
    .unwrap();
    assert!(p.skipped.is_empty(), "{:?}", p.skipped);
    assert_eq!(p.tools.len(), 4);
    // Each relief's centre from its corner: R along one of the walls.
    let offsets: Vec<[f64; 2]> = SQUARE
        .iter()
        .map(|c| {
            let t = p
                .tools
                .iter()
                .find(|t| (t.at[0] - c[0]).abs() < 1e-9 && (t.at[1] - c[1]).abs() < 1e-9)
                .unwrap();
            let o = [t.cylinder.base[0] - c[0], t.cylinder.base[1] - c[1]];
            let along_x = (o[0].abs() - R).abs() < 1e-9 && o[1].abs() < 1e-9;
            let along_y = o[0].abs() < 1e-9 && (o[1].abs() - R).abs() < 1e-9;
            assert!(along_x || along_y, "{t:?}");
            o
        })
        .collect();
    // A quarter turn about the pocket's middle takes each corner to the next; it takes each
    // relief to the next corner's: every corner bites the same way round.
    for (o, next) in offsets.iter().zip(offsets.iter().cycle().skip(1)) {
        assert!(
            (next[0] + o[1]).abs() < 1e-9 && (next[1] - o[0]).abs() < 1e-9,
            "{offsets:?}"
        );
    }
}

#[test]
fn an_edge_is_relieved_alike_either_way_round() {
    let r = record(&pocketed());
    let e = vertical_edges(&r, &[[10.0, 8.0]])[0];
    // The same record with edge `e` running the other way: its ends swapped, each loop using it
    // the other way along it.
    let mut flipped = r.clone();
    flipped.edges[e].vertices.reverse();
    for face in &mut flipped.faces {
        for lp in &mut face.loops {
            if let Loop::Edges { edges } = lp {
                for oe in edges.iter_mut().filter(|oe| oe.edge as usize == e) {
                    oe.forward = !oe.forward;
                }
            }
        }
    }
    let span = |c: Cylinder| {
        if c.axis[2] > 0.0 {
            (c.base[2], c.base[2] + c.height)
        } else {
            (c.base[2] - c.height, c.base[2])
        }
    };
    for style in [
        Style::Normal,
        Style::Minimal { gap: 0.25 },
        Style::TBone { side: Side::Longer },
        Style::TBone {
            side: Side::Shorter,
        },
    ] {
        let a = plan(&r, &[e], &[], &options(style)).unwrap();
        let b = plan(&flipped, &[e], &[], &options(style)).unwrap();
        assert_eq!((a.tools.len(), b.tools.len()), (1, 1), "{a:?} {b:?}");
        let (ca, cb) = (a.tools[0].cylinder, b.tools[0].cylinder);
        assert!(
            near([ca.base[0], ca.base[1], 0.0], [cb.base[0], cb.base[1], 0.0]),
            "{ca:?} {cb:?}"
        );
        let (sa, sb) = (span(ca), span(cb));
        // Blind at the floor (z = 4), open at the top (z = 10), whichever way the edge runs.
        assert!(
            (sa.0 - 4.0).abs() < 1e-9 && (sa.1 - (10.0 + R)).abs() < 1e-9,
            "{ca:?}"
        );
        assert!(
            (sb.0 - 4.0).abs() < 1e-9 && (sb.1 - (10.0 + R)).abs() < 1e-9,
            "{cb:?}"
        );
        assert_eq!(cb.radius, R);
    }
}

#[test]
fn a_relief_runs_past_a_sloped_open_end_by_more_than_the_radius() {
    // The sloped block (tan β = 0.75) with a 20 x 6 slot through it.
    let body = cut(
        sloped(),
        vec![make_box([10.0, 12.0, -1.0], [20.0, 6.0, 40.0]).unwrap()],
    );
    let r = record(&body);
    let corners = [[10.0, 12.0], [30.0, 12.0], [30.0, 18.0], [10.0, 18.0]];
    // One edge up each corner, from the flat bottom (z = 0) to the sloped top.
    let edges = vertical_edges(&r, &corners);
    let pieces: Vec<_> = edges
        .iter()
        .map(|&i| {
            let e = &r.edges[i];
            (point(&r, e.vertices[0]), point(&r, e.vertices[1]))
        })
        .collect();
    assert_eq!(edges.len(), 4, "{pieces:?}");
    let p = plan(&r, &edges, &[], &options(Style::Normal)).unwrap();
    assert!(p.skipped.is_empty(), "{:?} {pieces:?}", p.skipped);
    assert_eq!(p.tools.len(), edges.len(), "{pieces:?}");
    for c in corners {
        let spans: Vec<(f64, f64)> = p
            .tools
            .iter()
            .filter(|t| (t.at[0] - c[0]).abs() < 1e-9 && (t.at[1] - c[1]).abs() < 1e-9)
            .map(|t| {
                let c = t.cylinder;
                if c.axis[2] > 0.0 {
                    (c.base[2], c.base[2] + c.height)
                } else {
                    (c.base[2] - c.height, c.base[2])
                }
            })
            .collect();
        assert!(!spans.is_empty(), "{c:?}: {p:?}");
        let lo = spans.iter().map(|s| s.0).fold(f64::INFINITY, f64::min);
        let hi = spans.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max);
        // Past the bottom by the radius; past the sloped top (z = 2 + 0.75 y) by r + 2r tan β,
        // the reach of the relief's section (2r from the edge line) times the slope, plus r.
        let top = 2.0 + 0.75 * c[1];
        assert!((lo + R).abs() < 1e-9, "{c:?}: {spans:?} {pieces:?}");
        assert!(
            (hi - top - 2.5 * R).abs() < 1e-9,
            "{c:?}: {spans:?} {pieces:?}"
        );
        assert!(hi - top > R, "{c:?}: {spans:?}");
    }
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
