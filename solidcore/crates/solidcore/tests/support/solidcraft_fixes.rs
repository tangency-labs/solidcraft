//! SolidCraft's fix preset (`core::fixes()` in SolidCraft's `crates/smlib/src/core.rs`): every
//! `UpstreamFixes` flag on except those SolidCraft holds back (`HELD_BACK` there), which the
//! corpus records in `kernel_cases/held_back.json`. A test there keeps the file equal to
//! `HELD_BACK`, so tests read the file instead of copying the list, and retiring a flag that
//! is not held back edits neither.
//!
//! Other crates' tests include this file by path (`#[path]`), each using part of it.
#![allow(dead_code, clippy::expect_used, clippy::panic)] // a test helper

use solidcore::kernel::UpstreamFixes;

/// The flags SolidCraft holds back (`kernel_cases/held_back.json`).
pub fn held_back() -> Vec<String> {
    serde_json::from_str(include_str!("../kernel_cases/held_back.json")).expect("held_back.json")
}

/// Every `UpstreamFixes` flag SolidCraft runs with turned on, the rest off. A held-back name
/// the kernel no longer has fails: retiring a held-back flag removes it from `held_back.json`
/// (and from `HELD_BACK`) too.
pub fn solidcraft_fixes() -> UpstreamFixes {
    let mut f = UpstreamFixes::all();
    for name in &held_back() {
        *f.flag_mut(name)
            .unwrap_or_else(|| panic!("held_back.json: no flag {name}")) = false;
    }
    f
}

/// The names of the flags [`solidcraft_fixes`] turns on, sorted by name (as
/// `UpstreamFixes::NAMES`).
pub fn solidcraft_fix_names() -> Vec<&'static str> {
    solidcraft_fixes()
        .flags()
        .filter(|&(_, on)| on)
        .map(|(name, _)| name)
        .collect()
}
