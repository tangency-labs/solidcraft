# CPU finite Sun and gradient Sky

This source change adds the shared finite Sun and gradient Sky intent to CPU ray
tracing with explicit Indirect lighting (`options.path`). It is an opt-in transport
extension; selecting a light never enables GI or replaces another environment.
Raster remains schematic. The implementation and its renderer, saved/runtime and
pointer-UI tests are included in published `00edaf6a`/tree `38a7f0af` and executed
by CI7. Full CI7 remains red, and dedicated native Sun/Sky or Glass image-quality
acceptance remains pending; see [qualification](render-parity.md#qualification).

`CpuJob.sun` uses the existing Sun direction, angular radius in radians
(0.001…π/2), and relative linear-sRGB radiance (0…100,000,000 per channel).
It is distinct from a hard directional source: a hard source retains
`angular_radius_rad: None`, and the zero-radius/default route keeps its existing
producer/RNG/work behavior. The optional Sun shares the eight-directional limit;
point/spot and rectangle limits are unchanged.

`CpuJob.sky` uses the existing portable `SkyEnvironment::Sky` fields: linear
zenith/horizon/ground RGB, world-Z axis, rotation within ±1,000,000 radians and
intensity 0…100. Its radiance is the same unchanged f32 gradient at the GPU's 256
row centres. Longitude rotation is retained but this axisymmetric gradient is
invariant under it. CPU evaluates a selected row directly; it does not generate
an HDR image, quantize to RGBE or allocate an environment plane. CPU uses uniform
solid-angle proposals; GPU uses its existing alias-table proposals. This is the
same radiance target, not a claim of equal convergence or CPU/GPU pixel identity.

Constant, Sky and HDR illumination are exclusive sources. Finite Sun is additive
with any one of them. HDR retains its existing declared-primary policy, intensity
limit 64 and direct sample count; Sky takes one NEE proposal per shaded bounce.
The built-in HDR library requires explicit replacement consent and refuses an
analytic Sun when the selected HDR already owns its solar disk. Generic HDR bytes
have no inferred solar ownership. Display backplates remain separate from light.

CPU Path samples the finite Sun uniformly over its cap. Environment and Sun NEE
and escaped BSDF contributions each use their own power weight against the same
material PDF, with the actual competitor count and depth endpoint. Delta Glass
escape receives full emitter weight after the existing physical Beer segments.
Nearest geometry/area-emitter endpoints, thin-shadow continuation and certified
nested origins retain their existing contracts. Infinite NEE connections through
closed Glass are still outside the existing supported connection domain, as in
the GPU consumer; this does not add caustic or closed-interface NEE support.

Effects → Maps → Aging still supplies the shared BRDF/PDF and guide material.
Sky/Sun add no guide or snapshot plane. Optional Sun/Sky boxes are billed at actual
`size_of` in the retained request and both owned producer option copies. The
unchanged 4096-byte task slot must contain the enlarged inline option headers;
the static size assertion is retained. The retained Record's optional Sun/Sky
headers and inline Volume header are summed once by the shared fixed metadata
helper on every captured request, including raytrace=None. Regrade pays the same
actual-size sum in its retained request lease, independently of its presentation/
master lease and without duplicating source texture/HDR costs. Thus default
pixels/work/buffers stay identical, but its admission metadata estimate honestly
includes this sizeof delta; no previous layout is fabricated. Every new Sun stage consumes the existing
cumulative work ledger before mutation, and the next bounded host quantum observes
cancellation. None skips that stage and allocates no celestial heap state.

The common light editor saves/loads/filters finite Sun on either renderer. CPU
Sky is available from the Environment source picker only with explicit Indirect
lighting. Numeric load, Undo and environment edits preserve the shot camera.
When a Sun/Sky setup switches between CPU and GPU, its exact source values and
common rig are converted prospectively. Incompatible domains refuse atomically.
For GPU→CPU with no CPU Path, a separate reachable method action explicitly plans
Indirect depth, CPU progressive SPP/seed and traced shadows, then validates and
persists one Undo transaction. It retains the chosen CPU profile, adaptive policy,
presentation, lens and saved scene intent; it never raises a cap or clamps depth.
The same transaction includes an authored Atmosphere box, so a combined setup
never uses sequential effect writes or an obsolete GPU-only Volume refusal.
Atmosphere still refuses incompatible Glass/SSS, noise reduction and transmission
strata without clearing their values.

The published suites cover independent per-ordinal/energy cases, HDR addition,
complete PNG prefixes, raw FLOAT32 masters, lens/adaptive/certified-Glass routes,
map/aging closure, work/cancel and retained capacity, plus saved/common API and
pointer-UI fixtures. Native preview/final/export/reopen evidence for each such
optical combination remains separate. Results from an earlier binary or source
do not qualify a new implementation.
