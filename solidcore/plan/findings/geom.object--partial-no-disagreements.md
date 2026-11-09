# geom.object (partial; no disagreements)

624 default cases (24 fixture, 600 generated) — 593 pass bit for bit, 31 deferred (every
non-empty `SmShape`: topology.brep_p1, topology.face_p2, topology.edge, topology.vertex).
Upstream behaviour worth knowing:

- **`SmApiReadBrepFromFile` crashes on `pt_TestFiles/SmallBox.smb` read as ASCII**
  (null `SmShellData` in `SmTopologyData::ReadFromDB`, `SmBrepData.cpp:4307`; depends on
  heap layout, so it crashes in some runs only). The shape fixtures therefore build their
  B-reps with `SmApiCreateBox`; the shim still supports file sources.
- **Empty `SmShape`.** With no topology objects the constructor reports `SE_MSG` and runs
  `new (*cpContext) SmTree(...)` through a NULL context reference (works in practice);
  all three trees cover `(0,0,0)-(1,1,1)`. The port builds the same shape without the
  message.
- **"Been through new" handshake.** `SmObject()` leaves `m_cpContext` uninitialised when
  the thread-local `s_sBeenThroughNew` flag is set by an `operator new(SmContext&)` whose
  constructor never ran; the port has no per-object context, so nothing to reproduce.
- `SmTemporaryChangeValue::UseOrigValue` / `UseTempValue` on a default-constructed,
  never-`Set` guard dereference NULL; the port does nothing.
