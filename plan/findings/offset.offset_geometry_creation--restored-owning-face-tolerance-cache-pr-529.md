# offset.offset_geometry_creation — restored owning-face tolerance cache (PR #529)

Follow-up to #522 on merged main `ceb85b2a4`: replacing a face's B-spline stand-in
with its concrete database record preserved the owner and NURBS but discarded the
`Surface::owner_zone_tol_3d` cache that `Brep::add_face_record` had initialized.
This is a metadata handoff omission, not a missing numerical offset algorithm.
Two extended native regressions reproduce it before the fix: the restored cache is
`None` instead of `Some(1e-5)`, and a face-tolerance query returns `Unported`.

After installation, the importer now calls `Brep::sync_face_surface_tolerance` to
restore the invariant from that face's actual record. Tests check all six concrete
families, different face tolerances in both arenas, and unowned surfaces (which
must stay unowned and have no cached face tolerance). No default tolerance is
invented and the fixed comparison tolerance is unchanged.

Native result: still 898 cases / 713 passed / 185 deferred / 0 failed, max relative
error zero, no absolute floors, and both executive reports byte-identical to #522.
There are no new parity passes: 53 former `SmFace::GetTolerance` deferrals reach the
cache's missing owning-face handle instead. The `topology.face_p2` total stays 123:
116 cache-face-handle stops and seven `SmFace::GetTolerance` stops. Prefix replays
locate the seven remaining stops in trim/extend paths (`gen0081`, `gen0361`,
`gen0479`, `gen0562`, `gen0688`, `gen0730`, `gen0732`); they remain explicitly
deferred, not claimed as solved. The wider topology/cache-face adapter is separate
work. The unit stays partial.
