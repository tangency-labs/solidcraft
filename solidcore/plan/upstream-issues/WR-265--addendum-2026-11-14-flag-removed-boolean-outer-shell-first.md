Updates: WR-265

#### WR-265 (update, 2026-11-14). `boolean_outer_shell_first` is removed

After step 8, a Union, Difference or Intersection whose result is a manifold solid with more than two regions runs `set_region_is_void_flags_for_nested_solids` (`put_outer_shells_first`, `boolean/merge/p4.rs`), so every region's enclosing shell is first and its void flags follow the nesting. That is the only path now, and the flag is gone. SolidCraft already ran with it on. Removed: the field `UpstreamFixes::boolean_outer_shell_first`, SolidCraft's preset line, its entry in `fixes.json`, its name in the SolidCraft profile list of `issue_2260_gn_pipe_refined_range_end.rs` and every test setting of it.

- **Which path went:** the upstream one, which leaves step 6's shell order, so a union that seals a void can put the void's shell first and fail `SmBrep::AssertValid` test 12.
- **Results:** 3 golden rows of `boolean.api_brep` move (`wr265_cup_lid_cup_first`, `wr265_cup_lid_lid_first`, `wr265_void_then_union`): the recorded shell order, outer shell first. Each was attributed to this flag alone.
- **Tests:** `issue_388_island_in_void.rs` and the unit tests in `boolean/merge/p4.rs` lose their flag-off halves (the C++ order is no longer reachable). `outer_shells`'s unit test in `topology/brep/outer_shells.rs` needs a solid whose void is listed first: it reverses the list the Boolean returns.
