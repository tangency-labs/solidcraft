# NX keyboard preset

Choose **NX** in **Preferences › Navigation › Mouse mapping**, then enable **Use NX keyboard shortcuts**.
Choosing the mouse preset alone does not change any keys. The keyboard option is off by default,
including when opening saved settings that already use NX mouse navigation.

The preset maps familiar keys to existing SolidCraft commands. It does not add NX commands or
provide every feature of NX. Custom bindings remain saved and take priority over preset mappings.
Disabling the keyboard option, or choosing another mouse preset, restores standard SolidCraft keys.
The option is part of saved settings profiles and Navigation's Defaults resets it to off.

## Supported keyboard actions

| Key | SolidCraft action |
|---|---|
| Ctrl+F | Fit the view |
| F8 | Look normal to a selected planar face or plane; without a suitable selection, align to the nearest principal view |
| Shift+F8 | Look normal to the selected planar face or plane |
| End | Isometric view |
| Ctrl+Alt+F, Ctrl+Alt+R, Ctrl+Alt+T | Front, right and top views |
| F4 | Repeat the last command |
| Ctrl+E | Parameters |
| Ctrl+T | Move the selection in the sketch or model |
| X | Extrude |
| Ctrl+Q | Finish the sketch |
| Ctrl+Shift+A | Save As |
| Ctrl+W | Toggle the selected objects' visibility |
| Ctrl+B | Hide the selected objects |
| Ctrl+Shift+K | Show the selected objects |
| Ctrl+Shift+U | Show all objects |
| Ctrl+D | Delete the selection |
| Ctrl+J | Assign appearance |
| Ctrl+Space | Command search |
| Ctrl+1 | Keyboard Shortcuts preferences |
| Ctrl+2 | General preferences |
| F7, Ctrl+R | Orbit navigation tool |
| F6, Ctrl+Shift+Z | Hold while dragging to zoom |
| Shift+B | Select bodies |
| Shift+G | Select faces |
| Shift+C | Select components |

Other SolidCraft shortcuts continue to work unless a preset key uses the same chord.
New, Open, Save, Undo, Redo, clipboard actions, Escape and Delete keep their usual actions.
Shortcuts do not trigger while entering text. Embedded editors leave file and settings shortcuts
to their host. A browser or operating system can intercept a key before SolidCraft receives it.

## Mouse actions

The existing NX mouse preset provides the following controls:

| Gesture | Action |
|---|---|
| Left click | Select |
| Shift or Ctrl with left click | Add to the selection |
| Middle drag | Orbit |
| Shift+middle drag | Pan |
| Ctrl+middle drag | Zoom |
| Wheel | Zoom |
| Middle double-click | Fit when NX is chosen; separately configurable |
| Right click | Context menu |

The keyboard option does not change these mouse settings. True middle+left zoom and middle+right
pan are tracked separately in [PR #1608](https://github.com/tangency-labs/solidcraft/pull/1608) and
[issue #4824](https://github.com/tangency-labs/solidcraft/issues/4824). They are not supplied by this
keyboard preset. NX-specific mouse actions for dialog confirmation, radial toolbars or copying
sketch objects are not implied by choosing NX navigation.

For a sketch's normal view, select its supporting plane or planar face. The preset does not
introduce a separate command to orient the camera from the active sketch alone.

## References and coverage

This is an independent mapping of public interaction conventions to SolidCraft's own commands.
No commercial CAD program is driven or used to produce command lists or reference data.

Siemens documents [F8 alignment, including a selected planar face](https://blogs.sw.siemens.com/designcenter/nx-timesaving-clicks-tricks-part-3/)
and [End for isometric view and Ctrl+Q to finish a sketch](https://blogs.sw.siemens.com/news/nx-hints-and-tips-rapid-sketches-with-keyboard-shortcuts/).
The required fit, visibility and Save As mappings are recorded in
[issue #15101](https://github.com/tangency-labs/solidcraft/issues/15101).
NX versions and custom roles can assign different keys. Only the mappings listed above are
provided; SolidCraft has no substitute for an NX feature it does not implement.
