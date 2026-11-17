//! Whether this computer has OpenXR, found without loading anything: the loader library and a
//! registered OpenXR runtime are looked up as files and settings, so asking starts no VR software
//! (a run that does not start VR must not touch it). Preferences › VR headset says what it finds.

use std::path::{Path, PathBuf};

/// What a run without VR found of OpenXR on this computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Found {
    /// The loader and a runtime are there: VR can be tried.
    Ready,
    /// The OpenXR loader is neither beside the program nor on the search path (Windows, where an
    /// app brings it along).
    NoLoader,
    /// No OpenXR runtime (VR software such as SteamVR or Meta Quest Link) is registered.
    NoRuntime,
}

impl Found {
    /// What the two lookups mean together; the loader is named first, as it is ours to ship.
    pub fn of(loader: bool, runtime: bool) -> Found {
        match (loader, runtime) {
            (false, _) => Found::NoLoader,
            (true, false) => Found::NoRuntime,
            (true, true) => Found::Ready,
        }
    }
}

/// Look for OpenXR on this computer, reading only files and settings.
pub fn installed() -> Found {
    Found::of(loader_found(), runtime_found())
}

/// The OpenXR loader's file name as the system loads it.
#[cfg(target_os = "windows")]
const LOADER: &str = "openxr_loader.dll";

/// The loader beside the program, in the system folder or on `PATH`, where Windows looks for it.
#[cfg(target_os = "windows")]
fn loader_found() -> bool {
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    let system = std::env::var_os("SystemRoot").map(|root| PathBuf::from(root).join("System32"));
    let path = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect::<Vec<_>>()).unwrap_or_default();
    beside.into_iter().chain(system).chain(path).any(|dir| dir.join(LOADER).is_file())
}

/// On Linux the loader comes with the system's OpenXR packages, in library folders that differ
/// between distributions; the runtime lookup says enough, and a start says the rest.
#[cfg(target_os = "linux")]
fn loader_found() -> bool {
    true
}

/// A runtime named by `XR_RUNTIME_JSON` (which the loader prefers), or the active one in the
/// registry, whose manifest file is there.
#[cfg(target_os = "windows")]
fn runtime_found() -> bool {
    if std::env::var_os("XR_RUNTIME_JSON").is_some_and(|path| Path::new(&path).is_file()) {
        return true;
    }
    let key = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Khronos\OpenXR\1");
    key.and_then(|key| key.get_value::<String, _>("ActiveRuntime")).is_ok_and(|path| Path::new(path.trim()).is_file())
}

/// A runtime named by `XR_RUNTIME_JSON`, or an `active_runtime.json` where the loader looks.
#[cfg(target_os = "linux")]
fn runtime_found() -> bool {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    if var("XR_RUNTIME_JSON").is_some_and(|path| path.is_file()) {
        return true;
    }
    let dirs = std::env::var("XDG_CONFIG_DIRS").ok();
    runtime_manifests(var("XDG_CONFIG_HOME").as_deref(), var("HOME").as_deref(), dirs.as_deref()).iter().any(|path| path.is_file())
}

/// Where the OpenXR loader looks for the active runtime on Linux, in its order: the user's config
/// folder (`XDG_CONFIG_HOME`, else `~/.config`), each of `XDG_CONFIG_DIRS` (else `/etc/xdg`), then
/// `/etc`. Relative folders are skipped, as the XDG rules say.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn runtime_manifests(config_home: Option<&Path>, home: Option<&Path>, config_dirs: Option<&str>) -> Vec<PathBuf> {
    let user = config_home
        .filter(|dir| dir.has_root())
        .map(Path::to_path_buf)
        .or_else(|| home.filter(|dir| dir.has_root()).map(|home| home.join(".config")));
    let dirs: Vec<PathBuf> =
        match config_dirs.map(|dirs| dirs.split(':').filter(|dir| Path::new(dir).has_root()).map(PathBuf::from).collect::<Vec<_>>()) {
            Some(dirs) if !dirs.is_empty() => dirs,
            _ => vec![PathBuf::from("/etc/xdg")],
        };
    user.into_iter().chain(dirs).chain([PathBuf::from("/etc")]).map(|dir| dir.join("openxr/1/active_runtime.json")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loader_is_named_before_the_runtime() {
        assert_eq!(Found::of(false, false), Found::NoLoader);
        assert_eq!(Found::of(false, true), Found::NoLoader);
        assert_eq!(Found::of(true, false), Found::NoRuntime);
        assert_eq!(Found::of(true, true), Found::Ready);
    }

    #[test]
    fn runtime_manifests_follow_the_xdg_folders_in_order() {
        let files = runtime_manifests(Some(Path::new("/c")), Some(Path::new("/h")), Some("/a:/b"));
        let want = ["/c", "/a", "/b", "/etc"].map(|dir| PathBuf::from(dir).join("openxr/1/active_runtime.json"));
        assert_eq!(files, want);
    }

    #[test]
    fn runtime_manifests_fall_back_to_the_defaults() {
        let files = runtime_manifests(None, Some(Path::new("/h")), None);
        let want = ["/h/.config", "/etc/xdg", "/etc"].map(|dir| PathBuf::from(dir).join("openxr/1/active_runtime.json"));
        assert_eq!(files, want);
    }

    #[test]
    fn relative_and_empty_folders_are_refused() {
        let files = runtime_manifests(Some(Path::new("conf")), Some(Path::new("home")), Some("rel::x"));
        let want = ["/etc/xdg", "/etc"].map(|dir| PathBuf::from(dir).join("openxr/1/active_runtime.json"));
        assert_eq!(files, want);
    }
}
