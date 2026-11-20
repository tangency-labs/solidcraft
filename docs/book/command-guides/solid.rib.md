Add a thin wall along an open sketch line, filled out to the faces of the part: a gusset in the corner of an L-bracket, or a stiffening rib on a wall. You draw where the rib's edge goes, and the rib fills from there to the part.

#### Before you start

- You need a body and a sketch with an open line (or chain of lines) for the rib's outer edge. The line doesn't have to touch the part, but the rib must have faces to fill to.
- For a gusset, sketch on a plane through the middle of the bracket, and draw the line across the corner from one leg to the other.
- With Direction on In plane, the rib fills from the line toward the part inside the sketch plane, and Thickness is across that plane.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Curves | The open sketch line or lines for the rib's edge. | A diagonal line across the inside corner of an L-bracket. |
| Direction | In plane: the rib fills from the line within the sketch plane, Thickness across the plane. Normal: the rib grows along the sketch's normal instead, Thickness across the line. | In plane for a gusset. |
| Thickness | How thick the rib is. It starts at 2 in the design's unit. | 0.25 in for a gusset on a 0.25 in bracket. |
| Depth | How far the rib fills from the line. Empty means to the next face of the part. | Leave it empty for a gusset. |
| Flip | Fill toward the other side of the line. | Turn it on if the preview fills away from the corner. |

#### Try it: put a gusset in an L-bracket

1. Make an L-bracket: on the XZ plane, sketch an L with 3 in legs and a 0.25 in thickness, then extrude it 2 in symmetric so the XZ plane runs through its middle.
2. Start a sketch on the XZ plane. Draw a line across the inside corner, from 2 in up the vertical leg to 2 in out along the horizontal leg. Finish the sketch.
3. Click Rib. Click Curves and pick the line. Leave Direction on In plane and set Thickness to 0.25 in.
4. Check the preview fills toward the corner; if not, turn on Flip. Click OK. A 0.25 in gusset fills the corner, centered on the bracket.

#### Tips and common mistakes

- **Do** sketch an In plane rib on the plane where you want its middle. Half the thickness goes each way from that plane, so a rib sketched on an outside face hangs half off the part. With Normal, the rib grows out of the sketch plane instead, centered on the line: draw the line where the rib's middle goes, on a plane at the rib's edge.
- **Do** weigh Rib against a plain extrude. For one gusset on a machined part, a triangle sketched and extruded Symmetric works just as well. Rib pays off when the walls may change size (it fills to the new faces), when the faces it meets are curved or sloped, or when you need many ribs ({{#command solid.web}}).
- **Do** keep a machined rib's thickness at least as big as the end mill that has to cut around it, and think about how the pocket beside it will be cut. A thin rib between deep pockets chatters.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.25 in enters 6.35 mm.
- **Don't** close the sketch line into a loop. A rib needs an open line; use {{#command solid.extrude}} for a closed shape.

#### See also

{{#command solid.web}} makes a grid of ribs from several lines at once. {{#command solid.fillet}} rounds the rib's edges.
