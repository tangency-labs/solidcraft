# Glass convergence implementation

## Scope and dependency

This change implements bounded photon diagnostics, additive radiance statistics,
sampling controls and supported clear-glass reconstruction from
[the findings and plan](render-glass-convergence-plan.md).
Rendering PR #12546 merged on 9 November 2026 as
`ed2bafe7e312ff38f1d957a511d4c8e529dc4e32`.

The implementation is integrated on pinned main
`1ab3a5b1796564bc76dc57a513115f521edd0e90`, preserving graphics backend recovery,
optional indirect-clamp behavior and the shared `solidcraft-look` assets.
The [current integration record](render-glass-convergence-validation.md#current-integration)
separates this source's qualification from the failed or interrupted `a3475683`
attempts. Earlier `42918fe5` results retain their original source and do not
qualify this newer integration.

The current restack preserves upstream product-preview and backend controls,
the optional GPU indirect clamp, shared look setup and the recovered convergence
features. All 106 implementation file blobs and worktree byte hashes were
unchanged when transferred from `a3475683` to `1ab3a5b1`, before this documentation
update.
The former `42918fe5` checkpoint, its nonzero full gate, targeted baseline
reproductions, clear images, controls and public strategy measurements remain
[historical evidence](render-glass-convergence-validation.md#merged-dependency-and-completion-qualification).
They are not relabeled as measurements of the current source.

Changes a default: **no**. Existing documents retain independent camera samples,
fixed photon radii, legacy receiver ownership, one CPU Fresnel stratum, disabled
interface splitting, no CPU Russian roulette and the
existing combined-image filter. Every new transport or reconstruction choice is
explicit and saved. The raw linear master remains the unfiltered transport
estimate.

The goal is lower visible noise with auditable energy and bounded resources.
This is not a guarantee of exact, noiseless images at a finite budget. In
particular, photon density bias and missing refracted-connection strategies
cannot be diagnosed from a quiet per-pixel variance value alone.

## Cohesive modules and narrow interfaces

| Responsibility | Module | Contract |
| --- | --- | --- |
| Camera randomization | `render/sampling.rs`, `gpu/path/sampling.wgsl` | Stable pixel, observation, semantic-event and branch identities; existing independent draws preserved |
| First-Fresnel strata and bounded splitting | `render/raytrace/image_sampling.rs`; existing GPU lane dispatch and smooth-interface helper | Complete groups own counts; only post-split throughput receives branch weights |
| Glossy receiver ownership | `render/job/photon_receiver.rs`; CPU gather/continuation hooks and WGSL mirror | One shared base-roughness predicate transfers only the reciprocal continuous-source family |
| Linear component and moment types | `render/radiance.rs` | Six exclusive nonnegative RGB contributions, stable camera/light labels, Welford moments |
| CPU film statistics and summaries | `render/radiance_film.rs` | Optional allocation, complete observations, finite input, bounded summaries and cancellation |
| GPU film statistics | `gpu/path/radiance.rs`, `radiance.wgsl` | Appended planes within the existing paged film binding; bounded readback |
| Photon schedule and work validation | `render/job/caustics.rs` | Separate surface/volume radii, explicit refresh requirement and checked work counts |
| Photon probe and camera accounting | `render/job/photon_diagnostics.rs`, `photon_accounting.rs` | Bounded local/camera rankings, complete lane groups and at most four trace pixels with their own production denominators |
| Photon probe implementation | `gpu/path/photon_probe.rs`, `photon_probe.wgsl`; CPU gather/completion hooks | Prefix-weighted diagnostics reconcile with raw output without changing transport expressions or random draws |
| Independent pilot evidence | `render/job/adaptive_evidence.rs`, `photon_support.rs`, `film_adaptive_evidence.rs`; `gpu/path/adaptive_evidence.wgsl` | Separate camera/map scatter, component allocation and receiver-query support; no production-dependent stopping |
| Optical correspondence | Existing CPU/GPU guide modules, shared guide contract and packed triangle topology | Bounded clear-body routes with exact identities; unsupported paths remain explicit |
| Component reconstruction | `render/job/denoise_components.rs`, `gpu/path/denoise_components.rs` and small shaders | Sequential component filtering with matching guides, then one linear sum |
| Saved and interactive controls | Existing document, command and workspace modules | Preserve authored intent; validate incompatible requests before starting work |

The shared renderer layer owns numerical contracts and has no UI or GPU
runtime dependency. CPU and GPU integrate these contracts at their existing
sample-completion, photon-gather and output boundaries. The photon estimator,
camera sampler and display filter do not call one another's internal algorithms.
No geometry-kernel changes or new native dependencies are introduced.

## Photon convergence and diagnostics

`caustics.progressive_radius` requires `refresh_per_pass`. For the one-based map iteration `k = epoch + 1`,
its surface radius is `r0 * k^(-1/6)` and its volume radius is
`r0 * k^(-1/7)`. Each independent batch retains its own emitted-photon
normalization and kernel normalization. Complete camera observations are
averaged with equal weight. An old observation is never renormalized using the
new map's radius.

With `P` adaptive pilot passes, refreshed production ordinal `j` uses photon
epoch `P+j` on both backends. Ordinary pilots advance the radius schedule, so
production begins at `radii_mm(P)`, not at the initial radius. Optional independent
map-assessment batches instead use reserved RNG epochs and one fixed reference
radius: the radius of the first production map. Production restores the canonical
epoch and seed; a fixed production map restores epoch zero. No assessment map is
reused for production. The pilot-selected budget still changes the average
finite-bandwidth bias when production uses the evolving radius sequence.

This is an independently refreshed, decreasing-bandwidth density estimator,
not the visible-point flux/radius recurrence of SPPM. Its consistency argument
names smooth density, finite variance, a fixed transport domain and independent
batch assumptions. Singular caustics, sharp receiver BRDFs, finite budgets and
scene-dependent rejection can remain difficult. The detailed derivation and
qualifications are in [photon convergence](render-photon-convergence.md).

Surface and volume kernels use their own radii and dimensions. The spatial grid
admits both support sizes. Receiver identity, side, normal, path-depth and
leakage checks remain in place. The optional
`caustics.glossy_continuation_max_roughness` transfers supported smooth-glass
area/environment paths at selected opaque receivers exclusively to the existing
full BSDF continuation. The matching photon records and reciprocal endpoint
suppression change together. Hard-source, rough and volume ownership remain
unchanged. The [receiver contract](render-photon-convergence.md#optional-glossy-receiver-continuation)
defines actual mapped base roughness, clear-coat behavior, finite-depth limits
and the four-byte optional GPU threshold.

An optional `probe_pixel: [x, y]` and zero-based production `probe_sample`
select bounded detail. Separate top-eight lists and histograms rank receiver-local
events and their camera-weighted contributions; an RGB prefix can change that
ranking. Events retain source/PDF/flux, BRDF/BTDF/phase, normal correction, receiver
roughness, represented camera-interface count, path class and query support.

The report also follows the selected output pixel's complete trace footprint:
one trace pixel normally, four under X2 supersampling. It accumulates actual
photon-estimator and other-estimator terms alongside raw beauty, averages complete
Fresnel groups once, and divides each trace pixel by its own actual production
count before the spatial average. These estimator labels are separate from the
physical caustic/non-caustic partition. Finalization compares with the actual
premultiplied scene-linear raw pixel before denoising or finishing.

`selected` and `camera.selected_sample` distinguish requested detail from a
completed selected observation. If adaptive allocation never reaches that
ordinal, both backends still provide the configured report with `selected: false`
and the final output aggregate. That does not mean zero photon energy.
`final_output_radiance` states a retained event's share of the output mean after
all denominators are known. Raw query sums must not be compared with film energy
without these weights. Full units, equations and availability are documented in
[photon convergence](render-photon-convergence.md).

This is a bounded diagnostic rather than a whole-frame event log. Optional
storage is admitted before work; camera and photon random streams do not depend
on whether a probe is enabled.

## Additive path ownership

The stored partition has two axes:

| Camera-side label | Non-caustic | Caustic |
| --- | --- | --- |
| Other | Component 0 | Component 1 |
| Reflected | Component 2 | Component 3 |
| Transmitted | Component 4 | Component 5 |

The first supported camera-side optical event determines reflection or
transmission. Reaching an ordinary receiver freezes that camera-side label;
subsequent light-side events do not relabel the view. A polished opaque reflection
uses the shared sharp-metal domain. Rough dielectric NEE and photon gathering
classify reflected and transmitted contributions from their actual directions,
not from the independently sampled continuation branch.

The light-side label follows the physical path. Photon gathering contributes to
the caustic column, and an independently sampled endpoint or hard-light
connection with the same optical tail receives the same label. A regular
receiver starts a new light-side tail. An optical event after an earlier receiver
marks that tail as caustic. Thin-sheet visibility connections account for their
interfaces. The label is not simply “photon estimator versus camera estimator.”

Every contribution enters exactly one cell, with its existing transport weight
applied once. All six cells are in the same scene-linear space. The unfiltered
sum is checked against beauty for every complete production observation. CPU
uses a scaled tolerance of `1e-10 * max(1, abs(beauty))`; GPU uses
`2e-4 * max(1, abs(beauty))` for its f32 path arithmetic and reports the maximum
absolute residual. Both tolerances include an absolute floor below unit radiance.
These are accounting guards, not physical accuracy claims.
Tests also compare low-energy results with relative tolerances.

The GPU rough-photon helper retains the original per-photon total accumulation
order for raw beauty while recording the two directional components separately.
Component capture must not change the raw estimator through a different random
stream or a different grouping of its additions.

## Observation counts and uncertainty

A camera observation is the average of all requested first-Fresnel strata.
Its correlated lanes do not count as independent observations. Zero-contribution
samples and zero components still update the common count. Adaptive pilots do
not enter production moments. Production budgets are fixed by independent pilots,
so this feature does not introduce production-dependent stopping.

The film stores RGB mean and Welford M2 for beauty and, when requested, each
component. Beauty has its own moments: summing component variances would omit
covariance. With at least two complete observations, the displayed uncertainty
input is `M2 / (n * (n - 1))`. A smaller count is explicitly unavailable. Zero
observed scatter does not establish zero error.

This quantity describes camera-observation scatter. Reusing a photon map adds
dependence. A decreasing radius changes the expected observation between epochs.
A Sobol scramble introduces within-scramble dependence. Consequently the report
is deliberately not named a convergence certificate, calibrated confidence
interval or estimated photon-radius bias. Independent seeds/scrambles and
separate radius comparisons remain necessary for quality assessment.

`SamplingReport.budget_limited_pixels` reports pixels whose pilot allocation
wanted more samples than the cap. It does not claim that a finished production
pixel met an error threshold. The optional optical sample floor raises the
minimum for an admitted optical scene without using production radiance to
choose when to stop.

### Component and independent-map pilots

`adaptive.component_uncertainty` additionally allocates from the six component
pilot moments. It does not require allocating or enabling a production component
film. Complete observations, including zeros, enter the evidence; pilot sums must
recombine with beauty.

`adaptive.map_validation_batches` admits 2, 4 or 8 independent photon-map batches
with equal sizes and at least two camera observations per map. Its fixed
reference radius separates conditional camera scatter from variation between
maps. Fixed-map production allocates for reducible camera scatter and reports
map scatter separately. Refreshed production also includes estimated map scatter
in the allocation. Neither case treats camera samples as a cure for finite-radius
bias. Invalid evidence conservatively selects the maximum camera budget and is
reported explicitly.

`SamplingReport.pilot_evidence` reports component-budget pressure, map scatter,
missing/invalid evidence and receiver-query effective event support. The latter
uses scale-normalized positive contribution weights within each query; repeated
records or lanes are not declared independent photon observations. All production
budgets are frozen before production radiance is observed. The precise equations,
controls and limitations are in [camera sampling](render-camera-sampling.md).

## Reconstruction and endpoint guidance

`denoise_settings.variance_guided` supplies observed uncertainty to the existing
portable spatial filter. `component_filtering` requires
`radiance_components: true` and enables separate filtering of all six cells.
Component filtering uses component moments even when the combined-image variance
option is not selected.

Reflected components consult reflected endpoint correspondence; transmitted
components consult transmitted endpoint correspondence. An unrelated edge in the
other branch no longer vetoes all taps. The “other” component uses the ordinary
receiver contract. Filtering remains sensitive to identity, geometry, radiance,
coverage and the admitted optical domain. The six results are added linearly
before exposure, tone mapping and finishing. Alpha remains raw primary coverage.
Raw EXR output is produced from the original film.

`extended_optical_guides`, which defaults to false, additionally represents valid
clear-glass environment endpoints by direction and angular footprint, and analytic
emitter endpoints by identity and footprint. For a supported smooth closed body,
it follows up to four deterministic total internal reflections before a valid exit. An exact radix
route records entry, every internal reflecting triangle and the exit; distinct
routes cannot collide through a hash. The route must fit in 24 bits, the body
must have at most 2,047 triangles, and the requested transport depth must admit
the complete chain. Larger bodies, longer or unrepresentable routes and ambiguous
boundary hits receive explicit diagnostic categories. Per-triangle route identity
is conservative and can reject whole-beauty or transmitted-component taps across a
coplanar triangle seam.

CPU tracing uses the existing resumable traversal and charged work budget. GPU
guide work has a fixed event bound and a proportionally smaller dispatch limit.
Both retain the 96-byte guide. Packed GPU triangle metadata leaves existing
transport flags intact; material equivalence ignores only the guide metadata.
The reflection component does not depend on the transmission route, while the
transmission and combined-image filters require matching routes.

This does not convert a deterministic center ray into a rough-transmission
footprint. Rough glass, nested/scattering configurations, depth-of-field and other
ambiguous correspondence retain the existing conservative bypass. The dependency's
`optical_filter: false` preference also remains effective with extended guides and
component reconstruction. The guide diagnostics expose supported and bypassed
regions independently of this display preference.

See [denoising](render-denoise.md) for filter equations, unsupported domains,
raw-output preservation and the distinction between reconstruction and optional
highlight suppression.

## Resource bounds

All statistics and probes are optional. Admission includes their device, host,
readback and reconstruction allocations before rendering begins. CPU keeps large
statistics in a fallibly allocated optional slot rather than inflating every
image task and worker stack. Worker probes merge into one bounded owner report.

| Additional resource | Bound |
| --- | --- |
| CPU beauty statistics | 56 bytes per trace pixel for moments, plus a 16-byte uncertainty snapshot |
| CPU six-component statistics | 344 bytes per trace pixel for moments, plus the 16-byte uncertainty snapshot |
| CPU component reconstruction | 148-byte component snapshot and 16-byte linear sum per trace pixel; existing color/filter storage remains separately admitted |
| GPU beauty statistics | 3 appended vec4 planes per trace pixel |
| GPU six-component statistics | 27 appended vec4 planes per trace pixel, including complete-stratum and split-dispatch scratch |
| GPU host uncertainty | 16 bytes per trace pixel |
| GPU statistics readback | At most two core-page vec4 planes; nonadaptive admission includes the added second plane |
| GPU component reconstruction | 64 bytes per filter-window pixel, plus one 16-byte uncertainty plane shared with variance filtering |
| Component-only pilot evidence | 544 CPU bytes or 368 GPU bytes per trace pixel, independent of production component capture |
| Map-only pilot evidence | 256 CPU bytes or 128 GPU bytes per trace pixel |
| Combined component/map pilot evidence | 1,216 CPU bytes or 704 GPU bytes per trace pixel, plus separately admitted metadata and worker state |
| Photon probe | Optional bounded owner, lane/query scratch and transient reports; GPU 4,096-byte tail and optional 32 extra bytes per photon record; no per-image event log |

The GPU continues to use its existing row pages and complete denoising halos.
Film planes stay in the existing accumulation binding. Component preparation uses
six storage bindings, within the renderer's requested device limit. It gathers
one component window at a time and recombines on the device; it does not copy six
full-resolution component images to the host. Cancellation is checked during
allocation, gathering, filter dispatches, readback and encoding.

## Trying the options

For an existing valid clear-glass setup, open the **Sampling** inspector. With
CPU rendering, select **Use progressive sampling** if the setup still uses fixed
sampling, then enable **Indirect lighting** under **CPU ray tracing**.

Enable **Record light-path components** under **Adaptive**; adaptive allocation
itself does not need to be enabled. In the active renderer's denoise controls,
enable **Reduce noise in the finished image**, **Filter mirror and glass**,
**Filter light-path components separately** and **Extended clear-glass guides**.
The last control includes supported environment/emitter endpoints and bounded
internal reflections.

**Guide filtering with sample variation** is a separate whole-image option.
Component reconstruction already uses each component's observed variation.

The following are option fragments to merge into an existing valid saved job.
Retain its authored scene, optics, lighting, camera and admitted image size.
These fragments illustrate enablement, not a qualified quality preset.

CPU:

```json
{
  "method": "cpu_raytrace",
  "options": {
    "progressive": { "samples_per_pixel": 16, "seed": 0 },
    "path": { "max_depth": 8 },
    "radiance_components": true,
    "denoise": true,
    "denoise_settings": {
      "optical_filter": true,
      "component_filtering": true,
      "extended_optical_guides": true
    }
  }
}
```

GPU:

```json
{
  "method": "gpu_path",
  "options": {
    "samples": 16,
    "max_depth": 8,
    "radiance_components": true,
    "denoise": true,
    "denoise_settings": {
      "optical_filter": true,
      "component_filtering": true,
      "extended_optical_guides": true
    }
  }
}
```

For transient `render.preflight` or `render.start`, CPU requests use
`integrator: "raytrace"` and put these CPU options under `raytrace`. GPU requests
use `progressive: true` and put the GPU options at the request's top level;
omit `integrator` for that GPU protocol.

CPU Path also requires `primary_samples: 1` and traced shadows. Those are defaults,
but older setups can retain incompatible settings. Selected legacy reflection
and certified anisotropy overrides cannot be combined with CPU Path. Component
filtering requires capture even while denoising is temporarily disabled; clear
component filtering before disabling capture.

Sampling and photon experiments remain independently selectable:

| Control | Saved option under `job.options` | Condition |
| --- | --- | --- |
| **Adaptive → Sampling pattern → Scrambled Sobol** | `sampler: "scrambled_sobol"` | CPU requires progressive sampling |
| **Allocate samples by light-path component** | `adaptive.component_uncertainty: true` | Adaptive sampling enabled; CPU requires Path |
| **Photon maps for pilot assessment** | `adaptive.map_validation_batches` | Adaptive sampling and caustics enabled; 2/4/8 equal batches with at least two pilot observations per map |
| **Glass transport → Fresnel strata** | CPU `path.transmission_samples`; GPU `transmission_samples` | Choose 1, 2 or 4; lanes average into one observation |
| **Trace both glass branches** | CPU `path.split_first_interface`; GPU `split_first_interface` | Requires one Fresnel stratum; two complete weighted production lanes, one ordinary pilot lane |
| **Use camera paths for glossy caustics** | `caustics.glossy_continuation_max_roughness` | Optional finite [0,1] maximum mapped base roughness; only selected area/environment smooth-glass families |
| **Use Russian roulette for CPU paths** | `path.russian_roulette: true` | CPU Path |
| **New photon map for each pass** | `caustics.refresh_per_pass: true` | CPU caustics require Indirect lighting |
| **Reduce photon radius over successive passes** | `caustics.progressive_radius: true` | Requires map refresh; `radius_mm` is the initial radius |

The bounded photon probe is scripted rather than a new UI control. Set
`caustics.probe_pixel: [x, y]` and `caustics.probe_sample` in the same backend's
option location. Coordinates refer to the internal trace grid before supersample
downsampling; sample indices are zero-based production ordinals and exclude
pilots. The selected pixel may receive fewer observations under adaptive
allocation, so an unselected probe is distinct from a selected zero contribution.

## Validation and interpretation

The [measured validation record](render-glass-convergence-validation.md) retains
source and executable identities, actual image measurements and numerical findings.
The selected `42918fe5` candidate's completed full gate exited 1, with 14,687
passes, 28 failures and 68 ignored tests across 100 native binaries. Both side jobs
passed. The strict comparison retains 23 exact matches and five changed
panic-header locations. All five failures reproduced on bare selected main,
and a separately reviewed proof permits conditional image evidence without a
full-parent-baseline or passing-CI claim. The clear repeat, standalone web check
and software-raster controls capture completed. The public CPU receiver and
splitting profiles also completed, and their 320 raw masters were independently
decoded; the [recorded comparisons](render-glass-convergence-validation.md#public-cpu-receiver-and-splitting-comparisons-on-42918)
retain both improved and worsened configurations. Private GPU comparisons were
not performed at that checkpoint. Public receiver and splitting profiles execute
CPU `ImageTask`. GPU library tests, private GPU comparisons and the CPU/GPU clear
fixture have separate scopes.

The [completion evidence record](render-glass-convergence-validation.md#completion-evidence-and-publication-status)
keeps all a77 checks historical, including CI02's new failure, its passing exact
focused repair, the completed failed a77 CI03 and the earlier UI/clear/web receipts.
It also retains the transient UI test-helper mutation, exact restoration and
separate requalification. No historical result is relabeled for the new source.
Required evidence includes:

- Legacy sampler draws and default raw output preserved when optional features
  are disabled; diagnostic capture enabled/disabled gives the same transport.
- CPU/WGSL scrambled samples agree bit-for-bit and retain prefix/tile stability.
- Fresnel strata reproduce analytic fractions, pilots stay independent, counts
  refer to complete groups, and roulette preserves expected energy with correct
  refractive weighting.
- Components recombine for clear and rough glass, hard lights, photons and dim
  endpoints; path classification remains independent of the estimator.
- Component filters preserve branch edges, alpha and raw masters; unknown
  uncertainty and ambiguous guides keep conservative behavior.
- Bounded photon probes, progressive schedule normalization, memory admission,
  paging, preview, cancellation, saved setup and undo contracts.
- Device shader validation and real CPU/GPU rendering, in addition to numerical
  unit tests and the repository CI gate.

The original, public-safe benchmark is reproducible with:

```sh
cargo run --profile ci -p solidcraft-render --features raytrace-cpu \
  --example validate_glass_convergence -- NEW_OUTPUT_DIRECTORY smoke
```

The `quality` mode uses more pixels, seeds, photons and camera observations.
Both modes save PNGs, raw EXRs, exact settings, probe/component summaries,
independent-seed variation, integrated pixel radiance, quantiles and elapsed
time. The scenes compare a glass sphere over diffuse and polished receivers,
fixed and progressive radius, and increasing camera/map budgets. Outputs belong
outside the repository. A denser result from the same estimator is not an
independent reference for its bias.

The separate sampler/strata/roulette comparison uses:

```sh
cargo run --profile ci -p solidcraft-render --features raytrace-cpu \
  --example validate_glass_convergence -- NEW_OUTPUT_DIRECTORY sampling-quality
```

Use `sampling-smoke` for a bounded 16 by 16 check: two seeds, four observations
per configuration, four scene classes and seven configurations, without timing
calibration. The `sampling-quality` profile uses 32 by 32 images and eight
measurement seeds. Two separate calibration renders at 2 and 8 observations
choose each configuration's fixed production budget between 2 and 512
observations, targeting four seconds. Calibration uses disjoint seeds and only
timing information; its radiance does not select the production budget.

The four scenes cover clear glass over diffuse and polished receivers, rough
glass over a diffuse receiver, and dispersive glass over a diffuse receiver.
Configurations compare independent sampling, Sobol blocks, two/four Fresnel
strata, roulette, and selected combinations. Measurement order rotates by seed.
Fixed-radius photon maps refresh per observation in every configuration.
Production component capture and diagnostic probes are disabled.

Read the actual elapsed-time ratios and budget-bound flags alongside seed
variation and the variance-times-time proxy. Task setup and output encoding are
timed; subsequent file writes and analysis are excluded. This finite-seed,
shared-machine, CI-profile experiment does not establish an independent bias
reference, exact equal-time performance, rare-event tails, or GPU throughput.
The [completed prior-source sampling comparison](render-glass-convergence-validation.md#cpu-sampling-comparison-at-the-prior-source)
records all 28 quality groups and their actual timings, mean shifts and raw-image
statistics. Its results support scene-specific opt-in evaluation, not a universal
quality preset or an equal-time superiority claim.

The additional `receiver-smoke` / `receiver-quality` profiles compare the
exclusive glossy strategy with legacy gathering and the same camera estimator
with continuous-source photon ownership disabled. All retain the same geometric
normal policy. Positive continuation/reference pairs and unchanged diffuse/hard
negative controls are checked exactly on CPU. These are ownership oracles;
the camera estimator is not an independent physical-bias reference.

The `split-smoke` / `split-quality` profiles compare weighted splitting against
independent/Sobol sampling and two/four strata at equal requested camera-lane
budgets. Quality uses four scenes, six configurations and eight independent
replicates. Actual elapsed times and conservative admitted memory are recorded;
the latter is not measured peak RSS. Shared-hero dispersion is included.
The detailed [split contract](render-camera-sampling.md#bounded-weighted-interface-splitting)
explains prefix weighting, zero children, counts, cancellation and executor
capabilities. Neither profile assumes an equal-time result before measurement.

## Rough-transmission design

The [rough-transmission footprint design](render-rough-transmission-footprints.md)
fulfills Stage 5's requested design of sample-derived statistics. It defines
actual camera-prefix observations, rough-interface estimator exclusions,
complete-group and zero-event semantics, bounded CPU/GPU storage, paging,
conservative refusal and independent qualification. Production rough-transmission
capture and filtering are not implemented. Implementing and qualifying this
concrete design remains Stage 5 work, separate from the advanced research
candidates below.

## Scope and remaining qualification

The concrete diagnostic and adaptive omissions identified during review are now
implemented. The rough-transmission task has an
[implementation-ready design](render-rough-transmission-footprints.md); production
rough-transmission capture and filtering are not implemented. Their correspondence
and detail-preservation gates remain open. This distinction does not make every
original stage's acceptance criterion complete.

| Area | Delivered mechanism | Remaining qualification or implementation boundary |
| --- | --- | --- |
| Dependency | Integrated on pinned main `1ab3a5b1`, preserving upstream render, UI and shared look changes | The current integration record reports its own checks; earlier `42918fe5` results retain their original scope |
| Photon diagnosis | Local/camera rankings, query support and complete raw-pixel accounting | Interpret independent-seed OPUS measurements without claiming a converged reference |
| Glossy receivers | Optional exclusive area/environment continuation at selected mapped opaque base roughness | Independent energy/detail and rare-tail qualification; difficult hard-source families retain the existing estimator |
| Photon radius | Independent-map decreasing bandwidth with surface/volume normalization | Radius-dependent energy, detail and leakage qualification; measured variance can increase |
| Components/statistics | Six additive cells, beauty covariance, independent pilot/map assessment | No calibrated production confidence interval for map reuse, progressive bias or Sobol dependence |
| Reconstruction | Component guides, supported clear endpoints and bounded TIR routes; rough-footprint design | Production rough filtering and broader coverage/DOF/detail qualification |
| Sampling | Sobol, CPU strata/roulette, bounded CPU/GPU weighted splitting, component/map pilots and public comparison profiles | Equal-time error, rare tails and prior polished-case mean shifts remain qualification questions |

The glossy strategy and bounded splitting implement the concrete mechanisms
from stages 2 and 6. Their presence and ownership tests do not close the energy,
detail or equal-time gates. The earlier sampling experiment does not evaluate
these additions. The historical and selected-source clear repeats demonstrate
their stated supported domain and reference preservation. They do not establish
independent reconstruction-error or photon-bias bounds. These limits
prevent an all-stages-complete claim.

## Work that remains research

This implementation does not add a manifold solver, vertex connection and
merging, a continuous spectral representation, multiple microsurface scattering
for rough dielectrics, general emissive-mesh NEE, or new geometric boundary
admission. Those candidates remain described with their acceptance criteria in
the original plan. They require their own probability accounting and reference
comparisons; a checkbox or a noise filter would not implement them.

Unselected photon families retain their coverage and can still have rare large
contributions; selected camera continuations have their own sampling variance.
The diagnostics and benchmarks make these events measurable. Shrinking a radius
or changing receiver ownership is not a universal variance-reduction policy.
Future acceptance decisions must use measured energy, detail, independent-seed
behavior and elapsed time, rather than the apparent smoothness of one image.
