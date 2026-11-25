//! Release channels. The site serves the stable web app at `/app/`, the release candidate at
//! `/app-rc/` and the bleeding-edge build at `/app-edge/`: one origin, so one set of browser
//! storage. A channel that kept its designs and settings in the same places could save a design
//! in a file format the stable app can't open, or overwrite stable's settings. So each
//! pre-release channel keeps everything under its own name, and stable (and any copy served from
//! another path: the release zip, `trunk serve`) keeps the names it has always used, so what
//! people have saved at `/app/` is untouched.
//!
//! The channel comes from the directory the app is served from, not from the build, so a build
//! taken from a release zip is as separate as one built on the spot.

/// The channel the app is served as, from the page's path: `"rc"`, `"edge"`, or `""` (stable).
pub fn of_path(pathname: &str) -> &'static str {
    match pathname.trim_end_matches("index.html").trim_end_matches('/').rsplit('/').next() {
        Some("app-rc") => "rc",
        Some("app-edge") => "edge",
        _ => "",
    }
}

/// `name` as this channel stores it: unchanged for stable, `rc:name` and `edge:name` otherwise.
pub fn name_in(channel: &str, name: &str) -> String {
    if channel.is_empty() { name.to_owned() } else { format!("{channel}:{name}") }
}

#[cfg(target_arch = "wasm32")]
pub fn current() -> &'static str {
    web_sys::window().and_then(|w| w.location().pathname().ok()).map_or("", |p| of_path(&p))
}

/// A storage key (local storage, IndexedDB database name) of the running channel.
#[cfg(target_arch = "wasm32")]
pub fn key(name: &str) -> String {
    name_in(current(), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_follows_the_directory() {
        assert_eq!(of_path("/app/"), "");
        assert_eq!(of_path("/app/index.html"), "");
        assert_eq!(of_path("/app-rc/"), "rc");
        assert_eq!(of_path("/app-rc"), "rc");
        assert_eq!(of_path("/app-rc/index.html"), "rc");
        assert_eq!(of_path("/app-edge/"), "edge");
        assert_eq!(of_path("/"), "");
        assert_eq!(of_path("/solidcraft-web-0.30.1/"), "");
        assert_eq!(of_path("/app/app-rc/"), "rc", "only the directory's own name counts");
        assert_eq!(of_path("/app-rc/app/"), "");
    }

    #[test]
    fn stable_keeps_its_names() {
        assert_eq!(name_in("", "solidcraft.prefs"), "solidcraft.prefs");
        assert_eq!(name_in("rc", "solidcraft.prefs"), "rc:solidcraft.prefs");
        assert_ne!(name_in("rc", "x"), name_in("edge", "x"));
    }
}
