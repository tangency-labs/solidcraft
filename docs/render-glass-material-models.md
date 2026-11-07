# Rough Glass, dispersion, thin films and caustics

Glass assignments can optionally carry a bounded material model. CPU Indirect
lighting and GPU path tracing consume that model from the same saved optical
assignment. Existing assignments without `model` keep their smooth-dielectric
interpretation and existing sampling route. The additional controls describe
optical rendering; they do not change engineering materials or mass.

```json
{
  "model": {
    "roughness": 0.2,
    "dispersion": { "abbe_number": 60 },
    "thin_film": { "ior": 1.38, "thickness_nm": 285 }
  }
}
```

Each field controls a separate effect. Omit `dispersion` or `thin_film` to disable
that effect. Roughness zero selects a smooth interface. A zero-thickness film
has the exact bare-interface response. The editor preserves authored values and
reports incompatible combinations instead of silently clearing them.

## Rough reflection and transmission

`roughness` is finite in [0, 1]. It controls an isotropic GGX microfacet boundary,
with slope width `alpha = roughness²`. This is an optical Glass parameter;
opaque appearance roughness is not reinterpreted as transmission roughness.
Numerically unrepresentable positive widths or GGX peaks refuse; they are not
silently raised to a larger roughness. GPU f32 precision can refuse a width that CPU f64 admits.

For positive GPU roughness, the uploaded f32 slope width must also have a
**normal**, rather than merely nonzero, squared width: `alpha² >= 2^-126`
(`f32::MIN_POSITIVE`), with a finite GGX peak. WGSL permits specified arithmetic
inputs/results to flush subnormal values to zero; a finite theoretical peak can
therefore still produce a zero microfacet denominator. See the
[WGSL floating-point rules](https://www.w3.org/TR/2026/CRD-WGSL-20260825/#floating-point-evaluation).
For example, roughness `1e-9` meets this admission, while `3e-10` does not.
The CPU f64 domain is unchanged, and roughness zero retains the smooth limit.
Projected-response products or ratios outside safe GPU arithmetic also produce
an explicit optical-arithmetic failure rather than a clamped BSDF or invalid image.

The sampler draws visible microfacet normals for the incident direction. Exact
Fresnel selects reflection or transmission at the sampled facet. The transmitted
direction follows Snell refraction. Total internal reflection selects reflection.
Height-correlated Smith masking supplies the single-scattering geometric weight.
The continuous evaluator uses the same reflection/refraction mixture and
refraction Jacobian as the sampler. A sampled direction on the wrong geometric
side contributes zero; retrying until it passes would change the estimator.

For radiance transport, transmission carries `(incident_ior / transmitted_ior)²`.
Photon flux transport uses the corresponding importance convention and does not
multiply flux by that radiance ratio. Jobs with an active advanced model or
caustics use the geometric facet normal for optical sampling and evaluation.
This fixed facet normal gives reciprocal transport. Smooth Glass jobs without
either option use their original shading-normal policy, including when the
selected medium scatters. Opaque receivers use their supported authored shading
normals. Matched indices without an effective film retain a straight
unit-transmission delta event, including with authored roughness.

The model is single scattering on the microsurface. It does not add an empirical
energy-compensation term or claim a multiple-scattering GGX microsurface solution.
Macro-scale paths can still bounce through several actual Glass boundaries.
An ideal rough sheet walks its entry/exit interfaces and internal returns within
the finite authored path depth. That walk is distinct from coherent thin-film
interference and from a closed geometric solid.

The VNDF construction follows the published mathematical description in
[Heitz (2018)](https://jcgt.org/published/0007/04/01/paper.pdf). The transmission
half-vector and Jacobian follow
[Walter et al. (2007)](https://www.graphics.cornell.edu/~bjw/microfacetbsdf.pdf).
The implementations are original Rust and WGSL derived from the equations.

## Hard-source direct lighting

Rough closed interfaces and each rough virtual-sheet interface explicitly
connect to hard directional, point and spot sources. The three implicit studio
directions use the same estimator when no explicit lighting setup is authored.
The contribution is the continuous dielectric BSDF times the outgoing projected
cosine, source radiance and the supported shadow connection. Point/spot sources
retain their distance and cone attenuation. These deterministic connections have
unit MIS weight: a continuous BSDF sample cannot hit a zero-solid-angle source.
The GPU's selected-vertex factor K is a separate importance weight, described
below. Finite rectangles, finite suns, emissive geometry and environments retain
their separate sampled endpoint estimators.

GPU straight closed-solid shadows remain a separate, optional approximation.
For smooth jobs using the legacy approximation, a completed closed crossing
uses its Fresnel and Beer scale. A connection that stays in one medium
integrates that medium's Beer attenuation and cancels the equal-endpoint IOR
factors. Advanced optical transport applies the endpoint IOR ratio at crossings.

When an opaque camera prefix is already covered by that transmitted-light
estimate and the current rough closed material enables it, hard-source NEE at
the rough interface retains only reflection. Adding its transmitted response
would count the replaced family again. Camera/specular-only prefixes retain
the full rough response. A reflection turn or a material with disabled straight
shadows breaks the replaced transmission chain, so later physical connections
remain available. Reflection-only connections retain their actual incident
medium and its absorption; they are not treated as virtual sheets or vacuum.
This partition does not make the straight approximation a refracted-shadow
solver. Closed-shadow approximation still refuses with caustics, and rough
virtual sheets still require disabled straight transmitted shadows.

At a virtual sheet's world-facing entry, a direct reflected connection can leave
the sheet. At an internal return, a transmitted connection can leave it. The
finite interface walk includes the corresponding Fresnel/Beer factors and depth
limit. A later zero-contribution continuation does not erase an earlier direct
contribution. A legal straight smooth-sheet shadow connection can attenuate a
light connection; this does not add an arbitrary refracted shadow solver through
another closed or rough boundary.

Selected HG collisions also connect to those hard sources with the normalized
phase function. The phase angle compares physical propagation directions;
positive anisotropy favors forward scattering. Ideal smooth delta interfaces
do not acquire a continuous highlight from these connections. Ideal hard-source
endpoints remain nonvisible to pure specular camera paths under the existing
emitter convention.

The GPU uniformly retains one eligible optical/HG vertex for hard-source NEE,
then stores that request for a separate optical-resolve dispatch after the
camera walk. If K vertices were eligible and J is the uniformly selected vertex, the
result is `K * prefix_J * direct_J`. Conditioned on the camera path, its expected
value is `sum_i prefix_i * direct_i`, by the same uniform-reservoir identity as
the photon receiver selection below. This retains the complete expected direct
sum while storing one request. CPU continuation estimates its eligible vertices
through its cooperative connector.

The retained GPU payload uses six `vec4<u32>` lanes: 96 bytes plus separate count
and selection RNG state. It preserves the physical point's high/low parts,
incident direction, IORs, source triangle/event kind and throughput prefix; HG
requests also retain their current medium. Certified parent metadata reconstructs
closed-interface media. The existing sixteen-event camera limit, including every
consumed virtual-sheet interface, bounds K by 16; an exceeded count fails
explicitly.

The camera dispatch writes a 128-byte packet per active path in its bounded
tile: 96 bytes of selected request, 16 bytes of count/wavelength and 16 bytes of
already accumulated camera-path RGBA. Optional tile scratch is
`128 * min(image_pixels, 16384)` bytes, at most 2 MiB, prepaid and appended to the
existing accumulation buffer. It adds no uniform or storage binding and no
full-image auxiliary plane. Fixed film uses its original one-plane offsets;
adaptive film keeps its three pilot/production/budget-plane offsets. Packet
scratch follows those planes and uses the path's tile-local index. Triangle and
mode identifiers are numerically encoded in f32 storage lanes and recovered as
integers; the eight-million-triangle host limit keeps those IDs exactly
representable, and the HG event ignores its triangle sentinel.

A separate resolve dispatch computes the selected hard-light visibility, adds
its K-weighted contribution to the stored camera value, and performs the shared
film update. Camera and resolve use matching active-pixel/adaptive-budget guards;
only resolve updates the optical job's film. Fixed sample scaling, Fresnel
strata, pilot variance/budget selection and final adaptive normalization therefore
occur once per retained camera sample. Both pipelines are warmed with the actual
admitted bindings before production. The ordinary inactive route retains its
single-dispatch film update and original storage domain.

Selection has its own RNG domain and does not consume or modify transport RNG
state or camera sampler dimensions. Null, depth-limited and ordinary terminated
continuations preserve the retained estimator; invalid-medium paths keep their
existing diagnostics. Selecting one request adds variance when caustics are off
and several continuous optical/HG vertices contribute. With caustics enabled,
the first-continuous-event partition usually leaves K = 1; preceding mixed
coated/index-matched delta choices can still leave several eligible requests.
The photon gather remains a separate estimator with its own receiver reservoir.

The separate optical program keeps its visibility/BVH walk out of the camera
continuation program. Native driver qualification remains necessary: passing a small
traversal or isolated lighting probe does not establish correctness of the full
expanded path integrator.

## Dispersion and the three representative wavelengths

`dispersion.abbe_number` is finite in [5, 200]. The authored Glass IOR is `nd`,
the index at the helium d line, 587.5618 nm. A two-term Cauchy approximation fits
that index and `vd = (nd - 1) / (nF - nC)`, using F = 486.1327 nm and
C = 656.2725 nm. The fitted index must stay in [1, 3] over 380–780 nm; invalid
material pairs refuse rather than clamp the index.

With `B = ((nd - 1) / vd) / (1/F² - 1/C²)`, the model evaluates
`n(lambda) = nd + B * (1/lambda² - 1/d²)`. Wavelengths in that expression are
nanometres. The resulting shorter-wavelength rays refract more strongly for
normal positive dispersion. Dispersion can split focused or refracted RGB bands;
it is not an image-space chromatic-aberration effect.

The renderer represents R, G and B by **610, 550 and 460 nm**, respectively.
When a path needs wavelength-dependent directions, one band is selected and
weighted by its inverse selection probability. The band remains fixed through
that path's directional optical interfaces and medium stack. Smooth ideal sheets
can instead evaluate all three bands in their RGB slab closure: straight
transmission does not require a band-dependent direction. The CPU camera,
transmitted-shadow and photon consumers share that slab closure and its scalar
mean-proposal branch probability. Photon transport uses the same representative
bands. Absorption remains the authored RGB coefficients.

This is a three-band approximation. It does not integrate a continuous spectrum,
measured source spectra, CIE colour-matching functions or a named glass's measured
Sellmeier coefficients. It can show band separation, but it does not promise
spectrally accurate rainbow colour, narrow-band light response or colourimetry.
Abbe number and `nd` alone do not identify a unique real glass.
The definitions and wavelengths come from the manufacturer's
[SCHOTT TIE-29](https://media.schott.com/api/public/content/aaa572afd854434fb7b3faa4bc46103f?v=c0f4fa52).

## Coherent single-layer thin film

`thin_film.ior` is finite in [1, 3] and `thickness_nm` in [0, 100000]. This is a
single lossless dielectric coating on each optical interface. Its thickness is
nanometres, independent of an ideal sheet's millimetre absorption thickness or a
closed body's geometric thickness.

Complex Fresnel amplitudes combine the two film interfaces and their repeated
internal reflections through the coherent Airy sum. The s and p power responses
are computed separately before averaging. The film can be propagating or
evanescent; a thin evanescent film can tunnel energy into a transmitting substrate.
The final transmitted direction still follows the actual outside/substrate
indices. The film does not create a laterally displaced geometric boundary.

A small-phase transfer-matrix form remains finite at the exact film critical
angle, where directly dividing the two Airy amplitude sums can produce 0/0.
Zero film thickness bypasses the coating and reproduces bare Fresnel exactly.
An index matched to either adjacent medium preserves bare-interface power.
Substrate total internal reflection retains unit reflectance.

Only the local coating retains coherent phase. Separate Glass boundaries and
macro-scale paths combine radiance, not wave amplitudes. There is no multilayer
stack, absorbing complex film index, diffraction grating or propagated
polarization state. Three representative wavelengths can alias rapid film
oscillations and produce colours different from dense spectral integration,
especially for thicker films. The analytical spectral anti-aliasing technique of
[Belcour and Barla (2017)](https://hal.science/hal-01518344/document) is not claimed
here; that paper also explains the underlying Airy model and spectral aliasing.

## Caustics from directional, point and spot lights

The saved CPU/GPU job can independently enable the following bounded prepass:

```json
{
  "caustics": {
    "photons": 16384,
    "radius_mm": 1,
    "max_depth": 16,
    "seed": 81,
    "area_lights": false,
    "environment": false
  }
}
```

The CPU request carries these settings in its ray-tracing options; GPU jobs carry
them in the progressive job options. The Render workspace saves them with the
setup. Enabling caustics does not change existing Glass materials or renderer
quality defaults.

`photons` is 1–1,048,576, `radius_mm` is finite in [0.000001, 1000000], and light-path
depth is 1–64 events. The photon seed is independent of the camera sampler.
Sources are hard directional lights, point lights and spot lights, including the
existing three implicit studio directions when that lighting mode is active.
Directional emission covers the captured scene bounds; point and spot emission
uses the corresponding bounded source conventions.

With `area_lights: true`, the CPU and GPU also launch photons from one-sided
rectangular lights. A launch draws a uniform point on the rectangle and a
cosine-weighted direction from its emitting side. Its unnormalized flux is
emitted radiance times π times rectangle area. The photon estimate divides by
source-selection probability and total launches.

Rectangle photons cover paths from the emitter through one or more smooth
closed glass events to the first opaque receiver. The camera estimator omits
the reverse suffix only when it belongs to this same family and fits the
photon depth. The gather checks the camera depth, reserving an event slot for
the emitter hit. Direct illumination and paths through rough glass, sheets
or scattering events remain with the camera estimator for rectangle lights.

The prepass follows actual reflected and refracted optical paths, including rough
interfaces, wavelength-dependent IOR, coatings and supported selected-medium HG
scattering. Eligible receivers include rough dielectric interfaces, virtual-sheet
interface sides, selected HG collisions and the first opaque surface. Light paths
continue after rough or HG receiver visits and stop at the first opaque surface.
A first rough/HG event with no prior optical event is direct illumination, already
covered by direct-light sampling, and is not also stored as a caustic. Smooth
straight sheet transmission covered by the existing direct connection likewise
does not create another copy of that contribution.

One launched photon stores **at most one** of its eligible receiver visits. If a
path has K eligible visits, uniform reservoir selection gives every visit
probability 1/K, and the selected record stores K times that visit's flux.
Conditioned on the traced path, the expected gathered contribution is therefore
`sum_i (1/K) * K * flux_i * response_i = sum_i flux_i * response_i`.
K is bounded by the authored maximum light-path depth, at most 16. The final
estimate divides by launched photons, including paths with no deposit, rather
than the number of retained records. This bounds storage without silently
truncating later receivers; the reservoir adds variance.

Surface receivers use a uniform disk kernel `1 / (pi * radius²)` and compatible
normal/plane checks. The receiver BSDF multiplies deposited flux per geometric
area. For an opaque shading normal ns and geometric normal ng, the measure
conversion is `abs(incoming_light.ns) / abs(incoming_light.ng)`: deposited flux
already contains geometric projected incidence, so there is no second cosine.
Photon continuation uses the adjoint normal factor
`abs(wo.ns) * abs(wi.ng) / (abs(wo.ng) * abs(wi.ns))` after removing the radiance
IOR ratio; wo points back along the incoming photon and wi follows its outgoing
travel. These are fixed-normal transport measures derived from
[Veach (1997), sections 5.3.3–5.3.4](https://graphics.stanford.edu/papers/veach_thesis/thesis.pdf).
Exact tangent measures contribute zero; invalid or nonrepresentable measures
refuse instead of introducing a normal epsilon.

At an analog camera HG collision, the volume gather uses the ball kernel
`3 / (4 * pi * radius³ * scattering_per_mm)` times phase and photon flux.
The light collision density contains one scattering coefficient, while analog
camera free-flight sampling already accounts for its own collision probability;
dividing by that coefficient recovers fluence in this estimator. A path-integrated
ray-marching estimator has a different measure. The kernel's units are inverse
square millimetres, consistent with a flux-to-radiance estimate.

With caustics enabled, rough/HG direct connections and map gathering operate up
to the eye path's first actually sampled continuous event. Subsequent continuous
continuations do not also add the same photon-represented source paths. A coated
index-matched delta transmission is not a continuous event. Opaque direct lighting
and gathering remain enabled: light prepasses stop at their first opaque visit.
With `environment: true`, supported environment sources use the same smooth
closed glass path family. The photon launcher samples the environment's
directional distribution and a uniform point on the scene's bounding disk
perpendicular to that direction. Each launch carries radiance times the disk
area, divided by the directional density, source probability and total
launches. The camera estimator omits only the environment term of an owned
path. Finite sun contributions and paths outside the opt-in family retain
their camera estimates.

**This is a finite-radius density estimate.** It is biased at finite radius and
has finite-photon variance. Radius controls blur and geometric leakage; photon
count controls sampling density. A smaller radius can produce a sparse or noisy
estimate. Close surfaces and curvature still matter despite normal/plane checks.
A completed job does not prove convergence or exact caustic silhouettes.
There is no general bidirectional connection, manifold next-event solver or
unbiased specular-chain estimator implied by the Caustics control.

CPU preparation follows its cooperative work and cancellation budget. GPU uses
measured bounded clear/emission quanta and a prepaid photon hash table. The hash
has a power-of-two head count of at least twice the photon count and at most one 64-byte
receiver record per launched photon. GPU grid span must not exceed 2²⁰ radius cells on any axis.
The ordinary six-storage-buffer GPU route remains available on older devices;
only authored caustics require the separate seven-storage-buffer variant.

GPU punctual-source origins too close to a closed optical boundary for reliable
rounded medium classification refuse explicitly. A detected photon medium
ambiguity also fails the job; it is not folded into the camera-path tolerance or
silently discarded while still reporting a successful image.

Active external atmosphere remains incompatible with this photon prepass.
Enabled straight closed-solid shadow approximation also refuses with caustics;
the approximation and photon estimate must not count the same hard direct light.
Selected-medium scattering keeps its existing surface-denoiser and Fresnel-strata
restrictions. General overlapping optical media and nonconvex nested-parent
certification remain separate geometry limitations.

Rough ideal sheets require disabled straight transmitted shadows. Smooth sheets
with dispersion or coating interference retain the RGB slab shadow connection
and matching light/BSDF mixture probability. Closed-solid CPU shadow connections
remain unsupported; the physical camera and optional photon paths still refract
through those solids.

## Verification and practical limits

The shared mathematical tests cover exact bare and zero-film limits, the Abbe
definition, Brewster and TIR cases, quarter-wave anti-reflection, film thickness
periodicity, evanescent tunnelling, film critical-angle continuity, reciprocal
power, VNDF normalization and `f * abs(cos) / pdf` sample weights. A rough
single-scattering furnace rejects energy gain; it does not assert unit energy
after unmodelled microsurface multiple scattering. Independent tests also check
the normal-incidence hard-directional GGX response `F / (4 * pi * roughness^4)`,
projected evaluator/sample agreement, adjoint normal and surface-density measures,
volume units/scattering cancellation, and exact integer reservoir history counts
for every eligible path length from 1 through 16. These mathematical identities
require separate production-path tests to establish consumer integration.

Producer tests separately qualify CPU and GPU consumption, photon storage and
transport, saved/runtime capture and image output. Qualification must name the
tested source and adapter. Mathematical tests do not establish an application UI
capture, all-adapter acceptance or default-quality approval.

The publication record must supply the following results for this implementation.
Earlier smooth-Glass or opaque-render receipts do not fill these entries.

| Scope | Required qualification record |
| --- | --- |
| Shared mathematical functions | Exact tested source identity and numerical test result; distinguish a standalone source harness from the complete crate suite. |
| CPU producer | Complete renderer test result plus advanced material and photon images from the recorded source. |
| GPU shader structure | Both legacy six-buffer and caustic seven-buffer WGSL validation results; structure alone is not device execution. |
| Physical GPU producer | Adapter/backend, exact source, advanced material/caustic tests and completed images. |
| Saved/runtime/UI integration | Exact tests and any actual author/save/reopen evidence; API-only images do not establish pointer UI acceptance. |
| Repository gate | Actual `cargo xtask ci` result and any inherited failures, without promoting scoped passes into a green full-suite claim. |

See the [complete Glass matrix](render-glass-complete.md),
[CPU general Glass](render-cpu-general-glass.md),
[GPU boundary certificate](render-glass-boundaries.md) and
[renderer parity](render-parity.md) for the surrounding contracts.
