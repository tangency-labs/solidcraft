//! Reading a 3D mouse through `smed`, the SpaceMouse Enterprise driver daemon, on Linux. smed owns
//! the device and reports all of it over a Unix socket in JSON lines (its `docs/api.md`): the six
//! raw axes, every key by its 3Dconnexion id with whether it was a short or a long press, and the
//! rotation lock it keeps (its Lock key switches it; while it is on smed sends no rotation). The
//! app can switch the lock too. Message types come from the `smed-client` crate; the connection
//! is plain socket I/O here, so a test can stand in for the daemon with a socket pair.

use smed_client::{API_VERSION, ClientMessage, Press, ServerMessage, to_line};
use solidcraft_ui_egui::space_mouse::Axes;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// The longest line smed sends is far below this; a longer one is no smed and ends the read.
const MAX_LINE: usize = 64 * 1024;

/// Whether smed's socket is there, without connecting to it.
pub fn exists() -> bool {
    smed_client::default_socket_path().is_some_and(|path| path.exists())
}

pub fn connect() -> Option<UnixStream> {
    UnixStream::connect(smed_client::default_socket_path()?).ok()
}

/// What smed reported.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The device appeared (its vendor and product ids) or went away (`None`), with the lock when
    /// the message carried it (`state` does, `device` does not).
    Device(Option<(u16, u16)>, Option<bool>),
    Motion(Axes),
    /// A key by its 3Dconnexion id, a long press or a short one, down or up.
    Key {
        id: u8,
        long: bool,
        down: bool,
    },
    Lock(bool),
}

/// The events of one message; none for one the app does not use.
fn events(message: ServerMessage) -> Vec<Event> {
    match message {
        ServerMessage::State { device, lock } => vec![Event::Device(device.map(|d| (d.vid, d.pid)), Some(lock))],
        ServerMessage::Device { device } => vec![Event::Device(device.map(|d| (d.vid, d.pid)), None)],
        ServerMessage::Motion { t, r } => vec![Event::Motion(Axes { translation: t, rotation: r })],
        ServerMessage::Key { id, press, down, .. } => vec![Event::Key { id, long: press == Press::Long, down }],
        ServerMessage::Lock { on } => vec![Event::Lock(on)],
        _ => Vec::new(),
    }
}

/// Talks to smed until it closes the connection, an error ends it, or `stop` says to: the
/// handshake, then every message as `on` events. `stop` and `lock` (a change of the rotation lock
/// to send) are asked at every wake-up, so the stream needs a read timeout. Returns whether smed
/// answered the handshake: false when it refused it, went away before answering, or sent its hello
/// and first state not within `answer_within` (a socket that accepts and stays silent is no smed to
/// wait for). Once that state is in, a quiet device is no reason to end the read.
pub fn read(
    mut stream: UnixStream,
    answer_within: Duration,
    mut on: impl FnMut(Event),
    mut stop: impl FnMut() -> bool,
    mut lock: impl FnMut() -> Option<bool>,
) -> bool {
    let started = Instant::now();
    let hello = ClientMessage::Hello { api: API_VERSION, client: "solidcraft".into() };
    if let Err(error) = stream.write_all(to_line(&hello).as_bytes()) {
        log::info!("3D mouse: smed: {error}");
        return false;
    }
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut greeted = false;
    let mut answered = false;
    loop {
        if stop() {
            return greeted;
        }
        if !answered && started.elapsed() >= answer_within {
            log::info!("3D mouse: smed did not answer within {answer_within:?}; trying again later");
            return false;
        }
        if let Some(on) = lock() {
            let message = ClientMessage::SetLock { on };
            if let Err(error) = stream.write_all(to_line(&message).as_bytes()) {
                log::info!("3D mouse: smed: {error}");
                return greeted;
            }
        }
        match stream.read(&mut chunk) {
            Ok(0) => return greeted,
            Ok(len) => pending.extend_from_slice(chunk.get(..len).unwrap_or_default()),
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => continue,
            Err(error) => {
                log::info!("3D mouse: smed: {error}");
                return greeted;
            }
        }
        while let Some(end) = pending.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let message = match serde_json::from_slice::<ServerMessage>(&line) {
                Ok(message) => message,
                Err(error) => {
                    log::warn!("3D mouse: smed sent a line that does not read ({error}); ignoring it");
                    continue;
                }
            };
            if !greeted {
                match message {
                    ServerMessage::Hello { api, server } => {
                        log::info!("3D mouse: connected to {server} (API {api})");
                        greeted = true;
                        continue;
                    }
                    ServerMessage::Error { message } => {
                        log::warn!("3D mouse: smed refused the connection: {message}");
                        return false;
                    }
                    _ => {}
                }
            }
            answered |= greeted && matches!(message, ServerMessage::State { .. });
            for event in events(message) {
                on(event);
            }
        }
        if pending.len() > MAX_LINE {
            log::warn!("3D mouse: smed sent an overlong line; disconnecting");
            return greeted;
        }
    }
}

/// Asks smed once whether it has a device, without reading it: the handshake, then its first
/// state, then the connection closes. None when it does not answer (no smed, a refusal, or no state
/// before `wait` is up); the stream needs a read timeout well below `wait`.
pub fn has_device(stream: UnixStream, wait: Duration) -> Option<bool> {
    let until = Instant::now() + wait;
    let device = std::cell::Cell::new(None);
    let greeted = read(
        stream,
        wait,
        |event| {
            if let Event::Device(found, _) = event {
                device.set(Some(found.is_some()));
            }
        },
        || device.get().is_some() || Instant::now() >= until,
        || None,
    );
    device.get().filter(|_| greeted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    /// Long enough for every test daemon to answer.
    const ANSWER: Duration = Duration::from_secs(5);

    /// A daemon for the test: answers the handshake, sends `lines`, and hands back what the client
    /// sent after its hello.
    fn daemon(lines: &'static [&'static str]) -> (UnixStream, std::thread::JoinHandle<Vec<String>>) {
        let (client, server) = UnixStream::pair().unwrap();
        let handle = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let mut writer = server;
            let mut hello = String::new();
            reader.read_line(&mut hello).unwrap();
            assert!(hello.contains(r#""type":"hello""#) && hello.contains(r#""client":"solidcraft""#), "{hello}");
            writer.write_all(b"{\"type\":\"hello\",\"api\":1,\"server\":\"smed test\"}\n").unwrap();
            for line in lines {
                writer.write_all(line.as_bytes()).unwrap();
                writer.write_all(b"\n").unwrap();
            }
            // Wait for what the client sends, until it hangs up.
            let mut sent = Vec::new();
            writer.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
                sent.push(line.trim().to_owned());
                line.clear();
            }
            sent
        });
        client.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        (client, handle)
    }

    /// The device, its motion, a short and a long key and the lock come through as events; an
    /// unknown message and a line that does not read are skipped.
    #[test]
    fn messages_become_events_and_unknown_ones_are_skipped() {
        let (client, daemon) = daemon(&[
            r#"{"type":"state","device":{"vid":9583,"pid":50739,"name":"SpaceMouse Enterprise","firmware":"4.42"},"lock":true}"#,
            r#"{"type":"motion","t":[1,-2,3],"r":[0,0,0]}"#,
            r#"{"type":"lcd","page":1}"#,
            "not json",
            r#"{"type":"key","key":"space","id":176,"press":"short","down":true}"#,
            r#"{"type":"key","key":"t","id":3,"press":"long","down":false}"#,
            r#"{"type":"lock","on":false}"#,
            r#"{"type":"device","device":null}"#,
        ]);
        let mut seen = Vec::new();
        let mut reads = 0;
        read(
            client,
            ANSWER,
            |event| seen.push(event),
            || {
                reads += 1;
                reads > 50
            },
            || None,
        );
        assert_eq!(
            seen,
            vec![
                Event::Device(Some((0x256f, 0xc633)), Some(true)),
                Event::Motion(Axes { translation: [1, -2, 3], rotation: [0, 0, 0] }),
                Event::Key { id: 176, long: false, down: true },
                Event::Key { id: 3, long: true, down: false },
                Event::Lock(false),
                Event::Device(None, None),
            ]
        );
        drop(daemon);
    }

    /// A lock change asked for is sent to smed as `set_lock`, once.
    #[test]
    fn a_lock_request_is_sent_once() {
        let (client, daemon) = daemon(&[]);
        let mut request = Some(true);
        let mut reads = 0;
        read(
            client,
            ANSWER,
            |_| {},
            || {
                reads += 1;
                reads > 5
            },
            || request.take(),
        );
        assert_eq!(daemon.join().unwrap(), vec![r#"{"type":"set_lock","on":true}"#.to_owned()]);
    }

    /// A smed that refuses the handshake ends the read.
    #[test]
    fn a_refused_handshake_ends_the_read() {
        let (client, server) = UnixStream::pair().unwrap();
        let mut writer = server.try_clone().unwrap();
        writer.write_all(b"{\"type\":\"error\",\"message\":\"unsupported API version 9\"}\n").unwrap();
        client.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        let mut seen = Vec::new();
        assert!(!read(client, ANSWER, |event| seen.push(event), || false, || None), "a refusal is no handshake");
        assert!(seen.is_empty());
    }

    /// A socket that accepts and does not answer, or answers the hello and sends no state (nothing,
    /// or other messages), ends the read after the wait as a refusal does, whatever `stop` says: it
    /// is not waited for forever.
    #[test]
    fn a_smed_that_does_not_answer_ends_the_read_after_the_wait() {
        let hello = "{\"type\":\"hello\",\"api\":1,\"server\":\"smed test\"}\n";
        let motion = "{\"type\":\"motion\",\"t\":[1,0,0],\"r\":[0,0,0]}\n";
        for said in [String::new(), hello.to_owned(), format!("{hello}{motion}")] {
            let (client, server) = UnixStream::pair().unwrap();
            server.try_clone().unwrap().write_all(said.as_bytes()).unwrap();
            client.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
            let (done, ended) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut seen = Vec::new();
                let greeted = read(client, Duration::from_millis(200), |event| seen.push(event), || false, || None);
                done.send((greeted, seen)).unwrap();
                drop(server);
            });
            let (greeted, _) = ended.recv_timeout(Duration::from_secs(3)).unwrap_or_else(|_| panic!("{said:?}: still waiting"));
            assert!(!greeted, "{said:?}: no state is no answer");
        }
    }

    /// Once the first state is in, a quiet device keeps the read going past the wait.
    #[test]
    fn a_quiet_smed_is_read_on_after_its_first_state() {
        let (client, server) = UnixStream::pair().unwrap();
        let mut writer = server.try_clone().unwrap();
        writer.write_all(b"{\"type\":\"hello\",\"api\":1,\"server\":\"smed test\"}\n").unwrap();
        writer.write_all(b"{\"type\":\"state\",\"device\":null,\"lock\":false}\n").unwrap();
        client.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        let started = Instant::now();
        let greeted = read(client, Duration::from_millis(100), |_| {}, || started.elapsed() >= Duration::from_millis(400), || None);
        assert!(greeted && started.elapsed() >= Duration::from_millis(400), "ended by stop, not by the wait");
        drop(server);
    }

    /// The probe answers from smed's first state: a device or none; a refusal is no answer.
    #[test]
    fn has_device_reads_the_first_state() {
        let wait = Duration::from_secs(2);
        let (client, _daemon) =
            daemon(&[r#"{"type":"state","device":{"vid":9583,"pid":50739,"name":"SpaceMouse Enterprise","firmware":null},"lock":false}"#]);
        assert_eq!(has_device(client, wait), Some(true));
        let (client, _daemon) = daemon(&[r#"{"type":"state","device":null,"lock":false}"#]);
        assert_eq!(has_device(client, wait), Some(false));
        let (client, server) = UnixStream::pair().unwrap();
        server.try_clone().unwrap().write_all(b"{\"type\":\"error\",\"message\":\"unsupported API version 9\"}\n").unwrap();
        client.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        assert_eq!(has_device(client, wait), None);
    }
}
