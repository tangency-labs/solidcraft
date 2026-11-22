Cut a flat bevel along edges of a solid: the "break all edges" or "0.03 × 45°" callout on a print. You pick the edges and how the chamfer is sized.

#### Before you start

- You need a solid body with edges to chamfer.
- Pick edges one at a time, or click a face to chamfer every edge around it.
- Chamfer edges before you add other features that touch them. An edge that already meets a small chamfer or fillet has little room left, and a chamfer there can fail.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Edges | The edges to chamfer. Click edges one by one, or click a face to take all of its edges. | The four top edges of a block, or the top face. |
| Chamfer Type | Equal Distance: the same leg on both faces. That is a 45° chamfer only where the faces meet at 90°. Two Distances: a different leg on each face. Distance and Angle: one leg and an angle. | Equal Distance for a standard edge break on a square corner. Distance and Angle whenever the print gives an angle, like 0.06 × 30°, and for any chamfer on a sloped face. |
| Distance (Equal Distance) | The leg length on both faces. | 0.03 in for a typical edge break. |
| Distance 1 and Distance 2 (Two Distances) | The leg on each face. | 0.06 in by 0.03 in. |
| Distance 1 and Angle (Distance and Angle) | The leg on the first face, and the angle off that face. | 0.06 in at 45°. |
| Flip | Swaps which face the first distance is measured on. Shown for Two Distances and Distance and Angle. | Turn it on if the long leg ends up on the wrong face. |
| Tangent chain | Follows the edge around smooth (tangent) bends, such as a fillet, so one pick chamfers the whole run. | Leave it on for edges that flow into a fillet. |
| Preview | Shows the chamfer on the part while you set it up. | Leave it on. |

#### Try it: break the top edges of a plate

1. Make a 3 in × 2 in × 0.75 in block (sketch a rectangle, Extrude 0.75 in).
2. Click Chamfer and click the four top edges. Edges shows 4 selected.
3. Set Chamfer Type to Equal Distance and Distance to 0.03 in.
4. Click OK. All four top edges have a 0.03 × 45° chamfer.
5. To change it, edit the chamfer and set Chamfer Type to Distance and Angle, Distance 1 to 0.06 in and Angle to 30. Use Flip to swap which face takes the 0.06 in leg.

#### Tips and common mistakes

- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.03 in enters 0.762 mm.
- **Do** chamfer all the edges of one size in a single Chamfer. It's one feature to change when the print changes.
- **Don't** chamfer an edge right next to an existing small chamfer with a larger size. If the chamfer would run past the neighbouring face, the dialog says so; make the chamfer smaller or chamfer those edges together in one feature.

#### See also

{{#command solid.hole}} adds a countersink at the top of a hole. {{#command solid.fillet}} rounds an edge instead of beveling it.
