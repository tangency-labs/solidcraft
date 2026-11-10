Hollow out a solid to an even wall thickness, leaving the faces you pick open: a box with an open top, an enclosure, or a cup.

#### Before you start

- You need a solid body.
- Pick the faces to remove. They become the openings. Pick the top face of a block to get an open box.
- To make a closed hollow part with no openings, click the body instead of a face.
- Round or chamfer the edges around the opening after you shell, not before. Shell can fail on a round that runs into the opening.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Faces/Body | The faces to remove, or the body for a closed hollow part. | The top face of a block for an open box. |
| Thickness | The wall thickness. | 0.125 in for a small enclosure. |
| Direction | Inside keeps the outside size and builds the wall inward. Outside keeps the inside size and builds the wall outward. Both splits the wall across the original faces. | Inside when the outside size is on the print. Outside when the inside size matters, such as a pocket a part must fit in. |
| Tangent chain (under Options) | Also removes faces that run smoothly into the ones you picked, such as a fillet around the top edge. | On by default. Turn it off to remove only the faces you clicked. |

#### Try it: an open-top box

1. Make a 4 in × 3 in × 2 in block (sketch a rectangle, Extrude 2 in).
2. Click Shell and click the top face. Faces/Body shows 1 selected.
3. Set Thickness to 0.125 in and Direction to Inside.
4. Click OK. The block is an open box, still 4 in × 3 in × 2 in outside, with 0.125 in walls and floor.
5. To try Outside, edit the shell and set Direction to Outside. The inside is now 4 in × 3 in and the outside grows by the wall.

#### Tips and common mistakes

- **Do** pick Direction from the dimension that matters. Inside holds the outside size, Outside holds the inside size.
- **Do** shell first and round the rim after. Shell then doesn't have to follow the rounds.
- **Don't** click the body when you want an opening. Clicking the body makes a closed hollow part with no way in.

#### See also

{{#command solid.fillet}} rounds the rim after you shell. {{#command solid.extrude}} makes the block to start from.
