# intersect.curve_class_p3 (`SmCurveClassification` part 3: `FindInterval`, `FindPointClass`, `IsCoincident`, `InsertPointClass`, `RemovePointClass`, `MergePointClass`, `CleanupAndValidate`, `sm_ChangeEdgePointClassToNearVertex`, `sm_FixProblemsBySqueezing`, 2026-10-16; partial, no disagreements)

- **Why `UNPORTED(topology.edge)` / `UNPORTED(topology.vertex)` remain** (`SmEdge::GetVertex`,
  `GetInterval`, `GetCurve`, `SmVertex::GetPoint`): the classification refers to objects through
  the opaque handle `O: ClassObject`, as in `intersect::point_class`. A vertex or edge handle
  carries no `Brep`, so the port cannot resolve it to an arena id; the stubs are not waiting on
  the units' ports but on handle-to-B-rep resolution. The shim rejects the scripts that would
  walk a bare stand-in edge (undefined in C++: no edgeuse; UB-109/UB-110 cover the other
  undefined reads).
- **Other stubs** are the maintainer's open PRs: `intersect.curve_class_p2` (PR #85: `InsertAt`,
  `EditIntervalMin/Max`, `ConnectIntervals`, `GetClassifyCrvLoc`) and `intersect.curve_class_p7`
  (PR #65: `GetPointClassByIndex`, `GetPointClassMateByIndex`, `AreCoincidentMaybe`). Cases that
  reach them are `deferred`.
- **`sm_FixProblemsBySqueezing`** ends with a tail call; the port restarts a loop at the same
  point (visit order unchanged), bounded by interval count + 1.
