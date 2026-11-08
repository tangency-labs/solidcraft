#### WR-213. `SmBendVolume::WriteToDB` writes ASCII values with no separator before their label
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmBendVolume.cpp:3255`-`3256` (`rFileOut << m_dBendNeutralDist << "BendVolume Bend NeutralDistance \n"`, same for `m_dSingularityTol`); read back at `:3328`-`3329` (`rFileIn >> pBendNeutralDist[0]`).
- **What happens:** The ASCII line is `0.75845918449404492BendVolume ...`. libstdc++ stops the double at `B`; libc++ `num_get` first gathers every character in `0-9a-fA-FxX+-pPiInN` (`0.758...Bend`), so the extraction fails and the bend volume reads back wrong. The round trip only works with libstdc++.
- **Trigger:** `WriteToDB` then `ReadFromDB` of an `SmBendVolume` (or a curve/surface in a bend volume) in ASCII, built against libc++.
- **How found:** generated difftest cases failing on macOS arm64 only (`surface.srf_in_volume` read_db with a bend volume, 25 cases, e.g. gen1970: `values.out.len: cpp=70 rust=102`).
- **Confirmed:** yes, on macOS arm64 (Apple clang 21, libc++); benign on x86-64 Linux (libstdc++). GitHub issue #450.
- **Severity:** wrong result (platform dependent).
- **Rust port:** Has no ASCII writer; `BendVolume::read_from_db` (`crates/solidcore/src/surface/bend_volume.rs`) reads the double as libstdc++ does (stops at the label), so it matches the reference build.
