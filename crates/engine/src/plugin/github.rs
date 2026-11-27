//! GitHub release packages shared by URL installation and the plugin updater.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum FetchError {
    #[error("GitHub's rate limit was reached; try again in {} min", retry_after_s.div_ceil(60))]
    RateLimited { retry_after_s: u64 },
    #[error("{0}")]
    Failed(String),
}
pub type HttpGet = Arc<dyn Fn(&str) -> Result<Vec<u8>, FetchError> + Send + Sync>;
pub type HttpDownload = Arc<dyn Fn(&str, &Path, &dyn Fn(u64, Option<u64>) -> bool) -> Result<(), FetchError> + Send + Sync>;

/// Host HTTP services. Stored only in trusted session state, never in a plugin or design.
#[derive(Clone)]
pub struct Client {
    pub get: HttpGet,
    pub download: HttpDownload,
}
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}
impl PartialEq for Client {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.get, &other.get) && Arc::ptr_eq(&self.download, &other.download)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Default for Client {
    fn default() -> Self {
        Self { get: Arc::new(super::github_http::get), download: Arc::new(super::github_http::download) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub version: String,
    pub url: String,
    /// The asset's sha256 where the host lists one (GitHub); `None` for GitLab and Codeberg.
    pub sha256: Option<String>,
    /// The `owner/repo` it came from.
    pub repo: String,
}

/// The hosts a package may come from. Anything else is refused with a typed error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    GitHub,
    GitLab,
    Codeberg,
}
impl Host {
    /// The host's web name, as in its repository and release download URLs.
    pub fn web(self) -> &'static str {
        match self {
            Host::GitHub => "github.com",
            Host::GitLab => "gitlab.com",
            Host::Codeberg => "codeberg.org",
        }
    }
    fn from_web(name: &str) -> Option<Self> {
        [Host::GitHub, Host::GitLab, Host::Codeberg].into_iter().find(|host| host.web() == name)
    }
}

/// A repository on a supported host, optionally naming a release tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub host: Host,
    pub repository: String,
    pub tag: Option<String>,
}

fn safe_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag != "."
        && tag != ".."
        && tag.len() <= 200
        && tag.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
}

/// A GitLab project path: two to five names, since a project may sit in subgroups.
fn valid_gitlab_path(path: &str) -> bool {
    let names: Vec<&str> = path.split('/').collect();
    (2..=5).contains(&names.len())
        && names.iter().all(|name| {
            (1..=100).contains(&name.len())
                && *name != "."
                && *name != ".."
                && !name.starts_with('-')
                && name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        })
}

impl Source {
    /// `owner/repo[@ref]` (GitHub), or an https repository or release URL on GitHub, GitLab or Codeberg.
    pub fn parse(url: &str) -> Result<Self, craft_plugin::HostError> {
        let invalid = || {
            craft_plugin::HostError::InvalidInput {
            field: "url".into(),
            reason: "use owner/repo[@ref], or https://github.com/owner/repo, https://gitlab.com/owner/repo or https://codeberg.org/owner/repo, optionally /releases/tag/tag or /tree/tag".into(),
        }
        };
        if url.len() > 512 {
            return Err(invalid());
        }
        let text = url.trim();
        let Some(tail) = text.strip_prefix("https://") else {
            return Self::shorthand(text).ok_or_else(invalid);
        };
        let (host, path) = tail.split_once('/').ok_or_else(invalid)?;
        let host_kind = Host::from_web(host).ok_or_else(invalid)?;
        let segments: Vec<&str> = path.trim_end_matches('/').split('/').collect();
        let no_rest: &[&str] = &[];
        let (name_segments, rest): (&[&str], &[&str]) = match host_kind {
            Host::GitLab => match segments.iter().position(|segment| *segment == "-") {
                Some(at) => (segments.get(..at).unwrap_or_default(), segments.get(at + 1..).unwrap_or_default()),
                None => (segments.as_slice(), no_rest),
            },
            _ => (segments.get(..2).unwrap_or_default(), segments.get(2..).unwrap_or_default()),
        };
        let joined = name_segments.join("/");
        let repository = joined.strip_suffix(".git").unwrap_or(&joined).to_string();
        let name_ok = match host_kind {
            Host::GitLab => valid_gitlab_path(&repository),
            _ => craft_plugin::valid_github_repo(&repository),
        };
        if !name_ok {
            return Err(invalid());
        }
        let tag = match (host_kind, rest) {
            (_, []) => None,
            (Host::GitHub, ["releases", "latest"]) | (Host::Codeberg, ["releases", "latest"]) => None,
            (Host::GitHub, ["releases", "tag", tag])
            | (Host::GitHub, ["tree", tag])
            | (Host::Codeberg, ["releases", "tag", tag])
            | (Host::Codeberg, ["src", "tag", tag])
                if safe_tag(tag) =>
            {
                Some((*tag).to_string())
            }
            (Host::GitLab, ["releases"]) | (Host::GitLab, ["releases", "permalink", "latest"]) => None,
            (Host::GitLab, ["releases", tag]) | (Host::GitLab, ["tags", tag]) | (Host::GitLab, ["tree", tag]) if safe_tag(tag) => {
                Some((*tag).to_string())
            }
            _ => return Err(invalid()),
        };
        Ok(Self { host: host_kind, repository, tag })
    }
    fn shorthand(text: &str) -> Option<Self> {
        let (repo, tag) = match text.split_once('@') {
            Some((repo, tag)) => (repo, Some(tag)),
            None => (text, None),
        };
        let repository = repo.strip_suffix(".git").unwrap_or(repo);
        let tag = match tag {
            Some(tag) if safe_tag(tag) => Some(tag.to_string()),
            Some(_) => return None,
            None => None,
        };
        craft_plugin::valid_github_repo(repository).then(|| Self { host: Host::GitHub, repository: repository.to_string(), tag })
    }
    /// The host's release API request: the latest release, or the one for the named tag.
    pub fn release_url(&self) -> String {
        let repo = &self.repository;
        match (self.host, &self.tag) {
            (Host::GitHub, Some(tag)) => format!("https://api.github.com/repos/{repo}/releases/tags/{tag}"),
            (Host::GitHub, None) => latest_url(repo),
            (Host::Codeberg, Some(tag)) => format!("https://codeberg.org/api/v1/repos/{repo}/releases/tags/{tag}"),
            (Host::Codeberg, None) => format!("https://codeberg.org/api/v1/repos/{repo}/releases/latest"),
            (Host::GitLab, tag) => {
                let project = repo.replace('/', "%2F");
                match tag {
                    Some(tag) => format!("https://gitlab.com/api/v4/projects/{project}/releases/{tag}"),
                    None => format!("https://gitlab.com/api/v4/projects/{project}/releases/permalink/latest"),
                }
            }
        }
    }
    /// `host/owner/repo`, as shown in a package review.
    pub fn display(&self) -> String {
        format!("{}/{}", self.host.web(), self.repository)
    }
}

pub fn latest_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases/latest")
}

pub fn parse_release(json: &[u8], repo: &str, id: &str) -> Result<Offer, FetchError> {
    release(Host::GitHub, json, repo, Some(id))
}

/// Before the package is inspected its id is unknown: require exactly one release package.
pub fn package_release(host: Host, json: &[u8], repo: &str) -> Result<Offer, FetchError> {
    release(host, json, repo, None)
}

/// A release asset's name and download URL, as the host lists it.
fn release_assets(host: Host, v: &Value) -> Option<Vec<(String, String)>> {
    let text = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).map(str::to_string);
    let list = match host {
        Host::GitLab => v.get("assets").and_then(|assets| assets.get("links")),
        _ => v.get("assets"),
    }
    .and_then(Value::as_array)
    .filter(|a| a.len() <= 400)?;
    Some(
        list.iter()
            .filter_map(|item| match host {
                // A GitLab link's label is free text; the file name is the end of its address.
                Host::GitLab => {
                    let url = text(item, "direct_asset_url").or_else(|| text(item, "url"))?;
                    let name = url.rsplit('/').next()?.to_string();
                    Some((name, url))
                }
                _ => Some((text(item, "name")?, text(item, "browser_download_url")?)),
            })
            .collect(),
    )
}

fn release(host: Host, json: &[u8], repo: &str, id: Option<&str>) -> Result<Offer, FetchError> {
    let failed = |s: &str| FetchError::Failed(s.into());
    let v: Value = serde_json::from_slice(json).map_err(|e| FetchError::Failed(e.to_string()))?;
    let flag = |key: &str| v.get(key).and_then(Value::as_bool).unwrap_or(false);
    if flag("draft") || flag("prerelease") || flag("upcoming_release") {
        return Err(failed("the latest release is a prerelease"));
    }
    let tag = v.get("tag_name").and_then(Value::as_str).unwrap_or_default().trim();
    if !semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).is_ok_and(|version| version.pre.is_empty()) {
        return Err(failed("the latest release's tag isn't a version"));
    }
    let version = tag.trim_start_matches('v').to_string();
    let names = id.map(|id| [format!("{id}.zip"), format!("{id}-{version}.zip")]);
    // Where this host serves a release's files from; the tag and file name follow.
    let (prefix, middle) = match host {
        Host::GitLab => (format!("https://gitlab.com/{repo}/-/releases/").to_ascii_lowercase(), "downloads/"),
        _ => (format!("https://{}/{repo}/releases/download/", host.web()).to_ascii_lowercase(), ""),
    };
    let assets = release_assets(host, &v).ok_or_else(|| failed("the release has no bounded asset list"))?;
    let mut candidates = assets.iter().filter_map(|(name, url)| {
        if !url.to_ascii_lowercase().starts_with(&prefix) {
            return None;
        }
        let (release_tag, rest) = url.get(prefix.len()..)?.split_once('/')?;
        let filename = rest.strip_prefix(middle)?;
        let safe_segment = |segment: &str| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'+'))
        };
        (names.as_ref().map_or_else(|| name.ends_with(".zip"), |names| names.iter().any(|n| n == name))
            && release_tag == tag
            && filename == name
            && safe_segment(release_tag)
            && safe_segment(filename))
        .then_some(url)
    });
    let missing = || {
        FetchError::Failed(
            id.map_or_else(|| "the release must contain exactly one plugin .zip".into(), |id| format!("the latest release has no {id}.zip")),
        )
    };
    let url = candidates.next().ok_or_else(missing)?;
    if id.is_none() && candidates.next().is_some() {
        return Err(missing());
    }
    // GitHub lists each asset's sha256 digest; GitLab and Codeberg publish none, so a package from
    // them is bound by the archive hash the person reviewed instead.
    let sha256 = match host {
        Host::GitHub => Some(
            v.get("assets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|asset| asset.get("browser_download_url").and_then(Value::as_str) == Some(url.as_str()))
                .and_then(|asset| asset.get("digest"))
                .and_then(Value::as_str)
                .and_then(|s| s.strip_prefix("sha256:"))
                .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| {
                    FetchError::Failed(format!(
                        "the latest release lists no sha256 digest for {}",
                        url.rsplit('/').next().unwrap_or("the plugin package")
                    ))
                })?,
        ),
        Host::GitLab | Host::Codeberg => None,
    };
    Ok(Offer { version, url: url.clone(), sha256, repo: repo.to_string() })
}

/// The updater and installer use the same bounded download and digest check.
pub fn fetch(download: &HttpDownload, offer: &Offer, path: PathBuf) -> Result<PathBuf, String> {
    let fetched = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .map_err(|e| e.to_string())
        .and_then(|()| {
            download(&offer.url, &path, &|done, total| {
                done <= super::install::MAX_ZIP_BYTES as u64 && total.is_none_or(|n| n <= super::install::MAX_ZIP_BYTES as u64)
            })
            .map_err(|e| e.to_string())
        })
        .and_then(|()| {
            use sha2::Digest;
            use std::io::Read;
            let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            if file.metadata().map_err(|e| e.to_string())?.len() > super::install::MAX_ZIP_BYTES as u64 {
                return Err("the plugin archive exceeds 80 MiB".into());
            }
            let mut hash = sha2::Sha256::new();
            let mut bytes = [0u8; 65536];
            let mut reader = file.take(super::install::MAX_ZIP_BYTES as u64 + 1);
            loop {
                let n = reader.read(&mut bytes).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                hash.update(bytes.get(..n).unwrap_or_default());
            }
            if offer.sha256.as_ref().is_none_or(|expected| format!("{:x}", hash.finalize()) == *expected) {
                Ok(())
            } else {
                Err("the download does not match the release's checksum".into())
            }
        });
    match fetched {
        Ok(()) => Ok(path),
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            Err(error)
        }
    }
}

/// Fetch into a unique temporary directory. Tests supply these same HTTP boundaries without a network.
#[cfg(not(target_arch = "wasm32"))]
pub fn package(source: &Source, get: &HttpGet, download: &HttpDownload) -> Result<(Vec<u8>, Offer), String> {
    let offer =
        package_release(source.host, &get(&source.release_url()).map_err(|e| e.to_string())?, &source.repository).map_err(|e| e.to_string())?;
    let stamp = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let staging = std::env::temp_dir().join(format!("solidcraft-plugin-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = fetch(download, &offer, staging.join("package.zip")).and_then(|path| {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(super::install::MAX_ZIP_BYTES as u64 + 1).read_to_end(&mut bytes))
            .map_err(|e| e.to_string())?;
        if bytes.len() > super::install::MAX_ZIP_BYTES {
            return Err("the plugin archive exceeds 80 MiB".into());
        }
        Ok((bytes, offer))
    });
    let _ = std::fs::remove_dir_all(staging);
    result
}
