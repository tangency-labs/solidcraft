# surface.offset_surface: fresh goldens migration measurements (2026-11-06)

Updates: plan/findings/surface.offset_surface--default-third-derivatives-diagnostic-attribution.md

## Scope and provenance

PR #1914 still owns exactly four unconditional third-order terms and its default-kernel regression. Requested-grid/local-initialization work remains #2717's separate fix, now in standalone PR #2970 after #2765 was closed in its favour. The November 6 migration direction on #1914 supersedes the historical native-accounting/marker proposals, not the historical observations themselves. None of those observations is a blanket correctness certificate.

The original published head `052366249e42cfe4ad9defb109fea81c9060a0a3`, original checkout/ref, complete reachable history and source/evidence are preserved. A verified history bundle and an archive whose 22 members were read back precede the isolated refresh. The refresh's measured numerical source is `d9f7742902f8d21028fe1bf67f5c8ffada4c68f1`, rebased linearly on main `1483513fe360e7d0db93241930e9d1699f7e3fd1`. Its numerical file SHA-256 is `86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`.

The retired `crates/difftest/known_mismatches/surface.offset_surface.toml` is not restored. Current-main plan metadata is retained, including its later extension-removal update. All eight historical records are retained unchanged. No C++ code was built, linked or run for this migration.

## Fresh measurements, not blessed results

All commands used Rust 1.98.0, `CARGO_BUILD_JOBS=2`, `nice -n 10`, and the current Rust-only runners. Fixture data came from the existing local reference at pin `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`; the shared root reference was not changed. Two initial golden invocations stopped during setup because the isolated worktree lacked that fixture-data link; they measured no case output. The corrected setup changed only that ignored link.

- `cargo fmt --all --check`: pass; all 23 `surface::offset_surface::tests`: pass.
- `cargo run --profile goldens -p goldens -- --unit surface.offset_surface --goldens`: **3,182 cases, 443 changed, zero added, removed or broken, zero known failing**. This is a failed gate.
- An evidence-only `--bless --goldens-dir <temporary-directory>` measured the same 3,182 encodings outside the repository. It did not approve or replace committed expectations.
- A temporary collector used only the public registry/runner/output APIs, without kernel instrumentation, to retain every input, source, raw binary64 value/bits and golden encoding. All 3,182 encodings equal the independent evidence-directory measurement.
- Reverting exactly the four production terms, while retaining the regression, compiles and fails the intended assertion: `Suuv.x: got 1.850528767371623, want 1.740100020590324` (test exit 101). Its raw-output collector succeeds.
- The two raw runs have **3,182 unique unbroken identities and identical inputs and sources**, with no length/key changes. The four corrected terms were then restored byte-for-byte to the measured numerical source.

Raw corrected JSONL SHA-256: `735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`. Four-term-reverted JSONL SHA-256: `56cd28f6ad7ff18c4890469a148c0cfbaf49b70ff241ff9136cfbfee01d84dd1`. Temporary collector source SHA-256: `cb3b45f210f54494cee4fe3276ace13ca3ade2ac6c1d0e60bab9dfa613ffb7cb`.

## Attribution under the actual current gate

Comparison follows `crates/goldens/src/golden.rs`: arrays above 16 values are encoded in blocks of 256; block agreement uses the larger absolute sum, and 256 times that scale for the weighted sum. It is not the historical raw component-by-component comparator. The unit has no absolute floors. A Python transcription reproduces the gate's 443 changed identities; its full per-case comparison, old/new encodings, raw differences and input hashes are retained with the raw outputs.

**442 encoded changes are caused by the four terms. One encoded change, `gen1558`, is inherited:** the four-term-reverted run also disagrees with the committed golden. Corrected versus reverted encodings agree for that case under the current gate. Its `[sum, abs_sum, weighted_sum]` is:

| Committed golden | Four-term-reverted | Corrected |
|---|---|---|
| `[-3.10840560526347e21, 4.5707629311690324e21, -3.327944456358365e23]` | `[-3.108411605492941e21, 4.570766065003835e21, -3.327951246219241e23]` | `[-3.1084116056278434e21, 4.5707660648099993e21, -3.327951246351876e23]` |

That isolates inherited behavior, not its cause or correctness; no platform explanation is asserted as proved. At raw-bit level the four-term bundle moves 2,575 components across 549 cases; 513 cases have at least one component change beyond the raw `1e-9` relative rule. Neither number is the encoded gate's changed count.

Of the 443 encoded changes, historical identity groups contain 231 conditional local-jet cases, 88 unresolved-relative-jet cases, 83 unequal-order alias cases, eight pole-extrapolation cases and two base-normal-substitution cases. Another 31 identities are outside the old 518-case ledger (including the separately recorded `third_derivative_outputs` fixture). **405 of the 443** have both the same canonical input hash and the same historical changed legacy/corrected bits. Seven others in historical groups have different inputs/bits; the remaining 31 lack that ledger's record. Case names alone do not transfer historical evidence.

## Disposition and remaining work

No committed golden or known-failure entry is changed by these measurements. An inherited mismatch, a conditionally supported local jet, an explained rounding mechanism or a reproduced extrapolation is not, by itself, a correct geometric result. In particular, the alias, substitution, pole and unresolved-jet groups are not blessed, and their historical labels are not silently converted into new exemptions.

The draft correctness hold remains. Before readiness, justify each deliberately moved expectation with current old/new values and an applicable independent oracle, or demonstrate an actual remaining defect and list that case against its issue in `known_failing.json` without a golden line. Unexplained differences must remain visible. The direct default-kernel regression establishes its stated closed-form case; it does not establish all 3,182 cases, downstream callers or requested-grid correctness. The temporary collector and all measurements are archived evidence, not a new production diagnostic interface; the collector is removed before this record is committed. These measurements describe the exact named base, not the substantially advanced main at `c7b8a343`.
