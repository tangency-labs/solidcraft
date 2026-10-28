# Feature mirrors and patterns

Choose **Mirror**, **Rectangular Pattern** or **Circular Pattern** from the Solid
Create panel and set **Object Type** to **Features**. Select the source features
in the timeline or pick their faces in the viewport. Selecting features before
starting the command fills the Features input automatically.

Feature copies replay the source operations: a patterned hole cuts the target
solid, and an additive feature adds material. The mirror's New Body / Join
selector belongs to body copying and is hidden for feature replays.

Select the mirror plane, rectangular directions or circular axis using the
selection inputs. Click an input to pick into it; click a selected item again to
remove it, or use its clear button. The preview uses the same commands as OK.
Cancel leaves the design and undo history unchanged.

Rectangular feature patterns have two direction columns, each with Quantity,
Spacing, Flip and Center controls. Directions can meet at any angle. A missing
second direction produces a single row. Direction vectors are saved with the
feature; editing a picked guide later does not change these saved vectors.

Each rectangular direction has its own **Distance Type**:

- **Spacing** (the default): the value is the distance between neighbouring copies.
- **Extent**: the value is the distance from the first copy to the last, so five
  copies over an 80 mm extent stand 20 mm apart. With Center it is still the whole
  span.
- **To Object**: pick a face, vertex or point instead of typing a value. The copy
  furthest along the direction reaches it, measured from the middle of what is
  patterned (a planar face square to the direction counts as a whole). The object
  sets the side, so Flip changes nothing, and the copies follow the object and the
  source when the model changes. Not available for patterns of components yet.

Circular feature patterns have Quantity, Total angle, Flip and Center controls.
Six instances over 90 degrees use 18-degree steps, including both ends of the
arc. A full circle divides 360 degrees by the quantity without duplicating the
source at 360 degrees.

Quantities include the original. Center keeps the original fixed and alternates
copies on its two sides. For four instances the slots are `0, +1, -1, +2`;
Flip changes them to `0, -1, +1, -2`. This also chooses the extra side for an even
centered circular pattern. Rectangular directions have independent controls.

Double-click a saved feature in the timeline to edit it. Mirrors and patterns
retain their feature sources, including features that created bodies, and
patterns retain their placement controls and arbitrary direction vectors after
save/reopen. Source edits recompute the copies. Undo and redo cover the whole edit.

The command forms share sketch patterns' alignment names:

```json
{"command":"solid.pattern.rectangular","params":{"features":["Hole1"],"dir1":[1,0,0],"count1":4,"spacing1":15,"dir2":[0.6,0.8,0],"count2":2,"spacing2":20,"alignment":"center","alignment2":"center","reversed":true,"reversed2":false}}
```

```json
{"command":"solid.pattern.circular","params":{"features":["Hole1"],"axis":{"origin":[40,60,0],"dir":[0,0,1]},"count":6,"angle":"90 deg","alignment":"center","reversed":true}}
```

The distance types are `type1` / `type2`: `spacing`, `extent` (`spacing1` /
`spacing2` is then the first-to-last distance) or `to_object` with `to1` / `to2`,
a point on the face, vertex or point to reach:

```json
{"command":"solid.pattern.rectangular","params":{"features":["Hole1"],"dir1":[1,0,0],"count1":5,"type1":"extent","spacing1":80,"dir2":[0,1,0],"count2":3,"type2":"to_object","to2":[30,120,2.5]}}
```

Omitted alignments default to `right`, and omitted reversals to `false`, preserving
older designs. `left` is also accepted by the command API. New placement values
are stored through the pending document-format migration.
