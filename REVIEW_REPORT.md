# PR #15112 review corrections

All 22 findings were verified against the starting code and confirmed. No findings were rejected.
Changes remain unstaged; COMMIT_MSG.txt contains the proposed commit message.

## Major findings

| # | Finding | Result | Regression evidence |
|---|---|---|---|
| 1 | Undo behavior used as command authority | Fixed: explicit command effect defaults to edit; reviewed read-only commands opt in | Engine plugin test refuses read-entry calls to both solid.box and sketch.edit |
| 2 | Folder grants bypass caller scope for hidden paths | Fixed: every read/write checks the joined path, with hidden names denied unless explicitly scoped | Engine hidden-file and hidden-directory cases; runtime complete-path read/write callback and default hidden refusal |
| 3 | Agent permissive modes bypass plugin consent | Fixed: plugin.run retains Ask in every mode and with remembered allowance | Agent-chat pump regression for all modes and command case variants |
| 4 | Traps leave written targets changed | Fixed: stage sibling files, publish after app commit, discard on rollback | Runtime trap, failed commit, successful publication ordering, staged reads and publication-error tests |
| 5 | Script preflight stops after 10,000 steps | Fixed: normalize the complete accepted script | UI namespaced plugin call at step 10,001 refuses before any edit |
| 6 | Stop falsely claims no file effects | Fixed: both cancellation paths report possible file writes | UI client-cancel and window Stop tests |
| 7 | CLI fixture retains WIT placeholders | Fixed: substitute package/version from the shared API identifier | Existing CLI host-ceiling regression now reaches guest execution |
| 8 | Engine test computes mesh volume | Fixed: assert mesh counts, index validity and bounds | Existing mesh-transfer contract regression |

## Minor findings

| # | Finding | Result | Regression evidence / validation |
|---|---|---|---|
| 1 | README denies available filesystem/clock interfaces | Fixed: describe permission-gated interfaces, fixed clock epoch and absent WASI/socket/process APIs | Checked against shared WIT and host implementation |
| 2 | README uses API 0.1.0 | Fixed: interface note and manifest use 1.0.0 | Checked against WIT package declaration |
| 3 | Inspection completion registers in wrong session or replaces a newer result | Fixed: retain source session ID and request order; reject stale completions | Deterministic tab-switch and out-of-order UI tests |
| 4 | Display warm-up ignores Stop | Fixed: capture shared cancellation state for every display worker | Cancelled 17-body warm-up leaves all mesh caches cold |
| 5 | Plugin-issued sketch exits skip UI cleanup | Fixed: return accepted inner sketch exits and run cleanup after adoption | Real guest calls to sketch.finish and sketch.cancel clear the window tool |
| 6 | Missing locale provenance | Fixed: exact attribution rows for all three new pending plugin locale assets | Lists and asset checks |
| 7 | CLI execution test runs on unsupported architectures | Fixed: gate to x86_64/aarch64 | Existing architecture-gated host-ceiling regression |
| 8 | Preferences navigation discards resource drafts | Fixed: clear drafts only when opening a new dialog | UI navigation then Apply regression |
| 9 | Manifest accepts non-object properties | Fixed: reject present non-object values, including nested ones | Shared manifest array/null/nested-boolean cases |
| 10 | Transaction documentation leaves file rollback ambiguous | Fixed: document staging, rollback and postcommit publication failure | Runtime rollback and publication-error tests |
| 11 | Missing plugin.run params defaults to null | Fixed: default to an empty object | Engine guest echoes omitted params identically through both call forms |
| 12 | Metadata-only packages cannot be discovered | Fixed: load component bytes only when runtime exists; execution still requires runtime | Engine inspect/list/commands and execution-refusal regression |
| 13 | Discovery replaces explicitly inspected selection | Fixed: discovery fills only vacant package registrations | Engine selection-preservation regression |
| 14 | External example guest build instructions cannot be followed | Fixed: explicitly state guest/examples/distribution scripts live outside this repository and remove checkout guest-build directions | Host WIT and RFC references retained |

## Additional full-CI failures

- Fixed the folder/id mismatch diagnostic in the shared parser to include `does not match`.
  The UI test `a_folder_with_plugin_json_is_listed_and_a_bad_one_is_skipped` is unchanged.
  The existing shared manifest test also asserts the diagnostic.
- Fixed `external_plugin_code_always_requires_consent` setup to install the UI catalogs before
  formatting its consent question; the behavioral assertions are unchanged. The pump still
  preserves per-run consent independently of permission modes and remembered allowances.
- Added portability coverage so a granted root may have hidden/AppData ancestors while
  hidden descendants and mismatched read/write rights remain refused.

## Validation

Completed checks (all Cargo commands ran through buildq):

- `cargo test -p craft-plugin`: final run, 9 passed.
- `cargo test -p craft-plugin-wasm`: final run, 16 passed, including staging order across grant aliases.
- `cargo test --profile ci -p solidcraft-engine --features plugins plugin`: final run, 22 passed.
- `cargo test --profile ci -p solidcraft-cli --test run_results plugin`: 1 passed.
- `cargo clippy -p craft-plugin --all-targets -- -D warnings`: passed.
- `cargo clippy -p craft-plugin-wasm --all-targets -- -D warnings`: final run passed.
- `cargo clippy -p solidcraft-engine --features plugins --all-targets -- -D warnings`: final run passed.
- `cargo clippy --profile ci -p solidcraft-ui-egui --all-targets -- -D warnings`: final run passed.
- `cargo clippy --profile ci -p solidcraft-cli --test run_results -- -D warnings`: passed.
- `cargo fmt --all -- --check`: final run passed.
- `cargo xtask layers`: passed; 36 crates, no layering or solidcore seam violations.
- `cargo xtask lists`: passed; lists sorted, pending locale and attribution files fold cleanly.
- `cargo xtask mesh-decisions`: passed; 179 known decision sites, no new ones.
- `cargo xtask assets`: passed; all 1,088 assets attributed and 264 encoded resources match their source receipts.
- `git diff --check`: passed.
- Documentation API/WIT-reference and pending-locale JSON/provenance checks: passed.
- `cargo test --profile ci -p solidcraft-ui-egui --lib -- plugins agent_chat::gate agent_chat control history_off_thread_tests`:
  627 passed, two new regressions initially failed because
  one bypassed queued normalization and the other injected WAT at an export reference.
  Both test setups are corrected. The final targeted rerun passed all 25 tests
  (plugin preferences/listing, consent gate, long-script preflight and accepted sketch exits).
  Both additional full-CI regression tests passed unchanged in behavior.
- Final UI rerun: `cargo test --profile ci -p solidcraft-ui-egui --lib -- plugins::tests agent_chat::gate::tests history_off_thread_tests::accepted_plugin_sketch_exits_clear_the_window_tool history_off_thread_tests::plugin_preflight_checks_namespaced_calls_after_ten_thousand_steps`.

All requested targeted tests and checks passed. No full workspace suite was run locally.
An initial remote engine build reported missing plugin symbols from a source snapshot taken
while that file was being rewritten. Engine clippy on the stable local files passed;
the CI-profile engine run passed all 21 then-current plugin tests. A default-profile repeat
was terminated by SIGTERM without a compiler failure. The final CI-profile engine rerun
passed all 22 plugin tests, including grant-relative coverage. Buildq also emitted nonfatal
sccache server warnings and fell back to local compilation.

## Limits and handoff

File publication follows app commit. A publication error can leave some files changed;
this is explicitly reported and documented. Multi-file publication is not atomic.
No git metadata was written, and no commit or push was performed under this lane's rules.
The pre-existing untracked .rfc-0002.md was preserved. A concurrent edit to
tests/kernel-cases/EXEMPT and a coverage job from this worktree were also preserved;
they were not created by this review-fix agent.
The full suite, wasm/build matrix and broad CI gates remain for CI after the coordinator pushes.

UI screenshot evidence is unavailable in this sandbox: `xdpyinfo -display :0` fails,
and Xvfb and a SolidCraft executable are not installed. Automated UI regressions cover the changes.
