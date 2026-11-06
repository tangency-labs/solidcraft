//! What talking to EasyPrint (Printables' slicer in the browser) needs that doesn't touch a
//! network: the upload form, and the reading of its answer. The hosts do the HTTP themselves (the
//! desktop app with `ureq`, the browser build with `fetch`) and call these. `file.open_in_easyprint`
//! hands them the file.

use serde_json::Value;

/// Where a model is uploaded: `POST`, multipart, field `file`, `Accept: application/json`.
pub const IMPORT_URL: &str = "https://slicer.printables.com/import";
/// The most EasyPrint takes in one upload (100 MB); a bigger file is refused before it is sent.
pub const MAX_BYTES: usize = 100_000_000;

/// The file name to upload as: the design's name made safe, with the extension.
pub fn file_name(design: &str, ext: &str) -> String {
    let stem: String = design.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' }).take(80).collect();
    let stem = stem.trim();
    format!("{}.{ext}", if stem.is_empty() { "model" } else { stem })
}

/// The upload form: its `Content-Type` and the body, a `file` part holding `bytes` and a
/// `file_name` field.
pub fn multipart(file_name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let mut boundary = String::from("----solidcraft-easyprint-6b1f0d");
    while bytes.windows(boundary.len()).any(|w| w == boundary.as_bytes()) {
        boundary.push('x');
    }
    // A quote or a line break in the name would end the header early.
    let name: String = file_name.chars().filter(|c| !matches!(c, '"' | '\r' | '\n' | '\\')).collect();
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file_name\"\r\n\r\n{name}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// The address to open from EasyPrint's answer to an upload, or why there is none.
pub fn parse_import_response(status: u16, body: &[u8]) -> Result<String, String> {
    let json: Option<Value> = serde_json::from_slice(body).ok();
    if !(200..300).contains(&status) {
        let code = json.as_ref().and_then(|j| j.get("error")).and_then(Value::as_str);
        return Err(match code {
            Some("file-too-large") => "EasyPrint says the file is too large (100 MB at most)".into(),
            Some("invalid-extension") => "EasyPrint doesn't take this kind of file".into(),
            Some("missing-file") => "EasyPrint got no file".into(),
            Some(other) => format!("EasyPrint refused the upload ({})", other.chars().take(60).collect::<String>()),
            None => format!("EasyPrint answered HTTP {status}"),
        });
    }
    let url = json.as_ref().and_then(|j| j.get("easyprint_url")).and_then(Value::as_str).ok_or("EasyPrint's answer had no address to open")?;
    // Only a Printables address is opened in the browser, whatever the server says.
    let host = url.strip_prefix("https://").map(|rest| rest.split(['/', '?', '#']).next().unwrap_or_default());
    match host {
        Some(h) if !h.contains(['@', ':']) && (h == "printables.com" || h.ends_with(".printables.com")) => Ok(url.to_string()),
        _ => Err("EasyPrint's answer pointed somewhere other than Printables, so it was not opened".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str =
        r#"{"easyprint_url":"https://www.printables.com/slice?file_url=x","file_url":"https://slicer.printables.com/a","file_name":"t.stl"}"#;

    #[test]
    fn a_good_answer_gives_its_address() {
        assert_eq!(parse_import_response(200, OK.as_bytes()).unwrap(), "https://www.printables.com/slice?file_url=x");
    }

    #[test]
    fn errors_malformed_and_foreign_answers_give_no_address() {
        let e = parse_import_response(413, br#"{"error":"file-too-large"}"#).unwrap_err();
        assert!(e.contains("too large"), "{e}");
        assert!(parse_import_response(500, b"<html>").unwrap_err().contains("HTTP 500"));
        assert!(parse_import_response(200, b"not json").is_err());
        assert!(parse_import_response(200, b"{}").is_err());
        assert!(parse_import_response(200, br#"{"easyprint_url":7}"#).is_err());
        for url in [
            "http://www.printables.com/x",
            "https://evil.example/x",
            "https://printables.com.evil.example/x",
            "https://www.printables.com@evil.example/x",
            "javascript:alert(1)",
        ] {
            let body = format!(r#"{{"easyprint_url":"{url}"}}"#);
            assert!(parse_import_response(200, body.as_bytes()).is_err(), "{url}");
        }
    }

    #[test]
    fn the_form_holds_the_file_untouched_and_a_safe_name() {
        let bytes: Vec<u8> = (0..=255).collect();
        let (content_type, body) = multipart("a\"b\r\nc.stl", &bytes);
        let boundary = content_type.strip_prefix("multipart/form-data; boundary=").unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("filename=\"abc.stl\""), "{text}");
        assert!(body.windows(bytes.len()).any(|w| w == bytes.as_slice()));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
        // A file that happens to hold the usual boundary gets another one.
        let (ct, _) = multipart("m.stl", b"x----solidcraft-easyprint-6b1f0dx");
        assert!(!ct.ends_with("6b1f0d"));
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(file_name("Bracket v2", "3mf"), "Bracket v2.3mf");
        assert_eq!(file_name("../../etc/x", "stl"), "______etc_x.stl");
        assert_eq!(file_name("  ", "stl"), "model.stl");
    }
}
