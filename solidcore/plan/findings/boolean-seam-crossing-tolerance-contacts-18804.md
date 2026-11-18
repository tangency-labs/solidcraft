# A nearby notch is not a seam crossing

Issue: SolidCraft #18804; stress corpus S1558/S1746 and S0321/S0583/S1100.

## Reproduction

`solidcore-ops/tests/boolean.rs` constructs the upper half of a torus of major
radius 1.8 and tube radius 0.6. A cylinder of radius 0.07 and height 0.3 starts
at z=0, centred on its outer equator at longitude `(0.07 + d) / 2.4`, for
`d = 1e-5, 2e-5`. Both operands pass through their records, as in SolidCraft.
The corresponding large-scale recipes have major radius 500, tube radius 0.5,
notch radius 0.06 and depth 0.25.

Before the change, this command exits 101, with one failed test:

```
buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops --test boolean a_notch_beside_a_half_torus_seam_keeps_its_boundary_on_the_seam -- --nocapture
```

At d=1e-5 the failure is `SmFace::Test #29 Face crosses Seam`.
The torus face's U seam classification has its interior on the existing seam
edge (EdgeId(1)), with additional endpoint contacts on the notch's native
intersection edge (EdgeId(3)). Their reported gaps include
`1.200459439010687e-6`, `1.22056123325954e-6`, `1.199260859549311e-6` and
`1.1920888994140304e-5`. Neither a missing nor a near-miss seam is reported.

## Cause and construction

`topology/face/p4.rs::classify_seam` classifies within the boundary tolerance
band. `intersect/curve_class/p7.rs::is_crossed_by_edge` treats an interior
edge contact as a crossing, including a tolerance contact between disjoint
curves. Moving the original seam vertex cannot remove a false contact farther
along the meridian.

For the small recipe at d=1e-5, the cylinder's minimum y is
`2.4 sin(0.07001 / 2.4) - 0.07 = 7.139984192772175e-8`, strictly positive.
The native intersection spline also stays on +Y: evaluating its cubic spans'
stationary points gives a minimum about `3.5340299647140024e-8`. These numbers
diagnose the false contact; they are not acceptance thresholds.

The check now excludes a crossing candidate only with whole-domain separation
evidence. On a nonprocedural rational surface clamped in both directions, the
seam's native isocurve is the exact boundary control row. Native curves' piece
control points carry coordinate-error bounds (`Curve::piece_hull_points`,
`nurbs_crv::crv_piece_hull_points`). Outward arithmetic boxes their convex
hulls. A parameter rectangle is discarded only when its two boxes are strictly
disjoint on an axis; otherwise it is bisected. All rectangles must be discarded
to prove separation. Unsupported definitions, invalid domains, contact, and
exhausted subdivision grant no relief. No geometric tolerance changes.

This uses the exact-definition and whole-span enclosure rules in
`docs/definitions-and-certificates.md`, the seam invariant in
`docs/validity-and-tolerances.md` section 6, and the native convex-hull error
derivation in `nurbs/nurbs_crv.rs::piece_slack`. It does not use the surface's
display approximation or a sampled box. The proximity tests include a curve
with positive endpoints and midpoint that crosses twice between them, as well
as tangency and a positive curve with negative control points.

## Independent volume

Let R be the major radius, a the tube radius, b=R+a the outer radius and r the
notch radius. At cylindrical radius rho the torus's upper half has height
`sqrt(a^2 - (rho-R)^2)`. The notch covers an angular width
`2 acos((rho^2 + b^2 - r^2)/(2 rho b))`. Its depth exceeds the torus's height
throughout the footprint, so the removed volume is

```
integral[b-r,b] sqrt(a^2-(rho-R)^2) * 2 rho
    * acos((rho^2+b^2-r^2)/(2 rho b)) d rho.
```

The result volume is `pi^2 R a^2` minus this integral, independent of notch
longitude. The regression evaluates the integral at two quadrature resolutions.
For (R,a,r)=(1.8,0.6,0.07) the removed volume is approximately
0.00133018689643338 and the result is 6.39417346500947. For
(500,0.5,0.06), they are approximately 0.0008318638064 and
1233.69971827236. No golden or kernel-case expectation is taken from the
boolean's measured answer.

The recipes that reach the notch after three blind holes additionally remove
`3 pi (0.06)^2 (0.24)`. Each whole cylinder lies within the half torus and the
three holes have disjoint interiors. For the recipes with a closed hollow,
the inner torus has tube radius `c=a-wall` and lies above `z=wall`. Its void
volume is `2 pi R (c^2 acos(wall/c) - wall sqrt(c^2-wall^2))`, by revolving
that circular segment about the torus axis. Its maximum radius
`R + sqrt(c^2-wall^2)` is less than `b-r` for both corpus wall sizes (0.072
and 0.06), so the void and notch have disjoint interiors. These preparatory
volumes subtract independently from the result above.

## Validation

The existing boolean test file passes: 66 tests passed, none failed, and its
pre-existing slow union-of-sweeps test remains ignored. The new notch regression
passes both offsets at both scales, including the prior holes and closed voids.
The three affected kernel test modules pass all 21 tests, including
`disjoint_curves_need_whole_span_separation` and the existing classification
checks. The commands were:

```
buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops --test boolean -- --nocapture
buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml -p solidcore --lib -- curve::proximity::tests intersect::curve_class::p7::tests topology::face::p4::tests
buildq run solidcraft . -- cargo clippy --manifest-path solidcore/Cargo.toml -p solidcore -p solidcore-ops --all-targets -- -D warnings
```

Clippy reports no warnings. Formatting passes in both workspaces. No golden or
kernel-case expectation files were edited; broader checks remain for CI.

The uncut operand and the validated first small-scale result were saved as
native ASCII SMB records and drawn with `solidcore-diag` at the same view
(`-35,30`) and size (`1600x1200`). Both shaded renders mesh every face: zero
faces refused. Body and edge tolerances remain `1e-5` before and after the cut.
