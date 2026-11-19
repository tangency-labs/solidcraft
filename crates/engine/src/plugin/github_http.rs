//! Native GitHub HTTPS requests shared by plugin installation and desktop updates.

use super::github::FetchError;

/// Maximum size of a desktop update package (one GiB).
pub const MAX_PACKAGE: u64 = 1 << 30;

/// HTTPS GET for the update check (docs/updates.md): certificates from the operating system, a
/// 15 s limit, at most 1 MB, and no headers beyond what GitHub requires (a User-Agent without
/// the version or anything about this machine, and the API's media type).
pub fn get(url: &str) -> Result<Vec<u8>, FetchError> {
    let mut response = call(url, std::time::Duration::from_secs(15))?;
    response.body_mut().with_config().limit(1 << 20).read_to_vec().map_err(|e| FetchError::Failed(e.to_string()))
}

/// HTTPS download of an update package to `to` (docs/updates.md), reporting progress; stops when
/// `progress` returns false. The caller verifies the file.
pub fn download(url: &str, to: &std::path::Path, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<(), FetchError> {
    use std::io::{Read, Write};
    let fail = |e: String| FetchError::Failed(e);
    let mut response = call(url, std::time::Duration::from_secs(30 * 60))?;
    let total = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    if !progress(0, total) {
        return Err(fail("cancelled".into()));
    }
    let mut body = response.body_mut().with_config().limit(MAX_PACKAGE).reader();
    let mut file = std::fs::File::create(to).map_err(|e| fail(format!("{}: {e}", to.display())))?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = body.read(&mut buf).map_err(|e| fail(e.to_string()))?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if !progress(done, total) {
            return Err(fail("cancelled".into()));
        }
        file.write_all(buf.get(..n).unwrap_or_default()).map_err(|e| fail(format!("{}: {e}", to.display())))?;
    }
    file.sync_all().map_err(|e| fail(format!("{}: {e}", to.display())))?;
    Ok(())
}

/// One GET to GitHub: a 200 answer, or why not (rate limits included).
fn call(url: &str, timeout: std::time::Duration) -> Result<ureq::http::Response<ureq::Body>, FetchError> {
    use std::time::Duration;
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(true)
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .timeout_connect(Some(Duration::from_secs(5)))
        .user_agent("SolidCraft-update-check")
        .max_redirects(3)
        .build()
        .into();
    let response = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|e| FetchError::Failed(e.to_string()))?;
    let status = response.status().as_u16();
    let header = |k: &str| response.headers().get(k).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
    if status == 429 || (status == 403 && (header("x-ratelimit-remaining") == Some(0) || header("retry-after").is_some())) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let retry_after_s = header("retry-after").or_else(|| header("x-ratelimit-reset").map(|reset| reset.saturating_sub(now))).unwrap_or(3600);
        return Err(FetchError::RateLimited { retry_after_s });
    }
    if status != 200 {
        return Err(FetchError::Failed(format!("GitHub answered HTTP {status}")));
    }
    Ok(response)
}
