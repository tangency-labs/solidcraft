#### UB-273 addendum (2026-10-31): a second trigger, extrusions with a near-empty V interval

Updates: UB-273

The same unwritten-continuity read is reached by `SmSurfOfExtrusion` surfaces, not only curve-bounded ones. With `MakeNurb` applied, at any level but `SM_LEVEL_GIVEN`, an extrusion whose V interval is empty to 1e-9 but not zero, with the parameters not swapped, makes `CalculateContinuities` exit before writing its minimum. `sm_AssertTestSurface4` then reads that minimum. The Rust port refuses these inputs with "continuity output never written (upstream reads it uninitialised)" (`crates/solidcore/src/surface/surface/assert_valid.rs:531`).

13 generated `surface.surface_assert_valid` inputs (`ext0000`, `ext0106`, `ext0140`, `ext0197`, `ext0231`, `ext0241`, `ext0269`, `ext0282`, `ext0294`, `ext0296`, `ext0306`, `ext0334`, `ext0376`) stay guarded for this reason. The same inputs with the swap flag set fault instead (UB-282).
