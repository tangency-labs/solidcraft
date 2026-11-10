# Camera sampling and bounded optical path groups

The implementation is integrated on pinned main
`1ab3a5b1796564bc76dc57a513115f521edd0e90`, preserving graphics backend recovery,
optional indirect-clamp behavior and the shared `solidcraft-look` assets.
The [current integration record](render-glass-convergence-validation.md#current-integration)
separates this source's qualification from the failed or interrupted `a3475683`
attempts. Earlier `42918fe5` results retain their original source and do not
qualify this newer integration.

The implementation provides an opt-in randomized low-discrepancy sampler,
independent adaptive pilots, a conservative optical allocation floor, CPU
first-Fresnel groups, bounded CPU/GPU splitting of one eligible interface,
optional eta-aware CPU Russian roulette, component-aware pilot allocation and
bounded independent photon-map assessment. These controls
change the distribution or amount of transport work. They do not guarantee a
noise-free image, remove a finite path-depth cutoff, or resolve bias from a fixed
photon reconstruction kernel. Photon-map convergence has its own controls and
[documentation](render-photon-convergence.md).

## Saved intent and compatibility

| Control | Location | Default | Meaning |
| --- | --- | --- | --- |
| `sampler` | CPU and GPU job options | `"independent"` | Existing independent stream or `"scrambled_sobol"` camera sampling |
| `optical_min_samples` | `adaptive` | `0` | Scene-wide production floor when captured dielectric media are present |
| `component_uncertainty` | `adaptive` | `false` | Include six physical component estimates in the independent-pilot allocation |
| `map_validation_batches` | `adaptive` | `0` | Use 2, 4, or 8 independently randomized photon maps during equal-sized pilot batches |
| `transmission_samples` | CPU `path`; existing GPU top-level field | `1` | One, two, or four lanes at the first sampled Fresnel decision |
| `split_first_interface` | CPU `path`; GPU top-level field | `false` | Two weighted complete lanes at the first eligible smooth closed interface; requires one Fresnel stratum |
| `russian_roulette` | CPU `path` | `false` | Probabilistic continuation after three path events, with refractive correction |

Absent new fields retain old values. Default sampler, zero optical floor, one CPU
stratum, disabled interface splitting and disabled CPU roulette are omitted when
serialized. A saved
`{"max_depth":8}` CPU path still describes the original depth-eight transport.
Unknown sampler names and unsupported stratum counts are rejected.

The CPU independent stream is the established 53-bit counter sampler; GPU
independent paths keep their existing PCG state and call order. The new shader
wrappers consume the same number of PCG values when disabled. Photon emission
and guide construction retain their own sampling policies.

CPU low-discrepancy sampling requires progressive camera sampling. CPU Fresnel
groups and roulette belong to the explicit `path` integrator. The legacy direct
producer does not acquire path continuations by enabling the sampler.

Example CPU options fragment:

```json
{
  "budget": "studio",
  "progressive": { "samples_per_pixel": 128, "seed": 81 },
  "path": {
    "max_depth": 16,
    "transmission_samples": 4,
    "russian_roulette": true
  },
  "sampler": "scrambled_sobol",
  "adaptive": {
    "pilot_samples": 32,
    "min_samples": 32,
    "optical_min_samples": 64,
    "relative_error": 0.05,
    "absolute_error": 0.002,
    "coverage_error": 0.01
  }
}
```

The Studio profile is explicit: 128 production observations plus 32 pilots exceed
the Compact profile's ordinary per-pixel sample limit. Profiles and image sizes
are never silently increased to fit a new option.

## Randomized two-dimensional Sobol blocks

`crates/render/src/sampling.rs` owns the pure Rust sampler;
`crates/gpu/src/path/sampling.wgsl` implements the corresponding integer
calculation. The public key is:

```text
(seed, pixel identity, production ordinal, semantic domain)
```

Each semantic domain obtains a separately keyed two-dimensional block. The first
two binary Sobol generators are constructed arithmetically, using Gray-coded
ordinals. Hash-generated nested binary permutations scramble each coordinate.
The leading 23 scrambled bits select a cell; its midpoint is
(2 * bits + 1) / 2^24. Every result is strictly inside (0, 1) and exactly
representable in GPU f32 and CPU f64. Using 24 randomized bits directly would
include exact zero; an inverse free-flight sample could then round back onto its
source boundary. The midpoint convention avoids that reproducible failure
without changing the independent samplers. CPU high words participate in the
key; GPU admission uses one-u32 pixel and domain identities.

The construction follows the binary-generator and nested-permutation principles
described in [PBRT's Sobol sampler chapter][sobol]. The implementation uses no
imported direction-number table or copied sampler implementation. A nested
permutation preserves the dyadic stratification of each two-dimensional block.
The generated permutation choices use a finite integer hash, rather than a
stored table of independently generated random choices.

This is a collection of separately scrambled 2D nets. It is not an optimized
high-dimensional Sobol sequence spanning every path variable. That distinction
matters when comparing complex paths with many interactions. It also makes the
cost visible: nested scrambling performs more integer work than the existing
independent generators. Prefer measured error at equal elapsed time over an
assumption that the same sample count is faster or more accurate.

### Stable domains

Primary pixel position, lens position, spectral selection, BSDF proposal, light
sampling, free flight, scattering, and roulette use named domains. Path domains
also encode bounce depth; source choices and repeated virtual sheet interfaces
include their own indices. A skipped event does not shift later events through a
mutable dimension counter.

| Decision | CPU path event | GPU path event |
| --- | --- | --- |
| Primary pixel | Separate camera domain `0` | Separate camera domain `0` |
| Lens pupil | Separate `0x4c454e53` domain | Separate `0x4c454e53` domain |
| Opaque BSDF proposal | `0x3000`, `0x3001` | `0x3000`, `0x3001` |
| Fresnel branch / rough normal | `0x4000`, `0x4001` | `0x4000`, `0x4001`; indexed helper domains for sheets |
| Interior free flight / phase | `0x5000`, `0x5001` | `0x5000`, `0x5001` |
| Exterior free flight / phase | `0x6000`, `0x6001` | `0x6000`, shared mutually exclusive phase domain |
| CPU roulette / existing GPU roulette | `0x8000` | `0x7000` |

These are implementation identities, not a promise that CPU and GPU rendered
images have identical random paths. Their event namespaces, material evaluation,
and floating-point precision differ. The sampler reference test compares the
integer Sobol calculation for identical keys, while renderer tests compare
transport invariants.

The production ordinal is independent of target sample count, preview cadence,
tile shape, and worker scheduling. Increasing a cap retains the earlier sample
prefix. Adaptive pilots have a distinct seed tag and zero-based pilot ordinals.
They always use the independent sampler, even when production uses Sobol.
Shader photon entry points do not enable the camera Sobol state.

## Adaptive allocation and its limits

Adaptive rendering retains two phases:

1. Trace a fixed number of independent pilot observations.
2. Freeze a production budget per pixel, discard pilot radiance, and render fresh
   production observations.

For each RGB channel, the allocation heuristic estimates the count needed for
the pilot sample variance at tolerance
`absolute_error + relative_error * pilot_mean`. Coverage has its own tolerance.
The largest request is clamped between the chosen minimum and production cap.
Noisy or invalid pilot moments request the cap.

`optical_min_samples` is either zero or an integer from 16 through the production
cap. For a captured scene containing dielectric inputs, the effective minimum
is `max(min_samples, optical_min_samples)`. The floor is deliberately scene-wide:
an optical path can cross a pixel that is not classified as glass at its primary
hit, and a pilot may miss a rare reflected or transmitted source. An empty pilot
is therefore not evidence that a low production budget is sufficient.

This policy is conservative allocation, not a confidence bound. A fixed pilot
can still miss rare events. Production variance can differ from the pilot
variance, and a grouped Fresnel observation has a different distribution from
its single pilot path. Randomized low-discrepancy samples are dependent within a
run. Pixels sharing photons also share reconstruction error.

### Component-aware pilots

`adaptive.component_uncertainty: true` records each of the six disjoint physical
radiance components during the independent pilot phase. It does not enable
`radiance_components` for production or allocate a production component film.
CPU use requires the explicit progressive path integrator; GPU path jobs use the
same nested `adaptive` field. Complete component observations must recombine to
beauty within the existing backend precision tolerance. Every absent event is a
zero component observation, not an omitted sample.

The allocation takes the maximum of the beauty/coverage request and all component
RGB requests. A component uses `absolute_error / 6 + relative_error * mean` as its
tolerance. Dividing the absolute allowance keeps the six contributions within a
conservative shared budget when their errors accumulate; it is an allocation
heuristic, not a simultaneous confidence guarantee. Exclusive components may
exchange energy while beauty stays constant, so using beauty variance alone can
miss an unstable transmitted or caustic contribution.

### Independent photon-map assessment

`adaptive.map_validation_batches` accepts zero, two, four, or eight. A nonzero
value requires caustics, equal-sized pilot batches, and at least two camera
observations per batch. It divides the existing `pilot_samples`; it does not add
hidden camera observations. The setting's historical field name says validation,
but these batches select the production budget and are therefore **assessment
data, not a held-out validation set**.

Every assessment map uses a distinct reserved epoch domain
`0x80000000 | batch_index`. The user seed remains unchanged. The actual GPU epoch
key is bijective for a fixed seed and event domain, separating these identities
from every admitted ordinary production epoch. CPU counter sampling uses the same
separation. The reserved random epoch is never a radius epoch or a progress count.
Finite pseudorandom generators still do not promise mathematical independence of
all possible individual variates.

All assessment maps use the radius of the first canonical production map: epoch
zero for fixed maps, or epoch `pilot_samples` for refreshed maps. At the pilot to
production boundary, both backends rebuild that canonical production map with its
original seed, epoch, radius and photon indices. No assessment map is reused as a
production map. With a fixed production allocation, assessment cannot change the
production random streams. CPU emission workers copy the complete map identity;
GPU map changes occur before the first tile of a camera pass and never between
Fresnel lanes.

For B independent maps and K camera observations per map, each pixel and recorded
channel retains the current batch mean/M2, the mean/M2 of complete batch means,
and the sum of within-batch M2. The descriptive estimates are:

```text
W = sum(within-map M2) / [B * (K - 1)]
V = M2(batch means) / (B - 1)
M = max(0, V - W / K)
```

W estimates conditional camera scatter at the reference radius. M estimates the
additional map-to-map scatter after subtracting the estimated camera contribution
to batch means. A fixed production map allocates camera observations using W;
more camera observations cannot reduce that map's persistent error. Independently
refreshed production maps use W+M for the allocation heuristic.

These estimates can be noisy with only two or a few maps. Truncating a negative
variance-component estimate at zero does not establish that map error is absent.
Assessment at a fixed radius does not measure finite-radius bias. If production
uses progressively smaller radii, its distribution and expectation evolve; the
assessment is not calibrated production uncertainty or a confidence bound for the
radius schedule. Single-lane pilots, grouped production lanes and Sobol production
also have different dependence structures.

Map work admission includes B assessment builds followed by one canonical fixed
production map or C refreshed production maps, where C is the production cap.
No initial canonical map is built just to discard it. The default zero setting
retains the original fixed/refreshed map schedule and random streams.

### Photon support and bounded storage

When either pilot evidence control is active, each actual receiver query records
positive accepted contribution events after receiver, side, support, source and
kernel checks. The summary scales weights by their maximum before accumulating
sums and squared sums. Its inverse weight concentration,
`sum(normalized_weight)^2 / sum(normalized_weight^2)`, is reported as effective
**contribution-event support** within a query. It is not an independent effective
photon count. Reused records, multiple receiver queries, and Fresnel lanes never
increase a claimed number of independent maps.

The report distinguishes camera observations with a photon query from those whose
queries found no positive contribution. No query and an empty query are different
missing-evidence cases; neither demonstrates convergence. The minimum observed
query support is descriptive and is not substituted into the sample allocator as
an IID count. The selected production-pixel probe has separate ownership and does
not control pilot collection.

Optional pilot planes are admitted before transport. CPU planes contain four f64
values; GPU planes contain four f32 values in the existing paged accumulation
binding. The maximum combined mode uses 38 CPU planes (1,216 bytes per trace pixel)
or 44 GPU planes (704 bytes per trace pixel, including six deferred component
scratch planes), in addition to the existing film. Component-only mode uses
17 CPU or 23 GPU planes; map-only mode uses eight on either backend. Metadata,
worker result slots and ordinary finishing scratch retain their own admission.
The existing two-plane GPU readback staging is reused for bounded report scans.
Cancellation and a failed work debit occur before a CPU observation transaction
is published; partial paths or partially completed batches never become final
production observations.

### Report interpretation

`SamplingReport` distinguishes the following counts:

- `production_samples`: complete camera observations actually rendered, summed
  over pixels.
- `primary_path_evaluations`: production lanes plus unstratified pilots.
- `transmission_strata`: lanes per production observation.
- `pixels_at_cap`: pixels allocated the maximum count.
- `budget_limited_pixels`: pixels whose pilot estimate exceeded the cap, or
  whose invalid moments forced a conservative capped allocation.
- Optional `pilot_evidence`: component cap pressure, independent map and batch
  counts, fixed assessment radii, estimated single-map scatter relative to the
  allocation tolerance, invalid evidence, and receiver-query support summaries.

A pixel can be at the cap without exceeding the estimated requirement. A zero
`budget_limited_pixels` value does not certify convergence. The report cannot
observe a path the pilot missed or infer photon-map bias from repeated camera
samples. `map_scatter_above_tolerance_pixels` compares the estimated single-map
scatter with the allocation tolerance; it is not a count of proven unconverged
production pixels. `invalid_evidence_pixels` marks unavailable estimates which
force the production cap. Deserializing older reports supplies zero for the added
budget flag and no optional pilot evidence.

The UI labels are **Allocate samples by light-path component** and **Photon maps
for pilot assessment**, in the shared Adaptive section. The map selector enables
only equal batch sizes compatible with the current pilot count; saved invalid
intent is retained for validation instead of silently changing the count. The
controls do not enable production component recording or select a quality preset.

Ordinary one-stratum CPU rendering keeps its previous metadata behavior.
Explicit multi-stratum CPU jobs produce a sampling report even without adaptive
allocation so their extra path work is visible.

## First-Fresnel groups on the CPU

The bounded helper in `raytrace/image_sampling.rs` holds only a group accumulator
and lane identity. The existing path continuation slot is reused; no ray queue,
recursive branching, or growing collection of paths is introduced.

For N in {1, 2, 4}, lane j maps the first Fresnel variate u to:

```text
u_j = (j + u) / N
```

Floating-point addition can round a valid variate onto a later stratum's upper
boundary. Both CPU and GPU cap the mapped coordinate at the immediately preceding
representable value of that exact boundary. This keeps each bin half-open,
including the final bin below one; the one-stratum case returns the original
draw unchanged. Endpoint regressions include the largest valid f32 and f64
variates. The correction applies only to the sampling coordinate.

All lanes share the camera, lens, spectral, and other prefix choices until this
decision. After it, each lane has a separately keyed continuation. The original
material estimator still evaluates the reflection or transmission branch; its
weights are not replaced by an ad hoc split. Averaging the N conditional
strata covers the original unit interval. Only the first decision is stratified;
later interfaces do not create additional lanes.

The complete lane radiances and all six additive radiance components are averaged
before publication. Film coverage, production count, component covariance, and
camera moments each receive **one observation**. A zero-contribution lane is
included. Every lane must agree on the shared primary coverage. Adaptive pilots
always use one lane, and photon maps refresh per camera pass, not per Fresnel
lane. A photon diagnostic retains its actual Fresnel lane identity.

Cancellation is checked between continuations and before committing a group.
A partial group cannot enter the film, camera moments, or a completed image.
Serial and parallel producers use the same grouping owner, so a worker's tile
assignment does not alter the result.

### Work admission

For P pixels, K pilots, C capped production observations, and N lanes, CPU
preflight charges the camera-path envelope:

```text
P * (K + N * C)
```

The prefix is retraced in each lane and is therefore counted each time. Arithmetic
is checked for overflow. Saved setup validation, runtime admission, engine
preflight, and UI profile hints use this work bound. Actual adaptive reports
replace P*C by the sum of the frozen per-pixel production counts. The existing
shared traversal ledger still limits real operations and observes cancellation.

## Bounded weighted interface splitting

`split_first_interface: true` is a separate opt-in mode on CPU and GPU. It
requires `transmission_samples: 1`; combining two different branch-allocation
rules is rejected before work. A custom GPU executor must explicitly advertise
first-interface splitting support. Support for Fresnel strata alone does not
authorize silently ignoring the split request.

Both production lanes replay the same camera, lens, spectral and path prefix.
The first **bare, smooth, closed** dielectric interface consumes the single split
opportunity. Thin sheets, rough interfaces and interfaces with an active thin
film keep their ordinary sampling and do not consume it. If an eligible interface
is never reached, the two identical-prefix paths are still counted as two complete
lanes. Admission does not assume that every pixel receives useful split work.

For the actual incident/transmitted indices and unpolarized Fresnel value F,
the reflection continuation receives a throughput multiplier of `2 * F`.
The transmission continuation receives `2 * (1-F) * (n_i/n_t)^2`. The
existing complete-group mean divides the two lane results by two, giving the
physical weighted sum. Radiance, component contributions and deferred optical
light accumulated **before** the split retain their original weight; only the
future continuation changes. A zero-probability child completes with the common
prefix and coverage. It is not discarded from the denominator. F=0, F=1 and
total internal reflection therefore retain the same two-lane accounting.

The lanes use distinct continuation random domains after the split. Shared-hero
RGB dispersion is supported conditional on the already selected wavelength;
this does not add spectral lanes or continuous-spectrum transport. Medium
transitions, Beer attenuation and finite-depth accounting apply to the actual
child. Optional CPU roulette retains its eta-aware correction; GPU termination
retains its existing policy. The split grants no extra depth
and never recursively branches at later interfaces.

Pilots remain one ordinary unsplit lane. Photon-map refresh remains once per
complete camera image pass, shared across pixels and never between sibling lanes. Beauty, six components, coverage, adaptive counts,
moments and photon-probe accounting commit only after the complete two-lane
group. Cancellation before that point discards the partial group. The existing
CPU continuation and GPU lane dispatch owners execute the paths serially; no
ray queue, new per-pixel plane or growing branch tree is allocated. Active GPU
external volumes and subsurface scattering refuse split mode, using the effective
two-lane count even though the authored stratum count remains one.

The shared `production_lanes()` contract returns two in split mode and the
validated stratum count otherwise. Work admission, GPU paging and dispatches,
CPU scheduling, reports, selected-pixel probes and saved setup validation use
that count. The sampling report keeps `transmission_strata: 1` and records the
separate split flag, so the two notions are not conflated. The path envelope above
uses N=2 for split production and one for each pilot. This is a conservative path
budget, not a count of actual traversal operations or a timing guarantee.

The UI preserves explicit intent across CPU/GPU method changes, save/reopen and
undo. It disables incompatible stratum choices while splitting is authored and
keeps an authored split available to turn off. Switching into a CPU setup without
an admitted path integrator uses the existing explicit method-conversion choice;
painting a control does not silently enable indirect lighting.

## Eta-aware CPU Russian roulette

Optional CPU roulette runs once before tracing a new segment after at least three
path events. It uses a dedicated semantic variate and does not consume the
material or source dimensions. The authored maximum depth continues to bound the
path; roulette does not turn that cutoff into an infinite-depth estimator.

The implementation keeps physical throughput beta separate from an eta correction.
For a transmitted interface from n_i to n_t, the correction is multiplied by
(n_t/n_i)^2. This cancels the temporary radiance scaling for the survival
decision, including nested media and valid rough-sheet exits. Reflection does not
change the correction.

The survival probability is:

```text
p = clamp(max_component(beta) * eta_scale, 0.05, 1.0)
```

A rejected path stops. A surviving path divides its **future throughput** by p.
Already accumulated beauty and components are untouched. This preserves the
conditional expectation of subsequent contributions. Paths with corrected
throughput at least one are not randomly killed. PBRT discusses why refractive
scaling should be excluded from roulette decisions in its
[path-tracing chapter][roulette]; the local code uses the existing bounded CPU
continuation instead of importing that implementation.

CPU roulette is disabled by default. The pre-existing GPU roulette remains its
own renderer policy; enabling the CPU option does not modify GPU saved intent.

## Verification and remaining evaluation

The completed `42918fe5` CPU splitting comparisons and their finite-budget limits
are retained in the [public comparison record](render-glass-convergence-validation.md#public-cpu-receiver-and-splitting-comparisons-on-42918).
Qualification of pinned `1ab3a5b1` is tracked separately in the
[current integration record](render-glass-convergence-validation.md#current-integration).
The pre-restack focused results retain their original source; a suite name alone
does not establish that every case performed GPU device work.

The regression suite covers:

- Sobol generator columns, nested-scramble interval stratification, bounded
  variates, semantic key separation, and simple integration oracles.
- Rust/WGSL sampler equality for identical keys and preservation of disabled GPU
  PCG draws and state; a deterministic interior-scattering regression for the
  former zero sample at seed 453936, pixel 0, observation 43.
- Explicit optical floors, capped pilot requests, and report semantics.
- Exact finite-depth normal-incidence Fresnel fractions for one, two, and four
  CPU lanes with both samplers; one moment per group and independent pilots.
- CPU serial/parallel equality, quantum independence, partial-group cancellation,
  retained component sums, and resource refusal before allocation.
- Weighted first-interface split fractions, zero children, common prefixes,
  shared-hero dispersion, finite-depth/medium behavior and one group observation.
- Nested clear-glass roulette behavior and finite-depth Lambert cavity energy.
- Saved defaults, CPU/GPU setup roundtrips, undo, reversible UI controls, and
  profile hints that include the real lane count.
- Component pilot recombination, zero observations, failed transaction rollback,
  and component variation hidden by a constant beauty sum.
- Fixed versus refreshed map allocation, assessment identities and radii,
  canonical production restoration, and photon-build work accounting.
- CPU worker equality, GPU deferred optical scratch and paging, scalar allocation
  agreement, extreme-weight query support, and invalid or missing evidence.
- Strict quality-preset schemas and saved controls preserve the optional pilot
  policy without enabling production component capture.

These tests establish contracts and bounded numerical examples. They do not
establish a universal quality gain. Scene evaluation should retain raw linear
results, render several independent seeds, include both fixed-sample and
equal-time comparisons, and inspect reflected, transmitted, and caustic regions
separately. Photon-radius and photon-count changes belong in that comparison.
Camera-sample scatter supplied to filtering remains a display diagnostic rather
than a calibrated error interval for a Sobol run or shared photon map.

The [completed CPU sampling comparison](render-glass-convergence-validation.md#cpu-sampling-comparison-at-the-prior-source)
records four scene classes, all seven configurations, eight quality seeds and the
actual selected budgets and elapsed times. Sobol's measured benefit is scene and
budget dependent; two/four independent Fresnel lanes did not improve the measured
variance-times-time proxy in those four scenes. Unequal elapsed times and the
polished-case mean shifts leave equal-time and expected-energy qualification open.
The comparison used the prior `84bda659` source. It does not qualify the completion
source, GPU throughput or the optional component/map adaptive policy.

The newer public `split-smoke` and `split-quality` profiles compare independent,
Sobol, two/four strata and weighted splitting under a matched **requested lane
budget**. Four scene classes include area and environment sources and shared-hero
dispersion. The quality profile requests 32 lanes per pixel with eight independent
replicates: 32 one-lane, 16 two-lane or 8 four-lane complete observations.
It records actual durations, raw means, seed variation and admitted memory.
Equal requested lanes are not equal traversal work or elapsed time; timing and
mean-shift results must be reported before drawing a performance conclusion.

[sobol]: https://pbr-book.org/4ed/Sampling_and_Reconstruction/Sobol_Samplers
[roulette]: https://pbr-book.org/4ed/Light_Transport_I_Surface_Reflection/A_Better_Path_Tracer
