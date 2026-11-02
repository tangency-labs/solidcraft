//! Context pills (#2286): a pick made with Pick context (or `@`) sits in the message at the place
//! where the user made it, so "make A match B" cannot be read the other way round.
//!
//! A pill is text: `«1: Plate · 3 faces»`. The box stays an ordinary text field, so editing, paste,
//! selection and input methods keep working; the marker is only drawn as a pill and deleted as one.
//! What the pill stands for is kept beside the box ([`super::Pill`]); a marker whose number matches
//! no pick is plain text. When the message is sent the numbers are made 1, 2, 3 in reading order and
//! the agent gets the numbered groups after the message (see `Context::prompt_block`).

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Stroke};

/// Opens a pill's marker.
pub const OPEN: char = '«';
/// Closes it.
pub const CLOSE: char = '»';
/// The longest marker (characters) that counts as one; anything longer is plain text.
const MAX_MARKER: usize = 120;
/// The longest label a marker carries.
const MAX_LABEL: usize = 40;
/// The most digits of a pill's number.
const MAX_DIGITS: usize = 6;

/// Where a pill's marker is in a text, in characters (the end is not part of it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub id: u32,
    pub start: usize,
    pub end: usize,
}

/// The markers in `text`, in reading order.
pub fn spans(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars.get(i) == Some(&OPEN)
            && let Some(span) = marker_at(&chars, i)
        {
            i = span.end;
            found.push(span);
        } else {
            i += 1;
        }
    }
    found
}

/// The marker that starts at `start` (an [`OPEN`]), if it is well formed: digits, a colon, a label on
/// one line, [`CLOSE`].
fn marker_at(chars: &[char], start: usize) -> Option<Span> {
    let mut at = start + 1;
    let mut id: u32 = 0;
    let mut digits = 0;
    while let Some(d) = chars.get(at).and_then(|c| c.to_digit(10)) {
        digits += 1;
        if digits > MAX_DIGITS {
            return None;
        }
        id = id * 10 + d;
        at += 1;
    }
    if digits == 0 || chars.get(at) != Some(&':') {
        return None;
    }
    while at < chars.len() && at - start < MAX_MARKER {
        match chars.get(at) {
            Some(&CLOSE) => return Some(Span { id, start, end: at + 1 }),
            Some(&OPEN) | Some(&'\n') | None => return None,
            Some(_) => at += 1,
        }
    }
    None
}

/// The marker for pill `id` with `label` (made safe: no brackets, one line, a bounded length).
pub fn marker(id: u32, label: &str) -> String {
    let clean: String = label.chars().map(|c| if c == OPEN || c == CLOSE || c.is_control() { ' ' } else { c }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = if clean.chars().count() > MAX_LABEL { format!("{}…", clean.chars().take(MAX_LABEL - 1).collect::<String>()) } else { clean };
    format!("{OPEN}{id}: {clean}{CLOSE}")
}

/// The byte index of character `n` in `text` (the end of the text past its last character).
fn byte_at(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(b, _)| b)
}

/// Put `marker` into `text` at character `at` (clamped to the text), with a space on a side that
/// would touch other text. Returns the character index after the marker and its space: where the
/// caret goes, so the user keeps typing.
pub fn insert(text: &mut String, at: usize, marker: &str) -> usize {
    let at = at.min(text.chars().count());
    // Never inside another marker (the caret can sit in one: it is text): after it.
    let at = spans(text).iter().find(|s| s.start < at && at < s.end).map_or(at, |s| s.end);
    let before = text.chars().take(at).last();
    let after = text.chars().nth(at);
    let mut put = String::new();
    if before.is_some_and(|c| !c.is_whitespace()) {
        put.push(' ');
    }
    put.push_str(marker);
    let space_after = after.is_none_or(|c| !c.is_whitespace());
    if space_after {
        put.push(' ');
    }
    let len = put.chars().count();
    text.insert_str(byte_at(text, at), &put);
    at + len
}

/// `text` without the characters `start..end`.
pub fn remove(text: &str, start: usize, end: usize) -> String {
    text.chars().enumerate().filter(|(n, _)| *n < start || *n >= end).map(|(_, c)| c).collect()
}

/// Make the markers of `known` pills read 1, 2, 3 in reading order, each with its pill's own label
/// (a pill used twice keeps one number). A marker of no known pill (one pasted from an earlier
/// message, or typed) stands for nothing, so it is turned into `‹1: …›` text: the agent must not
/// read it as one of the numbered groups. Returns the new text and the old number of each new one
/// (index 0 is number 1).
pub fn renumber(text: &str, known: &[(u32, String)]) -> (String, Vec<u32>) {
    let chars: Vec<char> = text.chars().collect();
    let mut order: Vec<u32> = Vec::new();
    let mut out = String::new();
    let mut at = 0;
    for span in spans(text) {
        let Some((_, label)) = known.iter().find(|(id, _)| *id == span.id) else {
            out.extend(chars.get(at..span.start).unwrap_or_default());
            out.push('‹');
            out.extend(chars.get(span.start + 1..span.end.saturating_sub(1)).unwrap_or_default());
            out.push('›');
            at = span.end;
            continue;
        };
        let new = match order.iter().position(|id| *id == span.id) {
            Some(n) => n,
            None => {
                order.push(span.id);
                order.len() - 1
            }
        };
        out.extend(chars.get(at..span.start).unwrap_or_default());
        out.push_str(&marker(u32::try_from(new + 1).unwrap_or(u32::MAX), label));
        at = span.end;
    }
    out.extend(chars.get(at..).unwrap_or_default());
    (out, order)
}

/// Every marker in `text` with its number raised by `by`: a queued message joining an earlier one in
/// one prompt must not reuse its numbers.
pub fn shift(text: &str, by: u32) -> String {
    if by == 0 {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    for span in spans(text) {
        out.extend(chars.get(at..span.start).unwrap_or_default());
        let body: String = chars.get(span.start..span.end).unwrap_or_default().iter().collect();
        // «12: label» -> «(12 + by): label»
        let rest = body.split_once(':').map_or("", |(_, r)| r);
        out.push(OPEN);
        out.push_str(&span.id.saturating_add(by).to_string());
        out.push(':');
        out.push_str(rest);
        at = span.end;
    }
    out.extend(chars.get(at..).unwrap_or_default());
    out
}

/// How a marker is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub background: Color32,
    pub text: Color32,
    /// What it stands for has left the design: struck through and dimmed.
    pub gone: bool,
}

/// `text` as a layout job: the markers `look` returns something for are drawn as pills, the rest is
/// plain. The text keeps every character, so the caret and the selection are those of the box.
pub fn job(text: &str, font: &FontId, color: Color32, wrap_width: f32, look: &dyn Fn(u32) -> Option<Look>) -> LayoutJob {
    let plain = TextFormat { font_id: font.clone(), color, ..Default::default() };
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let mut at = 0;
    for span in spans(text) {
        let Some(look) = look(span.id) else { continue };
        let (a, b) = (byte_at(text, span.start), byte_at(text, span.end));
        if let Some(before) = text.get(byte_at(text, at)..a) {
            job.append(before, 0.0, plain.clone());
        }
        let mut format = TextFormat { font_id: font.clone(), color: look.text, background: look.background, ..Default::default() };
        if look.gone {
            format.strikethrough = Stroke::new(1.0, look.text);
        }
        if let Some(body) = text.get(a..b) {
            job.append(body, 0.0, format);
        }
        at = span.end;
    }
    if let Some(rest) = text.get(byte_at(text, at)..) {
        job.append(rest, 0.0, plain);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_are_found_in_reading_order() {
        let text = "make «2: Boss · 1 face» fit «10: Plate · 3 faces» now";
        let found = spans(text);
        assert_eq!(found.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2, 10]);
        assert_eq!(found[0].start, 5);
        assert_eq!(text.chars().nth(found[0].end - 1), Some(CLOSE));
    }

    #[test]
    fn malformed_markers_are_plain_text() {
        for text in ["«x: no digits»", "«1 no colon»", "«1: open", "«1: two «2: nested»»", "«1: line\nbreak»", "« : »", "«1234567: long»"]
        {
            let ids: Vec<u32> = spans(text).iter().map(|s| s.id).collect();
            // Only a well-formed inner marker may survive.
            assert!(ids.iter().all(|id| *id == 2), "{text}: {ids:?}");
        }
        let long = format!("«1: {}»", "x".repeat(200));
        assert!(spans(&long).is_empty());
    }

    #[test]
    fn a_marker_is_made_safe() {
        let m = marker(3, "bad « » label\nwith lines");
        assert_eq!(spans(&m).len(), 1, "{m}");
        assert_eq!(m, "«3: bad label with lines»");
        let m = marker(1, &"y".repeat(100));
        assert!(m.chars().count() <= MAX_LABEL + 8, "{m}");
        assert_eq!(spans(&m).len(), 1);
    }

    #[test]
    fn insert_adds_spaces_only_where_text_would_touch() {
        let mut text = String::from("fit to");
        let at = insert(&mut text, 3, "«1: A»");
        assert_eq!(text, "fit «1: A» to");
        assert_eq!(at, "fit «1: A»".chars().count());
        let mut text = String::from("fit ");
        insert(&mut text, 4, "«1: A»");
        assert_eq!(text, "fit «1: A» ");
        let mut text = String::new();
        let at = insert(&mut text, 99, "«1: A»");
        assert_eq!(text, "«1: A» ");
        assert_eq!(at, text.chars().count());
        // Non-ASCII text around the caret.
        let mut text = String::from("größe prüfen");
        insert(&mut text, 5, "«1: A»");
        assert!(text.starts_with("größe «1: A»"), "{text}");
    }

    #[test]
    fn a_pick_made_with_the_caret_inside_a_marker_goes_after_it() {
        let mut text = String::from("a «1: Plate» b");
        let inside = 6;
        let at = insert(&mut text, inside, "«2: X»");
        assert_eq!(text, "a «1: Plate» «2: X» b");
        assert_eq!(spans(&text).iter().map(|s| s.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(at, "a «1: Plate» «2: X»".chars().count(), "the caret is after the new mark");
        // At either edge of a marker the pick goes where the caret is.
        let mut text = String::from("a «1: Plate» b");
        insert(&mut text, 2, "«2: X»");
        assert!(text.starts_with("a «2: X» «1: Plate»"), "{text}");
    }

    #[test]
    fn remove_takes_exactly_the_marker() {
        let text = "a «1: A» b";
        let s = &spans(text)[0];
        assert_eq!(remove(text, s.start, s.end), "a  b");
    }

    #[test]
    fn renumbering_follows_reading_order_and_normalises_labels() {
        let known = vec![(7, "Plate".to_string()), (3, "Boss".to_string())];
        let text = "put «7: stale» on «3: x» then «7: again» and «9: unknown»";
        let (out, old) = renumber(text, &known);
        assert_eq!(out, "put «1: Plate» on «2: Boss» then «1: Plate» and ‹9: unknown›");
        assert_eq!(old, vec![7, 3]);
    }

    #[test]
    fn shifting_raises_every_number() {
        assert_eq!(shift("«1: A» and «2: B»", 3), "«4: A» and «5: B»");
        assert_eq!(shift("«1: A»", 0), "«1: A»");
        assert_eq!(shift("no markers", 5), "no markers");
    }

    #[test]
    fn the_layout_job_keeps_every_character() {
        let font = FontId::proportional(12.0);
        let look = |id: u32| (id == 1).then_some(Look { background: Color32::GRAY, text: Color32::WHITE, gone: false });
        let text = "ä «1: Plate» ö «2: other» ü";
        let j = job(text, &font, Color32::BLACK, 300.0, &look);
        assert_eq!(j.text, text);
        // One run per plain stretch and one for the pill.
        assert_eq!(j.sections.len(), 3);
        let gone = |_: u32| Some(Look { background: Color32::GRAY, text: Color32::WHITE, gone: true });
        let j = job(text, &font, Color32::BLACK, 300.0, &gone);
        assert_eq!(j.text, text);
        assert!(j.sections.iter().any(|s| s.format.strikethrough != Stroke::NONE));
    }
}
