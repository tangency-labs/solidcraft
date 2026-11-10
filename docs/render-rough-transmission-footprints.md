# Rough-transmission footprints: implementation design

## Status and purpose

This is the implementation-ready design requested by Stage 5, item 7 of
[the glass convergence plan](render-glass-convergence-plan.md). It defines a
bounded first implementation and its acceptance tests. It does **not** enable
rough-transmission filtering. The capture and reconstruction described here are
not implemented; production still uses the conservative bypass described in
[denoising](render-denoise.md). The [current integration record](render-glass-convergence-validation.md#current-integration)
does not change this design's implementation status or supply its missing
production qualification.

The evaluated renderer snapshot was integrated on pinned main
`1ab3a5b1796564bc76dc57a513115f521edd0e90`, preserving the shared look and asset
setup. That source restack does not implement this design. The
[current integration record](render-glass-convergence-validation.md#current-integration)
records the earlier evaluated-snapshot checks separately from the failed or
interrupted `a3475683` attempts and earlier `42918fe5` measurements. Those
checks do not implement or qualify the proposed rough-path capture. The
[publication qualification](render-glass-convergence-validation.md#publication-and-evaluated-snapshot)
also separates those evaluated-snapshot measurements from the later
`c47c2a676` publication on parent `201de37b9`.

The design estimates the distribution of actual sampled transmission endpoints.
A centre ray supplies neither that distribution nor evidence that a broad lobe
has one correspondence. Low radiance variance does not supply the missing
geometry. The design therefore adds explicit path observations and refuses
incompatible paths before using a footprint.

This is ordinary remaining implementation and qualification work within Stage 5.
It is separate from the manifold, spectral and other advanced methods in
section 11 of the plan.

## First supported domain

The first implementation filters only the two **Transmitted** components.
Reflected and Other components retain their existing guide policy.

An eligible camera prefix has these properties:

- A pinhole camera starts outside one certified, non-nested optical body.
- The primary hit is on one identified flat interface with a rough dielectric
  model. Its material is constant over that interface. Normal mapping,
  dispersion, thin films, coatings and scattering are absent.
- The path crosses that body and exits after exactly two transmission events.
  A thin sheet uses its existing two-interface model and a distinct sheet tag.
  Internal reflections, another optical body and unresolved exits are refused.
- The next hit is one finite opaque receiver triangle with a stable tangent
  frame. The initial domain excludes sharp mirrors, further optics, analytic
  emitters and environment endpoints.
- The trace-grid pixel has complete optical coverage: every completed
  observation identifies the same primary optical interface. A mixture of
  glass and opaque primary hits is partial optical coverage even when the
  beauty alpha happens to be one.

The camera depth must reach the receiver. The implementation observes existing
transport; it never extends the depth, changes termination, introduces a sample
or redirects a ray to obtain a guide. A missing endpoint is unavailable data.

This deliberately small domain permits real rough transmission onto a textured
opaque plane. It does not imply support for every rough glass scene. Exact
triangle identities can reduce support across coplanar tessellation seams.
The initial release must show useful eligible coverage on a public example;
a design that bypasses every representative rough pixel has not demonstrated
the intended improvement.

A proposed, default-false **rough_transmission_guides** setting requires
component filtering and captured components. It is not an existing saved option.
The existing optical_filter=false preference must bypass the result. No existing
saved option silently acquires the proposed rough behavior.

## Observe the camera prefix, not the final ray

One stored radiance component can contain several light connections, photon
gathers and continuation contributions. The final ray endpoint is not a guide
for all those contributions.

A separate prefix tracker follows the camera from the first rough interface
to the first qualifying opaque receiver. Once resolved, that receiver remains
the prefix endpoint for later lighting on the path. A later diffuse bounce,
light connection or photon gather does not replace it with a final light.

The tracker exposes geometry events, without owning transport or sampling:

    begin_primary(primary, material, coverage)
    optical_boundary(body, local_triangle, entering, crosses, sheet)
    resolve_receiver(receiver, point, normal, albedo, pixel_footprint)
    note_contribution(camera_class, light_class)
    finish_lane()
    finish_group()

These are proposed interfaces. Each event contains captured identities and
validated geometry, rather than references to a UI, GPU object or filter task.
The tracker cannot mutate throughput, radiance, random state or medium state.

The transition rules are explicit:

| Event | Tracker action |
| --- | --- |
| The first branch reflects | No transmitted endpoint observation; keep the existing reflected policy. |
| Two qualifying transmission events reach the receiver | Record one transmitted-prefix endpoint for that lane. |
| A second body, internal reflection, ambiguous edge, unsupported material or incomplete exit occurs | Refuse that transmitted prefix. |
| A regular receiver has been resolved | Freeze its identity and geometry for later contributions. |
| A Transmitted contribution has no compatible resolved prefix | Mark that light-side component ineligible. |
| A complete Fresnel group finishes | Publish geometry statistics and refusal flags together. |
| A lane/group is cancelled | Publish neither its geometry nor its production statistics. |

### Cover every estimator

CPU hooks belong beside the existing physical classification in
raytrace/image_indirect.rs, image_glass_model.rs and image_glass_lighting.rs.
GPU hooks belong beside radiance_record_class, radiance_optical_record and the
corresponding optical continuation in path.wgsl.

The following cases must not be assigned an opaque endpoint from an unrelated
continuation:

- A hard-light connection evaluated at a rough interface before its sampled
  continuation.
- A transmitted photon gather received at that interface.
- A light or environment endpoint reached before the qualifying opaque receiver.
- A shadow connection whose optical prefix differs from the recorded one.

Those cases retain their original radiance. They set a refusal bit for the
affected Transmitted/NonCaustic or Transmitted/Caustic cell. Geometry that is
incompatible with both cells refuses both. Supported photon gathering at an
already resolved opaque receiver may use that camera prefix; the footprint
does not claim to measure uncertainty in the photon map.

A contribution hook receives its existing physical cell label. It does not
infer a cell from the estimator name and does not apply a second transport
weight. The current six-cell raw partition remains unchanged.

The first implementation uses whole-cell refusal. It must not discard a small
unsupported contribution or filter only a guessed dominant path family. A later
increase in coverage would require an explicit supported/residual radiance
partition, including zero observations and a recombination check. That is a
separate implementation extension, not a permission to weaken refusal.

## Exact identity and refusal

A footprint key contains the primary optical body and primitive, the complete
entry/exit route, the receiver primitive, and the sheet/closed distinction.
Identities are exact captured ordinals within an immutable job. No hash or
quantized-normal comparison stands in for identity.

The existing bounded body-local route representation can encode the two boundary
events. Preserve its checked range and ambiguity rules. Out-of-range metadata
refuses guidance; it does not reduce the transport scene's admitted domain.

The first complete eligible endpoint establishes a key without consulting its
brightness. If another observation has a different primary, route or receiver,
the pixel becomes incompatible. This refusal is sticky for the job:

- Do not keep only the first route's samples.
- Do not replace the key with whichever route recently had more energy.
- Do not reset history at a preview or photon-map boundary.
- Do not average foreground, background, thin-sheet and closed-body keys.

Record separate reasons for insufficient observations, incomplete optical
coverage, incompatible route, mixed endpoint, unsupported contribution,
unsupported lens/material/medium, and invalid numerical geometry.

Exact identity rejects discrete route and receiver modes. Moments alone cannot
detect every spatially multimodal distribution on the same triangle or certify
unobserved tails. The filter remains a qualified display approximation. Such
limitations must remain explicit in diagnostics and image-quality evidence.

## Geometry statistics and sample semantics

Radiance means and M2 retain their current complete-observation semantics.
Every completed camera observation contributes to all six cells, including
zeros. The footprint does not change that denominator.

Geometry has a different measure. A non-transmitted lane has no endpoint; do
not insert a zero position into the footprint. A transmitted path that reaches
the receiver supplies geometry even when its eventual radiance is zero.

For N complete production lanes, each eligible endpoint draw has geometry
weight 1/N. Use the actual production_lanes contract for either Fresnel strata
or the optional bounded interface split; the authored stratum count alone is
not sufficient. Merge all lanes' geometry into one pending group, then commit
that group once. A zero-mass child terminating before a qualifying endpoint
contributes no geometry. If the endpoint was already resolved in the shared
prefix, retain that observation with its ordinary lane weight even if a later
suffix is zero. Track both complete observation groups and endpoint draws.
Correlated lanes, including duplicate prefixes when
no eligible split is reached, do not become independent confidence observations.
A forced child that produces an unsupported internal reflection or another
optical route before prefix-endpoint resolution still activates the existing
whole-cell refusal. Later lighting retains the already resolved receiver, as
defined by the prefix tracker above. Splitting supplies
no additional correspondence certification or exception to the first domain.

Use normalized weighted endpoint moments in a deterministic tangent frame of
the receiver triangle:

    W' = W + w
    delta = x - mean
    mean' = mean + (w / W') * delta
    M2' = M2 + w * outer(delta, x - mean')
    empirical_covariance = M2 / W

Here x has two tangent-plane coordinates. Geometry weights are sampling weights,
not luminance, throughput, Fresnel energy, photon flux or filtered radiance.
The covariance describes the observed endpoint distribution. It is not
M2/(n*(n-1)), a variance of the mean, or an error bound.

Retain the endpoint bounding rectangle, albedo mean and scatter, minimum normal
agreement and primary coverage/refusal state. These describe observed support
and provide rejection checks; a bounding rectangle is not proof of a uniformly
filled footprint.

Use at least eight complete groups with eligible endpoint observations before
enabling the first prototype. Also require eight endpoint draws and finite,
nondegenerate normalized statistics. These are conservative operating thresholds
to qualify, not a calibrated confidence level.

Fresnel branches, Sobol observations and reused photons retain their existing
dependence. Footprint statistics must not feed adaptive stopping or be reported
as photon-map convergence evidence. Independent adaptive pilots remain separate
from this production-only display metadata.

## Reconstruction rule

The existing 96-byte Guide remains unchanged. A rough display record supplies
one transmitted SurfaceGuide plus a separate footprint descriptor. The descriptor
does not create a reflected guide.

For each candidate tap:

1. Require active component reconstruction, optical filtering, a Transmitted
   cell, enough completed production observations and no refusal for that cell.
2. Check exact primary, route and receiver identity before component projection.
3. Check full optical coverage, unchanged alpha, compatible normals and material
   statistics. Keep all existing unrelated-path and invalid-value safeguards.
4. Compare endpoint distributions in the common receiver tangent frame.
5. Use the cell's existing radiance scatter for contrast and blend strength.

A concrete first distribution weight uses a positive covariance floor derived
from the projected trace-grid pixel footprint. Let A and B be the two empirical
covariances plus that floor, D their mean, and d their centroid difference:

    log_overlap =
        0.25 * log(det(A)) + 0.25 * log(det(B))
      - 0.5 * log(det(D)) - 0.125 * transpose(d) * inverse(D) * d
    overlap = exp(min(0, log_overlap))

This compares both footprint location and shape. It avoids treating a large
covariance as unconditional permission to blend distant samples. Check
positive definiteness and finite arithmetic before evaluating the expression.
Invalid covariance refuses the tap; adding a floor must not hide corrupt M2.

Keep the existing spatial support bounded. Initially apply rough reconstruction
only in the first 5-by-5, unit-step pass. Later passes copy that rough result.
Do not widen a spatial kernel merely because the optical lobe is broad. Do not
add an isolated-bright-pixel cap to rough optical guidance. Existing optional
display suppression must remain separately identified.

Geometry scatter and radiance scatter serve different purposes. Large endpoint
spread does not assert large radiance error; low radiance scatter does not assert
a reliable geometric correspondence. A Gaussian overlap is an approximation
even when moments agree. The image tests below decide whether it is useful.

Raw beauty, all six unfiltered components, their moments and the raw EXR remain
immutable. Recombination occurs in scene-linear space before presentation.
The beauty alpha remains the sole coverage value.

## CPU/GPU ownership and interfaces

A new shared renderer module, for example optical_footprint.rs, owns keys,
transition rules, normalized statistics, numerical checks and record sizes.
It must not import a CPU tracer, wgpu, output encoder or UI module.

| Owner | Proposed responsibility |
| --- | --- |
| CPU transport State | Optional current-prefix tracker; hooks observe existing events. |
| CPU FresnelGroup | Merge lane geometry without publishing a partial observation. |
| TransportContext / worker result | Carry one complete optional footprint observation alongside existing completed components. |
| Optional FootprintFilm | Accumulate only complete production groups; own sticky refusal and per-cell eligibility. |
| CPU output snapshot | Resolve receiver frames while the prepared scene is available; provide immutable rough display records. |
| ComponentDenoiseTask / DenoiseTask | Select the rough record only for the relevant transmitted cell; preserve ordinary guides elsewhere. |
| GPU optical continuation | Save/load the optional tracker across the existing wavefront stages. |
| GPU radiance_observe | Commit pending footprint statistics only after the last Fresnel lane. |
| GPU denoise window preparation | Gather committed records from every intersecting film page, normalize and produce one bounded descriptor window. |
| GPU component filter | Read one optional descriptor binding; keep raw accumulation and pending data read-only during reconstruction. |

Use the full component index where per-cell refusal is checked. The present
camera-only projection index is insufficient to distinguish the two transmitted
light classes. Neither backend may erase a route or refusal flag before this
check.

CPU output preparation remains resumable and cancellable. It must not perform
an uncharged full-frame conversion inside a constructor. CPU worker and task
size guards still apply; optional owners must not inflate the default task
beyond their admitted size.

## Explicit resource budget

The following is a proposed fixed layout, not a measurement of existing code.
Use checked arithmetic and compile-time size assertions when implementing it.

The committed accumulator is eight 16-byte lanes, **128 bytes per trace pixel**:

| Lane | Payload |
| --- | --- |
| 0 | Four exact key/route words. |
| 1 | Flags, completed-group count, eligible-group count, endpoint-draw count. |
| 2 | Geometry weight sum, two-coordinate mean, projected footprint floor. |
| 3 | Three independent centred covariance entries and minimum normal agreement. |
| 4 | Two-coordinate minimum and maximum observed bounds. |
| 5 | Three-channel albedo mean and maximum observed albedo deviation. |
| 6 | Three-channel centred albedo moments and geometric ambiguity margin. |
| 7 | Per-cell refusal mask and bounded transition/coverage bookkeeping. |

A normalized CPU display pixel also uses 128 bytes: a 48-byte SurfaceGuide
and an 80-byte footprint descriptor. It is a distinct type, without unsafe
reinterpretation. The descriptor has exact keys, covariance/floor, counts/refusal,
bounds, and albedo-scatter/normal data.

CPU peak admission includes the 128-byte committed film and a 128-byte immutable
output snapshot. Charge the actual optional tracker, pending-group and worker
result sizes separately. Snapshot conversion visits are charged and cancellable.
Do not assume a final-frame ownership transfer also removes preview costs.

GPU film admission reserves:

- Eight vec4 planes for committed statistics: 128 bytes per pixel.
- Eight vec4 planes for the incomplete Fresnel group: 128 bytes per pixel.
- Four vec4 planes for continuation metadata: 64 bytes per pixel.

That is **20 extra planes, 320 bytes per trace pixel**. It is deliberately
separate from existing radiance/component planes. Pending data cannot substitute
for committed data in previews. With full component capture and optional pilot
evidence disabled, the proposed fixed and adaptive layouts would have 48 and 50
total vec4 planes respectively. Combined component/map pilot evidence adds up
to 44 more planes, bringing the proposed adaptive total to 94. These totals
describe the future rough-footprint layout; the existing pilot evidence remains
separately admitted by the current implementation.

The GPU normalizer can gather the committed record into two 64-byte-per-pixel
buffers, then produce an 80-byte-per-pixel descriptor window. The proposed
additional window scratch is therefore 208 bytes per window pixel. Each single
storage binding remains below the existing 96-byte guide-window bound. The
normalizer reads triangles and the two halves, and writes the guide/descriptor;
the filter needs one additional read-only descriptor binding. Both fit within
the existing six-storage-buffer minimum without assuming an eighth binding.

Prepare rough guides only when the component loop reaches Transmitted. Earlier
Other/Reflected passes retain their original guides. Clear-guide pixels are
unchanged. Rebuild the window's original guides before processing another
window or published pass.

Keep the existing full-filter halo calculation, even though the first rough
prototype uses only unit-step support. Gather footprint rows across page
boundaries by logical pixel address. Never bind every extra film plane as one
denoise input, and never size scratch from the total accumulation-plane count.

At 3840 by 2160, the proposed GPU film addition alone is 2,654,208,000 bytes.
The CPU committed-plus-snapshot addition is 2,123,366,400 bytes. These exclude
existing film, scene, photons and filter resources. Admission must report and
enforce the real peak; it must not silently reduce quality or disable requested
capture to fit. The feature remains opt-in and may exceed a selected profile.

Metadata lanes require explicit integer encoding/decoding. Do not run radiance
arithmetic or finite-radiance guards over identity words. All counts and indices
must have checked ranges. Any proposed reuse of scratch or a smaller layout
requires its own lifetime and page-boundary proof.

## Required tests

Add tests to the existing area test modules rather than a per-bug test file.

- Independently computed weighted mean/covariance, parallel group merge,
  degenerate coordinates, high offsets, overflow, and invalid normal/albedo data.
- Complete 1/2/4 Fresnel groups, zero-radiance endpoints, absent transmitted
  events, partial-group cancellation and independent pilot/production counts.
- Exact entry/exit order, triangle identity through GPU BVH reordering, sheet
  identity, coplanar seams, mixed receiver bodies and incomplete optical coverage.
- A rough-interface NEE or photon gather refuses its affected cell instead of
  borrowing a later receiver. A gather at the resolved receiver retains its
  physical caustic classification and compatible camera prefix.
- A second observed route permanently refuses the pixel; previews and map
  refresh cannot reset that history.
- Symmetric footprint-overlap weights, dissimilar width/orientation penalties,
  invalid-covariance refusal and a bound on every blend weight.
- CPU/WGSL normalized-record and synthetic-filter parity within explicit
  f64/f32 tolerances, including optical_filter=false and separate light-cell
  refusal.
- Fixed/adaptive paging with halos, rows crossing pages, the capped storage
  binding, actual peak admission, preview/final behavior and cancellation.
- Capture/filter on versus off preserves raw transport, raw EXR, sample keys,
  alpha and unfiltered component recombination.
- Real rough slab/sheet over a textured opaque receiver, a geometric edge,
  a route-changing region and a caustic received behind the glass. Record
  eligibility and refusals as well as image output.

The existing clear/TIR guide tests must continue to pass unchanged. A rough
capture flag must not change the disabled raw sample stream or guide output.

## Independent qualification before enabling production

Arithmetic tests validate bookkeeping, not the rough BSDF distribution or
reconstruction quality. Require both of the following:

1. An independent ray/plane construction for endpoint coordinates and moments
   with prescribed directions, plus independently randomized sample comparisons
   against a numerical angular integral for the supported rough model.
   The integral must not call the production sampler as its own oracle.
2. Raw and filtered image comparisons against an independently checked reference
   with matching geometry, IOR, roughness convention, absorption, normal
   convention, finite depth, source measures and accepted path families.

A small deterministic quadrature for a matched planar case can qualify a
specific path family. Its truncation/error must be measured. A separately
implemented permissive reference renderer is another possible route, after
verifying model equivalence. Neither route is an existing result in this design.

A high-sample image from the same estimator is useful for stability comparisons
but is not an independent bias reference. A low seed variance or smooth PNG
alone cannot pass the quality gate.

Freeze numerical rejection thresholds before evaluating held-out seeds.
Measure raw/filtered error, integrated image radiance, edge profiles, transmitted
texture contrast, caustic structure, eligible coverage, peak memory and elapsed
time. Include paired capture-off tests and independent camera/photon seeds.
Compare fixed observation budgets and controlled elapsed-time budgets
separately. Preserve residual artifacts and refused regions in the evidence.

The initial acceptance is useful noise reduction within the declared domain,
with bounded measured detail loss and unchanged raw transport. If strict route
refusal leaves too little useful coverage, add a properly accounted supported/
residual partition or richer distribution records. Do not relax identity checks,
hide rare paths or label the covariance a confidence certificate.

## Implementation sequence

1. Add the shared event/record contract and independent numerical tests.
2. Add disabled-by-default CPU/GPU capture and complete-group/page integration;
   prove raw preservation before enabling reconstruction.
3. Add snapshot/window normalization and truthful diagnostic categories.
4. Add transmitted-cell reconstruction, optical opt-out and synthetic parity.
5. Qualify the declared image domain and resource limits, then expose the saved
   option and UI with its actual supported scope.

This spans the shared statistics layer, CPU transport/group/publication,
GPU continuation/film/layout, output/filter, diagnostics and saved controls.
It is not a change to the rough-material bypass condition alone. No kernel,
tessellation, photon estimator or native denoising dependency is required.
