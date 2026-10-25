# Assembly physical properties

The Browser's component **Properties** action measures the selected assembly placement,
including its nested components. Select the root design to measure every instance in the
assembly. The window shows material-aware mass, volume, surface area, center of mass and
the world bounding box. Pending occurrence moves are included without capturing them.

The same measurements are available through commands:

```json
{"command":"component.properties","params":{"component":"Bracket"}}
{"command":"component.properties","params":{"component":"root"}}
{"command":"component.properties","params":{"component":"Subassembly","descendants":false}}
{"command":"occurrence.properties","params":{"occurrence":"Bracket:2"}}
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

Sheet-metal perforation fields contribute their physical holes to volume, mass, surface area
and center of mass even though they do not cut the B-rep. Hole centers follow the panels when
folded or unfolded and follow each occurrence's placement. Measure and component/occurrence
Properties use the same correction. Bounding boxes and B-rep topology still describe the
uncut body. A failed perforation calculation returns an error rather than an uncorrected total.
