Updates: WR-101

#### WR-101 (update, 2026-10-31). Opt-in upper split interval `[dT, old_max]`

- **What happens (default, pinned):** `SmProtoEdge::Split` clamps this edge's interval to
  `[old_min, dT]` and then sets the new upper child from `this->m_pXSectInterval.GetMax()`,
  now `dT`: the upper child starts as `[dT, dT]`. When no edge definition is added to it
  afterwards, the upper child keeps that empty interval.
- **Trigger:** an interior proto edge split (`SmProtoEdge::Split`, reached from
  `SplitIntersectingProtoEdges`) where the split point is more than the proto edge's
  tolerance from either end but within ten times that tolerance of an edge definition's
  end: `SmEdgeDefinition::Split` declines to split there (`cbiFudge`), so no
  `AddEdgeDefinition` overwrites the interval. When a definition is split, its first
  `AddEdgeDefinition` replaces the upper interval with the split definition's interval,
  which masks the empty value.
- **Confirmed:** read at the pin (`3fd5b95`, `source/SMLib/src/SmProtoTopology.cpp:3047`-`3048` (archive line numbers with the older build; the pin has them at 3047-3048));
  the pinned behaviour is retained by default. The opt-in path intentionally differs from
  the pinned reference and was not compared against a C++ run.
- **Opt-in:** `Kernel::fixes.proto_edge_split_upper_keeps_the_old_max`, default `false`.
  It captures the lower edge's maximum before the clamp and gives the upper child
  `[dT, old_max]`; the lower child is `[old_min, dT]` either way. The refusal guards
  (non-interior `dT`, within the tolerance of an end) are unchanged.
- **Rust port:** `ProtoTopologyManager::proto_edge_split`
  (`crates/solidcore/src/topology/proto_topology/p3.rs`). The regression test
  `proto_edge_split_upper_interval_keeps_the_old_max`
  (`crates/solidcore/src/topology/proto_topology/p2.rs`) asserts `[dT, dT]` without the
  flag, `[dT, old_max]` with it, the lower `[old_min, dT]` in both, and the unchanged
  refusals.
`addendum_options: Kernel::fixes.proto_edge_split_upper_keeps_the_old_max`
