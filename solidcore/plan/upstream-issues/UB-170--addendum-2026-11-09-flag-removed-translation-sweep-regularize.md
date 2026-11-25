Updates: UB-170

#### UB-170 (update, 2026-11-09). `translation_sweep_regularize` is removed

Whole-body translational sweeps that are not merged and run in place remove the degenerate swept faces and the isolated slit loops they leave (`regularize_translation`, `IsDegenerate`, `SqueezeFace`, `DeleteEdge`), so the bottle of the record sweeps to a valid body of 84 faces and 317 edges. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::translation_sweep_regularize`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `tests/issue_1937_bottle_sweep.rs` (the ignored bottle fixture test asserts the regularized body; the box sweeps assert no check lines); the test that pinned the native failures is removed.
