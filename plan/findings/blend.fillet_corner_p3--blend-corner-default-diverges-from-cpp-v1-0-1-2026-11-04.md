# blend.fillet_corner_p3: with the blend corner the default, 13 units disagree with C++ v1.0.1 on 381 cases (2026-11-04)

- **Units:** `blend.fillet_corner_p2` (2 cases), `blend.fillet_corner_p3` (95),
  `blend.fillet_executive_p3` (155), `blend.fillet_geom_p4` (23), `intersect.api_intersectors`
  (1), `intersect.surface_intersector_p2` (40), `primitive.api_trim_ops` (9), `surface.torus`
  (12), `topology.api_files` (2), `topology.api_projection` (5), `topology.brep_cutting` (19),
  `topology.brep_local_operation` (6), `topology.brep_planar_section` (12).
- **Change:** tangency-labs/solidcore#2400 removes `UpstreamFixes::fillet_blend_corner`: the
  default kernel is the blend corner (WR-697's addendum of 2026-11-04), and the old corner code
  it replaced is deleted. The C++ reference (v1.0.1, `scripts/upstream-pin.env`) runs the pinned
  corner code, so the cases whose result the blend corner changes no longer agree with it.

## What the cases are

Every one is the flag's result, not the deletions': on main (9d6dba1b8) with
`fillet_blend_corner` and `fillet_nx1_tangent_side_edge_same_convexity` on by default (a local
build, not committed), the same 13 units give the same outcome and the same first mismatch on
every case they share with this branch. Which of the blend corner's behaviours (WR-697's
addendum lists them) moves each case was read off for these:

- `blend.fillet_corner_p3`'s `create` cases: `Create` reports corner type 9 (the Join, or the
  End at a smooth vertex) where upstream reports 8 (Nx1), 2 (degenerate), 3 (1x1) or 4 (2x2);
  in 8 cases the corner made has no closed Nx1 `m_bG1Case` (`-1`) where upstream's is `0`.
- `blend.fillet_corner_p2`'s two tangent-solver cases: the closed Nx1 corner's
  `GetTangentFilletSolver` override is deleted, a closed edge meeting itself G1 being the Join.
- `surface.torus`: its only flag site is the horn torus kept by
  `SmTorus::RebuildSTEPFromNURBParameters` (af7a4d9).
- `intersect.surface_intersector_p2`: its flag site is `FlushCurve`'s short last span, merged
  into the span before only when the merged span keeps the trace's accuracy (the record
  lengths differ).

- `blend.fillet_executive_p3`'s runner compares a refusal that names the size that builds
  (`SolidError::FilletRefused`) as its status 1001, as it compares `SolidError::Upstream`;
  it took it for a harness error before. 18 such refusals are the C++'s 1001 too and pass;
  4 (`stubs4-delete-edge-bottle-*`) are refusals where the C++ builds.

The others (`blend.fillet_executive_p3`'s end-to-end fillets, `blend.fillet_geom_p4`, and the
topology and primitive units, whose cuts, sections and projections intersect surfaces) reach
several of the flag's sites; their cases are not split by site.

Each is a known mismatch (`crates/difftest/known_mismatches/<unit>.toml`, `why` naming the
field and both values), and each unit is `partial`: the reference is not wrong, solidcore no
longer follows it there.

## Retired cases

The runners no longer generate the cases whose Rust code is deleted: corners of the 1x1 and 2x2
classes (`blend.fillet_corner_p1`, `_p2`, `_p3`), `AdjustTangentCorner`
(`blend.fillet_corner_p3`), `TestAndConvertBigRadFillet` (its curated `big_rad_*` cases, and the
`AdjustTopology` cases on a blending Nx2 corner of other than three edges, which reach it). The
geometry units' stand-in corners take the 4x3 type in place of the 2x2 one on both sides
(`blend.fillet_geom_p2`, `_p3`, `_p6`). Their disappearance, and the passes that became known
mismatches, are waived for this change (`plan/difftest-waivers.toml`).
