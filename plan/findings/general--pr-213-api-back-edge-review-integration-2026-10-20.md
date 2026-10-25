# PR #213 — API back-edge review integration (2026-10-20)
Merged canonical main `e5f3e441`, retaining both appended findings and upstream
issue sections. The API plane and disc call the p1 static constructors with the
caller Kernel, native `1e-5` tolerance and no-face `SM_ERR` checks. Other new
solid-B-rep helpers defer at upstream's first missing `SmBrep::SmBrep`
(`topology.brep_p3`), with the available p1 constructor/methods documented at
their call sites. All thirteen stale p1 tags are removed.
`SmApiCreateBlendPrimitive` is different: pinned `SmApiPrimitives.cpp:1182`
uses the caller's existing B-rep. It now constructs `PrimitiveCreation` with
that B-rep's infinite region before the missing p2 blend operation; it is not
retagged as a new-Brep constructor deferral. The planar-boolean cleanup now
forwards the caller's existing notification environment into p5. Two Rust
static-sheet regressions reach the actual p3 constructor blocker rather than
the former p1 stub. Successful primitive topology remains uncertified.
### PR #233 final shell-query and native verification
The newly available `GetShellVertices` now delegates to p2 for the actual owner
and native vertex-list order. Eight native-feature sweep service regressions
pass, including asymmetric owner lists; both p3 access regressions also pass.
All thirteen affected native gates have zero failures and maximum relative
error zero. P3 remains 296/285 pass/10 deferred/1 rejected, sweep 568/558 pass/
10 deferred/0 rejected, and p2 is 1709/1437 pass/272 deferred. Native sweep records
do not expose new vertex coordinates; their owner/point checks are Rust-only.
Complete primitive topology and nonempty HCR/merge/stitch continuations remain
uncertified. Final full-gate and actual-head CI evidence is recorded in the PR
discussion. No production code uses the earlier unavailable-query stub.
