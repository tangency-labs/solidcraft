Updates: WR-75

#### WR-75 (update, 2026-10-31). A crossing branch no longer replaces the marcher's curve past a tangency (#2089)

Not the fork's (tangency-labs/smlib 26cc451 and 7ac037e have the same `FindTangentCurves`); a change to the opt-in `Kernel::fixes.ssi_tangent_curve_tracer` itself.

- **Trigger:** an r 1 round of a block's top edge and the crease up from it between the side face and an r 2 round of the block/cylinder junction arc, with the r 2 round as a B-spline surface (#2089). At the crease's upper end the r 1 round's surface runs on almost tangent to the cylinder (angles up to about 1e-3 rad over 0.15, with contact points along it). The tracer traced a crossing branch through those contact points past the end where the regular marcher's crossing curve stops (z 17.15 against 17.0). The marcher's curve did not cover the branch, so the branch replaced it. The corner vertex then lay 0.15 off the r 2 round's rail, the next corner edge's `sm_SrfSrfIntersection` (cylinder / r 2 round, through that vertex) found no curve, and the round failed with fillet code 3, "Fillet Corner Error: Corner Edge Computation Failure".
- **Change:** a crossing branch is also dropped (the marcher's curve kept) when the marcher's curve covers every sample of it except samples inside the band where the surfaces are within tolerance of each other, the band the same pass's replacement test caps at `0.02 * size` (`20 tol / sin(angle) >= 0.02 size`), and covers at least one sample outside it.
- **Test:** `crates/solidcore/tests/issue_2089_tangent_tracer_crease_round.rs` (fails without the change with the tracer on; the volume matches SMLib's 13992.23942188 and is bit-identical with the tracer off).
