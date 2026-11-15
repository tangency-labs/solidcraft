//! tangency-labs/solidcore#488: a face a Boolean takes from its second operand is rebuilt
//! by `SmBrep::MakeFace` without the operand's UV trims. With the opt-in
//! `Kernel::fixes.keep_operand_uv_trims`, its edge uses get copies of the operand's exact trims
//! where the new edge runs on the copy of the operand edge's curve over the same interval.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_difference;
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
fn void_cut(keep: bool) -> (Vec<Option<String>>, Vec<Option<String>>) {
    let mut k = Kernel::default();
    k.fixes.keep_operand_uv_trims = keep;
    let cube = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let tool = create_cylinder(&mut k, Point3::new(5.0, 5.0, 3.0), 2.0, 4.0).unwrap();
    let before = side_trims(&k, &tool);
    let result = boolean_difference(&mut k, cube, tool).unwrap();
    (before, side_trims(&k, &result))
}

#[test]
fn issue_488_operand_face_keeps_its_exact_trims() {
    let (before, after) = void_cut(true);
    assert!(
        before.iter().all(Option::is_some),
        "the cylinder has its trims"
    );
    assert_eq!(
        after, before,
        "the void's side faces carry the cylinder's trims"
    );
}

#[test]
fn issue_488_operand_face_is_rebuilt_without_them_by_default() {
    let (before, after) = void_cut(false);
    assert_ne!(
        after, before,
        "upstream: MakeFace builds the face without the trims"
    );
}
