//! Shared character policy for a single line of plain annotation text.
//!
//! Callers impose their own length and surrounding-whitespace rules. This deliberately
//! rejects invisible formatting controls while allowing ordinary Unicode text.

/// Whether every character is suitable for one plain annotation line.
pub fn is_plain_line(text: &str) -> bool {
    text.chars().all(plain_char)
}

fn plain_char(c: char) -> bool {
    !c.is_control()
        && (!c.is_whitespace() || c == ' ')
        && !matches!(c, '\u{00ad}' | '\u{0600}'..='\u{0605}' | '\u{061c}' | '\u{06dd}' | '\u{070f}' | '\u{0890}'..='\u{0891}' | '\u{08e2}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{110bd}' | '\u{110cd}' | '\u{13430}'..='\u{1343f}' | '\u{1bca0}'..='\u{1bca3}' | '\u{1d173}'..='\u{1d17a}' | '\u{e0000}'..='\u{e007f}')
}
