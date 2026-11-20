Drag a sketched profile along a path to make a solid of the same section all the way: bent flat bar or angle, a handle, a rail, or, with Cut, a groove that follows a curve, such as an O-ring face groove around a rounded-rectangle cover.

#### Before you start

- You need a profile sketch (a closed shape) and a path: either a second sketch of connected curves, or a chain of a part's edges. An edge path needs no second sketch.
- Draw the profile on a plane square to the start of the path, touching the path. The profile then runs along the path from there.
- For a plain round tube or rod, {{#command solid.pipe}} is quicker: it needs only the path and a diameter, plus a wall thickness for a hollow tube. Without a wall it makes a solid rod.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Profile | The closed sketch shape to sweep. | A 0.5 in × 0.25 in rectangle for flat bar. |
| Path | The curves or edges the profile follows. | A line, an arc and a line for a bend. |
| Orientation | Turn with Path: the profile turns to stay square to the path, as bent bar does. Keep as Drawn: the profile keeps the direction it was drawn in. Follow Direction: the profile stays square to the path, and the Direction you pick fixes how it turns about the path. | Turn with Path for almost everything. |
| Direction | An axis, sketch line or straight edge, used only with Follow Direction. | The Z axis, to keep a profile upright on a path that climbs. |
| Rail, Second Rail | Guide curves the profile's edge follows. With a rail, the profile turns and changes size to stay on it. Rails replace Orientation, Twist and End Scale; Rail Fit sets how they reshape the profile. | A curve that widens a handle toward one end. |
| Operation | New Body when the design has no body yet; otherwise it starts at Join. Cut takes the swept shape out of the part. | Cut for a groove. |
| Twist (Advanced) | Turns the profile by this angle from the start of the path to the end. It starts at 0 deg. | 90 deg for a twisted bar. |
| End Scale (Advanced) | The profile's size at the end of the path, as a factor of its size at the start. It starts at 1. | 0.5 for a section that tapers to half. |
| Rail Fit (Advanced) | With a rail: Scale grows the whole profile evenly to meet the rail; Stretch stretches it only toward the rail. | Scale to keep the section's proportions. |

#### Try it: bend a piece of flat bar

1. Start a sketch on the XY plane. From the origin, draw a 3 in line along X, a tangent arc of 1 in radius turning 90°, and a 2 in line. Finish the sketch.
2. Start a sketch on the YZ plane. Draw a 0.5 in × 0.25 in rectangle centered on the origin. Finish the sketch.
3. Click Sweep. Click Profile and pick the rectangle. Click Path and pick the three path curves.
4. Leave Orientation on Turn with Path and click OK. The bar follows the bend, square to the path all the way round.

#### Tips and common mistakes

- **Do** start the profile on the path, square to it. A profile away from the path sweeps offset from it.
- **Do** keep bend radii bigger than the profile's half-width, or the inside of the bend folds into itself.
- **Do** use Cut for a groove of constant section along a curve, such as an O-ring face groove: sketch the groove's section where the path starts and sweep it around.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.5 in enters 12.7 mm.

#### See also

{{#command solid.pipe}} sweeps a round section with only a diameter. {{#command solid.extrude}} sweeps a profile along a straight line. {{#command solid.revolve}} sweeps it around an axis.
