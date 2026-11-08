//! WR-569 (the fork's tangency-labs/smlib@351c57c): `SmStitch::StitchIntoSolid` finds
//! the infinite region only, so a closed shell nested inside another stays in the region
//! the gluing put it in and the region inside it stays material.
//!
//! The case: twelve separate square faces, the six of the cube -5..5 and the six of the
//! cube -2..2, stitched into a solid. With `Kernel::fixes.stitch_solid_nests_cavities`
//! the regions are set as a nested solid model: the small cube is the cavity, and the
//! volume is 10^3 - 4^3 = 936.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::geom::vector3d::Vec3;
use solidcore::heal::stitch::p2::stitch_into_solid;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point2;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::topology_traverser::Marks;
use solidcore::{Brep, Kernel};

/// Adds the six faces of the cube of half side `h` centred on the origin to `b`, each a
/// separate face of the infinite region over the square `-h..h` of its plane.
fn add_cube_faces(k: &mut Kernel, b: &mut Brep, h: f64) {
    let axes = [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ];
    for a in axes {
        for s in [1.0, -1.0] {
            let n = Vec3::new(a.x * s, a.y * s, a.z * s);
            let o = Vec3::new(n.x * h, n.y * h, n.z * h);
            let surface = Surface::from(Plane::from_normal(o, n));
            let mut state = ArenaNotifyState::new(b);
            let mut env = state.env(None);
            let mut marks = Marks::default();
            let this_face = |face| arena_identity(CacheObject::Face(face));
            let mut fenv = FaceBuildEnv {
                env: &mut env,
                marks: &mut marks,
                this_face: &this_face,
            };
            let domain = Extent2d::from_points(Point2::new(-h, -h), Point2::new(h, h));
            create_face_from_surface(k, b, Some(surface), &domain, &mut fenv).unwrap();
        }
    }
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// Stitches the hollow cube; the kernel, the stitched B-rep and the stitch's solid flag.
fn stitched(fix: bool) -> (Kernel, Brep, bool) {
    let mut k = Kernel::default();
    k.fixes.stitch_solid_nests_cavities = fix;
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    add_cube_faces(&mut k, &mut b, 5.0);
    add_cube_faces(&mut k, &mut b, 2.0);
    let r = stitch_into_solid(&mut k, &mut b).unwrap();
    assert_eq!(r.lamina_edges, 0);
    (k, b, r.produces_a_solid)
}

/// Each region's void flag, infinite region first.
fn void_flags(b: &Brep) -> Vec<bool> {
    bp1::regions(b, None)
        .unwrap()
        .iter()
        .map(|&r| b.region(r).unwrap().is_void != 0)
        .collect()
}

#[test]
fn upstream_keeps_the_cavity_material() {
    let (k, mut b, solid) = stitched(false);
    assert!(solid);
    let flags = void_flags(&b);
    assert_eq!(flags.len(), 3);
    assert!(flags[0]);
    // the cavity's region stays material: both cubes count as solid
    assert_eq!(flags.iter().filter(|&&v| v).count(), 1);
    let v = volume(&k, &mut b);
    // the cavity counts as material: the whole cube -5..5
    assert!((v - 1000.0).abs() <= 1e-6 * 1000.0, "volume {v}");
}

#[test]
fn nested_solid_makes_the_cavity_void() {
    let (k, mut b, solid) = stitched(true);
    assert!(solid);
    let flags = void_flags(&b);
    // the infinite region and the cavity are void, the material between them is not
    assert_eq!(flags.len(), 3);
    assert!(flags[0]);
    assert_eq!(flags.iter().filter(|&&v| v).count(), 2);
    let v = volume(&k, &mut b);
    assert!((v - 936.0).abs() <= 1e-6 * 936.0, "volume {v}");
}
