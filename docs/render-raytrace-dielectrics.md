# Bounded CPU dielectric preview (#1655)

This page describes CPU dielectric transport **without explicit Path**. The
published source's renderer, document, engine and workspace tests executed in
CI7; the full gate remains red and native Glass visual acceptance is pending.
Explicit Path has the separate [general Glass](render-cpu-general-glass.md)
contract. See [qualification](render-parity.md#qualification). The render and engine crates keep `raytrace-cpu` optional; the application controls whether its normal
build enables it. The CPU executor remains native only. This unit reuses the image-job host,
queue, cancellation, admission lease, incremental BVH traversal and PNG encoder.

`render.preflight` and `render.start` accept explicit runtime material overrides
on the CPU route (`integrator: "raytrace"`, without `progressive: true`). GPU
progressive requests retain their own dielectric schema and transport. Neither
path infers glass from a saved appearance's alpha value.

Without Path, CPU preflight reports `transparency: "physical_transmission"` when
its request contains admitted dielectric overrides, with `dielectric_boundaries` limited to
`["thin_sheet","closed_box"]`. Without overrides it reports `"opaque_only"` and
an empty boundary list. This metadata describes the requested transport; both CPU
raytrace cases still require surface alpha one. The optional transparent PNG
background exports primary geometry coverage; glass does not gain a compositing
matte for a different transmitted background.

The native host's `descriptor_for(Raytrace)` advertises `refraction` in its
Preview / Image / PNG profile, and CPU requests containing dielectric overrides
require that capability during both preflight and host submission. This is the
bounded `thin_sheet` / `closed_box` contract below, shared by fixed and progressive
CPU sampling: it does not advertise general closed-mesh transmission, caustics
or diffuse global illumination.
Raster and the cooperative CPU host do not advertise this capability. The
`render.backends` registry publishes the additional `cpu-raytrace` descriptor only
for an installed native CPU worker with the build feature enabled. New unsaved
setups use CPU progressive when this host is available and legacy defaults are
off; saved, legacy and offline setups retain their existing route. The explicit
CPU raytrace route resolves its own integrator-specific descriptor. See [backend discovery](render-backends.md#installed-image-methods).

A CPU request example:

```json
{"integrator":"raytrace","settings":{"width":320,"height":240},
 "raytrace":{"shadows":false,"environment":{"color":[1,1,1],"intensity":1},"dielectrics":[
   {"body_index":0,"body_name":"exact captured body and occurrence name",
    "boundary":"closed_box","ior":1.5,
    "absorption_per_mm":[0.1,0,0],"thickness_mm":0}
 ]}}
```

Both the captured ordinal and exact captured name must match. There are at most
eight overrides, each with a name of 1–1024 bytes. IOR is finite in 1–3;
absorption channels are finite in 0–100 inverse millimetres. Missing overrides
use the established opaque surface. Alpha is still required to be one and never
selects glass. These values do not change geometry, mass, appearances, undo or
document persistence. There is no new saved render setup or format change.

`closed_box` is a deliberately strict certificate: exactly twelve nondegenerate
triangles at the corners of a positive-size world-axis-aligned box, with outward
winding and two complementary triangles tiling each of its six faces. Extra
subdivision, rotated boxes, curved solids, incomplete faces, duplicate coverage
and unknown boundaries refuse. A mesh flag or a user-selected boundary enum does
not establish closure. `thickness_mm` must be zero: actual geometric entry/exit
positions determine distance. The original outward geometric normal and front
face flag select the interface; interpolated shading normals do not bend rays.
Parallel slab entry/exit produces the Snell lateral displacement within this
certified subset. This does not certify arbitrary CAD tessellations.

`thin_sheet` accepts one nondegenerate planar triangle or two consistently wound,
coplanar triangles sharing one unique edge and lying on opposite sides of it.
It has no retained interior medium. Its explicit virtual normal thickness is
finite in 0–10000 mm, and is used only for absorption along thickness/cos(theta_t).
Zero virtual thickness uses zero optical distance, including exact grazing. With
equal indices this yields zero reflection and unit transmission. Positive virtual
thickness at index-matched grazing is explicitly unsupported because the ray
does not cross the slab; it is not classified as total internal reflection.
The effective reflection/transmission weights sum incoherent internal returns
between two parallel interfaces. Transmission keeps the original ray direction
and starts at the same surface: there is **no lateral displacement**, modeled
second geometric face or thin-film interference. Closed slabs are not two sheets.

The smooth-interface model uses Snell refraction, exact unpolarized Fresnel and
total internal reflection. Bulk attenuation is exp(-sigma * distance_mm) per
channel, measuring distance between the original geometric boundary positions
before numerical origin offsets. Radiance transport includes the squared IOR
ratio at transmission; a parallel air/glass/air traversal cancels these factors.
All contributions accumulate in linear light, followed by exposure, Reinhard and
sRGB encoding once. The implementation is original safe Rust written from public
optics equations; no rendering-engine source was consulted or adapted. The
[Snell equation](https://openstax.org/books/university-physics-volume-3/pages/1-3-refraction)
was consulted as a mathematical reference only; no text or assets were copied.

Primary misses preserve the request's display-sRGB background bytes, independent
of exposure, or zero RGBA when transparent background is requested. Reflected/transmitted misses default to black. An explicit runtime
`environment:{color:[r,g,b],intensity:n}` supplies a constant secondary radiance:
finite linear-sRGB channels in 0–1 and relative intensity in 0–64. It is separate
from the display background and receives the same one-time exposure/display
mapping as the accumulated optical contributions. Unit white environment radiance
maps to approximately 188/255 at zero exposure. It is not HDRI lighting and does
not illuminate opaque diffuse surfaces. Transparent PNG exports primary geometry
coverage with straight RGB; transmitted/reflected paths and raw EXR are unchanged.

Transport deterministically splits Fresnel branches using a fixed nine-slot DFS
stack, at most eight dielectric boundary events and 511 paths per pixel. An
unresolved dielectric continuation at that depth contributes black: this is an
explicit finite-depth preview that can lose energy, especially near TIR. It is
not an unbiased or converged final renderer. There is one active homogeneous
medium. Nested, touching and overlapping volume bounds refuse, as do sheets
touching/intersecting a volume, camera/near-plane origins inside a volume and an
exit without its matching entry. Box edges/corners with an ambiguous interface
normal refuse. Opaque surfaces terminate transport with RT2
direct lighting. Hard shadow visibility treats dielectric triangles as blockers;
transmitted illumination, colored shadows and caustics are not implemented.
The `shadows:false` fixture explicitly disables that conservative visibility.

No per-path heap allocation, new worker pool, cache or database is introduced.
One fixed optical task slot is reserved fallibly before processing pixels, with
its actual Vec capacity checked; the path stack remains fixed inside that slot.
Admission adds the prepared primitive table plus 32 KiB for bounded runtime
options, traversal/path/certificate/light state; each of the two retained option
copies has at most eight bounded names. Actual task/state/option capacity bytes
must fit that 32 KiB allowance. The original request's directional-light vector,
dielectric-selector vector and name capacities have a separate lease in the same
host pool, held by the immutable status record independently of PNG retention or
eviction. Original capacities are validated before cloning. Total job and shared-host limits remain
384/512 MiB for the compatibility Compact envelope. Current closed boxes may
contain 12 through 65,536 boundary triangles; every triangle must remain on the exact
outward box boundary and satisfy the unchanged edge/area certificate. Fallible
triangle/edge/sort scratch is prepaid for the largest selected box and reused
between boxes, in addition to the fixed state above. Other CPU profiles retain
their separately documented limits.
Preparation, all traversals and optical transitions consume the existing
cumulative billion-unit budget and cancellation token; limit exhaustion fails
the job without publishing a partial image. Each task unit performs one cursor
operation and at most one bounded optical transition; a task call processes at
most four units or four encoding rows. These are logical work/memory bounds,
not OS timing or RSS guarantees.

Source regressions cover F0=0.04, Snell/reversibility, TIR, absorption and slab
offset scaling, closure/winding/identity refusals, nested-volume admission,
bounded runtime DTOs, cancellation and small decoded PNG controls for clear
volumes and 2/8 mm absorption. These renderer regressions have executed.
Required acceptance also includes an original native scene with identifiable
background geometry, paired oblique slab PNGs, physical/persistence invariants,
work exhaustion, cancellation and captured-resource release. Current source
tests alone do not establish this visible acceptance or qualify the draft PR.

## Thin-sheet polarization correction

The incoherent internal-return series is evaluated separately for s and p
polarization. For one polarization with interface power reflectance `F` and
one-pass Beer factor `a`, its sheet response is
`R = F + (1-F)^2 F a^2 / (1-F^2 a^2)` and
`T = (1-F)^2 a / (1-F^2 a^2)`. The unpolarized result is the mean of the two
completed responses. Averaging the interface reflectances first changes this
nonlinear series and is incorrect at oblique incidence.

An independent lossless air-to-IOR-1.5 Brewster case has `Rs = 25/169` and
`Rp = 0`, hence sheet `R = 25/194` and `T = 169/194`. The previous draft gave
`R = 50/363`, exceeding the correct value by `625/70422`. The source regression
checks these rational values and the actual image consumer with an unlit
opaque transmission target and a unit white secondary environment. That image
must encode reflected energy as an sRGB byte of 95; the old series gives 98.
Normal incidence, matched indices, and absorbing Brewster cases are retained
as separate source regressions.

This correction preserves polarization only inside the analytic parallel-sheet
closure. The existing transport still carries a scalar unpolarized response;
it does not add full Stokes transport or change the closed-volume approximation.
The runtime schema, boundary certification, millimetre units, path limits, and
virtual-thickness geometry contract are unchanged. The rational-value and
actual decoded PNG regressions have executed in the CPU renderer suite; native
visual and combined workflow acceptance remain separate.


## Saved optical bindings and common Glass controls

The existing stable `RenderSetup.optics` contract can also be consumed by an
explicit CPU job. Definition/feature/component/occurrence identity resolves into
an exact visible captured body name and ordinal at Check/Render. The CPU request
uses its existing `DielectricOptions`; no engineering material, product opacity,
physical transmission, CAD geometry or mass changes.

Without Path, saved `closed_box` and `closed_mesh` intent execute only when the
captured mesh passes the strict axis-aligned-box certificate, including valid
planar tilings within its 65,536-triangle bound. The saved boundary is retained unchanged; a sphere, cylinder,
rotated box or arbitrary closed mesh does not gain support. Thin sheets retain
the strict one/two-triangle planar certificate and authored virtual thickness.
All overlap, near-plane, camera/lens, coat and per-path guards still apply.

Legacy CPU glass supports the authored IOR and per-mm Beer extinction for at most eight
targets. Extinction is 0..100 per mm and virtual thickness 0..10000 mm. Authored
transmissive shadows and any subsurface value refuse explicitly. Common thin
recipes retain their transmissive-shadow flag: turn that flag off explicitly
before rendering on this legacy route. CPU/GPU opaque coats and saved built-in
maps are separate supported contracts; they cannot share a Glass target. Explicit
Path provides general Glass/nested media and diffuse GI under its own certificate;
this legacy route does not gain those features, and no caustic claim is made.

The common Glass editor creates temporary request-local presets and can save
compatible numerical optics with the existing reviewed bind command in one undo.
Reopening a selected stable saved target shows its actual enabled state, IOR,
Beer extinction and boundary without allocating a temporary override on paint.
Edits to those saved controls replace or remove only their exact target in a
cloned image request, so live feedback uses the prospective values immediately.
The document changes only through explicit Save/replacement or Remove. Discard
returns to the saved values. Other saved rows remain intact; stale identities or
changed sources refuse instead of targeting a new body. Mixed legacy temporary
bindings or file-only HDR inputs retain an explicit conflict where they cannot
share this typed request.
