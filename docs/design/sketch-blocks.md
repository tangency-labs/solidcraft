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

Held constraints are drawn faded and can't be edited until the block is exploded.

There is one exception. In **Free** scale, a driving length dimension between members is
allowed after Make Block. It sets the size and takes the fourth freedom, which is the "a separate
single linear dimension" case in #4069. A second such dimension is refused through the existing
over-constraint check (`crates/engine/src/cmd/sketch.rs:3272-3296`), because it changes no DOF and
its residual is not zero.

Constraints between a member and outside geometry stay driving: they are what places the block.

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
  same way `Tie::Midpoint` works (`loci.rs:1279`).
- **Rigid clusters.** A Fixed block adds `Rel::Dist(O, X, L0)`, so O–X starts as a pair cluster in
  `Graph::build` (`decompose.rs:557-634`) and the frame ties grow it. `pair()` (`decompose.rs:845`)
  then lands it exactly on two shared ground points.
- **Free blocks** would need a `Frame::Similar` cluster. Until then they stay with LM until a
  length makes them rigid.

The seed is checked automatically, because in tests every solve also runs without the seed and
must give the same status and DOF (`solver.rs:2839-2857`).

### Size

A 300-point block has about 600 variables. Above `SPARSE_MIN_VARS` (64) LM uses sparse
Cholesky. In the per-point structure only O and X touch every row, so minimum-degree ordering
removes each member point with fill only among O and X.

The Full analysis is the cost to watch. If nothing pins O and X first, the dense RREF runs on
about 600 columns. A test should time this next to `large_sketch_solves_fast` (`tests.rs:1437`).
The phase-4 seed removes it, since `pinned` then takes almost everything out.

## Commands

These are ordinary engine commands with I/O schemas, so MCP, scripting and undo come with them
(`crates/engine/src/cmd/mod.rs:629`, `schema/unconverted.txt` only shrinks).

| Command | Params | Reply |
|---|---|---|
| `sketch.block` | `sketch`, `curves: [id]` (or from the selection), `scale?: "fixed" \| "free"` (default fixed), `origin?: point id`, `name?` | `block` id, `axis` line id, `origin` point id, `held: [constraint id]`, `dof` |
| `sketch.explode_block` | `sketch`, `block` | `curves`, `restored: [constraint id]` |
| `sketch.block_edit` | `sketch`, `block`, `scale?`, `name?`, `add?: [curve id]`, `remove?: [curve id]` | as `sketch.block` |

`sketch.block_edit` re-captures (u, v) from the current shape. In the UI it is reached by
**Edit Block** after Explode or after any change to the members.

How each existing command treats a block member:

- **`sketch.delete`** (`sketch.rs:5124`):
  - Deleting a member removes it from the block.
  - Deleting the axis line, or giving the block's id, explodes the block first.
  - Deleting the last member deletes the block and its axis.
- **Trim, Extend, Break, Fillet, Chamfer, Offset** refuse a member. They go through `curve_by_id`
  (`sketch_modify.rs:206-229`), which already refuses links and offset outputs. The new message
  is: "`l12` is part of block Dovetail: explode it, edit, then make the block again."
- **Move/Copy, Transform, Scale** (`sketch_modify.rs:4836`, `:5130`):
  - Picking any member moves the whole block once, the way text does (`sketch_modify.rs:4904-4917`).
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

## Insert as block (#22102)

- **Param.** `sketch.insert_dxf` and `sketch.insert_svg` (`crates/engine/src/cmd/sketch_import.rs:36`,
  `:42`) get `block?: bool`. With it, the inserted curves go through the same code as
  `sketch.block`, with the origin at the placement anchor and the axis along the insert angle.
- **With `fixed`.** `block: true, fixed: true` fixes only the axis: two Fix constraints instead of
  one per point (`sketch_import.rs:372-391`).
- **Dialog.** `crates/ui-egui/src/dialogs_insert_drawing.rs` gets an **Insert as block** row after
  Angle (`:113`), saved on `Place` and sent by `commands()` (`:130`), which already builds the
  command JSON. The shared-controls skill asks for 1280×720 screenshots of the new row.
- **Phase 5:** one block per DXF `BLOCK`. The DXF reader flattens blocks today
  (`crates/io/src/dxf.rs:3`, `:1334`), so `io` would first have to say which block each piece
  came from.

## UI

- **Ribbon:** Make Block in `SKETCH › CONSTRAIN` (`crates/ui-egui/src/workspace.rs:750-764`) and
  Modify, with a `c_block` icon (an original master in `crates/look/icons/design/svg/model/`, with
  an attribution row).
- **Right-click on a member:** Explode Block, Edit Block, and Select Block (selects every member).
  They sit in `sketch_entity_items` (`context_menu.rs:1805`) beside Edit Sketch Pattern and
  Explode Text.
- **Browser:** each block is a row under its sketch, the way patterns and projections are listed
  (`sketch_pattern_tree.rs`, `sketch_link_tree.rs`, hooked in at `browser.rs:3035`).
  - Click selects the members.
  - Double-click opens Edit Block.
  - Delete explodes the block, or deletes it if the user picks Delete Block.
- **Canvas:**
  - One block glyph near the origin. `glyph_constraints` already collapses a pattern to one glyph
    (`sketch_tools.rs:2514`).
  - Members and the axis drawn in the block colour (theme token `sketch_block`) while they are not
    fully constrained; once fully constrained they take the fully constrained colour like any
    other geometry. A light outline shows while the pointer is over a member or the glyph.
  - Held constraints faded.
- **Strings:** new keys in `assets/locales/pending/sketch-blocks.json` (the required locales) and a
  glossary entry for *block*.

## Design format

`crates/doc/src/format/pending/sketch-blocks.rs` holds a `//!` note only, with no `step`. It
records what each side of the change sees:

- **What is saved:** `blocks` on the sketch, plus one construction line per block.
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
5. **Later:**
   - one block per DXF `BLOCK`
   - patterns of blocks
   - editing a block in place without exploding it
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
