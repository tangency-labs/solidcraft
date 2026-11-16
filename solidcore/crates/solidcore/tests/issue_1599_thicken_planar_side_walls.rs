//! tangency-labs/solidcore#1599 (WR-564; the fork's tangency-labs/smlib#62): thickening a
//! sheet builds each side wall as the ruled surface between a lamina edge and its offset
//! (`BuildSideWallOfEdgeuse` / `CreateEdgeEdgeBlend`), and never puts a flat one on its
//! plane. A cylinder sheet (r 10, 20 high) thickened by +2 or -2 is a tube whose end rings
//! are each the line between the circle and its offset turned about the axis: flat, but
//! an `SmSurfOfRevolution`, not an `SmPlane`.
//!
//! Now (the former flag `thicken_planar_side_walls`) every planar wall goes onto its plane
//! and the ring's seam (the old surface's, now a slit through the ring) is deleted, so
//! each end is an `SmPlane` ring with two loops, and the volume is π (R² − r²) 20 (to the
//! 1e-6 accuracy the volume integration is asked for; it comes out within 6e-9).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{original_key, volume, Notifications};
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol;
use solidcore::math::{Point2, Vec3};
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::cone::p1::Cone;
use solidcore::surface::plane::p1::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::topology_traverser::Marks;
use solidcore::{Brep, EntityRef, Kernel};

/// A sheet of one `SmCylinder` face, radius 10 about z from z = 0 to z = 20
/// (`CreateFaceFromSurface` over its natural domain: closed round along a seam edge).
fn cylinder_sheet(k: &mut Kernel) -> Brep {
    let mut b =
        solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT).unwrap();
    let cylinder = Cone::new_cylinder_bounded(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        10.0,
        0.0,
        360.0,
        20.0,
        false,
        false,
        false,
    );
    let surface = Surface::from(cylinder);
    let d = surface.natural_uv_domain_with_nurb(k).unwrap();
    let domain = Extent2d::from_points(
        Point2::new(d.min().x, d.min().y),
        Point2::new(d.max().x, d.max().y),
    );
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key_of = original_key;
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let mut marks = Marks::default();
    let this_face = |f| original_key(EntityRef::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    create_face_from_surface(k, &mut b, Some(surface), &domain, &mut fenv).unwrap();
    b
}

fn thicken(distance: f64) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    let mut sheet = cylinder_sheet(&mut k);
    assert_eq!(sheet.face_list().len(), 1);
    let mut notifications = Notifications::new(&mut sheet);
    let shell = OffsetExecutive::shell_brep(
        &mut k,
        sheet,
        ShellOptions {
            offset: OffsetOptions {
                distance,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: false,
        },
        &[],
        &mut notifications.context(),
    )
    .result
    .unwrap();
    (k, shell)
}

/// The faces' classes, sorted, and each plane face's loop count.
fn kinds(b: &Brep) -> (Vec<&'static str>, Vec<usize>) {
    let mut classes = Vec::new();
    let mut plane_loops = Vec::new();
    for &f in b.face_list() {
        let s = b.face(f).unwrap().surface.as_ref().unwrap();
        classes.push(s.class_string());
        if s.is_kind_of(Plane::TYPE).unwrap() {
            plane_loops.push(
                solidcore::topology::face::p2::loops(b, f, false, None)
                    .unwrap()
                    .len(),
            );
        }
    }
    classes.sort_unstable();
    (classes, plane_loops)
}

fn expected_volume(distance: f64) -> f64 {
    let (r, big) = if distance > 0.0 {
        (10.0, 10.0 + distance)
    } else {
        (10.0 + distance, 10.0)
    };
    std::f64::consts::PI * (big * big - r * r) * 20.0
}

#[test]
fn issue_1599_tube_end_rings_are_planes() {
    for distance in [2.0, -2.0] {
        let (k, mut tube) = thicken(distance);
        let (classes, plane_loops) = kinds(&tube);
        assert_eq!(
            classes.iter().filter(|c| **c == "SmPlane").count(),
            2,
            "{distance}: {classes:?}"
        );
        assert_eq!(classes.len(), 4, "{distance}: {classes:?}");
        assert_eq!(plane_loops, vec![2, 2], "{distance}");
        assert!(brep_is_manifold_solid(&tube).unwrap());
        let v = volume(&k, &mut tube).unwrap();
        let want = expected_volume(distance);
        assert!(
            (v - want).abs() <= 1e-7 * want,
            "{distance}: volume {v} want {want}"
        );
    }
}

/// A sheet of one `SmPlane` face, 2 x 3 at z = 1 (`CreateFaceFromSurface` over the
/// domain).
fn plane_sheet(k: &mut Kernel) -> Brep {
    let mut b =
        solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT).unwrap();
    let domain = Extent2d::from_bounds(0.0, 0.0, 2.0, 3.0);
    let surface = Surface::from(Plane::new(
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Point2::new(1.0, 1.0),
        &domain,
    ));
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key_of = original_key;
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let mut marks = Marks::default();
    let this_face = |f| original_key(EntityRef::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    create_face_from_surface(k, &mut b, Some(surface), &domain, &mut fenv).unwrap();
    b
}

/// A 2 x 3 plane sheet thickened by 3 to 100 times the body's tolerance: the offset's
/// seams are open at the body's tolerance, so the merge's stitch at that tolerance closes
/// no region, and it widens as upstream's does (WR-455). The result is the slab, a
/// manifold solid of six faces and volume 6 t. (Stitching at the body's tolerance alone,
/// the former opt-in, left an open body; the default kernel widened already.) At 100
/// times the tolerance the slab passes `check_brep` at every level. At 3 and 10 times the
/// widened stitch joins the side walls' edges to the faces' without raising their
/// tolerance over the gap (Test #4, an edge/surface gap above the edge's tolerance),
/// as upstream's stitch does; that is the stitch's, not checked here.
#[test]
fn thin_thickened_sheet_is_a_closed_slab() {
    for times in [3.0, 10.0, 100.0] {
        let mut k = Kernel::default();
        let mut sheet = plane_sheet(&mut k);
        let t = times * sheet.tolerance();
        let mut notifications = Notifications::new(&mut sheet);
        let mut slab = OffsetExecutive::shell_brep(
            &mut k,
            sheet,
            ShellOptions {
                offset: OffsetOptions {
                    distance: t,
                    extend_convex_edges: true,
                    merge_results: false,
                },
                create_offset_solid: true,
                merge_sheet_side_walls: false,
            },
            &[],
            &mut notifications.context(),
        )
        .result
        .unwrap();
        slab.refresh_cached_topology();
        assert!(brep_is_manifold_solid(&slab).unwrap(), "{t}");
        assert_eq!(slab.face_list().len(), 6, "{t}");
        if times >= 100.0 {
            for level in [CheckLevel::Level0, CheckLevel::Level1, CheckLevel::Level2] {
                let checked = check_brep(&mut k, &mut slab, level).unwrap();
                assert!(checked.is_valid(), "{t} {level:?}: {:?}", checked.lines());
            }
        }
        let v = volume(&k, &mut slab).unwrap();
        assert!((v - 6.0 * t).abs() <= 1e-6 * 6.0 * t, "{t}: volume {v}");
    }
}
