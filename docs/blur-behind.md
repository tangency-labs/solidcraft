# Blur behind panels (#9809, #9810)

Implementation note for PRs 5 and 6 of the #8876 series. Native rendering,
theme-declared controls, runtime availability and a transient quality guard are
present. Built-in themes/defaults are unchanged. No build, test, GPU run,
screenshot, benchmark or pixel comparison has been run by this worker.
The 22 ms/60-sample threshold remains a starting policy, not a measured tuning result.

## Theme controls and runtime intent

Only a theme declaring `blur` offers its registered 0..40 physical-pixel range.
The slider edits in steps of one; finite fractional persisted/API values remain
valid and merely opening the control does not round them. Zero has a muted hollow
thumb, Off readout and flat 2 px cost line. Positive values use accent fill/thumb,
a px readout, and five bars of height 4/6/8/11/14 px: `ceil(radius / 8)` are filled.
The tooltip describes relative cost; it is not a timing or resource estimate.
The readout, meter and any disabled reason reserve space beside the slider.
An inactive parent prevents activation without changing the stored child value.

The app resolves availability once per UI/API composition and before positive
runtime activation. It uses the actual installed adapter's device type (cached
at installation, `Cpu` means `gpu-software`) and PR5's lazy cached renderer
capabilities, rather than renderer-name matching. Both current browser paths
are unavailable because they lack the native compositor integration. Unsupported
source/device paths and device/resource failures also fall back to ordinary painting.
There are no effect resources or markers on unavailable paths or at zero.
Opening Preferences alone does not activate a declared zero-radius effect.

Saved/raw intent is separate from runtime availability. Clearing an OS/device
restriction restores it automatically when the renderer can support it. Suspending
for a restriction never toggles the radius through zero to clear a latched resource
failure. Actual radius/host/size/background changes retain PR5's explicit recovery
boundaries; no polling loop retries a failed effect allocation every frame.

Authored declaration/ancestry and effective radius are resolved only on
`prepare_tabs`' existing theme/raw-option cache miss. Its borrowed content comparison
also catches followed reloads with unchanged id/base. Steady built-in, undeclared,
and offered-zero frames read a small copyable intent/revision: they do not order
option vectors, normalize raw maps, serialize/hash strings, or clone theme ids.
Runtime OS/device availability stays outside this cache and palette tokens.
The cache test counts normalization calls; it is not a global allocator measurement.

## Accessibility signals

The desktop injects a cached `Services::reduce_transparency` reading. Observation
starts lazily when a theme offers blur, reads on a background thread every five
seconds while needed, and repaints only when the cached signal changes. No UI-thread
registry/Objective-C IO, external processes, OS-setting writes, notification
registration or new unsafe code is used. The first unavailable/failed reading is
unknown, leaving other availability checks in charge; an idle thread stops reading
when the theme no longer offers blur.

- Windows: safe `winreg` reads HKCU
  `Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\EnableTransparency`.
  `0` disables blur (system transparency off), `1` permits it; missing/unexpected
  values are unknown.
- macOS: safe objc2 `NSWorkspace::sharedWorkspace()` /
  `accessibilityDisplayShouldReduceTransparency()`, with `NSWorkspace` and
  `NSAccessibility` features and an autorelease pool on the observer thread.
- Other operating systems currently have no signal here. Unknown does not disable
  blur. Browser rendering is independently unavailable as described above.

Platform readings and propagation latency still require manager validation on
their respective systems. Evidence fixtures never modify the host OS settings.

## Automatic quality guard

The guard uses elapsed **native queue-submission intervals**, not `app.frame_ms`
(UI construction time) and not GPU timestamps. The pass owner records evidence
only after an actual composition/presentation with a visible blur marker has
been submitted. Sampling consumes each egui frame at most once and requires the
immediately preceding painted frame, continued repaint activity on both sides,
positive radius, runtime availability, and a viewport not reported minimized or
occluded. Missing/logic-only paints, idle waits, unavailability, zero, or a changed
theme revision/radius reset the streak. egui layout re-passes cannot add samples.
Monitoring requests no continuous repaint.

After **60 consecutive eligible intervals strictly over 22 ms**, lower the effective
pass tier by one, down to two down/up steps. The natural tier remains determined
by radius; the effective tier is `min(natural tier, session cap)`. Radius is never
reduced or switched off. Recompute the normalized Kawase offset for that step
count, include the cap in the source cache signature, and reset the streak.

One inline notice is offered per session. Dismiss hides it; further eligible
reductions can continue to the minimum without reannouncing it. **Undo restores
the cap from before the first guard reduction and opts out of all further automatic
reductions for this session**, including across theme changes. This is transient
UI state, not a preference, design edit or document Undo entry.

Submission intervals include CPU/event-loop scheduling, presentation pacing and
other rendering work; they do not isolate blur or GPU execution. A low-refresh or
loaded system can exceed the starting budget for unrelated reasons. A cached blur
image still counts when visibly composited. Visibility depends on host reporting;
unsupported occlusion reporting cannot be inferred. Deterministic tests exercise
threshold/eligibility behavior; hardware measurements and threshold tuning are
still manager work. No measured GPU timing claim is made.

## Theme notices

Invalid saved entries, unsupported newer requirements and guard reduction share
a compact inline notice inside the existing Preferences host, before the empty-row
return. Dismissal is session-only, bounded to 64 problem identities and keyed by
theme, problem kind and raw value/requirement identity. A changed problem reappears.
Dismiss never changes invalid raw JSON or the API's notices; defaults remain in
effect. Newer-requirements imports discard unsupported definitions, so the warning
instructs users to upgrade and **reimport the original theme**, not to expect discarded
tables to recover automatically. No controls/no notices produces no header or spacer.

## Accepted CPU budget

The human decision is: **"Allow measured CPU checks; keep the change small."**
This replaces the spike's proposed static policy propagation and its unresolved
single-check requirement. The implementation has cheap per-host checks, a root
UI scope, and outer GPU preparation/composition/submission checks. It does **not**
claim one check per frame, two total dispatches, or measured zero CPU overhead.
There are no blur branches in `egui_wgpu::Renderer::render`'s primitive loop.
The initial source integration is for the root native viewport. The pass owner
supplies its viewport id before preparation; other native viewports use plain
painting and `capability_for` reports their missing source integration. They
cannot sample the root window's source or retain its pending submission key.

At radius zero, `Container` uses the original frame shape/painting path; no
marker, Arc/Box for a blur callback, metadata vector, blur pipeline, texture,
uniform, binding or extra pass is created. The plain Window/Panel hook delegates
to the original `Frame::show` path, including its existing content boxing. A
previously enabled effect is detached and its CPU/GPU storage dropped on zero;
wgpu retains any in-flight references until their submitted work completes.
Existing viewport/MSAA/AO/OIT resources are baseline resources, not blur resources.
An explicit harness tint override intentionally changes ordinary panel fills even
at zero; leave `surface_alpha` null for comparisons against main.

## Actual frame path and insertion points

The pinned dependencies are egui/egui-wgpu/eframe 0.36.2 and wgpu 30.0.1.
eframe and egui remain registry dependencies; only egui-wgpu has local compositor
changes for blur. The egui dependency is the published 0.36.2 crate.

| Stage | Actual path |
| --- | --- |
| Build UI | Native `eframe::App::ui` calls `SolveApp::ui` (`crates/ui-egui/src/lib.rs`). `blur::State::begin` installs a stack-scoped thread-local host policy. `cad_ui` draws the workspace under floating cards, or in the remaining central rectangle for docked layout. |
| View background | `viewport.rs::show` emits an egui gradient mesh before the viewport callback. `render_workspace.rs::show` emits its effective preview background, shared with the blur source. Neither is contained in the GPU renderer's scene texture. |
| Prepare scene | `gpu.rs::ViewportCallback::prepare` calls `solidcraft_gpu::view::Renderer::prepare`. Native `set_wgpu_split` already draws scene depth/MSAA/OIT offscreen. The UI target has one sample and no depth. The resolved scene is transparent outside geometry. |
| Acquire/order | `vendor/egui-wgpu/src/winit.rs::Painter::paint_and_update_textures` acquires/probes the drawable before preparing callbacks. `Renderer::update_buffers` uploads meshes, runs all callback `prepare`s, then all `finish_prepare`s. The enabled frame hook supplies actual physical target extent and the effective scale, including surface clamping. |
| Build source | The enabled `blur_native.rs::Source` replaces the viewport callback. Its `prepare` delegates unchanged to the original callback. Its `finish_prepare` retains the independent background/prepared viewport source and availability/cache evidence. A source cache hit avoids rebuilding that source. |
| UI composition | The native pass owner initializes C from the cached source and segments the original ordered UI stream at semantic backing markers. Before each marker it runs the existing blur pyramid from current C, then the marker samples B. Ordinary fill, border and content follow. The replaced source callback is skipped during composition. |
| Present/capture | A final triangle presents C into the normal drawable or existing capture texture. Existing screenshot copying, queue guard, texture frees and presentation remain after that. `submitted` commits a pending source key only after queue submission. |

`CallbackTrait::paint` receives an already active pass, not its encoder or target;
it cannot redirect the UI or start blur passes. Callback command buffers execute
before the main encoder. The pass owner ends each UI segment before encoding its dependent blur pyramid
and starts the next segment on the same encoder. No additional callback command
buffers are returned.

## Ordered backdrop and color contract

S, B and C retain separate resources and jobs. S is the independent cached
viewport/background source used for initialization, availability and evidence.
It includes prepared model/sketch/highlight/preview geometry, section caps,
reference images and point clouds. Source construction uses the actual prepared
renderer, background colors, placement and inherited clip.

C is the current ordered UI composition. It starts from the source, then receives
ordinary egui shapes in their original order. Before each actual translucent
backing marker, the existing down/up pyramid filters current C into B. The marker
samples that result before its ordinary surface fill and contents are painted.
Thus menus, dialogs and nested cards include preceding UI in their backdrop;
they do not replace underlying panels with the model scene. B is refreshed at
each boundary rather than shared unchanged by all panels. The final presentation
samples C. Read and write targets remain separate throughout the pyramid.

This uses the existing effect resources and adds **2 × steps GPU passes per
actual backing boundary**. Source-cache hits still save source construction;
they do not eliminate the per-boundary backdrop filtering. No extra resources
or disabled-path GPU passes are introduced. Outside the model rectangle the
source remains the deterministic background; geometry is not invented under docks.

Render preview uses its original product `Frame`, lighting, resolution reduction
and effective preview background, including authored-scene precedence and the
animated tab gradient. It does not call a model snapshot renderer or
replace the Render scene settings. Home/result-image contexts are unavailable;
a missing source callback in a painted frame drops target storage and reports
unavailable instead of sampling a previous workspace's pixels.

Source color is opaque viewport/background color. Translucent custom viewport
or outside backgrounds report unavailable before effect GPU allocation; their
composition with a transparent native window is not silently approximated.
For an unorm UI target, the
first down pass decodes each composition texel before bilinear interpolation; for an
sRGB target, texture sampling performs the decode. All subsequent filtering is
linear. Marker output follows egui's gamma-premultiplied convention: convert B
to gamma, multiply RGB/alpha by rounded coverage and host opacity, and for an
sRGB attachment decode that premultiplied RGB before framebuffer conversion.
This matches egui's existing shader ordering rather than treating its output as
straight-alpha linear color. Presentation samples C in its own format; it does
not apply a second manual gamma transform. The harness exercises both unorm and
sRGB formats. The worker has not yet verified those images on hardware.

## Semantic hosts, clipping and rounding

Blur eligibility follows actual backing UI surfaces rather than a workspace-surface
whitelist. Panels, ribbon and application bars, History, system windows, modal
dialogs and their dimming backdrops, menus/submenus, popovers, canvas editors,
notifications, file-loading shields, and tooltips use the same marker policy.
Nested backing cards participate too. Zero-alpha layout frames stay transparent;
text, strokes, selection/hover tints and model geometry do not create backing markers.
The effective fill alpha includes inherited painter opacity, so a normally opaque
surface fading in can participate without treating its text as another surface.

The app uses stock egui 0.36.2 public host APIs. `show_frame`, `show_backing`,
`show_edge`, `show_central` and bounded Window reserve a slot before the original
frame paints. Native popup and tooltip wrappers keep their placement, hover gates,
stacking, close behavior and response types. Response tooltip extension methods
route native enabled/disabled and pointer-anchored variants through the same policy.
Manual backing surfaces replace their original reserved slot, preserving ordering.
The off path calls the original hosts without marker slots or effect allocations.
`show_backing` adds no inset decoration to callers which previously had none.

Modal rendering first reserves full-screen backdrop blur, then retains native
input shielding/focus/dismissal and dimming. Its visible dialog frame is painted
through `show_backing` after dimming, so the dialog samples the actual dimmed UI
beneath it. The off path remains the original native Modal. Existing host geometry,
interaction and legacy paint assertions require manager validation after this
iteration; no automated checks were run by these implementation workers.

Markers retain all four final corner radii, host clipping and painter opacity.
Clipping does not invent rounded corners, and moved surfaces sample corresponding
screen pixels. The compositor respects ordered preceding UI, so a translucent
popup above a panel blurs the panel beneath it rather than cutting through to the
model. No private storage access, global matching-color search or arbitrary
rectangle heuristic selects backing surfaces.

## Radius, cache, allocation and recovery

Radius is finite physical pixels in 0..40; invalid requests are refused before
mutation. Zero selects plain rendering. Positive tiers have N down and N up passes:

| Radius | N | First down target |
| --- | --- | --- |
| >0..8 | 2 | ceil-half resolution |
| >8..18 | 3 | ceil-half resolution |
| >18..30 | 4 | ceil-half resolution |
| >30..40 | 5 | ceil-half resolution |

Every subsequent dimension is ceil-halved with a minimum of one pixel. Upsampling
ends at full resolution B. The original kernel has a center-plus-four-corners
down filter (weights 4,1,1,1,1) and an eight-tap up filter (axis weight 1,
diagonal weight 2). The offset, in input texels, is
`radius / (2 * sqrt((35/18) * (4^N - 1)))`. This normalizes the filters' tap sigma
across tier changes; resampling contributes an additional footprint. This is a
documented radius mapping, not a measured Gaussian-radius equivalence. Physical
spread and tier-boundary appearance still require the manager's impulse/golden review.

`view::frame_key` is exposed without changing its existing behavior. The blur
signature includes that exact scene signature (including camera, lighting,
selection/hover, preview, images/clouds, analysis, section, AO and supersampling),
source/background rectangles and clip after the source layer's final global
transform, background colors, physical target extent,
effective scale, target encoding and radius. It is per device/host; installation
on a replacement device retires the old state. A pending key becomes valid only
after submission. A resting repaint can reuse the source, while each painted backing boundary
still filters the current UI composition. Panel-only tint/layout changes reuse
the full-target source unless they change projection, source placement or extent.
No repaint timer is added to keep blur alive.

The source captures its Context/layer only in the enabled branch. `finish_prepare`
queries `layer_transform_to_global` after UI emission, including a transform set
after the callback shape was emitted. One validated snapshot transforms viewport,
background and inherited clip for both the cache key and source painting. The
callback shape stays local so egui applies its own transformation exactly once.

All effect GPU allocation is lazy and bounded. The target budget is 256 MiB,
computed with checked arithmetic before allocation, using four-byte S/C and
eight-byte down/up targets. Uniform bytes are also reported. The budget does not
claim to account for opaque driver pipeline/binding memory. Actual device limits
and format capabilities are queried. Resource creation/marker growth use error
scopes and return unavailable on refusal; failures latch until radius/host/size
changes, avoiding an allocation retry storm. Positive radius changes currently
retire/recreate effect storage, including pipelines; this is deliberately simple
but cold activation cost must be measured before PR 6 adds an interactive slider.

Native zero physical window size suspends the effect before callback preparation,
including when the surface was clamped to 1x1. Geometry/scale is checked before
casts or allocation. Device loss suspends storage before further effect work;
existing device probing and acquired-frame lifetime handling are retained.
Replacement size/device rebuilds from a fresh signature. No explicit destruction
of in-flight textures, new unsafe block, unwrap/expect/panic path, synchronous
readback, or GPU completion wait was added to production painting.

## Capability and other integrations

`blur::Capability` is `Available` or `Unavailable(reason)`. `State::capability`
distinguishes missing GPU/native split integration, software adapter, unsupported
filterable/renderable formats, unsupported source context, missing source frame,
resource/size refusal and lost device. The actual window adapter is classified
from its actual cached `DeviceType` (`Cpu` is software); no renderer-name guessing.
Software adapters and web paths allocate no effect resources or markers.

Web still uses eframe's unvendored `src/web/web_painter_wgpu.rs`, not the native
pass owner. Both WebGPU and WebGL2 report unavailable until that owner is wired
with equivalent acquire/zero-size/error ordering. This is an integration limit,
not a claim that WebGPU fragment blur is impossible. No eframe/epaint patch is used.

`ui.render`, CLI snapshots and scenario `shot` remain model-only captures, through
`engine::capture`/`gpu::capture::Capturer` or the existing CPU fallback. They do not
paint egui panels and cannot prove blur correctness. Their code paths are unchanged.
The full GPU fixture in existing `gpu.rs::tests` renders actual app egui output,
font textures, viewport callbacks and compositor into a readback target. Tests
remain in the existing area modules; no per-issue test module was added.

The PR6 controls consume this capability/reason and finite 0..40 contract, leave
saved intent intact on fallback, and request a frame on changes. They do not create
resources merely by opening Preferences. Blur remains a theme option, with no
independent global switch or quality preference.

## Internal activation and manager-only validation

The control hook is `ui.blur {radius_px?, surface_alpha?, fixture?}`. `surface_alpha` is an
internal capture-only override in 0..1; null restores ordinary fills. Transparent
host frames remain transparent. The hook returns requested radius, maximum,
capability, app-aware availability, transient guard/evidence state and cumulative
counters. Explicit radius activation is an internal session-only override;
ordinary product activation comes from a declared theme option. Default construction
is zero and performs no environment lookup or GPU capability probe.
The adapter capability probe is cached lazily on activation or an explicit query.

Counters increment at actual `create_texture`, buffer/bind-group/pipeline creation,
source/chain/UI/present encoding and marker emission/drawing. They include cache
hits and live texture/buffer/group/pipeline counts and texture/uniform bytes.
They are effect counters, not a fabricated whole-frame allocator or driver trace.
At zero, test deltas must remain zero and all live effect counts must be zero.
CPU allocations must additionally be measured with an external allocator/profiler;
no unsafe counting allocator was added. Timing alone cannot prove zero allocations.

No commands below were run by this worker. Manager's minimal focused checks:

```powershell
cargo test --locked --profile ci -p solidcraft-ui-egui --lib blur_stock_ -- --nocapture
cargo test --locked --profile ci -p solidcraft-ui-egui --lib blur_ -- --nocapture
cargo build --locked --profile ci -p solidcraft
```

The five `blur_stock_` tests compare stock and app-owned hosts without a GPU:
zero output includes identical Noop counts; positive output preserves chrome,
semantic fill/frame information and persistent IDs; asymmetric margins, stroke
width, host clipping, final joined corners and late layer transforms are covered.
They replace the old test of the vendored Window hook. They are authored only,
not executed by this worker. Open-flag forwarding also covers initially hidden,
closed and reopened windows, including stock close-request propagation via
`ui.close_kind(egui::UiKind::Window)`. Plain `ui.close()` targets Window's nearer
internal Collapsible, even when native title/collapse controls are disabled.
The other blur tests remain; PR5's broader `blur_` filter selected fourteen tests
before the PR6 additions listed below.

Cargo.lock was restored as text to the egui 0.36.2 registry source and checksum
from the matching local crates.io index entry; dependency versions are unchanged.
The full vendored egui copy and its path patch were removed; the egui-wgpu patch
and licences remain. PR6 additionally lists the already-locked winreg dependency
for the desktop app; dependency versions are unchanged. No cargo execution was
used. Set `SOLIDCRAFT_REQUIRE_GPU=1` to require an adapter
(hardware or software); a missing adapter/device fails every blur fixture consumer.
The explicit `cfg(test)` rendering fixture runs all positive compositor, cache,
pixel and suspension assertions on CI software adapters independently of production
software policy, retaining real adapter identity and format/device requirements.
A separate production-policy test asserts that actual software adapters report
`Unavailable("software adapter (gpu-software)")`, allocate no blur resources and
emit no markers. Production software blur remains unavailable.
Its requested radius remains 18 throughout UI preparation: fallback must not silently
replace the request with zero. Positive fixtures use isolated harness intent so theme
preparation cannot erase their explicit radius.

`FixturePolicy::Production` and `FixturePolicy::Rendering` exist only under
`cfg(test)`. Rendering bypasses only CPU policy through State's capability result;
the shared Preferences availability resolver consumes that result instead of
independently vetoing the real software identity. No production bypass is added.

To export fixed-scene 8/18/40 and zero light/dark images, counters, and warmed
UI/encode/GPU-completion benchmark samples:

```powershell
$env:SOLIDCRAFT_REQUIRE_GPU = '1'
$env:SOLIDCRAFT_BLUR_EVIDENCE = Join-Path $env:TEMP 'solidcraft-9809-blur-review'
cargo test --locked --profile ci -p solidcraft-ui-egui --lib blur_ -- --nocapture
```

`SOLIDCRAFT_BLUR_GOLDENS` can name a separate reviewed PNG directory for decoded
RGBA equality. No expected pixels were invented or automatically blessed here.
The fixed fixture tests both target encodings, source independence from panel
tint, rest/cache hits, camera invalidation, exact tier pass counts, on/off pixel
restoration, hostile sizes/radii, suspension/resource release, host-slot ordering,
and late layer translation/scaling. The latter checks transformed viewport,
background/clip placement, cache invalidation/reuse, and pixels outside the moved
clip even on a forced miss. Further device-loss, resize/DPI, clipping/overlap and
calibrated radius review remain manager validation work, not claimed test results.

For actual window evidence after the manager builds, start the branch app with
the same sample, dimensions, camera/theme and graphics settings as the baseline.
Use the manager's isolated TEMP settings/control-blur launch environment and its
written token file (`$blurTokenPath` below). Port 38876 is the user review app and
must not be contacted or relaunched; blur validation uses explicit port 38877:

```powershell
.\target\ci\solidcraft.exe --sample --control 38877
# In a second shell; always choose a NEW output directory:
.\scripts\blur-evidence.ps1 -Port 38877 -TokenPath $blurTokenPath -OutputDirectory "$env:TEMP\solidcraft-9809-window-review"
```

The collector saves ordinary zero UI/model captures before applying a tint,
light/dark 0/18 UI screenshots, feature counters, and warmed CPU `frame_ms` plus
screenshot-roundtrip timing samples. Roundtrip timing includes PNG readback/I/O
and is explicitly not GPU frame timing. The full fixture benchmark separately
waits for GPU completion, only in test code. For before/after zero CPU timing,
connect the collector to the manager's separately running main baseline with
`-Baseline -Port <baseline-port> -OutputDirectory <another-new-directory>`.
It never launches/builds the baseline or overwrites existing evidence. Baseline
artifacts under TEMP/solidcraft-8876-evidence and its checkout were not modified.
Compare decoded images, not PNG file bytes. If ffmpeg is available, its RGBA
framemd5 output provides pixel hashes and dimensions without Pillow:

```powershell
$mainPng = "$env:TEMP\solidcraft-8876-evidence\blur-main-zero-ui.png"
$branchPng = "$env:TEMP\solidcraft-9809-window-review\zero-ui.png"
$mainDecoded = (& ffmpeg -v error -i $mainPng -frames:v 1 -pix_fmt rgba -f framemd5 -) -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Baseline decode failed' }
$branchDecoded = (& ffmpeg -v error -i $branchPng -frames:v 1 -pix_fmt rgba -f framemd5 -) -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Branch decode failed' }
if ($mainDecoded -cne $branchDecoded) { throw 'Decoded RGBA/dimensions differ' }
```

Report timing medians/ranges, machine, adapter/backend/driver, size/DPI and power
settings from alternating warmed baseline/branch runs. Use PIX/RenderDoc or an
allocator profiler for complete GPU/CPU allocation/pass accounting. The manager
measured the exact prerequisite base `f1c2fc880`; comparisons require identical
restored UI/model settings. Current validation records belong in the PR body;
the new fixture is not a substitute for that comparison. Later manager gates are
wasm, layers, licences, lists, containers/clippy, then full CI before commit/merge.
Worker execution counts: tests/builds/checks/cargo/xtask/UI runs/screenshots = 0.

## PR6 manager reproduction and focused validation

These instructions are prepared, **not executed or visually verified by the worker**.
Use only the manager's isolated settings instance and control port (for example
38877); do not contact, restart or change the user's preview on 38876. No host OS
setting changes are needed for the injected examples. A real OS-signal review must
be reported separately from fixture screenshots.

The original theme fixture is
`crates/ui-egui/tests/fixtures/theme-blur.json` (id `blur-review`). It offers opacity,
blur and tab colours, defaults blur to zero, and includes Sketch/Render surfaces.
Send its JSON object as `ui.prefs`' `theme_import`, together with `open:"Interface"`.
For light screenshots, copy the object in memory, change `name` to `Blur review light`
and `base` to `light` (id `blur-review-light`), and import that. Model uses the base
palette; switch to Sketch/Render separately to review the authored tab colours.
Builds, launches, authenticated transport and screenshot saving belong to the manager.

For each light/dark fixture, keep model, camera, layout, scale and window dimensions
fixed. Use normal `ui.prefs`, with the active theme id, for each **0, 8, 18, 40**:

```json
{"method":"ui.blur","params":{"fixture":"clear","surface_alpha":null}}
{"method":"ui.prefs","params":{"theme_options":{"blur-review":{"opacity":75,"blur":18}}}}
{"method":"ui.screenshot","params":{"path":"ABSOLUTE-NEW-OUTPUT-PATH.png"}}
```

At zero expect Off, a hollow muted thumb and one flat cost line; at 8/18/40 expect
one/three/five filled bars. Verify the slider consumes remaining width, the readout
and meter do not overlap, and the disabled reason remains beside its control.
Also send `blur:18.25`, reopen Preferences and query `ui.prefs`: the value must remain
18.25 until an actual edit. At opacity 75 capture matched **blur 0 versus 18** views;
repeat with opacity 100 as the opaque comparison. The fixture's opacity option
enforces PR3's readability floors; do not assume opacity 0 means invisible text.
If an explicit opacity-zero comparison is desired, send `opacity:0` at both blur
0 and 18 and report that readability floor. These are real UI captures, not `ui.render`.

To display two unavailable reasons without changing this machine's OS/adapter:

```json
{"method":"ui.blur","params":{"fixture":"gpu-software"}}
{"method":"ui.prefs","params":{}}
{"method":"ui.blur","params":{"fixture":"reduce-transparency"}}
{"method":"ui.prefs","params":{}}
{"method":"ui.blur","params":{"fixture":"clear"}}
```

Capture each disabled slider with its adjacent reason. Label these screenshots
**injected availability evidence**, include the `evidence_fixture` reply, and verify
saved blur remains 18. Clearing the fixture restores normal real capability checks;
it cannot make a software/unsupported renderer capable. Compare effect counter
deltas/live counts while unavailable; there must be no new effect work and no live
effect resources. `web-unsupported` is an optional third reason fixture.

For the guard notice, set blur 40 via `ui.prefs`, let the value apply, then send:

```json
{"method":"ui.blur","params":{"fixture":"guard-60"}}
{"method":"ui.blur","params":{}}
```

This explicitly injects sixty eligible 23 ms samples, without inventing GPU timing
or starting a repaint loop. Capture the notice with **injected guard evidence** in
its description. Repeat the fixture to exercise cap 4 → 3 → 2 (no radius change).
Click Undo (`prefs:theme_notice_undo:guard`): cap returns to 5 and `opted_out` is true;
another `guard-60`, even after switching themes, cannot reduce it. In a separate fresh
session, Dismiss (`prefs:theme_notice_dismiss:guard`) hides the notice but subsequent
injections may keep reducing to 2 without reannouncing. `fixture:"clear"` removes
the injection/legacy radius override, not Undo's session opt-out. Restart the
manager's isolated app when a fresh session policy is needed.

For raw/newer warnings, the extended existing `prefs_theme_files` scenario saves
blur through `ui.prefs`, reimports that theme without the declaration, dismisses
the now-unknown-entry notice, checks that raw JSON and API notices persist, and
reintroduces the definition to restore the saved value. It also imports `requires:2`
and dismisses its warning without adding a controls header. The unit test additionally
loads malformed raw JSON, changes it to ensure reappearance, and bounds dismissal
storage. For a manager screenshot, import a copy of the fixture with name `Future
blur review`, `requires:2`, then open Interface; retain the original file for reimport
after upgrading, as its unsupported tables are discarded.

All nine new unit tests match the **`blur_`** filter, in the existing area modules:

- `blur_availability_preserves_saved_intent_and_parent_state`
- `blur_evidence_fixtures_are_atomic_transient_and_never_fake_support`
- `blur_guard_requires_consecutive_visible_actual_paints`
- `blur_guard_threshold_idle_minimum_and_session_undo`
- `blur_intent_cache_resolves_only_when_theme_or_raw_values_change`
- `blur_meter_boundaries_and_off_thumb`
- `blur_notices_dismiss_by_problem_identity_and_preserve_raw_json`
- `blur_offered_zero_has_no_effect_work`
- `blur_slider_reserves_readout_meter_and_disabled_reason`

The existing `blur_radius_schedule_and_hostile_values` also checks capped offsets.
The zero test uses actual GPU fixture counters with a declared zero option; the
cache test asserts normalization counts on 100 repeated built-in/zero/undeclared
cache hits. Neither pretends to count the application's global allocator. The
headless UI tests inspect constructed shapes/geometry, not screenshots.

Manager-only focused filters (none run by the worker):

```powershell
cargo test --locked --profile ci -p solidcraft-ui-egui --lib blur_ -- --nocapture
cargo test --locked --profile ci -p solidcraft-ui-egui --lib theme_options_ -- --nocapture
cargo test --locked --profile ci -p solidcraft-ui-egui --test scenarios prefs_theme_files -- --nocapture
```

The zero GPU fixture uses `FixturePolicy::Production`, a declared zero option and
no harness radius override. It follows the same adapter-availability gate:
`SOLIDCRAFT_REQUIRE_GPU=1` requires any adapter, including software, and a missing
adapter/device fails. `blur_production_policy_rejects_software_adapter` separately
checks production CPU fallback; positive shader tests select `FixturePolicy::Rendering`.
The PR5 fixture policy and PR6 integration have been reconciled in source only.
GPU pixels, platform builds/APIs, wasm, actual OS propagation,
real visible slow-frame behavior, allocator traces and threshold calibration remain
unverified here. No test/build/check or app interaction has been performed by this worker.
