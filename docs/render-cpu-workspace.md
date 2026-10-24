# CPU controls in the Render workspace

This incremental panel is built on advanced CPU source tree
`6e5db1e64376164878918eb68f1e5b311740afcd`. It prepares the same existing
`render.preflight` and `render.start` commands. It does not add a renderer, material asset or rendering dependency. The subsequent
numeric setup unit persists the portable subset through `RenderSetup.job`; see
[workspace settings](render-workspace.md#settings-and-product-camera).

Choose **CPU ray tracing** in Render. Three collapsible sections provide the
following controls; the existing AA 1/4/16, lighting presets, shadows and exposure
remain available in **Single image** sampling mode.

## Progressive CPU sampling

**CPU sampling mode → CPU progressive** accumulates samples of the existing CPU
integrator. Set **samples per pixel** from 1 to 64 and the separate CPU sampling
seed. Selecting the mode explicitly chooses one primary sample per pass; returning
to **Single image** restores the AA selector without discarding lights or HDR controls.
This is camera sampling of the existing integrator, not diffuse global illumination,
adaptive sampling or CPU denoising.

The initial progressive path accepts isotropic opaque surfaces and the supported
directional, point and spot lighting. Selected reflections require one reflection
sample. Area lights, LEDs, dielectric overrides and anisotropy must use Single image;
switching modes never silently removes these inputs. Check and Start report an
incompatible combination through the same request validation.

Admission allows at most 1,048,576 pixels, dimensions up to 4096, and
16,777,216 pixel × sample evaluations. For example, 1024 × 1024 supports 16 SPP,
whereas 64 SPP requires at most 262,144 pixels. Increasing SPP or loading a setup
never changes its dimensions automatically. If needed, the panel displays a
**Use the largest size at this aspect: W × H** action; only clicking it resizes
the draft. Save applies the same numeric limits without requiring a renderer and
leaves invalid drafts intact. Traversal, sampling and image encoding also share
the backend's cumulative work budget.

The result canvas shows provisional PNG checkpoints after 1, 2, 4, … samples,
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
path. This panel does not claim diffuse global illumination, caustics or a general
glass renderer.

## Environment capture

**Constant** environment supplies secondary reflected/transmitted radiance. It
is not direct diffuse lighting and does not replace the image background.
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
round-trip or clamp it. A constant environment and HDR are mutually exclusive.
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

Area lights, their sampling/seed and spot cones remain without dedicated panel
editors, but valid values loaded from a setup are preserved in Save, Check and Start.
Directions retain their original nonzero magnitude; showing a field does not normalize
or clamp it. Emission, surface effects, dielectric bindings and anisotropic faces
remain runtime API capabilities that this panel explicitly refuses to edit. A later
material/optics editor can expose those contracts separately.

## Integration and acceptance

This unit includes EN/ES/JA/ZH strings, request-contract regressions for body
identity and method isolation, environment bounds and light validation, delayed
picker protection, and an extended native UI test that selects a reflection mode,
checks and starts the same request, changes the HDR source after capture and
compares exported PNG bytes with the retained frame. The progressive UI adds
regressions for sequence caching, final publication racing the status read,
cancellation/export retention, explicit resolution adjustment, offline Save/Reload,
hidden values and reopening progressive setups with embedded HDR.

The author checked Rust formatting, locale-key coverage, whitespace and exact
patch replay. No Cargo build, test execution, native launch or visual quality
acceptance was performed for this incremental source unit. The integrator must
run the combined revision's CI and the native UI/render tests before treating
those regressions as passing. CPU controls and the GPU material editor have
separate modules and request builders; both must retain their final admission
guards when mounted together.
