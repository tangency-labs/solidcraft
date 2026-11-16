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
            // The sliver is the box's outline less its translate by dx along x: every line
            // y = const in 0 < y < 30 meets it in exactly dx (the outline is at least 36 wide
            // there), so its section is 30 dx (PROVEN for that set). The body cuts each
            // translated fillet dx of arc short of its tangent end (its face is 10 dx short,
            // MEASURED below), which leaves out the piece between the cut and y = 0 under that
            // arc, y = 2 - sqrt(4 - u^2) ~ u^2 / 4 over 0 < u < dx: dx^3 / 12 of the section at
            // each end (ARGUED from the measured cut; 1.7e-6 of the volume at 1e-2). Until
            // `brep_volume` closed tolerant edges' gaps (#16735) the volume measured 5 dx^2
            // less (4.8e-8 at 1e-4, 5.0e-6 at 1e-3), read then as the ends cut off where the
            // fillets cross; it was the caps' trims (below). It is now this closed form to the
            // 1e-8 the volume is asked for (MEASURED -2.3e-9, -2.3e-9 and -2.0e-9 at 1e-4,
            // 1e-3 and 1e-2).
            let h = 10.0;
            let section = dx * 30.0 - 2.0 * dx * dx * dx / 12.0;
            let expected = section * h;
            let v = volume(&r);
            assert!(
                (expected - v).abs() < 1e-8,
                "dx {dx} reversed {reversed}: {v} vs {expected}"
            );
            // Every face of the sliver: the two flat sides (26 x 10 each), the four fillets (a
            // quarter turn of R2 by 10 each) and the two caps, each the section above. A
            // fillet face the sector skipped and a neighbouring edge put in the wrong region
            // would add or drop 31.4. The two translated fillets end dx of arc short of their
            // tangent ends, over 10: 20 dx. The area routine takes each cap over its trims,
            // which enclose dx^2 / 2 less than the cap's edges (MEASURED: -0.5000, -0.5000
            // and -0.5015 dx^2 per cap at 1e-4, 1e-3 and 1e-2; not derived, the same class of
            // defect #16735 closed for the volume; tracked in #16854), so the area is dx^2
            // short of the faces.
            // Up to a second-order term (measured 0.02 dx^2 at most: the arcs are not straight)
            // and the area routine's slack of 1e-7 (measured 4e-8).
            let faces = 2.0 * 26.0 * 10.0 + 4.0 * FRAC_PI_2 * 2.0 * 10.0 + 2.0 * section;
            let trims_short = dx * dx;
            let area = r.area(1e-8).unwrap();
            assert!(
                (area - (faces - 20.0 * dx - trims_short)).abs() < 0.05 * dx * dx + 1e-7,
                "dx {dx} reversed {reversed}: area {area} vs {faces} - 20 dx - dx^2"
            );
            let (k, mut brep) = kernel_copy(&r);
            // What the fix does: every vertex of the result, the four tangent ends among them,
            // lies below XSectTol3d off each of its faces, and none has grown past the 5e-5 the
            // four tangent ends have at dx 1e-4, where nothing is moved (measured): the fix passes
            // the check by moving the vertex, not by growing its tolerance.
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
