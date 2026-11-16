Updates: WR-661

#### WR-661 (update, 2026-10-31). The standalone corner path's big-radius conversion follows the flag too

`SmFilletExecutive::TestAndConvertBigRadFillet` (`source/SMLib/src/SmFilletExecutive.cpp:5660`)
drops the status of `CalcRailIntRail` on the squeezed rail's start vertex; the fork
(tangency-labs/smlib 873c7c9) passes it on with `SER`. The port has two copies of the
routine. The session path (`blend/fillet_executive/session/big_radius.rs`) already passed
the status on under `Kernel::fixes.fillet_statuses_pass_on`. The standalone corner path
(`FilletExecutiveData::test_and_convert_big_rad_fillet_with_running`,
`blend/fillet_corner/p3.rs`, reached from `SmFilletCorner::AdjustTopology`'s
`TestAndConvertBigRadFillet` calls) still dropped every `SolidError::Upstream` whatever the
flag said. It now goes through `crate::error::passed_on` with the same flag. With the flag
off nothing changes: `Upstream` is ignored and any other error is returned, as before.

Test: `blend::fillet_corner::p3::tests::fillet_statuses_pass_on_big_radius_returns_a_failed_rail_int_rail`.
It builds two opposed rails between two Nx2 corners. After the squeeze the start vertex has
no rail, so `CalcRailIntRail` returns `SM_ERR`. With the flag off the conversion carries on:
the kept rail becomes a cross section and the conversion stops later at the synthetic
corners. With the flag on it returns the `CalcRailIntRail` status before the kept rail is
touched. The test fails without the change. Refs #2009.
