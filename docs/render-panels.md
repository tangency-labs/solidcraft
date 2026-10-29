# Render workspace panels

Tracking issue: #6281. Part of [the Render workspace UX roadmap](render-workspace-ui.md).

The old "Render studio" panel (eleven pages, a setup header and a pinned footer) is removed by the end of the Render workspace transition. Every control moves to a panel attached to the thing it edits. A control has one home. A *shortcut* is a promotion of that control to a first-class item (a stage bar chip, a handle in the view, a menu entry); it reads and writes the setting its home owns through the same command, so the two cannot disagree.

Every one of the 546 labels the old panel used and every `render.*` and `appearance.*` command has a home below or is listed under Removed. The panel designs are on the SolidCraft UX Layouts board, artboards `Render_Q0_Plan` to `Render_Q13_Dialogs`.

## Rules

- **One home.** Each control is owned by one editor; anything elsewhere is a shortcut that names its home.
- **Attached to a thing.** A panel edits what is selected in the Browser or the view. With nothing selected the Scene overview shows. There is no panel switcher and no All page.
- **Never hide, fade with a reason.** Controls a method does not support (Raster, CPU ray tracing, GPU path tracing) stay visible, faded, with the reason in words. Opening a panel never changes the method.
- **Apply with consent.** Anything that replaces saved state opens one review dialog first. Everything else writes at once through its command, with undo.

## What each Browser row opens

| Row | Panel | Holds |
| --- | --- | --- |
| Scene (nothing selected) | Scene | Orientation and readiness |
| Environment | Environment | World, backdrop, stage, atmosphere |
| Lights  ( + adds a light ) | Lighting | Source, shadows, budget |
| a light | Light | Sun, point or area |
| Shots  ( + saves a shot ) | Shot | Camera, lens, exposure, frame |
| Objects › body, face or component | Appearance | Look, glass, coat, maps |
| Objects › Wear | Wear | Layered ageing and contamination |
| Output | Output | Renderer, image, destination, render |
| Output › Sampling | Sampling | Quality and limits |
| Output › Effects | Effects | Tone, looks, lens, finishing, regrade |
| Output › Check | Check | Valid? cost? diagnostics |

## Decisions

Approved by the owner. Each changes something the earlier design said, or a default.

- **D-A.** Sampling, Effects and Check are rows under Output, not sections. The approved rules put Effects and Check inside the Output editor. A long scroll hides expert controls from the person who just wants a size. As child rows the Output editor stays short and each child is one click or one chip away. It changes the approved Output design.
- **D-B.** Wear is its own row under Objects. Wear acts on the whole scene, not one body, and is GPU only. Keeping it out of Appearance stops a per-body editor from silently carrying scene state.
- **D-C.** Dust and the participating medium live in Environment › Atmosphere. They were on the Aging page but they are air, not wear. Both are GPU only and fade with the reason when another method is chosen.
- **D-D.** Every control writes the shot through its command, with undo. Save Setup, Reload Saved Settings and the draft confirmation go away. This changes a default: today edits stay in a draft until Save Setup. The camera keeps Update shot and Revert because it moves by navigation, not by a control.
- **D-E.** Unavailable controls stay visible, faded, with the reason. Raster, CPU ray tracing and GPU path tracing support different things. Hiding a control makes people ask where it went; a faded control with 'needs GPU path tracing' answers it, and opening a panel never changes the method.
- **D-F.** One review dialog for every apply that replaces saved state. Studio, rig, stage, physical plan, quality budget and map sets all show what is replaced and what is kept, and need a tick before they apply.
- **D-G.** Diagnostics become Check › Under the hood. Raw request JSON, job JSON and the capability catalogue stay available for experts and agents, but out of the everyday list.

## Editors (the Inspector, one per selection)

### Scene

- **Attached to:** The scene (nothing selected)
- **Purpose:** Orientation and readiness: where the scene stands in each stage, and whether it can render.
- **Reached by:** Press Esc or click empty canvas; the Scene row at the top of the tree.
- **When:** On arrival, after finishing an edit, and before rendering. Everyday, read-mostly.

**Stages**

- Look: bodies with a look, saved overrides, wear layers · jumps to Objects
- Light: environment name, lights n · jumps to Environment / Lights
- Frame: current shot, lens, aspect · jumps to the shot
- Output: size, format, renderer, quality · jumps to Output

**Ready to render**

- Check result line with the count of notes · opens Check
- Renderer state: preparing, failed, unavailable, offline (explains why a stage control is faded)
- Check now · shortcut of Check
- Render shot · All shots (n) · shortcuts of the stage bar Render

**Reminders**

- Scene preserves the design: geometry, engineering properties and CAD are never changed here

### Appearance

- **Attached to:** A picked body, face or component
- **Purpose:** How one thing looks: the look chain, its values, and the optical overrides that belong to that body.
- **Reached by:** Click a body, face or component in the view or the tree; the quick bar's Edit; right-click Edit look.
- **When:** After choosing a look, when it needs tuning, and for glass, coat or maps. Everyday for the top; expert for the sections below it.

**Look**

- Scope chips: Face · Body · Component · Material (where the look comes from)
- Look card: sphere, name, source, Used by n · Replace everywhere
- Applies to: This face | All n users

**Edit**

- Colour (sRGB) · Finish: Matte, Satin, Gloss, Metal · Metallic · Roughness · Opacity
- Refractive index (IOR) for non-metals
- Opaque only: glass is a separate section, with a pointer when opacity is below 1

**Glass and absorption  (GPU)**

- Use physical glass · recipe · closed solid, thin sheet or axis-aligned box
- IOR · thin-sheet thickness · absorption R G B per mm · transmit straight shadow rays
- Interior scattering: collision rate, phase mean cosine (closed dielectric only)
- Cannot share a body with clear coat or maps

**Clear coat  (GPU)**

- Enable clear coat · coat weight 0–1 · coat roughness 0.045–1

**Maps  (GPU)**

- Enable maps · base colour image · normal image · metallic map · roughness map
- Mapping origin X Y Z · one U repeat · one V repeat · repeat multiplier
- Normal strength · texture V points upward · base map replaces body colour

**Saved with this shot**

- State line: temporary on this tab, or saved · Save these optics to the shot · Replace saved · Remove saved optics
- Saved surface binding (authored map): replace · remove · conflict with temporary overrides
- Revalidate this body when the model changed

**Reflections  (CPU)**

- This body: Off · Mirror · Rough (rough uses its own assigned look)
- Confirm this body after the scene changed

**Footer**

- Save to library… · Reset

### Wear

- **Attached to:** The scene's wear stack (the Wear row in the tree)
- **Purpose:** Ageing and contamination layers, in order, each aimed at all eligible bodies or one body.
- **Reached by:** The Wear row in the tree; the Wear chip in the Appearance editor of an affected body.
- **When:** Late in the Look stage, to take the CAD cleanliness off. Occasional; GPU only.

**Layers**

- Add layer menu: deposited dust, grime and staining, oxidised iron, copper-green patina, chipped paint, fine scratches, faded coating
- Ordered list (up to 8): eye, kind, target · move up · move down · remove

**Selected layer**

- Enabled · strength · coverage · pattern scale mm · seed
- Layer colour (sRGB) · layer roughness · layer metallic fraction · upward-facing deposition
- Surface class: all opaque, metallic only, nonmetallic only

**Target**

- All eligible opaque bodies, or one named body (filter, refresh from current bodies)
- Field origin in world mm

### Environment

- **Attached to:** The Environment row (the world around the product)
- **Purpose:** What lights and surrounds the product: the source, its orientation, what the camera sees behind it, the ground, and the air.
- **Reached by:** The Environment row; the ring in the view; Change in the Studios and environments panel.
- **When:** Light stage, right after choosing a studio. Everyday for source, rotation and intensity; occasional for the rest.

**Source**

- Source: None, Constant colour, Artistic sky, HDR file, Built-in HDR · Change opens Studios and environments
- Constant: colour (linear RGB) · Sky: zenith, horizon, ground (linear RGB)
- HDR file: choose, check file, details (size, primaries, digest) · saved inside the design vs file on disk · treat unspecified primaries as linear sRGB
- HDR lighting samples (CPU)

**Orientation and strength**

- Rotate (degrees) · Intensity · the ring in the view is the shortcut

**Backdrop**

- What the camera sees: Environment · Colour · Gradient · Clear
- Colour (sRGB) · gradient mode: vertical, horizontal, radial · first and second colour · centre X Y · radius
- It never lights, shadows or reflects; refused with transparent output

**Ground and stage**

- Fit ground: extent margin · clearance gap mm · diffuse reflectance (linear RGB) · replace existing stage (consent)
- Physical floor and wall · Polygonal cyclorama (both fit to the product)
- Saved panels n · Remove stage

**Atmosphere  (GPU)**

- Participating medium: enable · bounds min and max (world mm)
- Extinction · scattering albedo (linear RGB) · phase mean cosine

### Lighting

- **Attached to:** The Lights group row
- **Purpose:** The lighting as a whole: which source of lights, shadows, and how many of each kind remain.
- **Reached by:** The Lights row; the + on it adds a light.
- **When:** When starting a lighting set up or changing method. Occasional.

**Light source**

- Studio preset: Neutral · Key and rim · Warm and cool, or Custom lights (turns a preset into editable lights)
- Rig applied: name · replace (consent)

**Shadows and sampling**

- Cast shadows (CPU) · area-light samples (CPU)

**Budget**

- Used of allowed: GPU 1 sun and 4 area; CPU 8 directional, 8 point, 4 area · Add light menu

### Light

- **Attached to:** One light (Sun or directional, point, area)
- **Purpose:** One light: what it is, how bright and what colour, where it is and where it points.
- **Reached by:** Its row in the tree; its handle in the view; Light tools in the stage bar.
- **When:** Light stage, constantly. Everyday.

**Light**

- Type: Area · Point · Sun (Spot appears with the unified light model)
- Intensity: EV or relative 0–64 · Colour: temperature or linear RGB · Softness / angular radius

**Position**

- Azimuth · Elevation · Distance (× subject size) · Aim at: Product centre, a body, a point
- Or world X Y Z · Direction toward light (X Y Z)
- Place by highlight

**Shape  (area)**

- Full side U and V in mm · radiance (linear RGB)

**Falloff  (point)**

- Reference distance for intensity · minimum distance for attenuation

**Behaviour**

- Solo · Mute · Duplicate · Delete  (solo and mute also live on the tree row)

### Shot

- **Attached to:** One saved shot (a camera with its settings)
- **Purpose:** One camera: where it is, what it sees, its lens and exposure, and the shot's own housekeeping.
- **Reached by:** A shot in the tree or the Shots strip; the Frame tools; Save shot creates one.
- **When:** Frame stage. Everyday for projection, FOV and Update; occasional for the rest.

**Shot**

- Name · rename · duplicate · remove
- Camera changed since this shot was saved: Update shot · Revert

**Camera**

- Projection: Perspective | Orthographic · Field of view · Distance (× subject size or mm)
- Target X Y Z · Yaw · Pitch · Roll · Frame subject · Look at selection

**Lens  (GPU)**

- Depth of field: enable · aperture radius · focus distance (needs perspective)

**Exposure**

- Exposure EV (not for raster)

**Frame**

- Aspect: 16:9, 3:2, 1:1, 4:5, 9:16, Custom · Rule of thirds · Safe area · Centre mark
- Output size: read-only, with a link to Output (size lives there)

### Output

- **Attached to:** The Output row (the image to be made)
- **Purpose:** What image comes out: the renderer, its size and format, transparency, and where it goes.
- **Reached by:** The Output row; the size and format chips in the stage bar.
- **When:** Output stage, before rendering. Everyday.

**Renderer**

- Method: CPU raster | CPU ray tracing | GPU path tracing · what each supports
- State: GPU compiling shaders with elapsed time, failed with reason, unavailable, offline setup

**Image**

- Width × Height · lock aspect ratio · swap · use the largest size at this aspect
- Over the limit: which limit, and the way out
- Format: PNG display image · EXR with the raw linear master retained
- Transparent background (conflicts: backdrop, bloom, CPU ray tracing)

**Destination**

- Replace existing file · Export PNG… / EXR… · View image · Copy

**Render**

- Render shot · All shots (n) · Cancel · progress and phase · last result summary

### Sampling

- **Attached to:** Output › Sampling
- **Purpose:** How hard the renderer works: noise, depth, and the limits it admits.
- **Reached by:** Output › Sampling in the tree; the quality chips in the stage bar.
- **When:** When a render is too noisy, too slow or refused for size. Occasional; expert beyond the quality preset.

**Quality**

- Draft · Balanced · High · Custom · Review a quality specification…

**Path tracing  (GPU)**

- Samples per pixel (1–4096, powers of two) · maximum path depth · random seed
- Reduce noise in the finished image (denoise)

**Adaptive  (GPU)**

- Allocate samples with independent pilots · pilot samples · minimum production samples
- Relative RGB error · absolute floor · coverage error

**Glass transport  (GPU)**

- Conditional Fresnel strata 1, 2, 4 · certified nested dielectrics

**CPU ray tracing**

- Resource profile: Compact, Studio, Workstation · limits: memory, triangles, bodies, samples
- Sampling mode: single image | progressive · edge samples per pixel 1, 4, 16 · progressive samples and seed
- Reflection depth 1–8 · reflection samples 1–64

### Effects

- **Attached to:** Output › Effects
- **Purpose:** Everything that happens to the picture after it is traced: tone, looks, lens effects, finishing, regrade.
- **Reached by:** Output › Effects in the tree.
- **When:** After a first render looks right but flat. Occasional; GPU only.

**Tone**

- Tone mapping: Reinhard | ACES fitted

**Creative looks**

- Ordered stack: warm, cool, sepia, monochrome, teal and orange, cyanotype-inspired, faded print, bleach-bypass-inspired
- Enabled · look strength · move up and down · remove

**Lens effects**

- Highlight flare strength, threshold, halo, streak, ghost rings · chromatic edge offset · radial distortion
- Soft focus · detail sharpening · photographic grain · pattern seed · reset lens effects

**Finishing**

- RGB gain · contrast · saturation · vignette · bloom: strength, threshold, radius · reset

**Regrade**

- Regrade the raw master (no denoising, separate PNG) · needs a retained raw master

### Check

- **Attached to:** Output › Check
- **Purpose:** Is this request valid, what will it cost, and what is the machine doing. Also the escape hatch for experts.
- **Reached by:** The Check chip in the stage bar; Output › Check in the tree; a note count on the Output row.
- **When:** Before Render, and whenever something is refused. Everyday for the list; rare for details.

**Check**

- Check now · status line · stale marker when the request changed after the check
- Notes and warnings, each with a jump to the control that owns it

**This request**

- Method · size · samples · logical admission estimate (memory) · HDR digest captured
- Captured scene: bodies, triangles · live-preview payload bytes of the limit

**Renderers**

- Available backends and why one is unavailable · GPU compile progress · scene error

**Under the hood  (collapsed)**

- Exact current request · last preflight details · job details and errors · Copy JSON
- Explicit capability boundaries · browse the capability catalogue

## Stage panels (under the Browser)

### Appearances

- **Attached to:** The Look stage · applies to the selection
- **Purpose:** Browse and apply opaque looks.
- **Reached by:** Under the tree in the Look stage.
- **When:** Look stage. Everyday.

**Browse**

- Search · scopes: In Design · Library · Recent · Favourites · categories
- Applicable first · availability badge (native, projection, reference only)
- Click applies to the selection · drag onto the model · click with nothing selected arms Paint
- SELENE opaque appearances and material concepts live in the Library scope

### Surfaces and references

- **Attached to:** The Look stage · applies to the picked body
- **Purpose:** Authored PBR surfaces with maps, and physical reference data.
- **Reached by:** Under the tree in the Look stage: the Surfaces and References scopes of the library switch; Surface in the Appearance editor opens it.
- **When:** When a look needs real textures or a physical reference. Occasional.

**Surfaces**

- Authored PBR surfaces · source map preview · use as uniform appearance · use authored GPU maps
- Target body · plane · physical repeat scale · save maps to this shot

**Physical references**

- Physical profiles and candidates · prepare creation plan (read-only) · copy plan with sources

### Studios and environments

- **Attached to:** The Light stage · applies to the scene
- **Purpose:** Browse and apply complete lighting set-ups and environments.
- **Reached by:** Under the tree in the Light stage.
- **When:** Light stage, first. Everyday.

**Browse**

- Scopes: Studios · Environments · Rigs · Mine · Specifications
- Native studios (review, then apply) · authored light rigs · built-in HDR environments · concepts and scene specifications (read only)
- Apply opens Review and apply; nothing is replaced silently

### Frame presets

- **Attached to:** The Frame stage · applies to the current shot
- **Purpose:** Aspect and composition aids.
- **Reached by:** Under the tree in the Frame stage.
- **When:** Frame stage. Everyday.

**Aspect**

- 16:9 · 3:2 · 1:1 · 4:5 · 9:16 · Custom (choosing one resizes the output, as one undo step)

**Guides**

- Thirds · Safe area · Centre mark (view only, never saved)

### Size and quality

- **Attached to:** The Output stage · applies to the Output
- **Purpose:** Common sizes and fixed sampling budgets.
- **Reached by:** Under the tree in the Output stage.
- **When:** Output stage. Everyday for size.

**Size**

- Preview 1280 × 720 · Full HD · 4K (with the limit shown) · Square · Custom

**Quality specifications**

- Fixed sampling budgets to review and apply (opens Review and apply)

## Stage chrome

### Stage tools

- **Attached to:** The stage bar, row 2 (current stage)
- **Purpose:** The tools and the handful of promoted settings of the current stage.
- **Reached by:** Always visible under the stage chips.
- **When:** Constantly. Everyday.

**Look**

- Pick: Body | Face | Component · Paint · Pick Up · Preview chip (opens the Preview menu)

**Light**

- + Add light · Lights fixed to Camera | World · Place by highlight · Handles on|off

**Frame**

- Perspective | Orthographic · Lens (FOV) · Look at selection · Save shot

**Output**

- Size menu · Format PNG | EXR · Quality Draft | Balanced | High | Custom · Check with note count

### Preview menu

- **Attached to:** The Preview chip in the Look tools
- **Purpose:** What the live preview is, and how to get it back when it fails.
- **Reached by:** The Preview chip.
- **When:** When the preview looks wrong or is unavailable. Rare.

**Preview**

- Integrated preview: raster shading, fixed studio lighting · exposure and image lighting apply to the image only
- Retry live preview · Use integrated preview · the setup asks for an unavailable preview profile

### Quick bar

- **Attached to:** The current selection in the view
- **Purpose:** One-click actions on what is selected.
- **Reached by:** Appears over a selected body, face or component.
- **When:** Look stage. Everyday.

**Actions**

- Name chip · Assign look · Pick up · Copy and paste look · Select same look · Remove look · Edit look

### Result viewer

- **Attached to:** A completed render (or two, to compare)
- **Purpose:** Look at, compare and take away results.
- **Reached by:** Opens when a render finishes; View image; the Live | Render segmented control.
- **When:** After every render. Everyday.

**Toolbar**

- Live | Render 1 | Render 2 · swap · result picker · discard · Fit · 100% · Copy · Export · Retain result · Close

**Status**

- Matches current design | stale · captured revision · job · executor · checkpoint and provisional state
- Progress pill while rendering · display reduced to the device limit

### Shots strip

- **Attached to:** The scene's shots and results
- **Purpose:** Switch shots, see their results, start renders.
- **Reached by:** The bottom strip; its chevron hides it.
- **When:** Constantly. Everyday.

**Strip**

- Shot thumbnails with modified dot · + Save shot · results with state · n running, m waiting
- Size chip · Render Image · All shots (n) · Update shot (shortcut of the Shot editor)

### Workspace menu

- **Attached to:** The workspace card
- **Purpose:** Leave, and the few preferences about the workspace itself.
- **Reached by:** The Render card top left.
- **When:** Rare.

**Menu**

- Return to Design · Focus view (Tab) · Dock side · Show Shots strip

## Dialogs

### Review and apply

- **Attached to:** A resource about to replace something
- **Purpose:** One dialog for every apply that replaces or adds saved state, so nothing is replaced silently.
- **Reached by:** Apply in a stage panel; Replace in an editor.
- **When:** When applying a studio, rig, stage, physical plan, quality budget or map set. Occasional.

**Variants**

- Native studio: replaces lighting, environment, exposure and stage; kept fields; consent
- Rig: centre, extent, use captured scene bounds, include passive cards or emitters only
- Ground and stage: replace existing stage
- Physical plan: read-only, no material created, acknowledge partial data, copy plan
- Quality budget: acknowledge omissions, projected size, samples, depth
- Maps: spatial maps vs uniform approximation, saved vs temporary conflict

### Confirmations

- **Attached to:** A destructive action
- **Purpose:** Confirm removal.
- **Reached by:** Remove shot, remove stage, remove saved optics.
- **When:** Rare.

**Confirm**

- Remove shot (the shot only; geometry and completed images stay) · Remove stage · Remove saved optics

## Shortcuts

| Setting | Home | Shortcuts |
| --- | --- | --- |
| Render, Cancel | Stage bar (R, Esc) | Shots strip, Scene overview, Output › Render |
| Check | Output › Check | Stage bar Check chip, Scene overview, Output |
| Save shot, Update shot | Shots strip +, Shot editor | Stage bar Save shot, tree + |
| Output size | Output › Image | Output size chip in the stage bar, Size presets, strip size chip, aspect presets (ratio only) |
| Format | Output › Image | Stage bar format chips |
| Quality preset | Output › Sampling | Stage bar quality chips, quality specifications panel |
| Projection | Shot › Camera | Stage bar Perspective \| Orthographic |
| Field of view | Shot › Camera | Stage bar lens chip |
| Aspect and guides | Shot › Frame | Frame presets panel, overlay chips in the view |
| Environment rotation | Environment › Orientation | Ring in the view |
| Light position and brightness | Light | Handle and dial in the view |
| Solo and mute | Tree row | Light › Behaviour |
| Add light | Lights group + in the tree | Stage bar + Add light, Lighting › Budget |
| Pick: Body, Face, Component | Look tools | Keys |
| Apply a look | Appearances panel (click, drag) | Quick bar, Appearance › Look |
| Fit | Navigation bar | Shot › Camera Frame subject |
| Change environment source | Studios and environments | Environment › Source Change |
| Export | Result viewer | Output › Destination |

## Removed

- **Page switcher (11 icon pages) and the All controls page.** Replaced by one editor per selection. The dense page has no home: nothing in it is a control that is not elsewhere.
- **Render studio header: setup picker, setup name, Save Setup, Reload Saved Settings, Unsaved preview changes.** A shot is chosen in the tree or the strip and renamed in the Shot editor. Controls write the shot through commands, with undo; Update shot and Revert remain for the camera.
- **Draft confirmation: Preserve edited render settings, Save and continue, Discard.** Gone with the draft.
- **Live Preview | Render Result segmented control.** Becomes the Live | Render control of the result viewer.
- **Model | Library toggle in the Render Browser.** The library is in the stage panels; the tree stays the tree.
- **Pinned footer: Check Render Request, Start Render, Cancel, Export, Replace existing file, Return to Design.** Render and Cancel are the stage bar; Check is the Check chip; Export is the viewer; Replace existing file is in Output; Return to Design is the workspace menu.

## Where every old control went

Role is Home (the one place it lives), Shortcut (also promoted elsewhere) or Removed.


### Header and footer

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Setup picker and setup name | Shot | Shot › Name; switch in the tree or the strip | Home |
| Save Setup, Reload Saved Settings, Unsaved preview changes | — | Every control writes the shot through its command, with undo. Update shot and Revert remain for the camera | Removed |
| Preserve edited render settings (Save, Discard, Keep editing) | — | Gone with the draft | Removed |
| Live Preview \| Render Result | Result viewer | Live \| Render n | Home |
| Check Render Request | Check | Check › Check now | Home |
| Start Render | Stage bar | Render (R) | Home |
| Cancel | Stage bar | Cancel (Esc) | Home |
| Export completed image | Result viewer | Export | Home |
| Replace existing file | Output | Destination | Home |
| Job status, progress bar, captured HDR and error text | Shots strip, Output | Progress pill and Output › Render | Home |
| Return to Design | Workspace menu | Return to Design | Home |

### Page strip

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Eleven page icons and the All controls page | — | One editor per selection. Nothing on All has no other home | Removed |

### Scene

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Scene preserves the design note | Scene | Reminders | Home |
| Image method | Output | Renderer | Home |
| GPU path tracer state (compiling, failed, unavailable) | Output | Renderer (and the Scene overview when it blocks) | Home |
| Browse native studios, resolve and review, apply with consent | Studios and environments, Review and apply | Studios scope; Native studio variant | Home |
| Browse scene specifications, SELENE shot specifications | Studios and environments | Specifications scope (read only) | Home |
| Ground and passive stage: margin, gap, reflectance, replace, fit and save, floor and wall, cyclorama, saved panels, remove | Environment | Ground and stage | Home |
| Saved shots: new name, duplicate, rename, remove | Shot | Shot › Name, duplicate, remove; Confirmations | Home |

### Appearance

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Editable body appearance: filter, refresh, choose body, colour, metallic, roughness, IOR, apply | Appearance | Edit (the selection replaces the body picker) | Home |
| Browse authored surfaces, inspect, source map preview, use uniform, use GPU maps, save maps | Surfaces and references | Surfaces | Home |
| Browse physical references, prepare creation plan, copy plan | Surfaces and references, Review and apply | References; Physical plan variant | Home |
| Browse materials, SELENE appearances | Appearances | Library scope | Home |
| Saved surface bindings: replace, remove, conflict with temporary overrides | Appearance | Saved with this shot | Home |
| GPU image materials: enable, filter, add body, per-body include, revalidate | Appearance | The picked body replaces filter and add; Revalidate stays | Home |
| Glass and optical absorption (recipe, IOR, thickness, absorption, shadows, interior scattering) | Appearance | Glass and absorption | Home |
| Clear coat over the base material | Appearance | Clear coat | Home |
| Planar colour and normal maps, metallic and roughness maps, origin, repeats, strength | Appearance | Maps | Home |
| Save optics to the setup, replace saved, saved optical targets, remove | Appearance | Saved with this shot | Home |
| CPU reflections: filter, choose body, Off, Mirror, Rough, add, depth, samples | Appearance, Sampling | Reflections (this body); depth and samples in Sampling | Home |

### Aging and contamination

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Add layer, layer list, move, remove, enabled | Wear | Layers | Home |
| Strength, coverage, scale, seed, colour, roughness, metallic, upward bias, surface class | Wear | Selected layer | Home |
| Target and coordinates: all bodies or one body, refresh, origin | Wear | Target | Home |
| Airborne dust and participating medium | Environment | Atmosphere | Home |

### Lighting

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Browse authored light rigs, inspect, fit centre and extent, include cards | Studios and environments, Review and apply | Rigs scope; Rig variant | Home |
| GPU: use custom studio lighting, constant environment radiance | Lighting, Environment | Light source; Source › Constant colour | Home |
| GPU: Sun (direction, radiance, angular radius) | Light | Type Sun | Home |
| GPU: area lights up to 4 (centre, half-edges U and V, radiance), add, remove | Light, Lighting | Type Area; Budget | Home |
| CPU: lighting preset (Neutral, Warm and cool, Key and rim), cast shadows | Lighting | Light source; Shadows and sampling | Home |
| CPU: custom directional lights up to 8 | Light, Lighting | Type Sun (directional); Budget | Home |
| CPU: point lights up to 8 (position, colour, intensity, reference and minimum distance) | Light | Type Point; Falloff | Home |
| CPU: rectangular lights up to 4, area samples | Light, Lighting | Type Area; Shadows and sampling | Home |
| Raster: not supported note | Lighting, Light | Faded with the reason | Home |

### Environment

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Scene background colour | Environment | Backdrop › Colour | Home |
| Camera-only gradient backdrop (mode, colours, centre, radius) | Environment | Backdrop › Gradient | Home |
| Browse original HDR environments, embed | Studios and environments | Environments scope | Home |
| GPU environment: none, artistic sky, HDR file; choose, check, details, sRGB primaries | Environment | Source | Home |
| Sky gradient: zenith, horizon, ground | Environment | Source › Sky | Home |
| Environment intensity and rotation | Environment | Orientation and strength | Home |
| CPU environment: none, constant, HDR; colour; HDR lighting samples | Environment | Source | Home |
| Bounded homogeneous volume | Environment | Atmosphere | Home |

### Backgrounds

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Camera background colour and backdrop | Environment | Backdrop | Home |
| Apply to retained raw master | Effects | Regrade | Home |
| Physical floor and wall, cyclorama | Environment | Ground and stage | Home |
| HDR illumination is separate (jump to Environment) | Environment | Source | Home |

### Camera

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Perspective, vertical field of view, eye distance, target, yaw, pitch, roll | Shot | Camera | Home |
| Fit Product | Navigation bar | Fit (shortcut: Shot › Frame subject) | Shortcut |
| Restore Saved Camera | Shot | Revert | Home |
| Depth of field: enable, aperture radius, focus distance | Shot | Lens | Home |
| Fit Image, 100% (result) | Result viewer | Toolbar | Home |

### Lens and finishing effects

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Creative look stack: add, order, remove, enabled, strength | Effects | Creative looks | Home |
| Lens-inspired effects: flare, chromatic offset, distortion, soft focus, sharpening, grain, seed, reset | Effects | Lens effects | Home |
| Presentation filters: RGB gain, contrast, saturation, vignette, bloom, reset | Effects | Finishing | Home |
| Regrade the raw master | Effects | Regrade | Home |
| Retain raw linear EXR | Output | Image › Format | Home |

### Output

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Export format: display PNG or raw linear EXR | Output | Image › Format | Home |
| Width, height, lock aspect, swap, HD, Full HD, 4K, largest at this aspect, limits | Output, Size and quality | Image › Size; the presets in Size and quality | Home |
| GPU samples, path depth, seed | Sampling | Path tracing | Home |
| Nested dielectrics, adaptive sampling, transmission strata | Sampling | Adaptive; Glass transport | Home |
| Tone mapping, denoise | Effects, Sampling | Tone; Path tracing | Home |
| CPU: resource profile, sampling mode, edge samples, progressive samples and seed | Sampling | CPU ray tracing | Home |
| Exposure EV | Shot | Exposure | Home |
| Transparent background | Output | Image | Home |
| Browse and review quality specifications, apply fixed budget | Size and quality, Review and apply | Quality specifications; Quality variant | Home |
| Completed image preview, Open in the main canvas | Result viewer | Opens over the view | Home |

### Diagnostics

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Live-preview payload bytes, scene error | Check | Renderers; This request | Home |
| Refresh captured scene summary, bodies, triangles | Check | This request | Home |
| Last preflight: memory estimate, warnings, details | Check | This request; Under the hood | Home |
| Captured job details, exact current request, Copy JSON | Check | Under the hood | Home |
| Explicit capability boundaries, capability catalogue | Check | Under the hood | Home |

### Stage bar

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Pick Body, Face, Component; Paint; Pick Up | Stage tools, Quick bar | Look | Home |
| Preview note and gear | Preview menu | Preview | Home |
| Lights fixed to Camera or World | Stage tools | Light | Home |
| Perspective \| Orthographic | Stage tools | Frame (shortcut of Shot › Camera) | Shortcut |
| Output size and format chips, Check | Stage tools | Output (shortcuts of Output, Sampling, Check) | Shortcut |

### Browser

| Old control | Now in | Section | Role |
| --- | --- | --- | --- |
| Model \| Library toggle | — | The library is in the stage panels | Removed |
