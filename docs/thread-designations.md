# Metric thread designations

Generated size choices use the shared metric catalogue. Its coarse pitch is omitted from the designation: **M8** means diameter 8 mm and coarse pitch 1.25 mm. A fine or custom pitch remains explicit, for example **M8x1**. Hole catalogue choices and generated Thread size/pitch choices follow the same rule.

This changes generated UI text, not the resolved numerical pitch or geometry. Existing authored designation text, including explicit legacy coarse pitches, is retained when a dialog is merely displayed. Custom and fine designations remain editable. Catalogue dimensions are millimetres regardless of the design's display units.

For ISO metric tapped holes, the drill is the tap maker's for the size (M8 gives 6.8 mm, M8x1 7 mm), and nominal diameter minus pitch for a custom size the table lacks. Saving and reopening retains the resolved pitch. See the [tapped-hole catalogue](thread-catalogue.md) for the additional metric fine choices and finite UNC/UNF and parallel G/Rp catalogues with their tabulated pilot recommendations. The Thread command's forms are in [Threads](threads.md).

## Unified inch Thread designations

The Thread feature also accepts `<size>-<TPI> UNC|UNF|UNEF|UN` with an optional
`-1A`, `-2A`, `-3A` (external) or `-1B`, `-2B`, `-3B` (internal). The class is
kept in the callout and refused on the wrong side. For example, `1/4-20 UNC-2A`
has major 6.35 mm and pitch 1.27 mm; `#10-32 UNF` has major 4.826 mm and pitch
25.4/32 mm. Size can be #0 through #12, fractional inches or decimal inches.
The UI, scripts and MCP use these same strings. Automatic sizing stays metric;
select Unified to choose the nearest fitting catalogued UNC size. The Size list
also offers the existing UNF catalogue. See [Threads](threads.md) for the shared
60° basic profile and the supported classes.
