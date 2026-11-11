# SMLib: sweeps, threads, shell and offset

A triage of the regression rig's sweeps, threads and shell cases, and of the ladder's tiers 03
(analytic profiles), 04 (spline profiles), 06 (threads) and 09 (shells and offsets): 169 cases,
each run on SMLib (`solidcraft-cli kernel-cases`) on `kernel-smlib` at 4b8953726 and graded
against the case's own answer ([kernel-cases.md](kernel-cases.md)). This page lists only the
cases that do not pass, grouped by what goes wrong, worst first; it is kept by hand as fixes
land.

| | Cases | Pass |
|---|---|---|
| sweeps | 83 | 51 |
| threads | 28 | 11 |
| shell | 18 | 11 |
| ladder 03, 04, 06, 09 | 40 | 26 |
| **all** | **169** | **99** |

A thread off its hole's or shaft's size is refused on this branch (the owner's rule): those
cases pass by refusing, and are not listed.

## A. Wrong geometry (worst)

| Case | SMLib | SMLib's result |
|---|---|---|
| sweeps/2667-revolve-half-disc-three-quarters | fail | a half disc on the axis turned 270°: SMLib makes a quarter ball turned inside out (V = -226, want 679) |
| sweeps/3528-revolved-dome-cut-by-a-ball-on-its-seam | fail | the ball's cut is lost where it crosses the revolve's seam: V 48118, want 46425 |
| sweeps/2847-loft-rectangles-turning-through-half-a-turn | fail | inside out (V = -127, want 2262) |
| sweeps/450-revolve-spherical-shell-sketched-off-the-origin | fail | two shells where one is wanted, which the B-rep record can't close |

## B. Crash

| Case | SMLib | SMLib's result |
|---|---|---|
| ladder/09-shells-and-offsets/011-part-shell | crash | step 21 (solid.fillet) on the SMLib body: curve blend internal error "Two same vertices cannot construct an edge"; the fillet is the blends work's, not this page's |

## C. Threads: the groove's boolean fails SMLib's check

The groove (SMLib's screw sweep) cuts the body, and the result fails SMLib's own check: vertex
gaps of 1–6e-5 mm against vertex tolerances of 4–20e-6 mm (Test #0/#2 Bad Gap), an edge whose
geometry fails validation (Test #61), or a trim curve that does not lie on its edge when the
result is read back (SMLib export: pcurve).

| Case | SMLib | SMLib's result |
|---|---|---|
| ladder/06-threads/001-external-m10-full | fail | Bad Gap (1.89e-5 > 8.6e-6) |
| ladder/06-threads/002-external-m10x1-fine | fail | Test #61 bad edge |
| ladder/06-threads/003-external-m10-partial-length | fail | Bad Gap (1.91e-5 > 8.9e-6) |
| ladder/06-threads/004-external-m10-to-shoulder | fail | Bad Gap ×10 |
| ladder/06-threads/005-internal-m10-through | fail | Bad Gap (3.6e-5 > 1.8e-5) |
| ladder/06-threads/006-external-m10-into-chamfer | fail | Bad Gap |
| ladder/06-threads/007-external-m10-at-filleted-shoulder | fail | Bad Gap |
| ladder/06-threads/010-part-threaded-boss | fail | Bad Gap (2.26e-5 > 2.0e-5) |
| threads/1250-external-m10x1.5-shaft-full-length | fail | Bad Gap ×9 |
| threads/1395-long-external-m6-thread-24-turns | fail | Test #61 bad edge |
| threads/1995-m8x1-thread-in-7mm-hole | fail | Bad Gap (1.5e-5 > 7.5e-6) |
| threads/2038-cut-threaded-stud-from-bored-block | fail | Test #61 bad edge |
| threads/2038-internal-then-external-thread-on-one-body | fail | pcurve off its edge |
| threads/2038-join-two-threaded-bodies | fail | pcurve off its edge |
| threads/2193-modelled-m33x3.5-thread | fail | Bad Gap |
| threads/2209-fine-m44x1-thread-on-shaft | fail | Bad Gap ×7 |
| threads/2209-m44x1.5-thread-on-flange-hub | fail | Bad Gap (6.2e-5 > 2.05e-5) |
| threads/2406-modelled-m8-thread-in-6.8mm-pilot-hole | fail | Bad Gap |
| threads/2526-three-modelled-threads-in-one-body | fail | pcurve off its edge |
| threads/2822-modelled-thread-in-filleted-cube | fail | pcurve off its edge |
| threads/3311-two-tapped-holes-both-modelled-threaded | fail | pcurve off its edge |
| threads/3914-sketch-on-top-face-of-threaded-rounded-block | fail | pcurve off its edge (0.86 mm) |
| threads/3952-modelled-thread-in-blind-hole-of-revolved-ring | fail | pcurve off its edge |
| threads/4149-internal-thread-below-counterbore-filleted-rim | fail | Bad Gap |
| threads/602-fillet-hex-head-beside-modelled-thread | fail | Bad Gap |

## D. Coils and other sweeps in booleans

| Case | SMLib | SMLib's result |
|---|---|---|
| sweeps/793-coil-cut-by-a-box-above-a-plane | fail | pcurve off its edge |
| sweeps/793-coil-split-by-a-plane-across-its-axis | fail | pcurve off its edge |
| sweeps/2552-thread-groove-cut-by-a-square-coil-in-a-bore | fail | the boolean's result is not a closed solid |
| sweeps/96-loft-cut-box-sections-pad-first | fail | the boolean's result is not a closed solid |
| sweeps/96-loft-cut-box-sections-top-first | fail | the boolean's result is not a closed solid |
| sweeps/844-join-two-overlapping-swept-arc-walls | fail | boolean status 1006 |
| sweeps/3006-second-sweep-from-a-plane-along-a-closed-path | fail | read back: a face no shell uses |

## E. Shell and offset refused

| Case | SMLib | SMLib's result |
|---|---|---|
| ladder/09-shells-and-offsets/006-shell-rounds-r1-thinner-than-t2 | fail | shell status 1001 (rounds of r1 inside a 2 mm wall) |
| shell/1494-shell-box-with-cylinder-on-edge-line | fail | shell status 1001 |
| shell/1525-shell-box-with-rounds-chamfer-and-fillet | fail | shell status 1001 |
| shell/2939-offset-face-sphere | fail | check: edge/surface gap 5.015e-6 > 5e-6 |
| shell/3468-offset-face-boss-wall-next-to-fillet | fail | check: an edge tolerance of 2.0 mm |
| shell/2531-shell-point-pick-after-parameter-edit | fail | V 17952, want 22112: the shell's face pick after the edit |
| sweeps/2820-pipe-hollow-with-a-wall-along-a-bend | fail | sweep status 1001 |

## F. The engine or the case

| Case | SMLib | What |
|---|---|---|
| sweeps/3345-sweep-profile-mid-line-round-a-rounded-rectangle | fail | bbox ±46 × ±36, want ±32 × ±22: the profile's placement, not the kernel |
| sweeps/2516-revolve-about-a-named-construction-axis | fail | unknown sketch line `Axis1` |
| sweeps/3191-revolve-cut-leaves-a-cone | fail | the profile crosses the revolve axis |
| sweeps/4056-surface-loft-rails-…-stern-to-bow | fail | a guide rail misses section 2 |
| sweeps/2570-loft-circle-to-a-non-star-shaped-comb | fail | an arc turns more than once |
| shell/4009-closed-shell-internal-void | fail | `solid.shell` needs 1…1000 faces (a closed shell has none) |
| ladder/06-threads/008-multi-start-thread | unsupported | no start count yet |
| ladder/06-threads/009-thread-runout-length | unsupported | no runout length yet |

## G. No SMLib path yet ("not supported yet")

| What | Cases |
|---|---|
| sweep with a twist or an end scale | sweeps/1269-sweep-twisted-square-ninety-degrees, 2005-closed-sweep-twisted-new-body, 2005-closed-sweep-twisted-cut-inside-a-torus, 2891-twisted-sweep-cut-through-a-tube, 771-sweep-end-scale-cone-frustum |
| sweep along a path with corners | ladder/04/003-pipe-along-bezier, ladder/04/004-sweep-square-along-bezier, sweeps/96-sweep-rectangle-round-a-mitred-corner, 561-extrude-the-end-face-of-a-sweep, 3505-extrude-the-flat-wall-of-a-sweep-with-a-corner |
| loft through sections with holes | sweeps/1184-loft-ring-sections-keep-the-hole, 2435-loft-rectangular-ring-sections, 3111-loft-rectangular-ring-to-circular-ring |
| guide rails | sweeps/201-loft-one-guide-rail-cone-frustum, 771-sweep-one-guide-rail-cone-frustum, 771-sweep-two-guide-rails-cone-frustum |
| sweep along a path of points | sweeps/2862-three-pipes-meeting-at-one-point |
| draft extrusion with a straight foot | shell/391-deboss-text-on-flange-of-filleted-shelled-body |
