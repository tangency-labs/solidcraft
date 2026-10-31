# Live-link meshes

Version 1 (2026-10). Format tag `solidcraft.live-mesh`.

A program that follows the open design (a modelling or rendering program kept in step with
SolidCraft) needs each body as polygons it can edit: welded, with one polygon per planar face
where it can be, quads where the surface was meshed as a grid, the B-rep face of every polygon
and the B-rep edges marked. The `live.meshes` command sends them in this form
([control protocol](../control-protocol.md#live-link)).

## How a body becomes polygons

The body's display tessellation is joined face by face (`crates/geom/src/poly_mesh.rs`):

- A planar face whose triangles have one boundary loop is one polygon (an n-gon) through its
  boundary points. A planar face with holes keeps its triangles, joined into quads where they
  form good ones.
- On every other face, two triangles that share an edge become a quad when the quad is convex,
  folded by at most 30° and no corner is more than 45° from a right angle. The cells of grids
  and strips (cylinders, cones, blends, revolved faces) come back as quads; points such as a
  sphere's poles stay triangles.
- Points along a straight edge between two such polygons are dropped (a box is eight corners),
  B-rep vertices are kept.
- Vertices are shared across faces: a closed body gives a closed mesh. Each corner keeps the
  normal of its own face's surface, so creases stay sharp and curved faces shade smoothly
  however coarse the mesh.

`topology: "quads"` skips the n-gons and `topology: "triangles"` joins nothing.

## The object

```json
{
  "format": "solidcraft.live-mesh",
  "version": 1,
  "key": "3f0c9a7e5b1d42c8a6e0f9d1b2c3a4e5",
  "origin": [50.0, 25.0, 10.0],
  "counts": {"vertices": 8, "polygons": 6, "loops": 24, "edges": 12},
  "positions": "<base64>",
  "poly_sizes": "<base64>",
  "loops": "<base64>",
  "loop_normals": "<base64>",
  "poly_face": "<base64>",
  "edges": "<base64>",
  "edge_flags": "<base64>",
  "edge_brep": "<base64>",
  "face_names": ["F2:top", "F2:bottom", "…"],
  "stats": {"triangles": 0, "quads": 6, "ngons": 0, "open_edges": 0}
}
```

Every array is standard base64 (RFC 4648, with padding) of little-endian values:

| Field | Type | Length | Meaning |
|---|---|---|---|
| `positions` | f32 | 3 × vertices | x, y, z in millimetres, **relative to `origin`** (add it back) |
| `poly_sizes` | u32 | polygons | corners of each polygon (3 or more) |
| `loops` | u32 | loops | vertex of each corner, polygon after polygon, counter-clockwise seen from outside |
| `loop_normals` | f32 | 3 × loops | unit normal of each corner (absent when asked for without normals) |
| `poly_face` | u32 | polygons | B-rep face index of each polygon |
| `edges` | u32 | 2 × edges | segments of B-rep edges as vertex pairs, smaller first, each once |
| `edge_flags` | u8 | edges | bits: 1 B-rep edge (always), 2 sharp (the faces meet at an angle), 4 tangent (their normals along it differ by at most 1°), 8 seam within one surface |
| `edge_brep` | u32 | edges | B-rep edge index of each segment |

`face_names` gives each B-rep face's persistent name, by face index: unlike the index, a name
stays with its face when the design is rebuilt, so a reader can carry per-face work (UVs,
materials) across changes. `key` names the content (it is a hash of the arrays, the face names and
the options the mesh was made with). Two meshes with the same key are the same, so a reader keeps
what it has and asks only for keys it lacks. Units are millimetres and Z is up.

A reader refuses a `version` it does not know. Later versions may add fields; a reader ignores
fields it does not know.
