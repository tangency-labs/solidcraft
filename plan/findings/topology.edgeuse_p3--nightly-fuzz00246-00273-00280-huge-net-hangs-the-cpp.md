# topology.edgeuse_p3: a B-rep block with a ±1e300 surface control point hangs the C++ (nightly fuzz00246, fuzz00273, fuzz00280, HANG-29) (2026-10-31)

- **Unit:** `topology.edgeuse_p3` (`SmEdgeuse::CreateUVTrimCurve` and the ops built on it).
- **Inputs:** nightly fuzz run 41040730716 (2026-10-30), verdict `timeout`,
  `cpp: no result after 20 s`, each reproduced alone (no result in 25 s):
  - `fuzz00246`: base `cos_u_closed_cyl_sub_eu5_create_dev1_fail0`, mutations `/eu = 2147483648`,
    `/has_fail = true`, `/brep/surface/bspline/net/2/1/1 = 1e+300` (`crv_on_surf_face`).
  - `fuzz00273`: base `cos_v_closed_cyl_r3_eu5_goc_ok1`, mutations
    `/brep/surface/bspline/net/0/1/0 = -1e+300`, `/brep/surface/bspline/net/0/4/1 = 0.0`,
    `/want_dev = false` (`crv_on_surf_face`, `get_or_create`).
  - `fuzz00280`: base `adversarial_b115_eu4_create_dev`, mutation
    `/brep/surface/bspline/net/0/2/0 = -1e+300` (`face`, `validate_only`, `create`).
- **Where the C++ hangs (gdb):** HANG-29's `SmCurve::GlobalCoincidenceChecker` recursion under
  `SmEdgeuse::CreateUVTrimCurve` -> `SmSurface::DropAndTrimCurve` ->
  `SmSurfaceDropCurve::FindBoundaryStartPoints`. For `fuzz00246` it is reached while the
  fixture is built (`topology_edgeuse_p1` export, `SmFace::CreateUVTrimCurves`), for
  `fuzz00280` (`validate_only`, no UV curves in the export) in the op itself.
- **Why it was open:** the 2026-10-20 fuzz sweep saw the same class (8 timeouts) and dropped the
  block guard it had added to `topology_edgeuse_p3::eval`, because the runner exported the
  fixture through the `topology.edgeuse_p1` wrapper first
  (`general--fuzz-sweep-over-all-green-partial-units-2026-10-20.md`). The runner has since
  moved to `topology_edgeuse_p3::PreparedBrep`, so the guard can run before the export.
- **Fix (harness, no kernel change):** `topology_edgeuse_p3::PreparedBrep::new` and
  `eval` / `eval_prepared` call `topology_edgeuse_p2::check_block_magnitudes` (any block value
  above `MAX_BLOCK_MAGNITUDE = 1e150`, the `±f64::MAX` markers aside) before the native export,
  as `topology.edgeuse_p2` and `topology.face_p3` do. The runner's Rust side applies the same
  check to the same block, so both sides give a harness error
  (`invalid argument: B-rep value above 1e150 (HANG-29)`; the C++ side's message carries the
  runner's `c++ export:` prefix).
- **The Rust port on these inputs (main's runner, Rust side alone, `--digest-dir`):** it returns,
  in about 3 s per case, with plausible records: `fuzz00246` status 0 with a UV trim curve,
  `fuzz00273` status 0, `fuzz00280` status 1001. No hang; it is refused only to keep the two
  sides' outcomes alike.
- **Coverage:** no fixture or generated case has a block value above `1e150`; the gate is
  unchanged (see the PR). Regression inputs: `crates/difftest/tests/fuzz_corpus/topology.edgeuse_p3.json`;
  runner test `huge_block_value_is_refused_on_both_sides` (refusal at `-1e300`, agreement at
  `-1e150`); FFI test `prepared_guard_tests::magnitude_refusal_precedes_native_export`.
- **Still open:** `topology.loopuse` (2026-10-20 sweep `fuzz00839`) has the same missing guard.
- **Record:** `plan/upstream-issues/HANG-29--addendum-2026-10-31-edgeuse-p3-export-guard.md`.
