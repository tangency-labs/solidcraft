//! tangency-labs/solidcore#453's back rim: a 20 cube bored r5 along y, the bore's wall
//! touching the bottom face along the line x = 10, z = 0; the rim's arc from the bore's seam
//! at (15, 20, 5) over the top down to the touching point V = (10, 20, 0) rounded or
//! chamfered 3 (WR-697; the fork's smlib#124: d657afa and 41a39ec).
//!
//! The arc ends at the seam, where it runs on tangentially into the rest of the rim (the
//! End at a smooth vertex, cut by the plane z = 5 square to it: 41a39ec), and at V on the
//! bottom face. The touching line is an edge of four faces: with d657afa it bounds the
//! trim, so the faces the rails enclose are not collected across it into the whole body
//! (which was then dropped as a leaking trim, the blend's faces merged into a body left
//! whole). Without the blend corner the round and the chamfer fail as pinned SMLib's do
//! (1001 at the seam's corner).
//!
//! Expected values: the volumes of the fork at 18e96f1 (which contains smlib#124's
//! 619c2f0) run on the same inputs (`SmApiCreateBox`, `SmApiCreateCylinder` rotated and
//! translated, `SmApiBooleanDifference`, `SmFilletExecutive` with the round's or chamfer's
//! section, `SmApiBrepComputeVolume` at 1e-10): round 6382.568447649069, chamfer
//! 6316.944421027456, each with 11 faces. They are within the fork's capi test bound (1e-7
//! of `ops_kernel.cpp`'s Simpson closed form, 6382.568731366 less the end rib's certified
//! residual, and 6316.944424728): the input cases come from that test (fork capi test of
//! tangency-labs/solidcore#453, 41a39ec).
//!
//! The touching line is a non-manifold edge of the input, so the result's closed-manifold
//! check (`fillet_refuse_non_manifold_result`, WR-212, solidcore-only) is off here.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::{Brep, EdgeId};

/// Every opt-in fix on, without WR-212's closed-manifold check (the input is not one).
fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        *f.flag_mut(name).unwrap() = true;
    }
    f.fillet_refuse_non_manifold_result = false;
    k.fixes = f;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// The 20 cube less the bore r5 along y, its axis at x = 10, z = 5.
fn holed(k: &mut Kernel) -> Brep {
    let cube = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    let mut bore = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 5.0, 20.0).unwrap();
    let origin = Point3::new(0.0, 0.0, 0.0);
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut bore)),
        origin,
        Vec3::X,
        90.0,
    )
    .unwrap();
    api_translate(
        k,
        Some(TransformObject::Brep(&mut bore)),
        Vec3::new(10.0, 20.0, 5.0),
    )
    .unwrap();
    boolean_difference(k, cube, bore).unwrap()
}

/// The back rim's arc blended 3 (`xsect`: 0 a chamfer, 1 a round): the body's volume and
/// face count.
fn blended(xsect: u32) -> Result<(f64, usize), SolidError> {
    let mut k = kernel();
    let mut b = holed(&mut k);
    let e = pick(&k, &b, [10.0, 20.0, 10.0]);
    fillet_edges(&mut k, &mut b, &[e], 3.0, xsect, 2, 1.0)?;
    Ok((volume(&k, &mut b), b.face_list().len()))
}

/// The holed cube is the fork's.
#[test]
fn the_holed_cube_is_the_forks() {
    let mut k = kernel();
    let mut b = holed(&mut k);
    let v = volume(&k, &mut b);
    assert!((v - 6_429.203_673_205_027).abs() <= 1e-9, "{v}");
}

/// The round ends at the seam on the plane z = 5 and builds the fork's body.
#[test]
fn the_round_builds_as_the_forks() {
    let (v, faces) = blended(1).unwrap();
    assert!((v - 6_382.568_447_649_069).abs() <= 1e-8, "{v}");
    assert_eq!(faces, 11);
}

/// The chamfer builds the fork's body.
#[test]
fn the_chamfer_builds_as_the_forks() {
    let (v, faces) = blended(0).unwrap();
    assert!((v - 6_316.944_421_027_456).abs() <= 1e-8, "{v}");
    assert_eq!(faces, 11);
}
