//! tangency-labs/solidcore#746: solids touching along a line inside a face. A union
//! leaves that face an inner loop running along the contact line and back, each edge
//! used twice, which `SmTrimmingTools::CheckFace` rejects. With the opt-in
//! `Kernel::fixes.remove_slit_loops_where_solids_touch`, a volumetric Boolean removes such a
//! loop (only where each edge also bounds another face) and splits the shells joined
//! only through it: two solids touching along the line, each whole.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_union;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p1;
use solidcore::topology::face::p2 as face2;
use solidcore::{Brep, Kernel};

/// A 10 cube at the origin united with an r 2.5 cylinder along Z standing beside it,
/// touching its x = 10 face along the line x = 10, y = 5, z in [2, 8].
fn touching_union(remove_slits: bool) -> Brep {
    let mut k = Kernel::default();
    k.fixes.remove_slit_loops_where_solids_touch = remove_slits;
    let cube = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let post = create_cylinder(&mut k, Point3::new(12.5, 5.0, 2.0), 2.5, 6.0).unwrap();
    boolean_union(&mut k, cube, post).unwrap()
}

/// The largest number of loops on one face.
fn most_loops(b: &Brep) -> usize {
    p1::faces(b, None)
        .unwrap()
        .into_iter()
        .map(|f| face2::loops(b, f, false, None).unwrap().len())
        .max()
        .unwrap()
}

/// The regions other than the infinite one.
fn solid_regions(b: &Brep) -> usize {
    p1::regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|&r| Some(r) != b.infinite_region())
        .count()
}

#[test]
fn issue_746_union_touching_along_a_line_keeps_a_slit_loop_by_default() {
    let b = touching_union(false);
    assert_eq!(
        most_loops(&b),
        2,
        "the cube face keeps the slit loop (upstream)"
    );
}

#[test]
fn issue_746_slit_loop_removed_and_solids_split() {
    let b = touching_union(true);
    assert_eq!(most_loops(&b), 1, "no face keeps an inner loop");
    assert_eq!(solid_regions(&b), 2, "two solids touching along the line");
}
