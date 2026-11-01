# Render workspace UX roadmap

Tracking issue: #6281. Related: #5493 (render integration), #6201 (Render usability audit).

The Render workspace on `main` (basic delivery #6050 and the full ribbon, inspector and
libraries from #5494) works but is one ribbon tab over an 11-page inspector. This plan
restructures it, in small PRs, around the order a render is prepared. It reuses the existing
commands and controls; it does not add a second renderer or change the document format.

## Design rationale

Render does not use tools; it configures the variables of a scene. A ribbon suits tool-driven
workspaces, so this one has none. The four stages at the top (Look, Light, Frame, Output) carry
the most common tools and the major settings. Configuration happens in the Inspector on the right
(the user can move it). Presets and assets load from the panel at the bottom left. Anything in
the scene, such as a body, a light or the environment, is edited by selecting it in the Browser
or in the view.

## Interaction design

- **One selection.** The Browser lists what exists: Environment, Lights, Shots, Objects and Output.
  Selecting a row, or picking in the view, selects the same thing. The Inspector shows the one
  editor for the selection; with nothing selected it shows the scene overview. There is no panel
  switcher and no All page.
- **Stage bar.** At the top, as tall as the workspace card, in two rows: stage chips (Look, Light,
  Frame, Output), Render, Cancel and command search above; the tools of the current stage below
  (Pick and Paint; light placement; lens and Save shot; size, format and Check). Stages choose what
  the Browser expands and what the bottom-left panel offers. They never lock a control or force an
  order. Opening a stage or an editor never applies, saves or starts anything.
- **Bottom-left panel.** Presets and assets for the current stage: Appearances (applicable
  resources first), Studios and Environments, Frame presets and guides, Size presets.
- **Editors** (one per kind of selection):

| Selection | Editor | Holds | In the view |
| --- | --- | --- | --- |
| Body, face, component | Appearance | look chain, Applies to scope, colour, finish, transparency; Advanced: glass, clear coat, maps, saved optics, CPU reflections | outline in the live view only |
| Wear | Wear | aging layers, selected layer, target | none |
| Environment | Environment | source, rotation, intensity, backdrop, blur, ground and stage, atmosphere | rotation ring |
| Lights group | Lighting | light source, shadows and sampling, per-type light budget | none |
| A light | Light | type, intensity, colour, softness, position, aim, Place by highlight, duplicate, delete | handle, intensity dial, distance, orbit |
| A shot | Shot | projection, field of view, distance, exposure, frame and aspect, size, update, revert, duplicate, remove | frame overlay |
| Output | Output | renderer, size, format, transparency, destination, render | result viewer |
| Output › Sampling | Sampling | quality, path tracing, adaptive sampling, glass transport, CPU ray tracing | none |
| Output › Effects | Effects | tone, creative looks, lens effects, finishing, regrade | none |
| Output › Check | Check | request check, this request, renderers, under the hood | none |
| Nothing | Scene overview | one line per stage, Check, Render shot | none |

[Render workspace panels](render-panels.md) lists every panel, what it is attached to, how it is
reached and when, and where each control of the removed Render studio panel went.

- **Docks.** The Browser and the Inspector float and share the same top and bottom edges, starting
  below the stage bar and ending above the Shots strip. The compass floats in the canvas beside
  the Inspector. The Shots strip (`view.shots_strip`, a UI preference) hides from its chevron.
- **Picking and assigning.** Body, face or component picking with outlines that exist only in the
  live view and never in images. Click a library look, drag it onto the model, or paint; the
  Inspector shows where a look comes from.
- **Results.** The existing result viewer, then several retained images and a Live | Render compare.

## Where the old ribbon items go

| Ribbon item | Now |
| --- | --- |
| Assign, Paint, Pick Up | Look tools, quick bar on the selection, drag from the library |
| Edit, Replace All, Remove, From Material | Appearance editor and the right-click menu |
| Library, Same Look | Appearances panel; Select same look in the row menu |
| Presets, Environment | Studios and Environments panel; Environment row |
| Add Light, By Highlight, Solo | plus on Lights; Light editor; solo and mute on the row |
| Ground, Background | Environment editor |
| Save Shot, Update, Strip | Frame tools, the Shots strip, the strip chevron |
| Fit, Views | view bar at the bottom of the view |
| Lens, Exposure, Aspect, Guides | Shot editor and Frame presets |
| Size, Transparent, Quality, Check | Output tools, Output editor, Size presets |
| Render, Cancel | stage bar; R and Esc |
| Open, Compare, Export, All Shots | result viewer; Output editor and the strip |

Every command stays registered, so search (S), keys, the radial menu, scripts and the control
channel still reach all of them.

## Order of work

Each step is its own PR. Steps that edit the shared Render files are taken one after another.
"Keep" means the open PR stays as it is, "Modify" that it changes to fit the design above.

1. Roadmap and claim (this document). Modify: rewritten for this design.
2. Original icon drafts for owner review. Keep.
3. ui_kit widgets: lit sphere thumbnail and look chain. Keep.
4. Shell: stage bar replaces the four tabs and the ribbon; aligned floating docks; Shots strip host;
   free-canvas navigation. Modify.
5. Picking and outlines in the Render view. Keep; the Pick control sits in the Look tools.
6. Browser scene tree with Output row, plus on groups, solo and mute on rows, stage expansion. Modify.
7. Appearance engine: `appearance.chain`, `appearance.replace`, From Material tests. Keep.
8. Bottom-left stage panel with the Appearances grid; applicable resources first. Modify.
9. Assign gestures: click, drag, paint, Pick Up, copy and paste; Paint and Pick Up in the Look tools. Modify.
10. Appearance editor with the look chain, scope and mixed values; Advanced section. Modify.
11. Shots strip with thumbnails, Update and Revert. Modify: drop the ribbon item.
12. Output editor and result viewer: size presets, Check, Copy, progress pill. Modify.
13. Selection model and Inspector dispatch: one selection, one editor per kind, scene overview. New.
14. Movable Inspector (dock side). New.
15. Environment editor, ground, Studios and Environments panel, rotation ring. New.
16. Lights: named lights, Light editor, solo and mute, handles in the view, Place by highlight. New.
17. Shot editor, frame overlay, aspect presets, guides, Frame presets. New.
18. Output children (Sampling, Effects, Check) and Size presets. New.
19. Several retained results, Live | Render compare, render all shots. New.
20. Focus view, compact and dark layouts, localisation pass. New.
21. Deferred: a lighting rig shared across shots. It needs an owner decision and one
    `format/pending` file; no UI step depends on it, so the UI work goes first and every shot
    keeps its own lights until it is decided.
22. Wear, Lighting, Sampling and Check editors with every remaining control of the Render studio
    panel moved to its home ([panel plan](render-panels.md)); the Render studio panel is deleted. New.

## Rules for every PR

- Panels float and use `ui_kit::Container`, `Panel`, `Menu`, `Confirm` and the shared fields;
  colours come from `theme::Tokens`; cameras use `navigation::handle`.
- Everything is a command. New ids must not collide with `render.*` ids already registered;
  planned controls are faded with their reason and refuse without changing anything.
- No panics; input-derived numbers are bounded and finite. UI preferences are never saved in
  the design. A saved concept is one new `crates/doc/src/format/pending/<name>.rs`.
- Labels are catalog entries in en, ja, zh, de and ko (`cargo xtask lists --fix`).
- Commit messages are titles only. Each PR carries targeted tests and, for visible changes,
  before and after screenshots.
