//! UB-241 on tangency-labs/solidcore#2174's sheet: the r 10 sphere's face above z = 5,
//! taken alone, thickened outward (+1) as `SmApiShellBrepFull` does (a solid, extended
//! convex edges, merged results).
//!
//! The pinned C++ crashes there (SIGSEGV; `AddressSanitizer`: `sm_RmFromSpatialTree` via
//! `SmBrep::GlueVertices`). In the result merge, `GlueCoincidentBrepVertices` meets two
//! coincident Brep 1 vertices joined by more than one long common edge. It glues them
//! for the first edge, which deletes the second vertex, and glues them again for the next
//! (`plan/upstream-issues/UB-241--addendum-2026-11-01-*.md`). The port stops after the
//! first glue (the fork's da8d05e, unconditional). It reaches that stop on this input
//! once, for Brep 1, and returns a result.
//!
//! At the defaults that result is upstream's WR-673 one: a body of 4 faces, 8 edges and
//! 5 vertices that is not a manifold solid (one rim half got the whole offset circle, the
//! other nothing). `Kernel::fixes.offset_open_edge_second_trim_point` gives the sound
//! sector (`issue_2174_cap_rim_two_edges.rs`). Both shell entry points are checked:
//! `api_shell_brep_full`'s options (no merged sheet side walls; the difftest case
//! `issue2174_cap_two_rim_edges_api_shell_out`) and the issue's
//! (`merge_sheet_side_walls`).
//!
//! The stop is not what decides this result: with it removed the port glues through a
//! vertex its arena has already dropped, which is a soft no-op, and returns the same
//! body. The stop itself is tested in `topology::proto_topology::p3`
//! (`glue_stops_once_the_second_vertex_is_glued_away`). This test pins the Rust result
//! on the input that crashes the C++: no panic, no hang, the same body.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_intersection;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::offset::offset_executive::p2::{shell_brep_simple, OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{remove_faces, transform};
use solidcore::topology::{Brep, FaceId};

/// The issue's repro (and the difftest fixture's sheet): the ball intersected with a box
/// over z >= 5 turned 90 degrees about z, its flat face removed.
fn cap(k: &mut Kernel) -> Brep {
    let ball = create_sphere(k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    let mut above = create_box(k, Point3::new(-15.0, -15.0, 5.0), 30.0, 30.0, 10.0).unwrap();
    let turn = Axis2Placement::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
    );
    transform(k, &mut above, &turn, None, false).unwrap();
    let mut c = boolean_intersection(k, ball, above).unwrap();
    let flat: Vec<FaceId> = c
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            matches!(
                c.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::Plane(_)
            )
        })
        .collect();
    assert_eq!(flat.len(), 1);
    c.set_editing_enabled(true);
    remove_faces(k, &mut c, true, &flat).unwrap();
    c.refresh_cached_topology();
    assert_eq!(c.face_list().len(), 1);
    c
}

fn thicken_out(merge_sheet_side_walls: bool) -> Brep {
    let mut k = Kernel::default();
    let sheet = cap(&mut k);
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: 1.0,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls,
    };
    let mut b = shell_brep_simple(&mut k, &sheet, options, &[])
        .unwrap_or_else(|e| panic!("side walls {merge_sheet_side_walls}: {e}"));
    b.refresh_cached_topology();
    b
}

#[test]
fn outward_thicken_returns_upstreams_body() {
    for side_walls in [false, true] {
        let b = thicken_out(side_walls);
        let counts = (
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len(),
        );
        assert_eq!(counts, (4, 8, 5), "side walls {side_walls}");
        assert!(!is_manifold_solid(&b).unwrap(), "side walls {side_walls}");
    }
}
