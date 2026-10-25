# offset.offset_geometry_creation — exact analytic input records (PR #522)

The creator adapter now restores revolution, extrusion, cone, cylinder, sphere and
torus face surfaces from native binary `WriteToDB` input records (version 33), using
the existing polymorphic Rust reader. It checks face-list alignment, exact byte
framing, complete record consumption, concrete type and preserved owner identity.
Native import tests also check bit-identical underlying NURBS in both input arenas.
Only input geometry is imported; native operation outputs are not replayed.

Fixture names are not type evidence: `Solids/cylinder.smb` stores an `SmCone`, and
`torus.smb` stores an `SmSurfOfRevolution`. Their actual kinds stay intact. A narrow
unit-local fixture block constructs real `SmCylinder` and `SmTorus` faces to cover
those concrete readers and virtual offset branches as well.

On authoritative main `161925132`, the baseline is 874 cases / 631 passed / 243
deferred / 0 failed. The follow-up has 898 cases / 713 passed / 185 deferred / 0 failed,
with max relative error zero and no absolute floors. All 631 old passes stay passes;
58 old deferrals and 24 new signed/zero-distance cases now compare successfully.
The 80 former `harness:analytic-geometry-export` deferrals become 58 passes, five
`topology.face_p2` deferrals, one `intersect.surface_drop_lib_p3` deferral, and 16
`upstream:HANG-26` guards. Both executive reports remain byte-identical to baseline.

HANG-26 is not fixed: the native non-local offset of an exactly
`SmSurfOfRevolution` still stops before its potentially nonreturning self-intersection
when `skip_self_intersection` is false. Both sides retain that predicate; it is now
reported as the upstream limitation rather than a fictitious missing surface import.
HANG-27 and the seeded extension-cache policy are unchanged. This remains a partial
creator port, not complete constructive offset/shell or returned-B-rep certification.
- **Follow-up merge through `ceb85b2a`.** #522 (offset analytic import) and #523 (trim
  snap round-off) touch four kernel files. The new `Surface::trim_with_domain_with`
  dispatches like `trim_with_domain`, and its wrapper arms call the existing
  `trim_with_domain`, so construction still bounds the nesting at
  `MAX_GEOMETRY_NESTING`. `face::p1::shrink_geometry_with` calls it once. No new
  self-call, cycle or nested-geometry install. fmt, clippy, 1890 solidcore tests,
  39 difftest tests and the wasm check pass.
