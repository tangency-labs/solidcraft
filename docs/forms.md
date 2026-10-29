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
4. Use Insert Edge, Insert Point or Refine Selected Faces for local detail.
   Topology and alignment tools have their own Apply/Cancel preview.
5. Choose **Finish Form** to save the editable feature, or **Cancel Form**
   to discard the editing session. Edit the saved timeline feature to reopen it.

Undo/Redo during editing acts on Form actions. A staged tool can be cancelled
without accepting its preview; finishing publishes the Form to document history.
Weld/unweld can stage companion Forms, which are published only when finishing.

## Geometry and source references

Local refinement retains logical leaf identities and sparse details over
nested uniform subdivision. It is not a fully adaptive T-spline evaluator.
CAD conversion uses fitted NURBS patches and compatible boundaries; fitting
checks are sampled residuals, not continuous error guarantees. Failed or
unsupported operations must report their outcome and retain the source design.

Match retains named CAD face or closed-wire references and supports G0, G1
and bounded G2 checks. Form Surface retains a source face's boundary and edits
interior guides with G0/G1 continuity, optionally partitioning the result into
selectable CAD cells. Form Pipe follows sketch curves or CAD edges, with
per-chain overrides, closed loops and supported junctions. Sculpting a pipe
freezes its cage until explicit regeneration replaces it.

Preferences on the Design page control greedy selection, mesh feedback while
dragging, the interactive fit tolerance, adjacent-face matching and the
editable Surface face grid. Saved features retain their own source data.

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
| `form.edit` | Reopen a saved Form feature |
| `form.blend.import`, `form.blend.export` | Blender interchange |

Consult each command's input/output schema for exact parameters and limits.
Blender import reads bounded mesh cages, units, supported object transforms,
creases and supported Mirror/Subdivision modifiers. It does not run Blender
or execute scripts in the file. Unsupported objects are reported. Cage export
retains editable quads and subdivision settings. Export with `surface: true`
writes evaluated CAD triangle meshes without a subdivision modifier; evaluating
an active Form for export does not finish or mutate the live editor.

## Known gaps and review boundaries

- Match combined with symmetry or hierarchical refinement is not supported.
- Some topology operations require a base cage; hierarchical extrusion is
  not supported. Unsupported combinations return errors.
- General coincident-wall Join/Cut and live Boolean previews remain incomplete.
  A fallback retaining a separate Form is not a successful combined solid.
- Blender shape-key evaluation and additional transform/modifier cases remain
  incomplete. The importer is not a replacement for Blender's evaluator.
- Browser compilation has been checked; full browser interaction and draft
  recovery acceptance remain outstanding.

The implementation includes kernel changes and needs an explicit upstream
scope decision while kernel work is paused. The author's AstoCAD Forms source
was consulted as a behavioral reference with their authorization. The Rust
implementation and UI are original; this history does not satisfy a policy
that prohibits reading that reference, and needs maintainer agreement before
the draft can be accepted.
