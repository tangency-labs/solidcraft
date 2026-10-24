# Persistent naming

A fillet stores the edges it rounds, a sketch the face it sits on, a shell the faces it opens.
If those were face and edge *indices*, an upstream edit that adds a face would move the fillet
to another edge. SolidCraft stores **names**, and finds them again every time the timeline
evaluates.

## Why names follow the kernel's history

Names used to be worked out from geometry: sample points of each new face were looked up on the
faces of the feature's input, and the majority won; pieces of a split face were numbered by where
their centres lay. That guess is right most of the time, and wrong exactly when it matters: after
an upstream edit that splits, merges, mirrors or shrinks a face, points can land on a neighbour,
centres can swap order, and a fillet or a sketch quietly re-attaches to a different face. Nothing
tells the user, which is the worst thing a CAD tool can do.

The kernel now reports, for every operation, what each face and edge of its result came from
(`crates/kernel/src/history.rs`: a face's origins and how it changed, an edge's origin edge or
the faces whose meeting made it). Names follow that history, so they are exact and deterministic,
and when a reference can no longer be found exactly the feature says so instead of guessing.

## How a body's faces are named

`crates/doc/src/lineage.rs` composes the histories of the kernel operations a feature runs (a
tool built from a sketch, cut from each target, the cut body split into lumps) into one
**lineage** over operands that can be named:

| Operand | Its faces are named |
|---|---|
| a body of the model before the feature | as they were |
| a moved copy of one (pattern, mirror) | `<name>@F<feature>.<instance>` |
| the cavity side of a shell grown outward | `F<feature>:inner:<name>` |
| a body the feature built | by role: `F<feature>:side:<sketch curve>`, `start`/`top`/`bottom`/`end` caps, `+x`… box sides, `face:<k>` imported faces |
| a copied tool of a patterned feature | `F<feature>:<role>@F<pattern>.<instance>` |

`crates/doc/src/history_names.rs` names each face of the result from its origins: a kept or
trimmed face keeps its origin's name, a fillet or chamfer face is `F<feature>:blend:<edge>` of the
edge it replaces, a shell's inner wall `F<feature>:inner:<face>`. A side's sketch curve is the
curve the profile segment lies on exactly; nothing is sampled.

- **Pieces.** Faces that end up with one name are pieces of one face, and are told apart by the
  faces around them that the others lack: `F2:top#[F2:side:l4,F4:side:l4]` is the piece of the
  block's top bounded by its left wall and the slot's left wall. That survives the slot moving,
  growing or shrinking. Pieces nothing around them tells apart are numbered (`#1`, `#2`): by what
  they came from when that differs (the two halves of a circle's wall), else in kernel order and
  then unverified.
- **Merged faces.** A face made of several (a join flush with the body) takes the name of the face
  they are all pieces of, else the target's; it answers to the others' names too (aliases).
- **Edges** are named by the faces they separate, `<face A>|<face B>`, `~k` when several edges run
  between the same two faces (numbered by where they lie; a reference by such a number is
  reported when only geometry confirms it).
- **Lumps.** When a cut leaves a body in pieces, the whole is named first, so pieces of one face in
  different bodies are told apart, and the bodies (`Bar`, `Bar (1)`) are assigned in the order of
  their least face name, not the kernel's shell order (which an upstream edit can swap; see
  [Older designs](#older-designs)).

Names are worked out lazily, on first request, and shared by a body's clones.

### Where there is no history

A body made by an operation that does not report history yet (some kernel paths return none; see
the kernel's handoff lists, and the doc features that do not compose it yet: sheet metal, plastic
and surface features, emboss, rib, replace face, the faceted thread cut), or a face whose role
is not known exactly (a loft's sides, a coil's or pipe's generated profile, a hole's tool), is
named the old way, from geometry, and its names are marked **unverified** (`model.faces` reports
`exact: false`). A reference that resolves through an unverified name while its stored point no
longer lies on the face says so: `face ... was found by geometry, not exact history; check it`.

## How a reference finds its face again

`crates/doc/src/name_lookup.rs`, in order:

1. the face with that name;
2. the one face answering to it: merged into it (the feature says it was merged), or the named
   piece, which the faces listed in its name still bound;
3. the pieces of it (it was split, or it is on several bodies): the one nearest the reference's
   point, and the feature says it was split;
4. nothing: the feature falls back to the geometry it recorded and says the face no longer exists.

An edge name is found as the edge between the two faces it names, each found as above: when
exactly one edge runs between them it is that edge, even after a split of one of its faces.

A sketch's link to a projected edge (`sketch.project {edge: [x, y, z]}`, and Intersect and
Include 3D Geometry of an edge) keeps the edge's name beside its point. The link is that edge
wherever a change takes it; of an edge that was split it is the piece nearest the link's point,
and the sketch says so. When no edge has the name any more the link is lost and keeps its last
geometry, as it does for a face: it does not take whichever edge is nearest now. A link saved
without a name, by a build that kept only the point, is found by the edge nearest its point, as
it always was, and takes that edge's name when its sketch is next edited.

A command that takes a face as a point can be given a name `model.faces` lists instead
(`face.delete`, `solid.shell`, `solid.draft`, `solid.offset_face`, `solid.split_face`, the `faces`
of `solid.replace_face`, the `face` of `sheet.convert`). It is found with the same lookup
(`naming::locate_one`: the face with that name, or the one face answering to it, on the given
body or any body; a name that finds several faces is refused). The command takes a point on that
face (`naming::named_face_point`: the incentre of the face's roomiest triangle, moved onto the
surface of a plane, cylinder, cone, sphere or torus), which the feature stores like a picked one,
with the name it then gets as before. Nothing new is stored in the design. `model.faces` lists
that point for every face.

### Profiles picked by their curves

A profile picked by a point or an index is stored by the curve ids of its outer loop
(`ProfileSel::Curves`), so it follows dimension and parameter edits exactly. Editing the outline
itself can leave no profile bounded by exactly those curves; the feature then falls back to the
best match and warns, instead of failing and taking its body and everything after it along
(`profile_refs::fallback_loop`):

1. all the stored curves are still there and still close a loop on their own, but another curve
   now cuts the region: the pieces inside that loop that touch it, which together are the region
   that was picked (islands inside stay holes);
2. otherwise the profile whose outer loop shares the most of the stored curves, at least half of
   them (a side deleted and drawn again, a notch trimmed in); a tie goes to the one with the
   fewest curves the loop did not have, and a tie left after that is an error, as is a loop
   that matches nothing.

Loops of offset curves and legacy graph-span multisets keep their exact meaning. Commands still
check a pick with the exact rule, so a curve list that names no profile is refused when given.
A sketch on such an extrude's cap falls back the same way: its certificate names the loop, and
the cap of the same solid extrude at the same end, from the same sketch and on the same body,
that shares the most (at least half) of its curves stands for it, unless the exact cap is still
on another body (`Document::anchored_face_indices`).

A hole placed by a position on a face is the exception to "the point nearest the reference's
point": its position is a place, not a pick. The hole follows its face along its own axis, so it
moves with the face when the face moves, and when the face no longer reaches the position it stays
where it was placed and the feature says so, instead of sliding to the face's edge and drilling
half a hole there. (Holes at sketch points follow their sketch.)

## Older designs

Designs saved before format 14 stored names worked out from geometry, and a few of them name a
face that history names differently (a tube's outer wall was named after the sketch curve nearest
its centre, its inner circle). Migrating such a design marks those names (`?F2:top#2`), and they
are found among the names geometry gives, as the build that saved them found them: an older design
evaluates as it was saved. Picking the face again stores its exact name.

Their features also keep naming the pieces they split a body into in the kernel's shell order
(`Feature::shell_piece_order`, set by the migration): the body names stored after them (fillet and
chamfer bodies, targets, participants, materials, appearances, joints) mean the pieces in that
order. Features added later name their pieces after their faces.

## Not done yet

- Certificates (`FaceSource`, the strict attachment identities behind sketches on faces and
  configured face styles) still travel only where the kernel's planar-source mapping keeps them;
  carrying them through `History::face_sources` would let a certified attachment follow a face a
  later join reshapes, which today is refused on purpose.
- Operations without history, listed above, keep the unverified geometric names.
