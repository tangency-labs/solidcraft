# Sweeps: two fixes become the default, 3 golden rows move (2026-11-12)

- Flags retired (tangency-labs/solidcraft#14246, batch B43): `sheet_sweep_joins_profile_wires`
  (WR-409) and `sweep_frame_on_planar_path_normal` (WR-407; its other half,
  `cover_refuses_folded_continuation`, stays opt-in). Each WR record has an addendum of
  2026-11-12. The other flags read in `p3/sweeps.rs` (batch B42) are untouched.
- `cargo run --profile goldens -p goldens -- --unit sweep.api_brep --bless` at the fix:
  696 cases, changed=3, added, removed and broken 0. These are the three rows brett19's
  survey lists for B43.

## WR-409: a drilled hole's open profile revolved part way is built (2 rows)

`issue1463_drilled_hole_pz_120` and `issue1463_drilled_hole_mz_120`
(`fixtures/sweep.api_brep/issue1463-inside-out-revolve.json`): the open chain of the line
(2, 0, 5)-(2, 0, 1) and the line (2, 0, 1)-(0, 0, 0), revolved 120 degrees about +z and
about -z, without caps.

| row | old | new |
|---|---|---|
| `issue1463_drilled_hole_pz_120` | status 1001 | status 0, counts [2, 6, 5] |
| `issue1463_drilled_hole_mz_120` | status 1001 | status 0, counts [2, 6, 5] |

Upstream makes each profile curve a wire of its own, so the topology sweep's copies of the
two coincident vertices at (2, 0, 1) come out coincident and its stitch refuses them
(WR-409). With the wires glued there the sheet has two faces, 6 edges and 5
vertices: the profile's three and the turned copies of the two off the axis. The full
turns of the same profile (`_pz_360`, `_mz_360`) do not move.

The new rows are checked, not only counted: the regression test
`issue_1463_drilled_hole_profile_revolves_part_way_into_a_joined_sheet` (`p3/sweeps.rs`)
builds both on `Kernel::default()` and finds them valid at Level 2 (`check_brep`), both
faces cones (the native cone class; the wall's has half-angle 0), and the face areas
within 1e-8 of Pappus' closed forms: the wall 2 pi / 3 * 2 * 4 = 16 pi / 3 and the cone,
slant sqrt 5 about mean radius 1, 2 pi sqrt 5 / 3. Measured at accuracy 1e-9, both
axes: 16.755160819285 against 16.755160819146 and 4.683209820733 against 4.683209820694,
8.3e-12 relative each.

## WR-407: a curve sweep along a planar path moves by roundoff (1 row)

`box_face1_curve_path_cap` (`fixtures/sweep.api_brep/box-face-curve-sweep.json`): face 1
of the box 2 x 3 x 4 swept, capped, along the cubic through (0, 0, 4), (0, 0, 6),
(1, 0, 8), (3, 0, 9), which lies in the plane y = 0. Its start tangent is +z, so
upstream's `MakeUnitOrthoVectors` frame and the frame on the plane's normal differ, and the
path and profile are turned differently before the sweep.

| row | old | new |
|---|---|---|
| `box_face1_curve_path_cap` volume | 30.2282600710325 | 30.228259877867142 |

The volume moves by 1.9e-7 (6.4e-9 relative); counts, face groups and the volume status
stay. The sweep approximates its side faces, so the volume depends on the frame's
rounding; neither value is closed form.
