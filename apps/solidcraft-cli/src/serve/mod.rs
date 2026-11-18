//! `solidcraft-cli serve`: the server edition (docs/server-edition.md). One process offers MCP
//! over the streamable-HTTP transport (`/mcp`), the data folder over a small HTTP API
//! (`/api/v1/files`) and, with `--web DIR`, the browser app from the same origin. Every request
//! but the app's files and `/api/v1/health` needs the token; every file call is confined to the
//! data folder. Plain HTTP on loopback by default: TLS is the reverse proxy's.

mod files;
mod http;
mod mcp;
mod web;

use std::io::Write as _;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::json;
use sha2::{Digest, Sha256};

use http::{Head, HttpError, Response};

/// Where the server listens unless `--listen` says otherwise: this computer only.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8787";
/// The token's environment variable.
pub const TOKEN_ENV: &str = "SOLIDCRAFT_SERVE_TOKEN";
/// The shortest token accepted.
pub const MIN_TOKEN: usize = 32;
const MAX_TOKEN: usize = 1024;
/// Connections served at once; more are told to come back.
const MAX_CONNECTIONS: usize = 64;
/// How long a client has for each read and write.
const IO_TIMEOUT: Duration = Duration::from_secs(30);
/// What a refused token costs the caller.
const REFUSAL_WAIT: Duration = Duration::from_millis(250);
const DEFAULT_MAX_UPLOAD: u64 = 256 << 20;

pub const USAGE: &str = "  solidcraft-cli serve --dir FOLDER|DESIGN.sc [--listen 127.0.0.1:8787] [--web DIR] [--token-file FILE | --no-token] [--max-sessions 4] [--session-idle 30]\n                      [--max-upload 256MB] [--allow-origin ORIGIN]... [--allow-plugins] [--param-check off|warn|strict] [--compact]\n      (the server edition, docs/server-edition.md: MCP over HTTP at /mcp, the folder at /api/v1/ and, with --web,\n       the browser app; --dir is any folder (a git checkout too; saves go in place) or a design to open in it; the\n       token comes from SOLIDCRAFT_SERVE_TOKEN or --token-file, at least 32 characters, sent as `Authorization: Bearer\n       TOKEN`; --no-token only on 127.0.0.1; plain HTTP, so use a TLS reverse proxy or `tailscale serve` off this computer)\n";

const OPTIONS: &[&str] =
    &["--dir", "--data", "--listen", "--web", "--token-file", "--max-sessions", "--session-idle", "--max-upload", "--allow-origin", "--param-check"];
const SWITCHES: &[&str] = &["--allow-plugins", "--compact", "--no-token"];

struct Config {
    /// `None`: `--no-token`, on loopback only.
    token_digest: Option<[u8; 32]>,
    store: files::Store,
    site: Option<web::Site>,
    sessions: mcp::Sessions,
    session_config: mcp::SessionConfig,
    allow_origins: Vec<String>,
    max_upload: u64,
    /// The design `--dir` named, relative to the folder: the browser app opens it at start.
    open: Option<String>,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if OPTIONS.contains(&a.as_str()) {
            if args.get(i + 1).is_none_or(|v| v.starts_with("--")) {
                return Err(format!("serve: {a} needs a value"));
            }
            i += 2;
        } else if SWITCHES.contains(&a.as_str()) {
            i += 1;
        } else if a == "--token" {
            return Err(format!("serve: give the token in {TOKEN_ENV} or --token-file, not on the command line (every user can read it there)"));
        } else {
            return Err(format!("serve: unknown option `{a}`\nusage:\n{USAGE}"));
        }
    }
    let one = |name: &str| -> Result<Option<&str>, String> {
        let all = crate::flags(args, name);
        if all.len() > 1 {
            return Err(format!("serve: give {name} once"));
        }
        Ok(all.first().copied())
    };
    let dir = match (one("--dir")?, one("--data")?) {
        (Some(_), Some(_)) => return Err("serve: give --dir once (--data is another name for it)".into()),
        (Some(d), None) | (None, Some(d)) => d,
        (None, None) => return Err("serve: --dir is required: the folder the designs are kept in, or a design to open".into()),
    };
    let listen: SocketAddr = one("--listen")?
        .unwrap_or(DEFAULT_LISTEN)
        .parse()
        .map_err(|_| "serve: --listen wants ADDRESS:PORT, such as 127.0.0.1:8787 or 100.101.102.103:8787".to_string())?;
    let token = if args.iter().any(|a| a == "--no-token") {
        if !listen.ip().is_loopback() {
            return Err("serve: --no-token is only for --listen on 127.0.0.1 (this computer); anywhere else needs a token".into());
        }
        if one("--token-file")?.is_some() || std::env::var(TOKEN_ENV).is_ok_and(|t| !t.is_empty()) {
            return Err(format!("serve: --no-token with a token ({TOKEN_ENV} or --token-file): give one or the other"));
        }
        None
    } else {
        Some(read_token(one("--token-file")?)?)
    };
    // A design: its folder is served, and the design opens at start.
    let given = std::path::Path::new(dir);
    let (folder, open) = if given.is_file() {
        let name = given.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !solidcraft_engine::io::is_design_path(&name) {
            return Err(format!("serve: --dir {dir}: a folder, or a design (.{}) to open", solidcraft_engine::io::DESIGN_EXT));
        }
        let parent = given.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
        (parent.to_path_buf(), Some(name))
    } else {
        (given.to_path_buf(), None)
    };
    let store = files::Store::new(&folder).map_err(|e| format!("serve: {e}"))?;
    let site = one("--web")?.map(|w| web::Site::new(std::path::Path::new(w))).transpose().map_err(|e| format!("serve: {e}"))?;
    let number = |name: &str, default: u64, max: u64| -> Result<u64, String> {
        match one(name)? {
            None => Ok(default),
            Some(v) => v.parse::<u64>().ok().filter(|n| (1..=max).contains(n)).ok_or_else(|| format!("serve: {name} wants a number from 1 to {max}")),
        }
    };
    let max_sessions = number("--max-sessions", 4, 256)? as usize;
    let idle = Duration::from_secs(number("--session-idle", 30, 7 * 24 * 60)? * 60);
    let max_upload = match one("--max-upload")? {
        None => DEFAULT_MAX_UPLOAD,
        Some(v) => solidcraft_engine::io::limits::parse_flag(v).map_err(|e| format!("serve: --max-upload: {e}"))?,
    };
    let allow_origins: Vec<String> = crate::flags(args, "--allow-origin").into_iter().map(|o| o.trim_end_matches('/').to_ascii_lowercase()).collect();
    let plugins = args.iter().any(|a| a == "--allow-plugins");
    if !plugins {
        // No plugins folder is read: none can load, from a design or a call.
        // A folder inside this run's private scratch folder that is never made.
        let none = solidcraft_mcp::scratch_dir().map(|d| d.join("no-plugins")).ok_or("serve: could not make a private temporary folder")?;
        solidcraft_engine::plugin::configure_process_plugins(Some(none), false)
            .map_err(|_| "serve: --plugins-dir and --keep-plugin-approvals need --allow-plugins".to_string())?;
    }
    let options = solidcraft_mcp::Options::from_args_and_env(args).map_err(|e| format!("serve: {e}"))?;
    let param_check = crate::parse_param_check("serve", args)?;
    crate::install_renderer();
    // The data folder and this session's scratch folder (screenshots without a path); no
    // recovery folder, no home folder, no `--allow-any-path`.
    let scope = solidcraft_mcp::agent_scope_with(None, [store.root().to_path_buf()]);
    // Agents may name designs relative to the data folder.
    std::env::set_current_dir(store.root()).map_err(|e| format!("serve: {}: {e}", store.root().display()))?;
    let session_config =
        mcp::SessionConfig { scope, options, param_check, plugins, idle, max_sessions, open: open.as_ref().map(|n| store.root().join(n)) };
    let config = Arc::new(Config {
        token_digest: token.as_deref().map(digest),
        open,
        sessions: mcp::Sessions::new(session_config.clone()),
        session_config,
        store,
        site,
        allow_origins,
        max_upload,
    });
    let listener = TcpListener::bind(listen).map_err(|e| format!("serve: could not listen on {listen}: {e}"))?;
    let local = listener.local_addr().map_err(|e| format!("serve: {e}"))?;
    if !local.ip().is_loopback() {
        solidcraft_engine::note!(
            "solidcraft serve: listening beyond this computer; the server speaks plain HTTP, so reach it through a TLS reverse proxy"
        );
    }
    // Tests and scripts read this line for the port (`--listen 127.0.0.1:0`).
    eprintln!(
        "solidcraft serve: listening on http://{local} (folder {}{}{}{}{})",
        config.store.root().display(),
        config.open.as_deref().map(|n| format!(", opening {n}")).unwrap_or_default(),
        if config.token_digest.is_none() { ", no token: this computer only" } else { "" },
        if config.site.is_some() { ", browser app at /" } else { ", no browser app (--web DIR)" },
        if plugins { ", plugins on" } else { "" }
    );
    let _ = std::io::stderr().flush();
    let open = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            open.fetch_sub(1, Ordering::SeqCst);
            let mut stream = stream;
            let _ = Response::error(&HttpError::new(503, "too many connections; try again")).write_to(&mut stream, false);
            continue;
        }
        let (config, done) = (Arc::clone(&config), Arc::clone(&open));
        let spawned = std::thread::Builder::new().name("serve-connection".into()).spawn(move || {
            connection(&config, stream);
            done.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            open.fetch_sub(1, Ordering::SeqCst);
        }
    }
    Ok(())
}

/// The token from `--token-file` or the environment, checked.
fn read_token(file: Option<&str>) -> Result<String, String> {
    let env = std::env::var(TOKEN_ENV).ok().filter(|t| !t.is_empty());
    let token = match (file, env) {
        (Some(_), Some(_)) => return Err(format!("serve: give the token once, in {TOKEN_ENV} or --token-file, not both")),
        (Some(path), None) => {
            let bytes = solidcraft_engine::io::vfs::read(path, 64 << 10).map_err(|e| format!("serve: --token-file {path}: {e}"))?;
            String::from_utf8(bytes).map_err(|_| format!("serve: --token-file {path}: not text"))?.lines().next().unwrap_or("").trim().to_string()
        }
        (None, Some(t)) => t.trim().to_string(),
        (None, None) => {
            return Err(format!("serve: a token is required: set {TOKEN_ENV} or give --token-file FILE (make one with `openssl rand -hex 32`)"));
        }
    };
    if token.chars().count() < MIN_TOKEN {
        return Err(format!(
            "serve: the token is too short ({} characters); use at least {MIN_TOKEN}, such as `openssl rand -hex 32`",
            token.chars().count()
        ));
    }
    if token.len() > MAX_TOKEN || token.chars().any(|c| c.is_control() || c == ' ') {
        return Err(format!("serve: the token must be at most {MAX_TOKEN} bytes of visible characters"));
    }
    Ok(token)
}

fn digest(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

/// Whether `head` carries the token, compared in constant time on fixed-length digests.
fn authorized(config: &Config, head: &Head) -> bool {
    let Some(want) = &config.token_digest else {
        // `--no-token` listens on loopback only; a page that reached it through a rebound DNS
        // name still sends that name as Host, so only this computer's own names pass.
        return head.header("host").is_some_and(loopback_host);
    };
    let Some(value) = head.header("authorization") else { return false };
    let Some((scheme, token)) = value.split_once(' ') else { return false };
    if !scheme.eq_ignore_ascii_case("bearer") {
        return false;
    }
    let given = digest(token.trim());
    given.iter().zip(want.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// Whether a Host header names this computer (`localhost`, `127.0.0.1`, `[::1]`, any port).
fn loopback_host(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(host, |(h, p)| if p.bytes().all(|b| b.is_ascii_digit()) { h } else { host }),
    };
    name.eq_ignore_ascii_case("localhost") || name.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Whether a request from a browser comes from this server's own origin or one allowed.
/// Requests without `Origin` (agents, curl) pass; the token still decides.
fn origin_allowed(config: &Config, head: &Head) -> bool {
    let Some(origin) = head.header("origin") else { return true };
    let origin = origin.trim_end_matches('/').to_ascii_lowercase();
    if config.allow_origins.contains(&origin) {
        return true;
    }
    let Some(host) = head.header("host").map(str::to_ascii_lowercase) else { return false };
    origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://")).is_some_and(|o| o == host)
}

fn connection(config: &Config, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let head = match http::read_head(&mut stream) {
        Ok(Some(head)) => head,
        Ok(None) => return,
        Err(e) => {
            let _ = Response::error(&e).write_to(&mut stream, false);
            return;
        }
    };
    let head_only = head.method == "HEAD";
    let response = route(config, &head, &mut stream).unwrap_or_else(|e| Response::error(&e));
    let _ = response.write_to(&mut stream, head_only);
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

fn route(config: &Config, head: &Head, stream: &mut TcpStream) -> Result<Response, HttpError> {
    let path = head.path.as_str();
    let method = head.method.as_str();
    if path == "/api/v1/health" {
        return match method {
            "GET" | "HEAD" => Ok(Response::json(200, &json!({"ok": true}))),
            _ => Err(HttpError::new(405, "GET only")),
        };
    }
    let api = path == "/mcp" || path.starts_with("/mcp/") || path == "/api" || path.starts_with("/api/");
    if !api {
        return match (method, &config.site) {
            ("GET" | "HEAD", Some(site)) => site.get(path),
            ("GET" | "HEAD", None) => Err(HttpError::new(404, "this server has no browser app (start it with --web DIR)")),
            _ => Err(HttpError::new(405, "GET only")),
        };
    }
    if !origin_allowed(config, head) {
        return Err(HttpError::new(403, "requests from that origin are not allowed (the server's --allow-origin adds one)"));
    }
    if !authorized(config, head) {
        std::thread::sleep(REFUSAL_WAIT);
        return Err(HttpError::new(401, "send the server's token as `Authorization: Bearer TOKEN`"));
    }
    let session = head.header("mcp-session-id");
    match (method, path) {
        ("POST", "/mcp") => {
            let body = head.read_body(stream, solidcraft_engine::MAX_LINE as u64)?;
            config.sessions.post(session, &body)
        }
        ("DELETE", "/mcp") => config.sessions.delete(session),
        (_, "/mcp") => Err(HttpError::new(405, "POST a JSON-RPC message, or DELETE to end the session; this server opens no event stream")),
        ("GET", "/api/v1/info") => Ok(Response::json(
            200,
            &json!({
                "version": env!("CARGO_PKG_VERSION"),
                "commit": solidcraft_engine::io::SOURCE_COMMIT,
                "mcp": mcp::info(&config.session_config),
                "max_upload": config.max_upload,
                "open": config.open,
                "extensions": files::EXTENSIONS,
            }),
        )),
        ("GET" | "HEAD", "/api/v1/files") => Ok(config.store.list()),
        (_, p) if p == "/api/v1/folder" || p.starts_with("/api/v1/folder/") => {
            let rel = p.strip_prefix("/api/v1/folder").unwrap_or("").trim_start_matches('/');
            match method {
                "GET" | "HEAD" => config.store.folder(rel),
                "PUT" => config.store.make_folder(rel),
                _ => Err(HttpError::new(405, "GET to list, PUT to make a folder")),
            }
        }
        (_, p) if p.starts_with("/api/v1/files/") => {
            let rel = p.strip_prefix("/api/v1/files/").unwrap_or("");
            match method {
                "GET" | "HEAD" => config.store.get(rel),
                "PUT" => config.store.put(rel, head, stream, config.max_upload),
                "DELETE" => config.store.delete(rel, head),
                _ => Err(HttpError::new(405, "GET, PUT or DELETE")),
            }
        }
        _ => Err(HttpError::new(404, "no such API call (see docs/server-edition.md)")),
    }
}
