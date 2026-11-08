#### WR-508. `SmBrep::Transform` takes a scaled body's model size from its vertices alone, so a body with no vertices gets a zero tolerance
- **Severity:** Wrong result (a scaled body with no vertices is left with a zero tolerance)
- **Area:** Topology
- **Location:** `source/SMLib/src/SmBrep.cpp`, `SmBrep::Transform`. `sVertBBox` is filled as each vertex is transformed, and both `SetModelSizeEstimate` calls (brep and context) take `sVertBBox.GetSize().Length()`.
- **What happens:** a lone torus, which the port's `create_torus` gives a single vertex, so no vertex spans a size: `sVertBBox` is one point of size zero, so the scaled model size, the body tolerance and the context's come out zero. After a scale of 2 the body keeps a zero tolerance, where a torus made at the scaled size gets about 1e-5.
- **Trigger:** a lone torus (`R 6, r 2`) scaled by 2 or by 0.5 through `SmBrep::Transform` with `bScaleTol` TRUE.
- **How found:** SolidCraft fork (tangency-labs/smlib#50), reported in tangency-labs/solidcore#1521.
- **Confirmed:** no. The pinned C++ is not built on this machine, so the fork's commit is the evidence. The port reproduces the zero with the opt-in off: `transform_sizes_a_body_without_vertices_wr508` (`crates/solidcore/src/topology/brep/p5.rs`).
- **Fix (fork):** tangency-labs/smlib#50 drops `sVertBBox` and, inside the scale-tolerance block, takes `SmExtent3d sModelBBox; CalculateBoundingBox(sModelBBox, FALSE, FALSE, FALSE)` and uses its size's length in both `SetModelSizeEstimate` calls.
- **Rust port:** opt-in `Kernel::fixes.transform_model_size_from_body_box` (default `false`, upstream's vertex box) in `transform_in` (`crates/solidcore/src/topology/brep/p5.rs`), calling `query::brep::calculate_bounding_box` with `approx`, `tight` and `expand_by_zone_tol_3d` all FALSE. Test: `transform_sizes_a_body_without_vertices_wr508`, which asserts a positive body tolerance no smaller than a torus made at the scaled size with the flag on, and the zero with it off.
