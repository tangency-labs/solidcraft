Fold a sheet metal edge back through a half turn to form a return. Use a hem
when you need a doubled-over edge, rather than an upright wall.

#### Before you start

- You need an existing **sheet metal body** and a supported free sheet edge.
  Make a base flange or convert a suitable solid plate first.
- Select the **edge**, not a sketch line, face or point. Hem does not require
  a new sketch. A bend along a line inside the sheet uses Fold instead.
- Leave room for the return so it does not run into another wall or return.

#### Options

| Choice | Meaning |
| --- | --- |
| Edges | The sheet edges to turn back. Check that the preview changes the intended edge. |
| Length | The return's length, measured to its outer face. It is not the thickness of the sheet. |
| Gap | Controls the inner radius of the return bend. Leave it empty to use the sheet rule's hem setting. |
| Flip | Turns the return toward the other side of the sheet. |

#### Try it: a returned edge

1. Create an **80 × 50 mm** base flange in **1.5 mm** sheet.
2. Choose **Hem** and select a free 80 mm edge of the sheet.
3. Set **Length** to **8 mm** and **Gap** to **1.5 mm**.
4. Check which side receives the return; use **Flip** if necessary, then confirm.
5. Inspect **Create Flat Pattern** to see the material needed for the bend.

#### Tips and common mistakes

- **Do** check the bend in the preview: an edge returning toward another feature
  needs enough clearance.
- **Don't** draw a sketch line to tell Hem where to bend. Its input is an
  existing sheet edge; use Fold for a bend line through a panel.
- **Don't** use Hem for an ordinary right-angle wall. That is an edge Flange.

#### Examples and community resources

![A sheet metal plate with one edge returned back toward the plate](../generated/hem.png)

| Resource | What to explore | Author and licence |
| --- | --- | --- |
| [Editable hem demo](https://github.com/tangency-labs/solidcraft/blob/main/docs/book/scripts/hem.json) | The sheet rule, base flange and 8 mm return used in this illustration. | SolidCraft contributors; MIT OR Apache-2.0. |

[Contribute a tip, demo, model or screenshot](../dev/command-guides.md), explaining
the selected edge and dimensions.

#### See also

{{#command sheet.flange}} makes a base sheet or an angled wall.
{{#command sheet.fold}} bends along a line in the sheet.
{{#command sheet.manage_rules}} sets the default hem gap.
{{#command sheet.flat_pattern}} shows the developed part and bend lines.
