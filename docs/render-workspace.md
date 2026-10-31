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

## Stage bar and floating docks

Render has no ribbon. A shared 88 pt two-row stage bar sits 8 pt to the right of
the 104 × 88 pt workspace card, with matching top and bottom edges. The icon sits
above the Render label and Workspace caption. The header is 36 pt; the remaining
52 pt contains the tools, centred vertically below a 1 pt divider. Numbered **Look**, **Light**, **Frame** and **Output** chips
run `view.render_stage {stage: look|light|frame|output}`. The selection belongs to the
open document tab's view state; it is not saved in the design. Opening a stage never
applies or saves settings or starts an image job. It focuses the existing Inspector
page, while every Inspector page and registered Render command remains reachable.
The headless command validates and echoes the request without changing the document.

Render, Cancel and the shared command search (S) occupy the top row. The Pick control's
icon-text Body/Face/Component segments, Paint/Pick Up actions and direct-light
preview information chip occupy Look's second row with 8 pt side padding. Changing the
light coordinate frame remains disabled with its reason in this checkout. Frame
uses the existing explicit camera command for projection and the registered Save Shot
action opens the shared naming confirmation; Output shows the current
size/format and directs edits to the Output Inspector. No completion ticks are inferred.

The Browser and Inspector float in both shell layouts and share a top edge, 8 pt
below the bar. Their default outer widths are 266 pt and 305 pt, with shared resize
bounds. The Browser keeps the upper left card (up to ten compact scene rows); its
stage panel fills the remaining height below it, ending 8 pt above the 48 pt Shots
strip. Both cards share width and side preferences. Narrow windows shorten the
Browser to reserve library space, then omit cards that cannot fit safely.

Look hosts Appearances: search with the actual catalog count, In Design / Library /
Recent / favorites scopes, horizontally scrolling category chips and a three-column
sphere grid or list. Resources used in the design or matching a selected look's
finish family or category appear first. Clicking a look applies through
`appearance.assign` to selected bodies, faces or components; with no target it
only inspects. Favorites and recent history live in user preferences. Explicit
catalog navigation opens the corresponding stage library.

Look also offers **Surfaces and references**. Surfaces/References segments and search
lead to authored surface rows with availability badges. Selecting a surface loads
its bounded source-map preview; Target, Plane and Repeat precede Use uniform,
Use GPU maps and Save maps. GPU actions remain faded with a reason on other methods.
Physical profiles retain their properties and sources; Prepare creation plan opens
a read-only review with acknowledgement and Copy plan. It creates no material.

Light hosts **Studios and environments**: a Scope dropdown, search and three-column
tiles for Studios, Rigs, Environments and read-only Specifications. Mine remains
visible with the reason that personal studio saving is unavailable. Clicking selects;
Apply prepares a review without changing the method or saved settings.

The shared `ui_kit::ReviewAndApply` dialog presents a resource card, Replaces/Kept,
variant controls and explicit acknowledgement before applying. Studio, rig fit,
environment, ground/stage, physical plan, fixed quality budget and uniform/spatial
maps use this composition. A captured review expires when the document, revision,
shot, method or settings change. Rig fitting offers centre, extent, captured bounds
and emitters-only projection. Quality review reports projected dimensions, samples,
depth and omissions; `render.quality.prepare` remains reachable through command
activation. S5 can call `render_workspace::review_resource(app,
"render.quality.prepare", id)` for its selected specification.
Programmatic commands continue to execute without opening dialogs.

Remove shot uses the existing shared `Confirm`; remove stage and saved optics now
also use it. Resource removal confirmations expire after a document revision change.

Confirmed reviews run `render.preset.apply`, `render.library.studio.apply` or
`render.library.environment.apply`, preserving validation and undo. Studios require an explicitly selected
GPU path setup; environments require CPU or GPU ray tracing. Frame shows a faded Frame presets placeholder with its Shot editor reason. Output hosts the Size presets card described below. Stage navigation itself never applies a resource.

The 110 pt compass uses the free canvas beside the Inspector; the navigation bar
keeps its shared handler. The Shots strip's independent `view.shots_strip`
preference is unchanged.

`view.render_focus {on: true}` hides the Browser, Inspector and Shots strip and
releases their canvas reservations. Tab toggles focus while Render is active and
no text field has focus; the **Exit focus** chip sits at the Browser's remembered
side below the stage bar. The workspace card, stage bar, compass and view bar
remain available. Focus belongs to the window, is never saved in a design or
preferences, and retains the docks' sides, widths and folded state.

Below 1100 pt, or with `view.layout {mode: compact}`, the docks use their shared
minimum width without overwriting the remembered width. Below 800 pt the Browser
folds into a chip. Its floating card opens through
`view.render_browser {open: true}` and closes through **Fold Browser**; it shields
navigation beneath the card without reserving a canvas column. Neither command
edits or evaluates the design, moves the camera or creates an undo step.

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
Sampling > Path tracing exposes progressive sample counts from 1 to 4096,
maximum path depth (1–16) and random seed. Exposure (−32 to +32 EV) lives in Shot. One setup
exposure field controls the selected image method. Switching methods retains their
local numeric drafts while submitting and saving only the active method. Reloading
or choosing another setup replaces those drafts with the saved numeric intent.
The live product preview keeps its fixed studio response and does not apply image
exposure, environments, lens or material overrides.

Effects > Tone exposes Reinhard / ACES fitted tone mapping. Sampling > Path tracing
owns optional final edge-aware denoising; Shot owns thin-lens depth of field. Positive aperture
requires a perspective product camera. Denoising preserves raw accumulation but can
soften detail; glass and sharp specular first hits bypass its final spatial filter.

The Environment Browser row opens the Environment editor. The legacy Environment
and Background pages use that same editor, with five sections in order: Source,
Orientation and strength, Backdrop, Ground and stage, and Atmosphere (GPU).
Source offers None, Artistic sky, HDR file and embedded HDR for GPU, or None,
Constant and HDR for CPU. Change opens the Environments scope of Studios and
environments. Embedded HDR shows its name, kind and saved-in-design status; sky
exposes zenith, horizon and ground linear colours. CPU HDR has lighting samples.
Radiance `.hdr` input is bounded to 16 MiB, 2048 × 1024, 2:1, and linear
sRGB/D65 primaries; unspecified primaries require the explicit interpretation checkbox.
EXR is not supported. Rotation and intensity affect illumination, while the saved
background remains the camera backplate. Selecting a file runs the bounded public
`render.hdr.inspect` command; a typed path has an explicit **Check HDR file** action.
The picker belongs to the originating document revision, setup, method and input
selection. A cancelled or stale answer never replaces a newer selection.

Rotate and Intensity use the existing environment command; the rotation ring is
their shortcut. Backdrop offers World, Colour, Gradient and Clear. Gradient uses
the existing GPU camera backdrop (vertical, horizontal or radial, two colours,
centre and radius). It never lights, casts shadows or reflects, and is refused
with transparent output. World visibility and blur remain faded because no saved
renderer setting backs them. Unsupported method choices stay faded with a reason.

Ground and stage fits a saved ground, floor and wall, or polygonal cyclorama.
Replacing a stage requires the explicit consent checkbox; removing it uses the
shared **Remove the saved stage?** confirmation. The fields expose extent margin,
clearance gap and linear reflectance, followed by the saved panel count.
Atmosphere edits the bounded participating medium: enable, world bounds in mm,
extinction, scattering albedo and phase. The transitional Aging page's airborne
section calls this same function. CPU and raster show the controls faded with
the GPU requirement. These controls use existing render state and commands;
opening the editor never changes the design or renderer.

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

### Appearance editor and body optics

Picking a face, body or component opens Appearance. Its kind chips, look card and
Applies to scope lead to Edit: colour, finish, metallic, roughness, opacity and IOR.
The look card's source tag opens the inheritance chain. Reset restores the picked
target's values from when it was opened; Save to library uses the existing design
library export flow.

Glass and absorption (GPU), Clear coat (GPU), Maps (GPU), Saved with this shot,
and Reflections (CPU) follow in that order and start collapsed. Unsupported
methods remain visible with an explanation; opening a section never changes the
renderer. Optical bindings currently target one whole visible body, so face,
component and multiple-body picks show the same controls disabled with a reason.
The Browser command has no Wear selection kind yet, so no Open Wear link is shown.

The old Appearance page retains explicit target selection and the library/reference
and wear controls awaiting their own editors. Its body optics use the same sections
as the picked-body editor. Scene ordinal, exact name, stable target and document
identity are checked; a document revision change requires Revalidate this body.

Six original glass recipes use linear transmittance over a reference distance to
compute absorption per millimetre via -ln(T) / distance. Glass exposes boundary,
IOR, thickness, RGB absorption, straight shadow transmission and closed-solid
interior scattering. Thickness is disabled for closed glass, which uses geometric
distance. Glass cannot share a body with coat or maps. Full topology, orientation,
overlap and optical-boundary checks run during GPU preparation; Check does not
certify those properties.

Clear coat retains weight, roughness and IOR. Maps retain the bounded PNG/JPEG
picker flow for base colour, normal, metallic and roughness images, world-space
origin and U/V repeat vectors, normal strength, V orientation and base replacement.
The backend's map-size, encoded-byte, repeat and assignment limits still apply.
File-picker completion checks the original document, revision and body identity.

These local overrides are temporary on the document tab. Save these optics uses
render.optics.bind to save physical glass with the shot; replacement requires the
explicit Replace this body's saved optics checkbox. Coat and local image maps
remain temporary, and the save action explains that limit. Remove saved opens the
shared Remove saved optics? confirmation before render.optics.unbind; stale
confirmations cannot affect a changed document. Saved authored surface bindings
remain manageable in Saved with this shot. Reflections retains this body's
Off/Mirror/Rough CPU binding and explicit target confirmation.

Active GPU overrides still cause Check/Start to refuse a CPU request. Both buttons
submit the same freshly validated runtime parameters and the engine captures file
contents at that boundary.

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

There are no grid/axis/edge lines, sketches, construction objects, section cuts,
analysis shading, active-component fading, canvases or decals in the product scene. Browser visibility still controls which bodies appear.
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
view or navigation cube all work and
**Fit Product** frames visible product bodies without including sketches or selection.

While Frame shows the picture's frame, **Fit Product** and the fit on entering Render keep
the product inside that frame rather than the viewport, so a picture taller than wide moves
the camera back. The fit on entering waits for the saved setup to load, so it frames that
setup's aspect, not the draft it replaces. `render.camera.fit` takes the picture's `aspect`
(width over height, 0.000001 to 1000000, default 1) and refuses a fit whose camera would be
out of range, leaving the current camera as it was.

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
state do not change the product buffers. On desktop the preview takes up to eight of
the device's largest buffers of vertex geometry (2 GiB with the usual 256 MiB buffers,
never more than 2 GiB); uploads are split into buffers within that size. The web keeps
a 128 MiB budget. The budget is checked before allocation, and a refusal names the
scene's geometry size, the limit and how to get under it (hide bodies in Design). The
upload itself runs under its own GPU error scopes: an allocation the device refuses is
shown in the preview instead of drawing, and nothing else in the app changes how GPU
errors are handled. Messages sit on a theme panel card so they stay readable on any
background colour. The separate CPU image job keeps its own memory and work limits.
Malformed/non-finite triangles and invalid opacity fail.
Reinstalling the host invalidates product uploads. This is not a general device-loss,
render-job or cancellation protocol; those ownership mechanisms belong to R4.

Simple colours are shaded by the Design viewport's lights, in linear light and encoded
once, exactly as the CPU product image shades them, so the preview and the exported PNG
agree (Design itself shades the encoded colour, as it always has).
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

## Picking in the product view

The Look stage bar's second-row **Pick** control selects Body, Face or Component granularity,
using Design’s selection filter state through the `select.filter` command. Component
picks use the Browser’s exact occurrence path, including nested and repeated
placements; a body at the design root retains its body identity. Face picks use the
existing mesh-to-B-rep mapping and connected-surface selection rules.

Hover highlights the target; clicking selects it. Clicking empty space clears the
selection. Shift and Ctrl/Cmd follow Design’s add/toggle behavior and the Shift
add-only preference. Browser row selections and product picks share the same
selection commands and state, retained across Design/Render switches. Picking works
with the Shots strip shown or hidden. Render picking never starts a geometry edit.

Selection and hover use Design’s highlight drawing and theme tokens in a separate
GPU overlay on the live product view. The product scene and its upload stay unchanged.
CPU image jobs, exported PNGs, retained images and the Render Result canvas never
contain this overlay; the Pick control is disabled on the result canvas.

Look's tools row contains **Paint** (B) and **Pick Up** (I). A library click applies
to the current selection or starts painting when nothing is selected; dragging
previews the look until release. Esc or Done ends painting or sampling. Copy Look
and Paste Look use Ctrl/Cmd+Alt+C and Ctrl/Cmd+Alt+V.

The selection pill uses shared kit chrome and follows the projected selection.
Its actions open the look chain, Replace All, toggle a library favourite and hide
the selection. The name opens the selection menu. Assign, Replace All, Copy Look
and Remove are available there and in the Appearance Inspector; commands also
remain available to search and scripts. Replace All asks for the replacement in a
shared confirmation and applies one undoable command. A changed document cancels
that pending request. Hover uses the shared tooltip with the object and look names.

## Browser scene focus

The existing Camera Inspector page shows a standalone Shot editor when the Browser selection
is a saved shot. Opening it leaves the live camera and saved design unchanged. Camera fields
use `render.camera.set`; Update shot saves that camera and Revert restores the saved camera.
Name, output dimensions and exposure use the existing undoable shot/settings commands.
Distance is shown as a multiple of the visible subject's bounding-box diagonal (one millimetre
for an empty scene). The lens chip uses the camera's vertical FOV and a 24 mm sensor height.

Frame's floating panel offers five aspect presets and Custom, plus composition guides.
Clicking a preset saves its output dimensions in one undo step; Custom preserves dimensions.
`view.render_frame` independently validates and echoes transient aspect/guide preferences;
headless calls never edit a design. Guides are per-document UI state, never saved in design JSON.
The frame and its size/lens chips appear only in Frame or the Shot editor. Product preview,
navigation and picking use the fitted frame aspect; image jobs and the Result canvas omit guides.

The shared Browser lists Environment (with its source), Lights, saved Shots,
Objects and Output (with image dimensions and format). Shots **+** opens the
existing Shots manager; Lights **+**, Solo and Mute remain faded with an explicit
reason until named light commands exist. Existing light entries describe the
current numerical image setup, without inventing a saved rig.

`ViewState.selection` exposes `RenderSelection` for Nothing, Object (the existing
engine selection), Environment, Light, Shot and Output. It belongs to each document
view and is never saved in a design. `view.render_select` validates semantic focus
requests without opening dialogs or editing/evaluating a design. Object picks and
Browser selections keep the existing selection commands. Selecting Output opens
the Output editor; selecting Environment opens its editor. The legacy Environment
and Background entry points share the Environment editor.

Look expands Objects, Light expands Lights and Environment, and Frame expands
Shots. Output leaves expansion unchanged. Stages preserve selection and never
lock rows or prevent manual expansion.

## Appearance editor

Look shows one Appearance editor with scope, current look, the four-step look
chain and Edit controls. There is no Inspector page strip. Applies to chooses the
selected target or every exact user of the look; mixed values remain inert until
edited. Edit contains colour/hex, finish and metallic/roughness/opacity sliders.
Advanced reuses the existing surface library, optical bindings, glass, clear coat,
texture-map, reflection and aging controls. The separate Textures disclosure is
marked Soon. The shared footer exports the design library through `library.export`
and resets the current target's override through `appearance.assign`.
`view.render_inspector {visible}` controls transient per-document visibility;
selecting a stage reopens the editor. Output selection routes to its dedicated editor;
Environment selection routes to its editor. Other semantic selection focus routing
remains deferred.

## Output editor

The Output stage offers size, format, quality and a Check summary in the stage bar's
second row. Its lower-left Size presets card offers Preview (1280 × 720), Full HD,
4K, Square and Custom. Presets outside the selected executor's limits stay visible
with a disabled explanation showing the selected method's dimension/pixel limits and
largest fitting size. The default raster limit is 4,194,304 pixels, so 4K exceeds it;
methods with larger resource profiles can admit 4K. Custom retains the current size
and reveals its fields through `render.output.size`. Numerical size edits and preset
clicks patch width and height with `render.settings.set`, as one undo step. Opening
the panel never applies a preset or starts a job.

Selecting Output in the Browser shows a short editor with Renderer, Image,
Destination and Render sections. Sampling, Effects and Check are child rows under
Output, each with a summary and its own editor. Sampling owns Quality, GPU Path
tracing, Adaptive, Glass transport and CPU ray tracing. Method-specific controls
stay visible and faded with their reason on other methods. Effects owns Tone,
Creative looks, Lens effects, Finishing and Regrade, in that order; its controls
need GPU path tracing. Effect edits patch the saved job with `render.settings.set`;
opening an editor never saves or switches methods. Check owns the preflight notes,
This request, Renderers and collapsed Under the hood diagnostics. The legacy
Effects, Output and Diagnostics pages call the same editors.

The stage bar's Check chip and Output's note count open Check. Check now runs the
read-only `render.preflight` without admitting a job. Notes offer links to their
owning editor. Copy report and diagnostic JSON use `render.preflight.copy` and the
platform clipboard queue; headless calls only validate and echo bounded text.
Named quality presets, EXR export and rendering all shots remain faded with their
existing reasons. Destination selection uses the completed-image export picker.

Render and Cancel stay in the stage bar. Active jobs also show a single-row progress
pill with Cancel. The result viewer retains Copy, export, close, Fit/100% and shared
pan/zoom. R starts the active setup; Escape closes the image without cancelling its job.

Preferences › Graphics offers “Use previous Render preview geometry budget” (`render_previous_geometry_budget`). It restores the fixed limit (512 MiB on 64-bit desktop, 128 MiB elsewhere); by default, desktop previews use the device-based budget. Changes apply to the next preview frame.
