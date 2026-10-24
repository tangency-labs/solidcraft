# Runtime surface patterns for the opaque CPU preview

`raytrace.surface_effects` is an optional bounded runtime request field. Each
entry names one uniquely resolved captured body, an explicit right-handed world
frame in millimetres, three positive repeats in millimetres, roughness amplitude,
and bump amplitude in millimetres. No field is saved into the design, native file,
physical material, texture catalog, or appearance preset.

The original periodic field changes only the opaque surface roughness and shading
normal. It does not change vertices, geometric normals, intersections, bounding
boxes, body mass, volume, inertia, or shadow occlusion geometry. This is a periodic
preview field; it does not load a texture or displace a surface.

## Consumer contract

At most 32 effects and reserved slots are admitted. Names are 1–256 UTF-8 bytes,
without control characters, and must resolve to exactly one captured mesh. The
frame must be finite and orthonormal with positive handedness. Repeats are within
0.001–1,000,000 mm; roughness amplitude is within 0–1; bump amplitude is within
0–1,000 mm, with a maximum analytic gradient L1 bound of 4. Sampling beyond the
world/periodic precision bounds refuses. Duplicate and ambiguous selectors,
explicit JSON null, nonfinite values, and oversized capacities refuse.

Absent effects preserve the existing path. An explicit empty list stays distinct
in the request. Zero-amplitude entries are validated but excluded from the
prepared scan and preserve the completed image bytes. Dynamic name/vector
capacities in both task options copies and the prepared vector are included in
the existing admission sum; the retained request uses its original host budget
lease and remains accounted after frame eviction or while handles are held.

The current image consumer scans one prepared binding per continuation and checks
cancel/work before mutating the cursor. A completed selection precedes every
lighting stage. Each directional, point/spot, rectangular/LED, HDR and isotropic
reflection consumer receives the same changed surface and shading normal through
the shared hit contract. Original geometric normals still govern ray offsets,
visibility and emission sidedness. Emission is added through the existing path
throughput and receives no new direct-light sampling behavior.

The state resets at each reflected hit and each new primary sample. Fixed-primary
AA reuse is valid only after the effect scan and all shading stages have finished.
S/R sampling, sample denominators, coverage, linear accumulation and the final
display transform remain in the existing consumer. No alternate pixel encoder,
renderer, timer, or host pool is introduced. Public steps retain the four-work-unit
bound and existing task/job memory caps.

A nonidentity effect on a body selected for certified planar anisotropy refuses:
that model's orientation/rough-axis contract does not yet include these fields.
Identity effects remain allowed there. The integrated CPU dielectric consumer
also refuses nonidentity effects on its optical boundaries before transport;
Snell/Fresnel keeps the original geometric normal. An existing clear-coat layer
is preserved when changing substrate roughness for direct lighting. Selecting a
coated body for CPU reflection, anisotropy or dielectric transport refuses until
a matching layered transport estimator is available.

## UI and validation scope

This consumer change admits the field through the existing shared runtime request
and documents it in engine preflight. The current visible image controls continue
to refuse unexposed effect options. The separately recovered surface/emission
editor needs its own port and native acceptance before visible controls can be
claimed.

The recovered original source checkpoint authored five math/JSON tests, two
completed-image/admission tests, and a host lease lifecycle test. Those original
tests are retained. The current port adds strict name/capacity and aggregate
lease checks; all analytic-light/HDR combinations against an equivalent constant
shared surface; mirror/rough reflection through an emitting secondary hit; a real
32-binding scan with cancellation/work exhaustion; anisotropy overlap refusal;
and AA ordered-prefix/coverage/PNG/quantum checks. Existing malformed AA completion
cases are retained and extended for surface state.

These are source contracts and written regressions. The recovered checkpoint and
this scratch port have not been compiled or executed in this review. Qualification
requires the exact candidate's formatting, target layout bound, render CPU ON/OFF
tests, engine/UI contracts, Clippy, wasm, actual full project CI, and any newly
exposed visible/native flow. Historical unrelated focal successes or full-CI
failures remain attached to their original source revisions; they do not qualify
this port.
