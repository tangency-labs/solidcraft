//! #3012: a 40 x 30 x 10 box with its four vertical edges rounded R2, less the same shape
//! (an extruded rounded rectangle) shifted along x by dx, failed in both orders: the
//! boolean read the two fillets as coincident faces at their mated tangent edge, though
//! they part by up to dx along the surface, and the region of a face found coincident
//! there was the wrong side's. The result is one body of the sliver, dx * 30 * 10 mm3
//! less its two tapered ends. Its own test binary: the fix set is the process kernel's.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]

mod common;

use common::{edge_at, kernel_copy, volume};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::face::p2 as face;
use solidcore::topology::vertex::corner;
use solidcore_ops::boolean::boolean;
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::{blend, make_box, session, Blend, Body, BooleanOp, CheckLevel, Corners};
use std::f64::consts::{FRAC_PI_2, PI};

fn rounded_box() -> Body {
    let b = make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap();
    let edges: Vec<usize> = [[0.0, 0.0], [40.0, 0.0], [0.0, 30.0], [40.0, 30.0]]
        .iter()
        .map(|p| edge_at(&b, [p[0], p[1], 5.0]))
        .collect();
    blend(&b, Blend::Round, &edges, &[2.0], &[], Corners::Filled).unwrap();
    b
}

/// The same outline as a rounded rectangle, shifted by `dx`, extruded.
fn extruded(dx: f64) -> Body {
    let (x0, x1, y1, r) = (dx, 40.0 + dx, 30.0, 2.0);
    let line = |s: [f64; 2], e: [f64; 2]| Curve::Line {
        start: [s[0], s[1], 0.0],
        end: [e[0], e[1], 0.0],
    };
    let arc = |c: [f64; 2], start: f64| Curve::Arc {
        center: [c[0], c[1], 0.0],
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: r,
        start,
        end: start + FRAC_PI_2,
    };
    let outline = [
        line([x0 + r, 0.0], [x1 - r, 0.0]),
        arc([x1 - r, r], -FRAC_PI_2),
        line([x1, r], [x1, y1 - r]),
        arc([x1 - r, y1 - r], 0.0),
        line([x1 - r, y1], [x0 + r, y1]),
        arc([x0 + r, y1 - r], FRAC_PI_2),
        line([x0, y1 - r], [x0, r]),
        arc([x0 + r, r], PI),
    ];
    extrude(
        &outline,
        [0.0, 0.0, 1.0],
        10.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap()
}

#[test]
fn issue_3012_cut_of_filleted_boxes_a_sliver_apart() {
    // the fillets of both bodies are cones, not surfaces of extrusion
    use_the_two_fixes();
    for dx in [1e-4, 1e-3, 1e-2] {
        for reversed in [false, true] {
            let (a, b) = (rounded_box(), extruded(dx));
            let (x, y) = if reversed { (b, a) } else { (a, b) };
            let r = boolean(x, y, BooleanOp::Subtract)
                .unwrap_or_else(|e| panic!("dx {dx} reversed {reversed}: {e}"))
                .unwrap();
            let report = r.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "dx {dx} reversed {reversed}: {report}");
            assert_eq!(r.solid_count().unwrap(), 1, "dx {dx} reversed {reversed}");
            // The sliver is dx thick over the 30 of the outline's height, so its volume is
            // dx * 30 * 10. At each end the fillets of the two bodies cross dx / 2 from their
            // tangent point and the sliver is cut off there: a triangle of base dx and length
            // dx / 2 per end, 2 ends x 10 high, so at most 10 dx^2 is missing (measured 5 dx^2:
            // 4.8e-8 at 1e-4, 5.0e-6 at 1e-3). 1e-8 is the tolerance the volume is asked for.
            let h = 10.0;
            let expected = dx * 30.0 * h;
            let v = volume(&r);
            assert!(
                (expected - v) > -1e-8 && (expected - v) < 10.0 * dx * dx + 1e-8,
                "dx {dx} reversed {reversed}: {v} vs {expected}"
            );
            // Every face of the sliver: the two flat sides (26 x 10 each), the four fillets (a
            // quarter turn of R2 by 10 each) and the two caps (the sliver's section, V / 10
            // each). A fillet face the sector skipped and a neighbouring edge put in the wrong
            // region would add or drop 31.4. Each of the four fillet pieces ends where the two
            // fillets cross, dx / 2 of arc short, over 10: 20 dx at most. 1e-7 is the area
            // routine's slack (measured 4e-8).
            let faces = 2.0 * 26.0 * 10.0 + 4.0 * FRAC_PI_2 * 2.0 * 10.0 + 2.0 * v / h;
            let area = r.area(1e-8).unwrap();
            // The fillets' pieces are each exactly dx / 2 of arc short over 10 (4 x 5 dx = 20 dx),
            // up to a second-order term (measured 0.02 dx^2 at most: the arcs are not straight)
            // and the area routine's slack of 1e-7 (measured 4e-8).
            assert!(
                (area - (faces - 20.0 * dx)).abs() < 0.05 * dx * dx + 1e-7,
                "dx {dx} reversed {reversed}: area {area} vs {faces} - 20 dx"
            );
            // What the fix does: every vertex of the result, the four tangent ends among them,
            // lies below XSectTol3d off each of its faces, and none has grown past the 5e-5 the
            // four tangent ends have at dx 1e-4, where nothing is moved (measured): the fix passes
            // the check by moving the vertex, not by growing its tolerance.
            let (k, mut brep) = kernel_copy(&r);
            let xsect = corner::xsect_tol_3d_of_brep(&k, &brep);
            for v in brep.vertices().ids().collect::<Vec<_>>() {
                let (_, gap, _) =
                    corner::is_within_xsect_tol_3d_of_faces(&k, &mut brep, v, true).unwrap();
                assert!(
                    gap < xsect,
                    "dx {dx} reversed {reversed}: vertex gap {gap} vs {xsect}"
                );
                let tol = brep.vertex(v).unwrap().tolerance;
                assert!(
                    tol <= 5e-5 + 1e-12,
                    "dx {dx} reversed {reversed}: vertex tolerance {tol}"
                );
            }
        }
    }
}

/// The straight-edge fillets of both operands are cones of semi-angle 0 (`SurfaceKind::Cone`,
/// `is_cylinder`), the kind `faces_coincide` gates: a cylinder has no kind of its own.
#[test]
fn issue_3012_the_fillets_are_cylinders_of_the_cone_kind() {
    use_the_two_fixes();
    for body in [rounded_box(), extruded(1e-4)] {
        let (_, brep) = kernel_copy(&body);
        let round: Vec<_> = brep
            .face_list()
            .iter()
            .filter_map(
                |&f| match face::surface(&brep, f).unwrap().unwrap().kind() {
                    SurfaceKind::Plane(_) => None,
                    SurfaceKind::Cone(c) => Some(c.is_cylinder()),
                    other => panic!("a fillet face of kind {other:?}"),
                },
            )
            .collect();
        assert_eq!(round, [true; 4]);
    }
}

/// `session::set_fixes` sets the process kernel's fixes, shared by every test of this binary
/// (they run on parallel threads): every test here takes this one set, so none can race another.
fn use_the_two_fixes() {
    session::set_fixes(["linear_sweep_arc_is_cylinder", "classify_edge_interval"]).unwrap();
}
