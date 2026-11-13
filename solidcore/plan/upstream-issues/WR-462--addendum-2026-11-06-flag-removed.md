Updates: WR-462

#### WR-462 (update, 2026-11-06). `fillet_refuse_ball_not_fitting` is removed: `BallIsRegular` judges the ball on every kernel

The fork's tangency-labs/smlib#79 (410dfe1) replaced `BallFitsItsFaces` with
`SmFilletSolver::BallIsRegular` (WR-696). That check runs before the corners: each face encloses
the ball (`1 - r k > 0`), and every point of its section is progressive. It refuses with
`NoRoom`, naming the radius that works. Here it ran only with `fillet_gn_section_generator`, and
`fillet_refuse_ball_not_fitting` ran only without it. With the G^n section's flag removed (WR-685's
addendum of 2026-11-06), `BallIsRegular` runs on every kernel and the old check never ran. So
`fillet_refuse_ball_not_fitting` is removed, and with it:
- `blend::fillet_solver::ball_fit` (`ball_fits_its_faces`);
- the executive's `creation_ball_fits` and its two implementations.

- **Results:** a ball that does not fit a face it rolls on is refused before the corners on
  every kernel, as `NoRoom` (`BallDoesNotFit`) with the largest radius that builds it on the
  refusal (`SolidError::FilletRefused`). Before, the default kernel failed in the corners
  (1001, no reason).
- **Tests:** the unit test `a_ball_that_does_not_fit_is_no_room_before_the_corners`
  (`blend/fillet_executive/session/finish.rs`) now asserts that refusal, without the flag. The
  tests that set the flag drop the setting.
