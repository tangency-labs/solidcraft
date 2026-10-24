# Phased render and material integration

The native GPU image path is described in [Progressive product images](render-path.md).
Optional [CPU progressive camera sampling](render-cpu-progressive.md)
through its existing CPU executor and shared image-job protocol.

Tracked in #842, with parent render design #620 and physical/visual material roadmap
#922. The combined integration records retained merge ancestry and scoped donor
adaptations separately in the [source ledger](render-materials-lab.md). It is an
alternative review path, not evidence that individual PRs have merged. Each result in GitHub
is pinned to its tested source revision. Do not reuse an older green result for a new
merge or describe compiled wasm as verified browser interaction.

## Delivery phases

| Phase | Contribution / deliverable | Acceptance gate |
|---|---|---|
| 1 | R2 #667, R3 #706, R4a #800: document-owned setups, product viewport, bounded CPU PNG jobs | Setup migration/save/reopen/undo, document and job isolation, strict capabilities, cancellation, native evidence and repository gates |
| 2 | Merged HDR input #837 and surface reference #1019 | Bounded codec/BRDF tests; unchanged existing CPU capabilities until actual consumers exist |
| 3 | Opaque product PBR #1046; engineering-card persistence #1060 | Explicit supported optical fields, source/condition preservation, one-step undo, cold reopening, unchanged geometry and independent assigned density |
| 4 | Merged scalar catalogs #863; richer transport #1086; inspector/import #1094 | Exact selected revisions, copied definitions, hostile-input bounds, no lossy downgrade, scoped file access, reviewed import and actual native evidence |
| 5 | Environment consumer #813, then texture/wood mapping #1004/#786 using resource admission #909 | Actual linear/sRGB interpretation and lighting, persistent object-local frames, cancellation/memory bounds, transformed instances, separate native/browser evidence |
| 6 | Family-specific optical/data quality under #922 | Wood L/R/T and moisture/grade conditions; polymer grade/process/temperature/rate; glass thickness/IOR/absorption; metal alloy/temper/conductor response and brushed anisotropy; source/license review before shipping data |

The original setup and opaque-PBR integration introduced format 21. The current
document version is derived from `doc::format` and its pending descriptors; that
historical number is not the combined branch's current format. Saved setups
migrate from older formats with older-reader refusal. The Design viewport
retains upstream display-plan tessellation; Render uses its separate prepared
product meshes. Numerical intent and embedded HDR sources are saved, while
runtime jobs and device handles are not. See the versioned contracts rather
than reserving another format independently.

## Raster foundation and current image paths

The GPU product **viewport** and bounded CPU product raster **PNG** executor
remain available. The raster [fixed studio consumer](render-opaque-pbr.md) provides
direct lighting, opt-in opaque PBR and approximate sorted transparency; it does
not add HDRI, cast shadows, transmission or indirect bounces. Those raster limits
do not describe the separate CPU raytrace and GPU path consumers now integrated
in the personal branch. Their illumination, certified transmission and progressive
sampling contracts are recorded in the [branch capability table](render-materials-lab.md),
[CPU progressive contract](render-cpu-progressive.md) and [HDR resources](render-hdr-resources.md).
The Radiance utility alone prepares linear RGB; supported consumers apply the
explicit color policy and lighting. Imported data or saved intent never silently
adds an unsupported capability or EXR output. Unsupported requests fail and
preserve the last valid image and CAD state.
See [backends](render-backends.md), [settings](render-settings.md),
[workspace](render-workspace.md), [jobs](render-jobs.md),
[HDR input](render-hdr-input.md) and [surface reference](render-surface.md).

Both atomic PNG publication modes check the existing caller file scope. The
control-channel regression rejects outside folders and traversal and permits a
valid destination, including explicit overwrite. This guard and document-adoption
regression now belong to standalone R4a, so its safety does not depend on this
combined integration. Subsequent CPU/GPU image paths and the Render workspace
retain the shared job ownership and file-access services, including the async
picker described below.

## Picker integration before release

The merged upstream #1127 service (`file_picker`) replaces the earlier #1114
candidate in this branch. There is one picker service, provided by main. Its future
returns a selected path or cancellation; the event loop continues drawing/resizing
while model input, tab switching/closing and control engine commands wait.
The compatibility inspection flag `file_dialog_open` mirrors `file_picker_busy`
and exposes no paths. Render and library callbacks capture overwrite consent and
retain explicit job/editor/document identity. An evicted image fails instead of
substituting a newer result. Cancellation writes nothing. Source/host changes that
replace the requesting document refuse the export; a replaced editor cannot receive
an old library response. Controlled future regressions cover these boundaries.

Earlier native responsiveness evidence and tab-routing tests used #1114's candidate
and keep their original pins. They are not proof of the new hold-input policy.
The upstream service's own tests cover held input and resize frames; add native
review evidence before describing the refreshed product workflow as GUI-verified.

## Material, appearance and resource ownership

Physical properties, assigned density, visual appearances and environment lighting
are distinct. A card-only edit does not replace assigned density or visual style;
applying an appearance does not change mass. Missing engineering values stay unknown,
with per-property sources and measurement conditions. Numerical consistency is not
certification or a structural solver. Mechanical axes are not inferred from a texture.

The first scalar catalog remains usable without optical/engineering extensions.
Richer transport must preserve copied PBR/card records and reject lossy downgrade;
reopening a design must not require the source catalog. Keep source revisions,
conflicts and applicability explicit. All inspection/application uses existing engine
commands; programmatic calls never open pickers. Library export edits the reusable
file rather than the CAD design. Applying a definition is one undoable design edit.

Use original synthetic fixtures first. A licence string in imported data is a source
claim, not redistribution permission. Any shipped dataset or texture needs actual
source/license review and attribution. Commercial reference artwork is not copied.
These material and resource contracts do not imply a certified material database
or simulation solver.

## Verification and collaboration

Run `cargo xtask ci` before each committed arc. Record local/remote results, pinned
upstream, actual platform and failures separately. Do not weaken geometry baselines,
mask warnings or infer whole-app performance from cached capture/cancellation times.
Attach actual app/PNG evidence for visible behavior; label older evidence with its
source revision instead of relabeling it as current. Use scoped original examples.

The private `plan/` documents are unavailable in this checkout; no content is
invented. Public contracts and current instructions govern this work. Propose bounded
interfaces in GitHub where a handoff is missing, invite feedback, and check existing
claims/PRs before implementing. Keep independent contributor branches untouched.
