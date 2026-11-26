#### UB-134. `ExtractGeometry` slices a lazy analytic UV curve into an uninitialized B-spline
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmCurveClass.cpp:13524`-`:13534`, the `SmBSplineCurve` copies of the two UV curves and their `Trim` calls.
- **What happens:** An `SmLine` is a B-spline subclass but can have no materialized NURB. The explicit base copy keeps that null NURB; trimming the sliced curve calls `SmBSplineCurve::Evaluate` through `SmTol::MapTo1d` and dereferences it.
- **Trigger:** a common classified interval and a requested UV output whose valid `SmLine` has not had `MakeNurb` called. Reproduced with a 3D circle of radius 1e6, five face-classified intervals over `[0,1]`, and both UV line outputs.
- **How found:** native differential generation for `intersect.curve_class_p5`, then debugger tracing of the failing input and stack on Linux x86_64 / GCC 15.2.0.
- **Confirmed:** yes: SIGSEGV in `SmBSplineCurve::Evaluate`, called by `SmTol::MapTo1d`, `SmBSplineCurve::Trim` and `SmCurveClassification::ExtractGeometry`. The oracle now materializes both UV line NURBs before calling the upstream method; the fixed regression retains the input.
- **Severity:** crash / undefined behaviour.
- **Rust port:** materializes a line's B-spline base before slicing it, returning an error if that fails.
