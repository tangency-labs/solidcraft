# IsInnerShell on the B-rep with the kept trimmed caches (PR #694 merged with #697, 2026-10-24)

- **Result.** With `IsInnerShell` and `GetPointsInFace` on the caller's B-rep (#694) and
  the face surfaces' kept trimmed caches with `IntersectTSBound`'s cached branch (#697),
  topology.brep_p6's 7 region-nesting cases (`make_face.json#89..95`) match C++ bit for
  bit, the Mark2 counter included; their known-mismatch entries are removed and the unit
  is green (595/595). topology.topology_traverser stays green (909/909: the 14
  `IsInnerShell` rays solve on the face's kept cache, the caller-held cache now only
  stands for a surface without face), topology.shell 911/911.
- **Still differing (kept as known mismatches).** primitive.primitive_creation_p2
  `skin_concentric_loops` (record[7] cpp 1, rust 2) and sweep.api_surfaces
  `skin_primitive_annular` (faces[1] cpp 1, rust 2): the annular cap between the two
  concentric loops comes out as two faces in Rust, one face with a hole in C++. They no
  longer defer at `TrimSrfCache::build_tree_with_face` (#697's GlobalLineIntersect fix);
  the remaining difference is in how the inner loop is nested in the outer one and has
  not been narrowed to a call.
