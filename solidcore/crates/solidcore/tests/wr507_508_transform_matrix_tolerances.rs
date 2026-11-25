//! WR-507 / WR-508 (#1520, #1521) through the 4x4 `SmBrep::Transform`
//! ([`transform_matrix`]), the overload SolidCraft calls: the fork's own regression
//! tests (`TransformKeepsEdgeTolerance`, `ScaledBodyWithoutVertices` in
//! tangency-labs/smlib#49 and #50, `capi/tests/flat_roundtrip.cpp`) on the port. Each
//! fails on upstream's rule. And the size a scale gives a body staying that body's: the
//! kernel context and the other bodies keep theirs.
#![allow(clippy::unwrap_used)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives as api;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p1;
use solidcore::topology::{edge, vertex};
use solidcore::Kernel;
use std::f64::consts::PI;

/// WR-507 (tangency-labs/smlib#49): a 10 x 20 x 30 box at (1, 2, 3) whose first edge and
/// its two vertices are given 1e-3, far above the box's own tolerance. As given, mirrored
/// in x = 30 and turned a quarter about z (each a unit scale through the 4x4) the edge
/// keeps 1e-3, and scaled by 2 it has 2e-3. Upstream sets every tolerance to the body's.
#[test]
fn wr507_transform_matrix_keeps_or_scales_an_edge_tolerance() {
    let wide = 1e-3;
    let moves: [(&str, [[f64; 4]; 4], f64); 4] = [
        (
            "as given",
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            1.0,
        ),
        (
            "mirrored",
            [
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [60.0, 0.0, 0.0, 1.0],
            ],
            1.0,
        ),
        (
            "turned",
            [
                [0.0, 1.0, 0.0, 0.0],
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [5.0, 6.0, 7.0, 1.0],
            ],
            1.0,
        ),
        (
            "scaled",
            [
                [2.0, 0.0, 0.0, 0.0],
                [0.0, 2.0, 0.0, 0.0],
                [0.0, 0.0, 2.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            2.0,
        ),
    ];
    for (name, m, scale) in moves {
        let mut k = Kernel::default();
        let mut b = api::create_box(&mut k, Point3::new(1.0, 2.0, 3.0), 10.0, 20.0, 30.0).unwrap();
        assert!(
            b.tolerance() < wide * 1e-1,
            "{name}: box tolerance {:e}",
            b.tolerance()
        );
        let e = p1::edges(&b, None).unwrap()[0];
        edge::set_tolerance(&mut b, e, wide, false, false).unwrap();
        for v in [edge::start_vertex(&b, e), edge::end_vertex(&b, e)] {
            let v = v.unwrap().unwrap();
            vertex::set_tolerance(&mut b, v, wide, false, false).unwrap();
        }
        transform_matrix(&mut k, &mut b, &m, true).unwrap();
        let widest = p1::edges(&b, None)
            .unwrap()
            .into_iter()
            .map(|e| edge::tolerance(&b, e).unwrap())
            .fold(0.0_f64, f64::max);
        let want = wide * scale;
        assert!(
            (widest - want).abs() <= 1e-15 * scale,
            "{name}: widest edge tolerance {widest:e}, want {want:e}"
        );
    }
}

/// WR-508 (tangency-labs/smlib#50): a lone torus (R 6, r 2 about (10, 5, 3)), which has
/// no vertex spanning a size, scaled by 2 and by 0.5 through the 4x4. Upstream sizes it
/// from its vertex box and gives it a zero tolerance; the port's tolerance is at
/// least that of the same torus made at the scaled size, and a ball of radius 1.5 (scaled
/// alike) cuts out of its tube to the analytic volume. Without the fix the cut returns a
/// body whose volume is the whole torus' (3790.07 for a scale of 2, 59.220 for 0.5; the
/// fork's C API failed it with 1001). The volume is integrated to 1e-3, not the fork's
/// 1e-8, to keep the unoptimised test short; it is within 4e-5 of the analytic one, and
/// the uncut torus is 3% off.
#[test]
fn wr508_transform_matrix_sizes_a_lone_torus_from_its_box() {
    let c = Point3::new(10.0, 5.0, 3.0);
    for s in [2.0, 0.5] {
        let mut k = Kernel::default();
        let m = [
            [s, 0.0, 0.0, 0.0],
            [0.0, s, 0.0, 0.0],
            [0.0, 0.0, s, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let ck = Point3::new(c.x * s, c.y * s, c.z * s);
        let mut torus = api::create_torus(&mut k, c, 6.0, 2.0).unwrap();
        transform_matrix(&mut k, &mut torus, &m, true).unwrap();
        let mut made_k = Kernel::default();
        let made = api::create_torus(&mut made_k, ck, 6.0 * s, 2.0 * s).unwrap();
        let tol = torus.tolerance();
        assert!(
            tol > 0.0 && tol >= made.tolerance(),
            "scale {s}: tolerance {tol:e}, made at that size {:e}",
            made.tolerance()
        );

        let ball =
            api::create_sphere(&mut k, Point3::new(ck.x + 6.0 * s, ck.y, ck.z), 1.5 * s).unwrap();
        let mut cut = solidcore::boolean::api_brep::boolean_difference(&mut k, torus, ball)
            .unwrap_or_else(|e| panic!("scale {s}: ball cut out of the scaled torus: {e}"));
        let store = AttributeStore::new();
        let key_of = |_| ObjectKey(0);
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &key_of,
        };
        let v = brep_volume(&k, &mut cut, 1e-3, &attrs).unwrap();
        let want = s * s * s * (2.0 * PI * PI * 6.0 * 4.0 - 4.0 / 3.0 * PI * 1.5 * 1.5 * 1.5);
        assert!(
            (v - want).abs() <= 1e-4 * want,
            "scale {s}: volume {v}, want {want}"
        );
    }
}

/// A uniform scale by `s` about the origin, as the 4x4 `transform_matrix` takes it.
fn scale_by(s: f64) -> [[f64; 4]; 4] {
    [
        [s, 0.0, 0.0, 0.0],
        [0.0, s, 0.0, 0.0],
        [0.0, 0.0, s, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// The size a scaled body is given is that body's alone. A 100 x 100 x 10 plate X stores
/// the default estimate, so it answers with the kernel's (50, and a zone tolerance of
/// 1e-5 on the context). A 10 x 10 x 10 box Y scaled by 1000, scaling its tolerances as
/// `solidcore-ops` does, has the estimate of its own box (about its diagonal, 10000 * sqrt(3))
/// and the 1e-4 that goes with it; X's answers and the context's stay what they were.
/// Before, the scale wrote Y's estimate to the kernel context, X answered 17320.5 and the
/// context held 1e-4 until the next body was made.
#[test]
fn scaling_one_body_leaves_the_size_estimate_of_another() {
    use solidcore::geom::tol;

    let mut k = Kernel::default();
    let x = api::create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 100.0, 100.0, 10.0).unwrap();
    let mut y = api::create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let context = |k: &Kernel| {
        (
            k.context.stored_model_size_estimate(),
            k.context.stored_large_small_size_ratio(),
            k.context.this_zone_tol_3d(),
            tol::context_zone_tol_3d(k, Some(&k.context), tol::SmallTopoType::NormalSize),
        )
    };
    let of_x = |k: &Kernel| {
        (
            tol::brep_model_size_estimate(k, &x),
            tol::brep_large_small_size_ratio(k, &x),
            tol::brep_zone_tol_3d(k, &x, tol::SmallTopoType::NormalSize),
            x.tolerance(),
        )
    };
    let (context_before, x_before) = (context(&k), of_x(&k));
    assert_eq!(
        context_before,
        (tol::USE_DEFAULT, tol::USE_DEFAULT, 1e-5, 1e-5)
    );
    assert_eq!(
        x_before,
        (
            tol::MODEL_SIZE_ESTIMATE,
            tol::LARGE_SMALL_SIZE_RATIO,
            1e-5,
            1e-5
        )
    );

    transform_matrix(&mut k, &mut y, &scale_by(1000.0), true).unwrap();

    // Y carries its own size: the diagonal of a box enclosing its 10000 cube, at most
    // grown by the cube's 1e-5 tolerance on each side, in the 1e-4 band
    let diagonal = 10000.0 * 3.0_f64.sqrt();
    let grown = (10000.0 + 2.0 * 1e-5) * 3.0_f64.sqrt();
    let y_size = tol::brep_model_size_estimate(&k, &y);
    assert!(
        y_size >= diagonal && y_size <= grown * (1.0 + 1e-12),
        "Y's estimate {y_size}, its cube's diagonal {diagonal}, grown {grown}"
    );
    assert_eq!(y.this_model_size_estimate(&k, false), y_size);
    assert_eq!(
        tol::brep_zone_tol_3d(&k, &y, tol::SmallTopoType::NormalSize),
        1e-4
    );
    assert_eq!(y.tolerance(), 1e-4);
    // X and the kernel context are as before the scale
    assert_eq!(of_x(&k), x_before);
    assert_eq!(context(&k), context_before);
}

/// The healer sizes a body from that body. The same plate X is healed through its
/// tolerance step twice: on a kernel where nothing else happened, and on one where another
/// box was scaled by 1000 first. Both leave X storing the default estimate with every
/// tolerance at 1e-5. Before, the second history compared X with the scaled box's
/// estimate left on the kernel context and stored 50 on X in place of the default (a
/// value a saved body then carries).
#[test]
fn healing_a_body_after_scaling_another_is_as_on_a_fresh_kernel() {
    use solidcore::geom::tol;
    use solidcore::heal::face_props::HealerOpType;

    let healed_plate = |scale_another_first: bool| {
        let mut k = Kernel::default();
        let mut x =
            api::create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 100.0, 100.0, 10.0).unwrap();
        if scale_another_first {
            let mut y =
                api::create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
            transform_matrix(&mut k, &mut y, &scale_by(1000.0), true).unwrap();
        }
        p1::heal_brep(
            &mut k,
            &mut x,
            HealerOpType::FixTolsizes,
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let mut tolerances = vec![x.tolerance()];
        tolerances.extend(
            p1::vertices(&x, None)
                .unwrap()
                .into_iter()
                .map(|v| vertex::tolerance(&x, v).unwrap()),
        );
        tolerances.extend(
            p1::edges(&x, None)
                .unwrap()
                .into_iter()
                .map(|e| edge::tolerance(&x, e).unwrap()),
        );
        (
            x.this_model_size_estimate(&k, false),
            x.this_large_small_size_ratio(&k, false),
            tol::brep_model_size_estimate(&k, &x),
            tolerances,
        )
    };
    let fresh = healed_plate(false);
    assert_eq!(fresh.0, tol::USE_DEFAULT);
    assert_eq!(fresh.1, tol::USE_DEFAULT);
    assert_eq!(fresh.2, tol::MODEL_SIZE_ESTIMATE);
    assert_eq!(fresh.3, vec![1e-5; 1 + 8 + 12]);
    assert_eq!(healed_plate(true), fresh);
}

/// What the healer's property pass flags on each edge, vertex and face of `b`: too small a
/// tolerance and too large a tolerance for the zone tolerance it is measured with.
fn heal_property_flags(
    k: &Kernel,
    b: &mut solidcore::topology::Brep,
) -> Vec<[solidcore::heal::face_props::Tri; 2]> {
    use solidcore::heal::face_props::{FaceProps, HealerOpType, PropStage};
    use solidcore::heal::heal_data::p2::{EdgeProps, VertexProps};

    let mut flags = Vec::new();
    for (i, e) in p1::edges(b, None).unwrap().into_iter().enumerate() {
        let mut p = EdgeProps::new();
        p.set_props(k, b, e, i as u64, None, None, 0).unwrap();
        flags.push([p.bad_small_zone_tol_3d, p.bad_large_zone_tol_3d]);
    }
    for (i, v) in p1::vertices(b, None).unwrap().into_iter().enumerate() {
        let mut p = VertexProps::new();
        p.set_props(k, b, v, i as u64, None, None).unwrap();
        flags.push([p.bad_small_zone_tol_3d, p.bad_large_zone_tol_3d]);
    }
    for (i, f) in p1::faces(b, None).unwrap().into_iter().enumerate() {
        let mut p: FaceProps<p1::HealLoopHandle> = FaceProps::new();
        p.set_props(
            k,
            b,
            PropStage::Gaps,
            f,
            i as u64,
            HealerOpType::FixTolsizes,
        )
        .unwrap();
        flags.push([p.bad_small_zone_tol_3d, p.bad_large_zone_tol_3d]);
    }
    flags
}

/// The tolerances of `b`, its vertices and its edges, in that order.
fn body_tolerances(b: &solidcore::topology::Brep) -> Vec<f64> {
    let mut tolerances = vec![b.tolerance()];
    tolerances.extend(
        p1::vertices(b, None)
            .unwrap()
            .into_iter()
            .map(|v| vertex::tolerance(b, v).unwrap()),
    );
    tolerances.extend(
        p1::edges(b, None)
            .unwrap()
            .into_iter()
            .map(|e| edge::tolerance(b, e).unwrap()),
    );
    tolerances
}

/// A 1000 cube Y scaled by 2 with its tolerances (as `solidcore-ops` always scales): its
/// own estimate, about its 2000 cube's diagonal (3464), is past the default's band, so
/// its zone tolerance is 1e-4 (the default's is 1e-5), and the scale leaves its
/// tolerances at 1e-4. With `make_another`, an unrelated small box is made after the
/// scale. The kernel also holds a 400 cube Z over Y's far corner, made before the scale.
fn scaled_box_history(
    make_another: bool,
) -> (Kernel, solidcore::topology::Brep, solidcore::topology::Brep) {
    let mut k = Kernel::default();
    let z = api::create_box(
        &mut k,
        Point3::new(1800.0, 1800.0, 1800.0),
        400.0,
        400.0,
        400.0,
    )
    .unwrap();
    let mut y =
        api::create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1000.0, 1000.0, 1000.0).unwrap();
    transform_matrix(&mut k, &mut y, &scale_by(2.0), true).unwrap();
    if make_another {
        another_box(&mut k);
    }
    (k, y, z)
}

fn another_box(k: &mut Kernel) {
    api::create_box(k, Point3::new(-100.0, -100.0, -100.0), 1.0, 1.0, 1.0).unwrap();
}

/// The corner `calc_corner_from_faces` finds (as a Boolean merge asks it for a vertex off
/// its faces) at the origin corner of the scaled box Y once Y's z = 0 face is replaced by
/// the plane x = -2e-5, parallel to the x = 0 face: found, the point, the largest gap.
fn scaled_parallel_corner(make_another: bool) -> (bool, [f64; 3], f64) {
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::math::{Point2, Vec3};
    use solidcore::nurbs::knots::Side;
    use solidcore::surface::{plane::Plane, surface::Surface};
    use solidcore::topology::{face::p2 as face, vertexuse};

    let (mut k, mut y, _) = scaled_box_history(false);
    let v = y
        .vertices()
        .ids()
        .find(|&v| y.vertex(v).unwrap().point.distance(Vec3::ZERO) == 0.0)
        .unwrap();
    let normal_of = |b: &solidcore::topology::Brep, f| {
        face::surface(b, f)
            .unwrap()
            .unwrap()
            .evaluate_normal(&k, Point2::new(0.0, 0.0), Side::Left, Side::Left)
            .unwrap()
    };
    let fz = vertex::faces(&y, v)
        .unwrap()
        .into_iter()
        .find(|&f| normal_of(&y, f).dot(Vec3::Z).abs() > 0.5)
        .unwrap();
    let frame = Axis2Placement::new(Vec3::new(-2e-5, 0.0, 0.0), Vec3::Z, Vec3::Y);
    y.face_mut(fz).unwrap().surface = Some(Surface::from(Plane::create_canonical(&frame).unwrap()));
    for vu in vertex::vertexuses(&y, v).unwrap() {
        vertexuse::init_vertex_face_gap(&mut y, vu).unwrap();
    }
    if make_another {
        another_box(&mut k);
    }
    let c = vertex::corner::calc_corner_from_faces(&k, &mut y, v).unwrap();
    let p = c.best_point;
    (c.found_point, [p.x, p.y, p.z], c.max_gap_3d)
}

/// The scaled box Y is measured by the healer's property pass, healed through its
/// tolerance step, united with Z, and searched for a corner between two faces 2e-5 apart;
/// each once straight after the scale and once after an unrelated box was made in
/// between. Making a box elsewhere does not change how Y is measured, so both histories
/// give the same answer. Before, making any body reset the kernel context to the
/// default, so Y was measured at its own 1e-4 straight after the scale and at the
/// default's 1e-5 after another body was made: there every edge, vertex and face of Y was
/// flagged as having too large a tolerance, and the corner search took the two faces as
/// parting (2e-5 against a threshold of 5e-6 in place of 5e-5) and moved the corner to
/// their midpoint.
#[test]
fn a_scaled_body_is_measured_the_same_whether_or_not_another_body_was_made() {
    use solidcore::heal::face_props::HealerOpType;

    let history = |make_another: bool| {
        let (k, mut y, _) = scaled_box_history(make_another);
        let flags = heal_property_flags(&k, &mut y);

        let (mut k, mut y, _) = scaled_box_history(make_another);
        p1::heal_brep(
            &mut k,
            &mut y,
            HealerOpType::FixTolsizes,
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let healed = (y.this_model_size_estimate(&k, false), body_tolerances(&y));

        let (mut k, y, z) = scaled_box_history(make_another);
        let mut union = solidcore::boolean::api_brep::boolean_union(&mut k, y, z).unwrap();
        let store = AttributeStore::new();
        let key_of = |_| ObjectKey(0);
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &key_of,
        };
        let volume = brep_volume(&k, &mut union, 1e-3, &attrs).unwrap();
        let union_faces = p1::faces(&union, None).unwrap().len();

        (
            flags,
            healed,
            volume,
            union_faces,
            scaled_parallel_corner(make_another),
        )
    };
    let straight_after = history(false);
    assert_eq!(history(true), straight_after);
}

/// The scaled box Y is measured at the zone tolerance of its own size (1e-4) after another
/// body was made, while the kernel context keeps the default (1e-5): the healer's property
/// pass flags nothing on it, its tolerance step leaves every tolerance at 1e-4, and the
/// corner search takes half of 1e-4 as its parallel-face threshold, so two faces 2e-5
/// apart are not taken as parting and the corner stays where the faces meet.
#[test]
fn a_scaled_body_is_measured_at_its_own_zone_tolerance() {
    use solidcore::geom::tol;
    use solidcore::heal::face_props::{HealerOpType, Tri};

    let (mut k, mut y, _) = scaled_box_history(true);
    assert_eq!(k.context.this_zone_tol_3d(), 1e-5);
    assert_eq!(tol::brep_size_zone_tol_3d(&k, &y), 1e-4);
    assert_eq!(body_tolerances(&y), vec![1e-4; 1 + 8 + 12]);
    let flags = heal_property_flags(&k, &mut y);
    assert_eq!(flags, vec![[Tri::False; 2]; 12 + 8 + 6]);

    p1::heal_brep(
        &mut k,
        &mut y,
        HealerOpType::FixTolsizes,
        None,
        None,
        None,
        false,
    )
    .unwrap();
    assert_eq!(body_tolerances(&y), vec![1e-4; 1 + 8 + 12]);

    assert_eq!(scaled_parallel_corner(true), (true, [0.0; 3], 0.0));
}
