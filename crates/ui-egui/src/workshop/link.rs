//! The `solidcraft://` link a Workshop page's Open in SolidCraft button carries
//! (workshop docs/link-scheme.md):
//!
//! ```text
//! solidcraft://workshop/<kind>/<owner>/<name>[@<major.minor.patch>]
//! solidcraft://workshop/<kind>/<id>[@<major.minor.patch>]        (the first format)
//! ```
//!
//! `kind` is `model`, `plugin` or `part` (`library` is the same as `part`). Owner, name and id are
//! the site's address segments: lowercase letters, digits, dots and dashes, starting and ending
//! with a letter or digit, at most 128 characters. One trailing `/` is allowed. Anything else,
//! a query, a fragment, percent escapes, other hosts or schemes, is refused: the link names an
//! item only, never a file or an address to fetch.

/// The longest link read, in bytes.
pub const MAX_LINK: usize = 512;
/// The scheme and host every link starts with.
const PREFIX: &str = "solidcraft://workshop/";
/// The longest address segment.
const MAX_SEGMENT: usize = 128;

/// What a link opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A design, opened like File › Open.
    Model,
    /// A plugin package, installed through Install plugin….
    Plugin,
    /// A parts library from the solidcraft-parts index (`library` in the Workshop index).
    Part,
}

impl Kind {
    /// The kind as the signed index spells it.
    pub fn index_kind(self) -> &'static str {
        match self {
            Kind::Model => "model",
            Kind::Plugin => "plugin",
            Kind::Part => "library",
        }
    }

    fn from_link(kind: &str) -> Option<Self> {
        match kind {
            "model" => Some(Kind::Model),
            "plugin" => Some(Kind::Plugin),
            "part" | "library" => Some(Kind::Part),
            _ => None,
        }
    }
}

/// Which item a link names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// `<owner>/<name>`: the item's address on the site.
    Address { owner: String, slug: String },
    /// `<id>`: the stable id of the first link format.
    Id(String),
}

/// A parsed Workshop link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub kind: Kind,
    pub target: Target,
    /// `None`: the newest version this SolidCraft can run.
    pub version: Option<semver::Version>,
}

impl Link {
    /// The link as the site writes it, for messages.
    pub fn shown(&self) -> String {
        let kind = match self.kind {
            Kind::Model => "model",
            Kind::Plugin => "plugin",
            Kind::Part => "part",
        };
        let target = match &self.target {
            Target::Address { owner, slug } => format!("{owner}/{slug}"),
            Target::Id(id) => id.clone(),
        };
        match &self.version {
            Some(v) => format!("{kind}/{target}@{v}"),
            None => format!("{kind}/{target}"),
        }
    }
}

/// Whether `text` could be a Workshop link at all: the cheap check the single-instance channel and
/// the command line use before handing it on. [`parse`] decides.
pub fn looks_like_link(text: &str) -> bool {
    text.len() <= MAX_LINK && text.get(..PREFIX.len()).is_some_and(|p| p.eq_ignore_ascii_case(PREFIX))
}

/// Parse a link strictly; `None` for anything that is not exactly a Workshop link.
pub fn parse(text: &str) -> Option<Link> {
    if text.len() > MAX_LINK || !text.is_ascii() {
        return None;
    }
    let rest = text.strip_prefix(PREFIX)?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let (path, version) = match rest.split_once('@') {
        Some((path, version)) => (path, Some(parse_version(version)?)),
        None => (rest, None),
    };
    let mut parts = path.split('/');
    let kind = Kind::from_link(parts.next()?)?;
    let first = parts.next().filter(|s| segment(s))?;
    let target = match parts.next() {
        None => Target::Id(first.to_string()),
        Some(slug) if segment(slug) => Target::Address { owner: first.to_string(), slug: slug.to_string() },
        Some(_) => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(Link { kind, target, version })
}

/// `major.minor.patch`, digits only (no pre-release or build part).
fn parse_version(text: &str) -> Option<semver::Version> {
    let mut numbers = text.split('.');
    let mut next = || numbers.next().filter(|n| !n.is_empty() && n.len() <= 9 && n.bytes().all(|b| b.is_ascii_digit()));
    let (major, minor, patch) = (next()?, next()?, next()?);
    if numbers.next().is_some() {
        return None;
    }
    Some(semver::Version::new(major.parse().ok()?, minor.parse().ok()?, patch.parse().ok()?))
}

/// An address segment: `[a-z0-9]([a-z0-9.-]{0,126}[a-z0-9])?`.
pub(crate) fn segment(s: &str) -> bool {
    let bytes = s.as_bytes();
    let edge = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    !bytes.is_empty()
        && bytes.len() <= MAX_SEGMENT
        && bytes.first().is_some_and(edge)
        && bytes.last().is_some_and(edge)
        && bytes.iter().all(|b| edge(b) || *b == b'.' || *b == b'-')
}
