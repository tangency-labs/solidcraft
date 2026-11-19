Make a ring with a round section: an O-ring, a round handle, or, with Cut, a round-bottom groove around a shaft or boss. You set two radii and the center point.

#### Before you start

- You don't need a sketch or a body. A torus can start a new design.
- The ring lies flat around the Z axis, centered on the point.
- Both sizes are radii, not diameters. Major radius runs from the center to the middle of the ring's section. Minor radius is the radius of the section itself.
- To model an O-ring from its inside diameter (ID) and cross-section (CS): Major radius = ID ÷ 2 + CS ÷ 2, and Minor radius = CS ÷ 2.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Point | A vertex, edge, face or point for the center. Leave it empty to use X, Y and Z. | The center of a boss's end face. |
| Major radius | From the center of the ring to the middle of its section. It starts at 20 mm. | 0.5625 in for an O-ring 1.000 in ID × 0.125 in CS. |
| Minor radius | The radius of the round section. It starts at 5 mm. Keep it smaller than Major radius. | 0.0625 in for a 0.125 in cross-section. |
| Placement, X, Y, Z | Placement is always Center. X, Y and Z set the center when no point is picked. | 0, 0, 0. |
| Operation | It starts at New Body. New Body makes a separate body. Join adds it to a body it touches. Cut takes it out. Intersect keeps only the material inside both. | New Body for an O-ring. Cut for a round-bottom groove. |

#### Try it: model an O-ring

1. In a new design, click Torus.
2. For an O-ring 1.000 in ID × 0.125 in CS, set Major radius to 0.5625 in and Minor radius to 0.0625 in.
3. Leave X, Y and Z at 0 and Operation at New Body. Click OK.
4. The ring lies flat around the Z axis: 1.000 in inside, 1.250 in outside, 0.125 in thick.

#### Tips and common mistakes

- **Do** halve the sizes on the print. An O-ring is called out by ID and cross-section diameter; the dialog wants radii.
- **Don't** use a Cut torus for an O-ring gland. A standard gland has a flat bottom and square walls sized from the O-ring maker's table; sketch it and use {{#command solid.revolve}} instead.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.5625 in enters 14.2875 mm.

#### See also

{{#command solid.sphere}}, {{#command solid.cylinder}} and {{#command solid.box}} place the same way. {{#command solid.revolve}} turns any sketched section into a ring.
