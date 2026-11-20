//! Messages named by a number instead of their id, so the ids need not be in the program.
//!
//! A message's number is a hash of its id ([`number`]), the same wherever it is computed: by
//! `msg!` when a crate compiles, by a build script that packs a catalog into a table
//! ([`encode`]), and here for an id built at run time ([`Msg::of`]). A [`Msg`] is that number.
//! A catalog's table holds its texts under their numbers, sorted, with no ids:
//!
//! ```text
//! u32 count | count × u64 numbers, ascending | (count + 1) × u32 offsets | the texts, UTF-8
//! ```
//!
//! all little-endian. A text's `{placeholders}` are stored as marks that say which argument
//! goes there ([`template`]), counted in the order the English text names them, so a filled
//! message takes its values by position ([`Filled`]) and no placeholder name is kept either.

use std::collections::BTreeMap;

/// The number of the message `id`: FNV-1a over its bytes, 64 bits.
pub const fn number(id: &str) -> u64 {
    let bytes = id.as_bytes();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash
}

/// A message, by its number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Msg(u64);

impl Msg {
    /// The message with this number. `msg!` writes this after finding the id in the English
    /// catalog; nothing else should.
    #[doc(hidden)]
    pub const fn checked(number: u64) -> Self {
        Self(number)
    }

    /// The message with the id `id`, for an id built at run time (a command's label). No check
    /// is made that a catalog has it: a missing one reads as empty.
    pub fn of(id: &str) -> Self {
        Self(number(id))
    }

    pub const fn number(self) -> u64 {
        self.0
    }

    /// The message in this thread's language (English where the language lacks it).
    pub fn tr(self) -> &'static str {
        crate::locale().message(self)
    }
}

/// A message with its placeholders' values, in the order its English text names them.
#[derive(Clone, Copy, Debug)]
pub struct Filled<'a, const N: usize> {
    msg: Msg,
    values: [&'a str; N],
}

impl<'a, const N: usize> Filled<'a, N> {
    /// `msg!` writes this, with the values put in the English text's order.
    #[doc(hidden)]
    pub const fn checked(msg: Msg, values: [&'a str; N]) -> Self {
        Self { msg, values }
    }

    /// The message in this thread's language, filled in.
    pub fn tr(&self) -> String {
        fill(self.msg.tr(), &self.values)
    }
}

/// The mark that stands for a placeholder in a table's text; the character after it says which
/// argument: `0`–`9`, then `a`–`z`.
const MARK: char = '\u{1}';
/// The most placeholders a message may have.
pub const MAX_PLACEHOLDERS: usize = 36;

/// The placeholder names of `text`, each once, in the order they first stand.
pub fn placeholders(text: &str) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    let mut rest = text;
    while let Some((_, after)) = rest.split_once('{') {
        rest = after;
        if let Some((name, tail)) = after.split_once('}')
            && !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            if !names.contains(&name) {
                names.push(name);
            }
            rest = tail;
        }
    }
    names
}

/// `text` with each `{name}` of `order` replaced by the mark of its place in `order`. A name
/// that is not in `order` (a translation's mistake) stays as it is written.
pub fn template(text: &str, order: &[&str]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('{') {
        out.push_str(before);
        let place = after.split_once('}').and_then(|(name, tail)| Some((order.iter().position(|n| *n == name)?, tail)));
        match place.and_then(|(index, tail)| Some((char::from_digit(u32::try_from(index).ok()?, 36)?, tail))) {
            Some((digit, tail)) => {
                out.push(MARK);
                out.push(digit);
                rest = tail;
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `template` with each mark replaced by its value. A mark with no value stays out.
fn fill(template: &str, values: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + values.iter().map(|v| v.len()).sum::<usize>());
    let mut chars = template.chars();
    while let Some(c) = chars.next() {
        if c != MARK {
            out.push(c);
        } else if let Some(value) = chars.next().and_then(|d| d.to_digit(36)).and_then(|i| values.get(i as usize)) {
            out.push_str(value);
        }
    }
    out
}

/// A catalog as a table: `messages` is number → text, the texts already as they are to be shown
/// and with marks for their placeholders. `None` when two ids have the same number or a size
/// does not fit; the caller says which ids.
pub fn encode(messages: &BTreeMap<u64, String>) -> Option<Vec<u8>> {
    let count = u32::try_from(messages.len()).ok()?;
    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    for number in messages.keys() {
        out.extend_from_slice(&number.to_le_bytes());
    }
    let mut offset: u32 = 0;
    for text in messages.values() {
        out.extend_from_slice(&offset.to_le_bytes());
        offset = offset.checked_add(u32::try_from(text.len()).ok()?)?;
    }
    out.extend_from_slice(&offset.to_le_bytes());
    for text in messages.values() {
        out.extend_from_slice(text.as_bytes());
    }
    Some(out)
}

/// The text under `number` in `table`, or `None` when it has none or the table is not one.
pub(crate) fn lookup(table: &'static [u8], number: u64) -> Option<&'static str> {
    let count = u32::from_le_bytes(table.get(..4)?.try_into().ok()?) as usize;
    let numbers = table.get(4..4 + count.checked_mul(8)?)?;
    let offsets = table.get(4 + count * 8..4 + count * 8 + (count + 1).checked_mul(4)?)?;
    let texts = table.get(4 + count * 8 + (count + 1) * 4..)?;
    let at = |i: usize| numbers.get(i * 8..i * 8 + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
    let offset = |i: usize| offsets.get(i * 4..i * 4 + 4).and_then(|b| b.try_into().ok()).map(|b| u32::from_le_bytes(b) as usize);
    let (mut low, mut high) = (0, count);
    while low < high {
        let middle = low + (high - low) / 2;
        match at(middle)?.cmp(&number) {
            std::cmp::Ordering::Equal => return std::str::from_utf8(texts.get(offset(middle)?..offset(middle + 1)?)?).ok(),
            std::cmp::Ordering::Less => low = middle + 1,
            std::cmp::Ordering::Greater => high = middle,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leak(bytes: Vec<u8>) -> &'static [u8] {
        Box::leak(bytes.into_boxed_slice())
    }

    #[test]
    fn a_number_is_the_same_at_compile_time_and_at_run_time() {
        const FILE: u64 = number("ui.file");
        // `msg!` computes the same one (crates/ui-text-macros, the same test).
        assert_eq!(FILE, 0xa1ef_8b5f_db45_ae41);
        assert_eq!(Msg::of("ui.file").number(), FILE);
        assert_ne!(number("ui.file"), number("ui.files"));
        assert_eq!(number(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn a_table_gives_each_text_back_and_nothing_else() {
        let messages: BTreeMap<u64, String> = [("ui.file", "File"), ("ui.cancel", "Cancel"), ("ui.empty", ""), ("ja", "ファイル")]
            .into_iter()
            .map(|(id, t)| (number(id), t.to_string()))
            .collect();
        let table = leak(encode(&messages).unwrap());
        for (id, text) in [("ui.file", "File"), ("ui.cancel", "Cancel"), ("ui.empty", ""), ("ja", "ファイル")] {
            assert_eq!(lookup(table, number(id)), Some(text), "{id}");
        }
        assert_eq!(lookup(table, number("ui.missing")), None);
        assert_eq!(lookup(leak(encode(&BTreeMap::new()).unwrap()), 1), None);
    }

    /// A table that is cut short, or is not a table, gives nothing and never panics.
    #[test]
    fn a_broken_table_reads_as_empty() {
        let messages: BTreeMap<u64, String> = [(number("a.b"), "text".to_string()), (number("c.d"), "more".to_string())].into_iter().collect();
        let whole = encode(&messages).unwrap();
        for cut in 0..whole.len() {
            let _ = lookup(leak(whole[..cut].to_vec()), number("a.b"));
        }
        for hostile in [&[][..], &[0xff; 3], &[0xff; 64]] {
            assert_eq!(lookup(leak(hostile.to_vec()), 7), None);
        }
    }

    #[test]
    fn placeholders_become_marks_in_the_english_order_and_are_filled_by_place() {
        let english = "Could not save {name}: {reason}";
        let order = placeholders(english);
        assert_eq!(order, ["name", "reason"]);
        // A translation may name them in another order.
        let japanese = template("{reason} のため {name} を保存できません", &order);
        assert!(!japanese.contains('{'));
        assert_eq!(fill(&japanese, &["a.sc", "disk full"]), "disk full のため a.sc を保存できません");
        assert_eq!(fill(&template(english, &order), &["a.sc", "disk full"]), "Could not save a.sc: disk full");
        // A value is put in as it is, braces and marks included.
        assert_eq!(fill(&template("{name}!", &["name"]), &["{reason}"]), "{reason}!");
        // A name the English text lacks, and braces that are not a placeholder, stay as written.
        assert_eq!(template("{other} and {name} and { x } and {", &order), "{other} and \u{1}0 and { x } and {");
        // A mark with no value is left out.
        assert_eq!(fill("a\u{1}5b", &[]), "ab");
    }
}
