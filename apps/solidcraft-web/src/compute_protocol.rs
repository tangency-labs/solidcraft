//! Private wire for the browser Form CAD worker.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use solidcraft_engine::forms_prototype::validation::BrowserCheck;
#[cfg(target_arch = "wasm32")]
use solidcraft_ui_egui::forms_prototype::BrowserPreview;

const MAGIC: &[u8; 4] = b"SCF1";
const HEADER: usize = 13;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ready,
    Check,
    Checked,
    Failed,
    Preview,
    Previewed,
}
impl Kind {
    fn byte(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::Check => 1,
            Self::Checked => 2,
            Self::Failed => 3,
            Self::Preview => 4,
            Self::Previewed => 5,
        }
    }

    fn max_payload(self) -> usize {
        match self {
            Self::Ready | Self::Failed => 0,
            Self::Check | Self::Checked => BrowserCheck::MAX_PAYLOAD,
            #[cfg(target_arch = "wasm32")]
            Self::Preview => BrowserPreview::MAX_REQUEST,
            #[cfg(target_arch = "wasm32")]
            Self::Previewed => BrowserPreview::MAX_REPLY,
            #[cfg(not(target_arch = "wasm32"))]
            Self::Preview | Self::Previewed => BrowserCheck::MAX_PAYLOAD,
        }
    }
}
pub fn frame(kind: Kind, id: u64, payload: &[u8]) -> Result<Vec<u8>, &'static str> {
    if payload.len() > kind.max_payload() {
        return Err("Form CAD worker payload exceeds its transfer limit");
    }
    let mut bytes = Vec::with_capacity(HEADER + payload.len());
    bytes.extend(MAGIC);
    bytes.push(kind.byte());
    bytes.extend(id.to_le_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}
pub fn read(bytes: &[u8]) -> Result<(Kind, u64, &[u8]), &'static str> {
    if !bytes.starts_with(MAGIC) {
        return Err("unsupported Form CAD worker protocol");
    }
    let (header, payload) = bytes.split_at_checked(HEADER).ok_or("incomplete Form CAD worker frame")?;
    let id = u64::from_le_bytes(header.get(5..).and_then(|value| value.try_into().ok()).ok_or("missing Form CAD worker job ID")?);
    let kind = match (header.get(4), id, payload.is_empty()) {
        (Some(0), 0, true) => Kind::Ready,
        (Some(1), 1.., false) => Kind::Check,
        (Some(2), 1.., false) => Kind::Checked,
        (Some(3), 0, true) => Kind::Failed,
        (Some(4), 1.., false) => Kind::Preview,
        (Some(5), 1.., false) => Kind::Previewed,
        _ => return Err("invalid Form CAD worker frame"),
    };
    if payload.len() > kind.max_payload() {
        return Err("Form CAD worker payload exceeds its transfer limit");
    }
    Ok((kind, id, payload))
}

#[cfg(target_arch = "wasm32")]
pub fn frame_bytes(value: wasm_bindgen::JsValue) -> Result<Vec<u8>, &'static str> {
    use wasm_bindgen::JsCast;
    let bytes: js_sys::Uint8Array = value.dyn_into().map_err(|_| "Form CAD worker frame must be bytes")?;
    if bytes.length() as usize > HEADER + BrowserPreview::MAX_REPLY {
        return Err("Form CAD worker frame exceeds its transfer limit");
    }
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_job_identity_and_size_are_checked() {
        let request = frame(Kind::Check, u64::MAX, b"exact source").unwrap();
        assert_eq!(read(&request), Ok((Kind::Check, u64::MAX, b"exact source".as_slice())));
        assert!(read(&request[..HEADER - 1]).is_err());
        assert!(read(&frame(Kind::Check, 0, b"exact source").unwrap()).is_err());
        assert!(frame(Kind::Ready, 0, b"unexpected payload").is_err());
        assert!(frame(Kind::Check, 1, &vec![0; BrowserCheck::MAX_PAYLOAD + 1]).is_err());
        let mut oversized = frame(Kind::Check, 1, b"request").unwrap();
        oversized.resize(HEADER + BrowserCheck::MAX_PAYLOAD + 1, 0);
        assert!(read(&oversized).is_err());
        let request = frame(Kind::Preview, 1, b"preview").unwrap();
        assert_eq!(read(&request), Ok((Kind::Preview, 1, b"preview".as_slice())));
    }
}
