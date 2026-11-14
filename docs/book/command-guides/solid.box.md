Make a box from a length, width and height: a block of stock, a plate, a fixture base, or a block to cut features into. Place it by a corner or by its center, at a point you pick or at the X, Y and Z you type.

#### Before you start

- You don't need a sketch or a body. A box can start a new design.
- Length runs along X, Width along Y and Height along Z.
- Placement decides what the point is: Corner puts one corner of the box there, Center puts the middle of the box there.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Point | A vertex, edge, face or point to place the box at. Leave it empty to use X, Y and Z. | A corner of another block, to stack a box on it. |
| Length, Width, Height | The size of the box along X, Y and Z. Each starts at 20 mm. | 3 in × 2 in × 0.75 in for a plate. |
| Placement | It starts at Corner. Corner: the point is the box's low corner, and the box runs in +X, +Y and +Z from it. Center: the point is the middle of the box. | Corner at the origin so the part's dimensions start from 0, 0, 0, like a work offset on a corner. Center for a part dimensioned from its middle. |
| X, Y, Z | Where the point is, when none is picked. | 0, 0, 0. |
| Operation | It starts at New Body. New Body makes a separate body. Join adds it to a body it touches. Cut takes it out. Intersect keeps only the material inside both. | New Body for a fresh block. Cut for a square pocket or step. |

#### Try it: make a plate from the corner

1. In a new design, click Box.
2. Set Length to 3 in, Width to 2 in and Height to 0.75 in.
3. Set Placement to Corner and leave X, Y and Z at 0. Click OK.
4. The plate's corner sits at the origin, so every dimension on it reads from 0, 0, 0, the way you'd set a work offset on that corner.

#### Tips and common mistakes

- **Do** pick Corner or Center to match how the print dimensions the part. It makes later features easier to locate.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 3 in enters 76.2 mm.
- **Do** use {{#command solid.extrude}} from a sketch when the outline isn't a plain rectangle.
- **Don't** confuse Length and Width when the part has a long side. Length runs along X.

#### See also

{{#command solid.cylinder}} makes a round blank the same way. {{#command solid.bounding_solid}} puts a box around a part you already have.
