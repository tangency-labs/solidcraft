Updates: WR-212

#### Updates: WR-212 (2026-10-30). The refusal leaves the caller's body unchanged

- With `Kernel::fixes.fillet_refuse_non_manifold_result` on, the closed-manifold
  refusal at the end of `DoFillet` came after the merge, so `fillet_edges` returned
  status 1001 with the caller's body edited and open (tangency-labs/solidcore#422).
- The edge fillet wrappers (`fillet_edges`, `fillet_edges_per_edge`,
  `variable_radius_fillet` in `crates/solidcore/src/blend/fillet_executive/api.rs`)
  now take a `Brep::clone_for_rollback` before the session when the flag is on and
  put it back with `Brep::restore_from` on that refusal. The status is still 1001 from
  `blend::fillet_executive::session::finish`; it names no entity, since the refused
  result's edge is not in the restored body. Other failures keep the edits made before
  them, as upstream. Flag off: unchanged.
- `FilletSession::do_fillet` and the generic `FilletExecutive::do_fillet` still leave
  the merged target on the refusal; a caller driving the session directly sees that.
- **Test:** `crates/solidcore/tests/issue_422_round_over_through_hole.rs`
  (`issue_422_round_into_the_hole_is_refused_with_the_fix`: counts, closed manifold,
  debug dump and volume to the bit equal to before the call).
