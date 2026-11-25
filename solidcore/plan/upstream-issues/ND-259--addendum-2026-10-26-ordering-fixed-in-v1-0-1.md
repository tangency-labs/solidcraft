Updates: ND-259

#### ND-259 (update, 2026-10-26). Ordering fixed in v1.0.1; function name corrected

- **Function name:** the `m_vEUToEU.GetAllFirsts` call at c9c5979 `SmOffsetExecutive.cpp:1419` is in `SmOffsetExecutive::DoExtendedOffset`, not `DoSolidOffset`, which ends at about `:1053`. The trigger (a static shell with extend set) goes through `DoExtendedOffset`.
- **Fixed upstream in v1.0.1 (3fd5b95):** right after that `GetAllFirsts`, `DoExtendedOffset` now calls the new static `sm_OrderEdgeusesByBrep(*m_pOriginalBrep, sOffsetEdgeuses)`. That function orders the edge uses by the original B-rep (its faces, each face's two face uses, their edge uses, walked from the end, each edge use at its first sighting). Edge uses not in the B-rep go last, in input order. Its comment states the purpose: "so that the extended offset gives the same result in every process". The degenerate-wall midpoint merges then run in a heap-independent order.
- **Found by:** reading the c9c5979..3fd5b95 diff during the reference pin move.
- **Confirmed:** no. Read at 3fd5b95 `SmOffsetExecutive.cpp` (`sm_OrderEdgeusesByBrep`, `DoExtendedOffset`); the heap-fragmentation repro of `shell_boundary_negative_exactly_two_tol` has not been rerun against v1.0.1.
- **Rust port:** `OffsetExecutive::do_extended_offset` now orders the mapped edge uses with `order_edgeuses_by_brep`, the port of `sm_OrderEdgeusesByBrep`, instead of the relation's sorted-id order. Port and C++ should now agree on that case's result vertex bounds, which the `offset.offset_executive_p2` difftest omits. Comparing them again is a follow-up once the v1.0.1 rerun shows the C++ value is stable.
