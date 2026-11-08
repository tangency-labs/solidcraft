# 2026-10-23: Newly reached spine closure exports (#613)

CI run 39983042271 reached `closure_inf_spine_doubled` and
`review_spine_doubled_m1_closure_True`, deferred on base main b35cd5ac7.
`Graph::walk` in the p10 runner enumerated region arena slots; native `GetRegions`
starts at the owning-list head, which `ResetInfiniteRegion` rotates. Exporting that
list fixes the apparent reordered geometry/topology (first reported at dbls[339]).
The resulting strict comparison also exposed nine missing UV-curve owner refs.
The edge closure query now binds ownerless generated trims to their actual storing
edge uses under the construction caller's names, completing native `SetUVTrimCurve`
ownership (SmEdgeuse.cpp:4552-4579). Existing owner refs and the query status remain
unchanged. All 90 spine_doubled fixtures match C++ bit for bit, including the two CI
failures; the shared p6 exporter still has 588 passes, no failures and 7 deferrals.
The full local p10 rerun was superseded before completion; no full-suite pass is
claimed. No fixture, comparison gate or tolerance was changed.
