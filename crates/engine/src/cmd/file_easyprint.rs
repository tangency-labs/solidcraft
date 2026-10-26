//! `file.open_in_easyprint`: export the bodies and hand the file to the host to upload to
//! EasyPrint (Printables' slicer in the browser). The model leaves the machine, so the command
//! refuses unless the caller says the user agreed (`consent`). The host does the HTTP and opens the
//! answer; see [`crate::easyprint`].

use serde_json::{Value, json};
use solidcraft_io::Format;

use crate::easyprint::{self, MAX_BYTES};
use crate::params::{bad, bool_, str_, string_list};
use crate::{Result, Session};

/// The bodies to send as the bytes of a mesh file.
pub(super) struct Exported {
    pub bytes: Vec<u8>,
    /// `stl` or `3mf`.
    pub ext: &'static str,
    /// A 3MF was asked for and couldn't be written, so this is an STL.
    pub fell_back: bool,
    /// The bodies that went in: the ones named, else the selected ones.
    pub bodies: Vec<String>,
}

/// Export the bodies (the selected ones when none are named). A 3MF that can't be written is
/// written as an STL, and `fell_back` says so.
pub(super) fn export_bodies(s: &Session, cmd: &str, named: &[String], format: Format) -> Result<Exported> {
    let mut bodies = named.to_vec();
    if bodies.is_empty() {
        bodies = s.selection.iter().filter_map(|x| if let crate::Sel::Body { name } = x { Some(name.clone()) } else { None }).collect();
    }
    if bodies.is_empty() {
        return Err(bad(cmd, "select the bodies to send, or give `bodies`"));
    }
    let doc = s.doc.painted(&*s.world_state()?);
    let write = |format| solidcraft_io::export_with_metadata(&doc, &bodies, format, &s.doc.name, &s.doc.metadata);
    let mut fell_back = false;
    let (bytes, ext) = match (format, write(format)) {
        (_, Ok(b)) => (b, if format == Format::StlBinary { "stl" } else { "3mf" }),
        (Format::ThreeMf, Err(_)) => {
            fell_back = true;
            (write(Format::StlBinary)?, "stl")
        }
        (_, Err(e)) => return Err(e.into()),
    };
    Ok(Exported { bytes, ext, fell_back, bodies })
}

pub(super) fn open_in_easyprint(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.open_in_easyprint";
    let format = match str_(p, "format").map(str::to_ascii_lowercase).as_deref() {
        None | Some("3mf") => Format::ThreeMf,
        Some("stl") => Format::StlBinary,
        Some(other) => return Err(bad(cmd, format!("unknown mesh format `{other}` (3mf, stl)"))),
    };
    let upload = s.easyprint_upload.clone().ok_or_else(|| bad(cmd, "not supported yet: this app has no way to reach EasyPrint"))?;
    if !bool_(p, "consent").unwrap_or(false) {
        return Err(bad(cmd, "this uploads the model to Printables' servers: ask the user, then pass `consent: true`"));
    }
    let Exported { bytes, ext, fell_back, .. } = export_bodies(s, cmd, &string_list(p, "bodies"), format)?;
    if bytes.len() > MAX_BYTES {
        let mb = bytes.len().div_ceil(1_000_000);
        return Err(bad(cmd, format!("the model is {mb} MB and EasyPrint takes {} MB at most: send fewer bodies", MAX_BYTES / 1_000_000)));
    }
    let name = easyprint::file_name(&s.doc.name, ext);
    let size = bytes.len();
    upload(&name, bytes).map_err(|e| bad(cmd, e))?;
    Ok(json!({"file_name": name, "bytes": size, "format": ext, "stl_fallback": fell_back, "uploading": true}))
}
