Updates: WR-5

#### WR-5 (update, 2026-10-31). The axis map is read by basis behind an opt-in flag

- **What changed:** `SmExtent3d::Circumscribe`'s axis-aligned branch takes the interval
  at `|alAxisMap[ii]| - 1` and writes it to component `ii`. `SmPseudoBox::IsAxisAligned`
  fills `alAxisMap[ii]` for basis `ii` with the axis that basis is parallel to, so basis
  `ii`'s interval belongs on axis `|alAxisMap[ii]| - 1`, negated when the entry is
  negative.
- **Trigger:** basis `(y, z, x)` with intervals `[1,2]`, `[3,5]`, `[-7,-6]`: pinned gives
  min `(3, -7, 1)` and max `(5, -6, 2)`; reading the map by basis gives min
  `(-7, 1, 3)` and max `(-6, 2, 5)`.
- **Confirmed by the fork:** the map is read by basis. The doc comment at
  `SmPseudoBox.cpp:1737-1776` (`alAxisMap[0] = index+1 of basis parallel to X`) describes
  axis -> basis, but the code fills `alAxisMap[ii]` for basis `ii`; the code is
  authoritative. Swaps and sign flips agree either way; only a non-involutive
  permutation such as the 3-cycle above differs.
- **Opt-in:** `Kernel::fixes.circumscribe_axis_map_by_basis`, default `false` (pinned C++
  parity). `Extent3d::circumscribe_with` and `Extent3d::from_pseudo_box_with` take the
  kernel and read the map by basis when it is set; `Extent3d::circumscribe` and
  `Extent3d::from_pseudo_box` delegate with `Kernel::default()` and are unchanged. Test
  `geom::pseudo_box::tests::circumscribe_axis_cycle_quirk` asserts the pinned result, the
  corrected result, and identity and sign-flip controls.
- **Rust port:** still reproduces the pinned result by default; no difftest change on the
  default path.

addendum_options: Kernel::fixes.circumscribe_axis_map_by_basis
