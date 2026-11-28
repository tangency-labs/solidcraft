Updates: WR-545

#### WR-545 (update, 2026-11-23). `shell_edges_on_analytics_exact` is removed; every shell ends with its exact analytic edges

`ShellBrep` (`offset/offset_executive/p2.rs`, `make_shell_edges_exact`) always ends with
`make_edges_on_analytics_exact`: each marched edge between two faces on analytic surfaces takes the exact line or
conic the surfaces meet on through its ends and middle (`exact_piece`), and a vertex moves onto it only
within the tolerance. An edge whose exact curve cannot be found or checked (any error but a missing callee or a
cancellation) keeps its marched curve instead of failing the shell, so the pass cannot turn a shell that succeeded
without it into a refusal through one edge. Removed: the field and its doc file, the early return, the flag-off pins of
`issue_1950_shell_analytic_edges_face_owned` (the two volumes without the pass) and of
`issue_1476_swept_bar_shell` (the default kernel's 16 lines; the default now gives the former flag-on test's 18), and
`issue_1476`'s SolidCraft run with the flag off, now the same run as its flag-on twin, which is folded into it.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** 64 rows move, none in topology, counts or status, none to a refusal. `offset.offset_executive_p2` 62:
  59 by `result.edge_kinds` (B-spline edges becoming lines), 33 by `result.geometry` and 3 by
  `source.after.geometry` (vertex bounds, by 1e-16 to 1.7e-12 against tolerances of 1e-6). `blend.fillet_executive_p3`
  2 (`real-box-fillet{1v,4v}-r3-shell-closed`): `shell_geometry` (the rim's edges are now exact arcs and lines,
  parameterised as such) and `shell_mass_properties`, whose areas move closer to the closed forms `3444 + 23 pi` and
  `3168 + 92 pi` (main's were 1.9e-6 and 7.4e-6 short) and whose volumes move 1.1e-10 and 2.2e-10 further from
  `3420 + 21 pi` and `3168 + 84 pi`, inside the 1e-4 accuracy asked for.
- **Tests:** `goldens/tests/wr545_moved_rows_are_exact.rs` re-runs each of the 64 rows, checks it reproduces the
  committed row, certifies every line and conic edge between analytic faces on each face's implicit surface (a line
  on a surface of extrusion, which has no implicit, as one of its rulings within the surface's STEP domain), requires
  every line and conic edge the pass can have made to be certified, checks each edge's start and end vertex against
  the curve's start and end and every moved bound against the tolerance, and pins the two shell masses against their closed forms.
- **Ledgers:** the two shelled boxes are rows of the trim-tolerance ledgers. Their vertex entries that the exact
  edges move again (1v: 6, 4v: 20) are dropped from `certify/trim_tolerance_rows.jsonl`, `trim_tolerance_other.tsv`
  and `operand_trim_rows/fillet_p3.rs`, whose digests now include WR-545's moves; `trim_tolerance_main.tsv`'s
  `blend.fillet_executive_p3` (`6cf95a481143e5dd`) and `offset.offset_executive_p2` (`c6012e3e9432cae6`) digests
  are recomputed with a comment, as after #18490, #20179 and #18916.
