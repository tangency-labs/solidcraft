Updates: WR-87

#### WR-87 (update, 2026-10-31). Opt-in fix for unclosed surface return value

- **What changed:** The port now provides an opt-in fix for `sm_CheckSeamCrossing`
  via `Kernel::fixes.check_seam_crossing_unclosed`. When enabled (`true`),
  `check_seam_crossing` returns `false` when a surface is closed in neither direction
  (`closed_u == false && closed_v == false`). The default remains `false` (returning `true`),
  preserving exact pinned C++ reference parity.
- **Location (current pin):** `source/SMLib/src/SmFace.cpp:144-145`, in `sm_CheckSeamCrossing`
  (def. `:132-228`).
- **Effect:**
  - Default (`check_seam_crossing_unclosed = false`): returns `true` (C++ `SM_SUCCESS`),
    matching upstream fixture `seam_neither`.
  - Opt-in (`check_seam_crossing_unclosed = true`): returns `false` (no seam was crossed,
    no period shift applied).
  - Closed-U and Closed-V crossings and period shifts remain unaffected in both modes.
- **Caller impact:** Upstream's only caller (`SmFace::ComputeLoopOrientation`, ported in
  `topology::face::p3`) ignores the return value, so no existing macro B-rep behavior
  diverges.
- **Field:** `Kernel::fixes.check_seam_crossing_unclosed` (`crates/solidcore/src/kernel.rs`),
  off by default per AGENTS.md §4b.
- **Tests:** `topology::face::p2::tests::seam_crossing` (verifying default `true`) and
  `topology::face::p2::tests::seam_crossing_opt_in_unclosed_returns_false` (verifying opt-in `false`).
