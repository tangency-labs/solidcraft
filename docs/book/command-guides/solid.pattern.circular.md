Repeat a hole, a boss or a whole body around an axis: the bolt circle on a flange, or spokes on a hub. You pick what to copy, the axis, how many and over what angle.

#### Before you start

- Make one of the thing you want to repeat first, such as one hole on the bolt circle. On a round face there are no straight edges to locate a hole from, so sketch a point where the hole goes and use Hole with Placement set to Multiple.
- Choose what to copy with Object Type. Bodies copies whole solid bodies. Features repeats a step from the History, such as a Hole or a cut Extrude, on the same body.
- A sketch is neither. To repeat sketch curves, use the sketch's own Circular Pattern, or extrude the sketch first.
- You need an axis to turn about: an origin axis, a sketch line, a straight or circular edge, or a cylinder face such as the bore of the part.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Type | Rectangular, Circular or Path. The same dialog makes all three patterns. | Circular for a bolt circle. |
| Object Type | Bodies copies whole bodies. Features repeats History steps. | Features for holes in one part. Bodies for separate parts, such as loose spokes. |
| Objects or Features | What to repeat. With Features, pick the step in the History. | The Hole feature for one bolt hole. |
| Axis | The line the copies turn about. Clicking a round edge or a cylinder face uses its centre line. | The Z axis, or the face of the centre bore. |
| Quantity | How many in total, counting the original. | 6 for a 6-hole bolt circle. |
| Total angle | The angle the pattern covers. At 360 deg the copies are spaced evenly all the way round. Under 360 deg, the first and last copies sit at the two ends of the angle. | 360 deg for a full bolt circle. 90 deg with Quantity 4 puts holes at 0, 30, 60 and 90 deg. |
| Alignment (Features only) | Flip direction turns the pattern the other way. Center puts copies on both sides of the original: an odd Quantity splits evenly, an even Quantity puts one extra copy on one side, and Flip moves the extra copy to the other side. Quantity 4 over 90 deg with Center gives −30, 0, 30 and 60 deg. | Flip a partial pattern that went the wrong way around. |

#### Try it: a 6-hole bolt circle

1. Make a flange: sketch a 4 in circle on the XY plane at the origin and Extrude it 0.5 in.
2. Sketch a point on the top face, 1.5 in from the centre on the X axis (a 3 in bolt circle). Click Hole, set Placement to Multiple and pick the point. Set Diameter to 0.281 in and Extents to All, then click OK. The Hole must be finished so it shows in the History.
3. Click Circular Pattern. Set Object Type to Features and click the Hole step in the History.
4. Click Axis, then click the Z axis.
5. Set Quantity to 6 and Total angle to 360 deg. Click OK. There are 6 holes, 60° apart.
6. To make it 8 holes, edit the pattern and change Quantity to 8.

#### Tips and common mistakes

- **Do** pattern the Hole feature rather than the body when the holes are all in one part. Change the hole size once and every copy follows.
- **Do** count the original in Quantity. A 6-hole bolt circle is Quantity 6, not 5.
- **Don't** use 360 deg for a partial pattern you want to end on a copy. Under 360 deg the angle is from the first copy to the last; at 360 deg the last would land on the first, so the copies are spaced evenly instead.
- **Don't** pick the edge of the hole you are patterning as the Axis. That turns the hole about its own centre, so every copy lands on the original. Pick the Z axis or the centre bore.
- **Don't** pick the sketch you drew the hole from. Pick the Hole step, or extrude the sketch first.

#### See also

{{#command solid.pattern.rectangular}} repeats in rows and columns. {{#command sketch.pattern.circular}} repeats curves inside a sketch. {{#command solid.hole}} makes the first hole to pattern.
