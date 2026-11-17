Updates: WR-513

#### WR-513 (update, 2026-11-15). `fillet_vertex_error_diagnostics` is removed

`create_fillet_corners_recording_vertex_errors` (`crates/solidcore/src/blend/fillet_executive/p1.rs`) now always notes a vertex no corner type handles as `FilletErrorType::CornerUnsupported` ("Corner case not implemented", with the vertex and its location), unless a refused corner was noted first. `FilletSession::do_fillet` (`session/finish.rs`) keeps that note in `error_info()` across the later stage's failure. The returned error is still the later stage's own status, and the pending vertex entry and the operation's result do not change.

Removed: the field, SolidCraft's preset line, its entries in `kernel_cases/fixes.json` and `issue_2260_gn_pipe_refined_range_end.rs`, and every test setting of it.

The default kernel now reaches the note's restore, so it is guarded:
- `do_fillet` keeps only a note its own corner stage wrote, checked against a snapshot of the error info taken before that stage, not one left on the session before.
- It does not keep the note over a later no-room refusal (a NoRoom code, or a `no_room` record that differs from the snapshot). The callers read that refusal's code, reason and fit size (`offset_faces` chooses `OffsetFacesRefusal::NoRoom` from the code).

Not changed: `create_corner_vertices` (`fillet_executive/p3.rs`) still tells a corner's own unsupported note from an earlier vertex's by the code only. A second unsupported corner after a first falls to the generic topology failure, as on main and under SolidCraft's preset.

- **Results:** no goldens case changes.
- **Tests:**
  - The flag-off halves of these tests are removed; the fixed halves keep every assertion:
    - `create_corners_notes_an_unsupported_vertex_only_when_opted_in`, now `create_corners_notes_an_unsupported_vertex`;
    - `create_corners_consults_the_vertex_diagnostics_fix`, now `create_corners_notes_the_unsupported_vertex`;
    - `do_fillet_keeps_the_missing_corner_note_or_the_same_later_failure`, now `do_fillet_keeps_the_missing_corner_note_through_the_later_failure`. Its stale-note case, which tested only the flag off, goes too.
  - New: `do_fillet_does_not_keep_a_stale_missing_corner_note`. It seeds a stale note before a failing r 5 round on a unit cube's edge (code 4, not a no-room refusal, no corner) and checks that the reported error info equals the run without the seed.
  - Changed: `create_corners_preserves_order_pending_and_final_error_assignment` ran on the default kernel and checked the pinned no-op: the error output kept its earlier code 17 after an unsupported vertex. It now checks that the output carries that vertex's note, code 9 (`CornerUnsupported`) and vertex H(2), as `create_corners_original_factory_shape_still_notes_without_a_location` already checked for the same factory shape with the flag on.
