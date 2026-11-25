#### WR-780. `CreateSweepAlongPlanarPath` names the side of a mitre by a point 0.1 from the corner, past the end of the cutting line for a small profile
- **Severity:** Wrong result
- **Area:** Primitive creation, sweeps
- **Location:** `source/SMLib/src/SmPrimitiveCreation.cpp:6087` and `:6088` (the reference points), in the eight-argument `SmPrimitiveCreation::CreateSweepAlongPlanarPath` (def. `:5775`).
- **What happens:** At a corner of the path the two swept pieces are cut along a line through the corner, and each piece loses the side that holds a reference point (`ProjectAndTrim`, `SM_TT_DELETE_POINT`). The cutting line reaches `dExtend` from the corner each way, where `dExtend` is the largest dimension of the profile times 2 sqrt 2. The reference points are a fixed 0.1 from the corner along each leg, whatever the size of the profile. `ProjectAndTrim` uses the reference point twice: `sm_FindSideOfCurve` finds its side of the cutting line, and a face whose vertices and edge mid-points give no side is judged at its point nearest a ray through the reference point. Seen along the path's normal, the foot of the reference point on the cutting line is 0.1 cos(a) from the corner, where a is the angle between the leg and the cutting line (45 degrees at a right-angle corner, so 0.0707). When `dExtend` is shorter than that (a circle of radius below 0.0125), the foot lies past the end of the cutting line, `sm_FindSideOfCurve` fails, `CreateSweepAlongPlanarPath` ignores the status, and the piece is united untrimmed. Up to twice that size the reference point has a side, but the trim is still wrong or untidy (measured below, not traced further).
- **Trigger:** a circle of radius 1e-3 swept along the lines (0,0,0)-(0,0,1e-2) and (0,0,1e-2)-(1e-2,0,1e-2), tolerance 1e-7, both ends capped (`primitive.primitive_creation_p2` `gen0092` and `gen0189`). `dExtend` is 5.66e-3. The C++ returns status 1001 from the union. The port, whose sweep surfaces are exact, returned success with a body that fails `check_brep` level 2 (vertices 1.8284271e-3 off their faces).
- **How found:** tracing tangency-labs/solidcraft#13687 on the port, which followed the C++ line for line here.
- **Confirmed:** by reading the C++ (the two lines above) and on the port. Measured on the port before and after the change, for a right-angle elbow of two equal legs, both ends capped: the number of faces, and `check_brep` level 2. The correct body has 4 faces (two caps and the wall of each leg).

  | radius | leg | tolerance | before | now |
  |---|---|---|---|---|
  | 1e-3 | 1e-2 | 1e-7 | not valid | 4 faces, valid |
  | 1e-3 | 1 | 1e-7 | the sweep fails | 4 faces, valid |
  | 1e-2 | 1e-1 | 1e-5 | not valid | 4 faces, valid |
  | 1e-2 | 1e-1 | 1e-3 | not valid | valid |
  | 0.0124 and 0.0126 | 0.5 and 0.05 | 1e-7 | 7 or 8 faces, not valid | 4 faces, valid |
  | 0.017, 0.021, 0.024 | 0.5 | 1e-7 | 12 faces, valid | 4 faces, valid |
  | 0.0185 | 0.5 | 1e-7 | 7 faces, not valid | 4 faces, valid |
  | 0.026, 0.034, 0.036, 0.05, 0.2 | 0.5 | 1e-7 | 4 faces, valid | 4 faces, valid |
  | 0.03 and 0.1 | 1 | 1e-7 | valid, volume right | the same volume, bit for bit |

  The volume of the repaired bodies agrees with pi r^2 times the summed leg lengths to 2.5e-6 of it or better (radius 1e-2: 1.4e-6; radius 1e-3: 2.5e-6 and 2.1e-6).

  Solidcore now steps the smaller of 0.1 and half of `dExtend` along each leg. Below a `dExtend` of 0.2 the reference points, the cutting line and the pieces then keep one shape at every size, the shape that is trimmed correctly at radius 0.036. Test `elbow_of_two_lines_is_a_valid_body_of_the_closed_form_volume` (`crates/solidcore/tests/issue_2794_planar_path_mitred_elbow.rs`). All 283 golden units give the same results as before, except `gen0092` and `gen0189`, which now return a body.
- **Not covered:** radius 1e-3 with tolerance 1e-3 (a tolerance as large as the profile) still returns a body with 4 failed checks, before and after. The mitre of a closed path uses fixed lengths of its own (a cutting line of 50 each way, reference points at 10 or 30); they are not changed.
