# Runtime studio lighting

`render.preflight` and `render.start` accept `lighting` and `tone_map` when
`progressive: true`. They affect the captured job only: saving a design or render
setup does not persist them. Omitting lighting retains the three legacy directional
lights and their sample stream. Omitting tone_map retains Reinhard.

```json
{
  "progressive": true,
  "samples": 256,
  "max_depth": 8,
  "lighting": {
    "environment": [0.5, 0.5, 0.5],
    "area_lights": [
      {"center": [-200, -150, 300], "u": [100, 0, 0], "v": [0, -80, 0], "radiance": [8, 8, 8]}
    ]
  },
  "tone_map": "aces_fitted"
}
```

All radiance is linear RGB, bounded to 0..1000000. Environment defaults to black;
use equal RGB components for white illumination. The camera background remains
the requested backplate. The environment illuminates surfaces and is visible in
reflections/refractions. An empty lighting object makes an unlit studio.

Each of at most four rectangles uses world-space millimetres. `u` and `v` are
orthogonal half-edges, each 0.0001..1000000 mm long; the rectangle spans center
plus/minus each half-edge. Its emitting side faces `u cross v`. Centers are finite
and bounded to +/-1e12 mm. Rectangles are opaque, black on their back sides, visible
to camera and reflected rays, and occlude other emitters. No lights are added to
the model or product-scene body list.

The CLI accepts `--progressive --lighting FILE.json --tone-map aces_fitted`.
The file is the lighting object alone (at most 64 KiB); `reinhard` is the other
accepted tone map. Exposure remains `--exposure EV`.

## Estimator and output

Every opaque hit samples each rectangle once in area and the constant environment
once on the sphere. Rectangle density in solid angle is distance squared divided
by area times the emitting cosine. The environment density is 1/(4*pi). These
are combined with the BSDF continuation by the power heuristic on both direct
samples and emitted radiance reached by continuation. Each light is sampled, so
there is no light-selection probability. The actual opaque sampling density
includes its (1 - transmission) branch probability. Delta events receive emission
weight one. At the final allowed surface bounce, direct samples have weight one
because there is no continuing BSDF sample. Shadow intersections stop at the
sampled emitter; geometry beyond it cannot shadow it. See [glass shadow connections](render-glass-shadows.md) for thin-sheet MIS and
the opt-in solid straight-through direct-light approximation.

Accumulation stays linear. Only final output divides by coverage, applies exposure,
then one selected tone map and sRGB encoding. `aces_fitted` uses Narkowicz's published
rational fit; it is not the full ACES colour-management pipeline. The backplate
compositing contract is unchanged.

## References and validation

The implementation is original from the estimator mathematics in
[PBRT fourth edition, section 13.4](https://pbr-book.org/4ed/Light_Transport_I_Surface_Reflection/A_Better_Path_Tracer)
and the published coefficients of
[Narkowicz's 2016 curve](https://knarkowicz.wordpress.com/2016/01/06/aces-filmic-tone-mapping-curve/).
Tests compare rectangular illumination to independent f64 surface quadrature,
exercise blockers before and beyond emitters, compare continuing and final-bounce
MIS estimates with a white furnace, validate bounded requests, and check that
preflight leaves the document unchanged. GPU tests require an actual supported
adapter when SOLIDCRAFT_REQUIRE_GPU is set; no visual milestone is implied by these
numerical tests.

## Hard directional sources

The optional `lighting.directional_lights` array retains up to eight sources:
`{"direction":[0,0,1],"color":[1,0.5,0.25],"intensity":2}`. Direction points toward
the source, is finite/nonzero, and each component is within +/-1e9. Linear RGB is
0..1 and relative incident intensity is0..64, matching CPU directional intent.
The optional finite Sun and this list share an eight-source total. The finite Sun
keeps its angular-radius, radiance and continuous-MIS contract. Hard sources use
delta next-event samples with MIS weight one and consume no random numbers; they
are not enlarged disks. Existing rectangle, environment and Sun sample ordering
is unchanged. Surface/transmissive and external-medium visibility are shared with
the existing source loop, with no extra inlined BVH call site or storage binding.

Omitting `lighting` leaves the exact implicit three-source path. An omitted or
empty directional list adds no source or RNG consumption. Materializing the
implicit studio into explicit hard directions preserves those three values.
Zero-intensity/black sources remain authored but perform no sampling/visibility.
Point/spot lights and mesh emission remain unsupported by this GPU consumer.

The fixed uniform grows by256 bytes, prepaid as device+host upload storage. Its
final `studio_weights.w` lane stays positive zero, preserving the portable
geometry arithmetic ABI. Nondefault captured options live in one exact-capacity
fallible allocation with a retained lease under the existing request/job/host
caps. Structural defaults allocate no options slot; authored `Some` values are
never collapsed merely because they look like an identity at runtime.


## Point and Spot sources

`lighting.punctual_lights` is an optional bounded array of eight combined Point
and Spot sources, separate from the eight directional and four area slots.
Positions are world millimetres, RGB is linear, intensity0..64 is defined at the
explicit reference distance0.001..1e6mm. Minimum distance0.001..reference is an
omission boundary, not a brightness clamp. Spot emitted axes point FROM the
source; half cones retain exact authored radians. Smoothstep is in cosine space.
GPU feathers with cosine gaps below1/1024 explicitly refuse before source I/O;
CPU retains its wider existing f64 cone domain.

Each delta source is evaluated once at every admitted opaque or external-volume
vertex, including the final allowed bounce, with MIS weight1 and no random
numbers. Physical hit-to-source deltas use camera-relative high/low source and
hit positions. The robust shadow connection is re-aimed separately, excludes the
source endpoint, respects opaque blockers and existing thin/solid transmission,
and applies finite external-volume Beer attenuation. This does not broaden the
existing glass/SSS/volume compatibility policies or add refracted caustic NEE.

The additional fixed uniform is640bytes, charged for both device and host
upload. Three64-bit omission counters append24bytes to the existing failure
storage and readback, prepaid twice. Full nondefault `JobOptions` remains charged
by its exact retained slot size. No image plane, extra storage binding, cap raise
or new light-selection RNG dimension is introduced. Default/absent-list numerical
streams are protected by retained zero-source/mixed-family regressions, but
cross-platform byte identity, device validation and native usability still need
execution on this staged candidate.
