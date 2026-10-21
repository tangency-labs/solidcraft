# Native B-rep, schema 4

This is SolidCraft's geometry data model in `solidcraft_geom::brep`. It has no
modelling-backend dependency. It is the body encoding of a design's model cache
([the container format](design-container.md#model-cache)); it is not a STEP
replacement for external interchange. The container reader, binary codec and
kernel adapters are separate layers. The truck adapter writes it exactly: a body
decoded by the producer that encoded it is that body, number for number.

The records follow SMLib's flat form, `SmBrepData`. It stores one record per
logical use: a kernel keeps two mated halves of each face use, loop use and edge
use at run time, and it rebuilds them from the one record. The types take STEP's
B-rep names. Each type's documentation names its SMLib record and STEP entity.

## Units and numbers

Coordinates are IEEE-754 binary64, in millimetres, right-handed Z-up. Angles and
angular parameters are radians, always. (SMLib's STEP-style analytic parameters
are degrees; an SMLib adapter converts them, the pcurves on analytic faces
included.) References are unsigned 32-bit indices into the body's own lists,
never kernel addresses or runtime IDs. Tolerances are millimetres.

## Topology

| Record | SMLib | STEP | Holds |
|---|---|---|---|
| `Brep` | `SmBrep` | `MANIFOLD_SOLID_BREP`, `BREP_WITH_VOIDS`, `SHELL_BASED_SURFACE_MODEL` | tolerance, regions, shells, faces, edges, vertices |
| `Region` | `SmRegionData` | (implied by shells) | its shells, `void` |
| `Shell` | `SmShellData` | `CLOSED_SHELL`, `OPEN_SHELL`, `WIRE_SHELL`, `VERTEX_SHELL` | oriented faces and wire edges; or wire edges; or one vertex |
| `OrientedFace` | `SmFaceuseData` | `ORIENTED_FACE` | face, `forward` |
| `Face` | `SmFaceData` | `ADVANCED_FACE` | surface, domain, loops |
| `Loop` | `SmLoopData` | `FACE_BOUND` with `EDGE_LOOP` or `VERTEX_LOOP` | oriented edges, or one vertex |
| `OrientedEdge` | `SmEUData` | `ORIENTED_EDGE` and its `PCURVE` | edge, `forward`, pcurve |
| `Edge` | `SmEdgeData` | `EDGE_CURVE` | curve, interval, start and end vertex, tolerance, uses |
| `EdgeUseRef` | `SmEUData`'s radial fields | — | face, loop, position, `upper` |
| `Vertex` | `SmVertexData` | `VERTEX_POINT` | point, tolerance |

**Regions.** `regions[0]` is the infinite region. It has no outer shell and is
never a void. Each bounded region lists its outer shell first, a closed face
shell, then its inner shells. A bounded region is material unless `void`. A
closed shell separates two regions, so both list it: it is the outer shell of the
region it encloses and an inner shell of the region around it. An open shell, a
wire shell or a vertex shell lies in one region and only that one lists it. A
shell is closed when every edge its faces use is used at least twice among them.

**Shells.** A face shell lists oriented faces, and the wire edges embedded in it.
A wire shell is a connected graph of edges with no faces. A vertex shell is one
isolated point. One shell record stands for both runtime sides of a shell.

**Oriented faces.** A face is used by one oriented face, or by two in different
shells with opposite `forward`. `forward` means the face's normal in that shell is
its surface's natural normal, ∂S/∂u × ∂S/∂v. In a closed shell, normals point out
of the material.

**Loops.** A face's first loop is its outer loop and is an edge loop. A vertex
loop (a pole, a collapsed boundary) is only ever an inner loop. Loops are stored
relative to the surface's natural normal, whichever side a shell uses: seen from
that normal, the outer loop runs counter-clockwise and inner loops clockwise.
This lets one face sit in two shells without contradicting itself. An edge loop
is a closed chain: each oriented edge starts at the vertex where the one before
it ends, and the last ends where the first starts. An oriented edge runs from
its edge's start to its end when `forward`, otherwise from end to start.

**Edges.** An edge's `interval` is the part of its curve it covers, min < max.
Its `vertices` are at interval min and max; they are equal for a closed edge.
`uses` lists every oriented edge on it, each once: the primary use first (the
first one the canonical walk meets), then the rest in radial order, the
right-hand rule about the edge's direction. A wire edge has no uses. `upper` says
the radial cycle enters that use on the side of the surface's natural normal.
With loops stored relative to the natural normal it follows from the use:
`upper` is always `!forward` of the oriented edge it names.

**Tolerances.** The body, every edge and every vertex carry a tolerance, finite
and positive, never optional. The body's is SMLib's intersection tolerance.

## Canonical order

The lists are in one backend-independent order, and every adapter produces it. A
face's index in `faces` is its face index everywhere: in history, face sources,
paint and persistent names. The same holds for edges and vertices.

1. Regions: the infinite region, then bounded regions in the producer's order.
2. Shells and faces in order of first appearance, walking regions → shells →
   oriented faces.
3. Edges in order of first appearance, walking faces → loops (outer first) →
   oriented edges, then wire edges walking shells. A face whose first oriented
   face is reversed is walked backwards: each of its loops from its last
   oriented edge to its first. That is how a reversed face is traversed.
4. Vertices in order of first appearance, walking edges (start, then end), then
   vertex loops walking faces → loops, then vertex shells.

Nothing may be listed that the walk does not reach.

## Geometry

Curves and surfaces are unbounded. An edge's bounds are its `interval`, a face's
its loops inside its `domain`. Each kind's parameter is stated exactly. For the
kinds the truck adapter writes (`line`, `spline`, `rational_spline`,
`surface_intersection`, `plane_through`, `placed_revolution`) the parameter is
truck's own evaluation (`ParametricCurve::subs`, `ParametricSurface::subs`).
Analytic frames store an origin and orthonormal x and y axes; z = x × y.

### Curves

| Kind | Point at t | Range of t |
|---|---|---|
| `line` {start, end} | start + t (end − start): 0 at start, 1 at end | unbounded |
| `circle` {frame, radius} | origin + r (cos t x + sin t y) | periodic |
| `ellipse` {frame, radii} | origin + r₀ cos t x + r₁ sin t y | periodic |
| `polyline` {points} | segment i spans [i, i + 1] | [0, n − 1] |
| `spline` {knots, points} | B-spline on the full nondecreasing knot vector; degree = knots − points − 1 | [knots[degree], knots[count]] |
| `rational_spline` {knots, homogeneous} | the same basis, homogeneous `[x·w, y·w, z·w, w]` coefficients as the producer holds them | as `spline` |
| `surface_intersection` {surfaces, leader, tolerance} | the intersection of two surface records, traced from `leader` (a polyline or spline near it) to within `tolerance` mm; t is the leader's | the leader's |

A `surface_intersection` is the procedural curve itself, not a fit: another
kernel traces it again. Its point at t is the point of both surfaces in the plane
through the leader's point at t, normal to the leader there (truck's `subs`
converges to it within 10⁻⁶ mm).

### Surfaces

Natural normals are ∂S/∂u × ∂S/∂v.

| Kind | Point at (u, v) | u | v |
|---|---|---|---|
| `plane` {origin, u_axis, v_axis} | origin + u u_axis + v v_axis | unbounded | unbounded |
| `plane_through` {origin, u_point, v_point} | origin + u (u_point − origin) + v (v_point − origin) | unbounded | unbounded |
| `cylinder` {frame, radius} | origin + v z + r (cos u x + sin u y) | angle, periodic | height, unbounded |
| `cone` {frame, radius_at_origin, half_angle} | origin + v z + r(v) (cos u x + sin u y), r(v) = radius_at_origin + v tan(half_angle) | angle, periodic | height, on the side of the apex where r(v) ≥ 0 (a negative half angle narrows along z) |
| `sphere` {frame, radius} | origin + r (cos v (cos u x + sin u y) + sin v z) | longitude, periodic | latitude, [−π/2, π/2] |
| `torus` {frame, major_radius, minor_radius} | origin + (R + r cos v)(cos u x + sin u y) + r sin v z | longitude, periodic | meridian angle, periodic |
| `spline` {u_knots, v_knots, points} | tensor-product B-spline, `points[i][j]` with i along u | u knot domain | v knot domain |
| `rational_spline` {u_knots, v_knots, homogeneous} | the same, homogeneous coefficients | u knot domain | v knot domain |
| `placed_revolution` {profile, origin, axis, matrix, forward} | M · R(angle) · profile(s): the profile revolved about origin and unit axis by the right-hand rule, then mapped by the column-major affine matrix M | `forward`: s, the profile's parameter; else the angle | `forward`: the angle, [0, 2π); else s |

A `plane_through` is a plane given by three points, as its producer holds them;
writers use it rather than the axis form, whose axes would be rounded
differences of the points. A `placed_revolution` keeps the revolution exactly as
its producer holds it (truck's `RevolutedCurve` under a `Processor`): nothing is
baked into the profile, and `forward: false` swaps u and v, which reverses the
natural normal.

### Parameter ranges

An edge interval and each side of a face domain are finite with min < max. In a
periodic direction they may start anywhere and run up to one turn (2π), across
the seam. Otherwise they lie inside the natural range: a spline's knot domain, a
polyline's [0, n − 1], a sphere's latitudes, a cone on its open side of the apex.

### Pcurves

Every oriented edge carries its pcurve: the edge in its face's (u, v), as a 2D
B-spline (`Curve2`: `spline`, or `rational_spline` with `[u·w, v·w, w]`
coefficients). There is no polyline or analytic 2D kind. 2D lines and arcs are
exact as degree-1 and rational quadratic splines; anything else is a fitted
B-spline. A pcurve lies on the edge's own interval and runs from interval min to
max, whatever the oriented edge's direction. Its interval must equal the edge's
within 10⁻⁹ of the interval's magnitude (at least 1), room for a linear remap's
rounding. A seam edge has two uses on one face, each with its own pcurve, a
period apart. On a periodic surface a loop's pcurves are unwrapped, so each one
starts where the one before it ends.

Pcurves are SolidCraft's own, never a kernel's internal parameter space:
`brep::pcurve::pcurves` computes them from a face's surface record and each
use's curve record and interval, for every backend. It is exact in closed form
for lines, polylines and whole splines on planes, circles about the axis of a
cylinder, cone, sphere or torus (v fixed, u the circle's angle plus a constant)
and lines along a cylinder's or cone's ruling (u fixed, v linear), each checked
by lifting it; everything else is a cubic B-spline fitted within the tolerance.

## Validation and trust

`Brep::validate` checks structure and resources without a kernel. It caps 1,024
shells, 1,000,000 combined topology and geometry items, geometry nesting at 16,
spline degree at 16 and control points per spline direction at 4,096. Numbers
must be finite and at most 1e12 in magnitude. It checks knot order, active spline
domains, multiplicities, rectangular grids, positive rational weights,
nondegenerate planes, orthonormal frames, unit revolution axes, positive radii,
ring tori, cone dimensions and tolerances. Then the topology: every index in
range; the regions and shells as described above; each face used by one or two
oriented faces; first loops are edge loops and vertex loops are inner; loops are
closed chains through shared vertices; every edge's `uses` lists exactly its uses,
once each, primary first, with `upper` as stated; pcurve intervals; parameter
ranges; and the canonical order.

These are structural and resource guards, not a geometric validity certificate.
The validator does not check that edges lie on their surfaces, that pcurves lift
onto their edges, that a closed shell is watertight or that shells nest as the
regions say. A backend's adapter checks what it needs. The binary reader bounds
bytes, allocation and nesting before creating these structures.

`Brep::check_geometry` spot-checks, on a record `validate` accepted, that its
geometry agrees with its topology, each distance against the record's own
tolerance: every edge's curve meets its start and end vertices at its interval's
ends (within the edge's and the vertex's tolerances together); every oriented
edge's pcurve, lifted through its face's surface, lies on the edge's curve at the
interval's ends and middle (within the edge's) and starts on the same period as
the use before it ends, except along a direction that collapses at a pole; every
vertex loop's vertex lies on its face's surface (within the vertex's). It makes a
fixed number of evaluations per record, so its work is linear in the item
budget, and a point it cannot evaluate is an error naming the record. It is not
a certificate either: between the points it checks, an edge may still leave its
surface. The truck adapter runs it before building topology, which truck does
without checks of its own.

Schema tests build and validate one minimal body of each form (an open sheet, a
closed shell, a seam edge, a vertex loop with a wire and a vertex shell, a void)
and one fixture per defect, without a kernel.

## History

The runtime history types (`FaceOrigin`, `Change`, `FaceProv`, `Side`,
`EdgeOrigin`, `EdgeProv`, `History`) live in `solidcraft_geom::brep::history`, so
a backend can report history without the kernel crate. Indices are canonical face
and edge indices; operands are numbered by each operation. An operation returns
history beside the B-rep. A backend that cannot report it returns none and never
guesses. The saved form is the envelope's own records (below).

Face-source certificates, operation history, appearances and persistent document
identities belong to a versioned SolidCraft envelope around this geometry. They
must be preserved by the cache codec, not reconstructed from face counts or
floating-point comparisons. A decoded cache remains provisional until
authoritative re-evaluation finishes. The document and original blobs remain
sufficient to reopen when a cache, representation, producer or adapter is
unsupported.

## Body binary envelope, version 5

`SCBREP05` is the eight-byte envelope signature. Geometry schema 4 is nested in
`solid`; metadata schema 1 is nested in `metadata`; `runtime` holds what the body
measured about itself that later operations read (its size, from which kernel
tolerances follow, and the orientation mesh's missing-face count), so a decoded
body works to the same tolerances. These are separate version numbers because
each contract can evolve independently. Signatures 01 and 02 were development
prototypes; 02 introduced the explicit geometry schema, 03 separated metadata
from runtime Rust types, 04 made the encoding exact and 05 carries schema 4.
Unsupported versions are cache misses, not design-file migration failures: a
design whose cache holds an older signature opens and is evaluated again.

The envelope uses its own explicitly versioned records for color, paint, authored
boundaries, face-source certificates, operation history and boundary protection.
Plane, region and segment records are also owned by the wire schema. Exhaustive
manual adapter conversions separate these from runtime FaceSource, History,
FaceProv, EdgeProv, Paint, Seg2, Region2 and Plane types. Their serde derives or
internal refactors cannot silently redefine the saved fields. No truck serde
struct, pointer, tessellation or STEP string is written. Changing field semantics
requires a format version change, even if a Rust type keeps its name.

Values use tags: null=0, false=1, true=2, signed integer=3, unsigned integer=4,
binary64=5, UTF-8 string=6, array=7, object=8. Numbers use eight little-endian
bytes. Strings and collections use unsigned 32-bit little-endian lengths; object
keys are length-prefixed UTF-8, emitted lexicographically. Readers reject duplicate
keys, unknown tags, non-finite numbers and trailing bytes. Limits per body are
256 MiB, 20,000,000 values, depth 48, 1,000,000 entries per collection and 1 MiB per
string; a design's cache bounds the total (see the container format).

## The truck adapter

Truck keeps a face's wires relative to its surface and flips the face with an
orientation flag. That is schema 4's convention: the adapter writes truck's wires
as the loops and the flag as `forward`, with no wire reversed. It numbers faces
in `face_iter` order and edges in first appearance through `edge_iter`, which
walks a reversed face backwards, so the record's indices are the kernel's.

It writes every curve and surface as truck holds it: lines, B-spline and NURBS
coefficients, the three points of a plane, a revolution's profile, axis, matrix
and orientation, and an intersection's two surfaces, leader and tolerance. An
edge's interval is its truck curve's parameter range. Pcurves come from
`brep::pcurve` on the records just written; a face's domain is the
box around its pcurves' control points, clamped to the surface's natural domain
and at most one turn in a periodic direction. Every tolerance is the body's face
tolerance, 10⁻⁶ of its size and at least 10⁻⁵ mm, how far later operations take
its faces to be from its edges.

Regions come from truck's shells. Open shells lie in the infinite region. One
closed shell bounds one material region. With several closed shells, a
tessellated nesting test (the one `Body::lumps` uses) finds which bound material
and which voids, and what lies inside what. If truck's shell order is not the
canonical one (a void listed before another lump), or open and several closed
shells mix, the adapter refuses and the body is a cache miss. So it does for an
edge with more than two uses, whose radial order would need geometry.

Decoding builds the same structures without the constructors that would round
them (normalizing an axis, composing a matrix with the identity), and rebuilds
each shell's topology as recorded, including closed edges on one vertex and seam
edges a wire uses twice. It ignores pcurves, regions and domains, which truck
does not keep. It refuses what truck cannot hold: wire and vertex shells, wire
edges in a face shell, vertex loops, a face used by two shells, standalone
polylines, unclamped spline domains and a spline or intersection edge on part of
its curve. It loads circles and ellipses as exact rational arcs over the edge's
interval, and analytic surfaces as revolutions, which truck parameterizes its
own way.

Hashes prove which document and payload a cache belongs to, not who wrote it: the
container's key ties a cache to the producer, application build and platform that
computed it, and the decoder applies the structural and resource checks above to
every body. A second backend needs its own tested adapter before cross-kernel
interoperability can be claimed.

The box-and-fillet golden JSON fixture pins the decoded version-5 envelope,
field names, tags, topology, geometry and metadata. Integer and structural values
are exact; floating coefficients allow 1e-10 relative/absolute platform rounding.
The signature is asserted separately. `NATIVE_BREP_BLESS=1` rewrites the fixture
for an intended change.

## Bounded provisional display

`native_preview_mesh` samples at most 32 points along each curved edge, caps each
face's boundary at 4,096 points, and bounds spline controls and nested surfaces
before parameter searches. Searches use at most 16 refinement trials. Revolution
profiles involving procedural intersections are refused in this display path.
The separate mesher uses a fixed grid of at most 16 subdivisions per direction
and fixed periodic-seam connections; it never invokes adaptive edge, surface or
aspect-ratio refinement. Planes use a single grid cell.

Before projection, the caller reserves a conservative triangle bound covering
duplicated projected boundary points, periodic seams and the grid for the complete
body. Output is checked against that reservation and for finite coordinates.
Missing faces or exceeded budgets cause a cache miss. This mesh is approximate,
read-only display; normal modelling/export tessellation is unchanged.
