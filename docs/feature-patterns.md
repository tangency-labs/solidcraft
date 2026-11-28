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

Rectangular feature patterns have two direction columns, each with a Distance
Type, its values, Flip and Center controls. The Distance Type says what is given:
Quantity and Spacing (the default), Quantity and Extent (the spacing is the extent
divided by one less than the quantity) or Spacing and Extent (as many copies as
fit in the extent: 80 mm at 15 mm makes 6, the last at 75 mm). With Spacing and
Extent, Copy at the end adds one more copy exactly at the end of the extent when
the spacing doesn't divide it, so only the last gap is shorter. A centred
direction set by Spacing and Extent reaches half the extent each way from the
original, with as many copies on each side; Copy at the end then adds a copy at
either end. Quantity and List takes the gaps between neighbours as a
list separated by semicolons and uses them in turn, then from the start again:
`10 mm; 20 mm; 30 mm` with Quantity 5 puts copies at 10, 30, 60 and 70 mm.
Centred, the list runs out from the original on both sides. Directions can meet at any angle. A missing second direction
produces a single row. Direction vectors are saved with the
feature; editing a picked guide later does not change these saved vectors.

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

Omitted alignments default to `right`, and omitted reversals to `false`, preserving
older designs. `left` is also accepted by the command API. New placement values
are stored through the pending document-format migration.
