//! SolidCraft #14244, the kernel half: where chamfers meet at a corner, ops builds the mitre
//! (what it always built) or, asked, the chamfered corner: the kernel's Facet, the plane through
//! the chamfers' rail crossings. Closed forms on a box corner and a pyramid's apex.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic,
    clippy::disallowed_methods
)] // test: the pyramid's setback is a square root

mod common;

use common::{all_fixes, built, edge_at};
use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore_ops::session;
use solidcore_ops::sweep::{loft_to_point, Curve};
use solidcore_ops::{
    blend, blend_ordered, blend_with, chamfer_sides_facing, make_box, Blend, BlendOptions, Body,
    ChamferCorner, ChamferSecond, CheckLevel, Corners, Error, FitKind, Order,
};

/// The record's edge indices (the order `blend_ordered` reads) of the edges from `a` to `b`, each
/// pair either way round.
fn edges(body: &Body, pairs: &[([f64; 3], [f64; 3])]) -> (Order, Vec<usize>) {
    let (record, order) = body.to_record().unwrap();
    let at = |i: u32, p: [f64; 3]| {
        let q = record.vertices[i as usize].point;
        (q[0] - p[0]).abs() + (q[1] - p[1]).abs() + (q[2] - p[2]).abs() < 1e-9
    };
    let picked = pairs
        .iter()
        .map(|&(a, b)| {
            record
                .edges
                .iter()
                .position(|e| {
                    let [s, t] = e.vertices;
                    (at(s, a) && at(t, b)) || (at(s, b) && at(t, a))
                })
                .unwrap()
        })
        .collect();
    (order, picked)
}

/// The 40 x 30 x 20 box's corner (40, 30, 20): its three edges.
fn box_corner() -> (Body, Order, Vec<usize>) {
    let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
    let c = [40.0, 30.0, 20.0];
    let (order, e) = edges(
        &b,
        &[
            ([0.0, 30.0, 20.0], c),
            ([40.0, 0.0, 20.0], c),
            ([40.0, 30.0, 0.0], c),
        ],
    );
    (b, order, e)
}

fn chamfer(
    body: &Body,
    order: &Order,
    e: &[usize],
    d: f64,
    corner: ChamferCorner,
) -> Result<(), Error> {
    blend_ordered(
        body,
        order,
        Blend::Chamfer,
        e,
        &[d],
        &[],
        Corners::Filled,
        BlendOptions {
            chamfer_corner: corner,
            ..BlendOptions::default()
        },
    )
    .map(|_| ())
}

/// The box's corner chamfered d = 2 on its three edges. In the corner's frame (`u = 40 - x`,
/// `v = 30 - y`, `w = 20 - z`) the chamfers take `v + w < d` along x, `u + w < d` along y and
/// `u + v < d` along z, each a prism `d^2 / 2` times its edge, `45 d^2` in all. Two of them
/// overlap in `int_0^d (d - w)^2 dw = d^3 / 3` (three pairs), all three in `d^3 / 4` (each
/// slice `(d - w)^2`, less `(d - 2 w)^2 / 2` below `w = d / 2`). The mitre, the three chamfers'
/// union, removes `45 d^2 - d^3 + d^3 / 4 = 174` and adds three faces. The Facet cuts the
/// corner by the plane `u + v + w = 2 d` through the crossings `(d, d, 0)`, `(0, d, d)`,
/// `(d, 0, d)`, a triangle of side `d sqrt 2`; it removes the mitre's and the tetrahedron
/// between the mitre point `(d / 2, d / 2, d / 2)` and those crossings, `d^3 / 12`:
/// `174 + 2 / 3`, and adds a fourth face. Asked nothing, ops builds the mitre, as it always did.
#[test]
fn a_box_corner_s_three_chamfers_mitre_or_take_the_facet() {
    all_fixes();
    let d = 2.0_f64;
    let mitre = 45.0 * d * d - 3.0 * d * d * d / 4.0;
    let facet = mitre + d * d * d / 12.0;
    for (corner, removed, faces) in [
        (None, mitre, 9),
        (Some(ChamferCorner::Mitre), mitre, 9),
        (Some(ChamferCorner::Facet), facet, 10),
    ] {
        let (b, order, e) = box_corner();
        let r = match corner {
            Some(c) => chamfer(&b, &order, &e, d, c),
            None => blend_ordered(
                &b,
                &order,
                Blend::Chamfer,
                &e,
                &[d],
                &[],
                Corners::Filled,
                BlendOptions::default(),
            )
            .map(|_| ()),
        };
        r.unwrap();
        let v = b.volume(1e-10).unwrap();
        let want = 24000.0 - removed;
        assert!((v - want).abs() <= 1e-9 * want, "{corner:?}: {v} vs {want}");
        assert_eq!(b.counts().unwrap()[0], faces, "{corner:?}");
    }
}

/// The square pyramid of base 20 about the origin and apex (0, 0, 10), its four apex edges
/// chamfered at the distance `2 sqrt(2 / 3)` (the kernel's `pyramid_apex_of_four_equal_chamfers_
/// is_a_facet`: radius `2 sqrt 2` across the 120 degree edges, `r / sqrt 3` along each face). The
/// crossings `(+-2, 0, 8)`, `(0, +-2, 8)` lie in the plane `z = 8`, so the Facet builds; the
/// solid left is `3776 / 3`. Four chamfers' mitre is not designed and is refused.
#[test]
fn a_pyramid_apex_s_four_chamfers_take_the_facet() {
    all_fixes();
    let square = [[10.0, 10.0], [-10.0, 10.0], [-10.0, -10.0], [10.0, -10.0]];
    let section: Vec<Curve> = (0..4)
        .map(|i| {
            let (a, b) = (square[i], square[(i + 1) % 4]);
            Curve::Line {
                start: [a[0], a[1], 0.0],
                end: [b[0], b[1], 0.0],
            }
        })
        .collect();
    let apex = [0.0, 0.0, 10.0];
    let pyramid = || loft_to_point(&section, apex).unwrap();
    let pick = |b: &Body| edges(b, &square.map(|c| ([c[0], c[1], 0.0], apex)));
    let d = 2.0 * (2.0_f64 / 3.0).sqrt();
    let b = pyramid();
    let (order, e) = pick(&b);
    chamfer(&b, &order, &e, d, ChamferCorner::Facet).unwrap();
    let v = b.volume(1e-10).unwrap();
    let want = 3776.0 / 3.0;
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    let b = pyramid();
    let (order, e) = pick(&b);
    assert!(chamfer(&b, &order, &e, d, ChamferCorner::Mitre).is_err());
}

/// The blend corner's chamfer event is the only chamfer corner: with no fix turned on, the box
/// corner's mitre and Facet are the closed forms of
/// [`a_box_corner_s_three_chamfers_mitre_or_take_the_facet`] (the Facet once needed a fix and was
/// refused without it).
#[test]
fn the_box_corner_s_mitre_and_facet_need_no_fix() {
    session::set_thread_fixes(std::iter::empty()).unwrap();
    let d = 2.0_f64;
    let mitre = 45.0 * d * d - 3.0 * d * d * d / 4.0;
    let facet = mitre + d * d * d / 12.0;
    for (corner, removed, faces) in [
        (ChamferCorner::Mitre, mitre, 9),
        (ChamferCorner::Facet, facet, 10),
    ] {
        let (b, order, e) = box_corner();
        chamfer(&b, &order, &e, d, corner).unwrap();
        let v = b.volume(1e-10).unwrap();
        let want = 24000.0 - removed;
        assert!((v - want).abs() <= 1e-9 * want, "{corner:?}: {v} vs {want}");
        assert_eq!(b.counts().unwrap()[0], faces, "{corner:?}");
    }
    all_fixes();
}

/// The Facet is the chamfers' corner type: on a round it is an argument error.
#[test]
fn the_facet_on_a_round_is_refused() {
    all_fixes();
    let (b, order, e) = box_corner();
    let r = blend_ordered(
        &b,
        &order,
        Blend::Round,
        &e,
        &[2.0],
        &[],
        Corners::Filled,
        BlendOptions {
            chamfer_corner: ChamferCorner::Facet,
            ..BlendOptions::default()
        },
    );
    assert_eq!(r.err(), Some(Error::Argument));
}

/// The closed form of 3.1 for unequal distances: sizes 2, 3 and 4 on the box's edges along x,
/// y and z at the corner (40, 30, 20), so the sorted sizes are `a = 2` (edge 40), `b = 3` (edge
/// 30), `c = 4` (edge 20). The prisms give `(a^2 L_a + b^2 L_b + c^2 L_c) / 2 = 375`; the pair
/// overlaps are `P(a, c) = 20/3` and `P(b, c) = 27/2`, and `(a + b - c)^3 / 12 = 1/12`. The
/// mitre removes `375 - 20/3 - 27/2 - 1/12 = 354.75` and adds three faces (nine in all). The
/// facet adds `(b + c - a)(a + c - b)(a + b - c) / 12 = 5 * 3 * 1 / 12 = 5/4`, for `356`, and a
/// fourth face (ten in all).
#[test]
fn a_box_corner_s_unequal_chamfers_mitre_or_take_the_facet() {
    all_fixes();
    for (corner, removed, faces) in [
        (ChamferCorner::Mitre, 354.75, 9),
        (ChamferCorner::Facet, 356.0, 10),
    ] {
        let (b, order, e) = box_corner();
        blend_ordered(
            &b,
            &order,
            Blend::Chamfer,
            &e,
            &[2.0, 3.0, 4.0],
            &[],
            Corners::Filled,
            BlendOptions {
                chamfer_corner: corner,
                ..BlendOptions::default()
            },
        )
        .unwrap();
        let v = b.volume(1e-10).unwrap();
        let want = 24000.0 - removed;
        assert!((v - want).abs() <= 1e-9 * want, "{corner:?}: {v} vs {want}");
        assert_eq!(b.counts().unwrap()[0], faces, "{corner:?}");
    }
}

/// The closed form of 3.1 for two-distance chamfers: distance 2 along the face facing +z and 3
/// along the other face, on each of the box's three edges at the corner (40, 30, 20). The
/// planes are `v/2 + w/3 = 1`, `u/2 + w/3 = 1` and `u/2 + v/3 = 1` (the corner's frame of the
/// test above). A slice at depth `w` is strips of width `a = 2 (1 - w/3)` with area `70 a - a^2`,
/// plus the triangle of the third chamfer (area 3) less its part under the strips, which shows
/// outside the strips only for `a < 6/5`, by `(3 - 5 a / 2)^2 / 3`. Removed: `206 + 51 + 9/5 =
/// 258.8`. The result has nine faces, twenty-one edges and fourteen vertices.
#[test]
fn a_box_corner_s_two_distance_chamfers_mitre() {
    all_fixes();
    let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
    let e = [[20.0, 30.0, 20.0], [40.0, 15.0, 20.0], [40.0, 30.0, 10.0]].map(|p| edge_at(&b, p));
    chamfer_sides_facing(&b, &e, 2.0, ChamferSecond::Distance(3.0), [0.0, 0.0, 1.0]).unwrap();
    let v = b.volume(1e-10).unwrap();
    let want = 24000.0 - 258.8;
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    assert_eq!(b.counts().unwrap(), [9, 21, 14]);
}

/// Sizes 1, 2 and 3 on the box's corner: the mitre point of the three planes would lie outside
/// the corner, since 3 = 1 + 2 (the chamfer along z is as wide as the other two together). The
/// refusal names the largest size that builds, 3 less the fit margin (1000 edge tolerances, so
/// just under 3), and the edge's middle `(40, 30, 10)`. The body is unchanged by the refusal.
#[test]
fn a_chamfer_as_wide_as_the_other_two_together_is_refused_with_its_fit() {
    all_fixes();
    for corner in [ChamferCorner::Mitre, ChamferCorner::Facet] {
        let (b, order, e) = box_corner();
        let err = blend_ordered(
            &b,
            &order,
            Blend::Chamfer,
            &e,
            &[1.0, 2.0, 3.0],
            &[],
            Corners::Filled,
            BlendOptions {
                chamfer_corner: corner,
                ..BlendOptions::default()
            },
        )
        .err();
        match err {
            Some(Error::TooLarge {
                size,
                kind: FitKind::Distance,
                at: Some(at),
                ..
            }) => {
                assert!(size < 3.0 && size > 2.98, "{corner:?}: fit {size}");
                assert_eq!(at, [40.0, 30.0, 10.0], "{corner:?}");
            }
            other => panic!("{corner:?}: refused as {other:?}"),
        }
        let v = b.volume(1e-10).unwrap();
        assert!((v - 24000.0).abs() <= 1e-9 * 24000.0, "{corner:?}: {v}");
        assert_eq!(b.counts().unwrap(), [6, 12, 8], "{corner:?}");
    }
}

/// The round of radius 2 on the box's edge along x, then the chamfers of distance 2 on the
/// edges along y and z together. Expected: the union of the three removed regions,
/// `130.10753002` (`docs/generic-blend.md` section 3.4), nine faces.
#[test]
#[ignore = "#17893, a later PR: two blends that end on an earlier blend's end cross and are not mitred; fails with 'the trim leaks'"]
fn two_chamfers_made_after_a_round_at_their_corner_mitre_on_it() {
    all_fixes();
    let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
    blend(
        &b,
        Blend::Round,
        &[edge_at(&b, [20.0, 30.0, 20.0])],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    let e = [[40.0, 15.0, 20.0], [40.0, 30.0, 10.0]].map(|p| edge_at(&b, p));
    blend(&b, Blend::Chamfer, &e, &[2.0], &[], Corners::Filled).unwrap();
    let v = b.volume(1e-10).unwrap();
    let want = 24000.0 - 130.107_530_022_738;
    assert!((v - want).abs() <= 1e-8 * want, "{v} vs {want}");
    assert_eq!(b.counts().unwrap()[0], 9);
}

/// The chamfer of distance 2 on the box's edge along x, then the chamfers of distance 2 on the
/// edges along y and z together. Expected: the mitre of three equal chamfers, `174`, nine faces.
#[test]
#[ignore = "#17893, a later PR: as the round's case, fails with status 1001 (boolean::topology_intersector::p3::merge_curve_classes)"]
fn two_chamfers_made_after_a_chamfer_at_their_corner_are_the_corner_s_mitre() {
    all_fixes();
    let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
    blend(
        &b,
        Blend::Chamfer,
        &[edge_at(&b, [20.0, 30.0, 20.0])],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    let e = [[40.0, 15.0, 20.0], [40.0, 30.0, 10.0]].map(|p| edge_at(&b, p));
    blend(&b, Blend::Chamfer, &e, &[2.0], &[], Corners::Filled).unwrap();
    let v = b.volume(1e-10).unwrap();
    let want = 24000.0 - 174.0;
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    assert_eq!(b.counts().unwrap()[0], 9);
}

/// The round of radius 2 on the box's edge along x, then the chamfer of distance 2 on the edge
/// along y, then on the edge along z: expected the same solid as the round and the two chamfers
/// together, nine faces.
#[test]
#[ignore = "#17893, a later PR: the third blend fails with 'Fillet Corner Error: Unable to Make Topology'"]
fn chamfers_made_one_at_a_time_after_a_round_are_the_same_solid() {
    all_fixes();
    let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
    blend(
        &b,
        Blend::Round,
        &[edge_at(&b, [20.0, 30.0, 20.0])],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    blend(
        &b,
        Blend::Chamfer,
        &[edge_at(&b, [40.0, 15.0, 20.0])],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    blend(
        &b,
        Blend::Chamfer,
        &[edge_at(&b, [40.0, 30.0, 10.0])],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    let v = b.volume(1e-10).unwrap();
    let want = 24000.0 - 130.107_530_022_738;
    assert!((v - want).abs() <= 1e-8 * want, "{v} vs {want}");
    assert_eq!(b.counts().unwrap()[0], 9);
}

/// Unequal chamfers at a vertex where a cylinder cuts the box: the two planes and the cylinder
/// meet at the corner (40, y, 20), y = sqrt(45^2 - 40^2). Sizes 1, 2 and 1 on its three edges.
/// Expected: ten faces (seven before) and a valid body.
#[test]
#[ignore = "#17893, the curved-face PR: the pinned bevel does not find the chamfers' common point; fails with 'Vertex Computation Failure'"]
fn unequal_chamfers_at_a_vertex_beside_a_cylinder_build() {
    all_fixes();
    let b = built(|k| {
        let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0)?;
        let cyl = create_cylinder(k, Point3::new(0.0, 0.0, -5.0), 45.0, 30.0)?;
        boolean_intersection(k, block, cyl)
    });
    let y = (45.0_f64.powi(2) - 1600.0).sqrt();
    let e = [
        [40.0, y, 10.0],
        [40.0, y / 2.0, 20.0],
        [45.0 * 0.55_f64.cos(), 45.0 * 0.55_f64.sin(), 20.0],
    ]
    .map(|p| edge_at(&b, p));
    blend(
        &b,
        Blend::Chamfer,
        &e,
        &[1.0, 2.0, 1.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert_eq!(b.counts().unwrap()[0], 10);
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
}

/// The pyramid of base 20 about (0, 0, 10): three of its four apex edges chamfered 1.5. The
/// expected result is a solid; the brief gives no volume to pin, so only that it is one.
#[test]
#[ignore = "#17893 names this as a separate issue (apex of three of four edges): fails with NotSolid, a body of volume 0"]
fn three_chamfers_of_a_pyramid_s_apex_build_a_solid() {
    all_fixes();
    let square = [[10.0, 10.0], [-10.0, 10.0], [-10.0, -10.0], [10.0, -10.0]];
    let section: Vec<Curve> = (0..4)
        .map(|i| {
            let (a, b) = (square[i], square[(i + 1) % 4]);
            Curve::Line {
                start: [a[0], a[1], 0.0],
                end: [b[0], b[1], 0.0],
            }
        })
        .collect();
    let apex = [0.0, 0.0, 10.0];
    let b = loft_to_point(&section, apex).unwrap();
    let (order, e) = edges(&b, &square.map(|c| ([c[0], c[1], 0.0], apex)));
    blend_ordered(
        &b,
        &order,
        Blend::Chamfer,
        &e[..3],
        &[1.5],
        &[],
        Corners::Filled,
        BlendOptions::default(),
    )
    .unwrap();
    b.check_solid().unwrap();
}

/// The L block (the box less a notch at its top), its inner top vertex (20, 15, 20) where the
/// upright is concave and the two top edges convex: three chamfers of both convexities, which
/// the kernel refuses as bevels at that vertex. ops refuses them with the kernel's reason and
/// the vertex's point, and the body is unchanged.
#[test]
fn an_l_block_s_inner_corner_of_three_chamfers_is_refused_naming_the_vertex() {
    all_fixes();
    for corner in [ChamferCorner::Mitre, ChamferCorner::Facet] {
        let b = built(|k| {
            let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0)?;
            let cut = create_box(k, Point3::new(20.0, 15.0, -1.0), 21.0, 16.0, 22.0)?;
            boolean_difference(k, block, cut)
        });
        let e =
            [[20.0, 15.0, 10.0], [30.0, 15.0, 20.0], [20.0, 22.0, 20.0]].map(|p| edge_at(&b, p));
        let r = blend_with(
            &b,
            Blend::Chamfer,
            &e,
            &[2.0],
            &[],
            Corners::Filled,
            BlendOptions {
                chamfer_corner: corner,
                ..BlendOptions::default()
            },
        );
        match r {
            Err(Error::CornerUnsupported {
                why: Some(why),
                at: Some(at),
            }) if why.contains("chamfers of convex and of concave edges meet at the vertex")
                && at == [20.0, 15.0, 20.0] => {}
            other => panic!("{corner:?}: refused as {other:?}"),
        }
        let v = b.volume(1e-10).unwrap();
        assert!((v - 18000.0).abs() <= 1e-9 * 18000.0, "{corner:?}: {v}");
        assert_eq!(b.counts().unwrap()[0], 8, "{corner:?}");
    }
}
