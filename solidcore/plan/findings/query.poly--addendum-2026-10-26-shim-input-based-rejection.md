Updates: plan/findings/query.poly--opaque-owner-is-degenerate-rejected.md

The shim no longer writes a `-9` sentinel that difftest inspects. It decides from the built
model (the edge's poly face has an original-face label that no explicit owner resolves,
and `OKBackPtrs` holds), before any upstream call, and returns `STATUS_UNDEFINED_INPUT`
(-1101); `run_cpp` maps that status to `Output::rejected`. Same cases rejected (gen0378,
gen0496; 24 at `--seeds 6000`), same `owner_degenerate` coverage, library unchanged. The
shim comment that was outdated ("the Rust side defers this") is gone with the sentinel; the
shim source changed, so the prebuilt C++ library key changes.

intersect.curve_class_p1 (face point class guard): with the shim's zero-size-UV-domain
guard removed, a face whose UV domain size is (0,1), (1,0) or (0,0) segfaults in
`sm_ComputeFacePointClass`; sizes down to 1e-100 on the other axis run. Rejecting either-axis
zero is therefore exact, not over-broad. Cause not located.
