//! Right-to-left interface text. egui shapes each run of one font with harfrust, which draws a
//! run of Hebrew letters right to left, but it has no bidirectional layout: the runs of a line
//! (words, spaces, numbers, punctuation) are placed left to right in the order they are stored, so
//! a Hebrew sentence would show its words in the wrong order and its punctuation on the wrong
//! side. [`visual`] runs the Unicode Bidirectional Algorithm (UAX #9) over a string and returns
//! the order egui needs: the runs of the line in display order, brackets inside right-to-left
//! text mirrored, and the letters of each right-to-left run left in reading order for the shaper
//! to reverse.
//!
//! This is for translated interface text, which is stored in reading (logical) order in the
//! catalogs. Each line is its own paragraph and takes its direction from its first strong
//! character, so English text comes back unchanged. The layout itself stays left to right, and
//! text egui wraps onto several lines keeps the order of the whole string, so long right-to-left
//! messages are best kept to one line or split with `\n`.

use std::borrow::Cow;

/// Wraps a run that must stay one left-to-right unit inside right-to-left text: a left-to-right
/// override inside an isolate, so the run is placed as a whole and keeps its own order.
const UNIT_START: &str = "\u{2066}\u{202D}";
const UNIT_END: &str = "\u{202C}\u{2069}";

/// Whether `text` has a character from a right-to-left script (Hebrew, Arabic, Syriac, Thaana,
/// N'Ko, Samaritan, Mandaic and their presentation forms).
pub fn has_rtl(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFC}'))
}

/// `text` in the order egui draws correctly (see the module notes). Text without right-to-left
/// characters is returned as it is.
pub fn visual(text: &str) -> Cow<'_, str> {
    if !has_rtl(text) {
        return Cow::Borrowed(text);
    }
    let on_screen = screen_order(text);
    // The shaper reverses each run of right-to-left letters itself, so put those back in reading
    // order; everything between them stays where the algorithm placed it.
    let mut output = String::with_capacity(on_screen.len());
    let mut letters: Vec<char> = Vec::new();
    for c in on_screen.chars() {
        if has_rtl(c.encode_utf8(&mut [0; 4])) {
            letters.push(c);
        } else {
            output.extend(letters.drain(..).rev());
            output.push(c);
        }
    }
    output.extend(letters.drain(..).rev());
    Cow::Owned(output)
}

/// `text` as it appears on screen, read left to right: what [`visual`] makes egui draw.
pub(crate) fn screen_order(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            output.push('\n');
        }
        visual_line(line, &mut output);
    }
    output
}

/// A catalog message in display order (see [`visual`]), with each `{placeholder}` kept whole
/// where it stands, so a value substituted afterwards lands in the right place and keeps its own
/// order: a number, a file name, or a label that is already in display order.
pub fn visual_template(text: &str) -> Cow<'_, str> {
    if !has_rtl(text) || !text.contains('{') {
        return visual(text);
    }
    let mut marked = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('{') {
        marked.push_str(before);
        let Some((name, remaining)) = after.split_once('}') else {
            marked.push('{');
            rest = after;
            continue;
        };
        marked.push_str(UNIT_START);
        marked.push('{');
        marked.push_str(name);
        marked.push('}');
        marked.push_str(UNIT_END);
        rest = remaining;
    }
    marked.push_str(rest);
    Cow::Owned(visual(&marked).into_owned())
}

fn visual_line(line: &str, output: &mut String) {
    let info = unicode_bidi::ParagraphBidiInfo::new(line, None);
    let (levels, runs) = info.visual_runs(0..line.len());
    for run in runs {
        let Some(text) = line.get(run.clone()) else { continue };
        let rtl = levels.get(run.start).is_some_and(|level| level.is_rtl());
        if rtl {
            output.extend(text.chars().rev().filter(|c| !is_control(*c)).map(mirrored));
        } else {
            output.extend(text.chars().filter(|c| !is_control(*c)));
        }
    }
}

/// Directional formatting characters: they steer the algorithm and have no glyph to draw.
fn is_control(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// The mirrored form of a paired punctuation mark drawn inside a right-to-left run (UAX #9 L4),
/// for the marks interface text uses.
fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        '≤' => '≥',
        '≥' => '≤',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_to_right_text_is_unchanged() {
        for text in ["Extrude", "Save changes to Bracket?", "Units: mm", "", "a\nb", "↖ 2 selected"] {
            assert!(matches!(visual(text), Cow::Borrowed(t) if t == text), "{text}");
        }
    }

    /// One Hebrew word is one run the shaper reverses itself; the words of a sentence swap
    /// places, so the first word ends up on the right.
    #[test]
    fn words_swap_places_and_letters_stay_in_reading_order() {
        assert_eq!(visual("שלום"), "שלום");
        assert_eq!(screen_order("שלום"), "םולש");
        assert_eq!(visual("שמור בשם"), "בשם שמור");
        assert_eq!(screen_order("שמור בשם"), "םשב רומש");
    }

    #[test]
    fn numbers_units_and_latin_words_keep_their_order() {
        // "Units: mm": the colon follows the Hebrew word, so on screen it sits on its left.
        assert_eq!(visual("יחידות: mm"), "mm :יחידות");
        assert_eq!(screen_order("יחידות: mm"), "mm :תודיחי");
        assert_eq!(visual("סקיצה12"), "12סקיצה");
        assert_eq!(visual("ייצוא STEP"), "STEP ייצוא");
        assert_eq!(visual("עומק 20.5 mm"), "mm 20.5 עומק");
    }

    #[test]
    fn brackets_inside_right_to_left_text_are_mirrored() {
        // "Parameters (5)": the parentheses still open towards the number.
        assert_eq!(visual("פרמטרים (5)"), "(5) פרמטרים");
    }

    #[test]
    fn each_line_is_its_own_paragraph() {
        assert_eq!(visual("שלום עולם\nworld"), "עולם שלום\nworld");
    }

    #[test]
    fn placeholders_stay_whole_where_they_stand() {
        // "Save the changes to {name}?": the question mark ends the sentence on the left.
        assert_eq!(visual_template("לשמור את השינויים ב־{name}?"), "?{name}ב־ השינויים את לשמור");
        assert_eq!(visual_template("{count} נבחרו"), "נבחרו {count}");
        assert_eq!(visual_template("Save {name}?"), "Save {name}?");
        // Unmarked, the Latin placeholder name would make the whole line left to right.
        assert_eq!(screen_order("{count} נבחרו"), "{count} ורחבנ");
        assert_eq!(visual_template("סגור {"), visual("סגור {"));
    }
}
