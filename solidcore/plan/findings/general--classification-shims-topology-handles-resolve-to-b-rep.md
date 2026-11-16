# Classification shims: topology handles resolve to B-rep records (2026-10-20; harness gap, no port disagreement)

- **Gap.** The shims of `intersect.curve_class_p1/p2/p3/p7` and `intersect.point_class` build
  their vertex, edge and face objects as default-constructed `SmVertex` / `SmEdge` / `SmFace`
  with only `m_pCurve` / `m_pSurface` set, never joined to a B-rep; `intersect.curve_class_p6`
  uses the first face, edge, vertex and region of an `SmApiCreateBox` B-rep. Their Rust
  adapters answered none of the `ClassGeometry` topology hooks, so every topology member
  reached through them deferred as `topology.edge` / `topology.vertex` / `topology.face_p2`.
- **Fix (harness).** `BareTopology` (`crates/difftest/src/units/intersect_curve_class_p1.rs`)
  holds those objects as records of one `Brep` in exactly the state the shims give them
  (`Vertex::default()`, an edge with the curve, `Extent1d::new()` interval and tolerance 0,
  `Face::new` with the surface; an edge follows edits of the pool curve it shares). The p6
  runner imports the same box through `heal.stitch_p1`'s export and `topology.brep_p1`'s
  importer. The C++ shims are unchanged: every op they run needs only the bare object (the
  shims reject walks over the missing topology), so the compared values did not change.
- **Library.** The `ClassGeometry` wiring this PR first carried (`compute_point_parameters`
  edge / face, `interval_min_uv` / `interval_max_uv`) landed on main with #347 /
  wire/point-class-topology, in the same shape; main's version is kept.
- **Combined with main's adapter (2026-10-21, main 215f690e1).** #347 gives some generated
  scripts a real one-face B-rep (`PoolTopology`); when a case has it, the hooks answer from it
  only (main's behaviour, stand-ins stay deferred, so handle lookups never mix two B-reps);
  otherwise the stand-ins answer from `BareTopology`. Gains over main: curve_class_p1
  2054 -> 2070, p2 1495 -> 1627, p7 1327 -> 1474, point_class 2411 -> 2466, all
  `max_rel_err 0`.
