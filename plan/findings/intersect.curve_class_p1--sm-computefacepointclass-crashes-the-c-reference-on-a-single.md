# intersect.curve_class_p1: sm_ComputeFacePointClass crashes the C++ reference on a single-point UV domain

Unit `intersect.curve_class_p1`, op 50 (`sm_ComputeFacePointClass`) on the one-face
B-rep of a `heal.face_props` face with `domain = [0, 0, 1e-300, 1e-300]` (face
tolerance 1e-6, plane, three edge loops, any point). The shim's `SmFace::GetUVDomain()`
is then the single point (0,0)-(0,0) (the 1e-300 size is lost when the face is built),
and the C++ run ends in a segmentation fault; no Rust comparison is reached. Domains
of size 1e-9, 1e-14 and 1e-100 run and agree. Cause inside the upstream point solve
not located (no debugger on the box). The shim rejects such a face (`SC_ERR_ARG`)
for this op, so the generator drops those scripts. The guard also rejects a domain that is zero on one axis
only, which no generator produces and which was not tried (it is a precaution, not a measured crash).
Hypothesis for a later agent with a debugger: `smsurf_GetSurfaceCache` returns NULL when the
trimmed-surface-cache build fails on the point domain, and `SmCurveClass.cpp:3956-3963` then
dereferences `pSC->m_bPointTestEnabled` unchecked (the Rust port models that branch as
`SM_ERR_NULL_POINTER`). Not confirmed.
