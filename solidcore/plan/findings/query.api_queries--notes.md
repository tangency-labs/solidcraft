# query.api_queries

- **Native missing-curve crashes (2026-10-16, PR #125).** Clearing an edge's
  curve with `SetCurve(nullptr, TRUE, FALSE, FALSE)` leaves topology that several
  public queries dereference. The review identified closest B-rep queries,
  B-rep relationship/distance, edge/face picking and edge closest point. An
  additional 220 isolated native probes (line/B-spline wires, boxes, a face with
  vertex loops and a holed face; both box modes) confirmed crashes in face closest
  point on boxes and in interior-point/classification/closest-point paths on the
  latter faces. Extending the probes to 308 inputs also found tight B-rep/face
  bounding-box crashes on the curved quadratic patch. These are native SIGSEGVs,
  not Rust/C++ numerical disagreements.
  The FFI rejects those missing-curve combinations before construction; native
  null-input checks and the defined missing-curve edge queries remain testable.
  After the guards were expanded, 396 isolated probes over nine construction kinds
  and both box modes completed without a crash or timeout (192 rejected by guards,
  204 native calls completed).
- **Solution point extraction.** The query adapter follows `SmSolution::GetPoint`'s
  zero parameter advance for a preceding Surface (Face advances by two), and its
  ignored ordinary evaluation failures leave the point undefined. Unported
  geometry still propagates. Two core regression tests cover both behaviors.
- **Null-pointer coverage.** Typed Rust APIs cannot receive native null pointers.
  The 25 native null contracts are FFI tests, not differential passes with a
  synthesized Rust status.
