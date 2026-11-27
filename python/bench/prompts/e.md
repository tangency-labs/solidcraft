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

The scx vocabulary is also available: `from scx_vocab import *` gives `d` (the session above) and these functions, which act at once:
  box(x, y, z, at=min corner | center=(..), op='new'|'join'|'cut'|'intersect', name=, into=)
  cylinder(h, d=|r=, at=base centre, axis='Z'|'X'|'Y', op=, name=)   cone(h, d1, d2=0, at=, axis=, op=)   sphere(d=|r=, at=, op=)
  hole(at=point ON a face, d, depth=None (through), direction=None (into the face))
  s = sketch('XY'|'XZ'|'YZ', offset=0)   # u,v = (x,y)|(x,z)|(y,z); offset along Z|Y|X
    s.rect(p0, p1) s.rect_center(c, w, h) s.circle(c, d=) s.slot(center, length, width, angle=0) s.polygon(c, sides, r_vertex) s.poly([pts])
    s.extrude(dist, op=, symmetric=False, name=)   s.revolve(axis='X'|'Y'|'Z', angle=360, op=)
  loft([sketches], op=, name=)   fillet([points on edges], r)   chamfer([points on edges], d)
  hollow(t, open='top'|'bottom'|'front'|...|None, body=)   shell([point on each open face], t)
  combine(target, [tools], op='cut')   move(bodies, by=(dx,dy,dz), rotate=('Z', deg), copy=False)   remove(bodies)   rename(body, new)
  with part('Name'): ...   measure() -> {body: {volume, bbox, faces}}   edges(body)   faces(body)
op='join'/'cut'/'intersect' act on the last body made or changed (into='Body' picks another).
