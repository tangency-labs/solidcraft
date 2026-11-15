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

use common::all_fixes;
use solidcore::kernel::UpstreamFixes;
use solidcore_ops::session;
use solidcore_ops::sweep::{loft_to_point, Curve};
use solidcore_ops::{
    blend_ordered, make_box, Blend, BlendOptions, Body, ChamferCorner, Corners, Error, Order,
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

/// The Facet is the kernel's blend corner's, under its `fillet_chamfer_corner` fix: without it
/// the Facet is refused, not built as something else; on a round it is an argument error.
#[test]
fn the_facet_without_its_fix_or_on_a_round_is_refused() {
    session::set_thread_fixes(
        UpstreamFixes::NAMES
            .iter()
            .copied()
            .filter(|n| *n != "fillet_chamfer_corner"),
    )
    .unwrap();
    let (b, order, e) = box_corner();
    assert_eq!(
        chamfer(&b, &order, &e, 2.0, ChamferCorner::Facet).err(),
        Some(Error::Unsupported(
            "the chamfered corner needs the kernel's fillet_chamfer_corner fix"
        ))
    );
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
