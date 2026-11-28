Updates: WR-792

#### WR-792 (update, 2026-11-27). A keep list built by testing every edge does the same

The removal no longer tests an edge on its keep list. But a caller that builds its keep list with `GetTopologicalEdges` (the port's `topology::brep::p1::topological_edges`) still tests every edge first, on the B-rep itself. The test prepares the merge of each edge it accepts: the covering face's surface is expanded in place and its UV trims are dropped. Only a merge sets the face's domain anew, so an accepted edge that is then kept leaves its face on the expanded surface under its old domain.

- **Trigger:** tangency-labs/solidcraft#23077.
  - A small patch of a revolved sphere face, cut across the face's seam: two faces joined by a piece of the seam. It is thickened by 0.03.
  - The sheet-thicken wall merge (`offset::offset_executive::p2::merge_sheet_side_walls`, the fork's code, not upstream) tested every edge of the thickened shell, then kept all of them but the topological ones between two walls.
  - On the seam piece between the two offset faces, the cover expanded one face's surface over the other's longitudes (0° to 11.8° and 348.2° to 360°), to the whole turn, and `AdjustSTEPUVDomain` rebuilt its NURB on the STEP domain.
  - Afterwards that face's surface ran over 0 to 1 by 72.66 to 78.69 (degrees), while its face domain still said 0.965 to 1 by 0.399 to 0.601.
  - The edge was kept, so nothing merged. solidcore-ops refused the result as not a closed solid ("its pointer check fails"), and SolidCraft's export refused the face's trims as off the spline's knot range.
- **How found:** reported by SolidCraft (the issue's steps through SolidCraft's kernel interface). Traced in the port by printing each offset face's domain and its surface's natural domain before and after each step of the thicken. The surface changed inside `merge_sheet_side_walls` → `topological_edges` → `edge::is_topological` → `cover_other_coincident_surface` → `adjust_step_uv_domain`.
- **Confirmed:** measured in solidcore, no C++ run.
  - The kernel cases `shell/23077-thicken-sphere-patch-across-its-seam` and `-r50` fail before the fix ("not a closed solid") and pass after it, at the closed-form volume.
  - Golden digests of the touched units are unchanged.
- **Fix in solidcore:**
  - `topological_edges` takes the B-rep by shared reference and tests on a trial copy (`clone_for_trial_keeping_definitions`). The expansions land on the copy, in the same edge order, so the list is unchanged.
  - The wall merge asks on a trial copy whether any edge between two walls is topological, and asks nothing when it does not merge. The removal asks again, on the shell, and merges what it accepts.
- **Unconfirmed:**
  - The fillet executive's piecewise merge (`blend::fillet_executive::session::piecewise`, upstream's `GetTopologicalEdges` keep list) now gets the copy's list too. No failing case is known there, and its golden digests are unchanged.
  - `offset::offset_faces` tests its joins on the B-rep and the removal tests them again. An edge the second test turns down keeps an expanded face. No case is known.
