# Weld graphics

Open **WELDMENTS → CREATE → Weld**, next to Frame Member. The floating panel edits
local settings. **OK** saves one graphic and closes the panel; **New weld bead**
saves it and keeps the settings for the next number. **Cancel** or Esc discards
the current form. Undo removes the last saved graphic.

Weld graphics are saved separately from timeline features and bodies. They do
not create bead solids or change mass properties. The panel builds the symbol from the same settings for fillet and butt welds.
Other types save a null symbol until #14654 supports more symbol kinds.

The first app integration leaves the panel's Where chips local: their strings
are display labels, without stable selection identities. Picking and Define
symbol report that they are not connected. They do not claim to attach a weld.
Creation with no picks is supported. The symbol editor and viewport attachment
are follow-up work; this adapter does not change the panel API.

Programmatic creation runs directly:

```json
{"weld_type":"fillet","size":{"a":"3 mm"},"face":"convex","field_weld":true}
```

Pass that object to `weld.create`. It returns `{"id":42,"number":"W1"}`
(the identity varies). `weld.delete` takes `{"id":42}`; both commands are
undoable. They never open the form.

The input and output schemas are published through `engine.describe`.
The stable type ids, in order, are `fillet`, `butt`, `edge`, `plug`, `spot`,
`seam`, `stud`, `surfacing`, and `tack`. Save only the chosen type's sizes:

| Type | Size keys |
| --- | --- |
| fillet | a, z, z1 |
| butt | h, s (omit s for full penetration) |
| edge, seam | w |
| plug | fill |
| spot, stud | d |
| surfacing | t |
| tack | l, e |

At least one size is required. Default settings contain only fillet `a = 3`.
Values are length expressions, evaluated once and stored in millimetres.
Sizes must be within the annotation record's 0.000001–100000 mm bounds.

`picks` groups are `line`, `groove`, `point`, `hole`, `ring`, and
`area`. Each entry is either `{"point":[x,y,z]}`, or
`{"edge":[x,y,z],"body":"Body1","index":0}` (use `face` for a face).
Coordinates also accept `{"x":0,"y":0,"z":0}`. A bare
`{"body":"Body1","index":0}` pair means a face in
groove/area groups and an edge in the other groups. References must
identify existing exact topology; these are saved references, with no automatic
reattachment after topology changes in this first step. The `symbol` is a validated `geom::weld::Weld`, built by the panel on every save.
Butt keeps face, side and process even when penetration `s` is empty.

`process` is optional ISO 4063 text (1–3 ASCII digits, e.g. `"135"`), always used
as the supported symbol's tail. The panel filters processes by type and derives
display texture from the number; texture is not saved. Process defaults are
135, 135, 141, 135, 21, 22, 78, 135, 135 in the type order above.

Choices are saved as names: `side` is `arrow` or `other`; `ends` is `square`,
`tapered` or `rounded`; `spot_look` is `marks`, `ball`, `marks_ball`, `marks_pin`,
`pin` or `dent`; `reference_plane` is `sketch`; `face` is `flat`, `convex` or
`concave`. Butt and plug open flat; the others open convex.
`field_weld` and `all_around` remain saved settings for the future symbol editor
and closed-path detection; they have no panel controls. Tack is a type.

`density` (kg/m³) and `overlap` (mm) are optional and empty by default. Empty
density means no mass; overlap follows h until entered. Optional `spans` contains
`count` (integer 1–10000), `length_mm` and `pitch_mm` (positive finite dimensions,
accepting length expressions). Pitch is centre to centre. Supported chain fillets
write spans into the symbol; single spans, staggered spans, butt spans and spans
outside the geometry annotation bounds remain saved without symbol intermittency.
Use the Pattern tool to repeat spot welds.
