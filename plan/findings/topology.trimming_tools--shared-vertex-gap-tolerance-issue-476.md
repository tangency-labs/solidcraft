# topology.trimming_tools: shared-vertex gap tolerance (issue #476, PR #691, 2026-10-24)

`CheckLoop` used only the sum of the incident edge zone tolerances for its
3D endpoint gap report and the subsequent non-seam UV-gap certification. A
widened shared vertex can cover a gap that this edge-only limit rejects. The
Rust limit is now `max(edge_zone_1 + edge_zone_2, shared_vertex_zone)`.
The current start vertex use and the previous edge use's **mate** vertex use
must identify the same vertex. Missing or different vertices retain the edge
limit. This preserves the old edge allowance, does not enlarge any stored model
tolerance, and does not bypass periodic seam or singularity tests.

The new Rust regression failed before the change: gap `1.3e-5`, edge zones
`5.8e-6` each, vertex zone `2.6e-5` returned closure **3**, expected **0**.
Two regressions cover the inclusive vertex boundary and a gap just outside it,
a tighter vertex with the old edge allowance, different/missing vertices,
closed edges in both orientations, and two distinct edges meeting at a widened
vertex whose previous **start** vertex is tight. The latter walks both
orientations and both list starts. This is a local trimming-check fix; no
end-to-end cylinder union is claimed.

The native reproduction edits the U coordinate of one trim's first NURBS
endpoint on the existing public `plane_square.smb`, then sets explicit model
edge/vertex tolerances on both sides. The preparation's edit status is compared;
C++ `SmTrimmingTools::CheckLoop`, the comparator and its tolerances are unchanged.
For edge uses **0, 1 and 3**, the `1.3e-5` edit produces a measured gap
`1.3000000000040757e-5`. Unchanged C++ returns success with closure **3** and a
failed report at limit `1.16e-5`; Rust returns success with closure **0** and no
failure, since `2.6e-5` covers it. These are C++ correctness differences (WR-381),
not parity passes. At requested gap `2.61e-5`, both return closure **3** for measured
`2.6100000000361945e-5`; their failed-report limits differ (`1.16e-5` C++ versus
`2.6e-5` Rust). Those diagnostic differences are also retained explicitly, with
assert reporting enabled. The eu2 edits leave the queried endpoints unchanged
on both sides and are controls, rather than additional claimed reproductions.
Zero edits and gaps covered by the existing edge limit provide passing controls.
The earlier exploratory thin B-spline strip had no UV trims (its mirror step
was skipped); it is not used as evidence or committed as a regression.

Twelve native cases deliberately disagree and are individually listed in
`crates/difftest/known_mismatches/topology.trimming_tools.toml`:

| Case | C++ | Rust |
| --- | --- | --- |
| `vertex_gap_covered_eu0` | closure 3, one failed gap report | closure 0, no failures |
| `vertex_gap_covered_eu1` | closure 3, one failed gap report | closure 0, no failures |
| `vertex_gap_covered_eu3` | closure 3, one failed gap report | closure 0, no failures |
| `vertex_gap_too_wide_eu0` | closure 3, report limit 1.16e-5 | closure 3, report limit 2.6e-5 |
| `vertex_gap_too_wide_eu1` | closure 3, report limit 1.16e-5 | closure 3, report limit 2.6e-5 |
| `vertex_gap_too_wide_eu3` | closure 3, report limit 1.16e-5 | closure 3, report limit 2.6e-5 |

Six existing mirrored `Mouse.smb` cases also retain their operation status and
all other compared values but report the shared vertex's larger gap limit.
These are diagnostic differences, not six additional accepted-gap fixes:

| Case | C++ `values.out[9]` | Rust `values.out[9]` |
| --- | --- | --- |
| `check_loop_mirror_Mouse_0_0_0` | 6.95115100281493e-4 | 2.1819300674131145e-3 |
| `check_loop_mirror_Mouse_0_0_1` | 6.95115100281493e-4 | 2.1819300674131145e-3 |
| `check_loop_mirror_Mouse_0_1_0` | 6.912139700099413e-4 | 2.1819300674131145e-3 |
| `check_loop_mirror_Mouse_0_1_1` | 6.912139700099413e-4 | 2.1819300674131145e-3 |
| `check_loop_mirror_Mouse_0_3_0` | 6.95115100281493e-4 | 2.1819300674131145e-3 |
| `check_loop_mirror_Mouse_0_3_1` | 6.95115100281493e-4 | 2.1819300674131145e-3 |

Validation on main `cf4b7945e` plus this change: fmt; default and native workspace
Clippy with `-D warnings`; 2,081 kernel tests; the curve-point-handle integration
test and four primitive integration tests (one existing unsupported partial
sphere/torus test remains ignored); wasm; 46 difftest tests and one float-roundtrip
test all pass. Fresh native runs: `topology.trimming_tools` 3,279 cases, 3,267
matching and 12 explicit known mismatches; `topology.edgeuse_p1` 4,402 matching;
`topology.vertexuse` 2,686 matching. Zero unexpected failures or deferrals.
All eight healthy edge-allowance/zero-edit controls detect perturbations (4/4
and 4/4). The trimming unit remains partial, and no full macOS `--green` result
or unrun fork CI is claimed (known reference issues #448–450 remain).
The real-push rebase resolved only the appended known-manifest conflict,
preserving main's 16 entries plus this change's 12, and kept the findings index last.

## Stacked on main: opt-in (2026-10-26)

The fix above is applied only with `Kernel::fixes.check_loop_vertex_tolerance` (WR-381,
default off, following the precedent of the other `UpstreamFixes` fields). With the default,
`CheckLoop` is SMLib's and the twelve cases listed above match C++, so
`crates/difftest/known_mismatches/topology.trimming_tools.toml` is not needed and was not
carried over. The `issue_476_*` unit tests set the field and fail with it off;
`issue_476_default_keeps_the_edge_only_gap_limit` pins the default.
