# STEP regressions

STEP files that once broke an import, kept so the import stays fixed. Each is written by SolidCraft from
public commands, or entity by entity from formulas; none comes from another program.

| File | Made by | What it holds |
| --- | --- | --- |
| `rod_doubled_edge.step` | SolidCraft main (`edf57cec`), the commands of #4598: a Ø40 × 60 rod, a Ø8 cross bore cut 15 deep, the rod moved 100 mm, exported | The bore's rim exported as a B-spline of 1 399 control points running back and forth across the bore: before #4598 a moved edge's tolerance grew with the move. The triangulator's assertion lost the body on import (#4031, #4598). |
| `tiny_rounds_coarse_tolerance.step` | Written entity by entity from formulas | Three faces of radius 0.05 mm (a pin's quarter round, a corner ball, a quarter torus round a rim) in a file of 0.1 mm uncertainty, as an exporter writing a coarse tolerance gives them. A collapse test that took a surface thinner than the tolerance for a pole refused all three, and an enclosure lost 887 faces of its connectors. |
| `cylinder_spline_rims.step` | Written entity by entity from formulas | A cylinder of radius 10 and height 5 whose rims are closed rational cubic B-splines of two pieces, each an exact half circle, and no seam edge. A piece reaching half a turn about the axis was taken to reach every angle, so the wall had no angle left for its seam and was left out of the part. |
| `cylinder_rim_off_its_wall.step` | Written entity by entity from formulas | A cylinder of radius 10 and height 5 whose upper rim is a circle of radius 10.0002, 0.2 µm outside the wall, in a file stating 0.01 µm, the rims' vertices half a turn apart and no seam edge. The seam added from one rim to the other ends 0.2 µm off the cylinder; held to the file's uncertainty it was refused, and the wall was left out of the part. |
