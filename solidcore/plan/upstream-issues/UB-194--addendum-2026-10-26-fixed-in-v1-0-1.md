Updates: UB-194

#### UB-194 (update, 2026-10-26). Fixed in v1.0.1

- **Fixed upstream in v1.0.1 (3fd5b95):** `SM_HISTOGRAM_EDGELEN_INDEX` (`source/SMLib/inc/SmTArray.h`) is now `(ULONG)(smos_Floor(smos_Log10(a)) + 6)`. The 6 is added before the cast, so an edge length in `[1e-5, 1e4]` converts a value in `[1, 10]` and no negative double is converted. This is the same form as the sibling `SM_HISTOGRAM_GAPSIZE_INDEX`.
- **Found by:** reading the c9c5979..3fd5b95 diff during the reference pin move.
- **Confirmed:** no. Read at 3fd5b95 `source/SMLib/inc/SmTArray.h`; no arm64 C++ run at v1.0.1 yet.
- **Rust port:** `edge_len_histogram_index` (`heal/heal_data/p2.rs`) now computes `(floor(log10(a)) + 6) as u64`. Its results equal the old x86-64 emulation for every reachable input; a NaN length is still `InvalidInput`.
- **Difftest:** the x86-64 gate `X86_64_HASH_CASTS` (`crates/difftest/src/platform.rs`) stays. Its reason now cites only UB-35, which v1.0.1 leaves unchanged. Every unit it gates (`geom.hash_table` and `heal.heal_data_p1` to `p3`) still reaches `SmObjsInVoxels::MapPointToVoxel`, and the gate applies per unit, so no cases leave it. The arm64 histogram mismatches of `heal.heal_data_p1` to `p3` should disappear in a v1.0.1 arm64 run.
