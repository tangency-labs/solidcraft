# Integrated Render workspace (R3)

The native image panel offers CPU raster, optional CPU ray tracing and
[GPU path tracing](render-path.md) as explicit image methods. All three methods can be edited and saved offline. Check and Start are enabled
only when the selected method has an installed supporting host; an unavailable method never falls back.

R4a builds on this viewport contract with a separately advertised CPU product-image
executor and Check/Start/Cancel/Export controls. See [render jobs](render-jobs.md) for
the asynchronous protocol, image limits, captured revisions and PNG export. The legacy
`ui.render` snapshot remains a separate route.

Choose **Render** in the workspace switcher after finishing a modelling operation.
The current integrated GPU renderer shows a separate product scene. Return to Design
restores its camera, projection preference and existing viewport drawing policy.
The live product preview remains independent of the selected image method.

## Image methods and controls

**CPU raster** produces the existing preview PNG with approximate transparency.
**CPU ray tracing** requires a native worker and the `raytrace-cpu` build feature.
Its panel offers **Single image** with 1, 4 or 16 spatial samples per pixel, or
**CPU progressive** with 1–64 accumulated samples and a separate seed. Both use
Neutral / Warm and cool / Key and rim lighting, cast shadows and exposure from
−16 to +16 EV. Progressive sampling has a 1,048,576-pixel and 16,777,216 pixel × sample
limit; the UI proposes a smaller size only as an explicit action. It requires an
opaque background; the panel explains the incompatibility and lets the user clear
Transparent background. Additional collapsible sections expose body reflections,
captured HDR or constant environments, directional lights and point lights. Reflection
bindings require explicit confirmation after a document change; reusing a name cannot
redirect them to another body. Check and the current job display their separate HDR
capture metadata. See [CPU workspace controls](render-cpu-workspace.md) for units,
limits and the immutable capture contract. CPU glass, emission, surface effects,
anisotropy, area lights and spot cones remain API capabilities without panel editors.

**GPU path tracing** preserves asynchronous shader preparation and its timing display.
Its independent image device requires Vulkan; the live viewport may use another backend.
When no compatible Vulkan device is available, GPU settings remain editable and
saveable, while Check and Start explain why GPU execution is unavailable.
The panel exposes progressive sample counts from 1 to 4096 and an Advanced section
for maximum path depth (1–16), random seed and exposure (−32 to +32 EV). One setup
exposure field controls the selected image method. Switching methods retains their
local numeric drafts while submitting and saving only the active method. Reloading
or choosing another setup replaces those drafts with the saved numeric intent.
The live product preview keeps its fixed studio response and does not apply image
exposure, environments, lens or material overrides.

The GPU Advanced section also exposes Reinhard / ACES fitted tone mapping, optional
final edge-aware denoising, and a thin-lens depth-of-field control. Positive aperture
requires a perspective product camera. Denoising preserves raw accumulation but can
soften detail; glass and sharp specular first hits bypass its final spatial filter.

The Environment section selects no map, an original artistic sky gradient, or a local
Radiance `.hdr` panorama. HDR input is bounded to 16 MiB, 2048 × 1024, 2:1, and linear
sRGB/D65 primaries; unspecified primaries require the explicit interpretation checkbox.
EXR is not supported. Rotation and intensity affect illumination, while the saved
background remains the camera backplate. Selecting a file runs the bounded public
`render.hdr.inspect` command; a typed path has an explicit **Check HDR file** action.
The picker belongs to the originating document revision, setup, method and input
selection. A cancelled or stale answer never replaces a newer selection.

The panel distinguishes a pending file, a checked file and **HDR saved inside this
design**. It shows dimensions, declared colour primaries and intensity; the source
digest is available in a collapsed details section. Check, Start and Save use the
checked file's expected SHA-256, rejecting changed bytes until an explicit recheck.
An already captured image retains its immutable source independently of later edits.

Custom studio lighting exposes constant linear-RGB environment radiance, a finite-radius sun,
and up to four one-sided rectangular emitters. World-space centers and orthogonal
half-edge vectors specify their geometry in millimetres; radiance is linear RGB.
An environment map replaces the constant environment term. The path integrator uses
the published emitter/BSDF MIS contract.

### GPU image materials

The GPU image materials section assigns runtime optics to whole visible scene bodies.
Refresh the authoritative body list, explicitly choose a body, and add its assignment.
The captured ordinal and exact name must match; a document identity or revision change
requires explicit revalidation. A removed, renamed or reordered body cannot silently
receive another body's settings. The editor uses weak document identity references, so
stale assignments do not retain historical documents. The normal Appearance and material
library dialogs keep their existing behavior.

Glass, clear coat and planar maps have separate collapsible controls. Six original glass
recipes come directly from the starter pack: linear transmittance over the recipe's
reference distance becomes absorption per millimetre via `-ln(T) / distance`. Selecting
one never changes saved opacity or assigns a scalar appearance. The swatch shows the
recipe's transmittance, not a rendered preview. Manual IOR, RGB absorption, thin-sheet
thickness and straight-shadow transmission are available. Closed solids derive optical
distance from geometry; full topology, orientation, overlap and optical-boundary checks
run during GPU preparation. Successful Check does not certify those geometric properties.

Clear coat exposes weight, roughness and IOR over the existing opaque base. Runtime maps
accept bounded local PNG/JPEG base-color sRGB and OpenGL +Y normal images, normal strength,
and orthogonal world-space origin/U/V vectors in millimetres. Each vector spans one full
repeat. The backend's 1024×1024 per-map, 16 MiB encoded-total and 128-repeat bounds remain.
Glass cannot share one body with coat or maps. Limits are eight glass bodies, 64 coat
bodies and 16 mapped bodies, within a maximum of 64 editor assignments.

All these controls are temporary per-document-tab state, explicitly labelled in the UI.
They are not saved in the design, render setup or portable material library. Disable the
overrides before choosing a CPU method; active GPU assignments cause Check/Start to
refuse a CPU request. Both buttons submit the same freshly validated runtime parameters,
and file contents are captured by the engine at that boundary. File-picker completion
checks the original document, revision and body identity before accepting the path.

Check and Start build the same request from the current product camera, size and
selected method, including when Render Result is open. Every click revalidates host
availability and scene readiness. The engine checks resource admission at Start;
an earlier successful Check does not reserve a queue slot or memory.

## Product scene

`engine::product_scene::extract` consumes authoritative evaluated geometry, reusing
the document's world/occurrence placement and mesh Arcs. Body, component, material,
face and imported appearances retain their existing precedence and opacity. Source
geometry is never transformed a second time or altered to serve a renderer.

The shared L1 `ProductScene`/`ProductMesh` contract contains world-space meshes,
resolved sRGB colours, opacity and face appearance overrides. Face indices are the
existing mesh-to-B-rep mapping, resolved through document appearance references;
they are not new persistent names or triangle-derived face identities.

There are no grid/axis/edge lines, sketches, construction objects, selection or hover
colours, section cuts, analysis shading, active-component fading, canvases or decals
in this initial product policy. Browser visibility still controls which bodies appear.
The existing Design viewport and CPU snapshot routes keep their current policies.

Extraction refuses a stale live-drag model. The explicit model-reading `render.scene {}`
command follows the normal engine evaluation boundary, then returns bounded metadata:
body names, triangle counts, resolved colours/opacity, face-override counts and bounds.
It does not return mesh payloads, change the document or add an undo step. Product
extraction admits at most 16,384 visible bodies.

## Image result canvas

The image panel switches between **Live Preview** (the product camera) and
**Render Result** (the current CPU checkpoint or last completed PNG). Clicking the
small image preview also opens the result on the main canvas. Check, Start, Cancel and Export
remain available in the same panel. An empty result explains how to start a job;
it never substitutes the live preview for an exported image.

**Fit Image** frames the complete image and follows window resizing. **100%**
maps one image pixel to one physical display pixel, including on high-DPI screens.
Pan, wheel, trackpad and Space + drag use the shared Navigation preferences with
orbit disabled. Ctrl + arrow keys pan the image; Fit/Home frame it. Image navigation
does not move the product or Design camera and does not change the saved setup.

The canvas shows the image dimensions, executor, full job identity and captured
model revision. A visible warning identifies an earlier captured revision when the
document has changed. A CPU progressive checkpoint is explicitly provisional and
shows its samples per pixel and sequence. Its texture is cached by job identity
and sequence, separately from the last completed image. **Export final PNG** keeps
using that completed image even while a checkpoint is shown. Cancelling or failing
the active job clears its preview and returns to the retained final image.
Transparency is shown over a checker
background; display scaling and navigation never modify the PNG bytes.

Canvas selection, framing and pan/zoom are local to each open document tab. Switching
tabs never displays another document's texture. A new image starts fitted; later
checkpoints and transitions to final/cancelled state preserve pan/zoom. Final
publication is checked directly after the preview read so a stale status reply
cannot briefly select an older frame and reset the next frame's zoom. The canvas
itself adds no document fields; CPU sampling intent is saved with the job below.

## Settings and product camera

The right panel stages a named setup's camera, background, output and active image
method. **Save Setup** records the complete numerical `RenderSetup.job` and the current
product `Camera` through `render.settings.set`, with one undo step. A new checked HDR
uses `render.settings.capture_hdr` to save both source bytes and setup in that same
atomic step. Editing the name
is Save As; choosing an existing name from the picker loads that setup. **Reload Saved
Settings** replaces the numeric draft with the saved values. Drafts remain separate
for each open document, and loading a setup preserves completed images and result framing.

| Active method | Saved numeric intent |
| --- | --- |
| CPU raster | Transparency, with zero exposure |
| CPU ray tracing | Single-image AA or progressive SPP/seed, preset or custom directional lights, point/spot lights, area lights and their samples/seed, shadows, constant RGB environment or embedded HDR with rotation/intensity/samples, setup exposure |
| GPU path tracing | Transparency, samples/depth/seed, tone map, denoise, lens, RGB studio lighting/sun/area lights, procedural sky or embedded HDR with rotation/intensity/explicit colour assumption, setup exposure |

The CPU panel preserves existing area-light and spot-cone fields even though it does
not yet edit every field. Opening a GPU setup retains non-power-of-two sample counts,
RGB environment values, exact sky gradients and radians. Explicit empty light lists,
black environments and absent settings remain distinct. The only exposure authority
is `RenderSetup.exposure_ev`: CPU requests lower it to their raytrace options with zero
outer exposure; GPU jobs use the same value in their job options. Choosing raster
explicitly selects its zero-exposure response.

Saving uses pure numeric validation and does not require an installed renderer or a
current evaluated scene. New HDR capture reads and validates the bounded source through
the engine; failure leaves the document, resource table and local file selection intact.
After success, the draft changes to the embedded resource and contains no source path.
Saving that setup again, Save As, Reload and design reopen use the included bytes, even
when the original file has been removed. CPU/GPU setups may share the same immutable
payload. The engine limits each source to 16 MiB and the design to 64 resources/64 MiB.

Check and Start use the same complete typed setup and then perform normal host, scene
and capture admission. Temporary bindings can still use an embedded HDR through the
runtime resource selector. GPU maps and all body bindings, including disabled
assignments, cannot yet be saved as a complete job.
The panel explains the limitation and refuses Save before changing the document.
Those inputs can still execute through the existing transient request route. Older
setups with nondefault legacy environment rotation/intensity are also retained and
refused for full-job saving until explicitly converted; their values are never erased.

**Restore Saved Camera** runs `render.settings.apply {name}`, preserving target,
distance, yaw, pitch, roll, FOV and up axis as view state. It neither edits the document
nor settles a stale model. The UI honours its up axis in Render; the Design camera
is restored when returning. The camera moves exactly as in Design: the navigation preset,
Orbit around, zoom to the cursor, the SpaceMouse, the middle double-click, F6 and the axis
view or navigation cube all work (nothing is picked or selected here), and
**Fit Product** frames visible product bodies without including sketches or selection.

Preview drafts do not make the document dirty until saved. Unknown backend preferences
and unsupported modes remain saved intent: the strict resolver reports the unsupported
viewport request. **Use Integrated Preview** is an explicit draft change, never an
automatic fallback. The method selector describes the desired image execution independently of the
installed host. It does not claim support for a saved final-mode or external-engine
preference. Saved image settings do not change the fixed live product preview.

## Host capabilities and resources

`render.backends` always retains the built-in `cpu-raster` preview/PNG image executor.
An installed CPU image host adds `cpu-product`; a native host with `raytrace-cpu`
also advertises `cpu-raytrace`, with its bounded PBR/HDRI/refraction profile. Discovery
does not select that method automatically or enable native workers in the browser.
Only an installed UI GPU host adds `gpu-viewport`, advertising **Preview / Viewport**
with no image formats or HDRI/refraction/progressive features. The installed
[opaque PBR consumer](render-opaque-pbr.md) adds the `pbr` feature. The host reports
effective limits from its created device and depth attachment: at most 4096 pixels
per dimension and 4,194,304 pixels, within the 256 MiB pixel scratch admission policy.
The estimate includes the selected MSAA colour/depth cost; it excludes scene geometry,
driver overhead and cached source meshes. It is not a measurement of total GPU memory.

The preview never refuses a frame because of the screen size. When the viewport's physical
size is past those limits (a maximised 4K or 5K window is 8–15 million pixels), the preview
draws at the largest size of the same aspect the executor admits (`RenderLimits::fit`, both
axes scaled by one factor, scratch included) and filters it up into the viewport. A viewport
within the limits draws at full size.

PNG images keep the saved-setup limits above. A setup past them is refused with the limits and
the largest size at its aspect (for example `4096 × 4096` → `2048 × 2048`), and the image panel
offers to use that size.

Pure capability metadata lives in the session/host, outside persistence and history,
surviving document replacement, scratch sessions and document-tab switches. No wgpu
types enter the engine or document crates. A host without an installed GPU/depth buffer
reports that product viewport preview is unavailable, rather than silently using the
opaque CPU snapshot path to approximate transparent appearances.

GPU product batches are separate from Design batches while sharing the installed
device and pipelines. Model/visibility/appearance changes update the product scene;
camera movement updates uniforms without rebuilding vertices. Selection and section
state do not change the product buffers. Vertex uploads are bounded to 128 MiB and
checked before allocation; malformed/non-finite triangles and invalid opacity fail.
Reinstalling the host invalidates product uploads. This is not a general device-loss,
render-job or cancellation protocol; those ownership mechanisms belong to R4.

Transparency uses the existing raster far/near alpha-blending approximation, without
refraction or HDR illumination. Opaque PBR uses a separate surface batch in the
same installed renderer. R3 itself added no export job; R4a supplies CPU product
PNG jobs. There is no result gallery or Bevy dependency. Saved setups were added
in format 21, alongside opt-in optical parameters on existing looks.
`ui.render` remains the existing Design snapshot API, using its installed renderer;
`ui.screenshot` captures
the actual integrated Render workspace. Product-image export uses `render.start`
and `render.export`, rather than the Design snapshot API.

## Reproduce

Run the original procedural `examples/render-studio.json` through the engine/CLI and
open its saved design in the app. Select Render, adjust its background or camera and
save a setup. The fixture uses original boxes/cylinders and simple colour/opacity
assignments; no commercial CAD files, artwork or materials are used. Actual Windows
Vulkan/MSAA evidence is attached to [#673](https://github.com/bherbruck/solidcraft/issues/673).

## Explicit Render camera and the Design view contract

`render.camera.set {camera}` applies a complete camera snapshot as transient view
state. It uses the saved-render camera's strict decoder and finite mm/radian
limits, preserves its explicit roll and up axis, and does not evaluate geometry,
change a saved shot, modify the design or add an undo step. Omitted roll/up use
zero/Z defaults; all other camera fields are required. Unknown fields, invalid
axes, nonfinite values and out-of-bounds numbers refuse atomically.

The Render inspector uses this command for numerical edits and before Frame
Subject. Design's `view.apply` retains upstream's independent policy: it accepts
finite roll (clamped to +/-pi) and refuses an explicit up axis. Render does not
reintroduce the former fork-specific extension to that Design command. Saving
or restoring a shot still uses the existing render-settings commands.

The camera pitch label is explicitly **Camera pitch**, so the legacy English
label adapter does not replace mechanical screw pitch translations. Material
axis conventions have stable catalog IDs; the numerical/wood conventions and
engineering data remain unchanged.

The legacy English-label adapter gives explicit catalog IDs precedence and, for
an ambiguous source label, retains a translation already present in the chosen
locale before falling back to English. Adding an untranslated Render alias no
longer hides existing Library, Appearance or Checkpoint translations. Contextual
Render labels still use their own stable IDs rather than borrowing a different
feature's meaning.
