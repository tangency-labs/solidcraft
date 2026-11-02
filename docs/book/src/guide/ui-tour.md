# A tour of the window

![The SolidCraft window](../generated/sample-plate.png)

- **Title bar.** On Windows and Linux the application bar is the window's title bar. On the left:
  the File menu (New, Open, Save, Save As, Export…), undo and redo, the Home tab (the start page)
  and one tab per open design (each with its own undo history, selection and camera; a dot marks
  unsaved changes). Double-click a design's tab to rename the design: type the name, then press
  Enter or click elsewhere to keep it, or Esc to leave the old one (Preferences › Interface turns
  this off, for a double-click that maximizes the window instead). On the right: Report a bug,
  Help (?), Preferences (which hold the theme) and the window's buttons. On Linux, to have your
  desktop's own title bar and window buttons instead, set
  `SOLIDCRAFT_TITLEBAR=system` or put `"titlebar": "system"` in `graphics.json` in the settings
  folder (see [Troubleshooting](troubleshooting.md#reset-the-settings)) and restart; `app` (the
  default) puts the application bar back.
- **Toolbar.** The workspace switcher (DESIGN) sits on the left. The tabs (MODEL, SURFACE,
  ASSEMBLE, INSERT, INSPECT, MANAGE, VIEW and SHEET METAL) hold sections of commands. SKETCH appears while
  editing a sketch; SHEET METAL is always there and is highlighted for the active sheet body. Both select automatically
  when entered, and the other tabs remain available. Click a
  panel's name for all its commands. Drag a command from the dropdown onto a panel's label
  or empty space to append it; the insertion line shows its destination. Drop onto an existing
  button to insert before it. Buttons can also move between panels, and the layout is saved.
  Click the **+** after the tabs, right-click a tab or empty toolbar space, or use
  **Preferences > Interface > Toolbar tabs** to hide or restore tabs. Every existing tab,
  including Sheet Metal and Weldments, stays visible by default. Tabs you hide stay hidden in every
  design until you show them again. Hidden tabs keep
  their customised buttons, commands in search and shortcuts; with all tabs hidden, **+** remains.
- **Browser** (left). The design as a tree: document settings and units, named views, the
  origin, bodies, sketches, construction planes, canvases, joints and components. The eye at the
  row's right end shows or hides an item (**Preferences › Interface › Eye next to the fold arrow**
  puts it back beside the fold arrow; press an eye and drag up or down over other eyes to show or hide them all at once, as one undo step), a component's radio button makes it the active component,
  and right-clicking any row gives that item's menu. Right-click the design or a component and choose **Expand All** or
  **Collapse All** to open or close every folder under that row (the design row stays visible
  when the whole tree is collapsed). Rows drag into groups and other components. **Preferences ›
  Interface › Bodies folders** sets when a component's bodies sit in a Bodies folder of their own:
  **Several bodies only** (the default) lists a component's body straight under it when it has
  only one and no groups, **Never** lists every component's bodies and groups straight under it
  (the folder's menu items are then on the component's menu), and **Always** keeps the folder.
  The design's own Bodies folder, and a folder hidden with its eye, always stay.
- **Viewport.** Click to select, Shift or Ctrl to add (Preferences › Selection), drag for a window or crossing box. While sketching, Ctrl/Cmd or Alt/Option suppress auto-snap by default; Shift prefers a midpoint; the pointer also snaps to where two curves cross. To
  reach something under something else (two lines drawn on top of each other, coincident points,
  an edge in front of a face, overlapping bodies), hold the left button still for half a second
  (long-press on a touch screen) or right-click › **Select Other…**: a list of everything under
  the cursor opens there, including hidden bodies and sketches near the pointer.
  The popup grows to fit the available window space, opening above the pointer when it needs
  the extra room, and scrolls only when the rows do not fit. Turn off **Select hidden geometry
  with Select Other** in Preferences › Sketch & selection to restore the original visible-only
  list under the pointer. Picking a hidden object leaves its
  visibility unchanged, so an Extrude profile can be chosen through a covering body.
  The sketch being edited comes first. Hovering a row lights it up; clicking it
  selects it (Shift or Ctrl adds), and in a command's dialog it goes to the active selection box.
  Esc or a click elsewhere closes the list. Clicking again on the same spot, with nothing changed
  in between, cycles through visible candidates at that point. The
  wheel zooms at the cursor, the middle button pans and right-drag orbits freely (the mouse
  preset in Preferences switches to SolidWorks, Inventor, Alias, Maya, Blender, Tinkercad, OpenSCAD, HFSS or Touchpad style). On a
  trackpad, pinch zooms and two-finger scroll pans (Navigation › Two-finger scroll pans;
  off: scroll zooms like the wheel); mouse wheels zoom and the thumb wheel pans sideways.
  On Linux, pinch needs the native Wayland window system
  (see [Troubleshooting](troubleshooting.md)).
  To zoom with vertical two-finger scroll while holding a key, choose the key in Navigation › Zoom with two-finger scroll while holding (Cmd on macOS or Ctrl on Windows/Linux, Alt, or Control). It is off until you choose one.
  With the **Fusion** preset and two-finger pan enabled, hold **Shift** while scrolling with
  two fingers to orbit freely instead of panning. No click is needed. Releasing Shift returns
  to panning, even during the same gesture. Shift does not snap this gesture to standard views;
  orbit gestures do nothing on flat 2D pages. Other presets keep their existing scroll behaviour.
  Shift+middle orbits freely
  in the default and Inventor presets. In every preset, Ctrl+left drag (Cmd on macOS) that starts
  on empty space orbits; a Ctrl+click still adds to the selection. Shift+right-drag snaps to the nearest standard or
  diagonal view; Shift does not request snapping when the preset already requires it to orbit.

  Enable **Preferences → Navigation → Free orbit** to rotate the mouse and 3D mouse continuously
  through both poles, preserving roll. During an orbit drag, Shift snaps the view direction and
  the picture rotation to the nearest quarter turn; releasing Shift resumes the unsnapped rotation.
  Under it, **Turn around the vertical axis** makes sideways mouse drags spin the model around the
  world's vertical axis instead of the screen's, while up and down drags still tilt over the top
  without a limit. Both default to off for every mouse
  preset. With SolidWorks navigation, right-click a planar face and choose **Normal to Selection**
  to use Look At (also Ctrl+8); repeat to flip the side, preserving scale and projection.

  **Right drag orbits** (Preferences › Navigation, on by default) is shown where the mouse
  preset orbits on a right drag. Off, a right drag leaves the camera alone and the right button
  only opens the context menu. Picking the SolidWorks preset turns it off and sets Middle
  double-click to Fit; both can be changed back afterwards.

  **Mouse gestures** (Preferences › Navigation, off by default) work with the SolidWorks mouse preset
  only (the wheel orbits, Ctrl + wheel pans) and take over the right button, which then no longer
  orbits: hold it and move the pointer 4 pt to open a ring of tools (280 pt inside, 350 pt outside), even
  while a tool is active, and touch a tool's zone to take it. A right-click that does not move still
  opens the usual context menu. Each studio has its own
  tools, and **Customize…** sets how many tools a studio's ring holds and in which place. Zones
  alternate between two tints, three when the number of tools is odd. The optional radial context
  menu is a separate setting, and gestures take the right button from it while they are on.

  With **Touchpad** selected in Preferences › Navigation, hold **Shift** and move the pointer to
  pan, or hold **Alt** (**Option** on macOS) and move it to orbit—no mouse button is needed.
  Vertical two-finger scrolling zooms in this preset, regardless of the saved two-finger pan
  setting; switching presets restores that setting. Release the modifier to stop navigating.
  Shift+click and Shift+left-drag still select. Combined Shift+Alt or Ctrl/Cmd modifiers do not
  start pointer-only navigation.
  The axis view shows the world's X/Y/Z directions in SolidCraft's colors. Click a signed
  axis tip to look along that axis; click the aligned tip again to look from the opposite side.
  Drag the axis view to orbit around the view centre, with Shift for snapping. Top and bottom
  keep the nearest quarter-turn heading. The house, the navigation bar's Home button and a middle-button double-click in the view go
  home, framing what is shown now (Navigation › **Middle double-click** can make it Fit, which
  keeps the orientation, or turn it off).
  Interface › **OK and Cancel in the view's corner** puts a large check and cross at the view's
  top right while a dialog or a sketch is open, and moves the axis view or cube down below them.
  With a dialog open they do what its OK and Cancel do (the check is grey whenever OK is); in a
  sketch without a dialog they are Finish Sketch and Cancel Sketch.
  **View angles** in the navigation bar's View menu (or Preferences › Graphics, or the
  `view.angle_readout` command) shows the view's pitch, yaw and roll in degrees under the axis
  view, with **Ortho** or **Persp** for the projection; it is off by default and updates as the
  view turns. The angles turn about the up axis: yaw 0° is the Front view, 90° Left, −90° Right
  and 180° Back; pitch 90° is the Top view and −90° the Bottom; a positive roll turns the model
  clockwise on screen. Click the readout to type exact angles; **Apply** turns the view there
  (it keeps the view's centre and zoom). Scripts set the same angles in radians with `view.apply`.
  Right-click for the marking menu: eight commands around the cursor, plus a
  menu for whatever is under it. Near the window's bottom or right edge a menu opens above or
  to the left of the cursor, so it stays in view.
  Shaded views omit certified smooth joins between spline spans on extruded
  walls. Real creases, cap outlines and authored face splits remain visible.
  Wireframe retains its edge presentation, and hiding a join does not remove its CAD
  identity or change the exported solid. Unsupported or uncertain wall geometry
  keeps its existing edge presentation.
- **Dialogs** dock on the right with a live preview. Most features also show drag handles and a
  value box in the viewport. Preferences › Interface › **Dialogs** › *In the left column* opens
  them in the browser's place instead (and the History's, when it is at the left), at least as
  wide as that column; the column comes back as it was when the dialog closes.
  Interface › **Dialog buttons** › **Top** puts OK, Cancel and Apply in
  the dialog's title row, right of the title, where they stay while sections open and close;
  Cancel then also takes the place of the close button. Enter and Esc work as before. The Sketch
  panel's Finish Sketch, Look at and Slice move up under its title, above the options.
- **Timeline** (bottom). Every feature in order. Double-click to edit, drag the marker to roll
  back, and drag features to reorder them. A sketch's icon, here and in the browser, takes the
  colour of its state: the sketch colour while it is under constrained, the fully constrained
  colour, or red while its constraints conflict; its tooltip names the state (Preferences ›
  Sketch › **Sketch icons in the colour of their state**, on by default, turns this off).
  Preferences can put the timeline vertically at the
  far left of the viewport, beside the browser. Its seam handle switches between icon
  chips and compact feature names; the browser can collapse to a rail beside either width.
  Docked under the browser, the two share the column: drag the seam between them to give either
  more height, and the split is remembered.
  Its controls jump to the beginning, step back one feature, play, step forward one feature or
  jump to the end. **Play History**, in the middle of those controls and under
  **Inspect → History**, advances through the remaining features, pausing half a second after
  each rebuilt step appears. It becomes **Stop History** while running; it is also in command
  search and can be assigned a shortcut in Preferences → Keyboard Shortcuts. Click Stop History
  or press Escape to stop playback; navigation and editing stop it too. To keep the controls to
  four, right-click them and choose **Hide Play Button**, or clear the setting under
  Preferences → Interface.
  Interface › **Sketches** › **Under their feature** lists a sketch in the vertical timeline one
  level in, under the first feature built from it, behind that feature's fold arrow. Dragging the
  feature takes its sketch along. A later feature built from the same sketch gets a row that
  selects it, and while the marker sits between a sketch and its feature, or the two are in
  different folders, the sketch keeps a row of its own. The timeline along the bottom always lists
  features in creation order.
  Feature Folders can contain joints as well as features; closing a folder hides both behind one tile.
- **Status bar** (off by default: Preferences › Interface › **Status bar**). A thin bar along
  the window's bottom edge, below the timeline. From the right: the design's units (click them
  to change the design's units, one undo step), the open sketch's state in its colour (under
  constrained with its degrees of freedom, fully constrained, constraints conflict) and what is
  being edited (Editing Sketch1, or Editing Extrude1 while that feature's dialog is open). While
  it is on, the sketch's state no longer shows in the view's top-left corner.
- **Command search (S).** Click the ribbon search field or press S to open the search popup
  below it. Results group commands by their ribbon path and show matching features and user
  parameters from this design. With an empty query, recent commands appear first, followed by
  pinned commands in the Commands group. Up/Down
  choose, Enter runs, Tab fills the selected name, and Esc closes. A feature opens its existing
  editor; a parameter opens Change Parameters filtered to its name. To run with explicit values,
  enter an id and JSON object, for example `solid.extrude {"distance": 20}`. The popup validates
  JSON as you type and shows parser errors. Shift+Enter runs it and keeps the popup open.
  Right-click a command for its existing pin and toolbar actions.

Preferences (File › Preferences) hold the theme, workspace layout, the units of new designs, the mouse preset, the
visual style and autosave. Graphics › Colour vision gives the colours that carry a meaning (sketch
states, axes, bends, analyses, icons) values for a weak red, green or blue sense: Protan, Deutan or
Tritan. In a mode, the faces Draft and Accessibility Analysis mark as failing are striped too.
Graphics › Viewport › **Perspective view** draws the view with perspective, and its **Field of view**
(15° to 90°, about 34° by default) sets how strong the perspective is. The model keeps its size in the
view when the angle changes: a wider angle brings the camera closer. A sketch you are editing is always
drawn without perspective.
Command messages and errors show for a few seconds in the status chip at the bottom left of the view. Click the chip, or choose Help › **Messages…**, for every message of the session: newest first, errors in red, each line selectable, with **Copy all** for a bug report.
Help › Keyboard Shortcuts lists every key, and File › Keyboard
Shortcuts changes them.

### Workspace layout

General › **Workspace layout** switches between **Floating** cards and **Compact**
panels. Compact joins the workspace selector and ribbon into one top bar, puts
Browser flush underneath with a square join, and docks tool sheets below the
compass. Tool sheets remain draggable: their four corners round away from the right
edge, and their two right corners square when docked there. Browser keeps only the height its contents need. Assistant remains on the
outside edge, and History keeps its chosen bottom, left or shared Browser column.
The choice is saved between launches. Switching preserves the current tool,
camera, selection and design; Cancel and Restore Defaults work like other preferences.

General › **Show text under ribbon icons** is a separate, saved option for either
layout. Turn it off for shorter icon-only command buttons; tooltips, section names
and command menus remain available.

Interface › Layout › **Show origin and bodies at the top** (off by default) puts the active
component's Bodies folder and the Origin (planes, axes and point) at the top of the History when
it sits on the left or under the browser and is wide enough for names. They are the browser's own
rows: the same eyes, folds and right-click menus, so one list is enough for single-part work. The
History at the bottom is unchanged.

Interface › **Menus** › **Large icons** (the default is **List**) shows a section's
command menu as rows of large icons instead of a list of names. Point at an icon for its name,
shortcut and help; the arrow keys move across and between rows. Rest the pointer on an icon for
two seconds and a pin in its corner can be clicked to pin it to the ribbon or remove it (a
quicker click anywhere on the icon starts the command). Right-clicking the icon, or dragging it
onto the ribbon, also pins it; a pinned command always shows its pin.

Interface › **Leave pinned commands out of the drop-down menus** (off by default) lists in a
section's menu only the commands that are not already buttons on the ribbon. When the window is
too narrow for a section's buttons, its menu lists them again.

### Colour themes

Interface › **Theme** lists Dark, Light and System (follows the operating system, and changes
with it; on Linux it reads the desktop's dark or light style through the desktop portal, as
GNOME and KDE set it), the built-in
themes and any themes you have imported. A choice applies at once. Under the themes, **Viewport** draws the 3D view in the Light or Dark theme's colours whatever the window uses, for example the model on a light view in a dark window (**Follow theme**, the default, keeps the theme's own). **Logo** draws the SolidCraft logo at the top left in shades of the theme's accent instead of the theme's logo colours. The built-in themes are Gruvbox
(dark and light, each in hard, normal and soft contrast), SolidCraft 95, Platinum 9, Cupertino,
Green Phosphor, Amber Phosphor, Blueprint, One Dark, Tokyo Night, Solarized Light, Nord, High
Contrast, Graphite, Nosferatu, Catppuccin Mocha, Okibi and Akari (dark and light, from the
[Okibi design system](https://kaievns.github.io/okibi-akiri-design-system-spec/)), Clay and
Clay Dark (warm ivory paper and warm charcoal, with a terracotta accent and rounded corners), and
1-bit and 1-bit Dark (pure black and white with square corners: black on white, or white on
black, with selections drawn inverted).

A theme is a small JSON file that starts from the Dark or Light palette and changes any of its
colours and corner radii by name:

```json
{
  "format": "solidcraft-theme/1",
  "name": "My Theme",
  "base": "dark",
  "colors": {
    "panel": "#282828",
    "text": "#ebdbb2",
    "accent": "#83a598",
    "viewport_top": "#32302f",
    "viewport_bottom": "#32302f",
    "axis_x": "#fb4934",
    "axis_y": "#b8bb26",
    "axis_z": "#83a598"
  },
  "radii": {
    "radius_sm": 2,
    "radius_md": 4,
    "radius_lg": 6,
    "radius_xl": 8,
    "control_radius": 4,
    "sketch_mode_radius": 4
  }
}
```

Colours are `#rrggbb`, or `#rrggbbaa` for translucent ones. A few colours, such as the four
Continuity Analysis seams (`continuity`), take a list. Any colour the file leaves out keeps the
base palette's value. `axis_x`, `axis_y` and `axis_z` colour every X, Y and Z axis: the origin,
the view triad, the navigation cube and the Move gizmo. A file from an earlier version may set
`triad` or `gizmo` instead; it still reads, and those colours now go to every axis.

`radii` is optional. Its six keys above accept integers from 0 to 32 in logical UI points;
zero makes square corners. Omitted keys keep today's base values: 4, 6, 8, 10, 6 and 7,
respectively. The first four style small accents through large surfaces; `control_radius`
styles form controls, and `sketch_mode_radius` styles sketch creation buttons. Themes own
these settings, with matching shapes across light and dark variants. Dock joins remain
square, circular controls remain circular, and control sizes and spacing stay the same.
Platinum 9, 1-bit, 1-bit Dark, and the phosphor themes use square corners, SolidCraft 95 uses subtle 3-point
corners, Blueprint uses the standard radii, Cupertino uses softer corners, and Clay and Clay Dark use rounded corners. Other built-in
themes retain the defaults.

Tab accents also have separate fill, small-text ink and tint keys. Sketch uses
`sketch_accent`, `sketch_ink`, `sketch_soft`; Surface, Mesh, Sheet Metal and Render
use `<tab>_fill`, `<tab>_ink`, `<tab>_soft` (`sheet` for Sheet Metal). Form uses
`form_fill`, `form_tab_ink`, `form_soft` because `form_ink` names the general form
text. Model keeps `accent`, `selected_ink`, `accent_soft`. The older
`surface_accent`, `mesh_accent`, `form_accent` and `render_accent` keys retain their
existing roles and values. `tab_on_fill_dark` is the dark text fallback for tab
pills when `on_accent` has less than 4.5:1 contrast with the fill. See the
[per-tab token reference](../dev/ui-kit.md#per-tab-accent-tokens) for all pairs.

- **Import…** adds a theme file and turns it on. Importing a file with the same name again updates
  that theme. A file with an unknown colour name or a malformed colour is refused, and the message
  names every problem. A theme shared as text imports the same way without a file: with the
  Interface page open, press Ctrl+V (Cmd+V on a Mac), or right-click the Import tile and choose
  **Paste theme**. Pasted text that isn't a theme is left alone. A theme file dropped on the tile
  imports too.
- **Export…** saves the theme on now as a complete file listing every colour and radius. It is a good starting
  point for your own theme.
- **Remove** deletes the selected imported theme.

Retired window-band and procedural icon colours in older exports are discarded on import.
Command icons use the shared SVG palette.

Imported themes are kept with your preferences, so they work in the browser app too. Colour
vision modes still apply on top of any theme.

#### Theme-file options and overlays

Theme authors can add `options` and `overlays` to `solidcraft-theme/1`. Offered options appear
below the theme cards in Interface, with the theme's name. Switches, sliders, segmented choices
and dropdowns apply immediately and are saved per theme, including a followed theme file.
**Reset** appears when an offered value differs from its default and restores that theme's
defaults through the same preference update path. Cancel retains its existing behaviour of
restoring the Preferences window's opening snapshot. Themes with no offered options add no
options block or badge. Existing theme files need no changes or appearance changes.

Options flow left to right into two columns. A parent and its child occupy one cell; turning
the parent off disables its child without forgetting the child's value. Choices with four or
five values and long labels take a full row; choices with more than five values use a dropdown.
The first ten non-advanced top-level entries appear before **More · N**; advanced entries and
overflow always appear inside it. Section labels appear only above six top-level entries.
Help is available on hover. A registered behaviour unavailable on the current device stays
disabled in its place with a reason; unimplemented standard behaviours and their children
are absent. Theme cards show an Options badge only when the theme offers controls.

**Export…** preserves the authored definitions and overlays for a theme that declares them.
Older themes still export a complete token snapshot. The resolver API clamps numeric values;
invalid saved preference values instead fall back to the theme's defaults, retaining the raw
saved values for compatibility.

For example, add these two keys alongside a theme's `colors`:

```json
{
  "options": [
    {
      "id": "warm_accent",
      "type": "bool",
      "default": true,
      "label": {"en": "Warm accent"},
      "help": {"en": "Use a warmer accent colour."}
    },
    {
      "id": "rounding",
      "type": "range",
      "default": 4,
      "min": 0,
      "max": 12,
      "step": 1,
      "unit": "pt",
      "parent": "warm_accent"
    }
  ],
  "overlays": [
    {
      "when": {"opt": "warm_accent", "eq": true},
      "colors": {"accent": "#cc7744"}
    },
    {
      "when": {"opt": "rounding", "gte": 4, "lte": 12},
      "radii": {"control_radius": 8}
    }
  ]
}
```

A file can declare up to 16 options. Each `id` is unique and matches
`[a-z][a-z0-9_]{0,31}`. Every option has a `type` and a `default` (opacity may omit its
default to use 100):

- `bool`: the default is `true` or `false`.
- `choice`: `values` contains 2 to 12 entries, each with a unique `id` in the same syntax
  and a localized `label` map, for example `{"id":"warm","label":{"en":"Warm"}}`.
  The default names one of those ids.
- `range`: finite numeric `min`, `max` and positive `step` are required, with `min` no larger
  than `max` and the default inside those bounds. Optional `unit` has at most 4 characters.
  Numbers must fit a finite 32-bit float. Saved values are clamped to the bounds; `step`
  is an increment for future controls, not a rounding rule.

Optional `label` and `help` maps use language codes as keys, with `en` as the fallback.
Labels have at most 40 characters and help strings at most 160, per language. Each map
supports up to 64 language codes, each at most 32 ASCII letters, digits, hyphens or underscores.
These texts belong to the theme and need no application catalog entries. `advanced` is an
optional boolean; `section` is an optional string of at most 24 characters. `parent` names
a declared boolean option, with at most one child per parent and no cycles. While the
parent is off, overlays referring to the child are inactive; the child's value is retained.

Four ids are reserved for definitions owned by the app:

| Id | Type | Bounds | Unit | Parent |
|---|---|---|---|---|
| `opacity` | range | 40–100 | `%` | — |
| `blur` | range | 0–40 | `px` | — |
| `tab_colours` | bool | — | — | — |
| `tab_fade_ms` | range | 0–600 | `ms` | `tab_colours` |

A theme declares these with their required type and a default; it may also narrow a range's
`min` and `max`. The default is clamped to those bounds. Other fields are ignored, including
presentation text, `step` (fixed at 20 for `tab_fade_ms`, 1 for other standard ranges), unit and parent. A wrong type or
bounds outside the standard range are import errors. Declaring `tab_fade_ms` also requires
declaring its boolean parent, `tab_colours`. Custom options cannot redefine reserved ids.
`opacity`, `tab_colours` and `tab_fade_ms` have registered standard behaviours. `blur` remains
absent from the list until its behaviour is registered.
The resolver exposes declared standard options in the order `opacity`, `blur`, `tab_colours`,
each immediately followed by its child, then custom options in declared order.

Opacity scales only these floating surface fill tokens: `panel`, `panel_header`, `dialog_bg`,
`dialog_body`, `dialog_footer`, `overlay` and `nav_tip`. Scaling is applied after matching
overlays. All other tokens, including text, icons, borders, controls, viewport and document
colours, retain their resolved values. At 100%, with the implicit 100% default, or without
offered opacity, every token is exactly the original resolved value. The supported range is
40–100%; the theme may narrow it. To keep normal text at 4.5:1 against a panel over black and
white viewport extremes, each fill's actual alpha rises as necessary. This also protects
against background overrides and busy models. A theme whose authored foreground and opaque
fill already fail contrast receives full alpha; opacity never rewrites the theme's text colours.

Imported themes can also declare `tabs`, using exact ribbon registry names. Only `panel`,
`viewport_top` and `viewport_bottom` are accepted; an unknown tab or any other token is an
import error. Tabs without an entry keep the base surfaces. Existing tab accent fills, inks
and tints keep their own tokens. For example, save this original theme as a JSON file and import it:

```json
{
  "format": "solidcraft-theme/1",
  "name": "Garden tabs",
  "base": "dark",
  "options": [
    {"id": "tab_colours", "type": "bool", "default": true},
    {"id": "tab_fade_ms", "type": "range", "default": 240}
  ],
  "tabs": {
    "SKETCH": {"panel": "#123028", "viewport_top": "#103128", "viewport_bottom": "#204a3c"},
    "RENDER": {"panel": "#302039", "viewport_top": "#30203d", "viewport_bottom": "#493252"}
  }
}
```

The [imported light/dark fixtures](../../../../crates/ui-egui/tests/fixtures/theme-tabs.json)
also provide Model surfaces. No built-in theme declares tab surfaces. With `tab_colours`
off or absent, every tab keeps the base palette. With it on, `tab_fade_ms` blends panel and
viewport surfaces together in Oklab with an ease-out cubic curve; zero switches instantly.
A tab change during a fade starts from the colours currently displayed. Opacity readability
floors also apply to tab panels and interpolated alpha. A viewport set to the opposite light/dark
palette keeps that palette; a flat background preference takes precedence over tab gradients. Definitions and authored defaults
roundtrip through theme export; saved option values are excluded.

Render's untouched, unsaved preview also uses these animated surfaces, including viewport
palette and flat-colour preferences. Explicit scene backplates, transparency, environments,
gradient backdrops and saved setups keep their existing background instead. Saved setups do
not record whether their background was explicitly chosen, so even a setup saved only for its
camera conservatively retains its scene backplate. With tab surfaces absent/off, Render keeps
its original flat scene background. Theme surfaces never change rendered images or exports.

Gruvbox has two cards, **Gruvbox Dark** and **Gruvbox Light**. Each offers a
**Contrast** choice: **Soft**, **Standard** (the default), or **Hard**. These are the
same six palettes as the former separate cards, including their icon colours.
Saved selections and settings profiles using the former `-soft` and `-hard` ids
select the corresponding family and contrast. Selecting a family again restores its
last contrast; Reset returns it to Standard.

Up to 32 overlays apply in file order, after the base palette and the theme's own value tables.
Later matching entries win by key. Every overlay has one `when`: `{"opt":"id","eq":value}`
for an exact, correctly typed comparison, or `gte`, `lte`, or both for inclusive range bounds.
A choice comparison must name a declared choice. Unknown option ids, mixed comparisons and
malformed values are refused with the overlay's one-based position. An overlay may override
`colors`, `radii`, `borders` and `icon_palette`; all their existing key and value checks apply.
Overlays only select literal tables: there are no scripts, expressions or interpolation.

The optional integer `requires` defaults to 1, this app's supported level. A larger level
loads the authored base value tables, disables options and overlays, and exposes a
`needs_newer` flag through the theme API for future UI. The theme-definition API can export
options and overlays intact; the existing token snapshot export keeps its previous shape.
The value resolver reports notices for unknown saved ids, wrong types, unknown choices,
non-finite numbers and clamped values. Invalid saved values for declared options fall back to defaults;
unknown saved ids are omitted because they have no definition or default.

#### Follow a theme file

In the desktop app, **Follow a theme file** (under the themes) takes a theme file that another
tool writes and keeps it as the theme: whenever the file changes, SolidCraft reloads it within
about a second, and again when its window comes back to the front, without a restart. This suits
tools that generate a colour scheme from your wallpaper or keep one scheme across your desktop
and applications (pywal and matugen are two examples); have them write a `solidcraft-theme/1`
file from a template. **Choose…** picks the file, **Stop following** (or choosing any theme)
turns it off; it is off until you choose a file. Only a file that reads completely is applied:
while it is being written, or if it is broken, missing or larger than 256 KiB, the last good
theme stays on and the setting says why. `solidcraft --theme-file my-theme.json` follows a file
for one run without changing your preferences. The browser app has no file to follow; import the
theme there instead.

Graphics › **Frame rate** shows frames per second in the view's bottom-right corner. Click it,
or turn on **Frame rate details**, for the performance overlay:

- a graph of the last few seconds of frames, one bar per frame, with lines at 60 and 30 frames
  per second. The triangles drawn are a line over it, and a tick at the top marks each frame that
  rebuilt the scene, so you can see how geometry changes the frame time;
- a histogram of frame times, the 1% low (the mean of the slowest 1% of frames) and the worst
  frame, which show stutters that an average hides;
- what the GPU drew last frame (triangles, line segments, draw calls), the viewport's size in
  pixels with its MSAA and supersampling, the time the interface took, the last scene build and
  the graphics backend and adapter.

While the readout is on, the view redraws every frame. With vertical sync the frame rate tops
out at the display's refresh rate; the interface time and the GPU counts still show the work.

### Settings profiles

General › **Settings profiles** keeps the input and interface settings together under a name,
for example "Laptop" and "Large monitor", and switches them at once. A profile holds:

- the Interface, Navigation and Sketch pages: theme, interface scale and font, layout, mouse
  mode and the other navigation options, the radial menu and mouse gestures with every studio's
  command lists, and the sketch modifier keys;
- the interface language;
- the keyboard shortcuts, the toolbar and the S box favourites;
- the imported theme it uses, so it travels with an exported profile.

Settings that belong to the computer stay out: the 3D mouse (calibration, driver, speeds and
buttons), graphics, units and decimals, the AI page, recent files and window positions.

- **Active profile** lists the profiles; choosing one applies it. While its settings differ from
  what was saved, a note says so.
- **Save** keeps the current settings under the typed name, replacing a profile of that name,
  and makes it the active one.
- **Rename** and **Delete** act on the active profile.
- **Export…** saves the active profile as one `solidcraft-profile/1` file; **Import…** adds one
  without switching to it (a name already taken gets a number).

Switching, like any other change in Preferences, is taken back by Cancel. A setting a profile
holds but this computer refuses (an interface font that isn't installed) keeps its current value,
and the message says how many did. Import ignores unknown keys, leaves out values this version
can't read, and refuses files over 4 MiB. Scripts and agents reach the same actions with the
`settings.profile.*` commands (`save`, `apply`, `rename`, `delete`, `list`, `export`, `import`).

Navigation › **Smooth wheel zoom** is on by default. Turn it off to apply each wheel or
trackpad event immediately, with no smoothing tail. It is independent of **Animate view
changes**, which controls axis views, Look At, Home and Fit. Re-enabling smoothing during
a wheel gesture takes effect after that gesture ends, avoiding leftover zoom being replayed.

Navigation › **Alias** uses Alias's camera keys: Shift+Alt+left drag orbits, Shift+Alt+middle
drag pans and Shift+Alt+right drag zooms (drag down to move away, up to move closer). Without
Shift+Alt, a left drag still selects with a box and drags sketch geometry.

Some Linux desktops take Alt+drag for themselves (moving or resizing windows), so the viewport
never sees it. To free it:

- **KDE Plasma:** System Settings › Window Management › Window Behavior › Window Actions, set
  **Modifier key** to Meta (or None).
- **GNOME:** run `gsettings set org.gnome.desktop.wm.preferences mouse-button-modifier '<Super>'`
  (or set Tweaks › Windows › Window Action Key to Super).

Shift-orbit snapping includes 45-degree edge views and isometric corner views, with a
lightly damped spring transition and a small boundary margin to prevent flicker.

On desktop, Ctrl/Cmd+Plus (or Equals) enlarges the interface, Ctrl/Cmd+Minus reduces it,
and Ctrl/Cmd+0 restores 100%. These shortcuts update the UI scale preference. The UI scale
slider in Preferences applies the new layout when released, so it stays still while dragging.


Theme files can also define `borders`: `primary` outer outlines, `secondary` major section
rules, `tertiary` small dividers, `container` solid preview/card/keycap outlines, and independent
`control` outlines. Each accepts a `width`
from 0 to 4 UI points and a `color` in hex. Optional `primary_inset` adds a companion outline
inside panel chrome, with a `gap` from 0 to 12 points. Missing settings keep existing borders.
Blueprint uses a single white outer outline, dotted section lines and dark minor dividers. Secondary rules optionally accept `pattern: "dotted"`; the default is `solid`. Focus, selection and
error outlines retain their semantic colours. See the UI-kit border hierarchy for the schema.

SolidCraft 95 also defines optional `borders.bevel`: a per-band `width` and the
`highlight`, `light`, `shadow` and `dark_shadow` colours. The shared UI uses these
for raised panels/buttons, recessed fields and reversed pressed-button lighting.
Themes that omit this setting keep their existing flat borders.

Theme files may also set `icon_palette`: opaque colours for SVG display roles,
and three-colour pale/medium/strong ramps for Solid, Surface, Mesh, Form, Sketch,
constraints and construction. These settings colour the existing command artwork
without changing its shapes. Omitted roles keep the default light/dark palette.
