//! Regression test for WR-568 (the fork's tangency-labs/smlib@985faae, capi test
//! `groove_arc_round_ends_in_the_top`): the 40 x 20 x 20 box less a cylinder r 8 along
//! y, its axis 4 below the top and its seam turned up, leaves a 240 degree arc on the
//! front face, the groove's edge. Rounded r 0.5, the round is a surface of revolution
//! whose span `CalcTorusFilletGeom` took as the turn from the start section to the end
//! one less `IsArc`'s start angle, which for the boolean's circle is measured from the
//! circle's own frame: the torus came out short and the end rib, where the round
//! crosses the top, found no fillet surface (1001). With
//! `fillet_torus_span_from_start_section` the round is built: removed volume
//! pi + 2 asin(4 / rho) integrated over the section's corner, 1.8160502 (the fork
//! measures 1.8161426: the end ribs are fitted within the solver's 4e-5).

#![allow(clippy::unwrap_used)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::{edge, vertex, Brep, EdgeId};
use solidcore::{Kernel, Result, Vec3};

const CENTRE: Point3 = Point3::new(20.0, 0.0, 16.0);

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, b, 1e-8, &attributes).unwrap()
}

/// The grooved box and its arc edge on the front face (both ends on the circle).
fn grooved(k: &mut Kernel) -> (Brep, EdgeId) {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 20.0, 20.0).unwrap();
    let mut bar = create_cylinder(k, Point3::new(0.0, 0.0, -1.0), 8.0, 22.0).unwrap();
    // the seam (at +x) turned to -y, then the axis (z) laid along +y, the seam then up
    let o = Point3::new(0.0, 0.0, 0.0);
    let bar_ref = Some(TransformObject::Brep(&mut bar));
    api_rotate(k, bar_ref, o, Vec3::new(0.0, 0.0, 1.0), -90.0).unwrap();
    let bar_ref = Some(TransformObject::Brep(&mut bar));
    api_rotate(k, bar_ref, o, Vec3::new(1.0, 0.0, 0.0), -90.0).unwrap();
    let bar_ref = Some(TransformObject::Brep(&mut bar));
    api_translate(k, bar_ref, CENTRE - o).unwrap();
    let b = solidcore::boolean::api_brep::boolean_difference(k, block, bar).unwrap();
    let arcs: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            [s, t].into_iter().all(|v| {
                let p = vertex::point(&b, v.unwrap()).unwrap();
                p.y.abs() < 1e-6 && ((p - CENTRE).length() - 8.0).abs() < 1e-6
            })
        })
        .collect();
    assert_eq!(arcs.len(), 1, "{arcs:?}");
    (b, *arcs.first().unwrap())
}

/// The grooved box rounded r 0.5 along the arc: the result, whether it is a solid and
/// the volume the round removed.
fn round(fixed: bool) -> (Result<()>, bool, f64) {
    let mut k = Kernel::default();
    k.fixes.fillet_torus_span_from_start_section = fixed;
    let (mut b, arc) = grooved(&mut k);
    let before = volume(&k, &mut b);
    let result = fillet_edges(&mut k, &mut b, &[arc], 0.5, 1, 1, 1.0);
    let solid = result.is_ok() && is_manifold_solid(&b).unwrap();
    let removed = if solid {
        before - volume(&k, &mut b)
    } else {
        0.0
    };
    (result, solid, removed)
}

#[test]
fn wr_568_round_along_the_groove_arc_is_built() {
    let (result, solid, removed) = round(true);
    result.unwrap();
    assert!(solid);
    assert!((removed - 1.816_050_2).abs() < 1e-3, "{removed}");
}

#[test]
fn wr_568_without_the_opt_in_the_round_fails() {
    let (result, solid, _) = round(false);
    assert!(result.is_err() || !solid, "{result:?}");
}
