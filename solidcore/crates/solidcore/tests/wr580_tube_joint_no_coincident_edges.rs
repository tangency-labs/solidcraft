//! WR-580 (tangency-labs/solidcore#1519; the fork's tangency-labs/smlib@81cd61a): the seat
//! and top tubes of a bicycle frame (r 16; the top tube from (-150, 0, 820) to
//! (600, 0, 860), the seat tube from (0, 0, 300) to (-150, 0, 820)) as SolidCraft's bench
//! builds them: each axis rounded to 1e-6, its base and height to 1e-3, the circle started
//! 0.6131 rad round from its frame's x (Z x axis, or X for an axis within 0.9 of Z). The
//! top tube's wall cut by the seat's end plane runs within tolerance of the seat's end
//! circle between two of the joint's vertices: upstream's `SmEdgeDefinition::Imprint`
//! makes the intersection edge on the seat's end cap beside that circle, and the merge
//! copies the circle into the top tube's wall beside the curve's own edge (coincident
//! edges, `SmFace` tests 33 and 34).
//!
//! The fork builds the tubes with its C API's extrude; this builds each as
//! `SmApiCreateCylinder` (seam at its frame's +X) turned about Z to put the seam where the
//! bench's circle starts, about Y onto the axis, and moved to the base, as the
//! `boolean.api_brep` fixture `issue_1519_bench_tubes.json` does on both sides. The union
//! is run both with `proto_edge_splits_at_captured_vertices` (WR-579, #1517) set, as the
//! fork had it, and without it.
//!
//! Expected: `Imprint` takes the seat's end circle as the curve's edge on the cap (the
//! two whole curves proved within tolerance on their control points), and the union is
//! one manifold solid whose level-2 check reports nothing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, Kernel};

/// A tube of radius 16 and `length` along Z (`SmApiCreateCylinder` at the origin), turned
/// `z_deg` about Z (its seam off +X) and `y_deg` about Y (its axis onto the tube's), then
/// moved to `origin`: the fixture's `moved` cylinder.
fn tube(k: &mut Kernel, origin: [f64; 3], length: f64, z_deg: f64, y_deg: f64) -> Brep {
    let mut b = create_cylinder(k, Point3::ZERO, 16.0, length).unwrap();
    let o = Point3::ZERO;
    api_rotate(k, Some(TransformObject::Brep(&mut b)), o, Vec3::Z, z_deg).unwrap();
    api_rotate(k, Some(TransformObject::Brep(&mut b)), o, Vec3::Y, y_deg).unwrap();
    api_translate(k, Some(TransformObject::Brep(&mut b)), Vec3::from(origin)).unwrap();
    b
}

/// The union with WR-579 set to `split`: the check's failure lines, or what went wrong.
fn join(split: bool) -> Result<Vec<String>, String> {
    let mut k = Kernel::default();
    k.fixes.proto_edge_splits_at_captured_vertices = split;
    let top = tube(
        &mut k,
        [-150.0, 0.0, 820.0],
        751.066,
        125.12804241947079,
        86.94709765086374,
    );
    let seat = tube(
        &mut k,
        [0.0, 0.0, 300.0],
        541.202,
        35.12804241947077,
        -16.090829052954405,
    );
    let mut r = boolean_union(&mut k, top, seat).map_err(|e| format!("union: {e:?}"))?;
    if !is_manifold_solid(&r).unwrap() {
        return Err("not a manifold solid".into());
    }
    Ok(check_brep(&mut k, &mut r, CheckLevel::Level2)
        .unwrap()
        .lines())
}

#[test]
fn wr580_tube_joint_no_coincident_edges() {
    for split in [true, false] {
        let lines = join(split).unwrap();
        // exactly nothing: no coincident edges (`SmFace` tests 33 and 34) and no other report
        assert!(
            lines.is_empty(),
            "WR-579 {split}: the check fails: {lines:?}"
        );
    }
}
