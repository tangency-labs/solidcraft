# CPU controls in the Render workspace

The original incremental panel was built on advanced CPU source tree
`6e5db1e64376164878918eb68f1e5b311740afcd`. It prepares the same existing
`render.preflight` and `render.start` commands. It does not add a renderer, material asset or rendering dependency. The subsequent
numeric setup unit persists the portable subset through `RenderSetup.job`; see
[workspace settings](render-workspace.md#settings-and-product-camera).

Choose **CPU ray tracing** in Render. The original three sections below describe
its legacy request controls. The normal workspace also exposes the separately
validated CPU lens, guided noise reduction, opaque indirect lighting and saved
certified Glass controls. Existing fixed setups retain their AA 1/4/16, lighting,
shadows and exposure; adopting progressive sampling is an explicit action.

## Progressive CPU sampling

New normal CPU setups show progressive sampling as always-on status with samples
per pixel and seed controls. Existing fixed setups retain their exact intent and
show an explicit **Use progressive sampling** action; opening the page never
converts them. Adoption announces one camera sample per pass and refuses an
incompatible image budget or selected reflection count instead of changing size,
method, lighting or reflections. The previous Single image / Progressive selector
remains available through **Use previous Render image defaults**.
Progressive camera sampling is independent of the optional [opaque path lighting](render-cpu-path.md)
and [guided noise reduction](render-cpu-output.md) contracts. Independent-pilot
CPU adaptive sampling is also implemented. These controls combine only where
their geometry, memory, work and transport admission rules allow; they do not
certify convergence or a completion time.

Progressive sampling accepts the existing supported opaque/direct-light, selected
reflection, certified dielectric and planar anisotropy transport. Selected opaque
reflections require one attempt per camera sample. Without Path, Glass retains
certified sheets/axis-aligned boxes and anisotropy its planar subset. Explicit
Path supports general Glass but refuses selected reflection/anisotropy overrides. Geometry, HDR, clear-coat and optical overlap restrictions
remain explicit. The shared Glass editor saves the certified dielectric subset
with stable body/occurrence targets; see [saved optics](render-raytrace-dielectrics.md#saved-optical-bindings-and-common-glass-controls).
Selected opaque-reflection and anisotropic-face bindings still use the temporary
runtime identity/confirmation contract below.

The Compact resource profile allows at most 1,048,576 progressive pixels, dimensions
up to 4096, and
16,777,216 pixel × sample evaluations. For example, 1024 × 1024 supports 16 SPP,
whereas 64 SPP requires at most 262,144 pixels. Increasing SPP or loading a setup
never changes its dimensions automatically. If needed, the panel displays a
**Use the largest size at this aspect: W × H** action; only clicking it resizes
the draft. Save applies the same numeric limits without requiring a renderer and
leaves invalid drafts intact. Traversal, sampling and image encoding also share
the backend's cumulative work budget. Larger [CPU resource profiles](render-cpu-resource-profiles.md)
remain explicit choices; no profile, size or sample count is raised automatically.

The result canvas shows provisional PNG checkpoints after every complete pass, 1, 2, 3, 4, … samples,
before the requested final count. A provisional label includes its SPP and checkpoint
sequence; the job identity and captured revision remain unchanged. Checkpoint updates
reuse the current pan/zoom. Completion replaces the preview with its final image;
cancellation or failure removes the preview and returns to the previous completed
result with the same framing. **Export final PNG** always exports a completed frame,
even while a different provisional image is visible. With no completed frame, export
remains disabled. The UI keeps one decoded provisional texture and metadata, not
additional retained HDR/PNG buffers.

`CpuJob.progressive` persists the selected sample count and seed. Its absence or
`null` keeps Single image mode. Save As, Reload, setup switching and design reopen
preserve this option together with embedded HDR, nonedited numeric fields and the
setup's single exposure. The runtime CPU request nests it under
`raytrace.progressive`; the top-level `progressive: true` continues to select GPU
path tracing.

## CPU controls

| Section | Controls | Runtime limits |
| --- | --- | --- |
| Reflections | Visible body picker and filter, Off / Mirror / Rough, reflection depth and samples | At most 64 exact body names; 1–8 bounces; 1–64 reflection samples. Rough uses each body's existing opaque surface. |
| Environment | None, constant linear RGB radiance, or a checked/embedded Radiance HDR; intensity, HDR rotation and sampling | Intensity 0–64; RGB 0–1; finite rotation within ±1,000,000 radians (displayed as degrees); HDR samples 1/4/16/64. |
| Lights | Preset lighting or custom directional lights; additional point lights | At most eight directional and eight point lights; nonzero directions; linear RGB 0–1 and intensity 0–64. |

Numeric lighting, AA, shadows and constant-environment values are saved with the
active CPU method. Exposure uses the setup's one exposure field. Checked HDR bytes
and their controls can be embedded through the engine's atomic capture command;
body reflection bindings still cause Save to refuse a full job without discarding
those inputs. None of these controls changes a body's physical
material, geometry or mass. Raster CPU and GPU requests omit the CPU panel's options.
Switching methods retains local numeric drafts; Reload or choosing another setup
replaces them with the saved active method. Only the selected method's schema is sent.

## Body identity and validation

The picker lists visible evaluated bodies and narrows long lists with a filter;
it shows at most 256 rows per opening. Empty names, control characters and names
longer than 128 UTF-8 bytes cannot be selected. Duplicate visible names and stale
model evaluation are refused.

A reflection selection records its document pointer, scene revision and the
visible body's ordinal, creator feature and component. Check and Start refuse a
binding after a scene change. **Confirm this body** can refresh the revision only
when the same target still occupies the selected identity. If another body has
reused its name, remove the old selection and select the desired body explicitly.
This prevents a name-only runtime selector from silently following a replacement.
The engine still performs final capture and surface compatibility validation.

CPU reflections do not implement a general layered clear-coat transport model.
The engine rejects a coated body or face selected for an incompatible reflection
path. Optional opaque path lighting supplies diffuse/specular indirect transport
through its separate explicit control; it cannot silently absorb incompatible
selected optical settings. [General Glass](render-cpu-general-glass.md) is an
explicit Path capability. Optional [Glass material models](render-glass-material-models.md)
add GGX rough transmission, three-band dispersion, one lossless coherent film and
finite-radius hard-source photon caustics under their separate admission limits.

## Environment capture

In the legacy selected-reflection transport, **Constant** environment supplies
secondary reflected/transmitted radiance. Optional opaque path lighting also
estimates its direct and continued environment illumination. Neither route
replaces the separately authored image background.
**HDR** uses the CPU environment contract: Radiance `.hdr`, 2:1 panorama, at most
2048 × 1024 pixels and 16 MiB of source bytes, with explicit supported sRGB/D65
primaries. EXR and files without those primaries are refused. Selected rough
reflectors with HDR must meet the engine's fully metallic body/face constraints.

The picker uses the application's existing file dialog. A delayed answer is
accepted only by the same document, revision, setup, method and input selection.
Selecting a file runs `render.hdr.inspect`; typed paths require **Check HDR file**.
This checks the full bounded stream without a renderer or document mutation and
records its digest and metadata. Request construction itself does not read files.
**Check**, **Start** and **Save** send the same expected digest, so a changed source
is rejected until an explicit recheck. Cancellation and inspection failure retain
the prior selection. Once a job is captured, later file changes cannot alter its
pixels or export.

**Save Setup** uses `render.settings.capture_hdr` for a checked local source, saving
the immutable bytes and complete setup in one undo step. Only success switches the
panel to **HDR saved inside this design**. Further saves use `render.settings.set`;
Reload, setup switching and design reopen never need the original path. The CPU
HDR's rotation, intensity and sample count are saved separately from its source
ID. Rotation remains in radians internally; drawing the degree editor does not
round-trip or clamp it. Constant, gradient Sky and HDR environments are mutually exclusive. Explicit CPU Indirect lighting also supports the shared finite Sun and gradient Sky contracts; see [CPU celestial lighting](render-cpu-celestial.md).
CPU still requires explicit sRGB/D65 primaries; it never inherits the GPU's optional
assumption for an undeclared source.

The last CPU check stores its exact request and revision and displays the captured
HDR dimensions, intensity, rotation, samples and SHA-256. It marks itself stale
when the scene or editable request changes. The active job displays its own
captured HDR descriptor. These UI records retain metadata, not image buffers or
HDR source bytes. Saved HDR bytes belong to the document resource table, not those
view records. An embedded source can be used with temporary reflection bindings
through the same runtime renderer, but the bindings themselves remain unsaveable.
PNG export remains the existing retained-frame export.

## Light units and precedence

Directions point toward a light and are normalized by the runtime; a zero vector
is invalid. Custom directional lighting replaces the preset directions; an empty
custom list deliberately has no directional lights. Selecting a preset clears
that custom list. Point lights are added to the chosen directional setup.

Point positions and distances are world millimetres. The reference distance is
where the specified intensity applies; the minimum distance bounds inverse-square
attenuation and must not exceed the reference. Both distances range from 0.001 to
1,000,000 mm in this panel. Intensities are relative renderer values, not a claim
of lumens, watts or calibrated photometric measurements.

The original panel did not provide dedicated area/spot editors. The shared Lights
editor now exposes supported area and directional/point controls; loaded spot
cones and numeric sampling intent remain preserved in Save, Check and Start.
Directions retain their original nonzero magnitude; showing a field does not normalize
or clamp it. Emission, surface effects, dielectric bindings and anisotropic faces
remain runtime API capabilities beyond the original panel. The current shared
Glass editor exposes only its separately validated saved certified dielectric
subset; it does not make arbitrary optics or temporary bindings portable.

## Integration and acceptance

This unit includes EN/ES/JA/ZH strings, request-contract regressions for body
identity and method isolation, environment bounds and light validation, delayed
picker protection, and an extended native UI test that selects a reflection mode,
checks and starts the same request, changes the HDR source after capture and
compares exported PNG bytes with the retained frame. The progressive UI adds
regressions for sequence caching, final publication racing the status read,
cancellation/export retention, explicit resolution adjustment, offline Save/Reload,
hidden values and reopening progressive setups with embedded HDR.

For the historical incremental source unit, the author checked Rust formatting,
locale-key coverage, whitespace and exact patch replay. No Cargo build, test
execution, native launch or visual quality acceptance was performed for that unit. The integrator must
run the combined revision's CI and the native UI/render tests before treating
those regressions as passing. CPU controls and the GPU material editor have
separate modules and request builders; both must retain their final admission
guards when mounted together.

## Output presentation

The shared **Effects** editor accepts CPU ray tracing and GPU path tracing. Tone
mapping, creative looks, RGB gains, bloom and photographic image effects remain
presentation controls. CPU opaque path lighting, physical lens sampling and guided
noise reduction have their own explicit controls and compatibility checks. Noise
reduction filters complete progressive/final display passes without changing raw
film or the final unfiltered EXR. **Environment → Gradient** is also available for CPU output.
Physical depth of field is configured separately in **Frame → Lens**, through
the shared [thin-lens contract](render-thin-lens.md). CPU aperture sampling uses
the progressive film and requires a perspective camera; disabled or zero aperture
preserves the previous image exactly.
**Keep linear master** retains a final-only native FLOAT32 EXR. The export choice
uses the selected completed image's actual master, independently of the current
draft method or a new provisional render.

These values save under `job.options.output`, reopen without a render host, and
use the setup's single exposure/background authority. The first numerical edit
keeps the visible size and numerical baseline. Merely opening the editor never
saves or starts a job. [CPU output presentation](render-cpu-output.md) describes
the existing shared output bounds and raw ownership.

## Explicit CPU image budgets

The normal Sampling page shares Draft/Balanced/High with GPU, but projects CPU
cost budgets independently from the same Q-001/Q-002/Q-003 catalogue. CPU uses
960×540/16, 1600×900/32, 2560×1440/64 production samples and seed 21026. The smallest
existing profile admitting the complete pilot+production envelope is selected,
without lowering an already selected larger profile. Nonadaptive baseline
profiles are Compact/Studio/Studio. No memory/work/SPP cap is raised.

A CPU chip can change exactly width, height, progressive production sample cap,
progressive seed and resource profile in one reversible edit. Camera, lens,
HDR/light input, GI enablement/depth, denoise, optics, maps, aging, alpha,
finishing, raw-master intent and adaptive policy stay as authored. A preserved
adaptive minimum above the new sample cap refuses atomically. Saved fixed-AA
setups retain the explicit Adopt progressive action before these presets; a
quality click never silently normalizes primary AA or reflection attempts.

These are image-cost presets, not complete optical/convergence quality tiers or
CPU/GPU convergence equivalence. Geometry/memory and cumulative-work admission
still apply to the captured job. Canonical CPU clicks use the same numerical
transaction and current-shot save path; imported GPU quality review remains a
separate acknowledged flow. Merely painting/selecting a page does not apply a
preset or launch a final render.

The CPU budget/quality transaction and its pointer/save/reopen/Undo and hostile
budget regressions are included in the published source tested by CI7. See
[the qualification record](render-parity.md#qualification). Presets are not a
convergence or image-quality certificate; native Glass visual acceptance remains
pending.
