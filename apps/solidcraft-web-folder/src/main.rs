//! A bounded HTTP endpoint behind nginx; serializes conditional writes from all browsers.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

#[path = "../../solidcraft-web/src/server.rs"]
mod paths;

const MAX_BYTES: u64 = 256 * 1024 * 1024;
type Reply = (u16, String, Vec<u8>);

fn read_file(path: &Path) -> io::Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(io::Error::other("not a regular design file within the size limit"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::other("file is too large"));
    }
    Ok(bytes)
}

fn etag(bytes: &[u8]) -> String {
    format!("\"{:x}\"", Sha256::digest(bytes))
}

/// Reject symlinks, including links to other places within the shared folder.
fn resolve(root: &Path, name: &str) -> io::Result<PathBuf> {
    let mut path = root.to_path_buf();
    for part in name.split('/').filter(|p| !p.is_empty()) {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(io::Error::other("symlinks are not served")),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(path)
}

fn handle(root: &Path, method: &str, uri: &str, headers: &[(String, String)], body: &[u8]) -> io::Result<Reply> {
    let uri = uri.strip_suffix('/').unwrap_or(uri);
    let name = if uri == "/designs" {
        String::new()
    } else {
        let Some(name) = paths::from_endpoint(uri) else { return Ok((400, String::new(), Vec::new())) };
        name
    };
    let path = resolve(root, &name)?;
    let header = |key: &str| headers.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    let empty = |code| Ok((code, String::new(), Vec::new()));
    match method {
        "OPTIONS" => empty(204),
        "GET" | "HEAD" if path.is_dir() => {
            let mut entries = Vec::new();
            for (index, entry) in fs::read_dir(&path)?.take(paths::MAX_LISTING + 1).enumerate() {
                if index >= paths::MAX_LISTING {
                    return empty(413);
                }
                let entry = entry?;
                let Ok(name) = entry.file_name().into_string() else { continue };
                let kind = entry.file_type()?;
                if !paths::valid(&name) || (!kind.is_dir() && !kind.is_file()) {
                    continue;
                }
                entries.push(serde_json::json!({"name":name,"directory":kind.is_dir()}));
            }
            entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            let bytes = serde_json::to_vec(&entries)?;
            if bytes.len() > paths::MAX_LIST_BYTES {
                return empty(413);
            }
            Ok((200, String::new(), bytes))
        }
        "GET" | "HEAD" => {
            let bytes = read_file(&path)?;
            Ok((200, etag(&bytes), bytes))
        }
        "PUT" if !name.is_empty() && !path.is_dir() => {
            let previous = match read_file(&path) {
                Ok(bytes) => Some(etag(&bytes)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            };
            if header("if-match").is_some_and(|v| previous.as_deref() != Some(v)) || (header("if-none-match") == Some("*") && previous.is_some()) {
                return empty(412);
            }
            // A sibling temporary file keeps replacement atomic, including on a NAS mount.
            let temp = path.with_file_name(format!(
                ".solidcraft-write-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()
            ));
            let result = (|| {
                let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
                file.write_all(body)?;
                file.sync_all()?;
                fs::rename(&temp, &path)
            })();
            let _ = fs::remove_file(&temp);
            result?;
            Ok((if previous.is_some() { 204 } else { 201 }, etag(body), Vec::new()))
        }
        "MKCOL" if !name.is_empty() => {
            fs::create_dir(path)?;
            empty(201)
        }
        "DELETE" if !name.is_empty() => {
            if path.is_dir() {
                fs::remove_dir(path)?;
            } else {
                fs::remove_file(path)?;
            }
            empty(204)
        }
        _ => empty(405),
    }
}

fn serve(root: &Path, stream: &mut TcpStream) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.by_ref().take(8192).read_line(&mut line)?;
    let mut request = line.split_whitespace();
    let method = request.next().unwrap_or("").to_string();
    let uri = request.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    let mut total = line.len();
    loop {
        line.clear();
        let read = reader.by_ref().take(8192).read_line(&mut line)?;
        total += read;
        if total > 32_768 || read == 0 {
            return Err(io::Error::other("invalid headers"));
        }
        if line == "\r\n" {
            break;
        }
        let (key, value) = line.split_once(':').ok_or_else(|| io::Error::other("invalid header"))?;
        headers.push((key.to_ascii_lowercase(), value.trim().to_string()));
    }
    let length =
        headers.iter().find(|(k, _)| k == "content-length").map(|(_, v)| v.parse::<u64>()).transpose().map_err(io::Error::other)?.unwrap_or(0);
    let result = if length > MAX_BYTES {
        Ok((413, String::new(), Vec::new()))
    } else {
        let mut body = vec![0; length as usize];
        reader.read_exact(&mut body)?;
        handle(root, &method, &uri, &headers, &body)
    };
    let (status, tag, bytes) = match result {
        Ok(reply) => reply,
        Err(e) => (
            match e.kind() {
                io::ErrorKind::NotFound => 404,
                io::ErrorKind::PermissionDenied => 403,
                _ => 409,
            },
            String::new(),
            Vec::new(),
        ),
    };
    write!(
        stream,
        "HTTP/1.1 {status} Response\r\nConnection: close\r\nCache-Control: no-store\r\nX-SolidCraft-Folder: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        bytes.len()
    )?;
    if !tag.is_empty() {
        write!(stream, "ETag: {tag}\r\n")?;
    }
    write!(stream, "\r\n")?;
    if method != "HEAD" {
        stream.write_all(&bytes)?;
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let root = fs::canonicalize("/designs")?;
    if !root.is_dir() {
        return Err(io::Error::other("/designs must be a directory"));
    }
    let listener = TcpListener::bind("127.0.0.1:8081")?;
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                if let Err(e) = serve(&root, &mut stream) {
                    eprintln!("folder request: {e}");
                }
            }
            Err(e) => eprintln!("folder connection: {e}"),
        }
    }
    Ok(())
}
