//! WR-607 (the fork's tangency-labs/smlib 661f79c): `SmBrep::GlueVertices` joins the pair
//! of shells of the last region its search finds both vertices in, and only that pair. A
//! solid's vertex is in two regions (a shell of the solid's region and one of the infinite
//! region's), so where the two vertices' parts lie apart in both regions, the glue leaves
//! one region's two shells apart.
//!
//! The case: two 10 mm cubes, x 0..10 and x 20..30, in one B-rep (`MergeBrep`), the second
//! cube's solid shell moved into the first cube's solid region, so each region holds one
//! shell of each cube (as after deleting a face that alone held two parts of a body
//! together). Gluing the first cube's vertex (10, 0, 0) and the second's (20, 0, 0) joins
//! the infinite region's pair only upstream: the vertex is then in three shells. With
//! `UpstreamFixes::glue_vertices_joins_every_region` both pairs are joined: two shells,
//! one per region. Two cubes left in their own solid regions share the infinite region
//! only and glue the same with the flag on and off.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::{p3 as bp3, p5};
use solidcore::topology::{shell, vertex, Brep, RegionId, ShellId, VertexId};
use solidcore::Kernel;

/// The vertex of `b` at `p`.
fn vertex_at(b: &Brep, p: Point3) -> VertexId {
    *b.vertex_list()
        .iter()
        .find(|&&v| b.vertex(v).unwrap().point == p)
        .unwrap()
}

/// The shell of `v` that is not in the infinite region.
fn solid_shell(b: &Brep, v: VertexId) -> (ShellId, RegionId) {
    let inf = b.infinite_region();
    vertex::shells(b, v)
        .unwrap()
        .into_iter()
        .find_map(|s| {
            let r = b.shell(s).unwrap().region;
            (r != inf).then(|| (s, r.unwrap()))
        })
        .unwrap()
}

/// The two cubes in one B-rep; with `share_solid_region` the second cube's solid shell
/// is moved into the first cube's solid region.
fn two_cubes(k: &mut Kernel, share_solid_region: bool) -> (Brep, VertexId, VertexId) {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let mut other = create_box(k, Point3::new(20.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let serial = bp3::notify_serial(&mut k.context, &mut other).unwrap();
    let source_key = move |e| bp3::topology_ref(serial, e);
    let other = &other;
    bp3::with_kernel_callbacks(k, &mut b, |k, b, env| {
        bp3::merge_brep(k, env, b, other, &source_key, None, None, false)
    })
    .unwrap();
    let a = vertex_at(&b, Point3::new(10.0, 0.0, 0.0));
    let d = vertex_at(&b, Point3::new(20.0, 0.0, 0.0));
    if share_solid_region {
        let (_, region_a) = solid_shell(&b, a);
        let (shell_d, region_d) = solid_shell(&b, d);
        assert_ne!(region_a, region_d);
        shell::region_remove_shell(&mut b, region_d, shell_d).unwrap();
        shell::region_post_insert_shell(&mut b, region_a, shell_d).unwrap();
    }
    b.set_editing_enabled(true);
    (b, a, d)
}

/// Glues the second cube's vertex onto the first's: the survivor's shells afterwards,
/// and how many shells each region of the survivor holds.
fn glue(fix: bool, share_solid_region: bool) -> (usize, Vec<usize>) {
    let mut k = Kernel::default();
    k.fixes.glue_vertices_joins_every_region = fix;
    let (mut b, a, d) = two_cubes(&mut k, share_solid_region);
    // the premise: each vertex is in a solid shell and an infinite-region shell
    assert_eq!(vertex::shells(&b, a).unwrap().len(), 2);
    assert_eq!(vertex::shells(&b, d).unwrap().len(), 2);
    p5::glue_vertices(&mut k, &mut b, Some(a), Some(d), None).unwrap();
    let shells = vertex::shells(&b, a).unwrap();
    let mut regions: Vec<RegionId> = Vec::new();
    for &s in &shells {
        let r = b.shell(s).unwrap().region.unwrap();
        if !regions.contains(&r) {
            regions.push(r);
        }
    }
    let per_region = regions
        .iter()
        .map(|&r| {
            shells
                .iter()
                .filter(|&&s| b.shell(s).unwrap().region == Some(r))
                .count()
        })
        .collect();
    (shells.len(), per_region)
}

#[test]
fn upstream_glue_leaves_one_regions_pair_apart() {
    let (shells, per_region) = glue(false, true);
    assert_eq!(shells, 3, "{per_region:?}");
    assert!(per_region.contains(&2), "{per_region:?}");
}

#[test]
fn glue_joins_the_pair_of_every_shared_region() {
    let (shells, per_region) = glue(true, true);
    assert_eq!(shells, 2, "{per_region:?}");
    assert!(per_region.iter().all(|&n| n == 1), "{per_region:?}");
}

#[test]
fn cubes_in_their_own_regions_glue_the_same_with_and_without_the_fix() {
    // only the infinite region is shared: one pair, joined either way
    let off = glue(false, false);
    let on = glue(true, false);
    assert_eq!(off, on);
    assert_eq!(off.0, 3);
}
