#### WR-538. `SmTorus::AssertValid` tests its U interval twice and never its V interval, under the wrong report ids
- **Severity:** Wrong result (validation gap)
- **Area:** Surfaces (tori)
- **Location:** `source/SMLib/src/SmTorus.cpp`, `SmTorus::AssertValid` (def. `:1661`): the V overlap test reads `m_vAnalUVDomain.GetUInterval()`. The U and V tests report as 2 ("Same Parameterization") and 3 (the U label). `source/SMLib/src/SmSurfOfRevolution.cpp`, `SmSurfOfRevolution::AssertValid` (def. `:6152`): test 13 reports as 2.
- **What happens:** A torus whose V (minor) interval runs past a turn passes the overlap test, which reads the U interval again. The two overlap tests and the surface of revolution's U test also report under ids whose labels describe other tests.
- **Trigger:** a torus with V in [0, 400]. With the fix it fails test 4 (offset 40); before, only the unrelated parameterization test notices it.
- **How found:** tangency-labs/solidcore#482, at the time closed as not reached by the port. The fork's fix is tangency-labs/smlib@0cd9d08 ("the torus check tests its minor interval"), which is not merged upstream.
- **Confirmed:** by reading the pinned source (`3fd5b95`). The port's `torus_assert_valid` reproduces it.
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.torus_check_reads_v_interval`, the torus's V test reads the V interval, and the U and V tests report as 3 and 4. The surface of revolution's U test reports as 13. Test: `wr_538_torus_check_reads_its_v_interval` in `surface/surface/assert_valid.rs`.
