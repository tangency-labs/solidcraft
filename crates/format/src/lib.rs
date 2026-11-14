//! Versioned file formats: what an app needs to read every file it ever wrote, and to open a file
//! a newer build of it wrote.
//!
//! A file starts with `"format": "<prefix>N"`. A [`Family`] is one kind of file (an app's designs,
//! its drawings): its tag, its own numbers, and how a file of an older number is brought up to
//! this build's, step by step (N → N+1 → … → current). The app's code only knows the current
//! format; all translation lives in the steps:
//! - the steps written out by hand for the first formats, one per format ([`Formats::hand`]);
//! - then one folder per format, `v<N>/`, one file per change in it ([`Part`]), each with a step
//!   when files from before the change are rewritten as they open ([`Formats::folded`]);
//! - then the changes no release has a number for yet, in `pending/` ([`Formats::pending`]):
//!   while there are any, the build writes the next number, with their names in the file's
//!   `format_pending`, so a later build runs just the steps a file is missing. A release moves
//!   them into the next `v<N>/`. [`build`] lists the folders for a build script.
//!
//! A file a newer build wrote says so by a number past this build's or a change name this build
//! doesn't have ([`Family::newer`]); the app then reads what it can and keeps the rest
//! ([`newer`]). A file is saved as JSON, or in a [`container`] with its binary payloads.
//!
//! Steps are JSON-level (the file's shape) and return the app's own error type; every error this
//! crate makes is a [`FormatError`], which the app's error type takes `From`.

use serde_json::Value;

pub mod build;
pub mod container;
pub mod newer;

/// Why a file is refused: its text says what is wrong, as the user reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatError {
    /// The file isn't one of the family's, or it is damaged.
    Invalid(String),
    /// The work was cancelled (a save the person stopped): nothing more was written.
    Cancelled,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::Invalid(why) => f.write_str(why),
            FormatError::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for FormatError {}

/// One JSON-level upgrade step: rewrites a file in place, or says why it can't.
pub type Step<E> = fn(&mut Value) -> Result<(), E>;

/// One change to a family's file format: a file in its `v<N>/` or `pending/` folder.
pub struct Part<E> {
    /// The file's name, which a file written while the change was pending lists.
    pub name: &'static str,
    /// How a file from before the change is rewritten as it opens, if it is.
    pub step: Option<Step<E>>,
}

/// Another spelling of a change name that a file of one format may carry (a build that named the
/// change otherwise): read as the change it stands for, never written.
#[derive(Clone, Copy, Debug)]
pub struct Alias {
    /// The format whose files may say it.
    pub format: u32,
    /// The spelling in the file.
    pub name: &'static str,
    /// The change it stands for.
    pub known_as: &'static str,
}

/// A family's upgrade path: the steps written out by hand, the folded formats after them (each a
/// list of changes) and the pending changes, which make the next format.
pub struct Formats<'a, E> {
    /// Entry `i` turns format `i + 1` into `i + 2`.
    pub hand: &'a [Step<E>],
    /// Each format after the hand-written ones, oldest first, and its changes.
    pub folded: &'a [(u32, &'a [Part<E>])],
    /// The changes no release has a number for yet.
    pub pending: &'a [Part<E>],
}

/// A kind of file whose format is versioned: its own format tag, its own numbers and its own
/// changes. Reading a file's format, telling a file a newer build saved and the upgrade are the
/// same for every family.
pub struct Family<'a, E> {
    /// The app, as messages name it.
    pub app: &'static str,
    /// What a file of the family is called in messages.
    pub noun: &'static str,
    /// The tag before the number in a file's `format`.
    pub prefix: &'static str,
    /// The tag files saved under an older name carry: read, never written.
    pub legacy_prefix: Option<&'static str>,
    /// The format of a file that names none, if such files exist.
    pub untagged: Option<u32>,
    /// Other spellings of change names that files may carry.
    pub aliases: &'a [Alias],
    /// The upgrade path.
    pub formats: Formats<'a, E>,
}

/// How a file shows a newer build saved it ([`Family::newer`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Newer {
    /// The number the file says.
    pub format: u32,
    /// The number this build reads up to.
    pub reads_up_to: u32,
    /// The changes the file names that this build doesn't have.
    pub unknown_changes: Vec<String>,
}

impl<E> Formats<'_, E> {
    /// The format this build writes: the last folded one, or the next while changes are pending.
    pub const fn current(&self) -> u32 {
        self.hand.len() as u32 + 1 + self.folded.len() as u32 + if self.pending.is_empty() { 0 } else { 1 }
    }

    /// The changes format `version` is made of, if it is made of changes.
    fn parts(&self, version: u32) -> Option<&[Part<E>]> {
        let after = version.checked_sub(self.hand.len() as u32 + 2)? as usize;
        match self.folded.get(after) {
            Some((_, parts)) => Some(parts),
            None if after == self.folded.len() && !self.pending.is_empty() => Some(self.pending),
            None => None,
        }
    }

    /// Whether a change of this name is anywhere in this build's history.
    fn knows_part_name(&self, name: &str) -> bool {
        self.folded.iter().flat_map(|(_, parts)| *parts).chain(self.pending).any(|part| part.name == name)
    }
}

impl<E> Family<'_, E> {
    /// The format this build writes: the last folded one, or the next while changes are pending.
    pub const fn current(&self) -> u32 {
        self.formats.current()
    }

    /// What a file of the family is called in messages.
    pub const fn noun(&self) -> &'static str {
        self.noun
    }

    /// The names of the pending changes this build writes into a file (`format_pending`).
    pub fn pending_names(&self) -> Vec<String> {
        self.formats.pending.iter().map(|p| p.name.to_string()).collect()
    }

    /// The format tag of format `n`.
    pub fn format_string(&self, n: u32) -> String {
        format!("{}{n}", self.prefix)
    }

    /// The version in one of this family's format tags; `None` for any other tag, another
    /// family's included.
    pub fn version_of(&self, s: &str) -> Option<u32> {
        s.strip_prefix(self.prefix).or_else(|| self.legacy_prefix.and_then(|p| s.strip_prefix(p)))?.parse().ok()
    }

    /// The change a name a file of format `version` lists stands for: its alias's, or itself.
    /// An alias counts only while this build has the change it stands for, so recognising it
    /// never grants support to another reader.
    fn canonical_applied_name<'name>(&self, version: u32, name: &'name str) -> &'name str {
        match self.aliases.iter().find(|a| a.format == version && a.name == name) {
            Some(alias) if self.formats.knows_part_name(alias.known_as) => alias.known_as,
            _ => name,
        }
    }

    /// What says a file of format `version` with the changes `applied` came from a newer build:
    /// a number past this build's, or a change this build doesn't have anywhere in its history
    /// (a branch's pending change may have landed at a later number). A list beside a format that
    /// isn't made of changes means nothing (a tool or a test set an older number on a newer file).
    /// `None` for a file this build fully knows.
    pub fn newer(&self, version: u32, applied: Option<&[String]>) -> Option<Newer> {
        let current = self.current();
        let listed = applied.filter(|_| version > current || self.formats.parts(version).is_some()).unwrap_or_default();
        let unknown_changes: Vec<String> =
            listed.iter().filter(|name| !self.formats.knows_part_name(self.canonical_applied_name(version, name))).cloned().collect();
        (version > current || !unknown_changes.is_empty()).then_some(Newer { format: version, reads_up_to: current, unknown_changes })
    }
}

impl<E: From<FormatError>> Family<'_, E> {
    /// Read a file's format and take its `format_pending` out of its JSON, before anything else
    /// in it is read: a file that isn't this family's is refused here. One a newer build saved is
    /// not: [`Family::newer`] says so.
    pub fn take_version(&self, v: &mut Value) -> Result<(u32, Option<Vec<String>>), E> {
        let (app, noun) = (self.app, self.noun);
        let not_ours = |what: String| E::from(FormatError::Invalid(format!("not a {app} {noun} ({what})")));
        let Some(obj) = v.as_object() else { return Err(not_ours("expected a JSON object".into())) };
        let version = match obj.get("format") {
            None => self.untagged.ok_or_else(|| not_ours("no format".into()))?,
            Some(Value::String(f)) => self.version_of(f).ok_or_else(|| not_ours(format!("format `{f}`")))?,
            Some(_) => return Err(not_ours("format is not a string".into())),
        };
        let applied = take_names(v, "format_pending", noun).map_err(E::from)?;
        if version == 0 {
            return Err(not_ours("format 0".into()));
        }
        Ok((version, applied))
    }

    /// Bring a file of format `version` up to this build's; `applied` is its `format_pending`
    /// (both from [`Family::take_version`]): the changes of its format it already has, when it
    /// was written while they were pending. The others' steps run first, and already-applied
    /// changes never run again at their later number. A file past this build's number has no
    /// steps to run.
    pub fn upgrade(&self, v: &mut Value, version: u32, applied: Option<&[String]>) -> Result<(), E> {
        let formats = &self.formats;
        if version == 0 {
            return Err(E::from(FormatError::Invalid(format!("not a {} {} (format 0)", self.app, self.noun))));
        }
        // A list beside a format that isn't made of changes is ignored, like any field an upgrade
        // drops.
        let applied = applied.filter(|_| formats.parts(version).is_some());
        let already_applied =
            |part: &Part<E>| applied.is_some_and(|names| names.iter().any(|name| self.canonical_applied_name(version, name) == part.name));
        if applied.is_some() {
            for part in formats.parts(version).unwrap_or_default().iter().filter(|part| !already_applied(part)) {
                part.step.map_or(Ok(()), |step| step(v))?;
            }
        }
        for from in version..self.current() {
            match formats.hand.get(from as usize - 1) {
                Some(step) => step(v)?,
                None => {
                    for part in formats.parts(from + 1).unwrap_or_default().iter().filter(|part| !already_applied(part)) {
                        part.step.map_or(Ok(()), |step| step(v))?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Take a short list of names out of a file's JSON: its `format_pending` (the pending changes it
/// has).
fn take_names(v: &mut Value, key: &str, noun: &str) -> Result<Option<Vec<String>>, FormatError> {
    let Some(list) = v.as_object_mut().and_then(|o| o.remove(key)) else { return Ok(None) };
    let invalid = || FormatError::Invalid(format!("damaged {noun}: `{key}` is not a short list of names"));
    let list = list.as_array().filter(|l| l.len() <= 1000).ok_or_else(invalid)?;
    list.iter().map(|n| n.as_str().filter(|n| n.len() <= 128).map(str::to_string).ok_or_else(invalid)).collect::<Result<_, _>>().map(Some)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
