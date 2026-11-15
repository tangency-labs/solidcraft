//! A Boolean's result never names a vertex it no longer has, and its edges' cached end
//! pairs ([`solidcore::topology::Edge::vertices`]) agree with their uses. The #1058 probe
//! read that cache and found it naming deleted vertices: topology edits inside the merge
//! do not maintain it (upstream has no such field), so the public operations now refresh
//! it before they return. Geometry: box A = [0,10]^3 united with a 10 x 10 x 20 box B
//! whose crease crosses A's edge y = z = 10 at a depth `d`. `boolean.api_brep`'s
//! `crossing_creases_union_*` cases compare the same unions with C++.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_union;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::topology::edge;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, Kernel};

/// Every live edge whose end vertices, read through its edge and vertex uses
/// (`GetVertices`, as upstream), name a vertex the body does not have.
fn dangling_vertex_refs(b: &Brep) -> Vec<String> {
    let mut out = Vec::new();
    for e in p1::edges(b, None).unwrap() {
        match edge::vertices(b, e, None) {
            Ok(ends) => {
                for v in ends.iter().flatten() {
                    if b.vertex(*v).is_err() {
                        out.push(format!("{e:?}: vertex use names {v:?}, not live"));
                    }
                }
            }
            Err(err) => out.push(format!("{e:?}: GetVertices {err:?}")),
        }
    }
    out
}

/// B: a 10 x 10 x 20 box whose edge (x = y = 0, z -10..10) is turned onto the line along
/// (0, -1, 1) through (5, 10, 10) less `d` along (0, 1, 1) / sqrt 2 (as the #1058 probe).
fn crease(k: &mut Kernel, d: f64) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, -10.0), 10.0, 10.0, 20.0).unwrap();
    let o = Point3::new(0.0, 0.0, 0.0);
    for axis in [Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0)] {
        api_rotate(k, Some(TransformObject::Brep(&mut b)), o, axis, 45.0).unwrap();
    }
    let s = d / 2.0_f64.sqrt();
    let shift = Vec3::new(5.0, 10.0 - s, 10.0 - s);
    api_translate(k, Some(TransformObject::Brep(&mut b)), shift).unwrap();
    b
}

fn operands(k: &mut Kernel, d: f64) -> (Brep, Brep) {
    let a = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    (a, crease(k, d))
}

#[test]
fn boolean_union_of_crossing_creases_has_no_dangling_vertex_refs() {
    let mut failures = Vec::new();
    for d in [3e-6, 1e-5, 3e-5, 1e-4] {
        for fix in [false, true] {
            let mut k = Kernel::default();
            k.fixes.degenerate_proto_edge_in_one_vertex = fix;
            let (a, b) = operands(&mut k, d);
            if let Ok(u) = boolean_union(&mut k, a, b) {
                let bad = dangling_vertex_refs(&u);
                if !bad.is_empty() {
                    failures.push(format!("d={d:e} fix={fix}: {bad:?}"));
                }
                // every link, and the cached edge ends the Boolean refreshes on return
                if let Err(e) = solidcore::topology::integrity::check(&u) {
                    failures.push(format!("d={d:e} fix={fix}: integrity {e:?}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
