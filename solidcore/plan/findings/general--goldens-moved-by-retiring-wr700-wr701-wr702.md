# The goldens that retiring WR-700, WR-701 and WR-702 moves, and why the new values hold

- **Change:** `face_trim_curves_kept_where_exact` (WR-700), `swap_uv_keeps_analytic_definition`
  (WR-701) and `face_shrink_bound_on_knot` (WR-702) become the default and are removed.
- **Measured at:** main b13f07fbd with each flag forced on alone (`goldens --goldens --all`),
  and the branch head on that main. Each case below moves by its flag alone; the union of
  the three runs is exactly what the head moves. Rechecked after the rebase on main
  1ef8d8adb: `blend.fillet_executive_p3` fails the same 29 cases, the same 14 over 1e-6 with
  the same relative moves; the other five units' blessed lines apply as they were and pass.

## Which flag moves which golden

| Unit | Rows | WR-700 | WR-701 | WR-702 |
|---|---|---|---|---|
| `blend.fillet_executive_p3` | 29 | 29 | 7 | 4 |
| `primitive.api_trim_ops` | 2 | 0 | 2 | 0 |
| `query.face_p3` | 1 | 0 | 1 | 0 |
| `query.face_p4` | 4 | 0 | 4 | 0 |
| `surface.surf_of_revolution_p3` | 165 | 0 | 165 | 0 |
| `topology.api_files` | 19 | 0 | 19 | 0 |

The fillet columns count cases differing in some number past 1e-9 and overlap: 32 cases,
24 by WR-700 alone, 2 by WR-701 alone (the two `real-box-fillet12-r3-shell-*-issue-460`),
1 by WR-702 alone (`source-file-nurb_box-edge5`), 5 by more than one. Three of the 32
(`stubs4-degenerate-test5-e2-r0.5`, `-e3-r0.5`, `source-file-box_with_large_fillet-edge4`,
at most 4.5e-9 relative) stay inside the golden's block tolerance (1e-9 of the block's
absolute sum), so the golden counts 29. A full `--bless` rewrites more lines (135 at
1ef8d8adb, 109 at an earlier base): every case whose numbers differ in any bit. The bless
commit keeps main's line for every case that passes main's golden, so only the 29 change.

## `blend.fillet_executive_p3`: the rows moving more than 1e-6

| Case | Flag | Moved field | Main | Head |
|---|---|---|---|---|
| `stubs4-coons_01_03_29-e3-r0.5` | 700 | geometry block 0 sum | 51.7333024 | 52.7333024 |
| `stubs4-coons_01_03_29-e6-r0.5` | 700 | geometry block 0 sum | 257.6305000 | 258.6305000 |
| `real-box-edge-map-actual-session` | 700 | face geometry block 0 sum | 953.0433105 | 957.0433105 |
| `real-box-direct-shrink-actual-session` | 700 | shrink ints block 0 sum | 87097 | 87101 |
| `curve-sih-box_diff_cyl-all-edges-r1.0` | 700 | mass moment 10 | 2.8644e-4 | 2.8064e-4 |
| `curve-sih-...-r0.5`, `-r0.25`, `-r0.1`, `-step2` | 700 | mass moments 6 to 11 | | |
| `source-file-box_diff_cyl-all-edges` | 700 | mass moment 9 | -1.78659e-4 | -1.78767e-4 |
| `source-file-nurb_box-edge5` | 702 | topology block 0 sum | 301287 | 301286 |
| `private-upstream-rollover-404-cyl-on-box1-edge12-r2` | 700 | geometry block 0 sum | -34.7729116 | -34.7730638 |
| `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5` | 700 | geometry block 3 sum | 95.4131258 | 95.4131123 |
| `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1` | 700 | geometry block 0 sum | -58.2361801 | -58.2362847 |

- **The exact 1.0 shifts** (coons e3 and e6, and 4.0 in the two real-box rows): in
  `util::primitive::append` an edge writes its edge-use count and then the number of its
  edge uses with a UV trim. That number goes from 0 to 1 at one edge (block entry 230 in the
  coons cases, so the weighted sum moves by 230) and at four edges in the real-box faces
  (entries summing to 248). Those edge uses keep the exact trim the shrink used to drop and
  project again: the WR-700 change itself. The direct-shrink row's int block counts the same
  four trims after the shrink, 87097 to 87101 (its weighted sum moves by 85).
- **The `curve-sih` and `box_diff_cyl` rows:** only the near-zero moments of inertia
  (indices 3 to 5 and 9 to 11, 1e-7 to 1e-4) move; they are
  integrated to 1e-4 and the volume moves by at most 9.1e-10 relative. The trims are exact
  now, so the integration samples a slightly different boundary; no closed form exists for
  the body, and the moments the integrator resolves (indices 6 to 8, 1.5e4) agree to 6e-10
  relative.
- **`private-upstream-rollover-404` and `sphere_diff_3cylinders` e1:** vertex and edge
  tolerances drop from 4.36e-5 to 6.75e-5 down to the 4e-5 zone tolerance: a kept exact
  trim lies on its edge, so the tolerance no longer grows to cover a refit.
- **`source-file-nurb_box-edge5` (WR-702):** a bound now snaps onto its knot, the sliver
  span past it goes, two edges swap their order in the face's list, and one edge's
  tolerance drops from 6.34e-5 to 4e-5; the mass properties move by about 1e-13.
- Every moved case that keeps a body probes no worse than main: largest pcurve distance,
  largest edge tolerance, `check_brep` level 2.

## `query.face_p3` gen0506

The face's cylinder is built with `u` clamped from +-1234567 and `v` in
[0, 6.91492608675138], the same domain on main and here. Main's swap refit moved the
frame's origin to z = -1234567 and tilted its y axis by 7.2e-10 rad; the head keeps the
frame the cylinder was made with (origin 0, axes x and y). The bisector's `uv_line` is the
line between two of the face's loop points in the surface's parameters:

| End | Main | Head |
|---|---|---|
| start (u, v) | (-3.8678244843, 6.91492608675138) | (-3.8713482691, 6.91492608675138) |
| end (u, v) | (38.4221047691, -8.9e-16) | (38.4132950695, 0.0) |

Against the made domain: the head's end lies on the domain's start `v = 0` exactly, main's
is 8.9e-16 outside it; the `v` ends are the domain's bounds on both sides. The `u` ends move
by 3.5e-3 and 8.8e-3 and the `interval_bounds` with them (0.1061569 to 0.1062535,
0.1504638 to 0.1505659): the loop points are mapped into a different surface map (main's
refit frame against the made one). I have not derived that `u` move in closed form; the
domain itself does not shift.

## `surface.surf_of_revolution_p3` (165 swap cases)

A swap case's `out` is the surface's state after the swap: flags, frame (origin, x, y),
STEP domain, angles, the largest analytic domain, the generator's extent, the polar
converter (flag, angles, curve), the generator curve and the transposed net with its knots.
The new test `every_swap_case_is_its_surface_with_u_and_v_exchanged` checks every swap case
of the golden (fixtures and 16 seeds, 192 cases, all checked): the frame is unchanged bit
for bit, the swap bit flips, and the swapped surface at (b, a) of its natural domain is the
unswapped one at (a, b), to 1e-12 of the surface's size on a 9 x 9 grid.

## `topology.api_files` (19 cases)

The written records differ only in the `SmPlane UV Scale` lines of a box's swapped faces:
main writes 0.9999999999763531, 1.0000000000521443 and the like from the refit, here exactly
1.0. Each of the 19 files, written by main and by the head, reads back with
`solidcore-diag` to the same body: face and edge counts, surface kinds, edge lengths and
tolerances bit for bit equal.
The 19 cases: write_bytes0030, write_bytes0045, write_bytes0049, write_bytes0084, write_bytes0090, write_bytes0107, write_bytes0135, write_bytes0138, write_bytes0146, write_bytes0152, write_bytes0159, write_bytes0171, write_bytes0173, write_bytes0175, write_bytes0186, write_bytes0194, write_bytes0198, write_bytes_box_far_ascii, write_bytes_box_far_binary.

## `primitive.api_trim_ops` and `query.face_p4`

The two api_trim_ops cases reorder the same values (gen0073, gen0112); the 11 other lines a
full bless rewrites there pass main's golden and keep main's line. The four face_p4
cases build a natural face on a swapped cylinder that failed with 1001 on main
(`query.face_p4--natural-face-on-a-swapped-cylinder`).
