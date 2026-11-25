Updates: WR-492

#### WR-492 (update, 2026-11-16). The mitred cylinders at small turns unite in the port

Since `proto_edge_self_loop_on_vertex_degenerate` was removed (WR-214 addendum of 2026-11-16), a proto edge from a
proto vertex back to itself on a vertex is always degenerate. The port no longer adds the split's second vertex
definition, so the near-duplicate vertex this record describes is not created, and `SmMerge::MakeFaceuse` no longer
returns `SM_ERR` for the cylinder pair. C++ SMLib v1.0.1 still returns 1001 at 0.1, 0.25 and 0.3 degrees.

- **Result.** At 0.05, 0.1, 0.2, 0.25, 0.3 and 0.35 degrees the union is `check_brep` Level 2 clean with no failures
  or warnings, and a manifold solid. Its volume is 522.682480 (0.1 degrees: 522.6824801895), within 5e-9 relative of
  the closed form below and of the two pieces' sum.
- **Closed form.** Each cylinder (r 2.75) is cut by the mitre plane, which meets its axis at the common point: 10
  behind it for the first cylinder and 12 ahead of it for the second. The plane tilts from the axis normal by half
  the turn (at most 0.15 degrees here), so it moves along the wall by at most 2.75 tan(0.15 deg) = 7.2e-3, which is
  well short of the end cap. A plane cut that misses the cap leaves `pi r^2` times the axial length at the axis, so
  the union holds `pi r^2 (10 + 12)` = 166.375 pi = 522.6824777.
- **Test.** `crates/solidcore/tests/wr492_mitred_cylinders.rs`: `small_turns_fail_like_cpp` is replaced by
  `small_turns_unite_the_pieces`. It asserts Level 2 clean, a manifold solid and the volume within 1e-6 of the closed
  form and of the pieces' sum, the bound `larger_turns_unite_the_pieces` already uses. This supersedes the
  "refused at `Relate` before `MakeFaceuse`" entry for this test in the WR-658 flag-removal addendum
  (`WR-658--addendum-2026-11-16-flag-removed-brep-edit-statuses-pass-on.md`). That `Relate` conflict came from the
  self-loop split, which is now gone; with statuses passed on, the small turns still unite.
- **Not covered.** SolidCraft's a52 `lead-out-minus` pair (the curve-swept run against the straight run, 5 degrees)
  was not re-run for this addendum.
