Round the edges of a solid: the "R.03 typ" or "break all edges R.015" callout on a print, or the corner radius left in a pocket by the end mill. You pick the edges and the radius.

#### Before you start

- You need a solid body with edges to round.
- Pick edges one at a time, or click a face to round every edge around it.
- Round the larger radii first. Do the vertical corners of a block before its top edges, so the small top round runs smoothly around the big corner.
- An inside corner on a milled part can't be sharper than the cutter. Make inside radii a little larger than the end mill's radius, so the tool doesn't bury itself in the corner.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Edges | The edges to round. Click edges one by one, or click a face to take all of its edges. | The four vertical corners of a block, or the top face. |
| Radius | The radius of the round. A new fillet starts at 2 mm, shown in the design's unit. | 0.03 in for a typical edge break. |
| Type | Constant radius: one radius all along the edge. Chord length: the width across the round stays the same whatever angle the faces meet at. Variable radius: a radius at each end, and more along the edge if you add them. Curvature continuous: a smoother section where it meets flat faces (G2). Spline: the radius follows control radii you set along the edge. | Constant radius for almost everything on a machined part. Variable radius for a round that has to grow along an edge. |
| Corners (Constant radius) | Shown when three or more of the picked edges meet at one corner, such as rounding every edge of a block in one Fillet. Tangent makes a rolling-ball corner. Curvature makes a smooth patch set back along each edge, with a Setback box. | Leave it on Tangent. That is the corner a ball end mill leaves. |
| Face limits (Constant radius) | What happens when the radius is too big for the faces beside an edge. Exact radius stops and says why. Largest radius rounds every edge with the largest radius that fits, and shows the Applied Radius. Narrowed radius keeps the radius and narrows the round only where the faces run out. | Leave it on Exact radius, so a radius that doesn't fit is caught instead of quietly changed. |
| Turns (Constant radius) | Where two rounds turn around a sharp corner edge, such as the rounds along the base of a boss at its upright edges. Mitre runs the rounds on until they meet. Swept turns them around the corner edge. | Mitre unless you want the corner rolled around. |
| End Radius, Swap Ends, Mid Radius (Variable radius) | The radius at the far end of the first edge, which end gets the start radius, and extra radii along the edge (Add Mid Radius). Smooth radius change blends between them instead of running straight. | A round that goes from 0.06 in at one end to 0.25 in at the other. |
| Tangent chain | Follows the edge around smooth (tangent) bends, so one pick rounds the whole run. | Leave it on for edges that flow into an existing round. |
| Preview | Shows the round on the part while you set it up. | Leave it on. |

#### Try it: round the corners and break the top edges of a plate

1. Make a 3 in × 2 in × 0.75 in block (sketch a rectangle, Extrude 0.75 in).
2. Click Fillet and click the four vertical corner edges. Set Radius to 0.25 in and click OK.
3. Click Fillet again and click the top face. Edges shows 8 selected: the four straight edges and the four rounded corners.
4. Set Radius to 0.03 in and click OK. The top edge has a 0.03 in round all the way around.

#### Tips and common mistakes

- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.03 in enters 0.762 mm.
- **Do** round all the edges of one size in a single Fillet. It's one feature to change when the print changes.
- **Do** round the big corners before the small edge breaks. Done the other way, the small round has to turn a sharp corner and can fail or leave an odd shape.
- **Don't** make a round bigger than the faces beside it allow. With Face limits on Exact radius, the dialog says "The radius is too large for the faces beside these edges" and OK stays grayed out; use a smaller radius, or set Face limits to Largest radius.
- **Don't** model a round you'll never see just to look finished. Every fillet adds rebuild time and can get in the way of later features.

#### See also

{{#command solid.chamfer}} bevels an edge instead of rounding it. {{#command solid.shell}} hollows a part after the outside edges are rounded.
