//! #2048: `check_brep` of a body with a face on an `OffsetSurface` returned
//! `Unported { upstream: "SmFace handle of a surface's face", .. }`.
//! `SmOffsetSurface::AssertValid` test 2 runs `GlobalSurfaceIntersect` of the surface
//! with itself, and upstream's `smsurf_GetSurfaceCache` reaches the surface's face
//! through the surface (`GetFace()`); the port's face check now hands the face-aware
//! cache context to that solve.
//!
//! The input: a quarter cylinder (radius 10, length 20) as a rational B-spline face,
//! offset by 1 to a quarter cylinder of radius 11 on one `OffsetSurface` face, of area
//! `11 * pi / 2 * 20`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::nurbs::b_spline_surface::p1::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::offset::offset_executive::p2::{offset_brep_simple, OffsetOptions};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, FaceId};
use solidcore::{EntityRef, Kernel};

/// The external stock from SolidCraft #11776, reconstructed analytically here.
/// The uncommitted native driver behind #2613's twelve-case report is unavailable.
/// This is the continuous-flank member, not evidence for its historical segmentation.
fn m20_external_definition(k: &mut Kernel) -> Brep {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::curve::curve::Curve;
    use solidcore::math::{Point3, Vec3};
    use solidcore::primitive::api_curves::create_line_segment;
    use solidcore::primitive::api_primitives::create_cylinder;
    use solidcore::sweep::screw::screw_sweep;

    let base = Point3::new(-1.0, -64.0, 0.0);
    let pitch = 2.5;
    let root = 10.0 - 5.0 * pitch * 3.0_f64.sqrt() / 16.0;
    let outer = 10.0 + 0.05 * pitch;
    let half_root = pitch / 8.0;
    let half_outer = half_root + (outer - root) / 3.0_f64.sqrt();
    let points = [
        Point3::new(base.x + root, base.y, -pitch - half_root),
        Point3::new(base.x + outer, base.y, -pitch - half_outer),
        Point3::new(base.x + outer, base.y, -pitch + half_outer),
        Point3::new(base.x + root, base.y, -pitch + half_root),
    ];
    let profile: Vec<Curve> = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&a, &b)| Curve::from(create_line_segment(a, b).unwrap()))
        .collect();
    let stock = create_cylinder(k, base, 10.0, 20.0).unwrap();
    // Use the kernel's approximation budget (half the stock's zone tolerance),
    // leaving the rest for intersections and the exact definition's boundary checks.
    let tolerance = solidcore::geom::tol::brep_approx_tol_3d(
        k,
        &stock,
        solidcore::geom::tol::SmallTopoType::NormalSize,
    );
    // The cutter runs one pitch beyond each free end.
    let groove = screw_sweep(k, &profile, base, Vec3::Z, pitch, 10.0, tolerance).unwrap();
    boolean_difference(k, stock, groove).unwrap()
}

fn strict_closed_solid(k: &mut Kernel, body: &mut Brep) {
    assert!(solidcore::topology::brep::p2::is_manifold_solid(body).unwrap());
    let checked = check_brep(k, body, CheckLevel::Level2).unwrap();
    assert_eq!(checked.assert_valid, Some(true), "{:?}", checked.reports);
    assert_eq!(checked.failures().count(), 0, "{:?}", checked.reports);
}

fn binary_bytes(k: &Kernel, body: &Brep) -> Vec<u8> {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::topology::api_files::write_brep_to_file;
    use solidcore::topology::brep_data_write::FromBrepEnv;
    use solidcore::topology::topology::NotifyRecords;

    let attributes = AttributeStore::new();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &attributes,
        key_of: &|entity| body.owner_identity(entity),
        records: &records,
        curve_key: &|_| ObjectKey(0),
        uv_curve_key: &|_| ObjectKey(0),
        surface_key: &|_| ObjectKey(0),
    };
    write_brep_to_file(k, Some(body), &env, false).unwrap()
}

/// Opt-in artifacts use the caller's directory; ordinary test runs perform no I/O.
fn diagnostic_export(k: &Kernel, body: &Brep, stage: &str) {
    if let Some(directory) = std::env::var_os("SOLIDCORE_M20_OFFSET_EXPORT") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{stage}.smb")),
            binary_bytes(k, body),
        )
        .unwrap();
    }
}

/// Validity alone would also accept an unchanged shaft. Compare every retained
/// offset definition with the original wall at matching physical screw angles.
#[test]
fn continuous_m20_external_flanks_move_inward_on_their_definitions() {
    use solidcore::eval::geometry::sweep::SweepDefinitionRecord;
    use solidcore::math::Point2;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::offset::offset_faces::OffsetFacesExecutive;
    use solidcore::surface::procedural_sweep::ProceduralSweep;

    let mut k = Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..Kernel::default()
    };
    let mut body = m20_external_definition(&mut k);
    diagnostic_export(&k, &body, "before");
    strict_closed_solid(&mut k, &mut body);
    let picked: Vec<_> = body
        .faces()
        .ids()
        .filter(|&id| {
            body.face(id)
                .unwrap()
                .surface
                .as_ref()
                .unwrap()
                .procedural_sweep()
                .is_some()
        })
        .collect();
    assert_eq!(picked.len(), 2, "one continuous definition per flank");
    let originals: Vec<_> = picked
        .iter()
        .map(|&id| {
            let surface = body
                .face(id)
                .unwrap()
                .surface
                .as_ref()
                .unwrap()
                .copy(&k)
                .unwrap();
            let fu = solidcore::topology::face::p2::outer_faceuse(&body, id)
                .unwrap()
                .unwrap();
            let outward = if body.faceuse(fu).unwrap().orientation == OrientType::Same {
                1.0
            } else {
                -1.0
            };
            (surface, outward)
        })
        .collect();
    let distance = -0.1;
    OffsetFacesExecutive::new()
        .offset_faces(&mut k, &mut body, &picked, distance)
        .unwrap();
    diagnostic_export(&k, &body, "after");
    strict_closed_solid(&mut k, &mut body);

    let mut moved = 0;
    for (_, face) in body.faces().iter() {
        let surface = face.surface.as_ref().unwrap();
        let Some(output) = surface.procedural_sweep() else {
            continue;
        };
        if !output.definition().is_normal_offset() {
            continue;
        }
        moved += 1;
        let saved = output.definition().to_record(output.net()).unwrap();
        let SweepDefinitionRecord::Screw {
            offset: Some(offset),
            ..
        } = &saved.definition
        else {
            panic!("an offset wall must retain its exact source");
        };
        assert!((offset.signed_distance.abs() - distance.abs()).abs() < 1e-14);
        let (original, outward) = originals.iter().find(|(source, _)| {
            let source = source.procedural_sweep().unwrap();
            let record = source.definition().to_record(source.net()).unwrap();
            matches!(record.definition, SweepDefinitionRecord::Screw { profile, .. } if profile == offset.source)
        }).expect("each offset wall names one original flank");
        let original = original.procedural_sweep().unwrap();
        let input = original.definition();
        let result = output.definition();
        let restored = ProceduralSweep::from_record(&saved).unwrap();
        let reopened = restored.procedural_sweep().unwrap();
        assert!(output.bound().is_finite() && output.bound() > 0.0);
        assert!(output.bound() <= body.tolerance());
        let profile = input.profile().natural_interval();
        let first = input.stations()[0].max(result.stations()[0]);
        let last = input
            .stations()
            .last()
            .unwrap()
            .min(*result.stations().last().unwrap());
        assert!(last > first);
        // Irrational fractions avoid the net's regular station/profile grids.
        for fraction in [0.211_324_865_405_187, 0.618_033_988_749_895] {
            let angle = first + fraction * (last - first);
            let w = profile.min() + fraction * (profile.max() - profile.min());
            let before_uv = input
                .face_parameters(Point2::new(w, input.parameter_at_angle(angle).unwrap()))
                .unwrap();
            let after_uv = result
                .face_parameters(Point2::new(w, result.parameter_at_angle(angle).unwrap()))
                .unwrap();
            let jet = input.evaluate_definition(before_uv, 1, 1).unwrap();
            let normal = jet[1][0].cross(jet[0][1]);
            let normal = normal * (*outward / normal.length());
            let point = result.evaluate_definition_point(after_uv).unwrap();
            let delta = point - jet[0][0];
            let rounding = 2e-12 * (1.0 + point.length());
            assert!(
                (delta - normal * distance).length() <= rounding,
                "signed movement {delta:?}"
            );
            assert!((delta.dot(normal) - distance).abs() <= rounding);
            assert!(
                (output.net().evaluate_point(after_uv).unwrap() - point).length() <= output.bound()
            );
            assert!(
                (reopened
                    .definition()
                    .evaluate_definition_point(after_uv)
                    .unwrap()
                    - point)
                    .length()
                    <= rounding
            );
        }
    }
    assert_eq!(
        moved,
        picked.len(),
        "a successful offset must move both flanks"
    );
    // Whole-body definition persistence belongs to the native record converter:
    // `an_offset_m20_body_round_trips_its_screw_definitions` checks encoding,
    // decoding, strict reopened-body validity and exact offset witnesses there.
    // The legacy SMB writer serializes the spline surface, not this definition.
}

/// The issue's quarter cylinder: u degree 2 (rational quarter circle), v degree 1.
fn quarter_cylinder(k: &mut Kernel) -> Brep {
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let pw: Vec<Vec<CPoint>> = [(10.0, 0.0, 1.0), (10.0, 10.0, c), (0.0, 10.0, 1.0)]
        .iter()
        .map(|&(x, y, w)| {
            vec![
                CPoint::new(x * w, y * w, 0.0, w),
                CPoint::new(x * w, y * w, 20.0 * w, w),
            ]
        })
        .collect();
    let s = Surface::from(
        BSplineSurface::from_data(
            2,
            1,
            2,
            1,
            5,
            3,
            &pw,
            &[0., 0., 0., 1., 1., 1.],
            &[0., 0., 1., 1.],
        )
        .unwrap(),
    );
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
    {
        let this_face = |f: FaceId| -> ObjectRef { notify_ref(EntityRef::Face(f)) };
        let mut state = ArenaNotifyState::new(&b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        let mut fenv = FaceBuildEnv {
            env: &mut env,
            marks: &mut marks,
            this_face: &this_face,
        };
        create_face_from_surface(k, &mut b, Some(s), &d, &mut fenv).unwrap();
    }
    b.refresh_cached_topology();
    b
}

#[test]
fn check_brep_answers_on_an_offset_surface_face() {
    let mut k = Kernel::default();
    let mut b = quarter_cylinder(&mut k);
    assert_eq!(
        check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .assert_valid,
        Some(true)
    );

    let mut r = offset_brep_simple(
        &mut k,
        &mut b,
        OffsetOptions {
            distance: 1.0,
            extend_convex_edges: true,
            merge_results: true,
        },
    )
    .unwrap();
    r.refresh_cached_topology();

    let faces: Vec<FaceId> = r.faces().ids().collect();
    assert_eq!(faces.len(), 1);
    let surface = r.face(faces[0]).unwrap().surface.as_ref().unwrap();
    assert!(matches!(surface.kind(), SurfaceKind::OffsetSurface(_)));
    let area = brep_area(&k, &mut r, 1e-6).unwrap();
    let expected = 11.0 * std::f64::consts::FRAC_PI_2 * 20.0;
    assert!((area - expected).abs() < 1e-3, "area {area} vs {expected}");

    let c = check_brep(&mut k, &mut r, CheckLevel::Level2).expect("check_brep answers");
    assert_eq!(c.assert_valid, Some(true), "{:?}", c.reports);
    assert_eq!(c.failures().count(), 0);
}
