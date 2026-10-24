# HDR radiance in scene reflections (#1693 + #1658)

This document records the shared CPU HDR/reflection contract and its recovered
follow-ups. The personal rendering branch now ports that work onto the combined
CPU/GPU2 snapshot described in
[advanced CPU integration](render-cpu-advanced-integration.md). VNDF, spatial AA,
surface effects, visible emission and the bounded dielectric path are integrated
source, but their exact-revision execution and visual acceptance remain pending.
Historical checks do not qualify this new combination.

## Disjoint lighting paths

A camera ray that misses geometry still writes the requested background exactly.
It does not see the environment emitter and does not pass that background through
exposure. A reflected secondary ray that misses geometry reads the existing
owned environment through `radiance_linear_srgb(direction)` and adds
`throughput * Li` to its linear path radiance. Reflection sampling already put
`specular_BRDF * incident_cosine / matching_PDF` into that throughput; do not
divide by a second environment PDF at the miss. Mirror is an explicit delta
event with its existing Fresnel weight, independent of minimum roughness.

Selected mirror/rough reflectors receive no environment next-event estimate.
Nonselected opaque hits retain the existing environment direct-light estimator
and visibility. This partitions the environment contribution at each hit, without
MIS or double counting the same specular environment paths. Existing directional
direct lighting remains separate: there is no emissive geometry or infinite
directional-light intersection on a reflected miss. At the fixed bounce cap,
untraced continuation is discarded, as in the original finite-depth preview.

The first HDR+rough combination admits only fully metallic (`metallic == 1`)
selected bodies and every effective face override. Dielectric, partly metallic
or scalar face responses refuse in engine preflight, host admission and direct
task construction. This conservative body-wide check also refuses a mixed body
whose unvisited face is nonmetallic. Without this restriction, omitting the
environment next-event term would silently lose diffuse illumination. Supporting
those responses requires an explicit diffuse/specular partition or validated MIS.
Explicit mirrors retain their declared ideal-reflection/Fresnel intent.

The foundation retained the pinned reflection unit's projected-NDF GGX proposal.
The separate [VNDF follow-up](render-vndf-prototype.md) replaces its production
proposal and outgoing PDF together, with the same BSDF and null accounting;
its source-only checks do not transfer foundation validation. The integrated
planar anisotropic adjunct replaces only the selected face's matched rough sampler
and directional direct BRDF. There is no retry, PDF renormalization or diffuse-over-
specular-PDF mixture. Both samplers preserve the same HDR miss contract.

## Reflection sequence and deterministic primary reuse

The reflection variate generator receives the actual requested count R in 1..64
and rejects ordinals outside that count. It visits each radial stratum once using
a shared 32-bit midpoint jitter, with a pixel/bounce digital shift of a base-two
radical-inverse azimuth. Both coordinates remain strictly open. Exact dyadic
azimuth coverage applies only to power-of-two R; other counts retain radial
coverage only. Changing R changes the radial sequence, so there is no progressive
prefix, independent-draw, unbiasedness or universal variance-reduction guarantee.

The variate sequence itself changes no mapping, PDF or specular weight; the
matched VNDF follow-up is separate. Null-event accounting remains unchanged.
Every null attempt still contributes zero to the original R divisor. R-attempt
and bounce bounds are unchanged; actual outgoing paths, null outcomes and work
can differ with the directions. Five new source tests exercise the production
generator, including the white-conductor mean 1-ln(2), second moment 3-4ln(2)
and a derived 1/R quadrature bound for that specific monotone fixture. Existing
tests that retain the test-only hash control do not certify the new generator.
The production image's independent weight check uses the new sequence.

The primary generator uses a fixed center for one sample, or a fixed 2x2/4x4
grid for spatial AA. Within one spatial sample, a completed nonselected primary
surface's direct/environment shading can be reused across reflection samples,
while retaining the original ordered scalar sums, per-add radiance bound,
divisor and single display transform. See [primary AA](render-primary-aa.md). Selected reflectors and secondary
paths retain their full sampling. Future jitter, film or time-varying inputs
must revoke that repeatability proof. No antialiasing or film capability follows
from this optimization. Seven source tests cover its color, work and refusal
contracts; none has executed on this central candidate. Equal-R visible quality
and actual-work comparisons remain required for the new reflection sequence.

## Ownership, budgets and colour

The same captured source, decoder, decoded pixels, host pool and work budget are
used. `RaytraceOptions::retained_bytes` now accounts for original light capacities,
reflection selector slots and selector string capacities in one calculation used
by engine preflight and host admission. Fixed HDR capture metadata keeps the
Record lease; captured image storage stays under the work lease and is released
before PNG publication. The combined 384 MiB job limit and 512 MiB pool remain.

One existing decoder state is held in a fallibly allocated capacity-one vector,
with a checked 4096-byte state bound, covered by the existing 8192-byte HDR state
allowance. This keeps its scanline-scale array out of the image task's inline
metadata as the reflection cursor state grows. It is not another decoder, cache,
loader, worker or pool. Actual vector capacity must be one. Completion drops that
slot; decoded/scanline capacities and source capacities retain their prior checks.
A source regression checks the combined image task fits its 4096-byte host slot;
that test has NOT RUN.

Preparation, traversal and reflection continuation charge the shared cumulative
work budget and observe cancellation. Cursor-to-shading transitions yield before
another debit. HDR misses remain linear; finite radiance caps apply before the
sample average and single exposure/Reinhard/sRGB output transform. Primary
background is independent. Only explicit sRGB/D65 HDR primaries are admitted;
default/unknown Radiance primaries and unsupported optics remain refused.

## Added source regressions and remaining validation

Five source tests cover: a white ideal mirror receiving constant HDR exactly once;
unchanged camera background; rough-metal output from independent normal-view
GGX sample weights and deterministic variates; a broad bright cell rotating in
world space; rejection of mixed/diffuse rough responses; retained selector
capacities/task layout; and cancellation/work exhaustion during secondary
transport. All previous HDR and reflection test sources are retained unchanged.
These tests have NOT RUN, and the prepared PNG comparisons are not screenshots.

Before publication, integrate real prerequisite history, run the combined
default/feature checks, Clippy/wasm and required project gates, then produce and
inspect exact-build native images. The original constant and bright-cell HDR
fixtures carry MIT OR Apache-2.0 attribution; no external assets are added here.

## Planar anisotropic hooks (source only)

The Kernel-authored core and `PlanarTarget::sample` bridge are adopted from pinned
#1694 source; its reference ImageTask is not copied. One image task captures an
authoritative optional mesh face tag per primitive, binds at most two targets by
captured body ordinal AND exact body name AND face tag, and caches their validated
planar frames. A missing tag or identity mismatch refuses instead of retargeting
by plane geometry. Strict planarity can conservatively refuse imported/rotated
roundoff. Face appearance `surface: None` replaces body PBR with the scalar
response; it never inherits a metal response for admission.

At a rough selected hit, a bound target supplies the existing matched anisotropic
specular direction/PDF/weight. Equal axes delegate the existing isotropic sampler.
Other faces use the original rough sampler. Directional direct light uses the
bound anisotropic BRDF. HDR next-event lighting is still isotropic, so HDR plus
anisotropy on a non-reflector is rejected in preflight/host/task. The supported
HDR combination requires a rough reflection selection and the same fully metallic
body/ALL-face rule. A delta mirror plus anisotropic rough axes is rejected even
without HDR. No anisotropic HDR direct estimator is advertised.

Original target-name capacities join the same retained request calculation and
Record lease. Fixed cached targets and request clones fit the conservative bounded
state allowance; primitive face tags are included in actual primitive size.
Task construction repeats at most two 4096-triangle target validations, debiting
the existing cumulative work/cancellation callback. Pixel traversal and sampling
keep the existing one-unit yields, including secondary hits and final shadows.

The prior five HDR/reflection tests remain, with three added combination tests:
equal-axis HDR reflection matches isotropic PNG bytes; unsupported direct/mirror
combinations refuse; and an actual anisotropic secondary hit into another body
yields within one unit (four per public quantum) and observes cancellation.
Kernel's core/face tests and v4 continuation regressions are also retained from
pinned source. Every added/adopted test is NOT RUN in this integration. Independent
source review and the combined execution/visual gates remain pending.
