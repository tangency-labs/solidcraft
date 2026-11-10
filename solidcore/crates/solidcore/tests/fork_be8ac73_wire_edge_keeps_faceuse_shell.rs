//! WR-585 (the fork's tangency-labs/smlib@be8ac73): `SmBrep::MakeEdgeTopology` joins
//! the shells of a new edge's two vertices by moving the end vertex's shell into the
//! start vertex's. With a wire start shell and a faceuse end shell that moves every face
//! use of the faceuse shell (and passes over every edge of the B-rep for its wires).
//!
//! The repro: a 10 mm cube, a wire edge in its infinite region (from a vertex shell),
//! and a vertex shell after it, so the region lists `[outside of the cube, wire, vertex]`.
//! A new edge from the wire's free end to a cube corner joins the wire shell (start) to
//! the cube's outside faceuse shell (end). Upstream keeps the wire shell's object, which
//! takes the cube's six face uses; with
//! `UpstreamFixes::boolean_wire_edge_keeps_faceuse_shell` the cube's shell is kept and
//! takes the wires. Either way the region lists the joined shell first, then the vertex
//! shell, and the joined shell holds the same six face uses in the same order and both
//! wire edges.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::original_key;
use solidcore::geom::attribute::AttributeStore;
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::{
    make_edge_topology, make_edge_vertex_topology, make_shell_vertex_topology,
};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::shell::wire_edges;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, FaceuseId, ShellId};
use solidcore::Kernel;

struct Joined {
    /// the cube's outside shell and the wire shell, before the join
    cube_shell: ShellId,
    wire_shell: ShellId,
    vertex_shell: ShellId,
    cube_faceuses: Vec<FaceuseId>,
    /// the infinite region's shells after the join
    shells: Vec<ShellId>,
    /// the joined shell's face uses and wire edges (sorted)
    faceuses: Vec<FaceuseId>,
    wires: Vec<EdgeId>,
    new_edges: [EdgeId; 2],
}

fn join(fix: bool) -> Joined {
    let mut k = Kernel::default();
    k.fixes.boolean_wire_edge_keeps_faceuse_shell = fix;
    let mut b = create_box(&mut k, Vec3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let infinite = b.infinite_region().unwrap();
    assert_eq!(b.region(infinite).unwrap().shells.len(), 1);
    let cube_shell = b.region(infinite).unwrap().shells[0];
    assert!(b.shell(cube_shell).unwrap().is_faceuse_shell());
    let cube_faceuses = b.shell(cube_shell).unwrap().get_faceuses().unwrap();
    assert_eq!(cube_faceuses.len(), 6);
    let corner = b.edge(b.edges().iter().next().unwrap().0).unwrap().vertices[0];

    let (wire_shell, v0) = make_shell_vertex_topology(&mut b, None).unwrap();
    let (e1, v1) = make_edge_vertex_topology(&mut b, None, v0).unwrap();
    assert!(b.shell(wire_shell).unwrap().is_wire_shell());
    let (vertex_shell, _) = make_shell_vertex_topology(&mut b, None).unwrap();
    assert_eq!(
        b.region(infinite).unwrap().shells,
        vec![cube_shell, wire_shell, vertex_shell]
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
    // start: the wire's free end (wire shell); end: a cube corner (faceuse shell)
    let e2 = make_edge_topology(&mut k, &mut b, None, v1, corner, &mut env).unwrap();

    let shells = b.region(infinite).unwrap().shells.clone();
    let joined = shells[0];
    let faceuses = b.shell(joined).unwrap().get_faceuses().unwrap();
    let mut wires = wire_edges(&mut k, &b, joined).unwrap();
    wires.sort_unstable();
    Joined {
        cube_shell,
        wire_shell,
        vertex_shell,
        cube_faceuses,
        shells,
        faceuses,
        wires,
        new_edges: [e1, e2],
    }
}

fn check_common(j: &Joined) {
    assert_eq!(j.shells.len(), 2);
    assert_eq!(j.shells[1], j.vertex_shell);
    assert_eq!(j.faceuses, j.cube_faceuses);
    let mut expected = j.new_edges.to_vec();
    expected.sort_unstable();
    assert_eq!(j.wires, expected);
}

#[test]
fn upstream_keeps_the_wire_shell() {
    let j = join(false);
    check_common(&j);
    assert_eq!(j.shells[0], j.wire_shell);
    assert_ne!(j.shells[0], j.cube_shell);
}

#[test]
fn fix_keeps_the_faceuse_shell() {
    let j = join(true);
    check_common(&j);
    assert_eq!(j.shells[0], j.cube_shell);
    assert_ne!(j.shells[0], j.wire_shell);
}
