# Fuzz re-verification (2026-10-24)

Re-ran 2000-draw fuzz per unit on main 76f27c381 with `--fuzz-out fuzz-out/reverify-1024/<unit>`.

- `nurbs.b_spline_curve_p3`: FUZZ-PASS cases=2000 rejected=768 malformed=684 ran=548 differs=0 crashes=0 panics=0 timeouts=0 (9.2 s). No longer reproduces; suspected load-dependent budget flake or fixed since the sweep.
- `surface.surface_p8`: FUZZ-FAIL cases=2000 rejected=537 malformed=860 ran=602 differs=0 crashes=1 panics=0 timeouts=0 (crash fuzz01012: cpp killed by signal 11 on extreme coordinates ~1e308). Rust side did not crash; classified as upstream UB (C++ consumes garbage). Case saved.
- `surface.surface_point_solve_p1`: FUZZ-PASS cases=2000 rejected=530 malformed=905 ran=565 differs=0 crashes=0 panics=0 timeouts=0.
- `surface.surface_point_solve_p2`: FUZZ-PASS cases=2000 rejected=509 malformed=906 ran=585 differs=0 crashes=0 panics=0 timeouts=0 (issue #506 slow path not reproduced in this draw).
- `surface.surface_point_solve_p3`: FUZZ-FAIL cases=2000 rejected=509 malformed=936 ran=554 differs=0 crashes=0 panics=0 timeouts=1 (timeout fuzz00316: cpp no result after 20 s). Case saved; root cause not yet traced.
- `surface.plane_p2`: FUZZ-FAIL cases=2000 rejected=305 malformed=842 ran=851 differs=7 crashes=2 panics=0 timeouts=0. Crashes are upstream UB on extreme inputs; differs are rec.len/rec values on malformed records (C++ writes/reads garbage records Rust rejects). Classification pending per case.
- `curve.curve_p4`: FUZZ-PASS cases=2000 rejected=574 malformed=820 ran=606 differs=0 crashes=0 panics=0 timeouts=0.
- `curve.curve_p6`: FUZZ-PASS cases=2000 rejected=564 malformed=877 ran=559 differs=0 crashes=0 panics=0 timeouts=0.

### surface.surface_p8 (fuzz re-verification follow-up)

Crash case fuzz01012: C++ killed by signal 11 on a B-spline surface whose net contains ~1e308 coordinates (weight -1.7976931348623157e+308) and tiny control-point values. Rust side did not crash. Classified as upstream UB (C++ consumes garbage / segfaults). FFI guard to reject this input class is deferred to the maintainer's guard pass; documented here and in plan/upstream-issues.md.

### surface.surface_point_solve_p3 (fuzz re-verification follow-up)

Timeout case fuzz00316: cpp no result after 20 s on a B-spline surface with ~1e308 weights and tiny (~1e-6) control points, with wrap=true and a local solver. Root cause is SurfaceCache subdivision not terminating within the time bound on extreme weights; related to issue #506. Bounding the subdivision iteration count is deferred (requires a coordinated guard across surface cache code). Case saved.

### surface.plane_p2 (fuzz re-verification follow-up)

7 differs + 2 C++ crashes on malformed DB records (emptied/truncated knots, p=0, p>n, huge form/kt). Crashes are upstream UB on extreme inputs. Differ cases are rec.len/rec value mismatches where C++ writes/reads garbage records that Rust rejects. Classification per case is pending; guards are retained and no check was loosened. Cases saved under fuzz-out/reverify-1024/surface.plane_p2.

### surface.plane_p2 per-case classification (2026-10-24)

9 saved cases from `fuzz-out/reverify-1024/surface.plane_p2/`:

| case | C++ behavior | Rust behavior | malformed aspect | verdict |
|------|--------------|---------------|------------------|---------|
| fuzz00090 | garbage rec (1.001e3) | rejects (1.007e3) | malformed record | guard legitimate |
| fuzz00263 | 54 records | 1 record | truncated/emptied knots | guard legitimate |
| fuzz00807 | 54 records | 1 record | truncated/emptied knots | guard legitimate |
| fuzz00851 | crash (signal 11) | no crash | extreme input | upstream UB |
| fuzz00977 | 54 records | 1 record | truncated/emptied knots | guard legitimate |
| fuzz01069 | 54 records | 1 record | truncated/emptied knots | guard legitimate |
| fuzz01159 | 54 records | 1 record | truncated/emptied knots | guard legitimate |
| fuzz01255 | crash (signal 11) | no crash | extreme input | upstream UB |
| fuzz01894 | garbage rec (1.001e3) | rejects (1.007e3) | malformed record | guard legitimate |

No case was identified as Rust wrongly rejecting a valid record C++ handles correctly; all 9 confirm the garbage-class verdict. No Rust code change made.
