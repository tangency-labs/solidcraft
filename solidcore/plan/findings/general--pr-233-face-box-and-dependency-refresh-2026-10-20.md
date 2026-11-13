# PR #233 — face box and dependency refresh (2026-10-20)
Merged canonical main `e5f3e441`, keeping both issue append sets and main's
full p7 notes plus this PR's API extension. `primitive_face_box` now resolves
the caller's B-rep and selected face and invokes `query.face_p1` with the native
`tight=false` default. A Rust regression selects a nonzero face index in the
correct owner, checks the normal box, and rejects a foreign B-rep id. It does
not claim a completed rotational-sweep comparison.
Integrated the updated #222 (`d6b5f0c5`) and #213 (`f392e69a`) service contracts,
including caller notification state, HCR ownership, real p5 edits and p1 API
back-edges. The shared stitch callback retains owner-qualified map updates.
Main's expanded edge-blend API receives both edge uses from the primitive's
one B-rep (`b_2=None`), and HCR global visibility receives that owner's mutable
Kernel. Current main's real pre-edit notification now completes: the general
sweep adapter regression now reaches `query.brep::CalculateBoundingBox`, while
the empty linear-cap stitch completes successfully. Both check editing and
boolean flag restoration. Native successful stitch
and primitive topology construction remain uncertified.
