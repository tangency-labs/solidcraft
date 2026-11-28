# Feature mirrors and patterns

Choose **Mirror**, **Rectangular Pattern** or **Circular Pattern** from the Solid
Create panel and set **Object Type** to **Features**. Select the source features
in the timeline or pick their faces in the viewport. Selecting features before
starting the command fills the Features input automatically.

Feature copies replay the source operations: a patterned hole cuts the target
solid, and an additive feature adds material. The mirror's New Body / Join
selector belongs to body copying and is hidden for feature replays.

Select the mirror plane, rectangular directions or circular axis using the
selection inputs. Click an input to pick into it; click a selected item again to
remove it, or use its clear button. The preview uses the same commands as OK.
Cancel leaves the design and undo history unchanged.

Rectangular feature patterns have two direction columns, each with a Distance
Type, its values, Flip and Center controls. The Distance Type says what is given:
Quantity and Spacing (the default), Quantity and Extent (the spacing is the extent
divided by one less than the quantity) or Spacing and Extent (as many copies as
fit in the extent: 80 mm at 15 mm makes 6, the last at 75 mm). With Spacing and
Extent, Copy at the end adds one more copy exactly at the end of the extent when
the spacing doesn't divide it, so only the last gap is shorter. A centred
direction set by Spacing and Extent reaches half the extent each way from the
original, with as many copies on each side; Copy at the end then adds a copy at
either end. Quantity and List takes the gaps between neighbours as a
list separated by semicolons and uses them in turn, then from the start again:
`10 mm; 20 mm; 30 mm` with Quantity 5 puts copies at 10, 30, 60 and 70 mm.
Centred, the list runs out from the original on both sides. Directions can meet at any angle. A missing second direction
produces a single row. Direction vectors are saved with the
feature; editing a picked guide later does not change these saved vectors.

Circular feature patterns have Quantity, Total angle, Flip and Center controls.
Six instances over 90 degrees use 18-degree steps, including both ends of the
arc. A full circle divides 360 degrees by the quantity without duplicating the
source at 360 degrees.

Quantities include the original. Center keeps the original fixed and alternates
copies on its two sides. For four instances the slots are `0, +1, -1, +2`;
Flip changes them to `0, -1, +1, -2`. This also chooses the extra side for an even
centered circular pattern. Rectangular directions have independent controls.

Double-click a saved feature in the timeline to edit it. Mirrors and patterns
retain their feature sources, including features that created bodies, and
patterns retain their placement controls and arbitrary direction vectors after
save/reopen. Source edits recompute the copies. Undo and redo cover the whole edit.

The command forms share sketch patterns' alignment names:

```json
{"command":"solid.pattern.rectangular","params":{"features":["Hole1"],"dir1":[1,0,0],"count1":4,"spacing1":15,"dir2":[0.6,0.8,0],"count2":2,"spacing2":20,"alignment":"center","alignment2":"center","reversed":true,"reversed2":false}}
```

```json
{"command":"solid.pattern.circular","params":{"features":["Hole1"],"axis":{"origin":[40,60,0],"dir":[0,0,1]},"count":6,"angle":"90 deg","alignment":"center","reversed":true}}
```

Omitted alignments default to `right`, and omitted reversals to `false`, preserving
older designs. `left` is also accepted by the command API. New placement values
are stored through the pending document-format migration.

## Stable names for body copies

The command forms of `solid.pattern.rectangular`, `solid.pattern.circular` and
`solid.pattern.path` accept an optional `body_names` string when copying bodies.
For example:

```json
{"command":"solid.pattern.rectangular","params":{"bodies":["Rail block"],"dir1":[1,0,0],"count1":5,"spacing1":20,"body_names":"Block {i}"}}
```

The original keeps its name, `Rail block`. The four new bodies are `Block 1`,
`Block 2`, `Block 3` and `Block 4`. Quantity includes the original; `{i}` counts
only new bodies, starting at 1 in the pattern's existing copy order. When several
source bodies are selected, copies of the first source come first, followed by
copies of each later source, with numbering continuing across sources. Picked
placements use the same rule.

The pattern saves the template and applies it on every rebuild. Changing
`count1` to 7 in this example gives `Block 1` through `Block 6`; rebuilding or
saving and reopening preserves those names. The template must contain `{i}`
when more than one new body is made. A name that clashes with another body is
refused, and the error identifies the conflicting name.

Leaving out `body_names` keeps the existing naming behavior, including for old
designs. The template is for separate body copies, not feature replays,
component occurrences or joined pattern results.

The design stores the template as `body_name_template`, separate from the legacy
`body_names` list of pinned names. The command parameter remains `body_names`.
