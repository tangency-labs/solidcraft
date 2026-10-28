Updates: plan/findings/heal.heal_data_p2--own-reader-kept-on-import.md

# heal.heal_data_p1/p2/p3: Rust input now built by the Rust kernel (2026-10-28)

Units: `heal.heal_data_p2` (and `heal.heal_data_p1`, `heal.heal_data_p3`, whose heal
cases run through the same runner function and shim entry point).

The heal cases' Rust side no longer imports the C++ export. `own_brep` in
`crates/difftest/src/units/heal_heal_data_p2.rs` builds the B-rep with the Rust kernel
in the shim's call order: `.smb` files with the Rust reader and the fixture kinds
(wire, face, network, ellipse face) through `own_builder::build_any`; the box with
`PrimitiveCreation::create_box`; the sphere face with
`create_face_in_region_from_surface`; the one-vertex-loop face (block kind 6, on a
sphere, plane or torus) record by record, as `SmBrepConstructor` writes it, followed by
its `EndShell(TRUE)` stitch; and the seam-crossing cone patch (block kind 7, `prog_test`
`my_test_move_seam_refreshes_face_props`) with `BSplineSurface::create_cone_patch`,
`create_face_from_surface`, `replace_surface` with the turned full patch and a cleared
rectangular-trim flag. The result is renumbered into the shim's traversal order (the
`sc_topology_fixture.h` walk with vertex lists). The shim's `init_cached_gaps` is the
fixture's gap walk over the same order (with `GetVertexEdgeGap`), so the Rust side runs
`own_builder::init_gap_walk` again after the fixture kinds' own walk, as the shim does.
The vertex-list/cached-gap channel (`VertexLists`, `restore`) has been removed.

The shim's `normalise` (which replaced surfaces and edge curves that the shared export
could not carry with plain B-spline copies) has been dropped, so both sides heal native
geometry. Before it was dropped, an instrumented run of all three units' fixture and
generated cases called it 4581 times and it replaced no surface or curve. Every source
in the corpus was already plain, a plane or line, or exactly exportable. The C++ and
Rust per-case output digests (`DIFFTEST_RUST_DIGEST`, `.digest` and `.cppdigest`)
are bit-identical before and after for all 741 + 495 + 480 cases. Case ids, outcomes
and `values_compared` match per case. No known-mismatch entry was needed.

After main gained the kind 6 plane/torus variants and kind 7 (PR #1289), the count-only
instrumentation was run again over all three units, including those new cases: 3532
calls (p1 963, p2 1163, p3 1406), no surface or curve that `normalise` would replace
(the canonical torus and sphere are exactly exportable; the cone patches are plain
B-splines). Cases, outcomes and per-case `values_compared` match main's reports for
all 495 + 741 + 702 cases (p3: 658 passed, 44 rejected).
