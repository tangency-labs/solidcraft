Updates: WR-531

#### WR-531 (update, 2026-11-15). `offset_point_vertex_caps` is removed; a shell or offset always caps a one-edge vertex, a local operation never does

`DoExtendedOffset` (`offset/offset_executive/p2.rs`) always caps each vertex of the original with one edge (a
cone's apex, a drilled hole's tip) with the sphere `BuildOffsetOfVertex` builds for it, for shells and offsets.
A local operation builds no caps: it moves faces onto given surfaces, with no offset distance about a point. With
the flag on, a local operation on `Solids/nurb_fillet.smb` built a cap and failed with `SM_ERR` (1001, from
`CreateArcFromPoints`) whatever plane it was given. The opt-in described in the original record no longer exists.
Removed: the field, SolidCraft's preset line, its `fixes.json` entry, the flag check, and the flag settings in the
tests.

- **Defaults:** SolidCraft's preset had the flag on; local operations on bodies with a one-edge vertex no longer
  fail there. `Kernel::default()` changes.
- **Goldens:** no row of any unit moves.
- **Tests:** `wr_531_drilled_hole_shells_with_a_sphere_cap` runs on the default kernel. The flag-off test (the
  shell refused and the input retained) is removed. `a_flat_face_given_its_own_plane_leaves_the_body_unchanged`
  (`issue_2978_local_reach_beside_nets.rs`) is new: fc0006's operation succeeds and leaves the body's counts and
  volume as they were (with caps in local operations it failed with 1001).
