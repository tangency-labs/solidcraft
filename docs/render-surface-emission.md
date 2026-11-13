# Visible CPU surface emission (#1695)

This transient option adds visible emission to the existing opaque CPU consumer.
At the recovery handoff the source checkpoint was not compiled or run.
Subsequent combined CPU renderer regressions have executed for linear radiance,
sidedness, zero identity, reflected hits, target refusal and work/cancellation.
Current combined engine/UI/native qualification and project gates remain pending.

The transient `render.preflight` / `render.start` raytrace options accept:

```json
{"integrator":"raytrace","raytrace":{"lights":[],"emissions":[{"body_index":0,"body":"Emitter","color":[1,0.5,0.25],"strength":2,"sides":"front"}]}}
```

Targets match both the original captured mesh index and exact body name. Missing or
renamed targets refuse; duplicate body indices refuse. A body term applies uniformly
to its triangles, including triangles with face appearance overrides. Face reflectance
continues to override body reflectance independently. No face-specific emitter is
inferred from a face color or physical material.

Color is linear sRGB in [0,1], strength is finite relative radiance in [0,64].
No calibrated watts or automatic sRGB decoding is implied. `front` (default) emits
only when the unflipped geometric normal has positive dot product with the outgoing
view direction. `both` explicitly emits on either side, including tangency. Shading
normals never select an emitting side. Triangle winding defines the front.

Each hit adds `color * strength` to reflected linear radiance before the existing
single exposure, Reinhard tone map and sRGB encoding. Missing, empty and zero terms
preserve baseline output. Explicit null, unknown fields and unsupported combinations
refuse. The raster default and GPU preview gain no emission capability. Runtime
inputs do not modify design storage, geometry, engineering properties or caches.

Emission does not add a sampled direct-light source for neighboring surfaces.
Without optional [opaque path lighting](render-cpu-path.md), only existing selected
specular paths can reach an emitting surface; with other analytic/HDR lighting
disabled and no reflected emitter path, nonemitting surfaces remain dark. Explicit
path lighting can carry BSDF-sampled diffuse emission, without mesh-emitter
next-event sampling. The primary background
retains its original display color.

At most 32 emitter slots and 256 bytes per exact body name are admitted, including
original capacities. The host Record owns a request-metadata lease in the existing
512 MiB pool independently of PNG retention. Task copies and prepared terms enter
the checked combined 384 MiB job limit before reservations. Per-hit binding checks
debit cumulative work and observe cancellation; no new scheduler or pool is used.

Tests written cover linear output/exposure, zero PNG equivalence, front/back normals,
unlit neighbors, exact target refusal, duplicate/capacity/JSON bounds and metadata
accounting. The renderer cases have executed; current combined engine contracts,
full CI, wasm and real command/export/native acceptance remain separate gates.


## Integration with the current opaque consumer

Each resolved opaque hit adds its emission once, multiplied by the current path
throughput. Directional, punctual, rectangular and HDR stages then retain their
existing accumulation. Selected reflections can see an emitting surface; no
emitter geometry is sampled to illuminate other surfaces. A new bounce resets
the bounded emission cursor. Primary AA averages the same complete linear path
estimates, and the output transform remains outside those per-hit stages.

The task estimate explicitly adds both task-owned emission metadata copies and
the prepared emission vector. The host's existing complete retained-request
calculation also includes original emitter/name capacities, under one Record
lease. The existing 4096-byte ImageTask layout assertion remains a build gate.
The ordinary CPU/AA controls reject an emission option until its visible editor
is integrated; programmatic preflight/start accept the same bounded typed request.

The current-consumer port adds written regressions for AA/reflection divisors,
combined directional/point/area lighting and emission reached by a reflected ray.
The corresponding renderer integration regressions have executed on the combined
candidate. This does not qualify a visible emission editor or native acceptance.
