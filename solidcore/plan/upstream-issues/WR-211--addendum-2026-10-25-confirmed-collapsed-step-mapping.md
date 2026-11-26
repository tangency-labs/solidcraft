Updates: WR-211

# WR-211: reconfirmed and refused by the Rust port (#428, PR #686)

The prior unconfirmed/faithful-port note under WR-211 is superseded by a direct run
of the existing pinned C++ library. `SmCircle::AdjustSTEPInterval([360,540])` from
`[0,360]` and `[0,180]` returns `SM_SUCCESS` (native1000), with a NURBS domain `[1,1]`.
Calling `EditParameterization([360,540])` then yields exactly
`360 360 360 NaN NaN 540 540 540`. A requested `[90,90]` additionally expands the STEP
interval to `[90,450]` while collapsing NURBS to `[.25,.25]`, then also creates NaNs.
The probe stops before a surface clamp can hit UB-250 / #427. The C++ code was not
modified. Twenty native ellipse fixtures (eight bad edits and twelve healthy
controls), existing generated programs and caller regressions cover the Rust fix.
The port now returns `InvalidInput` for an empty/non-finite converted NURBS interval
before modifying the ellipse. A revolution's STEP adjustment also retains its old
surface domain when that generator edit is rejected. This uses #428's safe-refusal
option; periodic range extension is not implemented. The finding
[`curve.ellipse_p1--reject-collapsed-step-intervals.md`](../findings/curve.ellipse_p1--reject-collapsed-step-intervals.md)
lists all 111 exact native comparison divergences without counting any as a pass.
