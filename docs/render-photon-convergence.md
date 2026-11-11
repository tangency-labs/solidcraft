# Progressive photon density and bounded probes

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

## Controls and compatibility

The saved caustic controls add four optional values:

| Control | Default | Contract |
| --- | --- | --- |
| `progressive_radius` | `false` | Requires `refresh_per_pass=true`; changes surface and volume kernel radii for each global map epoch |
| `glossy_continuation_max_roughness` | absent | Optional finite [0,1] maximum mapped and aged opaque base roughness; transfers the supported smooth-glass area/environment family exclusively to full BSDF continuation, retaining hard-source and rough/volume photon ownership |
| `probe_pixel` | absent | One `[x,y]` coordinate in the global internal trace grid, before output downsampling |
| `probe_sample` | `0` | One production sample ordinal; adaptive pilots are excluded |

The existing `radius_mm` is the initial kernel radius. The existing photon count
is the emitted-path count **per map**, including misses and paths that deposit
nothing. The original fixed-map and fixed-radius refresh modes remain available.

A probe is diagnostic work. It does not select a different random stream, change
a reservoir draw, clamp flux, modify a receiver BRDF, or filter the raw film.
The probe controls are intended for a small reproducible diagnostic run.
GPU requests cannot combine `probe_pixel` with the optional upstream
`clamp_indirect`: validation refuses that combination before rendering.
A post-clamp per-photon attribution is not implemented. Component capture remains
supported: where the upstream clamp applies, it uses beauty's aggregate scale.
Deferred optical handling remains unchanged.


## Estimator and radius schedule

Let `k = epoch + 1`, where epoch is the zero-based global photon-map epoch.
Every complete camera pass gets a freshly generated map. Tiles and native
worker batches share that map. Neither tile coordinates nor the final requested
sample budget enter the radius schedule.

For surface receivers:

```text
r_surface(k) = r_initial * k^(-1/6)
K_surface(x, r) = indicator(|x| <= r) / (pi * r^2)
```

For interior volume receivers:

```text
r_volume(k) = r_initial * k^(-1/7)
K_volume(x, r) = 3 * indicator(|x| <= r) / (4 * pi * r^3)
```

The existing volume event-density estimator additionally divides by the local
scattering coefficient. That conversion remains in the volume gather. The
surface receiver retains the geometric/shading-normal measure correction.

Each photon flux already includes inverse source and emission probabilities and
division by the **emitted** path budget. The one-receiver-per-path reservoir
multiplies its selected receiver by its candidate count. Each map's gathered
radiance is then normalized with that map's kernel radius. The film averages
these already normalized estimates:

```text
L_K = (1/K) * sum(k=1..K, L_map(k, r(k)))
L_K = L_(K-1) + (L_map(K, r(K)) - L_(K-1)) / K
```

Old radiance is never rescaled using a new radius. No second division by photon
count is introduced. This is progressive **batch density estimation**. It does
not maintain an SPPM visible point's accumulated flux and effective photon count.

The camera also samples the nonsingular parts of its path in every pass. The
camera and photon seeds stay in separate domains. Under the default refreshed-map
schedule, with `P` adaptive pilot passes, both backends use photon epochs
`0..P-1` for pilots and `P+j` for production
ordinal `j`. The first production pass rebuilds the map at epoch `P`; no pilot
map is reused in production. Existing refresh regressions check epoch 16 after
16 pilots on CPU and GPU.

The radius schedule advances during pilots too. The initial radius therefore
belongs to the first pilot map when adaptive allocation is enabled; production
starts with `radii_mm(P)`. A fixed finite pilot prefix does not change the
limiting argument below.

The optional independent-map assessment uses `adaptive.map_validation_batches`
equal pilot batches at a common initial radius. Those maps have separate
assessment identities. Production restores the canonical map sequence: epoch
`P+j` for refreshed maps, or epoch zero for a fixed map. Assessment maps never
enter the production mean. With assessment disabled, a fixed-map job continues
to condition both pilot allocation and camera statistics on its one map.

Under the independent-stream assumption, conditional on the pilot data and its
chosen production count `N`, the expected production image is the mean of the finite-radius expectations
`mu(P)..mu(P+N-1)`. The production map sequence is independent of the data that
selected `N`, but the selected budget changes the average finite-bandwidth bias.
This does not establish an unbiased estimate of exact transport or a calibrated
convergence bound.

### Why decreasing radii and averaging can converge

This argument is for a locally regular density with finite second moments,
independent maps and a fixed emitted-path count `m` per map. It is not a claim
about every singular light path or every shading-normal configuration.

For a symmetric kernel in dimension `d`, local bias is of order `r^2`.
The variance of one map is of order `1/(m*r^d)`. Set `r_k = r_0*k^-a`.
For the average of independent maps:

```text
bias(L_K) = O(K^-2a)
variance(L_K) = O(K^(-1 + a*d) / m)
```

Both vanish when `0 < a < 1/d`. Balancing squared bias with variance gives
`a = 1/(d+4)`: `1/6` on a surface and `1/7` in a volume. The corresponding
mean-square rates are `O(K^-2/3)` and `O(K^-4/7)`, subject to those assumptions.
These are asymptotic orders, not error bars or a finite-budget guarantee.

The independent-map approach has a published precedent in Knaus and Zwicker,
[Progressive Photon Mapping: A Probabilistic Approach](https://www.cs.jhu.edu/~misha/ReadingSeminar/Papers/Knaus11.pdf),
ACM TOG 30(3), 2011. Their formulation avoids local visible-point statistics and
extends to volume density estimation. The implementation here uses the explicit
power schedule derived above; it does not copy an external implementation or
claim to reproduce that paper's exact finite-iteration recurrence.

### What the observed camera scatter measures

The statistics film records one complete camera observation per production
sample, after averaging that sample's Fresnel strata. Its Welford accumulator
reports `M2 / (K * (K - 1))` when at least two observations exist. The storage and
work are bounded; the reported value is not an upper bound on rendering error.
A count below two means unknown scatter, even though the stored numeric value
is zero. Denoising treats that count explicitly.

For independent observations with unequal means `mu_k` and variances `v_k`,
the expected reported value is

```text
E[M2 / (K*(K-1))]
  = sum(v_k) / K^2 + sum((mu_k - mean(mu))^2) / (K*(K-1))
```

A changing photon radius can therefore add mean drift to the measured scatter.
That term does not estimate the image's remaining density bias. Reusing a fixed
map conditions the camera statistic on that map; photons shared by neighboring
pixels and observations within a scrambled sequence also introduce dependence.
A zero observed scatter can coexist with a biased map or a rare contribution
that has not been seen. These statistics guide reconstruction and diagnostics;
independent seed comparisons and a suitable reference are still needed to
assess actual image error.

An adaptive pilot chooses a fixed production allocation from its own samples.
It does not certify the subsequent radius schedule or discover every rare path.
A probe sample may be below the maximum admitted production budget but above
its selected pixel's smaller allocation. That is an unselected probe, not an
invalid request and not evidence of zero photon energy.

### Spatial indexing and precision

The spatial grid uses `r_volume`, the larger radius. All surface and volume
queries still visit 27 adjacent cells. Surface acceptance uses `r_surface`,
so the larger grid cannot increase the surface kernel's support.

Receiver identity, side, normal compatibility, plane-distance, source-minimum
distance and path-depth checks remain active. The radius change does not gather
through an incompatible nearby surface.

GPU cells retain the existing precision limit of 2^20 cells per scene axis.
A progressive epoch that exceeds that limit fails explicitly. It does not clamp
the radius and then claim continued asymptotic convergence. CPU cell coordinates
keep their bounded integer-domain checks. A finite machine and finite path depth
always limit the represented problem.

## Transport ownership

This work retains the existing source and light-path partition:

| Light-path family | Estimator |
| --- | --- |
| Directional/point/spot source, at least one admitted optical event, then opaque/rough/volume receiver | Photon density estimate |
| Enabled rectangle/environment source, one or more smooth closed-glass events, then opaque receiver outside the optional continuation policy | Photon density estimate; the reciprocal camera endpoint contribution is suppressed |
| The same continuous-source family at an opaque receiver selected by `glossy_continuation_max_roughness` | Existing full opaque BSDF continuation; matching smooth-glass photon records are excluded from that receiver's gather |
| Ordinary opaque direct illumination and admitted non-owned continuous paths | Existing direct-light and camera-path estimators |
| Unsupported optical or boundary configuration | Explicit existing refusal or documented unsupported case |

The photon reservoir samples eligible receivers uniformly **within one emitted
path**, then multiplies the selected receiver by the number of candidates. It
does not normalize by deposited photons. Source power weighting uses the actual
quantized source probability. Guided emission retains the complete proposal
density.

### Optional glossy receiver continuation

`glossy_continuation_max_roughness: Option<f64>` defaults to absent. A present
value must be finite in [0,1]. The shared pure policy in
`job/photon_receiver.rs`, mirrored in WGSL, compares the receiver's actual
mapped and aged **base roughness** with that maximum. It does not represent a
clear-coat lobe's separate width. The existing full opaque base/coat BSDF and
matched mixture PDF remain intact.

At a qualifying opaque receiver, only area/environment photons already tagged
with one or more smooth closed-glass events are skipped. At that same receiver,
camera continuation does not establish the reciprocal continuous-endpoint
suppression. This transfers the complete supported path family exclusively to
the existing camera estimator. Each subsequent opaque receiver establishes its
own decision; an earlier receiver's choice cannot leak into a later segment.

Hard point/directional/spot sources retain photon ownership. Rough dielectric
and HG volume gathers, ordinary opaque direct-light MIS and the separate
finite-disk Sun retain their previous behavior. A threshold with both continuous
source flags disabled is valid and has no effect. Source emission, reservoir
selection, guiding probabilities and map normalization remain unchanged.
Existing body/side/normal/radius/plane/distance support checks and finite-depth
bounds apply. The caustic admission contract continues to reject straight
closed-glass shadow transmission on body materials and face overrides.

This is an exclusive transfer between the two estimators, with no extra MIS
weight between a finite-radius density estimate and unsmoothed continuation.
The physical component can remain a caustic while its diagnostic attribution
moves from photon to other-estimator radiance. The bounded probe therefore
reports what the selected estimator actually contributed.

The GPU adds one optional f32 threshold word and header bit. Device storage and
prepaid upload each increase by four bytes; no extra buffer, pixel plane or
photon-record field is needed. Optional radius/source-CDF offsets include the
new word, while the existing probe and optical bounds retain their end-relative
addresses. The absent-policy layout and six-buffer photon-disabled shader remain
unchanged.

The UI offers an explicit enable checkbox and a [0,1] base-roughness limit.
Selecting it initially uses 0.35; no saved setup acquires that value automatically.
If continuous sources are later disabled, the authored value remains saved and
dormant. Sampling and allocation policies stay independently selectable.

A camera continuation estimator can still have high variance for small sources
or difficult refractive paths. This option does not add manifold connections or
vertex connection and merging. Its exact camera-reference regressions establish
ownership reciprocity, not an independent bound on photon-density bias. The
public `receiver-smoke` and `receiver-quality` profiles retain raw comparisons
for area/environment glossy and polished receivers plus diffuse and hard-source
negative controls. The dielectric nonmetal receivers use IOR 1.5 so their glossy
Fresnel response is actually present.

At a rough dielectric receiver, the component API distinguishes each gathered
photon's reflection or transmission hemisphere before summing. That distinction
does not alter the BSDF or the source/path ownership predicate.

## Probe output and exact units

The optional report contains two different scopes. The selected internal trace
pixel and production ordinal control detailed photon events. A separate bounded
aggregate follows every production observation of the final output pixel that
contains that trace pixel. At 2x supersampling this footprint has four siblings,
each with its own adaptive production denominator.

| Field | Scope and units |
| --- | --- |
| `pixel`, `sample`, `epoch`, `seed`, `radii_mm` | Selected internal pixel, production ordinal, actual photon-map identity, seed and kernel radii |
| `gathers`, `visited`, `eligible`, `contributing` | Receiver-query and event counts across all lanes of that selected sample |
| `local_radiance_sum`, `histogram`, `largest` | Receiver-local RGB events before camera throughput and lane averaging |
| `camera_radiance_sum`, `camera_histogram`, `largest_camera` | The same events after their RGB camera prefixes, still before lane averaging |
| `camera.selected_sample` | One complete production observation, after the configured Fresnel-lane average |
| `camera.trace_pixels` | Production sums, coverage counts, actual counts and planned counts for each sibling trace pixel |
| `camera.output` | Final photon/other-estimator means and authoritative raw RGBA after per-pixel normalization and supersample resolve |

Each retained event preserves the emitted-path and source identities, actual
source-selection probability, emission density and measure, light depth,
reservoir candidate count, receiver identity/kind, flux, BRDF/BTDF/phase response,
normal correction, kernel radius and receiver-local RGB. It also records:

- `camera_prefix` and `camera_radiance`, with the actual RGB spectral, Beer,
  scattering, branch-probability, refractive and roulette weights already present
  in the camera throughput.
- `stratum` and `fresnel_strata`, the zero-based lane and complete-group size.
- `camera_depth`, `camera_interfaces` and `camera_path`. The interface count
  includes explicit dielectric camera-interface events before the receiver;
  shadow connections and sharp-metal classification are excluded. An analytically
  collapsed smooth sheet is one represented transport event, not an inventory of
  its marginalized internal reflections. Rough sheets count their explicit walk.
- `receiver_roughness`, absent for a volume receiver; a zero-based `gather`
  identity; and that query's eligible and positive event counts.
- `support` and `query_effective_event_support`. For positive receiver-local
  luminances, inverse normalized weight concentration is
  `(sum(w / max(w)))^2 / sum((w / max(w))^2)`. The max-scaled accumulator avoids
  squaring a potentially large physical radiance. This is descriptive positive
  contribution-event support, not a count of independent photon samples and not
  a confidence interval.
- `final_output_radiance`, available after final production counts: the retained
  event's share of the raw output-pixel mean.

Emission density excludes the separately reported source-selection probability.
Its measure is explicit: disk area, solid angle, area times solid angle, or
launch-disk area times solid angle. Delta source dimensions are not assigned a
fictitious continuous PDF.

Both logarithmic histograms cover positive luminance in powers of two. The first
bin extends below 2^-15; the last includes values at least 2^15.
`quantile_bounds` brackets a receiver-local quantile using these edges, not an
invented exact quantile. The two top-eight lists rank events independently:
multiplying a previously truncated local list by a nonuniform RGB prefix could
miss the camera's dominant event.

### From a photon event to the final raw pixel

For receiver event `e` in lane `l` of production observation `j`, the recorded
camera event is `C[j,l,e] = T[j,l,e] * L_local[j,l,e]`, component by component.
This event has not yet received the Fresnel-group averaging weight.

A complete observation averages the actual delivered photon terms of its
`S = 1, 2 or 4` lanes once. All lanes execute, including an early termination
before any Fresnel choice; pilots are excluded. The photon and other-estimator
partitions are recorded beside the existing raw addition and never replace it.
GPU gathers retain their original reduction order and kernel placement.
Consequently the sum of individually normalized event records and the delivered
photon total can differ by floating-point rounding.

For sibling trace pixel `i`, let `N[i]` be its own production count:

```text
photon_mean[i] = sum(j=0..N[i]-1, photon_observation[i,j]) / N[i]
output_photon_mean = sum(i=0..q-1, photon_mean[i]) / q
retained_event_output_share = C / (S * N[selected_pixel] * q)
```

Here `q = supersampling^2`, either one or four. Counts are never pooled across
adaptive siblings. Uncovered observations contribute zero radiance and still
count in their own pixel's denominator. The same rules apply to other-estimator
and beauty sums. The output report retains recombination residuals and compares
the reconstructed sum with the actual raw film.

These RGB values are scene-linear and coverage-premultiplied, matching the raw
EXR input. They precede denoising, display transforms and straight-alpha
unpremultiplication. A photon-estimator contribution is separate from the
physical caustic component: an ordinary camera or direct-light estimator may
also evaluate a physically caustic path. No physical component cell is used as
a substitute for photon-estimator accounting.

### Availability, resources and module boundaries

`camera.selected_sample` exists only after a full selected observation.
`camera.output` exists only after complete final counts have been checked.
A requested sample may be below the admitted maximum but beyond its pixel's
adaptive allocation. Both backends then return a configured report with
`selected=false`, no selected-sample detail, and an available final output
aggregate. This does not mean zero photon energy. While CPU work is still
running, a query can be selected before its complete camera observation exists.

No full-frame diagnostic log is allocated. CPU reports, per-lane scratch and
selected-query scratch occupy fallibly prepaid optional slots, reused across
queries and lane replays. Native worker reports merge in global pass and pixel
order. The admission includes worker duplication and bounded transient report
copies. Probe-off continuations retain empty slots without report allocations.

GPU probe jobs use 24-word photon records instead of 16-word records and one
fixed 1024-word (4096-byte) tail. The tail contains 128 header/histogram words,
two lists of eight 40-word entries, and four 64-word trace-pixel slots. Readback
copies this tail only. Uploads, the small footprint configuration, staging and
bounded host report copies are admitted. The normal record layout is unchanged.

The shared `job/photon_accounting.rs` module owns lane/observation/footprint
normalization and validation; it has no scene or BSDF dependencies.
`job/photon_diagnostics.rs` owns bounded ranking and the optional report
collector. The CPU continuation records terms at the existing addition points.
GPU `photon_probe` owns its fixed tail and readback, including persistence across
deferred optical-light resolution and film pages. The small
`PhotonContributionSupport` numeric type is shared with pilot diagnostics;
pilot support does not require enabling the detailed user probe.

## Validation and limits

The regression suite checks the radius schedule, analytic bias/variance orders,
bounded top contributions, histogram merging, validation of probe coordinates,
and production identity. Actual CPU and GPU render tests compare probes on/off,
verify independent epoch maps and inspect source metadata. Native worker tests
check deterministic map and report merging. Focused accounting tests include
nonuniform RGB prefixes that reverse top-event ranking, multiple queries and
lanes, early termination, 2x supersampling with unequal production counts,
unreached selected samples, relative checks at dim radiance and malformed GPU
readbacks. A controlled GPU replay retains one emitted map and all hash links
while comparing probe-off/on raw bits. Runtime receipts belong in the PR;
the presence of these tests alone is not a claim that a particular device ran them.

The evaluated-snapshot [public receiver comparisons](render-glass-convergence-validation.md#current-public-evidence)
and [anonymized private diagnostics](render-glass-convergence-validation.md#current-private-diagnostics)
retain actual source, copied-executable and accounting evidence. The private
groups vary different sampling axes; their comparison does not establish a
variance-reduction effect. Complete photon-versus-other estimator accounting
is distinct from bounded photon-family lower bounds and from full physical
component attribution. Only explicitly inspected peaks support the recorded
diagnostic conclusions; general rare-tail and energy gates remain open.

The earlier `42918fe5` receiver results retain their own
[public comparison record](render-glass-convergence-validation.md#public-cpu-receiver-and-splitting-comparisons-on-42918).
Historical probe results retain their original source and executable identities.

Under the smooth-density, finite-variance and fixed-domain assumptions in the
derivation above, the independently refreshed decreasing-bandwidth estimator can
reduce asymptotic reconstruction bias. That argument does not establish monotonic
finite-budget improvement. Single-map noise can increase as the radius shrinks.
The mode does not promise a lower error at every intermediate budget, solve a
divergent measure correction,
or remove the need to measure polished receivers across seeds.

Changing a radius redistributes radiance near edges and sharp caustic features.
Assess integrated energy as well as pixel error. Keep raw linear output. Compare
several independent photon seeds and camera seeds at matched elapsed time.
Resolved glints and real high-intensity caustics must not be treated as disposable
outliers.

## Reproducible evidence tool

The `validate_glass_convergence` example renders an original closed glass sphere
above diffuse and polished receivers. It compares fixed-radius refresh with the
progressive schedule at matching sample and photon budgets, using independent
camera and photon seeds for each repetition:

```sh
cargo run --profile ci -p solidcraft-render --features raytrace-cpu \
  --example validate_glass_convergence -- NEW_OUTPUT_DIRECTORY smoke
```

The output directory must not already exist. `smoke` uses a 16 by 16 image, two
seeds, 4 and 16 camera samples, and 1,024 photons per map. `quality` uses a 64 by
64 image, four seeds, 8, 16 and 64 camera samples, and 16,384 photons per map.
Both modes run one CPU worker and retain raw EXR, PNG, settings, selected photon
probes, component reports and incremental JSON measurements.

The final report includes independent-seed pixel variance, integrated image
radiance, quantiles, bright-pixel counts and elapsed time. Its bright-pixel
threshold is diagnostic only. It does not clamp image values. Times include
setup and encoding, so shared-machine load must be recorded separately when
comparing performance. No independent reference is built into this example;
a smaller measured seed variance alone does not establish a smaller bias or
better reconstruction error.
