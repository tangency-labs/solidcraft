# Render UI integration with the SolidCraft redesign

## Decision and design authority

Render will use the implemented SolidCraft shell, tab language, approved icons and shared controls. The aim is a visually coherent workspace in which a useful image starts with a scene and quality choice, while precise optical and rendering controls remain available in context.

This document is an **implementation specification**. It adds no runtime UI, screenshots of new behavior, icon artwork or registered command. [ui-integration.json](ui-integration.json) records the source revisions, identifiers, layout roles and acceptance cases. [UX.md](UX.md) explains the broader product workflows; [ux.json](ux.json) retains the 56 stable requirement IDs.

Two revisions have different jobs:

| Revision | Purpose |
|---|---|
| Personal integration base: `93a2922c5ee272fb605703ae84c05825b4a74607` — 0.18.2 | The fixed `lab/render-materials` snapshot. Runtime integration stays on this line until the owner changes that decision. |
| Upstream reference: `728f753b8094e1a54c5cc8b7e453982afa028023` — 0.18.4, inspected 2026-10-21 | Read-only check of the latest published main revision's implemented visual language. No merge or rebase is implied. |

The source audit found the relevant workspace layout, shell, Render panel, theme, palettes, UI kit and icon rules unchanged between these revisions. `toolbar.rs` has unrelated Sketch preference and assistant-motion changes; shared navigation also changed upstream, so reuse the navigation implementation on the integration branch rather than copying a camera handler from either snapshot. The JSON source records retain exact Git blob identities for comparison.

Use [the implemented workspace](https://github.com/tangency-labs/solidcraft/blob/728f753b8094e1a54c5cc8b7e453982afa028023/crates/ui-egui/src/workspace.rs), [toolbar](https://github.com/tangency-labs/solidcraft/blob/728f753b8094e1a54c5cc8b7e453982afa028023/crates/ui-egui/src/toolbar.rs), [UI kit](https://github.com/tangency-labs/solidcraft/blob/728f753b8094e1a54c5cc8b7e453982afa028023/docs/ui-kit.md) and [theme](https://github.com/tangency-labs/solidcraft/blob/728f753b8094e1a54c5cc8b7e453982afa028023/crates/ui-egui/src/theme.rs) as the authority. The older [redesign boards](https://github.com/tangency-labs/solidcraft/blob/728f753b8094e1a54c5cc8b7e453982afa028023/docs/design/ux-redesign/README.md) explicitly describe pictures of a proposal. They help explain the design direction, but do not override the actual code or establish current behavior.

## 1. Preserve the shell hierarchy

### Workspace, ribbon, local navigation and disclosure

| Level | Existing basis | Render extension |
|---|---|---|
| Application | Shared document tabs and application actions | Preserve document identity, Save, Undo/Redo, command discovery and global navigation. |
| Workspace | Workspace card and switcher using `ws_render` | Enter through the existing Render lifecycle; returning to Design restores its camera and projection policy. |
| Ribbon tab | `workspace::visible_tabs` returns only `RENDER` in Render | Keep one Render ribbon tab. Add useful task groups when their commands actually exist. |
| Ribbon group | `SCENE` and `IMAGE` contain `render.scene` and `render.start` | Preserve their group-menu, promoted-command, pinning and overflow behavior. |
| Local resource browser | Existing Appearance library consumer and shared search/list controls | Add a local Scenes / Materials / Lighting / Environments category selector. |
| Inspector | Existing controls in `render_workspace::panel` | Organize related values into the shared disclosure sections, with a contextual summary above them. |
| Canvas surface | Existing product viewport | Add a local Viewport / Result selector only when the result canvas is integrated. |

**Scene and Image are not tabs in the current implementation.** Do not label a proposed tab bar as an existing feature. Do not promote the four resource categories, the three disclosure depths or each quality profile into new global workspaces.

The planned ribbon can grow in this task order:

| Group | Promoted intent | Further actions in the group menu | Availability |
|---|---|---|---|
| Scene | Scene presets / saved shot | Duplicate shot, reset overrides, inspect scene | `render.scene` currently inspects metadata; a preset picker and apply contract are future work. |
| Appearance | Assign appearance | Local library, mapping, layers, selected scope | Reuse `appearance.assign` and the existing library consumer; advanced mapping and scopes need support. |
| Lighting | Choose studio rig | Add or edit owned emitters, source contributions | Proposed; expose only after rig and light commands are bound. |
| Environment | Choose environment | Ground, terrain, background, physical scene policy | Proposed; expose only after environment resources are bound. |
| Camera | Frame subject / saved camera | Lens, clipping, depth of field, saved shots | Reuse shared navigation and supported camera commands; optics remain capability-dependent. |
| Image | Render image | Quality/output, Check, progress, cancel, export | Extend existing image-job commands and controls. |

These are task-group names and intents, not a new set of registered commands. Start with one or two promoted actions per added group and let the existing ribbon fitting code manage overflow. User pins and keyboard discovery must survive the extension. The primary action should be a stable, named Render Image command; clicking a decorative preview must never start an expensive render unexpectedly.

### Floating and Docked layouts

Honor `shell::LayoutMode::Floating` and `Docked` as the application already does. Floating uses the established cards, gaps, rounding and shadows over the viewport. Docked uses the existing edge alignment and compact shell geometry. Render does not get its own independent layout preference.

The current Render settings container is a fixed 285-point right panel using kit controls. Moving it to `ui_kit::Panel` with `feature_frame`, bounded content and pinned actions is **planned integration**, not a description of the current container. Use the shared sizing and occlusion rules rather than hard-coding a separate Render layout engine.

Preserve access to the document browser. A future left resource surface may offer local Model / Library views within the same bounded area; the tree must retain visibility controls, selection and scroll state. Reuse `dialogs_library::LocalLibrary` and its command-backed import/apply behavior when expanding the Appearance library. Rendering assets do not justify an unrelated second library database or a replacement component tree.

Keep the compass and navigation controls usable. Register every new card, popup and tray with the shell's measured covered rectangles so clicking a slider cannot also orbit or select through it. Short windows must keep the active form and its actions reachable by scrolling rather than painting over the compass or the history strip.

## 2. A clear visual hierarchy

The rendered subject provides most of the visual interest. The surrounding controls use the current neutral surfaces, consistent typography and restrained selected-state color. A resource card earns space through a useful preview and readable name; decorative frames and multiple competing accents would make comparisons harder.

### Token and component mapping

| UI role | Existing authority | Integration rule |
|---|---|---|
| Render tab selection | `TabAccent::for_tab(..., "RENDER")` with `render_fill`, `render_ink`, `render_soft` | The selected ordinary Render tab uses `render_ink` on `render_soft`; retain the existing ribbon styling. |
| Global actions and status | `accent`, `on_accent`, `form_primary`, `form_error`, `form_success` and related fills | Preserve each semantic role; selecting Render does not recolor all global actions or warnings. |
| Card surfaces and rules | `dialog_body`, `dialog_footer`, `dialog_border`, `dialog_rule`, `panel`, `toolbar` | Obtain colors through `Tokens`; respect built-in and custom themes. |
| Controls | `control_height`, `control_radius`, `control_fill`, `control_border` | Use the kit's existing geometry and focus behavior. |
| Typography | `font_label`, `font_body`, `font_heading`, `font_small`, existing semibold and monospace helpers | Use established text roles; numbers and units align consistently. |
| Spacing and rounding | `space_*`, `radius_*`, shared Panel/Window frames | Use existing tokens, not per-dialog constants or copied screenshot measurements. |

Current defaults include 28-point controls, a 40-point header, 48-point footer, 264-point card basis and 70-point compact label basis. Table rows use `control_height + space_8`. These are observed values in `assets/styles/ui/palettes.rs`; callers should consume the tokens. The older board's 24-pixel rows do not replace the current kit measurements. Points are logical UI units, not a requirement to fix physical pixels at every display scale.

`render_accent` still exists, and `render_fill` is part of the Render accent vocabulary, but the ordinary selected Render tab uses `render_soft` and `render_ink`. `TabAccent::on_fill` belongs to the solid contextual-tab treatment used elsewhere. `icon_accent_for_tab` currently defines no Render icon override. **Keep each icon's existing semantic palette.** A Render tab tint is not permission to recolor shared SVGs or to repaint all actions in the tab color.

### Existing icon vocabulary

| Intent | Existing ID | Current binding or restriction |
|---|---|---|
| Render workspace | `ws_render` | Workspace card and switcher. |
| Inspect render scene | `render_scene` | `render.scene`; do not change its command meaning into apply-preset. |
| Render image | `render_image` | `render.start`. |
| Visual appearance | `appearance` | `appearance.assign`. |
| Engineering material | `material` | `material.assign`; retain its distinction from appearance. |
| Library | `library` | Existing Appearance library vocabulary. |
| Saved camera | `camera` | `render.settings.apply` and existing view actions. |
| Settings | `settings` | Existing Render settings commands. |
| Check request | `inspect` | `render.preflight`. |
| Progress / backend list | `list` | `render.status`, `render.backends`. |
| Cancel render | `delete` | Current `render.cancel` assignment; always show the Cancel Render label in job UI. |
| Export result | `export` | `render.export`. |
| Surface environment analysis | `environment_map` | Currently `inspect.environment_map`; this is not an HDRI import command. |
| Navigation | `fit`, `orbit`, `pan`, `zoom` | Keep existing shared navigation actions and labels. |

The literal icon IDs `sun`, `light`, `lighting`, `environment`, `scene`, `texture`, `quality`, `floor` and `stop` are not present in the inspected set. Use clear text for proposed actions until a semantically appropriate existing icon or reviewed alias/new master is assigned. A reused drawing cannot change the behavior of an existing command.

No SVG or acceptance record is changed by this contribution. Existing accepted `ws_render`, `appearance` and `render_image` masters were inspected at 64, 24 and 16 pixels in both themes through the repository palette renderer. Those checks concern artwork only. Future icon work must follow `assets/icons/ui/STYLE_GUIDE.md`, aliases and review protection; never copy a private variant into Render.

## 3. Library and preset discovery

### First view

Show a small, curated selection that fits the current subject and executor, with category, search and availability readily visible. Product, Automotive, Electronics, Making, Aerospace and SELENE are collections, not new application modes. Keep Recently used and Favorites nearby. The full concept catalog remains discoverable through an explicit concept/unsupported filter, with counts that distinguish authored concepts from renderable resources.

A scene card should contain a consistent thumbnail, short name, intended subject/scale, relevant availability and one clear action. Put technical details in its inspector. A missing native model or unsupported optical feature remains readable on the card; do not make a disabled button the only way to discover the reason. Never present a text-only concept with a fabricated “validated render” image.

Materials use a declared neutral reference rig, camera, exposure and sample shape. Offer “Preview in scene” as a separately labeled inspection. Keep scene lighting out of the calibration thumbnail so a dark preview is not mistaken for a different albedo. Rendered thumbnails require a shared image-backed kit element when implemented; the current `color_thumbnail` is a color swatch and does not by itself supply a 3D preview cache.

### Search and interaction

Search exact IDs, English names, process, intended use and finish. Allow conjunctions such as Automotive + Coated metal + Supported, and show active filters with a clear reset. Preserve search, filters, sort, selected resource and scroll position independently for each category and document context. Empty results explain which constraint filtered them out.

Use `search_field`, `searchable_dropdown`, `list_picker`, `segmented`, shared menus and table controls. Local category choices use visible text; at narrow widths the normal segmented control can fall back to its shared dropdown behavior. Do not force clipped icon-only choices through `compact: true` when their names no longer fit.

Click selects and inspects a resource. Apply commits a scene or assignment. A temporary hover preview, if implemented, is cancelable and must not dirty the document or start downloads/renders without a declared policy. Keyboard focus provides the same information; hover is never the only access path. Drag/drop is optional convenience after target picking is available, with a keyboard Apply alternative.

At present `NavCaps::RENDER` disables picking, and the Render viewport omits selection highlights. Assignments should initially use supported selection/browser inputs and the existing Appearance dialog. In-canvas face/occurrence targeting, prospective outlines and light handles require an explicit integration of selection identity and hit testing. Reuse `SelInput`, the engine's selection contracts and full occurrence paths; never infer persistent identity from triangle indices or visible labels.

## 4. Inspector and precision

Use `ui_kit::Panel`/`Window`, `scope`, `row` and `section`/`section_count` for the planned inspector. The header names the current subject/resource and its scope. The first section exposes at most six relevant adjustments; scene and quality pickers are separate selectors. Inapplicable fields stay out of the primary section. An unsupported field required by the chosen resource stays visible with its reason.

| Context | Example primary controls | Deeper sections |
|---|---|---|
| Appearance | Finish, color, roughness, map scale, direction, coat amount where supported | Layers, conductor/dielectric response, transmission/absorption, mapping, provenance. |
| Studio rig | Rig, intensity/exposure relationship, rotation, source size, key/fill balance, background separation | Per-source units, sampling, visibility, contribution ownership. |
| Environment | Resource, rotation, ground height, ground finish, background treatment, scale where meaningful | Terrain generation, displacement, memory, physical scene policy. |
| Camera | Saved view, framing, projection/lens, focus distance, aperture, clipping where supported | Sensor/aspect, shutter, motion blur and diagnostic overlays. |
| SELENE physical scene | Sun azimuth, Sun elevation, exposure, framing, terrain seed, dust coverage | Site/time assumptions, source binding, hardware emitters, optical roles and procedural detail. |
| Output | Quality profile, resolution, aspect, format, alpha, output destination | Sampling, denoising, passes, color transform, executor and memory limits. |

This table is a control budget, not a request to repeat the scene picker or exposure slider in every panel. Each property has one owner; other contexts may show a linked summary that focuses its owning field. Never give the same property two unsynchronized sliders.

Quick is the default collapsed inspector. Detailed means opening a named section. Expert means opening Advanced within that section. Disclosure does not change saved scene values, executor, image quality or physical scene policy. Preserve section state per relevant context, and show changed-field counts and warnings in collapsed headers.

Use `number_field`, `length_field`, `angle_field`, `scalar_field`, `slider` and `color_field`. Provide typed input, unit suffix, reset and a useful range. Show whether a value is inherited, overridden or locked. A scalar roughness control never becomes a “quality” slider. Color controls must identify their actual encoding; an RGB swatch cannot imply spectral data, calibrated radiance or a measured emission spectrum.

### Drafts, scope and undo

Keep resource browsing and preview drafts separate from applied document state. A preset apply resolves dependencies, scope and locks before a single document transaction. If a pending draft would be replaced, offer Apply / Discard / Keep editing at that conflict; ordinary section expansion or category navigation must not discard it. Existing Save Setup and Reload Saved Settings semantics remain clear while the richer resource layer is introduced.

Show face, body, component definition or supported occurrence scope in plain text before committing an appearance. Explain shared-definition effects with an affected-instance count. A material appearance preset preserves density, inertia, friction, restitution and joint limits. Do not expose an occurrence-only edit until the engine can persist and reproduce it.

Source, appearance, camera, light/environment and simulation locks are typed constraints. Show an actual conflict and the proposed changed fields, then either apply atomically or leave the scene unchanged. Locks cannot be silently disabled to make a preset appear successful. Reopening a saved shot must identify missing resources and stale source hashes without changing its intended appearance.

## 5. Image quality, progress and results

A default supported still should need roughly three to five deliberate actions after subject selection: open Render, choose a scene, optionally choose quality, and Render. Preflight occurs inside that final action on the exact immutable request. Keep a separate Check action for inspection, but a successful unchanged request should not demand a redundant confirmation. A real unresolved conflict can add a necessary step; record it in the usability study.

The quality picker starts with three relevant exact catalog profiles and offers the complete list. Dimensions, major supported features and any estimate appear nearby. Cost estimates must say what hardware and measurement they use; an invented time-to-finish is worse than an honest “Preparing”. Editing an output value marks the profile Custom without overwriting the original preset.

Preserve `render.preflight`, `render.start`, `render.status`, `render.cancel` and `render.export`. The result canvas is a future consumer of the completed job's actual image, not a second renderer or a Design screenshot. Its displayed aspect, crop, alpha, display transform and exported image must agree. A/B comparison locks framing, exposure and color transform by default and makes deliberate unlocks visible.

Keep preparation, running, encoding, canceling, canceled, completed and failed states distinct. A completed result stays available after a later failure when its owner still exists. Scene edits mark that result stale; switching documents cannot assign it to another design. A running job uses its captured request, even while another draft is edited. Cancel stops the owned job and preserves the document. Export targets the selected completed job and retains existing overwrite behavior.

Use the existing progress controls first. A future bounded result/job tray must share the shell's layout and occlusion accounting and must not create a permanent second global status bar. The current shell hides modeling history while Render is active; a result tray belongs to Render's layout, and returning to Design restores its existing history presentation. Global document operations remain in their existing application locations.

## 6. SELENE within this same interface

The [demo flow](../../assets/render/selene/ui/demo-flow.json) specializes this workspace. It does not open a separate lunar application or introduce another design language. The recommended hero, sample macro, LED work patch and studio diagnostic scenes use the same cards, quality picker, inspector and result state.

The default scene policy is Physical lunar. The Sun has one owner; atmosphere and automatic studio/sky fill stay off. Earth and star backgrounds remain off until their separate site/time/exposure binding is reviewed. Editorial scenes carry their label in the scene card, inspector and saved request. A more attractive unphysical fill must never be silently introduced by a quick-adjust control.

Until the improved native model is admitted, a SELENE scene explains **which source is missing** and offers its source record. It does not claim that the historical v7 is ready to render instead. The opaque appearance swatches are a separate prepared input: open them in a **new isolated document/process**, because the script begins with `file.new`. They have not been executed here and do not establish optical or lunar-scene readiness.

Later animation can add an approved bake selector and per-shot frame range through the same context. Joint setup, sample mass, contact physics and catch validation remain in their owning module. Changing render quality preserves the approved trajectory; changing physics invalidates it. The still workflow remains usable before that module exists.

## 7. Accessibility and responsive acceptance

The following are requirements to test in the application, not results of this planning contribution:

| Case | Required outcome |
|---|---|
| Light, dark and a custom theme | Shared tokens determine every control; text, selected state, focus and warning semantics remain readable. |
| Color-vision modes | Use the application's existing variants; meaning survives without color through labels, selection marks and status text. |
| Floating and Docked | Same commands, values and shortcuts; overlays cannot capture the wrong viewport action. |
| 1024 × 768, 1440 × 900 and 1920 × 1080 logical viewports | Search, active field, Render/Cancel and panel close remain reachable; ribbon groups use existing overflow behavior. |
| 100%, 150% and 200% display/UI scaling where supported | Text and hit areas follow logical metrics; there is no clipped primary action or forced microtext. |
| Keyboard-only | Logical focus order; searchable resources, numeric edits, scope, Apply, Render and Cancel are operable without hover or drag. |
| Assistive technology | Programmatic labels include units, selected scope and availability; progress announces phase changes without reading every sample. |
| Reduced motion | Preview animation is optional; no auto-turntable is required to inspect a resource, and focus does not follow moving imagery. |
| Long names, large values and future localization | Shared measurement/wrapping or full tooltips preserve meaning; values cannot overlap neighboring controls. |
| 600 materials and mixed availability | Search, selection and scrolling remain responsive; lazy previews cannot replace the selected item or move focus when they arrive. |
| Multiple documents and pending edits | Drafts, quality choices, source bindings, jobs and results remain owned by the correct document. |
| Missing source, unsupported optics or memory rejection | A readable blocker and recovery action appear before mutation; there is no silent approximation. |

Honor existing shortcut ownership. S continues to invoke the shared command search when no editor owns it. Tab/Shift-Tab traverse fields; Enter belongs to the active editor or selected action; Escape closes/cancels the current local interaction. Do not let an Enter used to commit a numeric value also start a render. Keep focus on the initiating control after menus/dialogs close and expose status without relying only on a disabled control's tooltip.

Measure first-time users on a bracket, painted housing, PCB, FDM part, wooden jig, LED optic and SELENE once supported. Record completion, interventions, scope mistakes, recovery with Undo and recognition of unsupported resources. A visually attractive image obtained only after undocumented expert repair does not meet the few-click objective.

## 8. Implementation order and evidence

1. **Adopt the existing shell and kit.** Confirm source mappings against the actual integration branch; align the settings container and disclosure without changing render semantics. Capture real before/after application evidence.
2. **Bind a small useful resource slice.** Extend the existing library and engine contracts; expose only supported presets and exact assignment scopes. Keep the broader concept inventory clearly separate.
3. **Integrate preview, requests and results.** Validate shared preflight/start behavior, per-document drafts, result ownership, cancel/export and toolbar overflow.
4. **Bind SELENE.** Admit the improved native source and reviewed appearances/stages, then exercise the same interface for physical hero and macro scenes.
5. **Extend advanced optics and animation as supported.** Add relevant sections without changing the basic workflow or introducing duplicate state owners.

For any new common control, extend `ui_kit`, its documented tokens, `gallery::ELEMENTS` and the appropriate existing tests instead of adding a private Render painter. Retain automation handles and accessible labels. Keep the UI thin; resource resolution, capability checks and document mutation belong in the engine, with UI calls through `app.run` / `app.start` and camera input through `navigation::handle`.

Actual implementation acceptance needs repository gates, native/WASM checks where relevant, real application captures in both themes/layouts and the interaction cases above. Source inspection, the icon contact sheet and JSON validation establish preparation only. [VALIDATION.md](VALIDATION.md) records what has actually run.

All content here is original planning based on SolidCraft's own public code and design rules. No commercial CAD interface, artwork, material library or GPL implementation was consulted. Existing assets remain in their original locations with their original acceptance records.
