Repeat a hole, a pocket or a whole body in a row or a grid: a row of tapped holes along a rail, or a grid of vent slots. You pick what to copy, the direction, how many and the spacing.

#### Before you start

- Make one of the thing you want to repeat first, such as one hole in the corner of the grid.
- Choose what to copy with Object Type. Bodies copies whole solid bodies. Features repeats a step from the History, such as a Hole or a cut Extrude, on the same body.
- A sketch is neither. To repeat sketch curves, use the sketch's own Rectangular Pattern, or extrude the sketch first.
- You need a direction for each row: an origin axis, a sketch line or a straight edge of the part.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Type | Rectangular, Circular or Path. The same dialog makes all three patterns. | Rectangular for rows and grids. |
| Object Type | Bodies copies whole bodies. Features repeats History steps. | Features for holes in one plate. |
| Objects or Features | What to repeat. With Features, pick the step in the History. | The Hole feature for the first hole. |
| Direction | The way the first row runs. | The X axis, or the long edge of the plate. |
| Quantity | How many along the first direction, counting the original. | 5 holes in the row. |
| Spacing | Centre-to-centre distance between neighbours in the first direction. | 1 in pitch. |
| Direction 2 | The way the second row runs. Leave it empty for a single row. | The Y axis for a grid. |
| Quantity 2 | How many along the second direction, counting the original. 1 means a single row. | 3 rows. |
| Spacing 2 | Centre-to-centre distance in the second direction. | 0.75 in. |
| Alignment and Alignment 2 (Features only) | Flip direction runs the copies the other way. Center puts copies on both sides of the original: an odd Quantity splits evenly, an even Quantity puts one extra copy on one side, and Flip moves the extra copy to the other side. Quantity 4 with Center gives copies at 0, +1, −1 and +2 spacings. | Flip when the row runs off the end of the part. |

#### Try it: a 5 × 3 grid of holes

1. Make a 6 in × 4 in × 0.5 in plate. Sketch a rectangle from the origin: type 6 in, press Tab to jump to the other side, type 4 in. Then Extrude it 0.5 in.
2. Add one 0.25 in through hole with Hole, 1 in in from each of the two edges at the origin corner.
3. Click Rectangular Pattern. Set Object Type to Features and click the Hole step in the History.
4. Click Direction, then the X axis (or the long edge of the plate). Set Quantity to 5 and Spacing to 1 in.
5. Click Direction 2, then the Y axis. Set Quantity 2 to 3 and Spacing 2 to 1 in.
6. Click OK. The plate has 15 holes on a 1 in grid.

#### Tips and common mistakes

- **Do** count the original in Quantity. Five holes in a row is Quantity 5, not 4.
- **Do** read Spacing as hole-to-hole pitch, the way the print dimensions it, not the total length of the row.
- **Do** pattern the Hole feature rather than the body when the holes are all in one part. Change the hole size once and every copy follows.
- **Don't** forget to check Alignment if the copies land off the part. Flip direction or Center left on from an earlier pattern moves the whole grid.
- **Don't** leave Quantity 2 at 1 and expect a grid. With Quantity 2 at 1 you get a single row even if Direction 2 is picked.

#### See also

{{#command solid.pattern.circular}} repeats around an axis, such as a bolt circle. {{#command sketch.pattern.rectangular}} repeats curves inside a sketch. {{#command solid.hole}} makes the first hole to pattern.
