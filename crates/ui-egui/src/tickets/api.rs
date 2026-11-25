//! The ticket server's HTTP API (tickets.solidcraft.io): every path, request and answer the app
//! uses, in this one module. The rest of the app speaks in [`Request`]s and parsed answers, so a
//! change on the server's side is a change here only.

use serde_json::{Value, json};

use crate::agent_chat::backend::HttpRequest as Request;

/// Where reports go.
pub const SERVER: &str = "https://tickets.solidcraft.io";
/// The client id the server knows the app by.
const CLIENT_ID: &str = "solidcraft";
/// The app's slug on the server, preselected in the report.
pub const DEFAULT_APP: &str = "solidcraft";

/// The server to talk to: [`SERVER`], or `SOLIDCRAFT_TICKETS_URL` on the desktop (a test server).
pub fn server() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(url) = std::env::var("SOLIDCRAFT_TICKETS_URL")
        && allowed_url(&url)
        && origin(&url).is_some_and(|o| o.path.is_empty() || o.path == "/")
    {
        return url.trim_end_matches('/').to_string();
    }
    SERVER.to_string()
}

/// A URL's parts the app checks: scheme, host (lower case), port and the path from its `/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
}

/// `url` taken apart; None for anything that isn't a plain `http(s)://host[:port][/path]` (a user
/// name, an empty host, a bad port).
pub fn origin(url: &str) -> Option<Origin> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') || h.ends_with(']') => (h, Some(p.parse::<u16>().ok()?)),
        _ => (authority, None),
    };
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':')) {
        return None;
    }
    Some(Origin { scheme, host: host.to_ascii_lowercase(), port, path: path.to_string() })
}

/// This computer, for a test server over plain HTTP.
fn local(host: &str) -> bool {
    host == "127.0.0.1" || host == "localhost"
}

/// Whether the app may send a request to `url`: HTTPS, or plain HTTP to this computer only.
pub fn allowed_url(url: &str) -> bool {
    origin(url).is_some_and(|o| o.scheme == "https" || local(&o.host))
}

/// Whether `url` is a page of the ticket server itself under `path` (a page the app opens in the
/// browser): the same scheme, host and port exactly, not a look-alike.
pub fn on_server(url: &str, path: &str) -> bool {
    match (origin(url), origin(&server())) {
        (Some(u), Some(s)) => u.scheme == s.scheme && u.host == s.host && u.port == s.port && u.path.starts_with(path),
        _ => false,
    }
}

fn request(method: &'static str, path: &str, token: Option<&str>, body: Option<Value>) -> Request {
    let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
    if let Some(token) = token {
        headers.push(("Authorization".into(), format!("Bearer {token}")));
    }
    if body.is_some() {
        headers.push(("Content-Type".into(), "application/json".into()));
    }
    Request { method, url: format!("{}{path}", server()), headers, body: body.map(|b| b.to_string().into_bytes()), timeout_s: 60 }
}

/// Why a call didn't give what was asked.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// No answer: offline, the server unreachable, a timeout. A report waits in the queue.
    Offline(String),
    /// The token the request went with is no longer good (a draft's, once the draft expired).
    SignedOut,
    /// The server answered with an error; its message.
    Refused(String),
}

impl Failure {
    pub fn text(&self) -> String {
        match self {
            Self::Offline(e) => format!("Could not reach {}: {e}", server()),
            Self::SignedOut => "The report's draft expired on the server. Send it again.".into(),
            Self::Refused(e) => e.clone(),
        }
    }
}

/// An answer's JSON, or why there isn't one.
pub fn answer(reply: Result<(u16, Vec<u8>), String>) -> Result<Value, Failure> {
    let (status, body) = reply.map_err(Failure::Offline)?;
    let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    match status {
        200..=299 => Ok(value),
        401 => Err(Failure::SignedOut),
        // Transient: a timeout, too many requests, the server or a gateway in trouble. Only a
        // refusal of the report itself (validation, permissions) is final.
        408 | 425 | 429 | 500..=599 => Err(Failure::Offline(format!("the server answered {status}"))),
        _ => {
            let message =
                value.get("message").or_else(|| value.get("error")).and_then(Value::as_str).unwrap_or("").chars().take(300).collect::<String>();
            Err(Failure::Refused(if message.is_empty() { format!("The server answered {status}.") } else { message }))
        }
    }
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(|s| s.chars().take(2000).collect())
}

// ---- report drafts (no account needed) --------------------------------------------------------

/// This computer's name, for the sign-in's name on the server; None where it isn't known.
pub fn hostname() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let name =
            std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok().or_else(|| std::fs::read_to_string("/etc/hostname").ok())?;
        let name: String = name.trim().chars().filter(|c| !c.is_control()).take(100).collect();
        (!name.is_empty()).then_some(name)
    }
}

/// A report draft for `ticket` (the same fields as a ticket): the server keeps it for 24 hours
/// and its page, opened in the browser, files it once the person signs in and confirms.
pub fn create_draft(ticket: &Value) -> Request {
    let mut body = ticket.clone();
    body["client_id"] = json!(CLIENT_ID);
    if let Some(host) = hostname() {
        body["hostname"] = json!(host);
    }
    request("POST", "/api/drafts", None, Some(body))
}

/// A draft the server opened: its id, the token its files are uploaded with, and the page to
/// open (its secret is in the fragment: never logged, never shown).
#[derive(Clone, PartialEq)]
pub struct Draft {
    pub id: String,
    pub upload_token: String,
    pub url: String,
}

impl std::fmt::Debug for Draft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Draft").field("id", &self.id).finish_non_exhaustive()
    }
}

pub fn parse_draft(v: &Value) -> Result<Draft, Failure> {
    let bad = || Failure::Refused("The server's answer to the report was incomplete.".into());
    let id = text(v, "draft_id")
        .filter(|i| !i.is_empty() && i.len() <= 128 && i.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .ok_or_else(bad)?;
    let upload_token = text(v, "upload_token").filter(|t| !t.is_empty() && t.len() < 1000).ok_or_else(bad)?;
    // Only the ticket server's own page opens in the browser.
    // Only a page of the ticket server itself opens in the browser: its host compared exactly.
    // The path is the server's to choose (it moved from /draft/ to /d/).
    let url = text(v, "url")
        .filter(|u| on_server(u, "/"))
        .ok_or_else(|| Failure::Refused("The server's draft address is not on the ticket server.".into()))?;
    Ok(Draft { id, upload_token, url })
}

// ---- resumable uploads ------------------------------------------------------------------------

/// The largest part the app sends, whatever the server allows (it asks for 8 MB parts).
pub const MAX_PART: usize = 8 << 20;

/// A file's SHA-256 as the server checks it, in hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Open an upload for a file of a draft (its upload token names the draft): its name, size,
/// hash, type and kind.
pub fn create_upload(token: &str, name: &str, bytes: &[u8], content_type: &str, kind: &str) -> Request {
    let body = json!({
        "filename": name,
        "size": bytes.len(),
        "sha256": sha256_hex(bytes),
        "content_type": content_type,
        "kind": kind,
    });
    request("POST", "/api/uploads", Some(token), Some(body))
}

/// An upload the server holds: its id, its part size and the parts still to send (numbered
/// from 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Upload {
    pub id: String,
    pub part_size: usize,
    pub missing: Vec<usize>,
}

/// An upload's answer (opened, or asked about to resume it).
pub fn parse_upload(v: &Value) -> Result<Upload, Failure> {
    let id = text(v, "id").or_else(|| v.get("id").and_then(Value::as_u64).map(|n| n.to_string()));
    let id = id.filter(|id| !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    let id = id.ok_or_else(|| Failure::Refused("The server did not open the upload.".into()))?;
    let part_size = v.get("part_size").and_then(Value::as_u64).and_then(|p| usize::try_from(p).ok()).unwrap_or(MAX_PART);
    if part_size == 0 || part_size > MAX_PART {
        return Err(Failure::Refused("The server asked for parts of an unsupported size.".into()));
    }
    let count = v.get("part_count").and_then(Value::as_u64).and_then(|c| usize::try_from(c).ok()).unwrap_or(0).min(100_000);
    let received: std::collections::BTreeSet<u64> =
        v.get("received").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
    let missing = (1..=count).filter(|n| !received.contains(&(*n as u64))).collect();
    Ok(Upload { id, part_size, missing })
}

/// Part `n` (from 1) of an upload: `bytes` is that part of the file.
pub fn upload_part(token: &str, upload: &str, n: usize, bytes: &[u8]) -> Request {
    let mut r = request("PUT", &format!("/api/uploads/{upload}/parts/{n}"), Some(token), None);
    r.headers.push(("Content-Type".into(), "application/octet-stream".into()));
    r.body = Some(bytes.to_vec());
    r.timeout_s = 600;
    r
}

/// Where an upload stands on the server (the parts it has), to resume it.
pub fn upload_status(token: &str, upload: &str) -> Request {
    request("GET", &format!("/api/uploads/{upload}"), Some(token), None)
}

pub fn finish_upload(token: &str, upload: &str) -> Request {
    request("POST", &format!("/api/uploads/{upload}/complete"), Some(token), Some(json!({})))
}
