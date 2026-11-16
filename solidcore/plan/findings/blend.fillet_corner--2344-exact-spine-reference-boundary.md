# blend.fillet_corner: the exact-spine executive reference boundary (#2382)

## Authoritative resolution

Main #2491/#2495 (`84a419512`, merged here in `627fce896`) independently
bisected the same cause and made the fork-parity test profile exclude
`fillet_exact_round_classes`. All original volume assertions are active and
unchanged; the exact-spine production construction remains intact. `dump_blends`
now follows that fork-parity profile. `diagnose_class_dependent_executive` now
explicitly enables the solidcore-native exact spine, so it remains a separate
diagnostic rather than contaminating the fork comparison. The historical
diagnostic below used the opposite switch before this reference decision.
No absolute floor, snapshot repin or production fallback was introduced.

Fresh verification of this aligned profile passes all seven original regular
setback tests and the strict full executive comparison: 13 inputs, 7 bodies,
167 vertices, all statuses/comments/fits/counts/volumes/coordinates at 1e-9
relative without an absolute floor. The exact-spine construction remains
outside that fork-parity claim. The historical blockers below are resolved
for the authoritative fork-test profile.

The review replies and persistence coordination are recorded on
[#2382](https://github.com/tangency-labs/solidcore/pull/2382#issuecomment-6595524107)
and [#2488](https://github.com/tangency-labs/solidcore/issues/2488#issuecomment-6595523768).

## Historical reproduction and diagnosis

Updates the two `blend.fillet_corner--2344-main-refresh-*` findings. Library
code: `8690f452867665e568138d08d731eb66196953f1`, authoritative main
`ba69ec95d` merged without conflicts; #2369 and #2274 are both merged.

The fresh default regression run reproduces the same four flag-off volume
failures: 3 passed, 4 failed, 2 ignored dump helpers. Its flag-on checks pass.
The fresh thirteen-input Rust executive dump passes Level2 BREP checks but
still fails the unchanged strict relative-only comparator at case 9, Rust
`(0,20,3)` against native
`(4.440892098500626e-16,20,3.0000000000000187)`.

## Controlled diagnosis

A temporary test-helper probe changed only `fillet_exact_round_classes` to
false, keeping the other settings and all original snapshot assertions. All
seven regressions then passed, including all four formerly failing snapshots
and the concave difference above 1e-9 relative. The probe was removed: no
environment-controlled override remains in `all_fixes`, and no expected value
or tolerance changed.

The committed ignored helper `diagnose_class_dependent_executive` makes that
class-dependent executive comparison reproducible without changing the regular
test configuration. It runs exactly the same thirteen inputs and dump order as
`dump_blends`, with only that existing flag disabled. It is a counterfactual
diagnostic, **not acceptance evidence for the all-fixes construction**. `FLAGOFF=1`
also disables `fillet_blend_corner`, as in the existing dump helper, for inspecting
the legacy volumes.

The committed explicit diagnostic passes the unchanged strict native comparator:
**13 cases, 7 bodies, 167 vertices**, including statuses/comments/fits, topology,
volumes and every coordinate at 1e-9 relative with no absolute floor. The same
library's all-fixes dump fails it. This is evidence for the isolated construction
boundary, not a green result for the all-fixes executive.

Code/history identifies the relevant intentional redesign: #2419,
`9c44f2332`, and the WR-669 `line-spine-exact-frame` addendum. With exact round
classes, `CalcLineSpineFilletGeom` projects the stations onto the offsets' exact
intersection line, takes the rails' feet on the face quadrics, and keeps the
cylinder's constructed frame. Native 20b1c01 still uses the solved stations and
refits the frame from the patch. The addendum explicitly calls this
solidcore-native. This construction must be preserved; restoring the old frame
to obtain a native comparison would undo accepted main work.

## Reproduction

Use the native overlay/build commands in `docs/setback-scale-search-validation.md`
and run its thirteen-input native driver independently. In Ubuntu WSL, with
`CARGO_BUILD_JOBS=2` and Cargo on PATH:

```sh
RUST_TEST_THREADS=2 nice -n 10 cargo test --profile difftest -p solidcore \
  --test fork_smlib104_setback_corner dump_blends -- --ignored --nocapture \
  > /tmp/setback-current.log 2>&1
RUST_TEST_THREADS=2 nice -n 10 cargo test --profile difftest -p solidcore \
  --test fork_smlib104_setback_corner diagnose_class_dependent_executive \
  -- --ignored --nocapture > /tmp/setback-diagnostic.log 2>&1
python3 harness/cpp/tests/compare_setback_executive.py \
  /tmp/setback-current.log /tmp/native-executive.log
python3 harness/cpp/tests/compare_setback_executive.py \
  /tmp/setback-diagnostic.log /tmp/native-executive.log
```

The first comparison remains a failure. Neither comparator path changes its
tolerance, adds an absolute floor, snaps a coordinate or reads C++ state into
Rust. The scale/count gate passes independently of this executive boundary.

Acceptance of the redesigned executive and maintenance of the four stale
legacy snapshots is raised on the
[blend hub](https://github.com/tangency-labs/solidcore/issues/2243#issuecomment-6593461835).
No failures are waived, no unit is promoted, and #2382 stays draft pending that
decision and the remaining gates. Production Rust streamed rows remain outside
this contribution.
