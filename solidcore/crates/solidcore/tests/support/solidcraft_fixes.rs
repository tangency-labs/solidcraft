//! SolidCraft's fix preset (`core::fixes()` in SolidCraft's `crates/smlib/src/core.rs`), as the
//! corpus records it in `kernel_cases/fixes.json`. A test there keeps the file equal to the
//! preset, so tests read the file instead of copying the list.

use solidcore::kernel::UpstreamFixes;

/// Every `UpstreamFixes` flag SolidCraft runs with turned on, the rest off. A name the kernel no
/// longer has fails: retiring a flag removes it from `fixes.json` too.
pub fn solidcraft_fixes() -> UpstreamFixes {
    let names: Vec<String> =
        serde_json::from_str(include_str!("../kernel_cases/fixes.json")).expect("fixes.json");
    let mut f = UpstreamFixes::default();
    for name in &names {
        *f.flag_mut(name)
            .unwrap_or_else(|| panic!("fixes.json: no flag {name}")) = true;
    }
    f
}
