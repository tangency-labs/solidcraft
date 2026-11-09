# Clear coat for CPU and GPU images

The Appearance editor’s Clear coat section uses the same saved opaque coat for
CPU ray tracing and GPU path tracing. Select a whole body, enable the coat, edit
Weight, Roughness and IOR, then choose **Save coat to shot**. Edits are previewed
through a stable-target request overlay; painting the editor never saves them.
**Discard coat edit** restores the saved state. Replacing different saved numbers
requires the explicit Replace control. Removing a saved coat has a confirmation
and one Undo step.

A saved setup carries `coats: [{target, coat: {weight, roughness, ior}}]` beside
its existing surface and optical bindings. `render.coat.bind` takes the exact
`render.scene` body ordinal/name and `expected_revision`, then stores the existing
stable definition/feature/component/occurrence identity. Check and Start resolve
that identity again. A hidden or replaced body is refused; a later body with the
same name or ordinal never inherits the coat. `render.coat.unbind` removes one
exact saved target without requiring a renderer. Empty lists are omitted, so old
setups retain their serialized shape.

Both methods support the existing 64-target override bound, weight 0..1,
roughness 0.045..1 and relative IOR 1..3. All saved numbers are checked even at zero
weight. Saved coats require an opaque target and cannot share a Glass target,
including at weight zero. CPU selected reflection and anisotropy overrides also
cannot share a coated target. These unsupported combinations refuse without
clearing authored values. CPU direct shading and its explicit progressive Path
transport consume the same coat; raster remains schematic and refuses bindings.

Saved maps may share an opaque coat target. They alter the substrate and shading
normal while retaining the coat parameters. Existing aging changes the substrate
and reduces coat weight by its physical layer mask; it keeps coat roughness/IOR
and never rewrites the saved coat. The guided denoiser operates on complete image
updates while the raw EXR remains unfiltered. Coat Save/Discard/Remove and saved
Undo/Redo preserve independent local map drafts. A conflicting external edit
keeps a pending local coat edit visible until it is discarded or reviewed again.

## Existing GPU runtime override API

GPU progressive image jobs accept an independent dielectric GGX coat over an opaque
metallic/roughness substrate. A dark, moderately rough plastic base with a smooth
coat can show narrow softbox reflections while retaining a dark body. Chrome uses
the existing metallic surface; it does not need a coat.

```json
{"command":"render.start","params":{"progressive":true,"raytrace":{"clear_coats":[{"body_index":0,"body_name":"Housing","weight":1,"roughness":0.12,"ior":1.5}]}}}
```

`render.preflight` accepts the same override. Index and exact name must both match
the captured `render.scene` body. At most 64 distinct bodies can be overridden.
Weight is finite in [0,1], perceptual roughness in [0.045,1], and relative IOR in
[1,3]. Unknown fields and repeated indices are errors. Weight zero is a no-op.
Coats cannot be combined with runtime transmission on the same body. Overrides
apply to the body and its faces in the job snapshot, never to the saved document,
engineering materials or source appearance. They require GPU progressive rendering.
The existing raw API retains its zero-weight no-op behavior; the saved binding
route above additionally requires an opaque target even at zero weight.

The implementation adds single-scattering GGX/Smith coat reflection with its own
Schlick Fresnel term. For cosine `c`, define the conservative directional bound
`B(c) = Fcoat(c / sqrt(2 * (1 + sqrt(1 - c*c))))`. The base BRDF is multiplied by
`(1-weight*B(N.V))*(1-weight*B(N.L))`. This is deliberately stronger attenuation
than macro-normal Fresnel, which can leak energy with rough coats over shiny bases.

For any reflected upper-hemisphere direction, the view/half-vector cosine is at
least the argument used in B. Schlick decreases with cosine, and a unit-Fresnel
single-scattering Smith GGX lobe reflects at most one. Thus the coat contributes
at most `weight*B(N.V)`; the attenuated unit-energy base contributes at most
`1-weight*B(N.V)`. The symmetric product retains reciprocity. This conservative
allocation can darken grazing views and omits internal inter-layer scattering and
refracted substrate directions; it is not a full layered transport solution.
Importance-sampled f64 furnace quadrature covers shiny bases, rough coats, partial
weights, IORs and grazing directions, alongside analytic and reciprocity checks.

Continuation selects the coat GGX lobe with probability `weight/2`, otherwise the
existing equal cosine/GGX base mixture. Evaluation uses the complete layered BRDF;
the PDF is the same combined mixture for continuation and direct-light MIS. GGX
samples below the surface are rejected without renormalizing the PDF. Zero weight
preserves the old BRDF and random-number consumption exactly. GPU BRDF/PDF tests
compare the production shader to the independent f64 numerical contract, including
roughness 0.045 and grazing directions.

The per-triangle resolved material occupies 64 bytes, including a 16-byte coat
record. Storage-limit checks and peak-memory estimates include this additional
record.

The model was designed from the public descriptions of
[layered materials in PBRT](https://pbr-book.org/4ed/Light_Transport_II_Volume_Rendering/Scattering_from_Layered_Materials)
and [Filament's clear-coat model](https://google.github.io/filament/main/filament.html#materialsystem/clearcoatmodel).
These motivate independent interfaces and substrate attenuation; this implementation
is original and deliberately simpler than a multilayer random walk. No reference
implementation source was copied.
