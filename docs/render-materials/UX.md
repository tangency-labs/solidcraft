# Render workspace UX plan

## Product objective

The common path is: **select a subject, choose a scene recipe, adjust the visible result, render**. A designer should obtain a useful presentation without knowing shader graph terminology. An expert should be able to inspect every physical assumption, replace any resource and automate the same operation.

The workspace follows **SolidCraft's implemented ribbon redesign**, including its workspace switcher, selected-tab treatment, approved SVGs, shared UI kit and Floating/Docked shell layouts. Its panels read engine state; document and job operations go through the shared command registry, while focus and disclosure remain UI state. Camera gestures use the shared navigation handler.

The concrete integration specification is [UI_INTEGRATION.md](UI_INTEGRATION.md), with source pins, existing command/icon mappings and future layout intent in [ui-integration.json](ui-integration.json). The upstream design reference was checked at **0.18.4, `728f753b8094e1a54c5cc8b7e453982afa028023`**. The implementation base remains the personal fork's fixed **0.18.2, `93a2922c5ee272fb605703ae84c05825b4a74607`**. This source inspection does not update that base or claim that the proposed library and inspector already exist.

## Layout and entry points

| Area | Default content | Deeper controls |
|---|---|---|
| Existing application bar | Existing document tabs, Save, Undo/Redo and global actions | Retain the current global hierarchy and shortcuts. |
| Workspace card and ribbon | Render workspace, one **Render** ribbon tab; current **Scene** and **Image** groups | Grow task groups only with bound commands; retain section menus, pins, overflow and command search. |
| Left resource browser, proposed | Search, recommended collections, Recent/Favorites and a local Scenes/Materials/Lighting/Environments selector | Preserve the document tree through a local Model/Library view; keep filters and selection when returning. |
| Center canvas | Existing product preview; later a local Viewport/Result selector when a result surface is implemented | Selection overlays, light handles, region rendering and A/B remain capability-gated additions. |
| Right inspector, proposed extension | Selected resource, explicit assignment scope and at most six primary adjustments | Use shared panel sections for full physical parameters, output and provenance. |
| Job feedback | Existing image-control progress; later a bounded result/job tray | Keep Cancel and the last result reachable without covering navigation or introducing a second global status bar. |

**Scene and Image are ribbon groups, not existing tabs.** Render currently returns only `RENDER` from the workspace tab selector. Library categories, inspector sections, quality profiles and viewport/result navigation must not be presented as four competing workspace tab systems. The current product preview also does not implement in-viewport material picking or selection outlines; those require a reviewed selection adapter before the corresponding workflow ships.

Start the library with a small recommended set for Product, Automotive, Electronics, Making, Aerospace and SELENE. Keep the full 1,160-concept planning inventory accessible through explicit availability filters. Search must also work by material ID, process, intended use and finish. Preserve query, category, filters, scroll position and selected item when moving between an item and its inspector. Virtualize long lists and load preview images on demand.

Show material thumbnails under one declared neutral calibration rig. Offer a second preview under the current scene. Consistent calibration makes roughness and color easier to compare; scene preview reveals whether the material suits the shot. A resource with an unavailable capability shows that fact before application.

## Four primary workflows

### 1. Fast product presentation

1. Select a body, component or assembly and open Render.
2. Choose a scene card such as **Electronics Bench Hero** or **Automotive Paint Review**. The card includes thumbnail, intended subject size, quality tier and availability.
3. Inspect the resolved subject and ground contact. A supported preset with no unresolved conflict applies through one undoable action; only an actual scope, replacement or lock conflict needs a separate decision.
4. Adjust no more than six primary controls: finish, roughness, texture size, light rotation, exposure and camera framing. Render performs preflight on the same captured request it will start; an unchanged supported request does not require a second manual Check click.

The future target is three to five deliberate actions from a selected subject to a sensible default image. This is a usability target to measure with first-time users, not a claim about the current implementation.

### 2. Precise appearance assignment

Select face, body, component definition or occurrence; show the scope in plain text before applying a finish. A shared-definition edit indicates how many occurrences it affects. Dragging a material highlights the prospective target without changing it. Drop applies one command; Escape cancels. Multi-selection offers a shared override with explicit mixed-value fields.

The first inspector section contains base finish, color, roughness, physical texture size and direction. Additional sections expose coat, conductor/dielectric behavior, transmission, absorption length, subsurface response, fibers, flakes, layers and mapping only where relevant. Sliders have units, direct numeric input, reset and useful bounds. Do not rename optical roughness to an ambiguous “quality” setting.

A separate Engineering panel retains density and design properties. A later Dynamics panel owns contact and inertial assumptions. Applying “dusty anodized aluminum” must not alter assembly mass, friction or restitution.

### 3. SELENE lunar still

Choose a pinned SELENE scene card, resolve the model binding and select a shot. The primary controls are Sun azimuth, Sun elevation, exposure, camera framing, terrain seed and dust coverage, each shown only when supported. The **Physical lunar** scene policy locks atmospheric scattering and studio fill off; it is a property of the scene, not another application workspace or expertise mode. Entering an editorial scene is an explicit, recorded change.

The default lunar sky is dark and the Sun is owned once. An Earth backdrop remains disabled until location, time, orientation and visibility are resolved. “Improve shadows” must never silently inject skylight. Provide a diagnostic view explaining that unlit areas receive only the contributions actually present in the scene.

### 4. Future simulation and final animation

Keep motion planning and physics approval separate from render quality. The timeline displays the state sequence, contacts and events; the simulation inspector reports mass, inertia, limits, constraint state and solver diagnostics. The user approves a baked trajectory before a final render queue starts. Changing friction, gravity, object topology or motion parameters invalidates the bake and displays the affected frames.

## Progressive disclosure

| Level | What the user sees | What remains accessible |
|---|---|---|
| Quick | Recommended scene cards and a small set of useful controls | Default inspector with deeper sections collapsed. |
| Detailed | Material layers, light contributions, ground, camera and color sections | Open the relevant named section in the same inspector. |
| Expert | Integrator, sampling, limits, spectral assumptions, passes, resource and execution details | Expand Advanced inside the relevant section; retain reproducible request export. |

These terms describe disclosure depth. Do not add a global Simple/Advanced/Expert toggle or another set of ribbon tabs. Expanding a section preserves the scene, quality, camera and unsaved draft. Defaults come from the resource and can be restored at field, section or whole-shot level. Changed fields visibly distinguish inherited values from overrides. A collapsed section shows an override count and relevant warning so it cannot hide an unsupported value.

## Quality and result navigation

Quality is a named, capability-resolved output budget. Viewport/Result selects the central surface once a result canvas exists. Quick/Detailed/Expert describes disclosure. Physical lunar/Editorial describes the scene's light and environment policy. Keep these concepts separate in labels and state.

Use exact profile identities from [quality-presets.json](quality-presets.json). For a normal product still, the recommended choices are **Interactive Draft (`Q-001`)**, **Balanced Product (`Q-002`)** and **Product Portfolio (`Q-003`)**. A lunar hero recommends **Interactive Draft (`Q-001`)**, **Product Portfolio (`Q-003`)** and **Lunar Hero Master (`Q-006`)**. A sample/vial macro recommends **Interactive Draft (`Q-001`)**, **Glass and Layers (`Q-004`)** and **Lunar Sample Macro (`Q-007`)**. The SELENE flow holds the exact recommendations for each of its twelve shots, including specialized LED and diagnostic choices. The full list remains accessible through the same quality picker. Show dimensions and supported features before starting, and record Custom after an override. These profiles remain future specifications, not current executor promises.

Backend/device selection lives in the output section. If a chosen profile is unsupported, explain the missing feature and offer an explicit compatible choice. A lower sample budget must never silently remove transmission, change illumination, or switch the executor. Keep render progress local to its document and captured request; a later edit marks the result stale instead of relabeling it as current.

## Scene composition and conflict resolution

Resolve a scene recipe before mutation. The preview of changes lists new resources, required capabilities, preserved locks, replacement scopes and unresolved references. Apply succeeds atomically or leaves the shot unchanged. Unsupported dependencies are named with a useful next action: select a supported preset, choose a suitable executor, or stay in a clearly identified approximation.

Never silently replace path tracing with raster preview, convert a transmitting lens to opaque plastic, remove a coating, add a second Sun or change the model scale. An optional preview approximation is saved as such and cannot inherit the “validated final” badge.

Preset ownership has four levels: resource defaults, scene composition, shot overrides and explicitly locked user fields. The resolved request records all four. Switching a lighting preset should not unexpectedly replace the camera, engineering properties or unrelated material assignments.

## Interaction details that matter

- Hover previews are temporary and cancelable; committing a recipe is a separate action.
- Direct light handles display the edited physical property and unit; selecting a Sun edits direction, not a nearby point-light distance.
- A/B comparison locks camera, exposure and display transform by default so the comparison measures the intended change.
- Search, lists, sliders, selection scope and render controls are keyboard accessible. Focus returns to the initiating control after dialogs.
- Status never relies on color alone. Use readable labels such as Concept, Ready, Preview approximation, Unsupported and Needs revalidation.
- Long operations expose progress phases and cancellation. A failed export preserves the completed image buffer where possible and offers a different destination.
- Autosave persists scene intent and references; cache eviction must not destroy a material definition or the last successful render.
- Offline reopening reports missing dependencies before rendering and identifies the exact pack and version.
- Library packs load on demand. Thumbnail generation and search indexing must not freeze geometry interaction.

## Existing commands and proposed extensions

The existing commands are the starting point. Their current parameter and scope limits still apply:

| Existing action | Integration rule |
|---|---|
| `appearance.assign` | Reuse supported appearance assignment; add reviewed scope support before offering occurrence-specific behavior. |
| `material.assign` | Keep engineering material separate from render appearance. |
| `render.settings.get`, `render.settings.set`, `render.settings.apply` | Inspect/save the supported setup and restore its camera; preserve per-document draft semantics. |
| `render.scene`, `render.backends` | Inspect evaluated product metadata and actual host capabilities. `render.scene` is not an apply-scene command. |
| `render.preflight`, `render.start` | Check and start the same request; extend the shared resolver when new resource fields land. |
| `render.status`, `render.cancel`, `render.export` | Keep existing job identity, cancellation, result ownership and export semantics. |

Read-only recipe resolution, atomic recipe application and shot duplication are future requirements. `render.preset.resolve`, `render.preset.apply` and `render.shot.duplicate` remain **candidate names, not registered commands**. Coordinate naming and typed contracts with the implementation thread. Do not create parallel `render.job.*` or `render.appearance.assign` routes, and do not send future recipe fields to current `render.settings.set`.

## Acceptance study

Use a small range of CAD subjects: machined bracket, painted enclosure, PCB, FDM part, wooden jig, LED optic and SELENE. Observe first-time and expert users completing the same three tasks: obtain a default image, change a specific finish without affecting sibling instances, and reopen/export an identical shot.

Record task completion, intervention count, accidental scope changes, time to a usable result, misunderstanding of availability, and whether people can recover with Undo. Test keyboard-only operation and mixed scale assemblies. Treat the “few clicks” objective as unmet if the image looks attractive only after undocumented expert repair.

Detailed acceptance requirements are in [ux.json](ux.json). The source-grounded layout, theme, icon and responsive review matrix are in [UI_INTEGRATION.md](UI_INTEGRATION.md). Usability and visual acceptance require the actual Rust application in both themes and both shell layouts; source review and planning data cannot establish either result.
