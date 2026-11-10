# Glass rendering: convergence findings and implementation plan

## Status and dependency

**Status: implementation in draft PR #13904, following merged [PR #12546][render-pr].**

PR #12546 merged on 9 November 2026 as
[ed2bafe7e312ff38f1d957a511d4c8e529dc4e32][merged-renderer].
The current implementation is integrated on
[1ab3a5b1796564bc76dc57a513115f521edd0e90][current-dependency].
This restack preserves main's shared `solidcraft-look` icons, fonts and themes.
Its [implementation record](render-glass-convergence-implementation.md) describes
the delivered modules, controls, resource contracts and remaining limits.
The [validation record](render-glass-convergence-validation.md#current-integration)
separates current checks from completed measurements of earlier source.

Implemented areas include bounded photon reports with camera/raw-output
accounting; six additive components and moments; independent map assessment;
progressive photon radii; clear-glass endpoint and bounded internal-reflection
guides; component filtering; scrambled camera sampling; CPU Fresnel strata and
roulette; exclusive supported glossy-receiver ownership; and bounded weighted
splitting at one eligible glass interface. These choices remain opt-in.

The rough-transmission footprint design is complete. Production rough-path
capture and filtering remain unimplemented Stage 5 work. Independent physical
error, general energy/detail preservation, equal-time improvements and rare-event
bounds remain qualification questions. The broader methods in section 11
remain research candidates. No all-stages-complete or noiseless-rendering claim
is made.

The source audit below was made on 8 November 2026. Its OPUS measurements retain
their original source. The implementation record takes precedence when identifying
delivered behavior; the original acceptance criteria remain here for review.

## 1. Goal and meaning of success

The practical goal is physically credible glass with no visible noise at the
intended output size, while preserving resolved reflections, transmitted detail
and caustics. A finite rendering budget cannot guarantee exact results for every
scene.

There are four different problems to measure:

| Problem | What it means | What can address it |
| --- | --- | --- |
| Sampling variance | Independent sample sets produce different estimates | Better sampling, more useful samples, appropriate estimators and residual denoising |
| Reconstruction bias | A finite photon radius or display filter changes the expected image | A justified progressive estimator, appropriate reconstruction and explicit quality limits |
| Missing or duplicated transport | A supported light-path family has no estimator, or contributes more than once | Correct path accounting and additional connection or merging methods |
| Optical or geometric approximation | The represented material or boundary differs from the intended object | Better material models or separately authorized geometry work |

A smooth image is not sufficient evidence of correct transport. A high-SPP image
using the same biased estimator is not automatically an independent reference.

The first target is the measured smooth/glossy photon-receiver problem in OPUS.
Progressive photon estimation, optical denoising and sampling efficiency are
separate milestones supporting that target.

## 2. Evidence and baseline

Use these distinctions in follow-up reports:

- **Verified source behavior:** established by inspection of the audited commit.
- **Recorded measurement:** an existing result with its original source and scope.
- **Hypothesis:** a possible explanation that still needs an isolating experiment.
- **Proposed implementation:** the original design; delivery status is recorded separately in the implementation document.
- **Acceptance criterion:** evidence needed before calling a stage complete.
- **Deferred research:** a candidate whose benefit or integration is not yet established.

The source baseline is the published PR head, not an experimental worktree or an
assumption about current main. See the [OPUS qualification record][opus],
[glass matrix][glass-matrix], [material models][glass-models] and
[denoising policy][denoise-doc].

### 2.1 Capabilities already present

| Area | Verified at the audited commit | Qualification or remaining limit |
| --- | --- | --- |
| Smooth glass | Exact unpolarized dielectric Fresnel, Snell refraction, total internal reflection and radiance IOR scaling | Finite depth, numerical precision and admitted geometry still apply |
| Absorption | RGB Beer attenuation over physical segment lengths | Absorption is not a surface-color overlay |
| Thin sheets | A distinct sheet model with thickness and slab response | A sheet is not interchangeable with a closed solid |
| Rough glass | Isotropic GGX visible-normal sampling with a matching evaluator and refraction Jacobian | Single microsurface scattering; no general multiple-scattering dielectric model |
| Dispersion | Abbe/Cauchy model and three representative RGB wavelengths | No continuous spectral transport or measured-glass spectral reconstruction |
| Thin film | A lossless single coherent dielectric coating | No general multilayer or absorbing coating stack |
| Optical media | Medium tracking and admitted nested configurations | Touching, intersecting and otherwise unsupported boundaries remain explicit limitations |
| Caustics | Finite-radius photon density estimates with receiver checks and bounded storage | Finite-radius bias and finite-photon variance |
| Photon emission | Optional optical-bounds guiding, source-power weighting and refresh per camera pass | These are already implemented; guiding is not guaranteed to improve every scene |
| Adaptive sampling | Independent pilots choose bounded production sample counts | Thresholds guide allocation; they are not certified convergence bounds |
| First Fresnel choice | GPU stratification with 1, 2 or 4 strata | A CPU counterpart was not established by this audit |
| Denoising | Spatial filtering with surface guides and a limited optical endpoint model | Many advanced optical cases bypass filtering |
| Diagnostics | Guide-bypass counts, categorical mask, presentation and export controls | Do not schedule these existing diagnostics as new features |
| Output and resources | Raw linear EXR preservation, expanded quality controls, CPU workers and GPU paging in #12546 | Recheck final merged limits and defaults before implementation |

The exact supported combinations matter. A saved control or a passing mathematical
test does not establish arbitrary scene support or device-level qualification.

### 2.2 Recorded OPUS outlier

The OPUS record attributes the largest investigated area-light spike as follows:

| Observation | Recorded value or outcome |
| --- | --- |
| Total luminance at the probed pixel | 4,920.72 |
| Smooth opaque photon-gather contribution | 4,919.75 |
| Same pixel with four times the photon budget | 1,230.53 |
| Independent photon seed | The spike disappears there and another appears elsewhere |

This is strong evidence that the investigated spike is dominated by the photon
estimate. It does not show that every bright pixel is erroneous. It also does not
identify a particular equation as defective.

The exact internal cause remains a hypothesis. Candidates include sparse useful
photons, unusually large photon flux, a narrow receiver BRDF, a large
shading-normal measure correction, or a combination.

Other low-sample OPUS comparisons retain large raw values. Some coherent
highlights persist. Display cleanup must therefore not assume that every intense
sample is disposable. See the original context in [the OPUS record][opus].

## 3. Current source map and concrete gaps

These links are pinned to the audited commit. They are implementation starting
points, not a guarantee that paths remain unchanged after #12546 merges.

| Concern | Current source |
| --- | --- |
| GPU photon generation, receiver reservoir and gather | [photons.wgsl][gpu-photons] and [photons.rs][gpu-photon-host] |
| CPU photon preparation, refresh, receiver handling and gather | [image_caustics.rs][cpu-photons] |
| Saved photon controls and work estimates | [job/caustics.rs][caustic-options] |
| GPU camera sampler and first-Fresnel stratification | [path.wgsl][gpu-path] |
| Optical direct-light requests and resolution | [glass_lighting.wgsl][gpu-glass-lighting] |
| GPU optical guide generation | [denoise_guides.wgsl][gpu-guides] |
| GPU spatial filter | [denoise.wgsl][gpu-denoise] |
| CPU optical guidance and filtering | [raytrace/denoise.rs][cpu-denoise] |
| Adaptive pilot and production allocation | [job/adaptive.rs][adaptive] and [film_adaptive.rs][adaptive-film] |
| Quality-control and diagnostic contracts | [render-quality-controls.md][quality-controls] |

### 3.1 Photon gathering

The opaque surface gather evaluates the receiver BRDF for each admitted photon.
Its surface kernel is a fixed disk normalized by its area, pi times radius
squared. Receiver identity, plane distance, normal and path-depth checks restrict
which photons contribute.

These checks do not ensure adequate angular sampling for a narrow glossy lobe.
The gather also applies the geometric/shading-normal measure correction. Instrument
that correction separately, especially near grazing directions.

Photon emission already supports a full-support source selection mixture and
optical-bounds guiding. Refresh can replace the map per camera pass. The first
priority is to evaluate and improve these existing systems, rather than introduce
duplicate controls.

### 3.2 Optical denoising

The current filter stores both reflected and transmitted endpoint geometry. It
still filters one combined radiance image. It multiplies compatibility weights
from both endpoints, which can reject useful taps when either branch contains an
edge.

The inspected optical-guide route bypasses:

- Rough, dispersive and thin-film glass.
- Nested media and selected-medium scattering.
- Unsupported additional glass bodies and some internal-reflection paths.
- Branches ending at emitters or the environment.
- Incompatible or unrepresentable endpoint correspondence.
- Depth-of-field cases and partial coverage.

The optical representation is bounded to captured body IDs 1 through 511.
Eligible optical pixels use the first two spatial scales. A third filter pass
does not extend their support.

The filter uses geometry and radiance contrast. It has no measured variance
input. Its deterministic center rays do not represent the full directional
distribution of rough transmission.

These are deliberate limits. Removing eligibility checks without a replacement
guide model would not implement support.

### 3.3 Sampling and adaptive allocation

The inspected GPU sampler is a PCG-style stateful pseudorandom generator. The
source audit did not find a Sobol/Owen low-discrepancy implementation in the
renderer paths examined.

GPU first-Fresnel stratification already conditions 1, 2 or 4 branch strata on the
same primary/prefix path, then decorrelates later streams. It is not deterministic
splitting of both branches at every glass boundary.

Adaptive pilots use RGB and coverage variance. The admitted pilot range is
16–256 samples, with 32 pilots and a minimum of 32 production samples by default.
The independent pilot chooses a fixed production budget; pilot radiance is
discarded from the production mean.

A short pilot can miss rare optical events. The current CPU production
accumulation does not continuously revise the pilot variance estimate.

## 4. Stage 0: reconcile the dependency baseline

The later user authorization permitted implementation before #12546 merged.
That merge and the exact source delta to the selected main revision are now
recorded. Restacking and qualifying the final implementation tree remain
separate from that source audit.

- [x] Record #12546's actual merge `ed2bafe7e312ff38f1d957a511d4c8e529dc4e32` and merge time.
- [x] Compare the premerge bc9 source with the merge and its direct child `a77b7ab2e463e41ae3ac27a05250765f0db0123d`; record all 17 changed paths and no overlap with this implementation's 106 paths.
- [ ] Qualify the final restacked main-based source with its own full gate and image receipts; retain the bc9 failures only as a known premerge comparison.
- [x] Check all open rendering PRs, issues and file ownership before claiming a stage; coordinate ownership within this draft.
- [x] Record effective saved, UI and runtime limits and compatibility defaults in the implementation and feature documents.
- [x] Add a deterministic public-safe sphere/receiver benchmark and reuse the existing glass/checker scene matrix.
- [x] Record CPU/GPU adapter, backend, driver, compiler and source identities in the [measured validation record](render-glass-convergence-validation.md).
- [x] Reproduce the original OPUS bright pixel, the public glass matrix and the photon-radius comparison, with explicit source identities and limits.
- [x] Continue in draft PR #13904 with explicit ownership of the photon, sampling and reconstruction modules.

The historical OPUS measurements remain historical evidence. Reproducing a new
baseline must not rewrite their source identity or promote them to new results.

## 5. Stage 1: explain the large photon contributions

Add bounded diagnostics for selected pixels or regions. Avoid full-frame
per-photon logging.

Record at least:

- Source identity and light-selection probability.
- Emission proposal and its full mixture density.
- Photon flux before the receiving material.
- Path class, interface count and receiver type.
- Number of eligible and contributing photons.
- Radius, local density and effective support.
- Receiver roughness and BRDF/BTDF response.
- Geometric/shading-normal measure correction.
- Largest individual contributions and contribution quantiles.
- Camera contribution, photon contribution and final raw value separately.
- Photon-map epoch, seed and relevant sample ordinals.

Use those measurements to distinguish statistical variance from invalid
arithmetic or transport accounting.

Audit flux normalization against emitted photons, including misses. Check light
selection, emission PDFs, receiver-reservoir weights and transport-mode IOR
factors. These are checks, not claims that the current implementation is wrong.

**Acceptance:** explain the dominant OPUS spikes across independent seeds.
An energy defect requires a minimal failing case and its correction. A high-
variance but valid estimate requires a better sampling strategy.

## 6. Stage 2: specify path ownership and glossy receivers

Write the estimator rules before changing receiver behavior.

A roughness-dependent policy must identify which estimator covers every affected
light path. Simply omitting a photon gather on polished surfaces can lose caustic
energy. Continuing the camera path may help, but can return to a difficult
refracted connection.

Evaluate these alternatives on the actual troublesome path family:

1. Continue narrow specular/glossy paths to a suitable later receiver where valid.
2. Use a connection strategy that accounts for the narrow receiving lobe.
3. Combine connection and merging strategies with justified probabilities and weights.
4. Retain the existing estimate where an alternative lacks coverage or is less efficient.

Define how each method interacts with direct-light sampling, environment and
rectangle endpoints, hard sources and the existing camera/photon partition.
If methods overlap, derive the weighting. If they partition paths, prove the
partition has neither gaps nor duplicate energy.

Keep receiver surface identity, side, normal, depth and leakage checks. A new
filtering radius must not gather light through nearby walls.

**Acceptance:** preserve integrated illumination on diffuse and polished
receivers, improve seed robustness, and retain caustics viewed through another
specular surface. A darker image caused by omitted paths fails this stage.

PBRT discusses why highly glossy receivers are difficult for photon gathering,
and why some ideal-specular configurations cannot be sampled by ordinary
incremental path construction [R1].

## 7. Stage 3: improve photon convergence

Distinguish three operations:

| Operation | What it changes | What it does not establish |
| --- | --- | --- |
| More photons in a fixed-radius map | Sampling density | Vanishing finite-radius bias |
| Independent map refresh | Dependence on one photon realization | A progressive-radius estimator |
| A justified progressive estimator | Radius, accumulated flux and normalization together | Exact finite-budget results or support outside its transport domain |

Investigate a progressive density estimator that fits the current GPU
architecture. Stochastic progressive photon mapping is a candidate [R2].
Adapting the existing forward-map gather and adopting a visible-point method
have different memory and scheduling costs.

The design must specify:

- Surface and volume kernels separately.
- Initial radius and its evolution.
- Coupled accumulated-flux and normalization updates.
- How existing per-map photon normalization and reservoir weights are converted.
- Behavior when batch size, source allocation or visible points vary.
- Treatment of photon-map independence and reused camera paths.
- Supported path classes and compatibility with finite path depth.
- Bounded batches, spatial indexing, cancellation and deterministic replay.
- Preview semantics while the radius and estimate evolve.

Do not assume that shrinking the radius while averaging old estimates with the
current weights is a valid progressive algorithm. Derive the update and test it
against analytic and independently checked cases.

Compare radius policies using integrated energy, spatial detail, leakage and
variation across seeds. A wider radius can look smooth while blurring a caustic.
A smaller radius can preserve detail while becoming too sparse.

Keep existing optical-bounds guiding and power weighting as selectable
comparators. Verify complete proposal densities and support. Optical-bounds
emission guiding is distinct from learned directional path guiding.

**Acceptance:** increasing the total budget improves the stated error measures
without unexplained systematic energy gain or loss, or lost paths. Measure
radius-dependent redistribution separately. Any consistency claim names its fixed
geometry, material model, path-depth limit and supported transport domain.

## 8. Stage 4: define additive components and uncertainty

### 8.1 Prevent double-counting in image components

Reflection, transmission and caustics are not automatically disjoint. A caustic
may be viewed through glass or in a mirror.

Define an exclusive additive path partition, or a two-axis decomposition such as:

| Camera-side classification | Non-caustic contribution | Caustic contribution |
| --- | --- | --- |
| Reflected | Reflected non-caustic radiance | Reflected caustic radiance |
| Transmitted | Transmitted non-caustic radiance | Transmitted caustic radiance |
| Other supported camera paths | Remaining non-caustic radiance | Remaining caustic radiance |

The implementation adopts this two-axis partition with six stored RGB cells.
Its precise multi-interface and estimator-independent semantics are documented
in the implementation record. Keep physical
path classification distinct from the estimator that sampled the path.

Accumulate and recombine components in the same scene-linear color space. Apply
transport weights exactly once. Before filtering, the components must sum to the
raw beauty image within a stated numerical tolerance. No component may be counted
twice or omitted. Define filtering, exposure and display-transform order explicitly;
independently tone-mapped components must not be summed.

### 8.2 Measure the right uncertainty

Separate:

- Variation from camera/path sampling.
- Variation between independent photon maps.
- Correlation from shared photon records and map reuse.
- Reconstruction error from the finite radius.
- Changes in the estimator as a progressive radius evolves.

Camera strata sharing one map are not independent photon observations.
Neighboring pixels can share photons. Conventional per-pixel sample variance
cannot certify all these sources of error.

Specify component moments, map/batch statistics and confidence diagnostics with
bounded storage. Consider independent map batches for assessment, with costs
reported separately from production work.

**Acceptance:** unfiltered component recombination preserves raw transport.
Statistics identify uncertain optical contributions without labeling a reused
sparse map as converged. Memory, paging, preview and cancellation tests cover all
new state.

## 9. Stage 5: extend optical denoising

Start with the existing portable Rust/WGSL filter.

1. Filter separately defined additive contributions using matching guides.
2. Use measured uncertainty to select filter strength and support.
3. Extend clear-glass guidance through longer valid specular chains.
4. Represent environment endpoints by direction and angular footprint.
5. Represent emitter endpoints with their appropriate identity and footprint.
6. Preserve reflected and transmitted edges independently.
7. Design sample-derived statistics or footprints for rough transmission.
8. Define explicit coverage and depth-of-field behavior.
9. Keep conservative fallback when correspondence is ambiguous.

The [bounded rough-transmission footprint design](render-rough-transmission-footprints.md)
specifies item 7's event contract, estimator coverage, normalized moments,
refusal policy, CPU/GPU resources and qualification gates. This completes the
design deliverable; production rough paths still bypass reconstruction until
that design is implemented and qualified. It does not complete this stage's
image-quality acceptance criteria.

A deterministic center ray cannot describe a broad rough-transmission lobe.
Relaxing the current bypass is not a sufficient implementation.

Retain raw linear output unchanged. Keep optional high-contrast suppression and
isolated-bright-pixel cleanup separate from transport. They may attenuate real
unresolved glints.

Open Image Denoise's guide recommendations are useful references [R3]. An
external comparison could help evaluate quality. Native integration is a
separate architecture/dependency decision, with browser fallback, licensing and
resource accounting. This plan does not select or add that dependency.

**Acceptance:** improve residual noise while retaining resolved glints, caustic
structure, transmitted texture and coverage. Compare raw components, unfiltered
recombination and filtered output. Existing bypass masks should accurately report
the newly supported domain.

## 10. Stage 6: improve sampling efficiency

### 10.1 Randomized low-discrepancy sampling

Investigate Owen-scrambled Sobol sampling as an explicit alternative to the
current sampler [R4].

Specify stable dimensions for pixel position, lens position, source choice,
light position/direction, microfacet normals, wavelength choice and continuation
events. Define event/branch indexing for variable-length paths. Avoid accidental
dimension reuse and correlations when different branches consume different
amounts of randomness.

Preserve reproducibility. Use independent randomizations for validation and
measure improvements at equal elapsed time. Samples within one scramble are
dependent. Do not treat the existing IID sample-variance formula as a calibrated
standard error without justification. Use independent scrambles or another
validated uncertainty model for adaptive allocation and convergence reporting.
Low-discrepancy sampling does not supply an estimator for a missing light-path family.

### 10.2 First-interface branching

Retain the existing GPU 1/2/4 first-Fresnel strata as the baseline. Evaluate
bounded reflection/transmission splitting at a selected early interface, with
the correct physical weights.

Do not recursively split every interface. Compare branch splitting against the
existing stratification using total path evaluations, time, memory and error.
Assess CPU parity as a separate integration task.

### 10.3 Optical adaptive allocation

Preserve the existing independent pilot/production separation.

Initial extensions should include conservative optical minimums, component
uncertainty, independent validation batches and photon-density diagnostics.
Report when pixels reach their budget while uncertainty remains high.

Production-dependent early stopping or reallocation needs a justified estimator.
Do not stop a pixel solely because its current samples happen to look quiet.

### 10.4 Russian roulette

CPU roulette removal does not prevent accurate glass. A future configurable
policy may improve efficiency if it preserves correct survival weighting and
accounts for refractive radiance scaling where relevant [R5].

Compare error at equal time. Report any changed default through the repository's
compatibility policy. Do not describe roulette as a denoiser or assume it reduces
variance per launched sample.

**Acceptance:** measurable efficiency improvement across several scene classes
and independent randomizations, with preserved expected energy and saved defaults.

## 11. Deferred transport and material research

These are later milestones. None is a prerequisite for documenting or isolating
the current OPUS problem.

### 11.1 Refracted light connections

| Candidate | Potential benefit | Limit or integration question |
| --- | --- | --- |
| Manifold next-event estimation [R6] | Solve selected refracted connections to sampled lights | Solver robustness, supported chains, multiple solutions and probability accounting |
| Specular manifold sampling [R7] | Sample more complex reflection/refraction chains and sharp caustic structures | Significant solver and PDF work; no universal noise-free guarantee |
| Bidirectional path tracing | Improve many indirect-lighting cases | Does not by itself cover every ideal-specular configuration |
| Vertex connection and merging [R8] | Combine connection and photon-merging strategies | A larger integrator, state and weighting project |

Begin with a bounded sphere/lens/OPUS-crop investigation. Record the supported
domain, solver failures, estimator bias where present, PDF validation and
equal-time results before choosing a production implementation.

General emissive-mesh sampling and matching light/BSDF probabilities remain
separate light-coverage work in the audited OPUS record. Revalidate that gap
against the merged source before adding it to an implementation stage.

### 11.2 Multiple scattering in rough glass

The current GGX model includes one microsurface scattering event. It can still
trace multiple interactions with actual geometric boundaries.

Investigate a matched multiple-scattering dielectric model [R9]. Validate energy,
reciprocity, evaluator/sampler agreement, the smooth limit and absorption. An
arbitrary brightness multiplier is not a substitute for that model.

### 11.3 Spectral transport

The current dispersion model uses representative wavelengths of 610, 550 and
460 nm. A continuous spectral mode needs wavelength sampling, spectral IOR and
absorption, light/material spectra and color matching into the output space.

Existing RGB-authored inputs do not determine unique spectra. Define their
spectral reconstruction policy and compatibility semantics explicitly. Measured
glass data also requires a licensed source and a supported parameter model.

Assess additional variance and cost. Spectral rendering improves optical
fidelity; it does not independently solve sparse photon estimates.

### 11.4 Geometry and broader media

The advanced optical/caustic route uses geometric facet normals. Coarse curved
boundaries can therefore limit the optical result even when visual shading is
smooth. Distinguish faceting from sampling noise in diagnostics.

The contribution rules at the original source audit paused kernel and
tessellation work. The current dependency instead vendors SolidCore under
`solidcore/` and directs geometry work there through its layer and ownership
rules. This rendering implementation changes neither the kernel nor tessellation.
Future geometric-quality work must follow the current
[contribution rules](../CONTRIBUTING.md) and
[layer contract](../solidcore/docs/layers.md); the historical pause is not a
present prohibition.

Touching liquids, overlapping media and unsupported nesting need defined boundary
semantics and robust handling. Do not replace explicit refusals with silent
boundary repair or invented gaps.

Multilayer/absorbing coatings, polarization, birefringence, heterogeneous interiors
and glass with external participating media are separate scope decisions. They
are not required for the first convergence milestone.

## 12. GPU architecture and resource contracts

Optimize useful quality per second on the actual adapter.

- Use bounded photon batches and an admitted spatial index.
- Keep camera and photon budgets distinct.
- Account for every component, moment, guide and progressive state buffer.
- Extend existing GPU paging with correct dependency halos and global coordinates.
- Include CPU scratch, upload staging and retained outputs in peak memory.
- Preserve cancellation, progressive previews and deterministic reproduction.
- Validate device storage-binding and dispatch limits independently of host RAM.
- Keep browser behavior explicit; native capability does not establish WebGPU support.
- Profile camera tracing, photon construction, gathering and filtering separately.

Expanded sample and resolution limits are useful controls, not a convergence
certificate. X2 supersampling improves spatial reconstruction but retains the
transport estimator's error. More camera samples cannot independently repair a
fixed sparse photon map.

Do not promise adapter speedups before measurements. A lower sample count can
take more time if each sample launches more paths or performs more expensive
gathers.

## 13. Validation matrix and measurement protocol

### 13.1 Small scenes

| Scene | Main purpose |
| --- | --- |
| Clear slab and background grid | Fresnel, refraction, transmitted brightness and guide detail |
| Absorbing slabs with different thicknesses | Beer attenuation and physical segment lengths |
| Glass sphere over a diffuse plane | Caustic energy, shape and photon-density behavior |
| The same sphere over a polished receiver | The current glossy-receiver risk |
| Caustic viewed through another glass body or mirror | Difficult specular-path coverage |
| Nested glass/liquid, inside camera and grazing interfaces | Medium tracking and total internal reflection |
| Rough, dispersive and coated glass | Advanced model behavior and denoiser support |
| Closely spaced surfaces and thin walls | Photon leakage and guide correspondence |
| Fine glints, glass edges and transmitted textures | Detail preservation and partial coverage |
| Fixed public-safe OPUS crops | Practical quality and adapter cost |

### 13.2 Protocol

For each relevant case:

1. Freeze the scene, camera, geometry, materials and exposure.
2. Record exact source, compiler, adapter/backend and settings.
3. Retain raw linear output and the corresponding filtered image.
4. Vary camera seeds and photon-map seeds independently.
5. Sweep camera samples, emitted photons, map count and radius policy separately.
6. Test depth convergence within the supported limit.
7. Measure integrated radiance over suitable regions as well as pixel error.
8. Include outlier counts/quantiles and seed distributions, not only favorable medians.
9. Compare image error at equal elapsed time and report peak memory.
10. Use analytic identities or independently justified references.
11. Compare CPU/GPU expectations statistically, without requiring identical random pixels.
12. Record unsupported cases and numerical/budget failures explicitly.

A radius change can redistribute light, so a per-pixel comparison alone can
misrepresent energy preservation. A biased high-SPP render using the same
algorithm is not automatically ground truth.

Start with small scenes and crops. Full-resolution OPUS is the final practical
qualification, after the isolated cases justify the cost. New renderer tests
belong in the area's existing test files, following repository guidance.

### 13.3 Required properties

- Unfiltered component sums reproduce raw beauty within a documented tolerance.
- Integrated radiance approaches the matched analytic or independent reference
  within stated sampling and reconstruction error, without unexplained systematic
  energy gain or loss. Measure radius-dependent redistribution separately.
- No supported path disappears during a glossy-receiver transition.
- Optional filtering leaves the raw master unchanged.
- Resolved highlights and caustic details survive filtering.
- Photon leakage does not cross incompatible receivers.
- Nonfinite values and ambiguous medium transitions remain visible failures.
- Budget exhaustion is not reported as successful convergence.
- Cancellation releases admitted resources and does not publish a false completed frame.
- Saved setups retain their established behavior unless an explicit preference changes it.

Numerical thresholds must be selected for the scene, scale and independent
reference. This planning document does not invent acceptance numbers without a
baseline.

## 14. Delivery sequence and open decisions

| Stage | Deliverable | Gate | Relative complexity |
| --- | --- | --- | --- |
| 0 | Reconcile #12546 after merge | Frozen merged baseline and ownership check | Small |
| 1 | Photon contribution diagnostics | Explain dominant spikes across seeds | Medium |
| 2 | Estimator ownership and glossy-receiver strategy | Complete supported-path coverage and energy preservation | High |
| 3 | Progressive photon estimator | Justified updates and improving measured convergence | High |
| 4 | Additive components and uncertainty buffers | Correct raw recombination and resource accounting | High |
| 5 | Broader optical denoising | Less noise with preserved detail and raw output | High |
| 6 | Sampling and adaptive improvements | Equal-time benefit without changed expectations | Medium to high |
| 7 | Selected transport/material research | Defined support, justified estimator and measured benefit | Research-dependent |

These are the original relative planning judgments, not calendar estimates.
The implementation record distinguishes delivered mechanisms, executed checks
and acceptance criteria that still require broader scene qualification.

After the baseline is established, contribution diagnostics and component
specification can proceed in parallel. Coordinate photon-estimator changes with
uncertainty statistics. Stages that change the same sampler, film or GPU buffer
layout need explicit ownership and sequential integration.

Open decisions:

- Which glossy-receiver strategy preserves the difficult path families efficiently?
- Which progressive formulation best fits existing photon storage and paging?
- Which additive component partition gives useful guides at an acceptable memory cost?
- How will independent photon-map uncertainty be assessed during production?
- Which clear-glass endpoint classes should be supported first by the denoiser?
- Does branch splitting improve error per second over the existing Fresnel strata?
- Which sampler changes should remain opt-in under the compatibility policy?
- Does a manifold connection method demonstrate enough benefit for production?
- When is spectral fidelity worth its added variance and authoring complexity?

Keep the initial implementation focused on the measured photon problem and the
data needed to assess it. Do not combine every deferred research item into one
unreviewable renderer change.

## 15. Handoff checklist

- [x] #12546 has merged; its actual merge, direct-child main revision and exact source delta are recorded.
- [x] The plan's dependency status reflects the merged source; the 17-path delta changes no contribution-rule file.
- [ ] The final main-based implementation has completed its own gate and image qualification.
- [x] Historical measurements remain labeled with their original provenance.
- [x] Related issues and open PRs have been checked before claiming implementation.
- [x] The implementation has a concrete scope, module ownership and published dependency baseline.
- [x] Path ownership and additive component semantics are defined and checked at complete-observation boundaries.
- [x] Compatibility defaults and saved-setting behavior are specified; all new choices are opt-in.
- [x] Validation commands, bounded scenes and work counts are recorded for the delivered mechanisms.
- [x] Actual implementation receipts are added only after execution and retained in the [measured validation record](render-glass-convergence-validation.md).
- [x] The PR remains draft until implementation review and qualification gates are satisfied.

## 16. References and licensing boundaries

References below describe mathematical methods or documented APIs. No external
implementation, model weights, figures or example assets are copied by this
planning document.

Follow the project's clean-room and license rules during implementation. Do not
read or incorporate GPL/LGPL/AGPL code. Any permitted code adaptation, dataset or
asset requires the repository's attribution and dependency process.

- **[R1]** Pharr, Jakob and Humphreys, *Physically Based Rendering*, third edition,
  [Stochastic Progressive Photon Mapping](https://pbr-book.org/3ed-2018/Light_Transport_III_Bidirectional_Methods/Stochastic_Progressive_Photon_Mapping).
  Photon reconstruction, glossy receivers and difficult specular-path coverage.
- **[R2]** Hachisuka and Jensen, *Stochastic Progressive Photon Mapping* (2009),
  [author publication page](https://graphics.ucsd.edu/~henrik/papers/sppm/).
  Progressive estimation is a mathematical design reference, not an imported implementation.
- **[R3]** Intel Open Image Denoise,
  [official documentation](https://www.openimagedenoise.org/documentation.html).
  Auxiliary features through specular paths and native API constraints.
- **[R4]** *Physically Based Rendering*, fourth edition,
  [Sobol' Samplers](https://pbr-book.org/4ed/Sampling_and_Reconstruction/Sobol_Samplers).
  Randomized low-discrepancy sampling and dimension management.
- **[R5]** *Physically Based Rendering*, fourth edition,
  [A Better Path Tracer](https://pbr-book.org/4ed/Light_Transport_I_Surface_Reflection/A_Better_Path_Tracer).
  Multiple importance sampling and refractive Russian-roulette considerations.
- **[R6]** Hanika, Droske and Fascione, *Manifold Next Event Estimation* (2015),
  [DOI](https://doi.org/10.1111/cgf.12681).
  A research candidate requiring a scoped implementation and probability derivation.
- **[R7]** Zeltner, Georgiev and Jakob,
  [Specular Manifold Sampling for Rendering High-Frequency Caustics and Glints](https://rgl.epfl.ch/publications/Zeltner2020Specular) (2020).
  Supported specular chains, variants and remaining limitations.
- **[R8]** Georgiev, Krivanek, Davidovic and Slusallek,
  [Light Transport Simulation with Vertex Connection and Merging](https://iliyan.com/publications/VertexMerging/) (2012).
  Connection/merging combinations and their weighting.
- **[R9]** Heitz, Hanika, d'Eon and Dachsbacher,
  [Multiple-Scattering Microfacet BSDFs with the Smith Model](https://eheitzresearch.wordpress.com/240-2/) (2016).
  A basis for further dielectric microsurface work.

[render-pr]: https://github.com/tangency-labs/solidcraft/pull/12546
[audited-commit]: https://github.com/tangency-labs/solidcraft/commit/66bc1b7de239d4b70feb41e6c489dedd24adff60
[current-dependency]: https://github.com/tangency-labs/solidcraft/commit/1ab3a5b1796564bc76dc57a513115f521edd0e90
[merged-renderer]: https://github.com/tangency-labs/solidcraft/commit/ed2bafe7e312ff38f1d957a511d4c8e529dc4e32
[opus]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/docs/render-opus-quality.md
[glass-matrix]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/docs/render-glass-complete.md
[glass-models]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/docs/render-glass-material-models.md
[denoise-doc]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/docs/render-denoise.md
[gpu-photons]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/gpu/src/path/photons.wgsl
[gpu-photon-host]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/gpu/src/path/photons.rs
[cpu-photons]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/render/src/raytrace/image_caustics.rs
[caustic-options]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/render/src/job/caustics.rs
[gpu-path]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/gpu/src/path/path.wgsl
[gpu-glass-lighting]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/gpu/src/path/glass_lighting.wgsl
[gpu-guides]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/gpu/src/path/denoise_guides.wgsl
[gpu-denoise]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/gpu/src/path/denoise.wgsl
[cpu-denoise]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/render/src/raytrace/denoise.rs
[adaptive]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/render/src/job/adaptive.rs
[adaptive-film]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/crates/render/src/film_adaptive.rs
[quality-controls]: https://github.com/tangency-labs/solidcraft/blob/66bc1b7de239d4b70feb41e6c489dedd24adff60/docs/render-quality-controls.md
