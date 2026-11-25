# SweepStore

`SweepStore` implements `SweepAccess` and `PrimitiveAccess` over one registry.
`SweepStore::new` borrows caller kernels, models, identities, records and attributes;
`SweepStore::owned` owns the same resources for standalone callers.

## Ownership and notifications

- Register each actual kernel once and reuse its returned `ContextId` for models
  sharing that kernel. `register_kernel` is fallible while a context is lent.
- Keep borrowed `next_handle` across sessions. Handles are monotonic; borrowed
  originals retain their identities and are never deleted by store cleanup.
- Identity providers cover future topology, new split curves and reserved region
  source-list handles. Missing registrations return explicit errors.
- Copies resolve source/destination keys separately and use the source callback
  context. New geometry binds to its actual caller topology owner.
- Merge sessions retain canonical mate maps. Notification leases, surviving bodies
  and owned intermediate cleanup are scoped across errors and unwinding.
- Read `take_cleanup_errors` after operations whose trait cleanup hook is infallible.

## Reference checks

```sh
CXX=g++ bash scripts/check-production-adapter.sh
```

Use `CXX=clang++` on macOS. `wire_sweep` exercises point-to-edge construction;
`merge_adapter` covers topology, points, intervals, attributes and copy callbacks.
The reference is unmodified SMLib/SM_API at `c9c5979f04cea8c38213004d4f04df0dcfb40d05`.
The ordinary full green/partial difftest and affected-unit perturbation checks
remain the repository validation gates. No tolerances or native guards are relaxed.

## Limits

The probes do not qualify complete solids, full constructive Booleans, advanced
HCR/stitching or native destructor equivalence. Unavailable UV-trim construction
remains deferred. The existing WR-168 curve-copy slicing deviation is retained;
full curve-copy geometry parity is not claimed.
