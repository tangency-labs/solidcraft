# Offset: a periodic seam is not taken for vanished when the rim root lies behind the far end (#19714)

Part of #19714. Builds on the seam-removal construction of #18803
(`offset.offset_executive_p2--periodic-boundary-construction-18803.md`).

## Cause

A boss's flat side pokes into a revolved bore cone. Its lens face is bounded by a hyperbola
(plane ∩ cone) whose apex lies on the cone's seam, so the seam splits the hyperbola into two
edges. Mating one of them, from the seam vertex V to the far vertex W, `periodic_boundary_end`
takes the candidate interval from the far root on the open face to the root on the bore's
bottom rim. That interval lies on the piece of the hyperbola behind the far end, so it carries
no seam crossing. The function therefore read the seam as vanished, and both intersection
branches claimed the same boundary event (`PeriodicBranchAmbiguous`).

The interval is the edge's image only if it leaves each far support the way the edge does at
its far vertex. The test checked for a chart crossing on the interval but never that the
interval leaves the far end in the edge's direction.

## Rule

For a far support with third-face normal N3 at the far vertex W, with the edge tangent T_e
at W directed toward the mated vertex, and the candidate tangent T_c at the far root directed
toward the rim root, with N3' the normal of the displaced, extended support at the root's uv:

- before = T_e · N3
- after = T_c · N3'

The candidate is the edge's image when both have the same sign. A regular offset keeps the
base surface's parameters and orientation, so N3 and N3' share a sense. A zero, non-finite or
missing value leaves the side unestablished: the end is `Unresolved`.

Measured signs (`notes/evidence/run5.log`, `run6.log`):

| case | before | after | verdict |
|---|---|---|---|
| #18803 S1983 half torus, seam really vanishes | -0.8646 | -0.2985 | same side: event kept |
| minimal recess below, wrong branch | -0.7619 | +0.8881 | other side: ordinary trim |

Normals: the z = 21 plane gives N3 = N3' = (0, 0, -1); the half torus gives (0, 0, 1).

## Test

`offset::tests::a_post_in_a_conical_recess_shells_open_at_its_bottom` (solidcore-ops,
`offset/tests.rs`): a 30 x 30 x 20 block with a conical recess (radius 7.2 at z = 0, falling
0.4 per mm), a 12 x 12 post from z = 1. Shelled open at the post's bottom with wall t = 0.4
and t = 0.2, then `check(CheckLevel::All)` valid and the volume compared to the closed form.

Input volume: 17835.098082931832 (the block less the recess).

The core is the same solid with every face moved t into the material, with the bottom left
open: the block inset by t, less the cone r(z) = 7.2 + t sqrt(1.16) - 0.4 z outside the post
of half side 6 - t from z = 1. Its volume is a frustum plus the integral, from z = 1 to where
r(z) = 6 - t, of the disc's area outside the square, 4 (r^2 acos(s / r) - s sqrt(r^2 - s^2)).

Independent evaluation (`notes/evidence/expected_min.py`, plain Python, one composite Simpson
integral after z = zt - u^2 to remove the 3/2-power end; identical at 4096 and 65536
intervals):

```
7.2 3.2 10.0 1.0 t 0.4: input 17835.098082931832 shell 1631.138823196248
7.2 3.2 10.0 1.0 t 0.2: input 17835.098082931832 shell 822.872121654742
```

The test asserts these at relative 1e-9. The base refuses t 0.4 with
`OffsetRefused { face: None, why: Unsound { what: NotManifoldSolid } }`.

## What stays refused

The issue's design shells at 0.4 mm with and without Fillet1. It does not shell at 0.9 mm or
above: the open boss bottom changes topology above about 0.80 mm (the bore cone passes the
moved boss corners, the bottom's arcs vanish and non-neighbouring boss sides must meet), which
is refused as `LoopsCross`. Above 1.0 mm the boss's corner cylinders collapse and
`procedural_offset` answers `InvalidInput`. Both are the arrangement of #13827 (S3 to S5),
not built here; tracked by #13827.
