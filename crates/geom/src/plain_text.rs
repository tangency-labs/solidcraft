//! Character policies for single-line drawing annotations.
//!
//! Callers impose their own length and surrounding-whitespace rules.

use unicode_general_category::{GeneralCategory as Gc, get_general_category};

/// Whether a line satisfies the weld-tail character policy.
pub fn is_plain_line(text: &str) -> bool {
    text.chars().all(plain_char)
}

fn plain_char(c: char) -> bool {
    !c.is_control()
        && (!c.is_whitespace() || c == ' ')
        && !matches!(c, '\u{00ad}' | '\u{0600}'..='\u{0605}' | '\u{061c}' | '\u{06dd}' | '\u{070f}' | '\u{0890}'..='\u{0891}' | '\u{08e2}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{110bd}' | '\u{110cd}' | '\u{13430}'..='\u{1343f}' | '\u{1bca0}'..='\u{1bca3}' | '\u{1d173}'..='\u{1d17a}' | '\u{e0000}'..='\u{e007f}')
}

/// Whether a line contains visible Unicode text suitable for a surface texture annotation.
/// Allows letters, numbers, punctuation, symbols, ASCII spaces and combining marks
/// following a base character. Rejects default-ignorable characters and blank symbols.
pub(crate) fn is_visible_line(text: &str) -> bool {
    let mut after_base = false;
    for c in text.chars() {
        match kind(c) {
            Kind::Base => after_base = true,
            Kind::Space => after_base = false,
            Kind::Mark if after_base => {}
            _ => return false,
        }
    }
    true
}

enum Kind {
    Base,
    Space,
    Mark,
    Refused,
}

fn kind(c: char) -> Kind {
    if c == ' ' {
        return Kind::Space;
    }
    if invisible(c) {
        return Kind::Refused;
    }
    match get_general_category(c) {
        Gc::UppercaseLetter
        | Gc::LowercaseLetter
        | Gc::TitlecaseLetter
        | Gc::ModifierLetter
        | Gc::OtherLetter
        | Gc::DecimalNumber
        | Gc::LetterNumber
        | Gc::OtherNumber
        | Gc::ConnectorPunctuation
        | Gc::DashPunctuation
        | Gc::OpenPunctuation
        | Gc::ClosePunctuation
        | Gc::InitialPunctuation
        | Gc::FinalPunctuation
        | Gc::OtherPunctuation
        | Gc::MathSymbol
        | Gc::CurrencySymbol
        | Gc::ModifierSymbol
        | Gc::OtherSymbol => Kind::Base,
        Gc::NonspacingMark | Gc::SpacingMark | Gc::EnclosingMark => Kind::Mark,
        _ => Kind::Refused,
    }
}

/// Letters, marks and symbols that draw nothing: the default-ignorable code points outside
/// the format category, plus two blank symbols.
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{034f}'
            | '\u{115f}'..='\u{1160}'
            | '\u{17b4}'..='\u{17b5}'
            | '\u{180b}'..='\u{180f}'
            | '\u{2800}'
            | '\u{3164}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{ffa0}'
            | '\u{fffc}'
            | '\u{e0100}'..='\u{e01ef}'
    )
}

#[cfg(test)]
mod tests {
    use super::is_plain_line;

    #[test]
    fn invisible_direction_marks_and_bom_are_rejected() {
        for text in ["x\u{200f}y", "x\u{feff}y", "x\u{202e}y", "x\u{2067}y", "x\u{2028}y", "x\ny"] {
            assert!(!is_plain_line(text), "{text:?}");
        }
    }

    #[test]
    fn ordinary_single_line_unicode_remains_valid() {
        for text in ["weld note", "é", "溶接", "123-ABC"] {
            assert!(is_plain_line(text), "{text:?}");
        }
    }
}
