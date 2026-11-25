Run a round tube or rod along a path of sketch curves: bent tubing, a coolant or hydraulic line, a handrail, or a wire. You give the outside diameter and, for a tube, the wall.

#### Before you start

- You need a sketch with the path drawn as connected curves: lines and arcs, end to end. The path is the tube's centerline.
- Diameter is the outside diameter (OD). Leave Wall empty for a solid rod; type a wall for a tube.
- Lines meeting at a sharp corner give a mitred joint, like welded pipe. For bent tubing, draw each bend as an arc at the bender's centerline radius.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Path | The sketch curves the tube follows. | A line, an arc and a line for a 90° bend. |
| Diameter | The outside diameter of the tube or rod. It starts at 5 in the design's unit. | 0.375 in for 3/8 tube. |
| Wall | The wall thickness, measured in from the OD. Empty means solid. | 0.035 in. |
| Operation | New Body makes a separate body. Join adds it to a body it touches. Cut takes it out. Intersect keeps only the material inside both. | New Body for a tube. Cut for a round channel through a part. |

#### Try it: a bent 3/8 tube

1. Start a sketch on the XY plane. Draw a 3 in line along X, a tangent arc of 1 in radius turning 90°, and a 2 in line along Y. Finish the sketch.
2. Click Pipe. Click Path and pick the line, the arc and the second line. Path shows 3 selected.
3. Set Diameter to 0.375 in and Wall to 0.035 in. Click OK.
4. The tube is 3/8 in OD with a 0.035 in wall, bent 90° on a 1 in centerline radius.

#### Tips and common mistakes

- **Do** draw the path through the tube's center, the way a tube bender's centerline radius and lengths are given.
- **Do** keep an arc's radius bigger than half the OD, or the inside of the bend folds into itself. Real tube needs much more, often 2 to 3 times the OD, so use the radius your bender's die gives.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.375 in enters 9.525 mm.
- **Don't** leave gaps in the path. The curves must meet end to end.

#### See also

{{#command solid.sweep}} runs any section shape along a path. {{#command solid.cylinder}} makes a straight rod without a sketch.
