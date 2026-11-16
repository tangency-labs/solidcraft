Put a box around a part: the size of the raw stock it's cut from. Margin adds cleanup stock on every side, the way you'd saw a blank oversize before squaring it up.

#### Before you start

- You need at least one body.
- The box is square to the design's X, Y and Z axes, not to the part. Model the part square to the axes, or the box comes out bigger than the stock you'd really cut.
- The box is a new body, named Bounding, or Bounding (2) if the design already has a body named Bounding. The part itself is not changed.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Bodies (all if none) | The bodies to put the box around. A body selected when you start is picked for you. Clear it to use every body in the design. | Pick one part when the design has several. |
| Margin | Stock added on every side of the box. The overall size grows by twice the Margin in each direction. It starts at 2 in the design's unit; set 0 for the part's exact size. | 0.0625 in on each side to leave room to face and square up a sawn blank. |

#### Try it: size the stock for a plate

1. Make a 3 in × 2 in × 0.75 in block.
2. Click Bounding Solid. Bodies shows the part if it was selected; otherwise leave it empty. Set Margin to 0.0625 in. The yellow preview shows the box. Click OK.
3. A new body, Bounding, covers the part. It is 3.125 in × 2.125 in × 0.875 in: the stock to order or saw.
4. Hide the new body in the Browser to see the part again. It is Bounding, or Bounding (2) if the design already had a body named Bounding.

#### Tips and common mistakes

- **Do** remember the Margin is per side. For 1/8 in of total cleanup on each dimension, set Margin to 0.0625 in.
- **Do** round the stock up to a size you can buy. The box gives the smallest block that holds the part; standard bar and plate sizes are usually a little bigger.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.0625 in enters 1.5875 mm.
- **Don't** expect a box square to the part when the part sits at an angle to the axes. The box stays square to X, Y and Z.
- **Don't** leave Bounding showing while you work. It covers the part; hide it in the Browser.

#### See also

{{#command solid.extrude}} makes the part from a sketch. {{#command solid.offset_face}} adds stock to a single face instead of the whole part.
