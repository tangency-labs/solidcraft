//! Shared character policy for a single line of plain annotation text.
//!
//! Callers impose their own length and surrounding-whitespace rules. Text is an allow-list of
//! Unicode general categories: letters, numbers, punctuation, symbols, the ASCII space, and
//! combining marks that follow a base character. Controls, format characters, unassigned,
//! noncharacter and private-use code points, every other space, and characters that draw
//! nothing (default-ignorable code points, braille blank, object replacement) are refused.

use unicode_general_category::{get_general_category, GeneralCategory as Gc};

/// Whether every character is suitable for one plain annotation line.
pub fn is_plain_line(text: &str) -> bool {
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
