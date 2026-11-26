Repeat a hole, a pocket or a body along sketch curves: holes along a curved rail, or a row of features following a guide. Pick what to copy, the path, how many and the spacing.

#### Before you start

- Make the feature or body to repeat first.
- Draw and finish a sketch containing the guide curves. Use a connected path and pick its curves in order.
- Choose Features to repeat a History step on the part, or Bodies to copy whole solids.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Type | Rectangular, Circular or Path. | Path for a guide that changes direction. |
| Object Type | Bodies copies whole bodies. Features repeats History steps. | Features for holes in a rail. |
| Objects or Features | What to repeat. | The Hole feature for the first hole. |
| Path | The sketch curves the copies follow. | A line and an arc forming the rail's guide. |
| Quantity | How many in total, counting the original. | 6 holes along the guide. |
| Spacing | Distance along the path between neighbouring placements. | 15 mm pitch. |
| Instances to skip | Choose individual copied openings or bodies by their markers. | Leave out one hole near a mounting bracket. |

#### Skipping copies

Turn on **Instances to skip** to show the copies' markers. Instance 1 is the original and always stays; instance 2 is the first copy. Each opening in a copied feature has its own marker, so you can leave out one hole without removing the other holes at that placement. Openings at the same placement share its instance number.

Green markers stay and red markers are skipped. Click a marker, or drag a box or lasso around several, to change the selection. Use **Restore** beside a skipped item to bring it back, or **Clear skipped instances** to restore all. Editing the pattern shows skipped copies again so they can be restored.

An exclusion follows its opening or body when the source list changes order. Reducing Quantity can temporarily remove a saved placement: the pattern warns, keeps the exclusion and applies it again when the placement returns. A newly drawn replacement for a deleted opening does not inherit the deleted opening's exclusion.

#### Try it: holes along a guide

1. Make a plate and add one through hole near one end.
2. Sketch a guide long enough for the copies and finish the sketch. For six holes at 15 mm spacing, it needs at least 75 mm of travel.
3. Click Pattern on Path, set Object Type to Features and select the Hole step in the History.
4. Click Path and select the guide curves in order. Set Quantity to 6 and Spacing to 15 mm.
5. Turn on Instances to skip and click the marker of a hole to leave out. It turns red; the other holes remain green.
6. Click OK. Edit the pattern later and use Restore to bring that hole back.

#### Tips and common mistakes

- **Do** count the original in Quantity.
- **Do** make the path long enough for the quantity and spacing. The command refuses copies that would run past its end.
- **Do** pick the Hole or cut feature to repeat openings on the same part.
- **Do** check the preview: the original stays where it was made, and the path determines the copies' movement from it.

#### See also

{{#command solid.pattern.rectangular}} repeats in rows and grids. {{#command solid.pattern.circular}} repeats around an axis. {{#command solid.hole}} makes the first hole to pattern.
