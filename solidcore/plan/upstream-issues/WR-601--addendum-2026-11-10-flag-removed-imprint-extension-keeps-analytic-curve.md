Updates: WR-601

#### WR-601 (update, 2026-11-10). `imprint_extension_keeps_analytic_curve` is removed

With `imprint_extends_edge_to_proto_vertex`, a line, circle or ellipse continued to its proto vertex is continued over its NURB and stays that kind (fork bec8a0b). That is now the only path, and the flag is gone. Removed: the field, SolidCraft's preset line, its entry in `fixes.json`, its arm in the goldens' `enable_fix` and every test setting of it.

- **Results:** no golden changes. Every goldens unit (283) gives the same output at this change as at main `87400c80e`, run on Windows. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on, so their results don't change.
- **Tests:** the `wr601_*` unit tests in `topology::proto_topology::p2::imprint_ends`. The assertions that a line or circle is not continued without the flag are deleted.
