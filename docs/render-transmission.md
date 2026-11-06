# Runtime dielectric overrides

The GPU runtime adapter uses the shared optical contract agreed in #1655. Its wire
adapter is isolated in `crates/engine/src/cmd/render/transmission.rs`. This raw
runtime API does not save an appearance or binding. The separate common Glass
editor and `RenderSetup.optics` provide stable saved assignments for CPU/GPU jobs;
see [workspace optics](render-workspace.md) and [renderer parity](render-parity.md).
The earlier experimental `pbr.transmission` proposal is removed.

Pass this request to `render.preflight` or `render.start`:

```json
{"progressive":true,"raytrace":{"dielectrics":[{
  "body_index":0,"body_name":"Glass","ior":1.5,
  "absorption_per_mm":[0.022314355,0.005129329,0],
  "boundary":"closed_mesh","thickness_mm":0}]}}
```

At most **eight** overrides are admitted. Both captured body ordinal and exact
name from `render.scene` must match; duplicate ordinals are rejected. An override
means a full dielectric interface, with no fractional transmission-weight field.
It replaces optical metalness only in the captured job. Saved appearances,
engineering properties, meshes, undo history and navigation do not change.

IOR must be finite in [1,3]. `absorption_per_mm` contains finite linear RGB
extinction coefficients in [0,100] per mm. Beer attenuation is
`exp(-sigma * geometric_distance_mm)`, using unshifted geometric hit endpoints,
not the distance after a numerical ray-origin offset. A colour authoring UI can
convert transmittance T at a stated distance d with `sigma = -ln(T)/d`; no such UI
or implicit sRGB conversion is added here. Zero sigma means no absorption.

Boundary is one of:

- `"thin_sheet"` with a sibling `"thickness_mm":3`: thickness in [0,10000] mm. An ideal
  parallel sheet with no interior medium or lateral ray displacement. Absorption
  uses thickness divided by the refracted cosine. Incoherent internal reflections
  are summed separately for s/p polarization, then averaged. Zero thickness is
  admitted and has no absorption, but retains the two-interface optical response.
- `"closed_box"` with `"thickness_mm":0`: strict axis-aligned bounding-plane check (1e-6 mm tolerance),
  followed by the GPU's closed, outward two-manifold topology validation.
- `"closed_mesh"` with `"thickness_mm":0`: general closed meshes under the selected
  producer's certificate. GPU uses its captured-f32 certificate. Explicit CPU Path
  uses the distinct exact-f64 [general Glass certificate](render-cpu-general-glass.md).
  CPU without Path retains the strict box subset; the shared enum does not widen it.

The GPU certificate matches topology representatives within 1e-6 mm Euclidean distance,
searching neighboring spatial cells so a rounding boundary cannot split a valid
CAD seam. It does not alter uploaded geometry or join chains beyond a representative's
tolerance. Weld scratch is bounded and budgeted for the largest closed dielectric
body being validated; opaque and thin-sheet bodies allocate none of it.
Preparation requires paired opposite edges, exactly one edge-connected shell, positive outward volume, and requires
one homogeneous IOR/medium over a closed body's faces. Without explicit GPU nested-media mode, selected closed-body bounds must be
disjoint. This conservative rule can reject separated curved shapes whose boxes
overlap. Explicit nested mode uses the hierarchy/origin policy in
[closed-glass boundaries](render-glass-boundaries.md); touching/overlapping media
are not repaired. Thin sheets can overlap each other's bounds. Default mode
requires outside camera/pupil origins. Unclassifiable crossings use the existing
medium diagnostics and failure policy.
Multiple shells in one body are refused with a connected-component check and a
negative two-shell fixture. Self-intersection of an otherwise oriented manifold
is **not certified**; such geometry is outside the supported input contract and
may not be detected. Edge counts and signed volume are not a proof of embedding.
The hole, inward-winding, disconnected-shell, nested/overlapping bounds and
camera-inside refusal paths have negative fixtures. GPU accepts the three named boundaries under that certificate. CPU without Path
uses the stricter [bounded dielectric preview](render-raytrace-dielectrics.md);
CPU Path uses [general Glass](render-cpu-general-glass.md). Preflight metadata must
describe the selected request, not infer arbitrary geometry from a generic
Refraction flag. The Glass follow-up reports the actual CPU Path boundaries.
Each strict prepared job carries its own certificate into the renderer; a
separate Start prepares fresh captured intent. Its qualification is separate
from historical CI7 in [renderer parity](render-parity.md).

Raster previews retain independently saved scalar opacity. CPU ray tracing has
its own conditional optical contract and [physical lens](render-thin-lens.md);
these controls are not inferred from the GPU runtime adapter.
CPU raytrace still requires surface opacity one, including for dielectric bodies.
Scalar alpha is never inferred to mean physical glass. Old opaque PBR appearances
still require opacity one. Unknown keys, nonfinite inputs, invalid ordinals and
out-of-range values are errors.

## Transport and validation

GPU transport uses geometric winding for entering/leaving media, exact s/p Fresnel
including total internal reflection, Snell refraction and Beer attenuation. It
includes the squared IOR ratio for radiance transport. Glass is smooth; rough
transmission and fractional mixtures are absent. Runtime [glass shadow connections](render-glass-shadows.md)
are enabled by default for thin sheets and opt-in for solids; the latter are explicitly
straight-through NEE-only approximations, not caustics. The maximum path depth remains 16.
The existing bounded per-path stack is retained. Default mode rejects nested
entries; explicit nested mode requires the separate containment certificate.

`crates/render/src/dielectric_oracle_tests.rs` is an original self-contained f64
fixture module prepared for sharing with #1659: normal/oblique/Brewster s/p,
Snell and critical-angle/TIR cases, Beer attenuation, lossless and absorbing slabs,
and zero-thickness sheets. Its provenance and numerical tolerances are explicit.
GPU tests additionally cover sphere furnace, actual-mesh ball-lens focus, and
absorbing slab transport with deliberately large ray offsets to detect undercounted
absorption distance. No test tolerance was relaxed for a backend.

Secondary-ray epsilon remains max(1e-5 mm, scene extent * 1e-7). Intersections use
original f32 dominant-axis projection and conservative bounds; they do not claim
universal exact watertightness. The shared-edge regression at pixel171, sample357,
seed1948 remains covered by the furnace. The general coordinate-transform idea is
from [Woop, Benthin and Wald (2013)](https://jcgt.org/published/0002/01/05/paper.pdf);
no external shader code was copied. See [thin-lens notes](render-thin-lens.md) for
CPU draft convention differences. The default lighting remains the legacy studio; explicit area, sun, sky and bounded
Radiance HDR inputs are described in [environment lighting](render-environment.md).
