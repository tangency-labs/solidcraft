# Metric thread designations

Generated size choices use the shared metric catalogue. Its coarse pitch is omitted from the designation: **M8** means diameter 8 mm and coarse pitch 1.25 mm. A fine or custom pitch remains explicit, for example **M8x1**. Hole catalogue choices and generated Thread size/pitch choices follow the same rule.

This changes generated UI text, not the resolved numerical pitch or geometry. Existing authored designation text, including explicit legacy coarse pitches, is retained when a dialog is merely displayed. Custom and fine designations remain editable. Catalogue dimensions are millimetres regardless of the design's display units.

Tapped Hole continues to derive its existing tap-drill diameter from nominal diameter minus pitch: M8 gives 6.75 mm, while M8x1 gives 7 mm. Saving and reopening retains the resolved pitch. Only the currently supported ISO metric family is offered; other thread families and a complete fine-pitch catalogue are not added by this change.
