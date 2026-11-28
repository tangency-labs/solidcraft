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
use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore_ops::session;
use solidcore_ops::sweep::{extrude, loft_to_point, Curve, DraftCorner};
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

/// Eight chamfers on a 20 mm cube remove eight prisms of volume 20 * 2^2 / 2.
/// At each top corner, inclusion-exclusion adds back 3 * 2^3 / 3 - 2^3 / 4 = 6.
/// The remaining volume is 8000 - 8 * 40 + 4 * 6 = 7704, with eight added faces.
#[test]
fn top_and_vertical_chamfers_build_in_either_order() {
    all_fixes();
    let top = [
        [10.0, 0.0, 20.0],
        [10.0, 20.0, 20.0],
        [0.0, 10.0, 20.0],
        [20.0, 10.0, 20.0],
    ];
    let vertical = [
        [0.0, 0.0, 9.0],
        [20.0, 0.0, 9.0],
        [0.0, 20.0, 9.0],
        [20.0, 20.0, 9.0],
    ];
    for picks in [&[vertical, top][..], &[top, vertical][..]] {
        let b = make_box([0.0; 3], [20.0; 3]).unwrap();
        for points in picks {
            let picked: Vec<_> = points.iter().map(|&p| edge_at(&b, p)).collect();
            blend(&b, Blend::Chamfer, &picked, &[2.0], &[], Corners::Filled).unwrap();
        }
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
        assert_eq!(b.counts().unwrap(), [14, 36, 24]);
        let v = b.volume(1e-10).unwrap();
        assert!((v - 7704.0).abs() <= 1e-9 * 7704.0, "{v} vs 7704");
    }
}

/// Before the top chamfers meet, each horizontal slice takes four disjoint strips from
/// the vertically chamfered octagon. At width t their area is 4 * (16 t + t^2), so a
/// top setback s removes 32 s^2 + 4 s^3 / 3 from 7840. At s = 1 the short top edges
/// of the vertical chamfers collapse; beyond that size the new chamfers need shared edges.
#[test]
fn top_chamfers_before_the_end_collision_match_the_octagon_slices() {
    all_fixes();
    let vertical = [
        [0.0, 0.0, 9.0],
        [20.0, 0.0, 9.0],
        [0.0, 20.0, 9.0],
        [20.0, 20.0, 9.0],
    ];
    let top = [
        [10.0, 0.0, 20.0],
        [10.0, 20.0, 20.0],
        [0.0, 10.0, 20.0],
        [20.0, 10.0, 20.0],
    ];
    for (s, want, counts) in [
        (0.5, 46991.0 / 6.0, [14, 36, 24]),
        (1.0, 23420.0 / 3.0, [14, 32, 20]),
    ] {
        let b = make_box([0.0; 3], [20.0; 3]).unwrap();
        for (points, distance) in [(vertical, 2.0), (top, s)] {
            let picked: Vec<_> = points.iter().map(|&p| edge_at(&b, p)).collect();
            blend(
                &b,
                Blend::Chamfer,
                &picked,
                &[distance],
                &[],
                Corners::Filled,
            )
            .unwrap();
        }
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
        assert_eq!(b.counts().unwrap(), counts);
        let v = b.volume(1e-10).unwrap();
        assert!((v - want).abs() <= 1e-9 * want, "s={s}: {v} vs {want}");
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

/// The L block: the 40 x 30 x 20 box less the notch `x > 20, y > 15`, volume 18000, 8 faces.
fn l_block() -> Body {
    built(|k| {
        let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0)?;
        let cut = create_box(k, Point3::new(20.0, 15.0, -1.0), 21.0, 16.0, 22.0)?;
        boolean_difference(k, block, cut)
    })
}

/// The L block, its inner top vertex V = (20, 15, 20) where the upright is concave and the two
/// top edges convex: three chamfers of both convexities, mitred (SolidCraft #23136).
///
/// In V's frame (`u = x - 20`, `v = y - 15`, `w = 20 - z`) the material is `u < 0` or `v < 0`,
/// the top face its reflex face. The concave chamfer adds the wedge `u, v > 0, u + v < s` the
/// whole height: `20 s^2 / 2`. The convex chamfers' planes `v = w - s` and `u = w - s` run on
/// over that wedge and take everything above both: at depth `w < s` the part `u > w - s,
/// v > w - s` of the L (the strips along the top edges, of lengths 20 and 15, the square between
/// them at V and the wedge's whole section): `(s - w) (35 + s - w) + s^2 / 2`, `35 s^2 / 2 +
/// 5 s^3 / 6` in all; at depth `s < w < 3 s / 2` what is left of the wedge between the planes,
/// `(3 s - 2 w)^2 / 2`, `s^3 / 12` in all, down to the mitre point `(s / 2, s / 2, 3 s / 2)`.
/// So the solid is `18000 - 15 s^2 / 2 - 11 s^3 / 12`, with three more faces. The mitre is the
/// corner whether asked or not; a Facet asked there is refused, naming the vertex, and leaves
/// the body as it was.
#[test]
fn an_l_block_s_inner_corner_of_three_chamfers_is_mitred() {
    all_fixes();
    let s = 2.0_f64;
    let want = 18000.0 - 7.5 * s * s - 11.0 * s * s * s / 12.0;
    for corner in [None, Some(ChamferCorner::Mitre)] {
        let b = l_block();
        let e =
            [[20.0, 15.0, 10.0], [30.0, 15.0, 20.0], [20.0, 22.0, 20.0]].map(|p| edge_at(&b, p));
        match corner {
            None => blend(&b, Blend::Chamfer, &e, &[s], &[], Corners::Filled).map(|_| ()),
            Some(corner) => blend_with(
                &b,
                Blend::Chamfer,
                &e,
                &[s],
                &[],
                Corners::Filled,
                BlendOptions {
                    chamfer_corner: corner,
                    ..BlendOptions::default()
                },
            )
            .map(|_| ()),
        }
        .unwrap();
        let v = b.volume(1e-10).unwrap();
        assert!((v - want).abs() <= 1e-9 * want, "{corner:?}: {v} vs {want}");
        assert_eq!(b.counts().unwrap()[0], 11, "{corner:?}");
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "{corner:?}");
    }

    let b = l_block();
    let e = [[20.0, 15.0, 10.0], [30.0, 15.0, 20.0], [20.0, 22.0, 20.0]].map(|p| edge_at(&b, p));
    let r = blend_with(
        &b,
        Blend::Chamfer,
        &e,
        &[s],
        &[],
        Corners::Filled,
        BlendOptions {
            chamfer_corner: ChamferCorner::Facet,
            ..BlendOptions::default()
        },
    );
    match r {
        Err(Error::CornerUnsupported {
            why: Some(why),
            at: Some(at),
        }) if why.contains("a facet of chamfers of convex and of concave edges is not built")
            && at == [20.0, 15.0, 20.0] => {}
        other => panic!("the facet: refused as {other:?}"),
    }
    let v = b.volume(1e-10).unwrap();
    assert!((v - 18000.0).abs() <= 1e-9 * 18000.0, "the facet: {v}");
    assert_eq!(b.counts().unwrap()[0], 8, "the facet");
}

/// The L block's upright chamfered with the top edges and the bottom edges at both its ends:
/// two corners of both convexities that share the concave chamfer. The top and the bottom each
/// take the strips and the corner of
/// [`an_l_block_s_inner_corner_of_three_chamfers_is_mitred`], and the wedge is added once:
/// `18000 + 10 s^2 - 2 (35 s^2 / 2 + 11 s^3 / 12)`, with five more faces.
#[test]
fn an_l_block_s_inner_corners_at_the_top_and_the_bottom_are_mitred_together() {
    all_fixes();
    let s = 2.0_f64;
    let want = 18000.0 + 10.0 * s * s - 2.0 * (17.5 * s * s + 11.0 * s * s * s / 12.0);
    let b = l_block();
    let e = [
        [20.0, 15.0, 10.0],
        [30.0, 15.0, 20.0],
        [20.0, 22.0, 20.0],
        [30.0, 15.0, 0.0],
        [20.0, 22.0, 0.0],
    ]
    .map(|p| edge_at(&b, p));
    blend(&b, Blend::Chamfer, &e, &[s], &[], Corners::Filled).unwrap();
    let v = b.volume(1e-10).unwrap();
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    assert_eq!(b.counts().unwrap()[0], 13);
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
}

/// A pillar 20 x 20 x 20 on a plate 40 x 40 x 4, at the plate's corner: at V = (20, 20, 4) the
/// pillar's upright edge is convex and its two foot edges concave, the plate's top the reflex
/// face. The material's complement there is the L block's corner, so the three chamfers' mitre
/// is that corner with material and space exchanged: the foot chamfers add `s^2 / 2` times
/// their lengths (20 each), the upright's takes `s^2 / 2` times its length (20), and the corner
/// adds `11 s^3 / 12`: `14400 + 10 s^2 + 11 s^3 / 12`, with three more faces.
#[test]
fn a_pillar_s_foot_corner_of_three_chamfers_is_mitred() {
    all_fixes();
    let s = 2.0_f64;
    let want = 14400.0 + 10.0 * s * s + 11.0 * s * s * s / 12.0;
    let b = built(|k| {
        let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 4.0)?;
        let pillar = create_box(k, Point3::new(0.0, 0.0, 4.0), 20.0, 20.0, 20.0)?;
        boolean_union(k, plate, pillar)
    });
    let faces = b.counts().unwrap()[0];
    let e = [[20.0, 20.0, 14.0], [10.0, 20.0, 4.0], [20.0, 10.0, 4.0]].map(|p| edge_at(&b, p));
    blend(&b, Blend::Chamfer, &e, &[s], &[], Corners::Filled).unwrap();
    let v = b.volume(1e-10).unwrap();
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    assert_eq!(b.counts().unwrap()[0], faces + 3);
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
}

/// The L block's inner top corner with the concave chamfer 0.5 and the convex ones 1 and 2.
/// The mitre point lies between the concave chamfer's rails only while the convex chamfers'
/// sizes differ by less than the concave one's (in V's frame of
/// [`an_l_block_s_inner_corner_of_three_chamfers_is_mitred`] the planes `u + v = c`,
/// `v = w - a` and `u = w - b` meet at `u = (c + a - b) / 2`, which is positive for
/// `b < a + c`). So the chamfer of 2 is too wide and builds up to 1.5, less the fit margin,
/// asked a corner type or not; the refusal names its edge's middle and leaves the body as it
/// was. With 1.4 in its place the corner builds, a valid body of three more faces.
#[test]
fn unequal_convex_chamfers_at_an_l_block_s_inner_corner_are_refused_past_their_fit() {
    all_fixes();
    for corner in [None, Some(ChamferCorner::Mitre)] {
        let b = l_block();
        let e =
            [[20.0, 15.0, 10.0], [30.0, 15.0, 20.0], [20.0, 22.5, 20.0]].map(|p| edge_at(&b, p));
        let options = BlendOptions {
            chamfer_corner: corner.unwrap_or(ChamferCorner::Mitre),
            ..BlendOptions::default()
        };
        let r = match corner {
            None => blend(
                &b,
                Blend::Chamfer,
                &e,
                &[0.5, 1.0, 2.0],
                &[],
                Corners::Filled,
            )
            .map(|_| ()),
            Some(_) => blend_with(
                &b,
                Blend::Chamfer,
                &e,
                &[0.5, 1.0, 2.0],
                &[],
                Corners::Filled,
                options,
            )
            .map(|_| ()),
        };
        match r {
            Err(Error::TooLarge {
                size,
                kind: FitKind::Distance,
                at: Some(at),
                ..
            }) => {
                assert!(size < 1.5 && size > 1.48, "{corner:?}: fit {size}");
                // the middle of the edge from (20, 15, 20) to (20, 30, 20), to rounding
                let off = (at[0] - 20.0).abs() + (at[1] - 22.5).abs() + (at[2] - 20.0).abs();
                assert!(off < 1e-9, "{corner:?}: at {at:?}");
            }
            other => panic!("{corner:?}: refused as {other:?}"),
        }
        let v = b.volume(1e-10).unwrap();
        assert!((v - 18000.0).abs() <= 1e-9 * 18000.0, "{corner:?}: {v}");
        assert_eq!(b.counts().unwrap()[0], 8, "{corner:?}");
    }

    let b = l_block();
    let e = [[20.0, 15.0, 10.0], [30.0, 15.0, 20.0], [20.0, 22.5, 20.0]].map(|p| edge_at(&b, p));
    blend(
        &b,
        Blend::Chamfer,
        &e,
        &[0.5, 1.0, 1.4],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert_eq!(b.counts().unwrap()[0], 11);
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
}

/// Past s = 1 the bands of two top chamfers reach past each other across the short top edge
/// of the vertical chamfer between them, and their planes meet that chamfer's plane at
/// (1, 1, 21 - s), joined to their rails' crossing (s, s, 20) by a new edge. The four
/// vertical prisms remove 160; the four top prisms 40 s^2, less their four pairwise corner
/// overlaps s^3 / 3; each top prism shares s^2 - s^3 / 6 with each of the two vertical
/// prisms at its ends, and each corner's three-way overlap is s^3 / 3 - 2 (s - 1)^3 / 3
/// (each slice of side a = s - w of the two strips, less the corner x + y < 2 cuts off
/// past a = 1). So the solid is 7840 - 32 s^2 - 4 s^3 / 3 + 8 (s - 1)^3 / 3 for 1 <= s <= 2,
/// 7704 at s = 2 as in the eight-edge chamfer, with 14 faces, 36 edges and 24 vertices.
#[test]
fn top_chamfers_past_the_end_collision_meet_on_the_vertical_chamfers() {
    all_fixes();
    let vertical = [
        [0.0, 0.0, 9.0],
        [20.0, 0.0, 9.0],
        [0.0, 20.0, 9.0],
        [20.0, 20.0, 9.0],
    ];
    let top = [
        [10.0, 0.0, 20.0],
        [10.0, 20.0, 20.0],
        [0.0, 10.0, 20.0],
        [20.0, 10.0, 20.0],
    ];
    for s in [1.2, 1.5, 2.0] {
        let want = 7840.0 - 32.0 * s * s - 4.0 * s * s * s / 3.0
            + 8.0 * (s - 1.0) * (s - 1.0) * (s - 1.0) / 3.0;
        let b = make_box([0.0; 3], [20.0; 3]).unwrap();
        for (points, distance) in [(vertical, 2.0), (top, s)] {
            let picked: Vec<_> = points.iter().map(|&p| edge_at(&b, p)).collect();
            blend(
                &b,
                Blend::Chamfer,
                &picked,
                &[distance],
                &[],
                Corners::Filled,
            )
            .unwrap();
        }
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
        assert_eq!(b.counts().unwrap(), [14, 36, 24], "s={s}");
        let v = b.volume(1e-10).unwrap();
        assert!((v - want).abs() <= 1e-9 * want, "s={s}: {v} vs {want}");
    }
}

/// A trapezoid prism (top face z = 10, corners (-1, 0), (1, 0), (3, 10), (-3, 10)), its two
/// sloping top edges chamfered at 1.2. Their bands reach past each other across the short top
/// edge but not the long one, so which connector is tried first, which follows the edges'
/// orientations, must not matter: the profile drawn either way round (every edge reversed)
/// chamfers to the same valid solid.
#[test]
fn sloping_top_chamfers_do_not_depend_on_the_edges_orientation() {
    all_fixes();
    let corners = [[-1.0, 0.0], [1.0, 0.0], [3.0, 10.0], [-3.0, 10.0]];
    let mut results = Vec::new();
    for reversed in [false, true] {
        let sides: Vec<Curve> = (0..4)
            .map(|i| {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                let (a, b) = if reversed { (b, a) } else { (a, b) };
                Curve::Line {
                    start: [a[0], a[1], 10.0],
                    end: [b[0], b[1], 10.0],
                }
            })
            .collect();
        let b = extrude(
            &sides,
            [0.0, 0.0, -1.0],
            10.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        let picked: Vec<_> = [[2.0, 5.0, 10.0], [-2.0, 5.0, 10.0]]
            .iter()
            .map(|&p| edge_at(&b, p))
            .collect();
        blend(&b, Blend::Chamfer, &picked, &[1.2], &[], Corners::Filled).unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
        results.push((b.counts().unwrap(), b.volume(1e-10).unwrap()));
    }
    assert_eq!(results[0].0, results[1].0);
    let (v, w) = (results[0].1, results[1].1);
    assert!((v - w).abs() <= 1e-9 * v, "{v} vs {w}");
}
