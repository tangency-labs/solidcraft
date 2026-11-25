//! WR-569 (the fork's tangency-labs/smlib@351c57c): `SmStitch::StitchIntoSolid` finds
//! the infinite region only, so a closed shell nested inside another stays in the region
//! the gluing put it in and the region inside it stays material.
//!
//! The case: twelve separate square faces, the six of the cube -5..5 and the six of the
//! cube -2..2, stitched into a solid. solidcore sets the regions as a nested solid model: the small cube is the cavity, and the
//! volume is 10^3 - 4^3 = 936.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::geom::vector3d::Vec3;
use solidcore::heal::stitch::p2::stitch_into_solid;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, PI};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::query::face::p1::calculate_shell_bounding_box;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{edge, vertex};
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
fn stitched() -> (Kernel, Brep, bool) {
    let mut k = Kernel::default();
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
fn nested_solid_makes_the_cavity_void() {
    let (k, mut b, solid) = stitched();
    assert!(solid);
    let flags = void_flags(&b);
    // the infinite region and the cavity are void, the material between them is not
    assert_eq!(flags.len(), 3);
    assert!(flags[0]);
    assert_eq!(flags.iter().filter(|&&v| v).count(), 2);
    let v = volume(&k, &mut b);
    assert!((v - 936.0).abs() <= 1e-6 * 936.0, "volume {v}");
}

/// A Ø20 x 20 cylinder with a Ø2 x 4 blind hole in its top face and a Ø20 x 5 cylinder joined
/// on top, which seals the hole into a cavity (tangency-labs/solidcraft#18287), rounded with r 2
/// on its bottom rim. The round opens the solid and its stitch closes it again; the split that
/// closes it gives the outside to a new region, and the solid's region kept the cavity's shell
/// ahead of the one enclosing it (WR-18287). The solid's region lists the outer shell (20 wide)
/// first, then the cavity's (2 wide), and the volume is the body's less the round's ring
/// (Pappus: the spandrel (1 - pi/4) r^2, its centroid (10 - 3 pi)/(12 - 3 pi) r in from the rim).
#[test]
fn a_round_on_a_solid_with_a_cavity_keeps_the_outer_shell_first() {
    let mut k = Kernel::default();
    let origin = |x: f64, z: f64| Point3::new(x, 0.0, z);
    let body = create_cylinder(&mut k, origin(0.0, 0.0), 10.0, 20.0).unwrap();
    let hole = create_cylinder(&mut k, origin(-4.0, 16.0), 1.0, 4.0).unwrap();
    let cap = create_cylinder(&mut k, origin(0.0, 20.0), 10.0, 5.0).unwrap();
    let body = boolean_difference(&mut k, body, hole).unwrap();
    let mut b = boolean_union(&mut k, body, cap).unwrap();
    // the bottom rim: the one edge whose ends lie at z = 0
    let rim: Vec<_> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            [s, t]
                .iter()
                .all(|v| vertex::point(&b, v.unwrap()).unwrap().z.abs() < 1e-9)
        })
        .collect();
    assert_eq!(rim.len(), 1);
    let r = 2.0;
    fillet_edges(&mut k, &mut b, &rim, r, 1, 1, 1.0).unwrap();

    let regions = bp1::regions(&b, None).unwrap();
    let void: Vec<bool> = regions
        .iter()
        .map(|&g| b.region(g).unwrap().is_void != 0)
        .collect();
    assert_eq!(void.iter().filter(|&&v| v).count(), 2, "{void:?}");
    let material = regions
        .iter()
        .find(|&&g| b.region(g).unwrap().is_void == 0)
        .unwrap();
    let widths: Vec<f64> = b
        .region(*material)
        .unwrap()
        .shells
        .clone()
        .into_iter()
        .map(|s| {
            calculate_shell_bounding_box(&mut k, &b, s, true)
                .unwrap()
                .size()
                .x
                .round()
        })
        .collect();
    assert_eq!(widths, [20.0, 2.0]);
    let inset = (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI) * r;
    let want = PI * 100.0 * 25.0 - PI * 4.0 - 2.0 * PI * (10.0 - inset) * (1.0 - PI / 4.0) * r * r;
    let v = volume(&k, &mut b);
    assert!((v - want).abs() <= 1e-6 * want, "volume {v}, want {want}");
}
