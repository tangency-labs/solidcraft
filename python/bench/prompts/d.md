SolidCraft CAD, driven from Python with the `solidcraft` package. Units mm, Z up. A SolidCraft session is already running: every Python script you run (with Bash) works on that same design, so build in as few scripts as you can.

    python - <<'PY'
    import solidcraft as sc
    d = sc.session()
    d.cmd.solid.box(length=60, width=40, height=8, corner=[0, 0, 0], body_name='Plate')
    for x, y in [(8, 8), (52, 8)]:
        d.cmd.solid.hole(position=[x, y, 8], diameter=6)
    PY

When a script ends it prints what changed: + new body, ~ changed body (volume before -> after, bbox), - removed. Check that instead of measuring again. A failed call raises an error with SolidCraft's message; calls before it stay done.

- d.cmd.<area>.<name>(...): every SolidCraft command, with keyword arguments: solid.box, solid.cylinder, solid.hole, solid.fillet, solid.chamfer, solid.shell, solid.extrude, solid.revolve, solid.loft, solid.move, solid.combine, sketch.create, sketch.line, sketch.circle.center, component.create, ... help(d.cmd.solid.fillet) shows its arguments; print(sc.find('word')) lists matching command ids with their parameters.
- d.measure(summary=True), d.body_topology(...), d.inspect_design(...): inspection. d.skill_reference(skill='solidcraft-modeling') returns the modelling guide (command shapes, plane axes, known traps).
- Edges and faces are picked by a point on them: d.cmd.solid.fillet(edges=[[x, y, z]], radius=2).
- Sketch: s = d.cmd.sketch.create(plane='XY')['sketch']; draw with sketch.* (sketch.line(points=[...], closed=True)); d.cmd.sketch.finish(); then d.cmd.solid.extrude(sketch=s, distance=10).
- operation='cut' | 'join' | 'intersect' with targets=['Body'] combines a new feature with existing bodies.
- numpy, scipy and pandas are installed: compute positions, outlines and sizes with them and pass arrays straight in.
