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
primitive selector offers Box, Cylinder, Quadball, Sphere, Pipe, Plane, Torus
and Tube.

The Edit Form panel opens with the primitive controls expanded. Enable
**Compact Form editor** in Design preferences to put selection and transforms
first and focus the panel on the active tool. In that layout, open **Primitive**
to replace the starting shape, **Primitive dimensions** to set its dimensions
and segments, or **CAD diagnostics** to run a CAD check.

1. Choose the primitive's dimensions and segment counts before sculpting.
   Primitive settings lock after sculpting; the transform tools remain available.
2. Use the All, Points, Edges or Faces selection filter. Ctrl-click toggles
   an element; Shift-click extends a range through connected elements.
   Clicking a filter button clears the selection and custom pivot. Enable
   **Keep selection when changing Form filters** in Design preferences to keep
   both. The selection summary counts selected points, edges, faces and whole
   surfaces independently of the filter.
   Double-click an edge to select its continuous loop. Connected and edge-loop
   selection are also available in the panel.
   On desktop, drag from empty space for a selection window, or enable Lasso
   for a freehand boundary. Shift adds; Ctrl+Shift toggles the region result.
   Window selection requires the complete element inside the boundary;
   crossing selection takes intersecting elements. Hidden elements are excluded.
3. Pull the selection directly or use the Move/Rotate and Scale handles.
   The transform mode buttons are next to the selection filters.
   Global, View and Selection orientations, numeric transforms and a custom
   pivot are available. Selection orientation uses the current fitted Form
   charts. For a group, the last directly picked member sets the axes.
   Pick a member to set the axes after a bulk selection. A face uses the U tangent at its logical
   centre. An edge uses its directed tangent at the middle of its logical
   parameter, and a point uses its first qualifying outgoing edge tangent.
   Edge and point normals combine the incident chart normals. Form Surface
   uses the directed native guide tangent and agreed chart normals. A Surface
   cell needs one owned native face with a certified convex UV boundary.
   Whole unpartitioned Surfaces, merged faces and singular or conflicting
   charts may have no Selection orientation. The axes stay fixed until the
   drag ends.
   Shift-drag a face or free boundary edge to extrude.
   On a refined Form, the cap keeps its face identity and the new walls can
   be refined. A mirrored face moves with its partner. If they share a seam,
   the drag follows the mirror plane to keep the seam joined.
   Enable **Soft selection** to influence nearby connected points with a radius
   and falloff. Pinned source rims remain fixed. **Snap translations** quantises
   movement in the gesture's axis or plane; its increment is frozen until release.
   The configured manipulator bypass modifier temporarily disables snapping.
   The numeric **Extrude Faces** and **Extrude Edges** actions use the same
   evaluated cap or rim displacement as Shift-drag. Each accepted action creates
   one Undo step; a failed action preserves the source, selection and pivot.
   The viewport readout shows the submitted move, angle or scale request, with
   separate pending, accepted and CAD-fitting states. Symmetry, pinned points
   and soft selection can make point motion differ from the request. Disable
   **Show Form transform values while dragging** in Design preferences to hide it.
   Desktop drags on refined forms process the latest transform target in the
   background. Release accepts the final target and Escape cancels the gesture.
4. Use Insert Edge, Insert Point or Refine Selected Faces for local detail.
   The panel shows the active insertion tool's options until you cancel it.
   Tools with an Apply/Cancel preview stage their result before accepting it.
   Delete faces and Dissolve edge act immediately and can be undone.
   With Insert Edge active, **Whole edge loop** previews a midpoint cut on the
   hovered face. Press **M** to switch between the face's U and V axes. On refined
   forms the loop can continue through aligned T-junctions and existing fine
   seams. Compatible mirrored strips insert together. Clicking accepts the
   preview as one transaction, and Undo restores the previous hierarchy.
5. Choose **Finish Form** to save the editable feature, or **Cancel Form**
   to discard the editing session. Edit the saved timeline feature to reopen it.

The topology section includes Fill Hole, Erase and Fill, Bridge, Collapse,
Weld, Unweld, Thicken, creases and uniform subdivision. Fill Hole offers
Automatic, Single quad, Minimal quads and Reduced star on base cages. Single
quad requires a four-edge opening. On a refined Form, select a rim edge or
point and use Automatic to restore a deleted region and its sculpted detail.
Single quad restores one four-edge leaf. In the desktop app, choose **New cap**
to create a new surface on one four-sided opening. Apply keeps one Undo step,
and Cancel restores the previous Form and selection. An independent Form can
hold up to eight caps on disjoint rims. Each cap follows its rim and keeps one
selectable face ID when saved. Use box or lasso selection to select faces
on a capped Form. To click a cap face, enable **Use exact CAD face picking**
in Design preferences. Edges and points can also be clicked.
Shape the cap by editing its rim. To remove it, select only the cap face and
choose **Delete Faces** or press Delete. Undo restores the cap. Capping the
same rim again creates a new face ID. Direct cap face transforms remain
unavailable. Extrusion, Match and symmetry require a Form without a derived
cap. Bridge also requires a Form without retired cap faces. Scripts can use
`fill_mode: "new_cap"` or `tool: "delete_faces"` with the cap face ID in
`form.prototype.topology`.

On a refined Form, select the faces of one four-sided interior region and
choose **Erase and Fill**. Apply replaces that region with a new cap over the
exposed rim in one Undo step. Other openings and caps keep their state.
Cancel restores the original faces and selection. The underlying subdivision controls remain in
the evaluator to preserve the surrounding skin. This operation takes a single
region on an independent Form without symmetry, extrusion or a trim wall.

To **Collapse** a refined boundary side in the desktop editor, select both
free endpoints of one planar leaf side. The midpoint stays on the fitted
source. The new edges and triangular face use the original chart. The
opposite edge and neighboring leaves keep their identities and sculpted
detail. Preview, Apply and Cancel use the normal Form tool controls.
The first refined Collapse supports one triangular trim per Form. Both
endpoints must be exclusive to the same quad and its opposite edge must
retain one uncut neighbor. Subsequent edits must preserve the exact planar
trim. The original endpoint handles and ordinal face aliases are retired.

In the Form editor, Thicken offers Normal or Sharp construction and Reverse
for base cages. To thicken the fitted CAD surface, Finish the open Form and use **Surface
Thicken** on the fitted sheet. A negative thickness reverses the offset. The
result is a separate timeline feature and the Form stays editable. Symmetric
thickness grows half the distance on each side. For a coplanar Form with
spline boundaries, SolidCraft joins two exact skins with walls swept from
those boundary curves. When the direct construction on a curved refined Form
fails validation, SolidCraft joins two half-thickness walls and verifies that
both offset skins survive. The feature keeps its chosen construction when the
source changes, and refuses offsets that fold or produce an invalid solid.
The alignment section provides directed or best-fit Flatten and Straighten.
These tools can act on control points or points on the evaluated Form. Some
topology operations require a base cage; unsupported inputs leave the accepted
source intact.

Crease edits on refined forms follow the selected logical edge or point into
the active refinement. The operation is available on forms without Match or a
derived trim wall.

If a saved Match or Form Surface support no longer resolves, normal Edit refuses
it without changing the source. The UI offers **Open retained source**; scripts
use `form.repair {"feature": id}` explicitly. The retained cage and sculpted
offsets remain visible with the source error, while editing and Finish stay
blocked. Choose **Reattach support** to stage a replacement with Apply/Cancel.
For Surface, choose the U/V swap/reverse correspondence explicitly; guide counts
stay fixed and offsets retain their lengths in the chosen logical grid frames.
A qualified Apply makes the repaired source editable; Undo restores the retained
unresolved state. Cancel leaves the saved feature unchanged. Recovery autosaves
retain that unresolved state and its history, never an unaccepted replacement.

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

To join two saved refined Forms, choose **Surface → Forms → Weld Refined
Forms**. Pick both bodies in the active component. Each must have one
coincident original opening. The preview checks the entire joined rim.
Apply creates one Weld in the timeline and keeps both source Forms editable.
The first body supplies the joined body's name. To separate the sources,
right-click the Weld in the timeline and choose **Remove Refined Form Weld**.

To split one closed independent refined Form, select a complete separating
seam or a face region whose boundary is the seam, then choose **Unweld**.
Preview shows two open pieces. Each keeps its subdivision controls,
sculpted detail and surviving face identities. Apply stages both pieces,
and Finish saves them at the same fit tolerance. Cancel discards the split.
Detached pieces exclude the sibling's material from selection and automatic
Fill Hole. Use **New cap** to close a qualifying rim. Refined Unweld is a
desktop tool and requires one complete native edge per logical seam side.

## Geometry and source references

Local refinement retains logical leaf identities and sparse details over
nested uniform subdivision. It is not a fully adaptive T-spline evaluator.
CAD conversion uses fitted NURBS patches and compatible boundaries; fitting
checks are sampled residuals, not continuous error guarantees. Failed or
unsupported operations must report their outcome and retain the source design.

The CAD check evaluates the active Form without publishing it.
Its report separates fitting, record validation and the geometry backend's
validation. The report covers the active Form before Add/Subtract/Intersect;
staged companion Forms are reported as unchecked. It does not certify continuous
fitting error or global freedom from self-intersections. Reports are discarded
when their source or settings change. Conversion errors and fit summaries stay
visible above the scrolling tool settings.

In the browser, Check Form CAD runs in a background worker for independent
Forms within its source and refinement limits. It checks the saved refinement
and extrusion steps using the same CAD conversion as the desktop command.
Use the desktop CAD check for a Form with retained CAD or sketch supports.

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
The Forms region-selection preference keeps the previous empty-space drag
behaviour available when disabled. Region queries run beside the desktop window
and apply only if the source, selection and view still match their captured state.
An unsupported or ambiguous query reports an error and preserves the selection.

For **Add**, **Subtract** and **Intersect**, enable **Show combined result**
to preview the same CAD operation that Finish will evaluate. The panel reports
when the preview cannot produce a combined body. The browser computes this preview
in a worker for small independent Forms and designs. Use the desktop app for
larger results or Forms with retained support. A matched opening can join to a
concave or curved support when the mating plane separates both shapes.

## Commands and interchange

The UI calls the same typed engine commands as scripts. Commands never open
file pickers; their host UI supplies file paths. Useful entry points are:

| Command | Purpose |
| --- | --- |
| `form.prototype.create` | Start a primitive Form |
| `form.prototype.inspect` | Inspect the active editor |
| `form.prototype.validate` | Check the active Form without changing it |
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
- Mixed-depth whole-loop insertion follows stations on the subdivision grid.
  Off-grid cuts, some chart-corner transitions, dissolved seams and trim walls
  require additional topology support. Mirrored strips must use compatible
  stations and seam directions where they overlap. Refined Fill Hole and
  Erase and Fill add four-sided caps one opening at a time. The separate Weld
  command joins two saved refined Forms at coincident rims. Interior Collapse
  and the in-editor Weld and Thicken tools still require a base cage.
- Region selection requires complete exact face and guide mappings. Unsupported
  analytic routes, periodic seam ambiguities, near-plane intersections and
  exhausted work budgets return an error rather than a partial selection.
  Exact region selection runs on desktop. The browser's CAD check supports
  independent Forms within its worker limits.
- The opt-in exact CAD face picker uses the current fitted surface for
  face clicks and Insert Edge. Some trims, seams and overlapping surfaces
  require a different click point.
- Coincident-wall Cut supports contained planar contacts. Other contacts and
  general intersections still depend on the kernel's Boolean support. The
  editor reports a warning when it retains a separate Form after a failed
  operation.
- Blender interchange covers a subset of the evaluator. Animated keys and
  unsupported modifiers or transforms require a cage evaluated in Blender.
- Earlier browser checks covered primitive creation, surface selection, insertion,
  scaling, extrusion, draft recovery and source-only reopening on WebGPU and
  WebGL. Fresh checks are required after integration; those earlier cases do not
  qualify every topology or support combination.

The implementation includes kernel changes and needs an explicit upstream
scope decision while kernel work is paused. The author's AstoCAD Forms source
was consulted as a behavioral reference with their authorization. The Rust
implementation and UI are original; this history does not satisfy a policy
that prohibits reading that reference, and needs maintainer agreement before
the draft can be accepted.
