#### WR-752. `SmPointGrid::SetUpMonotonic` omits the last turn of a closed row or column

- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmPointSet.cpp:2179`, `SmPointGrid::SetUpMonotonic`; row walk `:2210-2281`, column walk `:2285-2336` at v1.0.1 (`3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`).
- **What happens:** Each closed walk starts with the difference from its last point to its first, then visits the ordinary differences in order. It never compares the final ordinary difference against that closing difference. A genuine reversal there is accepted. Rotating the starting point of the same closed line changes the answer.
- **Trigger:** A closed line `(0,0,0), (0,1,0), (1,0,0), (2,0,0)`. Its ordinary differences are `(0,1,0), (1,-1,0), (1,0,0)` and its closing difference is `(-2,0,0)`. The only reversal is the omitted final comparison: 180 degrees, above the unchanged 170-degree criterion. The other three turns are 90, 135 and 45 degrees. Duplicate this line at z = 10 to make an eight-point grid with row size 4 and `bClosedRow = TRUE`; interleave each point with its duplicate at z = 10 to make row size 2 and `bClosedCol = TRUE`. The other grid direction is open and has only one step.
- **How found:** Issue [#2683](https://github.com/tangency-labs/solidcore/issues/2683), confirmed by a standalone probe of the pinned C++ library during [PR #2784](https://github.com/tangency-labs/solidcore/pull/2784).
- **Confirmed:** yes. `SmPointGrid::HasNonMonotonicPoints()` returns the following on the exact points above. `start` rotates the line before constructing either grid; all four starts describe the same closed line.

  | Grid direction | start 0 | start 1 | start 2 | start 3 | Correct result, every start |
  |---|---|---|---|---|---|
  | closed row | false | true | true | true | true |
  | closed column | false | true | true | true | true |

  Built against the precompiled reference `v1-3fd5b9548d19-726643fb17a52b7d`, with that revision's SMLib/SM_API headers, `-O2 -ffp-contract=off -fno-fast-math`, `NDEBUG`, `SM_GRAPHICS_CALLBACKS=1`, and the C++11 libstdc++ ABI. No C++ source changes.

The Rust regressions are the `issue_2683_*` tests in `surface/surface/assert_valid.rs`. This is separate from WR-737's criterion for whether a step carries a direction: completing a cycle must use the existing zero criterion. In the rounding-aware walk, repeated or rounding-only steps at the seam must also be passed over when comparing its first and last real directions.

Two existing generated `surface.surface_assert_valid` cases also reach the gap across a directionless seam: `gen0340` (seed `14642753455889217617`) and `gen1316` (seed `14642753455889218593`). Both are non-rational B-spline nets whose last U row repeats their first exactly: closed in U, open in V, with a zero closing difference in each column. The first and last **real** differences still reverse. This is where closing the cycle meets WR-737's rule of retaining the last direction across coincident points; the native absolute-zero walk also skips turns across such points.

Measured with solidcore's own `Vec3::angle_between` and `rad_to_deg` on the constructed nets (column indices start at zero):

| Case | Net U x V, degrees | Column | First real difference | Last real difference | Angle (degrees) |
|---|---|---:|---|---|---:|
| `gen0340` | 6 x 5, (2, 2) | 4 | `(0.002, 0, 0.0006517100637962953)` | `(-0.008000000000000002, 0, -0.003293533586377979)` | 175.67190164009531 |
| `gen1316` | 7 x 4, (3, 3) | 2 | `(0.2773744297824343, 0, -0.040546919335848614)` | `(-1.386872148912171, 0, 0.09756311033758217)` | 175.70733198415260 |
| `gen1316` | 7 x 4, (3, 3) | 3 | `(0.2773744297824343, 0, -0.01173078867850048)` | `(-1.386872148912171, 0, 0.14038671218489943)` | 176.64162349343900 |

Every listed real step is at least 0.002 long; none is rounding noise. Both cases emit no native report (`values.out.len = 3`), whereas the completed rounding-aware walk emits one `SmBSplineSurface` test #5, `Bad NonMonotonic CPts`, and returns false (`values.out.len = 157`). Confirmed in both the targeted unit run and the full 280-unit sweep against the same pinned C++ reference, at seeds 16. A separate reconstruction of each B-spline confirms `closed_u = true`, `closed_v = false`, one report of type 15002/test 5, and the angles above. Reproduce either native comparison with `--unit surface.surface_assert_valid --filter gen0340` or `--filter gen1316` (without `--baseline`).
