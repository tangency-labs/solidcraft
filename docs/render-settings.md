# Persistent render settings

Named setups save common render settings and, optionally, the complete numerical
configuration of one selected image method, including an optional embedded HDR.
Saving does not start a render, apply
the navigation camera or require an installed CPU/GPU executor. `render.preflight`
and `render.start` consume the same saved configuration and still check the current
host, geometry, capabilities and memory limits. Sample selections through 16,384,
8K output, X2 supersampling, CPU workers, denoising controls and guide capture use
these same saved jobs; see [render quality controls](render-quality-controls.md).

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

`render.lights.list {setup: "Studio"}` returns a uniform `{lights: [...]}` read
model without applying or saving anything. An unsaved client draft can instead
pass `{settings: <complete RenderSetup>}`; `{}` inspects the default setup.
Neither form selects a UI shot. Unknown parameters, null selectors, both selectors,
missing setup names and invalid settings refuse without mutation or evaluation.
CPU/GPU hard-directional/sun/point/spot/rectangle sources expose
`id`, `kind`, `name`, `name_key`, `number`, `implicit`, `intensity`, `colour` and
their world-space geometry. Background and environment illumination are separate
from this source list. Implicit fixed studios and CPU presets are expanded;
explicit empty lists remain empty. GPU sky/HDR suppresses its implicit studio.

Light IDs use the kind and zero-based source slot (`directional:0`, `point:0`,
`spot:1`, `area:0`); point and spot share indices in the punctual list. IDs survive
numerical edits and repeated reads, but changing kind, reordering/removing lights
or replacing a rig can change them. These are setup-local addresses, not durable
document identities. Unnamed lights display Key, Fill, Rim, then Light 4, etc.;
clients translate `name_key`, substituting `number` for later lights. Names are
not persisted by this command.

Successful light edits additionally return bounded `id_remap` entries
`{before: Light, after_id}` for each surviving original source. The engine uses
original slot identity, not numerical-value matching: a duplicate is a new source
even when its values equal its parent. CPU/GPU canonicalization puts the finite Sun
first. The UI verifies each before record against its current draft, then remaps
mute, solo and surviving unrelated selection; unmatched identities from a different
settings override are cleared. Add/set/duplicate still select their actual result.
Numeric editor drafts are invalidated. A source RenderSettings Arc tracks verified
authoring provenance. Unverified structural Undo/Redo, checkpoint, replacement or
same-shape reorder clears mute/solo, Light selection and parameter drafts, rather
than guessing identity from ordinals or equal values. One bounded before/after
Weak-Arc proof preserves the latest verified numerical light-set Undo/Redo; older
or otherwise unproved transitions clear. Fallback labels remain ordinal (Key, Fill,
etc.) and may change after insertion; this adds no persistent custom naming.
Untouched original Sun/area rows retain their raw authored RGB rather than an
unrelated common-editor divide/multiply round trip.

Colour is linear RGB. CPU colour/intensity retain their saved values. GPU hard directional colour/intensity also retain their exact saved values. Finite Sun RGB and GPU area RGB
radiance is factored into its largest channel as intensity and normalized colour
(black uses intensity zero and white colour). Their product recovers radiance;
no photometric equivalence between methods is implied. All rectangle edges are
full side vectors in millimetres, including GPU half-edges multiplied by two.
Directional vectors point towards a source; spot axes point from it. The optional
directional `angular_radius_rad` identifies a finite Sun (CPU requires explicit Indirect lighting).

`render.lights.add`, `.set`, `.duplicate` and `.remove` edit a named setup in one
undo step. They accept `name`, `id` except for add, optional complete `settings`
as an unsaved starting point, and optional `expected_revision`. Add/set accept
`set` with `kind`, `intensity_ev`, `temperature_k`, `softness`, `azimuth_deg`,
`elevation_deg`, `distance_mm`, `size_mm`, `aim_at`, `width_mm`, `height_mm`,
`position_mm`, `direction`, `edge_u_mm`, `edge_v_mm`, `radiance`, `colour`,
`reference_distance_mm` and `minimum_distance_mm`. These lower into the existing
light fields; there is no new saved light metadata. Temperature is an original
three-band, 6500 K white-balanced Planck tint approximation stored as linear RGB.
Area softness changes emitter size; finite Sun softness changes angular radius.
Hard CPU directional and point/spot sources refuse unsupported emitter softness.
CPU and GPU each admit 8 directional, 8 point/spot and 4 areas. Directional
sources include at most one finite Sun; CPU finite Sun requires explicit Indirect lighting. Point and Spot share eight slots. Both
methods expose Spot emission axis and inner/outer half angles; the UI displays
degrees while saved/runtime fields retain radians. A GPU feather needs
`cos(inner) - cos(outer) >= 1/1024`; narrower authored CPU cones explicitly refuse
on GPU rather than being widened or ignored. Materializing or duplicating the implicit GPU
studio preserves all three hard source values; it does not replace them with a Sun.
A directional source without angular_radius_rad is hard on either method. GPU
Add offers both Directional and the existing Sun (.02-radian radius). Finite Sun
duplication refuses; hard sources duplicate until the combined eight-source cap.

`render.lights.solo {id?, solo}` and `render.lights.mute {id, mute}` validate and
echo view requests. Enabling solo requires an id; disabling solo clears it.
The UI also verifies that the address exists in its current lighting draft.
`SolveApp::run` applies these replies to per-document `ViewState.lights`, never
to a design or undo history. Shot changes clear this transient scope. Saving,
undoing and reopening settings retain the unfiltered numerical setup.

`render_workspace_light::filter_request` filters copied settings at both
`image_request` and `SolveApp::run`'s Check/Render boundary. Explicit sources keep
their geometry and have zero intensity/radiance while off. Raster and the legacy
GPU studio carry three optional `studio_weights` in the captured `ImageRequest`;
they are finite 0..1 runtime values, never fields of `RenderSetup`. CPU raster
uses them in `studio::shade_weighted`; the GPU path uses its captured uniform.
CPU ray tracing and explicit GPU rigs refuse these fixed-studio weights.

The selectable Lights group runs `view.render_select {kind: "lighting"}` and opens
Lighting: Light source, Shadows and sampling, then Budget. Its + still adds a
light. CPU presets require review and consent before replacing directional lights;
Custom materialises their existing values. GPU budgets show eight directional
sources including the optional Sun and four Areas; CPU eight directional and
eight point/spot plus four Areas. Counts are
read-only sliders. Unsupported controls stay faded with a reason. No rig identity
is saved, so there is no Rig row.

Browser eyes and solo actions run the same view commands. A selected light mounts
`render_workspace_light::editor` in the Inspector's Lighting page. Without a
selected light, that page calls the same group editor. Light has a name/type
header, Area/Point/Sun segments, intensity, temperature and softness; Position;
Shape for areas; Falloff for points; and Behaviour (Solo, Mute, Duplicate, Delete).
Distances displayed as subject-size multiples lower to millimetres. Width U and
Height V are full side lengths (GPU storage uses half-edges). Independent vector
edits preserve the other side. Radiance and temperature lower to existing linear
RGB fields. Sun directions remain world vectors. Every edit is bounded and finite,
with the renderer validating the complete candidate. Opening an editor
does not apply or save anything. Edits run the four light commands; Place by
highlight stays disabled with its viewport-picking reason. The live product view
receives bounded analytic light samples in `Frame.product_lights`; both PBR and
non-PBR/transparent batches use them. This is an unshadowed preview, with area
emitters sampled at their centre and constant ambient illumination. HDR transport
and full emitter sampling remain the image renderer's responsibility.

| Field | Default | Saved contract |
|---|---|---|
| `backend` | `builtin` | Valid R1 backend ID, at most 64 ASCII bytes; unavailable IDs survive reopen |
| `mode` | `preview` | R1 `preview` / `final` mode; availability checked by a future render request |
| `camera` | Existing `Camera::default()` | Snapshot of target, yaw, pitch, roll, distance, fov and up; mm/radians, existing Z-up default |
| `width`, `height` | 1024, 768 | Selected final output size. Legacy/raster: sides at most 4096 and 4,194,304 pixels. Typed GPU: sides at most 8192 and 33,554,432 pixels. Typed CPU: selected resource profile plus output-feature limits; X2 charges the internal grid separately. |
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
| `cpu_raytrace` | `transparent_background` (default false), `options`: optional `progressive`, `primary_samples`, `preset`, `lights`, `punctual_lights`, `area_lights`, `area_samples`, `seed`, `shadows`, constant `environment`, embedded `hdr`, optional `lens` and `output` presentation | Primary samples 1/4/16 (default 1); shadows true; optional progressive/lists/preset/environment/HDR absent. At most 8 directional, 8 point/spot and 4 rectangular lights; progressive has the narrower contract below. |
| `gpu_path` | `transparent_background`, `options`: `samples`, `max_depth`, `seed`, `tone_map`, `denoise`, `clamp_indirect`, `lens`, `lighting`, sky `environment`, embedded `hdr` | Samples 1..65536 (default 64), depth 1..64 (default 8), seed u32 (default 0), Reinhard tone map, denoise false, optional values absent; `clamp_indirect` is 0.000001..1000000 when present ([biased indirect clamp](render-path.md#clamping-indirect-light)) |

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
empty object defaults to 16 samples and seed 0. With the Compact profile, samples
are 1..64 and the seed is u32. `primary_samples` must be 1, the internal trace grid
is at most 1,048,576 pixels, and `trace_width × trace_height × samples_per_pixel`
is at most 16,777,216. Trace dimensions equal the selected output dimensions
multiplied by `supersampling`. Saving checks these limits without a render host.
Larger CPU profiles use their own bounds; for example, Studio admits native
3840 × 2160 at 128 samples per pixel. Other memory, geometry and work limits
still apply. See [CPU resource profiles](render-cpu-resource-profiles.md).
Rectangular and continuous LED lights retain their
bounded direct-light integral for each camera sample. Dielectric and anisotropic-face
overrides retain the existing certified thin-sheet/axis-aligned-box and planar-face
transport and geometry guards. Embedded HDR and one setup exposure remain compatible. See
[CPU progressive sampling](render-cpu-progressive.md) for reflection limits,
work admission and provisional/final publication. The top-level runtime
`progressive: true` continues to select GPU.

CPU `lens` stores the same `{aperture_radius_mm, focus_distance_mm}` as GPU.
Positive aperture requires explicit progressive sampling plus a bounded perspective
setup camera. No second camera is saved; Frame lens edits preserve the saved
camera and current navigation. Absent/null keeps pinhole behavior; an explicit
zero aperture remains explicit and keeps the old fixed-AA/orthographic pixels.
Whole-pupil glass clearance and bounded ray intervals are checked against the
captured scene at render admission. See [physical thin lens](render-thin-lens.md).

CPU `output` optionally stores `{tone_map, finishing, backdrop, linear_exr}` using
exactly the shared presentation operators. It has no second exposure/background:
setup exposure and background remain authoritative. With native resolution and
denoising disabled, omission/null preserves the original CPU f64 PNG, as does
identity presentation with optional raw retention. Nonidentity effects apply to
every complete progressive pass; only final completion retains a raw master.
The shared output envelope is 8192 per side and 33,554,432 pixels, independently
of a larger CPU resource profile. See
[CPU output presentation](render-cpu-output.md).

GPU `lighting` retains the complete RGB ambient radiance, optional sun, at most
four rectangular lights and a bounded `directional_lights` array of
`{direction:[x,y,z], color:[r,g,b], intensity}`. Hard directions share the CPU
contract: finite nonzero vectors with components within +/-1e9, linear RGB 0..1,
relative intensity 0..64. The hard list and optional Sun total at most eight sources.
Empty hard lists are omitted when saving; existing designs preserve their meaning. Omitted/null lighting keeps the renderer's legacy studio;
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

For a complete job, additional request fields are `replace` and optional transient
`studio_weights` for a fixed studio. Parallel
runtime controls such as `progressive`, `integrator`, `raytrace`, `lens`,
`environment` or `transparent_background` fail, including explicit null/false
values. They cannot silently override a setup. For legacy `job` absent/null, the
existing runtime request API remains available; it has no inferred CPU/GPU method.

### Explicit portability boundary

External HDR paths, texture maps, temporary GPU clear-coat/texture body bindings and CPU
reflection, anisotropy, emission and surface-effect bindings are not
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
The optional CPU sampling field adds `render-cpu-progressive`. CPU presentation
and physical lens intent share the `cpu-output-presentation` pending descriptor.
No historical format number or migration step is rewritten. Older designs need no
job transformation: an absent job keeps their common settings and runtime API.
Builds without the descriptor open the newer design with a warning and preserve
its saved options they cannot read. Plain JSON, design containers and document autosave
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


CPU `transparent_background` is stored beside boxed numerical `options`, like
Raster/GPU, and defaults to false for older designs. True requests straight-alpha
primary geometry coverage in fixed/progressive PNGs; it does not alter engineering
appearance opacity or raw EXR. Active bloom, backdrop and photographic operators
still refuse with transparent output. The existing CPU output pending descriptor
covers this saved flag. See [transparent CPU PNG](render-cpu-output.md#transparent-png-coverage).


## CPU spatial denoising

An explicit CPU job can save `job.options.denoise: true`. The default is false;
false is omitted from the canonical wire representation, so older CPU setups
keep their previous image and allocation path. It lowers only to the native
CPU `raytrace.denoise` option. The corresponding GPU field remains separate.

The optional filter uses deterministic geometry guides, filters every complete
published image pass before presentation, and preserves raw film, RNG/sample
identity and final-only EXR. It defaults to three passes; saved denoising controls
select one through three passes, per-pass strength and firefly suppression. Guide,
ping-pong and task storage are admitted under the selected CPU resource profile.
The output envelope is 8192 per side and 33,554,432 pixels; X2 filtering runs on
the internal grid, bounded at 16384 per side and 134,217,728 pixels. Profile byte,
work and sample limits still apply. No request silently reduces size or changes
the selected executor. Offline validation and save/reopen retain these options
without initializing a renderer. See [spatial denoising](render-denoise.md) for
bypasses, bias, cancellation/work units and retained-master regrade limits, and
[quality controls](render-quality-controls.md) for the saved fields.


### CPU consumer of portable optics

Stable `optics` rows remain the same saved contract for CPU and GPU. An explicit
CPU job validates at most eight IOR/Beer targets offline. Without explicit Path it
refuses authored transmitted shadows/subsurface and keeps its strict sheet/box
certificate. The separately prepared Path route adds general closed Glass and
selected-medium scattering; its new source still needs qualification. On the original absent-Path route, a saved general closed boundary stays authored
as `closed_mesh` and must prove the box subset. No boundary, target, effect or material is silently
removed to make a job run. See [CPU certified glass](render-raytrace-dielectrics.md).
Common Glass controls hydrate actual saved numbers without creating a runtime
override on paint. Their unsaved edits overlay only the selected stable rows in
captured image settings; Save/replacement and Remove remain explicit document
commands with one undo. This consumes existing portable fields and adds no format
number or new design metadata.


The optional [CPU opaque path lighting](render-cpu-path.md) consumer is part of the combined source and has executed renderer regressions. It adds bounded progressive diffuse/specular GI only when `path` is authored; existing restrictions and bytes above describe the absent-path route. Prepared general Glass/SSS, independent bounded Atmosphere and Sun/Sky have separate source/qualification notes. Mixing active Atmosphere with Glass/SSS, selected anisotropy and combined document/engine/UI/native qualification remain incomplete.


GPU `lighting.punctual_lights` preserves each existing `PunctualLightOptions` field.
Intensity is the relative on-axis incident value at `reference_distance_mm` and
falls by inverse square distance, multiplied by the cosine-space Spot smoothstep.
This does not imply lumen/lux/IES calibration. Nearer-than-minimum or excessive
(above 1e12 linear RGB) samples contribute zero and increment typed diagnostics;
no inverse-square clamp is applied. Re-aimed finite shadow connections exclude
the source, and offset crossings increment a separate omission count.

The registered worker publishes `punctual_diagnostics` through the existing job
status. GPU counters are cumulative through the most recent complete image
readback, including adaptive pilots and production Fresnel strata; an interrupted
partial dispatch need not appear in that last snapshot. Each counter is 64-bit
via two carry words, separate from unchanged medium-failure words 0..7. The
existing host metadata/work-release lifetimes and retained-options lease apply.

CPU canonical quality chips use a separate `render.quality.prepare` projection,
`cpu_progressive_png`, from the same three Q-* entries. It returns only size,
progressive production count/seed and a bounded CPU profile; it does not enable
GI, change denoise or replace authored optics/effects. Optional `cpu` context
contains `minimum_budget` and the preserved `adaptive` policy. Profile selection
includes pilot+production camera work and never lowers that minimum profile.
The existing `fixed_samples_png` GPU and acknowledged imported-review contracts
remain unchanged. These projections are read-only and do not certify convergence.

CPU saved `sun` and `sky` preserve the shared numerical domains and explicit method-switch/adoption policy described in [CPU celestial lighting](render-cpu-celestial.md). The renderer, saved/runtime and UI tests have executed; dedicated native Sun/Sky image-quality acceptance remains pending.
