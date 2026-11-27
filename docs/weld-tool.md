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
The panel previews plain numeric sizes; the engine builds the supported symbol
from evaluated settings after resolving size expressions on save.
The stable type ids, in order, are `fillet`, `butt`, `edge`, `plug`, `spot`,
`seam`, `stud`, `surfacing`, and `tack`. Save only the chosen type's sizes:

| Type | Size keys |
| --- | --- |
| fillet | a or z; Advanced: z1, z2, s (deep throat) |
| butt | h, s (omit s for full penetration) |
| edge | w |
| seam | w; Advanced: l |
| plug | h (fill depth); Advanced: e |
| spot | d |
| stud | d; Advanced: e |
| surfacing | t; Advanced: w |
| tack | pitch: l, e; count: l, n; points: l |

At least one size is required. Default settings contain only fillet `a = 3`.
Values are length expressions, evaluated once and stored in millimetres.
Tack `n` is a unitless positive integer stored as a number (maximum 100000).
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

**Preferences → Design → Weld symbol standard** selects `iso2553_2019_a`
(default), `iso2553_2019_b` or `aws_a2_4_2020`. JIS, GB and GOST are listed
as unavailable. Each new graphic saves `symbol_standard`, so changing the
preference does not reinterpret existing graphics. The panel uses the same
convention: ISO A puts the identification line above for arrow side and below
for other side. All conventions keep the arrow leader. The symbol and shared
action bar remain pinned below the scrolling form. **New weld bead** is a framed
icon-and-label button at the left, using the existing weld bead icon; **Cancel**
and **OK** form the normal pair on the right. At narrow widths the left label
truncates without overlapping the pair. The button retains the `dialog:apply`
automation target and its tooltip; saving a bead keeps the tool open.

Butt `groove` is `square`, `v` (default), `bevel`, `u`, `j`, `y`, `hy`, `x`
or `k`. Only square and V have supported symbols; other grooves save a null
symbol and show a hint. `penetration` is `full` (default, omit `s`) or `partial`
(requires `s`). `root` is `as_welded` (default), `melt_through` or `backing_run`.
Root treatment and cap height are shown in the sketch; the geometry symbol
record cannot express them. `root_gap` is optional, non-negative millimetres;
`add_fillet` defaults to false. Those settings have no bead-solid effect yet.

Fillet equal-leg hints follow the plain numeric value: z = a × √2 or a = z / √2.
Unequal legs use their own `z1` and `z2` fields. Deep throat `s` maps to the
supported ISO deep-penetration callout. Unequal legs save a null symbol, with a
panel hint; added fillets remain sketch/settings data because the symbol record
cannot express them. AWS can
draw leg sizes; throat conversion and counted intermittent AWS symbols are not
supported by the geometry record. No automatic conversion is made.

Tack `tack_mode` is `pitch` (default), `count` or `points`. Its sketch and size
fields follow the mode. Geometry picks and tack bodies remain future work.
Advanced belongs to Size, directly below its sketch/fields. Clicking a sketch
dimension focuses its field and scrolls that field into view. The sketches are
schematic: dimensions change their drawn lengths, and repeated elements are
capped at six in the illustration. Intermittent spans keep their own sketch
inputs, separate from tack length, pitch and count. Spans and Chain / Staggered live together in Extent;
Ends stays in Advanced for every line type. The sketch letters `w` for edge/seam
width and `h` for plug fill depth are the owner's chosen provisional letters.

Still open: number/spacing annotations for spot welds and number of plug/stud
welds, surfacing layers, closed-path all-around detection, type-specific face
icons, revised looks/icons and floating-weld reference planes. `penetration_shown`
is a saved display flag; viewport penetration is a later job.

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
