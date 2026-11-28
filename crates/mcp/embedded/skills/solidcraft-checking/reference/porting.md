# Porting a model into SolidCraft

For a model that already exists as code (build123d, CadQuery, OpenSCAD) or as STEP. The port is
done when the SolidCraft STEP export matches the source geometry for every parameter value you
check: `compare_step.py` reports xor at most 1e-3 mm3. Volume, area and centre of mass alone are
not enough: in one port, a hexagon left at dof 1 changed the volume by 2e-7 of the total and the
centre of mass by 3e-4 mm, while xor showed 56 mm3 in 96 pieces along the outer walls.

1. **Reference geometry in the design's frame.** The SolidCraft origin often differs from the
   source. Translate the source model to the same frame and export it as STEP for each checked
   parameter value. Pick values that reach every branch of the layout logic.
2. **Deliberate differences.** Logic that SolidCraft expressions cannot hold (conditionals,
   search) gets a closed formula. Check the formula against the source over a wide range of
   values in plain Python first, list where it differs, and apply the formula to the reference
   too (patch the source function), so the comparison stays like for like. Tell the user each
   difference.
3. **Export and compare.** Add `parameters.change` and `file.export {path, bodies: [name]}` per
   value to a script, run it, then
   `uv run --no-project --with build123d python $SKILL/scripts/compare_step.py actual.step reference.step`.
   The pieces' boxes show where the shapes differ; a pair of pieces on the two halves of one face
   means that face is turned, usually by an under-constrained sketch.
4. **Freeze the test.** Measure the reference (volume, area, centre of mass, bounding box) and
   put the numbers into the copy of `templates/test_design.py` next to the design. Ask the user
   before removing the source model; git history keeps it.
