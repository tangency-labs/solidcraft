# NX keyboard shortcuts

Select **NX** under **Preferences → Navigation**, and enable **Use NX keyboard shortcuts with the NX navigation profile**. Through the control channel, the equivalent is `ui.prefs {"nav":"nx","nx_shortcuts":true}`.

The profile maps familiar NX keys to existing SolidCraft actions. It does not reproduce every NX command. Turn off `nx_shortcuts` to keep the NX mouse controls with the standard SolidCraft keyboard defaults. Choosing another navigation profile also restores those defaults.

Custom command bindings take priority over profile defaults and remain saved when profiles change. Registered command bindings appear under **Preferences → Keyboard Shortcuts**, where they can be changed or cleared. The additional selection, visibility and settings actions are listed in that page's NX hint. Text fields retain their normal editing keys.

## Supported profile bindings

| Keys | SolidCraft action |
|---|---|
| Ctrl+Shift+A | Save As |
| Ctrl+D | Delete the current selection |
| Ctrl+E | Change Parameters |
| Ctrl+J | Appearance |
| Ctrl+T | Move/Copy bodies, or Move/Copy Sketch Entities while editing a sketch |
| X | Extrude in the model; the existing Construction shortcut remains available in sketches |
| Ctrl+Q | Finish Sketch |
| F4 | Repeat Last Command |
| Ctrl+F | Fit |
| End | Isometric View |
| Ctrl+Alt+F / Ctrl+Alt+T / Ctrl+Alt+R | Front / Top / Right view |
| F8 | Align to a selected planar face or plane; otherwise snap to the nearest principal view |
| Shift+F8 | Normal to Selection, using SolidCraft's existing selection/sketch rules |
| Ctrl+Shift+Z or F6 | Hold the chord and drag with the left mouse button to zoom |
| Ctrl+R or F7 | Activate the Orbit tool |
| Ctrl+B | Hide selected objects |
| Ctrl+Shift+K | Show selected objects |
| Ctrl+Shift+U | Show All |
| Ctrl+W | Toggle visibility of the selection; a picked browser folder takes precedence |
| Shift+B | Select bodies |
| Shift+G | Select faces |
| Shift+C | Select components |
| Ctrl+Space | Open command search |
| Ctrl+1 | Open Keyboard Shortcuts preferences |
| Ctrl+2 | Open General preferences |

The existing New, Open, Save, Undo, Redo and clipboard keys remain available: Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Z, Ctrl+Y, Ctrl+X, Ctrl+C and Ctrl+V. Ctrl+Shift+S also remains a Save As shortcut. On macOS, SolidCraft's primary-modifier convention displays these Ctrl bindings as Cmd.

In this profile, **Ctrl+W does not close the design**. It toggles selected objects directly; it does not open NX's Show and Hide panel. Use the document tab's close button or context menu to close a design. F6 becomes held zoom instead of Fit; use Ctrl+F for Fit.

## Differences and unsupported actions

The profile does not implement Home for a trimetric view, Ctrl+Shift+B for swapping shown and hidden objects, or general Ctrl+A selection. NX edge and feature selection filters, layer management, application/workspace switching, and other NX-specific commands are also outside this mapping. An unmapped key may retain its existing SolidCraft action; it is not a promise of NX compatibility.

## Public references

- [learnNX shortcut reference](https://learnnx.com/wp-content/uploads/2024/11/learnNX-shortcuts.pdf): common editing, visibility and settings conventions.
- [SDC Publications, Parametric Modeling with Siemens NX, sample chapter](https://static.sdcpublications.com/pdfsample/978-1-63057-280-8-2.pdf): Finish Sketch uses Ctrl+Q.
- [Siemens, NX Timesaving Clicks and Tricks, Part Three](https://blogs.sw.siemens.com/designcenter/nx-timesaving-clicks-tricks-part-3/): distinctions between Home, End and F8 view shortcuts.

These references describe keyboard conventions. The implementation uses SolidCraft commands and does not include NX code, artwork or help text.
