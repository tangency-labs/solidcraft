//! The file name an export offers in the save dialog, from the Export file name preference: a
//! template where `{design}` stands for the design's name and `{body}` for the exported body's.
//! An empty template keeps the name each export offered before the preference existed.

/// Placeholder for the design's name.
pub(crate) const DESIGN: &str = "{design}";
/// Placeholder for the exported body's name.
pub(crate) const BODY: &str = "{body}";

/// Longest name offered, in characters; file systems commonly allow 255 bytes with the extension.
const MAX_CHARS: usize = 200;

/// The file name, without extension, offered for an export of design `design`. `body` is the
/// exported body (the first one when an export holds several; `None` when the design has no
/// body, and `{body}` then reads as the design's name). `legacy` is the name offered with an empty
/// template, or when the template leaves nothing usable as a file name.
pub(crate) fn stem(template: &str, design: &str, body: Option<&str>, legacy: &str) -> String {
    if template.trim().is_empty() {
        return legacy.to_string();
    }
    let named = template.replace(DESIGN, design).replace(BODY, body.unwrap_or(design));
    let safe: String = named.chars().map(|c| if forbidden(c) { '_' } else { c }).take(MAX_CHARS).collect();
    // Windows drops trailing dots and spaces; leading ones hide the file or read as a mistake.
    let safe = safe.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if safe.chars().all(|c| c == '_') { legacy.to_string() } else { safe.to_string() }
}

/// The body `{body}` names in an export of the whole design: its first body, in model order.
pub(crate) fn first_body(app: &crate::SolveApp) -> Option<String> {
    crate::open_meshing::session_world(&app.session).bodies.first().map(|b| b.name.clone())
}

/// A character no common file system takes in a file name.
fn forbidden(c: char) -> bool {
    c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_template_offers_the_old_name() {
        assert_eq!(stem("", "Bracket", Some("Base"), "Base"), "Base");
        assert_eq!(stem("   ", "Bracket", None, "Bracket"), "Bracket");
    }

    #[test]
    fn placeholders_take_the_design_and_body_names() {
        assert_eq!(stem("{design}-{body}", "Bracket", Some("Base"), "x"), "Bracket-Base");
        assert_eq!(stem("{body} ({design}) {body}", "Bracket", Some("Base"), "x"), "Base (Bracket) Base");
        assert_eq!(stem("print_{design}", "Bracket", Some("Base"), "x"), "print_Bracket");
        assert_eq!(stem("{design}-{unknown}", "Bracket", Some("Base"), "x"), "Bracket-{unknown}", "other text is kept as typed");
    }

    #[test]
    fn a_design_with_no_body_names_the_body_after_the_design() {
        assert_eq!(stem("{body}", "Bracket", None, "x"), "Bracket");
    }

    #[test]
    fn the_name_is_safe_as_a_file_name() {
        assert_eq!(stem("{design}/{body}", "A:B", Some("C?"), "x"), "A_B_C_");
        assert_eq!(stem(" .{design}. ", "Bracket", None, "x"), "Bracket");
        assert_eq!(stem("{body}", "Bracket", Some("a\nb\t"), "x"), "a_b_");
        assert_eq!(stem("{body}", "Bracket", Some(".."), "Old"), "Old", "nothing usable is left");
        assert_eq!(stem("///", "Bracket", None, "Old"), "Old");
        assert_eq!(stem(&"{design}".repeat(100), "Bracketé", None, "x").chars().count(), MAX_CHARS);
    }
}
