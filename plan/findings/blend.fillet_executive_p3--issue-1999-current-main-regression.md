# blend.fillet_executive_p3: issue #1999 persists with generic blend enabled (2026-11-02)

Tested main `e64c1dea87976bc28a5db97306a32b14c1803eab` on linuxbox. No kernel changes or PR #2003 repair were applied. The test fixture is copied from that PR's roof-profile helpers: width 6, height 1 plus a centered ridge of `3*tan(turn/2)`, extruded 2; public `fillet_edges` arguments after radius are `1, 1, 1.0`.

The 36-case diagnostic matrix is `crates/solidcore/tests/issue_1999_current_main_probe.rs`. All inputs pass Level2 before filleting. Four profiles run nine inputs each:

- `default`: all flags off.
- `half_degree`: only `fillet_half_degree_crease` on.
- `new_blend`: every `fillet_` and `corner_` flag on, plus `curve_check_zero_speed_tangent_end`. This includes the exact round classes, tangent trace, G^n generator, definition and blend corner options; unrelated flags stay off.
- `all_fixes`: `UpstreamFixes::all()` except `geometric_g2_continuity`, matching the current exact-class tests' broad profile. This is a diagnostic profile, not a recommendation to enable every fix in production.

Both new-construction profiles still return `Ok(())`, 8 faces and a manifold solid on each original failure, but Level2 reports two bad vertex/face gaps against tolerance `2e-5`:

| Turn (degrees) | Radius | Approximate measured gap | Outcome |
|---|---|---|---|
| 2.5 | 0.12 | 2.856354776e-5 | Success with invalid geometry |
| 0.6 | 2 | 2.741588096e-5 | Success with invalid geometry |
| 1.5 | 0.5 | 4.283988319e-5 | Success with invalid geometry |

Controls `(turn,radius) = (1,2), (1,20), (2.5,2)` return valid 8-face solids under both profiles. The three-edge corner at radius 1 and turns 2.5, 1.5 and 1.9 is refused with code 1001 at `blend::fillet_executive::session::finish`, leaving a valid 7-face body. This is a refusal, not successful corner construction.

Default reproduces the original 2.5-degree failure. The half-degree-only profile reproduces all three single-edge failures and the three invalid multi-edge successes. Default refuses sub-2-degree cases with code 1007, as before.

Reproduce on Linux with `CARGO_BUILD_JOBS=2`:

```sh
nice -n 10 cargo test --profile difftest -p solidcore --test issue_1999_current_main_probe -- --nocapture --test-threads=1
```

Result: the single matrix test intentionally fails its final assertion after executing all 36 cases; six invalid successes are listed (three cases in each new-construction profile). Build: 6m56s. Test execution: 2.20s. Full output, including every enabled flag and validation report, is saved locally at `reports/issue-1999/run.log` and on linuxbox at `/home/chris/solidcore-validation-notes/1999-current-main-e64c1dea8.log`.

The physical gaps match the original issue, suggesting the retained split-vertex problem remains. This run does not establish the internal root cause or compare the new construction to C++; it is Rust-only regression evidence. Issue #1999 remains relevant even though the post-processing repair in PR #2003 was closed as superseded. No issue comment, commit, push or PR was created by this investigation.

## Follow-up fix: PR #2530 (2026-11-03)

The blend curve's approximation allowance admitted an existing crease vertex outside
the body's vertex/face incidence bound. Under `fillet_blend_corner`, the classification
now requires that strict body bound before inserting a vertex contact; see WR-723.
It changes no entity tolerance and does not repair vertices after construction.

On the fixed implementation, all four integration tests pass: the 36-case profile
matrix, 18 scaled rounds, 24 cases around the contact boundaries, and two cases with
entity tolerances larger than the body's. Every scaled round has eight faces, passes
Level2 and manifold checks, retains its body tolerance, and has curve endpoints
within `1e-9 * scale` of their actual vertices. The old default-path failures remain
characterizations; multi-edge corner refusals are not counted as constructed rounds.
This run used the `difftest` profile with `CARGO_PROFILE_DIFFTEST_OPT_LEVEL=1` on
linuxbox. These are Rust regression results, not native parity claims.
