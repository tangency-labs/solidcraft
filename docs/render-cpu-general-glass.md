# CPU general Glass with indirect lighting

The published implementation is in commit `00edaf6a`, source tree `38a7f0af`.
Full CI7 ran the renderer, document, engine and UI tests; the renderer passed 744
tests and all 433 Render workspace tests passed. Full CI7 remains red in other
listed tests. The native Sample Plate evidence used opaque geometry, so it does
not qualify a native Glass image. See [capabilities and qualification](render-parity.md).

The Glass follow-up in this change prepares and retains the complete general-Glass
certificate for Check and Start. The editor merges saved and prospective targets
without saving the draft; checked file HDR capture keeps those stable identities.
See [PR #11087](https://github.com/tangency-labs/solidcraft/pull/11087) for this
follow-up's exact source and qualification; the historical CI7 and opaque native
results above remain separate.

CPU **Indirect lighting** (`options.path`) can consume up to 128 stable saved or
exact captured optical assignments. It requires explicit progressive camera
sampling, one primary attempt and traced shadows. Path depth is 1–64 and counts
shaded opaque surfaces, optical boundaries and active scattering events. An endpoint beyond this finite
limit contributes no further radiance; the limit is not a convergence guarantee.
There is no default, size, camera, profile or method change. Fixed saved choices
require explicit adoption through Sampling or the CPU Glass transport section.
GPU-only Fresnel strata and the GPU nested-media flag are not CPU controls.

Without `path`, the original planar sheet and strict axis-aligned box consumer,
deterministic Fresnel branch tree, camera stream, raw pixels and cumulative work
remain unchanged. Existing saved `closed_mesh` intent still lowers to that box
subset in this route. With `path`, `closed_mesh` instead selects the new certificate
and continuation. Explicit `closed_box` keeps its existing strict shape check.
No saved optical number, engineering geometry, physical material or mass changes.

The new certificate checks the **actual captured f64 triangle positions**. It does
not trust a closed flag, tolerance-weld seams, move vertices or repair triangles.
Signed zero shares an exact coordinate identity. Closed targets require exactly
two oppositely wound faces per edge, one connected shell and one connected circular
link at every exact coordinate vertex. A pinched vertex fails the link check even
when edge incidence and global connectivity pass. Adaptive exact predicates reject
intersection, non-topological contact, coplanar overlap and self-intersection.
Coordinates must be zero or have magnitude at least 2^-128 mm within the existing
1e12 mm world bound; this conservative predicate domain prevents underflow rather
than snapping small coordinates. An exact signed dyadic volume proves outward
orientation using a fixed 12-limb integer accumulator, not rounded volume or an
approximate ray-crossing witness. Imported or computed seams that are not exactly
closed refuse with their authored data intact.

Separated bounds use the existing fast disjoint test. Overlapping selected closed
bounds need a conservative nesting proof: each enclosing parent is convex and all
child vertices are strictly behind every outward parent plane. The immediate
parent is independent of body order; touching or crossing optical boundaries
refuse. Nonconvex nested parents remain unsupported. With Path, thin sheets accept
connected exact planar tessellations, including concavity and holes, within the
selected memory and cumulative work budget. All faces share one global winding;
boundary edges have one face and interior edges have two opposite faces. Each
vertex link forms one connected cycle or boundary path. Exact contact checks
reject overlaps, T-junctions, duplicate faces and pinched or touching fans without
repair. Signed zero has one coordinate identity. Without Path the original
one/two-triangle sheet certificate remains unchanged. Sheets still may not
overlap another selected optical bound.
Optical contact with analytic rectangle emitters or point/spot source positions
refuses explicitly. An ordinary opaque material wins an exact model-hit tie with
Glass, preserving an opaque contact surface instead of stepping past it.

Each perspective primary ray classifies its physical eye or sampled lens pupil
before any opaque hit or analytic emission. Exact half-open crossings initialize
the ordered nested medium stack; Beer absorption starts at that physical origin,
including opaque or emissive first hits inside Glass. Orthographic rays retain
their established before-bounds origin. A conservative bounds fast path avoids
classification work and allocation for unambiguous outside rays. Ambiguous samples
use one reusable prepaid state and a cooperative BVH scan. An actual boundary at
the origin or in the eye/pupil-to-near-clip segment refuses explicitly; endpoints
outside a small object do not bypass the segment check. No camera, ray or boundary
is moved to manufacture an outside origin.
The single path carries a 16-entry medium stack, independently of the 128 target and 64 event limits. The absent-Path legacy producer retains its eight-target and bounded deterministic branch contract. Every segment uses the
previous physical surface point for Beer absorption, before any numeric launch
adjustment. Geometric winding chooses entry/exit and verifies the certified parent.
Smooth captured optical normals are used only if both reflected and refracted
directions stay in the correct geometric hemispheres; otherwise the original
geometric normal is used. Snell, exact unpolarized Fresnel, total internal
reflection and the radiance IOR ratio are reused from the original f64 optics.
On-boundary continuations exclude only the producing body's exact source plane;
they do not consume a short segment with a finite normal offset.

Each Fresnel event chooses one counter-sampled delta branch. It creates no ray tree,
extra image plane or extra camera sample. Its domain uses the existing per-pixel,
per-ordinal, per-depth identity, including independent adaptive pilot seeds and the
original production seed. Reflected or refracted paths give directly reached HDR or
rectangle emission full delta weight. Opaque hits retain the complete existing
Effects→Maps→Aging closure before every BRDF/PDF and denoising guide use. Maps remain
unsupported on an optical target; unrelated opaque maps and aging remain active.

Thin-sheet transmitted shadows have an ordered closest-interface continuation.
They apply RGB parallel-sheet Beer/Fresnel transmission and carry the transmission
branch probability into the original light/BSDF sample-count MIS. If the remaining
finite depth cannot realize the competing BSDF chain, light weight is one. Finite
area connections retain geometry-wins-endpoint ties; point/spot connections retain
their existing excluded-source endpoint. Closed refractive shadow connections are
**not implemented**: authored `shadow_transmission:true` on a closed target refuses
instead of silently using a straight-line approximation. Closed Glass still reaches
HDR, area and mesh emission through actual sampled dielectric/GI paths. Hard-source
caustics require the separately authored photon prepass. Combining Glass with
external participating volume remains unsupported.
The independent [bounded Atmosphere](render-cpu-volume.md) is an opaque-Path
route; it does not erase those limits. Optional [Glass material models](render-glass-material-models.md)
add rough transmission, three-band dispersion, lossless thin-film interference and
finite-radius hard-source photon caustics. Assignments without a model keep the
smooth route. These newer source changes have separate qualification from the
historical CI7 counts above.

Certification is cooperative: exact edge merge sorting, connectivity, vertex links,
one face-pair contact check, determinant term or containment predicate run under
the existing cumulative work and cancel token. For N selected optical triangles,
including Path sheets as well as closed boundaries, its
prepaid peak is `sizeof(Task)+N*(sizeof(Face)+6*sizeof(Edge)+3*sizeof(usize)+3)`;
all vectors fallibly reserve exact capacity. The 12-limb volume state is in
`sizeof(Task)`. This change additionally bills one shared
proof slot by `sizeof(CertifiedOptics)`, including its Vec header; its outer Vec
header remains in the existing fixed metadata allowance. The ready cursor's
allocation moves into the image task, with its prior work charged once. Its
source-specific qualification is recorded in
[PR #11087](https://github.com/tangency-labs/solidcraft/pull/11087). Source-plane
cursor state is billed by the actual existing pixel and path slot sizes. No cap or static guard is raised. Contact and containment can
require quadratic work, so a valid large mesh can exhaust its selected profile
without producing a partial certificate or image. The fixed 4096-byte host slot and
all frame/metadata bounds remain compilation gates. The inside-origin increment
adds one conditional exact-capacity state slot, prepaid by its actual size; its
fixed BVH stack visits one node or primitive per charged step and is reused across
camera samples. It adds no image plane, additional ray, random draw or new cap.

Renderer regressions cover exact contacts, pinched vertices, rotated and arbitrary
closed meshes, outward sign under large-coordinate cancellation, nested physical
Beer lengths, independent per-ordinal Fresnel paths, real HDR and area endpoints,
opaque contact material, thin-shadow/BSDF MIS with 1/4/16/64 source samples, every
PNG prefix, worker quanta, raw EXR identity, adaptive pilot discard, cancellation,
work exhaustion, physical inside-eye and per-pupil nested Beer, first opaque and
analytic emission, exact boundary/near-clip refusal, initial-medium TIR, saved/runtime capture and the real pointer preset→Render→Save→
reopen workflow. The published source's tests executed in CI7; its exact counts
and failures are linked from [the qualification record](render-parity.md#qualification).
The mathematical source reviews supplement those tests. This earlier
qualification used an opaque native fixture, so it did not establish native
Glass controls or Glass image-quality acceptance. Later native Glass authoring
and Save/reopen evidence is recorded separately with the
[current Glass matrix](render-glass-complete.md) and
[advanced material guide](render-glass-material-models.md). Control and persistence
checks do not establish Glass/scattering image-quality acceptance.

## Subsurface continuation

The selected-medium continuation and its mathematical, producer, saved/runtime
and pointer-UI regressions are part of the earlier published source tested by CI7.
Current native Glass control/persistence evidence has a separate scope; visual
quality acceptance for Glass/scattering remains pending.
It consumes the existing saved `OpticalMaterial.subsurface` for a selected certified
closed CPU Path medium. The scalar collision rate is per millimetre; HG anisotropy
uses propagation directions, so positive values favor forward scattering. Beer
absorption stays separate. There is no RGB scattering albedo, heterogeneous
density or general external-volume consumer in this selected-medium model.
Advanced interface models are independently authored optical controls.

Each actual inside-eye/pupil or post-surface segment samples free flight from the
physical segment start, before its closest model or analytic-emitter endpoint.
The primary near clip does not shorten that optical length. A collision consumes
one finite Path vertex, validates its actual f64 position against the current
certified nested chain and launches one HG direction. The current continuation
also estimates hard directional, point and spot illumination at that collision,
including implicit studio directions. Supported contained connections include
matching scattering survival and Beer absorption. The optional photon map admits
rough-interface and HG volume receivers, with the bounded receiver reservoir,
camera-path partition and finite-radius bias described in the
[advanced material guide](render-glass-material-models.md). It adds no ray tree
or image plane. No-collision events retain their probability in endpoint MIS.
No arbitrary next-event connection through a refracting closed boundary is
invented; those source paths require the optional photon estimate. Independently
authored closed transmitted shadows stay refused.

Active scattering requires Path and disabled surface denoising, matching the
existing GPU random-walk compatibility domain. Thin sheets and Path=None retain
an explicit refusal, including authored inactive options on the legacy route.
With Path, `Some(rate0)` stays authored and adds no random draw, continuation work
or pixel/PNG/raw change. The pending collision/source-plane metadata is charged
through actual `sizeof(State)`, and captured optical storage uses actual element
sizes; this is not a promise of unchanged metadata admission bytes across builds.
The existing prepaid classifier slot is reused, with a debit before each scan or
transition. With active scattering, opaque bounce and direct-light rays also start
at the physical surface: a charged one-primitive-at-a-time range walk reuses the
existing exact source-plane cursor. This avoids consuming collision/absorption
distance through a normal offset; inactive options keep the old launch path. Nonrepresentable event positions or collapsed source/endpoint-plane
positions refuse instead of being moved by an epsilon. All existing profile,
4096-byte host-task and 1024-byte frame-metadata limits remain unchanged gates.

Regressions cover independent exponential survival, HG CDF/density mass
and first moment, per-ordinal first-hit eye/lens/nested Beer and slab walks,
contained light/BSDF survival MIS at 1/4/16/64 source samples, inactive identity,
white furnace, every complete prefix, quanta, raw/alpha/finishing/adaptive pilot
separation, cancellation/work exhaustion and saved/runtime/pointer save/reopen.
Their execution is covered by the published renderer suite, not by the opaque
native Sample Plate capture. See
[PR #11087](https://github.com/tangency-labs/solidcraft/pull/11087) for the Glass
follow-up's exact source and executed qualification scopes.
