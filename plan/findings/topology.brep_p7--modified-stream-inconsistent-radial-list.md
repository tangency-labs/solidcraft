# topology.brep_p7: modified `.smb` streams whose radial lists Rust's MakeTopologyFromData rejects and C++ builds (2026-10-27)

- **Unit:** `topology.brep_p7` (`SmBrep::ReadFromFile`); the deviation itself is in
  `topology.brep_p10` (`MakeTopologyFromData`, `crates/solidcore/src/topology/brep/p10.rs`).
- **Inputs:** built-in cases `read_<file>_index1` (one integer of the topology records
  overwritten, `rebuild = true`), all in the edge use records:
  - `plane_square.smb`, byte 1235 `0` -> `9` (edge use 0's type `0` -> `9`)
  - `surf_extrude.smb`, byte 1233 `0` -> `9` (the same field)
  - `surf_rev.smb`, byte 1235 `0` -> `9` (the same field)
  - `ellips_ts2.smb`, byte 1452 `1` -> `9` (edge use 11's edge `11` -> `19`)
- **C++:** status 0 with counts (regions shells faces loops edges vertices)
  `1 1 1 1 4 4` for the first three and `1 1 1 3 20 20` for `ellips_ts2`; the child
  process (`solidcore_ffi::isolate`) exits normally.
- **Rust:** `SolidError::InvalidInput` (`1007`) from
  `topology::brep::p10::make_topology_from_data`: "radial list links disagree" for the
  first three, "radial list leaves out an edge use of its edge" for `ellips_ts2`.
- **Cause:** a documented deviation of the `topology.brep_p10` port (file header,
  "Malformed data"): upstream rewrites each edge use's `m_pNext` / `m_pLast` from the
  record's next indices without checking that they form one consistent circular list
  per edge. With the modified records it leaves a radial list whose `m_pLast` disagrees
  with `m_pNext` (or one that leaves out an edge use of the edge) and carries on; the
  arena model keeps each edge's radial list as one vector and cannot hold that graph,
  so the port returns `InvalidInput`. The C++ B-rep is structurally inconsistent (the
  `plan/upstream-issues/UB-164--*.md` family: in-range but inconsistent cross-references
  that `MakeTopologyFromData` builds). Verified with the C++ reference: upstream's own
  `SmBrep::ValidatePointers` rejects all four B-reps (test 76 "Radial partner points back
  to edgeuse"; test 134 "Edge primary edgeuse is non-Null" for `ellips_ts2`), while the
  unmodified files validate clean: `plan/upstream-issues/WR-470--*.md`.
- **Status:** listed in `crates/difftest/known_mismatches/topology.brep_p7.toml` with
  `cpp_defect = "WR-470"` (parked; the Rust rejection is the correct result).
- **PR:** https://github.com/tangency-labs/solidcore/pull/1270
