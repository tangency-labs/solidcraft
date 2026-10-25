# heal.heal_data_p1: retain seam-query gap-cache writes (PR #563)

After integrating main `8c1eaa572`, CI run 39920844387 exposed two previously
deferred healer cases: gen0125 (seed 8971097847862315981, sphere radius
1.1017911871514507) and gen0164 (seed 8971097847862316020, radius
38.77897466460957), both with `init_gaps=false`. Their maximum vertex/face gap
references agree, but C++ reports raw cached length `-1.23456e-61`
(`-SM_UNDEF_DOUBLE`) while Rust retained `0.0`, at `values.out[139]` and `[69]`.
The first divergence is immediately after stage two: step 5 of gen0125 and
step 12 (after retargeting) of gen0164.

`SmVertexuse::CalcVertexFaceGap` (`SmVertexuse.cpp:1784-1836`) first resets all
same-vertex, same-face sibling caches, then computes only the requested use.
The healer's stored maximum-gap reference can therefore point at a cache that a
later seam query invalidates. Rust already implements that invalidation, but
`CurveOnClassify` performed it on a discarded B-rep snapshot.

The mutable healer now carries explicit query state through both seam directions
and applies only the snapshot's vertex/face and edge/face gap-cache values, even
when a query returns an error. It never resets an unqueried cache or replaces
the B-rep with a geometry clone. The read-only query APIs still discard their
cache writes; UV trim, geometry and topology edits remain outside this copy-back.
`HealData::cache_face_props_stage2` consequently takes `&mut Brep`.

Linux ARM64 native prefix replays now agree bit-for-bit for both complete scripts
(3183 and 2905 values) and all 25 prefixes (including the empty scripts). The exact
inputs are retained as `fixtures/heal.heal_data_p1/seam_query_gap_cache.json`.
Three Rust regressions cover sibling invalidation after classification failure,
resets before projection failure, and the untouched-cache path. All 2049 kernel
tests, fmt, workspace Clippy and the wasm check pass. No comparison tolerances,
existing cases, deferrals, native hang guards or known-mismatch entries changed.
