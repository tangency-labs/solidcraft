//! tangency-labs/solidcore#488: a face a Boolean takes from its second operand is rebuilt
//! by `SmBrep::MakeFace` without the operand's UV trims. The port (WR-361) gives its edge
//! uses copies of the operand's exact trims
//! where the new edge runs on the copy of the operand edge's curve over the same interval.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p1;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::{Brep, Kernel};

/// The UV trims on the edge uses of the cylindrical faces (the cylinder's side), each as
/// its end points' bits and its curve kind's data, sorted; a missing trim counts as
/// `None`.
fn side_trims(k: &Kernel, b: &Brep) -> Vec<Option<String>> {
    let mut out = Vec::new();
    for f in p1::faces(b, None).unwrap() {
        let face = b.face(f).unwrap();
        if !matches!(face.surface.as_ref().unwrap().kind(), SurfaceKind::Cone(_)) {
            continue;
        }
        let uses = eu1::faceuse_edgeuses(b, face.faceuse.unwrap(), false, None).unwrap();
        for eu in uses {
            out.push(eu1::uv_trim_curve(b, eu).unwrap().0.map(|c| {
                let ivl = c.natural_interval();
                let a = c.evaluate_point(k, ivl.min()).unwrap();
                let z = c.evaluate_point(k, ivl.max()).unwrap();
                let ends = [a.x, a.y, a.z, z.x, z.y, z.z].map(f64::to_bits);
                format!("{ends:?} {:?}", c.kind())
            }));
        }
    }
    assert!(!out.is_empty(), "no cylindrical face");
    out.sort_unstable();
    out
}

/// A 10 cube less an r 2 cylinder standing inside it (z 3..7): a closed void whose faces
/// are the cylinder's, taken over whole. Returns the cylinder's seam-face trims and the
/// result's.
fn void_cut() -> (Vec<Option<String>>, Vec<Option<String>>) {
    let mut k = Kernel::default();
    let cube = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let tool = create_cylinder(&mut k, Point3::new(5.0, 5.0, 3.0), 2.0, 4.0).unwrap();
    let before = side_trims(&k, &tool);
    let result = boolean_difference(&mut k, cube, tool).unwrap();
    (before, side_trims(&k, &result))
}

#[test]
fn issue_488_operand_face_keeps_its_exact_trims() {
    let (before, after) = void_cut();
    assert!(
        before.iter().all(Option::is_some),
        "the cylinder has its trims"
    );
    assert_eq!(
        after, before,
        "the void's side faces carry the cylinder's trims"
    );
}

/// The edge tolerances, in edge order, after an r 1 fillet of edges 15 and 24 of the
/// union of a 20x10x10 box and a 10x6x10 box standing on it at (5, -2, 10) (the
/// goldens' `corner3x2-union-overhang-front-edges15-24-r1`), with the union's trims
/// first dropped when `drop` is set.
fn corner_fillet_tolerances(drop: bool) -> Vec<f64> {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 10.0, 10.0).unwrap();
    let b = create_box(&mut k, Point3::new(5.0, -2.0, 10.0), 10.0, 6.0, 10.0).unwrap();
    let mut brep = boolean_union(&mut k, a, b).unwrap();
    if drop {
        for eu in solidcore::topology::brep::p2::edgeuses(&brep, None).unwrap() {
            if eu1::uv_trim_curve_pointer(&brep, eu).unwrap().is_some() {
                eu1::set_uv_trim_curve(&mut brep, eu, eu1::UvTrimCurveArg::Null, 0.0, true, None)
                    .unwrap();
            }
        }
    }
    let edges = p1::edges(&brep, None).unwrap();
    let sel = [edges[15], edges[24]];
    fillet_edges(&mut k, &mut brep, &sel, 1.0, 1, 1, 1.0).unwrap();
    p1::edges(&brep, None)
        .unwrap()
        .into_iter()
        .map(|e| brep.edge(e).unwrap().tolerance)
        .collect()
}

/// tangency-labs/solidcraft#17896: on a plane, `MakeFace`'s trim of a line edge is already
/// the exact UV line, so the union keeps it rather than copying the box's degree-1
/// B-spline trim (ends an ulp off). With the copy, the fillet's corner drop split and
/// raised edge 28 and its vertices to 1.0356e-4; without it every tolerance is the one
/// the fillet gives with no union trims at all (edge 28: 4e-5).
#[test]
fn issue_17896_plane_line_trims_are_not_copied() {
    let kept = corner_fillet_tolerances(false);
    let dropped = corner_fillet_tolerances(true);
    assert_eq!(kept.len(), dropped.len());
    for (i, (k, d)) in kept.iter().zip(&dropped).enumerate() {
        assert_eq!(k.to_bits(), d.to_bits(), "edge {i}: {k:e} against {d:e}");
    }
    assert_eq!(kept[28], 4e-5);
}
