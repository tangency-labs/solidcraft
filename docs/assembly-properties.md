# Assembly physical properties

The Browser's component **Properties** action measures the selected assembly placement,
including its nested components. Select the root design to measure every instance in the
assembly. **Inspect → Measure → Properties** opens the same window for the one selected
placement, or else for the active component. The window shows material-aware mass, volume,
surface area, center of mass, first moments, the moment of inertia and the world bounding box.
Pending occurrence moves are included without capturing them. While the window is open, the
view marks the center of mass with its principal axes, numbered and colored as the window
lists them. **Copy matrix**, under the tensor, puts it on the clipboard: three lines of
tab-separated values at full precision, in the units shown, ready to paste into a spreadsheet
or code. The window for the whole design is titled just **Properties**; one opened on a
component, placement or bodies carries its name.

The same measurements are available through commands:

```json
{"command":"component.properties","params":{"component":"Bracket"}}
{"command":"component.properties","params":{"component":"root"}}
{"command":"component.properties","params":{"component":"Subassembly","descendants":false}}
{"command":"occurrence.properties","params":{"occurrence":"Bracket:2"}}
{"command":"component.properties","params":{"component":"root","about":[0,0,0]}}
```

`component.properties` defaults to the active component and measures its first placement.
`occurrence.properties` measures the chosen instance. A nested occurrence can appear beneath
several instances of its parent; supply `path` (the complete list of occurrence ids from the
root) to choose one of those placements. An ambiguous occurrence without a path returns an
error. Both commands accept `descendants: false` to measure only directly owned bodies.

Results contain a `bodies` list and a `total` object. Each body's entry identifies the original
body name, owning component and occurrence path. Physical material density is resolved from
that original body, so copies keep their material and mass. Values use millimetres, square
millimetres, cubic millimetres and grams; coordinates use the world frame.

Mass and center of mass account for each body's density. Volume and area are summed over
placed bodies, including overlapping bodies and repeated instances. Measurements use the
kernel's tessellation-based body measurement. An empty component reports zero totals and
null center of mass and bounding box. A failed body measurement returns an error instead of
presenting an incomplete assembly total. These commands leave geometry, pending placements
and undo history unchanged.

## Moment of inertia

Each closed body's entry has `inertia_g_mm2`: its inertia tensor about its own center of mass,
in world axes, in g·mm². The `total` object has:

- `first_moment_g_mm`: the first moments of mass, Σ m·c (the total mass times the center of
  mass).
- `inertia_g_mm2`: the inertia tensor about the total center of mass, in world axes. Each body's
  own tensor is moved there with the parallel-axis theorem.
- `principal_moments_g_mm2` (smallest first) and `principal_axes` (unit vectors, one per
  moment). The first two axes point along the world axis each is nearest to; the third is the
  first × the second, so the three make a right-handed frame. Moments within 0.1% of the largest
  count as equal, as a round body's two equal moments do. Their axes are then the world axes
  nearest the plane they share.
- `inertia_about`, only when the command has `about: [x, y, z]` (mm, world): `{point,
  inertia_g_mm2}`, the tensor about that point in world axes.

The tensor is the matrix that turns an angular velocity into angular momentum. Its diagonal holds
the moments of inertia (Ixx = ∫ (y² + z²) dm …), and the off-diagonal terms are the negated
products of inertia (Ixy = −∫ x y dm …), the convention URDF and most dynamics libraries take. For
kg·m², divide by 10⁹.

The tensor is integrated over each body's welded export mesh, the one URDF export takes its
inertia from. It is then scaled to the body's measured mass, so it agrees with the mass and center
of mass shown. Planar bodies are exact. A round body is within about 0.1% at the standard export
quality. Each body is tessellated once, however many times the assembly places it; each placement
turns its tensor. A closed body whose mesh does not close has `inertia_g_mm2: null`, and so does
the total. Open bodies have no volume, so they have no tensor either, and the total leaves them
out.
