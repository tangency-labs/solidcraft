//! WR-685 (#2250; the SolidCraft fork's tangency-labs/smlib@11ea2d9 = d01d7b0's 0bd3f4d, smlib#78): with
//! `fixes.fillet_gn_section_generator` a blend-curve round is built by the G^n section
//! generator, `S(t) = f T1(1 - p1) + (1 - f) T2(p2)` between the rails.
//!
//! Expected values: the closed form of the section between two planes (the feet are
//! linear in tau, so the jets past order 1 vanish and `T_i` is the foot line itself), and
//! the fork-quoted areas cut off a square corner (113/756 r^2 at the thumbweight
//! w* = 0.034293402266607321, 0.154192655384275 r^2 at w = 0, both as #2252 quotes them
//! from the fork's commits).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::needless_range_loop
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_section::section::{GnFaces, GnSectionFsg};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, Vec2, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

/// The section between the planes z = 0 and x = 0 for the ball of radius r centred at
/// (r, y, r), in (x, z): `T1(u) = (u r, 0)`, `T2(u) = (0, u r)`.
fn closed_form(n: usize, w: f64, r: f64, t: f64) -> (f64, f64) {
    let (a, b) = ((1.0 - t).powi(n as i32 + 1), t.powi(n as i32 + 1));
    let f = a / (a + b);
    let p1 = t / (1.0 - w + w * t);
    let p2 = t * (1.0 - w) / (1.0 - w * t);
    (f * (1.0 - p1) * r, (1.0 - f) * p2 * r)
}

/// The area the closed-form section cuts off the corner (the shoelace sum over 200000
/// steps; the face lines through the corner add nothing).
fn closed_form_area(n: usize, w: f64, r: f64) -> f64 {
    let m = 200_000;
    let mut a = 0.0;
    let mut prev = closed_form(n, w, r, 0.0);
    for i in 1..=m {
        let p = closed_form(n, w, r, f64::from(i) / f64::from(m));
        a += prev.0 * p.1 - p.0 * prev.1;
        prev = p;
    }
    0.5 * a.abs()
}

fn plane(x_axis: Vec3, y_axis: Vec3) -> Surface {
    Surface::from(Plane::new(
        Vec3::ZERO,
        x_axis,
        y_axis,
        Vec2::new(1.0, 1.0),
        &Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
    ))
}

fn line(dim: u64, a: [f64; 3], b: [f64; 3]) -> BSplineCurve {
    BSplineCurve::create_line_segment(
        dim,
        Point3::new(a[0], a[1], a[2]),
        Point3::new(b[0], b[1], b[2]),
    )
    .unwrap()
}

/// The generator's surface between the square corner's rails, the ball of radius r along
/// y from 0 to `len`.
fn corner_surface(
    n: usize,
    w: f64,
    r: f64,
    len: f64,
) -> (
    GnSectionFsg,
    solidcore::nurbs::b_spline_surface::BSplineSurface,
) {
    let k = Kernel::default();
    let f1 = plane(Vec3::X, Vec3::Y); // z = 0, (u, v) = (x, y)
    let f2 = plane(Vec3::Y, Vec3::Z); // x = 0, (u, v) = (y, z)
    let center = line(3, [r, 0.0, r], [r, len, r]);
    let mut rail1 = line(3, [r, 0.0, 0.0], [r, len, 0.0]);
    let mut rail2 = line(3, [0.0, 0.0, r], [0.0, len, r]);
    let uv1 = line(2, [r, 0.0, 0.0], [r, len, 0.0]);
    let uv2 = line(2, [0.0, r, 0.0], [len, r, 0.0]);
    let mut g = GnSectionFsg::new(n, w);
    let s = g
        .create_surface_from_curves(
            &k,
            &center,
            [&mut rail1, &mut rail2],
            1e-6,
            GnFaces {
                surfaces: [&f1, &f2],
                uv: [&uv1, &uv2],
            },
        )
        .unwrap();
    (g, s)
}

/// The surface's section at every u is the closed form, for orders 2 to 4 and two
/// thumbweights; the boundaries are the rails and the certificate holds.
#[test]
fn square_corner_section_is_the_closed_form() {
    let (r, len) = (3.0, 7.0);
    for n in 2..=4 {
        for w in [0.0, 0.3] {
            let (g, s) = corner_surface(n, w, r, len);
            assert!(
                g.certified_bound >= 0.0 && g.certified_bound <= 1e-7,
                "{g:?}"
            );
            assert!(g.certified_spans >= 1);
            for u in [0.0, 0.37, 1.0] {
                for i in 0..=16 {
                    let t = f64::from(i) / 16.0;
                    let p = s.evaluate_point(Point2::new(u, t)).unwrap();
                    let (x, z) = closed_form(n, w, r, t);
                    let want = Point3::new(x, u * len, z);
                    assert!(
                        p.distance(want) <= 1e-12 * r,
                        "n {n} w {w} u {u} t {t}: {p:?} want {want:?}"
                    );
                }
            }
        }
    }
}

/// The area the section cuts off the corner, `(1/2) |int x z' - z x' dt|` over the
/// surface's own section (Simpson on 2000 steps of its points, z' and x' by the chord).
fn cut_area(s: &solidcore::nurbs::b_spline_surface::BSplineSurface) -> f64 {
    let m = 20_000;
    let pts: Vec<Point3> = (0..=m)
        .map(|i| {
            s.evaluate_point(Point2::new(0.5, f64::from(i) / f64::from(m)))
                .unwrap()
        })
        .collect();
    // the shoelace sum over the section and the corner (the two face lines pass through
    // the corner, so they add nothing)
    let mut a = 0.0;
    for w in pts.windows(2) {
        a += w[0].x * w[1].z - w[1].x * w[0].z;
    }
    0.5 * a.abs()
}

#[test]
fn square_corner_cut_areas_are_the_fork_s() {
    let r = 3.0;
    let (_, s) = corner_surface(2, 0.034_293_402_266_607_321, r, 7.0);
    let a = cut_area(&s) / (r * r);
    assert!((a - 113.0 / 756.0).abs() < 1e-8, "{a}");
    let (_, s) = corner_surface(2, 0.0, r, 7.0);
    let a = cut_area(&s) / (r * r);
    assert!((a - 0.154_192_655_384_275).abs() < 1e-8, "{a}");
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap_or_else(|| panic!("no edge near {p:?}"))
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The kernel with the G^n section on or off. With it, the fork's surface-and-plane trace
/// (WR-682, `fillet_surface_plane_curve_on_plane_equation`, tangency-labs/smlib@5bdcddf =
/// d01d7b0's 34b7a22, which precedes the G^n section in the fork) is on too: the end ribs
/// of an order 3 or 4 section (degree 10 or 13 across) against the box's end planes are
/// traced on the plane, as in the fork; upstream's intersector fails them (1001).
fn kernel(gn: bool) -> Kernel {
    let mut fixes = UpstreamFixes::default();
    fixes.fillet_gn_section_generator = gn;
    fixes.fillet_surface_plane_curve_on_plane_equation = gn;
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes;
    k
}

/// The box 40 x 30 x 20 with its top back edge rounded r 3 by the order-2 section at
/// w = 0: it loses 40 times the area the section cuts off, 0.154192655384275 r^2 (the
/// fork's box-edge case). Without the flag the order-2 blend curve is built.
#[test]
fn box_edge_order_2_round_takes_off_the_section_s_area() {
    let mut k = kernel(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let e = pick(&k, &b, [20.0, 30.0, 20.0]);
    fillet_edges(&mut k, &mut b, &[e], 3.0, 2, 2, 0.0).unwrap();
    let v = volume_of(&k, &mut b);
    let want = 24000.0 - 40.0 * 9.0 * 0.154_192_655_384_275;
    eprintln!("box edge volume {v:.17e}");
    assert!((v - want).abs() <= 1e-7 * want, "{v} want {want}");

    // orders 3 and 4 and the thumbweight w*: each takes off 40 times its section's area,
    // from the closed form (113/756 r^2 at w*, the fork's figure)
    for (n, w) in [(3, 0.0), (4, 0.0), (2, 0.034_293_402_266_607_321)] {
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let e = pick(&k, &b, [20.0, 30.0, 20.0]);
        fillet_edges(&mut k, &mut b, &[e], 3.0, 2, n, w).unwrap();
        let v = volume_of(&k, &mut b);
        let want = 24000.0 - 40.0 * closed_form_area(n as usize, w, 3.0);
        eprintln!("box edge n {n} w {w} volume {v:.17e}");
        assert!(
            (v - want).abs() <= 1e-7 * want,
            "n {n} w {w}: {v} want {want}"
        );
    }

    // an order or thumbweight the section cannot take is refused
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let e = pick(&k, &b, [20.0, 30.0, 20.0]);
    assert!(fillet_edges(&mut k, &mut b, &[e], 3.0, 2, 1, 0.0).is_err());
    assert!(fillet_edges(&mut k, &mut b, &[e], 3.0, 2, 2, 1.0).is_err());

    // off: the blend curve, which takes off a different area
    let mut k = kernel(false);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let e = pick(&k, &b, [20.0, 30.0, 20.0]);
    if fillet_edges(&mut k, &mut b, &[e], 3.0, 2, 2, 0.0).is_ok() {
        let v_off = volume_of(&k, &mut b);
        assert!((v_off - want).abs() > 1e-4, "{v_off}");
    }
}

/// The fork's boss root: an r 8, 10 high cylinder on a 60 x 40 x 10 plate, its root
/// rounded r 2 by the order-2 section at w = 0. The plate and boss are 24000 + 640 pi;
/// the round adds the section's area turned about the boss's axis, which the fork quotes
/// (and #2252 checks from the section) as 26043.0880831714 in all. The fork certifies
/// this blend within 3.9e-6 on 227 spans, off the exact blend by 1.75e-7.
///
/// Ignored by default: its certificate takes about 15 minutes in a debug build (run it with
/// `cargo test --release --test wr685_gn_section -- --ignored`).
#[test]
#[ignore = "slow in a debug build: a 227-span certificate"]
fn boss_root_order_2_round_adds_the_section_s_turned_area() {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::primitive::api_primitives::create_cylinder;
    let mut k = kernel(true);
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let boss = create_cylinder(&mut k, Point3::new(30.0, 20.0, 10.0), 8.0, 10.0).unwrap();
    let mut b = boolean_union(&mut k, plate, boss).unwrap();
    let s = 4.0 * 2f64.sqrt();
    let e = pick(&k, &b, [30.0 + s, 20.0 + s, 10.0]);
    fillet_edges(&mut k, &mut b, &[e], 2.0, 2, 2, 0.0).unwrap();
    let v = volume_of(&k, &mut b);
    let want = 26_043.088_083_171_4;
    eprintln!("boss root volume {v:.17e}");
    assert!((v - want).abs() <= 1e-6 * want, "{v} want {want}");
}

/// The fork's pipe on a plate: an r 10 cylinder along x (axis at y 20, z 16) on a
/// 60 x 40 x 10 plate, one root line rounded r 2 by the order-2 section at w = 0. Its
/// faces are a plane and a cylinder, so the jets are not linear in tau and the
/// certificate encloses curved faces; the fork certifies it on 3 spans. Volume
/// 40258.5435962825, as #2252 quotes from the fork's commit (the section's area times 60).
#[test]
fn pipe_root_order_2_round_takes_the_fork_s_volume() {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::primitive::api_primitives::create_cylinder;
    let mut k = kernel(true);
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let mut pipe = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0).unwrap();
    let pl = Axis2Placement::new(
        Point3::new(0.0, 20.0, 16.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    solidcore::topology::brep::p5::transform(&mut k, &mut pipe, &pl, None, false).unwrap();
    let mut b = boolean_union(&mut k, plate, pipe).unwrap();
    let e = pick(&k, &b, [30.0, 28.0, 10.0]);
    fillet_edges(&mut k, &mut b, &[e], 2.0, 2, 2, 0.0).unwrap();
    let v = volume_of(&k, &mut b);
    let want = 40_258.543_596_282_5;
    eprintln!("pipe root volume {v:.17e}");
    assert!((v - want).abs() <= 1e-7 * want, "{v} want {want}");
}

/// The fork's 6148773 (= d01d7b0's 1947a45): at the equal area thumbweight (the default,
/// -2) the box edge's round takes off what the ball's circle does, 40 times
/// `(1 - pi/4) r^2` (closed form), at every order. Fork d01d7b0 (built locally) gives
/// 23922.743344462557, 23922.743341082103 and 23922.743340991368 for n = 2, 3, 4.
#[test]
fn box_edge_equal_area_round_takes_off_the_circle_s_area() {
    use solidcore::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT;
    let mut k = kernel(true);
    let want = 24000.0 - 40.0 * 9.0 * (1.0 - core::f64::consts::FRAC_PI_4);
    for n in 2..=4 {
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let e = pick(&k, &b, [20.0, 30.0, 20.0]);
        fillet_edges(&mut k, &mut b, &[e], 3.0, 2, n, GN_DEFAULT_THUMBWEIGHT).unwrap();
        let v = volume_of(&k, &mut b);
        eprintln!("box edge equal area n {n} volume {v:.17e}");
        assert!((v - want).abs() <= 1e-7 * want, "n {n}: {v} want {want}");
    }
    // a negative thumbweight past -1 other than the default is refused
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let e = pick(&k, &b, [20.0, 30.0, 20.0]);
    assert!(fillet_edges(&mut k, &mut b, &[e], 3.0, 2, 2, -1.0).is_err());
}

/// The box's corner (40, 30, 20) with its three edges rounded r 3 by the order-2 section
/// in one call: the solvers share one generator, so the first blend's wedge resolves the
/// equal area thumbweight for all three, and the corner and the ease before its setbacks
/// (thumbweight 0, the fork's 22d258a = d01d7b0's adb9854) are built from it. Fork
/// d01d7b0 (built locally): 23829.652499315507 at the equal area default and
/// 23876.361929859548 at w = 0.
#[test]
fn box_corner_equal_area_rounds_take_the_fork_s_volume() {
    use solidcore::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT;
    let mut k = kernel(true);
    for (w, want) in [
        (GN_DEFAULT_THUMBWEIGHT, 23829.652499315507),
        (0.0, 23876.361929859548),
    ] {
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let es = [
            pick(&k, &b, [20.0, 30.0, 20.0]),
            pick(&k, &b, [40.0, 15.0, 20.0]),
            pick(&k, &b, [40.0, 30.0, 10.0]),
        ];
        fillet_edges(&mut k, &mut b, &es, 3.0, 2, 2, w).unwrap();
        let v = volume_of(&k, &mut b);
        eprintln!("box corner w {w} volume {v:.17e}");
        assert!((v - want).abs() <= 1e-7 * want, "w {w}: {v} want {want}");
    }
}

/// Two edges of different wedges rounded r 2 in one call, after the box's top back edge is
/// chamfered 5: the chamfer's top edge (135 degrees) and the box's bottom front edge (90).
/// The fork's solvers share one generator, so the equal area thumbweight is the first
/// blend's wedge's, for both: the volume depends on the edges' order. Fork d01d7b0 (built
/// locally): 23468.183912205881 with the chamfer's edge first, 23461.498019658513 with the
/// box's first, and 23496.557686573007 and 23465.663708708515 for each alone.
#[test]
fn equal_area_thumbweight_is_the_first_blend_s_for_every_edge() {
    use solidcore::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT;
    let mut k = kernel(true);
    let cases: [(&[usize], f64); 4] = [
        (&[0, 1], 23468.183912205881),
        (&[1, 0], 23461.498019658513),
        (&[0], 23496.557686573007),
        (&[1], 23465.663708708515),
    ];
    for (order, want) in cases {
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let c = pick(&k, &b, [20.0, 30.0, 20.0]);
        fillet_edges(&mut k, &mut b, &[c], 5.0, 0, 2, 0.0).unwrap();
        let both = [
            pick(&k, &b, [20.0, 25.0, 20.0]),
            pick(&k, &b, [20.0, 0.0, 0.0]),
        ];
        let es: Vec<EdgeId> = order.iter().map(|&i| both[i]).collect();
        fillet_edges(&mut k, &mut b, &es, 2.0, 2, 2, GN_DEFAULT_THUMBWEIGHT).unwrap();
        let v = volume_of(&k, &mut b);
        eprintln!("edges {order:?} volume {v:.17e}");
        assert!(
            (v - want).abs() <= 1e-7 * want,
            "{order:?}: {v} want {want}"
        );
    }
}

/// The fork's 22d258a (= d01d7b0's adb9854): the ease of a round before a setback blends it
/// into the order-2 section at thumbweight 0, not at the equal area default. The 40 mm
/// box's corner (40, 40, 40), its three edges rounded r 8, set back 12 and eased 4 (as
/// `issue_1376_setback_ease.rs`). Fork d01d7b0, built locally and driven through
/// `SmFilletExecutive` the same way, gives 62375.682794873181 (62279.332678480074 without
/// the ease); with the equal area section as the ease's target the volume differs.
#[test]
fn eased_setback_takes_the_order_2_section_at_thumbweight_0() {
    use solidcore::blend::fillet_executive::session::FilletSession;
    use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
    use solidcore::topology::{edge, vertex};
    let at = |b: &Brep, v| {
        let q = vertex::point(b, v).unwrap();
        (q.x - 40.0).abs() + (q.y - 40.0).abs() + (q.z - 40.0).abs() < 1e-6
    };
    for (ease, want) in [(0.0, 62279.332678480074), (4.0, 62375.682794873181)] {
        let mut k = kernel(true);
        k.fixes.fillet_setback_from_start_vertex = true;
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 40.0).unwrap();
        let edges: Vec<EdgeId> = b
            .edges()
            .ids()
            .filter(|&e| {
                let (s, t) = edge::start_end_vertices(&b, e).unwrap();
                [s, t].into_iter().flatten().any(|v| at(&b, v))
            })
            .collect();
        assert_eq!(edges.len(), 3);
        {
            let mut s = FilletSession::new(&mut k, &mut b).unwrap();
            s.set_mark_corner_patches(true);
            s.prepare_constant_radius_edges(&mut k, &edges, 8.0, 1, 2, -2.0)
                .unwrap();
            s.create_corners(&k, None).unwrap();
            let corner = s
                .fillet_corners()
                .unwrap()
                .into_iter()
                .find(|&id| {
                    s.corner(id)
                        .unwrap()
                        .vertex
                        .is_some_and(|v| at(s.target(), v))
                })
                .unwrap();
            s.set_corner_setback(corner, 12.0).unwrap();
            s.set_corner_ease(corner, ease).unwrap();
            s.do_filleting(&mut k, None).unwrap();
        }
        stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
        let store = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        let v = brep_volume(&k, &mut b, 1e-6, &attrs).unwrap();
        eprintln!("eased corner ease {ease} volume {v:.17e}");
        assert!(
            (v - want).abs() <= 1e-7 * want,
            "ease {ease}: {v} want {want}"
        );
    }
}
