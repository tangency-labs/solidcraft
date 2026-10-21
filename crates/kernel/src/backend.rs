//! Which kernel this process runs its operations on: truck (the default) or SMLib.
//!
//! The setting is the environment variable `SOLIDCRAFT_KERNEL`, read once per process: `truck`
//! (or unset) or `smlib`. Every operation that has an SMLib path asks [`backend`]. SMLib needs a
//! build with the kernel's `smlib` feature; set without it, or set to anything else, every such
//! operation fails with an error that says so rather than quietly running truck, so a comparison
//! run can never compare truck with itself.

use crate::{KernelError, Result};

/// The environment variable that picks the kernel.
pub const KERNEL_ENV: &str = "SOLIDCRAFT_KERNEL";

/// A kernel operations run on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Truck,
    Smlib,
}

impl Backend {
    /// The backend a setting names; `None` (unset) is truck.
    pub fn parse(setting: Option<&str>) -> std::result::Result<Backend, String> {
        match setting.map(str::trim) {
            None | Some("") | Some("truck") => Ok(Backend::Truck),
            Some("smlib") if cfg!(feature = "smlib") => Ok(Backend::Smlib),
            Some("smlib") => Err(format!("{KERNEL_ENV}=smlib: not supported yet: this build has no SMLib (feature `smlib`)")),
            Some(other) => Err(format!("{KERNEL_ENV}={other}: expected `truck` or `smlib`")),
        }
    }
}

/// The kernel this process runs on ([`KERNEL_ENV`], read on first use).
pub fn backend() -> Result<Backend> {
    static SETTING: std::sync::OnceLock<std::result::Result<Backend, String>> = std::sync::OnceLock::new();
    SETTING.get_or_init(|| Backend::parse(std::env::var(KERNEL_ENV).ok().as_deref())).clone().map_err(KernelError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_names_a_backend_or_is_refused() {
        assert_eq!(Backend::parse(None), Ok(Backend::Truck));
        assert_eq!(Backend::parse(Some("truck")), Ok(Backend::Truck));
        assert_eq!(Backend::parse(Some(" ")), Ok(Backend::Truck));
        assert_eq!(Backend::parse(Some("smlib")).is_ok(), cfg!(feature = "smlib"));
        assert!(Backend::parse(Some("occt")).is_err());
    }
}
