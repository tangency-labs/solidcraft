//! Dictation in the Assistant (#10883): the microphone button next to the message box records
//! through the host's [`Recorder`] (the desktop app's microphone), and a second press sends the
//! recording to the speech-to-text endpoint chosen in Preferences → AI. The endpoint speaks the
//! OpenAI-compatible `POST {base}/audio/transcriptions`, as whisper.cpp's server, Speaches,
//! LocalAI and hosted services do. The text it answers goes into the message box for the user to
//! read and send: nothing is sent by itself.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use solidcraft_ui_text::{format as trf, tr};

use super::agents;
use super::backend::{HttpFn, HttpRequest, Secrets};
use crate::SolveApp;

/// The system keychain entry holding the endpoint's key.
pub const KEY_ID: &str = "dictation-endpoint";
/// Most bytes of audio sent in one request (the OpenAI endpoint's own limit is 25 MB).
pub const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;
/// Most characters of a transcription put in the message box.
const MAX_TEXT_CHARS: usize = 20_000;
/// How long a transcription may take, in seconds.
const TIMEOUT_S: u64 = 120;

/// The host's microphone. The desktop app records with the system's default input device; the
/// browser build has none.
pub trait Recorder: Send + Sync {
    /// Start recording, or say why it can't (no microphone, no permission).
    fn start(&self) -> Result<Box<dyn Recording>, String>;
}

/// A recording in progress.
pub trait Recording: Send {
    /// Stop and answer the recording as a WAV file (mono, 16-bit PCM).
    fn finish(self: Box<Self>) -> Result<Vec<u8>, String>;
}

/// Where dictation is.
#[derive(Default)]
pub enum State {
    #[default]
    Idle,
    Recording(Box<dyn Recording>),
    /// The endpoint is working on a recording; its answer comes on the channel.
    Transcribing(Receiver<Result<String, String>>),
}

impl State {
    pub fn recording(&self) -> bool {
        matches!(self, State::Recording(_))
    }
    pub fn transcribing(&self) -> bool {
        matches!(self, State::Transcribing(_))
    }
}

/// The settings a transcription needs, read on the window thread.
#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    /// The base URL, normalised (`http://127.0.0.1:8178/v1`).
    pub base_url: String,
    pub model: String,
    /// ISO-639-1 code; empty: the endpoint detects the language (`auto` in the preferences).
    pub language: String,
    /// A key is stored in the keychain for this endpoint.
    pub key: bool,
}

/// The endpoint set in the preferences, or None while none is set.
pub fn endpoint(app: &SolveApp) -> Option<Endpoint> {
    let p = &app.preferences;
    let base_url = agents::normalize_url(&p.dictation_url).ok()?;
    let model = p.dictation_model.trim();
    Some(Endpoint {
        base_url,
        model: if model.is_empty() { "whisper-1".into() } else { model.chars().take(200).collect() },
        language: spoken_language(&p.dictation_language, solidcraft_ui_text::language()),
        key: p.dictation_key,
    })
}

/// The language sent with a recording: the one set in the preferences, else the app's own
/// (`ui`, a locale id such as `it` or `pt-BR`), or none for `auto`, which lets the endpoint
/// detect it. Detecting costs a second pass over the audio in whisper.cpp (about 2 s on a
/// laptop CPU), so the app's language is the default.
pub fn spoken_language(set: &str, ui: &str) -> String {
    let set = set.trim();
    if set.eq_ignore_ascii_case("auto") {
        return String::new();
    }
    if !set.is_empty() {
        return set.chars().take(16).collect();
    }
    match ui.split('-').next().unwrap_or_default() {
        // Whisper names Norwegian `no`.
        "nb" => "no".into(),
        code => code.to_ascii_lowercase(),
    }
}

/// The microphone button is shown: the host can record and send requests, and an endpoint is set.
pub fn available(app: &SolveApp) -> bool {
    app.services.recorder.is_some() && app.services.agent_http.is_some() && endpoint(app).is_some()
}

/// The button (or its shortcut) was pressed: start recording, or stop and transcribe.
pub fn toggle(app: &mut SolveApp, ctx: &egui::Context) {
    match std::mem::take(&mut app.agent_chat.dictation) {
        State::Idle => start(app),
        State::Recording(recording) => transcribe(app, ctx, recording),
        busy @ State::Transcribing(_) => app.agent_chat.dictation = busy,
    }
}

fn start(app: &mut SolveApp) {
    let Some(recorder) = app.services.recorder.clone() else {
        app.agent_chat.notice(tr("ui.dictation_no_microphone"), true);
        return;
    };
    if endpoint(app).is_none() {
        app.agent_chat.notice(tr("ui.dictation_not_set_up"), true);
        return;
    }
    // The server loads its model while you speak.
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(e) = ensure_server(app) {
        app.agent_chat.notice(trf("ui.dictation_server_failed", &[("error", &e)]), true);
        return;
    }
    match recorder.start() {
        Ok(recording) => app.agent_chat.dictation = State::Recording(recording),
        Err(e) => app.agent_chat.notice(trf("ui.dictation_record_failed", &[("error", &e)]), true),
    }
}

fn transcribe(app: &mut SolveApp, ctx: &egui::Context, recording: Box<dyn Recording>) {
    let (Some(endpoint), Some(http)) = (endpoint(app), app.services.agent_http.clone()) else {
        drop(recording.finish());
        app.agent_chat.notice(tr("ui.dictation_not_set_up"), true);
        return;
    };
    let secrets = app.services.secrets.clone();
    #[cfg(not(target_arch = "wasm32"))]
    let server = app.agent_chat.dictation_server.as_ref().filter(|_| endpoint.base_url == LOCAL_URL).map(|s| s.service.watch());
    let (tx, rx) = mpsc::channel();
    let wake = ctx.clone();
    let language = solidcraft_ui_text::language();
    let spawned = std::thread::Builder::new().name("dictation".into()).spawn(move || {
        let _language = solidcraft_ui_text::scope(language);
        let result = recording.finish().and_then(|wav| {
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(server) = &server {
                wait_until_ready(server)?;
            }
            run(&endpoint, secrets.as_deref(), &http, wav)
        });
        drop(tx.send(result));
        wake.request_repaint();
    });
    match spawned {
        Ok(_) => app.agent_chat.dictation = State::Transcribing(rx),
        Err(e) => app.agent_chat.notice(trf("ui.dictation_failed", &[("error", &e.to_string())]), true),
    }
}

/// One transcription, beside the window: the key from the keychain, the request, the answer.
fn run(endpoint: &Endpoint, secrets: Option<&dyn Secrets>, http: &HttpFn, wav: Vec<u8>) -> Result<String, String> {
    let key = match (endpoint.key, secrets) {
        (true, Some(store)) => store.get(KEY_ID).map_err(|e| trf("ui.ai_keychain_error", &[("error", &e)]))?,
        _ => None,
    };
    let seconds = wav.len().saturating_sub(44) as f64 / f64::from(WAV_RATE * 2);
    let bytes = wav.len();
    let request = request(endpoint, key.as_deref(), wav)?;
    let started = std::time::Instant::now();
    let answer = http(&request);
    log::info!(
        "dictation: {seconds:.1} s of audio ({bytes} bytes, language {:?}) transcribed in {:.2} s",
        endpoint.language,
        started.elapsed().as_secs_f64()
    );
    let (status, body) = answer?;
    parse(status, &body)
}

/// The request for `wav`. A key goes along only where it may be sent (HTTPS or this computer).
pub fn request(endpoint: &Endpoint, key: Option<&str>, wav: Vec<u8>) -> Result<HttpRequest, String> {
    if wav.is_empty() {
        return Err(tr("ui.dictation_nothing_recorded").to_string());
    }
    if wav.len() > MAX_AUDIO_BYTES {
        return Err(tr("ui.dictation_too_long").to_string());
    }
    let mut fields = vec![("model", endpoint.model.as_str()), ("response_format", "json")];
    if !endpoint.language.is_empty() {
        fields.push(("language", endpoint.language.as_str()));
    }
    let (content_type, body) = multipart(&fields, "dictation.wav", "audio/wav", &wav);
    let mut headers = vec![("Content-Type".to_string(), content_type), ("Accept".to_string(), "application/json".to_string())];
    if let Some(key) = key.filter(|k| !k.is_empty()) {
        if !agents::key_safe_for(&endpoint.base_url) {
            return Err(tr("ui.dictation_key_needs_https").to_string());
        }
        headers.push(("Authorization".to_string(), format!("Bearer {key}")));
    }
    Ok(HttpRequest { method: "POST", url: format!("{}/audio/transcriptions", endpoint.base_url), headers, body: Some(body), timeout_s: TIMEOUT_S })
}

/// A `multipart/form-data` body: the text fields, then the file. Answers the Content-Type (with
/// its boundary) and the body.
pub fn multipart(fields: &[(&str, &str)], file_name: &str, mime: &str, data: &[u8]) -> (String, Vec<u8>) {
    // A boundary that the audio would have to contain by chance: the WAV bytes are not text, and
    // the fields are short values the code chose.
    let boundary = format!("----SolidCraftDictation{:016x}", fnv(data));
    let mut body = Vec::with_capacity(data.len() + 512);
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// The text of the endpoint's answer, or why there is none.
pub fn parse(status: u16, body: &[u8]) -> Result<String, String> {
    let json: Option<serde_json::Value> = serde_json::from_slice(body).ok();
    if !(200..300).contains(&status) {
        let detail = json
            .as_ref()
            .and_then(|v| v.pointer("/error/message").or_else(|| v.get("error")).or_else(|| v.get("detail")))
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(300).collect());
        return Err(trf("ui.dictation_endpoint_error", &[("status", &status.to_string()), ("error", detail.trim())]));
    }
    let text = match json.as_ref() {
        Some(v) => v.get("text").and_then(|t| t.as_str()).map(str::to_string).ok_or_else(|| tr("ui.dictation_bad_answer").to_string())?,
        // `response_format=text` servers answer the bare text.
        None => String::from_utf8_lossy(body).into_owned(),
    };
    Ok(text.trim().chars().take(MAX_TEXT_CHARS).collect())
}

/// The address the setup steps' server answers on.
pub const LOCAL_URL: &str = "http://127.0.0.1:8178/v1";

/// whisper.cpp's server, started by the app for dictation (Preferences: `dictation_server`) and
/// stopped when the app quits (or the setting changes).
#[cfg(not(target_arch = "wasm32"))]
pub struct Server {
    service: super::process::Service,
    /// What it was started with, to start it again when the settings change.
    argv: Vec<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Server {
    fn drop(&mut self) {
        self.service.handle.stop_service(true);
    }
}

/// The port of [`LOCAL_URL`].
pub const LOCAL_PORT: u16 = 8178;
/// The model the setup steps download.
pub const SMALL_MODEL: &str = "ggml-small.bin";
/// How long the server may take to load its model.
#[cfg(not(target_arch = "wasm32"))]
const SERVER_START: std::time::Duration = std::time::Duration::from_secs(120);

/// The model file for `server`: `model` if set, else [`SMALL_MODEL`] where the setup steps put it
/// (`models/` of the whisper.cpp folder for a build, the server's own folder for a release zip).
pub fn model_for(server: &std::path::Path, model: &str, exists: impl Fn(&std::path::Path) -> bool) -> Result<std::path::PathBuf, String> {
    let model = model.trim();
    if !model.is_empty() {
        let path = std::path::PathBuf::from(model);
        return if exists(&path) { Ok(path) } else { Err(trf("ui.dictation_model_missing", &[("path", model)])) };
    }
    let dir = server.parent().unwrap_or(std::path::Path::new("."));
    let candidates = [dir.join("../../models").join(SMALL_MODEL), dir.join("models").join(SMALL_MODEL), dir.join(SMALL_MODEL)];
    candidates.iter().find(|p| exists(p)).cloned().ok_or_else(|| {
        let looked = candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
        trf("ui.dictation_model_not_found", &[("paths", &looked)])
    })
}

/// The command that starts `server` with `model` on [`LOCAL_PORT`], as the setup steps' last
/// command does.
pub fn server_argv(server: &str, model: &std::path::Path) -> Vec<String> {
    let mut argv = vec![
        server.to_string(),
        "-m".into(),
        model.display().to_string(),
        "--port".into(),
        LOCAL_PORT.to_string(),
        "-l".into(),
        "auto".into(),
        "--inference-path".into(),
        "/v1/audio/transcriptions".into(),
    ];
    if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        // See RUN_MAC_INTEL.
        argv.push("-ng".into());
    }
    argv
}

/// Something answers on [`LOCAL_PORT`] (a server started by the app or by hand).
#[cfg(not(target_arch = "wasm32"))]
fn listening() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], LOCAL_PORT));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(150)).is_ok()
}

/// Start the server set in the preferences, unless it is running already or dictation goes
/// elsewhere. A server someone started by hand on the port is used as it is.
#[cfg(not(target_arch = "wasm32"))]
fn ensure_server(app: &mut SolveApp) -> Result<(), String> {
    let program = app.preferences.dictation_server.trim().to_string();
    if program.is_empty() || endpoint(app).is_none_or(|e| e.base_url != LOCAL_URL) {
        app.agent_chat.dictation_server = None;
        return Ok(());
    }
    let path = std::path::PathBuf::from(&program);
    if !path.is_file() {
        return Err(trf("ui.dictation_server_missing", &[("path", &program)]));
    }
    let model = model_for(&path, &app.preferences.dictation_server_model, |p| p.is_file())?;
    let argv = server_argv(&program, &model);
    if let Some(server) = &app.agent_chat.dictation_server
        && server.argv == argv
        && server.service.handle.running()
    {
        return Ok(());
    }
    // Stopped first, so the port is free for the new one.
    app.agent_chat.dictation_server = None;
    if listening() {
        return Ok(());
    }
    let cwd = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    log::info!("dictation: starting {argv:?}");
    let service = super::process::Service::spawn(&argv, &cwd)?;
    app.agent_chat.dictation_server = Some(Server { service, argv });
    Ok(())
}

/// Wait for the server the app started to take requests, or say why it stopped.
#[cfg(not(target_arch = "wasm32"))]
fn wait_until_ready(server: &super::process::ServiceWatch) -> Result<(), String> {
    let started = std::time::Instant::now();
    loop {
        if listening() {
            return Ok(());
        }
        if !server.running() {
            let why = server.last_line().unwrap_or_default();
            return Err(trf("ui.dictation_server_stopped", &[("error", &why)]));
        }
        if started.elapsed() > SERVER_START {
            return Err(tr("ui.dictation_server_slow").to_string());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// One step of setting up whisper.cpp's server on this computer: what to do (a message id) and
/// the command to copy, if the step has one.
pub struct SetupStep {
    pub text: &'static str,
    pub command: Option<&'static str>,
}

const RUN_UNIX: &str = "./build/bin/whisper-server -m models/ggml-small.bin --port 8178 -l auto --inference-path /v1/audio/transcriptions";
/// On an Intel Mac whisper.cpp's Metal backend is 5 to 10 times slower than the CPU (measured on
/// a 2018 MacBook Pro): `-ng` keeps it off the GPU.
const RUN_MAC_INTEL: &str = "./build/bin/whisper-server -m models/ggml-small.bin --port 8178 -l auto --inference-path /v1/audio/transcriptions -ng";
const BUILD_UNIX: &str = "git clone --depth 1 https://github.com/ggml-org/whisper.cpp && cd whisper.cpp && cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build -j 4 --target whisper-server";

/// The steps that set up whisper.cpp's server on this system, for Preferences → AI → Dictation:
/// build it (Linux, macOS) or download its release (Windows), download the multilingual `small`
/// model, start it on [`LOCAL_URL`].
pub fn setup_steps() -> Vec<SetupStep> {
    if cfg!(target_os = "windows") {
        vec![
            SetupStep { text: "ui.dictation_setup_download_windows", command: Some("https://github.com/ggml-org/whisper.cpp/releases/latest") },
            SetupStep {
                text: "ui.dictation_setup_model",
                command: Some("Invoke-WebRequest https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin -OutFile ggml-small.bin"),
            },
            SetupStep {
                text: "ui.dictation_setup_run",
                command: Some(".\\whisper-server.exe -m ggml-small.bin --port 8178 -l auto --inference-path /v1/audio/transcriptions"),
            },
        ]
    } else {
        vec![
            SetupStep {
                text: if cfg!(target_os = "macos") { "ui.dictation_setup_build_macos" } else { "ui.dictation_setup_build_unix" },
                command: Some(BUILD_UNIX),
            },
            SetupStep { text: "ui.dictation_setup_model", command: Some("sh ./models/download-ggml-model.sh small") },
            SetupStep {
                text: "ui.dictation_setup_run",
                command: Some(if cfg!(all(target_os = "macos", target_arch = "x86_64")) { RUN_MAC_INTEL } else { RUN_UNIX }),
            },
        ]
    }
}

/// The sample rate of the WAV files sent: what speech-to-text models work at.
pub const WAV_RATE: u32 = 16_000;

/// Mono samples (−1…1) taken at `rate` Hz as a WAV file: 16 kHz, mono, 16-bit PCM. Resampled
/// linearly, which is enough for speech.
pub fn wav_16k(samples: &[f32], rate: u32) -> Vec<u8> {
    let rate = rate.max(1);
    let out_len = usize::try_from(u64::try_from(samples.len()).unwrap_or(u64::MAX).saturating_mul(u64::from(WAV_RATE)) / u64::from(rate))
        .unwrap_or(0)
        .min(MAX_AUDIO_BYTES / 2);
    let step = f64::from(rate) / f64::from(WAV_RATE);
    let mut pcm = Vec::with_capacity(out_len * 2);
    for i in 0..out_len {
        let at = i as f64 * step;
        let k = at.floor() as usize;
        let frac = (at - at.floor()) as f32;
        let a = samples.get(k).copied().unwrap_or(0.0);
        let b = samples.get(k + 1).copied().unwrap_or(a);
        let v = a + (b - a) * frac;
        let v = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
        pcm.extend_from_slice(&((v * f32::from(i16::MAX)).round() as i16).to_le_bytes());
    }
    let data_len = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&data_len.saturating_add(36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&WAV_RATE.to_le_bytes());
    wav.extend_from_slice(&(WAV_RATE * 2).to_le_bytes()); // bytes per second
    wav.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&pcm);
    wav
}

/// Put the endpoint's answer into the message box, after what is there.
pub fn poll(app: &mut SolveApp) {
    // The microphone never stays on behind a closed panel: closing the chat (or turning AI
    // features off) drops the recording, and nothing is sent.
    if app.agent_chat.dictation.recording() && !(app.agent_chat.open && super::shown(app)) {
        app.agent_chat.dictation = State::Idle;
        return;
    }
    let State::Transcribing(rx) = &app.agent_chat.dictation else { return };
    let answer = match rx.try_recv() {
        Ok(answer) => answer,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => Err(tr("ui.dictation_no_answer").to_string()),
    };
    app.agent_chat.dictation = State::Idle;
    match answer {
        Ok(text) if text.is_empty() => app.agent_chat.notice(tr("ui.dictation_nothing_heard"), false),
        Ok(text) => insert(&mut app.agent_chat.input, &text, &mut app.agent_chat.caret_to),
        Err(e) => app.agent_chat.notice(trf("ui.dictation_failed", &[("error", &e)]), true),
    }
}

/// Add `text` after `input`, with a space between them, and put the caret after it.
fn insert(input: &mut String, text: &str, caret: &mut Option<usize>) {
    if !input.is_empty() && !input.ends_with(char::is_whitespace) {
        input.push(' ');
    }
    input.push_str(text);
    *caret = Some(input.chars().count());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn local() -> Endpoint {
        Endpoint { base_url: "http://127.0.0.1:8178/v1".into(), model: "whisper-1".into(), language: "it".into(), key: false }
    }

    #[test]
    fn the_request_posts_the_audio_and_the_fields_as_a_form() {
        let r = request(&local(), None, b"RIFFdata".to_vec()).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.url, "http://127.0.0.1:8178/v1/audio/transcriptions");
        let (_, content_type) = r.headers.iter().find(|(h, _)| h == "Content-Type").unwrap();
        let boundary = content_type.strip_prefix("multipart/form-data; boundary=").unwrap();
        let body = String::from_utf8_lossy(r.body.as_ref().unwrap()).into_owned();
        assert!(body.starts_with(&format!("--{boundary}\r\n")));
        assert!(body.ends_with(&format!("\r\n--{boundary}--\r\n")));
        assert!(body.contains("name=\"model\"\r\n\r\nwhisper-1\r\n"));
        assert!(body.contains("name=\"language\"\r\n\r\nit\r\n"));
        assert!(body.contains("name=\"file\"; filename=\"dictation.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFFdata\r\n"));
        assert!(!r.headers.iter().any(|(h, _)| h == "Authorization"));
    }

    #[test]
    fn the_model_is_found_where_the_setup_steps_put_it() {
        crate::i18n::install();
        let built = std::path::Path::new("/w/whisper.cpp/build/bin/whisper-server");
        let in_models = |p: &std::path::Path| p == std::path::Path::new("/w/whisper.cpp/build/bin/../../models/ggml-small.bin");
        assert_eq!(model_for(built, "", in_models).unwrap(), std::path::Path::new("/w/whisper.cpp/build/bin/../../models/ggml-small.bin"));
        let zip = std::path::Path::new("C:/whisper/whisper-server.exe");
        let beside = |p: &std::path::Path| p == std::path::Path::new("C:/whisper/ggml-small.bin");
        assert_eq!(model_for(zip, "", beside).unwrap(), std::path::Path::new("C:/whisper/ggml-small.bin"));
        // A model set by hand wins, and must exist.
        assert_eq!(model_for(built, " /m/base.bin ", |_| true).unwrap(), std::path::Path::new("/m/base.bin"));
        assert!(model_for(built, "/m/base.bin", |_| false).unwrap_err().contains("/m/base.bin"));
        // Nothing found: the message says where it looked.
        assert!(model_for(built, "", |_| false).unwrap_err().contains("ggml-small.bin"));
    }

    #[test]
    fn the_server_starts_as_the_setup_steps_start_it() {
        let argv = server_argv("/w/whisper-server", std::path::Path::new("/w/ggml-small.bin"));
        let line = argv.join(" ");
        assert!(line.starts_with("/w/whisper-server -m /w/ggml-small.bin --port 8178 -l auto --inference-path /v1/audio/transcriptions"));
        assert_eq!(LOCAL_URL, format!("http://127.0.0.1:{LOCAL_PORT}/v1"));
        // The command shown in the setup steps starts the same server.
        for step in setup_steps() {
            if step.text == "ui.dictation_setup_run" {
                let shown = step.command.unwrap();
                assert!(shown.contains("--port 8178 -l auto --inference-path /v1/audio/transcriptions"), "{shown}");
                assert_eq!(shown.ends_with(" -ng"), line.ends_with(" -ng"));
            }
        }
    }

    #[test]
    fn the_spoken_language_defaults_to_the_apps() {
        assert_eq!(spoken_language("", "it"), "it");
        assert_eq!(spoken_language("  ", "pt-BR"), "pt");
        assert_eq!(spoken_language("", "zh-TW"), "zh");
        assert_eq!(spoken_language("", "nb"), "no");
        assert_eq!(spoken_language("de", "it"), "de");
        assert_eq!(spoken_language(" Auto ", "it"), "");
        assert_eq!(spoken_language(&"x".repeat(40), "it").len(), 16);
    }

    #[test]
    fn no_language_lets_the_endpoint_detect_it() {
        let r = request(&Endpoint { language: String::new(), ..local() }, None, b"RIFF".to_vec()).unwrap();
        assert!(!String::from_utf8_lossy(r.body.as_ref().unwrap()).contains("name=\"language\""));
    }

    #[test]
    fn a_key_goes_only_over_https_or_to_this_computer() {
        let r = request(&local(), Some("sk-1"), b"RIFF".to_vec()).unwrap();
        assert!(r.headers.contains(&("Authorization".to_string(), "Bearer sk-1".to_string())));
        let remote = Endpoint { base_url: "http://192.168.1.5:8000/v1".into(), ..local() };
        assert!(request(&remote, Some("sk-1"), b"RIFF".to_vec()).is_err());
        assert!(request(&remote, None, b"RIFF".to_vec()).is_ok());
    }

    #[test]
    fn empty_or_huge_recordings_are_not_sent() {
        assert!(request(&local(), None, Vec::new()).is_err());
        assert!(request(&local(), None, vec![0; MAX_AUDIO_BYTES + 1]).is_err());
    }

    #[test]
    fn the_answer_is_the_text_trimmed() {
        assert_eq!(parse(200, br#"{"text":" Crea un cubo di 20 mm.\n"}"#).unwrap(), "Crea un cubo di 20 mm.");
        assert_eq!(parse(200, b" plain text answer \n").unwrap(), "plain text answer");
        assert!(parse(200, br#"{"other":1}"#).is_err());
    }

    #[test]
    fn an_endpoint_error_says_what_the_endpoint_said() {
        // The message comes from the catalogs, which this test may be the first to need.
        crate::i18n::install();
        let e = parse(401, br#"{"error":{"message":"Invalid API key"}}"#).unwrap_err();
        assert!(e.contains("401") && e.contains("Invalid API key"), "{e}");
        let e = parse(500, b"boom").unwrap_err();
        assert!(e.contains("500") && e.contains("boom"), "{e}");
    }

    #[test]
    fn a_recording_becomes_a_16_khz_mono_wav() {
        // One second at 48 kHz: a 16 kHz file of one second, 16000 samples of 2 bytes.
        let samples: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        let wav = wav_16k(&samples, 48_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 32_000);
        assert_eq!(wav.len(), 44 + 32_000);
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize, wav.len() - 8);
    }

    #[test]
    fn hostile_samples_and_rates_make_a_valid_file() {
        for rate in [0, 1, 8_000, 44_100, 192_000] {
            let wav = wav_16k(&[f32::NAN, f32::INFINITY, -5.0, 5.0, 0.25], rate);
            assert!(wav.len() >= 44 && (wav.len() - 44).is_multiple_of(2));
        }
        let loud = wav_16k(&[5.0; 16], 16_000);
        assert_eq!(i16::from_le_bytes(loud[44..46].try_into().unwrap()), i16::MAX);
        assert_eq!(wav_16k(&[], 48_000).len(), 44);
    }

    #[test]
    fn the_text_goes_after_what_is_typed() {
        let mut input = "Make a plate".to_string();
        let mut caret = None;
        insert(&mut input, "100 by 70 mm", &mut caret);
        assert_eq!(input, "Make a plate 100 by 70 mm");
        assert_eq!(caret, Some(input.chars().count()));
        let mut empty = String::new();
        insert(&mut empty, "ciao", &mut caret);
        assert_eq!(empty, "ciao");
    }
}
