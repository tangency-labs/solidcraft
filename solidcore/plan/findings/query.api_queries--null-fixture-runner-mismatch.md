# query.api_queries: null fixture runner mismatch (2026-10-31)

Unit: `query.api_queries`. Input: nightly `fuzz00024`, based on `gen0607`,
mutation `/data/16 = 1.0`, op 25 (`SmApiVertexGetPoint`), kind 5.
The exact input is retained in `crates/difftest/tests/fuzz_corpus/query.api_queries.json`.

Before the runner fix, the C++ shim clears the selected vertex when the null flag
is set (`harness/cpp/shim/query_api_queries.cpp`, null branch before the query
switch). `SmApiVertexGetPoint` explicitly returns `SM_ERR_INVALID_INPUT` (1007)
for that pointer (`source/SM_API/src/SmApiQueries.cpp:1549`). The Rust runner
instead returns the harness error `null pointers have no typed Rust equivalent;
use the FFI contract tests`. A local replay fails with that Rust-side error.
This is a comparison-domain mismatch, not a kernel disagreement or C++ defect.

Following #502, #1358 and #1861's shared harness-domain convention, both runners
now apply the existing null-fixture rejection before dispatch. Native null
contracts remain exercised by the FFI's
`native_null_pointer_contract_is_checked_outside_rust_difftest` test. The fuzz
corpus regression checks that both runners refuse this input with the same error;
it does not claim a successful Rust null-pointer API call. No kernel behavior,
tolerance, native contract or unit status changes. PR: pending (offline worktree).
