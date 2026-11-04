# Forms workbench (draft)

Forms edits a low-resolution quad cage and evaluates a smooth Catmull–Clark
surface. The normal editing view shows selectable points, edges and faces on
the evaluated surface; the control cage is an optional display aid. Finishing
a Form fits CAD surfaces and retains the editable source in the timeline.

This implementation is experimental. It is not a claim of complete parity
with other workbenches or of general downstream kernel robustness.

## Try it

In the **Surface** tab, choose **Forms** to create a primitive, **Form Pipe**
for a sketch/edge path, or **Form Surface** for an existing CAD face. The
primitive selector offers Box, Cylinder, Quadball, Sphere, Pipe, Face, Torus
and Tube.

1. Choose the primitive's dimensions and segment counts before sculpting.
2. Use the All, Points, Edges or Faces selection filter. Shift adds to the
   selection; the panel also provides connected and edge-loop selection.
3. Pull the selection directly or use the Move/Rotate and Scale handles.
   Global, View and Selection orientations, numeric transforms and a custom
   pivot are available. Shift-drag a face or free boundary edge to extrude.
   On a refined Form, the cap keeps its face identity and the new walls can
   be refined. A mirrored face moves with its partner. If they share a seam,
   the drag follows the mirror plane to keep the seam joined.
4. Use Insert Edge, Insert Point or Refine Selected Faces for local detail.
   Topology and alignment tools have their own Apply/Cancel preview.
5. Choose **Finish Form** to save the editable feature, or **Cancel Form**
   to discard the editing session. Edit the saved timeline feature to reopen it.

Undo/Redo during editing acts on Form actions. A staged tool can be cancelled
without accepting its preview; finishing publishes the Form to document history.
Weld/unweld can stage companion Forms, which are published only when finishing.
Welding a saved Form transfers whole-body Move, Scale and Remove consumers to
the joined body. References to the consumed Form's faces, edges or vertices
need topology provenance that Weld does not yet retain. Finish refuses those
ambiguous references and keeps the document and editing session intact.
When autosave is enabled, recovery can reopen an unfinished Form with its
accepted edits, staged companion Forms and Finish settings. Apply a tool's
preview before relying on recovery to retain that change.

## Geometry and source references

Local refinement retains logical leaf identities and sparse details over
nested uniform subdivision. It is not a fully adaptive T-spline evaluator.
CAD conversion uses fitted NURBS patches and compatible boundaries; fitting
checks are sampled residuals, not continuous error guarantees. Failed or
unsupported operations must report their outcome and retain the source design.

Match retains named CAD face or closed-wire references and supports G0, G1
and bounded G2 checks. Compatible supports can be matched with symmetry and
hierarchical refinement enabled. Refining a matched opening preserves the
original support coordinates, and a candidate that fails its continuity
checks leaves the existing edit intact. Form Surface retains a source face's
boundary and edits interior guides with G0/G1 continuity, optionally
partitioning the result into selectable CAD cells. Form Pipe follows sketch
curves or CAD edges, with per-chain overrides, closed loops and supported
junctions. Sculpting a pipe freezes its cage until explicit regeneration
replaces it.

Preferences on the Design page control greedy selection, mesh feedback while
dragging, the interactive fit tolerance, adjacent-face matching and the
editable Surface face grid. Saved features retain their own source data.

For **Add**, **Subtract** and **Intersect**, enable **Show combined result**
to preview the same CAD operation that Finish will evaluate. The panel reports
when the preview cannot produce a combined body. A matched opening can join to
a concave or curved support when the mating plane separates both shapes.

## Commands and interchange

The UI calls the same typed engine commands as scripts. Commands never open
file pickers; their host UI supplies file paths. Useful entry points are:

| Command | Purpose |
| --- | --- |
| `form.prototype.create` | Start a primitive Form |
| `form.prototype.inspect` | Inspect the active editor |
| `form.prototype.refine` | Add local hierarchical leaves |
| `form.prototype.insert_edge`, `form.prototype.insert_points`, `form.prototype.insert_loop` | Insert topology |
| `form.prototype.match` | Associate an opening with CAD support |
| `form.surface`, `form.pipe` | Start source-driven Forms |
| `form.prototype.commit`, `form.prototype.close` | Finish or discard editing |
| `form.prototype.finish_options` | Stage the CAD tolerance and output operation |
| `form.edit` | Reopen a saved Form feature |
| `form.blend.import`, `form.blend.export` | Blender interchange |

Consult each command's input/output schema for exact parameters and limits.
Blender import reads bounded mesh cages, units, supported object transforms,
creases and supported Mirror/Subdivision modifiers. It bakes the saved state
of static relative or absolute shape keys into the editable cage before
applying modifiers. Animated keys and unsupported objects are reported.
Cage export retains editable quads and subdivision settings. Export with `surface: true`
writes evaluated CAD triangle meshes without a subdivision modifier; evaluating
an active Form for export does not finish or mutate the live editor.

## Known gaps and review boundaries

- Some topology operations require a base cage. Shift extrusion also supports
  trimmed boundary edges, mirrored free boundary edges and matched faces.
  Extrusion retains the matched opening; a candidate that fails the requested
  continuity checks leaves the previous Form intact. Unsupported combinations
  report an error instead of changing the source.
- Coincident-wall Cut supports contained planar contacts. Other contacts and
  general intersections still depend on the kernel's Boolean support. The
  editor reports a warning when it retains a separate Form after a failed
  operation.
- Blender interchange covers a subset of the evaluator. Animated keys and
  unsupported modifiers or transforms require a cage evaluated in Blender.
- Browser checks cover primitive creation, surface selection, insertion,
  scaling, extrusion, draft recovery and source-only reopening on WebGPU and
  WebGL. These cases do not qualify every topology or support combination.

The implementation includes kernel changes and needs an explicit upstream
scope decision while kernel work is paused. The author's AstoCAD Forms source
was consulted as a behavioral reference with their authorization. The Rust
implementation and UI are original; this history does not satisfy a policy
that prohibits reading that reference, and needs maintainer agreement before
the draft can be accepted.
