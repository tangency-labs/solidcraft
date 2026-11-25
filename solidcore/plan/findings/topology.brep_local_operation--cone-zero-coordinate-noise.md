# topology.brep_local_operation: zero coordinates of Solids/cone.smb differ by solver noise (2026-10-28)

Unit: `topology.brep_local_operation` (`SmBrep::LocalOperation`, PR #1427).

Inputs: `cone.smb`, the surface of face 0 replaced by a plane through the surface point at
the middle of the face's UV domain moved along the surface normal (`cone_face0_out`: offset
1.1436041365678737, no tilt; `cone_face0_tilt`: offset -0.77, tilt (-0.257, 0.105, -0.263)),
in `fixtures/topology.brep_local_operation/cone.json`.

Result: status, topology, tolerances and every other sample agree (`max_rel_err` 0 for
`cone_face0_same`). A sampled edge-curve coordinate whose value is 0 up to the solver's noise
differs in absolute terms by less than `1e-10` (model size about 10, tolerance 5e-6), so the
`1e-9` relative rule fails: `cone_face0_out` C++ `1.4210854715202004e-14`, Rust
`-7.857181572035188e-11`; `cone_face0_tilt` C++ `-7.276169484178809e-16`, Rust
`-8.103788770344583e-16`.

Suspected cause: the plane/cone intersection curve of the extended offset is fitted by an
iteration whose last digits depend on evaluation order; not isolated. Nothing is loosened:
the cases stay known mismatches rather than getting an absolute floor.
