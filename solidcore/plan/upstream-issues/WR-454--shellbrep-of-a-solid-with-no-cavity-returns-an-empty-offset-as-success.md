#### WR-454. `ShellBrep` of a solid inward returns success when the inset leaves no cavity
- **Severity:** Wrong result
- **Area:** Offset
- **Location** (at 3fd5b95): `source/SMLib/src/SmOffsetExecutive.cpp:191`-`:360`, `SmOffsetExecutive::ShellBrep` (the static overload): after `DoSolidOffset`, a non-manifold offset is returned with `SM_SUCCESS` at `:255`-`:262` (`// was SM_ERR: ??`).
- **What happens:** an inward shell of a closed solid whose wall is at least the body's inradius offsets to nothing. `ShellBrep` then sees a result that is not a manifold solid and, without `bCreateOffsetSolid`, returns it as `SM_SUCCESS`; the caller gets a body with no faces (the fork's report: the body unhollowed) where it should get an error. In the C++ build the same input also reached freed memory in the offset code (fixed in the fork).
- **Trigger:** a 10 x 10 x 10 box shelled inward by 5 (the inradius) or 6, extended convex edges and merged results.
- **How found:** fuzzing the shell through the C API in tangency-labs/smlib (`capi/fuzz/fuzz_ops.cpp`), tangency-labs/solidcore#380.
- **Confirmed:** no (read at 3fd5b95 `SmOffsetExecutive.cpp:255`-`:262`; the fork's test calling `SmApiShellBrepFull` past the shim's guard is not the pinned C++). In the port, the box above returns an empty B-rep as a success.
- **Fix:** tangency-labs/smlib@fbe43bb: for an inset of a manifold solid, an offset without faces fails with `SM_ERR_INVALID_INPUT`, and one that is not a closed solid with `SM_ERR`.
- **Rust port:** `OffsetExecutive::shell_brep` behaves as upstream by default. With `Kernel::fixes.shell_empty_cavity_fails` it fails as the fork does. Test `issue_380_shell_with_no_cavity_fails_with_the_fix`: a 10-cube shelled by 5 and 6 (an empty success by default, 1007 with the fix) and by 1 (a solid either way).
