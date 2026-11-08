Updates: UB-280

#### UB-280 (update, 2026-11-04). `rb_33_box_with_large_fillet_6` crashes in one harness build and builds in another
- **Location:** as UB-280 (`source/SMLib/src/SmFeatureExecutive.cpp:1922`, `SmFeatureExecutive::ExtSurfaceRebuild`).
- **What happens:** the defect is the same (a call through a NULL function pointer), and its outcome depends on the heap layout, as the 2026-10-31 addendum records. One more case shows it: in one harness build the C++ side returns a result that the Rust side matches; in another it crashes in its forked child process, and the case is rejected (`SC_ERR_CHILD_CRASHED`).
- **Trigger:** `blend.feature_executive` case `rb_33_box_with_large_fillet_6` (`box_with_large_fillet.smb`, `AddFeature({face 6}, rebuild types 3, 3)`, `DoDefeature`; `fixtures/blend.feature_executive/generated.json#72`).
- **How found:** tangency-labs/solidcore#2715 (taking `Clone` off `Brep`) changes only Rust and harness code. The case's C++ input is the fixture file, the block and the script, all built from the case alone; nothing from the Rust side reaches it.
  - The base build (`1f88b3c0a`) passes the case; two runs agree.
  - The #2715 build rejects it as a C++ child crash in three runs: two with the whole unit, one alone (`--filter rb_33_box_with_large_fillet_6`). Run alone, the C++ side crashes in 0.15 s, so it is a crash, not the 60 s timeout. The machine load was 32 in the first run and 3 in the others.
  - The unit's other counts are unchanged (128 or 129 passed; the same 2 failures, which the base build also has).
- **Confirmed:** yes, on the C++ build: the same C++ input crashes in one build and returns a result in another. A rejected case is neither a pass nor a failure. No `cpp_defect` marker is added: the case is rejected, not failed, in the build that crashes.
