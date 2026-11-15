//! The ticket server's HTTP API (tickets.solidcraft.io): every path, request and answer the app
//! uses, in this one module. The rest of the app speaks in [`Request`]s and parsed answers, so a
//! change on the server's side is a change here only.

use serde_json::{Value, json};

use crate::agent_chat::backend::HttpRequest as Request;

/// Where reports go.
pub const SERVER: &str = "https://tickets.solidcraft.io";
/// The client id the server knows the app by.
const CLIENT_ID: &str = "solidcraft";
/// What a token from the app's sign-in may do: file tickets and read the user's own.
const SCOPE: &str = "tickets:read tickets:write";
/// The app's slug on the server, preselected in the report.
pub const DEFAULT_APP: &str = "solidcraft";

/// The server to talk to: [`SERVER`], or `SOLIDCRAFT_TICKETS_URL` on the desktop (a test server).
pub fn server() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(url) = std::env::var("SOLIDCRAFT_TICKETS_URL")
        && (url.starts_with("https://") || url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost"))
    {
        return url.trim_end_matches('/').to_string();
    }
    SERVER.to_string()
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
    /// The token is no longer good: sign in again.
    SignedOut,
    /// The server answered with an error; its message.
    Refused(String),
}

impl Failure {
    pub fn text(&self) -> String {
        match self {
            Self::Offline(e) => format!("Could not reach {}: {e}", server()),
            Self::SignedOut => "Your sign-in has expired. Sign in again.".into(),
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
        // A gateway that cannot reach the app is as good as offline.
        502..=504 => Err(Failure::Offline(format!("the server answered {status}"))),
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

// ---- sign-in (device authorization, RFC 8628) ---------------------------------------------

pub fn device_code() -> Request {
    request("POST", "/api/device/code", None, Some(json!({ "client_id": CLIENT_ID, "scope": SCOPE })))
}

/// The code to show and the page to open.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    /// The page to open: with the code filled in where the server offers that.
    pub verification: String,
    pub interval_s: f64,
    pub expires_in_s: f64,
}

pub fn parse_device_code(v: &Value) -> Result<DeviceCode, Failure> {
    let missing = || Failure::Refused("The server's sign-in answer was incomplete.".into());
    let verification = text(v, "verification_uri_complete").or_else(|| text(v, "verification_uri")).ok_or_else(missing)?;
    // Only a page on the ticket server opens in the browser.
    if !verification.starts_with(&server()) {
        return Err(missing());
    }
    Ok(DeviceCode {
        device_code: text(v, "device_code").ok_or_else(missing)?,
        user_code: text(v, "user_code").ok_or_else(missing)?.chars().take(32).collect(),
        verification,
        interval_s: v.get("interval").and_then(Value::as_f64).unwrap_or(5.0).clamp(1.0, 60.0),
        expires_in_s: v.get("expires_in").and_then(Value::as_f64).unwrap_or(600.0).clamp(30.0, 3600.0),
    })
}

pub fn device_token(device_code: &str) -> Request {
    request("POST", "/api/device/token", None, Some(json!({ "device_code": device_code, "client_id": CLIENT_ID })))
}

/// What a poll of the sign-in says.
#[derive(Clone, Debug, PartialEq)]
pub enum Poll {
    Token(String),
    Pending,
    SlowDown,
    Expired,
    Denied,
    Failed(Failure),
}

pub fn parse_poll(reply: Result<(u16, Vec<u8>), String>) -> Poll {
    let (status, body) = match reply {
        Ok(r) => r,
        Err(e) => return Poll::Failed(Failure::Offline(e)),
    };
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return match text(&v, "access_token") {
            Some(token) if !token.is_empty() && token.len() < 1000 => Poll::Token(token),
            _ => Poll::Failed(Failure::Refused("The server's sign-in answer was incomplete.".into())),
        };
    }
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => Poll::Pending,
        Some("slow_down") => Poll::SlowDown,
        Some("expired_token") => Poll::Expired,
        Some("access_denied") => Poll::Denied,
        _ => Poll::Failed(answer(Ok((status, body))).err().unwrap_or(Failure::Refused(format!("The server answered {status}.")))),
    }
}

// ---- apps and tickets -------------------------------------------------------------------------

pub fn apps(token: &str) -> Request {
    request("GET", "/api/apps", Some(token), None)
}

/// An app tickets are filed under: its slug and name.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub slug: String,
    pub name: String,
}

pub fn parse_apps(v: &Value) -> Vec<App> {
    v.get("apps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| Some(App { slug: text(a, "slug")?.chars().take(60).collect(), name: text(a, "name")?.chars().take(60).collect() }))
        .take(50)
        .collect()
}

pub fn create_ticket(token: &str, ticket: &Value) -> Request {
    request("POST", "/api/tickets", Some(token), Some(ticket.clone()))
}

/// The new ticket's number.
pub fn parse_ticket(v: &Value) -> Result<u64, Failure> {
    v.get("id").and_then(Value::as_u64).ok_or_else(|| Failure::Refused("The server did not say which ticket it filed.".into()))
}

/// The ticket's page on the server.
pub fn ticket_url(number: u64) -> String {
    format!("{}/tickets/{number}", server())
}

// ---- resumable uploads ------------------------------------------------------------------------

/// The most one chunk carries, whatever the server allows.
pub const MAX_CHUNK: usize = 4 << 20;

pub fn create_upload(token: &str, ticket: u64, name: &str, size: usize, content_type: &str, kind: &str) -> Request {
    request(
        "POST",
        &format!("/api/tickets/{ticket}/uploads"),
        Some(token),
        Some(json!({ "filename": name, "size": size, "contentType": content_type, "kind": kind })),
    )
}

/// An upload the server opened: its id, where it continues and how much a chunk may carry.
#[derive(Clone, Debug, PartialEq)]
pub struct Upload {
    pub id: String,
    pub offset: usize,
    pub chunk: usize,
}

pub fn parse_upload(v: &Value) -> Result<Upload, Failure> {
    let id = text(v, "id").or_else(|| v.get("id").and_then(Value::as_u64).map(|n| n.to_string()));
    let id = id.filter(|id| !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    Ok(Upload {
        id: id.ok_or_else(|| Failure::Refused("The server did not open the upload.".into()))?,
        offset: parse_offset(v),
        chunk: v.get("chunkSize").and_then(Value::as_u64).map_or(MAX_CHUNK, |c| usize::try_from(c).unwrap_or(MAX_CHUNK)).clamp(64 << 10, MAX_CHUNK),
    })
}

pub fn parse_offset(v: &Value) -> usize {
    v.get("offset").and_then(Value::as_u64).and_then(|o| usize::try_from(o).ok()).unwrap_or(0)
}

pub fn upload_chunk(token: &str, upload: &str, offset: usize, bytes: &[u8]) -> Request {
    let mut r = request("PATCH", &format!("/api/uploads/{upload}"), Some(token), None);
    r.headers.push(("Upload-Offset".into(), offset.to_string()));
    r.headers.push(("Content-Type".into(), "application/offset+octet-stream".into()));
    r.body = Some(bytes.to_vec());
    r.timeout_s = 300;
    r
}

/// Where an upload stands on the server (to resume it).
pub fn upload_status(token: &str, upload: &str) -> Request {
    request("GET", &format!("/api/uploads/{upload}"), Some(token), None)
}

pub fn finish_upload(token: &str, upload: &str) -> Request {
    request("POST", &format!("/api/uploads/{upload}/complete"), Some(token), Some(json!({})))
}

// ---- the user's own tickets -------------------------------------------------------------------

pub fn mine(token: &str) -> Request {
    request("GET", "/api/tickets?reporter=me&sort=updated&limit=50", Some(token), None)
}
