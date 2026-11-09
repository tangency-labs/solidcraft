# General: WR-537 `seam_point_solutions_are_one`: the hair-off pin is empty by default in the port (2026-10-31)

- **Flag:** `Kernel::fixes.seam_point_solutions_are_one` (WR-537, fork tangency-labs/smlib@577f7be).
- **Audit:** class B. `tests/wr_537_pin_a_hair_off_a_seam.rs` reaches the guard with the flag on, but its assertion (an empty intersection) also holds with the flag forced off.
- **Tried (origin/main 36f1a9a67, instrumented):** a 40 x 40 x 10 plate less an r 3 hole at (20, 20), intersected with an r 3 pin offset along x by 1e-7, 5e-7, 1e-6, 2e-6 and 1e-5, with the flag on and off.
- **Result:** every case is an empty body (0 faces) both ways. The record's default failure (`SM_ERR` in `Homogenize`, from a seam point with two solutions) does not happen in the port for these pins, so the flag's branch changes nothing that can be asserted.
- **Waits on:** a placement whose seam point's solve really returns both period ends (SolidCraft's inspect.interference part; the record gives u = 6.39e-7 and 18.8496 on a face domain [0, 18.8496], which suggests a seam angle or face domain other than `create_cylinder`'s). No fork fix is missing in front of the branch.
