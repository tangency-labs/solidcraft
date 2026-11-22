//! Every record kind goes to the kernel as the kernel kind of the same fields (checked one way
//! against literals, so a swap made in both directions is caught), and back unchanged, bit for
//! bit; a malformed record is carried over for the kernel to refuse.

use solidcore::eval::{self as k, CurveEval, SurfaceEval};
use solidcore::prism::profile as kp;
use solidcore_convert::{
    curve, curve2, frame, loop2, plane, record_curve, record_curve2, record_frame, record_list,
    record_loop2, record_pair, record_plane, record_region2, record_seg2, record_surface,
    record_table, record_triple, record_vec2, record_vec3, region2, seg2, surface, vec2, vec3,
};
use solidcore_record::brep::{Curve, Curve2, Frame, Surface};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3, MAX_BEZIER_CTRL};

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
        assert_eq!(curve(&r).unwrap(), want);
        assert_eq!(record_curve(curve(&r).unwrap()).unwrap(), r);
    }
}

#[test]
fn every_surface_kind_becomes_the_kernel_kind_and_comes_back() {
    for (r, want) in surfaces() {
        assert_eq!(surface(&r).unwrap(), want);
        assert_eq!(record_surface(surface(&r).unwrap()).unwrap(), r);
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

/// Within the depth bound, malformed geometry (knots that do not match its points, a NaN) is
/// carried over as it is, and the kernel's evaluator is what refuses it.
#[test]
fn a_malformed_record_is_carried_over_for_the_kernel_to_refuse() {
    let bad_spline = Surface::Spline {
        u_knots: vec![0.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![vec![[0.0; 3]; 2]; 2],
    };
    assert_eq!(
        record_surface(surface(&bad_spline).unwrap()).unwrap(),
        bad_spline
    );
    assert!(SurfaceEval::new(&surface(&bad_spline).unwrap()).is_err());
    let bad_curve = Curve::Spline {
        knots: vec![0.0, 1.0],
        points: vec![[0.0; 3]; 3],
    };
    assert_eq!(record_curve(curve(&bad_curve).unwrap()).unwrap(), bad_curve);
    assert!(CurveEval::new(&curve(&bad_curve).unwrap()).is_err());
    let nan = Surface::Sphere {
        frame: Frame {
            origin: [f64::NAN, 0.0, 0.0],
            ..f()
        },
        radius: 1.0,
    };
    let Surface::Sphere { frame: g, .. } = record_surface(surface(&nan).unwrap()).unwrap() else {
        panic!("a sphere")
    };
    assert!(g.origin[0].is_nan());
}

// These builders and destructors never recurse, including for rejected borrowed inputs.
// The same shapes are built independently on both sides of the conversion boundary.
macro_rules! depth_fixtures {
    ($module:ident, $types:ident) => {
        mod $module {
            use super::$types::{Curve, Surface};

            fn line() -> Curve {
                Curve::Line {
                    start: [0.0; 3],
                    end: [1.0; 3],
                }
            }
            fn plane() -> Surface {
                Surface::Plane {
                    origin: [0.0; 3],
                    u_axis: [1.0, 0.0, 0.0],
                    v_axis: [0.0, 1.0, 0.0],
                }
            }
            fn intersection(leader: Curve, first: Surface) -> Curve {
                Curve::SurfaceIntersection {
                    surfaces: [Box::new(first), Box::new(plane())],
                    leader: Box::new(leader),
                    tolerance: 1e-6,
                }
            }
            fn revolution(profile: Curve) -> Surface {
                Surface::PlacedRevolution {
                    profile: Box::new(profile),
                    origin: [0.0; 3],
                    axis: [0.0, 0.0, 1.0],
                    matrix: super::M,
                    forward: true,
                }
            }
            // Exactly `depth` edges from root to deepest leaf. Profile chains alternate
            // intersection surface and revolution profile; leader chains stay in curves.
            pub fn curve(depth: usize, profiles: bool) -> Curve {
                if !profiles {
                    let mut c = line();
                    for _ in 0..depth {
                        c = intersection(c, plane());
                    }
                    return c;
                }
                if depth % 2 == 0 {
                    let mut c = line();
                    for _ in 0..depth / 2 {
                        c = intersection(line(), revolution(c));
                    }
                    c
                } else {
                    let mut s = plane();
                    for _ in 0..depth / 2 {
                        s = revolution(intersection(line(), s));
                    }
                    intersection(line(), s)
                }
            }
            pub fn surface(depth: usize, profiles: bool) -> Surface {
                if depth == 0 {
                    plane()
                } else {
                    revolution(curve(depth - 1, profiles))
                }
            }
            enum Node {
                Curve(Curve),
                Surface(Surface),
            }
            fn dismantle(root: Node) {
                let mut pending = vec![root];
                while let Some(node) = pending.pop() {
                    match node {
                        Node::Curve(Curve::SurfaceIntersection {
                            surfaces: [a, b],
                            leader,
                            ..
                        }) => {
                            pending.push(Node::Curve(*leader));
                            pending.push(Node::Surface(*a));
                            pending.push(Node::Surface(*b));
                        }
                        Node::Surface(Surface::PlacedRevolution { profile, .. }) => {
                            pending.push(Node::Curve(*profile));
                        }
                        _ => {}
                    }
                }
            }
            pub fn drop_curve(c: Curve) {
                dismantle(Node::Curve(c));
            }
            pub fn drop_surface(s: Surface) {
                dismantle(Node::Surface(s));
            }
        }
    };
}

use solidcore_record::brep as r;
depth_fixtures!(record_depth, r);
depth_fixtures!(kernel_depth, k);

fn depth_case(case: &str, depth: usize) -> Result<(), &'static str> {
    let profiles = case.ends_with("profile");
    match case {
        "curve-to-kernel-leader" | "curve-to-kernel-profile" => {
            let input = record_depth::curve(depth, profiles);
            let output = curve(&input);
            record_depth::drop_curve(input);
            assert_eq!(output.is_ok(), depth <= 16);
            if let Ok(output) = output {
                kernel_depth::drop_curve(output);
            }
        }
        "surface-to-kernel-leader" | "surface-to-kernel-profile" => {
            let input = record_depth::surface(depth, profiles);
            let output = surface(&input);
            record_depth::drop_surface(input);
            assert_eq!(output.is_ok(), depth <= 16);
            if let Ok(output) = output {
                kernel_depth::drop_surface(output);
            }
        }
        "curve-to-record-leader" | "curve-to-record-profile" => {
            let input = kernel_depth::curve(depth, profiles);
            let output = record_curve(input);
            assert_eq!(output.is_ok(), depth <= 16);
            if let Ok(output) = output {
                record_depth::drop_curve(output);
            }
        }
        "surface-to-record-leader" | "surface-to-record-profile" => {
            let input = kernel_depth::surface(depth, profiles);
            let output = record_surface(input);
            assert_eq!(output.is_ok(), depth <= 16);
            if let Ok(output) = output {
                record_depth::drop_surface(output);
            }
        }
        _ => return Err("unknown depth case"),
    }
    Ok(())
}

#[test]
fn geometry_depth_is_bounded() {
    const CHILD: &str = "SOLIDCORE_CONVERT_DEPTH_CHILD";
    if let Ok(spec) = std::env::var(CHILD) {
        let (case, depth) = spec.split_once(':').unwrap();
        let case = case.to_owned();
        let depth = depth.parse().unwrap();
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || depth_case(&case, depth))
            .unwrap()
            .join()
            .unwrap()
            .unwrap();
        return;
    }
    let executable = std::env::current_exe().unwrap();
    let mut failures = Vec::new();
    for case in [
        "curve-to-kernel-leader",
        "curve-to-kernel-profile",
        "surface-to-kernel-leader",
        "surface-to-kernel-profile",
        "curve-to-record-leader",
        "curve-to-record-profile",
        "surface-to-record-leader",
        "surface-to-record-profile",
    ] {
        for depth in [16, 17, 100_000] {
            let status = std::process::Command::new(&executable)
                .args(["--exact", "geometry_depth_is_bounded", "--nocapture"])
                .env(CHILD, format!("{case}:{depth}"))
                .status()
                .unwrap();
            if !status.success() {
                failures.push(format!("{case} depth {depth}: child {status}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every planar profile segment kind goes to the kernel's of the same fields (checked against
/// literals) and back unchanged; loops and regions keep their segments in order.
#[test]
fn planar_profiles_become_the_kernel_kinds_and_come_back() {
    let (a, b, c, d) = (
        Vec2::new(1.0, 2.0),
        Vec2::new(-3.5, 0.25),
        Vec2::new(0.0, -0.0),
        Vec2::new(7.0, 1e-300),
    );
    let ka = |v: Vec2| solidcore::math::Vec2::new(v.x, v.y);
    let mut p = [Vec2::new(0.0, 0.0); MAX_BEZIER_CTRL];
    p[0] = a;
    p[1] = b;
    p[2] = c;
    let segs = [
        (Seg2::Line { a, b }, kp::Seg2::Line { a: ka(a), b: ka(b) }),
        (
            Seg2::Arc {
                center: c,
                radius: 2.5,
                start: -0.5,
                sweep: -3.0,
            },
            kp::Seg2::Arc {
                center: ka(c),
                radius: 2.5,
                start: -0.5,
                sweep: -3.0,
            },
        ),
        (
            Seg2::Cubic {
                p0: a,
                p1: b,
                p2: c,
                p3: d,
            },
            kp::Seg2::Cubic {
                p0: ka(a),
                p1: ka(b),
                p2: ka(c),
                p3: ka(d),
            },
        ),
        (
            Seg2::Conic {
                a,
                apex: b,
                b: c,
                w: 0.75,
            },
            kp::Seg2::Conic {
                a: ka(a),
                apex: ka(b),
                b: ka(c),
                w: 0.75,
            },
        ),
        (
            Seg2::Bezier { n: 2, p },
            kp::Seg2::Bezier { n: 2, p: p.map(ka) },
        ),
    ];
    for (r, want) in &segs {
        assert_eq!(&seg2(r), want);
        assert_eq!(&record_seg2(&seg2(r)), r);
    }
    assert_eq!(record_vec2(vec2(d)), d);
    let outer = Loop2 {
        segs: segs.iter().map(|(r, _)| *r).collect(),
    };
    let hole = Loop2 {
        segs: vec![segs[0].0, segs[1].0],
    };
    assert_eq!(
        loop2(&outer).segs,
        segs.iter().map(|(_, k)| *k).collect::<Vec<_>>()
    );
    assert_eq!(record_loop2(&loop2(&outer)), outer);
    let region = Region2 {
        outer,
        holes: vec![hole.clone(), hole],
    };
    let k = region2(&region);
    assert_eq!(k.holes.len(), 2);
    assert_eq!(record_region2(&k), region);
}
