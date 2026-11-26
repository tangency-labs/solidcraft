Updates: WR-663

#### WR-663 (update, 2026-11-01). A refined parameter outside the range is an answer, not a failed status
- **What changes:** with `Kernel::fixes.query_statuses_pass_on`, `sm_RefineCCSolutions` no longer returns the status of `Inversion`. A refined end just past the solution's range keeps its fraction, and the shift limit judges it. Only a zero-length range gives no fraction, and then the end stays where it was. With the flag off nothing changes.
- **How found:** this site took 8311f8e's `SER(Inversion)` from the fork, but not the fork's later tangency-labs/smlib@8ea6a20 (9b93b88 before cherry-picking), which takes it back out. The fork at d01d7b0 has `(void)sSolIvl1.Inversion(..)` and `bHaveFracs`. The port's `SM_ERR` aborted the whole curve/curve intersection as `geom::extent1d::Extent1d::inversion`.
- **Confirmed:** in the Rust port.
  - #948's `min7.txt`: with this flag alone the difference failed in `Extent1d::inversion`, earlier than the default path. It now fails where the default path does, in `topology::brep::p6`. The fork fails that difference with 1001 too, so this does not fix #948.
  - The order-2 round of the pipe on a plate (#2252's `pipe_on_plate_g2`), on #2260's head 9f3cdcfbc with every fix on except `geometric_g2_continuity`: `Extent1d::inversion` 1001 before, built after. Volume 40258.543608517 against the fork case's 40258.5435962825 (3.0e-10 relative), seams G2. That case needs #2260's generator, so it is not a test here.
  - The fork's C++ was run only as far as #948's status (d01d7b0 built from `source/SMLib` and `source/SM_API`).
