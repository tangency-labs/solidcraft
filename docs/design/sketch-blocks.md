# Sketch blocks: a set of sketch geometry that keeps its shape

Status: proposal for #4069 (and the DXF case in #22102), decisions settled. Nothing here is built. Read at
bbb925608b, not run.

## What the user gets

Select some sketch geometry and choose **Make Block**. The selection becomes one block. Its
curves keep their shape relative to each other, and the block as a whole can still move and turn.
The user then places the block with ordinary constraints, for example:

- **Position:** coincident between the block's origin, or any of its points, and other geometry.
- **Turn:** parallel, perpendicular or an angle between the block's axis, or any of its lines, and
  other geometry.
- **Size:** with **Scale: Free**, one length dimension sets the block's size and its proportions
  stay as they are.

A rigid block has 3 degrees of freedom (x, y, angle); a scale-free block has 4. When the
constraints take all of them, the block turns black like any fully constrained geometry. Dragging
any member moves the whole block rigidly.

**Insert DXF** and **Insert SVG** get an **Insert as block** checkbox. With it, the inserted
profile arrives as one block: no auto-constraining and no constraining point by point. For the
first case in #4069, a dovetail placed along an angled centre line, the user needs two
constraints: one coincident and one parallel.

**Explode Block** turns a block back into plain geometry. The constraints that held inside it
come back where they still hold. A turned or resized block can leave some that don't: those are
removed, and Explode reports them.

**Edit Contents** opens a block to change its curves in place without exploding it, and **Close
Block** keeps the new shape in the same block (see "Editing a block in place"). Outside Edit
Contents a block is picked whole: a click on any of its curves selects the block, never one curve
of it.

## Name

"Group" is taken twice already:

- The glossary uses it for Browser and timeline folders (`docs/book/src/reference/glossary.md:378`).
- Rigid Group is an assembly joint.

This proposal uses **block**. It is the word #22102 uses and the usual drafting term for a named
set of 2D geometry. A block here is not a shared definition with instances: copies are
independent (see phase 5 under "Phases"). The name needs a new glossary entry.

## How it is stored

A block is an id-keyed object on the sketch, like `Sketch.patterns`
(`crates/sketch/src/model.rs:876`, `crates/sketch/src/pattern.rs:77`). It is **not** a new
`ConstraintKind`:

```rust
pub struct SketchBlock {
    pub id: String,              // "k1", from Sketch::fresh
    pub name: Option<String>,
    pub curves: Vec<String>,     // member curve ids
    pub axis: String,            // id of the block's own construction line (origin → +x)
    pub scale: BlockScale,       // Fixed | Free
    pub length: f64,             // the axis length when the shape was captured (L0)
    /// Each member point: its id and its place in the block's frame, (u, v) in units of L0.
    pub points: Vec<(String, [f64; 2])>,
    /// Each member circle or ellipse: its id and its radii at capture, in units of L0.
    pub radii: Vec<(String, f64)>,
    /// The constraints wholly inside the block, kept but not solved while it exists.
    pub held: Vec<String>,
}
// Sketch gains
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub blocks: Vec<SketchBlock>,
```

Why a field and not a constraint kind:

- **Ids survive edits.** Members are kept by id, so they survive index shifts. A block is pruned the
  same way patterns are when members go (`pattern.rs:319-347`, called from `remove_curves`,
  `model.rs:2626`). A constraint that holds a list of indices would be dropped whole by `remap`
  (`model.rs:715-801`) as soon as one member was deleted.
- **Older releases still open the sketch.** An older release reading an unknown `ConstraintKind`
  tag turns the whole sketch feature into a `NewerFileFeature` placeholder
  (`crates/doc/src/newer_file.rs`). An unknown `#[serde(default)]` field is kept and written
  back, so the sketch opens and only the rigidity is missing.
- **Redundancy marks stay where they belong.** The solver blocks that come from a sketch block
  are not user constraints, so `block_constraint` maps them to `None`
  (`crates/sketch/src/redundancy.rs:35`). A redundancy mark then never names the block itself.

### The frame: the block's own axis line

The pose (x, y, angle, and scale when free) lives in two ordinary points: the start and end of
one **construction line** that Make Block adds. It is drawn dashed in the block's colour. Its
start is the **block origin**.

- **No new kind of unknown.** The solver's unknowns stay point coordinates and radii
  (`solver.rs:163-239`). The rank and floating-pin code, which assumes width-2 point units, is
  unchanged.
- **Something obvious to constrain.** Coincident on the origin places the block. Parallel or an
  angle on the axis turns it. In Free scale, a length dimension on the axis sizes it.
- **Members can come and go.** The frame never depends on which members remain, so deleting
  members never re-anchors the block.
- **Older releases see a plain construction line.**

Where the axis goes by default:

- **Make Block:** the origin goes at the selection's bounding-box centre and the axis runs along
  sketch +X. L0 is half the box diagonal, rounded to a tidy number. With a point selected, the
  origin goes on that point.
- **Insert as block:** the origin goes at the dialog's "Place by" anchor, and the axis follows the
  Angle field.

## Solving

### Equations

In `build_blocks` (`solver.rs:2786`), each sketch block adds its rows right after the curves' own
relations (`Closure`, `Arc`). With O and X the axis ends, d = X − O and n = perp(d):

| Block | Rows | Reads |
|---|---|---|
| `Frame { p, o, x, u, v }`, one per member point | `P − O − u·d − v·n = 0` (2 rows) | P, O, X |
| `FrameLength { o, x, l0 }`, Fixed only | `|d| − L0 = 0` (1 row) | O, X |
| `FrameRadius { r, o, x, k }`, per member circle or ellipse radius | Fixed: `r − k·L0 = 0`; Free: `r − k·|d| = 0` | r, O, X |

- **Counts.** N member points give 2N + 4 variables (2N + 1 radii) and 2N (+1) rows, so a rigid
  block has 3 degrees of freedom and a free one 4. That is what #4069 asks for, and it comes
  from the ordinary Jacobian rank (`solver.rs:4107-4120`).
- **Floating pin still works.** The rows depend only on point differences, so the `Pin::Floating`
  analysis still holds.
- **Members are covered whatever they are.** Arcs keep `Block::Arc` (|b−c| = |a−c|), which the
  frame rows already satisfy. Spline fit points, control points and handles, and ellipse axis
  points, are all points, so they are covered. Knots, weights and conic `rho` are constants.
- **Seeds stay local.** Keep one row block per member point, not one block for the whole set.
  With per-point blocks, `pinned()` (`solver.rs:2035-2146`) fixes each member once O and X are
  known, and redundancy is judged point by point. One monolithic block would have dense rows,
  O(N²) finite differences and nothing to pin.

### Constraints inside a block

A constraint whose every point and curve is a member is **held**: it stays in the sketch, is
listed in `SketchBlock::held`, and `build_blocks` skips it while the block exists. Held
constraints are released by Explode Block, by Edit Block when a member goes, and by a copy of
part of a block:

- A Free block's held dimensions that are plain numbers (no parameter) are first scaled by how
  much the block grew, |d| / L0.
- Each held constraint that holds where the geometry is now drives again.
- Each one that doesn't is removed and reported as `dropped`. These are the ones that depend on
  the old orientation after a turn (horizontal, vertical, a line angle, a distance along X or Y),
  and the parameter-driven dimensions after a resize.

So Explode Block always leaves a solved sketch. A held constraint can't be changed while it is
held: Toggle Driven refuses it, and so does a parameter change when the parameter drives only
held dimensions.

Without this, every coincident, horizontal or dimension inside an imported or hand-drawn
profile would turn redundant the moment the block exists, and the sketch would fill with
redundancy marks.

Held constraints are drawn faded, can't be selected and can't be edited until the block is
opened with Edit Contents or exploded.

There is one exception. In **Free** scale, a driving length dimension between members is
allowed after Make Block. It sets the size and takes the fourth freedom, which is the "a separate
single linear dimension" case in #4069. A second such dimension is refused through the existing
over-constraint check (`crates/engine/src/cmd/sketch.rs:3272-3296`), because it changes no DOF and
its residual is not zero.

Constraints between a member and outside geometry stay driving: they are what places the block.

Any other constraint wholly inside a closed block is refused, whichever command adds it (or makes
a driven dimension driving): "`k12` would lie wholly inside block Dovetail, which keeps its shape:
open the block with Edit Contents to change its shape". The check is one place, after the edit in
`edit_solved` (`cmd/sketch_block.rs`, `refuse_inside_closed`), and looks only at the constraints
the edit added or made driving. A Fix, a driven (reference) dimension and a free block's length
dimension are not refused.

### Over-constraint and the refusals that come for free

- **A rigid block.** A length or distance between two members disagrees with the shape, so it is
  refused with the existing "would over-constrain" message. One that agrees with the shape is
  kept and marked redundant, as today.
- **Make Block on geometry the outside already pins.** Two fixed members can't both move, but the
  block holds them where they are, so nothing changes and the block reports the DOF that is left.
  Make Block never moves geometry. It captures (u, v) from the current solved positions.
- **`Sketch::has_size`** (`model.rs:1758`) counts a Fixed block as a size, so the first-dimension
  auto-scale doesn't scale the sketch around a rigid block.

### Dragging

Dragging uses the held solve (`crates/engine/src/cmd/sketch_drag.rs`). The frame rows are exact,
so grabbing any member moves the block rigidly with no extra code. `Sketch::coupled`
(`model.rs:2030`) and the cursor fallback's component union have to include the block's points,
so the whole block is treated as one component.

The cursor fallback refuses components over 400 variables (`cursor.rs:15`). Large blocks (a DXF
profile of a few hundred points) then rely on the held solve alone, which is acceptable.

### Constructive seeding (phase 4)

The seed reader (`loci.rs:816-1080`) ignores unknown kinds, so phase 1 leaves block rows to
Levenberg–Marquardt. That is cheap: the shape already holds at the start, and LM only has to find
the pose. Exact placement comes later:

- **Propagation.** A `Tie::Frame { p, o, x, u, v }` places P as soon as O and X are placed, in the
  same way `Tie::Midpoint` works (`loci.rs:1279`). Like a midpoint it also works backwards: P and
  one end of the axis place the other end, unless P is within 1 % of the axis length of that end.
- **Radii.** A Fixed block's member circles and ellipses are seeded at k·L0 with the dimensioned
  radii; a Free block's at k·|d| once the seed has placed O and X.
- **Rigid clusters.** A Fixed block adds `Rel::Dist(O, X, L0)`, so O–X starts as a pair cluster in
  `Graph::build` (`decompose.rs:557-634`) and the frame ties grow it. `pair()` (`decompose.rs:845`)
  then lands it exactly on two shared ground points.
- **Free blocks** would need a `Frame::Similar` cluster. Until then they stay with LM unless a
  length on the axis makes them rigid, or the seed places two of their points (O and a member,
  or X and a member).

The seed is checked automatically, because in tests every solve also runs without the seed and
must give the same status and DOF (`solver.rs:2839-2857`).

### Size: members left out of the solve (phase 4b)

With every member point in the solve, an unplaced block is one connected group of 2N + 4
unknowns. The solver takes at most `MAX_COMPONENT_VARS` (4000) in one group, so a block of more
than about 2000 points could not be solved unless both axis ends were fixed, and the Full
analysis of an unpinned block ran a dense RREF on about 2N columns (1 ms at 300 points, 9.5 ms
at 1000).

`crates/sketch/src/carried.rs` leaves out of the solve every member that only its own block's
rows read:

- **Which.** A member point is left out when the only rows that read it are its own two frame
  rows: no driving constraint names it, no other block lists it, it is no axis end, and it is not
  held (a dragged point is a constant, so its frame rows stay in and the drag moves the axis). A
  member radius is left out when only its `FrameRadius` row reads it. An `Arc` or `Closure` row
  whose points are all left out of one block goes with them: a block keeps its shape up to a
  similarity, which keeps both, so the row is a combination of their frame rows. Leaving one out
  can make another row read nothing left in, so the choice runs to a fixpoint (each unknown is
  closed once).
- **Placed after the solve.** The left-out points and radii are put where the solved axis puts
  them, `P = O + u·d + v·perp(d)` and `r = k·L0` or `k·|d|`, and the left-out arc rows are
  checked there. The constructive seed leaves their frame ties out too
  (`loci::read_leaving_out`): it would only place them where the solve puts them anyway.
- **The analysis, on the smaller group.** A left-out point's two rows are the only ones that
  read its columns, with the identity there, so the rank rises by exactly two for it (one for a
  radius): it adds no freedom. It is determined when every null vector of the rows left in moves
  it by nothing, read on the axis ends' columns (`linalg::Reduced::null_on`, on the reduction the
  rank analysis made), and, as for other unknowns, a short step along the freedoms (`Nearby`) is
  checked too: a member at the point a block turns about is determined. A redundant constraint is
  redundant with or without the left-out rows (rows that alone read their columns never take part
  in a combination of other rows), but the checks that are capped at `MAX_REDUNDANCY_VARS` (400)
  unknowns per group, the redundancy check and `Nearby`, now run on the group the members leave
  behind: a block of 200 to 1000 points that kept in was past the cap is now checked, so a
  constraint on it can be reported redundant where it was not before. `determined_by`, which
  explains a redundant constraint, reads the same reduced rows (`carried::carry`), so it names
  what the solve found.
- **Least motion kept.** LM damps a step by how far it moves the unknowns. Left out, the members
  would no longer weigh on a move of the axis, and an unplaced block dragged by one member would
  swing about its axis. Each block adds a row-less `Block::Carried` on its axis ends that adds to
  LM's damped matrix the quadratic form of its members' moves, `Σ|(1 − w)·δO + w·δX|²` with
  `w = u + iv` (plus `Σk²·(d̂·δd)²` for a free block's radii), at `λ/(1 + λ)`, the weight a member
  kept in the solve took. Determined results agree with the solve that keeps the members to its
  tolerance; where freedoms are left the two iterations take slightly different paths (a block
  moved 11 mm onto the origin turns by 2° either way, and the turns differ by 3e-5 rad).
- **Precision.** An axis end off by ε puts a member off by up to `reach`·ε, `reach` being the
  largest `|1 − w| + |w|` of the block's left-out points. A component that places left-out
  members is iterated up to three more times (`polish`), until its residual is a hundredth of the
  usual tolerance divided by `reach`, but not below 64 units in the last place of its largest
  coordinate, and within the solve's numeric budget.

A block whose members other constraints also hold keeps those members in its group, so more than
about 2000 of them can still make a group too large to solve; `SolveReport::oversized_block`
then says so. In this crate's tests every solve of a sketch with blocks (up to 1000 points) is
repeated with the members kept in, and must give the same status, conflicts and turned-over
constraints; when solved, the same freedoms and positions where fully determined; and the same
determined and redundant flags where the kept-in solve ran its capped checks
(`assert_same_as_whole`).

## Commands

These are ordinary engine commands with I/O schemas, so MCP, scripting and undo come with them
(`crates/engine/src/cmd/mod.rs:629`, `schema/unconverted.txt` only shrinks).

| Command | Params | Reply |
|---|---|---|
| `sketch.block` | `sketch`, `curves: [id]` (or from the selection), `scale?: "fixed" \| "free"` (default fixed), `origin?: point id`, `name?` | `block` id, `axis` line id, `origin` point id, `held: [constraint id]`, `dof` |
| `sketch.explode_block` | `sketch`, `block` | `curves`, `restored: [constraint id]` |
| `sketch.block_edit` | `sketch`, `block`, `scale?`, `name?`, `add?: [curve id]`, `remove?: [curve id]` | as `sketch.block` |
| `sketch.block_open` ("Edit Contents") | `sketch`, `block` | `block`, `restored`, `dropped`, `closed?` (the block that was open before) |
| `sketch.block_close` ("Close Block") | `sketch`, `block?` (default the open one) | `block`, `held`, `added`, `left_out?`, `removed?`, `dof` |

`sketch.block_edit` re-captures (u, v) from the current shape. In the UI it is reached by
**Edit Block**; it is refused while the block is open.

How each existing command treats a block member:

- **`sketch.delete`** (`sketch.rs:5124`):
  - Deleting a member removes it from the block.
  - Deleting the axis line, or giving the block's id, explodes the block first.
  - Deleting the last member deletes the block and its axis.
- **Trim, Extend, Break, Fillet, Chamfer, Offset** refuse a member of a closed block. They go
  through `curve_by_id` (`sketch_modify.rs:206-229`), which already refuses links and offset
  outputs. The message is: "`l12` is part of block Dovetail, which keeps its shape: open it with
  Edit Contents to change it." While the block is open they take its members.
- **Move/Copy, Transform, Scale** (`sketch_modify.rs:4836`, `:5130`):
  - Picking any member moves the whole block once, the way text does (`sketch_modify.rs:4904-4917`).
    A member of the open block moves on its own.
  - The move goes to O and X, and the solve carries the members.
  - Scale on a Fixed block changes L0. That is the only way to resize a rigid block, and it is
    undoable.
- **Clipboard** (`cmd/sketch_clipboard.rs:272`, `:487`):
  - Copying a whole block pastes a new, independent block.
  - Copying only part of a block pastes plain geometry.
- **AutoConstrain** (`sketch_constraints.rs:217-309`) skips constraints whose entities are all
  members. Left alone, a horizontal on a block line would lock the block's turn and a DistanceX
  would pin its position.
- **Pattern** of a whole block: phase 5. Until then, a pattern refuses members ("patterns of
  blocks are not supported yet").

## Editing a block in place (Edit Contents)

A block can be opened to change its curves without exploding it. Opening and closing are commands
of their own (`sketch.block_open`, `sketch.block_close`), so each is one undo step, and every edit
made while the block is open is its own step.

### Opening

`Sketch::open_block` (`crates/sketch/src/block.rs`):

- **Held constraints drive again** where they hold: a free block's plain dimensions are first scaled
  to its size. A held direction of a member line in the sketch (Horizontal, Vertical, a line angle
  with a plain value) that no longer holds because the block turned becomes the same direction
  relative to the axis, as the line lies now: Parallel or Perpendicular to the axis, or an angle to
  it. It keeps its id, and is held in that form when the block closes. The others that no longer
  hold (a resized block's parameter-driven dimensions, a turned block's distances along X or Y) are
  removed and reported as `dropped`; the UI says so in the status line. The block keeps no `held`
  list while it is open.
- **The frame leaves the solve.** `frame_rows` skips an open block, so its members move freely.
- **Everything outside stays still.** `Sketch::open_contents` says which points and curves are the
  block's contents: its members, its own points, and every curve and point made since it opened.
  `locked_points` locks every other point, the axis ends included, and the solver's layout keeps
  every other radius, so a constraint from outside onto the axis or a member cannot move the block,
  and an edit inside it cannot move anything outside it. A point shared by a member and an outside
  curve is locked too: an ellipse member whose axis lines (which share its centre and major point)
  are not members cannot move while the block is open, nor a member line joined end to end with an
  outside one at a shared point.
- **Nothing outside changes either.** Every sketch edit runs through one guard
  (`sketch_block::BlockGuard`, in `edit_solved` and in a sheet sketch's `edit_sheet`). While a block
  is open it refuses an edit that changes, moves, removes or puts in a block any curve or point
  outside the contents, the axis included, or that adds, changes or removes a constraint naming
  nothing inside the block or naming its axis (`Sketch::outside_change`): "`l6` is outside block
  Dovetail, which is open for editing its contents: close the block first". Deleting the axis is
  refused the same way. Commands that only read outside geometry as a reference are taken: a
  dimension or a coincident from a member onto outside geometry, a mirror of members across an
  outside line, a copy of an outside curve (the copy is new, and joins the block).
- **What it remembers** (`SketchBlock::open: BlockOpen`): the constraints wholly inside it that
  drove when it opened (`driving`, a free block's size), the curves and points outside it
  (`outside`, `points`), and its own points (`own`).
- **One block is open at a time.** Opening another one closes the open one first.

While a block is open every sketch tool works on its members: trim, fillet, dimensions, dragging a
single point, new curves, deleting members. `block_refusal` and the held-constraint refusals pass
an open block, Move/Copy moves its members on their own, AutoConstrain adds only relations among
its contents, and an open block that loses every member stays until it closes.

### Closing

`Sketch::close_block` captures the new shape into the same block:

- **Members:** the members still there, then every curve not in `outside` (and not the axis), in
  sketch order. A curve that can't be a member (text, a projection, an offset output, pattern
  geometry) stays in the sketch outside the block and is reported as `left_out`, and the UI says so;
  the curves of a block made while it was open stay that block's and are not reported. Splines
  bring their guide lines. A point on no curve made while it was open joins as a member on its own
  (`SketchBlock::loose`). A block left with no curve goes with its axis.
- **The frame:** (u, v) and radii are captured from where the curves are now, relative to the axis,
  which did not move. A fixed block's L0 stays the axis length.
- **Held again:** every constraint wholly inside the block, except those in `driving`. Inside means
  every point and curve it names is a member, a member's point or the axis, and one of them a
  member: a relation between a member and the axis is held too, while one on the axis alone (a
  placing horizontal) is not.
- **Outside constraints** onto the axis or member points are left as they are; they held while the
  block was open, so the closed block is placed as before.

### Open state: saved, and closed on leaving

The open state lives in the sketch (`SketchBlock::open`), so undo and redo restore it with the rest
of the design, and it is saved with the design when the design is saved while the sketch is edited.
That keeps every file consistent: an open block holds nothing and its contents are where they were
drawn. It never outlives the sketch's edit:

- **Leaving the sketch** closes it. The one place that makes sure, whatever ended the edit (Finish,
  Finish with AutoConstrain, a sheet sketch's Finish, an undo or a redo that left the sketch), is
  `Session::execute`: after every outermost command, `sketch_block::close_unedited` closes the
  blocks open in any model or drawing sketch not being edited, in that command's step.
  `finish_model_edit` also closes the block first, so a folded edit's step holds the closed block,
  and the history steps a finished edit leaves (`complete_edit_step`, and the redo steps of
  `finish_edit_history`) are closed too.
- **Opening a design** closes it (`format.rs` `decode`, beside the other load-time repairs), in the
  model's sketches and in drawings' sketches: no sketch is edited in a design just opened. There it
  closes with the members the file lists (`close_open_blocks_as_saved`), so stale or hostile `open`
  data cannot pull other curves into the block.

A session-only state was the alternative. It would need a capture on every save, autosave and crash
report; a design written without it would reopen with its open block's old shape and empty `held`
list, and lose the edits. The last release ignores blocks entirely and makes every held constraint
drive, which is what an open block already does, so saving the state costs older releases nothing.

### Members are locked outside Edit Contents

- **Picking:** a click on a member curve or a member point of a closed block selects all its
  members, as the glyph and the Browser row do (`sketch_block_tree::whole_blocks`, applied in
  `viewport::select`, which also serves Select Other). Dragging a member moves the whole block, as
  the solver's frame rows always did.
- **References:** a running tool (Coincident, Dimension, a drawing tool) and a dialog's inputs pick
  a member's point or curve on its own, as a reference to geometry outside the block. A constraint
  made that way wholly inside the block, or between a member and the block's axis, is refused (see
  "Constraints inside a block").
- **Copy and Paste** of an open block's curves paste a block that holds what the open block will
  hold when it closes.
- **Box selection:** a closed block is taken whole or not at all. A window or a lasso takes it when
  it takes every one of its shown curves; a crossing box takes it when it touches any of its curves
  or points (`sketch_block_tree::box_blocks`). An open block's curves are boxed one by one.
- **Held constraints and dimensions** can't be picked, boxed or edited in place outside Edit
  Contents; a double-click on a held dimension says which block holds it.

### UI

- **Edit Contents** sits in the block's menus (a member's right-click, the glyph's and the Browser
  row's), and a double-click on a member of a closed block opens it. A double-click on the glyph or
  the row opens Edit Block on a closed block and nothing on an open one. While the block is open
  the item is **Close Block**, and Edit Block is off.
- `sketch.inspect` reports a point as `fixed` only when it is fixed in the sketch, not when an open
  block holds it still.
- **While open:** the Sketch Palette starts with "Editing block *name*" and a **Close Block**
  button; the block's contents are drawn in the block colour and the rest of the sketch in the
  colour of a sketch not being edited.

## Insert as block (#22102)

- **Param.** `sketch.insert_dxf` and `sketch.insert_svg` (`insert()` in
  `crates/engine/src/cmd/sketch_import.rs`) get `block?: bool`. With it, the inserted curves go
  through the same code as `sketch.block` (`Sketch::make_block_at`, which `make_block` shares),
  with Fixed scale, the origin at the placement anchor (where `at` puts the file's origin, or the
  drawing's middle with `align: "center"`; with `size_from: "viewbox"` as well, the sheet's
  middle, not the drawing's) and the axis along the insert angle, half the drawing's diagonal
  long (rounded), however far the file origin is from the drawing. The reply's `block` gives the
  block's id, axis and origin. Making the block looks every curve up once, so a 20,000-curve
  drawing takes no per-curve search of the sketch.
- **Text.** `sketch.block` refuses text, so the drawing's text comes in beside the block, and the
  reply warns that it is not part of it. A drawing with only text makes no block (and says so).
- **With `fixed`.** `block: true, fixed: true` fixes only the axis: two Fix constraints on its
  ends, plus one on any point the drawing adds outside every curve (a DXF `POINT`). With the
  axis length row the two Fixes are one row more than a rigid block's 3 degrees of freedom, but
  no Fix is wholly redundant, so none is reported; a block later made Free is held exactly.
  Exploding such a block leaves its curves free and only the leftover axis line fixed.
- **Size.** A drawing of any size comes in as an unfixed block: its members follow the axis
  outside the solver (see "Size: members left out of the solve").
- **Dialog.** `crates/ui-egui/src/dialogs_insert_drawing.rs` gets an **Insert as block** row after
  Angle, held on `Place` while the window is open and sent by `commands()`. The live preview is
  the drawing alone (`preview_placement` leaves `block` out): the axis takes its length from the
  placed size, which the scale-1 preview import does not have.
- **Phase 5:** one block per DXF `BLOCK`. The DXF reader flattens blocks today
  (`crates/io/src/dxf.rs:3`, `:1334`), so `io` would first have to say which block each piece
  came from.

## UI

- **Ribbon:** Make Block in `SKETCH › CONSTRAIN` (`crates/ui-egui/src/workspace.rs:750-764`) and
  Modify, with a `c_block` icon (an original master in `crates/look/icons/design/svg/model/`, with
  an attribution row).
- **Right-click on a member:** Edit Block, Edit Contents (Close Block while open), Select Block
  (selects every member) and Explode Block.
  They sit in `sketch_entity_items` (`context_menu.rs:1805`) beside Edit Sketch Pattern and
  Explode Text. Two or more curves in no block offer Make Block instead, just before Merge Lines.
- **Browser:** each block is a row under its sketch, the way patterns and projections are listed
  (`sketch_pattern_tree.rs`, `sketch_link_tree.rs`, hooked in at `browser.rs:3035`).
  - Click selects the members.
  - Double-click opens Edit Block.
  - Right-click opens the block's menu: Edit Block, Edit Contents, Select Block, Explode Block and
    Delete Block.
  - Delete after a click deletes the block with its members and its axis, as Delete Block does,
    in one undoable step. Explode Block is only the menu item.
- **Canvas:**
  - One block glyph near the origin. `glyph_constraints` already collapses a pattern to one glyph
    (`sketch_tools.rs:2514`).
  - The glyph is picked like a constraint glyph (`viewport.rs`, `sketch_block_glyph_at`): dimension
    text, constraint glyphs and midpoints win over it, and it follows the pick filter for
    constraints. A click selects the members, as the Browser row does, so Delete then deletes the
    block; a right-click opens the row's menu; a double-click opens Edit Block. A click on the
    glyph never falls through to the geometry under it.
  - Members and the axis drawn in the block colour (theme token `sketch_block`) while they are not
    fully constrained; once fully constrained they take the fully constrained colour like any
    other geometry. A light outline shows while the pointer is over a member or the glyph.
  - Held constraints faded, and not picked.
  - A click on a member selects the whole block; a double-click opens Edit Contents.
- **Strings:** new keys in `assets/locales/pending/sketch-blocks.json` (the required locales) and a
  glossary entry for *block*.

## Design format

`crates/doc/src/format/pending/sketch-blocks.rs` holds a `//!` note only, with no `step`. It
records what each side of the change sees:

- **What is saved:** `blocks` on the sketch, plus one construction line per block, and `open` on
  a block saved while it was open for Edit Contents (closed again when the design opens).
- **Older designs:** they have no blocks, so nothing changes for them.
- **The last release opening a new design:** it keeps `blocks` and writes it back, but doesn't
  solve it. There every held constraint drives. A block that was turned or resized conflicts with
  its held constraints, so that sketch fails to solve until they are fixed.

## Tests (in existing area files)

| What | File |
|---|---|
| Frame rows, DOF 3/4, held constraints, Free + one length, refusal of a second length | `crates/sketch/src/tests.rs` (beside the pattern tests at 2565-2744), `crates/sketch/tests/analysis_levels.rs` |
| Redundancy never names the block; held constraints restored on explode | `crates/sketch/src/redundancy_tests.rs`, `crates/engine/src/cmd/sketch_redundant_tests.rs` |
| Drag one member, the block moves rigidly; cursor fallback component | `crates/sketch/src/least_motion_tests.rs`, `crates/engine/src/cmd/sketch_drag_live_tests.rs` |
| Commands, refusals, delete/prune, move as a unit, clipboard | `crates/engine/src/cmd/sketch_constraints_tests.rs`, `sketch_pattern_tests.rs` (as the model for the prune tests) |
| Insert as block | the existing insert tests beside `sketch_import.rs` |
| A 300-point block solves within the existing budget | `crates/engine/src/cmd/sketch_perf_tests.rs` |
| Hostile params (empty lists, unknown ids, an axis as a member, NaN origin) never panic | the existing `hostile_params_never_panic` coverage |
| Seed: frame ties, rigid pair cluster | `crates/sketch/src/construct_tests.rs`, `decompose_tests.rs` |
| Edit Contents: open, edit, close; curves joining and leaving; a free block; undo and redo; leaving the sketch and opening a design with a block open; refusals | `crates/sketch/src/tests.rs`, `crates/engine/src/cmd/sketch_constraints_tests.rs` |
| A block picked whole by a click and a box; member points picked by a constraint tool; Edit Contents from each menu, a double-click and the palette's Close Block | `crates/ui-egui/src/sketch_modify_ui_tests.rs` |

## Phases (one PR each, one topic each)

1. **Model and commands.** `SketchBlock`, the frame rows, held constraints, `sketch.block` and
   `sketch.explode_block`, the delete, prune, refuse and move rules, the format note, tests. This
   is the PR that sets the format and API shape, so it gets the design review. It is usable over
   MCP before any UI exists.
2. **UI.** Ribbon, right-click, Browser rows, the glyph, member colour, held-constraint fading,
   `sketch.block_edit`, strings, the icon. Wait for, or coordinate with, the glyph refactor in
   #18276, which moves `sketch_tools.rs` glyph code into `constraint_glyphs/`.
3. **Insert as block** for DXF and SVG, with the dialog row. This closes #22102.
4. **Exact seeding** and the large-block performance budget.
   - **4b.** Members nothing else reads are left out of the solve, so a block of any size solves
     and drags without being fixed.
   - **4c.** Editing a block in place (Edit Contents and Close Block), and members locked outside
     it (see "Editing a block in place").
5. **Later:**
   - one block per DXF `BLOCK`
   - patterns of blocks
   - shared definitions with linked instances, if wanted, as a separate proposal

## Decisions

1. **Name:** *block*. *Group* collides with the glossary's Browser and timeline folders and with
   the Rigid Group joint.
2. **Default scale:** **Fixed**, with Free one click away. #4069 asks for free scale and #22102
   for a fixed shape.
3. **Constraints inside a block:** kept and faded while the block exists, and restored by Explode
   Block where they still hold (see "Constraints inside a block"). Removing them at Make Block
   would lose dimensions inside the block that parameters drive.
4. **The axis line:** always shown and pickable, because it is the most direct way to place the
   block.
