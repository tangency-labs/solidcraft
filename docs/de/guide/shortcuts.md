# Keyboard shortcuts

Command shortcuts can be changed in File › Keyboard Shortcuts; these are the defaults.

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
| Ctrl+Z | Undo | Ctrl+Y | Redo |
| Ctrl+N | New Design | Ctrl+O | Open |
| Ctrl+S | Save | Delete | Delete the selection |

Keys handled by the window itself:

| Key | Action |
|---|---|
| S | the S box: search every command at the cursor |
| V | show or hide the selected bodies, sketches and planes |
| F2 | rename the selected browser or timeline item |
| F6 | fit the view to the model |
| ← → ↑ ↓ | turn the view 15° (with Shift, a quarter turn) |
| Ctrl+← → ↑ ↓ | pan the view |
| Ctrl+8 | look straight at the selected face, plane or sketch |
| Esc | cancel the running command, or clear the selection |

While a value box has the keyboard (a dimension being added, a dialog field), keys type into
it and single-letter shortcuts don't fire. With Sketch Dimension (D), the box opens as soon as
you click a line, circle or arc (or the second point or line), so D, click, type the value,
Enter.

Mouse and touch:

| Action | Result |
|---|---|
| Click | select (Shift or Ctrl adds) |
| Click again on the same spot | select the next thing under the cursor (click cycling) |
| Hold the left button still (long-press on touch), or right-click › Select Other… | list everything under the cursor to pick from |
| Double-click a sketch curve or edge | select its chain or tangent loop |
| Double-click the middle button | go Home: the home view, framing everything shown (Preferences › Navigation › Middle double-click: Home, Fit or Off) |

Help › Keyboard Shortcuts in the app lists the current bindings, including any you changed.

With the **Touchpad** navigation preset, hold **Shift** while moving the pointer to pan, or
**Alt/Option** to orbit, without pressing a mouse button. Vertical scrolling zooms. These
viewport gestures are separate from rebindable command shortcuts; mouse-button selection
and editing keep their existing modifiers.

### Sketch modifiers

While drawing or dragging a sketch point:

| Keys | Action |
|---|---|
| Ctrl/Cmd or Alt/Option (default) | suppress auto-snap for that gesture |
| Shift | prefer a line midpoint when targeting |

Change the snap-suppress chord in Preferences › Sketch. Selection still uses Shift and
Ctrl/Cmd to add or toggle (Preferences › Selection can make Shift add-only).
