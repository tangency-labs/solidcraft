//! Private wire for the browser Form CAD worker.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use solidcraft_engine::forms_prototype::validation::BrowserCheck;

const MAGIC: &[u8; 4] = b"SCF1";
const HEADER: usize = 13;
const MAX_BYTES: usize = HEADER + BrowserCheck::MAX_PAYLOAD;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ready,
    Check,
    Checked,
    Failed,
}
impl Kind {
    fn byte(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::Check => 1,
            Self::Checked => 2,
            Self::Failed => 3,
        }
    }
}
pub fn frame(kind: Kind, id: u64, payload: &[u8]) -> Result<Vec<u8>, &'static str> {
    if payload.len() > BrowserCheck::MAX_PAYLOAD {
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
    if bytes.len() > MAX_BYTES || !bytes.starts_with(MAGIC) {
        return Err("unsupported Form CAD worker protocol");
    }
    let (header, payload) = bytes.split_at_checked(HEADER).ok_or("incomplete Form CAD worker frame")?;
    let id = u64::from_le_bytes(header.get(5..).and_then(|value| value.try_into().ok()).ok_or("missing Form CAD worker job ID")?);
    let kind = match (header.get(4), id, payload.is_empty()) {
        (Some(0), 0, true) => Kind::Ready,
        (Some(1), 1.., false) => Kind::Check,
        (Some(2), 1.., false) => Kind::Checked,
        (Some(3), 0, true) => Kind::Failed,
        _ => return Err("invalid Form CAD worker frame"),
    };
    Ok((kind, id, payload))
}

#[cfg(target_arch = "wasm32")]
pub fn frame_bytes(value: wasm_bindgen::JsValue) -> Result<Vec<u8>, &'static str> {
    use wasm_bindgen::JsCast;
    let bytes: js_sys::Uint8Array = value.dyn_into().map_err(|_| "Form CAD worker frame must be bytes")?;
    if bytes.length() as usize > MAX_BYTES {
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
        assert!(read(&frame(Kind::Ready, 0, b"unexpected payload").unwrap()).is_err());
        assert!(frame(Kind::Check, 1, &vec![0; BrowserCheck::MAX_PAYLOAD + 1]).is_err());
        assert!(read(&vec![0; MAX_BYTES + 1]).is_err());
    }
}
