# Public boolean e2e (PR #816): cases newly compared once the context stubs were wired (2026-10-24)

Wiring the public-path stubs let 15 cases that were deferred on main reach a comparison. After
cherry-picking the swarm's fixes (merge_p4/merge_p3 canonical loop starts 940550b4d,
b07c2bda7, 009558c98, 393565928; MakeFaceuse surface attributes 411f01356/42b55934b;
BooleanPostProcess merged domain d06819904; offset_executive_p2 live counts 45444f346;
naming-scheme queries 09706e3d6, bb1945463, 56ae09d46), these still disagree:

- **boolean.api_brep `merge_breps_three_difference`** (TestSmBrep.cpp:1105): **fixed** on PR #816.
  The second Difference's ray fire missed the faces the first merge made: they kept the
  merge engine's namespace owner names (`notification_key(0, false, Face(f))`, e.g. key
  `2<<32 | 6`) in a B-rep recorded as notify-named. The engine's namespace and attribute
  store end with the merge, so `DependencyMerge::manifold_boolean` now hands that geometry
  the record's own name in the B-rep's scheme (`kernel_adapter::adopt_namespace_owners`),
  as upstream's owner is the record itself. fixer-1's structural-ownership branch also fixes
  it, but on main it breaks `tests/constructed_transforms.rs`
  `foreign_and_unowned_geometry_remains_filtered` (an unclaimed foreign face name counts as
  owned) and needed a tolerance workaround, so it was not taken.
- **boolean.merge_p3 `upstream_merge_breps_operation_0/1/2`, `upstream_piecewise_boxes_0/1`**: the same
  values in a different vertex order (e.g. operation_0 `values.record[24]` cpp 8 / rust 10,
  `[25]` cpp 10 / rust 6). Same signature as the existing entry "boolean.merge_p3: vertex list order
  after merges that imprint edges into faces" (operation_3, piecewise_boxes_2/3), whose cause is not
  traced; the canonical loop-start digests reduce the mismatch count (operation_0: 12 -> 6) but do not
  remove it.
- **offset.offset_executive_p2 `gen0054`, `gen0134`, `gen0177`, `gen0195`, `gen0210`**: region/shell/loop
  counts differ (gen0054 counts[0..4]: cpp 2 2 11 12, rust 3 6 12 16; gen0134: cpp 3 4 32 32, rust
  2 2 26 26; gen0210 counts[3] cpp 6 rust 9). Same family as gen0000/gen0003/gen0166 above (Rust
  merge region/shell handling of the inset shell's Difference); suspected to share the mixed owner
  naming above, since an inset shell is a contained operand classified by ray fire. Not traced.

These 10 cases are listed per case in `crates/difftest/known_mismatches.toml` (owner decision, PR #816), tracked in issues #832 (merge_p3 vertex order) and #833 (offset_executive_p2 counts).
