Updates: WR-524

#### WR-524 (update, 2026-11-14). `fillet_3x2_mixed_concave_side_edge` is removed

A 3x2 mixed-convexity corner now always takes its two fillets' roles from the unfilleted edge's convexity (`cross_uses_convex_fillet` in `FilletCorner::make_3x2_mixed_corner_topology`), as tangency-labs/smlib@1255993; upstream's fixed L-shaped-box roles are gone. SolidCraft already ran with the fix on. Removed: the field `UpstreamFixes::fillet_3x2_mixed_concave_side_edge`, SolidCraft's preset line, its entries in `fixes.json` and the goldens name map, the flag-off branch, and every test setting of it (the issue test's flag-off half, which pinned upstream's status 1001, with it).

- **Results:** 12 `blend.fillet_executive_p3` cases move, all two-box unions rounded r 1 at such a corner (`corner3x2-union-overhang-front-*`, `corner3x2-union-inset-side-step-*`, `stubs3-union-inset-side-step-edges16-22-r1` and `-17-22-r1`): each was refused with status 1001 and now builds one manifold solid that passes `check_brep` Level 2. Its V/E/F are exactly 23/36/15 (overhang) or 25/38/15 (inset): the 12 faces gain the two rounds and the corner patch. Each round's face is a cylinder of radius r whose axis runs along its edge, r from both of the edge's faces, on the side the boxes' convexity gives. These are the cases the original record lists as matching the pinned C++ with tangency-labs/smlib@1255993 applied, `max_rel_err=0`.
- **Tests:** `crates/solidcore/tests/issue_1475_mixed_corner_concave_side_edge.rs` (`corner_builds_with_the_fix`, `box_union_corners_with_swapped_roles_build`).
