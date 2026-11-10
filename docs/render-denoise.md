# Optional spatial denoising

The evaluated snapshot was integrated on pinned main
`1ab3a5b1796564bc76dc57a513115f521edd0e90`, preserving graphics backend recovery,
optional indirect-clamp behavior and the shared `solidcraft-look` assets.
The published implementation was subsequently rebased to parent `201de37b9`
as commit `c47c2a676`. The [publication qualification](render-glass-convergence-validation.md#publication-and-evaluated-snapshot)
preserves its implementation-path identities. The [separate publication gate](render-glass-convergence-validation.md#publication-snapshot-gate)
records the actual command outcome on that newer base; earlier UI and
image measurements retain the evaluated snapshot.
The [current integration record](render-glass-convergence-validation.md#current-integration)
identifies each executed check's source, actual outcome and hardware scope. It
keeps the failed or interrupted `a3475683` attempts and earlier `42918fe5`
measurements attached to their original source.

CPU ray-traced requests accept `raytrace.denoise: true`; saved CPU jobs store
`job.options.denoise`. GPU path requests keep their existing `denoise` flag and
saved GPU field. Shared `denoise_settings` select 1..3 passes, finite 0..1
per-pass strength, optional isolated-firefly suppression and `optical_filter`.
Omission keeps three passes at full strength with firefly suppression off, and
`optical_filter: true` enables the existing supported optical filtering.
The expanded clear-glass endpoints and bounded TIR coverage additionally require
`extended_optical_guides: true`, which defaults to false.
The existing optical filter extends earlier denoised designs beyond opaque surfaces.
Set `denoise_settings.optical_filter: false` to restore opaque-only denoising:
mirror and glass pixels retain their original radiance while opaque filtering
uses the selected pass count, strength and firefly policy. The option is shared
by CPU and GPU jobs, saved with the setup and exposed as **Filter mirror and glass**.
Camera-scatter guidance, extended optical endpoints and separate component
filtering are additional opt-in controls. Their omitted values remain false.
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
The clamp acts during transport and is off by default. A raw EXR bypasses
denoising but does not recover radiance removed by an explicitly enabled clamp.


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
The deterministic guide does not reconstruct depth of field. Direct emitters,
direct background and partial coverage retain their bypass. With
`denoise_settings.extended_optical_guides: true`, a secondary branch may end in
an environment direction or an analytic rectangle emitter. Environment guides
compare outgoing directions with a camera-angular footprint. Emitter guides
compare the captured source identity, its plane and the projected footprint.
Endpoint kinds are explicit in the existing guide records, so an endpoint cannot
match an opaque body with the same numeric ID. The option defaults to false.
An environment miss while still seeking a closed body's exit remains unsupported.

The same opt-in follows up to four **total internal reflections** inside one
certified smooth closed body before a transmitted exit. Each internal reflection
has exactly one surviving direction; additional probabilistic Fresnel branches
are not unfolded. The primary entry, every internal boundary and the final exit
must remain on that same unambiguous body. The camera path budget must admit at
least `3 + internal_reflections` vertices. Rough, dispersive, coated, nested,
scattering or lens-ambiguous paths keep their existing bypasses. Boundary-edge
ambiguity, a missing exit or any failed route bound preserves the whole pixel;
the implementation does not pretend an invalid pair is a valid partial guide.

TIR guides retain both complete endpoint records in the existing 96 bytes. A
separate finite optical tag marks the otherwise-redundant reflected-body lane as
an exact integer route: a prefix sentinel followed by entry, TIR and exit
body-local triangle ordinals in radix `body_triangle_count + 1`. The main tag
continues to carry all three body IDs. The route is checked against the exactly
representable 24-bit integer range of a numeric f32; no hash or collision tolerance
is used. Whole-beauty and transmitted-component filtering compare that route
before projecting the selected component. Reflected components ignore only the
transmitted route and retain their original endpoint checks. The independent
`optical_filter: false` preference bypasses every optical component, including
TIR guides, while diagnostics still report their available correspondence.

The GPU assigns local ordinals before BVH reordering and stores an 11-bit ordinal
and 11-bit body count in unused high bits of the existing material flag lane.
The original two material flags and all medium values keep their meaning;
physical boundary equivalence ignores only the new guide metadata. The wire
material size, guide size and number of storage bindings do not grow. The CPU
staging triangle's actual size remains included in admission. The limits are
reported separately:

| Diagnostic | Meaning |
| --- | --- |
| `optical_topology_limit` | The body exceeds 2,047 triangles or its local topology metadata is unavailable. |
| `optical_route_limit` | The complete boundary sequence cannot fit the checked 24-bit route. This can occur below the triangle-count cap. |
| `optical_reflection_limit` | More than four deterministic internal reflections would be required. |
| `path_depth` | The selected transport depth cannot reach this guide endpoint. |
| `degenerate_path` | A boundary is ambiguous or a physically valid continuation cannot be established. |

Distinct coplanar triangles remain distinct route identities. This conservative
choice can reduce support for whole-beauty and transmitted-component filtering
along tessellation seams; it avoids treating different internal boundary
sequences as the same correspondence.
The extension improves coverage of clear-body TIR paths, but does not guarantee
noise-free glass at a fixed sample count. Additional branching glass, rough lobes,
ambiguous nesting and lens footprints still require richer correspondence.

`denoise_settings.variance_guided: true` enables a separate, optional 16-byte
uncertainty plane per pixel. The renderer records complete camera observations,
including zero-contribution events, using RGB online moments. The filter uses
observed scatter to control contrast weights and per-pass blend strength. Zero
scatter with at least two observations preserves the original pixel; fewer than
two observations use the existing contrast policy. Raw moments remain unchanged
through all filter passes. Filtered neighbours are correlated, so the implementation
does not invent an independent-sample posterior after each spatial pass.

These statistics are conditional on shared photon maps. They do not measure
finite-radius bias, guarantee convergence, or turn samples within one randomized
low-discrepancy sequence into independent observations. A reused sparse map can
remain wrong even when camera scatter is zero. The option defaults to false and
requires explicitly admitted statistics storage. Its absence keeps the original
filter's numerical policy and scratch allocations.

Optional `radiance_components` statistics use six exclusive scene-linear
components: camera Other, Reflected or Transmitted, each crossed with NonCaustic
or Caustic. The first camera-side optical event supplies its category; ordinary
receivers freeze that category. Rough-interface light connections and photon
gathers classify reflection or transmission from each incoming direction before
a continuation branch is sampled. Caustic classification follows the physical
optical light tail and is independent of which estimator sampled it. Contributions
already contain all transport weights. Every observation updates every component,
including zeros; their unfiltered sum must reproduce beauty. A separate beauty
moment retains component covariance.

`denoise_settings.component_filtering: true` filters each of the six components
separately, then adds the results in scene-linear space before presentation. It
requires `radiance_components: true`; CPU capture additionally requires the
progressive path integrator. Each component always uses its own observed scatter,
regardless of the whole-beauty `variance_guided` control. Reflected components
compare the reflected guide endpoint; transmitted components compare the
transmitted endpoint. Both retain the primary body's identity and original
coverage, so unrelated radiance on the other branch cannot hide a valid
correspondence or guide its contrast weights. Opaque components use the ordinary
surface guides. Unsupported optical correspondence still bypasses filtering;
component separation does not make rough, nested, dispersive or lens paths
eligible. A pixel with only independent adaptive pilot observations retains its
original beauty until production component observations exist.

The [rough-transmission footprint design](render-rough-transmission-footprints.md)
specifies a future production-sample guide, including complete estimator
coverage, exact-route refusal, normalized geometric moments and bounded
CPU/GPU resources. It is a design deliverable; it does not change these current
bypasses or make geometric covariance a convergence certificate. Production
rough-transmission capture and filtering are not implemented.

The [recorded clear-glass repeat](render-glass-convergence-validation.md#clear-glass-repeat-at-the-prior-source)
preserves raw output and reproduces the expanded TIR-guide coverage on its pinned
`84bda659` implementation. It is a narrow supported-domain result, not an
independent error/detail reference or a validation of the later integration.

Reconstruction runs through one component at a time and preserves the beauty
alpha. Component moments and the raw film remain immutable. The final raw EXR is
written before any component reconstruction. This remains a biased display
filter: fine highlights, texture detail and soft shadows still need sample
convergence, and a low camera variance does not certify a photon map.

CPU camera-only RGB moments require 56 bytes per pixel; the six-component
moments plus beauty covariance require 344 bytes per pixel. The optional
uncertainty plane adds 16 bytes per pixel. These resources are admitted only when
requested and remain separate from the guide/filter costs below. GPU moments
live in admitted accumulation planes. Whole-beauty filtering uploads bounded
windows of uncertainty; component filtering prepares each window directly from
its component mean, M2 and common production count. The disabled path reuses a
read-only guide binding and allocates no uncertainty buffer.

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
kernels: one visit per internal image pixel for each selected filter pass. Component
filtering additionally admits 164 bytes per internal image pixel: a compact
148-byte snapshot of all six means/variances and their common count, plus one
16-byte linear sum. The existing color ping-pong and 16-byte uncertainty plane
are reused for each component. Snapshot, initialization, loading, filtering and
recombination are charged: `(14 + 6 * passes)` pixel visits per published image,
in addition to common output/guide work. Guide rays retain the existing incremental BVH node/triangle charges, using the same
cumulative job budget without resetting it between output and transport.
The general CPU glass certificate supplies the route's primitive range directly.
Legacy closed-box certificates additionally visit at most 510 captured body rows
at an entry; each row is charged and cancellation-checked. The small route state
is part of the admitted guide cursor and allocates no per-pixel array.

GPU admission keeps 128 bytes per filter-window pixel for six guide vec4s and two
colour buffers, plus separately admitted adaptive normalization and gathered film
input when needed. Component filtering adds four 16-byte window planes
(component input, M2, production count and linear sum); it shares one additional
16-byte uncertainty plane with the filter. No six-plane host image is allocated
for GPU reconstruction. Large films use bounded row pages, and the reusable filter
window includes the complete dependency halo: fourteen rows on each side for
three passes. Global image coordinates and footprints remain unchanged. Only the
core contributes to output and diagnostic counts. The total film remains resident;
current GPU job, storage-binding and native RAM limits apply before allocation.
GPU guides reuse production intersections and material evaluation; cancellation
is checked between bounded dispatch tiles. Legacy optical guidance has at most
three secondary traversal events; the opt-in TIR walk has at most seven. Cold
guide dispatches still start at 64 pixels, and the extended path caps dispatch
growth at three sevenths of the existing maximum while retaining measured-latency
adaptation. An unpaged job can reuse its completed
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
runs. Those counts describe that earlier source identity. Each qualification
receipt records its own build; it does not validate a later restack. Native 4K comparisons preserve raw
transport within floating-point precision, but their low sample counts do not
establish converged image quality. See [OPUS qualification](render-opus-quality.md).

## Evaluated snapshot evidence

The evaluated-snapshot [public clear-glass record](render-glass-convergence-validation.md#current-public-evidence)
compares each backend's raw off/filter outputs separately from its display
images. It records decoded raw identity, finite-value and component-accounting
checks, together with visual review and remaining image limitations. Raw
preservation does not establish cross-backend image parity, independent
physical reference agreement or preservation of every resolved glint,
caustic and texture. Broader coverage and depth-of-field checks remain open.

## Completion qualification

The historical a77 CPU/GPU clear-glass repeat completed: each backend's
raw/display/mask files match its own pinned prior84 outputs exactly, with 6,441
supported optical pixels. Relative to the pre-extension masks, all 2,687 former
TIR-bypass pixels are supported and all 3,754 prior optical pixels are retained.
The [completion evidence record](render-glass-convergence-validation.md#completion-evidence-and-publication-status)
keeps this result attached to its pre-fixture-correction source. The exact
one-example-file mapping and later test-only repair are recorded separately.
A77 CI02 did not pass; completed a77 CI03 also exited 1, with 28 exact known-premerge
failures. The selected `42918fe5` gate also exited 1, with 23 strict matches and
five changed panic-header locations. All five failures reproduced on bare
selected main. The `42918fe5` clear repeat then completed under its separate
image-eligibility proof: each backend's raw/display/mask bytes match its own
reference. Independent decoding confirms the 6,441 supported optical pixels.
Aggregate raw/component accounting is distinct from reported per-observation
maxima, as detailed in that checkpoint's qualification record. No new visual judgment
was made by that auditor; exact file identity permits reuse of earlier inspection.
No later source identity relabels this image result. Grain remains; there is no
independent reference-error or noiselessness claim. Earlier reconstruction
results retain their historical scope. Headless control captures do not measure
reconstruction quality.
