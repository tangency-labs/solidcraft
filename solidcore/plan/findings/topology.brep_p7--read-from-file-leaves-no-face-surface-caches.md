# topology.brep_p7: the Rust `ReadFromFile` leaves no face-surface caches where the C++ read leaves some (2026-10-28)

Unit: `topology.brep_p7` (`topology::brep::p7::read_from_file`), seen through
`offset.offset_geometry_creation`.

**How found.** The offset runner used to copy each face surface's cache state from the
C++ export onto its imported B-reps (`seed_cache_tree_states`). That is C++ process state
reaching the Rust side (`crates/difftest/README.md`, No C++ state reaches the Rust side),
so it was removed, and the runner now reads `.smb` file sources with the Rust reader
(`crate::own_builder::read_with`, renumbered into the shim's walk order).

**Effect.** One case changes: `gen0266` (generated, seed 9515163603058763022; B-rep A
`Mouse.smb`, B `Endcap.smb`, both read without rebuild). It is the case of
`plan/findings/surface.surface_p11--surface-cache-history-decides-liftcurve-s-c1-domain-off-the.md`.
Op 6, `ExtendTrimEdges` (record index 100): C++ `SM_SUCCESS` (the offset edge's curve
gets 21 control points), Rust `SM_ERR` (1001; the edge keeps its 11-point curve). The
records then differ in length (C++ 284, Rust 1776). It is the same result whether the
Rust side imports the C++ export without seeding or reads the file itself. Every other
case of the unit is unchanged (909 pass, 16 rejected).

**Both states (temporary probe, not committed).** Face-surface cache states after the
read, counted as (no cache, cache with unbuilt tree, cache with built tree):
- `Mouse.smb`: C++ (1, 0, 3); Rust (4, 0, 0).
- `Endcap.smb`: C++ (0, 1, 0); Rust (1, 0, 0).

**Suspected cause.** The C++ read (`MakeTopologyFromData` while the context does a
Boolean operation, then `sm_CheckAndFixUVTrimCurves`) calls `smsurf_GetSurfaceCache` on
face surfaces and leaves those caches behind: some with their trees built, and some made
during the Boolean operation whose trees are never built. The port's read leaves every
face surface with no cache. Later, `GetDomainWithContinuity` on Endcap's surface reads
the unbuilt tree's empty continuity arrays in C++ (the natural domain) and finds the
end-knot discontinuities in Rust (an empty domain), so `LiftCurve` fails on the Rust
side only (the mechanism in the surface_p11 finding above). The port models cache states
(`Surface::cache_tree_state`), but `read_from_file` and its callees do not record the
caches the upstream read makes.

**Status.** Rust divergence (not a C++ defect). Listed in
`crates/difftest/known_mismatches/offset.offset_geometry_creation.toml`. The fix is to
make the port's read path leave the same cache states upstream's does (topology.brep_p7
`read_from_file` / `make_topology_from_data` / `check_and_fix_uv_trim_curves`, through
`query::face::p2::face_surface_cache`). That would also bring every other runner that
reads files natively in line with C++.
