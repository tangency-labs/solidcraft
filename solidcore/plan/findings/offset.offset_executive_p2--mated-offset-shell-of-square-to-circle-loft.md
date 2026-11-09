# offset.offset_executive_p2: the mated-offset flag alone shelled a square-to-circle loft into a wall with no volume (2026-10-31)

- **Unit:** `offset.offset_executive_p2` (`OffsetExecutive::shell_brep`), with `offset.offset_executive_p1`'s `piecewise_merge` under `Kernel::fixes.offset_extended_edges_mated` (WR-344).
- **Issue:** tangency-labs/solidcore#1818.
- **Inputs:** a ruled loft from a 40 x 40 square at z = 0 to a circle r 15 at z = 60 (`create_skin_primitive`, four curves a section, capped; 6 faces, 12 edges, 8 vertices, volume 67644.0205), shelled -1 open at its top, extended convex edges, merged results, offset solid made. Operand file `fixtures/offset.offset_executive_p2/data/issue1818_square_to_circle_loft.smb` (Rust-written ASCII, generator `write_issue_1818_operand`).
- **Values (before the fix):**

  | Fixes | Result |
  |---|---|
  | none (upstream) | `Err` 1001, `boolean::merge::p2::make_faceuse`; pinned C++ the same (difftest case `issue1818_square_to_circle_loft_api_shell_1_top`) |
  | `offset_extended_edges_mated` | `Ok`, 10 faces, 16 edges, 16 vertices, integrity ok, not a manifold solid, volume 0, pointer check: 8 reports (edge ends 15.42 from their vertices) |
  | the above + `shell_empty_cavity_fails` | same `Ok` (the offset is a closed manifold body: 6 faces, volume 55963.44; 56219.20 on the default path) |
  | SolidCraft's fixes (`issue_1476_swept_bar_shell.rs`) | `Ok`, 10 faces, 20 edges, 16 vertices, not manifold, volume 0 |
  | the above + `offset_closes_loop_gaps`, or that flag alone | `Err` 1001, `offset::offset_executive::step_gaps` |
  | circle-to-circle control, every row | `Ok`, manifold, 6082.125217 (closed form 6082.123377) |

- **Which it is:** not a port deviation: the pinned C++ with only the fork's 85bbfaf line applied returns the same `Ok` body as the Rust flag (difftest, identical). Not a missing companion flag: the fork's fixes that predate 85bbfaf (`shell_empty_cavity_fails`, fbe43bb) and SolidCraft's whole set leave it; only the later WR-578 loop-gap closing refuses it, by failing in `CloseLoopGaps`. The fork's SMLib has the same defect; its C API's `finish` masks it (`SMLIB_ERR_NOT_SOLID`). Record: `plan/upstream-issues/WR-344--addendum-2026-10-31-shell-of-a-mated-offset-returns-no-solid.md`.
- **Fix:** the flag now refuses (`SM_ERR`, `offset::offset_executive::p2::shell_brep`) a shell of a closed manifold solid whose result is no closed manifold solid (only with extended convex edges, where the flag acts). Default path unchanged.
- **Open:** the offset itself. The pointer check of the intermediate offset (before `ReplaceImplicitOffsets`) reports edges whose ends lie 15.42 from their vertices on both paths; upstream's merge refuses that offset, the mated path's merge does not. Why the side faces' offsets end there (the faces are tangent at the circle and meet at 90 degrees at the square) is not investigated.
