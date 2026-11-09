# topology.brep_p4 / topology.brep_p5: StitchAndOrient's second pass, and the glue stub that hides 36 heal.stitch_p2 differences

While making `sweep.api_brep` compare successful sweeps (PR #441), two tiny-scale
`FromFaces` generated cases (`gen0044`, `gen0209`; discs of radius ~1.4e-5 and ~1.55e-5
swept along a 9.6e-5 path) disagreed: C++ 2 faces / 5 edges / 4 vertices, Rust
2 / 6 / 4. A topology dump of the C++ result (temporary shim instrumentation, not
committed) shows its `StitchAndOrient` **second pass** — tolerance doubled to `4e-4`
after the first pass leaves `lamina=4` — merging the two profiles' start circles into
one edge; the Rust pass never ran its glue.

- Cause: `topology::brep::p5::stitch_and_orient` opened one `plain_edit` (context
  notify lease) around the whole call, while the stitch's glue/squeeze helpers notify
  through their own `brep_p3::with_kernel_callbacks` edits. The nested edit is refused
  (`geom::context::lend_notify`, "nested edit") and `SmStitch` swallows the error, so
  the second pass silently left the B-rep unstitched. Fixed by running both stitch
  passes outside an edit; only the `OrientTrimmedSurfaces` pass uses the caller's env.
- Consequence: the second pass now reaches `remove_owned_vertex`
  (`UNPORTED(topology.brep_p1): SmBrep::m_pVertexListHead`, the shell-vertex branch of
  `GlueVertices`), so those cases defer instead of comparing. That stub is honest:
  completing it (`destroy_topology_record(Vertex)` subsumes upstream's list removal)
  un-defers 137 `heal.stitch_p2` cases and 36 of them differ from C++ — e.g.
  `TestSmStitchIntoShell` `pair_gap0_shell_*` end with one extra edge
  (`out[5]`: C++ 7, Rust 8). Baseline `heal.stitch_p2` is 368 pass / 0 fail / 188
  deferred (`topology.brep_p1` 178); with the glue completed it is 469 / 36 / 51.
  Whoever wires that stub must fix the shell-vertex glue path first (PR #441 keeps the
  stub, as main had it).
- Difftest after the fix (Linux x86_64, C++ reference built from the pinned upstream):
  `sweep.api_brep` 573 cases, 481 pass, 0 fail, 92 deferred (`topology.brep_p1` 45,
  `topology.brep_p4` 15, `query.face_p2` 15, `topology.edge` 9, ...);
  `heal.stitch_p2` unchanged at 368 / 0 / 188.
