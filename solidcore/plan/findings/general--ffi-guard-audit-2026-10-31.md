# General: FFI guard audit, refusals checked against isolated native runs (2026-10-31)

**Units:** surface.surface_assert_valid, topology.assert_valid, primitive.api_trim_ops, primitive.api_primitives.

Each guard below refuses only inputs that crash, hang or read undefined values in the pinned C++. Each was checked in an isolated native run (one process per input, frame names only) or against the pinned source.

| Unit | Guard | Inputs | Status |
| --- | --- | --- | --- |
| surface.surface_assert_valid | MakeNurb of an extrusion with an empty V interval (empty, or empty to 1e-9 with swap) | 23 of the 86 collapsed extrusions | narrowed to exactly these; SIGSEGV, UB-282 |
| surface.surface_assert_valid | near-empty V interval without swap: continuity output never written | 13 of the 86 | kept; UB-273 addendum 2026-10-31 |
| surface.surface_assert_valid | the other 50 collapsed extrusions (a zero or near-zero extrusion vector, an empty or near-empty U interval, a near-empty V interval at SM_LEVEL_GIVEN) and 7 height, radius and sphere-radius refusals (heights and radii down to 3e-7, a cone's zero radius, a sphere radius of 8e-7) | 57 (50 + 7) | removed; defined, compared and passing (surface_assert_valid rejected 386 → 329) |
| topology.assert_valid | op 2 (bare level-2 tests), null face surface (8) or edge curve (9) | `l2_box_tests_m8/m9_rejected` | kept; SIGSEGV, UB-281 |
| topology.assert_valid | op 2, null edge use CCW link (10) | `l2_box_tests_m10_null_edgeuse_link` | narrowed out: no fault in an isolated run (cylinder) or on the box fixture, every CCW walk on the path checks for null; compared and passing (assert_valid rejected 3 → 2) |
| primitive.api_trim_ops | `Trim3d` loop counts summing past the curve count | none generated | exact, no change; UB-284 (out-of-bounds read in `SmBrep::MakeFaceWithCurves`) |
| primitive.api_primitives | zero-area pyramid-plane loop | `gen0042`, `gen0374`, `gen1211`, `gen1222`, `gen1291` | kept; UB-119 |
