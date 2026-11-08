# 2026-10-21 — Production SweepAccess store (issue #255, PR #424)

`SweepStore` integrates the PR #222 orchestration with caller-owned B-reps,
actual kernels, stable object identities, notification records and attributes.
It borrows originals, owns temporary B-reps and complete HCR sessions, uses a
caller-retained monotonic handle counter, and rejects mismatched borrowed B-rep
keys. Shared contexts resolve to one kernel; dual borrows reject aliasing and use
the destination kernel. Full-kernel callbacks preserve context callbacks through
`Context::with_notification_context`. The five sweep evaluation calls affected by
current main's kernel-aware curve/surface APIs now take the destination context.

Tags use `AttributeStore` and original curve identities supplied by the caller.
Traversal imports/exports the caller's actual `Topology` records through a small
`Marks` record bridge, preserving order and partial traversal output. Mark locks
remain owned until release or store drop; unlike the reference's exhausted-mask
fallback, the adapter refuses when every permitted mark is already locked so it
cannot unlock a caller-owned mark. Temporary deletion clears its topology and
caller-declared exclusively owned auxiliary attribute users/records, is idempotent,
and never releases borrowed originals. The caller must provide correct persistent
identity and auxiliary ownership mappings; the adapter invents neither keys nor kernels.

Eight focused Rust adapter regressions cover shared/distinct contexts, both split
borrow orders, callback execution/restoration after an error, mark lifetime and
partial traversal state, tag ordering and original-curve identities, cross-session
handles/persistent original attributes, key mismatch refusal, HCR scene identity,
and production `TopologySweep::do_sweep` calls. A translation sweep creates a real
destination shell and vertex, then returns the exact `MakeWireEdgeVertex`
`Unported` while retaining that partial topology and restoring context flags.
These are production-store integration tests, not additional C++ geometry parity.

### Explicit remaining boundary

Current main has `MergeState::with_breps`; the older PR #222 note that the merge
constructor is entirely unported no longer describes current main. Its canonical
engine has a separate namespace/attribute store, and there is still no caller-key
copy-notification registration bridge or concrete `PiecewiseAccess` implementation.
This store therefore refuses `new_merge` explicitly at that registration bridge
(`boolean.merge_p2`), before creating a registration. `delete_merge` has nothing
to release. Successful consuming union/merge registration and full HCR construction
are not claimed as production-store acceptance. Available HCR retention uses the
actual `Hcr` scene; its unavailable construction path and other modelling defaults
remain explicit errors. No new topology constructors, tolerances, parity counts or
unit status promotions are introduced by this adapter.
