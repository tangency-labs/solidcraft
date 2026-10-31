# Exchange snapshot

Schema 1 (2026-10). Crate `solidcraft-exchange` (`crates/exchange`), CLI
`solidcraft-cli export-snapshot`.

Analysis tools (electromagnetics, structural, CFD) need a design's shape at given parameter
values: its faces, edges and corners by name and kind, how it was built, and its materials. They
should not depend on SolidCraft's kernel or document model, which change often. The exchange
snapshot is the one stable boundary between them: tools depend on `solidcraft-exchange` alone,
or read its JSON.

## API

```rust
use solidcraft_exchange::{Direction, Edit, FeatureId, Mm, Options, Plane, Point, Quantity, Tolerance};
let design = solidcraft_exchange::open(&bytes)?;           // a .sc / .solidcraft / JSON design
let params = design.parameters();                         // name, expression, typed value, source
let options = Options::default()
    .with_override("width", Quantity::Length(Mm(30.0)))   // or .with_override("width", "3 cm")
    .with_tolerance(Tolerance::Relative(2e-4))
    .with_edits(vec![
        Edit::Suppress(FeatureId(4)),                     // rebuild without feature F4
        Edit::Partition { name: "Mid".into(), plane: Plane::new(Point([30.0, 0.0, 0.0]), Direction::X), bodies: vec![] },
        Edit::SplitFace { name: "Strip".into(), body: "Body1".into(), face: "F1:+z".into(),
                          plane: Plane::new(Point([15.0, 0.0, 0.0]), Direction::X) },
    ]);
let snap = design.snapshot(&options)?;
let json = solidcraft_exchange::to_json(&snap)?;          // deterministic bytes
```

`snapshot` applies the overrides (all or none), then the edits, to a copy of the design,
evaluates it and leaves the design as it was. Snapshots of one `Design` share an evaluation, so
a second snapshot recomputes only the features its overrides and edits reach. A feature that
fails fails the snapshot (`Error::Evaluation`, each failure with its feature id), unless the options
allow failures (`with_failures_allowed(true)`): then the snapshot holds what the rest of the
timeline built and lists the failed features in `failures` (`feature`, `name`, `message`). Errors are
typed: `UnknownParameter`, `ParameterValue`, `Overrides`, `NoFeature`, `NoBody`, `NoFace`,
`Invalid`, `Limit`, `Read`, `NotSupported`.

```text
solidcraft-cli export-snapshot part.sc --out part.json --param width=30 --param r="1.5 mm" \
    [--tolerance 0.01 | --relative-tolerance 2e-4] [--body Body1]... [--suppress F4]... [--edits edits.json]
```

`--param` takes a number in the unit the design shows the parameter in, or an expression;
`--edits` a JSON array of edits: `{"suppress": 4}`,
`{"partition": {"name", "plane": {"point", "normal"}, "bodies"?}}`,
`{"split_face": {"name", "body", "face", "plane"}}`.

`from_json` reads a snapshot and checks it (`Snapshot::validate`): every index refers to
something there, every direction is a unit vector, coordinates are finite, NURBS patches are
well formed, body names are unique. A `Snapshot` in hand can therefore be walked safely, by
reference rather than by index: `body.triangles()` (each with its face and corner positions),
`body.edges_of(face)`, `body.edges_at(corner)`, `edge.points(&body)`, `body.face_named(..)`.
Lengths and angles add, scale and print with their unit (`Mm(3.0) * 2.0`, `"6 mm"`), and
names key maps that look up by `&str`.

## Types

Every value says what it is. Names and indices have their own types, the same in JSON as the
string or number they hold: `BodyName`, `FaceName`, `EdgeName`, `VertexName`, `MaterialName`,
`ParameterName`; `FeatureId`, `FaceIndex`, `EdgeIndex`, `VertexIndex`, `MeshVertex` (an index
into a body's `positions`). Persistent names are opaque: compare and keep them, never parse
them; what they say is given as data (`Face::features`, `Edge::faces`). Lengths are `Mm`, angles
`Radians` (JSON: the number); a `Point` is `[x, y, z]` in mm; a `Direction` is a unit vector
`[x, y, z]`, refused when it has no length; a `Quantity` is `{"length": mm}`, `{"angle": rad}` or
`{"number": n}`; a `Transform` is a row-major 4×4 matrix. Kinds are enums written as tagged
objects (`{"type": "cylinder", …}`), each with an `other` for kinds the reader does not know.

## Content

```json
{
  "schema": 1, "unit": "mm", "design": "blob",
  "parameters": [
    {"name": "d1", "expression": "15", "value": {"quantity": {"length": 15.0}}, "display_unit": "mm", "source": "feature", "comment": ""}
  ],
  "materials": [
    {"name": "Steel", "density_kg_m3": 7850.0, "properties": [
      {"property": {"kind": "elasticity", "elasticity": {"symmetry": "isotropic", "young_modulus_pa": 2e11, "poisson_ratio": 0.3}},
       "conditions": {"temperature_k": 293.15, "moisture_dry_basis_kg_kg": null, "strain_rate_per_s": null, "state": null}, "source": "s1"}
    ]}
  ],
  "bodies": [
    {
      "name": "Blob", "material": null, "closed": true,
      "positions": [[20.0, 0.0, 0.0], ...],
      "faces": [
        {"index": 0, "name": "F2:start", "surface": {"type": "plane", "point": [0, 0, 0], "normal": [0, 0, -1]},
         "triangles": [[0, 2, 1], ...], "features": [2], "attributes": {}}
      ],
      "edges": [
        {"index": 8, "name": "F2:side:s1#6|F2:side:s1#7",
         "bounds": {"between": [1, 2]}, "curve": {"type": "line", "start": [14, 14, 0], "end": [14, 14, 15]},
         "ends": [7, 8], "path": [22, 183]}
      ],
      "vertices": [
        {"index": 0, "name": "F2:side:s1#3|F2:side:s1#6|F2:start", "point": [20, 0, 0], "position": 0, "edges": [0, 7, 10]}
      ],
      "attributes": {}
    }
  ],
  "features": [
    {"id": 2, "name": "Extrude1", "kind_name": "extrude", "suppressed": false,
     "kind": {"type": "extrude", "operation": "new_body", "sketch": {"id": 1, "name": "Outline", "frame": {...}, "profiles": [...]},
              "direction": [0, 0, 1], "distance": 15.0, "two_sided": false, "targets": []},
     "inputs": [{"label": "Distance", "value": {"length": 15.0}}], "creates": ["Blob"], "bodies": ["Blob"]}
  ],
  "edits": [
    {"edit": {"partition": {"name": "Mid", "plane": {"point": [30, 0, 0], "normal": [1, 0, 0]}, "bodies": []}},
     "features": [6], "bodies": ["Blob"]}
  ]
}
```

- **Parameters.** Every user parameter, sketch dimension, named feature input and text
  parameter, in the parameters table's order. `value` is `{"quantity": …}` (in mm or radians,
  whatever unit the design shows it in, `display_unit`), `{"text": …}`, or `"invalid"` when the
  expression does not evaluate. `source`: `user`, `sketch`, `feature`.
- **Materials.** The physical materials the exported bodies are assigned. `density_kg_m3` is the
  density SolidCraft computes mass with. `properties` carries a design material's engineering
  card as entered, uncertified: `density` (`kg_m3`), `elasticity` (`isotropic`:
  `young_modulus_pa`, `poisson_ratio`; `orthotropic`: `axes`, `young_moduli_pa`,
  `shear_moduli_pa`, `poisson_ratios`), `tensile_strength`, `compressive_strength`,
  `yield_strength`, `shear_strength` (`pa`), `thermal_conductivity` (`w_m_k` per axis),
  `thermal_expansion` (`per_k` per axis), `specific_heat` (`j_kg_k`), each with the
  `conditions` it holds at and its card `source`.
- **Bodies.** In world coordinates, in the design's order. Each has the kernel's `size` (the
  diagonal of its B-rep vertices' bounding box) and `shells` (boundary shells: 1 for a solid in
  one piece without voids), and with `with_mass(true)` its `mass` (`volume`, `area`, `centroid`,
  bounding box `min` and `max`) as the kernel measures it from converged tessellations. A face's
  `inferred` is true when its name was worked out from the geometry rather than followed through
  the design's history. In an assembly every placement of a
  component's body is its own body: the first keeps the body's name, every further one adds its
  occurrence path in brackets (`Body1 [Arm:2]`).
- **Mesh.** One tessellation per body, the one the tagged 3MF export writes
  ([3mf-face-tags.md](3mf-face-tags.md)): welded watertight (vertices within 10⁻⁶ of the body size
  merged, T-junctions split), vertices shared across faces, triangles counter-clockwise seen from
  outside. `closed` says every side is used once each way. The chord tolerance is 2·10⁻⁴ of the
  body's size by default, never below 1 µm.
- **Faces.** In the body's face order, with the persistent name, the features that made or
  shaped the face, and its surface: `plane` (`point`, `normal` out of the material), `cylinder`
  (`origin`, `axis`, `radius`), `cone` (`apex`, `axis` into the cone, `half_angle`), `sphere`
  (`center`, `radius`), `torus` (`center`, `axis`, `major`, negative on a spindle torus's inner
  sheet, `minor`), `nurbs` (`u` and `v`, each `{degree, knots}` with as many knots as control
  points that way plus the degree plus one; `net[i][j]`, each `{point, weight}`, weight 1 for a
  B-spline; ∂S/∂u × ∂S/∂v points out of the material; the face is the part its edges bound) or
  `other`.
- **Edges.** In the body's edge order: named after the faces they bound (`~k` when several share
  them), with those faces (`bounds`: `{"between": [a, b]}`, or `{"seam": a}` where a closed
  surface meets itself), the exact `curve` (`line` with `start` and `end`; `circle` with
  `center`, `axis`, `radius` and the `angle` it runs counter-clockwise about the axis; `other`),
  the corners at its `ends`, and its `path`: the mesh vertices along it from start to end, the
  weld's own (the triangle sides on the edge, exactly).
- **Corners.** The B-rep's vertices: named after the faces that meet there (`~k` when repeated),
  with the mesh vertex at the point and the edges that end there.
- **Features.** The timeline's modelling features in order (sketches and construction geometry
  left out), suppressed ones flagged. `kind` gives the kinds analysis tools reason about with
  their key dimensions: `extrude` (`operation`, `sketch` with its `frame` and profiles of
  `line`/`arc`/`circle`/`other` curves, `direction`, `distance`, `two_sided`, `targets`),
  `revolve` (`angle`), `sweep`, `loft`, `box` (`corner`, `size`), `cylinder` (`base`, `axis`,
  `radius`, `height`), `sphere`, `torus`, `hole` (`position`, `direction`, `diameter`, `depth`),
  `combine` (`target`, `tools`), `fillet` (`radius`), `chamfer` (`distance`), `shell`
  (`thickness`), `split`, `pattern` (copy `transforms`), `mirror` (`transform`); any other kind
  is `other`, named by `kind_name`. `inputs` lists every numeric input with its label and typed
  value. `bodies` names the snapshot's bodies the feature made faces of or names.
- **Edits.** Each edit with the features it became and the bodies it cut: `suppress` (the
  feature), `partition` (one split per body the plane crosses; a body it misses is left as it is;
  the cut faces are `F<id>:section`), `split_face` (the pieces are `<face>#[<neighbour>]`).
  Suppressing a feature that makes a body takes the body away; one a later feature needs fails
  the snapshot.
- **Attributes.** Key/value maps on bodies and faces for downstream semantics (ports, boundary
  conditions). Designs have no place to store them yet, so they are empty.

## Compatibility

- Fields are only added, never renamed, retyped or removed. A field added later has a default, so
  readers of any version read any snapshot: missing fields read as their defaults, unknown fields
  are ignored.
- A kind a reader does not know (surface, curve, feature, property, parameter source) reads as
  `other`.
- `schema` goes up only for a breaking change; a reader refuses a schema above its own.

## Limits and determinism

The design file is at most 256 MB; a snapshot takes at most 1,024 overrides (finite values up to
10¹², expressions up to 4,096 bytes) and 1,024 edits (names up to 256 bytes), and holds at most
100,000 bodies, 1,000,000 faces, 20,000,000 triangles and 10,000,000 NURBS control points. A
relative tolerance lies in [10⁻⁶, 0.1], an absolute one in [0.001, 10,000] mm. The same design
and options give the same bytes: fixed body, face, edge, corner and triangle order, sorted maps
and shortest round-trip numbers (-0 written as 0).
