//! Internal components (experimental, #6892): parts and assemblies stored inside a design file. Each is a whole part or assembly file,
//! kept as its bytes ([`crate::blob::Blob`]) under `internal/` in the design container and named as
//! the file it unpacks to (`Top.scprt`). The design links one like any other design file, by a link
//! path that starts with [`LINK`]; while the design is open the app unpacks them beside it,
//! and a save packs them again. A build that knows nothing of them keeps the parts as they came.

/// The container folder the internal components live in.
pub const PREFIX: &str = "internal/";
/// A link path that names a member starts with this, then the member's name: a relative path no
/// file is expected at, so a build without internal components reports the link as missing and keeps its bodies.
pub const LINK: &str = ".~internal/";
/// At most this many internal components in one design; past it the rest is left out.
pub const MAX_FILES: usize = 4096;
/// The extensions a member's name may end with: the kinds of design file.
const EXTENSIONS: [&str; 3] = [".sc", ".scprt", ".scasm"];

/// A member's name: one file name the app can write on any system. 1 to 200 bytes, no folder
/// separator, control character or character Windows refuses, not starting with `.`, not ending
/// with a space or `.`, not a Windows device name, and ending with a design file's extension.
pub fn valid_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.ends_with(|c: char| c.is_ascii_digit()));
    (1..=200).contains(&name.len())
        && !name.starts_with('.')
        && !name.ends_with(' ')
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        && !device
        && EXTENSIONS.iter().any(|e| name.len() > e.len() && name.to_ascii_lowercase().ends_with(e))
}

/// The member a link path names, when it names one.
pub fn linked(path: &str) -> Option<&str> {
    path.strip_prefix(LINK).filter(|name| valid_name(name))
}

/// The link path that names the member `name`.
pub fn link(name: &str) -> String {
    format!("{LINK}{name}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_member_name_is_one_plain_design_file_name() {
        for good in ["Top.scprt", "Shelf Bracket 2.scprt", "Hinge.scasm", "Träger.sc", "a.b.scprt"] {
            assert!(valid_name(good), "{good}");
            assert_eq!(linked(&link(good)), Some(good));
        }
        for bad in [
            "",
            ".scprt",
            ".hidden.scprt",
            "a/b.scprt",
            "a\\b.scprt",
            "..",
            "top.txt",
            "top",
            "con.scprt",
            "COM1.scprt",
            "a:b.scprt",
            "a\n.scprt",
            "top.scprt ",
        ] {
            assert!(!valid_name(bad), "{bad:?}");
            assert_eq!(linked(&link(bad)), None, "{bad:?}");
        }
        assert!(!valid_name(&format!("{}.scprt", "x".repeat(200))));
        assert_eq!(linked("parts/Top.scprt"), None);
    }
}
