# Working with individual instances

Each occurrence placement has a full path of occurrence IDs, from the design root to that placement. A child definition reused under several parent instances has a different full path in each parent. Names are labels; paths identify placements across rename and position capture.

Select a placement through `select.set`:

```json
{"items":[{"type":"occurrence","path":[2,4]}]}
```

Properties accepts the leaf occurrence ID and the full path. Move can use world coordinates, including a world rotation pivot:

```json
{"occurrence":4,"path":[2,4],"space":"world","translate":[10,0,0],"capture":false}
```

A drag in the viewport, or `occurrence.drag`, moves one exact placement and its floating joint chain by a world translation:

```json
{"path":[2,4],"at":[0,0,10],"point":[20,0,10]}
```

It captures that placement's position (other pending positions stay pending) and is one undo step. With the design active, the viewport drags the placement on the picked body's path just inside the design; with a component placement active, the one just inside it. Grounded placements and placements that joints hold to something fixed are refused; a free chain of joints moves together, and a chain through a placement used several times is refused. The design's first top-level part is grounded when it is made (not when a design is opened); copies start free.

World movement includes pending parent and child positions. The default space remains parent-local for existing scripts. Capture Position commits pending transforms; Revert Position drops them. Copying an occurrence copies that selected occurrence's effective transform, including its pending position.

An occurrence definition inside a component is shared by every instance of that parent component. Moving, grounding, copying, or deleting that nested definition changes all its parent instances. Selecting its exact path determines the world frame used for movement and Properties; it does not create an independent child definition.

Activate a placement to author shared geometry through its frame:

```json
{"component":3,"occurrence":4,"path":[2,4]}
```

New geometry belongs to the component and appears in every instance. The selected frame determines how world inputs become local component geometry. Planar sketches and new 3D wires display at the active placement during authoring. Component-only activation keeps the first-placement behavior. Format 10 stores an explicit World or Component coordinate marker for 3D wire samples and fit points. Older drawn and detached wires retain their world positions; retained Include and surface producer links keep their component meaning, including lost references. Breaking a link preserves its explicit marker. Activate the desired occurrence before supplying world inputs to an explicit sketch owned by a repeated, nonactive component.

Visibility is an independent view setting for exact placements. Hide a path and all descendants with `browser.visibility`:

```json
{"visible":false,"paths":[[2,4]]}
```

`visible:true` removes that path's own hide setting. A hidden ancestor still hides its descendants. Add `isolate:true` to a visible path list to hide other branches. Show the bodies folder to clear occurrence hides. Selection and activation paths are session state. Visibility is saved with the design (hidden bodies, placements, sketches and origin items) and is an undo step of its own, so undo brings a hidden body or placement back; the saved document also retains occurrence IDs and transforms. Undo follows the existing behavior of clearing selection.

Paths are checked for missing links, repeated IDs, component mismatches and a maximum depth of 64. Descendant expansion rejects more than 100,000 placements or bodies instead of returning incomplete results. Computed body names normally retain their readable occurrence labels; collisions use full occurrence IDs so visibility and body commands still address the correct source.

Placed bodies retain their definition identity separately from display names. Body materials and appearances remain shared, with body overrides taking precedence over component overrides and material colors. Face overrides follow the placed geometry, including pending positions, and painted exports retain these colors.

Paste New copies the reachable component definition graph and its feature history. Repeated children stay shared within the new copy, while the copied definitions, sketches and generated dimension/input parameters are independent of the originals. Materials and body, component and face appearances follow the copied definition identities. Pattern and mirror output names remain explicit, so later body operations follow the copied bodies.

Configuration columns, row values and internal rules are duplicated for copied targets. Certified native face appearances follow the new feature lineage. Selected cylindrical Emboss target and authoring paths inside the copied graph follow the copied occurrences; an incompatible external authoring context is rejected before adoption.

A body reassigned from an external component receives an independent BaseFeature before the first copied feature that modifies it. Copied modifiers then replay once. The command reports these frozen bodies. Public user parameters remain design-wide; retained external sketch, project and feature links are reported. Joints and motion records wholly inside the copied occurrence graph receive new references. Records crossing its boundary are omitted with notices.

Copy evaluates on a scratch document, checks that original bodies remain unchanged, and verifies copied body measurements and sampled material occupancy before adopting it. Unsupported cross-boundary destructive operations, configured foreign faces without certified semantic lineage, reassigned mesh bodies, rolled-back timelines and exceeded work limits return errors without a partial copy. A successful copy selects the new occurrence, preserves your active component and unrelated pending positions, and places the copy under the chosen parent occurrence path. Activate the new occurrence to author features in it. Paste New preserves each 3D wire's World or Component marker and stored coordinates: Component wires follow the copied placement, while World wires retain their physical world positions.

Create Components from Bodies and Move Bodies to Component now add ownership-transfer features at the current timeline position. Earlier joins, cuts and face selections keep their original component frames. At the transfer, geometry converts once into the destination frame; later features authored there operate on the transferred body. Rolling the timeline back before the transfer displays the original owner. Body materials and face identity survive the transfer, and an inherited source-component appearance becomes a body override to preserve its visible look.

Transfers preserve the body's world pose through rotated or translated parents. Pending positions on the involved source and destination paths are captured atomically with the transfer so save/reopen retains that pose; unrelated pending positions remain pending. Undo and checkpoint restoration recover the original pending positions as well as the feature history; redo restores the captured transfer. The operation requires one source and one destination placement. Shared instance definitions with several placements are rejected because a single ownership change cannot preserve several different world poses. A component that supplies transfer history outside its subtree cannot be deleted until that dependent history is removed.

When the bodies of a new-body Extrude or Revolve are transferred into their source sketch's component through the same frame, the profile uses that recorded frame. Moving the component and capturing its position then moves the bodies once, matching the preview, even when the source sketch was moved into the component separately. Other cross-component sketch references continue to follow their live placements.

Older files without transfer features keep their saved assignment and offset behavior. No transfer timestamp is inferred from their final ownership map. Paste New freezes externally authored geometry after its successful entry transfer and remaps internal transfers, then replays later modifiers once. All transfer commands validate and evaluate on a scratch document before adoption.

New face-attached sketches capture the selected body's producer, certified face and exact occurrence path, together with the sketch's authoring placement. A supported face follows source edits; a missing or incompatible source reports a lost attachment rather than choosing another nearby face. Opening Edit restores the saved authoring occurrence as view context without creating an undo entry. Older saved point-only face references retain their legacy behavior.

`sketch.create` accepts `plane:{face:[x,y,z],body:"world body name",index:faceIndex,path:[occurrenceIDs]}`. Coordinates and custom planes are world inputs. The optional path must match the selected body. Point-only face picks must resolve uniquely to a supported exact attachment. For an unsupported planar face, explicitly add `fixed:true` to keep its current plane; this creates no live boundary projection. Curved faces cannot become planar attachments through this option.

Use `sketch.redefine` with the sketch ID and a new plane to repair or convert an attachment. Its preview is disposable; Apply records one undo step. An optional `author_path` explicitly repairs a missing author placement. Rebinding a repeated owner from another active component requires that path. Changing the target or converting to a fixed plane detaches only the old automatic face borders, retaining their currently evaluated geometry and entity IDs; unrelated projection links remain live. Pending positions on the involved target and author paths are captured with the edit, while unrelated pending positions remain pending.

Paste New remaps internal certified targets and authoring paths to the copied definitions. Timeline paste reports retained external face targets and refuses translating such attachments until an explicit fixed-plane conversion establishes independent geometry. Rename preserves certified body identity; another producer using the same body name cannot repair a lost attachment.

## Move a component's origin

A component's origin is the frame everything in it is written in, and the point its placements
are measured and turned about. An imported part keeps the origin its file had, often a corner
or a point in mid-air. `component.set_origin` moves the origin, and turns its axes if asked,
without moving anything in the world:

```json
{"component": "chair", "point": "bottom"}
{"component": 3, "path": [2, 3], "point": [120, 40, 0]}
{"component": 3, "frame": {"origin": [120, 40, 0], "x": [0, 1, 0], "z": [0, 0, 1]}}
```

- `point` is a world point, or `center` / `bottom`: the middle of the box around the
  placement's bodies (with its child placements' bodies), or the middle of its bottom (lowest
  world Z). `frame` also sets the axes: `z`, then `x` laid square to it. An axis left out keeps
  the component's current one. With `space: "component"` the inputs are in the component's
  current coordinates.
- `path` picks the placement the inputs are read in. The default is the component's first.
- Every placement of the component moves by the change, and everything it holds moves by the
  opposite amount, so nothing moves in the world. That covers bodies, imported meshes, sketches,
  construction geometry, joint origins on its placements, child placements, face attachments,
  canvases, and bodies it hands to or takes from other components. References to the
  component's own origin planes, axes and point are written out first, so they stay where they
  are. Move features, which turn about the origin, are re-expressed about the new one.
- A STEP import's bodies can't be rewritten, because they are the file's text. A box can't be
  either when the axes turn, because a box is always square to its component's axes. Each one
  gets a Move feature named *Origin Moved* right after it. The reply's `inserted` lists these.
- The change is worked out on a copy and compared with the design: every body, sketch plane,
  construction item and other placement must be where it was, and no feature may gain an error.
  Otherwise the command is refused and nothing changes. It is one undo step. Nothing new is
  saved, so the design opens in older releases.
- The command is refused while the timeline is rolled back, while components have moves that
  are not captured, for a linked component, and for a standard part (resizing rebuilds it).
- `keep_world: false` moves the placements only, so the contents go with the origin.

In the app, the Move panel of a placement (the Browser's **Move**) has **Move origin only**.
With it ticked, the distances, angles and triad move the origin instead of the part, and the
part stays where it is. OK puts the origin where the triad is.

## Floating joint chains

Dragging any member of a floating joint chain carries the chain together without changing its
joint values. A chain joined to a grounded part still uses `joint.reach` within its joints.
Repeated nested definitions are shared: their other instances follow in their respective
parent frames. The selected exact path determines the local change, which is carried through
the solver's first-placement frame to jointed peers, including repeated peers.

## Change the size of a standard component

An inserted standard part is one component with an active catalogue configuration.
Its Browser entry shows that configuration alongside its placement name, for example
`Screw:1 [M6×20]`. Double-click the entry or choose **Edit** to open its configuration
dialog. The activation radio still activates the component; double-clicking ordinary
components keeps its existing activation behavior. The viewport also offers
**Change Part Size** for a picked standard screw, bolt, nut, washer, pin or bearing.
The dialog starts with the
component's current family, size and actual length, including a non-catalogue
length. Choose a size and, for parts with a length, a length, then confirm with OK.
Cancel leaves the design unchanged.

Resizing regenerates the standard part, replacing any extra features added inside
that component. The dialog warns about this before applying the change.

This edits the component definition: every occurrence of that component receives
the new size. Its occurrences and their placements and joints are retained. Undo
reverts the size change in one step. Insert another standard part when a separate
component with a different size is needed.

The dialog uses the same `parts.library` catalogue and `parts.resize` command as
scripts. It is offered only for inserted standard parts. Other component types
retain their usual feature-editing tools.

The configuration is read from the existing standard-part family, size and length
metadata, so it survives save/reopen and follows undo/redo without renaming any
occurrence. Lengths in the compact configuration name are catalogue millimetres;
the dialog displays lengths in the design's units. General parameter-driven
configurations are outside this change.
