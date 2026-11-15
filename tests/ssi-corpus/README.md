# SSI corpus

119 surface/surface intersection (SSI) cases with exact ground truth, from
[ssi-corpus](https://github.com/jyviko/ssi-corpus) (`cases/data`, commit b04c8a6), dual-licensed
MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE` here). Each case is stored gzip-compressed as
`cases/<id>.json.gz` (the raw JSON is 31 MB; compressed, 9 MB). The content is unchanged.

The 25 `y23_*` cases come from Yang, Jia and Yan, "Topology Guaranteed B-Spline Surface/Surface
Intersection", ACM Transactions on Graphics 42(6), Article 211, 2023 (CC BY 4.0): six use the
paper's published surfaces, nineteen are reconstructions of its figures with closed-form truth.
Each cites the paper in its `source` field.

## A case

- `surfaces`: the two surfaces (plane, cylinder, cone, sphere, torus as typed parameters; B-spline
  and NURBS as control nets with knots and weights, optional `domain` trim box).
- `truth`: one exact description per surface, used for distances: implicit `plane`, `sphere`,
  `cylinder`, `cone`, `torus`, `ellipsoid`, `quadric`, polynomial `heightfield`, or `param` (a
  rational polynomial patch, distance by closest point).
- `expected`: the true intersection, sampled densely: `branches` (polylines), `isolated_points`,
  `singular_points` and `coincident_regions` (boundary polylines).
- `cov_tol`: the coverage tolerance; optional `ill_cond` (conditioning factor) and `feature_size`.

## How the tests use it

`crates/smlib/tests/ssi_score/` is a Rust port of the corpus scorer (`scoring/score.py` and the
distance helpers of `scoring/ssi_geom.py`). It takes a case and an engine result (sampled curves
with `xyz`, `uv1`, `uv2`, both surfaces evaluated at the UV samples, the curve type; isolated
points; the status; or a crash) and returns the verdict. `crates/smlib/tests/ssi_corpus.rs` checks
the scorer on every case, and, as an ignored test, its parity with the Python scorer on the raw
runner outputs (see that file).

## Scoring rules

With an approximation tolerance `tol`, `acc_tol = 10 * tol`. The coverage tolerance is `cov_tol`,
widened for ill-conditioned cases to `max(cov_tol, min(0.5, 2 * acc_tol * ill_cond))`. Runs at the
engine's default tolerance have no `acc_tol` and judge coverage and joins with a floor of 5e-3
(cases with a `feature_size` under it are N/A). Output curves of type TOUCHING, or no larger than
`cov_tol`, count as points. Verdicts, most severe first (all flags are kept):

- CRASH: the engine died or exceeded the time limit.
- ERROR: a non-success status, unless nothing was expected and nothing was returned.
- INCOMPLETE: an expected branch, isolated point or coincident region boundary is not covered
  within the coverage tolerance.
- WRONG TOPOLOGY: output off the true intersection; a different set of connected components
  (curves with their free-end count, and points, joined within `cov_tol`); two output curves that
  overlap by more than 20%; a singular point that no curve end or point lies on; non-finite
  samples.
- INACCURATE: a 3D sample farther than `acc_tol` from either surface, or a surface evaluated at
  the output UV farther than `acc_tol` from the other surface.
- PASS.
