Updates: WR-212

#### Updates: WR-212 (2026-11-09). Every failure leaves the caller's body unchanged

- The 2026-10-30 addendum put the caller's body back only on the closed-manifold
  refusal, and only with `Kernel::fixes.fillet_refuse_non_manifold_result` on; other
  failures kept the edits made before them, as upstream. On two cylinders joined at an
  angle, rounding two of the three join edges fails with status 1001 in
  `blend::fillet_executive::session::finish` after the merge has edited both edges, and
  the body it left failed the Level 2 check (two `Bad Edge` lines, `ValidateGeometry`).
- Every public operation in `crates/solidcore/src/blend/fillet_executive/api.rs` that
  fillets a caller's body (`fillet_edges`, `fillet_edges_per_edge`,
  `variable_radius_fillet`, `circular_fillet`, `chamfer_fillet`, `set_bevel_corners`,
  `surface_surface_fillet`) now runs under `Brep::with_rollback`: on any error, a cancel
  or a panic, each body it was given is put back as it was before the call. The flag now
  decides only whether the refusal happens. The refusal still names no entity.
- `FilletSession::do_fillet` and the generic `FilletExecutive::do_fillet` still leave the
  merged target on a failure; a caller driving the session directly sees that.
- **Tests:** `crates/solidcore/tests/fillet_round_failure_notes_status.rs`
  (`a_failed_round_leaves_the_body_as_it_was`), and `a_fillet_fails_as_a_whole` in
  `api.rs`.
