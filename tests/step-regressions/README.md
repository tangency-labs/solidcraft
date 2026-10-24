# STEP regressions

STEP files that once broke an import, kept so the import stays fixed. Each is written by SolidCraft from
public commands; none comes from another program.

| File | Made by | What it holds |
| --- | --- | --- |
| `rod_doubled_edge.step` | SolidCraft main (`edf57cec`), the commands of #4598: a Ø40 × 60 rod, a Ø8 cross bore cut 15 deep, the rod moved 100 mm, exported | The bore's rim exported as a B-spline of 1 399 control points running back and forth across the bore: before #4598 a moved edge's tolerance grew with the move. The triangulator's assertion lost the body on import (#4031, #4598). |
