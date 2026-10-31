//! WR-660: planar-path profile placement must follow a rigidly moved setup.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::query::{api_queries::brep_bounding_box, brep::BrepClassifier};
use solidcore::sweep::api_brep::create_sweep_along_planar_path;
use solidcore::{Brep, EntityRef, Kernel};

fn pose(axis: Vec3, degrees: f64, offset: Vec3) -> Axis2Placement {
    let mut p = Axis2Placement::IDENTITY;
    p.rotate_about_axis(degrees.to_radians(), axis);
    p.translate(offset);
    p
}
fn body(fixed: bool, transport: bool, centre: bool, pose: &Axis2Placement) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    assert!(!k.fixes.planar_path_profile_in_path_frame);
    k.fixes.planar_path_profile_in_path_frame = fixed;
    k.fixes.sweep_frame_on_planar_path_normal = transport;
    let pts = [
        Vec3::new(18., 0., -2.),
        Vec3::new(22., 0., -2.),
        Vec3::new(22., 0., 2.),
        Vec3::new(18., 0., 2.),
    ];
    let mut profiles: Vec<Curve> = (0..4)
        .map(|i| Curve::from(create_line_segment(pts[i], pts[(i + 1) % 4]).unwrap()))
        .collect();
    let mut path = Curve::from(create_arc(Vec3::ZERO, 20., 0., 90.).unwrap());
    for c in &mut profiles {
        c.transform(&k, pose, None).unwrap();
    }
    path.transform(&k, pose, None).unwrap();
    let refs: Vec<_> = profiles.iter().collect();
    let b = create_sweep_along_planar_path(&mut k, &refs, &[&path], centre, 3).unwrap();
    (k, b)
}
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    brep_volume(
        k,
        b,
        1e-6,
        &PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        },
    )
    .unwrap()
}
fn inside(k: &Kernel, b: &Brep, p: Vec3) -> bool {
    let c = BrepClassifier::of_brep(b)
        .point_3d_classify(k, p, 1e-5, false)
        .unwrap();
    match c.object() {
        Some(EntityRef::Region(r)) => b.region(r).unwrap().is_void() == 0,
        _ => false,
    }
}
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() < tol, "{a} != {b}");
}
/// `transport`: with WR-407's frame too, which takes the sweep's own pose-dependent fit
/// out; `box_tol` is the bound on the bounding boxes' drift.
fn compare(centre: bool, transport: bool, box_tol: f64) {
    let (k, mut flat) = body(true, transport, centre, &Axis2Placement::IDENTITY);
    let exact = std::f64::consts::PI * if centre { 160. } else { 144. };
    close(volume(&k, &mut flat), exact, exact * 1e-4);
    let bb = brep_bounding_box(&k, &flat, true).unwrap();
    for (axis, angle) in [
        (Vec3::X, 30.),
        (Vec3::Y, 30.),
        (Vec3::X, 90.),
        (Vec3::Y, 90.),
        (Vec3::X, 180.),
        (Vec3::new(1., 2., 3.), 67.),
        (Vec3::Z, 35.),
        (Vec3::X, 0.),
    ] {
        let p = pose(axis, angle, Vec3::new(3., -7., 11.));
        let (mut k, mut b) = body(true, transport, centre, &p);
        let v = volume(&k, &mut b);
        // Against the closed form (pi/4 (r1^2 - r0^2) 4): the sweep is an approximation
        // whose fit depends on the pose, to about 5e-5 of the volume.
        close(v, exact, exact * 1e-4);
        solidcore::topology::brep::p5::transform(&mut k, &mut b, &p.invert(), None, false).unwrap();
        let actual = brep_bounding_box(&k, &b, true).unwrap();
        // Without WR-407's frame the sweep's fit depends on the pose (up to 3.3e-3 at
        // the arc's far end); the roll this fixes is 0.73.
        let drift = actual
            .min()
            .distance(bb.min())
            .max(actual.max().distance(bb.max()));
        assert!(drift < box_tol, "{drift}: {actual:?} != {bb:?}");
        // Points clear of every face: the first-point anchor puts the profile at
        // radius 16 to 20, z 0 to 4, and the centre anchor at 18 to 22, -2 to 2.
        let (dr, dz) = if centre { (0., 0.) } else { (-2., 2.) };
        for r in [15., 18.5, 20., 21.5, 24.] {
            for z in [-2.1, -1., 0., 1., 2.1] {
                let (r, z) = (r + dr, z + dz);
                let q = Vec3::new(r * 0.5_f64.sqrt(), r * 0.5_f64.sqrt(), z);
                assert_eq!(
                    inside(&k, &b, q),
                    inside(&k, &flat, q),
                    "{q:?}, {axis:?} {angle}"
                );
            }
        }
    }
}
#[test]
fn centred_profile_is_rigid_motion_invariant() {
    compare(true, false, 5e-3);
}
#[test]
fn first_point_profile_is_rigid_motion_invariant() {
    compare(false, false, 5e-3);
}
#[test]
fn with_the_transport_frame_the_boxes_agree_closely() {
    compare(true, true, 1e-5);
    compare(false, true, 1e-5);
}
#[test]
fn default_keeps_upstream_pose_dependence() {
    for centre in [true, false] {
        let (k, mut flat) = body(false, false, centre, &Axis2Placement::IDENTITY);
        let p = pose(Vec3::X, 30., Vec3::ZERO);
        let (mut kt, mut tilted) = body(false, false, centre, &p);
        let vf = volume(&k, &mut flat);
        let vt = volume(&kt, &mut tilted);
        close(vf, if centre { 502.655040 } else { 452.388984 }, 1e-5);
        close(vt, if centre { 502.654666 } else { 484.256217 }, 1e-5);
        if centre {
            solidcore::topology::brep::p5::transform(
                &mut kt,
                &mut tilted,
                &p.invert(),
                None,
                false,
            )
            .unwrap();
            let bb = brep_bounding_box(&kt, &tilted, true).unwrap();
            close(bb.max().z, 2.732050807568877, 1e-6);
            let q = Vec3::new(20. * 0.5_f64.sqrt(), 20. * 0.5_f64.sqrt(), 2.1);
            assert!(inside(&kt, &tilted, q));
            assert!(!inside(&k, &flat, q));
        }
    }
}

#[test]
fn xy_and_parallel_xy_keep_the_exact_legacy_result_with_either_normal() {
    for angle in [0., 180.] {
        let p = pose(Vec3::X, angle, Vec3::new(3., -7., 11.));
        for centre in [true, false] {
            let (k, mut legacy) = body(false, false, centre, &p);
            let (fixed, mut corrected) = body(true, false, centre, &p);
            assert_eq!(volume(&k, &mut legacy), volume(&fixed, &mut corrected));
            let a = brep_bounding_box(&k, &legacy, true).unwrap();
            let b = brep_bounding_box(&fixed, &corrected, true).unwrap();
            assert_eq!(a.min(), b.min());
            assert_eq!(a.max(), b.max());
        }
    }
}

/// A path of two pieces, the line along +y for 20 and the quarter arc, in either order,
/// the square as in `body`. `fixed` turns on WR-407's frame too: without it the sweep's
/// own pose dependence still cuts the arc-then-line path to volume 0 in some poses.
fn two_piece_body(
    fixed: bool,
    centre: bool,
    line_first: bool,
    pose: &Axis2Placement,
) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.planar_path_profile_in_path_frame = fixed;
    k.fixes.sweep_frame_on_planar_path_normal = fixed;
    let pts = [
        Vec3::new(18., 0., -2.),
        Vec3::new(22., 0., -2.),
        Vec3::new(22., 0., 2.),
        Vec3::new(18., 0., 2.),
    ];
    let mut profiles: Vec<Curve> = (0..4)
        .map(|i| Curve::from(create_line_segment(pts[i], pts[(i + 1) % 4]).unwrap()))
        .collect();
    let mut paths: Vec<Curve> = if line_first {
        vec![
            Curve::from(
                create_line_segment(Vec3::new(20., 0., 0.), Vec3::new(20., 20., 0.)).unwrap(),
            ),
            Curve::from(create_arc(Vec3::new(0., 20., 0.), 20., 0., 90.).unwrap()),
        ]
    } else {
        vec![
            Curve::from(create_arc(Vec3::ZERO, 20., 0., 90.).unwrap()),
            Curve::from(
                create_line_segment(Vec3::new(0., 20., 0.), Vec3::new(-20., 20., 0.)).unwrap(),
            ),
        ]
    };
    for c in profiles.iter_mut().chain(paths.iter_mut()) {
        c.transform(&k, pose, None).unwrap();
    }
    let profile_refs: Vec<_> = profiles.iter().collect();
    let path_refs: Vec<_> = paths.iter().collect();
    let b = create_sweep_along_planar_path(&mut k, &profile_refs, &path_refs, centre, 3).unwrap();
    (k, b)
}

/// The reviewer's case: each piece in its own frame left the line rolled and the arc
/// not, and the mitre between them cut the sweep to volume 0.
#[test]
fn a_path_of_a_line_and_an_arc_is_rigid_motion_invariant() {
    for line_first in [true, false] {
        for centre in [true, false] {
            let (k, mut flat) = two_piece_body(true, centre, line_first, &Axis2Placement::IDENTITY);
            // the arc's volume, and the line's 20 x 16
            let exact = std::f64::consts::PI * if centre { 160. } else { 144. } + 320.;
            close(volume(&k, &mut flat), exact, exact * 1e-4);
            let bb = brep_bounding_box(&k, &flat, true).unwrap();
            for (axis, angle) in [
                (Vec3::X, 30.),
                (Vec3::Y, 30.),
                (Vec3::Y, 60.),
                (Vec3::X, 90.),
                (Vec3::Y, 90.),
                (Vec3::new(1., 2., 3.), 67.),
                (Vec3::new(-2., 1., 4.), 41.),
            ] {
                let p = pose(axis, angle, Vec3::new(3., -7., 11.));
                let (mut k, mut b) = two_piece_body(true, centre, line_first, &p);
                close(volume(&k, &mut b), exact, exact * 1e-4);
                solidcore::topology::brep::p5::transform(&mut k, &mut b, &p.invert(), None, false)
                    .unwrap();
                let actual = brep_bounding_box(&k, &b, true).unwrap();
                let drift = actual
                    .min()
                    .distance(bb.min())
                    .max(actual.max().distance(bb.max()));
                assert!(drift < 1e-3, "{drift}: {actual:?} != {bb:?}");
            }
        }
    }
}

/// The default for that path: the arc-then-line sweep is empty when tilted (the port;
/// not run on the C++), and the line-then-arc sweep rolled.
#[test]
fn default_two_piece_paths_keep_upstream_pose_dependence() {
    let p = pose(Vec3::X, 30., Vec3::new(3., -7., 11.));
    let (k, mut empty) = two_piece_body(false, true, false, &p);
    assert_eq!(volume(&k, &mut empty), 0.0);
    let (mut k, mut rolled) = two_piece_body(false, true, true, &p);
    close(volume(&k, &mut rolled), 822.654314, 1e-5);
    solidcore::topology::brep::p5::transform(&mut k, &mut rolled, &p.invert(), None, false)
        .unwrap();
    let bb = brep_bounding_box(&k, &rolled, true).unwrap();
    close(bb.max().z, 2.732050807568877, 1e-3);
}
