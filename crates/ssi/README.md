# ssi

Surface/surface intersection of rational B-spline patches, in Rust. The target is every case of the
[ssi-corpus](https://github.com/jyviko/ssi-corpus): transversal, tangent and singular intersections,
boundaries, coincident regions, near-degenerate pairs, seams and poles.

The method began as Form Studio's `src/kernel/intersect.js`, ported: subdivision into Bézier pieces with
Sederberg and Meyers's loop test, then marching with a Newton corrector. Where a patch carries its
analytic surface (`Patch::analytic`, a `surface3::Surf`), the surface's implicit equation composed with
the other patch's pieces (`field`) proves pieces apart and patches coincident, and coincident patches
meet in the boundary of the region they share (`coincide`). On such pairs the intersection is traced on
the other patch's own parameters (`trace2d`), tangencies included: touch points and crossings are
critical points of the field, proved one to a cell by the Hessian's sign and the gradient's winding;
tangent curves are its valleys (and, for third- and fourth-order contact, the curves where the field's
lower derivatives along one parameter vanish), proved by coefficient bounds within the resolution or the
input's own rounding; a pole of the traced
patch on the other surface is divided out of the field. A B-spline patch without an analytic surface gets
one when a polynomial of degree 10 or less holds it exactly (`algebraic`: the null vector of the matrix
composing each monomial with the patch's Bézier pieces, built from their control points and taken only
where its least singular value is at the rounding, then composed with every piece to zero), so NURBS
cylinders, spheres and tori, and constructed free-form sheets of low implicit degree, take the field path
too. A plane against a sphere, or against a patch whose exact form has degree 2 and cuts it in an
ellipse, is solved in closed form before any of that (`quadric`): the conic from the plane's distance to
the center, or from the form's one value at the conic's center, then placed on both patches by inversion
and clipped to their domains. A pair with no such form is traced near its tangencies on local fields (`taylor`): the separation of
one patch from the other along a fixed direction, as a Taylor polynomial over a piece with proved bounds on
its value's and derivatives' errors, read by the same cell tests; the polynomial proves, the surfaces place
every point. Contacts it cannot prove are refused by name as `Error::Tangent`, never approximated.

No dependencies: the exact geometry (`geom3`, `surface3`: analytic surfaces, their feet, and a certified
branch-and-bound closest point on B-spline surfaces) is part of the crate, and `shapes` builds the exact
patches of the corpus's planes, cylinders, cones, spheres and tori.

No intersection point is chosen by thresholding distances at sample points; US patent 12,229,885
claims that. Points come from Newton roots, and regions are dropped only by proof.

## In Solidcore

In Solidcore's workspace (`crates/ssi`) the crate sits beside the port, not in it: it is a method of its
own, written without reading upstream SMLib, so it is no port unit and changes no unit's parity.
`difftest --ssi-corpus` runs it as a third engine, `ssi`, graded by the same scorer as the C++ reference
and the port; built without the `cpp` feature, difftest runs that engine alone.

```sh
scripts/fetch-ssi-corpus.sh
cargo run --profile difftest -p difftest -- --ssi-corpus                  # the ssi engine
cargo run --profile difftest -p difftest --features cpp -- --ssi-corpus   # cpp, rust and ssi
```

## Score on the corpus

```sh
cargo build --release
SSI_ENGINE=ssi-rs SSI_RUNNER=target/release/ssi_corpus_runner python3 -I <ssi-corpus>/scoring/score.py fs 1e-6
```

The scorer needs numpy. Results land in `<ssi-corpus>/results/score_ssi-rs_fs_1e-6.json`. The SMLib column
is the corpus's own runner on SMLib (1.0.0, then 1.0.1); Solidcore's report on the same corpus commit
(`reports/ssi-corpus.json`) has the C++ at 81 and the port at 78, with 9 cases deferred.

| Date | Pass (of 119) | SMLib | Notes |
|---|---|---|---|
| 2026-10-22 | 42 | 81 | Port of Form Studio's `intersect.js`; same verdict as it on all 119 |
| 2026-10-22 | 57 | 81 | Curves on seams, knot lines, patch edges and through poles (classes 5 and 8); no regressions |
| 2026-10-22 | 66 | 81 | Analytic fields through icam-core: coincident regions (class 6, all 7), pieces proved apart; no regressions |
| 2026-10-23 | 69 | 81 | Field path: analytic pairs traced on one patch's own parameters; near-tangent sunk cylinders and planes 1e-8 apart pass |
| 2026-10-23 | 69 | 82 | Baseline moved to SMLib 1.0.1 (exact cylinder path; c4_steinmetz now passes there). Timed interleaved per case on one machine: on the 58 cases both pass, median 1.16 ms against SMLib 1.0.1's 1.04, total 0.52 s against 0.19 |
| 2026-10-23 | 77 | 82 | Touch points and crossings on the field path (classes 3 and 4): one critical point a cell, by degree theory; crossings are curve vertices |
| 2026-10-23 | 85 | 82 | Tangent curves on the field path (class 2) as certified valleys of the field. On the 69 cases both pass, median 0.80 ms against SMLib 1.0.1's 0.97, total 0.49 s against 0.24 |
| 2026-10-23 | 92 | 82 | Poles on the traced side: a pole on the other surface divided out of the field, crossings there split apart. On the 75 cases both pass, median 0.64 ms against SMLib 1.0.1's 1.05, total 0.47 s against 0.35 |
| 2026-10-23 | 100 | 82 | Implicit forms for B-spline patches (degree 4 or less, proved by composition), fitted when a pair is refused: NURBS quadrics and tori on the field path. On the 77 cases both pass, median 0.91 ms against SMLib 1.0.1's 1.07, total 0.47 s against 0.36 |
| 2026-10-23 | 101 | 82 | Valleys below zero split into a bracketed zero on each side, valleys above it proved clear; the field's rounding band narrowed to 1e-14 of its scale (two lines 2e-6 apart from a cylinder sunk 5e-13 into a plane) |
| 2026-10-23 | 104 | 82 | Contact curves of third and fourth order (sheets tangent along a line with cubic contact, a quartic contact parabola); a short first pair budget before implicit forms are fitted (2.6 s to 0.15 s on that parabola). On the 78 cases both pass, timed interleaved: median 1.85 ms against SMLib 1.0.1's 1.98, total 0.80 s against 0.79 (this machine ran about twice as slow as for the rows above) |
| 2026-10-23 | 110 | 82 | Exact implicit forms to degree 10 from control points (composition-matrix null vector at the rounding, proved by composition), fitted in step across patches so the lowest degree wins: six free-form Yang 2023 cases. On the 81 cases both pass, interleaved: median 2.25 ms against SMLib 1.0.1's 2.90, total 1.67 s against 0.97 (machine about three times slower than for the first rows) |
| 2026-10-23 | 111 | 82 | Contacts zero to the field's rounding: a critical point there is a touch or a crossing whatever the curvature, cells about such a touch inside its rounding band are its own, a pole divided out with no arc at it is a touch point, and contact points on a collapsed edge are inside their patch (three nested shells' touch points, two at poles) |
| 2026-10-23 | 114 | 82 | Topology where branches cross or meet: a valley at most zero and dipping below the band is two branches meeting where it reaches zero (two loops tangent at a point), its side arcs broken there; branches joined straight through their crossings, the vertices reported as points; arcs traced twice either way kept once. On the 82 cases both pass, interleaved: median 0.93 ms against SMLib 1.0.1's 1.06, total 0.93 s against 0.40 |
| 2026-10-23 | 114 | 82 | Speed, same answers: each patch's Bézier pieces made once, coincident boundaries clipped with warm-started inversions, sweeps solved on their line, binomial rows shared, implicit-form proofs reusing the compositions already made, pieces proved off the other surface by their points before composing, quadric forms fitted up front for pairs with no analytic side. On the 82 cases both pass, interleaved: median 0.65 ms against SMLib 1.0.1's 1.08, total 0.43 s against 0.39; faster on 42 of the 82 |
| 2026-10-23 | 115 | 82 | Zero edges divided out on the field path: a cell edge on which the field vanishes to its rounding (or the implicit form's residual), to order up to 4, is itself a crossing or tangent curve, and dividing its factor out leaves the rest of the zero set resolvable. A cubic contact ending on a tube's crease, where the other tube's form is singular, now traces (`y23_t2_ex5_two_loops`); a crossing on a form's double curve takes the patch's own closest point; densified points stay in both patches' domains; an arc that runs out and back along a folded edge is kept once. Same speed |
| 2026-10-23 | 116 | 82 | Local fields where neither patch has an implicit form (`taylor`): on a pair of pieces that is not loop-free, the separation of A from B along a fixed normal as a Taylor polynomial over A's piece, with proved bounds on its value's, gradient's and Hessian's errors (an approximate root's residual and a contraction, from B's derivatives' exact Bernstein numerators); the field path's cells read them, and every point is placed on the surfaces themselves. Two degree-8 x circle tubes whose loops cross at two points now trace (`y23_fig1a_two_loops_cross`, 3.8 s: slow, each local field costs 17 ms). Tried only once no exact form exists. Same speed elsewhere |
| 2026-10-23 | 118 | 82 | Closed forms for a plane against a sphere or a degree-2 form (`quadric`): the circle's radius as √((R − \|d\|)(R + \|d\|)) with d a compensated dot product, an ellipse from the form's value at its center told from the form's rounding and residual. The two loops 1e-7 across now pass (`c7_tiny_loop_sphere_1e-7`, `c7_tiny_loop_parab_1e-7`), where the field path read their depth, 1e-14 of the field, as rounding and gave a point. Interleaved against the row above on the 116 cases both pass: median 0.60 ms against 0.77, total the same within noise; plane and sphere pairs 3 to 4 times faster, while five degree-2 NURBS patches that are not ellipse cases pay 0.1 to 0.5 ms for a fit that is set aside |

The one case left, `y23_fig1d_two_quads`, fails on every engine scored here. It is refused by name
(`BOOLEAN_TANGENT`): its patches, 1e-8 apart, separate only at second order in places, below the 4.4e-13 its
degree-9 form is exact to.

## License

MIT OR Apache-2.0, at your option. `geom3` and `surface3` come from ImplicitCAM's `icam-core`,
relicensed here under the same terms by its author.
