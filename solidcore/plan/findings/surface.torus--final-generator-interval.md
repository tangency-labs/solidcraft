# Torus reconstruction shares its finalized generator interval (#19083)

Unit: `surface.torus`.

A half-disc of radius 0.5 centered at (-2, -5, 0), revolved 60 degrees about
(60, 0, 0) in direction (-1e-9, 1, 0), failed the symmetric public command.
The surface and its generator disagreed on their angular interval, producing
`SmSurfOfRevolution` Test #2 and `SmTorus` Test #2 validation failures.
The reporter's design was used only for diagnosis; tests use primitives.

Symmetric centering moves the body's native record and reimports its analytic
surfaces. Torus recognition, support installation and rebuilding each computed
provisional tube endpoints, snapped an endpoint near the seam to zero, and
constructed a circle from the retained NURB iso curve. The circle finalized its
own interval against that NURB, recovering the nonzero endpoint. The torus kept
the provisional interval instead. Directly rotating a kernel body did not
exercise this record reconstruction and did not reproduce the failure.

For a torus with major angle u and generator parameter v, the surface is
S(u, v) = O + Rz(u) (G(v) - O). Its analytic V interval is therefore exactly
G's analytic interval. A preliminary estimate cannot independently define the
surface interval once the generator has been constructed. All three paths now
construct the generator first and use its finalized `step_interval()` for the
surface's V bounds. This retains the NURB parameterization, including knot
spacing and inside-out direction, and changes no tolerance or recognition
fallback.

`reconstructed_torus_uses_final_generator_interval` checks exact interval
equality after recognition, stored-support installation and rebuilding, for
both UV orders and both generator orientations. Its tube phase is 1e-9 radians
(the primitive example's axis tilt), converted to degrees; the tube spans
180 degrees and the sweep spans 60 degrees. No measured expected value or
geometric tolerance is used in these assertions.

`symmetric_revolve_of_arc_about_tilted_offset_axis_is_valid` checks the public
command path with validation after each feature. No golden or kernel-case
expectation is changed. CI checks the affected unit goldens, full suite and wasm.

Targeted verification (all Cargo commands run through `buildq run solidcraft . --`):

- Before the fix, the primitive engine regression failed the two generator/surface
  parameterization checks. The reporter replay failed the same six checks as reported.
- `cargo test --manifest-path solidcore/Cargo.toml -p solidcore --lib surface::torus::tests -- --nocapture`:
  22 passed, including the new exact-interval regression and the existing full-turn,
  horn-torus, swapped-UV and inside-out cases.
- `cargo test -p solidcraft-engine --lib cmd::features::revolve_convention_tests -- --nocapture`:
  23 passed. A temporary, subsequently removed diagnostic prelude also confirmed
  the reporter replay succeeds on current source. The retained regression uses
  only its own primitive sketch and revolve commands.

Both workspace formatters passed. The queued `solidcore --lib` and
`solidcraft-engine --lib --tests` clippy checks (`-D warnings`) were cancelled
before execution under the updated lane rule: stop after the fix compiles and
its regressions pass, and leave wider checks to CI. CI also runs the full suite,
affected unit goldens and wasm checks. No files were merged or copied from main.
