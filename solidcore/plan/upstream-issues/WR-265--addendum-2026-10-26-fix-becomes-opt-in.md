#### Updates: WR-265 (2026-10-26). The fix becomes opt-in; the default is upstream's

- **Change (following tangency-labs/solidcore#1077, as WR-255 / WR-259):**
  `put_outer_shells_first` (`crates/solidcore/src/boolean/merge/p4.rs`, after step 8) runs
  only with the opt-in `Kernel::fixes.boolean_outer_shell_first` (`UpstreamFixes`, off by
  default). By default the Boolean leaves step 6's shell order, as upstream does.
- **Correction: the fix changes more than shell order.** The record says "only the shell
  order is wrong" and that `set_region_is_void_flags_for_nested_solids` keeps step 7's
  void flags on a manifold solid. That is not true of the port's implementation in
  general. The routine sets every region's void flag from the nesting walked from the
  infinite region. Where step 7 left flags that disagree with that nesting, the result's
  void flags change too. Seen in the SolidCraft corpus: `451-hollow-shell-cut`
  `block_crossing_inner_sphere_join` gives volume 268082.57 without the field and
  241446.07 with it (expected 242214.07, so it fails either way), and a scratch SolidCraft cup-and-lid case
  with the lid first gave -488000 with the field instead of 512000 (expected 488000). In
  both, the Rust result already diverged before the rewrite. The field's doc says so.
- **Confirmed on the C++ build:** `boolean.api_brep` cases `wr265_cup_lid_cup_first`,
  `wr265_cup_lid_lid_first` (the trigger, either operand order),
  `wr265_void_then_union` (a cube with a sealed void united with a bar through it) and
  `wr265_void_by_difference` pass at the default. The runner's record compares counts and
  `IsManifoldSolid`, not shell order or void flags, so these four pass either way. The
  record's claim that "unions ... of a body that already has a void keep the right order"
  does not hold for `wr265_void_then_union`: with a shell-order record (not committed),
  pinned C++ puts the cavity shell first there too.
- **Correction:** the record's "The regression test ... cannot exercise it yet" no longer
  holds: the whole cup-and-lid union runs in Rust.
- **Tests:** `issue_388_union_sealing_a_void_puts_the_outer_shell_first` (field on: outer
  shell first, either order) and `union_sealing_a_void_leaves_the_cavity_shell_first_by_default`
  (the cavity's shell first, as C++), in `crates/solidcore/src/boolean/merge/p4.rs`.
- **Callers** that relied on the previous default (e.g. SolidCraft, #388) must set the
  field.
