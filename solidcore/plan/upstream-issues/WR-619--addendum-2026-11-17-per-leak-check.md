Updates: WR-619

#### WR-619 (update, 2026-11-17). The union's solid-leak guard checks each leak, not the region count

The guard in `boolean/merge/p4.rs` (`editing_stages`) no longer refuses only a union left with no solid region
(`has_solid_region`, tangency-labs/solidcraft#14230's form). Instead, before the deletion it records, for each solid region that a deleted face separates from the infinite
region, the uses of that region's faces that stay (`solid_leaks`). After post-processing it refuses with
`BooleanFailure::SolidLeaks { face }` when, for any such region, one of those uses that is left lies in the
infinite region or a void (`leak_stays_solid`): the solid has merged into the outside. When none of those uses is
left the solid cannot be located, and the test is the old one: refuse when no solid region is left.

This catches a partial leak, where one solid merges into the outside while another solid region is left. A count of
solid regions cannot tell that from a real union of two touching solids, which also goes from two regions to one.
The partial leak is real: tangency-labs/solidcraft#16350's hole-pocketed strip union at d = 5e-6 with a separate
2 mm cube added to A returned only the cube (one solid region, volume 8) under the region count; it and its three
siblings (the notch, the cube in B) are now refused. `crates/solidcore/tests/wr_619_union_solid_leak.rs`
(`wr_619_union_leaking_one_of_two_solids_is_refused`) asserts it and fails with the region count in place of the
per-leak check. The tangency-labs/solidcraft#13455 coincident-wall joins never reach the check: they delete no face
between a solid and the outside, so no leak is recorded. No known input records a leak whose solid stays solid.
