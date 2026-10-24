# STEP import

SolidCraft reads STEP (ISO 10303-21; AP203, AP214 and AP242 geometry) with its own reader in `crates/kernel/src/step_in` (written from the public standards; no third-party STEP code). A STEP file becomes an **Import** base feature on the timeline: it holds the file's B-rep (the STEP text is kept in the design, so the design reopens without the file) and later features — fillets, holes, cuts sketched on its faces, shells — build on its bodies.

- **Open**: File → Open (`doc.open`) accepts `.step`/`.stp` in any case and starts a new design; `solidcraft part.step`; `solidcraft-cli eval part.step`, `solidcraft-cli run script.json --in part.step`; the MCP `open` tool.
- **Insert** into the current design: File → Insert STEP… or SOLID → INSERT (`file.insert_step {path}`).
- **Drag and drop** a STEP file on the window: inserted when the design has features, opened otherwise.

## Imports are components

Every import (STEP, IGES, 3MF, STL, OBJ; inserted, dropped, opened as a new design, or run as `file.insert_step`, `file.insert_mesh` or `doc.open` from a script, the CLI or MCP) becomes a **new component named after the file** (`bracket.step` → component `bracket`), placed in the active component at the file's origin. The Import feature belongs to it, so the file's bodies never land loose next to your own. Opening a file as a design makes the component too, so the browser shows the file's name. The command's reply reports it: `component` (id), `component_name` and `occurrence`.

A STEP **assembly** keeps its structure: each product under the file's root becomes a component nested under the import's, named after the product, at the place the file gives it. IGES groups and subfigures become nested components the same way. A part (or sub-assembly) the file places more than once becomes **one component with an occurrence per placement**: the Import feature builds the part once and skips the file's later copies (`skip` in the feature, format 22), and the occurrences show it everywhere else. Placements that are scaled, mirrored or otherwise non-rigid keep their geometry in place in the parent component. The reply adds `subcomponents` (how many components were made below the import's) and `rigid_joints`.

The Browser reuses body, sketch and construction-plane membership, child lists and resolved placements while the
camera moves. Large assemblies keep their tree open without resolving every part and
folder again on each frame. Geometry edits, assembly changes and uncaptured or reverted
positions refresh those queries; selection, visibility and locking are read live.

Hover picking pauses while the mouse moves the view and resumes when navigation stops.
A 3D mouse moving the view leaves it on: the pointer hand is free, so preselection and
clicks keep working while the other hand turns the view. Picking uses the current camera
and the cached body bounds directly in model order, so finding each body's bounds does not
scan the assembly again.

Hovering an assembly's Browser row retains its whole-body highlight buffers during
rotation, panning and zoom; the GPU transforms those buffers with the camera. The
origin and construction widgets resize independently. Whole-body
highlight generation scans each mesh once rather than scanning it again for every
face. Sketch profile feedback still updates its projected curves and lift toward
the eye as the view changes.

The contents of an imported assembly are **rigid by default**: rigid joints (a rigid group, its glyphs not drawn) hold each part where the file put it in its parent, so moving the import's component moves everything, and an inner part doesn't move on its own; joint degrees of freedom count the whole import as one body. To joint the parts one by one, release them with **Unlock Contents** on the component's browser menu (`component.unlock {component}`), one undo step. Undo takes a whole import away in one step.

Designs saved before imports made components keep their Import where it is (no component is made for it) and rebuild as they did.

Read: manifold solid breps (with voids), faceted breps, shell-based surface models; planes, cylinders, cones, spheres, tori, B-spline and rational B-spline surfaces (with knots, Bézier, uniform, quasi-uniform), surfaces of revolution and linear extrusion, trimmed surfaces; lines, circles, ellipses, hyperbolas and parabolas (as exact rational curves, which export writes back as exact rational B-splines), B-spline curves, polylines, trimmed/surface/seam curves; length and angle units (SI prefixes, conversion-based units such as inches); product names (meaningful solid names are kept, placeholders use part names; `names` below chooses); surface colours; assemblies (next-assembly-usage occurrences placed by item-defined transformations, and mapped items) — the product tree is kept in the feature and becomes the import's components (above). Files are untrusted: sizes, entity counts and nesting are capped and every kernel call is guarded. What cannot be read becomes a warning on the feature (a face left out gives an open body); not read yet: offset surfaces, pcurve-only edges.

Extruded circular and elliptical faces keep their profile's parameter seam outside
the face trim, or on an explicit seam edge. Narrow faces such as rounded gear tooth
tips therefore stay narrow through the full thickness instead of wrapping around
the supporting ellipse in the middle.

## Files from other CAD systems

Files from different systems differ in which way is up and in what they call their solids. `doc.open`, `file.insert_step` and the MCP `open` tool take two options for STEP and IGES files; without them a file comes in as written.

- **`up`**: `"z"` (the default) keeps the file's axes. Many systems write Y up, so such a part lies on its back in SolidCraft's Z-up space. `"up": "y"` turns the import's component a quarter turn about X (+90°: Y becomes Z), as the window's "Stand it up" offer does, so an assembly's parts and instances turn together. The import and the turn are one undo step, the reply names the turned occurrence as `stand_up`, and only the file's component turns, not what is already in the design. The window offers to stand a part up after a STEP file opens unless the call said which way is up (`up: "z"` as well as `"y"`) or the file came from SolidCraft, which already writes Z up. A STEP reply's `system` is the originating system the file's header names (`SolidCraft` for SolidCraft's own exports). A header that names SolidWorks (in FILE_NAME's originating system or preprocessor version, or in FILE_DESCRIPTION; for IGES, the start of the global section's native system ID, which becomes the originating system) says the file has Y up: the reply carries `exporter` (`SolidWorks`) and `exporter_up` (`y`), and the window's offer then says so ("Exported by SolidWorks, which usually saves Y up"). A file is never turned without asking, because the exporter may have saved it Z up; a component's right-click menu has Turn Up Axis (Y up → Z up, Z up → Y up: a quarter turn about X, one undo step) for fixing it any time. SolidCraft's own files say `z` and are left alone; any other exporter is unknown, so the offer stays.
- **`names`**: `"auto"` (the default) keeps meaningful solid names and replaces placeholders such as `Body1` or `Body 2 (3)` with the nearest named product, then the occurrence name if no product is named. `"solid"` keeps the solid's name; `"product"` chooses the part's name; `"instance"` chooses the assembly occurrence (`Post-2`), falling back to the product. If the chosen name is missing, the solid's name remains. Repeated names are numbered (`Post`, `Post (2)`). Names are stored with the Import feature and survive reopening the design. Files without names or assembly metadata retain generated body names and a flat structure.
- **Vertices off their curves**: some systems write an edge's vertex a little beside the end of its curve, often further off than the accuracy the file declares (`UNCERTAINTY_MEASURE_WITH_UNIT`). A vertex up to the file's own tolerance off, or up to 1/1000 of the model's size (never more than 1 mm), is read as that end of the edge: the curve's end is moved onto the vertex. A vertex further off leaves its face out, with a warning that says how far off it is.

```json
{"command": "file.insert_step", "params": {"path": "frame.step", "up": "y", "names": "instance"}}
```

## Imported appearances

Imported body colours and face colours are retained through later features. Body
colours remain the fallback under explicit design appearances and materials; face
colours follow persistent face names. A removed face has no colour assignment to
display on the resulting body.

## IGES

IGES 5.3 files (`.igs`/`.iges`) open, insert and drop the same way: `crates/kernel/src/iges_in.rs` (written from the published specification) restates the file as STEP and the STEP reader reads it, so the feature holds STEP text. Read: manifold solid B-reps (186) with their shells, faces, loops, edge and vertex lists; trimmed (144) and bounded (143) surfaces, and bare B-spline patches, sewn within their owning group into a solid by their shared edges (an open body when they do not close); groups (402 forms 1, 7, 14 and 15), subfigure definitions (308) and instances (408), retaining names, nesting and placements; lines, circular arcs, ellipses, composite curves, rational B-spline curves; planes (108, 190), cylinders, cones, spheres, tori, rational B-spline surfaces, surfaces of revolution and tabulated cylinders; transformation matrices; units; colours (314 and the predefined numbers); body names (name properties, else labels). Not read yet: ruled and offset surfaces, curves on surfaces given only in parameter space. Export (`file.export` to `.igs`) writes each body as a manifold solid B-rep on the same surfaces a STEP export uses, with its name and colour.

## FreeCAD and Rhino

FreeCAD `.FCStd` and Rhino `.3dm` files open, insert and drop the same way, in the desktop app, the browser app and the CLI. They are read with [cadmpeg](https://github.com/cadmpeg/cadmpeg) (Apache-2.0, `crates/io/src/native_cad.rs`): cadmpeg detects the format from the bytes, decodes it into its neutral model under resource caps, checks that model and writes it as STEP AP214, which the STEP reader then reads. The feature holds that STEP text, so the design reopens without the original file. The file's solids come in; its feature history, sketches and parameters do not. Losses cadmpeg reports become warnings on the feature, and a file without an importable solid is refused with a message. cadmpeg runs inside the panic guard. How much of a file is read follows cadmpeg's support for that format ([format support](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md)).

The workspace disables cadmpeg defaults and enables `fcstd`, `rhino` and the `step` writer on every host. Native builds additionally enable `sldprt` for the restricted, validated scope below. Other commercial codecs and the ShapeManager/ACIS `sat` codec stay disabled. SolidWorks import is available in the desktop app and CLI.

### Experimental SolidWorks part import

Desktop and CLI builds accept `.sldprt` through the same Open, Insert, drag-and-drop,
and script commands. The first supported scope is an analytic solid made of planes
and cylinders with embedded display geometry. Design-history editing,
unsupported surfaces and ambiguous boundaries are refused.

The Apache-2.0 cadmpeg reader supplies the B-rep. For a single active, unsuppressed
`Multi_CAD_Imported` feature, a matching single `LocalBodies` cache can supply the
geometry when the active partition is empty. Input limits and CRC checks apply
before the cache is used. Original bytes are never changed. Display points must
uniquely establish circular edge direction; longer arcs are retained when that is
what the evidence shows. Exterior boundaries are classified before STEP export.
Every face must be covered, the SolidCraft result must be closed with positive
volume, and its bounds must agree with the original display geometry. Its volume must also
agree with the faceted reference within 0.5%. A design
stores the resulting STEP, so reopening needs no original SLDPRT or commercial CAD
installation. Base RGB face colors are retained through AP214 and saved designs.
For the compact cached-part path, named `SDL/TYSA_COLOUR` attributes resolve explicit owner
face and RGB asset ids; conflicting assignments or missing RGB assets are refused.
Feature history and advanced appearances are not transferred.

### Experimental SolidWorks assembly import

Desktop and CLI builds also accept `.sldasm` with its referenced `.sldprt` files
in the same directory. Unpack an assembly ZIP first, then open the SLDASM. The
saved component tree supplies configuration identities and rigid placements;
repeated parts share geometry. Original absolute reference paths are ignored.
The imported design retains the resulting STEP assembly text and needs no source files
when reopened.

This path uses the schema-framed, pure Rust `parasolid-core` reader for the exact
`SCH_3501251_35102_13006` saved-solid profile. It reads analytic and NURBS surfaces,
source edge senses, tolerant surface-parameter boundaries, and material regions
with internal voids. Intersections without a supported exact conic form are
restated as cubic splines at a fraction of the source modelling resolution.
Metres become millimetres once; rotations and curve parameters keep their units.
Explicit body and face RGB assignments follow their source owner identities.
Assembly packaging preserves each validated part representation, including its
modelling uncertainty, internal voids and presentation styles, in an AP242 product tree.

A selected part partition must match the assembly's configuration number. An
empty saved partition may use a CRC-checked `LocalBodies` cache only for the exact,
unchanged imported feature. Source sheets are retained as shell-based surface models,
with their trimmed faces, placements and body/face RGB styles. They are not given
artificial thickness or converted to solids. The two sides of a source sheet do
not duplicate its faces. Closed surface-parameter boundaries without vertices are
supported when the source trim closes within tolerance. Loose sheet joins retain
the source tolerance (bounded to 1 mm); curve fitting remains capped at 0.00001 mm.
Wire-only bodies are reported and excluded. Every declared solid must still have
closed manifold shells;
missing parts, conflicting configurations, unsupported schemas or geometry, and
unresolved hidden or flexible occurrences are errors. A source regeneration flag
is reported when its saved static solid is used. Mates, feature history, journals,
textures and advanced appearances are not transferred. This is a validated saved
geometry subset, not general support for every SolidWorks version or assembly.
