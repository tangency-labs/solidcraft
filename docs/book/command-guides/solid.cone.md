Make a cone or a tapered cylinder from two diameters and a height: a lathe center point, a tapered plug or pin, a spot-drill point, or a tapered boss. Its base circle sits at the X, Y and Z you type, and it runs up the Z axis.

#### Before you start

- You don't need a sketch or a body. A cone can start a new design.
- Diameter is the base, at the X, Y and Z point. Top diameter is the other end, Height away along Z. A Top diameter of 0 makes a full cone that comes to a point.
- A negative Height runs the cone down from the base instead of up. Use it to point a cone into a part from its top face.
- To get a cone for an included angle: Height = (Diameter − Top diameter) ÷ 2 ÷ tan(half the angle). For a 60° point on a 1 in diameter, Height = 0.5 ÷ tan 30° = 0.866 in.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Diameter | The diameter of the base circle. It starts at 20 mm and must be more than 0. | 1 in for a 1 in shaft. |
| Top diameter | The diameter of the other end. It starts at 0, a point. More than 0 makes a tapered cylinder (a frustum). | 0 for a center point. 0.75 in for a plug that tapers from 1 in. |
| Height | How far the top is from the base, along Z. It starts at 20 mm. Negative runs it down. | 0.866 in for a 60° point on a 1 in diameter. |
| Placement, X, Y, Z | Placement is always Base Center: X, Y and Z set the center of the base circle. | 0, 0, 0. |
| Operation | It starts at New Body. New Body makes a separate body. Join adds it to a body it touches. Cut takes it out. Intersect keeps only the material inside both. | Join to put a point on the end of a shaft. Cut to cut a cone shape into a face. |

#### Try it: put a 60° point on a shaft

1. Make a 1 in diameter, 3 in tall cylinder at the origin with {{#command solid.cylinder}}.
2. Click Cone. Set Diameter to 1 in, Top diameter to 0 and Height to 0.866 in.
3. Set Z to 3 in, so the base sits on the top of the shaft. Set Operation to Join and click OK.
4. The shaft now ends in a 60° point, like a dead center.

#### Tips and common mistakes

- **Do** work out the Height from the included angle on the print. The dialog has no angle box; use the formula above.
- **Do** use {{#command solid.hole}} for a countersunk or counterbored hole. It sets the countersink angle and diameter for you.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.866 in enters 21.9964 mm.
- **Don't** set Diameter to 0. The base must have a size; put the point at the top with Top diameter 0 and use a negative Height if you need it to point down.

#### See also

{{#command solid.cylinder}}, {{#command solid.sphere}} and {{#command solid.box}} place the same way. {{#command solid.revolve}} turns any sketched profile, such as a Morse taper with a tang.
