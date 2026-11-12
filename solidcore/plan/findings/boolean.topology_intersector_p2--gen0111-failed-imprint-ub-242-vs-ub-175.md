# boolean.topology_intersector_p2: gen0111 failed imprint, UB-242 vs UB-175

Unit `boolean.topology_intersector_p2`, case `gen0111` (`IntersectInsertRelate` of
`torus.smb` with `ellips_ts2.smb`, approx and angle tolerance 1e-3). It deferred
before, when `FaceIntersect`'s homogenization could not build a face's trimmed surface
cache on read-only topology; with the B-reps lent to that drop it now runs through.
`ImprintIntersections` fails on both sides with `SM_ERR_NULL_POINTER` (1006; the C++
status was checked with a temporary print in the shim's verbatim
`IntersectInsertRelate`, not kept). The port then deletes the manager and returns that
error (the UB-242 fix in `plan/upstream-issues.md`, `values.state[2]` = 1006). The
verbatim C++ copy only reports it, runs `SetRelationships` (status 1000), and its manager
teardown would read a freed proto edge (UB-175), so the shim answers `-2`. No input rule
predicts a failed imprint, so the case is listed in `known_mismatches.toml`. Follow-up:
a runner classification for runs where both sides stop at a failed imprint, or a shim
copy that carries the UB-242 fix.

**Resolved (PR #839).** The shim copy now carries the UB-242 fix: it stops after a failed
imprint and answers `-2`. The port records `"UB-242"` in `Kernel::upstream_ub_passed`
at that point, and the runner answers `-2` for it. gen0111 and gen0498 pass and are no
longer listed. gdb confirms that the C++ `SmEdgeDefinition::Imprint` and
`ImprintIntersections` return 1006 in both.
