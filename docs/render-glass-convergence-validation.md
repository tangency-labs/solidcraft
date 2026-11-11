# Glass convergence: measured validation

This record separates numerical correctness, actual image behavior and remaining
quality questions. It accompanies [the implementation](render-glass-convergence-implementation.md)
and [the original findings and plan](render-glass-convergence-plan.md).
The experiments below do not establish universally noiseless glass or an
independently measured bound on photon-density bias.

## Current integration

### Publication and evaluated snapshot

Implementation commit `c47c2a676c938f8d0b323a27c1fd48a83a5ffcbb` was
already present in PR #13904 on parent
`201de37b96e812faa8c21e0cad26c6be5fc78efc` when this documentation
follow-up was prepared. **The earlier CI, UI, public-render and
private-diagnostic measurements evaluate parent `1ab3a5b1` plus frozen tree
`4861c92228b0d2aecd465ad77466ea83ade51b3c`. The separate publication
snapshot gate below records its actual outcome on `c47c2a676` and parent
`201de37b9`; earlier images and UI captures retain their original identities.**

The existing rebase preserves the exact blob, type and mode delta of all
**107 implementation paths: 100 non-document paths and seven documents**.
Its 30 added and 77 modified paths have no overlap with the intervening upstream
changes. All eight documents selected for this follow-up are also identical
between the evaluated and published snapshots before this edit.

The upstream advance contains **37 commits and 395 changed paths without rename
detection**: 273 modified, 76 added and 46 deleted. Rename-aware comparison
reports 371 entries, including 24 renames. These include substantive code
changes. Every published-versus-evaluated tree difference is exactly that
upstream advance; preserving the implementation patch does not preserve the
whole tested program or qualify those upstream changes.

This follow-up retains `c47c2a676` and changes exactly eight documentation
files. All other blobs and modes are preserved from that publication commit.
The reconciliation proof `concurrent-pr-reconciliation-01/comparison-v2.json`
has SHA-256
`59f29dc3eee2bd16885efed0e29951798a54468f7e089c0c7344e50d639c6f23`.
The remaining draft acceptance gates stay open. The earlier evaluated CI
retains exit 1; the publication gate's outcome is recorded separately below.

### Publication snapshot gate

The actual publication-snapshot `cargo xtask ci` run **did not pass**. It ran on commit `c47c2a676c938f8d0b323a27c1fd48a83a5ffcbb`, above parent `201de37b96e812faa8c21e0cad26c6be5fc78efc`, with tree `85ca46cd6dbb9cf6162eb8fd3db2989bdcabab64`. The command started at 20:33:22 UTC on 2026-11-10 and exited **1** at 21:15:48 UTC after **2,545.666 seconds**, without a timeout. Before/after source identities are equal, with no unstaged changes or untracked files. The fixture guard also completed, restored the original fixture bytes, mode and inode, and reported no errors.

This was the ordinary `nice -n 10 cargo xtask ci` command with no test filters or tolerance changes: two Cargo jobs, two native test workers, four libtest threads, two action-matrix jobs, two Rayon threads, both GPU-required flags set to `1`, and both GPU selectors set to `auto`. The hardware test stages were not reached. The isolated shared-crate stage uses its official separate target directory; subsequent workspace commands were configured to use the dedicated publication target. The recorded wall time includes compilation.

Formatting, asset attribution, icon, licence, list, skill and layer checks completed. The first three isolated shared-crate commands passed; the fourth stopped at a UI-kit library test:

| Official isolated workspace | Library tests passed | Failed | Result |
|---|---:|---:|---|
| `solidcraft-color` | 3 | 0 | Passed |
| `solidcraft-control` | 25 | 0 | Passed |
| `solidcraft-control-egui` | 31 | 0 | Passed, including its copied control crate |
| `solidcraft-look` | 330 | 1 | Stopped after the UI-kit library reported **294 passed / 1 failed / 0 ignored** |

These are executed library-test counts within the isolated commands. Some crates recur in more than one isolated workspace. Within `look`, color, look and theme completed before UI-kit failed; the other member binaries were not run. These are not full-workspace native totals. The remaining eight shared-crate commands, UI-container and mesh checks, workspace Clippy, full native dispatch, Wasm and doctest side jobs, and the full gate's web-offline stage were **not reached**. The original native-accounting analyzer consequently preserves `native_counts: null` and its missing-dispatch diagnostic; the separate early-stage analysis records the actual results above.

#### Isolated upstream UI test blocker

The failing test is `menu::tests::a_menu_row_draws_a_hosts_texture_and_an_item_still_reads_as_its_kit_name`. Its complete diagnostic reports an `epaint` 0.37.0 `TexturesDelta` drop assertion: one texture update remains unapplied. The test calls `run_ui` and retains its output without clearing the texture delta; the nearby menu test and the kit's `raw_frame` helper clear those deltas. The test uses the kit's embedded English catalog, egui's bundled default fonts and a synthetic host texture ID. The reviewed path loads no external model, image or host-font fixture.

This test was introduced by upstream commit `46bb71b5e1707b7a93712dd52f9367372ad22ca6`; it is absent from the earlier `1ab`/`4861` source. Its file is identical in parent `201de` and publication commit `c47`. The input-equivalence review goes beyond the test text: all **8,607 entries** in the reviewed repository input superset have identical blobs, types and modes between those two commits, with no overlap with the 107 implementation paths. Both reconstructed shared-dependency closures and generated manifest agree; all **593 copied source files** match parent blobs and modes. The standalone lock resolves **240 packages**; all package identities and applicable registry checksums occur in the identical parent/head root lock. The five egui text/font packages were checked against their checksum-authenticated cached archives. The sole local build script's 381 declared inputs and 403 generated embedded-file references stay within the copied look crate and match the parent inputs.

A separately recorded replay of the retained UI-kit test binary, using the standalone package directory and recorded policy with the official shared-target override, reproduced the failure in **0.016589 seconds**: **0 passed / 1 failed / 294 filtered**, exit **101**, no timeout, unchanged source and binary. The complete failure bodies are byte-identical after replacing only the single thread PID in each body. This is an **isolated reproduction with parent-identical source/dependency inputs**. It is not a fresh whole-parent build or a new full-suite parent qualification.

The 24 failures from the earlier full native run were not exercised by this early-stopped gate. Their earlier `1ab` reproduction records retain that scope; this run supplies no new result for those tests.

#### Focused publication renderer checks

Separate native renderer-library checks completed on the same unchanged `c47`/`85ca` source. Both exited **0**, without a timeout or test-name filter; both reported zero measured tests and zero filtered tests.

| Focused library | Passed | Failed | Ignored | Libtest execution time |
|---|---:|---:|---:|---:|
| CPU renderer, `solidcraft-render --lib --features raytrace-cpu` | **934** | **0** | **2** | 4.36 s |
| GPU renderer, `solidcraft-gpu --lib` | **268** | **0** | **3** | 71.17 s |

The CPU command used `--locked --profile ci --jobs 2` and took **451.118949 seconds including compilation**, finishing at 21:56:29 UTC. Its two ignored tests are the recorded manual sampler and parallel-timing probes. The GPU library was built separately with the locked `ci` profile and `solidcraft-engine/profile,solidcraft-engine/raytrace-cpu`; the build-only command took **1,065.826489 seconds**. Cargo identified one test executable, which was hashed before and after its independent run. That runtime command took **71.201076 seconds**, finishing at 21:49:47 UTC. Its three ignored tests are `glass_transport_evidence`, `path_performance` and `bench_plate`.

For this GPU runtime, the executed command explicitly overrode both GPU selectors to `hardware` and used `--nocapture`; the required flags remained set to `1`. The retained log contains **165** identical path-adapter identifications for **AMD Radeon 780M Graphics (RADV PHOENIX), IntegratedGpu, Vulkan**, plus four matching named-GPU lines and no device-unavailable skip messages. This supplies direct hardware-adapter evidence for this focused current-snapshot run. It is separate from the earlier CI05 result under auto selectors, whose successful adapter output was not retained.

#### Publication UI integration type-check

The native main UI library and its test targets also passed a separate compile-only check on unchanged `c47`/`85ca`: `cargo check --locked --profile ci --jobs 2 -p solidcraft-ui-egui --lib --tests --features solidcraft-engine/profile,solidcraft-engine/raytrace-cpu --message-format=json-render-diagnostics`, run under `nice -n 10`. It exited **0**, without a timeout, after **257.835059 seconds**, finishing at 22:03:31 UTC.

Cargo reported successful checking of the normal UI library, its unit-test library target and **21 integration-test targets**. All 23 library/test artifacts have `executable: null`, and the terminal Cargo record reports `build-finished: success`. This checks the rebased UI's native library/test code for compilation compatibility. **No UI tests were executed**, so there is no new UI test-pass total. These focused renderer and UI checks do not complete the failed full gate.

#### Separate publication web-offline check

A separate `nice -n 10 cargo xtask web-offline-test` invocation on the same unchanged `c47`/`85ca` source passed **all 20 logged scenarios**, exited **0**, and completed in **47.278860 seconds** without a timeout at 21:26:59 UTC. It used the pinned isolated Chrome and the recorded policy. The scenarios cover online/offline startup, incomplete and overlapping updates, retirement of cached builds, multiple deployment paths, sign-in interception and development builds.

This is the repository's default stand-in shell check of its current registration/service-worker behavior, without `--site`. It does not establish real-build application/API behavior and does not complete the failed full gate.

#### Publication evidence identifiers

The completed evidence is retained under the labels below. SHA-256 values identify the actual source-pinned receipts and analyses; the earlier `4861` image and CI evidence remains separately scoped below.

| Artifact | SHA-256 |
|---|---|
| `ci-publication-c47-01/receipt.json` | `47e6ded717b8705a18ed6440f16f0083a7294a9d391fecdd70469fbbef593c5d` |
| `ci-publication-c47-01/output.log` | `926228846fad90beaf123f8d11b8ea960d9b99be432189b86cdf973bbf2c4d90` |
| Publication fixture-guard receipt | `c0fd44f85b2e5f0fbb7d948c74b6b9174d8753ac27119ebf6c30bad987f6339a` |
| Original native parser summary | `6ab61f5aeda71a95c04c1e1344909a9201a04442d3ab431e0494f63d65dc74df` |
| Early-stage analysis summary | `d45cc4805d94ea63d352d359c9da81b1eb3fbe49cc7c6c1e9e05751e85b2c045` |
| Complete parent-input equivalence proof | `e2ac90875635136f5a9c2746201b7111b23a0e648098e565af65af38ad371349` |
| Isolated replay receipt | `0834a29b5a57b095ceca4cb88b8ae88b76b43e0b1904c334ea1991feae10a0c7` |
| PID-only replay comparison | `8f6f5eadb14f1705e41ed2d8948351cf2c6f29cf28b5afaf9bd7c6fc82deaf4e` |
| GPU build-only receipt | `91c7cdd33ee3265c7c37f8331bd7d1c92f41eab02ce30bd049b9d7a199b3c93c` |
| GPU build-only log | `9dc89bce088d1ca7345dec7037a21499a4eaf14903270f03fd26c9eeee34ed93` |
| Focused GPU runtime receipt | `21fd777a0396411b4d8ebdb2cb111fce410350326f0c5bb1ca29df2e9ce5d389` |
| Focused GPU runtime log | `2bc648ab1fb92f5cdf8b214209aca308d24f880ca4c7f99eb6eb39d6217fa4ac` |
| Focused GPU test executable | `eb49f1618cf025485257fb1a244951724bfe7605c3a71d8d34d856b6b515a422` |
| Focused CPU command receipt | `7b4c246d6a98fe82b032cc8ea8b9867188e2b37f79af38a642580e07d51bc394` |
| Focused CPU command log | `79c2490bbac94f7dc4e60e16ec8dcc0cb0f6e56ac47c0ec2a19ede4250d30795` |
| Focused CPU test executable | `f967ef45db80fb83f8d5ace7fd3120a25ff6d8832459b41578b7ad7443a24218` |
| UI compile-only receipt | `3a759f69d0481b93d48e9ca11464392f7a46e3b2ad3f11b7a3dca40dd5eb7d35` |
| UI compile-only log | `8a33cb71872fa3f7ab753d8e2714fb44a5111a330dbdf884036e7168c9500e28` |
| Separate web-offline receipt | `465ea549332087ac7871a2a5cf019529adeca3c7bea34175f63458d484152894` |
| Separate web-offline log | `5aa7290195b7f4dbc578da1835623319669062ddc16b66a4a6fd7041b53c769a` |

### Earlier evaluated snapshot

The evaluated implementation was integrated on pinned main
`1ab3a5b1796564bc76dc57a513115f521edd0e90`. The preceding `a3475683` restack
reconciled 20 of the 106 implementation paths with upstream changes since
`42918fe5`, preserving render UI, backend recovery and the optional GPU indirect
clamp. The final `a3475683` to `1ab3a5b1` delta contains 15 commits and 1,073
added, modified or deleted paths, counting both sides of renames, with **zero
overlap** with the implementation's 106 paths.

Main now supplies icons, glyphs, fonts and themes through the shared
`solidcraft-look` crate. This restack preserves that setup. Its dependency delta
adds the workspace crate without changing existing package identities. The
public UI driver still uses the supported bundled-font API; its actual capture
must use libraries and fingerprints built from the selected source.

Before adapting these documents, all 106 recovered implementation file blobs
and worktree byte hashes exactly matched the retained `a3475683` tree
`0513ae496b5feecc3681a5801643ac538054fb53`. The remaining tree delta was exactly
the selected upstream delta. An explicit task stash retains the original source;
no existing user stash was changed. The restack receipt below records that proof.
This preservation statement describes the transfer itself. The later three-file
GPU producer correction and formatter-only reflow have separate source records
in the current qualification below.

### Earlier evaluated snapshot gate

The evaluated snapshot's `cargo xtask ci` run completed on 10 November 2026 with **exit 1**:
**15,248 passed, 24 failed and 71 ignored tests across all 104 native binaries**.
Ninety-eight binaries succeeded and six failed. All 24 remaining failure
identities and diagnostics are accounted for by actual targeted runs on pristine
parent `1ab3a5b1`, with the comparison qualifications below. **The full gate did
not pass.**

This run, `ci-main-1ab-05`, binds frozen source tree
`4861c92228b0d2aecd465ad77466ea83ade51b3c` to pinned main
`1ab3a5b1796564bc76dc57a513115f521edd0e90`. It ran from **17:58:42.949905 to
19:24:54.239770 UTC**, taking **5,171.249 seconds**, without a timeout or source
change. The native-test phase took 1,845.9 seconds. Formatting, shared-crate
isolation, Clippy with `-D warnings`, and the workspace test build passed before
native dispatch. Two top-level jobs, four test threads, two action-matrix workers
and two Rayon workers bounded concurrency. GPU availability was required
(`SOLIDCRAFT_REQUIRE_GPU=1`, `VIEWPORT_TEST_REQUIRE_GPU=1`), with both
adapter selectors set to `auto`. The full-CI log retains the passing GPU
library summary but not its adapter identity; it does not establish a
hardware-only selection policy for that run.

| Evaluated snapshot check | Passed | Failed | Ignored | Scope |
| --- | ---: | ---: | ---: | --- |
| All native binaries | 15,248 | 24 | 71 | All 104 binaries; the rows below are included in this total |
| CPU renderer library | 934 | 0 | 2 | Renderer contracts and CPU execution |
| GPU library | 268 | 0 | 3 | Complete unfiltered library suite; GPU required, selectors `auto`; adapter identity not retained in the full-CI log |
| Main egui UI library | 5,226 | 0 | 22 | UI library tests; separate control captures have their own evidence |
| Boolean property tests | 16 | 0 | 2 | This fresh property-test run passed |
| WebAssembly check | — | — | — | All 27 crates passed in 91.4 seconds |
| Documentation tests | — | — | — | Side job passed in 22.8 seconds; the retained output supplies no aggregate doctest count |

The totals count one final outer summary per dispatched native binary. They
exclude the earlier shared-crate isolation results and both side jobs. The gate
stopped after the native failures and did not dispatch `web-offline-test`; it
supplies no standalone web-offline result.

A separate guard kept the existing Boolean regression fixture readable but
nonwritable during this run, preventing a failing property test from appending
to tracked source. It preserved the fixture bytes, existing seed replay, test
case policy and failure behavior. The guard restored the original `0600` mode,
inode and hash afterward; its own source check passed and its error list is
empty. The original command and actual nonzero result remain unchanged.

### Remaining failures and parent reproductions

The six failing binaries contain eight kernel-library failures, eleven
engine-library failures, one SMLib-library failure, one naming-corpus failure,
one action-matrix failure and two pleat-regression failures. Separate targeted
runs reproduced all 24 on pristine parent `1ab3a5b1`. They used a dedicated
parent build target, actual Cargo-reported executables, and matching native
feature, profile and toolchain fingerprints. These targeted runs do not claim
a complete parent-workspace CI pass.

All 23 non-action failure bodies match the earlier integrated CI03 run after
normalizing panic process IDs and whitespace. Twenty also match the parent
under those same rules; three additionally differ only by the incidental
backtrace hint. The action matrix retains **138 identical case identities and
per-case counts, totaling 1,359 outcomes**, plus an identical complete remaining
failure diagnostic and summary. Its elapsed/load timings and parallel progress
order differ. The original strict action-body comparison remains false; the
separate structured comparison accounts for those differences without making a
performance claim. No CI05 failure is left without a corresponding parent
reproduction.

The earlier CI03 `scale_invariant` counterexample was checked separately. Parent
production and test Rust code reproduced the exact diagnostic and complete
minimal input after adding only the retained 210-byte regression-fixture entry.
That replay used zero new random cases and the test's unchanged 200-shrink
limit; it failed with exit 101. It is **parent code with retained input**, not a
pristine whole-checkout run. The original strict surrounding-body comparison
remains false because of the documented save notices, success counter and
`--nocapture` wrapper. Only the known fixture was then restored, returning the
parent checkout to clean tree `d3916d65206e9e5d66dacab0815b85f02ee303e5`.
CI05's 16 passing fresh Boolean tests are a distinct result, not a claimed code
fix for that counterexample.

### GPU producer correction

CI03 exposed 15 GPU tests rejecting invalid path radiance. Three unchanged
controls—opaque box with coat/maps, fixed-seed furnace and indirect clamp—were
replayed on the tested RADV PHOENIX adapter:

| Source and producer | Box | Furnace | Clamp |
| --- | --- | --- | --- |
| Pristine parent `1ab3a5b1` | Pass | Pass | Pass |
| Integrated `3a88dee1`, opaque specialization enabled, one medium entry | Fail | Fail | Fail |
| Diagnostic `4fac244f`, opaque specialization enabled, sixteen entries | Fail | Fail | Fail |
| Diagnostic `d6d6ae54`, sixteen entries, both opaque overrides disabled | Pass | Pass | Pass |

Increasing stack capacity alone did not resolve the failure. With both opaque
overrides disabled, all sixteen printed box raw-float fingerprints and the
furnace centre matched the parent exactly. The clamp control passed the same
assertions; no separate whole-image equality comparison was recorded for it.
The guard reports invalid radiance without distinguishing nonfinite from
negative values. These checks isolate dependence on specialization; they do
not establish a compiler or driver cause, or qualify opaque throughput.

The retained fix uses one shared sixteen-medium scene shader with
`OPAQUE_SCENE=0`. Scene classification, cache routing, admission, texture/clamp
flags and transport formulas remain intact. The signed-primary-hook comparison
uses the actual production shader while retaining its eight cases and existing
source, uniform, classification and image-prefix checks.

The complete focused GPU suite passed **268 tests, zero failed and three
ignored**, with no filters and unchanged source
`9fdbe15d90d9c7b84a273f837c32188019c5c4a6`. Its retained `--nocapture` log
identifies the path executor as AMD Radeon 780M Graphics (RADV PHOENIX), an
integrated GPU using Vulkan. Its selectors were `auto`; these are direct
observations for that focused run. All fifteen CI03 GPU failures passed.
The three existing ignored tests cover glass-transport evidence, path performance
and the plate benchmark. The later mapping to final tree `4861c922` changes
only one descriptor's rustfmt layout and an optional trailing comma; the retained
receipt explicitly records that this is not a whitespace-only change. CI05
independently reran the complete GPU library on that final tree with the same
268/0/3 outcome. Neither suite duration is a throughput benchmark.

### Earlier attempts on the same pinned dependency

| Attempt | Source tree | Actual retained outcome |
| --- | --- | --- |
| `ci-main-1ab-03` | `3a88dee1abafe5002304853bad5ee80514a7a339` | Exit 1 after 5,741.417 seconds; 15,232 passed, 40 failed, 71 ignored across 104 native binaries. Fifteen GPU failures and the Boolean counterexample are absent from CI05's failure list. |
| `ci-main-1ab-04` | `9fdbe15d90d9c7b84a273f837c32188019c5c4a6` | Exit 1 after 26.016 seconds at the formatting check. No native tests or side jobs were dispatched; there is no native test total for this attempt. |

CI03's original receipt retains **`source_unchanged=false`**: the property test
appended its counterexample to the tracked regression fixture while HEAD and
the index stayed fixed. The before/after fixture, patch and original receipt
were preserved, and only that generated append was restored. This later
restoration does not turn CI03 into a source-stable gate. CI04 was source-stable
and did not time out; its formatting stop preceded the separately recorded
formatter correction and final CI05 run. These attempts retain their own source
identities and do not qualify final-source image results.

### Current gate evidence identifiers

These relative labels identify retained external evidence, not files committed
to the repository. Hashes bind the final outcome and comparisons; the named
reports retain the underlying command, artifact and failure-body records.

| Artifact | SHA-256 |
| --- | --- |
| `ci-main-1ab-05/receipt.json` | `503eb988de4ab1407d4130a1009e711a0bbdcc3f39f3674bc05c262218d25fc6` |
| `ci-main-1ab-05/output.log` | `dff7460fd8b13edf9d178c4db8df57654fa1ed97b04dbe221894846f99149a02` |
| `ci-main-1ab-05-fixture-protection.json` | `8514f3089a989afdd81e6ecaec4142cf10378af9cbd1905f7f231c552ffe8d59` |
| `ci-main-1ab-05-analysis/summary.json` | `9f490a9db47a43519abe9b6bf4f72502bf020175495df07b804c94c2497f1a49` |
| `ci-main-1ab-05-parent-comparison/summary.json` | `3762c7193232e75c45765246007dc2427d9b4f9804ccde49f3b09405f613f264` |
| `gpu-opaque-off-diagnostic-01/controls-and-comparison.json` | `635c9aa81c3580bcc1bddc5da1406ec65d828d1f47aeddf276ecd5e5ac9e982f` |
| `gpu-full-producer-final-01/qualification.json` | `a7a8760d73e721328b7d44c3e6496a96ebfdf73ffa86f9e3fc109a99b85f82ef` |
| `gpu-final-format-01/receipt.json` | `34854c248e359df5aed46936971b979d34b1c6ab7fb0f57dbd6c305921e72709` |
| `parent-repro-analysis-01/parent-main-1ab-boolean-retained-input-01/retained-input-semantic-comparison.json` | `ce06ca171163d33d246c068be18cce02ad2c62836d316653cc2d1af260ffa1d2` |
| `parent-retained-input-preparation-01/restoration.json` | `3559e1bd64e89afd4c815a6538714295c4a60c1774e032a64a490a437a501d94` |
| `ci-main-1ab-03/receipt.json` | `0461228a5aa8b4fe5da9f6446f6556965a7a21d3d292b09cdbfc904f46f5ff52` |
| `ci-main-1ab-04/receipt.json` | `ccb91a39885846b941729fd6d4b0722ff13e200fd893a6865e1a2463423e0716` |

### Current application-control capture

The accepted capture, `ui-20261110T193345Z-3nnfy7i4`, completed on
10 November 2026 at **19:34:55.202430 UTC** with unchanged source tree
`4861c92228b0d2aecd465ad77466ea83ade51b3c`. Library discovery, linking the
external driver and capturing the controls all passed. The driver used the
application's actual egui meshes, texture uploads and bundled fonts on a new
empty in-memory design. Live rendering was paused, no user session or private
design was loaded, and no image job ran.

Both **476 × 1,599** light-theme panels were visually inspected. Labels and
explanations were readable, with no overlap, clipping or text elision. Five
integration controls were verified for each backend:

| Control | CPU view | GPU view |
| --- | --- | --- |
| Point/directional caustics | Enabled | Enabled |
| Area-light caustics | Enabled | Enabled |
| Use camera paths for glossy caustics | Enabled | Enabled |
| Maximum base roughness | 0.35 | 0.35 |
| Trace both glass branches | Enabled | Enabled |

The demonstration keeps one Fresnel stratum while splitting is enabled. Saved
state preserves the separately authored GPU `clamp_indirect=8` through paint
and Glass-control edits. That clamp has no visible widget in these captures;
photon probes remain absent. The control receipt verifies both state and
visible bounds.

These images show **actual application controls rendered in software**. The
capture helper skips GPU callback primitives, so its scope excludes a native
presented window and GPU viewport rendering. Both panels were accepted for
public attachment after visual review; this capture record does not assert
that an upload has completed.

The first attempt, `ui-20261110T192503Z-utje877s`, stopped before linking or
capture when its unchanged provenance guard detected five rebuilt registry TLS
libraries. Source stayed unchanged. Dependency analysis placed all five outside
the driver's complete **414-node** library, procedural-macro and build-script
closure; no local normal library was rebuilt. Build-script fingerprints differed
in inherited Cargo package environment between nested xtask execution and direct
discovery. That analysis uses a preserved pre-CI05 fingerprint, without claiming
a post-CI05/pre-discovery snapshot.

The successful retry used the original helper, the same discovery command and
the existing guard. All 448 normal libraries were fresh. **All six direct rlib
hashes and twelve fingerprint-file hashes matched the first discovery**; the
first failed receipt remains unchanged. This establishes the capture's library
provenance without relaxing the guard or changing implementation source.

### Current standalone web-offline check

`web-offline-main-1ab-01` completed with **exit 0**, no timeout and unchanged
source tree `4861c92228b0d2aecd465ad77466ea83ade51b3c`. The separate
`cargo xtask web-offline-test` run took **60.692524 seconds**, from
19:35:43.285265 to 19:36:44.016420 UTC. Its full log records **20 passing
scenarios** and the terminal `web-offline-test: passed` result.

The run explicitly selected isolated official **Chrome for Testing
157.0.8103.50** through `CHROME`. It exercised initial online/offline launch,
failed and interrupted updates, multiple paths/tabs, slow networks, cache
retirement, overlapping deployments, missing deployments, sign-in redirects
and development-build behavior.

This was the default small stand-in application, using the production web
application's service-worker registration and `web-offline` generator. It
qualifies those service-worker/cache/update scenarios. A real built web
application was not supplied through `--site`, so this run does not establish
end-to-end application or API behavior. The final CI's separate 27-crate
WebAssembly compilation remains a distinct successful check. Neither result
changes the full workspace gate's exit 1.

### UI and web evidence identifiers

These relative labels identify retained external artifacts. The two control
PNGs contain the public empty-design fixture; their visual-review receipt
records acceptance for attachment separately from any later publication.

| Artifact | SHA-256 |
| --- | --- |
| `ui-20261110T193345Z-3nnfy7i4/receipt.json` | `0903a77d8b0d49321fee7ac7656dc217291d55e55ee64a1fa7314d7357919eeb` |
| `ui-20261110T193345Z-3nnfy7i4/captures/controls.json` | `5d0f263af0a01ea8704edbaeb8de3fc228b072d0375063cb9727c1e9a5faa4a2` |
| `ui-discovery-provenance-01/retry-comparison.json` | `f718a5879a0a6bb147203501dd7bc17a537868dd2e940f1426ab23d281049d2a` |
| `ui-final-visual-review-01.json` | `acdb5ac9e1b91d890337ff7c3132ca42607b722d50f1af5b3b76b3d5915fcfa5` |
| `ui-20261110T193345Z-3nnfy7i4/captures/cpu-glass-controls-light.png` | `be673ff5f07e7e1c66023f38dca61c3e292773d1bd77160e0944435e38bc3691` |
| `ui-20261110T193345Z-3nnfy7i4/captures/gpu-glass-controls-light.png` | `dd09eb687526a984b830f9c645400538cd7950979a7ddec7a4960fc01f8d3516` |
| `web-offline-main-1ab-01/receipt.json` | `306fb5e5efea59fd0e5a18107d8868e63fd5a74319cddbcb9f48d47f6f789f3c` |
| `web-offline-main-1ab-01/output.log` | `b8f70d3654cc1a23da1f627d343e919bb21e0ab8385c0b2f2548952b87bb9727` |
| `chrome-for-testing-157.0.8103.50/receipt.json` | `68f565a30ef7baf531ff96e82d117d0f88478d39e339e651dad738afcce323d1` |

### Current public evidence

The complete public suite finished on 2026-11-10 at 19:58:45 UTC on the frozen implementation tree `4861c92228b0d2aecd465ad77466ea83ade51b3c`, above parent `1ab3a5b1796564bc76dc57a513115f521edd0e90`. Both example builds and all four execution commands exited zero; source checks before and after each command matched. The actual Cargo-produced executables were copied and hashed before use. The clear comparison used the CPU renderer and required hardware GPU; the receiver and split matrices used the CPU renderer.

#### Clear reconstruction isolation

Both reconstruction modes rendered 256 × 192 pixels at 32 samples per pixel. Within each backend, the decoded FLOAT32 raw RGBA and the complete EXR file bytes were identical between off and filter. Display alpha remained opaque and exact; only display RGB changed. All raw RGB values were finite and nonnegative, with coverage in [0, 1]. Each captured frame reported 49,152 pixels and 1,572,864 complete observations across the six additive components.

| Backend | Display RGB pixels changed | Maximum component recombination error | Fixed absolute limit | Optical-guide pixels |
|---|---:|---:|---:|---:|
| CPU | 38,193 | 5.5511151e-16 | 1e-10 | 6,441 |
| GPU | 38,355 | 4.7683716e-07 | 1e-05 | 6,441 |

Aggregate component sums matched reported beauty at the unchanged relative limits of 1e-10 for CPU and 1e-6 for GPU; raw EXR sums matched beauty at 1e-6. The limits were not widened. The combined CPU/GPU off command took 17.174 seconds and the filter command 33.705 seconds; these are command times for different capture/filter workloads, not a throughput comparison.

Visual inspection of all four retained PNGs found the checkerboard and clear object intact, smoother surroundings after filtering, and remaining noise in the refracted region. The GPU images also retain a black polygon at the upper-left edge in both modes. Source inspection and projection of the recorded rectangular light identify this as its non-emitting back face: GPU emitter intersections are two-sided and terminate with coverage one when back-face emission is zero, while CPU rectangle intersections reject the back face. Both policies already exist on parent 1ab3a5b; the relevant intersection and PDF functions are unchanged. The GPU polygon contains 1,041 fully black pixels, and the guide classifies 1,072 emitter pixels. This backend visibility difference is disclosed rather than treated as backend parity. The pictures establish neither physical accuracy nor native-window behavior; they are headless public-fixture evidence.

#### Independently recomputed strategy matrices

A separate pinned analyzer decoded all 320 FLOAT32 EXRs and recomputed pixelwise RGB and luminance means and sample variances across eight randomizations with float64 Welford accumulation. Every producer group statistic matched; all raw values and coverage passed the stated validity checks. The 128-image receiver matrix covers six cases and 16 groups at 32 complete camera observations per pixel with 4,096 configured photons per fixed map. All 48 receiver RGBA pairs were bitwise exact: four supported glossy/polished cases matched their camera-reference estimator, and the diffuse and hard-source controls matched unchanged gather. Gather differed from the camera reference in all eight randomizations of each supported case, so those checks exercised the selected continuation path.

The 192-image split matrix covers four cases and 24 groups at 32 requested lane evaluations per pixel. Independent and Sobol variants use 32 complete observations; two strata and first-interface split use 16; four strata use eight. The configured map budget is one photon, so this matrix targets camera sampling rather than photon-map convergence. Both matrices use 32 × 32 images and one CPU worker. The receiver command took 125.326 seconds and the split command 127.726 seconds.

In the following table, variance is the frame average of the per-pixel variance across the eight images, and mean change is the relative change in frame-averaged luminance. Receiver rows compare continuation with gather; sampling rows compare the named strategy with independent sampling. Seconds are measured means per render. The complete 40-group table, including all measured strategies and timings, is retained in the independent analysis.

| Matrix / case | Strategy | Luminance variance ratio | Mean luminance change | Baseline seconds | Strategy seconds |
|---|---|---:|---:|---:|---:|
| Receiver / area glossy | continue-glossy | 3.77976 | +0.09566% | 1.0326 | 0.9848 |
| Receiver / area polished | continue-glossy | 0.216373 | +2.03744% | 1.0523 | 1.0169 |
| Receiver / environment glossy | continue-glossy | 0.966563 | +0.00364% | 0.7720 | 0.7258 |
| Receiver / environment polished | continue-glossy | 0.000189852 | -0.84729% | 0.7111 | 0.6828 |
| Sampling / area diffuse | sobol | 1.00456 | -1.48484% | 0.6128 | 0.6300 |
| Sampling / area diffuse | split | 1.55393 | -0.25648% | 0.6128 | 0.5933 |
| Sampling / area glossy | sobol | 0.657507 | -1.10459% | 0.6124 | 0.6394 |
| Sampling / area glossy | split | 1.54183 | -0.52108% | 0.6124 | 0.5661 |
| Sampling / environment polished | sobol | 0.52935 | +0.02125% | 0.5279 | 0.5502 |
| Sampling / environment polished | split | 1.74762 | -0.00276% | 0.5279 | 0.4884 |
| Sampling / dispersive area | sobol | 0.830184 | -0.14601% | 0.6255 | 0.6431 |
| Sampling / dispersive area | split | 1.56382 | -0.39563% | 0.6255 | 0.5832 |

Two-strata variance ratios were 1.3359–1.6782, and four-strata ratios were 2.4900–3.4395; all exceeded one in these four cases. Sobol plus splitting had ratios 1.39827, 0.92414, 0.91771, 1.0138 in the same case order. Thus continuation and Sobol have mixed measured outcomes, while first-interface splitting and extra strata did not improve this matched-requested-lane experiment. All remain opt-in.

Eight randomizations do not establish rare-event tails, expected energy, density bias, or error against an independent physical reference. The large polished-environment improvement and the area-glossy worsening are both retained, including their finite-seed mean shifts. The receiver reference shares geometry and geometric-normal policy, so exact pairing establishes accounting identity only. Requested observations and lanes do not match actual traversal work or elapsed time; timing was measured but not calibrated or held equal. Variance multiplied by elapsed time is only a descriptive proxy. These results do not qualify equal-time efficiency, GPU throughput, general denoising quality, or production rough-transmission footprints, and they do not change the full CI exit status of one.

#### Public evidence identifiers

The following retained artifact SHA-256 values identify this current run; older evidence remains historical.

| Artifact | SHA-256 |
|---|---|
| `public-20261110T193738Z-mf87czdu/receipt.json` | `035baf8818882cf6ab1f95929b446227b0e6c629d93baf28bcb58775127bc678` |
| `public-20261110T193738Z-mf87czdu/analysis.json` | `01363543ad4d22275553c3851139ea1ddf7b95c4ea7197da0ef8a747d7635e22` |
| `public-20261110T193738Z-mf87czdu/artifacts.json` | `da1837caad14442f26e497a318c1750496eef8b7af5d19639d99d7c89269cf0c` |
| `public-strategies-independent-01/summary.json` | `4a14169107eac393348f26d4629eafa8a573ca66278238c137470d23da43a9e0` |
| `public-strategies-independent-01/input-manifest.json` | `36fa5a666ed2cc336b150b2e21eadabf2ae0bf5a949dd32bb13404b7badd3eed` |
| `public-clear-classification-01/summary.json` | `9e26e8cfad514911c0d292bbd4a69d92d336bb8ab6feace7a5a24ed8f43ae0f2` |
| `public-clear-classification-01/parent-policy-comparison-v2.json` | `a82a71b432d049179442697077c9804178696c187afdcbee81d940e4652b80f6` |
| CPU clear EXR, identical in both reconstruction modes | `48a7ce8bfa20eb578757bfcdbd46d7ea919fcfb3620276f8ce16bc613c966835` |
| GPU clear EXR, identical in both reconstruction modes | `4b375dbd558680917fba2e56d213a7c36ae38aaf4ab503bca131f35a1d0062b7` |

### Current private diagnostics

These measurements belong to evaluated index tree `4861c92228b0d2aecd465ad77466ea83ade51b3c` on parent `1ab3a5b1796564bc76dc57a513115f521edd0e90`. They do not establish fresh validation of the later publication rebase. The six diagnostics completed with unchanged evaluated source and the prescribed controls. Decoded raw exports contained finite, non-negative RGB and valid coverage; the selected-pixel estimator ledgers passed their existing closure checks.

The two groups vary different factors. Reference runs A/E/F vary **map A/B/C while holding camera A fixed**. Continuation runs B/C/D vary **camera A/B/C while holding map A fixed**. The groups support descriptive map sensitivity and camera scatter. They are not an off/on variance-reduction experiment.

| Run | Comparison | Map | Camera | Original selected-pixel Y | Actual global maximum Y |
| --- | --- | --- | --- | ---: | ---: |
| A | Reference | A | A | 2,877.519255 | 6,154.899696 |
| B | Continuation | A | A | 0.636577 | 6,154.899696 |
| C | Continuation | A | B | 2.206454 | 2,806.913149 |
| D | Continuation | A | C | 1.670677 | 4,934.413172 |
| E | Reference | B | A | 0.492024 | 6,154.899696 |
| F | Reference | C | A | 0.492024 | 6,154.899696 |

Here Y is scene-linear `0.2126 R + 0.7152 G + 0.0722 B`, computed in float64 from the retained raw output. The table is rounded for readability.

At the original selected pixel, A's photon estimator contributed Y = 2,877.027135 and the other estimators contributed Y = 0.492024. In B these totals were 0.144553 and 0.492024. The photon shares of accounted beauty were approximately 99.9829% and 22.7079%. Reference map variants E/F also had zero photon contribution at this selected point; their other-estimator total remained 0.492024. C/D had zero selected-point photon contribution, with their other-estimator totals accounting for their selected values.

These are complete **photon versus all other estimators** totals at the original selected point. They are not a physical caustic/non-caustic partition or a complete six-component decomposition. Retained photon event-family totals are bounded summaries; their numerical remainder can include rounding error. Rebuilt maps may also change GPU atomic storage order. The selected-point reduction therefore does not establish a general quality, energy, or tail improvement.

A separate raw reader examined **every run's actual global luminance maximum**, including the baseline and stationary maxima. Each run had one unique maximum. A/B/E/F shared the same location, raw RGBA and Y; C and D had separate maximum locations. None of the six global maxima was at the original selected probe. The original selected-point ledgers consequently do not attribute those maxima.

All six complete-frame inspection overviews and all six actual-maximum figures were visually reviewed. The maximum figures pair original display pixels with exact raw Y neighborhoods. Around the shared A/B/E/F maximum, all eight neighboring Y values were below 3; around C they were below 0.57; around D they were below 2.54. These are isolated raw peaks. Their physical path families were not inferred from visual appearance. Grain and isolated bright points remained visible in the overviews.

The mean per-pixel luminance sample variance was 0.453543040 for the reference map A/B/C group with camera A fixed. It was 5.528541577 for the continuation camera A/B/C group with map A fixed. These statistics measure different axes and cannot be divided into an off/on variance-reduction ratio. Mean image Y changed by +0.0000395453 from A to B (`mean(Y_B) - mean(Y_A)`, in absolute scene-linear Y units), with unchanged coverage. Image-space luminance summaries do not establish integrated physical energy, bias, calibrated confidence, equal-time efficiency or a general rare-event bound.

#### Targeted measurements at retained maxima

The targeted repeats at the three selected maximum locations A/C/D each reproduced their original float32 RGBA bits exactly. Each selected location remained the fresh frame's unique global maximum. In all three **fresh** ledgers, photon RGB was zero, the other estimators equaled raw beauty, and the RGB recombination residual was zero.

| Targeted repeat | Retained maximum Y | Fresh Y at that location | Fresh photon Y | Fresh other-estimator Y |
| --- | ---: | ---: | ---: | ---: |
| A | 6,154.899696 | 6,154.899696 | 0 | 6,154.899696 |
| C | 2,806.913149 | 2,806.913149 | 0 | 2,806.913149 |
| D | 4,934.413172 | 4,934.413172 | 0 | 4,934.413172 |

These ledgers establish estimator partitions of the fresh measurements. They do not identify the physical subfamilies of the other-estimator contributions. Equal raw values do not establish identical paths or full-image identity, and fresh ledgers are not transferred to any of the original six captures. Runs B/E/F were not separately reprobed at their maxima. The original reports' maximum-attribution status remains incomplete.

Completed-evidence SHA256 bindings:

| Evidence | SHA256 |
| --- | --- |
| Six-run protocol receipt | `1698a7d3a94e6e74061d95e6b5d49fcf62d1cd83cf93705c6dc1248fab51b03f` |
| Every-maximum raw review | `246f5640c7c805f4367a2da3c30205465278cfc0360171c1a6113e053ffb612b` |
| Complete selected-point estimator ledger | `307bfbc3370d6d7cb9fa75c86745bb0e46aaa5b6e2517dc5d1ec0b2e78cea41b` |
| Root visual inspection completion | `612f174da6b5d39aafa23296938bac5b47adef534f5168dce4f1d9bf51a156d1` |
| Targeted A receipt | `a1556cb7cf1cdf88062594939081671b0478ec6702369f482bb1ac2896edd9fa` |
| Targeted A raw comparison and fresh ledger | `e7ef6d45c97117a7c4300ec8aae644097a41385920d862b0ac6c7ce734c07618` |
| Targeted C receipt | `27c23442778a49c8d0699e9f17d019a357c11fbf494b1112956e48ed16dd29dd` |
| Targeted C raw comparison and fresh ledger | `fce44fd5d1963aea4b01a4107d5d6304169c3b1afb24be7abcb43745610580b6` |
| Targeted D receipt | `1d92d33493d261c9da9270d4fc48bd326b0002fec2c29b2b10aa8cf77f461422` |
| Targeted D raw comparison and fresh ledger | `5df3f2dfaa5028416ffb254d14f7517e3d6a8494fe6803fc7bc755e0925c1511` |

### Publication follow-up and remaining scope

The earlier CI, UI, public-render and private-diagnostic records evaluate
frozen tree `4861c92228b0d2aecd465ad77466ea83ade51b3c` above parent
`1ab3a5b1796564bc76dc57a513115f521edd0e90`. They remain separate from
the [publication snapshot gate](#publication-snapshot-gate), which records the
actual command outcome for `c47c2a676` on parent `201de37b9`. The earlier
image and UI artifacts are not relabeled as captures from the publication base.

The separate publication proof preserves all 107 implementation-path blobs,
types and modes across the existing rebase. It does not establish equality of
the complete evaluated and published source trees. The publication gate has its
own actual outcome and source checks; neither that gate nor patch identity turns
earlier images into new image-quality evidence.

The final documentation follow-up changes exactly these eight documents from
`c47c2a676` and preserves every other blob and mode from that publication
commit. It preserves the existing implementation commit rather than rewriting
its history. The draft PR records the final follow-up commit, immutable document
links and separately reviewed public attachments. Capture receipts establish
what was produced and reviewed; they do not retroactively assert that an upload
had completed at capture time.

Only the two empty-design control panels and four public clear-glass
comparisons are selected for attachment. Private images, source inputs,
settings, coordinates, input hashes, logs and physical paths remain excluded.

The completed checks do not close every original acceptance gate. Integrated
energy and independent-seed robustness, caustics viewed through another specular
surface, increasing-budget error, and radius-dependent energy, leakage and
detail still need qualification. Accounting and pilot tests do not establish
calibrated production confidence with reused photon maps or Sobol dependence.
Rough-transmission capture and filtering remain unimplemented Stage 5 work;
broader glint, caustic, texture, coverage and depth-of-field preservation are
also open. Equal-time efficiency and expected-energy qualification remain open.
Admitted memory is not a measured peak resident-set size.

The private review supports only the explicitly inspected contributions and
does not establish a general rare-event bound. Advanced manifold connections,
broader merging, continuous spectral transport and rough-glass multiple
scattering remain separate research. PR #13904 remains draft.

### Retained a347 integration attempts

Both `cargo xtask ci` attempts below retained unchanged source identities. They
are historical attempts on `a3475683`, not successful qualification of `1ab3a5b1`.

| Attempt | Frozen index tree | Actual terminal outcome on 10 November 2026 |
| --- | --- | --- |
| `ci-main-a347-01` | `df1a7dcfcda6175bf446c291ea7dc4803a7ad99c` | Exit **1** at **14:31:44.462108 UTC**, after 2,165.744 seconds. Clippy's render-library test compilation failed with E0422: unqualified `AdaptiveSampling` in `runtime_request_roundtrip_tests`. No full-workspace native test result was produced. |
| `ci-main-a347-02` | `0513ae496b5feecc3681a5801643ac538054fb53` | Deliberately stopped with **SIGTERM, exit -15**, at **15:03:10.673086 UTC**, after 1,662.503 seconds, to integrate the selected main revision. This was not a timeout or a passing gate; the owned process session was empty before restacking. |

The test type was qualified as `adaptive::AdaptiveSampling` before attempt 02.
That source correction is preserved in the restack; the interrupted attempt
does not establish a completed compilation or test result.

| Retained integration artifact | SHA-256 |
| --- | --- |
| `ci-main-a347-01/receipt.json` | `fe3e93b5c3d80f3b62b0a7221536383977d70f23ce56a49deaf2711584ed25d4` |
| `ci-main-a347-01/output.log` | `b58515d81f18f92dc4ed12c911f3507923d1a0a702bf27273e82e8deab27bf96` |
| `ci-main-a347-02/receipt.json` | `43931b0e4e725484911e199b5b3cd77a30073eeb6b381e1e937f4d639c9eb1f0` |
| `ci-main-a347-02/output.log` | `2370adaf908ec0befd57b96876c6b349b5523dcbf2353f406ec3cfadc611cfb0` |
| `main-1ab-restack-01/application-receipt.json` | `2909b79391355f327f52b88fbed406d90400b2377d6f53af568707c1381cd2a4` |

### Current clamp compatibility

The optional GPU [indirect clamp](render-path.md#clamping-indirect-light) is
separate from denoising and [photon diagnostics](render-photon-convergence.md#controls-and-compatibility).
It remains off by default and round-trips through saved setups. Where the
upstream clamp applies, component capture uses beauty's aggregate scale;
deferred optical handling remains unchanged. A request combining this clamp
with a photon pixel probe is refused before rendering because per-photon
post-clamp attribution is not supported.

The following sections retain completed evidence from the earlier `42918fe5`
checkpoint and its predecessors. Those results do not qualify the newer restack.

## Merged dependency and completion qualification

This earlier completion candidate was pinned to main
`42918fe51b3bb4ae2200ae1f5ef32f0373276566`. Rendering PR #12546 had merged.
The candidate's full gate, `ci-completion-main-42918-01`, completed on
**9 November 2026 at 23:32:34.917838 UTC** with **exit 1**. It recorded
**14,687 passed, 28 failed and 68 ignored** native tests. The strict historical
comparison found **23 exact body matches and five changed bodies**; it found no
new test identities, unparsed failures or count mismatches. The five changes are
limited to audited first panic-header locations. All five failures subsequently
reproduced on bare `42918fe5`. A separately reviewed proof grants conditional
image-evidence eligibility while preserving **CI exit 1 and the strict mismatch
counts**. The selected-source CPU/GPU clear repeat and standalone web check have
completed. Actual egui controls also completed on a fresh second attempt after
a retained build timeout. Both public CPU strategy profiles completed and were
independently decoded. The private GPU comparison and final publication of this
checkpoint were not performed before integration moved to the newer base.

The a77-to-42918 audit contains **33 commits and 504 changed paths**, with
**32 overlaps with this implementation's 106 paths**. Main extracted shared
`viewport`, `viewport-gpu` and `viewport-egui` crates. The preserved implementation
was transferred into an isolated checkout. The first apply reported a module
conflict; its retained receipt is not a successful-apply claim. A reviewed
six-file compatibility patch resolved the module ownership and moved API imports:

| Adapted paths | Purpose |
| --- | --- |
| `crates/render/src/lib.rs` | Preserve upstream viewport extraction and the implementation's module declarations |
| `crates/render/examples/validate_glass_convergence.rs`, `crates/render/examples/glass_evaluation/strategies.rs` | Adapt viewport camera/material imports and the mesh trait where needed |
| `crates/render/src/raytrace/image_indirect.rs` | Use the extracted opaque-surface owner |
| `crates/render/src/raytrace/primary_aa_tests.rs`, `crates/gpu/src/path/tests.rs` | Use the moved surface and clear-coat owners in existing test references |

These six adaptations change no comparison fixture, settings, estimator
mathematics or accounting normalization. The broad upstream delta changes source
and dependency ownership; it is **not whole-tree equivalence** to a77. The seven
staged feature documents are byte-identical between the old and new frozen trees
before this documentation template. That document equality does not qualify code.

| Earlier frozen identity | Value |
| --- | --- |
| Selected main / dependency | `42918fe51b3bb4ae2200ae1f5ef32f0373276566` |
| Dependency tree | `1f793746e3bf742a27843830adc483be22fe3135` |
| Implementation index tree | `17322599705ce5aed42d1ad1774941223987da2a` |
| Full source-state SHA-256 | `eb526a89b8e553664970c6a207053d1f35a9d20a3ebc89f751dc16f0557360a6` |
| Non-Markdown code-identity SHA-256 | `da00fd71682a572ac6b6bd0245f0237de50a1bfa7f8de19fbf8c49c97c6b8d4a` |
| Formal freeze | 9 November 2026, 19:39:42.412631 UTC |

The pinned local dependency ref and source contract bind this checkout.
A later remote-main check is a separate publication step. This run followed a
no-write policy. The runner's source, code, index, dependency and reviewed-tool
endpoint checks passed, as did its reviewed-source contract checks. These checks
do not continuously monitor every read or establish compiler provenance from
source hashes alone. This runtime qualification applies to selected `42918fe5`,
not to later remote-main revisions.

A bounded later-main review observed `d0b739704d9394ac6e03fbc84f20b975b5e1ff18`:
**80 commits and 359 rename-aware diff entries** after selected `42918fe5`,
with zero overlap with the implementation's 106 staged paths. Its consumed-API
review found no concrete blocker requiring another adaptation. Dependency,
mesher, document-evaluation and UI behavior nevertheless changed upstream.
Zero path overlap and static compatibility do not prove a merged build or equal
output. Those results retain selected base `42918fe5`; **the later d0b739 base is not
runtime-qualified by these results**.

### Completed gate and evidence contracts

The reviewed pipeline contains **14 new tools** and a source-bound validation
contract. It preserves the earlier numerical comparisons, exact input-change
checks, full-grid accounting, deadlines, cleanup and actual-exit reporting.
Public receiver and splitting profiles execute CPU `ImageTask`. Their settings
and statistical analysis are unchanged by the port. Private comparisons retain
their original grid and input contract without publishing those inputs.

The completed run contains **100 unique native binary labels and two distinct
side jobs**, for **102 completed outer commands**. Of the native binaries,
**94 succeeded and six failed**. The new viewport libraries are included in that
inventory. The wasm side job passed for **25 packages**, and the doc-test side job
passed. No aggregate doc-test case count is inferred from its retained final line.

The native labels come from xtask's actual Cargo compiler-artifact parser.
The summary reconciles the complete observed label set, unique final results
and test totals, with no missing, duplicate or unexpected labels. These native
totals exclude earlier shared-crate isolation reruns, captured child summaries
and side-job tests. CI does not retain raw compiler JSON; no such retention is
claimed.

Both GPU-test families are explicit:

| Setting | Value |
| --- | --- |
| `SOLIDCRAFT_GPU` | `auto` |
| `SOLIDCRAFT_REQUIRE_GPU` | `1` |
| `VIEWPORT_TEST_GPU` | `auto` |
| `VIEWPORT_TEST_REQUIRE_GPU` | `1` |

When a GPU test needs a device, these settings make a missing device a failure.
They do not imply that every test exercises a device. Automatic selection alone
does not establish a physical adapter's identity or hardware classification.
Any stronger claim needs actual execution evidence.

The retained bc9 failure artifact is a **known premerge comparison**, executed
on its exact precommit tree with HEAD `f9ed9a90`, not an executed baseline of
`42918fe5`. Its failure bodies and narrow normalization policy remain unchanged.
A new or changed failure must be investigated; it cannot be relabeled as baseline.
The unchanged strict comparator therefore reports five changed bodies and
`all_observed_failures_match_baseline: false`. Even an exact match to a recorded
inventory would not turn a nonzero full-gate exit into a pass or prove the absence
of integration regressions. The separate completed per-case reconciliation below does not
change this strict result or create an executed full baseline for `42918fe5`.

### Completion evidence and publication status

The gate ran from **20:37:59.854827 to 23:32:34.917838 UTC**, taking
**10,474.989590 seconds**. Both the command and wrapper exited **1**. The owned
session completed normally within its four-hour bound, with no timeout,
interruption, cleanup or runner error. All source and dependency endpoint checks
passed. The independently reviewed summary is complete; **the full gate failed**.
Its 14,715 executed native tests comprise 14,687 passes and 28 failures; a further
68 were ignored. The repaired RAM-isolation case explicitly passed.

| Selected native library | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Render | 921 | 0 | 2 |
| GPU | 264 | 0 | 3 |
| Document | 667 | 0 | 2 |
| UI egui | 5,261 | 0 | 21 |
| Engine | 4,193 | 12 | 13 |
| Viewport | 38 | 0 | 0 |
| Viewport GPU | 61 | 0 | 0 |
| Viewport egui | 1 | 0 | 0 |

The separate UI integration inventory has **254 passed, zero failed and three
ignored across 21 binaries**. These rows are included in the native totals.
Historical a77 CI03 remains a separate completed failed gate with 28 exact
known-premerge matches; it is recorded below, not attributed to this candidate.

| Earlier 42918 checkpoint | Observed result | Scope |
| --- | --- | --- |
| Full main-42918 CI01 | **Completed, exit 1**; 14,687 passed / 28 failed / 68 ignored | 100 native binaries plus two side jobs; 23 strict body matches, five changed bodies; no new/unparsed/count issues |
| Targeted parent replay | **Five failures reproduced; wrapper exit 0** | Each exact test exited 101 with 0 passed / 1 failed / 0 ignored; diagnostics match under unchanged V2 |
| Image-eligibility addendum | **Granted conditionally for image evidence** | All 28 failures covered by 23 strict matches plus five actual parent reproductions; original CI exit 1 and strict false retained |
| Public CPU receiver strategy | **Completed: 128 renders, 16 groups, 48 exact pairs** | Complete ownership/reference pairs and negative controls; raw means, seed scatter, timing and finite-seed limits |
| Public CPU interface splitting | **Completed: 192 renders, 24 groups; mixed variance outcomes** | Complete comparisons at recorded requested lanes; actual time, mean shifts and rare-tail limits |
| Private GPU receiver comparison | **Not executed for this checkpoint** | Fresh same-source default-off baseline, exact copied binary, camera-seed runs and raw accounting; public-safe numerical findings only |
| Public CPU/GPU clear repeat | **Completed; run and independent audit exit 0** | Each backend's raw/display/mask bytes match its own reference; decoded finite RGBA and guide support verified |
| Actual egui controls capture | **Attempt 02 and independent audit passed; attempt 01 timeout retained** | Two light-theme software-raster panels; current CPU visually inspected, GPU inspection reused only after exact identity; no native window/viewport qualification |
| Web-offline qualification | **20 scenarios passed; command and wrapper exit 0** | Separate 21.553431-second fallback; generated stand-in app/service-worker only, not actual application/API qualification |
| Publication source binding | Capture completed; checkpoint superseded before publication | 14,582 staged entries, including 12,274 non-Markdown entries; no later index/commit comparison or publication has occurred |

### Public CPU receiver and splitting comparisons on 42918

Both public profiles completed using copied example executable
`229312fa7d834e427a4ec2e24ad6d0b2b20ecdba88186d7b101eb9a67ebe06d8`,
bound to the same frozen `42918fe5` implementation as the gate above.
The independent analyzer decoded all **320 raw EXRs** and recomputed their
pixel and seed statistics. It did not rerun transport.

The receiver profile contains **128 images, 16 groups, six scene cases and
eight randomizations**, at 32 complete observations per 32 × 32 image pixel.
All **48** required continuation/reference and negative-control pairs have
identical RGB bits, alpha bits and encoded EXR bytes. The diffuse and hard-source
controls are unchanged. The four selected glossy cases exercise a real ownership
change: their legacy-gather images differ from the camera comparator in every seed.
The comparator uses the same camera estimator, so equality is an ownership
check rather than an independent physical-accuracy reference.

| Receiver case | Continuation / gather Y variance | Continuation / gather mean Y | Variance × measured time / gather |
| --- | ---: | ---: | ---: |
| Area light, glossy | 3.77976 | 1.00096 | 3.47273 |
| Area light, polished | 0.637518 | 1.01861 | 0.584926 |
| Environment, glossy | 0.966563 | 1.00004 | 0.914139 |
| Environment, polished | 0.0111248 | 0.990681 | 0.0104309 |

Continuation reduces the observed variance in the polished cases but increases
it for the area-lit glossy receiver. These finite-run mean ratios retain the
measured brightness changes; a lower mean is not evidence of an energy correction.
The continuation times are 0.918–0.946 of the matching gather times.
The separate camera comparator takes 0.614–0.711 of gather time and does not
emit the disabled continuous-source photon family.

The splitting profile contains **192 images, 24 groups, four scene cases and
eight randomizations**, at 32 requested lane evaluations per pixel. Ordinary
sampling has 32 complete observations; two-lane modes have 16, and four strata
have eight. The table gives seed variance divided by ordinary independent sampling:

| Scene | Sobol | Two strata | Four strata | Independent split | Sobol split |
| --- | ---: | ---: | ---: | ---: | ---: |
| Area light, diffuse | 1.00456 | 1.43638 | 3.43946 | 1.55393 | 1.39827 |
| Area light, glossy | 1.36234 | 1.62140 | 2.74054 | 1.72331 | 1.84751 |
| Environment, polished | 0.187216 | 1.77895 | 3.49476 | 1.85539 | 0.182534 |
| Dispersive glass, area light | 1.25288 | 1.40767 | 2.58755 | 1.58339 | 1.39939 |

Independent splitting increased measured variance in all four cases at this
fixed lane budget. Sobol and Sobol splitting substantially reduced it only in
the environment-polished case. All measured split-profile time ratios lie
between 0.923 and 1.026; this was not a calibrated equal-time experiment.
Variance times measured elapsed time is a descriptive proxy, not a confidence
interval, bias correction or independent error metric.

Eight randomizations and 32 × 32 images do not bound rare tails or resolve
full-resolution detail. Source, geometry, finite depth and reconstruction radius
remain part of every result. Memory figures in the reports are conservative
admission estimates, not measured peak RSS. These CPU timings do not qualify GPU
performance, and none of these results qualify the later `a3475683` or pinned
`1ab3a5b1` integrations.

| Retained artifact | SHA-256 |
| --- | --- |
| Receiver execution receipt | `48978214561bbb756f7446ae66d4c8f8e88874a8516de9eed24de39e8647b9c1` |
| Receiver example report | `4dbb15a7d1350193d45aed43d059fd6de5b51df1ad35b3a7045706f0daaa6b58` |
| Combined independent analysis (`summary.json`) | `ed929566e5c54743929bdbe965ed2107b7954f80be3ed3e4cbde1c4984eb227e` |

The companion input manifest records each original receipt and EXR hash.
Complete per-seed RGB means, component values, signed shifts, timings and
admission estimates remain in the retained reports. The original full CI exit
and strict comparison are unchanged.

### Five changed diagnostics and actual parent reproduction

The unchanged comparison retains all five cases as **changed**, not exact
historical matches. The completed independent review checked each retained
candidate diagnostic against the historical body. Only the first panic-header
source position changed; all remaining normalized bytes are identical.

| Engine test | Historical source position | Candidate source position |
| --- | --- | --- |
| `cmd::edit::tests::inserting_variable_fillet_control_preserves_parameters_at_existing_positions` | `cmd/edit_tests.rs:8:47` | `cmd/edit_tests.rs:9:47` |
| `measure_centre_tests::a_fillet_round_a_boss_reports_its_radius` | `measure_centre_tests.rs:346:94` | `measure_centre_tests.rs:377:94` |
| `tests::chord_and_variable_fillets` | `tests.rs:30:19` | `tests.rs:31:19` |
| `tests::fillet_fit_taper_closes_to_a_point` | `tests.rs:30:19` | `tests.rs:31:19` |
| `tests::fillet_the_edges_between_two_overlapping_cylinders` | `tests.rs:30:19` | `tests.rs:31:19` |

These source paths are relative to `crates/engine/src`. The existing normalizer
still preserves source positions, assertion values and arbitrary notes. It only
normalizes numeric thread/process IDs, outer whitespace and the exact trailing
standard Rust backtrace hint. Neither the baseline nor this policy was changed.

The targeted engine-only replay completed on clean, pinned bare `42918fe5` at
**00:05:37.376821 UTC on 10 November 2026**, after **1,373.442775 seconds**.
The replay wrapper exited **0** because all five selected failures reproduced.
**Each test itself failed with exit 101**, with zero passed, one failed, zero
ignored, zero measured and 4,215 filtered tests. This is successful reproduction
of failures, not five passing tests.

All five complete parent diagnostics equal the candidate diagnostics under the
unchanged V2 normalizer. The replay retained the actual compiler JSON, copied
executable, feature selection and raw test outputs. Its 14 commands completed
normally. Source, binary, tool and immutable-input endpoint checks passed, with
no unresolved reasons. The independent review recomputed the full diagnostic
comparisons and compiler/artifact bindings; it did not rerun the tests.

The separate verifier recomputed both the audited historical-to-candidate
header-only differences and actual candidate-to-parent diagnostic equality.
Its coverage is **28 failed cases = 23 strict matches + five reproduced cases**,
with disjoint sets; the five reproduced cases are exactly the five strict
changed cases. It grants **conditional image-evidence eligibility only**.
The independent review itself grants no image or publication eligibility.
Neither record turns the original full CI into a pass or changes its strict
`all_observed_failures_match_baseline: false` result.

The recorded engine features are `bundled-render-library`, `default`, `profile`
and `raytrace-cpu`. This package-only build does not establish identical
transitive feature unification with the candidate workspace build. It is not a
full parent baseline or proof that every integration regression is absent.
Parent passes, different bodies, missing/ignored cases, timeouts, unparsed
failures and count mismatches remain blocking under the unchanged contract.

### Selected-source clear repeat and decoded accounting

The public clear fixture ran on both CPU and GPU at **256 × 192**, with
**32 observations per pixel** and **1,572,864 observations per backend**.
It enabled extended optical guides, component capture/filtering and variance
guidance. Caustics were absent and interface splitting was false.
Build and render commands both exited 0; the wrapper recorded passed output
validation and unchanged source, code, dependency and copied-binary identities.
The build took **480.786574 seconds**, and the two-backend render command took
**66.311433 seconds**. These are recorded command durations, not equal-time
quality measurements or isolated transport timings.

All six encoded files—raw EXR, preview PNG and guide-mask PNG for each backend—
match that backend's own prior84 reference exactly, including the same bytes
retained at the historical a77 checkpoint. Compared geometry, requests, effective
options and diagnostics also match. CPU output is not asserted equal to GPU output.
Exact encoded-file identity permits reuse of the earlier visual inspection.
**The independent auditor made no new visual judgment.**

The independent audit decoded actual RGBA and all 20 mask categories. Every RGBA
value is finite and RGB is nonnegative. Each backend has **6,441 supported optical
pixels = 3,754 retained optical pixels + 2,687 formerly TIR-bypassed pixels**.
Every former TIR pixel is now optical; all other mask pixels remain unchanged.
Decoded alpha counts are CPU **9,488 zero / 39,228 one / 436 partial** and GPU
**8,379 zero / 40,264 one / 509 partial**.

| Accounting check | CPU | GPU | Evidence unit |
| --- | ---: | ---: | --- |
| Component-to-beauty residual | `8.913048e-11` | `1.177167e-5` | Independently recomputed from six reported component totals and reported beauty total |
| Raw-to-reported-beauty residual | `3.887144e-6` | `1.512072e-5` | Decoded FLOAT32 EXR RGB totals versus reported beauty total |
| Maximum per-observation recombination error | `5.551115e-16` | `4.768372e-7` | Reported metadata only; not independently reconstructed from exported component images |

The aggregate checks satisfy the existing `2e-6` relative-scale tolerance; raw
export rounding is included in the second check. The EXRs contain RGBA beauty,
not per-pixel component images. These aggregate checks do not establish every
per-observation maximum independently. The audit rehashed 147 referenced files
and preserved the run's verdict. Its file endpoint checks are not continuous
source monitoring or a fresh whole-tree recomputation.

This fixture qualifies its stated clear-guide support and reference preservation.
It does not exercise the glossy receiver strategy or interface splitting, and
does not establish independent reconstruction error, noiselessness, photon-density
bias, equal-time improvement, real-widget behavior or saved-design behavior.

### Selected-source standalone web check

The separate web-offline command completed at **00:07:39.543403 UTC** on
10 November 2026, after **21.553431 seconds**, with **20 scenarios passed**.
Command and wrapper exits were 0. Source, code, dependency and reviewed-tool
endpoint checks passed. It exercised the generated stand-in app and service-worker
online/offline/update/failure behavior. It did not run the actual application
site or the web API test. This fallback leaves the failed full-CI exit unchanged.

### Selected-source actual-controls capture

**Attempt 01 is retained as a failure.** Its workspace compiler-artifact discovery
build reached the **1,800-second** bound and terminated with command exit **−15**
after **1,800.034407 seconds**, finishing at **00:50:33.965495 UTC on 10 November
2026**. It ran zero native tests and produced no capture log, controls record or
PNG. Source and code endpoints matched, but no capture binary was qualified;
the receipt's `binary_unchanged: false` is preserved. This timeout is not a
successful UI qualification.

A fresh attempt 02 used the same pinned wrapper, driver, source, eligibility
proof and capture settings. Only the artifact-discovery and driver-link bounds
increased to **7,200 seconds**; the capture bound stayed **180 seconds**.
No freshness, exact-artifact or semantic guard was relaxed. Attempt 02 ran from
**00:51:18.293603 to 01:05:45.901748 UTC**. Discovery, driver link and capture
all exited 0, taking **787.342342**, **5.798624** and **1.719472 seconds**,
respectively. The receipt records successful controls/output validation and
unchanged source, code, dependency, capture executable and library endpoints.

The immediate independent audit completed at **01:08:07.824000 UTC**, before
root started another owned Cargo build. It rehashed **623 references totaling
3,901,727,998 bytes** and recomputed exact current linkage. The retained inventory
has **477 link objects: 475 normal Cargo artifacts and two native archives**,
with **14 fingerprint proofs and six direct dependencies**. Those file endpoint
checks are not continuous monitoring or a new whole-tree qualification.

The capture ran **zero native tests**. It verified the existing five UI
integration-target results from the completed selected-source gate—24 passed
and two ignored—without rerunning them. It used a new empty in-memory design,
checked the queue before removing both default-host owners, kept Live paused,
and started no render job. No external app session or private design was
accessed. The owned capture process finished and its scenario scratch was empty.

Both English light-theme panels are **476 × 1,599 pixels**, with alpha 255
throughout. Saved and inspected controls enable area photons and glossy
continuation at maximum base roughness **0.35**, enable first-interface splitting,
and retain one authored Fresnel sample. Text/crop witnesses pass. The controls
JSON is byte-identical to its historical reference.

| View | Comparison with historical panel | Visual review |
| --- | --- | --- |
| CPU | 301 decoded pixels differ; maximum absolute 8-bit RGBA differences `[2, 2, 2, 0]`; dimensions unchanged | Root viewed the current PNG and accepted readable, unclipped labels, 0.35 threshold, checked splitting and disabled Fresnel count at 1 |
| GPU | Encoded bytes and decoded pixels are identical | Root explicitly reused the earlier inspection only after exact encoded-file identity |

The independent auditor made **no visual judgment**. These are real application
egui meshes and texture uploads rasterized in software, not a native presented
window or GPU viewport. The raster helper skips GPU callback primitives.
This validates the captured control state and panel presentation, not transport
quality, renderer performance, all themes/languages or an existing saved design.
The failed full-CI exit and strict failure comparison remain unchanged.

### Source capture and remaining evidence

The new `publication-source-binding-main-42918-01` capture completed at
**20:38:28.854266 UTC**, after the new gate started. Its immutable start snapshot
records no final exit. It binds **14,582 staged entries**, including **12,274
non-Markdown entries**, to source `eb526a89`, code identity `da00fd71` and index
tree `17322599`. The worktree matched the index and had no nonignored untracked
paths at capture. The retained manifest permits a later comparison to change
only the seven listed documentation files. No such comparison or publication
has occurred. A source-entry capture is not a build attestation, completed CI
result or proof of a future commit; a later HEAD has a distinct identity.

The current UI receipt verifies the five integration-target rows from the new
gate and records no duplicate replay. The retained attempt 01 timeout is separate
from attempt 02's successful capture and audit. The historical formatter incident
and recovery remain recorded below. Earlier a77 clear, UI and web artifacts keep
their original identities; the selected-source clear, UI and web runs above have
distinct current receipts. None turns the failed full gate into a pass.

Receiver continuation/reference comparisons check reciprocal ownership in the
same camera estimator. They are not an independent density-bias oracle. Matched
requested split lanes are not equal time or equal traversal work. Private
policy/camera-seed comparisons distinguish the paired policy change from
camera-seed scatter; they do not estimate off/on variance reduction from one
baseline. Energy, detail, rare-tail and equal-time acceptance remain separate.

No public image attachment was uploaded for this historical checkpoint. The
[current integration record](#current-integration) records final publication
status. No private scene, camera configuration, coordinates or physical artifact
paths are included here.

### Current source and prepared-tool artifacts

These names and hashes identify the earlier `42918fe5` evidence bundle. They
are not links to repository files. Static preparation reviews do not establish
runtime outcomes. The completed gate, unchanged strict comparison, actual
targeted reproduction, conditional proof, clear and web runs, and source-entry
capture have separate records below. The failed and successful UI attempts and
their audit are also retained. Both public CPU strategy profiles completed;
their execution and independent analysis receipts are recorded in the
[comparison section above](#public-cpu-receiver-and-splitting-comparisons-on-42918).
Private GPU comparisons and publication were not performed at this checkpoint.

| Completed gate or conditional-evidence artifact | SHA-256 |
| --- | --- |
| `ci-completion-main-42918-01-receipt.json` | `1fb61f16ebf16214a780893194cbfd0f0c9a422675f3fd69954a1772857b7ba7` |
| `ci-completion-main-42918-01.log` | `55ca5da669bbbd7f6cfe0b50b5a61b7556d9e58047710ec0bb8629645feee484` |
| `ci-completion-main-42918-01-baseline-comparison-final.json` | `369a71e0162dd0a7d45b4d53e0a5144763ccbca2782d2d030be015bba30c17d0` |
| `ci-completion-main-42918-01-native-summary.json` | `f51596f7378462335370897308fadbce658aa5cadeefdbecc7d9051fba281b96` |
| `ci-main-42918-completed-gate-independent-review-01/review.json` | `9f49a053f8673fdcda689d3e9e1c55b3837631fd1fe632707a99cfeee49beeaf` |
| `known-premerge-failure-source-map-42918-01/audit.json` | `254f0d453dedc5d95dfb29c960fd1093f31f26d42dad875691edf30cad3d097d` |
| `targeted-parent-image-eligibility-design-main-42918-02/contract.json` | `3fa1edae20de3b97819a1e8f019e3b18a5dc3e062b90349380d5b3962ec1d0fc` |
| `known-failure-replay-main-42918-preparation-01/final-manifest.json` | `b1c02dbf1b693be4b47664e6665717b1d949ba662581a34c4d42e573a0deb762` |
| `targeted-parent-image-tooling-main-42918-reconciled-preparation-01/final-manifest.json` | `16f35677b93921f5723d2c75a218f830df4995ac363764cc82933db27e9074f6` |

The replay plan, eligibility contract and successor-tool manifest above identify
the reviewed procedures. Their preparation alone was not a runtime result.
The actual replay, proof, independent review and subsequent runs are bound below;
the original 14 tools and strict comparison remain unchanged.

| Completed follow-on evidence or drift review | SHA-256 |
| --- | --- |
| `known-failure-replay-main-42918-01/receipt.json` | `1af734467c7faee4383b0aa98d6598532d2a289d900adebe3861c0ed13bf9dcb` |
| `known-failure-replay-main-42918-root-review-01.json` | `054f7365944d4bbae79b1770ebd0f3639984ea2b633465c1bdcb5aa293610b56` |
| `targeted-parent-image-proof-main-42918-01.json` | `484dc51c2b45e61516b5e0e159256223caa7c45932dd9b3f883d9e02b326f550` |
| `known-failure-replay-main-42918-independent-review-01/review.json` | `d259251f54341ee62bed9f3b6a660c42ec0a764c14f953f290c4578fb72040bd` |
| `completion-main-42918-web-offline-01/receipt.json` | `7020f76ea7e950e0f2000673df131efb2fa89e9d4cbab17f93c0bc31fa5ec6b0` |
| `completion-main-42918-web-offline-01/web-offline-test.log` | `4a22c5c811dc74a742d13051ab361cadcba50ccbaeede661e4af9f9e9f58a48a` |
| `glass-completion-clear-main-42918-reconciled-01/receipt.json` | `dccd5b90c3619c505daf20e28c440d689de9bef350f58dd4a4f10d79e4766cf4` |
| `clear-main-42918-reconciled-independent-audit-01/report.json` | `16ddc9d4602e57d5f54bc9d5424a4d999694a13e136d31102be5ab4add6ed578` |
| `publication-drift-d0b739-bounded-review-01/audit.json` | `620093a1782ea79737d3591f516575384de713a506c4faa30c997aab242dc33f` |

| Current actual-UI artifact | SHA-256 |
| --- | --- |
| `glass-controls-completion-main-42918-reconciled-01/receipt.json` | `0a46d2b0aee93b1ed56abfa7c833f55e112d6a8acca02d476cbc25ec238da7fe` |
| `glass-controls-completion-main-42918-reconciled-01/libraries.jsonl` | `8976d8073bf00bf2b2297bb0276c2cc968e597aaaf5ace2a2dd0d111ba6b50ad` |
| `glass-controls-completion-main-42918-reconciled-02/receipt.json` | `915bf7a0957a56c6bce7e4e1fb09e5162b01f3cde3c862ad2b7417f9d398968b` |
| `ui-main-42918-reconciled-independent-audit-01/report.json` | `117d59e0f55a1b4466dcc81c565637d890a9a36af1d65465ad8042cc810da948` |
| `glass-controls-completion-main-42918-reconciled-02/library-inputs.json` | `a465d803789d90cc18d4a87526fb72a861a1935e285792fb2b4fa883f5a17eb4` |
| `glass-controls-completion-main-42918-reconciled-02/captures/controls.json` | `1babe997f4e52acf9e42a05d2a117985c17d09b154ff31ca6a6f328061f8fa59` |
| `glass-controls-completion-main-42918-reconciled-02/captures/cpu-glass-controls-light.png` | `84cba74b79d376bb449847fd092da6c96471895eed8266790e435ac66a7cb750` |
| `glass-controls-completion-main-42918-reconciled-02/captures/gpu-glass-controls-light.png` | `dd09eb687526a984b830f9c645400538cd7950979a7ddec7a4960fc01f8d3516` |

| Source/contract artifact | SHA-256 |
| --- | --- |
| `completion-main-42918-source-freeze-01.json` | `2acb74b58c07950e5610c046346b5f7f9513339b52ff5f3883d7bcec4e1c4fc6` |
| `publication-source-binding-main-42918-01/capture-receipt.json` | `0c2392a8335b024dfcc5f6f058a7ef36855950f21fa232ba1c01864589145560` |
| `publication-source-binding-main-42918-01/ci-start-receipt.snapshot.json` | `22b15364d4128e5bd8972d3bb1cce6b45f79628b19bd7793df4999b8bd216580` |
| `publication-source-binding-main-42918-01/non-markdown-manifest.json` | `057be92ee28ad85cff0813658c9c99f81bf8175a214a124df423d63d0765ded4` |
| `completion-main-42918-validation-contract-01.json` | `d6af36841241cad2c4dadbbdae4691a6b5b5c7aa3471a618de57d21e13180025` |
| `main-42918-root-tool-review-01.json` | `a044f32456662f108d5b3fd025f43b434a89b5fd1d312d86edd6a240adefb2dc` |
| `publication-drift-main-42918fe5-preflight-01/manifest.json` | `0f2e4010bef10d32b1362b11c9bdaf997a8cd66b461a5b32181ee714f19ead8f` |
| `publication-drift-main-42918fe5-preflight-01/audit.json` | `3e6d365cbd8dd4e543e7b6ddda081ebc7d89590e5ee63f6a4666a26f7e36db11` |
| `main-42918-forward-integration-01/receipt.json` | `757d27cb3599b536baf35f717628317ca4ebfce41e017a0b628c26052565b55e` |
| `main-42918-compatibility-adaptation-01/receipt.json` | `18b9e70c5e5c8165f1acaafedb8e87fead02772dffaddf70df8dc26a058effad` |
| `main-42918-compatibility-adaptation-01/compatibility.patch` | `9b05ef8a6df15160a7def80bdaa1b57e2ddfe23905a868c2e81f201853fcaecf` |
| `main-42918-core-tool-preparation-01/final-manifest.json` | `b5f37c8396e37bb7ae8ee1ab3bdcbbda3591b75c9a6e1a7ec3c52771d9edc5eb` |
| `main-42918-ui-clear-web-preparation-01/manifest.json` | `69f248769fbf20db4857c92b9d1204af610651b8607d87314000ee8cf26e67db` |

| Reviewed tool | SHA-256 |
| --- | --- |
| `analyze_completion_strategies_main_42918_01.py` | `5a78324ac5f2ed5b503df942b29c319e4513a189aeaab6953a6eec2f25e0c393` |
| `capture_glass_controls_main_42918_01.rs` | `1da8c4df7ce57c00c6934e9597ce2781adb388b5d5605bfbccbb77231ef686fd` |
| `completion_image_evidence_main_42918_01.py` | `59992ebbcccb0f24f2bfc5805f3b453d104d80bdaf7adbf616fcf010b9b8aa4a` |
| `publication_source_binding_main_42918_01.py` | `4ebebe1e205c9b3a2c96fea1cadb5bdad3efcdd31316a7a9fec99cafd3b921f6` |
| `run_completion_ci_main_42918_01.py` | `3589cb5afd5c714b72e40a7fef4d9634ff1814eb5f5cf480c3f0e07549c2c396` |
| `run_completion_clear_evidence_main_42918_01.py` | `e62c58868bf7e91a2719fc80c4030d0b923d967382ba4d67f0a13f9110e09ded` |
| `run_completion_opus_evidence_main_42918_01.py` | `e1203a483f6c6a8f003137a0947e08288e8001dd3ea5cceeaa696928a32df9a4` |
| `run_completion_strategy_evidence_main_42918_01.py` | `20c05e7b6ca1b7065a241f491bf1cb6a5f59ba648c60bed20b75c34c7cb6645a` |
| `run_completion_web_offline_main_42918_01.py` | `6ec0d0c8288ad16377482e412925ffb144649431f5f888ad8055d67fe6b52187` |
| `run_glass_controls_evidence_main_42918_01.py` | `ffef9c32714f98b5efec9f69b07f842e8a6e9dc33f33809bf8bdf269be65a61a` |
| `run_opus_camera_probe_main_42918_01.py` | `3914c6622fa1b1b6d47b4f407fabec4db0ba0edd91f3e0df79d7626575e08afc` |
| `run_opus_receiver_evidence_main_42918_01.py` | `cbe959588f513b8bd778e015b9530de11d5eec68d2b9959ed4974b9e90b1a214` |
| `run_opus_receiver_probe_main_42918_01.py` | `f9d64bc5f837b8e1fd4d2290c4e77cb09016f1519bef9177c8ed861b077887e3` |
| `summary_ci_native_main_42918_01.py` | `35fdfc7dfb1f484d4860308dbd0add3339f9018cafb2582be2cd14d01517fece` |

The source-integration and tool records do not alter the historical checkpoint
results below. In particular, a77 CI02 remains a failed, image-ineligible gate,
its focused test-only repair remains one passing case, and completed a77 CI03
remains a failed full gate with 28 exact known-premerge matches. The older 23f173
implementation gate alone was deliberately superseded before completion.

The following historical checkpoint sections preserve their statuses as recorded
at those checkpoints. Current selected-42918 outcomes are stated above; historical
pending language does not supersede the completed results in this section.

## Historical a77 dependency and qualification

Rendering PR #12546 merged on **9 November 2026 at 12:47:03 UTC** as
`ed2bafe7e312ff38f1d957a511d4c8e529dc4e32`. The historical completion base was
observed main `a77b7ab2e463e41ae3ac27a05250765f0db0123d`, its direct child.
The published merge has one parent and does not retain premerge
`bc9b0b36208af7703779d131804891a78df47a76` ancestry.

The exact local-object audit records **17 changed paths** from bc9 to that main
revision, with **no intersection with this implementation's 106 paths**.
The comparison changes no files under `crates/render`, `crates/gpu`,
`crates/doc` or `crates/engine`, and neither root `Cargo.toml` nor
`Cargo.lock`. It does change production agent-chat UI. The other changes
include additive SolidCore tessellation API reexports/tests, folded locale
catalogs and generated license output. This is not a docs/test-only delta or
whole-tree equivalence. Nonintersection does not qualify the restacked source.

| Merge/source audit artifact | SHA-256 |
| --- | --- |
| `parent-bc9-merge-audit-01.json` | `1b5e5617611f570a4c9aff06ba46bdcaf4b4ce8fda7223bea7ee043f3cb0e636` |
| `parent-bc9-merge-audit-01.md` | `0ccf589f54c313ceaef65c726968296339879c37d3d505754494d1d96fc26733` |
| `parent-bc9-merged-source-delta-01.json` | `f59b3a0379d3ba4ca12a9c7dff657bd96b914e55e1825de3fda9b1d544e74e8c` |
| `parent-bc9-merged-source-delta-01.md` | `668dc67c72f65dd580fdf9810ef4cba56f2dbfc8e2f1429bce84809b271f671b` |

These audits record source and ancestry metadata; they execute no build, tests
or image qualification. The earlier `23f173d3` implementation CI was deliberately
superseded after **98 of 99 commands** and remains **incomplete**. The separate
parent baseline below is a completed parent run, not that incomplete
implementation run.

| Evidence | Recorded source and outcome | Scope |
| --- | --- | --- |
| [Completed pre-correction main-based gate](#completed-main-a77-gate) | `a77b7ab2` pre-correction completion source; 14,637 passed, 28 exact known-premerge failures, gate exit 1 | Endpoint bindings agree; the transient helper incident and completed separate recovery remain explicit |
| [Corrected fixture and CI02](#fixture-correction-smoke-checks-and-ci02-qualification) | Exact one-line example mapping; 32 receiver and 48 splitting smoke renders; CI02 exited 1 with 14,636 passed, 29 failed and 68 ignored | 28 exact known-premerge matches plus one new test-isolation failure; CI02 is ineligible for image qualification |
| [Test-only repair and CI03](#test-only-isolation-repair-and-ci03-freeze) | Original RAM scenario preserved; focused replay passed 1/1; full a77 CI03 completed with 14,637 passed, 28 failed, 68 ignored and exit 1 | All 28 failures match the known premerge bodies; repaired case passed. This is not a 42918 result |
| [Known premerge comparison](#dependency-baseline-bc9b0b36) | Exact precommit source tree of `bc9b0b36`; 14,519 passed, 28 failed, gate exit 1 | Premerge parent only; not an executed baseline of `a77b7ab2` |
| [Earlier parent baseline](#new-dependency-baseline-23f173d3) | Exact precommit source tree of `23f173d3`; 14,501 passed, 28 failed, gate exit 1 | Historical parent checkpoint, distinct from the incomplete implementation CI |
| [CI05 and preflight 07](#completed-prior-source-integrated-gate-ci05) | `84bda659` implementation checkpoint; 14,456 passed, 19 exact parent failures, gate exit 1; 28 focused passes separately | Repository verification at that frozen source |
| [Clear-glass repeat](#clear-glass-repeat-at-the-prior-source) | Same CI05 source; 6,441 supported optical pixels per backend and exact recorded output preservation | One 256 × 192, 32-observation fixture |
| [CPU sampling comparison](#cpu-sampling-comparison-at-the-prior-source) | Same CI05 source; 224 quality renders, 56 timing calibrations and a 56-render smoke check | Unequal measured times and unresolved polished-case mean shifts |
| [OPUS camera accounting](#opus-seed-diagnosis-at-the-prior-source) | Same CI05 source; three whole-grid captures and two targeted reprobes | Full camera-prefix attribution and sparse glossy-photon diagnosis |

### Historical main-a77 completion qualification

The implementation targeted at `a77b7ab2` completed its full gate with actual
exit 1 at the recorded pre-fixture-correction checkpoint; its results and source
incident are below. The standalone web check, restored UI replays, headless
controls and clear repeat also completed at that checkpoint. The corrected
example has an exact source mapping and passing small smoke checks. CI02 then
completed with exit 1: **14,636 passed, 29 failed and 68 ignored**. It retained
28 exact known-premerge matches and exposed one new test-isolation failure.
CI02 is ineligible for image qualification; no image qualification was launched against the completed CI02 gate.
The test-only repair is frozen separately and its exact focused replay passed.
Publication03 captured the repaired source entries. **Full a77 CI03 completed
with exit 1: 14,637 passed, 28 failed and 68 ignored.** All 28 failures match the
known premerge bodies, with no new or changed failure. The repaired case passed.
Current receiver/splitting/private comparisons remain assigned to `42918fe5`
and have not started.
The 28-failure bc9 artifact is a **known premerge comparison**, not an executed
baseline of a77 and not a prediction of this implementation's failure count.
Matching test names is insufficient: any changed assertion, source location or
cause must be retained and investigated. Earlier focused checks and receipts
remain attached to the source they actually exercised.

The public receiver and splitting profiles use CPU `ImageTask`; private GPU
comparisons and CPU/GPU clear fixtures have separate scopes. Their current
qualification is assigned to the selected `42918fe5` pipeline above. No complete
a77 strategy quality result is claimed here. The older comparisons and tools
remain retained with their actual source; PR #13904 remains draft.

### Fixture correction, smoke checks and CI02 qualification

**CI02 completed and did not pass.** This section keeps its corrected-example
source separate from the later test-only repair and completed failed CI03. The completed
CI01, standalone web, clear-glass and UI results
below retain source-state
`176b7ad929ad743be7ef1d9fb49c485ef33052331514aac54a113c29e412441c`
and code identity
`dbb4cfa4cbdc58c2ccb826f8cedce21848a9d20d9cebb53eefe54d5e8755da22`.
The receiver-quality attempt at that checkpoint stopped on request validation
before its environment cases. Its partial outputs and unsuccessful receipt remain
retained; they are not a completed quality matrix.

The correction changes exactly one request-construction line in
`crates/render/examples/glass_evaluation/strategies.rs`:

```rust
options.area_samples = (case.source == Source::Rectangle).then_some(1);
```

It replaces unconditional `Some(1)`. Rectangle fixtures still request one area
sample; environment and directional fixtures leave this area-light option absent.
Renderer admission correctly rejected the previous invalid request and remains
unchanged. The edit receipt and publication capture compare the complete staged
trees: every other path, file mode and blob is identical, including production
renderer, GPU, UI, schemas and dependencies. The **whole source identity changes**
because the example changes; earlier results are not relabeled as CI02 evidence.

| Recorded identity | Before the example correction | Corrected checkpoint |
| --- | --- | --- |
| Selected HEAD / dependency | `a77b7ab2e463e41ae3ac27a05250765f0db0123d` | `a77b7ab2e463e41ae3ac27a05250765f0db0123d` |
| Staged tree | `a87d588bf1768e2350f35a9bf56cb899446ed9e9` | `1adad15b0db40081e1f85a7d10c4c54e729c99e6` |
| Full source-state SHA-256 | `176b7ad929ad743be7ef1d9fb49c485ef33052331514aac54a113c29e412441c` | `f421be5af40af145d6b52b95f3cf8ddda71b63b19a2b9a2b9f31de392e40271c` |
| Non-Markdown code-identity SHA-256 | `dbb4cfa4cbdc58c2ccb826f8cedce21848a9d20d9cebb53eefe54d5e8755da22` | `8c658869bd7826cac25d83d6a03b1bb03788b6273369d0f1578d6b0c671e4d1a` |

The first corrected smoke attempt, `strategy-fixture-smoke-main-a77-02`, refused
before starting Cargo because the measured load exceeded its threshold of 24.
It executed no smoke profile. A fresh bounded attempt,
`strategy-fixture-smoke-main-a77-03`, completed on **9 November 2026 at
16:58:05.637591 UTC**. Its build exited 0 in **2.921922 seconds**. Both 16 × 16
profiles used two seeds and exited 0:

| Completed smoke profile | Cases | Groups | Renders | Renderer-reported paired checks | Command seconds |
| --- | ---: | ---: | ---: | ---: | ---: |
| CPU receiver | 6 | 16 | 32 | 12 | 1.769725 |
| CPU interface splitting | 4 | 24 | 48 | 0 | 4.424176 |

Both profile inventories and wrapper guards passed. The receipt records unchanged
source, code, index and copied executable; the earlier failed-quality receipt also
remained unchanged. The copied smoke executable SHA-256 is
`d22abd2f3d63c80ccbbc6f89d46072da1538161626a8a856091f554bf923f323`.
**Raw smoke images were not independently decoded.** These are functional fixture
checks, not full quality measurements, independent image-error comparisons,
rare-tail coverage, calibrated timing or a convergence guarantee.

The formal corrected freeze was recorded at **17:00:01.459229 UTC**. CI02 was a
fresh full-gate invocation bound to that checkpoint. The run followed a no-write
policy; the runner checked source, code, dependency, helpers and the exact index
tree at its endpoints. It used a bounded owned process session; it did **not** continuously
monitor source. Endpoint equality does not prove uninterrupted continuity, and
source snapshots are not build attestations. The historical CI01 formatter
incident remains attached to CI01; the new run does not erase it.

#### Completed CI02 outcome

`ci-completion-main-a77-02` ran on **9 November 2026 from 17:00:26.303069 to
18:29:45.451143 UTC**, taking **5,359.056598 seconds**. The command was
`nice -n 10 cargo xtask ci`. Both the actual CI command and wrapper exited
**1**. **The full gate did not pass.**

All 97 native binaries and both side jobs completed. The native totals were
**14,636 passed, 29 failed and 68 ignored**: 91 successful binaries and six failed
binaries. There were no measured or filtered-out native cases. Counts use each
binary's final outer result once, excluding captured child results and side jobs.
The 22-crate wasm check and doc-test side job passed; no aggregate doc-test
case count is claimed.

| Selected native library | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Renderer | 954 | 0 | 2 |
| GPU | 313 | 0 | 3 |
| Document | 664 | 0 | 2 |
| UI | 5,253 | 0 | 21 |
| Engine | 4,184 | 13 | 13 |
| Kernel | 808 | 11 | 4 |
| SMLib | 350 | 1 | 1 |

These suite totals do not claim that every GPU-library case performed device work.
The unchanged failure comparison found **28 exact normalized-body matches and
one new failure**, with zero changed baseline bodies, unparsed records or count
mismatches. CI02 does not match the recorded baseline failure inventory. The known-premerge comparison was
executed on the exact precommit source tree later committed as bc9, not on a77.

The native runner used `SOLIDCRAFT_TEST_RUNNER=libtest` and
`RUST_TEST_THREADS=4`. Cargo, native-job, Rayon and action-matrix concurrency were
each bounded at two; GPU availability was required. The owned session finished
normally within its four-hour deadline, with no timeout, interruption or cleanup.
Source, code, dependency, reviewed helpers and exact index-tree endpoint checks
passed. Receipt verification is not a passing test-gate result or continuous
source monitoring.

**CI02 is ineligible for image qualification. No image qualification was launched against the completed CI02 gate.**
The prepared 02 wrappers and all unsuccessful or partial attempts remain retained.
Current-source receiver, splitting and private results require the separate
repaired-source gate and remain pending.

#### New RAM-test isolation failure

The new engine failure was
`cmd::render::job_tests::native_camera_queue_preparation::native_batch_retained_plan_and_completed_images_allow_a_new_ram_epoch_before_next_capture`.
At the original `job_tests.rs:3322` assertion, the receipt reported
**124,956,245** admitted bytes against the host-local expected **4,686**.

The two sides belong to different accounting domains. `ImageHost::admitted_bytes()`
reads one host's `Budget.used`. Preflight's `host_memory.admitted_bytes` comes from
the process-wide native-memory pool, minus the current request's credit.
Other engine tests can legitimately retain leases in that shared pool. The later
epoch-refresh assertion also assumes process-wide quiescence. Waiting for this
test's own worker does not synchronize unrelated hosts.

The exact scenario passed in CI01. The attested parent JUnit records a pass in
0.039 seconds under nextest's per-test-process execution. The original function
and production memory-ledger blob are unchanged across CI01, CI02, selected a77
and bc9. The log cannot identify the specific foreign lease owner. This is a
source-backed test-isolation defect; no production memory-accounting change or
general correctness claim follows from this diagnosis.

#### Test-only isolation repair and CI03 freeze

The repair changes only `crates/engine/src/cmd/render/job_tests.rs`. The existing
test keeps its name and runs the same real-platform scenario in an exact child
process. All original scenario statements and assertions are byte-identical.
There is no relaxed assertion, new production API, ignored case or baseline change.

The parent selects exactly one test and gives the child 120 seconds. The child
validates its test-specific recursion marker. Owned-child cleanup kills and reaps
on failure. The parent verifies the exact one-test pass count, zero ignored cases
and one completion marker emitted after every original assertion. A zero-test success
cannot satisfy that contract. The separate focused runner allows 180 seconds
for the parent and child cleanup.

The repair receipt preserves the original/new file bytes and exact patch. Complete
staged-tree comparison shows every other path, mode and blob unchanged, including
the renderer, GPU, UI, schemas, example fixture and dependencies. It does not
relabel CI02 or historical images as tests of the repaired source.

| Recorded identity | CI02 / smoke attempt03 | Test-only repair / completed CI03 |
| --- | --- | --- |
| Selected HEAD / dependency | `a77b7ab2e463e41ae3ac27a05250765f0db0123d` | `a77b7ab2e463e41ae3ac27a05250765f0db0123d` |
| Staged tree | `1adad15b0db40081e1f85a7d10c4c54e729c99e6` | `1e4fa594ee1c4c019e8bb929f2303aeb342d31dc` |
| Full source-state SHA-256 | `f421be5af40af145d6b52b95f3cf8ddda71b63b19a2b9a2b9f31de392e40271c` | `a2a80ee154cd8607e8299c2662d6634f85e51bf860563478e19c1bbba345e70a` |
| Non-Markdown code-identity SHA-256 | `8c658869bd7826cac25d83d6a03b1bb03788b6273369d0f1578d6b0c671e4d1a` | `bdca5f509e4d281e9f9069603aab94fa2d7977884dcf712658decc101b30876c` |

Formal freeze03 was recorded at **18:43:56.768613 UTC**. It binds the repaired
source, exact staged tree, selected a77 dependency and retained CI02/repair
receipts. It is a source snapshot, not an execution result or build attestation.

**The completed smoke attempt03 is not full CI03.** The 32 receiver and 48
splitting smoke renders ran on `f421be5a` before the test repair. Completed full
CI03 exercised `a2a80ee1`. Their shared suffix does not imply the same source or
validation scope.

##### Completed exact-test replay

`native-epoch-isolation-focused-main-a77-03` passed on **9 November 2026**.
Its receipt spans **18:44:21.779134 to 19:03:40.532856 UTC**. The feature-unified
workspace `cargo test --workspace --profile ci --no-run` build exited 0 in
**1,155.013705 seconds**. Compiler JSON selected the exact engine library test
artifact; the runner copied and hash-checked that executable before selecting
the one named test.

The listing confirmed exactly one selectable case. The test command exited 0 in
**0.033824532 seconds** with **one passed, zero failed, zero ignored and 4,209
filtered out**. The wrapper also exited 0. These are one scenario and its parent/
child isolation checks, not two independent test passes. The parent process had
no inherited child marker, so the isolation branch was exercised.

The recorded copied executable SHA-256 is
`983f16bbbaf78c17582d2da0a75e9cbab22852a15aff78f6afcf8f793a447734`.
Source/freeze and dependency endpoints agree, and the executable remained
unchanged. No command timed out or required cleanup; no owned child remained
running. This focused pass verifies the repaired test's executed behavior.
It neither replaces CI02's retained failure nor substitutes for the full-gate
result recorded below.

##### Completed historical a77 CI03

`ci-completion-main-a77-03` started at **19:05:22.408810 UTC** and finished on
**9 November 2026 at 20:37:20.003249 UTC**. Its recorded gate duration was
**5,516.894786 seconds**. The CI command and wrapper both exited **1**.
**The full gate did not pass.**

All **99 native and side-job command rows** completed: **97 unique native
binaries** (91 successful and six failed) and both side jobs. The native total
was **14,637 passed, 28 failed and 68 ignored**, with zero measured or filtered-out
cases. Each binary's final outer result is counted once; earlier shared-crate
checks, child-process summaries and side-job counts are excluded.

| Historical CI03 native library | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Renderer | 954 | 0 | 2 |
| GPU | 313 | 0 | 3 |
| Document | 664 | 0 | 2 |
| UI | 5,253 | 0 | 21 |
| Engine | 4,185 | 12 | 13 |
| Kernel | 808 | 11 | 4 |
| SMLib | 350 | 1 | 1 |

The **21 UI integration binaries** passed **254**, failed **0** and ignored **3**
cases. These are included in the native totals, not additional passes. The GPU
library total does not imply that every case used a device. Both side jobs passed:
the wasm check covered **22 crates**, and the doc-test job completed without a
retained aggregate doc-test count. These historical inventories do not replace
the new candidate's planned 100-native/two-side-job/25-wasm contract.

The final comparison found **28 exact normalized-body matches** to the retained
known premerge bc9 artifact: **zero new failures, changed bodies, unparsed records
or count mismatches**. The repaired native RAM-epoch test is explicitly `ok` in
the completed log and is absent from the failed-test inventory. Its successful
execution does not erase the original CI02 failure or turn CI03 into a passing
gate. The known comparison was not executed on bare a77 or selected main 42918.

The receipt binds source `a2a80ee1`, code `bdca5f50`, index `1e4fa594` and selected
a77 dependency. Source, code, index-freeze, dependency, runner and helper endpoint
checks are true. The owned session completed normally, without timeout,
interruption, cleanup or recorded execution/verification error. The run followed
a no-write policy; the runner checked endpoints. This is not continuous monitoring
or a claim that source hashes alone attest every compiler read.

| Completed historical CI03 artifact | SHA-256 |
| --- | --- |
| `ci-completion-main-a77-03-receipt.json` | `7017ee80ddd678aec936154b8df9752c3d1350b95e200ae560f804e2cb86c72d` |
| `ci-completion-main-a77-03.log` | `d4c35fc6cbcedee7a5d186a49335b8b246bd39079c5f97a45ccd464f1d86ba83` |
| `ci-completion-main-a77-03-baseline-comparison-final.json` | `21c695c787af10f5b691ee6ffc0714a5ce46df79c3b6946873d26db46a011cef` |
| `ci-completion-main-a77-03-native-summary.json` | `07e61308068094012c0a8b608baacfbf0587d5a031d3ec2cbad74afb6516408f` |

The prepared a77 03 image wrappers require completed eligible a77 CI03 records
and exact repaired source/code binding. They supply no completed comparison here.
Current image qualification will use the distinct `42918fe5` pipeline above;
preparation is not a measurement. The a77 03 publication binding retains both
separate mappings: CI01 to the one-line example correction, then CI02 to the
single test-isolation repair.

`publication-source-binding-main-a77-02` captured **14,533 staged entries**, of
which **12,256 are non-Markdown**, while CI02 was in progress. Its receipt binds
the original capture, exact one-line edit and corrected frozen tree. Its historical
comparison contract permits changes only to the seven existing documentation
files. It is not a CI result, a publication action or proof of a future commit.
A later commit has a distinct HEAD-sensitive identity and cannot be relabeled
as the tested HEAD.

The fresh `publication-source-binding-main-a77-03` capture completed at
**19:05:54.741382 UTC**, after full CI03 started. It binds **14,533 staged
entries**, including **12,256 non-Markdown entries**, to repaired source
`a2a80ee1` and staged tree `1e4fa594`. It preserves both source mappings:
the one example request line, followed by the existing test's isolation patch.
Every other path/mode/blob entry remains unchanged across each respective step.

This is a completed source-entry capture, not a completed CI result. The capture
records an unknown CI exit. No later documentation/index or commit comparison
has been performed. These a77 capture and comparison contracts remain historical.
The selected `42918fe5` candidate has its own separate capture/comparison path,
with the same seven-document-only allowance and all other entries preserved.

### Historical completion evidence and publication status

The completed `completion-bc9-preflight-03` check belongs to the implementation
**before** its main restack. It passed **41/41 selected cases**, with no selected
failures or ignored cases: 17 renderer cases, 16 from the GPU library suite,
three UI, two document and three engine cases. Formatting, strict workspace and
all-target Clippy, and the feature-unified workspace test build also passed.
It finished on **9 November 2026 at 13:53:46 UTC**. Those suite counts do not
claim that every selected GPU-library case performed device work, and the check
is not relabeled as verification of the later `a77b7ab2` source.

#### Completed main-a77 gate

`ci-completion-main-a77-01` finished on **9 November 2026 at
15:56:25.980177 UTC**, after **6,462.440435 seconds**. The actual CI command
and its wrapper both exited **1**. **The gate did not pass.**

All **99 native and side-job command rows** completed: 97 native binaries
(**91 successful, six failed**) and both side jobs. The native total was
**14,637 passed, 28 failed and 68 ignored**, with no measured or filtered-out
cases. These totals count each native binary's final outer result once; they
exclude earlier isolation checks, child-process output and side-job counts.
The wasm check passed for **22 crates**. The doc-test side job passed, but the
retained command output does not supply an aggregate doc-test count.

| Selected native library | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Renderer | 954 | 0 | 2 |
| GPU | 313 | 0 | 3 |
| Document | 664 | 0 | 2 |
| UI | 5,253 | 0 | 21 |
| Engine | 4,185 | 12 | 13 |
| Kernel | 808 | 11 | 4 |
| SMLib | 350 | 1 | 1 |

The GPU-library total is a suite result, not a claim that every case exercised
a device. The document-library count is separate from the doc-test side job.

The final comparison found **28 exact normalized-body matches** to the unchanged
known premerge artifact: **zero new failures, changed bodies, unparsed failure
records or failure-count mismatches**. All 28 known failures were observed.
This comparison neither makes the gate green nor establishes an executed a77
baseline, whole-tree source equivalence or absence of integration regressions.

| Recorded endpoint identity | Value |
| --- | --- |
| Selected main / dependency | `a77b7ab2e463e41ae3ac27a05250765f0db0123d` |
| Dependency tree | `7ab76cd5461c37c7703400a22f7ac358e655fed2` |
| Full source-state SHA-256 | `176b7ad929ad743be7ef1d9fb49c485ef33052331514aac54a113c29e412441c` |
| Non-Markdown code-identity SHA-256 | `dbb4cfa4cbdc58c2ccb826f8cedce21848a9d20d9cebb53eefe54d5e8755da22` |

The receipt's before/after source, code and dependency records agree; its
helper and runner checks are also true. The log, receipt, final comparison and
native summary bind to the same recorded inputs. These are **endpoint checks**.
The source snapshot explicitly is not a build attestation, and the transient
helper mutation below means the source freeze was **not uninterrupted**.
The receipt's successful verification flag is not a passing test-gate flag.

The table below records only historical a77 checkpoint evidence, including
completed CI03's actual failed gate and matching known-premerge failure inventory.
The selected `42918fe5` pipeline and its pending fields are separate above.
An unsuccessful attempt remains part of the record.

| Check | Current outcome field | Qualification boundary |
| --- | --- | --- |
| Pre-fixture-correction main-a77 gate | **Completed; actual CI and wrapper exit 1.** 14,637 passed, 28 failed, 68 ignored; 97 native binaries and both side jobs completed. All 28 normalized failure bodies match the known premerge comparison. | The gate did not pass. Preserve the transient-mutation disclosure and endpoint-only source-binding limits below; the bc9 artifact is not an executed a77 baseline. |
| Corrected-example CI02 | **Completed; actual CI and wrapper exit 1.** 14,636 passed, 29 failed, 68 ignored; 97 native binaries and both side jobs completed. 28 exact known-premerge matches and one new RAM-test isolation failure. | Gate failed and image qualification is ineligible. No image qualification was launched against the completed CI02 gate. |
| Test-isolation focused replay | **Passed:** one exact case, zero failed/ignored, 4,209 filtered out; build/test/wrapper exited 0. Source/binary/dependency checks agree. | Separate from both CI02 failure history and the complete CI03 gate. |
| Repaired-source full a77 CI03 | **Completed; actual CI and wrapper exit 1.** 14,637 passed, 28 failed, 68 ignored; 97 native binaries and both side jobs completed. All 28 normalized failure bodies match; repaired RAM case passed. | Source `a2a80ee1`; distinct from smoke attempt03 on `f421be5a` and the new selected 42918 candidate. Gate remains failed. |
| Restored UI helper requalification | **Completed separately in attempts 02 and 03:** each replay passed 24, failed 0 and ignored 2 across the five exact targets. | The same cases were repeated; no 48-distinct-test claim. Does not replace the original full gate or erase its source incident. |
| Required standalone web-offline check | **Passed separately; command and wrapper exit 0.** All 20 expected scenarios in order and exactly one success marker; 23.713286 seconds. | Default stand-in app/service-worker harness only; does not replace the full gate, test a real compiled web app/API or erase the earlier source incident. |
| Headless glass-control capture | **Attempt 03 passed:** CPU/GPU saved-option checks, full control text and two 476 × 1,599 PNGs; no-host/no-job/paused/cleanup guards passed. | Real application widgets software-rasterized in empty harnesses; no native desktop screenshot, GPU viewport or render-quality claim. Attempts 01/02 remain retained. |
| Public CPU/GPU clear-glass repeat | **Passed at the recorded checkpoint:** all six raw/display/mask files match their own pinned prior84 references; 6,441 optical pixels per backend. | One 256 × 192, 32-observation fixture. Grain remains; no independent image-error, equal-time, named-GPU or new receiver/split qualification. |
| Historical public CPU receiver attempt | **Incomplete:** stopped on environment request validation after 64 logged area renders. No complete quality analysis is claimed at this source. | Retain partial outputs and the failed attempt. New selected-source comparison remains pending above. |
| Historical public CPU splitting qualification | No complete quality comparison is claimed here. The separate 48-render smoke check retains its corrected-fixture source. | New selected-source comparison remains pending above; requested lanes do not establish equal time. |
| Historical private GPU receiver qualification | No new a77 receiver comparison is claimed here; earlier OPUS measurements remain in their original-source section below. | New selected-source comparison remains pending above. No private scene, coordinates or artifact paths enter this document. |

#### Transient helper formatting and exact restoration

During the running main-a77 gate, formatting an external capture driver followed
its path module into `crates/ui-egui/tests/support/raster.rs`. This was the UI
**test helper**, not the production renderer's raster module. The incident
receipt records one unstaged path and an unchanged index. The accidental file's
recorded mtime was **9 November 2026 at 14:31:15.374469 UTC**; restoration of the
indexed original was recorded at **14:37:36.458700 UTC**.

The preserved diff contains wrapping, trailing commas, equivalent closure-block
syntax and semicolons on diverging `continue` statements. Independent review
found no change to numeric literals, arithmetic order, predicates, call order,
side effects or pixel compositing. Source locations and binary bytes can still
differ. The exact restored file SHA-256 is
`ea22488450d734cdecb64bb2b65849e63310d5764b83c4f1f852491dd2e0e7ea`.

**The source freeze was interrupted.** Matching initial/final snapshots do not
erase that event. The timestamps bound the observed file state, not the instant
at which each compiler read it. Keep the original gate result and incident
record together; do not present a later successful recovery as a replacement
for the original gate or as proof of uninterrupted historical source identity.

The reviewed include closure contains five native UI integration targets.
Attempts 02 and 03 each completed this same restored-source replay:

| Target | Declared cases | Passed per replay | Failed | Ignored |
| --- | ---: | ---: | ---: | ---: |
| `assistant_render` | 1 | 1 | 0 | 0 |
| `disclosure_render` | 2 | 2 | 0 | 0 |
| `joint_move_render` | 2 | 2 | 0 | 0 |
| `point_alignment` | 19 | 17 | 0 | 2 |
| `popup_regressions` | 2 | 2 | 0 | 0 |
| Total | 26 | 24 | 0 | 2 |

The ignored cases remain `context_menu_alignment_review_images` and
`midpoint_selection_review_images`. The two completed replays are not 48
distinct tests. Each used the restored helper, a feature-unified workspace
no-run build and the exact five Cargo artifact records. Per-target lists,
unfiltered execution logs and executable receipts reconcile with these counts.
The independent review of attempt 02 also rehashed all five executables and
confirmed their exact recorded bytes.

The earlier attempts remain part of the record:

- **Attempt 01:** `glass-controls-completion-main-a77-01` ran from
  15:57:42.095354 to 16:02:07.507351 UTC on 9 November 2026. Its no-run
  build exited 0 after 263.306502 seconds, then the wrapper refused with
  `Cargo rebuilt a normal library; review before capture`. It executed zero
  native tests and produced no capture. The refusal established neither UI
  outcome; the successful build alone was insufficient.
- **Attempt 02:** `glass-controls-completion-main-a77-02` completed the
  **24-pass, zero-failure, two-ignored** replay, then its external capture
  driver exited 1 with `Unexpected engine image host`. The capture attempt
  failed; the already completed test replay remained valid.
- **Attempt 03:** `glass-controls-completion-main-a77-03` repeated the same
  **24-pass, zero-failure, two-ignored** replay and completed the capture.
  The revised external driver checks the empty queue/results before clearing
  both default host owners; all per-frame no-host/no-job guards remain.

These separate requalifications do not replace the original full gate or erase
its transient source mutation. The successful capture is described below.

#### Required standalone web check

`cargo xtask ci` calls the web-offline check only after the native/wasm/doc phase
succeeds. A failure in that earlier phase returns before the web check. The
required separate command is therefore retained with its own receipt:

```sh
nice -n 10 cargo xtask web-offline-test
```

It uses the recorded Chrome executable, a new temporary browser profile and a
local fixture server. No user browser session or authentication is involved;
the sign-in scenario is simulated locally. With no `--site`, it exercises the
generated stand-in app, not a real compiled web application or `web-api-test`.
The success contract is exit zero, **20 named scenario results in source order**,
and exactly one `web-offline-test: passed` marker. The source describes 22
planned page launches; retries may add navigations. A passing standalone result
does not alter the full gate's recorded exit or its known-premerge comparison.

The separate run, `completion-main-a77-web-offline-01`, completed on
**9 November 2026**, from **16:03:22.616105 to 16:03:46.330056 UTC**,
in **23.713286 seconds**. The command and wrapper both exited **0**.
The retained log contains **all 20 expected scenario results in the required
order** and **exactly one** `web-offline-test: passed` marker. Its recorded
source, code, dependency and tool endpoint checks agree with the pinned inputs.
This establishes the stated stand-in/service-worker check only. It neither
qualifies a real compiled web application/API nor changes the full gate's
actual exit **1** or erases its earlier source incident.

#### Completed headless UI capture at the recorded checkpoint

`glass-controls-completion-main-a77-03` ran from **16:32:36.507710 to
16:33:30.491923 UTC on 9 November 2026** and passed its recorded checks.
The capture command exited **0**. Both **476 × 1,599** light-theme PNGs show
the application's actual CPU/GPU inspector controls; the recorded text
witnesses are fully inside their clip rectangles and are not elided. Both
images were visually reviewed.

The recorded interactions enabled **Use camera paths for glossy caustics**
with **Maximum base roughness = 0.35**, and **Trace both glass branches**
with one configured Fresnel sample. The serialized CPU and GPU options
contain the corresponding receiver and splitting values.

The harnesses contain new empty in-memory designs. Their queues/results
were checked empty before host removal; Live stayed paused; render hosts,
image jobs and private designs remained absent. Cleanup checks passed.
The receipt records unchanged source, code, libraries and capture binary.
These are **software rasters of real egui shapes and textures**. The raster
helper skips GPU callbacks: no native presented window, GPU viewport, private
session or render-quality measurement is claimed.

#### Completed clear-glass repeat at the recorded checkpoint

`glass-completion-clear-main-a77-01` finished on **9 November 2026 at
16:30:48.571098 UTC**, with final exit **0**. Build and combined CPU/GPU
render commands both exited 0. Recorded build time was **1,012.821746 seconds**;
the command rendering both backends took **70.287345 seconds**. These are
combined wall-time observations, not separate backend timings or an equal-time
comparison.

Each completed frame is **256 × 192**, with **32 complete observations per
pixel** (1,572,864 observations per backend), component/variance reconstruction
and extended optical guides. All **six encoded files**—raw EXR, display PNG
and diagnostic-mask PNG for each backend—are byte-identical to that backend's
pinned completed prior84 output. CPU and GPU are compared only with their own
references; cross-backend pixel identity is not expected.

The separate comparison to the earlier **pre-extension** masks confirms the
guide expansion exactly, rather than inferring it from aggregate counts:

| Guide correspondence | CPU | GPU |
| --- | ---: | ---: |
| Previously supported optical pixels retained | 3,754 | 3,754 |
| Former TIR-bypass pixels now supported | 2,687 | 2,687 |
| Current supported optical pixels | 6,441 | 6,441 |
| Changed mask pixels outside the former TIR set | 0 | 0 |

The current raw EXRs also match the respective historical off and pre-extension
filter masters. Independent decoding found finite RGBA, nonnegative RGB, and
unchanged coverage alpha in [0, 1]. The display request composites over an
opaque background, so PNG alpha is 255 throughout; it matches the historical
off PNG and is not a direct quantization of raw coverage alpha.

The completed frames identify `cpu-raytrace` and `gpu-path`. **This clear run
does not record a GPU adapter name, vendor/device ID, driver, graphics API or
hardware-versus-software adapter classification.** Its receipt cannot name
a device or establish hardware execution by every GPU-library test.

Visible grain remains in the glass. The matches establish preservation and
this fixture's supported-guide coverage, not independent image-error/energy
accuracy, general convergence or noiselessness. The new interface split is
off and caustic-map options are absent here; receiver/splitting comparisons
remain separate. Rough-transmission reconstruction remains unimplemented.

#### Incomplete receiver-quality attempt

`glass-completion-receiver-quality-main-a77-01` finished on **9 November 2026
at 16:43:49.175210 UTC** with an unsuccessful experiment. The build exited 0
after **67.335713 seconds**; the experiment exited **1** after **98.368900 seconds**.
The log records **64 completed area-source renders** across three area cases
and eight configuration groups, with eight seeds per group. Their 64 EXRs,
64 PNGs and associated metadata are retained.

At the start of the environment portion, request validation stopped the run:
`area_samples and seed require nonempty area_lights`.
The planned inventory was six cases, 16 groups and 128 renders. This is an
**incomplete comparison**, not a completed quality matrix; the remaining
inventory is unobserved. No final independent strategy analysis or quality
conclusion is claimed from these partial outputs. The exact one-line correction
and successful fresh smoke profiles are recorded above. They do not complete this
quality matrix. A77 CI02 later failed with a new test-isolation defect. Completed
a77 CI03 retained 28 exact known-premerge failures with no new or changed failure;
the repaired case passed. Fresh complete comparisons are assigned to the selected
`42918fe5` candidate; they had not started at this historical checkpoint.

No public image attachment was uploaded at this checkpoint. Later publication
status belongs to the [current integration record](#current-integration).

#### Preserved completion and historical evidence

These artifacts remain in the external evidence bundle. Generated files below
are identified by name and hash, rather than linked as if they were in Git.

| Artifact | SHA-256 |
| --- | --- |
| `ci-completion-main-a77-01-receipt.json` | `090c79a067c7620077addc7edc10696b536bbcf7c2ddabb7b282f06bf6bd660b` |
| `ci-completion-main-a77-01.log` | `187676bccd5537adec5b755d3a0d5f43587abcfd68f9501bac7e1e737eaab7c4` |
| `ci-completion-main-a77-01-baseline-comparison-final.json` | `51f2b41fe8d5e5e7e973754b740492557b823a44ff85317bf8bba77d5fd5470e` |
| `ci-completion-main-a77-01-native-summary.json` | `421cde07b07a12454542e35094dd5e831f6dded06ce29d08865adb8eb156a3a5` |
| `completion-bc9-preflight-03/receipt.json` | `da6ac4653afc33c1cd71e11469f76b26ad9b61102a8324187456ecfb6c4e3a0e` |
| `completion-main-a77-web-offline-01/receipt.json` | `640a257a5f8e80af54077abbfbfa0194640bde1491dfb1ae4f91c8308f9e7178` |
| `completion-main-a77-web-offline-01/web-offline-test.log` | `e820120d577a6bfbb8b839b651054f0755f7018f3632352060f26ea7a37ce8ba` |
| `glass-controls-completion-main-a77-01/receipt.json` | `424a2452942c948e57687c85363fef66c2158bba0ccd053af6e744dae0bdb4dd` |
| `glass-controls-completion-main-a77-01/libraries.jsonl` | `c32e9c33bf12da8076f80a2c72e682b87b1c4ffda7bf8517f6b3d185fc60369c` |
| `glass-controls-completion-main-a77-02/receipt.json` | `30f6add7191204c22d3e650bc245802737ab6db4e350b7d1fe406285e1092973` |
| `glass-controls-recovery-02-independent-review-01.md` | `c502b9a3e648aad34b2dc3b2d8dcb124054e4305af7a706002a6d838313c2df6` |
| `glass-controls-recovery-02-independent-review-01.json` | `4064195db9ed06599ec824a8e49a797076c06a2c5ef87830df12b23ccb478784` |
| `glass-controls-completion-main-a77-03/receipt.json` | `8dc877a8869a6cbb2e1b4b011963ff548b7fdd30b076177969a2c86aa1a8649d` |
| `glass-controls-completion-main-a77-03/captures/controls.json` | `1babe997f4e52acf9e42a05d2a117985c17d09b154ff31ca6a6f328061f8fa59` |
| `glass-controls-completion-main-a77-03/captures/cpu-glass-controls-light.png` | `be673ff5f07e7e1c66023f38dca61c3e292773d1bd77160e0944435e38bc3691` |
| `glass-controls-completion-main-a77-03/captures/gpu-glass-controls-light.png` | `dd09eb687526a984b830f9c645400538cd7950979a7ddec7a4960fc01f8d3516` |
| `glass-completion-clear-main-a77-01/receipt.json` | `2eeba49c0350a5bc6243edf109bc2cec2f920f151819c36106b6281155563e7b` |
| `clear-main-a77-independent-review.md` | `28b6234c687127c9167e6c514c2e0878b495eb7f2e776e4531bfbb5d4865caef` |
| `clear-main-a77-independent-review-02.json` | `a383114adda632be8a1a41b10d1bb1e436264f820883ac1d96b2486a517f23a0` |
| `glass-completion-receiver-quality-main-a77-01/receipt.json` | `707fb84401bce2d24c20610321d7576f52cdece13c4f4e1468f561f35c61aa69` |
| `glass-completion-receiver-quality-main-a77-01/strategy.log` | `bfbc3b276341e1d92871b49e5dd41a7dae17de6800c97a7563ad483781178751` |
| `actual-ui-rustfmt-incident-01/receipt.json` | `a579f1a3a82d1d6e82f84827f7168bb5a8934ddaaaca7ff572fc95472b2ca095` |
| `actual-ui-rustfmt-incident-01/index-original.rs` | `ea22488450d734cdecb64bb2b65849e63310d5764b83c4f1f852491dd2e0e7ea` |
| `actual-ui-rustfmt-incident-01/accidental-format.rs` | `09b78165a3c6ff36d3d21db3d8ebcbf7b4cdfeb4f4fff250191c1aa4afca9971` |
| `actual-ui-rustfmt-incident-01/format-only.diff` and `format-only-binary.diff` | `4103cfc08cbd696f095f539ef18c6ebd6d54e79e6cc73d0a3640188047464443` |
| `actual-ui-rustfmt-incident-independent-review-01.md` | `b5cb0c7e7845c1b69b7df3bfc305f4a50c4028184cec3543dba124d6438cbcb4` |

#### Corrected-source receipts and prepared evidence tools

These are the immutable correction, smoke, capture and completed CI02 records.
Prepared tool versions are listed separately from observed outcomes. CI02 retained
one new failure and is ineligible for image qualification. No image qualification was launched against the completed CI02 gate. Tool
hashes alone are not receiver, splitting or private results.

| Artifact | SHA-256 |
| --- | --- |
| `strategy-fixture-smoke-main-a77-02/edit-receipt.json` | `0a5d570934cebd75c20b8a53ed95f1e727c332e36414a32fb22d8a66ab771b6c` |
| `strategy-fixture-smoke-main-a77-02/receipt.json` | `74df5735a3830fdfcf49950d347e3c76e814ebf56984f64f72647494e813a239` |
| `strategy-fixture-smoke-main-a77-03/receipt.json` | `a1eb3ec5f022fbba9f063666c4fad50b6724d88ef0690e838760a7c8a2830454` |
| `strategy-fixture-smoke-main-a77-03/receiver-smoke/report.json` | `b36b2905feb15db51b8ceb2f3b6e418b37ccf72de7ac9e8a721742f98d89c3d0` |
| `strategy-fixture-smoke-main-a77-03/split-smoke/report.json` | `b30a4280860a9cf59cce2168315e3a5ac03721206c636c22882b0550d0b51aa0` |
| `completion-main-a77-source-freeze-02.json` | `a9d6f9c24de108cb18294b1e483f56e6116ea736eb807e1d2e799bf52945d945` |
| `publication-source-binding-main-a77-01/capture-receipt.json` | `0bc0b95e94798116306ebc472dcf9b3e551c0a5a19dc76df088f5af9ba792e4b` |
| `publication-source-binding-main-a77-02/capture-receipt.json` | `f7b8efefe922e893ea62a59d1fa7fc954d531fe2966f758fb10e916da68ae50d` |
| `publication-source-binding-main-a77-02/non-markdown-manifest.json` | `46acb950acc22c3e35e8b450264ea155d11d5c65c6cca445b33c967412e71235` |
| `run_completion_ci_main_a77_02.py` | `42ac833c7ae70ab15f604518a547a6b2aeb179e026c6fd9419688537069226c7` |
| `summary_ci_native_main_a77_02.py` | `fad9526e8a3f687e5a71b73f365ade8f39cbc287e5de3624a8bd18242d1fc133` |
| `completion_image_evidence_main_a77_02.py` | `36d1bed8b2f0bb62150a211b7a85f223c1eeb3fc34a762c00f723f1879fc001a` |
| `run_completion_strategy_evidence_main_a77_02.py` | `00cd35df59db8ad196dbf558206eafe61797b87841ea56e7f7dc50af500af3fd` |
| `analyze_completion_strategies_main_a77_02.py` | `fec011761b9778e12d15a5073577fc82b6df69a846d3094e4dad3deb8701cbd0` |
| `publication_source_binding_main_a77_02.py` | `9d6af4bf5bc6d345ddbf070e43cfb3f8893730389da3661dbf04615fad27d163` |

#### CI02 failure, isolated repair and historical CI03 artifacts

The failure, triage, repair, formal freeze, focused pass and publication03 capture
are observed records. Completed historical CI03 artifacts are listed above. The
03 tools below remain distinct from their outcomes; their hashes alone establish
neither runtime success nor current-source image results.

| Artifact | SHA-256 |
| --- | --- |
| `ci-completion-main-a77-02-receipt.json` | `2706c22bc22681800f9d729d38773e99ce25b64bfe33bffd3eb2b9e8092e9945` |
| `ci-completion-main-a77-02.log` | `5e068e258acafaa29c95f7d56c4f619135675afb7d29654bb75f2d340d06b7ef` |
| `ci-completion-main-a77-02-baseline-comparison-final.json` | `cd30324c520aeb1ac07670b2130cb77b00aafdc9f5f0b28b8966b71617b26bd3` |
| `ci-completion-main-a77-02-native-summary.json` | `a4763722ad74f3737cd45c836d55d2f0de75e81d20916b73cbf536293940a7bf` |
| `ci02-native-epoch-isolation-triage-01/triage.md` | `c60537ccd515e20136222c36ad6e6069ebcd19ae6e66b66537830ccfd8c853c2` |
| `ci02-native-epoch-isolation-triage-01/triage.json` | `aa45abc6425d7e5bd33ea979f3fea62b8e5383a40bb4a02333bb47aa84f1a2b8` |
| `native-ram-epoch-isolation-repair-01/receipt.json` | `0cf2590903c383f4929aef80efb89c5d6a786a12370ff3239dc9f3f09cc972c9` |
| `native-ram-epoch-isolation-repair-01/isolation.patch` | `ce84c429133975737a4dba5d6315a7e9da973f8fc78cdc0c9dc1f7233c3f3c49` |
| `native-ram-epoch-isolation-repair-01/job_tests.before.rs` | `9b884daba671a5d28307864eda77ca7f9b12762ab214e996bab63f1571b1fcea` |
| `native-ram-epoch-isolation-repair-01/job_tests.after.rs` | `e4f32e3cebbe8cb06b21c9ff177d88b5eb123aa3fa651506a9b4795789acd948` |
| `completion-main-a77-source-freeze-03.json` | `6aff5e558a7a96633e7f30c569455667ccc7a754dc98525852aca94cb7bb7295` |

| Completed replay / source-capture artifact | SHA-256 |
| --- | --- |
| `native-epoch-isolation-focused-main-a77-03/receipt.json` | `992cc07f1882b7bc039db8834b69a9dfc26e00bf745f573077a73618b7ade442` |
| `native-epoch-isolation-focused-main-a77-03/build.stdout.log` | `aa21bc7100f0c8f85f6396fa06daa82a5fda4c78377daa48a26dd4995e4006c6` |
| `native-epoch-isolation-focused-main-a77-03/build.stderr.log` | `c34140d97da08d943ca0c5f138718d75e653056aa86fb13d72cebcb2e8d396eb` |
| `native-epoch-isolation-focused-main-a77-03/list.stdout.log` | `a73cdb81c9fdb35bd83c580043ed480b0f5a6e2f892ac99612b2e7877548c390` |
| `native-epoch-isolation-focused-main-a77-03/test.stdout.log` | `c61dad26e8c36108049414aa99f13a2bc8ed1e613dd3a8f35c0154d3bf3bb720` |
| `publication-source-binding-main-a77-03/capture-receipt.json` | `427519f4246ad49a9eacc383e3b779fefc6d2475b90afc2cb5f790d44dc31c10` |
| `publication-source-binding-main-a77-03/non-markdown-manifest.json` | `e7dcaab01b04a9a78faf5c5767f747cf2f7b2b239b745fd8912ab18516bf3387` |
| `publication-source-binding-main-a77-03/ci-start-receipt.snapshot.json` | `5dd4d4d7eac7d4476f475d6a8a0820c3432873c3bb0054806fbfd764d1219cde` |

| Prepared tool | SHA-256 |
| --- | --- |
| `run_completion_ci_main_a77_03.py` | `3233ebde74061672817382b799b73890038582db320ed2ce79135006a7f0c14d` |
| `summary_ci_native_main_a77_03.py` | `e7c4a4d2edfe67d7b46bcfbdb08cd93ffa29b2741bf6ed6d23a0dc0f856a1e3c` |
| `completion_image_evidence_main_a77_03.py` | `fbb0bfc377384b22d815f896aba8cfa0a71774b5651ef58e2aec6db5c00211ac` |
| `run_completion_strategy_evidence_main_a77_03.py` | `f72612afe5d47de3b306490344d68b92915c2495f9d5af8c24600735769669d3` |
| `analyze_completion_strategies_main_a77_03.py` | `8d6d62a42d8bf5636ffb5e70d77f2e453c5840922bbe5aaa72a24aedd7d15610` |
| `publication_source_binding_main_a77_03.py` | `c6be10a029ad35190559589b6a7ffadb732c8a7d922470b7ea01a48dca4adaf0` |

## Dependency baseline (bc9b0b36)

**Known premerge comparison; not an executed baseline of main `a77b7ab2`.**

The completed premerge parent gate tested the exact staged tree later committed as
`bc9b0b36208af7703779d131804891a78df47a76`. It ran from
**2026-11-09 08:32:27.020 to 09:58:11.777 UTC**, taking **5,144.757 seconds**.
**The gate exited 1.** Native nextest exited 100 with **14,519 passed and
28 failed**, or **14,547 executed tests**, plus **68 filter exclusions**.
The log contains 97 collected binaries; JUnit contains 92 nonempty suites,
14,547 unique testcase identities, zero XML errors/skipped rows and no retries.
The wasm check for 22 crates and the doc-test side job passed.

All 28 failure identities and normalized bodies match the preceding
`23f173d3` parent exactly: 12 engine-library cases, 11 kernel cases, two engine
pleat cases, one CLI action matrix, one CLI naming corpus and one SMLib case.
The extracted baseline retains every raw body and its hash. Normalization only
handles recognized diagnostic process/thread IDs, outer whitespace, selection
from the first Rust panic header with any discarded prefix recorded, and at most
one exact trailing standard backtrace hint. Assertions, source paths, values and
error causes are not erased.

The before/after tree is `dca6b91cd15bdfde824d5ff6bac070491641457a`, with no
unstaged or untracked paths. The tested precommit HEAD is
`f9ed9a90b453517d83e787169887c180e17656fd`; the local commit receipt binds that
parent and source tree to `bc9b0b36`. This establishes exact **source-tree
equivalence**, not an executable compiled with HEAD `bc9b0b36`. Embedded Git
identity and HEAD-derived cache metadata retain that distinction.

| Parent artifact | SHA-256 |
| --- | --- |
| `validation/quality16k-v12-ci.json` | `71718def90c180779956019a876f693cab9a72f4945227846cb5829598587368` |
| `validation/quality16k-v12-ci.log` | `0e27462f226e6d14bee345da5d214196159e19114009e14d60493378abc46d43` |
| `validation/quality16k-v12-ci-local-ci-junit.xml` | `754c5e6078194bbc88f1c687d9dd4e037732298f7488ba35a4c49c6a668b7854` |
| `quality16k-v12-local-commit.json` | `17fb26a037d3ebcef2896cd2d940889a10b1309f57f8df185995b0041b698ab0` |
| `parent-bc9-baseline-failures.json` | `ee0f2ac052ddd80dd005b727804d2b4dd58cc6b6e414eb27077a47c6d506d00b` |
| `new-parent-bc9-baseline-audit.md` | `07cc24a19fdb8074b8b15cf5658778023583740eaee5602274dd81c643770b5c` |

The first four artifacts belong to the parent's external
`merge-review-20261108T082012Z` bundle. The baseline and audit were derived from
its original qualified JUnit and source receipts, without rebuilding or modifying
the parent. They establish a comparison reference, not a passing integrated
gate or merge readiness.

## New dependency baseline (23f173d3)

A completed parent `cargo xtask ci` run tested the exact staged source tree later
committed as `23f173d334f349224e379c61ce2e7052b976c1a1`. It ran from
**2026-11-09 05:28:58.136 to 06:44:29.302 UTC**, taking **4,531.166 seconds**.
**The gate exited 1; it did not pass.** Nextest executed **14,529 tests**:
**14,501 passed and 28 failed**, with **68 filter exclusions**. It reported
97 collected binaries. The JUnit contains 92 nonempty suites, 14,529 unique
executed testcase identities, 28 failures, zero errors and zero skipped rows;
the 68 exclusions are not skipped JUnit rows. The wasm check for 22 crates and
the doc-test side job both passed.

| Parent failing suite | Failed cases |
| --- | ---: |
| CLI action matrix | 1 |
| CLI naming corpus | 1 |
| Engine library | 12 |
| Engine pleat regressions | 2 |
| Kernel | 11 |
| SMLib | 1 |

Source-before and source-after records are identical, with no unstaged or
untracked paths. Their tree is
`f814b8cbe893b97283a44cfdea7b7306636c8d43`, which matches the published commit's
tree. The tested HEAD was its parent,
`11a799b5653a3c34195ae242bf92bb82a1c48d5d`; the local commit receipt binds that
tree and parent to `23f173d3`. This is exact **source-tree equivalence before
commit**, not a claim that the test executable was built with HEAD `23f173d3`.
The build includes HEAD-derived cache/provenance metadata, so that distinction
is retained.

| Parent artifact | SHA-256 |
| --- | --- |
| `validation/quality16k-v11-ci.json` | `def9fac7b22fc6a4dcccd8fac393f85c998410e801312380384bc425af805af3` |
| `validation/quality16k-v11-ci.log` | `d3b727405135490c0c98d213fb834bedd8c64a90cf288a337a48045ad3754d87` |
| `validation/quality16k-v11-ci-local-ci-junit.xml` | `c5d6862abe800cb66a4b97729583bfb361d8a48518fb11e9f19fc0b67b550f1c` |
| `quality16k-v11-local-commit.json` | `378e189e03c0fa5fb1032822c740c22e4599acefd25b53839837cc574238c10f` |
| `parent-23f173-baseline-failures.json` | `8407131a35697e5bc9f11af5318173477aa303ab37f63e603643666a9bb19372` |

The first four artifacts belong to the parent's external
`merge-review-20261108T082012Z` evidence bundle. The final artifact retains all
28 raw failure bodies and the pinned V2 normalization policy for the new
integration comparison. The earlier 19-failure `84bda659` baseline remains
separate. Matching names alone is insufficient; source locations, values and
diagnostic bodies must be assessed. No comparison against a new integration log
has been completed here.

## Historical measurement environment and source identities

The original capture matrix, photon-radius sweep and OPUS measurement used
rendering PR #12546 at `66bc1b7de239d4b70feb41e6c489dedd24adff60`. The later
internal-reflection comparison used its updated dependency,
`84bda659c1f695d17cb7d4d9926f6eb8264e001f`, after the implementation was rebased.
The dependency remained open during these measurements. It has since merged as
recorded above. PR #13904 remains draft for implementation review and fresh
qualification of the selected main-based source; these historical receipts are
not relabeled as post-merge results.

Measurements used an AMD Ryzen 7 7840U, an AMD Radeon 780M integrated GPU with
RADV/Vulkan, Mesa `26.2.6-arch3.1`, Linux `7.3.1-1-cachyos`, and Rust
`1.99.1 (58cf8f9c3 2026-10-13)` with LLVM 23.1.1. Rust builds used the repository's
`ci` profile, two Cargo build jobs and low scheduling priority. These are
correctness and quality measurements on a shared machine, not release-build
throughput claims.

The first complete image/OPUS measurement set preceded the bounded internal-
reflection guide extension. Its identity was:

| Item | Recorded identity |
| --- | --- |
| Worktree HEAD | `6b04dd2008e04bf555f58f21c4f6861ade3cbbf7` |
| Tracked binary-diff SHA-256 against HEAD | `c74e82daaf8f948c94dadaa560177d0470a12fff3d76fe255efe5578fa870eb9` |
| Complete source-state receipt SHA-256 | `6cda338164d13a032662d8c603879e9c35746158002d14840110f82a88b5fb99` |
| CLI executable SHA-256 | `0aa416698aefcd21152611d70dc159f39bccc645efcd3a31476bbf95f53ae4fc` |
| Later glass comparison executable SHA-256 | `8b7a64426a1908feaa9776c1a7857544a0e2f37c65106e427b316d29e74521c2` |
| Earlier CPU matrix/clear comparison executable SHA-256 | `03cc783104bf4488ed490d307586cd615955a136f0cada47bb48df61cc3e5916` |

The source-state receipt combines HEAD, the tracked diff and every nonignored
untracked path, mode and content hash. A source checkpoint retains the actual
diff and new files. The observed checkout identity is distinguished from the
executable hash; it is not substituted for a build attestation. A subsequent
same-command build completed with the source unchanged. The OPUS run also
recorded identical source states before and after execution.

The earlier CPU matrix and first clear-glass pair used the second comparison
executable listed above. The later executable additionally exports the categorical
guide mask in filter mode; the repeated CPU filter run verified that enabling
those diagnostics preserved both raw and display output. The per-run receipts
retain each executable identity.

Generated images, raw masters, complete settings and private scene evidence stay
outside git. The public benchmark scenes are original geometric fixtures.

## Historical dependency CI baseline (84bda659)

The earlier pinned dependency did not have a clean whole-workspace test baseline.
Its recorded `cargo xtask ci` run on 8 November executed 14,391 tests:
14,372 passed and 19 failed; another 68 were skipped by its nextest runner.
The source tree was unchanged across that run and exactly matches commit
`84bda659c1f695d17cb7d4d9926f6eb8264e001f`.

| Recorded identity | Value |
| --- | --- |
| Tested source tree, before and after | `bea3d3105ce7ce7e176381cbe25540bb652c8424` |
| CI interval, UTC | 2026-11-08 21:36:59–23:14:31 |
| CI log SHA-256 | `744b667ecea38dd15a2edfab455f831ac54452ff2c3c05d124c2ac4dfd314194` |
| CI receipt SHA-256 | `a8ab3404e8f2991a29eba94ca6bf2ba8ed598f9a4f3826a9ddb09065c7a869c4` |
| JUnit SHA-256 | `7cf6cbb28f6de18dfa1ad5803f021ba4f3a23c94d69bb39cd7018b662c3d62d6` |

The CI ran before that staged source tree was committed, so its recorded HEAD is
`44525da6699b46cf00ce696c4c5dfd06874116d5`. The receipt records no unstaged or
untracked files; its source-tree identity, rather than that earlier HEAD, binds
the execution to the published dependency. The receipt, log and JUnit hashes were
independently checked.

| Baseline suite | Failed tests |
| --- | ---: |
| CLI action matrix | 1 |
| CLI naming corpus | 1 |
| Engine | 8 |
| Kernel | 8 |
| SMLib | 1 |

For example, the naming corpus rejects `oracle/83-boundary-loop-thicken` while
loading step 8, `surface.thicken`: the offset solver cannot prove the offset of
face 0 by −1 regular. The fixture already carries a `known_failure` annotation;
the naming test does not consume that annotation. The two action-matrix failures
are existing shell operations on the lid-ribs and filleted/shelled-box fixtures.

A baseline test name alone does not excuse a new failure. The implementation's
validation compares both the failed-test identity and its panic/error body with
this recorded baseline, removing thread/process identifiers and outer
whitespace. V2 additionally removes at most one exact trailing Rust
`RUST_BACKTRACE=1` hint line: nextest emits that hint in each failing process,
while grouped libtest execution does not repeat it for every panic. Source
locations, error text, assertions and values are preserved. The original strict
comparison and the seven reviewed hint-only differences remain in the evidence. New rendering failures are repaired and retested. Known dependency
failures remain visible; the complete gate must not be described as green while
they remain. Kernel/tessellation code and those expected outcomes are outside
this rendering change.

## Historical integrated checkpoint (CI04)

After the internal-reflection extension, the complete `cargo xtask ci` run
finished in 5,540.48 seconds on an unchanged source tree. It executed 97 native
test binaries and completed both side jobs: the wasm check for 22 crates and
the doc tests. The native totals were **14,435 passed, 21 failed and 68 ignored**.
The gate exited **1**.

| Suite at this checkpoint | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| CPU renderer | 924 | 0 | 2 |
| GPU renderer on the hardware adapter | 295 | 0 | 3 |
| Document library | 658 | 0 | 2 |
| UI library | 5,186 | 1 | 21 |
| Engine library | 4,111 | 9 | 13 |
| Kernel | 811 | 8 | 4 |
| SMLib | 350 | 1 | 1 |

Separate UI integration binaries passed another 254 tests, with three ignored.
The formatting, asset, license, layer, isolated shared-crate, UI-container,
mesh-decision and strict Clippy checks passed. This does not change the failed
result of the full gate.

All 21 failures were reconciled against the source-attested dependency:
19 matched its failed-test identities and normalized panic/error bodies;
none had a changed baseline body, an incomplete failure block or an unexplained
count. Two failures were new test-fixture assumptions:

- The saved CPU denoise test expected an exact settings object without the three
  newly added false defaults. Its expected object now includes those fields.
  The legacy input deliberately still omits them, preserving the compatibility
  test and all subsequent raw-output, regrade and document-history assertions.
- The look-stack UI test clicked a registered control after it had scrolled
  below the inspector's visible area. It now reveals that control through the
  existing UI helper before clicking. Ordering, undo, saved settings,
  photographic intent and transparency assertions remain unchanged.

A separate repair run verified that only those two reviewed test-local blocks
changed and that all other source remained identical. Full formatting, strict
workspace Clippy and the workspace-feature-unified build passed; each affected
test then ran in its entirety and passed, with exactly one selected test and
zero ignored tests. Source and isolated executable hashes remained unchanged
through those checks. These subsequent checks do not rewrite CI04 as a passing
full-suite run.

The standalone browser offline/update scenarios also passed, in 22.25 seconds
with unchanged production source. They use the offline-test stand-in application;
they are not browser renderer image-quality qualification.

| Checkpoint evidence | SHA-256 |
| --- | --- |
| CI04 complete log | `7af3f8d9d0a345efcd00b06d73a4ee8de3de381369019046edaa4ed390404d05` |
| CI04 source/execution receipt | `79e8e1dfd52bba860563f537542345893ebca8be396d728c50383e3853dd2697` |
| Complete V2 failure comparison | `6cfff1eb34927e9210dfaca8b2af2c81345476fb1b6ced8e1b3f35dfd86e9a2c` |
| Scoped fixture-repair receipt | `8bd1e674dec9425a9e1990513684f9c8b30400e3b9a910254b92ff681c1bd44e` |
| Offline browser log | `8ef01b3e6c60c70bfd77851530f39a4ae5774f1bef2aa7ef160d65d4942d323e` |

This checkpoint precedes the follow-on camera-prefix probe and component/map
pilot work. It is retained as evidence for that source state, not substituted
for verification of later production changes.

## Completed prior-source integrated gate (CI05)

The completed `cargo xtask ci` run for the follow-on camera-prefix probe and
component/map pilot implementation used the frozen `84bda659` dependency
checkpoint, before transfer to later integration checkouts. It completed on
**9 November 2026**. It ran from
**08:03:40.191 to 09:05:53.783 UTC**, with **3,733.45 seconds** of recorded total
elapsed time and **3,655.7 seconds** in the native test phase.

The run completed **97 native test binaries: 92 successful and five failed**.
Native totals were **14,456 passed, 19 failed and 68 ignored**, with zero measured
or filtered-out tests. That is 14,475 executed native tests and 14,543 discovered
tests including ignored cases. Both side jobs passed: the
`wasm32-unknown-unknown` check for 22 crates and the doc tests.

**The full gate exited 1. It did not pass.** All 19 observed failures match the
pinned dependency's failed-test identities and normalized panic/error bodies.
The completed comparison reports **zero new failures, zero changed baseline
bodies, zero unparsed failure records and zero count mismatches**. This is a
baseline-equivalent nonzero result, not a green repository gate or a claim that
every possible regression has been excluded.

### Core suites and counting policy

| Library suite in CI05 | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| CPU renderer | 936 | 0 | 2 |
| GPU renderer | 301 | 0 | 3 |
| Document library | 658 | 0 | 2 |
| UI library | 5,187 | 0 | 21 |
| Engine library | 4,113 | 8 | 13 |
| Kernel | 811 | 8 | 4 |
| SMLib | 350 | 1 | 1 |

The recorded environment set `SOLIDCRAFT_REQUIRE_GPU=1`. The 21 UI integration
binaries passed 254 tests, with three ignored; those results are already
included in the native aggregate above.

The summary counts exactly one final outer libtest result per native binary.
It excludes earlier shared-crate isolation checks, captured child-process
summaries and side-job test counts. Successful native commands retain their
final stdout result on the runner row. Failed commands are checked against
their outer running-test count, final failed-name list, parsed testcase
sections and the final failed-command footer. All 97 native identities are
unique. The doc side job retains its final successful result, so no aggregate
doc-test count is inferred from that one line.

The five failed native binaries account for the same 19 tests recorded on
dependency `84bda659c1f695d17cb7d4d9926f6eb8264e001f`:

| Failed binary family | Failed tests matching the parent |
| --- | ---: |
| CLI naming corpus | 1 |
| CLI action matrix | 1 |
| Engine library | 8 |
| Kernel | 8 |
| SMLib | 1 |

The final V2 comparison was recomputed from the complete immutable log and
pinned parent failure artifact. It compares from the first Rust panic header
when present, or the whole testcase diagnostic otherwise. Normalization
removes only recognized numeric thread/process identifiers, outer whitespace
and at most one exact trailing standard Rust backtrace-hint line. Source
locations, assertions, values and other diagnostic text remain significant.
Every failed block was complete, and the declared and parsed failure counts
agreed. The original logs and diagnostic bodies remain in the evidence.

### Tested source identity

The complete source state and the non-Markdown code identity were unchanged
from before to after CI05. The same identities were used by the immediately
preceding successful targeted preflight.

| Recorded identity | Value |
| --- | --- |
| Worktree HEAD | `dd39f36fc139939bb3da8b930b29827936d86bfa` |
| Full source-state SHA-256, before and after | `05811f372ed18d3d6b95a1f0dc0734c79d22ff84c0b61615625b0a0bff02edb9` |
| Non-Markdown code-identity SHA-256, before and after | `2b96dc8b70cf97bf93a8f3e4ea97da494606f1eb084819f563c78673d0cb20a3` |
| Recorded compiler | `rustc 1.99.1 (58cf8f9c3 2026-10-13)` |

The full source identity includes the tracked diff against HEAD and any
nonignored untracked files; HEAD alone does not identify the implementation
under test. This is an execution/source checkpoint, not a substitute for the
separate executable and build receipts attached to rendered-image evidence.
The elapsed times are recorded run durations on the shared machine, not a
throughput comparison with previous CI runs.

### Relation to CI04 and targeted preflight 07

These checkpoints remain distinct:

| Checkpoint | Scope | Observed result |
| --- | --- | --- |
| CI04 | Earlier complete integrated source state, before follow-on probe/pilot work | 97 native binaries; 14,435 passed, 21 failed, 68 ignored; exit 1. Nineteen failures matched the parent and two were reviewed test-fixture failures. |
| Targeted preflight 07 | Selected follow-on regressions on the same full source/code identities later used by CI05 | 28 selected tests passed: 14 CPU renderer, eight GPU-suite, three engine and three UI; no selected tests ignored. Formatting, strict workspace/all-target Clippy and the feature-unified workspace test build also passed. |
| CI05 | Complete integrated run including the follow-on implementation and fixture repairs | 97 native binaries; 14,456 passed, 19 failed, 68 ignored; exit 1. All 19 failures match the parent; no new or changed failure was observed. |

Targeted preflight 07 finished at **08:03:04.540 UTC**, before CI05 began. Its
selected results are not added again to CI05's native totals. The separate
targeted passes do not retroactively make CI04 pass, and CI05's baseline match
does not change its nonzero exit code. The earlier image and OPUS measurements
retain their own source and executable identities.

### Retained evidence

The following artifacts remain outside git. Each hash identifies the complete
file used for this final accounting.

| Artifact | SHA-256 |
| --- | --- |
| `ci-rebased-implementation-05.log` | `da2767f4a25ee794927f15d3d2243b13bada0aa4a8647cf12d7f9d681498b175` |
| `ci-rebased-implementation-05-receipt.json` | `dbcd23e719674260a9b80d6a3df6aab11d0bfc176011a9860068f6cfe04140b5` |
| `ci05-baseline-comparison-final.json` | `7b9b2a5fb889fc77fbbf89cfb132e6cc841c4cde77352d7cda35575095a8f15d` |
| `ci05-final-native-summary.json` | `b39ee2f7a43c2d6cbc3068c69219186699c94cd649a6b67adf78461d2a78a662` |
| `parent-84bda-baseline-failures.json` | `97967c752b389006051b828abbf1f9720aa845aa462ea674a82fdd77ee3a1ce5` |
| `completion-preflight-07/receipt.json` | `eb297076a179fc0c895bee6a64b3d03503575ab5c992e5686bb95a74e8c67d70` |
| `summary_ci_native.py` | `5083f25ad537fc1fb85052dd8ff240ad7d151730da8e7c49340a35703b0468b4` |
| `compare_ci_baseline_v2.py` | `c8633cdefaad36a6de45de6003778dcff4ea12096cae644907a3ac77fc37d36e` |

The summarizer was first qualified against the completed CI04 record, including
its two captured child success summaries, and reproduced that record's native,
core-suite, UI-integration and side-job results exactly. Its CI05 analysis
verified the receipt/log/comparison hashes, all native identities, both side
jobs and every failed-block count.

This section reports repository verification. It does not establish noiseless
glass, an independent image-error or photon-bias bound, equal-time sampling
improvement, or a fresh private-scene result. Those conclusions require their
own completed render and analysis receipts.

## Correctness checks before the internal-reflection extension

| Check | Executed result |
| --- | --- |
| Full CPU renderer library with `raytrace-cpu` | 917 passed, 2 existing ignored |
| Targeted GPU guide/filter, paging, adaptive, sampler, component, photon-probe and shader checks | 36 passed on the hardware adapter |
| Pending design-format merge/fold check | Passed |
| Render-workspace UI tests | Initial run: 484 passed; two new fixture errors identified and corrected before final verification |

The UI fixture corrections preserve all assertions. One now distinguishes the
authored setup camera from the independently preserved live camera across Save,
Undo and reopen. The other allows an ordinary layout frame before clicking a
control whose enabled state changed during the previous frame.

The GPU checks include component-M2 overflow with finite, constant beauty:
exclusive components can exchange large values while their sum has zero
variance. Both accumulation and readback reject invalid component moments before
reconstruction. The raw accumulator is not clamped or rewritten. Other checks
cover CPU/WGSL sample-bit parity, open-interval and half-open Fresnel coordinates,
complete-group observation counts, dim-radiance recombination, raw/alpha
preservation and full-window versus paged reconstruction.

These earlier image and photon-radius experiments retained the independent
sampler, one Fresnel stratum and disabled CPU roulette. The later
[CPU sampling comparison](#cpu-sampling-comparison-at-the-prior-source) evaluates
those options across four scene classes, with its timing and finite-seed limits
reported separately. It does not establish equal-time superiority.

The completed post-extension follow-on checks are recorded in
[CI05 and preflight 07](#completed-prior-source-integrated-gate-ci05). They belong
to the frozen prior source; the new integration gate remains outstanding.

## Capture must preserve transport

The existing `glass_comparison` example rendered all fourteen cases at
64 × 48, 16 camera observations: clear, tinted, hollow, nested, sheet, rough,
dispersion, film, rough sheet, caustics, rough lights, rough-sheet lights,
rough caustics and scattering caustics.

On CPU, capture on/off produced **byte-identical EXRs and PNGs in all fourteen
cases**. Each capture report counted 49,152 complete observations. The maximum
reported recombination residual across the cases was
`2.842170943040401e-14`.

On GPU, all fourteen PNGs were identical. Thirteen EXRs were byte-identical.
The rough-caustics comparison differed in one of 12,288 FLOAT channel values:
blue at `(20, 6)`, by one ULP (`2.9802322387695312e-8`). The maximum reported
component recombination residual across the cases was `3.0517578125e-5`.

A repeated capture-off job also changed one channel by one ULP: green at
`(26, 22)`, `7.450580596923828e-9`. Photon buckets use parallel atomic insertion,
so independently built maps need not traverse the same photons in the same
floating-point reduction order. An independent-job byte comparison therefore
cannot isolate capture-related arithmetic. The follow-up controlled regression
reuses one emitted map to check capture off/on without that source of variation.
It passed for a rough closed dielectric and a rough thin sheet, using two Fresnel
strata and forced pages. Both replays retain every photon and hash link, include
nonzero reflected or transmitted rough-photon energy, and produce exactly equal raw
linear output. The one-ULP independent-job observation is not relabeled as
physical noise or hidden by image quantization.

## Earlier clear-glass reconstruction

At 256 × 192 and 32 samples, both CPU and GPU filter runs preserved their
respective raw EXRs exactly. The CPU raw SHA-256 was
`48a7ce8bfa20eb578757bfcdbd46d7ea919fcfb3620276f8ce16bc613c966835`.
Enabling the categorical guide diagnostics also preserved the CPU EXR and PNG
exactly.

Before the internal-reflection extension, both backends reported:

| Guide class | Pixels |
| --- | ---: |
| Supported optical correspondence | 3,754 |
| Total internal reflection bypass | 2,687 |

Other categories include ordinary surfaces, background/emitters and partial
coverage; their counts are backend-specific. The two optical counts agree.
An independent analytic ray/box/Snell calculation predicted those same 3,754
transmissive-exit and 2,687 first-exit-TIR centers. All predicted TIR pixels were
unchanged by the original filter, while 3,725 of the transmissive-exit pixels
changed. The lower noisy band was therefore a measured guide-domain limit.

The supported region was actively reconstructed: 2,593 of 3,074 pixels changed
in the central glass crop. Visible grain remained at 32 samples. Changing pixels
is evidence that reconstruction ran; it is not, by itself, a quality score.

This result motivated the bounded clear-body internal-reflection extension.
A subsequent run on dependency `84bda659c1f695d17cb7d4d9926f6eb8264e001f`
used the same scene, view, sample count and reconstruction choices. Its comparison
executable SHA-256 was
`10cd7d835247b7d1152ff22256f487bf84d42e67d6986e6f0fcf2193d2bd4b85`.
Both backends then reported **6,441 supported
optical pixels and zero TIR bypasses**: all 2,687 previously bypassed center paths
were represented. This fixture encountered none of the new topology, route or
reflection-count limits.

Both raw EXRs remained byte-identical to their respective pre-extension masters.
The GPU raw SHA-256 was
`68afb86f3fbf48a9cf1eaf718dec56f202af8a9efae74a09a082f772017ff851`;
the CPU hash is recorded above. In the display output, 2,655 of the 2,687 formerly
bypassed CPU pixels changed, as did 2,666 on GPU. All **42,711 pixels outside the
optical region** were unchanged in each backend, and no alpha value changed.
A small number of already-supported optical neighbors also changed as useful
filter taps became available: 125 CPU pixels by at most 4/255 per channel and
120 GPU pixels by at most 2/255.

Inspection of the actual 256 × 192 images shows reduced grain in the lower glass
band while the checkerboard remains visible. Grain is still visible elsewhere at
32 samples. These observations establish expanded, localized reconstruction and
raw preservation; they do not provide a calibrated image-error or detail-loss
bound. Rough, nested, scattering and otherwise ambiguous correspondence remains
explicitly unsupported.

## Clear-glass repeat at the prior source

The public clear-glass comparison was repeated on **9 November 2026**, after
CI05, with the frozen `84bda659` follow-on implementation. This repeat precedes
the transfer to the earlier `23f173d3` integration. Both CPU and GPU rendered the
same **256 × 192**, **32-observation** fixture in filter mode. The build and
example invocation both exited 0. Their receipt retains CI05's nonzero,
baseline-equivalent gate status separately.

Within **each backend**, the new raw EXR is byte-identical to both its
pre-extension master and its earlier internal-reflection-extension master.
The display PNG and categorical guide mask are also byte-identical to their
respective earlier extension outputs. These are exact file comparisons, not
tolerance-based or CPU-versus-GPU equality claims. The exported hashes and
diagnostic counts were checked against the actual files.

| Backend | Supported optical pixels | TIR bypasses | Topology-limit bypasses | Route-limit bypasses | Reflection-limit bypasses |
| --- | ---: | ---: | ---: | ---: | ---: |
| CPU | 6,441 | 0 | 0 | 0 | 0 |
| GPU | 6,441 | 0 | 0 | 0 | 0 |

The earlier visual comparison therefore applies unchanged to these recorded repeat
outputs. All 2,687 formerly bypassed center paths remain supported; compared with
the original pre-extension display, 2,655 of those CPU pixels and 2,666 GPU pixels
change. The 42,711 pixels outside the optical region remain unchanged in each
backend, and alpha is preserved. The reviewed images retain the earlier
reduction in grain in the lower glass band, with visible grain still present
elsewhere at 32 observations.

This repeat establishes preservation and repeatability for the recorded clear
fixture. It is not an independent image-error or detail-loss reference, a photon-
density bias measurement, or a noiselessness guarantee. The fixture encounters
none of the bounded topology, route or reflection-count limits; it does not
qualify broader rough, nested or ambiguous optical correspondence.

### Source, executable and timing

The source was unchanged before the build, after the build and after rendering.
Its full source and non-Markdown code identities are the same as CI05:

| Identity | SHA-256 |
| --- | --- |
| Full source state | `05811f372ed18d3d6b95a1f0dc0734c79d22ff84c0b61615625b0a0bff02edb9` |
| Non-Markdown code identity | `2b96dc8b70cf97bf93a8f3e4ea97da494606f1eb084819f563c78673d0cb20a3` |
| Copied comparison executable, before and after | `2043d3d215dee2d0be463095fabdcbcd454dd4b5bff3bb146ad4b882e130556a` |
| `final-glass-implementation-02/receipt.json` | `4d42b91478ea7022c467b926238a222a2601e2f4c847a909262afbad1060477a` |

The recorded build took **423.978 seconds**. The combined CPU/GPU example
invocation took **164.132 seconds**, including its setup and output work.
These are provenance timings on the shared machine, not per-backend transport
benchmarks or evidence of a speedup. The wrapper finished at
**09:16:31.271 UTC**.

### Export identities

These files are retained under
`final-glass-implementation-02/images/` outside git.

| Export | SHA-256 |
| --- | --- |
| `cpu-clear.exr` | `48a7ce8bfa20eb578757bfcdbd46d7ea919fcfb3620276f8ce16bc613c966835` |
| `gpu-clear.exr` | `68afb86f3fbf48a9cf1eaf718dec56f202af8a9efae74a09a082f772017ff851` |
| `cpu-clear.png` | `0a289195de58bfc9c472a282f84bc085f978f7b6ebf7768d5da149a53f05cdf3` |
| `gpu-clear.png` | `df805cc7a50ac12cefe5ff34d87eff583a1cb70edde5634324fa3b6b3a9c38c7` |
| `cpu-clear-denoise-mask.png` | `6d0185565f4faafb5b93ff624131ad58fb55cdb656d1978e8a27118af0a8dacc` |
| `gpu-clear-denoise-mask.png` | `2bfb133de5d464af8980291b2654bc2868cc259e9e1b7310b761278fccefefa9` |

## Photon-radius experiment: an explicit finite-budget tradeoff

The original sphere/receiver example completed **48 quality renders**:
64 × 64, four independent paired camera/photon seeds, 8/16/64 observations,
16,384 photons per independently refreshed map, a 1 mm initial radius, and
fixed-radius versus decreasing-radius modes on diffuse and polished receivers.
Each 64-observation run emitted 1,048,576 photons. The sphere is explicitly
triangulated; its faceting is retained. These 64 × 64 fixtures measure finite-budget
estimator behavior; they are too coarse to qualify final-resolution caustic detail.

The table reports the spatial mean of the per-pixel luminance variance across
four independent seeds, measured from raw linear EXRs. It does not use the
within-film Welford scatter as a convergence certificate.

| Receiver | Observations | Fixed-radius seed variance | Progressive-radius seed variance | Progressive / fixed |
| --- | ---: | ---: | ---: | ---: |
| Diffuse | 8 | 0.000815660 | 0.000948599 | 1.163 |
| Diffuse | 16 | 0.000418722 | 0.000508038 | 1.213 |
| Diffuse | 64 | 0.000101870 | 0.000147642 | 1.449 |
| Polished | 8 | 0.547418 | 1.100605 | 2.011 |
| Polished | 16 | 0.173798 | 0.371476 | 2.137 |
| Polished | 64 | 0.0705196 | 0.128848 | 1.827 |

Increasing the budget reduced this measured variation in both modes. Shrinking
the radius **increased finite-budget variance at every tested setting**. It is
therefore not presented as an automatic noise improvement.

At 64 observations, the four-seed mean sum of image-pixel luminance changed from
1,537.49 to 1,541.96 on the diffuse receiver and from 352.799 to 311.015 on the
polished receiver. The latter is about 11.8% lower. These are sums over the
camera image, not integrals of total physical flux. Different spatial bandwidths,
sharp caustic structure and a narrow receiver lobe can change finite-radius bias.
Without an independent reference, the brightness difference is neither proved
bias correction nor proved energy loss. Four seeds are insufficient for a broad
robustness claim. The radius option remains explicit, and the fixed-radius
comparator remains available.

Recorded times include preparation and output encoding. Shader-cache warmup,
shared-machine load and concurrent CPU/GPU work were not controlled, so those
times and the report's variance-times-seconds quantity do not establish an
equal-time efficiency result.

## CPU sampling comparison at the prior source

This completed 9 November 2026 experiment used the same frozen `84bda659`
implementation as CI05. Its receipts and measurements remain attached to that
source; they do not validate the later `23f173d3` integration, the premerge bc9
completion source or the selected main-based restack.

The completed experiment verifies the implementation's recorded sampling behavior and exposes substantial scene-dependent tradeoffs. **It does not establish a universal quality gain, equal-time superiority, unbiased efficiency, or noiseless glass.** The full quality matrix contains every one of the 28 scene/configuration groups; none is omitted from the tables below. Source: the completed experiment receipt, original quality report, and independently checked analysis [^sampling-receipt], [^sampling-quality-report], [^sampling-analysis].

### Protocol and evidence scope

The quality profile rendered four original scenes—clear glass over diffuse and polished receivers, rough glass over a diffuse receiver, and dispersive glass over a diffuse receiver—with seven configurations each. Each group contains eight independently seeded camera/photon-map replicates at 32 × 32 pixels: **224 measured renders**. Configurations use Independent or separately scrambled two-dimensional Sobol blocks, one/two/four first-Fresnel lanes, and optional eta-aware CPU roulette. This is CPU transport; it neither qualifies GPU throughput nor compares full deterministic branch splitting. [^sampling-quality-report], [^sampling-analysis]

Each quality configuration first ran two timing-only calibrations at 2 and 8 observations, using seeds disjoint from the measurement seeds: **56 calibration renders**. A linear setup-plus-per-sample timing estimate chose a fixed 2..512 observation budget toward a 4-second target. All budgets for a scene were frozen before measurement radiance was inspected. The selected budgets actually ranged from **4 to 55 observations**; no configuration reached either admitted bound. Different configurations therefore render different observation counts and different numbers of camera lanes and photon maps. Photon emission uses 2,048 paths per refreshed map, fixed radius 1 mm, and maximum camera/photon depth 16; maps refresh once per complete camera observation, not per Fresnel lane. [^sampling-quality-report], [^sampling-analysis]

The executable was built with the CI profile and run with one renderer worker, CPU affinity 0,1 and nice level 10 on the shared measurement machine. Per-render timings include setup and task output encoding; subsequent artifact writes are excluded. Quality measurements totaled 791.645167 seconds, calibrations 88.254557 seconds, and the full quality command 881.561133 seconds. This is a record of those timings, not a controlled throughput benchmark. [^sampling-receipt], [^sampling-analysis]

The timing calibration did **not** make the results equal-time. Group mean times ranged from **1.743935 to 10.305076 seconds**, and individual measured renders from **1.180418 to 13.268313 seconds**. **20/28** group means fell outside 3.2..5.0 seconds (0.8..1.25 × the target); **12/28** fell outside 0.8..1.25 × their scene's Independent mean; **26/28** had at least one seed outside the target interval. None of these mismatches arose from an admitted sample-bound clamp. Shared-machine load and an imperfect two-point timing model are possible contributors; this experiment does not isolate their contributions. [^sampling-analysis]

The analyzer verified completed command receipts, all 28-group/seed/calibration matrices, executable and report hashes, per-run records, and 1,026 input artifacts. It independently recomputed statistics from the native EXRs. Both the Rust report and analyzer decode the same exported f32 channels and then accumulate moments in f64; checks allow 2 × 10⁻¹² relative reduction differences, no absolute epsilon, and require exact agreement at zero. [^sampling-analysis], [^sampling-input-manifest]

### Findings across the full quality matrix

**Sobol alone showed a repeatability benefit at the measured budgets, with a clear timing exception.** Its luminance-variance ratios were 0.505169 for clear/diffuse, 0.544159 for clear/polished, 0.498638 for rough/diffuse, and 0.587957 for dispersive/diffuse. The corresponding variance×mean-time ratios were 0.504445, **1.022304**, 0.625240, and 0.387646. In polished glass, the lower variance was measured in a 55-observation run averaging 10.305076 seconds, versus 32 observations and 5.485259 seconds for Independent: **1.878685 × the elapsed time**. Lower variance there does not establish a better time-adjusted result. For the dispersive scene, RGB-channel variance differs from luminance variance; its Sobol RGB variance and variance×time ratios were 0.662749 and 0.436958. [^sampling-analysis]

**Independent first-Fresnel strata did not improve the measured variance×time proxy in any scene.** Two lanes produced luminance proxy ratios of 1.096475, 2.643710, 1.682734 and 1.235095 in scene order; four lanes produced 1.869264, 3.103825, 2.296424 and 2.128657. The timing calibration selected fewer complete observations for these configurations. These results concern the complete configurations and their selected budgets; they do not isolate a same-observation-count effect of stratification. They provide no basis for changing the default to two or four lanes. [^sampling-analysis]

**Combining controls was scene dependent.** Sobol plus four lanes had luminance variance×time ratios 0.641666, **4.428928**, 0.967024 and 0.788835. Adding roulette gave 0.603345, **4.021710**, **1.110854** and 0.670368. Thus the polished receiver remained markedly worse by this proxy, and the rough/diffuse combination crossed from slightly below to above baseline. Roulette alone gave 1.016762, **1.407628**, 0.940682 and 0.990414; differences close to one are descriptive and are not established improvements with only eight seeds. Different selected budgets and measured durations prevent attributing every change to one control. [^sampling-analysis]

**Mean radiance remains an unresolved qualification issue in the polished case.** Relative mean-luminance shifts versus Independent ranged from +0.996760% to +17.782922% there. Sobol/four-lane and Sobol/four-lane/roulette runs shifted by **+17.453904% and +17.782922%**. The other three scenes' shifts were much smaller: clear/diffuse −0.022621%..+0.152794%, rough/diffuse −0.062630%..+0.125749%, and dispersive/diffuse +0.010079%..+0.185221%. These are signed differences between finite-seed observations, not proof of estimator bias, evidence of unbiasedness, or an independent bias correction. Every RGB and luminance shift appears below. [^sampling-analysis]

### Complete quality matrix

All ratios are **configuration / Independent within the same scene**; lower variance or variance×time ratios mean lower measured values of that quantity. The product V×t is only a descriptive proxy. VY is the pixel-averaged sample variance of luminance across eight complete independent-seed images, using n−1 normalization. VRGB additionally averages the three per-channel variances. Luminance is measured directly, so RGB covariance is retained in VY. Obs × lanes separates complete camera observations from first-Fresnel path evaluations. [^sampling-analysis]

Timing flags: **T** = mean outside 0.8..1.25 × the 4-second target; **B** = mean outside 0.8..1.25 × Independent; **S** = at least one replicate outside the target interval. **—** means none of those timing flags. All 28 calibration-bound flags are false. Table values are rounded; the JSON preserves full precision. A zero baseline denominator would yield an unavailable ratio, including 0/0, rather than zero or infinity; none of the reported quality baseline variance denominators was zero. [^sampling-analysis]

#### clear-diffuse

| Configuration | Obs × lanes | Mean s [min, max] | Time / baseline | Time / target | VY | VRGB | V ratio Y / RGB | V×t ratio Y / RGB | Timing flags |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| independent | 41 × 1 | 5.07261 [4.60991, 5.761] | 1 | 1.26815 | 0.000224903 | 0.000224903 | 1 / 1 | 1 / 1 | T, S |
| sobol | 35 × 1 | 5.06535 [4.43401, 6.59017] | 0.998568 | 1.26634 | 0.000113614 | 0.000113614 | 0.505169 / 0.505169 | 0.504445 / 0.504445 | T, S |
| strata2 | 25 × 2 | 4.6798 [3.71078, 6.63275] | 0.922563 | 1.16995 | 0.000267299 | 0.000267299 | 1.18851 / 1.18851 | 1.09648 / 1.09648 | S |
| strata4 | 12 × 4 | 4.08765 [3.66249, 4.47259] | 0.805828 | 1.02191 | 0.000521703 | 0.000521703 | 2.31968 / 2.31968 | 1.86926 / 1.86926 | — |
| sobol-strata4 | 10 × 4 | 3.37851 [2.77364, 4.04983] | 0.66603 | 0.844628 | 0.000216676 | 0.000216676 | 0.963419 / 0.963419 | 0.641666 / 0.641666 | B, S |
| roulette | 32 × 1 | 4.02603 [3.53523, 5.12405] | 0.793679 | 1.00651 | 0.000288117 | 0.000288117 | 1.28107 / 1.28107 | 1.01676 / 1.01676 | B, S |
| sobol-strata4-roulette | 12 × 4 | 4.11326 [3.60098, 4.82311] | 0.810876 | 1.02831 | 0.000167343 | 0.000167343 | 0.744067 / 0.744067 | 0.603345 / 0.603345 | — |

Signed mean shifts are configuration minus Independent; they have no bias-correction interpretation.

| Configuration | Mean RGB | Δ mean RGB | Mean Y | Δ mean Y | Relative Δ mean Y |
| --- | --- | --- | ---: | ---: | ---: |
| independent | 0.375091, 0.375091, 0.375091 | +0, +0, +0 | 0.375091 | +0 | +0.000000% |
| sobol | 0.375373, 0.375373, 0.375373 | +0.000281881, +0.000281881, +0.000281881 | 0.375373 | +0.000281881 | +0.075150% |
| strata2 | 0.375007, 0.375007, 0.375007 | -8.48483e-05, -8.48483e-05, -8.48483e-05 | 0.375007 | -8.48483e-05 | -0.022621% |
| strata4 | 0.375515, 0.375515, 0.375515 | +0.000423634, +0.000423634, +0.000423634 | 0.375515 | +0.000423634 | +0.112942% |
| sobol-strata4 | 0.375665, 0.375665, 0.375665 | +0.000573116, +0.000573116, +0.000573116 | 0.375665 | +0.000573116 | +0.152794% |
| roulette | 0.375089, 0.375089, 0.375089 | -1.9826e-06, -1.9826e-06, -1.9826e-06 | 0.375089 | -1.9826e-06 | -0.000529% |
| sobol-strata4-roulette | 0.375607, 0.375607, 0.375607 | +0.000515339, +0.000515339, +0.000515339 | 0.375607 | +0.000515339 | +0.137390% |

#### clear-polished

| Configuration | Obs × lanes | Mean s [min, max] | Time / baseline | Time / target | VY | VRGB | V ratio Y / RGB | V×t ratio Y / RGB | Timing flags |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| independent | 32 × 1 | 5.48526 [3.21628, 7.37602] | 1 | 1.37131 | 0.54346 | 0.54346 | 1 / 1 | 1 / 1 | T, S |
| sobol | 55 × 1 | 10.3051 [6.99766, 13.2683] | 1.87869 | 2.57627 | 0.295729 | 0.295729 | 0.544159 / 0.544159 | 1.0223 / 1.0223 | T, B, S |
| strata2 | 12 × 2 | 3.79966 [2.47111, 5.18696] | 0.692705 | 0.949916 | 2.07412 | 2.07412 | 3.8165 / 3.8165 | 2.64371 / 2.64371 | B, S |
| strata4 | 8 × 4 | 4.00598 [2.32565, 6.35318] | 0.730317 | 1.00149 | 2.30969 | 2.30969 | 4.24997 / 4.24997 | 3.10383 / 3.10383 | B, S |
| sobol-strata4 | 5 × 4 | 2.54007 [1.85233, 3.65907] | 0.463072 | 0.635017 | 5.19778 | 5.19778 | 9.56423 / 9.56423 | 4.42893 / 4.42893 | T, B, S |
| roulette | 22 × 1 | 4.5792 [3.31651, 6.39541] | 0.834819 | 1.1448 | 0.916355 | 0.916355 | 1.68615 / 1.68615 | 1.40763 / 1.40763 | S |
| sobol-strata4-roulette | 6 × 4 | 3.0454 [2.02175, 4.40441] | 0.555196 | 0.761349 | 3.9367 | 3.9367 | 7.24376 / 7.24376 | 4.02171 / 4.02171 | T, B, S |

Signed mean shifts are configuration minus Independent; they have no bias-correction interpretation.

| Configuration | Mean RGB | Δ mean RGB | Mean Y | Δ mean Y | Relative Δ mean Y |
| --- | --- | --- | ---: | ---: | ---: |
| independent | 0.0930192, 0.0930192, 0.0930192 | +0, +0, +0 | 0.0930192 | +0 | +0.000000% |
| sobol | 0.0940793, 0.0940793, 0.0940793 | +0.00106008, +0.00106008, +0.00106008 | 0.0940793 | +0.00106008 | +1.139639% |
| strata2 | 0.0990458, 0.0990458, 0.0990458 | +0.00602661, +0.00602661, +0.00602661 | 0.0990458 | +0.00602661 | +6.478890% |
| strata4 | 0.0964562, 0.0964562, 0.0964562 | +0.00343698, +0.00343698, +0.00343698 | 0.0964562 | +0.00343698 | +3.694911% |
| sobol-strata4 | 0.109255, 0.109255, 0.109255 | +0.0162355, +0.0162355, +0.0162355 | 0.109255 | +0.0162355 | +17.453904% |
| roulette | 0.0939464, 0.0939464, 0.0939464 | +0.000927179, +0.000927179, +0.000927179 | 0.0939464 | +0.000927179 | +0.996760% |
| sobol-strata4-roulette | 0.109561, 0.109561, 0.109561 | +0.0165415, +0.0165415, +0.0165415 | 0.109561 | +0.0165415 | +17.782922% |

#### rough-diffuse

| Configuration | Obs × lanes | Mean s [min, max] | Time / baseline | Time / target | VY | VRGB | V ratio Y / RGB | V×t ratio Y / RGB | Timing flags |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| independent | 17 × 1 | 2.37429 [1.74687, 3.55929] | 1 | 0.593572 | 0.000805238 | 0.000805238 | 1 / 1 | 1 / 1 | T, S |
| sobol | 19 × 1 | 2.97711 [2.07451, 4.97078] | 1.2539 | 0.744277 | 0.000401522 | 0.000401522 | 0.498638 / 0.498638 | 0.62524 / 0.62524 | T, B, S |
| strata2 | 8 × 2 | 1.9214 [1.51766, 3.09098] | 0.809251 | 0.480349 | 0.00167439 | 0.00167439 | 2.07937 / 2.07937 | 1.68273 / 1.68273 | T, S |
| strata4 | 4 × 4 | 1.74393 [1.18042, 2.89528] | 0.734509 | 0.435984 | 0.00251756 | 0.00251756 | 3.12648 / 3.12648 | 2.29642 / 2.29642 | T, B, S |
| sobol-strata4 | 5 × 4 | 1.89174 [1.46652, 2.60026] | 0.796759 | 0.472934 | 0.000977314 | 0.000977314 | 1.2137 / 1.2137 | 0.967024 / 0.967024 | T, B, S |
| roulette | 21 × 1 | 2.92776 [2.34001, 4.00765] | 1.23311 | 0.73194 | 0.000614278 | 0.000614278 | 0.762853 / 0.762853 | 0.940682 / 0.940682 | T, S |
| sobol-strata4-roulette | 7 × 4 | 3.16291 [2.49689, 4.70123] | 1.33215 | 0.790728 | 0.000671471 | 0.000671471 | 0.833879 / 0.833879 | 1.11085 / 1.11085 | T, B, S |

Signed mean shifts are configuration minus Independent; they have no bias-correction interpretation.

| Configuration | Mean RGB | Δ mean RGB | Mean Y | Δ mean Y | Relative Δ mean Y |
| --- | --- | --- | ---: | ---: | ---: |
| independent | 0.376572, 0.376572, 0.376572 | +0, +0, +0 | 0.376572 | +0 | +0.000000% |
| sobol | 0.376502, 0.376502, 0.376502 | -6.98452e-05, -6.98452e-05, -6.98452e-05 | 0.376502 | -6.98452e-05 | -0.018548% |
| strata2 | 0.37654, 0.37654, 0.37654 | -3.13821e-05, -3.13821e-05, -3.13821e-05 | 0.37654 | -3.13821e-05 | -0.008334% |
| strata4 | 0.376346, 0.376346, 0.376346 | -0.000225641, -0.000225641, -0.000225641 | 0.376346 | -0.000225641 | -0.059920% |
| sobol-strata4 | 0.377045, 0.377045, 0.377045 | +0.000473535, +0.000473535, +0.000473535 | 0.377045 | +0.000473535 | +0.125749% |
| roulette | 0.376336, 0.376336, 0.376336 | -0.000235849, -0.000235849, -0.000235849 | 0.376336 | -0.000235849 | -0.062630% |
| sobol-strata4-roulette | 0.376891, 0.376891, 0.376891 | +0.000319224, +0.000319224, +0.000319224 | 0.376891 | +0.000319224 | +0.084771% |

#### dispersive-diffuse

| Configuration | Obs × lanes | Mean s [min, max] | Time / baseline | Time / target | VY | VRGB | V ratio Y / RGB | V×t ratio Y / RGB | Timing flags |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| independent | 36 × 1 | 2.80813 [1.84165, 4.16734] | 1 | 0.702034 | 0.000688802 | 0.00144313 | 1 / 1 | 1 / 1 | T, S |
| sobol | 26 × 1 | 1.85143 [1.30133, 2.86702] | 0.659311 | 0.462858 | 0.000404986 | 0.000956434 | 0.587957 / 0.662749 | 0.387646 / 0.436958 | T, B, S |
| strata2 | 22 × 2 | 2.58963 [1.72292, 4.68206] | 0.922187 | 0.647406 | 0.000922521 | 0.00206146 | 1.33931 / 1.42846 | 1.2351 / 1.31731 | T, S |
| strata4 | 14 × 4 | 3.06586 [1.93071, 5.16802] | 1.09178 | 0.766465 | 0.00134297 | 0.00314042 | 1.94972 / 2.17611 | 2.12866 / 2.37583 | T, S |
| sobol-strata4 | 12 × 4 | 2.73892 [1.7463, 4.91143] | 0.975352 | 0.68473 | 0.000557082 | 0.00112558 | 0.808769 / 0.779959 | 0.788835 / 0.760735 | T, S |
| roulette | 31 × 1 | 2.39134 [1.54221, 4.29808] | 0.851575 | 0.597834 | 0.000801104 | 0.00166198 | 1.16304 / 1.15165 | 0.990414 / 0.980716 | T, S |
| sobol-strata4-roulette | 12 × 4 | 2.32736 [1.68259, 3.31381] | 0.828792 | 0.58184 | 0.000557137 | 0.00112579 | 0.808849 / 0.780101 | 0.670368 / 0.646541 | T, S |

Signed mean shifts are configuration minus Independent; they have no bias-correction interpretation.

| Configuration | Mean RGB | Δ mean RGB | Mean Y | Δ mean Y | Relative Δ mean Y |
| --- | --- | --- | ---: | ---: | ---: |
| independent | 0.37534, 0.374911, 0.374615 | +0, +0, +0 | 0.374981 | +0 | +0.000000% |
| sobol | 0.375013, 0.375262, 0.375196 | -0.000327175, +0.00035154, +0.000581011 | 0.375204 | +0.000223813 | +0.059687% |
| strata2 | 0.375324, 0.375136, 0.37427 | -1.63011e-05, +0.000225212, -0.000345003 | 0.375113 | +0.000132697 | +0.035388% |
| strata4 | 0.376018, 0.374964, 0.374994 | +0.00067842, +5.29872e-05, +0.000378709 | 0.37519 | +0.000209471 | +0.055862% |
| sobol-strata4 | 0.375821, 0.375701, 0.37499 | +0.000481426, +0.000789824, +0.000374609 | 0.375675 | +0.00069428 | +0.185151% |
| roulette | 0.375716, 0.374868, 0.374457 | +0.000376419, -4.31458e-05, -0.000157535 | 0.375018 | +3.77947e-05 | +0.010079% |
| sobol-strata4-roulette | 0.375809, 0.375704, 0.374992 | +0.000469198, +0.000793547, +0.000377385 | 0.375675 | +0.000694544 | +0.185221% |

### Smoke: functional coverage and rare-event caution

The smoke profile completed all 28 groups and **56 renders**, using 16 × 16 pixels, four observations, two seeds and 512 photon paths per refreshed map, with no timing calibration. It is functional/repeatability coverage only. It must not supply a quality ranking. [^sampling-smoke-report], [^sampling-analysis]

The polished smoke case demonstrates why that restriction matters. Sobol alone had mean luminance **0.236812448**, versus **0.002689285** for Independent—a ratio of **88.057769**—and a luminance-variance ratio of **34,142.075**. The two Sobol replicate sums of pixel luminance were **0.220102** and **121.027872**; the second included a pixel of luminance **115.260147**, while the corresponding Independent maximum was **0.543507**. Sobol/four-lane combinations exhibited the same bright-event difference (mean ratios ≈88.059, variance ratios ≈34,142). This shows insufficient coverage of rare bright events in the two-seed sample, not a robust superiority/inferiority result for either sampler. No value was clamped to obtain these statistics. [^sampling-smoke-report], [^sampling-analysis]

### Practical conclusion and limits

The experiment supports retaining these controls as opt-in and evaluating actual elapsed time and raw image behavior scene by scene. It does not support a new universal quality preset or a claim that first-Fresnel strata, roulette, or their combinations always reduce noise. The polished case and its mean shifts remain an explicit image/reference/rare-tail qualification gap. [^sampling-analysis]

- Eight quality seeds and two smoke seeds are small empirical samples. Low or zero observed scatter cannot establish absence of rare paths.
- Replicate seed labels are paired across configurations. The analysis supplies descriptive variance ratios, not independent ratio confidence intervals.
- Quality budgets differ and actual elapsed times are frequently mismatched. Multiplying variance by time does not create an unbiased equal-time comparison or correct timing drift.
- A fixed photon radius and finite path depth remain in every configuration. There is no independent unbiased reference, no measurement of radius or truncation bias, and no proof of energy convergence.
- Pixels share photon-map error, and Fresnel lanes/Sobol observations within a run are dependent. Their counts must not be substituted for independent replicate counts.
- Image-wide averages can obscure local caustics, reflection/transmission edges and chromatic artifacts. The complete raw images and component-specific investigations remain necessary for image qualification.
- This matrix does not measure the quality effect of the new component/map-aware adaptive policy, GPU throughput, component denoising or the unimplemented rough-transmission footprint reconstruction. Those have separate contracts/evidence/gates.
- Shared-machine timings include setup and output encoding. No hardware or load-controlled causal speedup claim follows from this run.

### Provenance and immutable evidence

The experiment ran from **2026-11-09 09:24:31 UTC to 09:40:58 UTC**. Its build and both profile commands exited 0. The copied executable and complete source remained unchanged; the analyzer found the same prior-source/code identity as the completed CI05 receipts. That attestation is distinct from a passing full gate: the recorded **full CI05 exit is 1**, and the analyzer explicitly retains `full_ci_passed: false`. The sampling evidence does not relabel the complete gate; baseline-failure interpretation belongs in the separate CI validation section. [^sampling-receipt], [^sampling-analysis]

Artifact names below are relative to the external evidence bundle. The repository, frozen runners/analyzer, existing reports and render artifacts were not modified to produce this section.

#### Sampling artifact hashes

| Identity or artifact | SHA-256 / commit |
| --- | --- |
| Git HEAD before the uncommitted implementation | `dd39f36fc139939bb3da8b930b29827936d86bfa` |
| Complete source state | `05811f372ed18d3d6b95a1f0dc0734c79d22ff84c0b61615625b0a0bff02edb9` |
| Non-Markdown code identity | `2b96dc8b70cf97bf93a8f3e4ea97da494606f1eb084819f563c78673d0cb20a3` |
| Copied sampling executable | `f0353f64eea5ab2e978381037762d8c126fb7d5262f76fd06f4162a0e9121cba` |
| CI05 receipt | `dbcd23e719674260a9b80d6a3df6aab11d0bfc176011a9860068f6cfe04140b5` |
| Experiment receipt  | `82ee90601871d23303ec65b09970082de66041de0747e644735f46806e469300` |
| Quality report  | `9988f34fc56c5b3466a0428156e2e59681efc85b479501fb2a1cf65b33d09943` |
| Smoke report  | `a7ac65306248e2168b3998e58c9bfda3c55f5115f76c8cd5a028ab725b64a40a` |
| Verified analysis JSON  | `03a0cb277f1c88b44dbb6f3163cd7ad6ced5765905abc6e61589c1b910a0a926` |
| Verified analysis Markdown | `a6266e77b2c42de3839a3be1c8e767abceb45c6e26d968818cb5c13d4855833f` |
| Input manifest  | `d472ef02c7decbc3653dd9c674fddb1bb9369fc0d247526f87b5ddc1c9207611` |
| Analyzer | `dd7b5f014f36383a189e3e7988a178b6d87ef35b83f22150d0c9c6beb6fddc40` |
| Experiment runner | `a7c6468ba7f708645fa31d5b32a9b67f1261d2094179dfa5e0300008384093f0` |
| Recorded example source | `b51f5ea3501a7f003609037eb2981e585265e91cea3e84be4d748ae48afc5eb6` |

[^sampling-receipt]: External evidence artifact `sampling-comparison-final-01/receipt.json`. Its SHA-256 appears in the [sampling artifact hash table](#sampling-artifact-hashes).
[^sampling-quality-report]: External evidence artifact `sampling-comparison-final-01/sampling-quality/report.json`. Its SHA-256 appears in the [sampling artifact hash table](#sampling-artifact-hashes).
[^sampling-smoke-report]: External evidence artifact `sampling-comparison-final-01/sampling-smoke/report.json`. Its SHA-256 appears in the [sampling artifact hash table](#sampling-artifact-hashes).
[^sampling-analysis]: External evidence artifact `sampling-comparison-analysis-final-01/summary.json`. Its SHA-256 appears in the [sampling artifact hash table](#sampling-artifact-hashes).
[^sampling-input-manifest]: External evidence artifact `sampling-comparison-analysis-final-01/input-manifest.json`. Its SHA-256 appears in the [sampling artifact hash table](#sampling-artifact-hashes).

## OPUS seed diagnosis at the prior source

The completed probes now connect receiver-local photon records to the actual raw output. They also show that bright, sparsely supported events remain: changing the photon seed removes the original event at its location while creating large positive changes elsewhere. This is a measured diagnosis, not demonstrated noise removal. [^opus-seeds] [^opus-accounting]

These measurements use the frozen implementation based on `84bda659`, with the same full source/code identities as CI05. They do not validate the later integration, the premerge completion source on `bc9b0b36`, or the selected main-based restack. Export receipts identify the actual executor as **`gpu-path`** for all three initial renders and both targeted reprobes. This backend is read from the rendered-frame receipts, not inferred from CLI feature names. [^opus-attestation] [^opus-reprobes]

### Retained setup failure and completed evidence

The first attempt built successfully but refused the unchanged fixture at preflight because its CLI lacked the optional `bundled-render-library` asset pack. It started no render job and exported no images. Its failed receipt and clean CLI exit remain recorded. A new versioned wrapper enabled that existing feature, verified it in the compiler artifact, and completed the three-seed sequence. This was a build-configuration correction; the source and recorded scene setup were not changed. [^opus-refusal] [^opus-attestation]

The completed sequence used three independent photon-map replicates, labeled Map A, Map B and Map C with unchanged camera settings and the full original grid. The analyzer decoded every raw grid and recorded finite RGBA, nonnegative RGB and valid coverage throughout. Its source/binary checks passed. The governing **CI05 gate still exited 1**, with 19 failures matching its pinned parent; evidence eligibility does not turn that gate green. [^opus-seeds] [^opus-attestation]

### The original bright location, with final-camera accounting

Here `Y = 0.2126R + 0.7152G + 0.0722B` is computed from scene-linear raw RGB. Photon and other-estimator columns are the final output partition, after the complete camera prefix and actual averaging. [^opus-accounting]

| Photon-map replicate | Actual gathers | Contributing events | Photon Y | Other-estimator Y | Raw Y |
| --- | ---: | ---: | ---: | ---: | ---: |
| Map A | 3 | 2 | 4920.245699 | 0.469897 | 4920.715741 |
| Map B | 3 | 0 | 0.000000 | 0.469897 | 0.469897 |
| Map C | 3 | 0 | 0.000000 | 0.469897 | 0.469897 |

For Map A, the photon estimator supplies **99.99045%** of final raw Y. Its dominant event has receiver-local Y **3,908.84618** but final camera-weighted Y **4,919.75251**. That single event supplies **99.98042%** of the complete raw pixel. The earlier receiver-local fraction therefore cannot be substituted for a final-pixel fraction. The nonuniform RGB prefix is part of the estimator weight. [^opus-accounting]

Maps B and C still complete all three receiver queries at that location. They find no eligible contributing photons there, and the raw result equals the unchanged other-estimator value. These are explicit zero-contribution observations, not missing or unreached diagnostics. All three reported raw pixels exactly match the independently decoded EXR values. [^opus-seeds] [^opus-accounting]

### New bright locations and their completed reprobes

The full-grid comparison selected each new seed’s largest positive Y change relative to Map A. Each location was then rendered again using the same attested executable, unchanged camera setup and the same photon seed. Both fresh **selected raw pixels exactly match their initial same-seed pixels**. This statement is limited to those selected pixels; the reprobe wrapper does not assert full-image byte equality. All source/code/binary/driver/original-evidence guards passed, and both owned CLIs exited cleanly. [^opus-reprobes]

| Diagnosed observation | Dominant receiver-local Y | Dominant final-output Y | Other-estimator Y | Complete raw Y | Dominant event share of raw Y |
| --- | ---: | ---: | ---: | ---: | ---: |
| Original location, Map A | 3908.846183 | 4919.752509 | 0.469897 | 4920.715741 | 99.980425% |
| Positive-delta location, Map B | 2302.752196 | 737.501443 | 0.049984 | 737.551470 | 99.993217% |
| Positive-delta location, Map C | 190.275986 | 186.861331 | 2.465900 | 189.327240 | 98.697541% |

The Map B reprobe has four gathers and one eligible/contributing photon; Map C has three gathers and one. The same area-emitter identity supplies the dominant event in all three diagnosed observations. The dominant receivers are opaque glossy surfaces. Each dominant query has **one positive contribution event**, effective event support **1**, and reservoir visits **1**. This support statistic describes weight concentration; it is not an independent photon sample count or a confidence interval. Private emitter identities, scene coordinates and geometry are omitted. [^opus-accounting]

The surface-gather calculation is

\[
L_i^{\mathrm{local}}=\frac{\Phi_i\odot f_i\,G_i}{\pi r^2},
\qquad L_i^{\mathrm{camera}}=\beta_i\odot L_i^{\mathrm{local}}.
\]

The stored flux `Φ` already includes emitted-photon and reservoir normalization. It must not be divided by the photon count or source-selection probability a second time. The receiver response `f`, normal-measure correction `G` and support area produce the local record; the full RGB prefix `β` produces its camera-weighted value. The recorded lane, sample and spatial denominators are all one at these probes. The final-output fields therefore expose the event’s actual share directly. The normal-measure correction is near one in all three dominant cases. [^opus-accounting]

Recomputing those local and prefix products from the recorded factors differs by less than **7.5 × 10⁻⁸** and **4.9 × 10⁻⁸** relatively, respectively. Across all five selected observations, the largest relative RGB residual between raw output and photon-plus-other partition is **8.10 × 10⁻⁸**; the two zero-photon observations have zero residual. Raw addition order and individually reported event arithmetic need not have identical float32 rounding. These small measured residuals support accounting consistency, not an independent proof of the physical estimator or source calibration. [^opus-accounting]

### Full-image changes are not a convergence result

The table describes spatial statistics over each unchanged full grid. A “sum of pixel luminance” is an unweighted image sum, not a solid-angle integral or total physical energy. Spatial quantiles are not Monte Carlo confidence intervals. [^opus-seeds]

| Photon-map replicate | Sum of pixel luminance | Mean Y | Median Y | 99th percentile Y | 99.9th percentile Y | 99.99th percentile Y |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Map A | 2,055,325.136643 | 0.247796723 | 0.090403380 | 3.061040 | 9.211917 | 31.938682 |
| Map B | 2,047,826.361800 | 0.246892646 | 0.090382905 | 3.052124 | 9.201578 | 32.026696 |
| Map C | 2,046,870.709068 | 0.246777429 | 0.090393812 | 3.058676 | 9.212494 | 31.598021 |

The global maximum Y is **11,820.47759 in every seed**, even though large seed-dependent events move. Using the global maximum alone would miss those changes. The largest positive changes are **+737.50149** for Map B and **+186.86134** for Map C; their initial raw Y values are **737.55147** and **189.32724**, respectively. The sums of pixel luminance differ from Map A by **−0.36485%** and **−0.41134%**. No alpha value changes. These differences do not establish that either darker realization is a physical correction. [^opus-seeds]

Three map seeds, one camera setup and two locations selected after inspecting their deltas do not establish convergence, unbiased transport, absence of grain or a general outlier distribution. GPU atomic insertion order can also affect map ordering and roundoff. The completed reprobes support the sparse-photon/glossy-receiver diagnosis at the measured locations. They do not demonstrate a new glossy-receiver connection strategy, reference agreement, or energy-preserving removal of the events. They do not justify clamping real highlights. [^opus-seeds] [^opus-reprobes]

### Source, timing and immutable evidence

The initial build took **196.569 seconds**. Initial CLI-session times were **312.094**, **299.848** and **293.188 seconds**. Reprobe outer wall times were **327.202** and **295.700 seconds**. These include setup, export, cleanup and verification as applicable on a shared host; they are provenance timings, not controlled transport-throughput measurements. [^opus-attestation] [^opus-reprobes]

| Identity | Recorded SHA-256 or commit |
| --- | --- |
| Worktree HEAD | `dd39f36fc139939bb3da8b930b29827936d86bfa` |
| Full source-state identity | `05811f372ed18d3d6b95a1f0dc0734c79d22ff84c0b61615625b0a0bff02edb9` |
| Non-Markdown code identity | `2b96dc8b70cf97bf93a8f3e4ea97da494606f1eb084819f563c78673d0cb20a3` |
| Copied CLI executable | `28bd007c9d6921aba578e30041419c4bfbb2d990f0c03897ce2746a172525e71` |

### OPUS evidence artifacts

The following public labels replace private artifact filenames. Their hashes
continue to identify the original, unmodified external receipts and analyses.
The footnotes use the same labels; they are not filesystem paths.

| Public evidence label | SHA-256 |
| --- | --- |
| `Historical private artifact A` | `6a1fdb4ab8e869469d1d59d8d89c4d5067bbd69a87ab7011d9c5f12dc757a46e` |
| `Historical private artifact B` | `7dfe6d07bd5e20132c7e683f90ec716db588645fb24f1822f499ae8561ca3876` |
| `Historical private artifact C` | `9f5ef4e15af4b72f0d20a128460924f9acdd9c9dd0a133c85511ba75f8a09879` |
| `Historical private artifact D` | `2c67bc2d5fafb1e369c60076022aa6c762f05c0470d4279cdd80663bf080d46f` |
| `Historical private artifact E` | `76ca0e3522d2fa3829f2eb607b9c4ee3507a33870df81c8488881cc675c9dd68` |
| `Historical private artifact F` | `086d7cc363b72f43b3ba007af4c9037c7a3b9b94cd49a4878f982ecc7d696b45` |
| `Historical private artifact G` | `1aa4343bb2808629cb6227228fb33803efd85e8661d77306a2da76c1b022ed47` |
| `Historical private artifact H` | `425a546200a51cffdf300fff9d43747be080759ef070d6b5981a255d059da0bf` |
| `Historical private artifact I` | `dbb399ffde443c3a670e27a48fb79b4cac6b2d6af8426caea1c406fe09916b62` |
| `Historical private artifact J` | `0ecc2865b77b52ec2414b71fba2eb36fed93806b5758a5aad60c3400c0a701ec` |
| `Historical private artifact K` | `f1e18638d9028032869a8afda5864ce89ce92368b8269241d13ba3c123b01d57` |
| `Historical private artifact L` | `b2488dc5297b3bc31cfb65c3ccec9af6d08db7ae3d9cff9052e097d813be200f` |

[^opus-attestation]: `Historical private artifact B` binds the build, copied executable, source/code identities, actual CI05 outcome, three reports, exports and cleanup. Its hash is listed under [OPUS evidence artifacts](#opus-evidence-artifacts).
[^opus-seeds]: `Historical private artifact C` is the completed full-grid decoder/analysis receipt. Its immutable analyzer hash is listed in the same table.
[^opus-accounting]: `Historical private artifact L` retains public-safe scalar calculations and exact report hashes. It preserves receiver-local versus final-camera units and the source scope, without private coordinates, scene settings or geometry.
[^opus-reprobes]: `Historical private artifact G` and `Historical private artifact H` bind the completed fresh reports, selected EXR decoding, settings-only probe changes, same-seed selected-pixel comparison and unchanged evidence checks.
[^opus-refusal]: `Historical private artifact A` retains the failed default-feature attempt. It has no rendered image result and is not presented as a rendering-quality experiment.


## Reproduce the public experiments

Choose fresh output directories outside the checkout. The commands below retain
raw masters and exact request/options receipts:

```sh
cargo run --profile ci -p solidcraft-render --features raytrace-cpu \
  --example validate_glass_convergence -- NEW_OUTPUT_DIRECTORY quality

cargo run --profile ci -p solidcraft-gpu \
  --features solidcraft-render/raytrace-cpu --example glass_comparison -- \
  NEW_OUTPUT_DIRECTORY both 16 all 64 capture

cargo run --profile ci -p solidcraft-gpu \
  --features solidcraft-render/raytrace-cpu --example glass_comparison -- \
  NEW_OUTPUT_DIRECTORY both 32 clear 256 filter
```

For matched controls, use a different fresh output directory and replace
`capture` or `filter` with `off`, keeping backend, scene, size and sample count
identical. `filter` saves the categorical guide mask as well as its counts.
Compare raw radiance within a backend; CPU/GPU bit-identical pictures are not a
requirement. The private OPUS fixture and its reproduction settings are not
published with these public-safe examples.


The completion profiles use the same public example with fresh directories:

```sh
cargo run --profile ci -p solidcraft-render --features raytrace-cpu \
  --example validate_glass_convergence -- NEW_RECEIVER_DIRECTORY receiver-quality

cargo run --profile ci -p solidcraft-render --features raytrace-cpu \
  --example validate_glass_convergence -- NEW_SPLIT_DIRECTORY split-quality
```

Use `receiver-smoke` or `split-smoke` for bounded initial checks. Receiver
quality contains six scene cases, 16 configured groups and 128 seeded renders;
its 48 exact pairs cover positive continuation/reference comparisons and
diffuse/hard negative controls. Split quality contains four cases, six
configurations each and eight seeds, or 192 renders. Its 32 requested lanes per
pixel become 32, 16 or 8 complete observations for one, two or four lanes.
These matrices and the runtime/mean comparisons must be retained even when a
configuration performs worse.
