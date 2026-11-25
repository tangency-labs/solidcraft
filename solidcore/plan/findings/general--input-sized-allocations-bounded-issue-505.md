# Input-sized allocations bounded (issue #505)

The 17 memory-dependent fuzz inputs of #505 (the 23 above minus the six #502 already
refuses) now fail the same way on every machine: the allocation sites they reach
(`geom::grid::Grid::new`, `geom::local_solve1d::IterQueue1d::try_new`,
`geom::solution_array::SolutionArray::reserve_to`, `nurbs::fit_arrays::alloc`/`alloc2`,
`nurbs::srf_basic::size_arrays`, `nurbs::nurbs_crv::filled`,
`primitive::api_curves::create_regular_polygon`) refuse more than
`solidcore::limits::MAX_ALLOC_BYTES` (1 GiB) up front, with the error they already
returned for a failed reservation. Upstream has no bound of its own on these arguments
(degree, count, index, voxel or side count; `NL_MAXDEG` is not enforced by the knot
vector routines and the FFI accepts knot degrees up to 64), so the cap is a documented
deviation far above anything upstream or the fixtures build. Each reduced input is a
`crates/difftest/tests/fuzz_corpus/` case (`issue: #505`); replayed with and without a
4 GiB address-space limit, all 17 return in under 0.02 s and none maps more than 1 GiB.
