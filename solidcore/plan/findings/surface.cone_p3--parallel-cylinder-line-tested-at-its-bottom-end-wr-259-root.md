# surface.cone_p3 — parallel-cylinder line tested at its bottom end (WR-259, root cause of FAIL-3665)

`fixtures/solidcraft/3665-column-in-channel.json` failed in all four cases at the cut of
the r 3.04 cylinder centred at (0, 20) (`boolean::merge::p2::make_faceuse` or
`topology::brep::p6`, status 1001), and so does the C++ reference (FAIL-3665).

Cause: `SmCone::IntersectWithCone` (`SmCone.cpp:2950`-`2971`) tests the line of two
parallel cylinders only at its bottom end, v = 0 of the full cylinder. The column's
cylinder face has a domain trimmed by an earlier Boolean to v in [0.8, 10.5], so the
point falls outside it and the crossing line is dropped with `SM_SUCCESS`. The imprint
chain stays open and `FindRadialSector` finds no edge use. Reproduced on the C++
reference with the six `parallel_cylinders_*` cases in
`crates/difftest/src/units/surface_cone_p3.rs`: 2 lines on the natural domains, 0 when a
domain's v minimum is above 0, the same in Rust.

Change: the Boolean opts into testing the line at its middle
(`Kernel::cone_parallel_midpoint_test`, set and restored by
`boolean::merge::p4::manifold_boolean`); the default stays upstream, so `surface.cone_p3`
still matches the C++ bit for bit. SolidCraft cases on main 70146d6fc: PASS 28 -> 32,
ERROR 4 -> 0, FAIL 0, DEFERRED 29 unchanged; the four 3665 cases pass against their
closed-form volumes. Ceiling: a line that overlaps both domains only away from its middle
is still dropped (upgrade: clip `[dMin, dMax]` to both domains' v ranges). Ruled out:
WR-214 (`split_self_loops_on_vertices` is already off on this path).
