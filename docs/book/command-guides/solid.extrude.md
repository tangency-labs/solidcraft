Turn a flat sketch region into a solid, or add or remove material from an existing
body. For example, a rectangle extruded 20 mm becomes a rectangular block.

#### Before you start

- For a solid extrusion, finish a sketch with at least one closed region. Select
  the region you want, rather than an individual edge or point.
- You can also select a planar face of an existing body instead of a sketch.
- To add or remove material, an existing body must meet the extrusion. Choose
  **New Body** when you want an independent piece.
- **Thin Extrude** uses sketch curves instead of filled regions; open chains are
  allowed. This is useful for a wall or a ring.

#### Options

| Choice | What it does | When to use it |
| --- | --- | --- |
| Profiles | Chooses the filled sketch regions to extrude. A loop inside another loop makes a hole. | Select just the regions you need when a sketch contains several. |
| Distance | Sets how far the shape extends. Values can include units, such as **20 mm**, or a named design parameter. | Make a part to a known size. |
| Reverse Direction | Sends the extrusion to the other side of the sketch. | The preview points away from the intended body. |
| Symmetric | Splits the entered total distance equally across the sketch plane. **10 mm** extends **5 mm on each side**. | Keep the sketch at the middle of a part. |
| Two Sides | Sets a separate distance on each side. | The sketch is between two unequal extensions. |
| Through All | Reaches through the target body. | Cut a hole that must stay open when the body becomes thicker. |
| To Object | Stops at a selected target face, vertex or point. An offset can stop before or beyond that target. | Let the end follow another part of the design. |
| Start | Starts at the sketch, at an offset from it, or from a selected object. | Leave a gap before the extrusion begins. |
| Taper Angle | Slopes the side walls. | Make drafted sides; reduce the angle if the profile's narrow details cannot support it. |
| Operation | **New Body** makes a separate piece; **Join** adds material; **Cut** removes it; **Intersect** keeps only the overlap. | Choose the intended result explicitly when the automatic preview is not what you want. |
| Target bodies | Chooses which existing bodies take part in a join, cut or intersection. | Several bodies lie in the extrusion's path. |
| Thin Extrude | Builds walls along curves; thickness and side control where the material lies. Two-sided walls can have different thicknesses. | Make ribs, walls or rings without drawing both wall boundaries. |

#### Try it: a 20 mm block

1. Create a sketch on the XY plane and draw a closed rectangle.
2. Finish the sketch and choose **Extrude**.
3. Select the rectangular region, set **Distance** to **20 mm**, and choose
   **New Body**.
4. Check the preview's direction and confirm. You should have one solid block.
5. Edit the extrusion in the timeline to change its distance later.

#### Tips and common mistakes

- **Do** check the operation before confirming. Automatic operation can join an
  addition or cut into an existing body; it is not a promise of a separate part.
- **Do** use Through All for a cut that should follow later thickness changes.
- **Don't** expect an open outline to make a filled solid. Close it, or choose
  Thin Extrude for a wall.
- **Don't** assume a symmetric distance is the distance on each side. New
  symmetric extrusions use the total distance; older saved designs can retain
  their earlier convention.
- Taper with a two-sided or symmetric extent is not supported. A very steep
  taper can also be refused rather than creating an invalid result.

#### Examples and community resources

![Bracket made with an extrusion and a second extrusion that cuts the holes](../generated/bracket.png)

The bracket starts with a closed L-shaped profile; another extrusion removes
the two holes. Its dimensions are controlled by design parameters.

| Resource | What to explore | Author and licence |
| --- | --- | --- |
| [Editable bracket recipe](https://github.com/tangency-labs/solidcraft/blob/main/examples/bracket.json) | A complete SolidCraft example using an extrusion, a fillet and a cut. | SolidCraft contributors; MIT OR Apache-2.0. |

[Contribute a tip, demo, model or screenshot](../dev/command-guides.md) to add a
reviewed resource beside this explanation.

#### See also

{{#command solid.revolve}} turns a profile around an axis.
{{#command solid.sweep}} carries a profile along a path.
{{#command sheet.flange}} creates sheet metal with a thickness rule.
