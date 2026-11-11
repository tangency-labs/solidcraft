# General: WR-536 `boolean2d_plane_covers_far_faces`: the port's default already joins far disks cleanly (2026-10-31)

- **Flag:** `Kernel::fixes.boolean2d_plane_covers_far_faces` (WR-536, fork tangency-labs/smlib@67b8a1e).
- **Audit:** class B (`plan/findings/audits/flag-reachability-2026-10-31/`). `tests/wr_536_boolean_2d_far_disks.rs` reaches the guard, but nothing fails when the flag is forced off.
- **Tried (origin/main 36f1a9a67, instrumented):** two r 2 `create_planar_circle` disks joined with `boolean_2d` Union, at 1500, 10 000 and 100 000 mm apart, with the flag on and off.
- **Result:** every case gives two faces with every edge tolerance 1e-5, with and without the flag. The record's failure (the far disk built on the near disk's plane, edge tolerances 5875 and 11756) does not occur: the port's planar circles' cover already reaches the far disk, so the default does not diverge.
- **Waits on:** a geometry whose plane's 100-width search domain really misses the other face (the fork's capi test uses four disks at the corners of a 3 m hole pattern, joined in pairs and then all four; its plane sizes are not known here). No fork fix is missing in front of the branch.
