//! Messages named by their place in the English catalog instead of their id, so the ids need
//! not be in the program.
//!
//! The ids of the English catalog, sorted, are numbered from 0: a message's number is its place
//! among them. `msg!` writes that number when a crate compiles, and a build script packs each
//! catalog into a table ([`encode`]) that holds its texts in the same order, so a text is found
//! by its place, with no id and no search:
//!
//! ```text
//! u32 count | u32 fingerprint | ⌈count / 8⌉ bytes, bit i set when the catalog has message i
//!           | (count + 1) × u32 offsets | the texts, UTF-8
//! ```
//!
//! all little-endian. The places only mean something for one list of ids, so a [`Msg`] carries
//! the list's [`fingerprint`] and a table refuses a message numbered against another list
//! (a crate compiled before a message was added) instead of showing a neighbour's text.
//!
//! A text's `{placeholders}` are stored as marks that say which argument goes there
//! ([`template`]), counted in the order the English text names them, so a filled message takes
//! its values by position ([`Filled`]) and no placeholder name is kept either.

/// What tells one sorted list of ids from another: FNV-1a over the ids, each followed by a
/// newline, folded to 32 bits. `msg!` computes the same (crates/ui-text-macros).
pub fn fingerprint<'a>(ids: impl IntoIterator<Item = &'a str>) -> u32 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in ids.into_iter().flat_map(|id| id.bytes().chain(std::iter::once(b'\n'))) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash ^ (hash >> 32)) as u32
}

/// A message, by its place among the English catalog's sorted ids.
#[derive(Clone, Copy, Debug)]
pub struct Msg {
    place: u32,
    /// The [`fingerprint`] of the ids the place was counted in.
    ids: u32,
    /// The id, in a build with debug assertions (development and tests), where a message no
    /// catalog has reads as its id, as a message looked up by id does. Empty in a release build,
    /// which is how the ids stay out of it (see [`debug_id`](crate::debug_id)).
    id: &'static str,
}

impl PartialEq for Msg {
    fn eq(&self, other: &Self) -> bool {
        (self.place, self.ids) == (other.place, other.ids)
    }
}

impl Eq for Msg {}

impl std::hash::Hash for Msg {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.place, self.ids).hash(state);
    }
}

/// `$id` in a build with debug assertions, empty otherwise: what `msg!` gives [`Msg::checked`]
/// for the id. The choice is made where `msg!` is used, and a release build never refers to the
/// literal, so it is not in the program.
#[doc(hidden)]
#[macro_export]
macro_rules! debug_id {
    ($id:literal) => {
        if cfg!(debug_assertions) { $id } else { "" }
    };
}

impl Msg {
    /// The message at `place` among the ids with the fingerprint `ids`. `msg!` writes this after
    /// finding the id in the English catalog; nothing else should.
    #[doc(hidden)]
    pub const fn checked(place: u32, ids: u32, id: &'static str) -> Self {
        Self { place, ids, id }
    }

    /// What stands for the message when no catalog has it: its id where the build kept it.
    pub(crate) const fn unread(self) -> &'static str {
        self.id
    }

    /// The message with the id `id`, for an id built at run time (a command's label): its place
    /// in the installed English catalog. `None` when that catalog has no such id. This reads
    /// the ids, so a host that uses it keeps them.
    pub fn of(id: &str) -> Option<Self> {
        let english = crate::english();
        let messages = english.messages();
        messages.contains_key(id).then(|| Self {
            id: "",
            place: messages.range::<str, _>((std::ops::Bound::Unbounded, std::ops::Bound::Excluded(id))).count() as u32,
            ids: table_fingerprint(english.table),
        })
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
        self.fill_in(crate::locale())
    }

    pub(crate) fn fill_in(&self, locale: crate::Locale) -> String {
        fill(locale.message(self.msg), &self.values)
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

/// A catalog as a table. `ids` are the English catalog's ids, sorted; `texts` gives this
/// catalog's text for an id it has, already as it is to be shown and with marks for its
/// placeholders. `None` when a size does not fit.
pub fn encode<'a>(ids: &[&str], texts: impl Fn(&str) -> Option<&'a str>) -> Option<Vec<u8>> {
    let count = u32::try_from(ids.len()).ok()?;
    let mut has = vec![0u8; ids.len().div_ceil(8)];
    let mut offsets = Vec::with_capacity((ids.len() + 1) * 4);
    let mut body = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        offsets.extend_from_slice(&u32::try_from(body.len()).ok()?.to_le_bytes());
        if let Some(text) = texts(id) {
            *has.get_mut(i / 8)? |= 1 << (i % 8);
            body.extend_from_slice(text.as_bytes());
        }
    }
    offsets.extend_from_slice(&u32::try_from(body.len()).ok()?.to_le_bytes());
    let mut out = Vec::with_capacity(8 + has.len() + offsets.len() + body.len());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&fingerprint(ids.iter().copied()).to_le_bytes());
    out.extend_from_slice(&has);
    out.extend_from_slice(&offsets);
    out.extend_from_slice(&body);
    Some(out)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// The fingerprint of the ids `table` was made for (0 for what is not a table).
fn table_fingerprint(table: &[u8]) -> u32 {
    u32_at(table, 4).unwrap_or(0)
}

/// The text of `msg` in `table`, or `None` when the catalog lacks it, the table was made for
/// another list of ids, or it is not a table.
pub(crate) fn lookup(table: &'static [u8], msg: Msg) -> Option<&'static str> {
    let count = u32_at(table, 0)? as usize;
    let place = msg.place as usize;
    if place >= count || table_fingerprint(table) != msg.ids {
        return None;
    }
    let has = table.get(8..8 + count.div_ceil(8))?;
    if has.get(place / 8)? & (1 << (place % 8)) == 0 {
        return None;
    }
    let offsets = 8 + has.len();
    let texts = table.get(offsets + (count + 1).checked_mul(4)?..)?;
    let from = u32_at(table, offsets + place * 4)? as usize;
    let to = u32_at(table, offsets + place * 4 + 4)? as usize;
    std::str::from_utf8(texts.get(from..to)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leak(bytes: Vec<u8>) -> &'static [u8] {
        Box::leak(bytes.into_boxed_slice())
    }

    /// The fingerprint of `ui.cancel` and `ui.file`.
    const FINGERPRINT: u32 = 0x13f5_3633;

    const IDS: [&str; 4] = ["ui.cancel", "ui.empty", "ui.file", "ui.only_english"];

    fn msg(id: &str) -> Msg {
        Msg::checked(IDS.iter().position(|i| *i == id).unwrap() as u32, fingerprint(IDS), "")
    }

    #[test]
    fn a_fingerprint_tells_lists_of_ids_apart() {
        // `msg!` computes the same one (crates/ui-text-macros, the same test).
        assert_eq!(fingerprint(["ui.cancel", "ui.file"]), FINGERPRINT);
        assert_ne!(fingerprint(["ui.cancel", "ui.file"]), fingerprint(["ui.cancel", "ui.files"]));
        // Where one id ends and the next begins counts.
        assert_ne!(fingerprint(["ab", "c"]), fingerprint(["a", "bc"]));
        assert_ne!(fingerprint([]), fingerprint([""]));
    }

    #[test]
    fn a_table_gives_each_text_back_by_its_place() {
        let japanese = |id: &str| match id {
            "ui.cancel" => Some("キャンセル"),
            "ui.empty" => Some(""),
            "ui.file" => Some("ファイル"),
            _ => None,
        };
        let table = leak(encode(&IDS, japanese).unwrap());
        assert_eq!(lookup(table, msg("ui.cancel")), Some("キャンセル"));
        assert_eq!(lookup(table, msg("ui.file")), Some("ファイル"));
        // A text that is empty is not a text that is missing.
        assert_eq!(lookup(table, msg("ui.empty")), Some(""));
        assert_eq!(lookup(table, msg("ui.only_english")), None);
        // A place beyond the table, and a message counted in another list of ids.
        assert_eq!(lookup(table, Msg::checked(4, fingerprint(IDS), "")), None);
        assert_eq!(lookup(table, Msg::checked(0, fingerprint(["ui.cancel"]), "")), None);
        assert_eq!(lookup(leak(encode(&[], |_| None).unwrap()), Msg::checked(0, fingerprint([]), "")), None);
    }

    /// A table that is cut short, or is not a table, gives nothing and never panics.
    #[test]
    fn a_broken_table_reads_as_empty() {
        let whole = encode(&IDS, |_| Some("text")).unwrap();
        for cut in 0..whole.len() {
            let table = leak(whole[..cut].to_vec());
            for id in IDS {
                let _ = lookup(table, msg(id));
            }
        }
        for hostile in [&[][..], &[0xff; 3], &[0xff; 64]] {
            assert_eq!(lookup(leak(hostile.to_vec()), Msg::checked(7, 0xffff_ffff, "")), None);
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
