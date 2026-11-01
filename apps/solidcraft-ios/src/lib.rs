//! Native iOS entry point; geometry, commands, documents and UI are the desktop implementation.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(unsafe_code)]

#[cfg(target_os = "ios")]
mod app;
#[cfg(target_os = "ios")]
mod native;
#[cfg(any(target_os = "ios", test))]
mod paths;

/// Called once by the package's minimal Swift executable, on the main thread.
/// Exporting a C symbol requires the unsafe attribute, not pointer manipulation.
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn solidcraft_ios_main() {
    if let Err(error) = app::run() {
        eprintln!("SolidCraft iOS: {error}");
    }
}
