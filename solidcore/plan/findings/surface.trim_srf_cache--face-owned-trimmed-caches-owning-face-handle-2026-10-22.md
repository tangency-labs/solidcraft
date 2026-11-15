# surface.trim_srf_cache — face-owned trimmed caches (owning-face handle, 2026-10-22)

`SmSurface::GetFace()` of a trimmed surface cache was the largest deferral bucket
(`SmFace handle of a surface's face`, 1971 CI cases). The cache now reaches its face
through `surface::trim_srf_cache::CacheFace` (implemented over the B-rep by
`query::face::p2::BrepCacheFace`): `BuildTree`'s face walk (`CreateUVTrimCurves`,
loop uses, edge and vertex uses, `ComputeUVPoint`, vertex tolerances), and
`MarkFaceContainment` now dispatches to `SmTrimSrfCache::PointClassify` (the former
DISPATCH GAP). Wired so far: `SmFace::Point3DClassify` (`query.face_p1`).

Not wired, with the measured reason: `SmFace::GlobalLineIntersect`
(`topology::face::p4`, 1118 `topology.face_p4` cases). Building the face's trimmed
cache lazily for the surface's line solve (patch kept on branch
`surface/face-line-intersect-wip` of the fork) turns 993 of them into passes, but
19 fail, all lines through a singular point: `cone.smb` along the axis through the
apex (`p=(0,0,0)`, `v=(0,0,±1)`: C++ solutions `u=15.307…, v=0` then `u=0, v=0`, Rust
`u≈3.9e-33, v≈3.9e-31` then `u=15.307…`; a third solution has Rust `v=5.9e-31`
against C++ `0`) and `surf_sphere150.smb` along the pole axis (`v=(1,0,0)`: C++ four
solutions, Rust two, one per pole; with `bFireRay` the first `u` is C++ `0`, Rust
`11.5099…`). The same Rust solve over an untrimmed cache gives the same
counts and tiny nonzero `v`, so the difference sits in the LSI leaf solves at a
singular point over the trimmed tree, not in the face reads; suspected: the trimmed
tree's subdivision around the apex/pole vertex uses (`ImplantVertexuse`, first
exercised with a real face here) or `sm_IsPointValid`, which upstream answers with
`SmTrimSrfCache::PointTest` (a face classification) and the port with the base
domain test. Cases (`topology.face_p4`): `cone.smb_0_f1_op19`, `cone.smb_1_f2_op19`,
`cone.smb_f0_tol4867..4869`, `cone.smb_f1_tol4885..4887`, `cone.smb_f2_tol4903..4905`,
`surf_sphere150.smb_0_f1_op17`, `surf_sphere150.smb_1_f2_op17`,
`surf_sphere150.smb_f0_tol5302/5304`, `_f1_tol5320/5322`, `_f2_tol5338/5340`.
