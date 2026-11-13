//! A sheet turned over (`topology::brep::sheet::turn_over_sheet`) through the record, both ways:
//! the record keeps each sheet face's upward use, so a turned sheet's record has each face's
//! `forward` negated and its faces, edges and vertices as they were; built again it is turned
//! over in the kernel too, and gives the same record; a sheet not turned over goes round
//! byte for byte as it did.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // a test's fixtures

use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_primitives as primitives;
use solidcore::topology::brep::sheet::turn_over_sheet;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2::upward_faceuse;
use solidcore::{Brep, FaceId, Vec3};
use solidcore_convert::brep::{to_kernel, to_record, Order};
use solidcore_record::brep::{Brep as Record, Shell};

fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    k
}

fn valid(k: &mut Kernel, b: &mut Brep) {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert!(c.is_valid(), "{:?}", c.failures().collect::<Vec<_>>());
}

fn bytes(r: &Record) -> Vec<u8> {
    serde_json::to_vec(r).unwrap()
}

/// Each face's side in its shell: `(face, forward)`, in shell order.
fn sides(r: &Record) -> Vec<(u32, bool)> {
    r.shells
        .iter()
        .flat_map(|s| match s {
            Shell::Faces { faces, .. } => faces.iter().map(|f| (f.face, f.forward)).collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// Each face's upward use's orientation.
fn upward(b: &Brep) -> Vec<OrientType> {
    b.face_list().iter().map(|&f| side(b, f)).collect()
}

/// Face `f`'s upward use's orientation: its side against its surface.
fn side(b: &Brep, f: FaceId) -> OrientType {
    let fu = upward_faceuse(b, f).unwrap().unwrap();
    b.faceuse(fu).unwrap().orientation
}

/// The kernel body built from `r` faces each face the way `r`'s sheet uses it: a face used
/// backward is turned over (its upward use the opposite one).
fn faces_as_recorded(b: &Brep, order: &Order, r: &Record) {
    for (face, forward) in sides(r) {
        let want = if forward {
            OrientType::Same
        } else {
            OrientType::Opposite
        };
        assert_eq!(side(b, order.faces[face as usize]), want, "face {face}");
    }
}

/// The 3 by 2 rectangle sheet at (1, 2, 3).
fn sheet(k: &mut Kernel) -> Brep {
    primitives::create_plane(k, Vec3::new(1.0, 2.0, 3.0), 3.0, 2.0).unwrap()
}

#[test]
fn a_sheet_not_turned_over_goes_round_byte_for_byte() {
    let mut k = kernel();
    let mut b = sheet(&mut k);
    assert!(upward(&b).iter().all(|o| *o == OrientType::Same));
    let (r, _) = to_record(&mut k, &mut b).unwrap();
    let (mut back, order) = to_kernel(&mut k, &r, true).unwrap();
    faces_as_recorded(&back, &order, &r);
    valid(&mut k, &mut back);
    let (again, _) = to_record(&mut k, &mut back).unwrap();
    assert_eq!(bytes(&again), bytes(&r));
}

#[test]
fn a_turned_sheet_s_record_reads_each_face_the_other_way() {
    let mut k = kernel();
    let mut b = sheet(&mut k);
    let (before, _) = to_record(&mut k, &mut b).unwrap();

    turn_over_sheet(&mut b).unwrap();
    valid(&mut k, &mut b);
    let (turned, _) = to_record(&mut k, &mut b).unwrap();
    turned.validate_built().unwrap();
    assert_eq!(
        turned.faces, before.faces,
        "surfaces, domains and loops kept"
    );
    assert_eq!(turned.edges, before.edges);
    assert_eq!(turned.vertices, before.vertices);
    let (s0, s1) = (sides(&before), sides(&turned));
    assert!(!s0.is_empty());
    assert_eq!(s0.len(), s1.len());
    for ((f0, fw0), (f1, fw1)) in s0.iter().zip(&s1) {
        assert_eq!(f0, f1);
        assert_eq!(*fw1, !*fw0, "face {f0}'s forward is negated");
    }

    // built again, the kernel's sheet is turned over too, and gives the same record
    let (mut back, order) = to_kernel(&mut k, &turned, true).unwrap();
    faces_as_recorded(&back, &order, &turned);
    valid(&mut k, &mut back);
    let (again, _) = to_record(&mut k, &mut back).unwrap();
    assert_eq!(bytes(&again), bytes(&turned));

    // turned over twice, the sheet's record is the one it had
    turn_over_sheet(&mut back).unwrap();
    let (twice, _) = to_record(&mut k, &mut back).unwrap();
    assert_eq!(bytes(&twice), bytes(&before));
}
