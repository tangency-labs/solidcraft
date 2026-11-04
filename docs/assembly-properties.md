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

## Per occurrence and per rigid group

`assembly.mass_properties` (no parameters) gives what a dynamics solver needs of each moving
part, measured as above, in kg, mm and kg·mm²:

```json
{"command":"assembly.mass_properties","params":{}}
```

- `occurrences`: the design's own bodies (`occurrence: 0`) and then every occurrence, each with
  its `frame` (origin and unit axes in world coordinates), its own bodies (material, density,
  volume and mass), and `mass`: its bodies' total `mass_kg`, `center_of_mass_mm` (world) and
  `center_of_mass_local_mm` (in its frame), and the tensor about that centre in world axes
  (`inertia_world_kg_mm2`) and in its frame's axes (`inertia_local_kg_mm2`). Only the bodies the
  occurrence's component owns count, not those of occurrences nested in it.
- `groups`: occurrences that move as one body, each with the occurrences it holds, the same
  `mass` totals (combined with the parallel-axis theorem) in the frame of its lowest
  occurrence (`frame_occurrence`; 0 is the world), and `fixed` when it holds the design itself
  or a grounded occurrence. An unsuppressed rigid joint (Rigid Group makes them) puts its two
  occurrences in one group, and a nested occurrence that no joint places goes with the
  occurrence it sits in, since the joint solver leaves it where its parent puts it.
- `warnings`: open bodies (no mass), closed bodies whose mesh does not close (`inertia_*: null`),
  scaled placements (no tensor in their own axes) and subassemblies placed more than once (the
  occurrences inside them are listed at the first placement, where joints place them).

A body without a material assignment has the default material's density, as Properties shows.
Pending occurrence moves count. Rust callers get the same numbers, typed, from
`Session::mass_properties()`.
