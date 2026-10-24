# Render workspace UX roadmap

Tracking issue: #6281. Related: #5493 (render integration), #6201 (Render usability audit).

The Render workspace on `main` (basic delivery #6050 and the full ribbon, inspector and
libraries from #5494) works but is laid out as one tab with an 11-page inspector. This plan
restructures that UI, in small PRs, around the order a render is prepared. It reuses the existing
commands and controls; it does not add a second renderer or change the document format.

## Interaction design

- **Tabs:** Appearance, Scene, Camera, Render. Scene groups: Studio, Lights, Stage. Camera
  groups: Shots, View, Lens, Frame. Render groups: Image, Render, Result, Batch. Render and
  Cancel stay at the right of the tab row on every tab. Opening a tab or an editor never
  applies, saves, regrades or starts anything.
- **Panels:** floating cards built only from the shared `ui_kit` components: a left dock with the
  Browser scene tree (Environment, Lights, Shots, Objects, with a By object / By look lens) and the
  Appearances library; a right Inspector that follows the active tab and selection; a hideable
  Shots strip at the bottom (`view.shots_strip`, a UI preference); a HUD with the Pick control.
- **Picking:** body, face or component picking in the product view, with outlines that exist
  only in the live view and never in images.
- **Assigning an appearance:** click a library look, drag it onto the model, or paint; the
  Inspector shows where a look comes from (face, body, component, physical material).
- **Lights and shots:** named lights with solo and mute and an orbit manipulator; shots with
  thumbnails, Update and Revert, a frame overlay and aspect presets.
- **Results:** the existing result viewer, then several retained images and a Live | Render compare.

## Order of work

Each step is its own PR; steps that edit the shared Render files are taken one after another.

1. Roadmap and claim (this document).
2. Original icon drafts for owner review (assets only).
3. ui_kit widgets: lit sphere thumbnail and look chain.
4. Shell: task tabs, floating panels, Shots strip, HUD, free-canvas navigation.
5. Picking and outlines in the Render view.
6. Browser scene tree.
7. Appearance engine: `appearance.chain` (read), `appearance.replace`, tests for From Material.
8. Appearances library grid (shared card stacked under the Browser; applicable resources first).
9. Shots strip with bounded, off-thread thumbnails; Update and Revert.
10. Appearance Inspector with the look chain, Applies to scope and mixed values.
11. Assign gestures: click, drag, paint, Pick Up, copy and paste.
12. Output inspector and result viewer additions: size presets, Check, Copy, progress pill.
13. Frame overlay and aspect presets.
14. Scene visuals: environment strip, rotation ring, studio presets.
15. Lights: named lights, solo and mute, orbit manipulator, Place by Highlight.
16. Several retained results, compare, render all shots.
17. Focus view, compact and dark layouts, localisation pass.
18. A lighting rig shared across shots (needs an owner decision and one `format/pending` file).

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
