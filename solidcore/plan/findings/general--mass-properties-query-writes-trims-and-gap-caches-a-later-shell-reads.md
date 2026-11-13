# general: a mass-properties query writes UV trims and gap caches that a later shell reads (2026-10-30)

- **Unit:** general (mass.integrator, topology.face_p1/p3, topology.vertexuse, offset).
- **Inputs:** tangency-labs/solidcore#1528's plate:
  - a 27.5 thick sweep of three straight sides and a half circle r 10 (x < 0);
  - minus an r 5 drill along x from x = -10 to 7.5 at (y, z) = (12.5, 12.5);
  - shelled 2 inward, open at x = 42.5.
- **What a query writes:** `brep_volume` (`SmApiBrepComputeVolume`) on the operand changes it. The calls run brep_properties → face_properties → `create_oriented_trim_boundary` → `CreateUVTrimCurves(FALSE)` (fixer-2's vol1528 trace, backtraces split at the query):
  - it makes the UV trims of faces that had none: 26 → 30 edge uses with trims;
  - `FixUVTrimCurves` snaps an existing trim's end control point onto its neighbour's: 4.8413e-12 → 0.0;
  - it fills vertex uses' vertex/face gap caches (`GetVertexFaceGap`): gap 8.9e-16 on vertex use 129;
  - it builds one face's trimmed-surface cache.
- **Both values:** a later shell reads these trims, the snapped curve and the gap caches.
  - The default shell's volume moves from ...677376 (clean operand) to ...677383 (after the query).
  - With WR-578's `offset_closes_loop_gaps`, the plate shells on a clean operand: 16 faces, 36 edges, sound, 10400.21299138017. After a prior volume query it is refused (1001 from `close_loop_gaps`).
- **C++:** pinned SMLib does the same. fixer-8's operand-state extension (#1793) of `offset.offset_executive_p2`'s `issue1528_plate_api_shell_2` (from #1757) reports each side's operand after the query, and Rust and C++ agree in both cases (with and without the query), including the shell volumes. So this is parity, not a port divergence.
- **Consequence for tests and probes:** measuring an operand before an offset changes the offset. Probes and tests should shell a clean operand, or measure a copy. fixer-5's #1528 probe measured first and saw WR-578 refuse a plate it builds on a clean operand.
- **Upstream record:** `plan/upstream-issues/WR-583--a-mass-properties-query-changes-the-b-rep-a-later-offset-reads.md`.
