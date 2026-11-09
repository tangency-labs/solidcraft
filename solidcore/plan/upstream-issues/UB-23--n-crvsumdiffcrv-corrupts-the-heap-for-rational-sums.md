#### UB-23. `N_CrvSumDiffCrv` corrupts the heap for rational sums
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: curves, fitting, surfaces
- **Location:** `NL_CrvAdv.cpp:10058` `N_CrvSumDiffCrv`; products at `:10136`/`:10142`, sums at `:10155`/`:10160`.
- **What happens:** `N_CrvFuncMultiply*` rescales the knot vector of an operand in place between the two products. When `tmp` comes out with more control points than `num`, the loop over `tmp`'s `n` writes past `num`'s control points.
- **Trigger:** A rational sum of two curves over different parameter ranges, or with interior knots closer than `NL_PTOL` but not equal.
- **How found:** crash in the C++ reference run (`double free or corruption`, `free(): invalid next size`).
- **Confirmed:** yes.
- **Severity:** crash/UB (heap overflow).
- **Rust port:** Returns `InvalidInput`.
