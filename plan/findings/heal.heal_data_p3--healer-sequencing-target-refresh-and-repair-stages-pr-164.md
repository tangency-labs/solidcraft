# heal.heal_data_p3 (healer sequencing, target refresh and repair stages; PR #164, 2026-10-16; partial, no disagreements)

- **Gate:** 474 cases, 267 passed, 207 deferred, zero failures, `max_rel_err=0`,
  no absolute floors. The shared real-B-rep scripts compare target/props lists,
  problem-list order, linked flags, statistics and counters. Terminal `HealTgts`
  cases compare status, stop/done step, recursion depth and target counts; repair
  attempts with populated queues are terminal because native mutations invalidate
  the original fixture graph. Empty repair queues retain the complete state dump.
- **Missing dependencies:** 50 cases stop at B-rep tolerance/model-size operations
  (`topology.brep_p1`), 74 at edge-use gaps (`topology.edgeuse_p3`), 61 at a
  face-owner tolerance lookup (`topology.face_p2`, existing context-free surface
  dispatch), 20 at face degeneracy (`topology.face_p3`), and two single-vertex
  sphere reconstruction cases at `SmFace::GetEdges` (`topology.face_p4`). No
  successful mutation is simulated. The remaining repair adapters return the
  owning unit's `Unported` error and carry every caller-supplied parameter.
- **Faithful sequencing:** `HealTgts` resets stop/done state and tests
  `done < requested`, so requesting disabled `Fix_BackPointers` runs the edge
  cache instead. `Fix_BadLoops` remains disabled, as in the pinned source (its
  missing-poles regression is not an owned function of this unit).
  `Fix_SplitFaceAtSeams` deliberately writes the split-*edge* tried list and
  catches new targets up through `Fix_SplitEdgesAtSeam`, matching upstream.
  The native last-direction edge-marking loop is preserved.
- **Bounded catch-up:** nested `UpdateTargetLists -> HealTgts` calls are capped at
  32 passes. Old-target counts, stop/done state and recursion counters are restored
  on success and error. Native operational errors from nested healing are ignored;
  unported callees and errors representing invalid memory access propagate.
- **Representation:** repairs receive a mutable B-rep rather than toggling
  upstream's B-rep editing flag. Mutation adapters return remove/add identities
  instead of installing a context callback during each mutation; accumulated lists preserve native uniqueness
  and removal/re-add order. `update_tracked_targets` accepts an explicit resolver
  for the existing tracker's opaque keys. Nonempty generic classification outputs
  must gain a typed topology resolver when their missing callees are wired; the
  current implementation rejects unresolved handles, never invents arena ids.
- **PR #164 review:** restored the successful seam-merge flag/tried-state block,
  retaining U/V merge statuses and the last split-edge list across faces. The
  wire-deletion list retains the last duplicate, matching `SmTArray`. Null faces
  skip all three live-member guarded repairs. Bare/SE mutation calls ignore
  ordinary statuses but retain `Unported`; a missing degenerate edge reports
  native `SM_ERR`. The recursion counter decrements only while positive.
  `SmBrep::IsLiveTopologyMember` is recorded as a private co-port in its owner's
  notes. After merging PR #65, `FindEdgeObjects` calls the p7 implementation;
  nonempty opaque handles still require the documented typed resolver.
- **Native safety regression:** a forged done step >=6 can send new vertices into
  an unsized incremental voxel model (UB-36), as confirmed by the maintainer's
  SIGSEGV. Step 33 predicts the post-removal vertex counts and refuses only that
  unsized incremental path. Direct FFI tests exercise all five reported ops and
  preserve initialized/empty/fully-removed old targets and both step-limit
  boundaries. Catch-up at step 7 or later is terminal because it can mutate the
  fixture graph. Twenty review fixtures include null-face skips and a
  no-direction crossed-seam flag snapshot. Wire variants compare all five guard
  refusals without the box's earlier unported edge-use gap calculations; native FFI tests run independently
  of Rust deferrals. Nonempty mutation paths remain uncertified at their missing
  dependencies.
- **PR #164 round 2:** native `UpdateTargetLists` resizes props to TARGET counts,
  not the previous props count. Uncached removal may leave its target behind;
  an add (even NULL) would create NULL holes (UB-163). The shim and Rust runner
  now predict target and props counts separately for all three kinds and refuse
  any post-removal drift before a nonempty add. Target-to-props removal lookups
  are bounded before calling native `Get*Props_ForTgts`. The unsized-voxel guard
  uses remaining target vertices and the count of live vertices being added.
  Independent safe-FFI tests cover all three reported crash sites plus 240
  cache-mask/kind/add/removal-path combinations, preserving empty-add removals
  and synchronized updates. Twenty-four additional differential fixtures cover
  the same boundaries without counting earlier deferrals as mutation parity.
- **Count versus status:** `FindEdgeObjects` returns a count, so `classified_edges`
  must propagate resolver errors directly. Its nonempty unresolved-output refusal
  is no longer swallowed by the soft-status helper. This path still defers at
  `classification_has_dead_topology` until p2 lands; it is not certified here.
- **Partial merge outputs:** p5 now provides a crate-private fallible adapter whose
  error retains output from completed intervals. The healer ignores ordinary
  status but consumes those lists, as native `SER` only skips the failing
  interval's append. The public p5 Result contract is unchanged. An accumulator
  regression checks failures in both passes and native visit order; actual
  topology mutation remains deferred, not simulated by this test.
- **Harness:** a generation-aware props registry prevents allocator address reuse
  from reviving retired records. Duplicate removals are refused only if they
  resolve to the same props twice (or trigger the existing native pair-removal UB);
  repeated unrelated topology entries remain valid. The single-vertex sphere
  fixture is constructed with `SmBrepConstructor`, including swapped UV, from
  `my_test_heal_single_vertex_sphere`; API box and closed-sphere fixtures follow
  the upstream healer regressions. `heal.heal_data_p1` now calls the real sequence.
- **aarch64 hosts: 2 expected failures (2026-10-17).** `gen0061` and `gen0211` differ in
  the edge-length histogram, as 26 `heal.heal_data_p2` cases do (UB-194, see General).
  Both pass with aarch64 casts in the port.
