//! The text of a design saved as plain JSON. It is serde_json's pretty layout (two spaces a level,
//! one value a line) with one difference. A short list of plain numbers, such as a point
//! `[x, y]`, a direction `[x, y, z]` or a matrix row, stays on one line. Moving a sketch point then
//! changes one line of the file instead of four, so a diff in Git shows what changed.
//!
//! The layout works on serde_json's compact text and only adds whitespace outside strings. Keys
//! keep their order and every number and string keeps serde_json's exact text, so the file reads
//! back as the same design and saving it again gives the same bytes.

/// The most numbers a list may hold and still stay on one line. A longer list keeps one number a
/// line, so changing one of its numbers still changes one line.
const MAX_INLINE: usize = 16;

/// `compact`, serde_json's compact text of a value, laid out for reading and diffing. Text that is
/// not compact JSON comes back laid out as far as it goes, never a panic.
pub(crate) fn pretty(compact: &str) -> String {
    let b = compact.as_bytes();
    let mut out = String::with_capacity(compact.len() + compact.len() / 2);
    let mut depth = 0usize;
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        match c {
            b'"' => {
                let end = string_end(b, i);
                out.push_str(compact.get(i..end).unwrap_or_default());
                i = end;
            }
            b'{' | b'[' => {
                let close = if c == b'{' { b'}' } else { b']' };
                if b.get(i + 1) == Some(&close) {
                    out.push_str(if c == b'{' { "{}" } else { "[]" });
                    i += 2;
                } else if let Some(end) = (c == b'[').then(|| short_numbers_end(b, i)).flatten() {
                    out.push_str(&compact.get(i..end).unwrap_or_default().replace(',', ", "));
                    i = end;
                } else {
                    out.push(char::from(c));
                    depth += 1;
                    new_line(&mut out, depth);
                    i += 1;
                }
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                new_line(&mut out, depth);
                out.push(char::from(c));
                i += 1;
            }
            b',' => {
                out.push(',');
                new_line(&mut out, depth);
                i += 1;
            }
            b':' => {
                out.push_str(": ");
                i += 1;
            }
            _ => {
                // A number or a literal (`true`, `false`, `null`), up to the next structural byte.
                let start = i;
                while b.get(i).is_some_and(|c| !matches!(c, b',' | b'}' | b']' | b':' | b'"' | b'{' | b'[')) {
                    i += 1;
                }
                out.push_str(compact.get(start..i).unwrap_or_default());
            }
        }
    }
    out
}

fn new_line(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// Where the string that opens at `start` ends (just past its closing quote), or the end of the text
/// when it never closes.
fn string_end(b: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while let Some(&c) = b.get(i) {
        match c {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// Where the list that opens at `start` ends (just past its `]`), when it holds only numbers and
/// at most [`MAX_INLINE`] of them.
fn short_numbers_end(b: &[u8], start: usize) -> Option<usize> {
    let mut count = 1;
    let mut i = start + 1;
    loop {
        match b.get(i)? {
            b']' => return (count <= MAX_INLINE).then_some(i + 1),
            b',' => count += 1,
            c if c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E') => {}
            _ => return None,
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn laid_out(v: &Value) -> String {
        pretty(&serde_json::to_string(v).unwrap())
    }

    /// Without a list of numbers the text is serde_json's own pretty text, byte for byte. That
    /// covers nested objects and lists, empty ones, and strings holding quotes, escapes, brackets
    /// and characters outside ASCII.
    #[test]
    fn without_number_lists_the_text_is_serde_jsons_pretty_text() {
        let v = json!({
            "name": "Plate \"A\" {v2} [draft], 1:2",
            "path": "C:\\designs\\plate.sc",
            "notes": ["a,b", "", "\u{2300}20 mm", "\u{1F527}"],
            "empty_list": [],
            "empty_object": {},
            "flags": [true, false, null],
            "features": [{"id": 1, "kind": {"extrude": {"distance": 10.0}}}, {"id": 2, "name": "Sketch2"}],
            "mixed": [1, "two", 3.5],
            "nested": [[{"x": 1}], [[]]]
        });
        assert_eq!(laid_out(&v), serde_json::to_string_pretty(&v).unwrap());
    }

    /// A short list of numbers stays on one line, also inside another list, and every number keeps
    /// serde_json's text.
    #[test]
    fn a_short_list_of_numbers_stays_on_one_line() {
        let v = json!({"text": [1.4749999990191256, -0.0], "axis": [0, 0, 1], "tiny": [1e-12, 2.5e20], "rows": [[1.0, 0.0], [0.0, 1.0]]});
        let expected = "{\n  \"axis\": [0, 0, 1],\n  \"rows\": [\n    [1.0, 0.0],\n    [0.0, 1.0]\n  ],\n  \"text\": [1.4749999990191256, -0.0],\n  \"tiny\": [1e-12, 2.5e+20]\n}";
        assert_eq!(laid_out(&v), expected);
    }

    /// A list of more than [`MAX_INLINE`] numbers keeps one number a line, so changing one of them
    /// changes one line. Exactly [`MAX_INLINE`] stay on one line.
    #[test]
    fn a_long_list_of_numbers_keeps_one_number_a_line() {
        let long: Vec<u32> = (0..=MAX_INLINE as u32).collect();
        let text = laid_out(&json!({ "v": long }));
        // `{`, the key, one line per number, `]` and `}`.
        assert_eq!(text.lines().count(), MAX_INLINE + 5, "{text}");
        let full: Vec<u32> = (0..MAX_INLINE as u32).collect();
        assert_eq!(laid_out(&json!({ "v": full })).lines().count(), 3);
    }

    /// The text reads back as the same value, and laying it out again changes nothing.
    #[test]
    fn the_text_reads_back_as_the_same_value() {
        let v = json!({"a": [1, 2], "b": {"c": [[-1.5e-7, 3], "x,y"], "d": [{"e": [0.1, 0.2, 0.3]}]}, "f": "]]}"});
        let text = laid_out(&v);
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), v);
        let again: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(laid_out(&again), text);
    }

    /// Text that is not compact JSON, cut short or with an open string, is laid out as far as it
    /// goes without a panic.
    #[test]
    fn broken_text_never_panics() {
        for broken in ["", "[", "{\"a\":[1,2", "\"open", "\"esc\\", "]]}}", "[1,2,", "{\"k\":\"\u{e9}\"", "\u{e9}[1]"] {
            let _ = pretty(broken);
        }
    }
}
