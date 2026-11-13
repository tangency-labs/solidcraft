# Sketch patterns

While editing a sketch, use the **PATTERN** toolbar group for **Mirror**,
**Rectangular Pattern** or **Circular Pattern**. On a narrow window the group
collapses to a dropdown. Select several curves before starting a tool, or add
them to its **Entities** input. Each tool previews the result before **OK**;
**Escape** cancels it.

- **Mirror:** select the source geometry and a straight sketch **Mirror line**.
  Symmetry constraints keep the copies linked to the source and mirror line.
- **Rectangular Pattern:** pick **Direction** and optionally **Direction 2**.
  Each accepts a straight sketch line or an origin axis in the sketch plane.
  The lines can meet at any angle. Each direction has its own column with a line
  picker, quantity, spacing and two icon buttons: **Flip direction** and **Center**.
  Without Center, copies extend from the original along the line from start to end;
  Flip reverses that direction. Center distributes copies on both sides while the
  original remains fixed. Quantities are totals including the original: 3 means
  one copy on either side; 4 means two on one side and one on the other. Flip swaps
  which side receives the extra copy. Both controls are independent for each direction.
  Negative spacing reverses the chosen direction. With no direction selected, the first uses sketch X
  and the second is perpendicular to the first. A second quantity of 1 makes
  a single row. Picked sketch lines remain live references as they change.
- **Circular Pattern:** pick a **Center** point (or click empty sketch space),
  then set **Quantity** and **Total angle**. A full turn distributes instances
  evenly without duplicating the first; a partial turn includes both endpoints.
  **Center** places copies on both angular sides of the original. **Flip direction**
  reverses rotation and chooses the extra side for even totals. For six total holes
  over 90 degrees, the spacing is 18 degrees: the original stays at 0 degrees, with
  copies at +18, -18, +36, -36 and +54 degrees. Flip moves the last copy to -54 degrees.
  Five holes over 90 degrees use 22.5-degree spacing and extend 45 degrees either side.
  Without Center, a positive angle proceeds counterclockwise from the original;
  Flip reverses it.

Circular **Quantity** accepts a unitless parameter or expression, such as `HoleAmount`
or `HoleAmount + 1`. The expression stays in the editor after applying and reopening;
changing its parameters updates the copies. It must evaluate to a whole number from
2 to 500. Enter a fixed number to replace the parameter link.
Existing fixed-count patterns keep their counts; enter a parameter name to link an older pattern.

Quantities include the original. Edit the original lines or their dimensions
to update their copies. Right-click a rectangular or circular pattern instance
and choose **Edit Sketch Pattern**, or double-click its pattern constraint badge,
to change the quantities, spacing, directions or angle. Rectangular and circular
patterns also appear as children beneath their sketch in the browser and left
feature history. Expand the sketch to find them; double-click a
pattern to open its saved editor, including from a finished sketch. Surviving copies keep
their IDs. Patterns support undo/redo and survive saving and reopening.

The same operations are available through `sketch.mirror`,
`sketch.pattern.rectangular` and `sketch.pattern.circular`. For example:

```json
{
  "command": "sketch.pattern.rectangular",
  "params": {
    "entities": ["l1", "l2", "l3"],
    "direction": "guide1",
    "direction2": "guide2",
    "count": 3,
    "spacing": 20,
    "count2": 2,
    "spacing2": 10,
    "alignment": "center",
    "alignment2": "center",
    "reversed": false,
    "reversed2": true
  }
}
```

`direction` and `direction2` reference sketch line IDs. `dir` and `dir2`
instead accept fixed `[x,y]` vectors. Distances are millimetres; the circular
command's `angle` parameter is in degrees. The combined quantity is capped at 500.
Deleting a direction line breaks its rectangular pattern association and leaves
the remaining geometry in place; undo restores the association.

Circular patterns accept the same `alignment` and `reversed` parameters, for example
`{"entities":["hole"],"center":[0,0],"count":6,"angle":90,"alignment":"center","reversed":false}`.
