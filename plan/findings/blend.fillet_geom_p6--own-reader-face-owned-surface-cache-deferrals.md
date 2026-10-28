# blend.fillet_geom_p6: own-reader targets reach the face-handle stub of the surface cache (2026-10-28)

Unit: `blend.fillet_geom_p6` (the stub belongs to `topology.face_p2`).

Inputs: 37 of the 400 generated cases, `.smb` targets only: `cone.smb` 9, `vase_rev.smb`
8, `Endcap.smb` 7, `torus.smb` 6, `Mouse.smb` 6, `box_with_large_fillet.smb` 1 (for
example `gen0011` on `vase_rev.smb`).

Before (and on this PR): all 400 cases pass. The Rust target is imported from the C++ export, and the
import leaves the face surfaces without an owner.

With the switch (tried, not merged): the Rust side read the target with the Rust reader and renumbered it into the
shim's traversal order (`crate::own_reader`). Each face then owned its surface, as it does
in the C++ B-rep. In these 37 cases the op's surface evaluation calls
`get_surface_cache` (`smsurf_GetSurfaceCache`) on a face-owned surface with no face
handle bound, so `SurfaceCache::face` returns
`Unported { "SmFace handle of a surface's face", "topology.face_p2" }`
(`crates/solidcore/src/surface/surface_cache/p1.rs`). The cases were reported as deferred
under `topology.face_p2`. The regression ratchet rejected that (37 baseline passes became
deferred), so the runner stays on the C++ export import for now. Before the switch they passed only because the imported
surfaces had no owner, so the Rust side took the unowned-surface branch while the C++
side took the face-owned one.

No values disagreed. This is coverage the import hides, not a mismatch. The C++
results were unchanged.

To resolve: give the surface cache the face handle of a face-owned surface (the
`topology.face_p2` binding, `bind_trimmed_face`) on these paths. Then switch the runner's Rust
target to `crate::own_reader::edgeuse_graph_brep`, as the other blend runners do.

PR: difftest own-reader rollout, group E (blend units).
