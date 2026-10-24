# A tour of the window

![The SolidCraft window](../generated/sample-plate.png)

- **Title bar.** On Windows and Linux the application bar is the window's title bar. On the left:
  the File menu (New, Open, Save, Save As, Export…), undo and redo, the Home tab (the start page)
  and one tab per open design (each with its own undo history, selection and camera; a dot marks
  unsaved changes). On the right: Report a bug, Help (?), Preferences (which hold the theme) and the
  window's buttons.
- **Toolbar.** The workspace switcher (DESIGN) sits on the left. The tabs (MODEL, SURFACE,
  ASSEMBLE, INSERT, INSPECT, MANAGE, VIEW and SHEET METAL) hold sections of commands. SKETCH appears while
  editing a sketch; SHEET METAL is always there and is highlighted for the active sheet body. Both select automatically
  when entered, and the other tabs remain available. Click a
  panel's name for all its commands. Drag a command from the dropdown onto a panel's label
  or empty space to append it; the insertion line shows its destination. Drop onto an existing
  button to insert before it. Buttons can also move between panels, and the layout is saved.
- **Browser** (left). The design as a tree: document settings and units, named views, the
  origin, bodies, sketches, construction planes, canvases, joints and components. The eye shows or
  hides an item, a component's radio button makes it the active component, and right-clicking any
  row gives that item's menu. Rows drag into groups and other components.
- **Viewport.** Click to select, Shift or Ctrl to add (Preferences › Selection), drag for a window or crossing box. While sketching, Ctrl/Cmd or Alt/Option suppress auto-snap by default; Shift prefers a midpoint. To
  reach something under something else (two lines drawn on top of each other, coincident points,
  an edge in front of a face, overlapping bodies), hold the left button still for half a second
  (long-press on a touch screen) or right-click › **Select Other…**: a list of everything under
  the cursor opens there, the sketch being edited first. Hovering a row lights it up; clicking it
  selects it (Shift or Ctrl adds), and in a command's dialog it goes to the active selection box.
  Esc or a click elsewhere closes the list. Clicking again on the same spot, with nothing changed
  in between, selects the next thing in that list. The
  wheel zooms at the cursor, the middle button pans and right-drag orbits freely (the mouse
  preset in Preferences switches to SolidWorks, Inventor, Alias, Blender, Tinkercad, OpenSCAD or Touchpad style). On a
  trackpad, pinch zooms and two-finger scroll pans (Navigation › Two-finger scroll pans;
  off: scroll zooms like the wheel); mouse wheels zoom and the thumb wheel pans sideways.
  With the **Fusion** preset and two-finger pan enabled, hold **Shift** while scrolling with
  two fingers to orbit freely instead of panning. No click is needed. Releasing Shift returns
  to panning, even during the same gesture. Shift does not snap this gesture to standard views;
  orbit gestures do nothing on flat 2D pages. Other presets keep their existing scroll behaviour.
  Shift+middle orbits freely
  in the default and Inventor presets. In every preset, Ctrl+left drag (Cmd on macOS) that starts
  on empty space orbits; a Ctrl+click still adds to the selection. Shift+right-drag snaps to the nearest standard or
  diagonal view; Shift does not request snapping when the preset already requires it to orbit.

  Enable **Preferences → Navigation → Free orbit** to rotate the mouse and 3D mouse continuously
  through both poles, preserving roll without axis snapping. It defaults to off for every mouse
  preset. With SolidWorks navigation, right-click a planar face and choose **Normal to Selection**
  to use Look At (also Ctrl+8); repeat to flip the side, preserving scale and projection.

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
  Right-click for the marking menu: eight commands around the cursor, plus a
  menu for whatever is under it. Near the window's bottom or right edge a menu opens above or
  to the left of the cursor, so it stays in view.
  Shaded views omit certified smooth joins between spline spans on extruded
  walls. Real creases, cap outlines and authored face splits remain visible.
  Wireframe retains its edge presentation, and hiding a join does not remove its CAD
  identity or change the exported solid. Unsupported or uncertain wall geometry
  keeps its existing edge presentation.
- **Dialogs** dock on the right with a live preview. Most features also show drag handles and a
  value box in the viewport.
- **Timeline** (bottom). Every feature in order. Double-click to edit, drag the marker to roll
  back, and drag features to reorder them. Preferences can put the timeline vertically at the
  far left of the viewport, beside the browser. Its seam handle switches between icon
  chips and compact feature names; the browser can collapse to a rail beside either width.
  Previous, Play and Next sit at the top. Double-click Previous to roll to the start.
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

### Colour themes

General › **Theme** lists Dark, Light and System (follows the operating system), the built-in
Gruvbox themes (dark and light, each in hard, normal and soft contrast) and any themes you have
imported. A choice applies at once.

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
    "triad": ["#fb4934", "#b8bb26", "#83a598"]
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

Colours are `#rrggbb`, or `#rrggbbaa` for translucent ones. A few colours, such as the view
triad's X, Y and Z, take a list. Any colour the file leaves out keeps the base palette's value.

`radii` is optional. Its six keys above accept integers from 0 to 32 in logical UI points;
zero makes square corners. Omitted keys keep today's base values: 4, 6, 8, 10, 6 and 7,
respectively. The first four style small accents through large surfaces; `control_radius`
styles form controls, and `sketch_mode_radius` styles sketch creation buttons. Themes own
these settings, with matching shapes across light and dark variants. Dock joins remain
square, circular controls remain circular, and control sizes and spacing stay the same.
Platinum 9 and the phosphor themes use square corners, SolidCraft 95 uses subtle 3-point
corners, Blueprint uses the standard radii, and Cupertino uses softer corners. Other built-in
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
  names every problem.
- **Export…** saves the theme on now as a complete file listing every colour and radius. It is a good starting
  point for your own theme.
- **Remove** deletes the selected imported theme.

Retired window-band and procedural icon colours in older exports are discarded on import.
Command icons use the shared SVG palette.

Imported themes are kept with your preferences, so they work in the browser app too. Colour
vision modes still apply on top of any theme.

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
