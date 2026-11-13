#### UB-new. `SmBendVolume::WriteToDB` writes values with no separator before the label [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** Surface
- **Location:** `SmBendVolume.cpp:3255-3256` (`m_dBendNeutralDist << "BendVolume ..."`); reader at `SmBendVolume.cpp:3328` (`rFileIn >> pBendNeutralDist[0]`).
- **What happens:** `WriteToDB` emits `0.758...BendVolume ...` with no separator, so libc++'s greedy `num_get` consumes the leading `B` of `BendVolume` as a hex-float/exponent tail and fails the extraction, while libstdc++ stops at `B`. New upstream defect (missing separator; also a libstdc++-only round trip).
- **Trigger:** A bend volume written by `WriteToDB` and re-read on a libc++ build (`surface.srf_in_volume` read_db cases).
- **How found:** macOS libc++ repro, GitHub issue #450.
- **Confirmed:** yes, 2026-10-24 on macOS arm64 (Apple clang 21, libc++). `surface.srf_in_volume` gen1970: `values.out.len: cpp=70 rust=102`.
- **Severity:** wrong result / UB.
- **Rust port:** unchanged; unit is only authoritative on a libstdc++ build.
- **Platform dependence:** libstdc++ only.


- **Confirmed (2026-10-24, issue #450):** UB-5 — macOS libc++ stack-protector abort in `topology.database_io` gen0185 (and gen0576, gen0883, gen1324, gen1363, gen1519, gen1590, gen1652, gen1783).
- **Confirmed (2026-10-24, issue #450):** UB-6 — hang/segv reached from a subnormal under libc++ in `topology.brep_data` gen0018 (hang) and gen0110 (segv).
