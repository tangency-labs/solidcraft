Updates: WR-445

#### WR-445 (update, 2026-11-06). The fix is the default; its flag is retired

`Kernel::fixes.fillet_setback_from_start_vertex` is gone and its path is the only one:
`set_back_frame` (`crates/solidcore/src/blend/fillet_geom/p2.rs`) walks the edge's own interval
from the end the set-back vertex marks, a negative arc length from the other end. Upstream's walk
from `GetVertex()` over the curve's natural interval is deleted.

The record's trigger, the box 60 x 40 x 20 with r5 rounds at (60, 40, 20) set back 8, built with
the default kernel, had volume 47241.18 and its x edge's round ended at x = 8, with the corner
patch running from x = 8 to the corner and 0.255 past the box. It now has volume 47325.94, inside
the convexity bounds [47271.976, 47384.893], and the six section ends lie on the setback planes
x = 52, y = 32 and z = 12 (`crates/solidcore/tests/issue_2933_setback_from_the_filleted_vertex.rs`).
The two unit tests in `p2.rs` check the default kernel on every end of every edge of a box and on
edges whose interval is the middle of a longer line.
