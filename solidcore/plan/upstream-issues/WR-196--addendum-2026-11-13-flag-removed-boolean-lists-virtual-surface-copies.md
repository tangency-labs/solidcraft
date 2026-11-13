Updates: WR-196

#### WR-196 (update, 2026-11-13). `boolean_lists_virtual_surface_copies` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch L1a). `boolean::merge::p3::list_operand` copies each surface operand with the virtual `Copy` (the derived surface, its natural domain and its kind), and `boolean_lists` copies its planar outputs the same way. The model of upstream's sliced copy (`BaseSlicedSurface`, its zero natural domain and the `SM_ERR` of its face creation) is deleted.

- **Tests:** `surface_lists_copy_each_surface_whole` runs on `Kernel::default()` and takes two overlapping unit squares through the planar 2D path for every operation, each output plane spanning one region of the result; the test of the sliced copy is gone. The goldens fixture `boolean.merge_p3/surface-operands.json` expects the virtual copy's results.
- **Results:** 9 of the 438 `boolean.merge_p3` rows move from 1001 (measured on the default kernel at the retiring change). `plane_list_vs_plane_list`, `planes_with_options` and `planes_scale_1e-9` are 1007: the planar 2D path refuses planes that are not coincident. `planes_invalid_operation` is 1007: the operation is refused once both lists are built. `sphere_list_vs_sphere_list` (the lens, volume 21.205771 against 21.205750 in closed form), `box_vs_sphere` (the sphere less the box, 4.169309 against 4.169315), `box_vs_plane` (the box, volume 24, and the plane through it), `two_planes_vs_box` (two whole square sheets) and `shell_vertex_vs_plane` build.
