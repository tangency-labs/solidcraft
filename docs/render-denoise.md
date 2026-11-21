# Optional render denoising

## Experimental Open Image Denoise

`denoise_settings.method` selects `spatial` (the existing default) or `oidn`.
`oidn_quality` selects `fast`, `balanced` or `high` (default). Spatial passes,
strength, firefly suppression and optical-filter settings remain saved but do
not control OIDN. Old files and commands keep their existing Spatial behavior.
Disabled denoising preserves the selected method without executing it.

OIDN is a non-default native build feature, `denoise-oidn`, forwarded by the
engine, desktop app and CLI. A developer build uses the pinned `oidn = 2.5.1`
Rust wrapper and an installed Open Image Denoise 2.5.1 CPU runtime. For example,
on macOS with Homebrew:

```sh
brew install open-image-denoise
OIDN_DIR="$(brew --prefix open-image-denoise)" \
  cargo build -p solidcraft-cli --features denoise-oidn
```

`OIDN_DIR` points to the installation containing `lib/`; otherwise the wrapper
uses pkg-config. Do not enable the wrapper's `bundled` download feature: the
published archive omits checksum inputs, and its platform list does not cover
all SolidCraft targets. The app does not download models at runtime. Default
builds and wasm retain portable settings but refuse an explicit OIDN render
with a reason, rather than silently substituting Spatial. A feature-enabled
build directly links the native library: a missing base library can prevent
startup. Runtime/device errors are not a missing-library loading fallback.

Both ray-traced renderers pass normalized scene-linear trace-grid RGB to the
CPU RT filter with HDR enabled, sRGB disabled and no auxiliary guides. OIDN runs
before supersampling resolve, bloom, exposure/tone mapping and photographic
finishing. The GPU remains the renderer; validated HDR readback supplies its
OIDN input. The raw film stays unfiltered. To keep an EXR, set `linear_exr: true`
under `job.options.output` for a saved CPU job or `job.options` for a saved GPU job.
If native OIDN fails **after the final raw EXR is encoded**, `render.status` reports
`failed`, the original error and `linear_master`, but no display frame. In the same
session, call `render.export` with the full failed job ID, `format: "exr"` and an
`.exr` path. This writes the original bytes without retracing. Export promptly:
failed masters share the four-entry terminal history and are not selectable images.

This recovery is available through a persistent command session (for example,
`solidcraft-cli mcp --headless`), not the one-shot PNG CLI or the completed-image
picker. Cancellation, a failed nonfinal pass, or a job without a retained final
master has nothing to export. Nothing is written automatically.

Coverage alpha is copied unchanged, but transparent-background output is explicitly refused: preserving
alpha alone does not qualify RGB reconstruction at transparent silhouettes.
Opaque antialiased silhouettes still need image-quality comparisons.

Native objects and buffers are owned by an OIDN worker. Output steps only poll
completion and do not report a fabricated native progress percentage. Admission
refuses cooperative hosts, so task retirement cannot block their caller's UI
thread. Cancellation discards late results; the owning native host retains its
work lease until the native worker joins. The current safe wrapper cannot
interrupt inference, limit native threads, set `maxMemoryMB`, or query tiling.
A cancelled job can therefore remain retiring until inference returns. OIDN's
native progress callback itself only attempts cancellation; it does not guarantee
an interruption or a retirement deadline. Adding a safe wrapper callback would
not change that native guarantee.

The in-place path admits 24 extra bytes per trace pixel: one packed caller RGB
plane and the wrapper's native RGB plane, in addition to the existing raw RGBA.
It also reserves a provisional 512 MiB allowance for models, native workspace
and runtime state. This is **an estimate, not an enforced native memory cap**.
OIDN documents `maxMemoryMB` as a request that actual allocations may exceed,
even if a future wrapper exposes it. See the official
[filter and cancellation contracts](https://www.openimagedenoise.org/documentation.html#filters).
X2 charges its four-times-larger trace grid. The allowance alone exceeds the
Compact CPU profile's 384 MiB limit; choose Studio or Workstation explicitly for
CPU OIDN. Those profiles still enforce their image and memory admission limits.
Color-only OIDN does not allocate Spatial guides. Active OIDN refuses
`denoise_diagnostics`: those guide masks describe Spatial eligibility and bypasses,
not OIDN processing. Turn diagnostics off or choose Spatial explicitly. With
`denoise: false`, guide inspection remains available without changing the saved
OIDN choice. Every complete published checkpoint currently runs OIDN as well, creating fresh native objects;
total render timings therefore include repeated inference and initialization.
They are not isolated final-denoise timings or live-preview qualification.

This feature remains experimental and is not enabled in release defaults.
Shipping still requires audited safe progress/cancellation, memory/thread and
resource-reuse controls, measured large-image/native memory admission, qualified
platform packaging and installed-app startup, representative CAD image/detail
comparisons, and an explicit progressive-preview policy. A developer-linked
proof alone does not complete [#16902](https://github.com/tangency-labs/solidcraft/issues/16902).
Retained-master regrading still uses the raw EXR and does not reapply OIDN.

## Existing Spatial filter

CPU ray-traced requests accept `raytrace.denoise: true`; saved CPU jobs store
`job.options.denoise`. GPU path requests keep their existing `denoise` flag and
saved GPU field. Shared `denoise_settings` select 1..3 passes, finite 0..1
per-pass strength, optional isolated-firefly suppression and `optical_filter`.
Omission keeps three passes at full strength with firefly suppression off, and
`optical_filter: true` enables the supported optical filtering described below.
This deliberately extends earlier denoised designs beyond opaque surfaces.
Set `denoise_settings.optical_filter: false` to restore opaque-only denoising:
mirror and glass pixels retain their original radiance while opaque filtering
uses the selected pass count, strength and firefly policy. The option is shared
by CPU and GPU jobs, saved with the setup and exposed as **Filter mirror and glass**.
Enabling suppression adds high-contrast tap attenuation and an isolated-outlier cap.
Setting `denoise: false` preserves the raw image path; optional
`denoise_diagnostics` can still capture guides independently.
See [quality controls and diagnostics](render-quality-controls.md). CPU filtering uses only the native
Rust renderer: no GPU device, WGSL runtime or software-GPU fallback is involved.
Raster cannot consume raytrace options. Offline editing does not imply that an
image host is installed.

Every complete published pass uses the selected filter, including nonfinal CPU
and fixed/adaptive GPU feedback. Fast updates can coalesce at screen repaint;
the producer does not omit an effect merely to obtain an intermediate image.
Filtering uses normalized linear coverage-premultiplied radiance before bloom,
exposure, tone mapping, sRGB encoding and photographic finishing. It never
changes the raw film, sample keys or random stream. Radiance and transport
failures are checked before filtering. Final-only raw FLOAT32 EXR comes from the
unfiltered pass, before denoising; no raw companion is retained per preview.

The filter selects the first one, two or three edge-aware a-trous passes with pixel
steps 1, 2 and 4 and the separable five-tap kernel [1,4,6,4,1]. It is inspired by
the published method of [Dammertz, Sewtz, Hanika and Lensch, HPG 2010](https://holger.dammertz.org/publications/dammertz10hpg_bibtex.html).
No third-party filtering source or weights are copied. Deterministic pixel-centre
rays supply effective albedo, shading normal, body identity, position, depth and
projected pixel footprint. CPU guides reuse its actual prepared hit and supported
surface-effect evaluation; positions are made camera-relative in f64 before
conversion to f32. Guides are captured once per immutable CPU job and never
consume an absolute camera-sample ordinal.

Bilateral weights use albedo difference, normal agreement, distance from the
local tangent planes, axial depth difference and radiance contrast. Spatial
thresholds scale with the projected pixel footprint. Every pass preserves the
original coverage. Online convex averaging avoids multiplying HDR radiance by
large kernel weights.

When firefly suppression is selected, extremely bright taps receive a stronger
contrast penalty at every scale.
The first pass can also suppress an isolated bright pixel when at least five
immediate neighbors agree with its geometry and material guides and every
supporting neighbor is much dimmer. The suppression scales RGB together to
retain its hue. A bright neighbor on the same surface can preserve a thin
highlight. This display heuristic can still attenuate a genuine unresolved
glint, so keep the raw EXR for radiometry. Speckle the filter leaves on smooth
metal can be limited with the separate, biased
[indirect clamp](render-path.md#clamping-indirect-light).

Guide generation and diagnostic eligibility are independent of the optical-filter
preference. Turning it off bypasses filtering of Optical pixels; their diagnostic
category continues to report the available optical correspondence. It does not
change transport, sample identities, raw EXR, alpha or the unsupported-material policy.

Optical guides follow deterministic centre rays beyond supported sharp
reflections and smooth glass. A guide stores the primary body and the opaque
endpoints visible along the reflected and transmitted branches. If both
branches contribute, both endpoint identities must agree between taps. Both
endpoints retain separate albedo, normal, position, depth and footprint guides.
The filter multiplies their compatibility weights, so a material or geometric
edge on either branch prevents cross-edge averaging, even on the same body. Optical
filtering uses steps 1 and 2 and stronger colour contrast. The isolated-pixel
suppression is disabled on optical guides to preserve glints.

The bounded trace supports near-perfect metal, the CPU's explicit Mirror
override, smooth thin glass and an eligible single closed glass body. It
falls back to bypass if a branch has no compatible opaque endpoint, if the
path involves unsupported rough, dispersive, coated, nested or scattering
glass, or if the optical guide cannot be represented. Optical correspondence
uses captured body IDs 1 through 511 and uses two 48-byte surface records per pixel.
The deterministic guide does not separate the radiance of the two branches
or reconstruct depth of field. Emitters, background and partial coverage
retain their bypass. This is a biased display filter, so fine highlights,
texture detail and soft shadows still need sample convergence.

CPU admission prepays 96 bytes per pixel for guides plus two 16-byte colour
buffers. If shared presentation or EXR already admits one linear buffer, the
additional pixel storage is 112 bytes rather than 128. Bounded guide cursor and
output-task storage is also admitted; there is no retained image per pass.
The existing CPU resource profile and shared host pool still apply. Final output
is bounded at 8192 pixels per side and 33,554,432 pixels; X2 can use an internal
guide/filter grid up to 16384 per side and 134,217,728 pixels. Admission includes
the enlarged grid and reconstruction storage, even when EXR retention is off.
No size is reduced and no profile is raised automatically. Each filter continuation processes at most 256
pixels with a fixed 25-tap gather per pixel and cancellation checks between
pixels. Work units count these bounded pixel visits, as for existing finishing
kernels: one visit per internal image pixel for each selected filter pass. Guide rays
retain the existing incremental BVH node/triangle charges, using the same
cumulative job budget without resetting it between output and transport.

GPU admission keeps 128 bytes per filter-window pixel for six guide vec4s and two
colour buffers, plus separately admitted adaptive normalization and gathered film
input when needed. Large films use bounded row pages, and the reusable filter
window includes the complete dependency halo: fourteen rows on each side for
three passes. Global image coordinates and footprints remain unchanged. Only the
core contributes to output and diagnostic counts. The total film remains resident;
current GPU job, storage-binding and native RAM limits apply before allocation.
GPU guides reuse production intersections and material evaluation; cancellation
is checked between bounded dispatch tiles. An unpaged job can reuse its completed
guides across checkpoints; paged jobs regenerate each window in the same bounded
allocation. CPU and GPU filters share the numerical policy, not bit-identical
transport or a common device pipeline. See the
[GPU production envelope](render-gpu-production-envelope.md) for storage accounting.

Preflight names the enabled method, selected passes/settings, guide/bypass policy,
work convention and limitations. Saved settings preserve the explicit controls.
Diagnostics add bounded category counts and a separately retained PNG mask; they
never change guide eligibility or the beauty/linear outputs. Retained-master
regrading has no geometry guides and does not reapply denoising; its metadata
reports `denoise_applied: false`, and the original raw EXR remains unchanged.
Submit otherwise identical unfiltered and filtered jobs to retain both PNGs.

Executed CPU renderer regressions include an independent direct f64 gather
oracle, HDR/constant/coverage identity, noise reduction, body/albedo/normal/depth
edges, production centre-ray guides and bypasses, translated geometry, lens-guide
identity, false-path byte/work/allocation identity, every filtered prefix,
quantum independence, raw film/EXR preservation, cancellation and full scratch
admission. Earlier OPUS qualification recorded 827 CPU tests (one ignored) and 260 GPU tests
(three ignored), with unrelated failures in the combined document, engine and UI
runs. Those counts describe that earlier source identity; the PR validation
receipt records the current quality-control build. Native 4K comparisons preserve raw
transport within floating-point precision, but their low sample counts do not
establish converged image quality. See [OPUS qualification](render-opus-quality.md).
