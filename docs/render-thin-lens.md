# Physical depth of field

The Frame editor exposes the same physical thin lens for CPU ray tracing and GPU
path tracing. Lens changes save numerical intent in one undo step; they do not
move the view or replace the saved shot camera. Positive CPU aperture requires
explicit progressive sampling. A saved fixed CPU shot keeps its sampling choice
until the user adopts progressive sampling in Sampling.

`render.preflight` and `render.start` accept the GPU runtime lens:

```json
{"progressive":true,"samples":256,
 "lens":{"aperture_radius_mm":0.5,"focus_distance_mm":120}}
```

Use a perspective camera in `settings.camera` or the current view. Focus distance
is measured from the eye along the forward axis, to a plane perpendicular to that
axis. It is not distance along each ray. Aperture is the physical radius in mm,
not an f-number. Units follow the scene; a radius of 0.5 mm is small for a room and
large for a 3 mm object. Saved CPU and GPU jobs both use `job.options.lens`;
the setup camera remains their single saved camera authority. The CPU runtime
form is nested inside `raytrace`, rather than the GPU top-level `lens`:

```json
{"integrator":"raytrace", "settings":{"width":512,"height":512,
 "camera":{"target":[0,0,0],"yaw":0,"pitch":0,"fov":0.7,"distance":120}}, "raytrace":{
 "progressive":{"samples_per_pixel":16,"seed":81}, "primary_samples":1,
 "lens":{"aperture_radius_mm":0.5,"focus_distance_mm":120}}}
```

Saving, reopening and undoing lens intent work without an installed render host.
Neither a missing lens nor an explicitly saved zero aperture is inferred into an
active lens. Legacy fixed CPU and orthographic images keep their original path.

A uniform-area circular aperture uses `r = radius * sqrt(u)` and `phi = 2*pi*v`.
Each primary ray starts at that aperture point and aims at the pinhole ray's
intersection with the focus plane. The image is the average of these radiance
samples: changing aperture changes blur, not an aperture-area exposure factor.
Exposure remains the existing explicit EV control. Circular bokeh has no blade
shape, diffraction, lens distortion or chromatic aberration model.

Omitting the lens or choosing radius zero preserves the existing pinhole or
orthographic image bit-for-bit on the same adapter, including its random stream.
Positive aperture requires perspective; orthographic requests fail admission.
Radius is finite in [0, 1e7] mm and focus distance in [1e-6, 1e9] mm. Active lenses
also require camera distance >= 1e-6 mm and tan(fov/2) in [1e-6, 1e6] to keep the
f32 basis and focal point bounded. Existing image and scene caps still apply.
The whole aperture must remain outside glass volumes, as must the pinhole camera.
CPU admission conservatively requires the sphere containing the entire circular
pupil to stay outside every certified closed glass box, including contact. This
can refuse a disk that would fit beside glass; it does not guess its starting
medium. Existing near-plane and per-path certification checks still apply.
CPU also checks the whole pupil's possible near/far ray interval against its
existing coordinate and distance envelope before starting transport. Extreme
physical lens/camera combinations that exceed that envelope refuse explicitly.
No general glass geometry or medium nesting is added.

Tests read rays from the production GPU camera function and from the CPU camera
generator, comparing against analytical focus and defocus oracles with an oblique
rolled camera. CPU image tests also check actual defocused coverage, every
complete-pass prefix, worker-quanta independence, zero-aperture PNG/master/work
identity, cancellation and conservative glass admission. The CPU pupil sampler
uses a separate counter domain keyed by seed, pixel and absolute film ordinal;
it adds one charged continuation per camera sample, no extra rays or image
buffers. Existing job memory and cumulative work caps remain unchanged.
At axial depth z, the displacement from a pinhole ray is
`aperture_point * (1 - z/focus_distance)`: zero at focus, opposite-signed beyond it.
Other tests cover strict admission, zero-aperture image identity and fixed-seed
repeatability with an active lens. Glass evidence is an original procedural scene,
with test-only uniform environment lighting, not a claim of HDRI implementation.

## Historical coordination

The original GPU lens work compared the then-current CPU integration draft
[#1698](https://github.com/tangency-labs/solidcraft/pull/1698), head
`463a3e0daaa6b84f169ba5f8f215fba4642c194a` (2026-10-17), without copying its
source. That draft predates the current CPU lens and certified optical transport.
It is not the current capability contract.

CPU primary geometry uses f64 with outward representable bounds; GPU uses f32
after target rebasing. Sample sequences and secondary-origin policies differ,
so cross-method pixel identity is not promised. Focus-plane geometry and shared
physical units are the analytical comparison. CPU HDR still requires declared
linear sRGB/D65 primaries; adding a lens does not add a gamut conversion or GI.
