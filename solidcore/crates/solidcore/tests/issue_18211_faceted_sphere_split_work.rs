//! Issue #18211: splitting a closed planar-faceted sphere at a plane deletes half its
//! faces and all interior edges/vertices of the deleted half.
//!
//! Deleting those faces and resulting wire edges one by one reconciled shell connectivity
//! per deleted face and per bridge wire edge, walking the surviving hemisphere O(N) times
//! (`O(N^2)` shell-traversal face visits and `O(N^2)` shell face-use list shifts, plus
//! another `O(N^2)` in `faceuse_shell_replacement` when closing the hemisphere with the
//! cutting plane). Batch face deletion reconciles each surviving shell once: shell
//! traversal visits each surviving face once (`O(N)`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::boolean::api_brep::boolean_intersection;
use solidcore::curve::curve::Curve;
use solidcore::heal::stitch::p2::stitch_into_solid;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::math::det;
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::primitive::api_trim_ops::create_planar_faces;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p3::{
    merge_brep, notify_serial, topology_ref, with_kernel_callbacks,
};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

/// Closed planar-faceted UV sphere of radius `r` with `nlat` latitude bands and
/// `2 * nlat` longitude sectors (`2 * nlat^2` facets), stitched into a manifold solid.
fn faceted_uv_sphere(k: &mut Kernel, nlat: usize, r: f64) -> Brep {
    let nlon = 2 * nlat;
    let ring_pt = |i: usize, j: usize| -> Point3 {
        let phi = PI * (i as f64) / (nlat as f64);
        let theta = 2.0 * PI * (j as f64 + 0.5) / (nlon as f64);
        let sp = det::sin(phi);
        Point3::new(
            r * sp * det::cos(theta),
            r * sp * det::sin(theta),
            r * det::cos(phi),
        )
    };
    let north = Point3::new(0.0, 0.0, r);
    let south = Point3::new(0.0, 0.0, -r);

    let mut polygons: Vec<Vec<Point3>> = Vec::new();
    for j in 0..nlon {
        let j1 = (j + 1) % nlon;
        polygons.push(vec![north, ring_pt(1, j), ring_pt(1, j1)]);
        for i in 1..(nlat - 1) {
            polygons.push(vec![
                ring_pt(i, j),
                ring_pt(i + 1, j),
                ring_pt(i + 1, j1),
                ring_pt(i, j1),
            ]);
        }
        polygons.push(vec![ring_pt(nlat - 1, j), south, ring_pt(nlat - 1, j1)]);
    }

    let mut combined: Option<Brep> = None;
    for poly in polygons {
        let mut edges = Vec::new();
        for idx in 0..poly.len() {
            let a = poly[idx];
            let b = poly[(idx + 1) % poly.len()];
            edges.push(Curve::from(create_line_segment(a, b).unwrap()));
        }
        let mut facet = create_planar_faces(k, &edges).unwrap();
        match &mut combined {
            None => combined = Some(facet),
            Some(acc) => {
                acc.set_editing_enabled(true);
                let serial = notify_serial(&mut k.context, &mut facet).unwrap();
                let source_key = move |e| topology_ref(serial, e);
                let facet = &facet;
                with_kernel_callbacks(k, acc, |k, b, env| {
                    merge_brep(k, env, b, facet, &source_key, None, None, false)
                })
                .unwrap();
            }
        }
    }
    let mut b = combined.unwrap();
    let stitched = stitch_into_solid(k, &mut b).unwrap();
    assert_eq!(stitched.lamina_edges, 0);
    assert!(stitched.produces_a_solid);
    b
}

/// Splits a faceted UV sphere of `2 * nlat^2` facets against the `y >= 0` half-space box
/// and returns `(shell_traversal_faces, [faces, edges, vertices], volume)`.
fn split_faceted_sphere(nlat: usize) -> (u64, [usize; 3], f64) {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let mut sphere = faceted_uv_sphere(&mut k, nlat, 10.0);
    let full_vol = brep_volume_default(&k, &mut sphere, 1e-6).unwrap();
    let half_box = create_box(&mut k, Point3::new(-60.0, 0.0, -60.0), 120.0, 60.0, 120.0).unwrap();

    k.work_counters.reset();
    let mut half = boolean_intersection(&mut k, sphere, half_box).unwrap();
    let marked = k.work_counters.shell_traversal_faces();

    half.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut half, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "nlat={nlat}: {lines:?}");
    assert!(is_manifold_solid(&half).unwrap(), "nlat={nlat} not solid");

    let counts = [
        p1::faces(&half, None).unwrap().len(),
        p1::edges(&half, None).unwrap().len(),
        p1::vertices(&half, None).unwrap().len(),
    ];
    let vol = brep_volume_default(&k, &mut half, 1e-6).unwrap();
    assert!(
        (vol - 0.5 * full_vol).abs() <= 1e-4 * full_vol,
        "nlat={nlat}: half volume {vol} vs expected {}",
        0.5 * full_vol
    );
    println!(
        "faceted sphere nlat={nlat} ({} faces): {marked} shell traversal faces, out={counts:?}, vol={vol:.3}",
        2 * nlat * nlat
    );
    (marked, counts, vol)
}

#[test]
fn faceted_sphere_plane_split_walks_each_surviving_face_once() {
    let (m8, c8, _) = split_faceted_sphere(8);
    let (m12, c12, _) = split_faceted_sphere(12);
    // Exact surviving hemisphere topology: nlat^2 surviving + nlat cut + 1 cap face.
    assert_eq!(c8[0], 73, "nlat=8 faces: {c8:?}");
    assert_eq!(c12[0], 157, "nlat=12 faces: {c12:?}");
    // Batch deletion reconciles each surviving shell once: 72 surviving sphere faces + 1
    // surviving half-box cap = 73 for nlat=8, and 156 + 1 = 157 for nlat=12 (linear O(N),
    // never O(N^2)).
    assert_eq!(m8, 73, "nlat=8 shell traversal faces");
    assert_eq!(m12, 157, "nlat=12 shell traversal faces");
}
