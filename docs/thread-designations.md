# Metric thread designations

Generated size choices use the shared metric catalogue. Its coarse pitch is omitted from the designation: **M8** means diameter 8 mm and coarse pitch 1.25 mm. A fine or custom pitch remains explicit, for example **M8x1**. Hole catalogue choices and generated Thread size/pitch choices follow the same rule.

This changes generated UI text, not the resolved numerical pitch or geometry. Existing authored designation text, including explicit legacy coarse pitches, is retained when a dialog is merely displayed. Custom and fine designations remain editable. Catalogue dimensions are millimetres regardless of the design's display units.

For ISO metric tapped holes, the drill is the tap maker's for the size (M8 gives 6.8 mm, M8x1 7 mm), and nominal diameter minus pitch for a custom size the table lacks. Saving and reopening retains the resolved pitch. See the [tapped-hole catalogue](thread-catalogue.md) for the additional metric fine choices and finite UNC/UNF and parallel G/Rp catalogues with their tabulated pilot recommendations. The Thread command's forms are in [Threads](threads.md).
