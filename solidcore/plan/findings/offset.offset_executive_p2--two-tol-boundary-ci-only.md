# offset.offset_executive_p2: shell_boundary_negative_exactly_two_tol CI-only disagreement, not caused by the narrowed UB-242 stop (2026-10-25)

PR #884's CI difftest-sweep (head `f23c532dc`) reported `values.result.geometry[6]` C++
`9.999985000000001e-1`, Rust `9.99999e-1` (rel err 5.0e-7). These are the same values as
the entry "shell_boundary_negative_exactly_two_tol after PR #849" above. Locally, the same
head, and that head merged with main `2d37267fe`, pass the case with every value
bit-identical (34 values, max rel err 0). With temporary logging, none of the case's
`IntersectInsertRelate` calls has a failing imprint, so the narrowed UB-242 stop does not
apply. The CI run of an unrelated branch (`difftest/endcap-file-load-cache-seed`,
`88c9219dc`), which still lists `gen0235` as a known mismatch and so lacks that change,
fails the same case with the same values. The exact-tolerance boundary (shell boundary at
exactly twice the tolerance) evidently resolves differently on the CI runners. Not parked
as a known mismatch, because it passes locally and the gate would reject the entry.

PR #884.
