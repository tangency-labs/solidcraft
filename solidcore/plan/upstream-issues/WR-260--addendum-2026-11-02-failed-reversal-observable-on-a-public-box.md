Updates: WR-260

#### WR-260 (update, 2026-11-02). The failed-reversal guard is observable on a public box edge

The `Kernel::fixes.edge_reversal_keeps_failed_edge` guard of `topology::edge::reverse_orientation` was isolated against a public multi-face body instead of the synthetic wire of the routine-level `issue_521` test.

- **Failure condition (unchanged):** the `issue_521` condition, a curve whose `ReverseParameterization` is `SmCurve`'s base `SM_ERR` (1001); the run uses a `HermiteCurve`, the same kind that test installs.
- **Body:** `primitive::api_primitives::create_box` `10 x 20 x 30` (6 faces, 12 edges). Its first edge (`interval [0,20]`, shared by two faces, 4 edge uses) gets UV trims on every use through `topology::edgeuse::p3::get_or_create_uv_trim_curve`, then its curve is replaced by the Hermite curve through the public edge record.
- **Measured, default (`false`), upstream behaviour:** `reverse_orientation` returns `Ok(())`; interval `[0,20] -> [1e20,-1e20]`; orientations `[Same, Opposite, Same, Opposite] -> [Opposite, Same, Opposite, Same]`; UV trims `[yes, no, yes, no] -> [no, no, no, no]`; 6 faces / 12 edges unchanged.
- **Measured, opt-in (`true`):** returns `Err(Upstream { operation: "curve::curve::p2::Curve", code: 1001, entity: None })`; interval, orientations and UV trims are identical to before the call; 6 faces / 12 edges unchanged.
- **Result:** the guard changes an observable public outcome. `crates/solidcore/tests/wr260_failed_reversal_observable.rs::failed_reversal_is_observable_on_a_public_box_edge` pins both outcomes exactly (interval, per-use orientation, per-use trim, returned error), using only public items.
- **Limit:** no public modelling operation produces an edge that fails the reversal today: Hermite curves stay inside intersection, and Iso/Conic/Composite curves have no public edge producer. A `.smb` file can, by contrast, carry a Hermite edge curve into a `Brep` through the public reader (`Curve::read_from_db` handles `HERMITE_CURVE_TYPE`), so the failure is reachable through the public edge record write or through such a file, not through an ordinary construction. The default path, and difftest, are unchanged.
