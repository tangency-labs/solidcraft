Updates: WR-410

#### WR-410 (update, 2026-11-10). The best crossing is the default; the flag is retired

`Kernel::fixes.boolean_vertex_by_best_crossing` is removed (policy #2392: a ported fix becomes the default and its old path goes). `FindOrCreateProtoEdge` (`topology/proto_topology/p3.rs`) now always passes each end's proto vertex to `add_crossing` with the angle at which the face pair's curve crosses the edge, so a vertex definition takes the position of its steepest crossing. The upstream behaviour (the first crossing places the vertex) is no longer reachable in the Rust kernel; the C++ defect is unchanged in v1.0.1, as recorded above.

Effect on the default kernel, measured on a SolidCraft main export (a65ecb00) with and without the change, goldens blessed in the retiring PR:
- The goldens compare flags 7 rows in two units (`boolean.api_brep`, `offset.offset_executive_p2`). Blessing those two units rewrites 96 rows; the other 89 move within the compare's tolerance (77 by at most 7e-13 relative, the rest by at most 1.8e-9).
- 5 of the 7 keep their status and move by 1.2e-7 to 3.3e-7 relative (`issue1519_bench_tubes_union_measured`'s two moved measures are residuals near 3e-13, 2.4e-6 relative).
- The WR-614 strip union at d = 1.5e-5 (`wr614_strip_union_d1.5e-05`, `wr614_strip_measured_union_d1.5e-05`) goes from a 13-face body of volume 0.0 to `make_faceuse` status 1001, the failure the same union already had at d = 1e-5 (WR-614; the C++ fails there too). SolidCraft's fix set, which already ran WR-410, refuses it in `manifold_boolean` by name.
- The kernel_cases rig (SolidCraft's fixes, which already had WR-410 on) is unchanged.
