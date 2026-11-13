Updates: WR-196

#### WR-196 (update, 2026-11-13). `boolean_lists_virtual_surface_copies` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch L1a). `boolean::merge::p3::list_operand` copies each surface operand with the virtual `Copy` (the derived surface, its natural domain and its kind), and `boolean_lists` copies its planar outputs the same way. The model of upstream's sliced copy (`BaseSlicedSurface`, its zero natural domain and the `SM_ERR` of its face creation) is deleted.

- **Tests:** `surface_lists_copy_each_surface_whole` runs on `Kernel::default()` and takes a list of planes through the planar 2D path for every operation; the test of the sliced copy is gone. The goldens fixture `boolean.merge_p3/surface-operands.json` expects the virtual copy's results.
- **Results:** the `boolean.merge_p3` rows of surface lists move from `SM_ERR` to the virtual copy's result; the pull request lists them.
