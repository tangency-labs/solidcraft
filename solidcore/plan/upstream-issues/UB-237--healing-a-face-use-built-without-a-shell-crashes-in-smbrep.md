#### UB-237. Healing a face use built without a shell crashes in `SmBrep::TestClosureSplitRegion` [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmBrep.cpp:27907` (`pFUStart->GetShell()->GetRegion()`), `SmBrep::TestClosureSplitRegion` (def. `:27845`), called from `SmHealData::Fix_BadSheets` (`SmHealData.cpp:4832`, def. `:4732`), reached from `SmBrep::MakeTopologyFromData` when the healer runs (`bHealerIsEnabled` and `bAlwaysHeal` or a file healer version below the current one).
- **What happens:** `MakeTopologyFromData` builds a face use whose record lies in no face-use shell without a shell. `Fix_BadSheets` hands it to `TestClosureSplitRegion`, which dereferences the null shell.
- **Trigger:** `.smb` data whose shells leave a face use record uncovered (the `topology.brep_p10` "spine" fixtures with the IGES patch: region 1 without shells), read with the healer enabled.
- **How found:** crash in the C++ reference run (`difftest --unit topology.brep_p10 --fuzz 2000`: `fuzz-out/n2000/topology.brep_p10/fuzz00353.json`, `fuzz00790.json`, `fuzz01898.json`, `fuzz01957.json`; mutations `/options/5 = true`, i.e. healer enabled).
- **Confirmed:** yes (SIGSEGV, 4 cases).
- **Severity:** crash/UB (null dereference). A crafted file with an old healer version reaches it through `ReadFromFile`.
- **Rust port:** not reached; the `topology.brep_p10` runner rejects healing requests on such data (`crates/difftest/src/units/topology_brep_p10.rs:182` `heal_guard`).
