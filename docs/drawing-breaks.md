# Parallel view breaks (W36 first slice)

This slice provides a view-plane geometry map, a read-only engine command and a
native developer example. It does not yet create a persistent broken drawing
view in the application. W36 remains open for that integration.

## Try the developer example

```sh
cargo run --profile ci -p solidcraft --example broken_view
```

For the reproducible oblique sample, append `-- --oblique-sample`. Its two holes
and both long edges remain horizontally aligned. The cut angle controls the
boundary lines; this horizontal fixture always shortens along X.

`cargo test --profile ci -p solidcraft-ui-egui --test broken_view_preview`
renders that actual preview UI without a desktop window to
`target/broken-view-review/aligned-oblique.png` for visual review.

1. Choose the line angle before placing a pair. The initial angle is -90 degrees.
2. Start Broken View, then click the first and second lines on the sample view.
   The tool stays active for further pairs; all pairs share one angle.
3. Right-click or Escape cancels the pending pair. With no pending pair, it
   finishes the tool. Completed edits support local Undo, Redo and Clear breaks.
4. Change the paper gap for subsequent pairs, the straight/zigzag appearance, or
   the view scale (1:1 or 1:2). Gaps remain paper-sized when the scale changes.

An undo/redo restoration can be refused if its gap would not shorten the view
at the current scale. Geometry and history remain unchanged; restore the previous
scale to retry that edit.
The example also refuses edits that remove every sample edge, preserving the
previous display instead of trying to fit an empty canvas.

The angle control is disabled while a first line or any completed break exists.
Cancel the pending pair and clear completed breaks to choose a new angle. This
is a restriction of the shared-angle prototype, not the final Drawing task UI.

The original fixture is a 240 by 70 mm rectangle with two explicitly sampled
polygonal holes. The displayed true-length label describes that fixture; it is
not a production dimension or evidence of analytic-curve clipping. The canvas
shows the shortened result. Placement uses the inverse map to recover original
model coordinates from this display; clicks inside an empty paper gap are
ignored. The example uses temporary state and has no save/load.

## Geometry contract

`solidcraft_geom::drawing_break::BreakMap::new(angle_rad, shortening_axis, scale, bands)` takes
parallel bands in original view-plane model millimetres. The line angle is
anticlockwise; its normal is `(-sin(angle), cos(angle))`. Each band's `start` and
`end` are coordinates along that normal. The independent shortening vector is
normalized; `(1, 0)` preserves y and `(0, 1)` preserves x. `gap_mm` is measured
in paper millimetres along that axis. Cuts parallel or nearly parallel to the
shortening axis (`abs(normal dot axis) < 1e-6`) are refused.

The map scales uniformly, removes each open band interior, and translates the
retained pieces only along the shortening axis to leave the requested paper gap.
Oblique cuts therefore preserve alignment across that axis. Both boundary
lines remain drawable. `to_paper` returns no point inside a removed span;
`to_model` returns no point inside a paper gap. `line` clips straight segments
at the band boundaries before translating each retained piece. Measure original
geometry, never the shortened displayed fragments.

Construction normalizes reversed endpoints, sorts bands, and rejects overlap,
touching bands, non-finite values and gaps that would not shorten the view.
There are at most 64 bands. Model components are capped at 1e9 mm and mapped
paper components at 1e5 mm. Boundary classification budgets 16 machine epsilons
of the scaled break coordinates, shortening and maximum paper extent together;
bands, retained spans and paper gaps must exceed four such budgets. Centre
view-plane coordinates near the geometry; the caps do not certify accuracy.

## Engine command

All example geometry edits use `drawing.break.preview`, which leaves the source
design unchanged and creates no undoable document entry. For example:

```json
{
  "angle_rad": -1.5707963267948966,
  "shortening_axis": [1, 0],
  "scale": "1:2",
  "breaks": [{"start": 60, "end": 140, "gap_mm": 5}],
  "lines": [[[0, 0], [200, 0]]],
  "paper_points": [[32, 0], [65, 0]]
}
```

The result has two retained segments, ending at paper x=30 and starting at x=35,
and a total shortening of 35 paper mm. The first pick is in the gap and returns
`null`; the second maps back to model x=200 (within floating-point roundoff).
The result also reports the normal, normalized shortening axis, normalized bands
and paper gap intervals along the normal. `shortening_axis` defaults to `[1, 0]`.
For an oblique cut, a requested axis gap `g` occupies `g * abs(normal dot axis)`
in those normal-coordinate intervals. Forward mapping, clipping and inverse
picking all use the same directional translation.

Unknown fields and invalid input return errors. Input is capped before typed
parsing in every parameter-check mode: 64 bands, 2,000 segments, 2,000 picks and
64 bytes of scale text. More than 20,000 resulting pieces is an atomic error.

## Later W36 slices

Persistent view records and commands, source references and recomputation,
Drawing workspace placement/editing, general-curve clipping, mixed-angle
breaks, dimension layout, document undo/redo and save/load remain integration
work. This example does not replace the existing sheet, template, view,
annotation or workspace providers.
