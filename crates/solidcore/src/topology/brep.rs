//! The B-rep object (`SmBrep`): construction from serialized data, editing and
//! queries, split into the units `topology.brep_p1` .. `topology.brep_p10`.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/inc/SmBrep.h`.
//!
//! | Rust      | Upstream                                                        |
//! |-----------|-----------------------------------------------------------------|
//! | [`cutting`] | `SmBrepCutting`, `SmPlaneCutter`, `SmApiCut` (unit `topology.brep_cutting`) |
//! | [`local_operation`] | `LocalOperation`                                     |
//! | [`mirror`]| `Mirror` (unit `topology.brep_mirror`)                          |
//! | [`p1`]    | see the module                                                  |
//! | [`p2`]    | see the module                                                  |
//! | [`p4`]    | `~SmBrep`, `DeleteEdge`, `DeleteVertex`, `StitchFaces`, `ShrinkGeometry` |
//! | [`p5`]    | B-rep editing and transformation operations, part 5             |
//! | [`p6`]    | see the module                                                  |
//! | [`p7`]    | see the module                                                  |
//! | [`planar_section`] | `CreatePlanarSectionCurves` (unit `topology.brep_planar_section`) |
//! | [`p8`]    | making faces from surfaces and from loops of curves              |
//! | [`vertex_fans`] | `SplitVertexByFans`, `JoinVertexFans` (the fork's, `4ecfbab`) |
//! | [`p10`]   | `MakeTopologyFromData`, `ProjectAndTrim`, `RemoveAllTopology`, `TestClosureSplitRegion` (edge version) |
//!
//! Deviations: the B-rep records are the arenas of [`crate::topology::Brep`].

pub mod cutting;
pub mod local_operation;
pub mod mirror;
pub mod p1;
pub mod p10;
pub mod p2;
pub mod p3;
pub mod p4;
pub mod p5;
pub mod p6;
pub mod p7;
pub mod p8;
pub mod p9;
pub mod planar_section;
pub mod validation;
pub mod vertex_fans;

// Supported public paths; numbered modules are implementation partitions.
pub use p1::{create_parallel_projection_curves, edges, faces, regions, ParallelProjectionOptions};
pub use p10::{
    make_topology_from_data, merge_curves_on_surface_through_surface, MakeTopologyOptions,
    TopologyEnv, HEALER_VERSION,
};
pub use p2::{is_manifold_solid, is_sheet, shell_vertices, ArenaNotifyState};
pub use p3::{
    api_brep_copy, delete_face, new_brep, orient_trimmed_surfaces, topology_ref,
    with_kernel_callbacks, DeleteFaceOptions,
};
pub use p4::stitch_faces;
pub use p5::{
    make_edge_in_face, remove_topological_edges_and_vertices, stitch_and_orient, EdgeInFace,
    StitchAndOrientOptions,
};
pub use p7::copy_faces;
pub use p8::{
    create_face_from_surface, create_faces_with_curves, create_planar_face_with_3d_curves,
    create_planar_faces_with_3d_curves, FaceBuildEnv,
};
pub use p9::{make_vertex_loop, orient_sheet_surfaces};
