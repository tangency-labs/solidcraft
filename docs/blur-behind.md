# Blur behind panels (#9809)

Implementation note for PR 5 of the #8876 series. Native implementation and
manager-run harnesses are present. No preference, product UI or default changes
are introduced. No build, test, GPU run, screenshot, benchmark or pixel comparison
has been run by this worker. Direct rustfmt is the only executable formatting work.

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
| Build source/blur | The enabled `blur_native.rs::Source` replaces the viewport callback. Its `prepare` delegates unchanged to the original callback. Its `finish_prepare` draws the independent background/prepared viewport into S, then the blur chain, all on the same encoder. A cache hit encodes neither source nor blur passes. |
| UI composition | The native pass owner selects an installed `FrameCompositor` before beginning the UI pass. It renders the unchanged egui primitive stream into C. At semantic markers, `PanelMarker::paint` samples B; ordinary panel fill/content follow in their original order. |
| Present/capture | A final triangle presents C into the normal drawable or existing capture texture. Existing screenshot copying, queue guard, texture frees and presentation remain after that. `submitted` commits a pending source key only after queue submission. |

`CallbackTrait::paint` receives an already active pass, not its encoder or target;
it cannot redirect the UI or start blur passes. Callback command buffers execute
before the main encoder. Source production and its dependent blur therefore use
the same encoder and return no additional callback command buffers.

## Independent source and color contract

S, B and C are separate textures with separate jobs:

- **S** is full-target, single-sample viewport/background color. Its construction
  takes the explicit background rectangle/colors, inherited clip and prepared
  `ViewportCallback::Frame`. The background pipeline has a uniform-only bind
  group: it cannot sample C or a panel texture. The source viewport paint uses
  the same prepared renderer, with its actual screen placement/scissor.
- **B** is the final full-target linear RGBA16F Kawase result, shared by every
  eligible panel in that native viewport. Intermediate down/up targets are also
  RGBA16F and are distinct read/write textures on every pass.
- **C** has the UI target's format and contains the normal sharp viewport plus
  egui UI/markers. Only the final presentation samples C. C never feeds S or B.

No prefix of the UI paint list is blurred. A docked panel or window can precede
the viewport in that list; this does not affect source construction. Panels,
menus, HUDs, toasts, dimension editors, cursors and input shields are excluded.
The source does include model/sketch/highlight/preview geometry, section caps,
reference images and point clouds drawn by the GPU viewport. A panel outside
the model rectangle sees the deterministic source background, not invented
geometry behind a dock. Extending the projection under docks is a separate change.

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
first down pass decodes each source texel before bilinear interpolation; for an
sRGB target, texture sampling performs the decode. All subsequent filtering is
linear. Marker output follows egui's gamma-premultiplied convention: convert B
to gamma, multiply RGB/alpha by rounded coverage and host opacity, and for an
sRGB attachment decode that premultiplied RGB before framebuffer conversion.
This matches egui's existing shader ordering rather than treating its output as
straight-alpha linear color. Presentation samples C in its own format; it does
not apply a second manual gamma transform. The harness exercises both unorm and
sRGB formats. The worker has not yet verified those images on hardware.

## Semantic hosts, clipping and rounding

Eligible surfaces are `Panel`, `Feature`, `Palette` and `Chrome`, with nonzero,
translucent outer fill. `Container` owns eligibility; alpha alone does not select
arbitrary UI. Opaque fills have no marker. Viewport, native application bar,
Canvas, InputShield, HUDs, Toast, Window and Popup are outside the initial policy.

The app uses stock egui 0.36.2 public APIs. In the positive eligible path,
`Container::show_frame`, `show_edge` and the bounded `FeatureWindow::show`
reserve an early painter slot before invoking the original host. Once geometry
is known they populate that slot with a marker; the original shadow, fill,
stroke and contents remain in their original slots. The zero path reserves
nothing and calls the existing `Prepared::end`, `Panel::show` or `Window::show`.
No host becomes transparent, and child `UiStack` frame/background metadata,
styles, margins, stroke widths, IDs and interactions remain stock.

`show_frame` owns `Frame::begin` and uses the final content rectangle and
joined-edge corner rounding. `show_edge` captures the panel painter before the
body can narrow its clip or change opacity, then measures the actual frame
content (the response may be clamped on overflow). Its marker uses the original
host clip, including padding/borders and any host clipping. Current adaptive
edge callers use ordinary `show`; the helper deliberately exposes no animated
panel entry point.

Only `Panel::window_placed` creates an adaptive stock Window. Its small
`FeatureWindow` wrapper keeps the existing builder private and forwards only
`fixed_size` and `show`. These sheets explicitly disable native title bars,
collapse, resize, scrolling and fades, and use an explicit persistent ID and
Middle layer. The early slot is on that exact layer. The final Area response
includes the outer margins; subtracting the original total margins reconstructs
the content rectangle for marker geometry even though stock Window relocates
`inner_margin` into an internal Frame. The marker inherits the host painter's
visibility/opacity and Area's viewport clip, rather than Resize's content-only
clip. System windows (`Surface::Window`) remain outside blur eligibility.

Manual workspace ribbon cards and History retain semantic background painting;
manual HUD painting remains plain. No private storage access, extra outer
visual container, duplicate frame or search for matching colors/rectangles is
used. The tests inject a marker function into these same helpers without a GPU
to compare stock and decorated output, geometry, IDs and semantic metadata.

A marker precedes the ordinary shadow, fill, border and content on the same
layer. It retains all four final corner radii and painter
opacity. At paint it uses the final transformed callback rectangle and actual
physical scale, matching egui rectangle snapping when enabled. It changes the
GPU viewport to the full target, retains the renderer's inherited scissor, and
uses an antialiased rounded distance mask at the original unclipped rectangle.
Thus clipping an edge does not invent a new rounded corner. UVs come from screen
position, so a moved panel samples the corresponding source pixels. Distinct
retained marker uniforms prevent one panel's queue write from changing another's
draw. Marker count is bounded to 256. Nested/overlapping panels sample the same B,
never each other's fill or content.

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
after submission. A resting repaint reuses B with zero source/blur passes; normal
UI composition/presentation still occurs. Panel-only tint/layout changes reuse
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
with the same `gpu::select::tier_of` as headless captures; no renderer-name guessing.
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

PR 6 must consume the capability/reason and finite 0..40 contract, leave saved
user intent intact on fallback, and request a frame on changes. It must not create
resources by opening Preferences. There is no persistent blur preference in PR 5.

## Internal activation and manager-only validation

The control hook is `ui.blur {radius_px?, surface_alpha?}`. `surface_alpha` is an
internal capture-only override in 0..1; null restores ordinary fills. Transparent
host frames remain transparent. The hook returns requested radius, maximum,
capability and cumulative counters. Activation is control-only and session-only;
default construction is zero and performs no environment lookup or GPU probe.
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

The four `blur_stock_` tests compare stock and app-owned hosts without a GPU:
zero output includes identical Noop counts; positive output preserves chrome,
semantic fill/frame information and persistent IDs; asymmetric margins, stroke
width, host clipping, final joined corners and late layer transforms are covered.
They replace the old test of the vendored Window hook. They are authored only,
not executed by this worker. The existing seven GPU/radius/budget tests and
zero marker/tint test remain; the broader `blur_` filter selects twelve tests.

Cargo.lock was restored as text to the egui 0.36.2 registry source and checksum
from the matching local crates.io index entry; dependency versions are unchanged.
The full vendored egui copy and its path patch were removed; the egui-wgpu patch
and licences remain. No cargo execution was used. Set `SOLIDCRAFT_REQUIRE_GPU=1` to require an adapter
(hardware or software); a missing adapter/device fails every blur fixture consumer.
The explicit `cfg(test)` rendering fixture runs all positive compositor, cache,
pixel and suspension assertions on CI software adapters independently of production
software policy, retaining real adapter identity and format/device requirements.
A separate production-policy test asserts that actual software adapters report
`Unavailable("software adapter (gpu-software)")`, allocate no blur resources and
emit no markers. Production software blur remains unavailable.

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
