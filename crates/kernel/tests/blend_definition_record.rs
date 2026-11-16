//! A round's definition crosses through the body's record: the round on a bore's rim (a plane
//! square to a cylinder's axis, the exact circle class) is written as its definition with its net,
//! read back and rebuilt as the same record, and a later operation on the body builds.

use solidcore_record::Vec3;
use solidcore_record::brep::{Brep, Surface};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::*;

/// The faces of a record that are blends by their definition.
fn blends(record: &Brep) -> Vec<usize> {
    record.faces.iter().enumerate().filter(|(_, f)| matches!(f.surface, Surface::Blend { .. })).map(|(i, _)| i).collect()
}

fn rounded_bore() -> Body {
    let block = box_solid(Vec3::ZERO, Vec3::new(30.0, 30.0, 10.0)).unwrap();
    let bore = cylinder(Vec3::new(15.0, 15.0, 0.0), Vec3::Z, 5.0, 10.0).unwrap();
    let drilled = boolean(&block, &bore, BoolOp::Cut).unwrap().unwrap();
    fillet(&drilled, &[Vec3::new(20.0, 15.0, 10.0)], 0.3).unwrap()
}

/// The round's faces are blends by their definition, each with its net as its form; the record
/// written, read back and rebuilt in the kernel is the same record.
#[test]
fn a_rounds_definition_survives_save_reopen_and_rebuild() {
    let body = rounded_bore();
    let record = brep_record(&body).unwrap();
    let faces = blends(&record);
    assert!(!faces.is_empty(), "the round is written by its definition");
    for &i in &faces {
        assert!(matches!(record.faces[i].nurbs, Some(Surface::Spline { .. } | Surface::RationalSpline { .. })), "face {i} has its net");
    }
    let reopened = decode_native_body(&encode_native_body(&body).unwrap()).unwrap();
    assert_eq!(brep_record(&reopened).unwrap(), record, "saved and opened, number for number");
    let rebuilt = body_from_smlib(&smlib_body(&reopened).unwrap()).unwrap();
    let again = brep_record(&rebuilt).unwrap();
    assert_eq!(blends(&again), faces, "rebuilt, the same faces are blends");
    for &i in &faces {
        assert_eq!(again.faces[i].surface, record.faces[i].surface, "face {i}'s definition, unchanged");
    }
}

/// A body whose round came back from its record takes later operations: a shell open at the top.
#[test]
fn a_body_rebuilt_from_its_record_takes_a_shell() {
    let body = rounded_bore();
    let reopened = decode_native_body(&encode_native_body(&body).unwrap()).unwrap();
    let rebuilt = body_from_smlib(&smlib_body(&reopened).unwrap()).unwrap();
    let shelled = shell(&rebuilt, &[Vec3::new(2.0, 2.0, 0.0)], 1.0).unwrap();
    assert!(shelled.validity().is_empty(), "{:?}", shelled.validity());
}

/// A ray onto the round meets one of its blend faces, evaluated on its net: halfway around the
/// rim's torus (bore radius 5, round 0.3), at 45° between the top and the bore, the point is on
/// the round and the normal its outward one there, into the bore and up.
#[test]
fn a_ray_onto_the_round_meets_its_blend_face_with_its_normal() {
    let body = rounded_bore();
    let faces = blends(&brep_record(&body).unwrap());
    let h = 0.5f64.sqrt();
    let normal = Vec3::new(-h, 0.0, h);
    let point = Vec3::new(15.0 + 5.3 - 0.3 * h, 15.0, 9.7 + 0.3 * h);
    let hit = body.fire_ray(point + normal * 10.0, normal * -1.0).unwrap().expect("the ray meets the round");
    assert!(faces.contains(&hit.face), "face {} is not one of the blends {faces:?}", hit.face);
    assert!((hit.point - point).len() < 1e-6, "{hit:?}");
    assert!((hit.normal - normal).len() < 1e-6, "{hit:?}");
    assert!((hit.depth - 10.0).abs() < 1e-6, "{hit:?}");
}
