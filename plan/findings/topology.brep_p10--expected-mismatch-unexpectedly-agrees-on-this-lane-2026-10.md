# topology.brep_p10 — expected mismatch unexpectedly agrees on this lane (2026-10-24)

Fresh full run: 1404 cases, 1380 passed, 1 failed, 4 deferred, 0
`skipped(platform)`, 7 rejected, 12 known mismatches;
`max_rel_err=6.978000677784215e-7`. Crucially, the sole failure is not that
relative error. It is `read_Solids_fillet_ts`, input
`Solids/fillet_ts.smb`, with harness error
`listed in crates/difftest/known_mismatches.toml but the case is now pass: remove the entry`.
Its actual comparison has 1683 floating values and 9486 exact items compared,
zero mismatches and `max_rel_err=0`. The current known-mismatch annotation
expects different UV-trim/gap mutations (historical value lengths 1659/1683).
This lane instead agrees and correctly fails the stale-expectation guard.
Three isolated `--filter read_Solids_fillet_ts --no-report` runs reproduce
that error with `max_rel_err=0`; a second full run reproduces all outcome
counts above. This is not evidence of scheduling/worker instability.

There is only one newly failing case. For contrast, the existing
`known_mismatch` case `gen0115` contributes the unit's maximum error:
`values.dbls[654]`, C++ `9.7170165910819e-4`, Rust
`9.717009810547064e-4`; `read_Boolean_extrudeHoles1` remains a known
length disagreement (`values.dbls.len=909/921`). They are not additional
unwaived failures or evidence that the sole failure is floating-point residue.

Class: platform/environment-sensitive expected-mismatch inversion, not a
newly observed numeric regression. What would settle the policy: compare the
same fixture on authoritative Linux x86-64, determine why the expected UV-trim
mutation difference disappears here, and only then revise the expectation in
a separately owned harness change. This lane does not remove the waiver,
promote the case or infer an upstream defect from unexpected agreement.
