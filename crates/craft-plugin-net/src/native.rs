//! The transports on a desktop or command-line app: blocking sockets polled in short slices, so a
//! call stays cancellable and inside its deadline however slow the network is.
use crate::{MAX_STDERR_BYTES, Options, Request, Stream, Wait, failed};
use craft_plugin::{HostError, Result};
use std::future::Future;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a wait lasts before `Wait` is asked again.
const POLL: Duration = Duration::from_millis(200);
/// How long one address may take to accept a connection.
const CONNECT: Duration = Duration::from_secs(20);

pub(crate) fn connect(host: &str, port: u16, request: Request, options: &Options, wait: Wait) -> Result<Box<dyn Stream>> {
    match request {
        Request::Tcp => Ok(Box::new(Tcp { socket: tcp(host, port, wait)? })),
        Request::Tls => Ok(Box::new(Tls::open(host, port, options, wait)?)),
        Request::Ssh { user, command } => Ok(Box::new(Ssh::open(host, port, &user, &command, options, wait)?)),
    }
}

fn waiting(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

/// Runs blocking `work` (a name lookup, one connection attempt) on a thread of its own and asks
/// `wait` every `POLL` until it ends, so a server that doesn't answer can't hold the call past Cancel
/// or its deadline. A call given up on leaves the thread to end on its own (each attempt is bounded
/// by `CONNECT` or the system's lookup timeout) and drops what it made.
fn off_thread<T: Send + 'static>(wait: Wait, work: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    use std::sync::mpsc::{RecvTimeoutError, channel};
    let (sender, receiver) = channel();
    std::thread::Builder::new()
        .name("plugin-net-connect".into())
        .spawn(move || {
            let _ = sender.send(work());
        })
        .map_err(|error| failed("net.open", error.to_string()))?;
    loop {
        match receiver.recv_timeout(POLL) {
            Ok(done) => return Ok(done),
            Err(RecvTimeoutError::Timeout) => wait()?,
            Err(RecvTimeoutError::Disconnected) => return Err(failed("net.open", "the connection attempt stopped")),
        }
    }
}

fn tcp(host: &str, port: u16, wait: Wait) -> Result<TcpStream> {
    wait()?;
    let name = host.to_string();
    let found = off_thread(wait, move || (name.as_str(), port).to_socket_addrs().map(Iterator::collect::<Vec<_>>))?;
    let addresses = found.map_err(|error| failed("net.open", format!("can't find {host}: {error}")))?;
    let mut last = None;
    for address in addresses {
        wait()?;
        match off_thread(wait, move || TcpStream::connect_timeout(&address, CONNECT))? {
            Ok(socket) => {
                wait()?;
                let setup =
                    socket.set_read_timeout(Some(POLL)).and_then(|()| socket.set_write_timeout(Some(POLL))).and_then(|()| socket.set_nodelay(true));
                setup.map_err(|error| failed("net.open", error.to_string()))?;
                return Ok(socket);
            }
            Err(error) => last = Some(error),
        }
    }
    wait()?;
    Err(failed(
        "net.open",
        match last {
            Some(error) => format!("can't connect to {host}:{port}: {error}"),
            None => format!("{host} has no address"),
        },
    ))
}

struct Tcp {
    socket: TcpStream,
}

impl Stream for Tcp {
    fn read(&mut self, max: usize, wait: Wait) -> Result<Vec<u8>> {
        let mut buffer = vec![0; max];
        loop {
            match self.socket.read(&mut buffer) {
                Ok(count) => {
                    buffer.truncate(count);
                    return Ok(buffer);
                }
                Err(error) if waiting(&error) => wait()?,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(failed("net.read", error.to_string())),
            }
        }
    }

    fn write(&mut self, data: &[u8], wait: Wait) -> Result<()> {
        write_all(&mut self.socket, data, wait, "net.write")
    }

    fn finish(&mut self, _wait: Wait) -> Result<()> {
        self.socket.shutdown(Shutdown::Write).map_err(|error| failed("net.finish", error.to_string()))
    }
}

fn write_all(socket: &mut TcpStream, mut data: &[u8], wait: Wait, what: &str) -> Result<()> {
    while !data.is_empty() {
        match socket.write(data) {
            Ok(0) => return Err(failed(what, "the connection closed")),
            Ok(count) => {
                data = data.get(count..).unwrap_or_default();
                wait()?;
            }
            Err(error) if waiting(&error) => wait()?,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(failed(what, error.to_string())),
        }
    }
    Ok(())
}

/// TLS driven by hand over the polled socket, so a timeout never loses part of a record.
struct Tls {
    connection: rustls::ClientConnection,
    socket: TcpStream,
    /// The server closed the socket.
    closed: bool,
}

impl Tls {
    fn open(host: &str, port: u16, options: &Options, wait: Wait) -> Result<Self> {
        let config = tls_config(options)?;
        let name = rustls::pki_types::ServerName::try_from(host.to_string())
            .map_err(|_| HostError::InvalidInput { field: "host".into(), reason: format!("`{host}` is not a name TLS can check") })?;
        let connection = rustls::ClientConnection::new(config, name).map_err(|error| failed("net.open", error.to_string()))?;
        let mut tls = Self { connection, socket: tcp(host, port, wait)?, closed: false };
        while tls.connection.is_handshaking() {
            if tls.closed {
                return Err(failed("net.open", format!("{host}:{port} closed the connection during the TLS handshake")));
            }
            tls.pump(wait, "net.open")?;
        }
        // rustls may finish the handshake with the client's Finished still queued.
        tls.flush(wait, "net.open")?;
        Ok(tls)
    }

    /// Sends what TLS has to send, then reads what the server sent, waiting if nothing came.
    fn pump(&mut self, wait: Wait, what: &str) -> Result<()> {
        self.flush(wait, what)?;
        if !self.connection.wants_read() || self.closed {
            return Ok(());
        }
        match self.connection.read_tls(&mut self.socket) {
            Ok(0) => self.closed = true,
            Ok(_) => {
                self.connection.process_new_packets().map_err(|error| failed(what, format!("TLS: {error}")))?;
            }
            Err(error) if waiting(&error) => wait()?,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(failed(what, error.to_string())),
        }
        Ok(())
    }

    fn flush(&mut self, wait: Wait, what: &str) -> Result<()> {
        while self.connection.wants_write() {
            match self.connection.write_tls(&mut self.socket) {
                Ok(_) => wait()?,
                Err(error) if waiting(&error) => wait()?,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(failed(what, error.to_string())),
            }
        }
        Ok(())
    }
}

impl Stream for Tls {
    fn read(&mut self, max: usize, wait: Wait) -> Result<Vec<u8>> {
        let mut buffer = vec![0; max];
        loop {
            match self.connection.reader().read(&mut buffer) {
                Ok(count) => {
                    buffer.truncate(count);
                    return Ok(buffer);
                }
                // Only TLS's goodbye (close_notify) ends the data: a connection dropped without it
                // may have cut the reply short.
                Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Err(cut_short()),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if self.closed {
                        return Err(cut_short());
                    }
                    self.pump(wait, "net.read")?;
                }
                Err(error) => return Err(failed("net.read", error.to_string())),
            }
        }
    }

    fn write(&mut self, mut data: &[u8], wait: Wait) -> Result<()> {
        while !data.is_empty() {
            let count = self.connection.writer().write(data).map_err(|error| failed("net.write", error.to_string()))?;
            data = data.get(count..).unwrap_or_default();
            self.flush(wait, "net.write")?;
        }
        Ok(())
    }

    fn finish(&mut self, wait: Wait) -> Result<()> {
        self.connection.send_close_notify();
        self.flush(wait, "net.finish")
    }
}

fn cut_short() -> HostError {
    failed("net.read", "the server closed the connection without ending TLS: the reply may be cut short")
}

/// The app's TLS settings: rustls with ring and the platform's certificate checks, as the update
/// check uses; or, for a test server, only the roots the options name.
fn tls_config(options: &Options) -> Result<Arc<rustls::ClientConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| failed("net.open", format!("TLS: {error}")))?;
    let config = if options.tls_roots.is_empty() {
        use rustls_platform_verifier::BuilderVerifierExt;
        builder.with_platform_verifier().map_err(|error| failed("net.open", format!("TLS: {error}")))?.with_no_client_auth()
    } else {
        let mut roots = rustls::RootCertStore::empty();
        for root in &options.tls_roots {
            roots.add(rustls::pki_types::CertificateDer::from(root.clone())).map_err(|error| failed("net.open", format!("TLS root: {error}")))?;
        }
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    Ok(Arc::new(config))
}

/// One SSH session running one command, on a runtime of its own.
struct Ssh {
    runtime: tokio::runtime::Runtime,
    _session: russh::client::Handle<HostKeys>,
    channel: russh::Channel<russh::client::Msg>,
    command: String,
    /// Output received and not read yet.
    pending: Vec<u8>,
    stderr: Vec<u8>,
    exit: Option<u32>,
    /// The signal that stopped the command, as the server named it.
    signal: Option<String>,
    /// The server's answer to running the command: accepted or refused.
    accepted: Option<bool>,
    eof: bool,
    closed: bool,
}

/// Checks the server's key against the user's known hosts, and says why it refused one.
struct HostKeys {
    host: String,
    port: u16,
    known_hosts: Option<PathBuf>,
    refusal: Arc<Mutex<Option<String>>>,
}

impl russh::client::Handler for HostKeys {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &russh::keys::PublicKeyOrCertificate) -> std::result::Result<bool, Self::Error> {
        let russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            self.refuse("its host certificate can't be checked; add its key to known_hosts".into());
            return Ok(false);
        };
        let fingerprint = key.fingerprint(Default::default());
        let file = self.known_hosts.clone().or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".ssh").join("known_hosts")));
        if file.is_some_and(|file| revoked(&file, key)) {
            self.refuse(format!("its host key ({fingerprint}) is marked @revoked in your known hosts"));
            return Ok(false);
        }
        let checked = match &self.known_hosts {
            Some(path) => russh::keys::check_known_hosts_path(&self.host, self.port, key, path),
            None => russh::keys::check_known_hosts(&self.host, self.port, key),
        };
        match checked {
            Ok(true) => return Ok(true),
            Ok(false) => {
                self.refuse(format!("its host key ({fingerprint}) is not in your known hosts; connect once with `ssh` to check it and add it"))
            }
            Err(russh::keys::Error::KeyChanged { line }) => self.refuse(format!(
                "its host key ({fingerprint}) is NOT the one your known hosts list (line {line}); someone may be intercepting the connection"
            )),
            Err(error) => self.refuse(format!("can't check its host key: {error}")),
        }
        Ok(false)
    }
}

/// Whether a `@revoked` line of the known hosts file names `key`: OpenSSH then refuses it, whatever
/// other line lists it, and russh's own check reads no markers.
fn revoked(file: &std::path::Path, key: &russh::keys::PublicKey) -> bool {
    let (Ok(text), Ok(ours)) = (std::fs::read_to_string(file), key.to_openssh()) else { return false };
    let mut ours = ours.split_whitespace();
    let (Some(kind), Some(data)) = (ours.next(), ours.next()) else { return false };
    text.lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next() == Some("@revoked") && fields.nth(1) == Some(kind) && fields.next() == Some(data)
    })
}

impl HostKeys {
    fn refuse(&self, reason: String) {
        if let Ok(mut refusal) = self.refusal.lock() {
            refusal.get_or_insert(reason);
        }
    }
}

/// Runs `future` on `runtime`, asking `wait` between slices.
fn block<F: Future>(runtime: &tokio::runtime::Runtime, future: F, wait: Wait) -> Result<F::Output> {
    let mut future = Box::pin(future);
    loop {
        // The timer is made inside the runtime: tokio's timers need its context to exist.
        match runtime.block_on(async { tokio::time::timeout(POLL, &mut future).await }) {
            Ok(output) => return Ok(output),
            Err(_) => wait()?,
        }
    }
}

impl Ssh {
    fn open(host: &str, port: u16, user: &str, command: &str, options: &Options, wait: Wait) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|error| failed("net.open", error.to_string()))?;
        let refusal = Arc::new(Mutex::new(None));
        let keys = HostKeys { host: host.into(), port, known_hosts: options.known_hosts.clone(), refusal: refusal.clone() };
        let config = Arc::new(russh::client::Config::default());
        wait()?;
        // Looked up on a thread of its own: tokio's lookup would run on the runtime's blocking pool,
        // and dropping the runtime on Cancel would wait for it.
        let name = host.to_string();
        let found = off_thread(wait, move || (name.as_str(), port).to_socket_addrs().map(Iterator::collect::<Vec<_>>))?;
        let addresses = found.map_err(|error| failed("net.open", format!("can't find {host}: {error}")))?;
        let connected = block(&runtime, russh::client::connect(config, addresses.as_slice(), keys), wait)?;
        let mut session = connected.map_err(|error| {
            let why = refusal.lock().ok().and_then(|refusal| refusal.clone()).unwrap_or_else(|| error.to_string());
            failed("net.open", format!("SSH to {host}:{port}: {why}"))
        })?;
        if !sign_in(&runtime, &mut session, user, options, wait)? {
            return Err(failed(
                "net.open",
                format!("SSH to {user}@{host}: no key was accepted; add yours to the SSH agent (ssh-add) or use an Ed25519 key without a passphrase"),
            ));
        }
        let channel = block(&runtime, session.channel_open_session(), wait)?.map_err(|error| failed("net.open", error.to_string()))?;
        block(&runtime, channel.exec(true, command), wait)?.map_err(|error| failed("net.open", error.to_string()))?;
        let mut ssh = Self {
            runtime,
            _session: session,
            channel,
            command: command.into(),
            pending: Vec::new(),
            stderr: Vec::new(),
            exit: None,
            signal: None,
            accepted: None,
            eof: false,
            closed: false,
        };
        // `exec` only sends the request: the server's answer says whether the command runs.
        while ssh.accepted.is_none() && ssh.receive(wait)? {}
        match ssh.accepted {
            Some(true) => Ok(ssh),
            Some(false) => Err(failed("net.open", format!("SSH to {host}: the server refused to run `{command}`"))),
            None => Err(failed("net.open", format!("SSH to {host}: the server closed the session before running `{command}`"))),
        }
    }

    /// Takes the next message from the server into the buffers. False once the channel closed.
    fn receive(&mut self, wait: Wait) -> Result<bool> {
        if self.closed {
            return Ok(false);
        }
        match block(&self.runtime, self.channel.wait(), wait)? {
            Some(russh::ChannelMsg::Data { data }) => self.pending.extend_from_slice(&data),
            Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                let room = MAX_STDERR_BYTES.saturating_sub(self.stderr.len());
                self.stderr.extend(data.iter().take(room));
            }
            Some(russh::ChannelMsg::ExitStatus { exit_status }) => self.exit = Some(exit_status),
            Some(russh::ChannelMsg::ExitSignal { signal_name, error_message, .. }) => {
                let message = error_message.trim();
                self.signal = Some(if message.is_empty() { format!("{signal_name:?}") } else { format!("{signal_name:?}: {message}") });
            }
            Some(russh::ChannelMsg::Success) => self.accepted = Some(true),
            Some(russh::ChannelMsg::Failure) => self.accepted = Some(false),
            Some(russh::ChannelMsg::Eof) => self.eof = true,
            Some(russh::ChannelMsg::Close) | None => {
                self.eof = true;
                self.closed = true;
            }
            Some(_) => {}
        }
        wait()?;
        Ok(true)
    }
}

/// Signs in with the agent's keys, then the key files. True when one was accepted.
fn sign_in(
    runtime: &tokio::runtime::Runtime,
    session: &mut russh::client::Handle<HostKeys>,
    user: &str,
    options: &Options,
    wait: Wait,
) -> Result<bool> {
    let rsa_hash = block(runtime, session.best_supported_rsa_hash(), wait)?.ok().flatten().flatten();
    #[cfg(unix)]
    if options.ssh_agent
        && std::env::var_os("SSH_AUTH_SOCK").is_some()
        && let Ok(mut agent) = block(runtime, russh::keys::agent::client::AgentClient::connect_env(), wait)?
    {
        let identities = block(runtime, agent.request_identities(), wait)?.unwrap_or_default();
        for identity in identities {
            let russh::keys::agent::AgentIdentity::PublicKey { key, .. } = identity else { continue };
            let hash = if key.algorithm().is_rsa() { rsa_hash } else { None };
            let result = block(runtime, session.authenticate_publickey_with(user, key, hash, &mut agent), wait)?;
            if matches!(result, Ok(russh::client::AuthResult::Success)) {
                return Ok(true);
            }
        }
    }
    let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".ssh"));
    let files = options.ssh_keys.clone().unwrap_or_else(|| home.map(|ssh| vec![ssh.join("id_ed25519"), ssh.join("id_ecdsa")]).unwrap_or_default());
    for file in files {
        // A key with a passphrase, or a kind this build can't sign with, is left to the agent.
        let Ok(key) = russh::keys::load_secret_key(&file, None) else { continue };
        let key = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash);
        let result = block(runtime, session.authenticate_publickey(user, key), wait)?;
        if matches!(result, Ok(russh::client::AuthResult::Success)) {
            return Ok(true);
        }
    }
    Ok(false)
}

impl Stream for Ssh {
    fn read(&mut self, max: usize, wait: Wait) -> Result<Vec<u8>> {
        while self.pending.is_empty() && !self.eof {
            self.receive(wait)?;
        }
        if !self.pending.is_empty() {
            let count = max.min(self.pending.len());
            return Ok(self.pending.drain(..count).collect());
        }
        // The output ended: wait for the command's exit status, which may follow.
        while self.exit.is_none() && self.receive(wait)? {}
        match self.exit {
            Some(0) => Ok(Vec::new()),
            None => match &self.signal {
                // Stopped by a signal: what it printed so far is all there is, and it is not whole.
                Some(signal) => Err(failed(&self.command, format!("was stopped by signal {signal}"))),
                None => Ok(Vec::new()),
            },
            Some(status) => {
                let said = String::from_utf8_lossy(&self.stderr).trim().to_string();
                Err(failed(&self.command, if said.is_empty() { format!("exited with status {status}") } else { said }))
            }
        }
    }

    fn write(&mut self, data: &[u8], wait: Wait) -> Result<()> {
        block(&self.runtime, self.channel.data(data), wait)?.map_err(|error| failed("net.write", error.to_string()))
    }

    fn finish(&mut self, wait: Wait) -> Result<()> {
        block(&self.runtime, self.channel.eof(), wait)?.map_err(|error| failed("net.finish", error.to_string()))
    }
}
