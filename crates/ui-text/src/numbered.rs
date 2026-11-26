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
//! A text is stored as it is shown, `{placeholders}` and all, so it is filled as a message read
//! by its id is ([`Filled`]).

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
    /// The id, which names a message no table has when nothing else can: when it was counted in
    /// another list of ids than the installed English catalog's (a test's own catalogs), or when
    /// no catalog is installed (a test that installs none). Empty for [`Msg::of`].
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

impl Msg {
    /// The message at `place` among the ids with the fingerprint `ids`. `msg!` writes this after
    /// finding the id in the English catalog; nothing else should.
    #[doc(hidden)]
    pub const fn checked(place: u32, ids: u32, id: &'static str) -> Self {
        Self { place, ids, id }
    }

    /// What stands for the message when nothing can say which it is: its id.
    pub(crate) const fn unread(self) -> &'static str {
        self.id
    }

    /// The place, when it was counted in the ids with the fingerprint `ids`.
    pub(crate) fn place_in(self, ids: u32) -> Option<usize> {
        (self.ids == ids).then_some(self.place as usize)
    }

    /// The message with the id `id`, for an id built at run time (a command's label): its place
    /// in the installed English catalog. `None` when that catalog has no such id. This reads
    /// the ids, so a host that uses it keeps them.
    pub fn of(id: &str) -> Option<Self> {
        let english = crate::english();
        let place = english.ids().binary_search(&id).ok()?;
        Some(Self { place: u32::try_from(place).ok()?, ids: english.ids_fingerprint(), id: "" })
    }

    /// The message in this thread's language (English where the language lacks it).
    pub fn tr(self) -> &'static str {
        crate::locale().message(self)
    }
}

/// A message with its placeholders' values, by name.
#[derive(Clone, Copy, Debug)]
pub struct Filled<'a, const N: usize> {
    msg: Msg,
    values: [(&'static str, &'a str); N],
}

impl<'a, const N: usize> Filled<'a, N> {
    /// `msg!` writes this, after finding that the names are exactly the message's placeholders.
    #[doc(hidden)]
    pub const fn checked(msg: Msg, values: [(&'static str, &'a str); N]) -> Self {
        Self { msg, values }
    }

    /// The message in this thread's language, filled in.
    pub fn tr(&self) -> String {
        self.fill_in(crate::locale())
    }

    pub(crate) fn fill_in(&self, locale: crate::Locale) -> String {
        crate::interpolate(locale.message(self.msg), &self.values)
    }
}

/// A catalog as a table. `ids` are the English catalog's ids, sorted; `texts` gives this
/// catalog's text for an id it has, already as it is to be shown. `None` when a size does not fit.
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

    /// Counted among ids no catalog has (or before any is installed), a message reads as the id
    /// it kept, in a release build too, and never as nothing.
    #[test]
    fn a_message_nothing_can_name_reads_as_the_id_it_kept() {
        assert_eq!(Msg::checked(0, FINGERPRINT ^ 1, "ui.nowhere").tr(), "ui.nowhere");
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
}
