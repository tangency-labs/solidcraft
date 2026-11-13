Updates: WR-697, WR-616

#### WR-697 / WR-616 (update, 2026-11-02). Corners of four or more rounds are the Ball or the Setback (`fillet_blend_corner`, smlib#125)

- **Source:** the SolidCraft fork's tangency-labs/smlib#125, merged into `replay/1.0.1` at
  `18e96f1`. This is a port of its SMLib commits, not solidcore-native work. It runs under
  the existing opt-in `Kernel::fixes.fillet_blend_corner`, with no new flag and no new
  record id. The fork's capi commits (`fc29fdb`, `cb2a478`, `96e58a1`) and the capi parts
  of the others are not ported. They were read for the cases' inputs and expected values.
- **What happens (pinned and before):** a vertex of four or more equal rounds has no blend
  corner. The pinned `SmFilletConvexNxNCorner` (or `SmFilletConcaveNxNCorner`) builds it,
  and its patch only approximates the ball the rounds lie on. WR-502 is the square pyramid's
  case of this.
- **Four or more rounds** (`278bf77`):
  - `SmBlendCorner::ClassifyBall` takes `n >= 3` rounds; for `n >= 4` the faces must all be
    planes. The rounds' offset planes must pass through the point where the first three
    meet (`sm_BallPlanesMeet`, by Cramer's rule, within the rounds' tolerance).
  - `SmFilletCorner::Create` asks for it in the NxN case before `ClassifySetBack`.
  - The Ball's faces are the sphere's n-gon fanned from contact 0 into `n - 2` triangles,
    each the Ball's rational (4, 2) triangle patch with its pole at contact 0
    (`BuildBallSurface`, `CreateBallPatch`). `MakeBallTopology` and `BallCentre` take `n`
    rounds.
  - `ClassifySetBack` leaves only three equal rounds to the Ball, so four or more equal
    rounds without a common ball are a Setback.
- **Exact zeros in the convexity certificate** (`3e10318`): along a setback patch's side
  that is G1 to a face plane by construction (its first two rows of control points on the
  plane, `sm_SbSidesOnPlanes`), `L = -S_u . N_u` and `M = -S_v . N_u` are identically 0. So
  `sm_SbCertifiedConvex` sets their Bernstein rows there to exactly 0 instead of reading
  them from rounding.
- **WR-502** (`fillet_corner_sphere_from_four_arcs`) is kept as it is. With
  `fillet_blend_corner` a four-round apex with a common ball is the Ball's patches before
  `MakeAnalyticCornerSurface` is reached, so the WR-502 sphere is not built there. Without
  `fillet_blend_corner`, WR-502 behaves as before.
- **Rust port:**
  - `crates/solidcore/src/blend/blend_corner/ball.rs` (`classify_ball`, `ball_centre`,
    `ball_corner_edge_geom`, `create_ball_patch`, `planes_meet`; `BallCorner` holds the
    patches and their loops).
  - `crates/solidcore/src/blend/fillet_corner/topology/ball.rs` (`n` rounds).
  - `crates/solidcore/src/blend/fillet_executive/session.rs` (the dispatch, `n` solvers).
  - `crates/solidcore/src/blend/blend_corner/setback.rs` (`n == 3` in `classify_setback`).
  - `crates/solidcore/src/blend/blend_setback/certify.rs` (`sides_on_planes`,
    `certified_convex`) and `graph.rs` (the call).
- **Default unchanged:** without `fillet_blend_corner` every corner is the pinned one.
- **Tests:** `crates/solidcore/tests/fork_smlib125_four_round_corners.rs`, from the fork's
  capi cases (`ball regular_apex_4/5/6`, `setback four_face_apex`,
  `setback four_face_pocket_floor`) and WR-502's pyramid. They are checked by closed form and
  geometry:
  - `check_brep` Level2 passes, and the face counts are the sharp body's plus `2 n - 2`
    (Ball) or `3 n` (Setback).
  - Each Ball patch lies on the closed-form sphere (centre on the axis, `r / n_z` below the
    apex) to 1e-12 of the part's size, along its edges too. Its normal is radial, and it is
    G1 to every neighbour along its edges (1e-9 rad).
  - With the flag off, the regular apex of four is the pinned corner.
- **Open setbacks** (`ef57570`, slice 2):
  - `ClassifySetBack` no longer leaves a corner asked open (`SetSetBackFill(.., bOpen)`) to
    the pinned NxN corner: it is the Setback. `MakeSetBackTopology` neither certifies nor
    grows an open corner's setbacks.
  - `BuildSetBackFill`: an open G1 corner gets no fill (`OpenSetBackSprings`). On each face
    the spring curve is the parabola tangent to the two rails at their ends (P, Q; X their
    crossing by least squares), stored as its two quadratic halves at the base point
    `B = (P + 2 X + Q) / 4`. Each half is certified on its plane and tangent to its rail, or
    the corner is refused. An open G2 corner builds its fill without the containment
    check. `CreateSetBackPatches` makes no patches for an open G1 corner.
  - `sm_SbOneWeightPolynomial`: a spring side or a patch's iso-curve whose weights are all
    one weight is stored non-rational (`SmBSplineCurve::MakeNonRational`, ported as
    `BSplineCurve::make_non_rational`). This applies to every Setback under the flag.
  - The executive skips keep-features and runs `TrimAtCreases` when a corner is an open
    setback (`HasOpenSetBack`). `TrimAtCreases` imprints the open corners' spring curves on
    their faces with the sheet's boundary, and accepts exactly `3 n` free edges per open
    corner (the hole) instead of a closed solid.
- **Each round's side** (`ce748fa`): in `TrimAtCreases` a concave round's sheet lies in the
  void it fills, so its pieces inside the body are the ones deleted. A sheet face of no
  round (a corner's) between blends that add material and blends that take it off is refused
  (`SM_FILERR_CORNER_UNSUPPORTED`). In solidcore the side is read only with
  `fillet_blend_corner`; without it every round takes material off, as before.
- **NxN setback paths kept** (`dc757a6` not taken): the fork's dc757a6 deletes the convex
  and concave NxN corners' setback branches, so at `18e96f1` a setback asked at a corner the
  Setback does not build is ignored by the corner's topology, with no error. solidcore keeps
  those branches with `fillet_blend_corner` too (solidcore#2365): the Setback does not yet
  build every corner they build (three equal rounds at a box corner whose vertex is an
  `AddSetBackVertex`, a corner beside a curved face, a concave notch), and SolidCraft's
  setback and G2 corners set back through `AddSetBackVertex` and `SetSetBackDist` built
  there before this port. Deleting them refused seven of SolidCraft's rig cases that built,
  so the old result is kept by default, unchanged. A setback asked at a corner that builds
  none is refused in the kernel (`WR-697--addendum-2026-11-02-setback-asked-not-built-refused.md`).
- **Checked against the fork:** a driver built on the fork's SMLib and SM_API at `18e96f1`
  (`SmFilletExecutive` with `SetSetBackFill`, keep-features on) gives the same results as
  solidcore for the regression cases: the open box corner (sheet 9/27/19, 9 free edges, the
  six halves' control points), the open concave notch (12/36/25, 9 free edges), the
  mixed-convexity refusal, and the curved-edge corner (whose setback the fork ignores and solidcore
  builds on the kept NxN path).
- **Rust port (slice 2):** `blend_corner/setback.rs` (`classify_setback`,
  `make_setback_topology`, `build_setback_fill`, `open_setback_springs`,
  `one_weight_polynomial`, `create_setback_patches`, `FilletCorner::is_setback_open`),
  `nurbs/b_spline_curve_n_lib.rs` (`make_non_rational`), `fillet_executive/session/finish.rs`,
  `session/trim_at_creases.rs` (`has_open_setback`, the springs, the free-edge count, each
  round's side), `fillet_corner/topology.rs`, `topology/concave_nxn.rs` and
  `session/concave_nxn.rs` (the NxN setback paths, unchanged).
- **Tests (slice 2):** `crates/solidcore/tests/fork_smlib125_open_setbacks.rs`, from the
  fork's capi cases `setback_open_g1_box_corner_hole` and `setback_open_concave_notch_hole`
  (counts, free edges, P/X/Q of each spring) plus a mixed-convexity notch and a curved-edge
  corner, with the flag on and off; `crates/solidcore/tests/issue_2365_nxn_setback_corners.rs`
  (SolidCraft's seven set-back NxN corners, at main's values).
