Updates: WR-714

#### WR-714 (update, 2026-11-03). A round on an earlier round's torus, run past the face, is its definition

A constant round by its definition (`CircularCrossSectionFsg::definition_surface`, WR-698) solved its stations on the offsets' base surfaces only (tangency-labs/solidcore#2500).

- **Defect:** #1945's round of a bar's top front edge, run over the first round (a surface of revolution of a circle's arc, the concave round of the boss's base), rolls on past that face to its End and round the torus's tube, on the offset's extension of the base as itself (`offset_extension_as_itself`). The station at the trace's start, past the face, did not solve on the base alone, so the gate took the round as not its ball's and built the trace's B-spline (4.9e-5 off the envelope of its balls, a bare blend face for the #2489 ledger).
- **Cause:** past its generator's interval a surface of revolution revolves the generator's extrapolation, not its circle; the offset's extension of it as itself (the fork's 43fd6b6) is the circle's.
- **Change (with `offset_extension_as_itself`):** a surface of revolution of a circle's arc that the offset extends as itself is taken with its stored extension (`rolled_on` in `fillet_standard_solver/p2.rs`), the same surface at the same parameters over the base's domain. A cylinder, cone, sphere or torus continues as itself on its own and keeps its analytic kind in the definition (taking their extensions too gave #2406's definitions B-spline faces), and planes are left as they are.
- **Test:** `crates/solidcore/tests/issue_1945_rail_uv_trim.rs`: both runs of the round are `ProceduralBlend`s, 8.9e-15 off the envelope of their balls.
