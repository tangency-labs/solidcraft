# geom.hash_table — ARM64 conversion-dependent voxel results (2026-10-24)

Fresh full run: 619 cases, 184 passed, 435 failed, 0 deferred, 0
`skipped(platform)`, 0 rejected, 0 known mismatches;
`max_rel_err=1.9983959312403277`. Two full runs reproduce the counts.
Samples (C++ first, Rust second):

- `heal_fitcrv_incremental`: planar input points (`z=0`), tolerance `0.01`;
  `values.floats.len=236/364`, `ints.ints.len=3935/4161`.
- `map_points_outside`: box `[0,0,0,1,1,1]`, voxel counts `[2,3,4]`, first
  mapped point `[-0.5,0.2,0.2]`; `values.floats[0]=0/0.5`.
- `gen0000` (seed `5793377638189413714`): zero-width y extent,
  `values.floats[19]=5.078904121819958/12.31764670862244`.

Class: architecture-dependent behavior, not small floating-point residue.
These are the negative/out-of-range and zero-extent voxel mapping conditions
already documented in UB-35 (`SmHashTable.h:1957-1959`). Rust deliberately
emulates the x86-64 reference; ARM64's saturating native conversion differs.
What would settle it: retain these exact inputs in an x86-64 reference replay
and verify the oracle restriction proposed by the separately owned #773.
Do not change the deterministic Rust conversion to match this host.
No new `upstream-issues.md` entry: UB-35 already records the defect.
