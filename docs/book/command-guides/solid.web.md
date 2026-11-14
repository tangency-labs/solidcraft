Add a grid of thin walls from several open sketch lines at once, each filled out to the faces of the part: the ribs of a waffle or isogrid pocket, dividers in a tray, or supports inside a housing. Each line becomes its own wall.

#### Before you start

- You need a body and a sketch of open lines, one for each wall. Lines can cross; each one makes its own wall.
- For walls standing in a pocket or tray, sketch the lines on the top face or a plane above the pocket and set Direction to Normal. The walls then grow down from the sketch to the floor, Thickness across each line.
- With Direction on In plane, each wall fills inside the sketch plane instead, as {{#command solid.rib}} does, with half the Thickness each side of the plane.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Curves | The open sketch lines, one wall per line. | Two crossing lines over a pocket. |
| Direction | In plane: each wall fills within the sketch plane, Thickness across the plane. Normal: each wall grows along the sketch's normal, Thickness across its line. | Normal for walls standing in a pocket. |
| Thickness | How thick each wall is. It starts at 2 mm. | 0.080 in for the ribs of a lightened pocket. |
| Depth | How far each wall fills from the sketch. Empty means to the next face of the part. | Leave it empty to run the walls down to the floor. |
| Flip | Fill toward the other side of the sketch. | Turn it on if the preview grows away from the part. |

#### Try it: divide a tray into four

1. Make a 4 in × 3 in × 1 in box and use {{#command solid.shell}} on its top face with a 0.1 in wall, so it's an open tray.
2. Start a sketch on the tray's top rim plane. Draw one line across the middle lengthwise and one across the middle widthwise, each ending at the inside walls. Finish the sketch.
3. Click Web. Click Curves and pick both lines. Set Direction to Normal and Thickness to 0.1 in. Leave Depth empty.
4. Check the preview runs down into the tray; if not, turn on Flip. Click OK. The tray now has four pockets.

#### Tips and common mistakes

- **Do** make the walls between machined pockets at least as thick as you can hold without chatter, and check the end mill can reach the corners between them. The corner radius left is the cutter's radius.
- **Do** use one Web for the whole grid. It's one feature to change when the rib layout changes.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.1 in enters 2.54 mm.
- **Don't** close the lines into loops. Web needs open lines; for a pocket outline use {{#command solid.extrude}} with Cut.

#### See also

{{#command solid.rib}} makes a single wall the same way. {{#command solid.shell}} hollows the part first.
