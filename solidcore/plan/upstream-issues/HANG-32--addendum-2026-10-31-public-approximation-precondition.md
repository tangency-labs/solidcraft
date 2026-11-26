Updates: HANG-32

#### HANG-32 (update, 2026-10-31). The public projection entry rejects a non-normal approximation tolerance early

The public `Surface::create_parallel_projection_curves` (and its `_with_context` wrapper, which shares the private common entry) now rejects a supplied approximation tolerance unless it is finite and at least `f64::MIN_POSITIVE` (`t.is_finite() && t >= f64::MIN_POSITIVE`), before any cache or geometry preparation. The error is `SolidError::InvalidInput { operation: "surface::surface::p7::Surface::create_parallel_projection_curves", reason: "approximation tolerance must be finite, positive and normal (HANG-32)" }`. This matches the existing `topology.brep_p1` FFI guard, which calls `finite()` before the `MIN_POSITIVE` comparison and therefore also refuses `+inf` (not finite).

The API boundary and defaults are otherwise unchanged. `None` is accepted as before. Every finite positive normal value is accepted, including `f64::MIN_POSITIVE` and `f64::MAX`; there is no new upper cap. The angle tolerance, the projection distance, the default numerics and the valid normal result paths are unchanged. No `Kernel::fixes` flag is added.

Evidence and limits:

- The HANG-32 report records the C++ reference running for more than 15 minutes without returning for `5e-324` (the smallest subnormal); the looping routine was not located, and that run was not repeated here. This addendum therefore does not claim a specific underflow cause and does not claim the Rust side was nonterminating or undefined.
- The Rust global solve was already bounded before this change: `MAX_SOLVE_DEPTH` = 1000 levels and `MAX_SOLVE_BRANCHES` = 2^23 (#504, `geom::global_solver`). The change is a deterministic early public-API contract, not a proven Rust hang/UB fix. Its value is parity with the existing FFI refusal and avoiding a potentially long bounded solve on an invalid tolerance.
- No 15-minute reproducer or timeout test is added. The regression test is `surface::surface::p7::tests::hang_32_projection_rejects_non_normal_approximation_tolerance`. It calls the public entry and checks the exact new reason for the smallest and largest subnormal, `0`, `-0`, a negative value, `NaN`, `+inf` and `-inf`. It checks that `None`, `MIN_POSITIVE`, `1e-6` and `MAX` still reach the later unsupported-curve refusal, and that the zero projection vector keeps its precedence. The accepted cases use an unsupported Hermite curve to stop before the intersector, so they prove the guard accepts those values, not that a successful projection was compared.
