#### UB-126. `SmCurveClassification::IntersectVertices` dereferences an absent surface owner
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmCurveClass.cpp:14692`, `pOwner->IsKindOf(SmFace_TYPE)`.
- **What happens:** when the classification has no face, the method dereferences `crSurface.GetOwner()` without checking for null, even for an empty vertex list. Its large-gap branch also dereferences the vertexuse returned by `GetVertexuseOfFace` without checking it.
- **Trigger:** a face-less classification and an unowned surface; or large-gap processing of a vertex without a vertexuse on the selected face.
- **How found:** source review and FFI guard design during `intersect.curve_class_p5` (PR #141).
- **Confirmed:** no native crash executed. The oracle supplies a real face owner and excludes nonempty large-gap tests of its standalone vertices.
- **Severity:** crash / undefined behaviour.
- **Rust port:** continues with no face when the surface has no owner, instead of dereferencing a null owner. Unresolved vertex/face callback data returns `InvalidInput` instead of dereferencing a missing vertexuse; a missing face skips the gap lookup and retains the default intersection tolerance.
