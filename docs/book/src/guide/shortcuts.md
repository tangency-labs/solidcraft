# Keyboard shortcuts

Command shortcuts can be changed in File › Keyboard Shortcuts; these are the defaults. Click a
key to replace it, or the **+** after it to give the command another key: every key it has
runs it, and menus and tooltips show the first one.

Click a binding to record a new shortcut. A shortcut can contain Shift, Ctrl/Cmd and
Alt/Option modifiers plus exactly one other key. Hold the modifiers, then press the final
key; you can take your time assembling the modifiers before that key. The recorder also
allows modifiers to arrive after the ordinary key: it previews the chord and waits one
second after the last added modifier, and until all keys are released, before confirming.
For example, record Shift+Cmd+M for Sketch Mirror on macOS. Escape cancels recording without
changing the old binding. Pressing two different ordinary keys cancels the attempt.

The binding button shows modifiers immediately as you press them, even before adding the
ordinary key. Releasing a modifier before that key removes it from the preview. A status
line tells you to add a key, release all keys, or wait for confirmation; the proposed chord
stays visible during confirmation. Nothing is saved while the preview is still pending.

On macOS, the editor, Help list, toolbar hints, context menus and S search display Cmd and
Option. Existing settings retain SolidCraft's Ctrl/Cmd alias for the primary modifier;
Control and Command pressed together are recorded explicitly as Ctrl+Cmd. Conflicting
bindings still require confirmation, and built-in reserved shortcuts remain reserved.
Recording changes app settings, not the design file.

This follows the usual [modifier-before-key convention documented by Apple](https://support.apple.com/en-ie/102650)
and the [key-event and modifier-state model documented by W3C](https://www.w3.org/TR/uievents/#keys-modifiers).
The one-second grace period is SolidCraft's recording aid, not a standard-mandated delay;
using an already assigned shortcut has no added delay. Operating systems and browsers can
intercept reserved combinations before SolidCraft receives them. Choose a different chord
if that happens; recording is cancelled if the app loses focus.

| Key | Command | Key | Command |
|---|---|---|---|
| L | Line | E | Extrude |
| R | Two-Point Rectangle | Q | Press Pull |
| C | Center Diameter Circle | F | Fillet |
| D | Sketch Dimension | H | Hole |
| X | Construction | M | Move/Copy |
| O | Offset | J | Joint |
| T | Trim | I | Measure |
| P | Project | A | Appearance |
| Ctrl+Z | Undo | Ctrl+Y, Ctrl+Shift+Z | Redo |
| Ctrl+N | New Design | Ctrl+O | Open |
| Ctrl+S | Save | Delete | Delete the selection |
| Ctrl+Shift+S | Save As | Ctrl+A | Select All |
| Ctrl+1 | Front View | Ctrl+2 | Back View |
| Ctrl+3 | Left View | Ctrl+4 | Right View |
| Ctrl+5 | Top View | Ctrl+6 | Bottom View |
| Ctrl+7 | Isometric View | Home | Home View |
| End | Fit: frame the selection, or everything shown; with a drawing open, the sheet | | |
| Ctrl+Alt+O | Open View in New Window | Ctrl+Alt+W | Move View to Window (next window) |

L, R and C start their tool family (lines, rectangles, circles) in the variant you used last, as
the family's toolbar button does: after a Midpoint Line, L starts a Midpoint Line again. Pick
another variant from the button's drop-down (or the Sketch Palette) and the key starts that one
from then on. The choice is kept across designs and restarts. With **Pressing a tool's shortcut
again switches its mode** on (Preferences › Sketch), pressing the key again before the first
click steps to the family's next variant.

One key can be bound to a sketch tool and to a model command at once: in a sketch the key runs
the sketch tool, outside one the model command. Outside a sketch, a sketch tool's key opens
Create Sketch and starts the tool in the new sketch. By default a model command's key pressed
in a sketch still runs it, and one that builds a feature (E, Extrude) finishes the sketch first.
With **Keep model keys out of sketches** on (File › Keyboard Shortcuts), a sketch and the model
keep their keys apart: while editing a sketch only sketch keys and keys that work everywhere
(Measure, the views, the file commands) run, and a model key such as E or F does nothing, so a
stray key can't close a heavy sketch.

Keys handled by the window itself:

| Key | Action |
|---|---|
| S | command search under the ribbon search field |
| V | show or hide the selected bodies, sketches and planes; with a right-click menu open, its Show/Hide (a folder, the Origin folder, an origin plane) |
| F2 | rename the selected browser or timeline item; with several bodies selected, all of them: the first takes the name, the others `name (2)`, `name (3)`…, as **Rename Selected Bodies** does in a selected body's menu |
| F6 | fit the view to the selection, or to everything shown when nothing is selected; with a drawing open, fit the sheet (as End does) |
| ← → ↑ ↓ | turn the view 15° (with Shift, a quarter turn); while the Browser has the keyboard (after a click on one of its rows, until a click elsewhere), ↑ ↓ move the selection through its rows and ← → fold or unfold a row, and the view stays still (Preferences › Navigation › *Arrow keys move through the Browser while it has focus*) |
| Ctrl+← → ↑ ↓ | pan the view |
| PageUp / PageDown | zoom in / out |
| + / − | zoom in / out |
| Ctrl+8 | look straight at the selected face, plane or sketch |
| Esc | cancel the running command, or clear the selection |

While a value box has the keyboard (a dimension being added, a dialog field), keys type into
it and single-letter shortcuts don't fire; Home and End move the caret. A focused list (a panel menu)
keeps Home and End for its first and last row. With Sketch Dimension (D), the box opens as soon as
you click a line, circle or arc (or the second point or line), so D, click, type the value,
Enter. With Preferences › Design › Step values with the arrow keys and the wheel on, ↑ ↓ in a
value box, or the wheel over it, step a plain number by 1 (with Shift by 10, with Alt by 0.1);
see [Parameters and expressions](parameters.md).

In an Extrude that is being made, Shift+Enter applies it as a new body, whatever it would have joined
or cut; Enter alone applies it as the dialog shows.

Mouse and touch:

| Action | Result |
|---|---|
| Click | select (Shift or Ctrl adds) |
| Click again on the same spot | select the next thing under the cursor (click cycling) |
| Hold the left button still (long-press on touch), or right-click › Select Other… | list everything under the cursor to pick from |
| Double-click a sketch curve or edge | select its chain or tangent loop |
| Right-click in the view with a dialog open | go on to the dialog's next empty selection box; once every box has something, OK. An empty box the feature needs stays, and the status line says what to pick (off by default; turn it on in Preferences › General › Dialogs › Right-click confirms dialog inputs) |
| Double-click the middle button | go Home: the home view, framing everything shown (Preferences › Navigation › Middle double-click: Home, Fit or Off) |

To zoom by dragging, assign a key to **Drag to Zoom** in File › Keyboard Shortcuts.
Hold the shortcut, then drag up or down with the left mouse button in the viewport.
Releasing the shortcut stops the zoom. The Zoom button in the navigation bar also
shows the assigned shortcut.

Help › Keyboard Shortcuts in the app lists the current bindings, including any you changed.

### Keyboard profiles

If you already know another CAD tool's keys, pick that tool in **Keyboard profile** at the top of
File › Keyboard Shortcuts. A profile adds the other tool's keys to SolidCraft's own commands. A
command keeps its SolidCraft key too, unless the profile gives that key to another command. Keys
with no SolidCraft command are left out, and a profile never adds commands.

Your own keys always win. A key you change is a change from the profile: it stays when you choose
another profile or go back to **SolidCraft (default)**, and **Reset** on its row goes back to the
profile's key. Under each command's name the list says **From** and the profile's name, or
**Changed by you**. When the profile would take a key SolidCraft keeps for itself (Save, Open,
Close, Preferences, Undo and the other keys in the window table above), or a key one of your own
keys already uses, it leaves that key out and says so under the drop-down. Export and Import save
and read the chosen profile with your keys. Choosing a mouse preset in Preferences › Navigation
doesn't change the keyboard profile.

SolidCraft's own keys already match Fusion's for most tools (E, H, Q, F, M, V, S, A, J, L, R, C,
T, O, I, P, X and D). Each profile adds:

| Profile | Key | SolidCraft command | Differences |
|---|---|---|---|
| Fusion | Shift+N | Component Colors | |
| Fusion | Ctrl+Alt+B | Browser Tree (show or hide) | |
| Fusion | Ctrl+Alt+N | Navigation Bar (show or hide) | |
| Inventor | S | Create Sketch | Search Commands loses S |
| Inventor | R | Revolve | in a sketch, R still draws a rectangle |
| Inventor | F3, F4 | Zoom Mode, Orbit Mode | |
| Inventor | F6 | Isometric View | not used: F6 fits the view in SolidCraft; Ctrl+7 is Isometric View |
| Inventor | ], /, . | Plane, Axis, Point | |
| Inventor | F2 | Pan Mode | not used: F2 renames in SolidCraft |
| NX | Ctrl+F | Fit | Fit loses End |
| NX | End | Isometric View | |
| NX | F8 | Normal to Selection | with nothing selected, F8 faces the open sketch; it doesn't turn to the nearest standard view |
| NX | Z, A | Line, 3-Point Arc (in a sketch) | |
| NX | Ctrl+Q | Finish Sketch | |
| NX | R | Revolve | in a sketch, R still draws a rectangle |
| NX | Ctrl+Shift+A | Save As | Toggle Assistant loses Ctrl+Shift+A |
| NX | Ctrl+W | Show/Hide Selection | not used: Ctrl+W closes the design in SolidCraft; V still shows and hides |
| Onshape | Shift+E, Shift+F, Shift+W | Extrude, Fillet, Revolve | |
| Onshape | Shift+S | Create Sketch; in a sketch, Point | |
| Onshape | F | Fit | Fillet loses F |
| Onshape | N | Normal to Selection | |
| Onshape | Shift+1 to Shift+7 | Front, Back, Left, Right, Top, Bottom and Isometric View | Ctrl+1 to Ctrl+7 still work |
| Onshape | Y | Show/Hide Selection | V is Vertical in a sketch, so Show/Hide loses V |
| Onshape | P | Show/Hide Planes | Project moves to U |
| Onshape | [ | Measure | I is Coincident in a sketch; Measure keeps I in the model |
| Onshape | Shift+I | Isolate | SolidCraft hides the other parts; it doesn't make them see-through |
| Onshape | Shift+X | Section Analysis | |
| Onshape | L, A, R, G | Line, 3-Point Arc, Center Rectangle, 2-Point Rectangle (in a sketch) | Transform loses G |
| Onshape | D, O, U, X, M, Q | Sketch Dimension, Offset, Project, Extend, Trim, Normal/Construction (in a sketch) | Move/Copy Sketch Entities loses M |
| Onshape | I, E, H, V, T, B | Coincident, Equal, Horizontal, Vertical, Tangent, Parallel (in a sketch) | |
| Onshape | Shift+O, Shift+U, Shift+J, Shift+M, Shift+L, Shift+Q | Concentric, Curvature, Fix/Unfix, MidPoint, Perpendicular, Symmetry (in a sketch) | |
| Onshape | Shift+F | Fillet (in a sketch) | |
| SolidWorks | F | Fit | Fillet loses F |

Ctrl+1 to Ctrl+7 and Ctrl+8 already match SolidWorks' standard views and Normal To in every
profile. Inventor's command aliases of two or more letters (such as CH for Chamfer) aren't
supported, because a SolidCraft shortcut is a single key. Each profile is one file in
`crates/ui-egui/keyboard-profiles/`, so a profile can be added or corrected without changing any
code.

With the **Touchpad** navigation preset, hold **Shift** while moving the pointer to pan, or
**Alt/Option** to orbit, without pressing a mouse button. Vertical scrolling zooms. These
viewport gestures are separate from rebindable command shortcuts; mouse-button selection
and editing keep their existing modifiers.

### Sketch modifiers

While drawing or dragging a sketch point:

| Keys | Action |
|---|---|
| Ctrl/Cmd or Alt/Option (default) | suppress auto-snap for that gesture |
| Shift | prefer a line midpoint when targeting (skips a curve-curve crossing) |
| Shift, while drawing a Two-Point or Center Rectangle | make the rectangle a square: the second corner moves to the larger of the two distances, and the sketch gets an Equal constraint between two sides, so one dimension defines it; off by default, turn it on in Preferences › Sketch |

Change the snap-suppress chord in Preferences › Sketch. Selection still uses Shift and
Ctrl/Cmd to add or toggle (Preferences › Selection can make Shift add-only).
