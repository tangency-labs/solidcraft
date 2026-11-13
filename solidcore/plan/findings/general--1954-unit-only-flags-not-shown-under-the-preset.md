# General: three shipped flags pinned only by unit tests show no operation-level difference under SolidCraft's preset (2026-10-31)

**Context (#1954).** A reachability pass ran on main 94d020e5 over the 80 `UpstreamFixes` flags added since the audit base d1c6fc48a. It used tdamsma's method: one instrumented run of solidcore's and difftest's tests, then each hit rerun with that flag forced off.
- 78 of the 80 flags have a test that fails forced off.
- Among the flags SolidCraft ships (#8526's `fixes()` set, less `HELD_BACK`) in fillet, boolean and offset code, eight are pinned only by unit tests.

**Covered or deferred:**
- **`fillet_nx1_tangent_side_edge_same_convexity` (WR-600):** now covered at the operation level by #2099's peg tests in `tests/fork_856b54e_tangent_end.rs`. With the flag off a chamfer fails with 1001; with it on it builds at its closed-form volume.
- **`fillet_run_on_past_far_end_only` (WR-643):** an open follow-up. With it off, those same peg blends (the WR-600-on chamfers and round) still build at their closed forms: front chamfer 9341.798, concave chamfer 9457.586, concave round 9434.384. So they don't exercise it. Showing it needs its own input, a rail that runs past only the far end of its side edge.

**Unit-only, no operation-level difference shown.** Each record's trigger was built and run with SolidCraft's shipped preset, the flag off and then on:

| flag | input | flag off | flag on | what probably masks it |
|---|---|---|---|---|
| `fillet_split_edge_keeps_near_half` (WR-612) | 60 x 40 x 20 box; the three edges at (60, 40, 20) r 5, G2 blend at thirds (cross section 2, continuity 2), set back and eased: setback/ease 12/0.5, 12/1.25, 13/0.5, 13/5, 12/3 | a solid, 15 F / 36 E / 23 V, no bad edges; 47403.364 at setback 12, 47371.031 at 13 | identical | `fillet_corner_face_curve_halves` (WR-659, shipped) makes an eased corner's patch sides the face curves' own halves, so both halves match the curve to scaled zero and the near-half fallback is never needed. The fork's trigger was the *open* eased setback (`smlib_blend_setback_open_eased`), which has no solidcore entry point. |
| `imprint_extension_keeps_analytic_curve` (WR-601) | WR-593's peg: a stadium bar (r 5, length 20, height 10) joined with the cylinder of its end radius above its round end | the bar-top/post-wall edge is an `SmCircle`, end gaps 1.8e-15 and 7.9e-15; a solid, volume 3570.796330 | identical | This construction's circle already reaches its proto vertex, so the imprint continuation never runs. The fork's arc stopped 5.4e-10 short, from SolidCraft's sketch residues. |
| `imprint_extension_keeps_analytic_curve` (WR-601), with a residue | the same peg with a 1e-10 sketch residue in the bar's outline (its first side line starts 1e-10 off y = -5, as in WR-593's test) | the same `SmCircle`, end gaps 1.8e-15 and 7.9e-15; a solid, volume 3570.7963300496613 (3570.796330046521 without the residue) | identical | The residue moves the volume but still leaves the circle reaching its proto vertex. The fork's 5.4e-10 shortfall comes from SolidCraft's own sketch solve and import, which a solidcore-built peg doesn't reproduce. |
| `fillet_3x2_tangent_side_edge_roles` (WR-597) | the record's trigger: a block 40 x 20 x 15 running tangent into an R 10 x 30 post, its long top edge and the concave arc rounded r 2 together | 1001 (`session::finish`) | 1001 | As WR-597's record says, the round fails with either role choice, at a later step this flag doesn't cover. Nothing to show until that's fixed. |

**Status.** No tests were added. Each flag's branch stays pinned by its unit test. An operation-level pin for WR-612 needs an input WR-659 doesn't already cover, and one for WR-601 needs SolidCraft's own imported peg, whose circle stops short of its proto vertex. WR-597 needs its unrelated corner failure fixed first.
