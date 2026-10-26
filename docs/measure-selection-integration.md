# Measuring between faces and edges

Inspect → Measure accepts one or two vertices, edges, faces and sketch references.
One reference shows its properties. Two references show their distance; circular
geometry also offers centre, minimum and maximum measurements. The viewport marks
the two ends as **1** and **2**, matching the panel, and keeps those badges clear
of the distance and X/Y/Z labels.

## Choosing references

Hold the pointer over an edge, face or sketch curve to reveal its available
centres, midpoints and endpoints. Click a revealed point to measure from that
exact position. A nearby hole rim remains selectable for its diameter rather
than always selecting the centre.

For overlapping geometry, hold the left mouse button still or use **Select
Other** from the viewport menu. The list groups each body's references together,
with the nearest body first. While measuring or selecting normally it also offers
opposite faces of the same body and obscured edges and vertices. Normal clicks
still select the front geometry. Placement commands, including Hole, retain their
nearest-face list and local snapping.

In Preferences → Graphics, **Include hidden geometry in Select Other** controls
the extra references (`select_other_hidden_geometry`, enabled by default). Turn
it off to restore the nearest-face list. The preference is saved with the other
application preferences. Hidden bodies and geometry removed by a section remain
excluded. Ray traversal is bounded to 64 crossings per body, and the existing
20-row limit per kind still applies.

## Example: plate thickness

Create a 40 × 30 × 10 mm box and open Measure. From the top view, open Select Other
away from an edge, choose the upper face, reopen the list and choose the opposite
lower face. The minimum distance is **10.00 mm**, with ΔZ = 10.00 mm and ΔX/ΔY = 0.
Changing the camera or inspecting the references does not edit the model.

## Integration provenance

This change integrates the front-to-back ordering from PR
[#5509](https://github.com/tangency-labs/solidcraft/pull/5509), hover reference
points from [#5913](https://github.com/tangency-labs/solidcraft/pull/5913), and
numbered measurement ends from
[#6021](https://github.com/tangency-labs/solidcraft/pull/6021). It preserves the
external sketch-reference correction already merged in
[#4427](https://github.com/tangency-labs/solidcraft/pull/4427).

Integration fixes give hover points a separate selection bit from external sketch
lines, retain unit-aware Move inputs and sketch curve priority, keep Hole's local
snapping, and avoid collisions between numbered ends and axis labels. The source
commits are `4224323e`, `483bdd6c`, `a2b0ba13` and `a960a430`; engine snap support
from `65fd7cf9` is already in the base. The original reports and PRs remain linked
for contributor coordination.