# general: #521 follow-up after merged #704: current scope of PR #700 (2026-10-24)
Updates: plan/findings/general--521-topology-evaluation-status.md

The earlier vertex error-policy draft and its 43 WR-261 native differences above
are historical validation of that draft, not the current branch contract. Main
#704's complete vertex implementation, regression, 70 end-tangent fixtures, shim,
FFI and query-loop documentation are preserved byte for byte. The vertex error
propagation change and its one regression were removed, as was the runner's
angle-error normalization. All 43 formerly listed vertex cases now compare equal
under the merged contract: the strict harness first rejected all 43 as stale
passes, then they alone were removed and the full 5598-case vertex unit reran
green. The 20 constant-segment fixtures remain matching parity coverage; they
exercise zero tangents/angle failures, not failed GetEndTangent evaluation.

Six remaining regressions cover edge reversal, loop UV corner evaluation, edge-use
loop gaps and deviation, and healing classification/retry state. Their four kernel
modules and the 20 fixture inputs are byte-identical to the original tested
implementation. The four WR-266 healing differences remain explicit and partial:
status 1040 is unchanged, and only the incomplete marker changes from 5 to 4
(including gen0345's later snapshot). This was rechecked from raw native outputs.
The earlier C++ WR-261 problem record remains valid; its altered Rust predicate
policy is outside this follow-up.

Current validation on `0cdc106a4a941c31be2f48402084dca3a72832f0`: fmt, default/native
workspace Clippy with warnings denied, 2094 kernel unit tests, wasm, 48 runner
tests, and five integration passes (one existing partial-sphere/torus case ignored)
under the optimized difftest profile used by CI all pass. Fresh native 13-unit
validation has 37684 cases: 37163 matching, six explicit known differences (four
WR-266 healing markers, two existing main Rust/harness MoveSeam limitations),
462 UNPORTED deferrals and 53 native guard rejections (37 UB-137 plus 16 existing
input guards), zero unexpected failures. Twelve units come from the initial
matrix; vertex was rerun strictly after removing the proven stale entries. No
production source or fixture changed between those runs. The current main cache
wiring is retained; old 6ec validation figures above retain their original base.
No canonical CI pass or full macOS --green is claimed.


Rejection-category clarification for the current PR #700 matrix: the 53 native
rejections comprise 37 UB-137 crash traps, one HANG-39 input guard and
15 missing-entity cases. Earlier references to the latter 16 as “input guards”
were imprecise. No parity comparison ran for those rejected cases; none is counted
as a matching result. The combined sixteen-fix matrix separately has 994 UNPORTED
deferrals plus six `harness:offset-result-adapter` deferrals, and 95 rejections
(37 crash traps, 25 other input guards, 18 construction failures, 15 missing
entities). The recorded totals and zero-unexpected-failure results are unchanged.
