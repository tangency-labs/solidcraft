#### UB-164 addendum (2026-10-27): a modified edge use type makes `SmBrep::ReadFromFile` loop
Updates: UB-164

- **What changed:** UB-164's trigger can also make the C++ read never return, not only crash.
- **Trigger:** `Mouse.smb` with byte 1692 `0` -> `9`, which changes edge use 5's type from `0` to `9` (line `5 0 5 1 1 5 -6 -10` of the edge use records). `SmBrepData::ReadFromFile` accepts the stream. `SmBrep::ReadFromFile` then runs at 100% CPU until the 30 s alarm of its isolated child process ends it. This happens with `bRebuildUVTrimCurves` both TRUE and FALSE, so the loop is in the part of the read the two share: `SmBrepData::ReadFromFile` and `MakeTopologyFromData`, most likely the latter's walk over the rewritten edge use lists. There was no debugger on the machine to take a backtrace.
- **How found:** `topology.brep_p7` difftest case `read_Mouse.smb_index1`, which is `rejected` (child ran past `READ_SECONDS`). Stable over 4 runs (PR #1270).
- **Confirmed:** yes, the C++ reference build hangs on it every run.
- **Other results of the same sweep (PR #1270):** 55 streams, each with one integer of the topology records overwritten and accepted by the stream guard and by Rust's B-rep data read:
  - 25 read the same on both sides.
  - 25 crash C++ and 1 hangs it (the stream above): 26 rejected.
  - 4 read as `SM_SUCCESS` with a pointer-invalid B-rep: WR-470.
