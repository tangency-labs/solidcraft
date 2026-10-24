# Persistent render settings

Named setups save common render settings and, optionally, the complete numerical
configuration of one selected image method, including an optional embedded HDR.
Saving does not start a render, apply
the navigation camera or require an installed CPU/GPU executor. `render.preflight`
and `render.start` consume the same saved configuration and still check the current
host, geometry, capabilities and memory limits.

## Commands

```json
{"command":"render.settings.set","params":{"name":"Studio","set":{"width":1600,"height":900,"exposure_ev":1.5}}}
{"command":"render.settings.get","params":{"name":"Studio"}}
{"command":"render.settings.get","params":{}}
```

`set` creates a named setup from defaults or replaces supplied top-level fields in an
existing setup. A supplied `camera` or `job` replaces the whole value; it is not a recursive
patch. Omitted fields retain their values. `job: null` explicitly clears the job and
restores the legacy request protocol. Unknown fields (including camera/job fields),
invalid types and invalid values fail before document mutation. A new empty `set`
saves the defaults; setting an existing setup to identical values is a no-op,
preserving redo history and revision.
Each effective edit is one undo step and marks the document dirty. There is no delete
or rename command in this slice.

Named `get` returns `{name, setup}` or an error for a missing name. Without a name,
it returns `{setups, defaults}`. Reads do not evaluate the model or change history.

| Field | Default | Saved contract |
|---|---|---|
| `backend` | `builtin` | Valid R1 backend ID, at most 64 ASCII bytes; unavailable IDs survive reopen |
| `mode` | `preview` | R1 `preview` / `final` mode; availability checked by a future render request |
| `camera` | Existing `Camera::default()` | Snapshot of target, yaw, pitch, roll, distance, fov and up; mm/radians, existing Z-up default |
| `width`, `height` | 1024, 768 | Positive, each at most 4096; product at most 4,194,304 pixels |
| `format` | `png` | R1 output format (`png` / `exr`); saving EXR does not add an encoder |
| `exposure_ev` | 0 | One exposure authority: raster requires 0; CPU raytrace accepts -16 through 16; GPU path accepts -32 through 32. Legacy setups retain -32 through 32. |
| `environment_rotation` | 0 | Legacy finite radians, magnitude at most 1,000,000. A typed `job` requires 0. |
| `environment_intensity` | 1 | Legacy finite value, 0 through 100. A typed `job` requires 1. |
| `background` | [236, 240, 245] | Three sRGB byte values; display backplate, independent of illumination |
| `job` | absent | Optional typed active `raster`, `cpu_raytrace` or `gpu_path` configuration; absent/null keeps the legacy protocol |

Camera target coordinates and positive distance are bounded to magnitude 1e9 mm;
angles to 1e6 radians. FOV is finite, `0 <= fov < pi`; zero means orthographic in
the existing camera model. Both existing camera up axes are retained. Setup names
are trimmed, nonempty, at most 128 UTF-8 bytes, without control characters. A document
has at most 32 setups. Validation also runs during document deserialization.

The raster product-image executor (`render.preflight` / `render.start` without `progressive`)
doesn't apply exposure or the environment controls. A setup that sets them still renders there;
the reply's `ignored_controls` lists the ones that render leaves out (for example
`["exposure_ev"]`). A raytrace request with them is refused; progressive rendering applies the
exposure, and a saved job passes it to its executor.

## Complete numerical job intent

`job` is tagged by `method`. It saves the active method, rather than separate CPU
and GPU drafts. All numbers use the existing renderer contracts: world distances
are millimetres, angles are radians and light colours/radiances are linear RGB.

| Method | Saved fields | Main limits and defaults |
|---|---|---|
| `raster` | `transparent_background` | Defaults to false; no nonzero exposure or raytrace controls |
| `cpu_raytrace` | `options`: optional `progressive`, `primary_samples`, `preset`, `lights`, `punctual_lights`, `area_lights`, `area_samples`, `seed`, `shadows`, constant `environment`, embedded `hdr` | Primary samples 1/4/16 (default 1); shadows true; optional progressive/lists/preset/environment/HDR absent. At most 8 directional, 8 point/spot and 4 rectangular lights; progressive has the narrower contract below. |
| `gpu_path` | `transparent_background`, `options`: `samples`, `max_depth`, `seed`, `tone_map`, `denoise`, `lens`, `lighting`, sky `environment`, embedded `hdr` | Samples 1..4096 (default 64), depth 1..16 (default 8), seed u32 (default 0), Reinhard tone map, denoise false, optional values absent |

CPU `preset` and explicit `lights` are mutually exclusive. An omitted/null light
list remains unspecified; `lights: []` explicitly removes the default directional
lights. Point lights retain their optional spot cone, its world emission axis and
both half angles. Rectangular lights retain full side vectors, sample count and
seed; `area_samples` and `seed` require a nonempty rectangular-light list. Area
samples are 1/4/16/64. Lists are bounded while deserializing and are never truncated.
The CPU constant environment retains `{color, intensity}` independently of the
backplate. See [CPU light union](render-light-union.md) for the spatial and light
validation contracts.

CPU `progressive: {samples_per_pixel, seed}` stores bounded camera accumulation
using the existing CPU transport. An absent/null value preserves fixed AA; an
empty object defaults to 16 samples and seed 0. Samples are 1..64 and the seed is
u32. `primary_samples` must be 1, output is at most 1,048,576 pixels, and
`width × height × samples_per_pixel` is at most 16,777,216. Saving checks these
limits without a render host. Area/LED lights, dielectric and anisotropic-face
overrides are refused in this mode; the fixed CPU route retains them. Embedded
HDR and one setup exposure remain compatible. See
[CPU progressive sampling](render-cpu-progressive.md) for reflection limits,
work admission and provisional/final publication. The top-level runtime
`progressive: true` continues to select GPU.

GPU `lighting` retains the complete RGB ambient radiance, optional sun and at most
four rectangular lights. Omitted/null lighting keeps the renderer's legacy studio;
an explicit all-zero lighting object means a black studio. Those values are not
interchangeable. `lens` retains aperture radius and axial focus distance; nonzero
aperture requires a perspective camera. An explicit zero-aperture lens remains
explicit. Tone mapping and denoising retain their existing meanings and do not
create new executor capabilities.

The portable procedural GPU `environment` is:

```json
{"kind":"sky","gradient":{"zenith":[0.16,0.32,0.7],"horizon":[0.8,0.9,1],"ground":[0.12,0.1,0.08]},"rotation_rad":0,"intensity":1}
```

Gradient entries are finite linear RGB in 0..1,000,000; rotation is finite within
±1,000,000 radians; intensity is 0..100. Omitted gradient/rotation/intensity use the
shown defaults. The original procedural gradient is not a calibrated atmosphere.
An environment replaces the constant ambient-light term; it does not replace the
display background. See [environment rendering](render-environment.md),
[thin lens](render-thin-lens.md) and [studio lighting](render-studio-lighting.md).

An embedded HDR uses the separate `options.hdr` field and cannot coexist with a
constant CPU environment or a GPU sky. It references immutable bytes in
`render.resources`, not a file name. [Saved HDR resources](render-hdr-resources.md)
documents atomic file capture, CPU/GPU colour policies, strict container hydration
and the offline save/reopen contract. Optional `hdr` absent/null means no HDR;
when an HDR object is supplied, its resource and controls are required.

`RenderSetup.exposure_ev` is the only saved EV. CPU lowering copies it into the
runtime `raytrace.exposure_ev` and resets the legacy common-settings EV to zero;
GPU lowering uses it in `JobOptions`. CPU/GPU `options.exposure_ev` is rejected.
The two legacy common environment fields must stay at 0/1 when `job` is present;
the method's typed environment owns those values. Existing legacy values are
preserved, and a conflicting conversion fails instead of clearing them.

### Save, check and start

```json
{"command":"render.settings.set","params":{"name":"CPU studio","set":{"width":1600,"height":900,"exposure_ev":1,"job":{"method":"cpu_raytrace","options":{"primary_samples":4,"preset":"warm_cool","shadows":true}}}}}
{"command":"render.preflight","params":{"setup":"CPU studio"}}
{"command":"render.start","params":{"setup":"CPU studio","replace":true}}
{"command":"render.settings.set","params":{"name":"GPU studio","set":{"width":1600,"height":900,"exposure_ev":0.5,"job":{"method":"gpu_path","options":{"samples":256,"max_depth":8,"seed":42,"tone_map":"aces_fitted","lighting":{"environment":[0.1,0.2,0.3],"area_lights":[]},"environment":{"kind":"sky","intensity":0.5}}}}}}
{"command":"render.preflight","params":{"setup":"GPU studio"}}
```

An inline `{"settings": full_setup}` has the same semantics as a saved `setup` name.
The engine resolves the typed job **before** choosing CPU or GPU dispatch. A saved
`gpu_path` method requires the `gpu-path` executor; another executor announcing
progressive sampling cannot replace it. A missing or unsupported host blocks
Check/Start while leaving the saved intent intact.

For a complete job, the only additional request field is `replace`. Parallel
runtime controls such as `progressive`, `integrator`, `raytrace`, `lens`,
`environment` or `transparent_background` fail, including explicit null/false
values. They cannot silently override a setup. For legacy `job` absent/null, the
existing runtime request API remains available; it has no inferred CPU/GPU method.

### Explicit portability boundary

External HDR paths, texture maps, GPU dielectric/clear-coat/texture body bindings and CPU
reflection, anisotropy, emission, surface-effect and dielectric bindings are not
part of this saved job format. Their fields are rejected, even when an unsupported
list is empty. Runtime requests can still use those existing features with an
inline setup whose `job` is null. Saving a complete setup must not discard them or
turn a current body name/ordinal into a persistent identity. HDR bytes are portable
through `render.settings.capture_hdr`; portable maps and durable body/face bindings
still require separate contracts. Runtime requests can reference an existing HDR
resource while supplying temporary bindings, without persisting those bindings.

No captured scene revision, worker, device, image result, `JobId`, host availability
or external path is serialized. Reproducible input configuration does not promise
bit-identical pixels across devices or renderer versions.

## Persistence and evaluation

Named setups live in `render: {setups: {name: setup}, resources?: {sha256: resource}}`;
empty render state is omitted.
The optional numerical job adds the `render-job-intent` pending format descriptor.
Embedded HDR adds its own `render-hdr-resources` descriptor.
The optional CPU sampling field adds `render-cpu-progressive`.
No historical format number or migration step is rewritten. Older designs need no
job transformation: an absent job keeps their common settings and runtime API.
Pending builds without the descriptor refuse the newer design instead of silently
losing its saved options. Plain JSON, design containers and document autosave
snapshots carry all HDR bytes. A command-only crash replay before a new checkpoint
still needs the source path recorded by `capture_hdr`; it must match the recorded
digest. This unit does not make that journal self-contained.

The engine avoids geometry evaluation only after proving that the document differs
solely in validated render settings, with unchanged active sketch and pending moves.
The proof compares the complete document after normalizing the render field, so mixed
geometry/appearance/placement edits take the usual conservative refresh path. It uses
a document clone/comparison, not a constant-time or whole-application speedup claim.
Unchanged render pointers short-circuit the check for normal modelling commands.

Render-only edits, undo/redo and checkpoint restore preserve model state, its stale
marker, cached tessellation and the prepared world state. A stale live-drag result never
becomes authoritative through a settings edit; the next model-reading command still
settles it normally. Revision advances for consumers that track document changes.
Geometry, physical materials and appearances retain their existing representations;
the setup stores image configuration, not material assignments.

No new third-party dependency or asset is introduced. `doc` depends on the existing
pure `render` crate for its shared camera and R1 identifiers, within the L2 → L1 rule.
