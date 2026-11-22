# query.face_p4 — exact loop-classification disagreement (2026-10-24)

Fresh full run: 500 cases, 492 passed, 1 failed, 5 deferred, 0
`skipped(platform)`, 0 rejected, 2 known mismatches; `max_rel_err=0`.
The sole failing case is `gen0360`, seed `4634847627957580463`: a swapped
scale-1 cylinder (`a=1.3301972357953764`, `c=1.74191392660755`), one
vertical UV line at `u=0.3916538774595547` and one rectangular UV loop,
face 33, imported stage-3 props (`props=1`). C++/Rust differ in
`ints.classify_loop_ios[8]=1/2`, `[10]=1/0`, `[11]=1/5`, and
`ints.looptree.len=17/9`. A standalone
`--filter gen0360 --no-report` reproduces all four integer mismatches.
There are not two or three new failing cases to sample; `gen0157` and
`gen0300` remain separate pre-existing known mismatches.

Class: reproducible geometry/classification divergence, provisionally
platform-sensitive, with the first causal operation still unlocated. It is
not a small compared floating-point residue (`max_rel_err=0` does not make
different integer classifications equal), nor does isolation suggest a
scheduling-dependent worker failure. The runner documents approximate imported
3D curves as a classification hazard; whether that or a native platform-specific
branch explains this scale-1 cylinder remains unproven. What would settle it:
replay this exact input on x86-64 and compare the imported curves, bisector and
first containment decision. Neither #773's conversion-only restrictions nor
#793's deform-only skips establish an exemption for this case. No new upstream
issue without evidence that the native result itself is defective.
