Create a sheet metal base, bend an existing edge into a wall, or build a contour
from an open sketch. The sheet metal rule supplies the material thickness and
bend settings so the folded and flat versions describe the same part.

#### Before you start

| Flange type | What you need to select | Result |
| --- | --- | --- |
| Base | A closed region in a finished sketch. | A new flat sheet metal body. |
| Edge | One or more supported edges of an existing sheet metal body. | Bent walls along those edges. A sketch is not required for this mode. |
| Contour | An open chain of straight sketch lines. | A sheet following that outline, extended by the chosen distance. |

Choose or create a **Sheet Metal Rule** before building the part. To work from
an ordinary solid plate, use **Convert to Sheet Metal** first. An arbitrary
solid edge does not become a sheet metal edge just by selecting Flange.

#### Options

| Choice | Meaning |
| --- | --- |
| Type | Base, Edge or Contour. The selection determines which input is appropriate. |
| Rule | Sets thickness, bend radius, K-factor and the gaps/reliefs that keep bends manufacturable. Change thickness in the rule, not as a flange height. |
| Height | Sets the edge flange's height. Check the height reference and preview when matching a drawing dimension. |
| Angle | Sets how far the edge bends; **90°** makes a right-angle wall. |
| Per-edge angle and height | Lets selected edges in one feature have different dimensions. Empty values use the common setting. |
| Position | Places the flange relative to the selected edge. The preview shows which material remains inside or outside the original outline. |
| Flip | Bends toward the other side of the sheet. |
| Corner Relief Shape and size | Use the rule's setting or override relief between edge flanges of this feature. These overrides are not available for base or contour flanges. |
| Distance | Sets the length of a contour flange's extrusion along the open sketch. |

#### Try it: two sides of a tray

1. Set a sheet rule with **1.5 mm** thickness and **1.5 mm** bend radius.
2. Draw and finish an **80 × 50 mm** rectangular sketch.
3. Choose **Flange**, select the rectangle's region and make a **Base** flange.
4. Choose **Flange** again, select an edge and set its height to **20 mm**.
5. Add another 20 mm flange on the opposite edge. Check the bend directions
   before confirming each feature.
6. Use **Create Flat Pattern** to inspect the developed part and bend lines.

#### Tips and common mistakes

- **Do** use one edge flange feature for adjoining walls whose shared corners
  should close together. Flanges made as separate features keep their own ends.
- **Do** inspect warnings and the flat pattern; a relief that cannot fit may be
  left out with a warning.
- **Don't** confuse height with sheet thickness. The rule owns thickness.
- **Don't** select a sketch point for an edge flange: choose an edge of the
  sheet. For a base flange, select a closed sketch region instead.

#### Examples and community resources

![An 80 by 50 mm sheet with two opposite 20 mm edge flanges](../generated/sheet-metal.png)

| Resource | What to explore | Author and licence |
| --- | --- | --- |
| [Editable sheet metal demo](https://github.com/tangency-labs/solidcraft/blob/main/docs/book/scripts/sheet-metal.json) | The rule, base sketch and two edge flanges used in this illustration. | SolidCraft contributors; MIT OR Apache-2.0. |

[Contribute a tip, demo, model or screenshot](../dev/command-guides.md), explaining
the edge or region that must be selected.

#### See also

{{#command sheet.hem}} returns an edge back toward the sheet.
{{#command sheet.fold}} bends a sheet along a chosen line.
{{#command sheet.manage_rules}} controls thickness and bend settings.
{{#command sheet.flat_pattern}} shows the developed part.
