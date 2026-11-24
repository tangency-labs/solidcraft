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
    pub sha256: String,
}

/// An HTTPS GitHub repository, optionally naming a release tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub repository: String,
    pub tag: Option<String>,
}
impl Source {
    pub fn parse(url: &str) -> Result<Self, craft_plugin::HostError> {
        let invalid = || craft_plugin::HostError::InvalidInput {
            field: "url".into(),
            reason: "use https://github.com/owner/repo, optionally /releases/tag/tag or /tree/tag".into(),
        };
        if url.len() > 512 {
            return Err(invalid());
        }
        let tail = url.trim().strip_prefix("https://github.com/").ok_or_else(invalid)?.trim_end_matches('/');
        let mut parts = tail.split('/');
        let owner = parts.next().ok_or_else(invalid)?;
        let repo = parts.next().ok_or_else(invalid)?;
        let repo = repo.strip_suffix(".git").unwrap_or(repo);
        let repository = format!("{owner}/{repo}");
        if !craft_plugin::valid_github_repo(&repository) {
            return Err(invalid());
        }
        let rest: Vec<_> = parts.collect();
        let tag = match rest.as_slice() {
            [] | ["releases", "latest"] => None,
            ["releases", "tag", tag] | ["tree", tag]
                if !tag.is_empty()
                    && *tag != "."
                    && *tag != ".."
                    && tag.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.')) =>
            {
                Some((*tag).to_string())
            }
            _ => return Err(invalid()),
        };
        Ok(Self { repository, tag })
    }
    pub fn release_url(&self) -> String {
        match &self.tag {
            Some(tag) => format!("https://api.github.com/repos/{}/releases/tags/{tag}", self.repository),
            None => latest_url(&self.repository),
        }
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
    if v.get("draft").and_then(Value::as_bool).unwrap_or(false) || v.get("prerelease").and_then(Value::as_bool).unwrap_or(false) {
        return Err(failed("the latest release is a prerelease"));
    }
    let tag = v.get("tag_name").and_then(Value::as_str).unwrap_or_default().trim();
    if !semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).is_ok_and(|version| version.pre.is_empty()) {
        return Err(failed("the latest release's tag isn't a version"));
    }
    let version = tag.trim_start_matches('v').to_string();
    let names = id.map(|id| [format!("{id}.zip"), format!("{id}-{version}.zip")]);
    let prefix = format!("https://github.com/{repo}/releases/download/").to_ascii_lowercase();
    let assets =
        v.get("assets").and_then(Value::as_array).filter(|a| a.len() <= 400).ok_or_else(|| failed("the release has no bounded asset list"))?;
    let mut candidates = assets.iter().filter_map(|asset| {
        let name = asset.get("name")?.as_str()?;
        let url = asset.get("browser_download_url")?.as_str()?;
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
        .then_some((asset, url))
    });
    let missing = || {
        FetchError::Failed(
            id.map_or_else(|| "the release must contain exactly one plugin .zip".into(), |id| format!("the latest release has no {id}.zip")),
        )
    };
    let (asset, url) = candidates.next().ok_or_else(missing)?;
    if id.is_none() && candidates.next().is_some() {
        return Err(missing());
    }
    let sha256 = asset
        .get("digest")
        .and_then(Value::as_str)
        .and_then(|s| s.strip_prefix("sha256:"))
        .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| {
            FetchError::Failed(format!(
                "the latest release lists no sha256 digest for {}",
                asset.get("name").and_then(Value::as_str).unwrap_or("the plugin package")
            ))
        })?;
    Ok(Offer { version, url: url.to_string(), sha256 })
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
            if format!("{:x}", hash.finalize()) == offer.sha256 { Ok(()) } else { Err("the download does not match the release's checksum".into()) }
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
