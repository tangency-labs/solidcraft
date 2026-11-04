//! Every record kind goes to the kernel as the kernel kind of the same fields (checked one way
//! against literals, so a swap made in both directions is caught), and back unchanged, bit for
//! bit; a malformed record is carried over for the kernel to refuse.

use solidcore::eval::{self as k, CurveEval, SurfaceEval};
use solidcore_convert::{
    curve, curve2, frame, plane, record_curve, record_curve2, record_frame, record_list,
    record_pair, record_plane, record_surface, record_table, record_triple, record_vec3, surface,
    vec3,
};
use solidcore_record::brep::{Curve, Curve2, Frame, Surface};
use solidcore_record::{Plane, Vec3};

const O: [f64; 3] = [1.5, -2.25, 3.125];
const X: [f64; 3] = [0.6, 0.8, 0.0];
const Y: [f64; 3] = [0.0, 0.0, 1.0];

fn f() -> Frame {
    Frame {
        origin: O,
        x_axis: X,
        y_axis: Y,
    }
}

fn kf() -> k::Frame {
    k::Frame {
        origin: O,
        x_axis: X,
        y_axis: Y,
    }
}

const M: [[f64; 4]; 4] = [
    [1.0, 2.0, 3.0, 4.0],
    [5.0, 6.0, 7.0, 8.0],
    [9.0, 10.0, 11.0, 12.0],
    [13.0, 14.0, 15.0, 16.0],
];

/// Each record curve with the kernel curve it must become.
fn curves() -> Vec<(Curve, k::Curve)> {
    let pts = vec![[0.0, 0.5, 1.0], [1.0, 1.5, 2.0], [-1.0, 0.25, 7.0]];
    let hom = vec![[0.0, 0.5, 1.0, 1.0], [2.0, 3.0, 4.0, 2.0]];
    vec![
        (
            Curve::Line {
                start: [0.1, 0.2, 0.3],
                end: [1.0, 2.0, 3.0],
            },
            k::Curve::Line {
                start: [0.1, 0.2, 0.3],
                end: [1.0, 2.0, 3.0],
            },
        ),
        (
            Curve::Circle {
                frame: f(),
                radius: 2.5,
            },
            k::Curve::Circle {
                frame: kf(),
                radius: 2.5,
            },
        ),
        (
            Curve::Ellipse {
                frame: f(),
                radii: [3.0, 1.25],
            },
            k::Curve::Ellipse {
                frame: kf(),
                radii: [3.0, 1.25],
            },
        ),
        (
            Curve::Polyline {
                points: pts.clone(),
            },
            k::Curve::Polyline {
                points: pts.clone(),
            },
        ),
        (
            Curve::Spline {
                knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
                points: pts.clone(),
            },
            k::Curve::Spline {
                knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
                points: pts,
            },
        ),
        (
            Curve::RationalSpline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                homogeneous: hom.clone(),
            },
            k::Curve::RationalSpline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                homogeneous: hom,
            },
        ),
    ]
}

/// Each record surface with the kernel surface it must become: every u/v pair distinct.
fn surfaces() -> Vec<(Surface, k::Surface)> {
    let (uk, vk) = (vec![0.0, 0.0, 1.0, 1.0], vec![-1.0, -1.0, 0.0, 2.0, 2.0]);
    // Two rows along u of three points along v: a transposition changes the shape.
    let pts = vec![
        vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.1], [0.0, 2.0, 0.3]],
        vec![[1.0, 0.0, 0.5], [1.0, 1.0, 0.7], [1.0, 2.0, 1.1]],
    ];
    let hom = vec![
        vec![
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.5, 0.25, 0.5],
            [0.0, 2.0, 0.3, 1.0],
        ],
        vec![
            [2.0, 0.0, 1.0, 2.0],
            [1.0, 1.0, 0.7, 1.0],
            [0.75, 1.5, 0.5, 0.75],
        ],
    ];
    let line = (
        Curve::Line {
            start: [2.0, 0.0, 0.0],
            end: [3.0, 0.0, 1.0],
        },
        k::Curve::Line {
            start: [2.0, 0.0, 0.0],
            end: [3.0, 0.0, 1.0],
        },
    );
    vec![
        (
            Surface::Plane {
                origin: [0.0, 1.0, 2.0],
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 1.0, 0.0],
            },
            k::Surface::Plane {
                origin: [0.0, 1.0, 2.0],
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 1.0, 0.0],
            },
        ),
        (
            Surface::PlaneThrough {
                origin: [0.0; 3],
                u_point: [4.0, 0.0, 0.0],
                v_point: [0.0, 5.0, 0.0],
            },
            k::Surface::PlaneThrough {
                origin: [0.0; 3],
                u_point: [4.0, 0.0, 0.0],
                v_point: [0.0, 5.0, 0.0],
            },
        ),
        (
            Surface::Cylinder {
                frame: f(),
                radius: 2.0,
            },
            k::Surface::Cylinder {
                frame: kf(),
                radius: 2.0,
            },
        ),
        (
            Surface::Cone {
                frame: f(),
                radius_at_origin: 2.0,
                half_angle: 0.3,
            },
            k::Surface::Cone {
                frame: kf(),
                radius_at_origin: 2.0,
                half_angle: 0.3,
            },
        ),
        (
            Surface::Sphere {
                frame: f(),
                radius: 2.0,
            },
            k::Surface::Sphere {
                frame: kf(),
                radius: 2.0,
            },
        ),
        (
            Surface::Torus {
                frame: f(),
                major_radius: 5.0,
                minor_radius: 1.0,
                tube_start: 0.5,
            },
            k::Surface::Torus {
                frame: kf(),
                major_radius: 5.0,
                minor_radius: 1.0,
                tube_start: 0.5,
            },
        ),
        (
            Surface::BallPatch {
                center: [1.0, 2.0, 3.0],
                radius: 2.0,
            },
            k::Surface::BallPatch {
                center: [1.0, 2.0, 3.0],
                radius: 2.0,
            },
        ),
        (
            Surface::Spline {
                u_knots: uk.clone(),
                v_knots: vk.clone(),
                points: pts.clone(),
            },
            k::Surface::Spline {
                u_knots: uk.clone(),
                v_knots: vk.clone(),
                points: pts,
            },
        ),
        (
            Surface::RationalSpline {
                u_knots: uk.clone(),
                v_knots: vk.clone(),
                homogeneous: hom.clone(),
            },
            k::Surface::RationalSpline {
                u_knots: uk,
                v_knots: vk,
                homogeneous: hom,
            },
        ),
        (
            Surface::PlacedRevolution {
                profile: Box::new(line.0.clone()),
                origin: [0.5, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                matrix: M,
                forward: false,
            },
            k::Surface::PlacedRevolution {
                profile: Box::new(line.1.clone()),
                origin: [0.5, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                matrix: M,
                forward: false,
            },
        ),
        (
            Surface::PlacedRevolution {
                profile: Box::new(line.0),
                origin: [0.0; 3],
                axis: [1.0, 0.0, 0.0],
                matrix: M,
                forward: true,
            },
            k::Surface::PlacedRevolution {
                profile: Box::new(line.1),
                origin: [0.0; 3],
                axis: [1.0, 0.0, 0.0],
                matrix: M,
                forward: true,
            },
        ),
    ]
}

#[test]
fn every_curve_kind_becomes_the_kernel_kind_and_comes_back() {
    let mut all = curves();
    let s = surfaces();
    let (sa, ka) = s[0].clone();
    let (sb, kb) = s[4].clone();
    let (la, lk) = all[0].clone();
    all.push((
        Curve::SurfaceIntersection {
            surfaces: [Box::new(sa), Box::new(sb)],
            leader: Box::new(la),
            tolerance: 1e-6,
        },
        k::Curve::SurfaceIntersection {
            surfaces: [Box::new(ka), Box::new(kb)],
            leader: Box::new(lk),
            tolerance: 1e-6,
        },
    ));
    for (r, want) in all {
        assert_eq!(curve(&r), want);
        assert_eq!(record_curve(curve(&r)), r);
    }
}

#[test]
fn every_surface_kind_becomes_the_kernel_kind_and_comes_back() {
    for (r, want) in surfaces() {
        assert_eq!(surface(&r), want);
        assert_eq!(record_surface(surface(&r)), r);
    }
}

#[test]
fn pcurves_frames_and_planes_become_the_kernel_kinds_and_come_back() {
    let (kn, pts, hom) = (
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![[0.0, 0.5], [1.0, 2.0], [3.0, -1.0]],
        vec![[1.0, 0.0, 1.0], [0.5, 0.25, 0.5], [0.0, 1.0, 1.0]],
    );
    for (r, want) in [
        (
            Curve2::Spline {
                knots: kn.clone(),
                points: pts.clone(),
            },
            k::Curve2::Spline {
                knots: kn.clone(),
                points: pts,
            },
        ),
        (
            Curve2::RationalSpline {
                knots: kn.clone(),
                homogeneous: hom.clone(),
            },
            k::Curve2::RationalSpline {
                knots: kn,
                homogeneous: hom,
            },
        ),
    ] {
        assert_eq!(curve2(&r), want);
        assert_eq!(record_curve2(curve2(&r)), r);
    }
    assert_eq!(frame(&f()), kf());
    assert_eq!(record_frame(&frame(&f())), f());
    let p = Plane {
        origin: Vec3::new(1.0, 2.0, 3.0),
        x: Vec3::new(0.0, 1.0, 0.0),
        y: Vec3::new(0.0, 0.0, 1.0),
    };
    assert_eq!(
        plane(&p),
        k::Frame {
            origin: [1.0, 2.0, 3.0],
            x_axis: [0.0, 1.0, 0.0],
            y_axis: [0.0, 0.0, 1.0]
        }
    );
    assert_eq!(record_plane(&plane(&p)), p);
}

/// Copies keep every bit: a negative zero, a NaN payload, an infinity, a subnormal.
#[test]
fn vectors_keep_every_bit() {
    let odd = [
        -0.0,
        f64::from_bits(0x7ff8_0000_dead_beef),
        f64::NEG_INFINITY,
        f64::from_bits(1),
    ];
    for x in odd {
        for v in [
            Vec3::new(x, 1.0, -2.0),
            Vec3::new(3.0, x, 0.25),
            Vec3::new(0.5, -7.0, x),
        ] {
            let kv = vec3(v);
            assert_eq!(
                [kv.x.to_bits(), kv.y.to_bits(), kv.z.to_bits()],
                [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
            );
            let back = record_vec3(kv);
            assert_eq!(
                [back.x.to_bits(), back.y.to_bits(), back.z.to_bits()],
                [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
            );
        }
    }
    let (a, b, c) = (
        solidcore::Vec3::new(1.0, 2.0, 3.0),
        solidcore::Vec3::new(-0.0, 5.0, 6.0),
        solidcore::Vec3::new(7.0, 8.0, 9.0),
    );
    assert_eq!(
        record_pair((a, b)),
        (Vec3::new(1.0, 2.0, 3.0), Vec3::new(-0.0, 5.0, 6.0))
    );
    assert_eq!(
        record_triple((a, b, c)),
        (
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(-0.0, 5.0, 6.0),
            Vec3::new(7.0, 8.0, 9.0)
        )
    );
    assert_eq!(
        record_list(&[c, a]),
        vec![Vec3::new(7.0, 8.0, 9.0), Vec3::new(1.0, 2.0, 3.0)]
    );
    let mut d: k::Partials =
        [[solidcore::Vec3::ZERO; k::MAX_PARTIAL_ORDER + 1]; k::MAX_PARTIAL_ORDER + 1];
    d[1][2] = a;
    d[3][0] = c;
    let t = record_table(&d);
    assert_eq!(
        (t[1][2], t[2][1], t[3][0], t[0][3]),
        (
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::ZERO,
            Vec3::new(7.0, 8.0, 9.0),
            Vec3::ZERO
        )
    );
}

/// Every record kind has its kernel kind, so the conversion never refuses: a malformed record
/// (knots that do not match its points, a NaN) is carried over as it is, and the kernel's
/// evaluator is what refuses it.
#[test]
fn a_malformed_record_is_carried_over_for_the_kernel_to_refuse() {
    let bad_spline = Surface::Spline {
        u_knots: vec![0.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![vec![[0.0; 3]; 2]; 2],
    };
    assert_eq!(record_surface(surface(&bad_spline)), bad_spline);
    assert!(SurfaceEval::new(&surface(&bad_spline)).is_err());
    let bad_curve = Curve::Spline {
        knots: vec![0.0, 1.0],
        points: vec![[0.0; 3]; 3],
    };
    assert_eq!(record_curve(curve(&bad_curve)), bad_curve);
    assert!(CurveEval::new(&curve(&bad_curve)).is_err());
    let nan = Surface::Sphere {
        frame: Frame {
            origin: [f64::NAN, 0.0, 0.0],
            ..f()
        },
        radius: 1.0,
    };
    let Surface::Sphere { frame: g, .. } = record_surface(surface(&nan)) else {
        panic!("a sphere")
    };
    assert!(g.origin[0].is_nan());
}
