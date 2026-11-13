# Render ribbon, native acceptance and display bloom - 2026-10-23

## Scope and authority

Owner-directed continuation on the personal `lab/render-scale-resources` branch, draft PR #7. The source base is `071f5f873bc9d7e98342faf029ad2029c1b98e54`. Upstream/main/releases remain untouched. The owner requested a coordinated implementation batch followed by grouped compilation, not a full rebuild after each small edit.

The ribbon follows Nathan Dane/Nateaux's implemented shared shell and the task-order plan in `docs/ribbon-layout.md`, `docs/ui-kit.md`, `docs/design/ux-redesign/README.md`, `docs/render-materials/UI_INTEGRATION.md` and upstream tracker #2016. Source inspection does not update the frozen integration base or claim completion of that author's separate remaining File-view/layout-preference work.

## Implemented source changes, awaiting grouped execution

- One Render tab with task-ordered **Scene, Appearance, Lighting, Environment, Camera and Image** groups. Two promoted commands per group, existing SVG artwork, shared section menus, contextual icon accents, command search, pin/customization data and responsive fitting. The Image group receives a bounded reserved area; extra personal pins still use the existing trimming/menu policy rather than overflowing the screen or being deleted.
- Interactive adapters open the implemented inspector/library for parameterized operations, preserving exact unsaved numeric intent. Nothing is silently applied merely by selecting a library or opening a tool. Direct engine JSON commands retain their existing semantics.
- Check, Render, Cancel and Export share one action implementation between ribbon, menu/search and sticky footer. The current edited request, active job and exact selected completed result remain authoritative. Disabled reasons reflect the current host/request/result; the engine API is not rewritten around GUI state. Entering Render from Design does not silently launch an expensive default render.
- A registered `render.camera.fit` operation frames the current visible subject using the shared camera contract. It preserves orientation and engineering geometry and changes only view state. The generic Design `view.apply` command is not repurposed.
- Corrected stale output help that still said EXR was unimplemented. Seven existing Render catalogs receive matching labels and refusal reasons; other supported locales retain the established English fallback.
- Optional **display bloom** with explicit strength, relative linear highlight threshold and output-pixel radius. Three normalized separable box passes spread positive highlights with full-kernel normalization and zero outside the image. This is an authored display effect, not measured lens scattering, volumetrics or a transmission-convergence improvement.
- Bloom advances through bounded cancellable row/column groups, uses fallible allocation, and admits two RGB FLOAT32 scratch images (24 bytes/pixel) before rendering. The GPU raw accumulation and EXR are never modified. Opaque output is required; transparent PNG is explicitly refused rather than flattened. Registered executors must advertise bloom support separately from their older finishing contract.

## Presentation order and limits

Raw scene-linear output is retained before denoising and every presentation operation. When enabled, bloom extracts positive highlights from the post-denoise, coverage-premultiplied linear image, convolves them without changing that image, then adds a tone-compressed glow in linear display light after ordinary finishing/background composition and before sRGB encoding. Exposure and creative RGB gains affect the glow. Contrast/saturation/vignette apply to the ordinary presentation; the bloom overlay is the subsequent explicitly creative layer. A zero-strength request has zero scratch cost and unchanged PNG bytes.

Bloom strength is 0..2, threshold 0..1,000,000 relative radiance, and radius 1..64 output pixels. The filter does not claim resolution-independent physical units, energy conservation after artistic threshold/overlay/tone mapping, noise reduction or new transport. The convolution itself has independent direct-kernel, energy, hue, symmetry, chunk-size, invalid-input and cancellation tests.

## Native finishing baseline acceptance

The preceding fresh `071f5f87` application is being exercised separately in an isolated profile, with no changes to personal preferences. The actual GUI command pipeline has produced PNG and FLOAT32 EXR outputs at 640x400. A changed presentation and its neutral control produce different PNG bytes and identical raw EXR SHA-256 `46cea7fd6091ab16de524a99ae614bb9b7d4b63d1e1c3e8bfb4484eaadc89207`. This is real native execution, not a conversion of PNG or a substitute for new ribbon/bloom acceptance.

Provisional captures, receipts and external validation tools are under `D:/codex-tools/render-ribbon-20261023`. The new batch still requires grouped strict lint/tests, a fresh application and actual light/dark, docked/floating and narrow-window inspection. No green aggregate CI or release is claimed by this recovery checkpoint.

## Still separate

Adaptive sample allocation, further transmission-estimator convergence, post-hoc regrading of an old master, volumetrics, arbitrary AOVs/compressed EXR, and remaining advanced material/scene capabilities stay separate roadmap units. Existing unsupported resources retain their availability labels. New ribbon groups and display bloom do not upgrade those concepts to executable capabilities.


## Repeated execution exposed a native atomic-publication edge

On the frozen first checkpoint the renderer (465), GPU (100, plus three existing ignores), and Render UI (85) suites passed. The test orchestrator initially selected a nonexistent `toolbar::tests::` group; it refused to count zero executed tests as success. After correcting the filter without recompilation, a repeated UI suite observed native Windows access-denied (error 5) while atomically replacing an already created PNG. Twelve isolated repetitions passed, so the external holder/cause of that intermittent denial is **not identified**.

The existing single-attempt rename is now hardened with an original bounded Windows-only retry around the already-synced rename operation. Only native errors 5/32 are eligible. There are at most eight attempts with requested sleeps totalling 127 ms; all other errors and other platforms keep a single attempt. No target is truncated/deleted, no permission/read-only flag is relaxed, no handle is force-closed, and no security software is disabled. Persistent errors remain errors; the last native code is preserved. The request path scope and explicit overwrite consent are unchanged. Backup publication uses the same narrow rename rule.

Deterministic tests cover exact attempt/wait bounds, permanent/unlisted errors and success. Windows-native tests deliberately open a reader without delete sharing: the original single rename is verified to fail, the old complete bytes stay intact, releasing that reader permits one atomic replacement, and a permanently retained reader still produces an error and leaves the old file untouched. This addresses a reproducible sharing case without claiming that every access-denied event has the same cause.

Only public platform semantics informed this change (no external implementation code):
- https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew
- https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw
- https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes--0-499-

The Camera group also exposes its implemented numerical inspector directly through the registered, read-only `render.camera.get` command, alongside Frame and the saved-camera menu action. Direct engine inspection returns an explicit current camera or null rather than inventing a saved view.


## Wider guards found inherited locale gaps and a real camera-field loss

The next frozen-source run passed 465 renderer, 85 Render UI, 27 shared ribbon, two pin-state, 16 search, nine atomic-write and five file-scope tests; the remaining document/GPU and shared-kit checks also passed. The intentionally broader localization guard exposed three existing completeness issues: 166 missing Japanese and 253 missing Simplified Chinese Render messages, a missing English registry label for physical backgrounds, and a duplicate English `Center X` label selecting an untranslated UK key. The complete-catalog contract is retained: the missing Japanese/Chinese messages receive explicit original translations rather than English-filled placeholders or a downgraded completeness test. The physical-background registry label is catalogued, UK center labels are explicit, and the contradictory EXR diagnostic is corrected.

The new orientation test also caught a real earlier bug: explicit `view.apply` parsed a live Camera through the legacy NamedView shape and silently discarded `roll` and `up`. The explicit-camera path now validates and preserves those two optional fields, with unchanged zero-roll/Z-up defaults when omitted. Named-view document storage remains unchanged. Engine tests cover both world-up conventions, malformed-field atomic refusal and legacy defaults; the Render ribbon has an exact orientation-preservation regression. This is a production correction, not relaxation of the failed test.

All source changes in this correction are grouped before the next compilation. Earlier failing receipts remain available and are not relabelled green.


## Final grouped code gates and independent feature-profile CI correction

The frozen `106f5d57046ca8854a0dad2671730a9e68aa94d4` / tree `eb9ca44845902300b7c2f038c74d4de6bc481216` grouped run completed **PASS** with source unchanged: 465 renderer tests; 86 Render UI; 27 shared ribbon; two pin-state; 16 command-search; 29 localization; one interactive menu-pin; 69 Render-engine; 17 browser/camera; nine atomic-file; five file-scope; 16 shared-kit guard; 71 shared-widget; 301 document (one explicit existing ignore); and 100 hardware GPU (three explicit existing ignores). A broader workspace substring additionally passed 78 cases; it overlaps other scopes and is not added to a synthetic total. The native application/CLI build and official 12-crate WASM check also passed. Strict workspace/all-target Clippy passed before the run.

Application SHA-256: `e09ea35eb164355782c34335129f4be057ecd1a669b22a047246b0d341814211`. CLI SHA-256: `b1d81d4652d0f0d1dae1b3efcf03c67cefb0722a56f63e1c781b24e36546333e`. Real-window acceptance uses this exact immutable build; later test-only/CI/documentation changes are not retroactively attributed to the executable.

Separately, GitHub resource gate `39871253915` failed in both Windows and Linux because the earlier large-primary-film test imported `film::primary` without its `raytrace-cpu` feature guard. Workspace feature unification hid this default-profile compilation error locally. The CPU-only test now has the same feature condition as the production module; it is not deleted or disabled when CPU ray tracing is enabled. The resource gate explicitly runs strict lint and unit tests for **both** default and optional-CPU configurations, with a Python regression requiring that distinction. This is a test/configuration correction with no production-renderer behavior change; the actual default-profile result must be verified separately.


The separately executed default-feature lint then exposed an inherited test-helper ordering lint (`CpuExecutor` followed the last enabled inline test module only in the no-CPU configuration). That test-only helper is moved ahead of test modules without suppressing the lint or changing runtime code. Both feature profiles are rechecked; a first failure is not a passing matrix result.
