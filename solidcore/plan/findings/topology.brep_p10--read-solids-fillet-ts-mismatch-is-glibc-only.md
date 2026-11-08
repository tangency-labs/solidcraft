# topology.brep_p10: `read_Solids_fillet_ts` disagrees only on glibc; known mismatches can name their libc (2026-10-25)

- **Unit / input:** `topology.brep_p10`, case `read_Solids_fillet_ts`
  (`Solids/fillet_ts.smb` via `SmBrep::ReadFromFile`).
- **Linux x86-64 (CI, glibc):** known mismatch. C++ merges 3 extra 3-way vertex clusters
  (22 vertices, 1 shell) where Rust keeps 25 vertices and 2 shells. Cause, recorded in
  `plan/findings/topology.brep_p10--heal-notify-projectandtrim-fixes-and-read-solids-fillet-ts.md`:
  `SmHealData::Fix_CoinVertices` keeps a dangling `SmVertexProps*` (UB-255); once glibc's
  7-slot tcache bin fills, a new `SmVertexProps` reuses the freed address and the stale
  pointer compares equal.
- **macOS arm64 (Apple clang, libc++, system malloc), main d4e1814c3:** the case passes:
  1683 floating values and 9486 exact items compared, 0 mismatches, `max_rel_err=0`. The
  allocator does not hand the freed address back at that point, so the C++ takes the
  intended path and agrees with Rust. The stale-entry guard then failed the whole unit
  (`listed in ... but the case is now pass: remove the entry`), as the earlier lane report
  `plan/findings/topology.brep_p10--expected-mismatch-unexpectedly-agrees-on-this-lane-2026-10.md`
  also observed.
- **Change (PR #1010):** a known-mismatch entry may carry `libc = "glibc"`. The entry still
  has to name an existing case, but on any other C library it does not apply and the case
  runs as an ordinary case that must pass. CI (Linux, glibc) keeps the entry and the
  stale-entry guard unchanged. `read_Solids_fillet_ts` is the first user.
