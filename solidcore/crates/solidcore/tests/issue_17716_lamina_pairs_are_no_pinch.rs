//! #17716: #16991's pinch-line skip (`set_relationships` leaves a second edge relation
//! unmade where the operand pinches to no thickness) must not take two coincident sheet
//! borders for a pinch. A pinch is a body meeting itself, so each of its two edges bounds
//! material on both sides; a lamina edge (a sheet's free border) bounds it on one side only.
//!
//! The case is goldens `primitive.primitive_creation_p1` `boolean_multiple_coplanar`: the
//! union of two planar sheets in `z = 0`. A is the unit square `[0, 1]²` and the rectangle
//! `[-2, 0] × [-1, 2]` beside it; B is the rectangle `[1, 3] × [2, 4]` and the same unit
//! square. With the skip taking A's coincident lamina borders for a pinch, the union
//! "succeeded" with an open body of zero volume that fails Level 2 (2 faces, 12 edges, 12
//! vertices, 13 `Test #61` lines, probe 16390-x6); before #16991 the union was refused at
//! the edge relation the skip leaves unmade (probe 16390-x7), and so it is again.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::math::Vec3;
use solidcore::primitive::primitive_creation::p1::{Boolean2d, PrimitiveCreation};
use solidcore::surface::plane::p1::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel};

/// One distinct identity per object with its class type code, as the goldens unit keys
/// them.
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(((kind + 1) << 40) | u64::from(i)),
    }
}

/// A planar sheet face in `z = 0` over `[u0, u1] × [v0, v1]`.
fn sheet(k: &mut Kernel, b: &mut Brep, env: &mut NotifyEnv<'_>, [u0, v0, u1, v1]: [f64; 4]) {
    let frame = Axis2Placement::new(Vec3::new(0.0, 0.0, 0.0), Vec3::X, Vec3::Y);
    let domain = Extent2d::from_bounds(u0, v0, u1, v1);
    let mut plane = Plane::create_canonical(&frame).unwrap();
    plane.adjust_step_uv_domain(&domain).unwrap();
    let mut marks = Marks::default();
    let this_face = |f| key_of(EntityRef::Face(f));
    let mut fenv = FaceBuildEnv {
        env,
        marks: &mut marks,
        this_face: &this_face,
    };
    create_face_from_surface(k, b, Some(Surface::from(plane)), &domain, &mut fenv).unwrap();
}

#[test]
fn coplanar_sheets_with_coincident_borders_are_refused_at_the_relation() {
    let mut k = Kernel::default();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let use_default = solidcore::geom::tol::USE_DEFAULT;
    let mut a = solidcore::topology::brep::p3::new_brep(&mut k, use_default, use_default).unwrap();
    let mut b = solidcore::topology::brep::p3::new_brep(&mut k, use_default, use_default).unwrap();
    sheet(&mut k, &mut a, &mut env, [0.0, 0.0, 1.0, 1.0]);
    sheet(&mut k, &mut a, &mut env, [-2.0, -1.0, 0.0, 2.0]);
    sheet(&mut k, &mut b, &mut env, [1.0, 2.0, 3.0, 4.0]);
    sheet(&mut k, &mut b, &mut env, [0.0, 0.0, 1.0, 1.0]);
    a.set_tolerance(1e-5);
    b.set_tolerance(1e-5);
    a.set_editing_enabled(false);
    b.set_editing_enabled(true);
    let mut output = None;
    let (mut marks_a, mut marks_b) = (Marks::default(), Marks::default());
    let result = PrimitiveCreation::new(None, 17, true).boolean_2d(
        &mut k,
        &mut a,
        &mut b,
        Boolean2d::Union,
        &mut output,
        (&mut marks_a, &mut marks_b),
        &mut env,
    );
    let e = result.unwrap_err();
    assert_eq!(
        e.operation(),
        "boolean::topology_intersector::p1::relate",
        "{e}"
    );
    assert!(
        e.to_string().contains("another entity is already related"),
        "{e}"
    );
}
