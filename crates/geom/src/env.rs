//! SolidCraft's environment variables, read in one place.
//!
//! Every variable is `SOLIDCRAFT_<NAME>`.

use std::ffi::OsString;

/// The prefix.
pub const PREFIX: &str = "SOLIDCRAFT_";

/// `SOLIDCRAFT_<name>`. `name` is the part after the prefix, e.g. `"RECOVERY_DIR"`.
pub fn var_os(name: &str) -> Option<OsString> {
    lookup(name, |k| std::env::var_os(k))
}

/// [`var_os`] as UTF-8; a value that is not valid UTF-8 counts as unset.
pub fn var(name: &str) -> Option<String> {
    var_os(name).and_then(|v| v.into_string().ok())
}

/// Whether the switch `SOLIDCRAFT_<name>` is on: set to anything but empty, `0`, `false` or `off`
/// (any case). Unset is off.
pub fn flag(name: &str) -> bool {
    is_on(var_os(name))
}

fn is_on(value: Option<OsString>) -> bool {
    value.is_some_and(|v| {
        let v = v.to_string_lossy();
        let v = v.trim();
        !(v.is_empty() || v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
    })
}

fn lookup(name: &str, get: impl Fn(&str) -> Option<OsString>) -> Option<OsString> {
    get(&format!("{PREFIX}{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, OsString> = pairs.iter().map(|(k, v)| ((*k).to_string(), OsString::from(*v))).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn the_prefixed_variable_is_read() {
        let get = env(&[("SOLIDCRAFT_RECOVERY_DIR", "/new")]);
        assert_eq!(lookup("RECOVERY_DIR", get), Some(OsString::from("/new")));
    }

    #[test]
    fn the_pre_rename_prefix_is_not_read() {
        let get = env(&[("SOLVECRAFT_CONTROL_PORT", "4000")]);
        assert_eq!(lookup("CONTROL_PORT", get), None);
    }

    #[test]
    fn a_flag_is_on_unless_unset_empty_zero_false_or_off() {
        for on in ["1", "true", "yes", "on", "ON"] {
            assert!(is_on(Some(OsString::from(on))), "{on}");
        }
        for off in ["", " ", "0", "false", "FALSE", "off", "Off"] {
            assert!(!is_on(Some(OsString::from(off))), "{off:?}");
        }
        assert!(!is_on(None));
    }

    #[test]
    fn unset_is_none() {
        assert_eq!(lookup("WAYLAND", env(&[("OTHER", "1")])), None);
    }
}
