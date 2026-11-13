//! `create_curve_sweep_from_faces` (`SmApiCreateCurveSweepFromFaces`) of a box face along
//! a straight path. The face's edge curves are the profile; the ends are capped only when
//! `SweepOptions::cap_ends` is set (upstream `SmSweepOptions::bCapEndsArg`, default
//! false). Without it the result is the open tube of the 4 side faces, volume 0, as the
//! C++ reference (`sweep.api_brep` fixture `box-face-curve-sweep.json`); with it, the
//! 2 x 3 x 5 prism of 6 faces. A face parallel to the path sweeps to no side face.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::sweep::api_brep::{create_curve_sweep_from_faces, CurveSweep};
use solidcore::topology::brep::p1;
use solidcore::topology::{face, vertex, FaceId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The 2 x 3 x 4 box at the origin and its top face (z = 4).
fn box_top(k: &mut Kernel) -> (Brep, FaceId) {
    let bx = create_box(k, Point3::new(0.0, 0.0, 0.0), 2.0, 3.0, 4.0).unwrap();
    let top = p1::faces(&bx, None)
        .unwrap()
        .into_iter()
        .find(|&f| {
            face::p4::vertices(&bx, f, None, false, None)
                .unwrap()
                .into_iter()
                .all(|v| vertex::point(&bx, v).unwrap().z == 4.0)
        })
        .unwrap();
    (bx, top)
}

fn sweep_top(k: &mut Kernel, options: Option<&SweepOptions>) -> CurveSweep {
    let (bx, top) = box_top(k);
    let path = Curve::from(
        create_line_segment(Point3::new(0.0, 0.0, 4.0), Point3::new(0.0, 0.0, 9.0)).unwrap(),
    );
    create_curve_sweep_from_faces(k, &[(&bx, top)], Some(&path), None, None, options).unwrap()
}

#[test]
fn capped_box_face_sweep_is_the_prism() {
    let mut k = Kernel::default();
    let options = SweepOptions {
        cap_ends: true,
        ..SweepOptions::default()
    };
    let mut s = sweep_top(&mut k, Some(&options));
    assert_eq!(p1::faces(&s.brep, None).unwrap().len(), 6);
    assert_eq!(
        (s.start_faces.len(), s.side_faces.len(), s.end_faces.len()),
        (1, 4, 1)
    );
    let v = volume(&k, &mut s.brep);
    assert!((v - 30.0).abs() <= 1e-9 * 30.0, "volume {v}");
}

#[test]
fn uncapped_box_face_sweep_is_an_open_tube() {
    let mut k = Kernel::default();
    for options in [None, Some(SweepOptions::default())] {
        let mut s = sweep_top(&mut k, options.as_ref());
        assert_eq!(p1::faces(&s.brep, None).unwrap().len(), 4);
        assert_eq!(
            (s.start_faces.len(), s.side_faces.len(), s.end_faces.len()),
            (0, 4, 0)
        );
        assert_eq!(volume(&k, &mut s.brep), 0.0);
    }
}
