# sweep.topology_sweep (PR #222; partial)

- **Observed differential coverage.** 56 named fixtures plus 512 generated scripts:
  554 pass, 14 deferred, zero failures and zero rejected native setups. All four maps
  are compared after each script (counts, membership and values for all 65 keys),
  including null entries, first vertex images, overwritten edge images and stale
  reverse entries. Callback status/keep decision and map mutations, options,
  repetitions (all 64 bits) and eight integer fields of each edge tag compare exactly.
  The two identity namespaces on the Rust side correspond to distinct native objects;
  this is not evidence of copying real cross-B-rep topology. Floating-point helper
  coordinates/transforms are emitted for successful ops 8/9 as origin and X/Y/Z
  axes (12 doubles); callback distances are also emitted without integer truncation.
  Unrequested or failed frame slots are zero-filled on both sides, not read from
  unspecified native outputs. The report's `values_compared`
  counts floats only; the `state` integer arrays are nevertheless compared exactly.
- **Deferred and unexecuted paths.** Fourteen cases stop at
  `SmBrep::MakeShellVertexTopology` (`topology.brep_p2`), through part 9's
  real `MakeShellVertex` port. The selected-vertex base error and
  rotational axis-identity `DoSweep` paths now complete on both sides after wiring
  `GetInfiniteRegion`. The native fixture's newly constructed source vertex has its
  own identity on the Rust side too; repeated axis sweeps do not reuse a map-pool
  vertex. These observe statuses and map state, not generated wire/face geometry.
  Map entries enumerate the 64 pool objects plus NULL; fixture-owned vertices
  outside that pool affect map cardinality, but their individual pairs are not emitted.
  C++ execution beyond a Rust dependency deferral
  is not a parity pass. The full edge/face construction, profile loop mapping,
  silhouette/HCR flow, back-face move, stitch, merge and recursive wire sweep bodies
  are ported through their final returns, but no native differential evidence is
  claimed for those tails. Missing calls have default `Unported` implementations
  in `SweepAccess`. It requires caller-owned context, B-rep, key/attribute, mark and
  lifecycle resolution; this PR does not provide a production store/adapter for it (follow-up [#255](https://github.com/tangency-labs/solidcore/issues/255)).
  Existing faithful face/edge/vertex geometry and traversal helpers are reused.
  The plan's rectangle/polygon test entry points are indirect callers, not direct
  topology-sweep fixture replays.
- **Safety deviations and limits.** Execution rejects repetitions above 4096 before
  mutating context; the option itself retains the full unsigned upstream value.
  A soft `CollectFaces` failure that did not mark its start face returns an error
  instead of retrying forever. Temporary scene, wire and merge objects are cleaned
  on Result errors; partial marks/output are preserved when a soft failure made
  progress. Context flags restore in reverse construction order, including aliased
  contexts and failure to obtain the second mutable context. These are safe Rust
  unit regressions with a service double, not native topology comparisons.
- **Consuming union ownership.** `SmMerge::ManifoldBoolean` with default
  `m_bKeepOtherBrep=FALSE` deletes the source on success (`SmMerge.cpp`, final cleanup).
  `SmTopologySweep::DoSweep` retains `pWireBrep` outside its repetition loop. The Rust
  port clears the handle after successful union and releases only live owned wire
  temporaries on early errors; otherwise adding cleanup would double-delete the
  consumed object, and retaining the handle could reuse it on a later repetition.
  A two-repetition Rust regression makes the second pass have silhouettes but no
  wires, and rejects any access to the consumed source. No native reproducer of the
  upstream stale-pointer scenario has been run; do not call it a confirmed C++ crash.
- **Prior-head harness stress (`049ef767`).** Perturbation detected all 551 comparable scripts. The 20,000
  mutated-input campaign has 7,389 executed, 3,817 rejected and 8,794 malformed cases:
  zero differing results, crashes, panics or timeouts. This covers the bounded script
  API, not the dependency-blocked construction paths or a general crash guarantee.

- **Review integration (2026-10-19).** Merged canonical main through `5387f70c` (including `topology.brep_p10`) without
  rebasing the published branch; preserved both appended findings sections. Available
  `SweepAccess` defaults now call `Brep::infinite_region`, owning-list face/wire
  queries, region/shell traversals, part 6 edge copying, part 7 face construction and
  copying, part 3 edge-use binormal evaluation, and part 10 topology removal. Only unavailable callees remain
  deferred locally; the stitch owner is `heal.stitch_p2`. Borrow-splitting callbacks
  pass the real caller context and face-object resolver, without a default kernel
  or inferred object keys. Native binormal point/vector outputs are written only on
  success when no optional UV-vector output is requested, so errors retain the
  caller's values. Merge-copy edge-mate registration still awaits `boolean.merge_p2`.
- **Curve ownership hand-off.** Part 7's `make_face_with_curve_owners` extends the
  existing face constructor with input-slot owner records. Discarded degenerate or
  coincident curves have `None`; each created edge is retained in its original slot,
  and completed slots survive a later error. The original API delegates to it without
  changing its result. SweepFaceSame skips discarded slots, matching native null
  curve slots, and rejects a malformed short ownership array. A Rust unit regression
  checks discarded slots before a later dependency failure; actual successful face
  construction remains deferred and is not claimed as compared coverage.
- **Tolerance conversion regression.** Both edge and vertex tolerance helpers apply
  `tol::zone_tol3d` using the owning entity's actual kernel before SetTolerance. A
  regression covers negative tolerances, `USE_DEFAULT` with a non-default system
  tolerance (0.125), ordinary positive tolerances and edge-to-vertex cascading.
  Without conversion the negative/default cases do not grow the zero tolerance.
  Four additional access-default regressions check list order/duplicates, NULL
  infinite regions, CopyEdges/CopyFaces/MakeFaceWithCurves dependency attribution,
  and binormal output preservation on error.

  Resolver failures during CopyFaces are kept outside the callee's copy status:
  rejecting aliasing or an invalid B-rep does not attempt notifier lookup or become
  a false `Unported(boolean.merge_p2)`. The access-default regression checks both.

- **Prior review harness stress (`86c7eb33`, 2026-10-19).** Perturbation detected all 554 comparable scripts.
  The 20,000 mutated-input campaign has 7,378 executed, 3,893 rejected and 8,729
  malformed cases, with zero differences, crashes, panics or timeouts. This remains
  evidence for the script API and its newly completed root paths, not for deferred
  topology construction or a production adapter.
