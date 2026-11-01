# Persistent naming: what a second implementation suggests

Status: proposal, not adopted. Nothing here is built in SolidCraft. Related: #4341, #5198, #5308,
#5857, #5858, #5860, #5861, #23.

This note comes from building persistent naming for Form Studio, a T-spline modeling tool with its
own feature timeline, rollback marker, suppression, and editing of earlier features. Its kernel is
a control mesh, not a B-rep, so its entities are the vertices, edges and faces of a cage. A
feature's picks still have to survive the same upstream edits SolidCraft's do.

Form Studio is LGPL-2.1. Rule 2 of CONTRIBUTING.md says not to read it, so this note links none of
its code and contains none of it. What follows is method and measured results, written by the
code's author and offered under MIT OR Apache-2.0. No commercial CAD was driven or read for any of
it.

## What SolidCraft already gets right

The hard parts are done. Names follow the history each kernel operation reports (#136), not
sampled geometry. Faces are named by their role in the feature that made them (`F2:side:l4`).
Pieces are told apart by the faces around them, a merged face still answers to its old names, and
a feature says when a reference was split or found only by geometry. Form Studio follows the same
rules.

The failures still open sit in three places: what happens when a name is gone, how a name is
written down, and where an ordinal comes from. Six changes follow, the most harmful failure first.

## 1. When a name is gone, fall back by lineage, never by geometry alone

SolidCraft already has this rule for one kind of reference. A sketch's link to a projected edge
whose name is gone "keeps its last geometry ... it does not take whichever edge is nearest now"
(`docs/book/src/dev/naming.md`). Blends don't follow it yet. Read at 49c0faa, not run:

- A blend edge whose name no longer resolves falls back to "the edge at the picked point"
  (`crates/doc/src/eval.rs:4128`).
- A blend with no stored body takes the body with the nearest edge (`blend_target`,
  `eval.rs:3951`), and `named_body` (`eval.rs:3997`) ends with the nearest candidate.

Either path can put a fillet on an edge of an unrelated body. That is #4341: suppress Extrude1, or
move Sketch1 500 mm, and Chamfer1 lands on Body2. The warning is honest, but the geometry is
wrong, and every later feature builds on it.

Form Studio resolves a stored reference with the first of these four steps that finds something:

1. **Exact.** The entity with that name.
2. **Pieces.** The name is gone because an earlier feature split the entity. The result is every
   live piece its history leads to, including pieces that were split again (see change 5).
3. **Lineage guess.** Nothing was split from it, but its maker rebuilt it differently, for example
   a subdivision changed from 2 × 2 to 3 × 3. A candidate must come from the same feature and
   share either the role or at least one origin with the stored reference. Scoring starts at 4 for
   the same feature, then adds 2 for the same role, 2 for each shared origin and 1 for the same
   ordinal. Distance to the stored point only breaks ties. The result is one entity, never every
   candidate that matches.
4. **Lost.** No candidate. The pick resolves to nothing, the feature says so, and its other picks
   still apply. A feature with every pick lost is in error, and the document passes through it
   unchanged.

What step 3 leaves out matters most: no entity made by another feature can stand in, however close
it is. In SolidCraft terms, a stored reference keeps one level of what its name was made from
(feature, role, the names of its origins, ordinal). After the name is lost, the lookup searches the
rebuilt faces that feature made. It doesn't search space. Change 3 describes where that recipe is
kept.

The stored point stays useful in three ways: it breaks ties, it raises the existing "found by
geometry, not exact history; check it" note, and it is the only key for references saved without a
name. It should never choose a body or an edge on its own. Freezing a lost reference at its last
geometry, as sketches already do, is fine, because nothing new gets picked.

**Measured in Form Studio,** before names existed, when picks were raw indices (2026-10-05, 3
scripted histories, 18 features):

| Test | Result |
|---|---|
| Replay from the start | byte-identical to every recorded state |
| Edit earlier features' numbers, topology unchanged | all 33 later features on the same entities |
| Insert a subdivision upstream | of 60 later features: 0 kept exactly, 19 kept by position, 25 refused, 16 ran on a different entity with no error |

The 16 silent wrong results came from indices that now pointed at other entities. An index, like a
picked point, records where something was, not what it was. After names were built, six rebuild
tests pin the rules down. All six pass today:

- An unchanged rebuild finds every pick exactly.
- A subdivision inserted before a move of the face it splits: the move takes every piece.
- A feature inserted elsewhere leaves every later name unchanged.
- Deleting the feature that made a picked face leaves the later feature in error, with the
  document passing through it.
- A subdivision changed from 2 × 2 to 3 × 3: a move of its corner piece moves the new corner piece.
- A loop inserted before a crease of the edge it cuts: both halves are creased.

We have not re-run the 60-feature insert test since names were built.

## 2. A suppressed maker is not a lost reference

When the feature that made a referenced face or edge is suppressed, or sits past the rollback
marker, the reference isn't lost. It is waiting. Form Studio reads the maker from the feature id
in the stored recipe:

- **Maker suppressed.** The pick stays on the dependent feature and is shown as suppressed. That
  feature is not in error, and nothing is re-pointed. Unsuppress the maker and the pick resolves
  exactly again.
- **Maker deleted.** The pick is shown as missing. The user can clear missing references or pick
  again.

Neither case falls back to geometry. SolidCraft names already start with their maker (`F2:...`),
so it can tell these cases apart with no new data. #4341's first repro (suppress Extrude1) is this
case.

## 3. Store a name as data, and keep the text for people

#5857 and #5858 are problems with the text form: where a nested name's suffix belongs, and an id
that contains `|`. #5861 plans a better grammar for both. The text form has one more hazard, read
from source and not run. `later_suffixes` (`crates/doc/src/persistent_name/parser.rs:74`) decides
which face a piece qualifier belongs to by comparing feature numbers (`*f > feature`). Feature
numbers follow creation order (`Document::next_id`), and `timeline.reorder` moves a feature
without renumbering it. After a reorder, the same text can therefore be read the other way.

Form Studio never parses a name. This would take two steps here, each useful on its own:

1. **Save `FaceName` and `EdgeName` as structured values, not text.** `persistent_name.rs`
   already reads names into a tree; the design file would keep that tree instead of a string. A
   tree has no ambiguous suffix (#5857), a curve id is a field and can hold `|` (#5858), and no
   rule has to compare feature numbers. #5861's migration then happens once: read old text with
   today's rules and write the tree. The parenthesized text becomes the display form for
   `model.faces`, MCP and messages. A command given that text looks it up in the live model, and
   nothing parses text back into an identity.
2. **Make every name one level deep.** In Form Studio, each entity's name is a fixed-length token
   derived from a one-level recipe: feature, kind, role, origins (the tokens of its parents) and
   ordinal. The recipe stays in the body's name table while the entity lives. A stored reference
   keeps the token, the recipe and the picked point. Depth lives in the table: a piece of a piece
   of a blend is a chain of one-level records. A stored reference is then the same size however
   many splits came before it (#5860; the `MAX_NAME_LEN` comment notes that a face engraved with
   text passes 4 KiB today). The recipe is also the data change 1's lineage guess needs.

Form Studio uses a 53-bit hash and redraws on a collision within a mesh. A Rust version needs a
hash whose output is fixed across platforms and releases (not `std`'s `DefaultHasher`), and 64
bits or more.

## 4. Ordinals come from the feature, never from the kernel's face order

In #5198 the same design names a face `F3:bottom#1` on one kernel and `F3:bottom#2` on the other,
so the reference picks the other cap. The `#k` that breaks a remaining tie follows the kernel's
face order (`naming.md`: "else in kernel order and then unverified"). We suggest two rules:

- **No two faces of one feature share a role and origins.** When two do, the role table is too
  coarse. An extrude's two caps are `start` and `end`, whichever way it points.
- **A real tie gets its ordinal from the feature's own definition.** For example, a closed
  curve's wall split by a seam is ordered by the curve parameter where each piece starts, and
  caps along a direction by their distance along it. The order a kernel returns faces in is
  never used.

In Form Studio the ordinal counts what one edit made with the same feature, role and origins, in
the tool's own construction order. Form Studio has only one kernel, so we set this rule in advance
and never saw it fail. A gate for it: build each design in the naming tests on both kernels and
compare every name. The kernel switch (#3992, #4012) needs that gate, and it lives in the
document and engine tests, not in `crates/kernel/`.

## 5. A split pick resolves to all its pieces

When a named face or edge was split, SolidCraft takes the piece nearest the stored point
(`name_lookup.rs`; `locate_edge`: "the piece of it nearest `near`"). Form Studio takes every
piece. The user picked the whole entity, and after a split the whole entity is its pieces:

- A fillet on an edge that an upstream slot now interrupts should round both remaining runs.
- A shell that opened a face should open both halves.

A consumer that needs exactly one entity, such as a sketch plane or a joint, takes the piece
nearest the stored point from that list and says so, as it does today.

This changes what some saved designs rebuild to, so it raises the `Changes a default` question
(see Questions below).

## 6. A body's identity is not its name

In #5308, references to a body use its display name. Features already keep the two apart: a
minted id (`F12`) and a name the user edits. Bodies can do the same, and bluecmd's plan on #5308
does it: an id minted from `next_id` and stored with the feature slot by slot, with the name
becoming a label. That is better than deriving the identity from the creating feature and the
lump's place, as an earlier version of this note proposed, because a stored id survives later
changes to how lumps and faces are named. Form Studio does the same for features: the id is
minted once and kept through renames, and a rename never rewrites a reference.

A stored id is only as stable as the step that decides which lump fills which slot. Today the
pieces of a split body are assigned in the order of their least face name
(`crates/doc/src/lineage.rs:554` at 33281cf), and text order says nothing about which piece is the
original. Suppose an upstream edit makes a cut slice a small piece off a body, and that piece's
names sort first. The piece then takes the body's id, and with it the material and appearance.
Assigning slots by lineage keeps the id on the main piece:

- Each face contributes its own name and the names of the faces it was split from.
- When a slot is pinned, the feature stores a MinHash signature of that set beside `body_names`.
- On evaluation, each slot takes the lump whose signature overlaps its own the most.

The details are in the
[#5308 discussion](https://github.com/tangency-labs/solidcraft/issues/5308#issuecomment-6470264793).

## Measuring it

Classify every reference resolution the engine performs: exact, merged, pieces, lineage guess,
geometry only, or lost. "Geometry only" is the class that can be silently wrong. Count it in the
naming tests (`crates/engine/src/naming_history_tests.rs`, from #23) and in the action-matrix
corpus, and ratchet it down to zero for references that have a name. The count changes no
behavior, so it can land first and show what each later change buys.

## Order of work

Each step is a small PR on one topic:

1. Classify and count resolutions. No behavior change.
2. Suppressed versus missing (change 2; #4341, first repro).
3. Lineage fallback for blend edges, then for face references (change 1; #4341, second repro).
4. Structured names, together with #5861's step 2 (change 3).
5. Cap roles and ordinals from the feature (change 4; #5198).
6. Body identity (change 6), which bluecmd plans on #5308, then slots assigned by lineage.

Steps 3, 4 and 6 save something new in a design, so each adds a file under
`crates/doc/src/format/pending/`. Steps 3 and 5 change what some saved designs rebuild to.

## Questions for maintainers

1. Where today's point fallback lands on some edge, a lost reference would apply nowhere and the
   feature would report it. Is that acceptable for saved designs, or should it sit behind a
   preference?
2. Should names be saved as structured values before #5861's step 2, or instead of it?
3. Should a split pick resolve to all its pieces by default?

## Background

- J. Kripac, "A mechanism for persistently naming topological entities in history-based
  parametric solid models", *Computer-Aided Design* 29(2), 1997.
- V. Capoyleas, X. Chen, C. M. Hoffmann, "Generic naming in generative, constraint-based design",
  *Computer-Aided Design* 28(1), 1996.
- D. Marcheix, G. Pierra, "A survey of the persistent naming problem", *Proceedings of the
  Seventh ACM Symposium on Solid Modeling and Applications*, 2002.

## What was read and what was run

SolidCraft was read at 49c0faa (2026-10-28 UTC). It was not built or run, so the mechanisms given for
#4341 and for reordering come from reading the source. Form Studio's six rebuild tests were run on
2026-10-27 and all pass. The insert measurement is from 2026-10-05, before names existed.
