//! The Workshop's signed index (workshop docs/index-format.md): checking it and finding the item a
//! link names in it.
//!
//! The index is trusted only when its Ed25519 signature, taken from the same response's
//! `X-Workshop-Signature` header, verifies with a key built into SolidCraft. Its sequence must not
//! go below one seen before (an old snapshot replayed to hide a revocation is refused), and an
//! expired snapshot is refused. Items that are delisted, or versions revoked with `block`, are
//! refused with the Workshop's reason.

use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::link::{Kind, Link, Target};

/// The index's address.
pub const INDEX_URL: &str = "https://workshop.solidcraft.io/api/index.json";
/// The only host the index and its files come from (parts libraries also from GitHub releases).
pub const HOST: &str = "workshop.solidcraft.io";
const FORMAT: &str = "solidcraft-workshop-index/1";
const ISSUER: &str = "https://workshop.solidcraft.io";

/// The keys the Workshop signs with: key id (the first 16 hex characters of sha256(key)) and the
/// raw 32-byte public key. A rotation ships the new key next to the old one.
pub const KEYS: &[(&str, &str)] = &[("5247b5af3f50dbc4", "11a01a9b2e249ae4fd1d707cdbd7f34371e3206aad5c17a8c78e8c8e1dd1ed09")];

/// The largest file each kind downloads.
const MAX_DESIGN: u64 = 100 << 20;
const MAX_PACKAGE: u64 = 100 << 20;
const MAX_LIBRARY: u64 = crate::update::install::MAX_PACKAGE;

/// Why a link opens nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not a Workshop link.
    BadLink,
    /// The index couldn't be fetched.
    Unreachable(String),
    /// No signature, an unknown key or a signature that does not verify.
    Untrusted,
    /// The index isn't one SolidCraft reads (format, issuer, JSON).
    BadIndex(String),
    /// A lower sequence than one already seen.
    Replayed { seen: u64, got: u64 },
    /// The snapshot is past its `expires_at`.
    Expired,
    /// Nothing in the index has this address or id.
    NotListed(String),
    /// The item was delisted.
    Delisted(Option<String>),
    /// The version (or every version) was revoked with `block`.
    Revoked(Option<String>),
    /// The item has no such version.
    NoSuchVersion(String),
    /// Every version (or the one asked for) needs a newer SolidCraft.
    NeedsNewerApp(String),
    /// The item's entry can't be used (no file, a file from elsewhere, too large).
    BadItem(String),
    /// The download failed.
    Download(String),
    /// The download doesn't match the pinned sha256.
    Checksum,
    /// A parts library zip holds something a library may not.
    LibraryContents(String),
}

#[derive(Clone, Debug, Deserialize)]
pub struct Index {
    pub format: String,
    pub issuer: String,
    pub key_id: String,
    pub sequence: u64,
    #[serde(default)]
    pub issued_at: String,
    pub expires_at: String,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub delisted: Vec<Delisted>,
    #[serde(default)]
    pub revoked: Vec<Revoked>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub author: Author,
    #[serde(default)]
    pub licence: String,
    #[serde(default)]
    pub versions: Vec<Version>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Author {
    #[serde(default)]
    pub handle: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Version {
    pub version: String,
    #[serde(default)]
    pub min_app_version: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub files: Vec<File>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct File {
    pub name: String,
    pub role: String,
    pub size: u64,
    pub sha256: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Delisted {
    pub id: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Revoked {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub sha256: Vec<String>,
}

/// The tier badge, the same three words the Workshop shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Official,
    Community,
    /// Also any tier this SolidCraft doesn't know: never shown as more trusted than it is.
    Unverified,
}

impl Tier {
    fn from_index(tier: &str) -> Self {
        match tier {
            "official" => Tier::Official,
            "community" => Tier::Community,
            _ => Tier::Unverified,
        }
    }
}

/// What a link resolved to: everything the question shows and the one file to download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub kind: Kind,
    pub id: String,
    pub owner: String,
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub author: String,
    pub tier: Tier,
    pub licence: String,
    pub version: semver::Version,
    pub permissions: Vec<String>,
    pub file: File,
    /// A `warn` revocation's reason: shown with the question.
    pub warning: Option<String>,
}

fn hex(text: &str, len: usize) -> Option<Vec<u8>> {
    if text.len() != len * 2 || !text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    (0..len).map(|i| text.get(i * 2..i * 2 + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok())).collect()
}

/// The key id of a public key: the first 16 hex characters of its sha256.
pub fn key_id(key: &[u8; 32]) -> String {
    Sha256::digest(key).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Check `body`'s signature with the key `key_id` names among `keys`, then read it. Nothing about
/// the body is looked at before the signature verifies.
pub fn verify_with(keys: &[(&str, &str)], body: &[u8], signature: Option<&str>, key_id: Option<&str>) -> Result<Index, Refusal> {
    let (Some(signature), Some(key_id)) = (signature, key_id) else { return Err(Refusal::Untrusted) };
    let (signature, key_id) = (signature.trim(), key_id.trim());
    let key = keys.iter().find(|(id, _)| *id == key_id).and_then(|(_, key)| hex(key, 32)).ok_or(Refusal::Untrusted)?;
    let key: [u8; 32] = key.try_into().map_err(|_| Refusal::Untrusted)?;
    let key = VerifyingKey::from_bytes(&key).map_err(|_| Refusal::Untrusted)?;
    let signature: [u8; 64] = hex(signature, 64).and_then(|s| s.try_into().ok()).ok_or(Refusal::Untrusted)?;
    key.verify_strict(body, &Signature::from_bytes(&signature)).map_err(|_| Refusal::Untrusted)?;
    let index: Index = serde_json::from_slice(body).map_err(|e| Refusal::BadIndex(e.to_string()))?;
    if index.format != FORMAT {
        return Err(Refusal::BadIndex(format!("format {}", index.format)));
    }
    if index.issuer != ISSUER {
        return Err(Refusal::BadIndex(format!("issuer {}", index.issuer)));
    }
    // The body names the key it was signed with: a body signed with one key can't be passed off
    // under another's id.
    if index.key_id != key_id {
        return Err(Refusal::Untrusted);
    }
    Ok(index)
}

/// [`verify_with`] with the keys built into SolidCraft.
pub fn verify(body: &[u8], signature: Option<&str>, key_id: Option<&str>) -> Result<Index, Refusal> {
    verify_with(KEYS, body, signature, key_id)
}

/// The freshness rules: `seen` is the highest sequence verified before; `now` is in Unix seconds.
pub fn check_fresh(index: &Index, seen: Option<u64>, now: i64) -> Result<(), Refusal> {
    if let Some(seen) = seen
        && index.sequence < seen
    {
        return Err(Refusal::Replayed { seen, got: index.sequence });
    }
    let expires = unix_seconds(&index.expires_at).ok_or_else(|| Refusal::BadIndex(format!("expires_at {}", index.expires_at)))?;
    if now >= expires {
        return Err(Refusal::Expired);
    }
    Ok(())
}

/// `YYYY-MM-DDTHH:MM:SS[.fff]Z` as Unix seconds.
pub fn unix_seconds(text: &str) -> Option<i64> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let time = time.split_once('.').map_or(time, |(whole, _)| whole);
    let mut d = date.split('-').map(|n| n.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|n| n.parse::<i64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next()??);
    if d.next().is_some() || t.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&day) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// A version as SolidCraft compares it: `0.48.0-nightly…` runs what 0.48.0 runs.
fn release_of(version: &semver::Version) -> semver::Version {
    semver::Version::new(version.major, version.minor, version.patch)
}

fn runs_on(min: Option<&str>, app: &semver::Version) -> Result<(), Refusal> {
    let Some(min) = min.filter(|m| !m.is_empty()) else { return Ok(()) };
    let needed = crate::update::parse_version(min).ok_or_else(|| Refusal::BadItem(format!("min_app_version {min}")))?;
    if release_of(&needed) > release_of(app) { Err(Refusal::NeedsNewerApp(release_of(&needed).to_string())) } else { Ok(()) }
}

/// The revocation of `severity` covering `id` at `version`: a record without a version covers
/// every version; one with a version covers the same version as semver reads it (`1.2.0`,
/// `v1.2.0` and `1.2.0+build` are one version). A record whose version doesn't parse covers every
/// version, so a mistyped `block` blocks rather than lets through. `version` `None` asks for a
/// record that covers every version.
fn revocation<'a>(index: &'a Index, id: &str, version: Option<&semver::Version>, severity: &str) -> Option<&'a Revoked> {
    index.revoked.iter().find(|r| {
        r.id == id
            && r.severity == severity
            && match (r.version.as_deref().filter(|v| !v.trim().is_empty()), version) {
                (None, _) => true,
                (Some(listed), None) => crate::update::parse_version(listed).is_none(),
                (Some(listed), Some(version)) => {
                    crate::update::parse_version(listed).is_none_or(|listed| listed.cmp_precedence(version) == std::cmp::Ordering::Equal)
                }
            }
    })
}

/// Whether the file can come from `url`: the Workshop, or a parts library's GitHub release.
fn allowed_file_url(kind: Kind, url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    if url.len() > 2048 || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host == HOST {
        return true;
    }
    // https://github.com/<owner>/<repo>/releases/download/<tag>/<asset>
    let parts: Vec<&str> = path.split('/').collect();
    kind == Kind::Part && host == "github.com" && parts.len() == 6 && parts.get(2..4) == Some(&["releases", "download"][..])
}

/// The item `link` names, at the version it asks for (or the newest that runs on `app`).
pub fn resolve(index: &Index, link: &Link, app: &semver::Version) -> Result<Resolved, Refusal> {
    let kind = link.kind.index_kind();
    if let Target::Id(id) = &link.target
        && let Some(gone) = index.delisted.iter().find(|d| &d.id == id && d.kind.as_deref().is_none_or(|k| k == kind))
    {
        return Err(Refusal::Delisted(gone.reason.clone()));
    }
    let item = index
        .items
        .iter()
        .find(|item| {
            item.kind == kind
                && match &link.target {
                    Target::Address { owner, slug } => &item.owner == owner && &item.slug == slug,
                    Target::Id(id) => &item.id == id,
                }
        })
        .ok_or_else(|| Refusal::NotListed(link.shown()))?;
    if let Some(gone) = index.delisted.iter().find(|d| d.id == item.id) {
        return Err(Refusal::Delisted(gone.reason.clone()));
    }
    let mut versions: Vec<(semver::Version, &Version)> =
        item.versions.iter().filter_map(|v| crate::update::parse_version(&v.version).map(|parsed| (parsed, v))).collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    let (version, entry) = match &link.version {
        Some(wanted) => {
            if let Some(revoked) = revocation(index, &item.id, Some(wanted), "block") {
                return Err(Refusal::Revoked(revoked.reason.clone()));
            }
            let (version, entry) = versions.iter().find(|(v, _)| v == wanted).ok_or_else(|| Refusal::NoSuchVersion(wanted.to_string()))?;
            runs_on(entry.min_app_version.as_deref(), app)?;
            (version.clone(), *entry)
        }
        None => {
            let usable = versions.iter().filter(|(v, _)| revocation(index, &item.id, Some(v), "block").is_none()).collect::<Vec<_>>();
            if usable.is_empty() {
                return Err(match revocation(index, &item.id, None, "block") {
                    Some(revoked) => Refusal::Revoked(revoked.reason.clone()),
                    None => Refusal::NoSuchVersion(String::new()),
                });
            }
            let runs = usable.iter().find(|(_, entry)| runs_on(entry.min_app_version.as_deref(), app).is_ok());
            match runs {
                Some((version, entry)) => (version.clone(), *entry),
                None => {
                    let oldest = usable.last().map(|(_, e)| e.min_app_version.as_deref());
                    return Err(runs_on(oldest.flatten(), app).err().unwrap_or_else(|| Refusal::NoSuchVersion(String::new())));
                }
            }
        }
    };
    let role = match link.kind {
        Kind::Model => "design",
        Kind::Plugin => "package",
        Kind::Part => "library",
    };
    let mut files = entry.files.iter().filter(|f| f.role == role);
    let (Some(file), None) = (files.next(), files.next()) else {
        return Err(Refusal::BadItem(format!("version {version} has no single {role} file")));
    };
    if hex(&file.sha256, 32).is_none() {
        return Err(Refusal::BadItem(format!("the checksum of {}", file.name)));
    }
    if !allowed_file_url(link.kind, &file.url) {
        return Err(Refusal::BadItem(format!("{} comes from {}", file.name, file.url)));
    }
    let limit = match link.kind {
        Kind::Model => MAX_DESIGN,
        Kind::Plugin => MAX_PACKAGE,
        Kind::Part => MAX_LIBRARY,
    };
    if file.size == 0 || file.size > limit {
        return Err(Refusal::BadItem(format!("{} is {} bytes", file.name, file.size)));
    }
    // A revoked file's content, listed under any item, is never used.
    if index.revoked.iter().any(|r| r.severity == "block" && r.sha256.iter().any(|h| h.eq_ignore_ascii_case(&file.sha256))) {
        return Err(Refusal::Revoked(None));
    }
    let warning = revocation(index, &item.id, Some(&version), "warn").map(|r| r.reason.clone().unwrap_or_default());
    let author = match item.author.name.as_deref().filter(|n| !n.trim().is_empty()) {
        Some(name) if !item.author.handle.is_empty() => format!("{name} (@{})", item.author.handle),
        Some(name) => name.to_string(),
        None if !item.author.handle.is_empty() => format!("@{}", item.author.handle),
        None => item.owner.clone(),
    };
    Ok(Resolved {
        kind: link.kind,
        id: item.id.clone(),
        owner: item.owner.clone(),
        slug: item.slug.clone(),
        name: item.name.clone(),
        summary: item.summary.clone(),
        author,
        tier: Tier::from_index(&item.tier),
        licence: item.licence.clone(),
        version,
        permissions: entry.permissions.clone(),
        file: file.clone(),
        warning,
    })
}
