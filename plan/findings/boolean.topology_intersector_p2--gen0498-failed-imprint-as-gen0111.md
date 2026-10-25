# boolean.topology_intersector_p2: gen0498 failed imprint, as gen0111

Unit `boolean.topology_intersector_p2`, case `gen0498` (`Endcap.smb` and `toruswvy.smb`
rebuilt, two relates, then `IntersectInsertRelate`, approx 1e-6, angle 0.349). The
same disagreement as `gen0111` above (`values.state[10]`: the port 1006, the shim -2).
It deferred at `FaceIntersect`'s read-only trimmed-cache lookup, and later in main also
at `Point3DClassify` (wired by PR #816). With both gone, `ImprintIntersections` fails
with `SM_ERR_NULL_POINTER` on both sides (the C++ status was checked with a temporary shim
print, not kept; the port's comes from `topology::brep::p5`). The port stops there
(UB-242 fix); the verbatim C++ copy runs `SetRelationships` (1000) and its teardown
reaches UB-175. Listed in `known_mismatches.toml`.
