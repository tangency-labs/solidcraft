# blend.hcr: the Rust-built scene reaches TrimSrfCache::build_tree_with_face in IsPointVisible (2026-10-28)

Unit: `blend.hcr` (runner `crates/difftest/src/units/blend_hcr.rs`).

Change: the runner's Rust side no longer imports the C++ scene (the `SmHCR` scene copy,
its transform and its `CalculateBoundingBox`). It builds the B-rep block with the Rust
kernel (`.smb` files with the Rust reader), adds the extra wire edge, constructs the HCR
with `Hcr::new` / `Hcr::with_transform` (the Rust `copy_brep`, transform, infinite region
and bounding box) and renumbers the scene into the shim's traversal order.

Effect: 434 cases, 431 pass, 3 deferred (before: 434 pass). The three cases are
`ComputeEdgeVisibility` (op `edge_visibility`) on `.smb` file scenes (`gen0103`,
`gen0253`, `gen0320`; for example `gen0103`: `Mouse.smb`, view
`(0.0936, -0.5806, -0.8407)`, `approx_tol 1e-6`, `ang_tol 0.2`). The C++ side completes
them, as it did before. The Rust side returns
`SolidError::Unported { upstream: "TrimSrfCache::build_tree_with_face for a face-owned
surface", unit: "surface.trim_srf_cache" }` from `SurfaceCache::get_tree`
(`surface/surface_cache/p3.rs`), called by `Hcr::is_point_visible` from
`compute_curve_visibility` from `compute_edge_visibility`. So the cases are deferred,
not failed. Nothing was compared for them (before, each compared 25 values).

Suspected cause: with the imported scene, the ray surfaces of these faces took a path
that never built a face-owned trimmed cache through `get_tree`. The scene the Rust kernel
builds itself (native surfaces and UV trim state from the Rust reader, copied by
`copy_brep`) makes `IsPointVisible` list the faces as face-owned `RaySurface::Face`
entries, which then need the unported face-owned trimmed tree build. This is a gap in
the library, not in the harness. The cases should run again once
`TrimSrfCache::build_tree_with_face` is ported for face-owned surfaces.

Also: `gen0110` (a face over an out-of-box B-spline at 1e-7 scale) fails to build on both
sides. The C++ side now reports its own build failure as `brep_build_status_<status>`
(from the `topology.vertexuse` fixture export, C++ side only), matching the Rust
build's label, instead of the FFI's `fixture rejected`.

PR: difftest: blend.hcr — Rust-side Hcr scene instead of the C++ export (stacked on #1375).
