# OSD v2 mesh exchange (experimental)

Open or import `.osd` through **File → Import**, choosing the separate **OSD**
filter, or use `doc.open` /
`file.insert_mesh`. Export selected bodies with **File → Export → OSD** or
`file.export` with `format: "osd"`. This is an independent Rust implementation;
it does not load a SprutCAM library, launch SprutCAM, or require its installation.
It uses the existing mesh import, component placement and export pipeline.

## What is preserved

- Triangle lists, strips and fans, including strip winding.
- The stream's affine placement, including reflections. Matrices replace the
  current placement; they are not accumulated.
- Flat RGB colour groups (see the solid reconstruction colour limitations below).
- Coordinates interpreted as millimetres. The stream has no supported unit tag.

By default, closed geometry is reconstructed as an **editable solid**, using the
existing prism converter where possible (0.001 mm tolerance), otherwise sewing
the triangles into planar B-rep faces. Solid operations and STEP export then work
without a separate Convert Mesh operation. This does not recreate original
sketches, dimensions, feature history, machining operations or assembly semantics;
curved regions can remain faceted, and operations such as fillets depend on that
topology and the kernel's capabilities. Save new editing history as `.sc`.

Reconstruction is bounded to 20,000 triangles per file. If geometry is open,
non-manifold, too detailed or cannot pass the reconstruction checks, **Auto keeps
the mesh and reports why**, rather than inventing missing faces. The `osd_mode`
parameter of `doc.open`, MCP `open` and `file.insert_mesh` selects `auto` (default),
`mesh` (the original import path) or `solid` (fail instead of falling back).
Mesh bodies can still be exported as OSD, STL, OBJ or 3MF.

In **Preferences → Design → OSD**, turn off **Import OSD as editable solids**
to restore mesh import for files opened or inserted through the UI. The control
channel names this preference `osd_import_solids`. An explicit `osd_mode` takes
precedence over the UI preference.

Disjoint, independently closed colour groups keep their body colours. When colour
groups split a shell or have overlapping bounds (including a differently coloured
cavity), they are joined for solid reconstruction so that voids remain empty;
individual face colours are then lost with a warning. Use `osd_mode: "mesh"` to
retain those colour groups. Sewing uses the kernel's existing 0.000001 mm weld;
the faceted result must remain closed and preserve enclosed volume within
max(0.000001 mm³, 0.00001%). These checks do not certify arbitrary self-intersecting
input as suitable for every CAD operation.

Display points/lines, wire passes and reference/snap records are omitted with
warnings. Zero-area triangles are skipped with a warning. Varying vertex colours
are reduced to the last vertex's colour with a warning. Unknown opcodes,
unsupported primitive modes, incomplete records, non-finite values and singular
or projective matrices fail the import instead of returning a partial model.

## Export limitations

The writer emits a canonical version 2 triangle stream with an identity placement,
world-space bounds and flat colours. Coordinates are float32; triangles that
collapse at that precision cause an error. Boolean edits can create such tiny
triangles even when the original OSD imported successfully; save the edited solid
as STEP or `.sc` in that case. Names, alpha, CAD topology, display
wires, reference records and the original byte representation are not preserved.
Body boundaries are lost: bodies sharing a colour merge into one mesh on reimport.
The 16 opaque bounding-box metadata bytes are written as zeros.

**Compatibility of generated files with SprutCAM has not been verified.** The
reader/writer roundtrip proves compatibility with this implementation, not with
every vendor reader. Treat OSD export as experimental and retain the `.sc` file.

Both directions cap files at 256 MiB and triangles at 1 million. Import also caps
input vertices at 3 million and distinct flat colours at 256. Counts are bounded
before allocation; the parser uses checked byte slices and validates indices.

## Empirical layout supported here

All numeric values are little-endian, with no alignment padding between records.
The header is a 101-byte short-string field: byte 0 is 43, followed by the ASCII
string ` The CAM: OpenGL stream file (version 2.00)` (including its leading space).
A uint32 version value of 2 follows at offset 101. Records start at offset 105;
each is a one-byte opcode followed by a fixed-size payload. EOF ends the stream.
Truncation at a complete record/primitive boundary cannot always be detected.

| Opcode | Payload bytes | Interpretation used by this implementation |
| --- | ---: | --- |
| 0 | 4 | uint32 primitive: 0–3 display-only, 4 triangles, 5 strip, 6 fan |
| 1 | 0 | End primitive |
| 2 | 12 | Three float32 normal components (validated; mesh normals recomputed) |
| 3 | 12 | Three float32 vertex coordinates |
| 4 | 12 | Three float32 RGB channels in 0…1 |
| 5 | 1 | Display pass: 1 solid, 0 wire |
| 6 | 64 | Six float64 bounds plus 16 opaque bytes |
| 7 | 128 | Sixteen float64 affine matrix values; translation at indices 12–14 |
| 8 | 48 | Six float64 reference values; skipped |
| 9 | 72 | Nine float64 reference values; skipped |
| 10 | 104 | Thirteen float64 reference values; skipped |

For matrix `m` and point `(x,y,z)`, the first output coordinate is
`x*m[0] + y*m[4] + z*m[8] + m[12]`; the next two use indices 1/5/9/13
and 2/6/10/14. The affine last row must be `(0,0,0,1)`.

## Provenance and review status

No public binary specification was located. This description is **empirically
inferred**, not an official or exhaustive format specification. The initial
investigation used user-supplied OSD files and static inspection of a vendor
library to understand record handling. A separate byte-analysis investigation
corroborated record sizes and stream structure on 25 files. No vendor API was
invoked for the implementation. No vendor code, binary, disassembly or user model
is included in this contribution. Tests construct original analytic fixtures.

This provenance does not satisfy the project's usual public-specification-only
approach by itself. Maintainer review and an explicit policy decision are needed
before this experimental contribution is eligible for upstream integration.

Tests cover analytic square areas and winding, reflected/scaled placement,
colours, omitted reference/display data, truncations, malformed values, mutations,
float32 export failures and the native mesh feature/export path. An analytic
non-prismatic pyramid tests direct solid import, cold document reopen, a Boolean
cut and STEP export/reimport; open and non-manifold inputs, colour seams and the
solid reconstruction budget are checked separately.
