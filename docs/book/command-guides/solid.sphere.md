Make a ball from a diameter: a tooling ball, a ball end on a shaft, or, with Cut, a ball seat or detent pocket. Place its center at a point you pick or at the X, Y and Z you type.

#### Before you start

- You don't need a sketch or a body. A sphere can start a new design.
- The point is always the center of the sphere.
- To cut a ball seat into a part, put the center on or near the face and use Cut. How deep the seat goes depends on how far the center sits below the face.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Point | A vertex, edge, face or point for the center. Leave it empty to use X, Y and Z. | The end of a shaft, for a ball end. |
| Size | Whether you type a Diameter or a Radius. | Diameter, to match a ball's size. |
| Diameter | The sphere's diameter (or Radius, if Size is set to Radius). It starts at 20 mm. | 0.500 in for a tooling ball. |
| Placement, X, Y, Z | Placement is always Center. X, Y and Z set the center when no point is picked. | 0, 0, 0. |
| Operation | It starts at New Body. New Body makes a separate body. Join adds it to a body it touches. Cut takes it out. Intersect keeps only the material inside both. | New Body for a ball. Cut for a ball seat or a detent pocket. |

#### Try it: cut a ball seat into a plate

1. Make a 3 in × 2 in × 0.75 in block with its corner at the origin.
2. Click Sphere. Set Diameter to 0.5 in.
3. Set X to 1.5 in, Y to 1 in and Z to 0.75 in, so the center sits on the middle of the top face.
4. Set Operation to Cut and click OK. A half-ball seat, 0.500 in across and 0.25 in deep, is cut into the top face.

#### Tips and common mistakes

- **Do** raise the center above the face for a shallower seat. With a 0.500 in ball and the center 0.1 in above the top face, the seat is 0.15 in deep and about 0.46 in across.
- **Don't** put the center below the face for a ball seat. The pocket then goes deeper than half the ball and is narrower at the top than inside: an undercut a ball end mill can't cut.
- **Do** use Diameter when the print gives a ball diameter. Switching Size converts the value for you.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.5 in enters 12.7 mm.
- **Don't** forget a ball seat cut this way is a full sphere shape. For a drilled seat with a drill point or a flat bottom, use {{#command solid.hole}}.

#### See also

{{#command solid.cylinder}} and {{#command solid.box}} place the same way. {{#command solid.revolve}} makes a ball end that blends into a turned profile.
