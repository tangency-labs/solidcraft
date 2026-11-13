# topology.brep_p5 — Claude review follow-up (#194)

Claude Opus 5.5 independently reviewed head `24996494` against official base
`85a60e00` and the pinned upstream. It confirmed the six earlier must-fix items
were addressed, and found incorrect dependency tags and latent failure-lifecycle
differences. Follow-up source `ecccb67b` also includes official main `9a7c1ec0`
(the independently landed offset executive and shared reader/marker changes).
No open dependency PR branch is included.

- Detached shell, vertex and vertex-use records now share one erasure barrier,
  owned by `topology.brep_p4` / `SmBrep::~SmBrep`. These are plain deletes after
  detachment, not `DeleteVertex`: calling that again would repeat notifications
  and list removal, or delete a vertex in place of a vertex-use. The append-only
  arenas still need stable erasure and real lifetime/key routing.
- Squeeze's bounding-box barriers name the actual `SmLockBrepBox` constructor
  and destructor (part 1). Pending cleanup's `Clear`/`release` co-port is recorded
  in part 1's notes.
- After face deletion, any ordinary stitch failure sets the accumulated failure
  status and continues cleanup/output appends; only `Unported` defers. A Rust
  regression checks typed failures, retained failure state and missing-code
  propagation. Native mutation/late-failure execution remains uncertified.
- Both unchecked native region-removal statuses in GlueVertices are ignored.
  Wire traversal still propagates errors: pinned `SmShell::GetWireEdges`
  (SmShell.cpp:130-229) always returns success; the Rust error boundaries are
  missing code or invalid pointer/handle accesses. Turning those into an empty
  wire list would fabricate output rather than preserve a defined native failure.
- Proto-topology's handle adapter has no actual owning editing getter, unlike
  the boolean intersector adapter. Its part 1 editing-metadata barrier remains
  correct without assuming TRUE. The newly landed offset caller has the same
  missing editing metadata and mutable-context routing. Its new part 5 stub now
  identifies that actual part 1 gap and documents forwarding the real flag,
  caller Kernel and gathered faces into the complete part 5 body when available.

The implementing agent checked these amendments against the review and pinned
source. Both follow-up Claude review attempts hit the Claude API rate limit and did
not complete; no independent approval of the amended revision is claimed.
The previous `a9f20328` validation above remains historical for that source.

Validation of functional source `ecccb67b` against integrated official main
`9a7c1ec0` is complete on runner 2 (same pinned reference/toolchain/compiler flags
as above). All **1,279** tracked build/fixture files match the local source;
subsequent changes are reports and plan metadata only. Current results:

- Formatting, explicit registered-module rustfmt, plain/native strict workspace
  Clippy, **1,369** kernel tests, WASM and **3** native FFI guard tests pass.
- All **12** affected differential units pass their gates: **1 green, 11 partial,
  0 failed**, including the newly landed offset caller. All twelve report hashes
  match the runner. Only that caller's blocker changes (`brep_p5` to `brep_p1`);
  counts remain 693 total, 675 passed, 17 deferred, 1 rejected, 0 failed.
- Part 5 remains **791 total, 737 passed, 54 deferred, 0 failed**, maximum relative
  error 0, fixed relative tolerance 1e-9 and no absolute floors. The 409/301/27
  coverage split and **0/12** meaningful successful topology-mutation paths
  certified remain unchanged.
- Sensitivity: **737/737** detected, 54 deferred. Extended 4096 generated:
  **4375 total, 4195 passed, 180 deferred, 0 failed**, maximum relative error 0.
- Full green/partial sweep: **205 units, 113 green, 92 partial, 0 failed**.
  `--status-of topology.brep_p5` prints `partial`. No dependency unit is promoted.

Commands run under `nice -n 10` with one Cargo job and sccache; no profile or
numerical-tolerance override. Logs are `/tmp/p5-07925044-claude1-*.log` on runner 2
and copied locally. The worktree-owned temporary 4 GiB swap file was removed
following validation; no persistent runner configuration changed. All previously
listed mutation, sharing, opaque-adapter and reader coverage limits still apply.
