//! The content blocks a tool result is made of (MCP `CallToolResult`), and the contents a resource
//! read sends (MCP `ReadResourceResult`).

use serde_json::{Value, json};

/// What `resources/read` sends for one resource: text of a MIME type, or bytes of a MIME type,
/// which go out base64-encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Contents {
    Text { mime_type: String, text: String },
    Blob { mime_type: String, data: Vec<u8> },
}

impl Contents {
    /// `value` as JSON text (`application/json`).
    pub fn json(value: &Value) -> Self {
        Self::Text { mime_type: "application/json".into(), text: value.to_string() }
    }

    /// The MCP contents block of resource `uri`.
    pub(crate) fn block(&self, uri: &str) -> Value {
        match self {
            Self::Text { mime_type, text } => json!({"uri": uri, "mimeType": mime_type, "text": text}),
            Self::Blob { mime_type, data } => json!({"uri": uri, "mimeType": mime_type, "blob": base64(data)}),
        }
    }
}

/// A tool result of one text block, flagged as an error when `is_error` (the model sees the
/// message; an error is still a result, not a JSON-RPC error).
pub fn text_result(text: impl Into<String>, is_error: bool) -> Value {
    let mut r = json!({"content": [{"type": "text", "text": text.into()}]});
    if is_error {
        r["isError"] = json!(true);
    }
    r
}

/// A tool result of an image (`data` of type `mime`, base64-encoded here) followed by a text
/// block saying what it shows.
pub fn image_result(data: &[u8], mime: &str, text: impl Into<String>) -> Value {
    json!({"content": [
        {"type": "image", "data": base64(data), "mimeType": mime},
        {"type": "text", "text": text.into()},
    ]})
}

/// Standard base64 with padding (RFC 4648), as MCP image content carries it.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(T[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
