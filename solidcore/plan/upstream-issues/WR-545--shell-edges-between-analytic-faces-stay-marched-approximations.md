#### WR-545. `ShellBrep` leaves the edges between analytic faces as marched approximations, not their exact lines and conics
- **Severity:** Inexact result (edges 2.4e-6 off their circles, read back as splines)
- **Area:** Offset (shell)
- **Location:** `source/SMLib/src/SmOffsetExecutive.cpp`, `SmOffsetExecutive::ShellBrep`. The shell's edges, where its offset faces meet each other or the open face, come from the offset executive's and the boolean's marched intersections and keep their B-spline approximations.
- **What happens:** a rounded box (R6 uprights) shelled 2 inward, open at the top, had the inner quarter circles of its rim as cubics 2.4e-6 off the R4 circles. SolidCraft read them back as splines: a sketch projected from the rim came apart at its corners, and the lip refused the rim's edges.
- **Trigger:** any shell whose result has an edge between two faces on analytic surfaces that meet on a line or conic.
- **How found:** SolidCraft (robustness `lip_on_a_rounded_shelled_rim`, the sensor enclosure's lid lip). The fork's fix is tangency-labs/smlib@cd85d34 ("shell: edges between analytic faces are their exact lines and conics"), which is not merged upstream.
- **Confirmed:** no C++ run was made here. The Rust port follows the pinned C++ (`3fd5b95`).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.shell_edges_on_analytics_exact`, `shell_brep` ends with `make_edges_on_analytics_exact` (the fork's `MakeEdgesOnAnalyticsExact`) on its result, but not on the non-manifold offset it returns early:
  - Each edge whose curve is not analytic, between two faces on analytic surfaces, takes the line or conic of the surfaces' own intersection through its ends and middle (`exact_piece`, as WR-495 uses), running as the edge did.
  - A conic that fails its validity check is not taken.
  - The changed edges' faces get their UV trim curves made again.

  Test: `tests/wr_545_shell_rim_arcs_exact.rs`.
