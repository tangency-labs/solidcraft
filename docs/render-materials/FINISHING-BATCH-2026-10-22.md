# Render finishing and full-master acceptance batch - 2026-10-22

## Owner scope

Continue the personal renderer plan with grouped implementation and bounded validation: close the actual full-master GUI acceptance, connect operational background/stage resources, add useful native finishing and real linear-output support. Save recovery checkpoints to `0xlillith/solidcraft`, branch `lab/render-scale-resources`, draft PR #7. Do not update upstream, main or releases. Reuse the shared Nateaux/Nathan-Dane UI resources rather than a parallel widget/icon set.

## Previously outstanding GUI acceptance - completed

The application freshly built from `f8e0b07b17dc0cb4851549ae5247b063e6b648a4`, tree `3410b37817d2d337eb840f1f6d9fecfb48474d67`, was exercised on Pegasus. The build passed with its source unchanged. Executable SHA-256: `97a7edea5d3a64cd69902991e3cc39ddcfa72520bfb7875939c09b217a4be58f`.

The unchanged latest functional SELENE opened with zero feature errors and showed all 2,094 originally visible bodies / 2,144,099 triangles in the native Render preview. Actual light/docked and dark/floating screenshots were inspected. The normal `app.quit` request completed with **exit code 0**, without forced process termination. Both native model copies retained SHA-256 `3bd2bfa1c6336394c4a4208d30b52c80d06658a3bd7ea57cc93b774411d1bd68`; personal preference hashes were unchanged. The underlying grouped document suite also completed: **301 passed, 0 failed, 1 explicitly ignored**.

The isolated native review and receipts are outside the repository at `D:/codex-tools/render-finishing-20261022/full-master-review`. This evidence closes the previous preview-allocation / recursive-destruction issue for that precise application, not every renderer capability.

## Coordinated implementation

- Original cancellable single-part uncompressed FLOAT32 OpenEXR writer from public format documentation. Raw scene-linear sRGB/D65 coverage-premultiplied values are retained before all display operations, not reconstructed from PNG. GPU-only opt-in, separately admitted memory, bounded worker encoding/hashing, immutable completed-frame ownership and exact atomic export.
- Neutral-by-default creative RGB gain, contrast, saturation and vignette controls, with explicit numerical bounds and unchanged neutral display output. These first controls require a fresh render; they do not mutate an old completed frame.
- Camera-only linear-light gradients with vertical, horizontal and radial modes, independent of the lighting environment. Gradient/transparent output conflicts fail explicitly.
- Real finite render-only floor/wall and 24-strip polygonal cyclorama, with reviewed revision, explicit replacement consent, subject clearance, one undo, saved transforms and no engineering-geometry or material changes.
- Eight native authored HDR/ground compositions, adding dark-metal, warm-DIY, spacecraft-clean-bay and woodshop variants. Conceptual `SCENE-*` and broader materials/capabilities retain their actual readiness labels.
- Shared-kit Output/Environment/Scene editors and immutable PNG/EXR export selection. A completed PNG without a retained raw master cannot be exported as EXR.
- Regression sources cover numerical identity, chunk invariance, raw-value preservation above display white, actual GPU output, same-frame export, reservation lifetime/cancellation, document reopen/undo and UI state.

See `docs/render-finishing-linear-output.md` for the exact pipeline, public specification sources, boundaries and command contract. The optional official OpenEXR Python reader is installed only in an isolated external validation environment; no production dependency or Cargo lockfile is changed.

## Current gate state at the first recovery checkpoint

The grouped whole-workspace/all-target type check passed. Formatting and whitespace checks passed. Full grouped test execution, strict lint, independent reference-reader decoding and native acceptance of the new controls are **pending**. The previous full-master GUI check is separate baseline evidence and must not be mislabelled as acceptance of this new code. A WIP checkpoint preserves work without claiming complete CI or release readiness.

## Explicit remaining units

Adaptive sample scheduling, additional transmission-path convergence work, compressed/multipart/AOV EXR, bloom, volumetric effects, post-hoc interactive regrading and remaining advanced scene/material implementations are not made operational by this batch. They stay visible as separate planned work. Further acceptance must exercise the actual producer rather than merely expose a control or upgrade a catalog readiness flag.


## Grouped execution checkpoint

On frozen `9db56e70` / tree `f7292256c2392f09c7ca61b88daa2e82ace01c94`, strict workspace/all-target Clippy and the grouped test build passed. Execution: **462 renderer tests; 301 document tests (one explicit ignore); 68 Render engine tests; 81 Render UI tests; 16 shared-kit guards** all passed. The GPU suite passed 98 tests and exposed one new test synchronization error, with three existing explicit ignores.

The failing assertion queried the reservation immediately after a queued cancellation notification; the native worker still owned that queued source until it drained it. The follow-up test waits for actual bounded resource release while still requiring zero retained bytes and no published frame. It changes neither production cancellation nor memory accounting. The raw-output fixture is also strengthened to use the public uniform-environment contract rather than the internal numerical furnace hook, so primary misses must decode as transparent black.

The initial raw GPU output decoded independently with the official OpenEXR 3.5.2 reader as FLOAT32 RGBA, with correct dimensions, scanline order and D65 chromaticities and radiance above one. Tests prove PNG-only versus retained-master PNG identity and raw-master byte identity across denoise/exposure/tone/filter/backdrop changes. The public-lighting version and corrected cancellation assertion are pending the focused re-execution recorded next. Actual engine PNG/EXR atomic export, wrong-suffix/session rejection and existing-file preservation passed already. Core production Rust remains unchanged by these test corrections.
