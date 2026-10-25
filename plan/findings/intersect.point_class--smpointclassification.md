# intersect.point_class (`SmPointClassification`)

- **Review fixes (PR #21).** The constructor takes an `SmZoneTol3d`, already converted, and
  stores it as is (converting again turned `-SM_USE_DEFAULT` into the system tolerance).
  `MergeIntoObject` runs `SmVertex::SetTolerance` in the reference build
  (`SM_USE_OLDTOL`); it is an `UNPORTED(topology.vertex)` stub. Topology kinds without
  their own `GetTolerance` (region, faceuse, loop, loopuse, vertexuse) use
  `SmTopology::GetTolerance` (`SM_ZONE_TOL_3D`), and a faceuse `SmTopology::IsConnectedTo`
  (`FALSE`); loop, loopuse and vertexuse do override `IsConnectedTo` and stay stubs. The
  shim now builds real `SmEllipse` / `SmCircle` curves, `SetClassObject(class, NULL)` is
  accepted for every class, and loads may read up to the last solution parameter.

- **`SM_PC_POINT` stores an `SmPoint3d*` cast to `SmObject*`.** Upstream then calls virtual
  `SmObject` members on it in `ComputePointParameters` (`SmTol::GetZoneTol3d(m_pObject)`),
  `GetGap3d` with a surface and `SmTol::GetObjZoneTol3d` / `GetXSectTol3d` (the same call),
  `IsConnectedTo` and `MergeIntoObject` (`GetType`): undefined behaviour. The port treats
  a point like any other non-topology object (system zone tolerance, not connected, not
  mergeable); the shim and the runner never send those ops for a point classification.
- **`GetGap3d(pSurface)` returns `SM_ERR` as the gap value** (1001.0) when the object
  point has no single drop point on the surface; the port does the same. Callers must not
  read it as a distance.
- **Release-build behaviour.** The reference build has no `SM_DEBUG_CODE`, so
  `SetTParam`'s `bSnapToIvl` does nothing and `SetClassObject` accepts any object for any
  class. `FindObjPoint3d`'s `SM_CAST_PTR` still checks the kind, so a mismatched class
  and object gives `SM_ERR_NULL_POINTER` (ported as such). Scripts only pair a class with
  an object of its kind.
- **Stand-in objects.** The shim builds vertices, edges and faces without a B-rep and
  `SmObject` stand-ins for volumes, regions, edgeuses and "other" objects. Upstream
  dereferences a stand-in in `FindObjPoint3d` / `GetGap3d` (volume, edgeuse),
  `ComputePointParameters`, `GetBrep` and `GetCurveObject` (edgeuse) and `MergeIntoObject`
  (region, face, edge), so those ops are rejected at run time on both sides and dropped by
  the generator. A stand-in region's zone tolerance is the system one upstream (it does
  not answer `IsKindOf(SmTopology_TYPE)`); the port asks `topology.shell` (deferred).
- **Deferred (356 of 2479 cases):** the surface point solve of `ComputePointParameters`
  and `GetGap3d` (`surface.surface_p4` 256; `SmSurface::EvaluateNormal`, which follows
  it, was wired by `surface.surface_p7`, PR #24) and every topology member (`topology.vertex` 41,
  `topology.edge` 27, `topology.face_p2` 21, `topology.face_p4` 5, `topology.shell` 3,
  `topology.edgeuse_p1` 3). The volume branch (`SmVolume::GlobalPointSolve` /
  `EvaluatePoint`, `surface.volume_p2`) is wired (`Volume::global_point_solve` with
  compounding, `Volume::evaluate_point`) but not exercised: the shim has no real volume
  to classify to. A found answer with an empty solution array (upstream reads
  `sSolutionArray[0]` past the end) is `SM_ERR`.
- **`SmSpaceBend` / `SmSpaceUnbend` type queries** are compared through the static
  `GetClassType` / `GetClassTypeString` (an instance needs the unported bend units); the
  `IsKindOf` chains are unit-tested only.
