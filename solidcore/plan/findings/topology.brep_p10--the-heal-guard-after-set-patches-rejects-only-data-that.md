# topology.brep_p10: the heal guard after `Set` patches rejects only data that reaches the healer changed (2026-10-23)

- **Before.** The runner's `heal_guard` rejected every healing case with a `Set` patch
  (`healing after a set patch walks malformed indices`; 14 generated cases). One crash
  had been seen on that path: the 2026-10-20 fuzz sweep's `fuzz01957` (fixture
  `spine_fan.smb`, `set {field 2, index 1, value 0}`, SIGSEGV), kept in
  `crates/difftest/tests/fuzz_corpus/topology.brep_p10.json`. It is still rejected (only
  its reason text changed).
- **Measured** (guard arm removed, each case in its own process, reference build at the
  pinned harness key). The 14 cases all ran cleanly (12 pass, 1 deferred, 1 then hits the
  IGES arm). A stress generator forcing healing plus one `Set` (fields 0..=15, index
  0..=6, values 0..=6, -1, -2, -5, 999999) over the upstream `.smb` files: 21 SIGSEGVs and
  2 timeouts in 400 cases (25 per field), and 3 Rust/C++ disagreements. Crashes came with
  fields 2 (loop start edge use), 5 (edge use loop), 7 (edge curve), 12 (edge use type)
  and 13 (loop edge use count) (9 of 25), each alone, plus field 6 and 8 only together
  with 7 or 13. E.g. `Tess/nurby_box_cyl_sphere.smb`, options
  `[true,false,false,false,true,true]`, `set {field 7, index 1, value 6}` (edge 1 on edge
  6's curve): SIGSEGV with healing, passes bit-identical (1407 values) with healing off,
  and the same patch with values 0, 2, 3, 5 or 7 does not crash. None of them is an
  out-of-range index (the shim's simulation of the build passes them all): the build
  accepts records that are consistent index-wise but not topologically or geometrically,
  and the healer walks that B-rep unchecked. Whether it crashes depends on the geometry,
  so there is no input rule finer than "the patch changed what the build accepts".
- **After.** The arm rejects a case only when its `Set` patches change the patched data
  (compared with the same patches minus the `Set`s) and the Rust build with healing off
  does not return `InvalidInput`. `InvalidInput` is exactly the data the shim's build
  simulation turns into `-2` before upstream runs (UB-164 proper), so upstream never runs
  there. A `Set` past its array, or to the value a field already holds, is compared. The
  decision reads only the inputs and the Rust build, never C++ output. The 400 stress
  cases rerun with the new guard: 0 crashes, 0 timeouts, 0 disagreements, 157 rejected.
- **Result.** topology.brep_p10 1404 cases: 1166 pass, 0 failed, 231 deferred, 7 rejected
  (was 1156 / 0 / 231 / 17). The 10 cases now compared are status matches (both sides
  `-2`). Still rejected: 4 IGES (UB-237) and 3 `Set` cases (`gen0182`, `gen0204`,
  `gen0218`) whose patch changes data the build accepts. These 3 ran cleanly in
  isolation, but the same class crashes elsewhere and no input rule separates them.
- **Not committed.** The stress generator. Its crashing inputs are listed above, and
  UB-164 in `plan/upstream-issues.md` records the healer trigger.
