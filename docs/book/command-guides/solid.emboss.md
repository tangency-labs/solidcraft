Raise sketch text or a logo out of a face, or sink it in: an engraved part number, a serial or rev letter, a logo, or raised lettering on a casting pattern. It works on flat faces and wraps around round ones.

#### Before you start

- You need a body and a sketch with closed profiles to emboss, usually text made with the sketch's Text tool. Sketch it on or parallel to the face it goes on.
- Deboss sinks the profiles into the face: that's engraving. Emboss raises them out of it.
- Text sketched on a plane tangent to a cylinder wraps around the cylinder. Pick the cylinder's wall as the Target Face to say which one when there is more than one.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Profiles | The sketch profiles to emboss: the letters or the logo outline. | The text "PN 1234-A". |
| Target Face (optional) | The face the profiles go on. Leave it empty to use the face under the sketch. | The outside of a shaft, to wrap a part number around it. |
| Depth | How deep the profiles sink in, or how high they stand out. It starts at 1 in the design's unit. | 0.005 in to 0.010 in for engraving. |
| Type | Emboss raises the profiles out of the face. Deboss sinks them into it. It starts at Emboss. | Deboss for an engraved part number. |

#### Try it: engrave a part number

1. Make a 3 in × 2 in × 0.75 in block.
2. Start a sketch on the top face. Start the sketch's Text tool: it's in the sketch ribbon's Create dropdown, between Centerline and Divide Curve, or press S and type Text. Write "PN 1234-A", 0.25 in high, and finish the sketch.
3. Click Emboss. Click Profiles and pick the letters. Each letter shape counts as its own profile, so Profiles shows several selected.
4. Set Type to Deboss and Depth to 0.010 in. Click OK. The part number is engraved 0.010 in deep into the top face.

#### Tips and common mistakes

- **Do** use Deboss for markings on a machined part. Engraving takes one pass with an engraving tool; raised lettering means milling away everything around it.
- **Do** keep engraving shallow: 0.005 in to 0.010 in reads clearly and doesn't take a small engraving tool too deep.
- **Do** pick a text height and font your engraving tool can cut. Strokes thinner than the tool's tip get lost.
- **Do** change the Depth from its default. It starts at 1 in the design's unit, far too deep for a marking.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.010 in enters 0.254 mm.

#### See also

{{#command solid.extrude}} cuts or raises a profile to a set distance. {{#command solid.hole}} places drilled and tapped holes.
