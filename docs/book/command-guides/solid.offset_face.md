Push faces of a solid in or out by a set distance, and the faces around them follow. Use it to add finish stock, open up a pocket or bore, make a pocket deeper or a wall thicker, without going back into the sketch.

#### Before you start

- You need a solid body. The faces can be flat or curved, such as the wall of a bore.
- A positive Distance pushes the face out of the material and adds stock. A negative Distance pulls it into the material and removes stock.
- On an inside wall, such as the side of a pocket or the wall of a bore, out of the material means into the opening. A negative Distance opens the pocket or bore up; a positive one closes it in.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Faces | The faces to move. You can pick more than one. | The floor of a pocket, or the wall of a bore. |
| Distance | How far each face moves, square to the face. Positive adds material, negative removes it. | 0.010 in to leave stock on a face for finishing. -0.005 in on a bore wall to open the bore 0.010 in on the diameter. |

#### Try it: deepen and widen a pocket

1. Make a 3 in × 2 in × 0.75 in block and cut a 1 in × 1 in pocket 0.25 in deep into the top.
2. Click Offset Face (in the Modify menu) and click the floor of the pocket. Set Distance to -0.125 in. The arrow points down into the part. Click OK. The pocket is now 0.375 in deep.
3. Click Offset Face again and click one side wall of the pocket. Set Distance to -0.25 in. The arrow points into the material, away from the pocket. Click OK. The pocket is now 1.25 in wide that way.

#### Tips and common mistakes

- **Do** remember a bore moves on the radius. To open a bore 0.010 in on the diameter, offset its wall -0.005 in.
- **Do** watch the arrow before clicking OK. It shows which way the face will move.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.010 in enters 0.254 mm.
- **Do** use Offset Face for a quick size change on a finished model, such as a stock allowance or a fit adjustment. If the size will change again, change the sketch or the feature instead, so the dimension lives where the print puts it.

#### See also

{{#command solid.extrude}} sets a depth when the feature is made. {{#command solid.shell}} sets a wall thickness for a whole hollow part. {{#command solid.hole}} sets a hole's size from the start.
