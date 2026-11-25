# Preferences redesign: design reference

This folder holds everything the Preferences design boards know, so that nobody needs the
design canvas to build a page. It belongs to tracker #9863 and was extracted from the 13
`Prefs2_*` boards (canvas version 202) plus four reference boards. Once merged it is
authoritative: if a board and this folder disagree, fix this folder in a docs-only pull request.

Nothing in the redesign changes a default. It regroups settings, restyles their controls and adds
the settings-profile switcher.

## Files

| File | What it holds |
|---|---|
| `pages/<page>.md` | One spec per page: sections in order, each control with its verbatim label, control type, the setting it edits, default, hover text, figure, and the mismatches with the code. |
| `figures/*.light.svg`, `figures/*.dark.svg` | Every illustration on the boards as a standalone SVG, verbatim geometry. `figures/README.md` states the house style. |
| `measurements.md` | Dialog frame, spacing, control sizes, radii, type sizes, as measured from the boards. |
| `tokens.md` | Every board colour mapped to a `theme::Tokens` field, or flagged "no token". |
| `boards/` | Not included: the boards are rendered from the canvas, which is private. The figures and the measurements replace them. |

Pages: [General](pages/general.md), [Appearance](pages/appearance.md), [Layout](pages/layout.md),
[Graphics](pages/graphics.md), [Design](pages/design.md), [Sketch](pages/sketch.md),
[Tools](pages/tools.md), [Navigation](pages/navigation.md), [3D mouse](pages/mouse3d.md),
[Shortcuts](pages/shortcuts.md), [Assistant](pages/assistant.md), [Plugins](pages/plugins.md).

## Page structure: 9 pages become 12, in four headings

`General` sits on top, ungrouped. Every heading holds at least two pages. The page list also shows
a profile switcher card at its foot on every page.

| Heading | Pages |
|---|---|
| (none) | General |
| Look | Appearance, Layout, Graphics |
| Work | Design, Sketch, Tools |
| Input | Navigation, 3D mouse, Shortcuts |
| Extend | Assistant, Plugins |

Today's pages (`PAGES` in `crates/ui-egui/src/prefs.rs`): General, Interface, Design, Navigation,
Keyboard Shortcuts, Sketch, Graphics, AI, Plugins. The old board's map (`Prefs2_Map`) showed "Sketch
& selection" under Input and Graphics under "Display"; the later boards and tracker supersede it.

### Where settings move

| Setting group | From | To |
|---|---|---|
| Layout, toolbar, status bar, notices, menus, History and browser placement, dialog placement, toolbar tabs, navigation bar | Interface | Layout |
| Theme, theme options, viewport background, logo, scale and text, colour vision | Interface | Appearance |
| 3D mouse (all of it, including the navigation bridge) | Navigation | 3D mouse |
| Direct interaction, "Tool at the cursor" | Navigation | Tools |
| Hole, Extrude, Placement, Assembly Move (joint handles, near and far steps, ghost) | Design | Tools |
| "Axis distances in Measure" | Graphics | Tools |
| Sketch and selection | Sketch | Sketch |
| Largest file, network locations, slicer reuse, print-face highlight | General / Design | General > Files |
| Renderer, window system, graphics card | Graphics | Graphics ("This computer") |
| Ambient occlusion | Graphics > Edges and shading | Graphics > Current view (a fourth tile) |
| Origin plane names, hide-origin | Interface / Design | Design > Origin |
| Profiles | General | General > Profiles, plus the profile card in every page list |

## Settings profiles

`crates/ui-egui/src/settings_profiles.rs` holds the Interface, Navigation and Sketch preferences,
language, shortcuts, toolbar and S-box favourites, and the imported theme in use. It excludes the 3D
mouse, Graphics and the Assistant. Pages a profile stores (General for language, Appearance, Layout,
Sketch, Navigation, Shortcuts) carry an **In profile** tag beside the page title, with the hover text
"Saved in settings profiles". The profile card at the foot of the page list shows the active profile
name, with a dot and the hover text "Changed since saved" when it was edited.

## Five ways to show a setting

| Way | Use |
|---|---|
| Picture cards | Options that look different. |
| Segmented | Two to four short options. |
| Checkbox | Explanation on hover (the (i) icon). |
| Stepper plus live sample | Numbers with their result. |
| List or table | Per-item data. |

The checkbox, segmented, dropdown, slider, stepper and badge are existing `ui_kit` controls. New:
the page-list group headings and profile card, the "In profile" tag, the picture card, toggle tile,
preset card, hover-help icon and carousel (step 2 of #9863), the theme carousel and options panel,
the layout preview, the navigation-bar editor and the colour vision sample row.

## Design rules the boards follow

1. **Few words.** No explanatory paragraphs. Help moves to an (i) icon with a hover tooltip (the
   `title` text on the board, quoted verbatim in the page files). Visible text is labels, values and
   short badges ("Restart needed", "Desktop only", "Not on this GPU").
2. **One illustration style**: see `figures/README.md`.
3. **Generous spacing**: 38 px between sections (`measurements.md`).
4. Colours are the existing theme tokens, never literals; where a board colour has no token,
   `tokens.md` says so.
5. Section titles are plain text on the board and upper-cased by the style
   (`text-transform: uppercase`, 11 px, weight 600, tracking 0.07 em). Write them in sentence case
   in catalogs.

## Reading a page file

Each control row is `Label | control | edits | default | hover | figure | notes`:

* **Label** and **hover** are verbatim from the board. Board spelling is UK English (Centre,
  Colour); the catalogs hold US text in `en.json` and the UK spelling in `en-GB.json`.
* **Edits** is the `Prefs` field (in `crates/ui-egui/src/prefs.rs`, the `preference_settings!`
  table) or the app/session field or command that the control changes, as of main at the time of
  writing. `app.ui.*` and `app.session.*` fields are session values that Preferences edits through
  its `Snapshot`.
* **Default** is the default of the field in `Prefs::default()`.
* **Notes** flag every mismatch: a control with no setting behind it, a setting with no control on
  the board, a label that differs from the current catalog, and anything uncertain.

## Controls with no board

Settings present in the code with no control on any board (not to be added by this redesign):
`parameters_apply_live` (Parameters apply live) and `personal_sheet_rules`. Settings that exist
only for other features and sit on no Preferences page (`render_*`, `save_mesh_*`, `view_dxf_*`,
`send_slicer`, `custom_slicers`, `easyprint_consent`, `update` and similar memos) are also not on
the boards.

## Placeholder content (not to be built as drawn)

The boards show sample content. Build the real data, not these examples:

| Page | Placeholder |
|---|---|
| General | Profile names (Laptop, Large monitor, Presentation), the network host chips (`\\files.studio`, `\\nas01`), "3 saved", "2 saved" crash reports, the update time. |
| Appearance | The "Studio glass" imported theme and its options; the sample "Extrude 112.5 mm" distance row. |
| Layout | The navigation-bar slot icons and names (Faces and edges, Spin, Projection, Ground shadow, Ambient occlusion) stand in for the app's own slots and icons; "10 of 16 slots". |
| Navigation | The mapping rows and the mouse figure show the Fusion-style mapping only as an example of the shape. |
| 3D mouse | The device ("SpaceMouse Compact"), the live bars, and the button jobs (Open Preferences, View · Fit, View · Top, Hold Shift, Not assigned). |
| Shortcuts | The whole command list, groups and key assignments. |
| Assistant | Agent names and states (Claude Code, Codex CLI, Local model), "Signed in", "Key in environment". |
| Plugins | The three plugin cards (Gear generator, Bracket tools, Thread library), their versions, permissions and descriptions. |

## Known gaps

* The boards were checked in a browser in the light theme after the last edits; the dark half is
  generated from the same layout.
* Theme options follow the text of `Theme_Options.dc.html`; the Appearance board was not compared
  against its rendering.
* The graphics presets are the three in the code (Performance, Quality, Custom). An earlier board
  showed a fourth, "Balanced", which does not exist.
* Screenshots of the boards (`boards/*.png`) are not included. The canvas is private and the
  extracted figures, measurements and token map are the substitute.
