//! WR-579 (tangency-labs/solidcore#1517; the fork's tangency-labs/smlib@9eec159): a bicycle
//! frame's seat tube (r 16, from (0, 0, 300) towards (-150, 0, 820)) ends 3.7e-4 short of
//! where the top tube (r 16, from (-150, 0, 820) to (600, 0, 860)) starts. Where the two
//! walls touch, the seat's end circle runs within tolerance of the top tube's wall for a
//! stretch and crosses it there too: the crossing and the stretch's ends fall in one proto
//! vertex as three vertex definitions too far apart to combine, and upstream's
//! `SmProtoEdge::Resolve` keeps the stretch's edge whole past the crossing's vertex. The
//! union then fails.
//!
//! The fork builds each tube with its C API's extrude, the circle starting on the side the
//! walls touch (the top tube's seam at -y, the seat's at +y). This builds them as
//! `SmApiCreateCylinder` (seam at its frame's +X) turned about Z to put the seam there,
//! about Y onto the axis, and moved to the base, as the `boolean.api_brep` fixture
//! `issue_1517_tube_ends.json` does on both sides.
//!
//! Expected with `proto_edge_splits_at_captured_vertices`: one manifold solid, its volume
//! between the larger tube's and both together. Its check still fails on a separate
//! defect, as the fork's does: an ellipse piece 8.6e-5 long (the top tube's wall cut by the
//! seat's end plane) whose NURB lies 2.1e-5 off its ellipse (`SmEllipse` test 2;
//! `SmExtent1d::ContainsPeriodicValue`, #1518). That is the only failure allowed.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
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

/// The top tube: (-150, 0, 820) to (600, 0, 860), its seam at -y.
fn top(k: &mut Kernel) -> Brep {
    tube(
        k,
        [-150.0, 0.0, 820.0],
        751.0659092250161,
        -90.0,
        86.94711748520757,
    )
}

/// The seat tube: from (0, 0, 300) towards (-150, 0, 820), 3.7e-4 short, its seam at +y.
fn seat(k: &mut Kernel) -> Brep {
    tube(
        k,
        [0.0, 0.0, 300.0],
        541.2019951093923,
        90.0,
        -16.09081634885217,
    )
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// #1518's report, the one check failure the joined body still has: the ellipse piece
/// 8.6e-5 long (the top tube's wall cut by the seat's end plane) whose NURB lies 2.1e-5
/// off its ellipse (`SmExtent1d::ContainsPeriodicValue`, a separate defect the fork's
/// commit also leaves). The numbers are deterministic.
const ELLIPSE_1518: &str = "SmEllipse::Test #2 Bad Nurb to STEP Approx: \
Dist(NurbPt, EvalSTEP(STEPInversion(NurbPt)) greater than tol \
(offset=2.1419823e-05, tolerance=8.2099963e-08) [owner SmEllipse]";

/// The union with the fix set to `fixed`: the result's volume when it is one manifold
/// solid whose check reports exactly #1518's ellipse ([`ELLIPSE_1518`]), else what went
/// wrong.
fn join(fixed: bool) -> Result<f64, String> {
    let mut k = Kernel::default();
    k.fixes.proto_edge_splits_at_captured_vertices = fixed;
    let (t, s) = (top(&mut k), seat(&mut k));
    let mut r = boolean_union(&mut k, t, s).map_err(|e| format!("union: {e:?}"))?;
    if !is_manifold_solid(&r).unwrap() {
        return Err("not a manifold solid".into());
    }
    let lines = check_brep(&mut k, &mut r, CheckLevel::Level2)
        .unwrap()
        .lines();
    println!("check (fix {fixed}): {lines:?}");
    // exactly #1518's line, once; any other report fails
    if lines != [ELLIPSE_1518] {
        return Err(format!("check: {lines:?}"));
    }
    Ok(volume(&k, &mut r))
}

#[test]
fn wr579_near_coincident_tube_ends_join() {
    let unfixed = join(false);
    println!("flag off: {unfixed:?}");
    assert!(unfixed.is_err(), "upstream's union succeeded: {unfixed:?}");

    let v = join(true).unwrap();
    let mut k = Kernel::default();
    let (mut t, mut s) = (top(&mut k), seat(&mut k));
    let (vt, vs) = (volume(&k, &mut t), volume(&k, &mut s));
    assert!(
        v > vt.max(vs) && v < vt + vs,
        "union volume {v}, tubes {vt} and {vs}"
    );
}
