# query.poly: legacy opaque-owner IsDegenerate cases are rejected, not deferred (2026-10-24)

`gen0378` and `gen0496` (and 22 more at `--seeds 6000`) call `SmPolyEdge::IsDegenerate`
on an edge whose poly face has a label for an original face while `OKBackPtrs` holds.
Upstream then calls `SmFace::GetSurface` on that original face (whenever the back pointers hold; it maps UV
points); the generator's label is not an `SmFace`, so the call is undefined in C++ (the
shim never made it: it wrote `-9`). They were counted as deferred (`topology.face_p2`)
although no port of `SmFace::GetSurface` could make them comparable. `run_cpp` now
returns `Output::rejected` for a `-9` in the `results` of `degenerate` or
`owner_degenerate` (AGENTS.md section 4, step 5.6), the unit has 0 deferred and is
green. Real original faces stay compared by the `owner_degenerate` cases
(`fixtures/query.poly/real_owners.json`, 16 cases). The library is unchanged: the
key-only `PolyBrep::edge_is_degenerate` still returns `Unported`
(`SmFace::GetSurface`) for an unresolved label. Cost if the ruling is wrong: those
cases would be real comparisons that nobody can run without a real owner face. The comment in
`harness/cpp/shim/query_poly.cpp` op_degen ("the Rust side defers this") is outdated and left
as is so the prebuilt C++ library key does not change.
