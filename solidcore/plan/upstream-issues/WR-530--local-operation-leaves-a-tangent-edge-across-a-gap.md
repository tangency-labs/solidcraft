#### WR-530. A local operation that moves a face beside a tangent neighbour returns an edge as loose as the gap
- **Severity:** Wrong result (an invalid body returned as a success: SmBrep Test #59 after its tolerances are settled)
- **Area:** Offsets and shells
- **Location:** `source/SMLib/src/SmOffsetGeometryCreation.cpp`, `SmOffsetGeometryCreation::ExtendTrimEdges` (def. `:1384`): the drop of the lifted curve onto the other extended surface (`:1583`); `SmOffsetGeometryCreation::EdgeuseOffset` (def. `:2205`): the ignored failure (`:2294`, "Problem Extending Lamina/Tangent Edge - Will be Fixed Later").
- **What happens:** A local operation (`SmBrep::LocalOperation`) rebuilds a tangent edge by lifting it onto one face's new surface and dropping it onto the other face's surface. When only one of the two faces gets a new surface, the drop crosses the gap between them and nothing reports it. `EdgeuseOffset` also ignores any failure there, and no later step of a local operation closes the gap, so the body comes back with an edge as loose as the gap.
- **Trigger:** a 60 x 40 x 10 plate with an r10 boss and an r2 round at the boss's base, with the boss wall offset out by 1 (the capi's `smlib_offset_faces`). Also one piece of a split top face moved up.
- **How found:** reported by SolidCraft (tangency-labs/solidcore#402). The fork's fix is tangency-labs/smlib@23c5b0a ("a local operation fails where a moved face leaves its tangent neighbour"), which is not merged upstream.
- **Confirmed:** no C++ run was made here. The Rust port reproduces both sites line for line at the pin (`3fd5b95`, usd-brep v1.0.1).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.local_operation_refuses_loose_tangent_edges`, a local operation (`OffsetExecutive::is_local_operation`, true for the library executive under `OffsetOperation::LocalOperation`) is refused in either case below; offsets and shells are unchanged:
  - `extend_trim_edges` returns `SM_ERR` when the drop's `max_drop_to_surf` is more than `10 * max(offset edge tolerance, approximation tolerance)`;
  - `edgeuse_offset` passes `extend_trim_edges`'s error on.

  `tests/wr_530_local_operation_loose_tangent_edge.rs` uses the boss case. It checks that the local operation is refused with the flag, and returns a body without it.
