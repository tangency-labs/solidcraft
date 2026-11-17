Updates: WR-529

#### WR-529 (update, 2026-11-13). `revolve_holed_profile_void` is removed

A full-turn revolve whose stitch nested every closed shell (a holed profile: the ring it turns the hole into lies inside the ring around it) takes its void flags from the nesting (the fork's tangency-labs/smlib@38b4212), so the hole is a void. That is now the only path in `nest_closed_shells_as_cavities`, and the flag is gone. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** no golden changes. With it and `local_operation_box_holds_new_surfaces` both retired, every goldens unit (283) gives the same output as at main `e5384730a` on Windows, except two cases of `topology.brep_local_operation`, a local-operation unit. With this flag alone, that unit and the revolve and sweep units (`primitive.primitive_creation_p3`, `sweep.api_brep`, `sweep.rotational_sweep_geometry`) give main's output. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on.
- **Tests:** the holed-ring revolve test in `primitive::primitive_creation::p3::sweeps`.
