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
    /// The asset's sha256 as GitHub lists it.
    pub sha256: Option<String>,
    /// The `owner/repo` it came from.
    pub repo: String,
}

/// A GitHub repository, optionally naming a release tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
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

impl Source {
    /// `owner/repo[@ref]`, or an https repository or release URL on github.com.
    pub fn parse(url: &str) -> Result<Self, craft_plugin::HostError> {
        let invalid = || craft_plugin::HostError::InvalidInput {
            field: "url".into(),
            reason: "use owner/repo[@ref], or https://github.com/owner/repo, optionally /releases/tag/tag or /tree/tag".into(),
        };
        if url.len() > 512 {
            return Err(invalid());
        }
        let text = url.trim();
        let Some(tail) = text.strip_prefix("https://") else {
            return Self::shorthand(text).ok_or_else(invalid);
        };
        let (host, path) = tail.split_once('/').ok_or_else(invalid)?;
        if host != "github.com" {
            return Err(invalid());
        }
        let segments: Vec<&str> = path.trim_end_matches('/').split('/').collect();
        let joined = segments.get(..2).unwrap_or_default().join("/");
        let repository = joined.strip_suffix(".git").unwrap_or(&joined).to_string();
        if !craft_plugin::valid_github_repo(&repository) {
            return Err(invalid());
        }
        let tag = match segments.get(2..).unwrap_or_default() {
            [] | ["releases", "latest"] => None,
            ["releases", "tag", tag] | ["tree", tag] if safe_tag(tag) => Some((*tag).to_string()),
            _ => return Err(invalid()),
        };
        Ok(Self { repository, tag })
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
        craft_plugin::valid_github_repo(repository).then(|| Self { repository: repository.to_string(), tag })
    }
    /// The release API request: the latest release, or the one for the named tag.
    pub fn release_url(&self) -> String {
        match &self.tag {
            Some(tag) => format!("https://api.github.com/repos/{}/releases/tags/{tag}", self.repository),
            None => latest_url(&self.repository),
        }
    }
    /// `github.com/owner/repo`, as shown in a package review.
    pub fn display(&self) -> String {
        format!("github.com/{}", self.repository)
    }
}

pub fn latest_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases/latest")
}

pub fn parse_release(json: &[u8], repo: &str, id: &str) -> Result<Offer, FetchError> {
    release(json, repo, Some(id))
}

/// Before the package is inspected its id is unknown: require exactly one release package.
pub fn package_release(json: &[u8], repo: &str) -> Result<Offer, FetchError> {
    release(json, repo, None)
}

fn release(json: &[u8], repo: &str, id: Option<&str>) -> Result<Offer, FetchError> {
    let failed = |s: &str| FetchError::Failed(s.into());
    let v: Value = serde_json::from_slice(json).map_err(|e| FetchError::Failed(e.to_string()))?;
    let flag = |key: &str| v.get(key).and_then(Value::as_bool).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return Err(failed("the latest release is a prerelease"));
    }
    let tag = v.get("tag_name").and_then(Value::as_str).unwrap_or_default().trim();
    if !semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).is_ok_and(|version| version.pre.is_empty()) {
        return Err(failed("the latest release's tag isn't a version"));
    }
    let version = tag.trim_start_matches('v').to_string();
    let names = id.map(|id| [format!("{id}.zip"), format!("{id}-{version}.zip")]);
    let prefix = format!("https://github.com/{repo}/releases/download/").to_ascii_lowercase();
    let assets = v
        .get("assets")
        .and_then(Value::as_array)
        .filter(|a| a.len() <= 400)
        .ok_or_else(|| failed("the release has no bounded asset list"))?
        .iter()
        .filter_map(|item| {
            let text = |key: &str| item.get(key).and_then(Value::as_str).map(str::to_string);
            Some((text("name")?, text("browser_download_url")?))
        })
        .collect::<Vec<_>>();
    let mut candidates = assets.iter().filter_map(|(name, url)| {
        if !url.to_ascii_lowercase().starts_with(&prefix) {
            return None;
        }
        let (release_tag, filename) = url.get(prefix.len()..)?.split_once('/')?;
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
    // GitHub lists each asset's sha256 digest; the download must match it.
    let sha256 = v
        .get("assets")
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
            FetchError::Failed(format!("the latest release lists no sha256 digest for {}", url.rsplit('/').next().unwrap_or("the plugin package")))
        })?;
    Ok(Offer { version, url: url.clone(), sha256: Some(sha256), repo: repo.to_string() })
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
    let offer = package_release(&get(&source.release_url()).map_err(|e| e.to_string())?, &source.repository).map_err(|e| e.to_string())?;
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
