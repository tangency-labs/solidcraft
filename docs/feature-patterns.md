# Feature mirrors and patterns

Choose **Mirror**, **Rectangular Pattern**, **Circular Pattern** or **Pattern on Path** from the Solid
Create panel and set **Object Type** to **Features**. Select the source features
in the timeline or pick their faces in the viewport.

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
either end. Directions can meet at any angle. A missing second direction
produces a single row. Direction vectors are saved with the
feature; editing a picked guide later does not change these saved vectors.

In a rectangular, circular or path pattern, turn on **Instances to skip** to
pick individual copies in the viewport. Each opening in a copied multi-hole
feature has its own marker. Instance 1 is the original and always stays;
instance 2 is the first copy. Openings at the same placement share its instance
number and can be skipped separately.

Green markers stay and red markers are skipped. Click a marker or drag around
several to toggle them. The toolbar's **Select** panel controls the drag shape:
turn on **Lasso** for a freehand loop, or leave it off to use a box.
**Ctrl/Cmd-click** toggles. **Shift-click** toggles by default and only adds copies
to the skip list when the add-only Shift preference is enabled. Use **Restore**
beside a skipped item to bring it back, or **Clear skipped instances** for all
copies. Editing a saved pattern shows its skipped copies again.

Selections follow the original sketch point, profile or body rather than its
position in a reordered source list. Deleting an opening keeps its exclusion
dormant; drawing a new opening at the same position does not transfer the
exclusion to it. Reducing Quantity can also leave a saved placement outside the
pattern. The pattern warns and keeps that exclusion, which becomes active again
when its source and placement return.

The three pattern commands accept the same exclusion options for Features and
Bodies. `skip: [2, 4]` leaves out whole placements, including any openings added
to their source later. `skip_members: [[2, "Hole pair", 0]]` leaves out only the
first opening of the feature named **Hole pair** at instance 2. The member index
starts at zero and is resolved when the selection is made; saved selections
follow their original source after reorder, save/reopen, undo and redo. A source
feature ID can be used instead of its name. For a body pattern, use source 0 and
the body's current index in the selected bodies list.

New selections must refer to existing copies and source members. Invalid
instance numbers, sources and members are refused without changing the design.
When editing an existing pattern, its unchanged saved exclusions may remain
dormant. Saved selectors can be sent back unchanged through the command API.

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
older designs. `left` is also accepted by the command API.
