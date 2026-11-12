# nlib.srf_nurb: retiring `closed_smooth_pole_tangent_limit` (WR-725) moves 50 golden rows in six units (2026-11-11)

- **Unit:** `nlib.srf_nurb` (`N_SrfIsClosedSmooth`, `Surface::is_closed_smooth` / `is_closed_smooth_flag`), reached through `nlib.srf_adv_p1` (`eval_pt_derivs_unbounded`) and the blend solver and offset-surface units that evaluate past a surface's domain.
- **Status:** survey group C (needs judgement) when written. The per-row checks are in this record's addendum (2026-11-12): better where a reference exists, neutral where none does; the retirement ships with them. Part of #14246.
- **What the flag did:** WR-725's G1 test across a seam at a pole takes the limit (the mixed derivative) where upstream takes the angle of two vanishing vectors. The flag was read from a `fixes` argument, not from `Kernel::fixes`, and every library caller passed `UpstreamFixes::default()`. So no kernel ran it, SolidCraft included: its preset line changed nothing. Retiring it (making the limit the only path) changes results for every caller.
- **Inputs:** goldens `--all` on SolidCraft main a65ecb00 + the retirement patch (swarm run 14246-pole-x1, 2026-11-11), against main's expected. The same base without the patch (runs 14246-a3-x2 and 14246-a9-x2, whose patches do not reach these units) passes all six units, so the moves are the patch's, not expected/'s drift. A blessed-vs-blessed run on main 0371e7d2 (14246-pole-x1b) is to confirm this and list every moved row.
- **What moves (measured):** six units fail the compare, 50 rows (the compare's listing below names 36: it prints the first ten of a unit); blessing them rewrites 207 rows in all.
  - `nlib.srf_nurb` (4): gen0652, gen1542, gen1707 go from status 17 (`NL_GEO_ERR`) to closed; gen1708 from 17 to not closed.
  - `nlib.srf_adv_p1` (4): gen0286, gen1037, gen1736, gen2028 go from status 17 to an unbounded extension (`has_ext` 1, closure flag 0). Three of the new extension nets hold ±inf, as 332 of the unit's 734 extension rows on main already do.
  - `surface.offset_surface` (10 flagged, 56 rewritten): values move by up to 0.17 relative, no status change.
  - `blend.fillet_standard_solver_p1` (24 flagged, 133 rewritten): up to 6.5e-5 relative.
  - `blend.fillet_standard_solver_p2` (6): up to 0.47 relative. In gen0067 the block of its 80 point values moves its max by exactly 2.0 and its sum by exactly 1.0 (15.572 to 17.572, 72.556 to 73.556); gen0131's `out[4]` moves by exactly 1.0 (1.1234 to 2.1234).
  - `blend.fillet_solver_p4` (2): gen0205 by at most 4e-9 relative; gen0381 completely (point block (28146.5, 72084.0, 479133.4) to (201.1, 11349.3, -53937.1)).
- **Suspected cause (inferred, not checked):** the closure answer decides how `eval_pt_derivs_unbounded` extends a surface past its domain, so every evaluation off the domain near a pole on the seam changes. The exact 1.0 / 2.0 moves look like a parameter period or a different seam branch, which would be a regression, not a refinement.
- **Checks to run before any retirement (in order):** the exact-1.0 moves (what moves by one period, and whether the 3D point moves with it); gen0652 and gen1542 against the sphere closed form (a sphere's seam is G1 at its poles); gen1707 and gen1708; `blend.fillet_solver_p4` gen0381 against the exact surface if it is analytic.

Flagged rows named by the compare's listing:

| unit | case | differences |
|---|---|---|
| `blend.fillet_solver_p4` | `gen0205` | 1 |
| `blend.fillet_solver_p4` | `gen0381` | 1 |
| `blend.fillet_standard_solver_p1` | `rv2j_cdjac_coneu_pl_(0.1, 0.1)_(True, False)_3_0` | 1 |
| `blend.fillet_standard_solver_p1` | `rv2j_cdjac_coneu_pl_(0.1, 0.1)_(True, False)_3_1` | 1 |
| `blend.fillet_standard_solver_p1` | `rv2j_cdjac_coneu_pl_(0.1, 0.1)_(False, True)_3_0` | 1 |
| `blend.fillet_standard_solver_p1` | `rv2j_cdjac_coneu_pl_(0.1, 0.1)_(False, True)_3_1` | 1 |
| `blend.fillet_standard_solver_p1` | `rv2j_cdjac_coneu_pl_(2.0, 0.5)_(True, False)_3_0` | 1 |
| `blend.fillet_standard_solver_p1` | `rv2j_cdjac_coneu_pl_(2.0, 0.5)_(True, False)_3_1` | 1 |
| `blend.fillet_standard_solver_p1` | `rv2j_cdjac_coneu_pl_(2.0, 0.5)_(False, True)_3_0` | 1 |
| `blend.fillet_standard_solver_p1` | `rv2j_cdjac_coneu_pl_(2.0, 0.5)_(False, True)_3_1` | 1 |
| `blend.fillet_standard_solver_p1` | `rv2j_cdjac_coneu_bump_(0.1, 0.1)_(True, False)_3_0` | 1 |
| `blend.fillet_standard_solver_p1` | `rv2j_cdjac_coneu_bump_(0.1, 0.1)_(True, False)_3_1` | 1 |
| `blend.fillet_standard_solver_p2` | `gen0067` | 1 |
| `blend.fillet_standard_solver_p2` | `gen0120` | 1 |
| `blend.fillet_standard_solver_p2` | `gen0131` | 1 |
| `blend.fillet_standard_solver_p2` | `gen0133` | 1 |
| `blend.fillet_standard_solver_p2` | `gen0165` | 1 |
| `blend.fillet_standard_solver_p2` | `gen0340` | 1 |
| `nlib.srf_adv_p1` | `gen0286` | 8 |
| `nlib.srf_adv_p1` | `gen1037` | 10 |
| `nlib.srf_adv_p1` | `gen1736` | 12 |
| `nlib.srf_adv_p1` | `gen2028` | 16 |
| `nlib.srf_nurb` | `gen0652` | 2 |
| `nlib.srf_nurb` | `gen1542` | 2 |
| `nlib.srf_nurb` | `gen1707` | 2 |
| `nlib.srf_nurb` | `gen1708` | 2 |
| `surface.offset_surface` | `extension_singular_clamp` | 1 |
| `surface.offset_surface` | `gen0267` | 3 |
| `surface.offset_surface` | `gen0354` | 1 |
| `surface.offset_surface` | `gen0539` | 1 |
| `surface.offset_surface` | `gen0617` | 1 |
| `surface.offset_surface` | `gen1256` | 1 |
| `surface.offset_surface` | `gen1521` | 1 |
| `surface.offset_surface` | `gen1852` | 1 |
| `surface.offset_surface` | `gen1979` | 1 |
| `surface.offset_surface` | `gen2080` | 1 |
