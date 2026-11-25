//! SolidCraft's fix preset (`core::fixes()` in SolidCraft's `crates/smlib/src/core.rs`): every
//! `UpstreamFixes` flag on. A fix that is broken is fixed or removed in solidcore, never held
//! off in SolidCraft.
//!
//! Other crates' tests include this file by path (`#[path]`), each using part of it.
#![allow(dead_code)] // a test helper

use solidcore::kernel::UpstreamFixes;

/// Every `UpstreamFixes` flag on, as SolidCraft runs.
pub fn solidcraft_fixes() -> UpstreamFixes {
    UpstreamFixes::all()
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
