# CPU bounded Atmosphere

This implementation and its renderer, host-ownership, document/engine and pointer
regressions are in published source `00edaf6a`/tree `38a7f0af`, tested by CI7.
Full CI7 remains red. Native Atmosphere/Glass image quality and performance are
not established by the opaque Sample Plate capture. See
[qualification](render-parity.md#qualification).

CPU Indirect lighting may consume the existing `HomogeneousVolume` contract: one
world-space axis-aligned box, scalar extinction in inverse millimetres, RGB
single-scattering albedo, and HG phase bias using propagation directions. It is the
same bounded fog domain used by GPU Atmosphere, not a weather model, density grid,
heterogeneous medium, spectral solver or emissive volume.

The portable field is `CpuJob.volume`; the runtime field is
`raytrace.volume`. Both preserve `min_mm`, `max_mm`, `extinction_per_mm`,
`scattering_albedo` and `anisotropy`. Coordinates stay within ±1e9 mm, each extent
is at least 1e-6 mm, extinction is 0–1000/mm, each albedo component is 0–1 and phase
bias is −0.95–0.95. Existing image, geometry, cumulative work, memory, ray-distance,
codec, 4096-byte host task and 1024-byte completion metadata caps remain unchanged.
Those layout assertions compiled in the published CI7 source; the passing scoped
renderer suite does not establish native Atmosphere image quality or performance.

Positive extinction requires explicit progressive CPU Indirect lighting. It cannot
combine with Glass, selected-medium subsurface scattering or surface Denoise.
Existing selected-reflection/anisotropy Path restrictions remain in force. No
setting is silently cleared, no fixed saved sampling is adopted automatically, and
an unsupported request fails with its authored values retained. CPU has one Path
attempt per film ordinal; GPU-specific Fresnel-stratum controls are not introduced
as CPU transport parameters. Zero extinction stays authored while bypassing all
new worker phases, random dimensions and heap owners. Omitted volume keeps the old
serialized CPU wire. Literal fixed layout/admission byte identity with an older
binary is not claimed.

## Physical transport and model visibility

A perspective ray uses its actual eye or sampled pupil as the optical start, so
fog before the model near plane is not lost. Its original model near interval is
retained. For orthographic projection, the optical start is the earlier of that
original model near plane and the fog box's signed line entry; the model interval
moves by the same ray parameter. The pixel line, camera, captured geometry, model
bounds and framing never move. Far optical queries include the box even when it
extends ahead of or beyond the model, or when the scene contains no model at all.
A box extending beyond an actual admitted infinite-ray distance refuses explicitly.

The single finite path samples an exponential flight within the forward ray/box
segment, before its closest visible model or analytic-emitter endpoint. A collision
applies RGB scattering albedo once, contributes foreground coverage, counts one
Path event and evaluates direct lighting with the HG phase function. Continuation
uses the shared exact HG sampler. Surviving sampled paths carry no-event probability
in their endpoint density without multiplying throughput a second time. Absorption
is represented by the complementary probability and scattering albedo; it is not
an additional double attenuation.

The existing bounded directional, point/spot, rectangle/LED and constant/HDR
lighting continuations provide next-event samples. Volume vertices use their
physical position and HG response/PDF; opaque vertices retain the completed
Effects→Maps→Aging closure. Visibility is blocked by the same opaque geometry and
other analytic emitters, and its box segment supplies transmittance to both the
NEE contribution and matching phase/BSDF competitor density. Source sample counts
and finite-depth endpoints retain complementary MIS. No new BRDF/PDF/clamp body or
refracting-boundary approximation is added. The composed Sun/Sky stages use the same
medium-aware response/PDF and physical source helper; combined phase/escape and
SSS-delta regressions are included in the executed published renderer suite.

Active-volume opaque launches use the existing exact producing-plane exclusion.
A cooperative primitive-range walk charges one neighboring primitive per step,
avoiding a surface normal offset that would change the fog length. Interior volume
launches have no surface offset. An actual collision that aliases a source,
endpoint or box boundary refuses rather than moving geometry or clipping radiance.

## Work, retained ownership and prepared oracles

One optional context lives in the already prepaid finite Path state. Actual State
size pays pending source/event/range/MIS metadata; there is no ray tree, extra film,
image plane, source image or decoder allocation. The inline runtime volume field
adds a fixed request-header charge on every retained job, even for absent CPU
options. Ordinary capture/preflight share the fixed metadata helper; regrade pays
its own header once in the Record request lease and does not rebill the source
HDR/texture bytes. This lease survives work release and an absent frame until the
last status handle drops. Held frame/presentation metadata retains its existing
separate lifetime. The composed Sun/Sky and inline volume headers are summed once.

Every new worker operation debits before changing state. Box/free-flight/phase
math is bounded, range discovery visits one primitive per debit, and traversal
uses the existing cooperative cursors. Cancellation or work exhaustion cannot
publish a late sample, checkpoint or master.

Prepared regressions use independent exponential CDF, analytic slab/box lengths,
HG formula/direction and light-sampling loops for depths 1/2/16. They cover pure
absorption, a real perspective near prefix and pupil, orthographic foreground and
background union, empty-scene coverage, single/multiple scattering, white-furnace
energy, opaque blockers, finite rectangle and real nonconstant HDR sources with
1/4/16/64 samples, survival-density MIS, exact physical source/range debits, every
PNG prefix, raw FLOAT EXR/alpha, finishing, adaptive pilot separation, worker quanta,
cancellation and retained-header release. Typed saved/runtime and cold-container
Start/Undo tests preserve the camera, geometry and engineering materials. The
shared Atmosphere adapter is composed with the celestial editor: one prospective
method transaction preserves Volume, Sun, Sky, the analytic rig, camera and full
authored sampling/output intent. A known volume-only edit retains mute/solo/light
selection only with exact source provenance; unknown source replacement clears it.
Actual pointer, save/Undo/cold and combined native Start/raw-master assertions are
included in the published UI test module and CI7 execution. This is regression
coverage, not native Atmosphere image-quality acceptance or a green full gate.
