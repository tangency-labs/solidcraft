# Fills and hatch patterns on a sheet

A fill belongs to one saved sheet sketch. It keeps the identities of its complete
outer boundary and every hole. It uses paper XY millimetres from the sheet's
lower-left corner, independently of display units and view scale.

These commands extend the [sheet-sketch receiver](drawing-sheet-sketches.md).
They use the open drawing's history; they do not create or evaluate model features.
The native canvas and pointer tools remain separate consumers of the shared scene.

## Create a fill

Create geometry with the existing sheet-sketch tools and finish the sketch.
Call `drawing.fill.profiles` with its exact encoded sketch ID. Its
`fill_profiles.boundaries` lists complete, certified source-curve identities;
`uncertified` counts profiles that cannot supply that identity.
An incomplete profile search refuses the inspection instead of offering guesses.

Copy one complete boundary into `drawing.sketch.fill`. For example, replace the
IDs and boundary below with values returned by inspection:

```json
{
  "sketch": "<encoded sheet-sketch ID>",
  "boundary": {"outer": ["<curve ID>"], "holes": []},
  "paint": {"kind": "solid"}
}
```

Curve IDs within each boundary and the hole list are canonical, ordered values.
Preserve the complete inspected identity. A profile index, a nearby point, an
incomplete curve set or the outer boundary without its current holes cannot
replace it. Complete source coverage does not certify every geometric or
manufacturing property of the source sketch.

Solid paint retains the source region's exact supported paper segments, including
conics and higher-degree Beziers. The selected writer may refuse a curve that its
shared grid cannot represent. It does not replace that curve with chords.

## Import and use patterns

`drawing.pattern.import` stores independent definitions inside the drawing.
Choose an original built-in with either request:

```json
{"kind": "builtin", "pattern": "lines_45"}
{"kind": "builtin", "pattern": "cross_hatch"}
```

Import PAT text with `kind: "pat"`, `source`, optional `units: "mm"` or
`"inches"`, and optional positive `scale` (default 1). PAT angles are degrees;
stored angles are radians. Lengths are converted once to paper millimetres.
The parser requires a final blank physical line after the last family.

```json
{
  "kind": "pat",
  "source": "*DOTS,Original example\n0,0,0,0,2,0,-4\n\n",
  "units": "mm",
  "scale": 1
}
```

The reply lists the saved patterns and their exact encoded IDs. Duplicate names
remain distinct snapshots. Importing another definition never changes an existing
fill's pattern. `drawing.pattern.inspect` reads one ID or the complete list.
`drawing.pattern.delete` refuses while a fill references the selected snapshot.

Use a saved pattern by ID, with an explicit physical pen width:

```json
{
  "sketch": "<encoded sheet-sketch ID>",
  "boundary": {"outer": ["<curve ID>"], "holes": []},
  "paint": {"kind": "hatch", "pattern": "<encoded pattern ID>", "pen_width_mm": 0.25}
}
```

The current bounded generator admits line polygons with disjoint interior polygon
holes, and canonical full-turn or two-semicircle disks with at most one concentric circular
hole. Other curved hatch boundaries refuse explicitly. They remain usable for
solid paint when their exact source profile is admitted.

Positive dash values draw, negative values leave gaps, and zero values place dots.
Phase follows the authored origin and signed member deltas across clipping and
holes. Dots are filled disks with the selected pen diameter. Their centres lie
strictly inside the region. Strokes use the clipped centreline and centred pen
width. Dots that cannot survive the actual writer grids refuse instead of becoming
empty geometry or zero-length strokes.

## Edit, loss and history

`drawing.fill.edit` takes an exact `fill` ID and optional replacement `boundary`
and `paint`. A replacement boundary must resolve completely now. An unchanged
edit adds no history step. `drawing.fill.delete` removes one fill.
`drawing.fill.inspect` reads one ID or all fills, with their boundary and paint.

A completed profile search that loses the saved boundary retains the last valid
region with `lost_boundary` status. Adding a hole changes the whole identity;
deleting a curve and drawing another nearby does not repair it. Undo can restore
the original association. Repair uses an explicitly selected complete boundary.
An unavailable source, failed solve, incomplete search or conversion refusal
reports failure separately and draws no new fill geometry.

Retention snapshots update atomically around affected sheet-geometry edits and
source-parameter changes, including named feature inputs. They join the command's
existing history step. Model undo/redo carries current retention for matching fill identities; Drawing undo/redo preserves it across unrelated paper edits. A full checkpoint restores its complete saved state, including its retained regions. Ordinary paper-property commands and read-only scene
extraction do not solve fills or mutate saved snapshots. Saved definitions,
identities and retained regions survive reopen. Deleting a sheet with explicit
`contents: "delete"` removes its sketches and fills together.

The scene paints borders, views, fills and then sketch strokes. Each fill keeps
its own object ID and `FILL` layer. Its `fills` status list is separate from the
view and sketch lists. The provider shares region resolution for fills of the
same sketch and expands one stale fill per step.

`drawing.export` currently writes SVG. It names and refuses failed or lost-boundary
fills before touching the destination. The same prepared scene is available to
the PNG writer. App export dialogs, PDF command wiring and broader curve support
retain their existing owners.

## Bounds

Each drawing admits at most 1,000 fills, 1,000 pattern snapshots, 128 filled
sketches, 8,192 retained boundary pieces and 20,000 boundary curve IDs in all.
A boundary has at most 512 curve IDs and 128 holes. A source-update command
refreshes at most 128 filled sketches across the affected drawings.

PAT input is at most 1 MiB. A pattern has at most 64 families and six dash entries
per family. Generation retains its independent caps: 10,000 candidate lines,
2,000,000 work units, 65,536 clipping events, 200,000 dash visits and 40,000 marks.
Unsafe numeric progress, ambiguous contacts, unsupported geometry or exhausted
budgets return complete errors. These bounds are admission and work policies,
not accuracy certificates.
