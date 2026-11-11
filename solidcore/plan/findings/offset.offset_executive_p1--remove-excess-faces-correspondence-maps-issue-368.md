# offset.offset_executive_p1: remove offset face correspondence before deleting topology (2026-10-24)

Issue #368 (fkirill's PR #643, stacked). `remove_excess_faces_with_kernel_context` now
borrows the executive mutably and forgets every candidate face before calling
`RemoveFaces`. Incoming pairs are removed from `faceuse_to_faces`, `edge_to_faces`,
`vertex_to_faces` and `blend_to_faces`; a deleted blend's outgoing pairs are removed too.
This carries tangency-labs/smlib 5fe723c. The pinned reference leaves stale face pointers
for `PiecewiseMerge`; append-only Rust arena records mask this as stale topology.

`geom::tol::Relation` now ports `GetFirsts` and `RemovePair`, including the native
status when a key is absent and success when both keys exist in unrelated pairs. The
tests cover multiple sources, surviving faces, both blend directions, repeated cleanup
and removal of empty keys. The helper is exercised directly because the nonempty
measurement path still reaches its documented evaluation dependencies; no end-to-end
shell success is claimed. No differential fixture or tolerance changes.

Validation for #368 on macOS/Apple Clang, Rust 1.98.0 (original PR): geom.tol 3450/3450
exact passes (eight RemovePair scripts under every cardinality and GetFirsts observations
for every relation case); offset executive p1 1048 cases, 909 pass, 138 deferred, one
rejected, max relative error 1.850371707708594e-16; p2 322 cases, 141 pass, 164
deferred, 17 rejected, max relative error zero. No failures in these units.

Upstream record: `plan/upstream-issues/UB-258--remove-excess-faces-leaves-removed-faces-in-correspondence-maps.md`.
