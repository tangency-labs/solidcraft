# Weld graphics

Open **WELDMENTS → CREATE → Weld**, next to Frame Member. The floating panel edits
local settings. **Done** saves one graphic and closes the panel; **New weld bead**
saves it and keeps the settings for the next number. **Cancel** or Esc discards
the current form. Undo removes the last saved graphic.

Weld graphics are saved separately from timeline features and bodies. They do
not create bead solids or change mass properties. Their optional manual symbol
uses the existing geometry weld annotation record.

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
Type ids match the panel's type table. Optional `size` keys are `a`, `z`,
`z1`, `h`, `s`, `d`, `fill`, `collar`, `t` and `w`. Values are
length expressions, evaluated once when created and stored in millimetres.
Sizes must be within the annotation record's 0.000001–100000 mm bounds.

`picks` groups are `line`, `groove`, `point`, `hole`, `ring`, and
`area`. Each entry is either `{"point":[x,y,z]}`, or
`{"edge":[x,y,z],"body":"Body1","index":0}` (use `face` for a face).
Coordinates also accept `{"x":0,"y":0,"z":0}`. A bare
`{"body":"Body1","index":0}` pair means a face in
groove/area groups and an edge in the other groups. References must
identify existing exact topology; these are saved references, with no automatic
reattachment after topology changes in this first step. The optional `symbol`
is a validated `geom::weld::Weld`.
