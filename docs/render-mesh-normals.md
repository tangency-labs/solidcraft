# Shading normals on imported meshes

Imported meshes can retain a normal at every triangle corner. These are shading
metadata: positions, triangle indices, watertight connectivity and optical boundary
certificates keep their independent geometric meaning. CPU and GPU capture already
consume the resulting display mesh normals.

OBJ `vn` indices, including negative indices, retain authored normal seams. OBJ `s`
groups without normals generate angle-weighted normals within each connected
manifold fan. `s off` and different groups keep hard boundaries. Vertex welding does
not merge corner normals. OBJ UV coordinates remain outside the current saved
procedural material mapping contract.

Files without normals or smoothing groups retain flat shading. Honoring OBJ
normals and smoothing groups changes newly imported annotated files. To restore
the previous flat shading, insert with `crease_angle_degrees: 0`. This explicit
selection clears imported corner-normal metadata after validation and preserves
the original flat display and saved-storage paths.

For a separate fixture copy, explicitly request a positive crease policy when
importing its source:

```json
{"command":"file.insert_mesh","params":{"path":"/absolute/path/curved-part.obj","crease_angle_degrees":30}}
```

The optional finite angle ranges from 0 to 180 degrees. It regenerates shading
normals; it does not refine or move geometry. Normal generation joins only edges
with exactly two oppositely wound incident triangles, and only when their normal
angle is within the requested angle. It also handles exact duplicate vertices in
STL-style sources. Boundary edges, nonmanifold edges and disconnected fans stay
separate. Normals use corner-angle weights, making a fan less sensitive to triangle
areas. A canceled normal sum falls back to that triangle's geometric normal.
Faces without usable geometric normals do not join or weight neighboring fans.
Their corner metadata uses a finite unit fallback; the existing mesh-body rules
still discard repeated-index triangles.

For an already embedded mesh, the Rust document API
`MeshData::generate_crease_normals(angle_degrees)` performs the same explicit edit
without re-importing or changing positions or indices. Apply it to a separate
fixture copy and record the selected bodies and angle. This avoids replacing body
identities and saved surface bindings. The helper computes the result before
replacing the shared normal array, so invalid input leaves the source unchanged.

No angle can infer all intentional artistic boundaries. A shallow chamfer can fall
inside the selected angle. Prefer authored normals/smoothing groups for such
parts, use an angle below a deliberate crease, and inspect the result. Do not
apply one angle to every OPUS body without checking its flats and fillets. Smooth
normals cannot repair a polygonal silhouette or replace a finer authoritative
surface tessellation. Advanced reciprocal optical facet models may deliberately
use geometric normals for transport.

Saved JSON uses optional `corner_normals`: nine f64 values per triangle, in corner
order. Empty/absent arrays preserve old flat shading and old serialized fields.
Container mesh blobs use `SCMESH02` when normals are present and continue reading
`SCMESH01`. Both cold evaluation and native model caches retain normals. Shared
arrays keep Undo copies cheap; content fingerprints include normal changes.
Normal storage costs 72 bytes per triangle in the saved source and separately in
an evaluated shaded mesh body; cache/preview bounds include those bytes.

Normals transform by the inverse transpose under nonuniform scale. Mirroring
reverses triangle and normal-corner order while retaining outward geometric winding.
The display conversion normalizes normals and puts them in their geometric
triangle's hemisphere. Geometric normals remain authoritative for medium entry,
exit and topology checks.
