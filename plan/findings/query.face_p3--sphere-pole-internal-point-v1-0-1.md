# query.face_p3: CalculateAnInternalPoint on sphere.smb face 0 returns the pole in the port, a nearby point in C++ 1.0.1 (2026-10-26)

- **Units:** `query.face_p3` (case `internal_sphere_f0`, `fixtures/query.face_p3/cases.json#97`) and `topology.face_p2` (generated cases `extra00744`, `extra00806` and `extra01154`, seed 4207804418; all op `internal` on `sphere.smb`). Both call `SmFace::CalculateAnInternalPoint` (port: `topology::face::p2::calculate_an_internal_point`).
- **Inputs:** `TestFiles/pt_TestFiles/sphere.smb`, face 0 (the generated cases select the same face).
- **Values:**
  - C++ (v1.0.1, 3fd5b95): `(-0.6590203814069528, -0.20256878381003973, 4.952237678536436)`.
  - Rust: `(0, 0, 5)`, the sphere's pole.
- **What differs:**
  - `CalculateAnInternalPoint` classifies 20 constant-V UV lines, then 20 constant-U ones, against the face. It evaluates the first FACE interval at 45% of its range.
  - In the port the first line (V = Vmin, a degenerate line through the pole) already gets a FACE interval. There, `SmCurveClassification::IntersectUVDiscontinuties`' boundary pass finds one solution on each of boundaries 1..5 and none on 6 and 7, classifies no crossing, and falls back to the midpoint test, which marks the mid SURFACE (trace gsphere2, 2026-10-26).
  - C++ evidently does not take its point from that line: its point lies off the pole, at z = 4.952.
- **Ruled out:**
  - The 1.0.1 natural-boundary slot fix in `IntersectUVDiscontinuties` (`intersect.curve_class_p8`): slots 0, 1 and 2 and the post-loop index are the same in both versions, and the newly filled slot 3 (the V-max line) has no solutions here.
  - The lazy-surface natural domain in `SmVertexuse::ComputeUVPoint`: `topology/vertexuse.rs` now uses `natural_uv_domain_with_nurb`, and the case still fails on the clean g5 build.
  - The 1.0.1 `ComputeUVSector` opposite-orientation count: its breakpoint never fired on this path.
  - `CalculateAnInternalPoint`, `sm_FaceClassifyUVLine` and `SmFace::CurveOnClassify`: unchanged in 1.0.1.
- **Rust unchanged by the pin:** a Rust-only digest of these cases on a main build and on the pin give the same pole `(0, 0, 5)`, so C++ moved in 1.0.1 and the port does not yet follow some 1.0.1 change on this path.
- **Status (open):** a call-sequence trace (2026-10-26) shows C++ calls `IntersectTSBound` on each UV line inside `CalculateAnInternalPoint` and tries a second line before classifying, while the port classifies the first line (V = Vmin) directly. Which 1.0.1 change causes that is not yet found; the case stays a known mismatch until it is.
- **Tolerance:** unchanged. The case is listed as a known mismatch with this finding, without `cpp_defect`, since nothing shows the C++ result is wrong.
- **PR:** the pin move to v1.0.1 (`pin/v1.0.1`).
