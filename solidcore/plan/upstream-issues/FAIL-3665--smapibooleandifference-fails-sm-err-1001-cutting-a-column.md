#### FAIL-3665. `SmApiBooleanDifference` fails (`SM_ERR`, 1001) cutting a column whose corner meets an internally tangent circle
- **Severity:** Wrong result
- **Area:** Surface
- **Location:** `SmMerge::MakeFaceuse` (three cases) and `SmBrep::MakeFaceTopology` (one), reached from `SmApiBooleanDifference`.
- **What happens:** In `fixtures/solidcraft/3665-column-in-channel.json` every case fails at the cut of the r 3.04 cylinder centred at (0, 20) from the column (step 12 of `column_in_channel`, step 14 of the other three). That circle is internally tangent to the column's r 23.04 wall (r 23.039 in `control_column_clear`), so the column ends in a sharp corner on the wall. SolidCraft's expected volumes are closed forms; SMLib never reaches the final union.
- **How found:** a standalone SM_API driver running the case steps on the C++ reference build (`SmApiCreateBox/Cylinder`, `SmApiBooleanIntersection/Difference`, `SmApiBrepCopy`), with gdb return breakpoints on `SmMerge::MakeFaceuse`, `SmBrep::MakeFaceTopology` and `SmBrep::TestClosureSplitRegion`.
- **Confirmed:** yes, on the C++ build for main 92454ca60. The Rust port fails at the same step in the same function (`boolean::merge::p2::make_faceuse` and `topology::brep::p6`, status 1001).
