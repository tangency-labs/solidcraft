Make a cylinder from a diameter and a height: a round blank, a pin, or a boss standing on a face. Stand it on a face and locate it from two edges, the way a print dimensions it, and it can join the part. A cylinder on a face grows outward, so Cut only removes material where the cylinder overlaps the part; to cut into the part, leave Face empty and set the base below the face with X, Y and Z.

#### Before you start

- To stand the cylinder on a part, you need a flat face for it to stand on. With no face picked, it stands at the X, Y and Z you type, along the Z axis.
- Locate it from edges of that face the same way a print dimensions a boss from two edges. Pick an edge as Reference 1 and type its distance, then do the same for Reference 2. Both edges must be straight and must not be parallel to each other.
- Height runs from the face out along the face's normal, away from the part.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Face | The flat face (or a vertex) the cylinder stands on. Leave it empty to place it by X, Y and Z. | The top face of a plate, for a boss. |
| Reference 1 and Distance 1 | An edge of the face, and how far the cylinder's center is from it. | The left edge, 0.75 in. |
| Reference 2 and Distance 2 | A second edge, and how far the center is from it. | The front edge, 0.75 in. |
| Size | Whether you type a Diameter or a Radius. | Diameter, to match the print. |
| Diameter | The cylinder's diameter (or Radius, if Size is set to Radius). It starts at 20 in the design's unit. | 0.500 in for a boss. |
| Height | How long the cylinder is, from the face out. It starts at 20 in the design's unit. | 0.25 in. |
| Placement, X, Y, Z | With no face picked, where the center of the cylinder's base goes. | 0, 0, 0 to start a round blank at the origin. |
| Operation | It starts at New Body. New Body makes a separate body. Join adds it to the part. Cut takes it out of the part. Intersect keeps only the material inside both. | Join for a boss. New Body for a round blank or a pin. |

#### Try it: put a boss on a plate

1. Make a 3 in × 2 in × 0.75 in block.
2. Click Cylinder. Click Face and click the top face of the block.
3. Click Reference 1, pick the left edge of the top face and set Distance 1 to 0.75 in. Click Reference 2, pick the front edge and set Distance 2 to 0.75 in.
4. Set Diameter to 0.5 in and Height to 0.25 in. Set Operation to Join and click OK. A 0.500 in boss stands 0.25 in tall, 0.75 in from each edge.

#### Tips and common mistakes

- **Do** use Diameter when the print gives a diameter. Switching Size between Diameter and Radius converts the value for you.
- **Do** use New Body for a round blank or a dowel pin that is its own part, so it doesn't merge with the part around it.
- **Do** use {{#command solid.hole}} for a drilled or tapped hole instead of a Cut cylinder. The hole carries its callout and tap drill.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.5 in enters 12.7 mm.

#### See also

{{#command solid.hole}} locates a hole from edges the same way. {{#command solid.extrude}} makes a boss of any shape from a sketch. {{#command solid.thread}} adds a thread to the cylinder.
