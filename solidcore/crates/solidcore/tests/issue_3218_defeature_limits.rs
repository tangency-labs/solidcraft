//! #3218: a defeature that keeps a feature on its base names why its patch is not built
//! ([`DefeatureFailure`], carried by the refusal's [`Reason::Defeature`] at the open loop's
//! edge, and by each [`KeptFeature::limit`]). Each limit is decided where the patch's
//! construction refuses, from what it found there:
//!
//! - `LoopNotPlanar`: a planar cap asked for a cylinder fillet's loop (two lines and two arcs
//!   on the cylinder), which no plane holds;
//! - `ExtensionRefused`: the faces along the loop asked to be extended by 0;
//! - `TangentNeighbours`: the two shoulders of a rib extend as far as each other,
//!   sharing a smooth edge while their far edges remain off the loop;
//! - `NotEnclosed`: a box face, whose neighbours, extended, run past it;
//! - `ExtensionWithinTolerance`: the faces along a fillet's loop extended by less than the
//!   distance within which the loop's vertices and the patch's are one point.
//!
//! The rib is swept from its profile. The annular groove cases revolve dimensioned
//! profiles. The file-based cases use the pinned upstream checkout when present.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::feature_executive::{FeatureExecutive, KeptFeature, RebuildBrepType};
use solidcore::error::defeature::DefeatureFailure;
use solidcore::error::{Reason, SM_ERR};
use solidcore::geom::attribute::AttributeStore;
use solidcore::mass::enclosure::enclose_volume;
use solidcore::math::det;
use solidcore::surface::implicit::AnalyticSupport;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::FaceId;
use solidcore::{Brep, EntityRef, Kernel, SolidError};

/// An upstream test file, read and arena-named as the goldens reader does.
fn read(k: &mut Kernel, file: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(file);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// Face `face` of `file` defeatured alone with base rebuild `base_rb` (and the base extension
/// distance `ext`, when given): the refusal's limit, checked to be at an edge with the
/// status `SM_ERR`, and the base unchanged.
fn limit(
    file: &str,
    face: usize,
    base_rb: RebuildBrepType,
    ext: Option<f64>,
) -> Option<DefeatureFailure> {
    let mut k = Kernel::default();
    let mut b = read(&mut k, file)?;
    let faces = bp1::faces(&b, None).unwrap();
    Some(limit_on_faces(&mut k, &mut b, &[faces[face]], base_rb, ext))
}

fn limit_on_faces(
    k: &mut Kernel,
    b: &mut Brep,
    removed: &[FaceId],
    base_rb: RebuildBrepType,
    ext: Option<f64>,
) -> DefeatureFailure {
    let before = bp1::faces(b, None).unwrap();
    let error = {
        let mut ex = FeatureExecutive::new(k, b).unwrap();
        ex.add_feature(k, removed, base_rb, RebuildBrepType::ExtendSurface)
            .unwrap()
            .unwrap();
        if let Some(d) = ext {
            ex.set_base_ext_dist(d);
        }
        ex.do_defeature(k).unwrap_err()
    };
    assert_eq!(bp1::faces(b, None).unwrap(), before, "base changed");
    assert!(
        matches!(error.entity(), Some(EntityRef::Edge(_))),
        "{error:?}"
    );
    assert_eq!(error.sm_status(), Some(SM_ERR), "{error:?}");
    match error.failure().map(|f| &f.reason) {
        Some(Reason::Defeature(limit)) => *limit,
        _ => panic!("expected a refusal naming its limit, got {error:?}"),
    }
}

#[test]
fn a_planar_cap_on_a_cylinder_fillet_s_loop_is_refused_as_not_planar() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::PlanarCap,
        None,
    ) else {
        return;
    };
    assert_eq!(l, DefeatureFailure::LoopNotPlanar);
}

/// The not-planar refusal keeps, as its cause, the error the base patch's construction failed
/// with: the construction's own `LoopNotPlanar` refusal, which in turn keeps the planar cap's.
#[test]
fn a_not_planar_refusal_keeps_its_construction_failure() {
    let mut k = Kernel::default();
    let Some(mut b) = read(&mut k, "box_with_large_fillet.smb") else {
        return;
    };
    let faces = bp1::faces(&b, None).unwrap();
    let error = {
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        ex.add_feature(
            &mut k,
            &[faces[6]],
            RebuildBrepType::PlanarCap,
            RebuildBrepType::ExtendSurface,
        )
        .unwrap()
        .unwrap();
        ex.do_defeature(&mut k).unwrap_err()
    };
    assert!(
        matches!(
            error.failure().map(|f| &f.reason),
            Some(Reason::Defeature(DefeatureFailure::LoopNotPlanar))
        ),
        "{error:?}"
    );
    let cause = error.cause().expect("the construction's error is kept");
    assert!(
        matches!(
            cause.failure().map(|f| &f.reason),
            Some(Reason::Defeature(DefeatureFailure::LoopNotPlanar))
        ),
        "{cause:?}"
    );
    assert!(cause.cause().is_some(), "{cause:?}");
}

#[test]
fn an_extension_by_nothing_is_refused_as_the_extension_s() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::ExtendSurface,
        Some(0.0),
    ) else {
        return;
    };
    assert_eq!(l, DefeatureFailure::ExtensionRefused { distance: 0.0 });
}

/// `fillet1019` defeatured as the goldens' `blend.feature_executive` rows run it (the base
/// rebuilt by extending its surfaces, the feature from the other B-rep), one feature per
/// list of face indices in `faces`, added in turn: `made` says which make a feature. No
/// feature kept. Returns the base and the feature B-reps.
fn defeatured(
    k: &mut Kernel,
    faces: &[&[usize]],
    made: &[bool],
) -> Option<(Brep, Vec<Option<Brep>>)> {
    let mut b = read(k, "Fillet/fillet1019.smb")?;
    let all = bp1::faces(&b, None).unwrap();
    let features = {
        let mut ex = FeatureExecutive::new(k, &mut b).unwrap();
        let mut got = Vec::new();
        for set in faces {
            let fs: Vec<_> = set.iter().map(|&i| all[i]).collect();
            let f = ex
                .add_feature(
                    k,
                    &fs,
                    RebuildBrepType::ExtendSurface,
                    RebuildBrepType::FromOtherBrep,
                )
                .unwrap();
            got.push(f.is_some());
        }
        assert_eq!(got, made, "{faces:?}");
        let kept = ex.do_defeature(k).unwrap();
        assert!(kept.is_empty(), "{faces:?}: {kept:?}");
        ex.into_feature_breps()
    };
    Some((b, features))
}

/// #3218 first asked that `fillet1019`'s bottom fillet (face 3), which ends on the corner
/// round (face 6), be refused as [`DefeatureFailure::TangentNeighbours`]: the patch's piece
/// on the x- wall seemed cut from the round's extension only along an edge proved smooth,
/// so nothing bounded it. That reading came from #15247's misclassification: the boolean
/// took a point outside the round's extension's hole for outside that face, which left the
/// patch open. Classified correctly, the patch closes and the fillet comes off, so this
/// test now proves the bodies it builds are the right ones, with the inputs of the goldens'
/// `single_face_fillet1019_3` row: the base ([`certify`]: the frustum with the top fillet and
/// the corner round, the round carried down to the bottom by its own cylinder) and the
/// feature B-rep ([`certify_feature`]: the fillet's gap).
#[test]
fn a_fillet_ending_on_a_tangent_fillet_comes_off_to_the_rolling_ball_body() {
    let mut k = Kernel::default();
    let Some((mut b, features)) = defeatured(&mut k, &[&[3]], &[true]) else {
        return;
    };
    certify(&mut k, &mut b, Removed::Bottom);
    certify_feature(&mut k, features, Removed::Bottom);
}

/// The top fillet (face 8), which ends on the corner round at the top, comes off the same
/// way (the goldens' `single_face_fillet1019_8`): the frustum with the bottom fillet and the
/// corner round ([`certify`]), and the fillet's gap as the feature B-rep
/// ([`certify_feature`]).
#[test]
fn a_top_fillet_ending_on_a_tangent_fillet_comes_off_to_the_rolling_ball_body() {
    let mut k = Kernel::default();
    let Some((mut b, features)) = defeatured(&mut k, &[&[8]], &[true]) else {
        return;
    };
    certify(&mut k, &mut b, Removed::Top);
    certify_feature(&mut k, features, Removed::Top);
}

/// One feature per fillet face (the goldens' `fillet_each_fillet1019`): only the bottom
/// fillet makes a feature (the round and the top fillet, which touch it, make none), so the
/// bodies are the bottom fillet's ([`certify`], [`certify_feature`]).
#[test]
fn a_feature_per_fillet_of_fillet1019_takes_off_the_bottom_fillet() {
    let mut k = Kernel::default();
    let Some((mut b, features)) = defeatured(&mut k, &[&[3], &[6], &[8]], &[true, false, false])
    else {
        return;
    };
    certify(&mut k, &mut b, Removed::Bottom);
    certify_feature(&mut k, features, Removed::Bottom);
}

/// The three rounds as one feature, the base rebuilt by extending its surfaces and the
/// feature from the other B-rep (the goldens' `fillet_group_fillet1019_rb23`): the base is
/// the plain frustum ([`certify`]: on the bottom, the top and the four walls only, holding
/// the frustum's volume) and the feature B-rep the three rounds' gaps as one solid
/// ([`certify_feature`]).
#[test]
fn the_three_rounds_of_fillet1019_as_one_feature_leave_the_frustum() {
    let mut k = Kernel::default();
    let Some((mut b, features)) = defeatured(&mut k, &[&[3, 6, 8]], &[true]) else {
        return;
    };
    certify(&mut k, &mut b, Removed::All);
    certify_feature(&mut k, features, Removed::All);
}

/// The trimmed faces containing a point, including their boundaries.
fn faces_at(k: &Kernel, b: &mut Brep, p: solidcore::Vec3) -> Vec<FaceId> {
    use solidcore::query::face::p1::point_on_face;

    bp1::faces(b, None)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            point_on_face(k, b, f, p, k.tolerances.zone_tol_3d)
                .unwrap()
                .is_some()
        })
        .collect()
}

/// Registers the single face containing the point for removal.
fn feature_at<'a>(k: &mut Kernel, b: &'a mut Brep, p: solidcore::Vec3) -> FeatureExecutive<'a> {
    let picked = faces_at(k, b, p);
    assert_eq!(picked.len(), 1);
    let mut ex = FeatureExecutive::new(k, b).unwrap();
    ex.add_feature(
        k,
        &picked,
        RebuildBrepType::ExtendSurface,
        RebuildBrepType::FromOtherBrep,
    )
    .unwrap()
    .unwrap();
    ex
}

/// The native B-rep, including the topology, trims and tolerances.
fn native_body(k: &Kernel, b: &Brep) -> Vec<u8> {
    use solidcore::geom::attribute::ObjectKey;
    use solidcore::topology::api_files::write_brep_to_file;
    use solidcore::topology::brep_data_write::FromBrepEnv;

    let env = FromBrepEnv {
        attributes: &AttributeStore::default(),
        records: &NotifyRecords::new(),
        key_of: &solidcore::topology::loopuse::notify_ref,
        curve_key: &|_| ObjectKey(0),
        uv_curve_key: &|_| ObjectKey(0),
        surface_key: &|_| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, false).unwrap()
}

/// A plate and ball joined by a conical bevel from their top and equator.
fn beveled_ball_on_plate(k: &mut Kernel) -> Brep {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::primitive::api_primitives::{create_cone, create_cylinder, create_sphere};
    use solidcore::Vec3;

    let plate = create_cylinder(k, Vec3::new(0.0, 0.0, -2.0), 8.0, 2.0).unwrap();
    let ball = create_sphere(k, Vec3::new(0.0, 0.0, 1.0), 3.0).unwrap();
    let bevel = create_cone(k, Vec3::ZERO, 4.0, 3.0, 1.0).unwrap();
    let sharp = boolean_union(k, plate, ball).unwrap();
    boolean_union(k, sharp, bevel).unwrap()
}

/// The bevel joins two disjoint rims whose extended supports meet on one circle.
/// A valid closed shell can still carry the buried part of the sphere and the
/// plate's interior disc. Test the actual trim and the material on both sides.
#[test]
fn the_bevel_between_a_ball_and_a_plate_leaves_one_sharp_circle() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::kernel::UpstreamFixes;
    use solidcore::mass::integrator::properties::{brep_area, brep_volume_default};
    use solidcore::primitive::api_primitives::create_sphere;
    use solidcore::query::api_queries::find_edge;
    use solidcore::topology::brep::is_outward_solid;
    use solidcore::topology::edge;
    use solidcore::Vec3;
    use std::f64::consts::PI;

    // Both cavities stay below the restored top; the closer one clears it by 0.005.
    for (radius, z) in [(0.0_f64, -1.0), (0.1, -1.0), (0.1, -0.105)] {
        let mut k = Kernel {
            fixes: UpstreamFixes::all(),
            ..Kernel::default()
        };
        let mut b = beveled_ball_on_plate(&mut k);
        if radius > 0.0 {
            let cavity = create_sphere(&mut k, Vec3::new(3.4, 0.0, z), radius).unwrap();
            b = boolean_difference(&mut k, b, cavity).unwrap();
        }
        let mut ex = feature_at(&mut k, &mut b, Vec3::new(0.0, 3.5, 0.5));
        assert!(ex.do_defeature(&mut k).unwrap().is_empty());
        let first = native_body(&k, ex.base_brep());
        let repeated = ex.do_defeature(&mut k);
        assert!(
            repeated.as_ref().is_ok_and(Vec::is_empty),
            "repeated with cavity radius {radius} at z = {z}: {repeated:?}"
        );
        assert_eq!(ex.features().len(), 1);
        assert!(
            first == native_body(&k, ex.base_brep()),
            "the repeated edit changed the base"
        );
        let mut removed = ex.into_feature_breps().pop().unwrap().unwrap();

        // Plate plus the spherical cap of height 4, less any cavity. The removed ring is the
        // conical frustum less the sphere's zone between z = 0 and z = 1.
        for (body, volume, area) in [
            (
                &mut b,
                (464.0 - 4.0 * det::powi(radius, 3)) * PI / 3.0,
                (176.0 + 4.0 * det::powi(radius, 2)) * PI,
            ),
            (
                &mut removed,
                11.0 * PI / 3.0,
                (14.0 + 7.0 * 2.0_f64.sqrt()) * PI,
            ),
        ] {
            let check =
                validation::check_brep(&mut k, body, validation::CheckLevel::Level2).unwrap();
            assert!(check.is_valid(), "{check:?}");
            assert!(is_outward_solid(&mut k, body).unwrap());
            let v = brep_volume_default(&k, body, 1e-10).unwrap();
            let a = brep_area(&k, body, 1e-10).unwrap();
            assert!(
                (v - volume).abs() <= 1e-8 * volume,
                "volume {v}, expected {volume}"
            );
            assert!((a - area).abs() <= 1e-8 * area, "area {a}, expected {area}");
        }

        // Both points satisfy x² + y² = 8 and lie away from the primitive seams.
        for p in [Vec3::new(2.0, 2.0, 0.0), Vec3::new(-2.0, -2.0, 0.0)] {
            let faces = faces_at(&k, &mut b, p);
            assert_eq!(faces.len(), 2, "the plate and ball at {p:?}");
            let e = find_edge(&k, &b, p).unwrap();
            let incident = edge::faces(&b, bp1::edges(&b, None).unwrap()[e.index], None).unwrap();
            assert!(
                faces.iter().all(|f| incident.contains(f)),
                "the two trims share an edge at {p:?}"
            );
        }
        assert!(
            faces_at(&k, &mut b, Vec3::new(0.5, 0.5, 0.0)).is_empty(),
            "no buried disc"
        );
        assert!(
            faces_at(&k, &mut b, Vec3::new(0.0, 5.0_f64.sqrt(), -1.0)).is_empty(),
            "no buried spherical cap"
        );
    }
}

/// A small cavity touches the new plate at one point when its centre is at
/// z = -0.1, crosses it at z = 0, and lies wholly in the bevel at z = 0.25.
#[test]
fn a_cavity_in_the_removed_bevel_keeps_the_original_body() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::kernel::UpstreamFixes;
    use solidcore::primitive::api_primitives::create_sphere;
    use solidcore::Vec3;

    for (z, turn_sphere) in [(-0.1, false), (-0.1, true), (0.0, false), (0.25, false)] {
        let mut k = Kernel {
            fixes: UpstreamFixes::all(),
            ..Kernel::default()
        };
        let mut input = beveled_ball_on_plate(&mut k);
        let mut sphere = if turn_sphere {
            // Same cavity, with the tangency in the face interior instead of at a pole vertex.
            let mut s = create_sphere(&mut k, Vec3::ZERO, 0.1).unwrap();
            let placement = [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, -1.0, 0.0, 0.0],
                [3.4, 0.0, z, 1.0],
            ];
            solidcore::topology::brep::mirror::transform_matrix(&mut k, &mut s, &placement, true)
                .unwrap();
            s
        } else {
            create_sphere(&mut k, Vec3::new(3.4, 0.0, z), 0.1).unwrap()
        };
        if z < 0.0 {
            // The old outer boundary clears the spherical cavity before a new
            // plane is extended to its top point (3.4, 0, 0).
            let clearance =
                solidcore::query::contact::boundary_distance(&k, &mut input, &mut sphere, 0.2)
                    .unwrap();
            assert!(clearance.lower > 0.1, "original clearance {clearance:?}");
        }
        let mut b = boolean_difference(&mut k, input, sphere).unwrap();
        let obstacle = faces_at(&k, &mut b, Vec3::new(3.4, 0.1, z));
        assert_eq!(obstacle.len(), 1);
        if z < 0.0 {
            assert_eq!(faces_at(&k, &mut b, Vec3::new(3.4, 0.0, 0.0)), obstacle);
        }
        let mut ex = feature_at(&mut k, &mut b, Vec3::new(0.0, 3.5, 0.5));
        let before = native_body(&k, ex.base_brep());
        let error = ex
            .do_defeature(&mut k)
            .expect_err("the cavity prevents this patch");
        let face = obstacle[0];
        let expected = if z > 0.0 {
            DefeatureFailure::PatchEnclosesFace { face }
        } else {
            DefeatureFailure::PatchIntersectsFace { face }
        };
        assert!(
            matches!(error, SolidError::Failed(ref f) if f.reason == Reason::Defeature(expected)),
            "at z = {z}, sphere turned = {turn_sphere}: expected {expected:?}, got {error:?}"
        );
        assert!(ex.features().is_empty(), "a refused feature was published");
        let after = native_body(&k, ex.base_brep());
        assert!(
            before == after,
            "the native B-rep changed on refusal at z = {z}"
        );
    }
}

/// The two rims of a through hole lie on distinct, parallel planes. Each cap
/// fills its own rim. The healed solid is the entire 40 × 30 × 5 plate.
#[test]
fn the_two_caps_of_a_through_hole_fill_the_plate_separately() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::kernel::UpstreamFixes;
    use solidcore::mass::integrator::properties::{brep_area, brep_volume_default};
    use solidcore::primitive::api_primitives::{create_box, create_cylinder};
    use solidcore::topology::brep::is_outward_solid;
    use solidcore::Vec3;

    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let plate = create_box(&mut k, Vec3::ZERO, 40.0, 30.0, 5.0).unwrap();
    let cutter = create_cylinder(&mut k, Vec3::new(20.0, 15.0, -1.0), 5.0, 7.0).unwrap();
    let mut b = boolean_difference(&mut k, plate, cutter).unwrap();
    let wall = Vec3::new(20.0, 20.0, 2.5);
    {
        let mut ex = feature_at(&mut k, &mut b, wall);
        assert!(ex.do_defeature(&mut k).unwrap().is_empty());
    }

    let check = validation::check_brep(&mut k, &mut b, validation::CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{check:?}");
    assert!(is_outward_solid(&mut k, &mut b).unwrap());
    assert!(
        faces_at(&k, &mut b, wall).is_empty(),
        "the cylindrical wall was removed"
    );
    let top = faces_at(&k, &mut b, Vec3::new(20.0, 15.0, 5.0));
    let bottom = faces_at(&k, &mut b, Vec3::new(20.0, 15.0, 0.0));
    assert_eq!(top.len(), 1);
    assert_eq!(bottom.len(), 1);
    assert_ne!(top, bottom, "the top and bottom have separate disks");

    let v = brep_volume_default(&k, &mut b, 1e-10).unwrap();
    let a = brep_area(&k, &mut b, 1e-10).unwrap();
    assert!((v - 6000.0).abs() <= 1e-8 * 6000.0, "volume {v}");
    assert!((a - 3100.0).abs() <= 1e-8 * 3100.0, "area {a}");
}

#[test]
fn a_box_face_is_refused_as_not_enclosed() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        0,
        RebuildBrepType::ExtendSurface,
        None,
    ) else {
        return;
    };
    assert!(matches!(l, DefeatureFailure::NotEnclosed { .. }), "{l:?}");
}

/// Removing the crown and flanks of a 2-wide rib leaves its raised outline on
/// both end faces. The shoulders at y = 3 leave a gap if extended by 0.5 each.
/// At 1 they meet at x = 0 in the same plane, but their free edges run past the
/// ends of the rib and cannot close that outline.
#[test]
fn rib_shoulders_that_just_meet_leave_the_patch_open_at_a_tangent_edge() {
    use solidcore::curve::curve::Curve;
    use solidcore::math::{Point3, Vec3};
    use solidcore::primitive::api_curves::create_line_segment;
    use solidcore::query::face::p1::calculate_bounding_box;
    use solidcore::sweep::api_brep::create_linear_sweep;

    let mut k = Kernel::default();
    let profile = [
        (-5.0, 0.0),
        (5.0, 0.0),
        (5.0, 3.0),
        (1.0, 3.0),
        (1.0, 4.0),
        (-1.0, 4.0),
        (-1.0, 3.0),
        (-5.0, 3.0),
        (-5.0, 0.0),
    ]
    .map(|(x, y)| Point3::new(x, y, 0.0));
    let outline: Vec<_> = profile
        .windows(2)
        .map(|ends| Curve::from(create_line_segment(ends[0], ends[1]).unwrap()))
        .collect();
    let curves: Vec<_> = outline.iter().map(Some).collect();
    let mut body = create_linear_sweep(&mut k, &curves, Vec3::Z, 10.0, true).unwrap();
    // Only the crown and the two flanks have their midpoint above the shoulders.
    let rib: Vec<_> = bp1::faces(&body, None)
        .unwrap()
        .into_iter()
        .filter(|&f| calculate_bounding_box(&k, &body, f, false).unwrap().mid().y > 3.25)
        .collect();
    assert_eq!(rib.len(), 3);

    let extend = RebuildBrepType::ExtendSurface;
    assert_eq!(
        limit_on_faces(&mut k, &mut body, &rib, extend, Some(0.5)),
        DefeatureFailure::NotEnclosed {
            extended_by: Some(0.5)
        }
    );
    assert_eq!(
        limit_on_faces(&mut k, &mut body, &rib, extend, Some(1.0)),
        DefeatureFailure::TangentNeighbours {
            within_deg: k.tolerances.ang_tol_deg
        }
    );
}

/// `box_with_large_fillet`'s fillet (face 6), its faces extended by 0.001: the loop's
/// vertices carry tolerances of 2.374e-3, so a vertex of the extended boundary and the
/// loop's vertex are one point within their summed zone tolerances and the 0.001 extension
/// is no feature the kernel can resolve (the merged vertex's tolerance covers it). The
/// refusal names that identity radius, from the loop vertices' tolerances and the patch's:
/// a lower bound on the extension, not a size that builds a valid body (see the bracket test
/// below and #16457).
#[test]
fn an_extension_within_the_loop_s_tolerance_is_refused_naming_the_identity_radius() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::ExtendSurface,
        Some(0.001),
    ) else {
        return;
    };
    let DefeatureFailure::ExtensionWithinTolerance {
        extended_by,
        identity_radius,
    } = l
    else {
        panic!("a refusal naming the identity radius, got {l:?}");
    };
    assert_eq!(extended_by, 0.001);
    let mut k = Kernel::default();
    let mut b = read(&mut k, "box_with_large_fillet.smb").unwrap();
    let widest = bp1::vertices(&b, None)
        .unwrap()
        .into_iter()
        .map(|v| b.vertex(v).unwrap().tolerance)
        .fold(0.0_f64, f64::max);
    let patch = b.tolerance();
    assert!(
        identity_radius > widest && identity_radius <= (widest + patch) * (1.0 + 1e-12),
        "radius {identity_radius}, loop vertex tolerance {widest}, patch {patch}"
    );
    let faces = bp1::faces(&b, None).unwrap();
    let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
    ex.add_feature(
        &mut k,
        &[faces[6]],
        RebuildBrepType::ExtendSurface,
        RebuildBrepType::ExtendSurface,
    )
    .unwrap()
    .unwrap();
    ex.set_base_ext_dist(1.01 * identity_radius);
    let r = ex.do_defeature(&mut k);
    assert!(r.is_ok(), "extended by 1.01 x {identity_radius}: {r:?}");
}

/// The same fillet bracketed at its identity radius: extended by 0.9999 x the radius the
/// patch still fails and is refused naming the radius; extended by 1.0001 x it builds. On
/// this model the construction's own failure ends at the radius to within 1e-4 (measured:
/// it fails at 2.3843e-3 and builds at 2.3845e-3, the radius being 2.38440e-3), so the
/// refusal is named over no extension that would have built. The body built past it is not
/// valid: Level 2 reports 6 x `SmBrep` test 61 (an edge fails `ValidateGeometry`), as main's
/// body does at every extension from 0.001 to 0.01 (#16457). The radius is a lower bound on
/// the extension, never a size that fits; this asserts the defect as it is, so a fix shows.
#[test]
fn the_identity_radius_brackets_where_the_patch_starts_to_build() {
    // (no upstream test file: nothing to check)
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::ExtendSurface,
        Some(0.001),
    ) else {
        return;
    };
    let DefeatureFailure::ExtensionWithinTolerance {
        identity_radius, ..
    } = l
    else {
        panic!("a refusal naming the identity radius, got {l:?}");
    };
    let below = 0.9999 * identity_radius;
    let l = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::ExtendSurface,
        Some(below),
    );
    assert_eq!(
        l,
        Some(DefeatureFailure::ExtensionWithinTolerance {
            extended_by: below,
            identity_radius,
        })
    );
    let mut k = Kernel::default();
    let mut b = read(&mut k, "box_with_large_fillet.smb").unwrap();
    let faces = bp1::faces(&b, None).unwrap();
    let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
    ex.add_feature(
        &mut k,
        &[faces[6]],
        RebuildBrepType::ExtendSurface,
        RebuildBrepType::ExtendSurface,
    )
    .unwrap()
    .unwrap();
    ex.set_base_ext_dist(1.0001 * identity_radius);
    let r = ex.do_defeature(&mut k);
    assert!(r.is_ok(), "extended by 1.0001 x {identity_radius}: {r:?}");
    drop(ex);
    let check = validation::check_brep(&mut k, &mut b, validation::CheckLevel::Level2).unwrap();
    let lines = check.lines();
    let invalid_edges = lines.iter().filter(|l| l.contains("Test #61")).count();
    assert_eq!(
        invalid_edges, 6,
        "#16457: the body past the radius fails ValidateGeometry on 6 edges: {lines:?}"
    );
}

/// A kept feature carries its limit too: of the two fillets of `cyl_on_box_w_fillet` one
/// comes off and the other is kept, naming why.
#[test]
fn a_kept_feature_names_its_limit() {
    let mut k = Kernel::default();
    let Some(mut b) = read(&mut k, "Fillet/cyl_on_box_w_fillet.smb") else {
        return;
    };
    let faces = bp1::faces(&b, None).unwrap();
    let kept: Vec<KeptFeature> = {
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        for f in [3, 8] {
            ex.add_feature(
                &mut k,
                &[faces[f]],
                RebuildBrepType::ExtendSurface,
                RebuildBrepType::ExtendSurface,
            )
            .unwrap()
            .unwrap();
        }
        ex.do_defeature(&mut k).unwrap()
    };
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].limit.is_some(), "{kept:?}");
}

// ---- the certificate for fillet1019 (#15247 / #3218) ------------------------------------

/// An interval `[lo, hi]` with outward rounding: each operation's result is widened by one
/// ulp each way, so it holds the exact result of the operation on any reals in its operands
/// (the IEEE operations `+ - * / sqrt` are correctly rounded).
#[derive(Clone, Copy, Debug)]
struct I {
    lo: f64,
    hi: f64,
}

fn up(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        x
    } else if x == 0.0 {
        f64::from_bits(1)
    } else if x > 0.0 {
        f64::from_bits(x.to_bits() + 1)
    } else {
        f64::from_bits(x.to_bits() - 1)
    }
}

fn down(x: f64) -> f64 {
    -up(-x)
}

impl I {
    fn p(x: f64) -> Self {
        Self { lo: x, hi: x }
    }
    fn new(lo: f64, hi: f64) -> Self {
        assert!(lo <= hi, "{lo} > {hi}");
        Self { lo, hi }
    }
    fn width(self) -> f64 {
        self.hi - self.lo
    }
    fn sqr(self) -> Self {
        let m = self * self;
        if self.lo <= 0.0 && self.hi >= 0.0 {
            Self::new(0.0, m.hi)
        } else {
            m
        }
    }
    /// The square root of the part of `self` at or above 0: used where the exact value is
    /// known not to be negative (a discriminant of a line meeting a circle it is tangent to
    /// at worst), so a negative lower end is rounding.
    fn sqrt0(self) -> Self {
        assert!(self.hi >= 0.0, "{self:?}");
        Self::new(down(self.lo.max(0.0).sqrt()).max(0.0), up(self.hi.sqrt()))
    }
    fn max(self, o: Self) -> Self {
        Self::new(self.lo.max(o.lo), self.hi.max(o.hi))
    }
    fn min(self, o: Self) -> Self {
        Self::new(self.lo.min(o.lo), self.hi.min(o.hi))
    }
    fn contains(self, o: Self) -> bool {
        self.lo <= o.lo && o.hi <= self.hi
    }
    /// The measure of `[a, b]` (`0` when empty), for `a`, `b` known to their intervals.
    fn seg(a: Self, b: Self) -> Self {
        Self::new(down(b.lo - a.hi).max(0.0), up(b.hi - a.lo).max(0.0))
    }
}

impl core::ops::Add for I {
    type Output = I;
    fn add(self, o: I) -> I {
        I::new(down(self.lo + o.lo), up(self.hi + o.hi))
    }
}
impl core::ops::Sub for I {
    type Output = I;
    fn sub(self, o: I) -> I {
        I::new(down(self.lo - o.hi), up(self.hi - o.lo))
    }
}
impl core::ops::Neg for I {
    type Output = I;
    fn neg(self) -> I {
        I::new(-self.hi, -self.lo)
    }
}
impl core::ops::Mul for I {
    type Output = I;
    fn mul(self, o: I) -> I {
        let p = [
            self.lo * o.lo,
            self.lo * o.hi,
            self.hi * o.lo,
            self.hi * o.hi,
        ];
        let lo = p.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = p.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        I::new(down(lo), up(hi))
    }
}
impl core::ops::Div for I {
    type Output = I;
    fn div(self, o: I) -> I {
        assert!(o.lo > 0.0 || o.hi < 0.0, "division by {o:?}");
        let p = [
            self.lo / o.lo,
            self.lo / o.hi,
            self.hi / o.lo,
            self.hi / o.hi,
        ];
        let lo = p.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = p.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        I::new(down(lo), up(hi))
    }
}

/// `fillet1019` as a model: the frustum of the four walls drafted in by `t` from the 2W
/// square at z = 0 to height H, its x- wall's bottom and top edges and its (x-, y-) corner
/// rounded by rolling balls of radius r. Each round's gap (between its two planes and the
/// ball's cylinder) is a prism; R3 (bottom), R8 (top) run along y between the y walls, R6
/// along the corner's line (t, t, 1).
struct Model {
    w: I,
    h: I,
    t: I,
    r: I,
    s: I,  // sqrt(1 + t^2)
    d2: I, // 1 + 2 t^2
    cx3: I,
    cx8: I,
    cz8: I,
    ax: I, // the corner axis at z = 0: (ax, ax, 0)
    g: I,  // n5 . n1 = t^2 / s^2
}

impl Model {
    fn new(w: f64, h: f64, top_half: f64, r: f64) -> Self {
        let (w, h, r) = (I::p(w), I::p(h), I::p(r));
        let t = (w - I::p(top_half)) / h;
        let one = I::p(1.0);
        let s2 = one + t.sqr();
        let s = s2.sqrt0();
        let d2 = one + I::p(2.0) * t.sqr();
        let cx3 = -w + t * r + r * s;
        let cz8 = h - r;
        let cx8 = -w + t * cz8 + r * s;
        let ax = -w + r * s;
        let g = t.sqr() / s2;
        Self {
            w,
            h,
            t,
            r,
            s,
            d2,
            cx3,
            cx8,
            cz8,
            ax,
            g,
        }
    }

    /// The bottom gap's height along the line x = c + t z (parallel to the x- wall, c from
    /// -W at the wall to cx3 at the bottom tangent line): where the line meets the ball's
    /// circle, `z = e^2 / (b + sqrt(b^2 - s^2 e^2))`, `e = c - cx3 <= 0`, `b = r - t e`.
    fn h3(&self, c: I) -> I {
        let e = c - self.cx3;
        let e = I::new(e.lo, e.hi.min(0.0));
        let b = self.r - self.t * e;
        let disc = b.sqr() - (I::p(1.0) + self.t.sqr()) * e.sqr();
        e.sqr() / (b + disc.sqrt0())
    }

    /// The top gap's height along x = c + t z, from the circle up to z = H, and the part of
    /// the y walls' length 2 (W - t z) integrated over it: `(h, h (2W - t (H + z_up)))`.
    fn h8(&self, c: I) -> (I, I) {
        let one = I::p(1.0);
        let k2 = one + self.t.sqr();
        let e = c - self.cx8 + self.t * self.cz8;
        let disc = k2 * self.r.sqr() - e.sqr();
        let wv = (-(self.t * e) + disc.sqrt0()) / k2;
        let z_up = self.cz8 + wv;
        let h = self.h - z_up;
        let h = I::new(h.lo.max(0.0), h.hi.max(0.0));
        (h, h * (I::p(2.0) * self.w - self.t * (self.h + z_up)))
    }

    /// The corner gap's length along y at height 0 and x = c (the same at every height on
    /// x = c + t z: the gap is a prism along (t, t, 1)): the y with the point inside the
    /// y- wall (`Y >= -r s`), in the sector between the ball's two tangent rays
    /// (`Y >= X / g`, `Y <= g X`) and outside the ball's cylinder (`Q(Y) >= 0`), with
    /// `X = c - ax`, `Y = y - ax`.
    fn lambda(&self, c: I) -> I {
        let x = c - self.ax;
        let a = (-(self.r * self.s)).max(x / self.g);
        let b = self.g * x;
        let t2 = self.t.sqr();
        let q2 = I::p(1.0) - t2 / self.d2;
        let q1 = -(I::p(2.0) * t2 * x / self.d2);
        let q0 = x.sqr() * q2 - self.r.sqr();
        let disc = q1.sqr() - I::p(4.0) * q2 * q0;
        let root = disc.sqrt0();
        let y_minus = (-q1 - root) / (I::p(2.0) * q2);
        let y_plus = (-q1 + root) / (I::p(2.0) * q2);
        I::seg(a, b.min(y_minus)) + I::seg(a.max(y_plus), b)
    }

    /// The frustum's volume, `4 (W^3 - (W - t H)^3) / (3 t)`.
    fn frustum(&self) -> I {
        let top = self.w - self.t * self.h;
        I::p(4.0) * (self.w * self.w * self.w - top * top * top) / (I::p(3.0) * self.t)
    }
}

/// `int f` over `[a, b]`, enclosed: `n` cells, each `f` of the cell times its width.
fn integral(a: f64, b: f64, n: usize, f: impl Fn(I) -> I) -> I {
    let mut sum = I::p(0.0);
    let at = |i: usize| {
        if i == n {
            b
        } else {
            a + (b - a) * (i as f64) / (n as f64)
        }
    };
    let mut c0 = a;
    for i in 1..=n {
        let c1 = at(i);
        let cell = I::new(c0, c1);
        sum = sum + f(cell) * (I::p(c1) - I::p(c0));
        c0 = c1;
    }
    sum
}

/// The model's volumes: the file's, without the bottom round, without the top round.
struct Expected {
    base: I,
    without_bottom: I,
    without_top: I,
    gap_bottom: I,
    gap_top: I,
    frustum: I,
    gap_all: I,
}

fn expected(m: &Model, n: usize) -> Expected {
    let w = -m.w.lo;
    let v_f = m.frustum();
    let v3 = integral(w, m.cx3.hi, n, |c| {
        let h = m.h3(c);
        I::p(2.0) * m.w * h - m.t * h.sqr()
    });
    let c8 = m.cx8 - m.t * m.h;
    let v8 = integral(w, c8.hi, n, |c| m.h8(c).1);
    let v6 = m.h * integral(w, m.ax.hi, n, |c| m.lambda(c));
    let v36 = integral(w, m.cx3.hi, n, |c| m.lambda(c) * m.h3(c));
    let v68 = integral(w, c8.hi, n, |c| m.lambda(c) * m.h8(c).0);
    Expected {
        base: v_f - v3 - v6 - v8 + v36 + v68,
        without_bottom: v_f - v6 - v8 + v68,
        without_top: v_f - v3 - v6 + v36,
        gap_bottom: v3 - v36,
        gap_top: v8 - v68,
        frustum: v_f,
        gap_all: v3 + v6 + v8 - v36 - v68,
    }
}

type V3 = [I; 3];

fn v3(x: solidcore::math::Vec3) -> V3 {
    [I::p(x.x), I::p(x.y), I::p(x.z)]
}
fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale3(a: V3, s: I) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot3(a: V3, b: V3) -> I {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm3(a: V3) -> I {
    (a[0].sqr() + a[1].sqr() + a[2].sqr()).sqrt0()
}
fn unit3(a: V3) -> V3 {
    scale3(a, I::p(1.0) / norm3(a))
}
/// `|x|`'s upper end.
fn abs_hi(x: I) -> f64 {
    x.hi.max(-x.lo)
}

/// A plane (unit normal `n`, `n . X = d`) or a cylinder (a point `p` of its axis, unit
/// axis `u`, radius `r`), known to intervals.
enum Sup {
    Plane { n: V3, d: I },
    Cyl { p: V3, u: V3, r: I },
}

/// A face support's implicit data: the placement's normal (or axis) `x × y`, unitised. A
/// cone of semi-angle exactly 0 is its cylinder.
fn sup_of(a: &AnalyticSupport) -> Sup {
    match a {
        AnalyticSupport::Plane { placement, .. } => {
            let n = unit3(cross3(v3(placement.x_axis()), v3(placement.y_axis())));
            Sup::Plane {
                n,
                d: dot3(n, v3(placement.origin())),
            }
        }
        AnalyticSupport::Cylinder { placement, radius }
        | AnalyticSupport::Cone {
            placement,
            base_radius: radius,
            semi_angle_deg: 0.0,
        } => Sup::Cyl {
            p: v3(placement.origin()),
            u: unit3(cross3(v3(placement.x_axis()), v3(placement.y_axis()))),
            r: I::p(*radius),
        },
        other => panic!("not a plane or cylinder: {other:?}"),
    }
}

/// How far a point of `b` within `R0` of the origin can be from `a` (an upper bound, from
/// interval arithmetic): planes `|n_a - s n_b| R0 + |s d_b - d_a|`; cylinders
/// `|r_a - r_b| + |(p_b - p_a)⊥u_a| + (R0 + |p_b|) |u_b - s u_a|`, `s` the sign aligning
/// the normals (axes). `None` for different kinds.
fn bound(a: &Sup, b: &Sup) -> Option<f64> {
    const R0: f64 = 50.0;
    let sign = |x: I| {
        if x.lo + x.hi >= 0.0 {
            I::p(1.0)
        } else {
            I::p(-1.0)
        }
    };
    match (a, b) {
        (Sup::Plane { n: na, d: da }, Sup::Plane { n: nb, d: db }) => {
            let s = sign(dot3(*na, *nb));
            let tilt = norm3(sub3(*na, scale3(*nb, s)));
            Some(up(up(tilt.hi * R0) + abs_hi(s * *db - *da)))
        }
        (
            Sup::Cyl {
                p: pa,
                u: ua,
                r: ra,
            },
            Sup::Cyl {
                p: pb,
                u: ub,
                r: rb,
            },
        ) => {
            let s = sign(dot3(*ua, *ub));
            let w = sub3(*pb, *pa);
            let perp = norm3(sub3(w, scale3(*ua, dot3(w, *ua))));
            let tilt = norm3(sub3(*ub, scale3(*ua, s)));
            let reach = I::p(R0) + norm3(*pb);
            Some((abs_hi(*ra - *rb) + perp.hi + (tilt * reach).hi) * (1.0 + 4.0 * f64::EPSILON))
        }
        _ => None,
    }
}

/// The model's surfaces, named: the six planes and the three rounds' cylinders.
fn model_supports(m: &Model) -> Vec<(&'static str, Sup)> {
    let (z, one) = (I::p(0.0), I::p(1.0));
    let inv_s = one / m.s;
    let wall = |n: V3, d: I| Sup::Plane {
        n: scale3(n, inv_s),
        d: d * inv_s,
    };
    let along_y = [z, one, z];
    let corner = scale3([m.t, m.t, one], one / m.d2.sqrt0());
    vec![
        (
            "bottom",
            Sup::Plane {
                n: [z, z, one],
                d: z,
            },
        ),
        (
            "top",
            Sup::Plane {
                n: [z, z, one],
                d: m.h,
            },
        ),
        ("wall x-", wall([one, z, -m.t], -m.w)),
        ("wall x+", wall([one, z, m.t], m.w)),
        ("wall y-", wall([z, one, -m.t], -m.w)),
        ("wall y+", wall([z, one, m.t], m.w)),
        (
            "bottom round",
            Sup::Cyl {
                p: [m.cx3, z, m.r],
                u: along_y,
                r: m.r,
            },
        ),
        (
            "top round",
            Sup::Cyl {
                p: [m.cx8, z, m.cz8],
                u: along_y,
                r: m.r,
            },
        ),
        (
            "corner round",
            Sup::Cyl {
                p: [m.ax, m.ax, z],
                u: corner,
                r: m.r,
            },
        ),
    ]
}

/// A face surface's support, its NURBS form proved on it: its analytic kind's
/// ([`AnalyticSupport::check`]), or, for a B-spline face, the support of the analytic
/// `CopyAndAddAnalytics` recognises in it ([`BSplineSurface::recognised_analytic`]) with
/// the face's own form proved on that support's implicit surface by [`lies_on`]. (A face
/// whose B-spline its recognised analytic does not give at the same parameters keeps the
/// B-spline, an exact copy, WR-450 #18923: the support is the same point set.)
///
/// [`BSplineSurface::recognised_analytic`]: solidcore::nurbs::b_spline_surface::BSplineSurface::recognised_analytic
fn proved_support(k: &Kernel, s: &solidcore::surface::surface::Surface) -> AnalyticSupport {
    use solidcore::surface::implicit::lies_on;
    use solidcore::surface::surface::SurfaceKind;
    if AnalyticSupport::of(s).is_some() {
        return AnalyticSupport::check(k, s).unwrap();
    }
    let SurfaceKind::BSpline(form) = s.kind() else {
        panic!("a face of no analytic kind and no B-spline form");
    };
    let analytic = form
        .recognised_analytic(k)
        .unwrap()
        .expect("an analytic recognised in the face's B-spline");
    let support = AnalyticSupport::of(&analytic).expect("the analytic's support");
    assert!(
        lies_on(form, &support.implicit()).unwrap(),
        "the face's B-spline off its recognised analytic {support:?}"
    );
    support
}

/// Each face's support, its NURBS form proved on it ([`proved_support`]), matched to the
/// nearest of `named`: the name and the certified distance bound.
fn match_supports(k: &Kernel, b: &Brep, named: &[(&'static str, Sup)]) -> Vec<(&'static str, f64)> {
    bp1::faces(b, None)
        .unwrap()
        .into_iter()
        .map(|f| {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let sup = sup_of(&proved_support(k, s));
            named
                .iter()
                .filter_map(|(name, m)| bound(m, &sup).map(|d| (*name, d)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap()
        })
        .collect()
}

/// Which round the defeature takes off.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Removed {
    Bottom,
    Top,
    /// All three rounds, as one feature: the base is the plain frustum.
    All,
}

/// The surfaces bound (the faces' bound, [`bound`]) holds for points within `R0` = 50 of the
/// origin: every face's box lies within 45 of it.
fn assert_within_r0(k: &Kernel, b: &Brep) {
    for f in bp1::faces(b, None).unwrap() {
        let bx = solidcore::query::face::p1::calculate_bounding_box(k, b, f, false).unwrap();
        for i in 0..3 {
            assert!(
                bx.min_at(i).abs() <= 45.0 && bx.max_at(i).abs() <= 45.0,
                "{bx:?}"
            );
        }
        let corner = (0..3)
            .map(|i| det::powi(bx.min_at(i).abs().max(bx.max_at(i).abs()), 2))
            .sum::<f64>()
            .sqrt();
        assert!(corner <= 45.0, "{bx:?}");
    }
}

/// `b` is sound (manifold, its Level 2 errors no more of each kind than the file's; the rest
/// are warnings), within `R0`, and its faces lie on the model's surfaces within 1e-9, named
/// exactly as `names` (name, count; the copied patch faces beside the faces they extend
/// count apart). Returns the faces' largest bound.
fn sound_on_model(
    k: &mut Kernel,
    b: &mut Brep,
    named: &[(&'static str, Sup)],
    names: &[(&str, usize)],
    what: &str,
) -> f64 {
    const ON: f64 = 1e-9;
    let mut fresh = read(k, "Fillet/fillet1019.smb").unwrap();
    let file_lines = validation::soundness(k, &mut fresh).unwrap().check.lines();
    let snd = validation::soundness(k, b).unwrap();
    assert!(snd.manifold, "{what}");
    // an error's kind is its text before the measured values
    let kind = |l: &String| l.split(" (offset").next().unwrap_or(l).to_string();
    let errors = |lines: &[String]| {
        let mut v: Vec<String> = lines
            .iter()
            .filter(|l| !l.contains("Warning"))
            .map(kind)
            .collect();
        v.sort();
        v
    };
    let (built, file) = (errors(&snd.check.lines()), errors(&file_lines));
    for e in &built {
        let n = |v: &[String]| v.iter().filter(|x| *x == e).count();
        assert!(n(&built) <= n(&file), "{what}: {e}");
    }
    assert_within_r0(k, b);
    let faces = match_supports(k, b, named);
    let mut got: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (name, d) in &faces {
        assert!(*d <= ON, "{what}: {faces:?}");
        *got.entry(name).or_default() += 1;
    }
    let want: std::collections::BTreeMap<&str, usize> = names.iter().copied().collect();
    assert_eq!(got, want, "{what}: {faces:?}");
    faces.iter().map(|p| p.1).fold(0.0, f64::max)
}

/// `x` widened by the volume a surface moved by at most `delta` can sweep over `AREA`, a bound
/// on these bodies' surface area (the frustum's is about 9700 mm^2).
fn grow(x: I, delta: f64) -> I {
    const AREA: f64 = 2e4;
    I::new(down(x.lo - AREA * delta), up(x.hi + AREA * delta))
}

/// `b`'s certified volume enclosure, asked to 0.2: it must hold `model` grown by `delta`.
fn holds_volume(k: &Kernel, b: &mut Brep, model: I, delta: f64, what: &str) -> I {
    const WIDTH: f64 = 0.2;
    let full = enclose_volume(k, b, WIDTH).unwrap();
    let v = I::new(full.lo, full.hi);
    assert!(v.width() <= WIDTH, "{what}: {full:?}");
    assert!(
        v.contains(grow(model, delta)),
        "{what}: model {model:?}, built {v:?}"
    );
    v
}

/// The proof that `b` is `fillet1019` with the `removed` round taken off.
///
/// 1. The model is `fillet1019`: every face of the file lies on one of the model's surfaces
///    ([`model_supports`]: the frustum's planes, rolling-ball cylinders of radius 5.08 tangent
///    to their two planes) within `delta` (an interval bound, under 1e-9), each of the nine
///    surfaces carrying a face, and the file's certified volume enclosure holds the model's
///    ([`expected`]: the frustum less the three rounds' gaps, their overlaps added back, each
///    a one-dimensional integral enclosed with outward rounding).
/// 2. `b` is sound and lies on the model's surfaces ([`sound_on_model`]), its faces named
///    exactly: the removed round's cylinder carries none, the walls and the bottom (top) are
///    extended and the corner round is carried on by its own cylinder.
/// 3. `b`'s certified volume enclosure holds the model's volume without the removed round's
///    gap (that gap less its overlap with the corner round's), widened by the area bound
///    times `b`'s own surface bound: the two volumes differ by at most the enclosure's width
///    (0.2).
fn certify(k: &mut Kernel, b: &mut Brep, removed: Removed) {
    let m = Model::new(25.4, 25.4, 23.177787946441534, 5.08);
    let named = model_supports(&m);
    let e = expected(&m, 100_000);

    // 1. the model is the file
    let mut base = read(k, "Fillet/fillet1019.smb").unwrap();
    let file_names = [
        ("bottom", 1),
        ("top", 1),
        ("wall x-", 1),
        ("wall x+", 1),
        ("wall y-", 1),
        ("wall y+", 1),
        ("bottom round", 1),
        ("top round", 1),
        ("corner round", 1),
    ];
    let delta = sound_on_model(
        k,
        &mut base.clone_standing_for_self().unwrap(),
        &named,
        &file_names,
        "file",
    );
    let kb = holds_volume(k, &mut base, e.base, delta, "file");

    // 2. sound, and on the model's surfaces less the removed round
    let (names, want): (&[(&str, usize)], I) = match removed {
        Removed::Bottom => (
            &[
                ("bottom", 3),
                ("top", 1),
                ("wall x-", 2),
                ("wall x+", 1),
                ("wall y-", 2),
                ("wall y+", 2),
                ("top round", 1),
                ("corner round", 2),
            ],
            e.without_bottom,
        ),
        Removed::Top => (
            &[
                ("bottom", 1),
                ("top", 2),
                ("wall x-", 2),
                ("wall x+", 1),
                ("wall y-", 1),
                ("wall y+", 2),
                ("bottom round", 1),
                ("corner round", 2),
            ],
            e.without_top,
        ),
        Removed::All => (
            &[
                ("bottom", 2),
                ("top", 2),
                ("wall x-", 2),
                ("wall x+", 1),
                ("wall y-", 2),
                ("wall y+", 3),
            ],
            e.frustum,
        ),
    };
    let what = format!("{removed:?} base");
    let built_delta = sound_on_model(k, b, &named, names, &what);

    // 3. the volume
    let kv = holds_volume(k, b, want, built_delta, &what);
    eprintln!(
        "{what}: file delta {delta:e}, model {:?} file {kb:?}; delta {built_delta:e}, model {want:?} built {kv:?}",
        e.base
    );
}

/// The feature B-rep of the one feature made (the goldens' `feature0`): the removed round's
/// gap as a solid, proved as [`certify`] proves the base. Sound and on the model's surfaces
/// (the round, the planes along it extended, the corner round's extension, the y walls
/// closing its ends), and its certified volume enclosure holds the gap's model volume (the
/// gap prism less its overlap with the corner round's).
fn certify_feature(k: &mut Kernel, features: Vec<Option<Brep>>, removed: Removed) {
    let m = Model::new(25.4, 25.4, 23.177787946441534, 5.08);
    let named = model_supports(&m);
    let e = expected(&m, 100_000);
    assert_eq!(features.iter().filter(|f| f.is_some()).count(), 1);
    let mut fb = features.into_iter().flatten().next().unwrap();
    let (names, gap): (&[(&str, usize)], I) = match removed {
        Removed::Bottom => (
            &[
                ("bottom round", 1),
                ("bottom", 2),
                ("wall x-", 1),
                ("wall y-", 1),
                ("wall y+", 1),
                ("corner round", 1),
            ],
            e.gap_bottom,
        ),
        Removed::Top => (
            &[
                ("top round", 1),
                ("top", 1),
                ("wall x-", 1),
                ("wall y+", 1),
                ("corner round", 1),
            ],
            e.gap_top,
        ),
        Removed::All => (
            &[
                ("bottom round", 1),
                ("top round", 1),
                ("corner round", 1),
                ("bottom", 1),
                ("top", 1),
                ("wall x-", 1),
                ("wall y-", 1),
                ("wall y+", 2),
            ],
            e.gap_all,
        ),
    };
    let what = format!("{removed:?} feature");
    let delta = sound_on_model(k, &mut fb, &named, names, &what);
    let kv = holds_volume(k, &mut fb, gap, delta, &what);
    eprintln!("{what}: delta {delta:e}, model {gap:?} built {kv:?}");
}

/// An annular groove meets the rest of the turned part at two disjoint circles.
/// A single patch must use both circles to recover the wall between them.
mod ring_grooves {
    use solidcore::blend::feature_executive::{FeatureExecutive, KeptFeature, RebuildBrepType};
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::curve::curve::Curve;
    use solidcore::error::defeature::DefeatureFailure;
    use solidcore::error::Reason;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::geom::vector3d::Vec3;
    use solidcore::kernel::UpstreamFixes;
    use solidcore::mass::enclosure::enclose_volume;
    use solidcore::math::det;
    use solidcore::nurbs::b_spline_curve::{p1::BSplineCurve, NurbCircleParam};
    use solidcore::primitive::api_primitives::create_sphere;
    use solidcore::surface::implicit::AnalyticSupport;
    use solidcore::sweep::directed::revolve;
    use solidcore::topology::brep::{p2::is_manifold_solid, validation};
    use solidcore::topology::{
        edgeuse::p1::uv_trim_curve, face::p4::upward_edgeuses, face::p4::vertices, vertex,
    };
    use solidcore::{Brep, FaceId, Kernel, SolidError};
    use std::f64::consts::PI;

    struct Groove {
        /// Radii of the opposite wall, and of the grooved wall at z = 0 and 20.
        wall_radii: [f64; 3],
        /// The entire groove, from one support at z = 8 to the other at z = 12.
        meridian: &'static [(f64, f64)],
        semicircle: bool,
    }

    const SQUARE: Groove = Groove {
        wall_radii: [0.0, 5.0, 5.0],
        meridian: &[(5.0, 8.0), (4.0, 8.0), (4.0, 12.0), (5.0, 12.0)],
        semicircle: false,
    };

    const TAPERED: Groove = Groove {
        wall_radii: [0.0, 5.0, 7.0],
        meridian: &[(5.8, 8.0), (4.8, 8.0), (5.2, 12.0), (6.2, 12.0)],
        ..SQUARE
    };

    impl Groove {
        fn body(&self, k: &mut Kernel) -> Brep {
            let [back, bottom, top] = self.wall_radii;
            let mut profile = vec![(back, 0.0), (bottom, 0.0)];
            profile.extend_from_slice(self.meridian);
            profile.extend([(top, 20.0), (back, 20.0), (back, 0.0)]);
            if back > bottom {
                profile.reverse(); // The selected wall is inside the bore.
            }
            turned_part(k, &profile, self.semicircle)
        }
    }

    fn turned_part(k: &mut Kernel, profile: &[(f64, f64)], semicircle: bool) -> Brep {
        let point = |(r, z)| Vec3::new(r, 0.0, z);
        let curves: Vec<Curve> = profile
            .windows(2)
            .map(|ends| {
                let [a, b] = [ends[0], ends[1]];
                let curve = if semicircle && [a, b] == [(5.0, 8.0), (5.0, 12.0)] {
                    BSplineCurve::create_circle_segment(
                        3,
                        &Axis2Placement::new(point((5.0, 10.0)), -Vec3::Z, -Vec3::X),
                        2.0,
                        0.0,
                        180.0,
                        NurbCircleParam::Quadratic,
                    )
                } else {
                    BSplineCurve::create_line_segment(3, point(a), point(b))
                };
                Curve::from(curve.unwrap())
            })
            .collect();
        let refs: Vec<_> = curves.iter().map(Some).collect();
        let mut body = revolve(k, &refs, Vec3::ZERO, Vec3::Z, 360.0, false).unwrap();
        body.use_arena_naming().unwrap();
        body
    }

    fn kernel() -> Kernel {
        let mut k = Kernel::default();
        for &name in UpstreamFixes::NAMES {
            *k.fixes.flag_mut(name).unwrap() = true;
        }
        k
    }

    fn face_points(b: &Brep, f: FaceId) -> Vec<Vec3> {
        vertices(b, f, None, false, None)
            .unwrap()
            .into_iter()
            .map(|v| vertex::point(b, v).unwrap())
            .collect()
    }

    fn faces_in_band(b: &Brep, lower: f64, upper: f64) -> Vec<FaceId> {
        b.face_list()
            .iter()
            .copied()
            .filter(|&f| {
                let points = face_points(b, f);
                !points.is_empty() && points.iter().all(|p| (lower..upper).contains(&p.z))
            })
            .collect()
    }

    fn solid(k: &mut Kernel, b: &mut Brep, faces: usize, volume: f64) {
        assert_eq!(b.face_list().len(), faces);
        assert!(is_manifold_solid(b).unwrap());
        let check = validation::check_brep(k, b, validation::CheckLevel::Level2).unwrap();
        assert!(check.is_valid(), "{:?}", check.lines());
        let v = enclose_volume(k, b, 1e-6).unwrap();
        assert!(
            v.width() <= 1e-6 && v.lo <= volume && volume <= v.hi,
            "expected {volume}, got {v:?}"
        );
    }

    fn shaft_wall_tolerances(b: &Brep) -> Vec<f64> {
        b.face_list()
            .iter()
            .filter_map(|&f| {
                let face = b.face(f).unwrap();
                let support = face.surface.as_ref().and_then(AnalyticSupport::of);
                matches!(support, Some(AnalyticSupport::Cylinder { radius: 5.0, .. }))
                    .then_some(face.tolerance)
            })
            .collect()
    }

    fn add_groove(k: &mut Kernel, ex: &mut FeatureExecutive<'_>, faces: &[FaceId]) {
        let feature = ex
            .add_feature(
                k,
                faces,
                RebuildBrepType::ExtendSurface,
                RebuildBrepType::FromOtherBrep,
            )
            .unwrap()
            .unwrap();
        assert_eq!(ex.features()[feature].loops.len(), 2);
    }

    fn remove(
        k: &mut Kernel,
        b: &mut Brep,
        groove: &Groove,
        reach: Option<[f64; 2]>,
    ) -> solidcore::Result<Option<Brep>> {
        let fs = faces_in_band(b, 7.5, 12.5);
        assert_eq!(fs.len(), groove.meridian.len() - 1);
        let mut ex = FeatureExecutive::new(k, b)?;
        add_groove(k, &mut ex, &fs);
        if let Some(distances) = reach {
            ex.features_mut()[0].set_base_ext_dists(&distances).unwrap();
        }
        let kept = ex.do_defeature(k)?;
        assert!(kept.is_empty(), "{kept:?}");
        Ok(ex.into_feature_breps().into_iter().next().flatten())
    }

    #[test]
    fn both_circles_bound_one_wall_on_a_shaft_frustum_or_bore() {
        let mut k = kernel();
        let vee = Groove {
            meridian: &[(5.0, 8.0), (4.0, 10.0), (5.0, 12.0)],
            ..SQUARE
        };
        let round = Groove {
            meridian: &[(5.0, 8.0), (5.0, 12.0)],
            semicircle: true,
            ..SQUARE
        };
        let bore = Groove {
            wall_radii: [7.0, 3.0, 3.0],
            meridian: &[(3.0, 8.0), (4.0, 8.0), (4.0, 12.0), (3.0, 12.0)],
            ..SQUARE
        };
        let shaft_volume = PI * 5.0 * 5.0 * 20.0;
        for (groove, faces, volume) in [
            (&SQUARE, 3, shaft_volume),
            (&vee, 3, shaft_volume),
            (&round, 3, shaft_volume),
            (&TAPERED, 3, PI * 20.0 / 3.0 * (25.0 + 35.0 + 49.0)),
            (&bore, 4, PI * 20.0 * (49.0 - 9.0)),
        ] {
            let mut body = groove.body(&mut k);
            let feature = remove(&mut k, &mut body, groove, None).unwrap();
            solid(&mut k, &mut body, faces, volume);
            if groove.meridian == SQUARE.meridian {
                // The extracted feature is the annular gap between radii 4 and 5.
                solid(&mut k, &mut feature.unwrap(), 4, PI * 4.0 * (25.0 - 16.0));
            }
        }
    }

    #[test]
    fn cylinder_and_cone_continue_to_the_circle_where_they_meet() {
        let mut k = kernel();
        let groove = Groove {
            wall_radii: [0.0, 5.0, 7.5],
            meridian: &[(5.0, 8.0), (4.0, 8.0), (4.0, 12.0), (5.5, 12.0)],
            ..SQUARE
        };
        let mut body = groove.body(&mut k);
        remove(&mut k, &mut body, &groove, None).unwrap();
        // r = 5 below z = 10, then a 10 mm frustum from r = 5 to 7.5.
        let volume = PI * (250.0 + 10.0 / 3.0 * (25.0 + 37.5 + 56.25));
        solid(&mut k, &mut body, 4, volume);
    }

    fn sufficient_reach(
        k: &mut Kernel,
        body: &mut Brep,
        groove: &Groove,
        available: [f64; 2],
    ) -> [f64; 2] {
        let error = remove(k, body, groove, Some(available)).unwrap_err();
        let SolidError::Failed(failure) = error else {
            panic!("expected an annular reach refusal, got {error:?}");
        };
        let Reason::Defeature(DefeatureFailure::InsufficientAnnularReach {
            available: reported,
            sufficient,
        }) = failure.reason
        else {
            panic!("expected the sufficient reach, got {failure:?}");
        };
        assert_eq!(reported, available);
        sufficient
    }

    #[test]
    fn supports_that_cannot_reach_each_other_leave_the_groove_unchanged() {
        let mut k = kernel();
        let unequal = Groove {
            wall_radii: [0.0, 5.0, 6.0],
            meridian: &[(5.0, 8.0), (4.0, 8.0), (4.0, 12.0), (6.0, 12.0)],
            ..SQUARE
        };
        // Three 8, 4 and 8 mm cylinders; different support radii never meet.
        let mut body = unequal.body(&mut k);
        assert!(remove(&mut k, &mut body, &unequal, None).is_err());
        solid(
            &mut k,
            &mut body,
            7,
            PI * (8.0 * 25.0 + 4.0 * 16.0 + 8.0 * 36.0),
        );

        // Both rims share the axial 4 mm gap. Splitting the missing reach evenly
        // gives a pair that just closes it, including when the supplied reaches differ.
        for (available, expected) in [([0.25, 0.25], [2.0, 2.0]), ([0.25, 1.25], [1.5, 2.5])] {
            let mut body = SQUARE.body(&mut k);
            let sufficient = sufficient_reach(&mut k, &mut body, &SQUARE, available);
            assert_eq!(sufficient, expected);
            solid(&mut k, &mut body, 7, PI * (16.0 * 25.0 + 4.0 * 16.0));
            let [lower, upper]: [f64; 2] = shaft_wall_tolerances(&body).try_into().unwrap();
            remove(&mut k, &mut body, &SQUARE, Some(sufficient)).unwrap();
            solid(&mut k, &mut body, 3, 500.0 * PI);
            let [wall]: [f64; 1] = shaft_wall_tolerances(&body).try_into().unwrap();
            assert!(
                wall <= lower && wall <= upper,
                "wall {wall}, original supports {lower}, {upper}"
            );
        }
        // On the 1:10 taper the measured reaches run along the slant. Rounding them
        // down can leave their axial projections just short of 4 mm on the retry.
        let mut body = TAPERED.body(&mut k);
        let sufficient = sufficient_reach(&mut k, &mut body, &TAPERED, [0.001, 0.003]);
        let axial = sufficient.map(|s| s / det::hypot(1.0_f64, 0.1));
        assert!(axial[0] + axial[1] >= 4.0);
        remove(&mut k, &mut body, &TAPERED, Some(sufficient)).unwrap();
        solid(&mut k, &mut body, 3, PI * 20.0 / 3.0 * (25.0 + 35.0 + 49.0));
    }

    fn remove_two(
        k: &mut Kernel,
        body: &mut Brep,
        blocked: &[FaceId],
        free: &[FaceId],
        blocked_first: bool,
    ) -> KeptFeature {
        assert_eq!((blocked.len(), free.len()), (3, 3));
        let order = if blocked_first {
            [blocked, free]
        } else {
            [free, blocked]
        };
        let mut ex = FeatureExecutive::new(k, body).unwrap();
        for fs in order {
            add_groove(k, &mut ex, fs);
        }
        let kept = ex.do_defeature(k).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].feature, usize::from(!blocked_first));
        kept[0]
    }

    #[test]
    fn a_refused_step_groove_still_allows_the_independent_square_groove_in_either_order() {
        let mut k = kernel();
        let profile = [
            (0., 0.),
            (5., 0.),
            (5., 8.),
            (4., 8.),
            (4., 12.),
            (6., 12.),
            (6., 15.),
            (5., 15.),
            (5., 17.),
            (6., 17.),
            (6., 20.),
            (0., 20.),
            (0., 0.),
        ];
        for blocked_first in [true, false] {
            let mut body = turned_part(&mut k, &profile, false);
            let blocked = faces_in_band(&body, 7.5, 12.5);
            let free = faces_in_band(&body, 14.5, 17.5);
            let kept = remove_two(&mut k, &mut body, &blocked, &free, blocked_first);
            assert!(
                matches!(kept.limit, Some(DefeatureFailure::NotEnclosed { .. })),
                "{kept:?}"
            );
            assert_eq!(faces_in_band(&body, 7.5, 12.5).len(), 3);
            assert!(faces_in_band(&body, 14.5, 17.5).is_empty());
            // The unreached step stays at r=4 for 4 mm; the full upper 8 mm are r=6.
            solid(
                &mut k,
                &mut body,
                7,
                PI * (8.0 * 25.0 + 4.0 * 16.0 + 8.0 * 36.0),
            );
        }
    }

    fn assert_same_lip(before: &Brep, after: &Brep, face: FaceId) {
        let a = before.face(face).unwrap();
        let b = after.face(face).unwrap();
        assert_eq!(b.tolerance, a.tolerance);
        assert_eq!(b.uv_domain, a.uv_domain);
        let uses = upward_edgeuses(before, face, false, None).unwrap();
        assert_eq!(upward_edgeuses(after, face, false, None).unwrap(), uses);
        for eu in uses {
            let (old, old_owner) = uv_trim_curve(before, eu).unwrap();
            let (new, new_owner) = uv_trim_curve(after, eu).unwrap();
            assert_eq!(new_owner, old_owner, "storage of the UV trim on {eu:?}");
            assert!(
                match (old, new) {
                    (Some(a), Some(b)) => a.same_definition(b),
                    (None, None) => true,
                    _ => false,
                },
                "UV trim changed on {eu:?}"
            );
            let edge = before.edgeuse(eu).unwrap().edge.unwrap();
            let a = before.edge(edge).unwrap();
            let b = after.edge(edge).unwrap();
            assert_eq!(b.tolerance, a.tolerance);
            assert_eq!(b.interval, a.interval);
            for v in a.vertices {
                assert_eq!(
                    vertex::tolerance(after, v).unwrap(),
                    vertex::tolerance(before, v).unwrap()
                );
            }
        }
    }

    #[test]
    fn a_sleeve_blocks_one_groove_and_the_other_still_comes_off() {
        let mut k = kernel();
        // The attached outer sleeve has an inward lip at z = 10..11. Its radius of
        // 4.5 clears the first groove (r = 4) and cuts through the proposed r = 5 wall.
        let profile = [
            (0., 0.),
            (8., 0.),
            (8., 11.),
            (4.5, 11.),
            (4.5, 10.),
            (7., 10.),
            (7., 1.),
            (5., 1.),
            (5., 8.),
            (4., 8.),
            (4., 12.),
            (5., 12.),
            (5., 15.),
            (4., 15.),
            (4., 17.),
            (5., 17.),
            (5., 20.),
            (0., 20.),
            (0., 0.),
        ];
        let original = turned_part(&mut k, &profile, false);
        let lip_faces = faces_in_band(&original, 9.5, 11.5);
        assert_eq!(lip_faces.len(), 3);
        for blocked_first in [true, false] {
            let mut body = turned_part(&mut k, &profile, false);
            let blocked: Vec<_> = faces_in_band(&body, 7.5, 12.5)
                .into_iter()
                .filter(|&f| {
                    face_points(&body, f)
                        .iter()
                        .any(|p| det::hypot(p.x, p.y) < 4.25)
                })
                .collect();
            let free = faces_in_band(&body, 14.5, 17.5);
            let kept = remove_two(&mut k, &mut body, &blocked, &free, blocked_first);
            let Some(DefeatureFailure::PatchIntersectsFace { face }) = kept.limit else {
                panic!("expected the interfering sleeve face, got {kept:?}");
            };
            for &f in &lip_faces {
                assert_same_lip(&original, &body, f);
            }
            let lip = face_points(&body, face);
            assert!(lip.iter().all(|p| (9.5..11.5).contains(&p.z)));
            assert!(lip.iter().any(|p| det::hypot(p.x, p.y) < 5.0));
            assert!(lip.iter().any(|p| det::hypot(p.x, p.y) > 5.0));
            // Shaft with the 4 mm groove, plus the sleeve's three annular sections.
            let volume =
                PI * (500.0 - 4.0 * 9.0 + 39.0 + 9.0 * 15.0 + (64.0 - det::powi(4.5_f64, 2)));
            solid(&mut k, &mut body, 13, volume);
        }
    }

    #[test]
    fn a_ball_wholly_inside_the_missing_material_keeps_its_region() {
        let mut k = kernel();
        let shaft = SQUARE.body(&mut k);
        // The entire ball lies between the groove floor at r = 4 and its healed wall at r = 5.
        let radius = 0.2;
        let ball = create_sphere(&mut k, Vec3::new(4.5, 0.0, 10.0), radius).unwrap();
        let mut body = boolean_union(&mut k, shaft, ball).unwrap();
        body.use_arena_naming().unwrap();
        let [ball_face]: [FaceId; 1] = faces_in_band(&body, 9.5, 10.5).try_into().unwrap();
        let mut groove = faces_in_band(&body, 7.5, 12.5);
        groove.retain(|f| *f != ball_face);
        assert_eq!(groove.len(), 3);
        let mut ex = FeatureExecutive::new(&mut k, &mut body).unwrap();
        add_groove(&mut k, &mut ex, &groove);
        let err = ex.do_defeature(&mut k).unwrap_err();
        let SolidError::Failed(failure) = err else {
            panic!("expected a refusal naming the enclosed face, got {err:?}");
        };
        assert_eq!(
            failure.reason,
            Reason::Defeature(DefeatureFailure::PatchEnclosesFace { face: ball_face })
        );
        let original = PI * (500.0 - 4.0 * 9.0 + 4.0 / 3.0 * det::powi(radius, 3));
        solid(&mut k, &mut body, 8, original);
    }
}
