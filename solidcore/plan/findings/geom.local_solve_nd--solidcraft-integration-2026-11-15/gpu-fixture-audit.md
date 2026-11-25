# GPU blur fixture timing audit

The default-setting test fails in the PR integration in both the full gate and
two isolated executions. The same exact test on clean be6 is **FAIL with the same default-setting cache assertion**.
The local full CI retains exit 1; none of the diagnostic controls replaces it.

## Observations

| Run | Result | Evidence |
| --- | --- | --- |
| First full app phase, same UI source | PASS, 9.687 s | `../root-ci.log:15441` |
| Final paired full app gate | FAIL, `resting source`, 1 vs 2 | `root-ci.log:18888–18907` |
| Unchanged current binary, one exact test, evidence enabled | FAIL at gpu.rs:1406, 1 vs 2 | `gpu-flake-audit/run-1.log` |
| Same binary/defaults, evidence I/O removed | FAIL at gpu.rs:1415, 2 vs 1 | `gpu-flake-audit/run-2.log` |
| Same binary, only existing display-refinement switch set to 0 | PASS, all test assertions, 6.27 s | `gpu-flake-audit/run-3-refinement-off.log` |
| Clean be6, unmodified source, normal refinement setting | FAIL | `gpu-flake-audit/baseline-run.log` |

The two early-exit current failures take about 0.71 s. Their durations cannot be
compared with a successful execution that reaches all formats, themes and radii.
No GPU skip was permitted: all controls set `SOLIDCRAFT_REQUIRE_GPU=1` and show
one test actually executing. Current isolated runs started at low host load.

## Source-level mechanism and confidence

**Inference, strongly supported by the control:** the fixture assumes a stationary
camera means its asynchronously refined scene is already settled. It warms a fixed
number of frames, then requires that the blur source never be redrawn on an idle
or tint-only frame. Those are different conditions.

- `crates/ui-egui/src/gpu.rs:1128–1151`: `BlurFixture::frame` always supplies
  `egui::RawInput.time = Some(1.0)` but calls the real app logic/UI.
- `crates/ui-egui/src/viewport.rs:1032–1086`: the viewport waits 200 ms before
  choosing/swapping display refinements, using `crate::now_ms()` rather than egui's
  supplied time, and then collects worker results.
- `crates/ui-egui/src/lib.rs:4102`: `now_ms` reads wall-clock SystemTime.
- `crates/ui-egui/src/viewport.rs:718–729`: `scene_key` hashes `lod_generation`
  and mesh-LOD generation. A refinement can therefore change a scene key without
  any camera input or model edit.
- `crates/viewport-gpu/src/view.rs:3947` and
  `crates/ui-kit/src/blur_native.rs:611–624`: the blur key includes the viewport
  frame key, scene keys and placement.
- `crates/ui-kit/src/blur_native.rs:565–593`: a changed key causes a new source
  pass. The existing compositor marks the pending key valid on submission; the
  fixture waits for the submitted GPU work before the next frame.

The optional refinement pipeline is enabled by default and controlled by the
existing process-local `SOLIDCRAFT_DISPLAY_REFINE` switch in `display_lod.rs:43`.
Disabling it makes every assertion in this exact fixture pass. The failing
assertion also moves when capture I/O is removed, consistent with elapsed-time
and worker-completion sensitivity. This is evidence for a fixture settling race,
not evidence that a source cache should ignore a changed scene.

No invalidating frame-key field was directly instrumented. The precise worker
completion/frame causing each miss remains an inference rather than a captured
runtime trace. No driver loss, allocation refusal, pixel-comparison error or
missing adapter was observed.

## Baseline and scope

The clean snapshot is be6ecb06420d501a8362d6de3d0de7ef324dafb9. Its Cargo.lock,
kernel configuration, N-D solver, WR383 range refinement and GPU test source were
checked byte-for-byte against that immutable commit before the focused build.
The seven UI/cache files listed in `gpu-flake-audit/source-check.json` are identical in the PR,
that baseline and current main c71236a40de9f36713194df624663a252d0f71a3.

Only the UI library test target was built on baseline, and only this one test
was executed. The build used Rust 1.99.1,
profile ci, nice priority, two jobs, sccache, offline/locked dependencies and an
isolated pre-existing baseline target. Full commands, environment, build PID,
times and verified source hashes are in `gpu-flake-audit/baseline-run.json`; existing-binary
commands and raw failures are in the run JSON/log pairs. No production source,
assertion, known/quarantine list, index or ref was changed.

The refinement-off run is **diagnostic only**, not a proposed gate setting,
workaround or passing default gate. A focused follow-up should make the fixture
wait for or explicitly control refinement readiness before asserting resting
source reuse, preserving the cache/pixel assertions. Arbitrary additional sleeps
or accepting a new source pass would not establish that condition.

The independently reproduced default-setting failure is tracked in [#17280](https://github.com/tangency-labs/solidcraft/issues/17280).
Private run paths above identify unbundled validation logs; they are not distributed artifacts.
