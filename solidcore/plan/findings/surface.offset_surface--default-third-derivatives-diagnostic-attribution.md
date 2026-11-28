# surface.offset_surface: default third-derivative diagnostic attribution (2026-11-04)

- **Unit / PR:** `surface.offset_surface`, #1914. The approved four-term correction is unconditional; this record does not propose another geometry change.
- **Measured revision:** `8ec4825280f7b2f9a41fc4183d609f8a080146e9`, on base `c4276a13c92ff696d40f331f8830fbb8cc5d269a`.
- **Command:** `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=bin/review1914-target SOLIDCORE_CPP_DIR=<main-checkout>/bin/cpp-v101 nice -n 10 cargo +1.98.0 run -p difftest --features cpp -- --unit surface.offset_surface` (with `RUSTUP_TOOLCHAIN` and `RUSTC_WRAPPER` unset).
- **Observation:** 3,182 cases, 2,663 passed, 519 failed, zero deferred; maximum relative error `1.9972685729541615`. This is a failed native diagnostic, not a parity pass. A separate full `curve.crv_on_surf` run reached the shell's combined 1,200-second limit without a summary.

## One individually supported C++ defect

The existing `fixtures/surface.offset_surface/edge_cases.json#0`, `third_derivative_outputs`, is named in the confirmed original WR-160 and WR-161 records. Its base is a regular cubic B-spline patch; its seven steps request rectangular derivatives of total order at most three or equal-order `(3,3)` upper-half derivatives.

The ten reported mismatches map through the unchanged runner's output serializer:

| `values.out` indices | Zero-based step | Derivative / component | Cause |
| --- | --- | --- | --- |
| 19 | 0, `eval` | `D(1,2).z` | WR-160 and/or WR-161 |
| 50, 51 | 2, `eval` | `D(0,3).y,z` | WR-161 |
| 76, 77 | 4, `eval` | `D(0,3).y,z` | WR-161 |
| 86 | 4, `eval` | `D(1,2).z` | WR-160 and/or WR-161 |
| 114 | 5, `eval_simple` | `D(1,2).z` | WR-160 and/or WR-161 |
| 126, 127 | 6, `eval_simple` | `D(0,3).y,z` | WR-161 |
| 136 | 6, `eval_simple` | `D(1,2).z` | WR-160 and/or WR-161 |

For example, `out[19]` is C++ `0.8656646974185226` versus Rust `-0.21398042233584189`; `out[50]` is C++ `-2.316474981309934` versus Rust `6.683525018690066`. The four corrected expressions follow differentiation of `Bu × Bv` and `B + d*N`, and a separate default-kernel polynomial closed-form regression fails before the fix. This supports a case-specific `cpp_defect = "WR-161"` marker explaining both WR-160 and WR-161. The marker parks this case only; it does not count it as a pass or establish correctness of every unrelated output.

## Remaining failures stay visible

`upper_half_slot_overwrite` (`edge_cases.json#1`) is **not parked**. Its `out[26]` is step 1's unequal-order `(3,1)` upper-half request: the serialized `D(2,0).z` slot is overwritten by a `Duvv` write. C++ `0.42964767031822376` versus Rust `-0.055925937843527895` therefore is not evidence that the destination `Duu` is correct. The unequal-order slot issue remains outside this PR. Its other four mismatches do not justify parking the entire case.

The expanded pass gives every remaining case bundle-level causal evidence and measured values in [the per-case addendum](surface.offset_surface--default-third-derivatives-diagnostic-attribution--addendum-2026-11-04-per-case-four-term-attribution.md). Reverting only the four corrected terms in a temporary source copy makes all full outputs agree with C++ under the unchanged comparator; that is not individual-term isolation or proof of correctness by C++ agreement.

The 518 cases split into 282 with independent conditional local-jet support, 103 relative-jet disagreements, 116 unequal-order alias cases tracked separately in #2717, nine non-alias pole-extrapolation cases and eight non-alias fallback/substitution cases. After the records were published, the maintainer-authorized 282 supported cases are listed against the confirmed WR-160/161 addenda naming their values. Together with the preserved original fixture entry, there are 283 case-specific markers. The other **236 cases remain ordinary running failures**. No post-marker full native sweep is claimed. Further numerical measurements may explain a disagreement but do not automatically authorize a marker.

No tolerance, input, runner, guard, fixture or comparison is changed to remove failures. The unit remains selected by `--green` and main's sweep through a provisional `partial` scheduling entry; its measured native result is still **fail**, not a passing or partial report. The PR stays draft under the maintainer's requirement to resolve every unexplained case, with the alias cases waiting for #2717's fix. Rust's own canonical CI is a separate gate, not permission to bypass that hold.

The existing passing cases and failing inputs remain in the runner. Every changed component now maps to its actual operation and serialized derivative slot, including overwritten destinations. The stronger correctness checks remain conditional on supplied base derivatives at the selected surface/UV; they do not globally prove providers, wrappers or the outer clamping/extension map. No end-to-end curve-on-surface improvement is claimed.

## Reference and evidence bounds

All 492 SMLib/SM_API source blobs in the preserved reference copies were checked against pin `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`. Shared `cpp-v101` library hashes matched preserved historical build receipts, but this is not a reproducible-build proof. Libraries and shared sources were not rebuilt or modified.

The native results above belong to `8ec4825`, not later integration heads. Local full report, logs and source-hash receipts were preserved before integration. Focused final-head checks and canonical CI must be reported separately; this record does not turn a timeout or a historical run into a current-head pass.
