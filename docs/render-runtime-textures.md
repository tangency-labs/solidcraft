# Opaque surface maps

Surface maps provide base colour, OpenGL tangent-space normals, roughness and metallic values. Built-in sources can be saved with a shot using stable definition/feature/occurrence identity and pinned version/digests. See [built-in library](render-built-in-library.md) for its bounded resource contract. Legacy examples below retain modulation and rows-down defaults. Engineering geometry and material properties are unchanged.

Progressive GPU jobs accept immutable per-body base-color and OpenGL tangent-space
normal maps. Runtime commands capture file bytes once; queued work does not reopen paths.
Existing saved built-in and temporary GPU maps retain single-level bilinear
sampling. Newly imported design-owned maps opt into footprint-filtered mip chains. See
[portable surfaces](render-portable-surfaces-and-stages.md) for design-owned bytes,
physical grain orientation, offline reopening and native CPU execution.

```json
{"progressive":true,"raytrace":{"textures":[{"body_index":3,"body_name":"Walnut tabletop","frame":{"origin":[0,0,480],"u":[900,0,0],"v":[0,90,0]},"base_color_srgb":"C:/art/wood.png","normal_opengl":"C:/art/wood-normal.png","normal_strength":0.35}]}}
```

CPU ray tracing consumes saved opaque maps in direct lighting and the optional
progressive Indirect lighting route. In the library, choose a surface and **Save
maps to shot**. The same binding command works offline without a render host:

```json
{"name":"Studio","set":{"job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":16,"seed":9}}}},"surface":{"id":"SURF-001","body_index":0,"body_name":"Panel"}}
```

Pass this object to `render.library.surface.bind`; it is one Undo step. The target
must match `render.scene`. Check and Start resolve the saved stable identity again.
Missing, hidden, changed or ambiguous targets and mismatched installed source
versions/digests refuse rather than rebinding another body. CPU temporary map paths
remain unavailable. Imported design-owned images use the separate saved-resource
workflow described in [portable surfaces](render-portable-surfaces-and-stages.md).
CPU maps require an opaque target. With Indirect lighting, Glass may be assigned to
another body, but maps and Glass cannot share a target. Selected custom reflections,
anisotropic faces and procedural surface-effect overrides remain incompatible;
authored settings stay intact and the request refuses. Raster does not consume
these bindings. A CPU producer must be installed before a CPU image can start.

Each `u` and `v` world-space vector spans one complete repeat, in millimetres. The
vectors must be nonzero, orthogonal, and 0.001–1,000,000 mm long. The origin is UV
zero. Image V increases downwards; normal red and green point along the positive
projected U and V directions, with handedness preserved. Projection is planar,
not triplanar: surfaces parallel to the projection axes may stretch. Each body's
bounding-box corners must remain within ±128 repeats; the frame origin must be
within 10 million mm of the camera target. These caps limit f32 coordinate loss.

Base RGB is explicitly sRGB, converted to linear reflectance before bilinear
filtering. Legacy runtime maps multiply the body's linear base color; authored
and imported saved surfaces replace it with the map's absolute reflectance. Normal channels are untransformed linear data. Normal
strength scales tangent XY in 0..1 before normalization; degenerate vectors, an
invalid tangent projection, or a normal behind the geometric/view hemisphere
fall back to the original shading normal. Alpha is ignored by this opaque-only
consumer. Linear roughness and metallic maps replace those scalar parameters.
An explicit scalar face appearance suppresses newly imported body maps on that face.
Existing GPU built-in and temporary map roles retain their previous precedence,
including when a request mixes legacy and imported roles.
Displacement, height and alpha maps are unsupported.

Newly imported GPU maps and native CPU maps prepare complete floor-halved,
linear area-filtered mip chains through 1×1. The default filter interpolates
bilinearly within each level and linearly between levels. It uses the largest
singular value of the unwrapped UV-to-texel Jacobian for LOD. Repeat seams do
not create false derivatives. Legacy built-in and temporary GPU maps retain
their single-level sampler. Content sharing includes the map's filtering and
face-precedence policy, so mixed requests retain each map's behavior.

The saved `texture_filtering` flag enables anisotropic filtering in both image
producers. The minor footprint axis selects the mip level. Up to eight taps
sample along the major axis. The filter widens the minor axis to bound the
anisotropy ratio. Primary footprints use the captured camera, hit plane and
fixed pupil sample. Quality-mode secondary footprints project a camera-pixel
cone onto the receiver plane and grow with accumulated perspective path length.
Orthographic cones retain their initial diameter. CPU deterministic denoise guides
use the primary footprint. Secondary transport does not track focused specular
ray differentials or a temporal error bound. When a CPU secondary footprint is
unavailable, the sampler uses the coarsest mip.

Quality filtering normalizes the source tangent directions before averaging.
When a footprint combines different directions, the mean vector gets shorter.
The sampler uses the lost squared length as direction variance and widens the
base specular lobe by adding `normal_strength² × variance` to squared GGX alpha.
This bounded approximation retains broad highlights where normal detail is too
fine to resolve. The effect fades in as the footprint grows beyond one texel.
Magnification, uniform directions and zero normal strength keep the original
roughness. Coat parameters keep their authored values. On GPU, the unused alpha
lane in each normal-map mip stores the variance so direction quantization does
not broaden a uniform normal. Texture memory remains within the same budget.

PNG/JPEG input is 8-bit, maximum 1024×1024 per map. Embedded ICC profiles and
nonidentity orientation must be normalized beforehand. Temporary GPU maps retain
their historical decoder admission and explicit-role interpretation. Ancillary
PNG color declarations do not change their encoded samples. Portable design-owned
imports require normalized sRGB declarations for base color and validate static
metadata before publication. The native CPU metadata worker also rejects conflicting
base-color declarations. Normal and scalar roles retain their raw numerical channels.
The cooperative native CPU map worker accepts static noninterlaced 8-bit PNG layouts
and refuses JPEG, interlace, APNG and other depths. Normalize these inputs to the
PNG contract before using them on CPU. Import stores exact supplied bytes. The
sRGB base/data normal distinction follows the public
[Khronos glTF material semantics](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#materials).

At most 128 exact name-and-index body bindings retain 64 MiB of encoded capacity.
Native 64-bit capture admits 512 MiB of normalized float4 base pixels. Wasm and
32-bit capture retain 96 MiB. Capture capacity and device residency have separate
limits. Admission accounts for captured sources, decoder scratch, normalized
pixels, prepared mips, staging and device storage against the selected job budget.
Codecs run on the worker. Cancellation is checked before and after bounded decode
and during conversion and upload preparation. Public status contains metadata
only. Completion or cancellation releases captured source bytes after work
retires, including when a caller retains a job or frame handle.

With `texture_filtering` enabled, GPU upload preserves exact RGBA8 source texels.
The shader decodes sRGB base-color texels before linear filtering and reads normal
and scalar base texels as UNORM8 data. Derived levels use linear RGBA16_UNORM at
eight bytes per texel. Each mip is generated from the float pyramid before
quantization. The tail quantization error is at most 1/131070, approximately
0.00000763 in absolute linear reflectance or data. CPU mip values retain float
precision. Without the flag, GPU retains the established float4 storage for
legacy and portable maps.

OPUS's 74 distinct 512×512 maps require 310,378,496 float4 base bytes and
413,837,600 bytes with complete CPU float4 mip chains. With the quality flag, the
GPU mip chains require 129,324,176 bytes before lookup records. The full texture
buffer fits a 128 MiB storage binding. Admission separately checks geometry,
film, device limits and the complete job budget.

The GPU executor selects a bounded opaque pipeline for map-free scenes with
no physical transmission or active volume. Mapped scenes, Glass and active volume
rendering use the general pipeline. This avoids the RADV specialization's
mapped-surface normal corruption; captured filtering and face precedence remain
authoritative.
The map-free opaque specialization has one private media-stack entry; the
general pipeline retains sixteen. The authored nested-media policy still controls
admission and boundary transitions. Captured scene options select at most six cached programs.

The independent progressive device requests six storage buffers per shader
stage. An adapter limited to fewer cannot install this executor; raster and
tessellation device requirements remain unchanged.

CPU completes the Effects→Maps→Aging closure before the hit's direct/GI BRDF,
PDF and proposal, and before deterministic denoise guides. Supported combinations
are documented in [CPU Path](render-cpu-path.md) and [CPU Glass](render-cpu-general-glass.md).
Maps add no rays or sampling dimensions. Omitted maps allocate no prepared cache
and retain the original image calculation. CPU work and registered-executor
resource Arcs move into work; public requests retain summaries. Completed frames,
raw EXR masters and status handles do not retain source bytes after work release.
`work_released`, rather than a terminal phase alone, remains the retirement barrier.
Cancellation, decode failure and late cancellation cannot publish a partial cache
or completed image.

Independent f64 tests cover explicit sRGB decoding, linear normal channels,
repeat filtering, handedness, degenerate fallback, admission and lifecycle.
GPU reference tests compare the actual packed storage and WGSL sampler/normal
frame against those CPU utilities. GPU execution and final visual evidence must
be reported separately; source/shader validation alone is not rendering proof.
