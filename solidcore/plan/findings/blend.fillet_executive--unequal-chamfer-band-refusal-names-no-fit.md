# blend.fillet_executive: an unequal chamfer refused at a shared face's bands named no fit (2026-11-08)

**Unit:** `blend.fillet_executive` (`FilletExecutive::refuse_overlapping_bands`, the fork's `BandsClearOtherRails`, `fixes.fillet_refuse_overlapping_bands`).

**Inputs:**
- a 30 x 2 x 10 box;
- both top long edges, at z = 10 and y = 0 and y = 2, chamfered by `prepare_chamfers_along` with `ChamferSize::Distances(d)` on both;
- corners bevelled;
- every fix on except `geometric_g2_continuity`, as ops runs.

**Before** (#3190 review follow-up, probe run 3190-3): every case is refused by the band check (code 8, "a blend's rail runs inside the band another blend takes off a face they share (the blends are larger than the face allows)").

| d | `fit_size()` | `chamfer_fit` |
|---|---|---|
| (1.5, 1.5) | `Radius` 1.0000000000001137 | 1.0000000000001137 |
| (1.5, 3.0) | None | None |
| (3.0, 1.5) | None | None |
| (0.5, 2.0) | None | None |
| (2.0, 0.5) | None | None |
| (1.2, 0.9) | None | None |

**Cause:** the band refusal's fit is the fork's `BandsMeetAt`. It bisects one radius common to both blends, so it has none to name when the two chamfers' offsets differ. The refusal named no size, against docs/generic-blend.md section 3.5. `chamfer_fit`'s `FitKind::Scale` branch was therefore never reached.

**After:** the band refusal's fit is named as a scale of both blends (`FitKind::Scale`) where no common radius is, from the band check's own quantities. At each point the check samples on the first rail, it already measures:
- the second edge's foot `e` and the second rail's foot `r`;

and one drop more:
- the first edge's foot `f`.

From those, `lambda = (W - margin) / (|p - f| + |e - r|)` with `W = |f - e|`, rounded down by `gamma_7` (three operations for lambda, three for a check of the scaled chamfers).

It is named only where that is exact, decided from the geometry's kinds: both blends chamfers, the shared face a plane, both edges parallel lines. Elsewhere nothing is named, as before.

`chamfer_fit` reads it as the first distance. The fork's radius (`BandsMeetAt`) still comes first where both blends roll one radius.

The check refuses only bands overlapping by more than its `apart`, ten of the solvers' tolerances. So between the named size and the size the check refuses, `(W + apart) / (w_a + w_b)`, lie the §3.5 margin (1000 edge tolerances) and `apart`: about 1010 tolerances.

Tests: `crates/solidcore/tests/chamfer_band_scale.rs`.
- (1.5, 3) names a scale whose setbacks sum to `2 - margin`. It builds at `lambda d`, leaving the top face the margin wide, and is refused just above the check's threshold (there by the solver, see Left).
- (3, 1.5) names a scale and builds at it.
- (1.5, 1.5) names Radius 1.

**Left:**
- The equal case's fork radius is the closed meet, not less the margin. The two conventions differ (the fork's kept; the new one per section 3.5).
- Chamfers between faces that are not planes, edges that are not parallel lines, and rounds of unequal radii still name no fit. The fork's check still refuses them.
- Just above the check's own threshold, the rails already cross. There the solver's refusal ("Fillet Solver Error: Rounds Of Two Edges Cross On A Face") comes before the band check's and names no size: another refusal without its fit.
- The check itself is sampled: `blend.fillet_executive--band-check-is-sampled-not-an-enclosure.md`.

PR: the #3190 follow-up (shipping to SolidCraft after the move).
