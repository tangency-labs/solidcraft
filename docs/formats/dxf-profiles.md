# DXF profiles

A DXF profile is a small file that says how a flat pattern's DXF is laid out for one cutter:
which layer each kind of line goes on, in which colour and linetype, and which kinds are left
out. A fabrication shop writes one for its laser, punch or waterjet and keeps and shares it like
a [rule pack](sheet-rule-packs.md). `sheet.export_dxf` takes it as `profile`: the path of a
profile file, or the profile itself. Profiles are read only from local files you name;
SolidCraft never fetches one.

Without a profile the DXF has SolidCraft's standard layers. The neutral profile below is those
layers written out, and exporting with it gives the same file, byte for byte, as exporting
without a profile.

## JSON (version 1)

The neutral profile, every role in full:

```json
{
  "format": "solidcraft-dxf-profile",
  "version": 1,
  "name": "Neutral",
  "roles": {
    "outline":     {"on": true, "layer": "OUTLINE",     "color": 7, "linetype": "continuous"},
    "cut":         {"on": true, "layer": "CUT",         "color": 4, "linetype": "continuous"},
    "cut_text":    {"on": true, "layer": "TEXT",        "color": 2, "linetype": "continuous"},
    "etch":        {"on": true, "layer": "ETCH",        "color": 6, "linetype": "continuous"},
    "bend_up":     {"on": true, "layer": "BEND_UP",     "color": 1, "linetype": "dashed"},
    "bend_down":   {"on": true, "layer": "BEND_DOWN",   "color": 5, "linetype": "dashed"},
    "bend_notes":  {"on": true, "layer": "BEND_NOTES",  "color": 8, "linetype": "continuous"},
    "pockets":     {"on": true, "layer": "POCKET",      "color": 3, "linetype": "continuous"},
    "perforation": {"on": true, "layer": "PERFORATION", "color": 3, "linetype": "continuous"},
    "roll":        {"on": true, "layer": "ROLL",        "color": 3, "linetype": "continuous"}
  }
}
```

A shop's profile gives only what it changes. This one cuts the outline and the cut-outs from one
layer, draws both bend directions on one layer with short dashes, and leaves out everything its
cutter would take for a contour:

```json
{
  "format": "solidcraft-dxf-profile",
  "version": 1,
  "name": "Example laser",
  "note": "Both bend directions on FOLD; no text, notes or roll lines.",
  "roles": {
    "outline":    {"layer": "CUT", "color": 7},
    "cut":        {"layer": "CUT", "color": 7},
    "cut_text":   {"on": false},
    "etch":       {"layer": "MARK", "color": 2},
    "bend_up":    {"layer": "FOLD", "color": 3, "linetype": "hidden"},
    "bend_down":  {"layer": "FOLD", "color": 3, "linetype": "hidden"},
    "bend_notes": {"on": false},
    "roll":       {"on": false}
  }
}
```

- `format` must be `solidcraft-dxf-profile`. `version` is a whole number; left out, it is 1.
- `name` and `note` describe the profile.
- `roles` holds the roles the profile changes. A role left out, and a value left out of a role,
  keep the neutral profile's.

## Roles

| Role | What it holds |
|---|---|
| `outline` | the outer contour of each piece, and the cuts with no width along split bends |
| `cut` | what is cut out inside the outline, and the loose pieces a cut leaves |
| `cut_text` | letters cut through the sheet (the export's `text: cut`) |
| `etch` | letters marked on the sheet (`text: etch`), and text machined to a depth |
| `bend_up`, `bend_down` | the bend centre lines, by direction |
| `bend_notes` | the note beside each bend line (the export's `bend_notes`) |
| `pockets` | the boundaries of pockets, one layer per side and depth |
| `perforation` | the holes of a perforation |
| `roll` | where a roll starts and ends, and its middle |

Each role has:

- `on`: `false` leaves the role's entities and its layer out of the file. Default `true`.
- `layer`: the layer's name, 1 to 200 characters: letters, digits, spaces inside the name and
  marks such as `-`, `_`, `.`, `#`, `$`, `(`, `)`; not `< > / \ " ' : ; ? * | = , `` ` `` and not
  layer `0`. Names are plain ASCII for now.
- `color`: an AutoCAD colour index, a whole number from 1 to 255 (1 red, 2 yellow, 3 green,
  4 cyan, 5 blue, 6 magenta, 7 white or black, 8 grey).
- `linetype`: `continuous`, `dashed` (12.7 mm dashes, 6.35 mm gaps) or `hidden` (2 mm dashes,
  1 mm gaps). The file defines the linetypes its layers use.

Roles may share a layer, as the outline and the cut-outs do above; they must then give it the
same colour and linetype. Layer names ignore case, as DXF does.

The `pockets` role's `layer` replaces the word `POCKET` at the start of each pocket layer's
name; the side and depth stay (`MILL_TOP_Z0.000000_TO_1.000000_MM` for `"layer": "MILL"`),
because the DXF records a pocket's depth nowhere else.

## With the export's other options

- A role is written when both the export's options and the profile allow it. `bend_lines: false`
  leaves the bend and roll lines out whatever the profile says, and `bend_notes: true` writes
  notes only when the profile has `bend_notes` on.
- `text: cut` or `etch` chooses which role the letters cut through the sheet take; the profile
  then styles that role or leaves it out. With `cut_text` off those letters are not in the file,
  so the cutter does not cut them.
- `layers: single` puts every entity on layer 0 and cannot be combined with a profile.
- `curves`, `note_height` and `per_body` work as without a profile. The entities and their order
  are the same; only their layers change.

## What is refused

A profile is checked before anything is written, and a refused profile writes no file. It is
refused, with a message naming the role and the value, when it is over 1 MiB, is not UTF-8 or
not valid JSON, has another `format`, names a role or a field this version does not know, has
an `on` that is not `true` or `false`, a colour that is not a whole number from 1 to 255, a
linetype other than the three, a layer name DXF cannot hold, or two roles that share a layer
but disagree on its colour or linetype.

## Newer profiles

A profile of a later version, or with fields or roles this version does not know, is refused
rather than read in part: a cutting file that silently ignored a line of its profile could send
the wrong lines to the cutter.
