//! CSV text fields, as every SolidCraft export writes them and the parameter import reads them.
//!
//! A spreadsheet reads a field that starts with `=`, `+`, `-` or `@` as a formula, so a part
//! numbered `=1+1` would show `2`. A field like that is written with a `'` first, which
//! spreadsheets show as text (OWASP's CSV Injection page). [`unguard`] takes the `'` off again
//! when SolidCraft reads its own file back.

use std::borrow::Cow;

/// The first characters that make a spreadsheet read a field as a formula, from OWASP's list.
/// The full-width forms act as formulas in some locales, such as Japanese, so they are here too.
const FORMULA_STARTS: [char; 11] = ['=', '+', '-', '@', '\t', '\r', '\n', '＝', '＋', '－', '＠'];

/// What makes a field need quotes: a separator (`;` as well as `,`, since a spreadsheet set to
/// `;` splits there and the new cell could start a formula), a quote, a tab or a line break.
const NEEDS_QUOTES: [char; 6] = [',', ';', '"', '\t', '\r', '\n'];

/// A text field, quoted only when it needs quotes.
pub fn field(text: &str) -> String {
    let text = guard(text);
    if text.contains(NEEDS_QUOTES) { quote(&text) } else { text.into_owned() }
}

/// A text field, always in quotes.
pub fn quoted(text: &str) -> String {
    quote(&guard(text))
}

/// A field as SolidCraft wrote it, with the `'` that [`field`] or [`quoted`] put first taken off.
/// A field without one is returned as it is.
pub fn unguard(text: &str) -> &str {
    match text.strip_prefix('\'') {
        Some(rest) if starts_a_formula(rest) => rest,
        _ => text,
    }
}

/// `text` with a `'` first when a spreadsheet would read it as a formula. A field that already
/// starts with `'` and then a formula gets one more, so [`unguard`] gives back exactly what was
/// written. A whole plain number (`-5`, `+1e-9`) is read as that number, not as a formula, so it
/// is left as it is.
fn guard(text: &str) -> Cow<'_, str> {
    if starts_a_formula(text) && !plain_number(text) { Cow::Owned(format!("'{text}")) } else { Cow::Borrowed(text) }
}

/// Whether `text`, after any `'` and spaces it starts with, starts as a formula does. Spaces count
/// because a spreadsheet may take them off as it reads (LibreOffice's "Trim spaces").
fn starts_a_formula(text: &str) -> bool {
    text.trim_start_matches(['\'', ' ']).starts_with(FORMULA_STARTS)
}

/// A finite number and nothing else, spaces included. Rust's `f64` parsing also takes `inf`,
/// `infinity` and `NaN` in any case, and none of them is finite.
fn plain_number(text: &str) -> bool {
    text.parse::<f64>().is_ok_and(f64::is_finite)
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

#[cfg(test)]
#[path = "csv_tests.rs"]
mod tests;
