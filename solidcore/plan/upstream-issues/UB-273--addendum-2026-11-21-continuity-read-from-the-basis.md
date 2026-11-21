Updates: UB-273

#### UB-273 (update, 2026-11-21). The inherited check reads the basis's continuities

- **Status in the port:** the "Rust port" line of UB-273 ("Preserve the generic continuity failure as `InvalidInput`") no longer holds. A curve-bounded surface answers `GetKnots`, and so `CalculateContinuities`, from its basis surface (WR-20163, the curve-bounded record), so the inherited `SmSurface::AssertValid` at levels 0..2 reads the basis's continuities, which are written.
- **What changes:** in `surface.surface_assert_valid`, 106 curve-bounded cases lose the inherited check's reports that came from the unwritten continuities ("Bad Surf Coincidence at Seam", "Bad Surf Discontinuity", and their default `Unknown_Assert` label); 49 of them become valid, those reports having been their only ones. No report is added. The boundary-curve reports and the default-label handling of failed boundary tests are unchanged, and the bare basis surface's own check makes none of the removed reports.
- **Location:** `crates/solidcore/src/surface/surface/assert_valid.rs`; `crates/goldens/src/units/surface_surface_assert_valid.rs` (its module doc).
- **Regression evidence:** `surface::surface::assert_valid::tests::curve_bounded_given_is_defined_and_test_4_reads_the_basis_continuities`.
