# blend.fillet_executive: the band check is a 15-point sample with a 10-tolerance slack, not an enclosure (2026-11-08)

**Unit:** `blend.fillet_executive`. The check is `geometry_bands_clear_other_rails` in `crates/solidcore/src/blend/fillet_executive/session.rs`: the fork's `BandsClearOtherRails`, behind `fixes.fillet_refuse_overlapping_bands` (WR-542).

**What it does** (read from the code, not changed):
- For each pair of blends sharing a face, with no common vertex, it takes the points `s / 16`, `s = 1 .. 15`, of the first blend's rail over its kept interval.
- At each point `p` it drops onto the second blend's edge (foot `e`) and rail (foot `r`).
- It refuses only where `p` lies more than `apart` inside the second's band: `apart < (p - r) . (e - r) / |e - r| < |e - r| - apart`, with `apart` ten times the largest solver tolerance.

**What follows:**
- It is not an enclosure. Between two neighbouring points, the rail can run into the other band and out again unseen. That happens where the band's inner side, or the rail, turns faster than a sixteenth of the kept interval resolves: a rail on a strongly curved face, or a short bulge of one band.
- An overlap of up to `apart` anywhere is also passed.
- So a refusal is proved where it fires, at a point. A pass proves nothing between the points.
- Every point of the first rail is sampled, but only the interior ones (`s = 1 .. 15`). An overlap only at a rail's end, near a corner the check does not exempt, is not seen.

**A case** (not constructed or run): on a plane face, a blend along a straight edge and a second blend whose edge bulges toward it over a short stretch (a shallow arc spanning less than a sixteenth of the first rail's kept interval). The second band can reach across the first rail between two sample points while every sample point lies outside it. Building it needs a body with such an edge, which I did not make.

**Not fixed** (team-lead, 2026-11-08). An enclosure would bound the rail's distance from the band's inner side over each interval, as `query::distance_bound` bounds a point's distance from a face, and refine where the bound does not decide. Changing the check changes verdicts for every blend pair, so it needs a goldens measurement.

Found while naming the band refusal's fit (`blend.fillet_executive--unequal-chamfer-band-refusal-names-no-fit.md`).
