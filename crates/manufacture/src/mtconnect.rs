//! MTConnect machine plugin (Haas NGC / Mazak Smooth / many Fanuc adapters).
//!
//! HTTP GET against an agent (`…/current`). Transport is injectable so unit tests never need
//! a real machine — inject fixed XML or point at a loopback agent.

use std::sync::Arc;

use crate::job::Availability;
use crate::machine_host::{MachineBackend, MachineBackendDescriptor, MachineError, MachineLiveStatus, MachineResult};

/// Minimal HTTP GET used by [`MtConnectBackend`].
pub trait HttpGet: Send + Sync + 'static {
    fn get(&self, url: &str) -> Result<String, String>;
}

/// Blocking std sockets HTTP/1.0 GET (native tests / desktop). Not for wasm.
#[derive(Clone, Debug, Default)]
pub struct StdHttpGet;

impl HttpGet for StdHttpGet {
    fn get(&self, url: &str) -> Result<String, String> {
        http_get_blocking(url)
    }
}

/// Fixed-body transport for tests.
#[derive(Clone, Debug)]
pub struct FixedHttpGet {
    pub body: String,
}

impl HttpGet for FixedHttpGet {
    fn get(&self, _url: &str) -> Result<String, String> {
        Ok(self.body.clone())
    }
}

/// MTConnect agent client plugin.
pub struct MtConnectBackend {
    /// Base without trailing slash, e.g. `http://127.0.0.1:8082/VF-2`.
    pub base_url: String,
    pub http: Arc<dyn HttpGet>,
    pub label: String,
}

impl MtConnectBackend {
    pub fn new(base_url: impl Into<String>, http: Arc<dyn HttpGet>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self { label: format!("MTConnect ({base_url})"), base_url, http }
    }

    pub fn with_std(base_url: impl Into<String>) -> Self {
        Self::new(base_url, Arc::new(StdHttpGet))
    }

    pub fn with_fixed_xml(base_url: impl Into<String>, xml: impl Into<String>) -> Self {
        Self::new(base_url, Arc::new(FixedHttpGet { body: xml.into() }))
    }

    fn current_url(&self) -> String {
        format!("{}/current", self.base_url)
    }
}

impl MachineBackend for MtConnectBackend {
    fn descriptor(&self) -> MachineBackendDescriptor {
        MachineBackendDescriptor {
            id: "mtconnect".into(),
            label: self.label.clone(),
            protocol: "mtconnect".into(),
            availability: if self.is_available() { Availability::Available } else { Availability::Unavailable },
            note: Some("Read-only MTConnect Streams (current). FOCAS/THINC are separate plugins.".into()),
            endpoint: Some(self.base_url.clone()),
        }
    }

    fn is_available(&self) -> bool {
        self.status().map(|s| s.available).unwrap_or(false)
    }

    fn status(&self) -> MachineResult<MachineLiveStatus> {
        let url = self.current_url();
        let body = self.http.get(&url).map_err(MachineError::new)?;
        parse_mtconnect_current(&body).map_err(MachineError::new)
    }
}

/// Parse a MTConnectStreams `/current` document into a live status.
pub fn parse_mtconnect_current(xml: &str) -> Result<MachineLiveStatus, String> {
    if xml.trim().is_empty() {
        return Err("empty MTConnect body".into());
    }
    if !xml.contains("MTConnect") && !xml.contains("Streams") {
        return Err("response does not look like MTConnect XML".into());
    }
    let execution = first_element_text(xml, "Execution").or_else(|| first_element_text(xml, "execution")).unwrap_or_else(|| "UNKNOWN".into());
    let program = first_element_text(xml, "Program").or_else(|| first_element_text(xml, "program"));
    let tool_number = first_element_text(xml, "ToolNumber").or_else(|| first_element_text(xml, "tool_number")).and_then(|s| s.trim().parse().ok());
    let spindle_rpm =
        first_element_text(xml, "SpindleSpeed").or_else(|| first_element_text(xml, "spindle_speed")).and_then(|s| s.trim().parse().ok());
    let feedrate_mmpm =
        first_element_text(xml, "PathFeedrate").or_else(|| first_element_text(xml, "path_feedrate")).and_then(|s| s.trim().parse().ok());
    let available = !matches!(execution.to_ascii_uppercase().as_str(), "UNAVAILABLE" | "UNKNOWN" | "");
    Ok(MachineLiveStatus { available, execution, program, tool_number, spindle_rpm, feedrate_mmpm, message: None, source: "mtconnect".into() })
}

/// Tiny element text extractor — enough for MTConnect sample/event payloads.
fn first_element_text(xml: &str, local: &str) -> Option<String> {
    let open_candidates = [format!("<{local}>"), format!("<{local} "), format!("<{local}\t"), format!("<{local}\n")];
    let mut start = None;
    for open in &open_candidates {
        if let Some(i) = xml.find(open.as_str()) {
            let after = i + open.len();
            let content_start = if open.ends_with('>') { after } else { xml[after..].find('>')? + after + 1 };
            start = Some(content_start);
            break;
        }
    }
    let start = start?;
    let close = format!("</{local}>");
    let end = xml[start..].find(&close)? + start;
    let text = xml[start..end].trim();
    if text.is_empty() { None } else { Some(text.to_string()) }
}

fn http_get_blocking(url: &str) -> Result<String, String> {
    let url = url.trim();
    let rest = url.strip_prefix("http://").ok_or_else(|| "only http:// URLs are supported for StdHttpGet".to_string())?;
    let (host_port, path) = match rest.split_once('/') {
        Some((hp, p)) => (hp, format!("/{p}")),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| format!("bad port in `{host_port}`"))?),
        None => (host_port, 80u16),
    };
    if host != "127.0.0.1" && host != "localhost" {
        // Keep the first plugin hard-jailed to loopback so tests/dev cannot accidentally
        // hit shop-floor machines without an explicit future allow-list.
        return Err("StdHttpGet only allows 127.0.0.1 / localhost (loopback jail)".into());
    }
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    let mut stream = TcpStream::connect((host, port)).map_err(|e| format!("connect {host}:{port}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
    let req = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1).or_else(|| text.split("\n\n").nth(1)).ok_or_else(|| "HTTP response missing body".to_string())?;
    if !text.contains("200") && text.starts_with("HTTP/") {
        return Err(format!("HTTP error status in response head: {}", text.lines().next().unwrap_or("")));
    }
    Ok(body.to_string())
}

/// Sample Haas-ish `/current` document for fixtures.
pub fn sample_haas_current_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<MTConnectStreams xmlns="urn:mtconnect.org:MTConnectStreams:1.3">
  <Header creationTime="2026-01-01T00:00:00Z" sender="SolidCraftFake" version="1.3"/>
  <Streams>
    <DeviceStream name="VF-2" uuid="sc-fake-vf2">
      <ComponentStream component="Controller" name="controller">
        <Events>
          <Execution dataItemId="exec" name="execution" timestamp="2026-01-01T00:00:00Z">ACTIVE</Execution>
          <Program dataItemId="prog" name="program" timestamp="2026-01-01T00:00:00Z">O2001</Program>
        </Events>
      </ComponentStream>
      <ComponentStream component="Path" name="path">
        <Events>
          <ToolNumber dataItemId="tn" name="tool_number" timestamp="2026-01-01T00:00:00Z">7</ToolNumber>
        </Events>
        <Samples>
          <PathFeedrate dataItemId="pf" name="path_feedrate" timestamp="2026-01-01T00:00:00Z">1200</PathFeedrate>
          <SpindleSpeed dataItemId="ss" name="spindle_speed" timestamp="2026-01-01T00:00:00Z">8100</SpindleSpeed>
        </Samples>
      </ComponentStream>
    </DeviceStream>
  </Streams>
</MTConnectStreams>
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine_host::MachineHost;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn parses_sample_haas_xml() {
        let st = parse_mtconnect_current(sample_haas_current_xml()).unwrap();
        assert_eq!(st.execution, "ACTIVE");
        assert_eq!(st.program.as_deref(), Some("O2001"));
        assert_eq!(st.tool_number, Some(7));
        assert_eq!(st.spindle_rpm, Some(8100.0));
        assert_eq!(st.feedrate_mmpm, Some(1200.0));
        assert!(st.available);
    }

    #[test]
    fn backend_with_fixed_xml_is_testable() {
        let backend = MtConnectBackend::with_fixed_xml("http://127.0.0.1:8082/VF-2", sample_haas_current_xml());
        let host = MachineHost::new(backend);
        let st = host.status().unwrap();
        assert_eq!(st.tool_number, Some(7));
        assert_eq!(host.descriptor().protocol, "mtconnect");
    }

    #[test]
    fn std_http_get_against_loopback_agent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let xml = sample_haas_current_xml().to_string();
        thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            let body = xml;
            let resp = format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\nContent-Type: text/xml\r\n\r\n{body}", body.len());
            let _ = sock.write_all(resp.as_bytes());
        });
        let backend = MtConnectBackend::with_std(format!("http://127.0.0.1:{port}/VF-2"));
        let st = backend.status().unwrap();
        assert_eq!(st.execution, "ACTIVE");
        assert_eq!(st.tool_number, Some(7));
    }

    #[test]
    fn std_http_rejects_non_loopback() {
        let err = StdHttpGet.get("http://192.168.1.10:8082/VF-2/current").unwrap_err();
        assert!(err.contains("loopback"), "{err}");
    }
}
