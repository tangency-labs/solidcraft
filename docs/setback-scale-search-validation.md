# Setback scale search comparison (#2344a)

Both dependencies, #2369 (`ee6e1b349`) and #2274, are merged. The native target for
the scale change is SMLib `20b1c01`, not the pinned v1.0.1 reference. Only the two
Apache-2.0 files `source/SMLib/src/SmBlendSetback.cpp` and
`source/SMLib/inc/SmBlendSetback.h` were fetched from that commit.

The focused driver is `harness/cpp/tests/blend_setback_scale_search.cpp`. It can
be linked with those two files and existing fork SMLib/SM_API libraries (the
local comparison used the shared fork libraries at `5e8c9c1`). Put the two files
in a scratch include/source directory and compile the new source object before
linking the static libraries, so the old setback object is not selected.
Use the harness defines and `-O2 -ffp-contract=off -fno-fast-math`, under
`nice -n 10`. The driver checks statuses, exact scales, attempts, solve counts
and whether a refusal reason is present. It does not consume Rust outputs.

Native results:

| Corner | Scale | Attempts | QPs |
| --- | ---: | ---: | ---: |
| rounds 2,3,4, rule setbacks | 1.056 | 12 | 35 |
| rounds 1,1.5,2, rule setbacks | 1.056 | 12 | 35 |
| rounds 2,3,4, upright held at 4.5 | 1.046 | 12 | 39 |
| rounds 2,3,4, asked setbacks 4.25 | 1.110 | 12 | 34 |
| rounds 5,5,5, setbacks 8 | 1 | 1 | 2 |
| rounds 2,3,4, every edge held at its rule setback | 0 (refused) | 2 | 10 |

Rust focused gate:

```sh
CARGO_BUILD_JOBS=2 nice -n 10 cargo test --profile difftest -p solidcore --lib thousandth_scales_and_attempt_budget -- --nocapture
```

Explicit expensive monotonicity gate (separate from default tests):

```sh
CARGO_BUILD_JOBS=2 nice -n 10 cargo test --profile difftest -p solidcore --lib setback_monotone_grid -- --ignored --nocapture
```

The smallest certifying scale on the grid whose neighbour below fails; monotonicity checked, not proved.

The matrix in this contribution has seventeen descriptions, including axis
permutations. It is not the capi fixture inventory outside the source allow-list; the pocket rim
fixture remains a validation follow-up. The twelve-case executive pass is historical;
the expanded merged-head comparison has the explicit failures recorded below.
The pinned C++ difftests validate default behavior only. Do not treat them as
proof of opt-in fork parity. Streamed rows and their row/step counters are outside
this slice. The blend corner, once behind `fillet_blend_corner`, is the default since
#2400, which deleted the flag and the superseded corner code.

## Native executive reference

The executive driver is `harness/cpp/tests/blend_setback_executive.cpp`. It
constructs all inputs independently through SM_API and uses that API's context
for the executive too. A different context has different topology mark ownership
and is not an equivalent input.

The existing fork libraries are at `5e8c9c1`. For the executive, merely replacing
`SmBlendSetback.o` is insufficient: #104 changes the layout of blend corners.
Fetch the changed files under **source/SMLib/inc and source/SMLib/src only** from
`20b1c01` into an ignored overlay directory. The changed-source inventory is
the comparison of Git tree blob IDs at `5e8c9c1` and `20b1c01`; do not rely
on GitHub's compare-files endpoint, which truncates after 300 files. In addition
to the initial compare listing, the complete inventory includes
`SmFilletSection.cpp` and `SmProceduralBlend.cpp`.

Recompile the changed sources and all blend/fillet consumers of their headers,
then link the overlay before the existing static libraries:

```sh
bash harness/cpp/tests/build_setback_executive.sh \
  bin/reference-2344 /home/karlvonrichter/github/smlib-fork \
  /home/karlvonrichter/github/fork-cpp/lib bin/reference-2344/executive
nice -n 10 bash harness/cpp/tests/run_setback_executive.sh \
  bin/reference-2344/executive > /tmp/native-executive.log
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 nice -n 10 cargo test \
  --profile difftest -p solidcore --test fork_smlib104_setback_corner \
  dump_blends -- --ignored --nocapture > /tmp/rust-executive.log 2>&1
python3 harness/cpp/tests/compare_setback_executive.py \
  /tmp/rust-executive.log /tmp/native-executive.log
```

The comparator requires thirteen independently constructed executive results:
matching success/refusal, comments, named fits, topology counts, volumes and
every vertex coordinate, at 1e-9 relative with no absolute floor. It documents
only the existing unsupported-corner code-number mapping (native 10, Rust 9).
The Rust dump also requires Level2 `check_brep` to have no lines.

This reuses the shared library build without modifying it or creating another
checkout. Only the allowed SMLib sources are compiled. The native overlay is
not the pinned reference used for default-behavior difftests.

## Dependency refresh (2026-11-03)

### Resumed refresh

The newest authoritative-main merge is `84a419512` in `627fce896`, without
conflicts or manual resolutions. #2491/#2495 resolve the four fork-parity
snapshot failures by excluding the solidcore-native exact-spine change from
that test profile, keeping the assertions active and unchanged. The fork dump
now follows that profile; the separate exact-spine diagnostic enables the
redesigned construction explicitly. Earlier failure evidence below is historical.
Main's updated AGENTS makes difftest a maintainer-requested tool, while retaining
its strict parity/floor rules. This contribution still runs its native and
C++ caller comparisons; it introduces no absolute floor.

Fresh results on that newest merge plus the fork-dump profile alignment:
all 34 tests in the six selected setback/G2/unequal/Gn-memo/#2020 integration
suites pass (three diagnostic helpers and main's documented #2020 exclusion
remain ignored in the regular run). The full thirteen-input fork-profile Rust
executive dump passes Level2 BREP checks and the unchanged independent native
comparison: **13 cases, 7 bodies, 167 vertices**, statuses/comments/fits/counts/
volumes/every coordinate at 1e-9 relative, no absolute floor. This supersedes
the historical fork-profile snapshot/parity blockers below. It does not claim
that the intentionally different exact-spine profile matches that older fork.
The focused `thousandth_scales_and_attempt_budget` gate also passes freshly:
all six exact scale/attempt/QP tuples above, including the room-limited and
refusal cases. The assertion is on counts and values, not runtime.
The seven affected legacy C++ caller units pass on the newest merge: executive
p1-p3, corner p1-p3 and geom p3, **3497 cases including 52 guard rejections**,
zero failures or deferrals and maximum reported relative errors 0. These are
default-path regression checks, separate from the fork-profile executive proof.
Formatting, workspace clippy (`--workspace --all-targets -- -D warnings`) and
the wasm32 check also pass on this newest source state. Full solidcore tests,
runner self-tests and the complete `--green` sweep were not run for this slice;
these targeted passes are not a claim that PR CI has run or passed.

The preceding cancellation refresh merged `d7319e27e` in `a2ce56e45`, also without
conflicts or manual resolutions. It preserves #2373/#2375 cancellation polling
and propagation in Gn sections, offsets, shells and sweeps. Those changes do
not alter the uncancelled numerical formulas or row/tie order. The evidence
below is attributed to the earlier library unless explicitly identified as
post-cancellation-merge verification.

Post-cancellation-merge verification on `a2ce56e45`: formatting and all seven
integration tests in `issue_2371_gn_section_cancel` (4) and
`issue_2372_offset_shell_sweep_cancel` (3) pass. These include fired-token
propagation, in-operation cancellation and bit-for-bit no-token versus
unfired-token results. This does not resolve the reference-boundary failures.
The six legacy pinned C++ executive/corner caller units also pass on that
merge: 3172 cases including 45 guard rejections, zero failures or deferrals.
They remain default-path regression evidence, not opt-in fork acceptance.
All 17 tests across the G2 (8), unequal-box (2) and direct-setback (7)
integration suites pass on this merge as well.
Workspace clippy (`--workspace --all-targets -- -D warnings`) and the wasm32
check also pass on this merge.

Authoritative main `ba69ec95d` is merged locally in `8690f4528` without
conflicts or manual resolutions. Both #2369 and #2274 are now merged; their
review/merge is no longer a dependency blocker. This preserves main's exact-spine
cylinder construction from #2419 (`9c44f2332`, WR-669 addendum).

That construction is explicitly solidcore-native: it projects the stations onto
the exact spine and retains the cylinder's frame, whereas the native 20b1c01
reference takes solved stations and refits the frame. A controlled test-only
probe disabling exact round classes restores all seven original regressions
(the normal profile is still 3 pass / 4 fail). The explicit ignored
`diagnose_class_dependent_executive` helper now isolates that class-dependent
construction on the same thirteen-input matrix, without an environment override
in the regular tests. That explicit diagnostic passes the unchanged strict
native comparator (13 inputs, 7 bodies, 167 vertices); the normal profile fails.
This does not demonstrate a scale-search defect or accept
the current construction against the old frame. The acceptance boundary is raised on
the [blend hub](https://github.com/tangency-labs/solidcore/issues/2243#issuecomment-6593461835).
The strict comparator remains unchanged and failures are not waived.

The thirteen-case native driver was rerun successfully (seven bodies, 167
vertices). The fresh Rust dump completes with Level2 checks passing, but the
normal strict comparison still fails at case 9. The six-case scale/attempt/QP
gate and formatting pass. No legacy snapshot expectation or tolerance changed.
The new finding is
`plan/findings/blend.fillet_corner--2344-exact-spine-reference-boundary.md`.
All six affected legacy pinned C++ caller units pass freshly on this merged
library: executive p1-p3 and corner p1-p3, 3172 cases including 45 guard
rejections, zero mismatches or deferrals, all reported maximum relative errors 0.
This is a default-path regression gate, not acceptance against the fork for
the opt-in construction. On this same merged library, all eight G2 regressions,
both unequal-box regressions, all seven direct-setback tests and the focused
exact-spine cylinder regression pass. Workspace clippy and wasm32 checks also
exit successfully.
The PR remains draft. CI run 41549430827
at `ed7a76660` still has zero jobs and needs repository-admin approval.

### Earlier refresh evidence

Merged authoritative main `ebe6722e09169cce29a20c455fc6ec606b57caa0`
and #2369 `ee6e1b349e9ebae287246cc42a787a6620ab429e` into the
published branch, without rebasing or changing dependency branches. #2274 is
merged. At that earlier refresh #2369 was open with thirteen successful checks;
it has since merged, so its review/merge is no longer a dependency blocker.

The main merge resolved two documentation conflicts: retain `JoinSection = 16`
and `Precomputed = 14` with main's discriminant explanation; retain the new
Ball patch, End/Join and collapsed-offset documentation alongside the
thousandth-grid scale guarantee. The dependency merge kept that guarantee over
its old `to 1e-2` text and removed a blank line in the enum documentation.
Its added `BlendCorner.end` initializer is preserved.

The executive matrix now includes #2369's unequal (2, 2, 3) rounds on a
20 mm box at (20, 0, 20), as a thirteenth independent native/Rust input.
Its native scale-search volume is `7927.724282512234`, replacing the dependency's
old-search `7928.142563966`. Both states of the added Rust regression pass;
the flag-off expected volume was preserved.

### Results of that earlier refresh

Workspace clippy (`--workspace --all-targets -- -D warnings`), wasm32 check
and formatting pass under Ubuntu WSL with two Cargo jobs and low priority.

The six-case scale/attempt/QP gate passes, as do all eight G2 regressions and
all seven direct setback tests in `fork_870c970_blend_setback`.
All six pinned C++ caller units pass on the merged code: executive p1–p3 and
corner p1–p3, 3172 cases including 45 guard rejections, zero mismatches or
deferrals (`difftest: 6 unit(s), 6 passed, 0 partial, 0 failed`).
The executive dump builds seven bodies and passes Rust Level2 checks.
Stored-versus-streamed native executive comparison passes all thirteen cases,
seven bodies and 167 vertices.

The **strict Rust/native executive comparison fails**: cases 9, 10 and 12 have
ten unmatched vertices with coordinate differences near zero, despite matching
volumes and topology counts. No absolute floor or tolerance was introduced.
Details: `plan/findings/blend.fillet_corner--2344-main-refresh-relative-zero-parity.md`.

The setback integration suite also has four flag-off volume-snapshot failures
(three pass, two helpers ignored). Their flag-on checks pass. No old expected
volume or tolerance was changed. Details and reproduction:
`plan/findings/blend.fillet_corner--2344-main-refresh-flag-off-volumes.md`.
These are explicit readiness blockers; historical proof below does not waive them.

### Historical evidence before this refresh

At `274e0a4a6`, the twelve-case native executive gate passed: six bodies,
144 vertices, statuses, comments, named fits, topology counts, volumes and
every coordinate at 1e-9 relative with no absolute floor. Workspace clippy,
wasm, fmt, the focused six-case scale/count gate, setback and G2 suites and
all six pinned C++ executive/corner caller units passed (3172 cases including
45 guard rejections; zero mismatches or deferrals). This is historical proof,
not evidence for the merge resolutions above.

Full `cargo test -p solidcore`, runner self-tests and `--green` were not run
for this slice; there is no unit promotion or CI-green claim. Production Rust
streaming remains unimplemented; the native preparation is documented separately.

### CI permission boundary

At the resumed verification's pre-push head `ed7a76660`,
[41549430827](https://github.com/tangency-labs/solidcore/actions/runs/41549430827)
is completed `action_required` with zero jobs. This is a workflow approval
gate, not a failed test. Historical run
[41450960214](https://github.com/tangency-labs/solidcore/actions/runs/41450960214)
at `274e0a4a6` also completed `action_required`. Earlier run
[41431300743](https://github.com/tangency-labs/solidcore/actions/runs/41431300743)
at `9f5dce3` had zero jobs and requires repository-admin approval. The earlier
approval attempt returned HTTP 403, "Must have admin rights to Repository".
The current account still has pull permission only on the authoritative repo.
No code edit or rerun can supply that permission. The PR body records workflow
status separately after each push; none of these approval-gated runs is green evidence.
