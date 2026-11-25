//! Real connections over loopback: a TCP echo server, a TLS server with a test certificate
//! authority, and an SSH server with test keys made for the run. Nothing leaves the machine.
use crate::{Connections, Options, Request};
use craft_plugin::{ConnectPermission, HostError, Transport};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

fn allow(host: &str, port: Option<u16>, transport: Transport) -> Vec<ConnectPermission> {
    vec![ConnectPermission { host: host.into(), port, transport }]
}

/// How long one test may wait on the network before it fails instead of hanging.
const TEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

thread_local! {
    // Each test runs on a thread of its own: its deadline starts with its first wait.
    static DEADLINE: std::time::Instant = std::time::Instant::now() + TEST_DEADLINE;
}

/// The `Wait` of a test: fine until the test's deadline, then an error, so a stuck exchange fails.
fn go() -> craft_plugin::Result<()> {
    if DEADLINE.with(|deadline| std::time::Instant::now() > *deadline) {
        return Err(HostError::LimitExceeded { limit: format!("the test's {} s", TEST_DEADLINE.as_secs()) });
    }
    Ok(())
}

fn read_all(connections: &mut Connections, id: u32) -> Vec<u8> {
    let mut all = Vec::new();
    loop {
        let chunk = connections.read(id, 4096, &mut go).unwrap();
        if chunk.is_empty() {
            return all;
        }
        all.extend(chunk);
    }
}

#[test]
fn only_the_endpoints_the_manifest_names_are_reached() {
    let mut connections = Connections::new(allow("*.example.com", Some(443), Transport::Tls), Options::default());
    for (host, port, request) in [("example.org", 443, Request::Tls), ("git.example.com", 80, Request::Tls), ("git.example.com", 443, Request::Tcp)] {
        let refused = connections.open(host, port, request, &mut go).unwrap_err();
        assert!(matches!(&refused, HostError::PermissionDenied { permission } if permission.contains("permissions.connect")), "{refused}");
    }
    assert!(matches!(connections.open("bad host", 443, Request::Tls, &mut go), Err(HostError::InvalidInput { .. })));
    assert!(matches!(connections.read(7, 10, &mut go), Err(HostError::InvalidInput { .. })));
}

#[test]
fn the_users_allowlist_limits_what_the_plugin_was_allowed() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let only = |text: &str| Options { allowed_hosts: vec![craft_plugin::AllowedHost::try_from(text.to_string()).unwrap()], ..Options::default() };
    // The plugin may reach any host, but the user lists other servers: refused before connecting.
    let mut elsewhere = Connections::new(allow("*", None, Transport::Tcp), only("github.com"));
    let refused = elsewhere.open("127.0.0.1", port, Request::Tcp, &mut go).unwrap_err();
    assert!(matches!(&refused, HostError::PermissionDenied { permission } if permission.contains("allowed servers")), "{refused}");
    let mut other_port = Connections::new(allow("*", None, Transport::Tcp), only("127.0.0.1:1"));
    assert!(matches!(other_port.open("127.0.0.1", port, Request::Tcp, &mut go), Err(HostError::PermissionDenied { .. })));
    // On the list: the connection is made.
    let mut listed = Connections::new(allow("*", None, Transport::Tcp), only(&format!("127.0.0.1:{port}")));
    listed.open("127.0.0.1", port, Request::Tcp, &mut go).unwrap();
}

#[test]
fn plain_tcp_carries_bytes_both_ways_and_ends() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_read_timeout(Some(TEST_DEADLINE)).unwrap();
        let mut request = Vec::new();
        socket.read_to_end(&mut request).unwrap();
        socket.write_all(b"got ").unwrap();
        socket.write_all(&request).unwrap();
    });
    let mut connections = Connections::new(allow("127.0.0.1", None, Transport::Tcp), Options::default());
    let id = connections.open("127.0.0.1", port, Request::Tcp, &mut go).unwrap();
    connections.write(id, b"hello", &mut go).unwrap();
    connections.finish(id, &mut go).unwrap();
    assert_eq!(read_all(&mut connections, id), b"got hello");
    server.join().unwrap();
}

#[test]
fn a_cancelled_call_stops_waiting_for_a_silent_server() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut connections = Connections::new(allow("127.0.0.1", None, Transport::Tcp), Options::default());
    let id = connections.open("127.0.0.1", port, Request::Tcp, &mut go).unwrap();
    let mut waits = 0;
    let mut cancel_soon = || {
        waits += 1;
        if waits > 2 { Err(HostError::Cancelled) } else { Ok(()) }
    };
    assert!(matches!(connections.read(id, 10, &mut cancel_soon), Err(HostError::Cancelled)));
    drop(listener);
}

/// A certificate authority and a server certificate for `localhost`, made for this run.
fn test_certificates() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut ca_params = rcgen::CertificateParams::new(Vec::new()).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let issuer = rcgen::Issuer::new(ca_params, ca_key);
    let server_key = rcgen::KeyPair::generate().unwrap();
    let server = rcgen::CertificateParams::new(vec!["localhost".into()]).unwrap().signed_by(&server_key, &issuer).unwrap();
    (ca.der().to_vec(), server.der().to_vec(), server_key.serialize_der())
}

fn tls_server(certificate: Vec<u8>, key: Vec<u8>) -> (u16, thread::JoinHandle<()>) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![rustls::pki_types::CertificateDer::from(certificate)], rustls::pki_types::PrivateKeyDer::Pkcs8(key.into()))
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket.set_read_timeout(Some(TEST_DEADLINE)).unwrap();
        let connection = rustls::ServerConnection::new(Arc::new(config)).unwrap();
        let mut stream = rustls::StreamOwned::new(connection, socket);
        let mut line = String::new();
        if BufReader::new(&mut stream).read_line(&mut line).is_ok() {
            let _ = stream.write_all(format!("secret {line}").as_bytes());
            stream.conn.send_close_notify();
            let _ = stream.flush();
        }
    });
    (port, server)
}

#[test]
fn tls_checks_the_certificate_and_then_carries_bytes() {
    let (ca, certificate, key) = test_certificates();
    let (port, server) = tls_server(certificate.clone(), key.clone());
    let options = Options { tls_roots: vec![ca], ..Options::default() };
    let mut connections = Connections::new(allow("localhost", None, Transport::Tls), options);
    let id = connections.open("localhost", port, Request::Tls, &mut go).unwrap();
    connections.write(id, b"hello\n", &mut go).unwrap();
    assert_eq!(read_all(&mut connections, id), b"secret hello\n");
    server.join().unwrap();

    // The system's trusted roots don't know the test authority: the handshake is refused.
    let (port, server) = tls_server(certificate, key);
    let mut connections = Connections::new(allow("localhost", None, Transport::Tls), Options::default());
    let refused = connections.open("localhost", port, Request::Tls, &mut go).unwrap_err();
    assert!(matches!(&refused, HostError::CommandFailed { reason, .. } if reason.contains("TLS")), "{refused}");
    let _ = server.join();
}

mod ssh {
    use super::*;
    use russh::keys::ssh_key::private::{Ed25519Keypair, KeypairData};
    use russh::keys::{PrivateKey, PublicKey};
    use russh::server::{Auth, Msg, Session};
    use russh::{Channel, ChannelId};

    /// A throwaway key from a seed made for this run: no key material lives in the source.
    fn key(salt: u8) -> PrivateKey {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let mut seed = [salt; 32];
        for (i, byte) in nanos.to_le_bytes().iter().chain(std::process::id().to_le_bytes().iter()).enumerate() {
            seed[i % 32] ^= byte.wrapping_add(salt);
        }
        PrivateKey::new(KeypairData::Ed25519(Ed25519Keypair::from_seed(&seed)), "test").unwrap()
    }

    /// Accepts one client key and runs `shout` (echoes its input in capitals), `fail`, `killed`
    /// (prints, then a signal stops it) or refuses to run `refuse`.
    struct Server {
        client: PublicKey,
        command: String,
    }

    impl russh::server::Handler for Server {
        type Error = russh::Error;

        async fn auth_publickey_offered(&mut self, _user: &str, offer: russh::server::PublicKeyOffer<'_>) -> Result<Auth, Self::Error> {
            let _ = offer;
            Ok(Auth::Accept)
        }

        async fn auth_publickey(&mut self, user: &str, key: russh::server::VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
            if user == "git" && key.public_key.key_data() == self.client.key_data() {
                Ok(Auth::Accept)
            } else {
                Ok(Auth::Reject { proceed_with_methods: None, partial_success: false })
            }
        }

        async fn channel_open_session(
            &mut self,
            _channel: Channel<Msg>,
            reply: russh::server::ChannelOpenHandle,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            reply.accept().await;
            Ok(())
        }

        async fn exec_request(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
            self.command = String::from_utf8_lossy(data).into_owned();
            if self.command == "refuse" {
                session.channel_failure(channel)?;
                session.close(channel)?;
                return Ok(());
            }
            session.channel_success(channel)?;
            if self.command == "killed" {
                session.data(channel, b"half a pack".to_vec())?;
                session.exit_signal_request(channel, russh::Sig::KILL, false, "killed", "")?;
                session.eof(channel)?;
                session.close(channel)?;
            }
            if self.command == "fail" {
                session.extended_data(channel, 1, b"fatal: no such repository\n".to_vec())?;
                session.exit_status_request(channel, 128)?;
                session.eof(channel)?;
                session.close(channel)?;
            }
            Ok(())
        }

        async fn data(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
            session.data(channel, data.to_ascii_uppercase())?;
            Ok(())
        }

        async fn channel_eof(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
            session.exit_status_request(channel, 0)?;
            session.eof(channel)?;
            session.close(channel)?;
            Ok(())
        }
    }

    /// An SSH server on loopback for one client, and the known-hosts line naming its key.
    fn server(client: PublicKey) -> (u16, String, thread::JoinHandle<()>) {
        let host = key(7);
        let known = format!("[127.0.0.1]:{{port}} {}", host.public_key().to_openssh().unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let handle = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                // Bounded, so a stuck exchange ends the server too.
                let _ = tokio::time::timeout(TEST_DEADLINE, async move {
                    let Ok((socket, _)) = listener.accept().await else { return };
                    let config = Arc::new(russh::server::Config { keys: vec![host], ..Default::default() });
                    if let Ok(session) = russh::server::run_stream(config, socket, Server { client, command: String::new() }).await {
                        let _ = session.await;
                    }
                })
                .await;
            });
        });
        (port, known.replace("{port}", &port.to_string()), handle)
    }

    fn options(known_hosts: &str, client: &PrivateKey) -> (tempfile::TempDir, Options) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("known_hosts"), format!("{known_hosts}\n")).unwrap();
        let key = dir.path().join("id_ed25519");
        std::fs::write(&key, client.to_openssh(Default::default()).unwrap().as_bytes()).unwrap();
        let options = Options {
            known_hosts: Some(dir.path().join("known_hosts")),
            ssh_keys: Some(vec![key]),
            ssh_agent: false,
            tls_roots: Vec::new(),
            allowed_hosts: Vec::new(),
        };
        (dir, options)
    }

    fn ssh(command: &str) -> Request {
        Request::Ssh { user: "git".into(), command: command.into() }
    }

    #[test]
    fn ssh_signs_in_runs_the_command_and_carries_its_input_and_output() {
        let client = key(1);
        let (port, known, server) = server(client.public_key().clone());
        let (_dir, options) = options(&known, &client);
        let mut connections = Connections::new(allow("127.0.0.1", None, Transport::Ssh), options);
        let id = connections.open("127.0.0.1", port, ssh("shout"), &mut go).unwrap();
        connections.write(id, b"hello ", &mut go).unwrap();
        connections.write(id, b"pack", &mut go).unwrap();
        connections.finish(id, &mut go).unwrap();
        assert_eq!(read_all(&mut connections, id), b"HELLO PACK");
        connections.close(id);
        drop(connections);
        server.join().unwrap();
    }

    #[test]
    fn a_failed_ssh_command_says_what_it_printed() {
        let client = key(2);
        let (port, known, server) = server(client.public_key().clone());
        let (_dir, options) = options(&known, &client);
        let mut connections = Connections::new(allow("127.0.0.1", None, Transport::Ssh), options);
        let id = connections.open("127.0.0.1", port, ssh("fail"), &mut go).unwrap();
        let failed = loop {
            match connections.read(id, 4096, &mut go) {
                Ok(chunk) if chunk.is_empty() => panic!("the failure was not reported"),
                Ok(_) => {}
                Err(error) => break error,
            }
        };
        assert!(
            matches!(&failed, HostError::CommandFailed { command, reason } if command == "fail" && reason.contains("no such repository")),
            "{failed}"
        );
        drop(connections);
        server.join().unwrap();
    }

    #[test]
    fn a_command_the_server_refuses_or_a_signal_stops_is_an_error() {
        let client = key(4);
        let (port, known, refusing) = server(client.public_key().clone());
        let (_dir, refusing_options) = options(&known, &client);
        let mut connections = Connections::new(allow("127.0.0.1", None, Transport::Ssh), refusing_options);
        let refused = connections.open("127.0.0.1", port, ssh("refuse"), &mut go).unwrap_err();
        assert!(matches!(&refused, HostError::CommandFailed { reason, .. } if reason.contains("refused to run `refuse`")), "{refused}");
        drop(connections);
        refusing.join().unwrap();

        let (port, known, killing) = server(client.public_key().clone());
        let (_dir, killing_options) = options(&known, &client);
        let mut connections = Connections::new(allow("127.0.0.1", None, Transport::Ssh), killing_options);
        let id = connections.open("127.0.0.1", port, ssh("killed"), &mut go).unwrap();
        assert_eq!(connections.read(id, 4096, &mut go).unwrap(), b"half a pack", "what it printed comes first");
        let stopped = connections.read(id, 4096, &mut go).unwrap_err();
        assert!(matches!(&stopped, HostError::CommandFailed { command, reason } if command == "killed" && reason.contains("KILL")), "{stopped}");
        drop(connections);
        killing.join().unwrap();
    }

    #[test]
    fn an_unknown_or_changed_host_key_is_refused() {
        let client = key(3);
        let (port, _known, changed) = server(client.public_key().clone());
        // The known hosts list another key for this host and port.
        let other = format!("[127.0.0.1]:{port} {}", key(9).public_key().to_openssh().unwrap());
        let (_dir, listed) = options(&other, &client);
        let mut connections = Connections::new(allow("127.0.0.1", None, Transport::Ssh), listed);
        let refused = connections.open("127.0.0.1", port, ssh("shout"), &mut go).unwrap_err();
        assert!(matches!(&refused, HostError::CommandFailed { reason, .. } if reason.contains("NOT the one")), "{refused}");
        let _ = changed.join();

        let (port, _known, unknown) = server(client.public_key().clone());
        let (_dir, empty) = options("", &client);
        let mut connections = Connections::new(allow("127.0.0.1", None, Transport::Ssh), empty);
        let refused = connections.open("127.0.0.1", port, ssh("shout"), &mut go).unwrap_err();
        assert!(matches!(&refused, HostError::CommandFailed { reason, .. } if reason.contains("not in your known hosts")), "{refused}");
        let _ = unknown.join();

        // Listed, but also marked @revoked: refused, as OpenSSH refuses it.
        let (port, known, revoked) = server(client.public_key().clone());
        let key = known.split_once(' ').unwrap().1;
        let (_dir, marked) = options(&format!("{known}\n@revoked * {key}"), &client);
        let mut connections = Connections::new(allow("127.0.0.1", None, Transport::Ssh), marked);
        let refused = connections.open("127.0.0.1", port, ssh("shout"), &mut go).unwrap_err();
        assert!(matches!(&refused, HostError::CommandFailed { reason, .. } if reason.contains("@revoked")), "{refused}");
        let _ = revoked.join();
    }
}
