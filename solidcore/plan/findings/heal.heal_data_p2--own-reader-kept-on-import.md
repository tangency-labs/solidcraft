# heal.heal_data_p2, boolean.merge_p2, boolean.merge_p5: Rust input still imported from the C++ export (2026-10-28)

Units: `heal.heal_data_p2`, `boolean.merge_p2`, `boolean.merge_p5`.
PR: `difftest: own-reader builders, group F (heal, boolean, intersect)`.

The group F own-reader switch builds the Rust input of `heal.stitch_p1`/`p2`,
`boolean.merge_p1`, `boolean.topology_intersector_p1`/`p3` and
`intersect.curve_class_p5`/`p6` with the Rust kernel. These three units still import the
C++ export on the Rust side and are left for a follow-up, because their sources cannot
be built faithfully with a builder swap alone:

- `heal.heal_data_p2`. The shim's `prepare` calls `normalise`, which replaces every face
  surface and edge curve that the shared export cannot carry exactly with a plain
  B-spline copy of its NURB on the C++ B-rep itself. The C++ side then heals that changed
  B-rep. A Rust B-rep built natively keeps the analytic kinds, so for any source with
  such geometry the two sides would heal different geometry. Switching therefore needs a
  shim change too: drop `normalise` so both sides heal native geometry. That changes the
  C++ side's results, so it has to be reviewed on its own. There are two more gaps.
  Block kind 6 (the single-vertex sphere) is built with `SmBrepConstructor`
  (`StartBrep` ... `StartAndEndSingleVertexLoop`), which has no Rust builder in the
  difftest crate yet. `init_cached_gaps` is this shim's own gap walk over every vertex
  use, which differs from `own_builder::init_gap_walk`. Today the Rust side also takes
  each vertex's vertex use order and the cached gap records from the shim
  (`ffi::vertex_lists`, `restore`). A native build makes both itself, so the follow-up
  removes that channel as well.
- `boolean.merge_p2`, `boolean.merge_p5`. Their operands are recorded `graph` fixtures
  (`topology_topology_traverser::Fixture`: 48 `graph` keys in
  `fixtures/boolean.merge_p2/cases.json`, 158 in `fixtures/boolean.merge_p5/*.json`). The
  Rust side decodes the same stored graph through the shared importer. To switch them,
  each fixture's graph has to be rewritten as a source spec that both sides build. That
  is a fixture rewrite, not a builder swap.

No values were compared differently by this PR for these units. Their counts are
unchanged (all green on main).
