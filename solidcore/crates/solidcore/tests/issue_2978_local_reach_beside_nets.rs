//! The local operation's reach beside bare B-spline nets: planes moved beside the walls of
//! `Solids/extrusion.smb`, two of which are extrusions with no analytic label (of a cubic
//! B-spline and of a rational curve of lines and an arc; the reader makes its other faces
//! planes and cylinders), build sound bodies of their closed-form volumes instead of being
//! refused as `ReachUncertified`; so does the base of `Solids/pyramid.smb` moved up,
//! beside sides stored as bare B-spline nets; and
//! the planes moved over the curved nets of `Solids/nurb_fillet.smb` (the cases whose
//! extension can leave a body that is no manifold solid) are built sound or refused with
//! the input left as it came in, never returned unsound.
//!
//! Each case is a goldens case of `topology.brep_local_operation`, made as its runner makes
//! it: a face's surface replaced by the plane through the point at the middle of its UV
//! domain, moved `offset` along the surface's normal there, the normal tilted by `tilt`.
//!
//! The solids are the upstream test files, read when the pinned upstream checkout is
//! present (the tests are skipped otherwise, as in CI's test job).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{face_area, face_centroid};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::knots::Side;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::volume;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2 as face_ops;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::FaceId;
use solidcore::{Brep, EntityRef, Kernel};

/// The upstream test file `Solids/<name>`, or `None` without the upstream checkout.
fn solid(k: &mut Kernel, name: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Solids")
        .join(name);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
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

/// The plane the goldens runner gives face `index` (`GetFaces` order): through the point
/// at the middle of its UV domain moved `offset` along the normal there, the normal plus
/// `tilt`: the face, and the plane's origin and normal.
fn plane_for(
    k: &Kernel,
    b: &Brep,
    index: usize,
    offset: f64,
    tilt: [f64; 3],
) -> (FaceId, (Point3, Vec3)) {
    let face = bp1::faces(b, None).unwrap()[index];
    let surface = face_ops::surface(b, face).unwrap().unwrap();
    let uv = b.face(face).unwrap().uv_domain.evaluate(0.5, 0.5);
    let p = surface.evaluate_point(k, uv).unwrap();
    let n = surface
        .evaluate_normal(k, uv, Side::Left, Side::Left)
        .unwrap();
    let origin = Point3::new(p.x + n.x * offset, p.y + n.y * offset, p.z + n.z * offset);
    let normal = Vec3::new(n.x + tilt[0], n.y + tilt[1], n.z + tilt[2]);
    (face, (origin, normal))
}

/// `b`'s faces `moves` (index, offset, tilt) given their planes by a local operation; the
/// planes' origins and normals.
fn moved(
    k: &mut Kernel,
    b: &mut Brep,
    moves: &[(usize, f64, [f64; 3])],
) -> solidcore::Result<Vec<(Point3, Vec3)>> {
    let (faces, planes): (Vec<FaceId>, Vec<(Point3, Vec3)>) = moves
        .iter()
        .map(|&(i, offset, tilt)| plane_for(k, b, i, offset, tilt))
        .unzip();
    let surfaces = planes
        .iter()
        .map(|&(o, n)| Surface::from(Plane::from_normal(o, n)))
        .collect();
    let mut notifications = Notifications::new(b);
    local_operation(k, b, &faces, surfaces, &mut notifications.context())?;
    Ok(planes)
}

/// The face of `b` whose surface's point at the middle of its domain is at height `z`, and
/// whose normal there is along z (a cap of the extrusion).
fn cap_at(k: &Kernel, b: &Brep, z: f64) -> FaceId {
    let caps: Vec<FaceId> = bp1::faces(b, None)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            let s = face_ops::surface(b, f).unwrap().unwrap();
            let uv = b.face(f).unwrap().uv_domain.evaluate(0.5, 0.5);
            let p = s.evaluate_point(k, uv).unwrap();
            let n = s.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap();
            (p.z - z).abs() < 1e-9 && n.x.abs() < 1e-9 && n.y.abs() < 1e-9
        })
        .collect();
    assert_eq!(caps.len(), 1, "{caps:?}");
    caps[0]
}

/// The integrators' accuracy, and the agreement asked of two integrals at it.
const ACCURACY: f64 = 1e-6;
const AGREE: f64 = 1e-5;

/// Walls of the extrusion (z from 0 to 10) moved onto planes, the caps kept: the result
/// is the prism over its bottom cap, ten times the cap's area, and sound, and each new wall
/// stands on its plane (at least four of the body's vertices on it, two at each cap).
fn walls_moved_build_their_prism(moves: &[(usize, f64, [f64; 3])]) {
    let mut k = Kernel::default();
    let Some(mut b) = solid(&mut k, "extrusion.smb") else {
        return;
    };
    let faces = bp1::faces(&b, None).unwrap().len();
    let planes = moved(&mut k, &mut b, moves).unwrap();
    for (o, n) in planes {
        let n = n * (1.0 / n.length());
        let on: Vec<f64> = b
            .vertex_list()
            .iter()
            .map(|&v| solidcore::topology::vertex::point(&b, v).unwrap())
            .filter(|p| (*p - o).dot(n).abs() <= b.tolerance())
            .map(|p| p.z)
            .collect();
        let at = |z: f64| {
            on.iter()
                .filter(|&&h| (h - z).abs() <= b.tolerance())
                .count()
        };
        assert!(at(0.0) >= 2 && at(10.0) >= 2, "{on:?}");
    }
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    assert_eq!(bp1::faces(&b, None).unwrap().len(), faces);
    let v = volume(&mut k, &mut b).unwrap();
    let bottom = cap_at(&k, &b, 0.0);
    let want = 10.0 * face_area(&k, &mut b, bottom, ACCURACY).unwrap();
    assert!((v - want).abs() <= AGREE * want, "{v} vs {want}");
}

/// gen0000: the quarter cylinder wall (face 4) given its tangent plane moved out 1.14, and
/// the cubic wall (face 2) its tangent plane at its middle. Each new plane meets the caps,
/// the flat wall beside them and the rational wall (face 5, an extrusion), whose ruled
/// extension past its end the moved planes reach.
#[test]
fn two_walls_given_planes_out_build_the_prism_of_their_profile() {
    walls_moved_build_their_prism(&[(4, 1.143_604_136_567_873_7, [0.0; 3]), (2, 0.0, [0.0; 3])]);
}

/// gen0021: the same two walls, both moved out (0.20 and 0.55).
#[test]
fn two_walls_moved_out_build_the_prism_of_their_profile() {
    walls_moved_build_their_prism(&[
        (4, 0.202_101_138_781_204_4, [0.0; 3]),
        (2, 0.548_365_023_570_791_1, [0.0; 3]),
    ]);
}

/// gen0048: the cubic wall (face 2) given its tangent plane at its middle.
#[test]
fn a_cubic_wall_given_its_tangent_plane_builds_the_prism_of_its_profile() {
    walls_moved_build_their_prism(&[(2, 0.0, [0.0; 3])]);
}

/// gen0041: the top cap (face 1) moved down 0.16 and tilted. The walls keep their surfaces
/// and meet it along their rulings (the two extrusions as nets); the body is the solid over the
/// bottom cap under the tilted plane, whose volume is the cap's area times the plane's
/// height over the cap's centroid (the height is linear), and sound.
#[test]
fn a_top_moved_down_and_tilted_holds_its_height_over_the_centroid() {
    let mut k = Kernel::default();
    let Some(mut b) = solid(&mut k, "extrusion.smb") else {
        return;
    };
    let bottom = cap_at(&k, &b, 0.0);
    let (area, centroid) = face_centroid(&k, &mut b, bottom, ACCURACY).unwrap();
    let tilt = [
        -0.095_716_453_310_687_6,
        0.014_193_158_954_830_198,
        -0.155_603_183_282_545_88,
    ];
    let planes = moved(&mut k, &mut b, &[(1, -0.163_195_178_504_810_47, tilt)]).unwrap();
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    // the plane's height over the centroid: (x − o)·n = 0 solved for z
    let (o, n) = planes[0];
    let height = o.z - ((centroid.x - o.x) * n.x + (centroid.y - o.y) * n.y) / n.z;
    let want = area * height;
    let v = volume(&mut k, &mut b).unwrap();
    assert!((v - want).abs() <= AGREE * want, "{v} vs {want}");
}

/// gen0076: the base of `Solids/pyramid.smb` (a plane) moved up 1.31 toward the apex. Two
/// of the sides are nets off their planes by the rounding of their stored points (single
/// precision: 1.9e-6); the base meets them on their nets. What is left is the pyramid above
/// the new base, similar to the old one: the volume times `((h − d) / h)³`, `h` the apex's
/// height over the old base, `d` the move.
#[test]
fn a_pyramid_base_moved_up_leaves_the_similar_pyramid_above_it() {
    let mut k = Kernel::default();
    let Some(mut b) = solid(&mut k, "pyramid.smb") else {
        return;
    };
    let before = volume(&mut k, &mut b).unwrap();
    let h = b
        .vertex_list()
        .iter()
        .map(|&v| solidcore::topology::vertex::point(&b, v).unwrap().z)
        .fold(f64::NEG_INFINITY, f64::max);
    let planes = moved(&mut k, &mut b, &[(0, 1.308_567_886_431_627_2, [0.0; 3])]).unwrap();
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let d = planes[0].0.z;
    assert!(d > 1.3 && d < 1.31, "{d}");
    let want = before * ((h - d) / h).powi(3);
    let v = volume(&mut k, &mut b).unwrap();
    assert!((v - want).abs() <= AGREE * want, "{v} vs {want}");
}

/// The cases of `Solids/nurb_fillet.smb` that can leave a non-manifold body once the plane
/// beside its curved nets has a reach: built only as a manifold solid of positive volume,
/// or refused with the input as it came in (its volume, faces and edges).
fn sound_or_refused_unchanged(moves: &[(usize, f64, [f64; 3])]) {
    let mut k = Kernel::default();
    let Some(mut b) = solid(&mut k, "nurb_fillet.smb") else {
        return;
    };
    let before = (
        volume(&mut k, &mut b).unwrap(),
        bp1::faces(&b, None).unwrap().len(),
        b.edge_list().len(),
    );
    match moved(&mut k, &mut b, moves) {
        Ok(_) => {
            assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
            assert!(volume(&mut k, &mut b).unwrap() > 0.0);
        }
        Err(e) => {
            let after = (
                volume(&mut k, &mut b).unwrap(),
                bp1::faces(&b, None).unwrap().len(),
                b.edge_list().len(),
            );
            assert_eq!(after, before, "{e}");
        }
    }
}

/// `nurb_fillet_face0_same` (and its `attributed_` twin): face 0 given its tangent plane.
#[test]
fn a_curved_net_given_its_tangent_plane_is_sound_or_refused_unchanged() {
    sound_or_refused_unchanged(&[(0, 0.0, [0.0; 3])]);
}

/// `nurb_fillet_face4_same` (and its `attributed_` twin): face 4 given its tangent plane.
#[test]
fn another_curved_net_given_its_tangent_plane_is_sound_or_refused_unchanged() {
    sound_or_refused_unchanged(&[(4, 0.0, [0.0; 3])]);
}

/// `fc0004`: faces 16 and 0 given tilted planes, 16's moved in 1.21.
#[test]
fn two_curved_nets_given_tilted_planes_are_sound_or_refused_unchanged() {
    sound_or_refused_unchanged(&[
        (
            16,
            -1.212_618_021_690_697_2,
            [
                -0.227_499_561_725_195_12,
                -0.119_235_175_885_283_43,
                0.212_040_256_859_765_76,
            ],
        ),
        (
            0,
            0.0,
            [
                0.195_822_407_983_586_08,
                -0.180_816_552_666_528_46,
                -0.250_316_266_099_896_8,
            ],
        ),
    ]);
}
