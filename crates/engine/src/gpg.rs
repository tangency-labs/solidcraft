//! GnuPG for design provenance.
//!
//! SolidCraft does not touch the user's secret keys. It asks GnuPG for a detached OpenPGP
//! signature over a manifest and for the matching transferable public key; the private key (often
//! on a YubiKey's OpenPGP smartcard) never leaves GnuPG or the card. Verification happens in the
//! provenance module against the public key embedded in the record, so it works without GnuPG and
//! in the browser.
//!
//! Only the desktop build has this module: the browser cannot run GnuPG, and its save path reports
//! that signing needs the desktop app.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A GnuPG installation SolidCraft can run.
pub struct Gpg {
    program: PathBuf,
}

/// A secret key GnuPG can sign with, as `file.gpg_keys` lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpgKey {
    /// The primary key's fingerprint (GnuPG's uppercase hex).
    pub fingerprint: String,
    /// The primary key's long key id, for display.
    pub key_id: String,
    /// The primary user id.
    pub user_id: String,
    /// The key (or one of its subkeys) lives on an OpenPGP smartcard.
    pub on_card: bool,
}

/// The signer GnuPG produced: the detached signature, the public key that verifies it, and what to
/// show the user about who signed.
pub struct GpgSignature {
    pub signature: Vec<u8>,
    pub key: Vec<u8>,
    /// The fingerprint of the (sub)key that actually signed.
    pub signing_fingerprint: String,
    pub user_id: Option<String>,
}

impl Gpg {
    /// GnuPG on this computer, if one can be run. The common install locations are tried because
    /// a window launched from the desktop does not always have the shell's `PATH`.
    pub fn find() -> Option<Gpg> {
        for program in candidates() {
            let Ok(status) = Command::new(&program).arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() else {
                continue;
            };
            if status.success() {
                return Some(Gpg { program });
            }
        }
        None
    }

    /// The secret keys GnuPG can sign with, including keys whose secret part is on a smartcard.
    pub fn secret_keys(&self) -> Result<Vec<GpgKey>, String> {
        let output = self.command(["--with-colons", "--with-secret", "--list-secret-keys"])?;
        parse_secret_keys(&String::from_utf8_lossy(&output))
    }

    /// Sign `manifest` with `key` (a fingerprint or user id given to GnuPG; empty: GnuPG's
    /// default key). GnuPG may ask for a smartcard PIN through its pinentry; this blocks until the
    /// user answers, so call it off the window thread.
    pub fn sign_manifest(&self, key: Option<&str>, manifest: &[u8]) -> Result<GpgSignature, String> {
        let dir = TempDir::new()?;
        let manifest_path = dir.path().join("manifest.bin");
        let signature_path = dir.path().join("manifest.sig");
        write_private(&manifest_path, manifest)?;
        let mut arguments: Vec<String> = vec![
            "--batch".into(),
            "--no-tty".into(),
            "--yes".into(),
            "--status-fd".into(),
            "1".into(),
            "--logger-fd".into(),
            "2".into(),
            "--digest-algo".into(),
            "SHA256".into(),
            "--detach-sign".into(),
            "--output".into(),
            signature_path.to_string_lossy().into_owned(),
        ];
        if let Some(key) = key.map(str::trim).filter(|key| !key.is_empty()) {
            arguments.push("--local-user".into());
            arguments.push(key.to_owned());
        }
        arguments.push(manifest_path.to_string_lossy().into_owned());
        let output = self.run(arguments)?;
        if !output.status.success() {
            return Err(signing_error(&output));
        }
        let Some(signing_fingerprint) = signed_fingerprint(&output.stdout) else {
            return Err(signing_error(&output));
        };
        let signature = std::fs::read(&signature_path).map_err(|error| format!("the GnuPG signature could not be read: {error}"))?;
        if signature.is_empty() {
            return Err("GnuPG reported success but wrote no signature".to_owned());
        }
        let (primary, user_id) = self.describe_key(&signing_fingerprint)?;
        let key = self.export_public_key(&primary)?;
        Ok(GpgSignature { signature, key, signing_fingerprint, user_id })
    }

    /// The primary fingerprint and user id of `fingerprint`'s key.
    fn describe_key(&self, fingerprint: &str) -> Result<(String, Option<String>), String> {
        let output = self.command(["--with-colons", "--list-keys", fingerprint])?;
        let text = String::from_utf8_lossy(&output);
        let mut primary = None;
        let mut user_id = None;
        for line in text.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            match fields.first().copied() {
                Some("fpr") if primary.is_none() => {
                    primary = fields.get(9).map(|value| value.to_string());
                }
                Some("uid") if user_id.is_none() => {
                    user_id = fields.get(9).map(|value| unescape_colon(value));
                }
                _ => {}
            }
        }
        let primary = primary.ok_or_else(|| format!("GnuPG knows no key for {fingerprint}"))?;
        Ok((primary, user_id))
    }

    /// The key's transferable public key, binary OpenPGP packets. An unknown fingerprint makes
    /// GnuPG exit 0 with nothing exported, so empty output is an error.
    fn export_public_key(&self, fingerprint: &str) -> Result<Vec<u8>, String> {
        let output = self.command(["--export", fingerprint])?;
        if output.is_empty() {
            return Err(format!("GnuPG exported no public key for {fingerprint}"));
        }
        Ok(output)
    }

    /// Run GnuPG and return its stdout.
    fn command<const N: usize>(&self, arguments: [&str; N]) -> Result<Vec<u8>, String> {
        let output = self.run(arguments.iter().map(|argument| (*argument).to_owned()).collect())?;
        if !output.status.success() {
            return Err(signing_error(&output));
        }
        Ok(output.stdout)
    }

    fn run(&self, arguments: Vec<String>) -> Result<std::process::Output, String> {
        Command::new(&self.program)
            .args(arguments)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| format!("GnuPG could not be started ({}): {error}", self.program.display()))
    }
}

/// What the last signature produced says about its signer, for the save reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningInfo {
    pub fingerprint: String,
    pub user_id: Option<String>,
}

/// Implements the provenance format's `OpenPgpSigner` with GnuPG, and keeps what GnuPG said about
/// the signer so the save reply can name the key.
pub struct Signer<'a> {
    gpg: &'a Gpg,
    key: Option<&'a str>,
    done: std::cell::RefCell<Option<SigningInfo>>,
}

impl<'a> Signer<'a> {
    pub fn new(gpg: &'a Gpg, key: Option<&'a str>) -> Signer<'a> {
        Signer { gpg, key, done: std::cell::RefCell::new(None) }
    }

    pub fn signing_info(&self) -> Option<SigningInfo> {
        self.done.borrow().clone()
    }
}

impl solidcraft_io::OpenPgpSigner for Signer<'_> {
    fn sign(&self, manifest: &[u8]) -> solidcraft_io::Result<solidcraft_io::OpenPgpSignature> {
        let done = self.gpg.sign_manifest(self.key, manifest).map_err(solidcraft_io::IoError::Invalid)?;
        let info = SigningInfo { fingerprint: done.signing_fingerprint, user_id: done.user_id.clone() };
        *self.done.borrow_mut() = Some(info);
        Ok(solidcraft_io::OpenPgpSignature { signature: done.signature, key: done.key })
    }
}

/// The `--local-user` fingerprint GnuPG reports in its machine-readable status, when it succeeds.
fn signed_fingerprint(status: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(status);
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("[GNUPG:] SIG_CREATED ") else { continue };
        // SIG_CREATED <type> <pk> <hash> <class> <timestamp> <fingerprint>
        return rest.split_whitespace().last().map(str::to_owned);
    }
    None
}

/// Turn GnuPG's machine status and log into a sentence a user can act on.
fn signing_error(output: &std::process::Output) -> String {
    let status = String::from_utf8_lossy(&output.stdout);
    let log = String::from_utf8_lossy(&output.stderr);
    let trimmed: String = log.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("").trim().to_owned();
    if status.contains("INV_SGNR") || log.contains("No secret key") {
        return "GnuPG has no secret signing key for that identity".to_owned();
    }
    if status.contains("SC_OP_FAILURE") || log.contains("Bad PIN") || log.contains("Bad passphrase") {
        return "GnuPG rejected the PIN or passphrase (a wrong PIN uses up a card retry)".to_owned();
    }
    if log.contains("Operation cancelled") {
        return "Signing was cancelled".to_owned();
    }
    if log.contains("Inappropriate ioctl") || log.contains("No pinentry") || log.contains("pinentry") {
        return "GnuPG could not ask for the smartcard PIN: no pinentry is available".to_owned();
    }
    if log.contains("OpenPGP card not available") || log.contains("card not available") {
        return "GnuPG could not find the OpenPGP smartcard".to_owned();
    }
    if trimmed.is_empty() { "GnuPG could not sign the design".to_owned() } else { format!("GnuPG could not sign the design: {trimmed}") }
}

/// Where GnuPG is usually installed, before `PATH` (a desktop-launched window may lack the shell's
/// `PATH`).
fn candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    // Standard install locations first: a relative or unexpected PATH entry must not decide
    // which GnuPG runs. Only absolute PATH entries are considered, and duplicates are dropped.
    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from("/opt/homebrew/bin/gpg"));
        candidates.push(PathBuf::from("/usr/local/bin/gpg"));
        candidates.push(PathBuf::from("/usr/local/MacGPG2/bin/gpg2"));
    }
    #[cfg(windows)]
    {
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            let root = PathBuf::from(program_files);
            candidates.push(root.join("GnuPG").join("bin").join("gpg.exe"));
            candidates.push(root.join("GnuPG").join("bin").join("gpg2.exe"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        candidates.push(PathBuf::from("/usr/bin/gpg"));
        candidates.push(PathBuf::from("/usr/bin/gpg2"));
        candidates.push(PathBuf::from("/usr/local/bin/gpg"));
        candidates.push(PathBuf::from("/usr/local/bin/gpg2"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path).filter(|directory| directory.is_absolute()) {
            candidates.push(directory.join("gpg"));
            candidates.push(directory.join("gpg2"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|program| seen.insert(program.clone()));
    candidates
}

/// Parse `gpg --with-colons --list-secret-keys`: one entry per key with a signing-capable secret
/// key or subkey. Fields follow GnuPG's colon format (1 type, 5 key id, 10 user id, 12
/// capabilities, 15 token serial).
pub fn parse_secret_keys(text: &str) -> Result<Vec<GpgKey>, String> {
    let mut keys: Vec<GpgKey> = Vec::new();
    let mut current: Option<(GpgKey, bool)> = None;
    let flush = |current: &mut Option<(GpgKey, bool)>, keys: &mut Vec<GpgKey>| {
        if let Some((key, can_sign)) = current.take()
            && can_sign
            && !key.fingerprint.is_empty()
        {
            keys.push(key);
        }
    };
    for line in text.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        let kind = fields.first().copied().unwrap_or("");
        let field = |index: usize| fields.get(index).copied().unwrap_or("");
        match kind {
            "sec" => {
                flush(&mut current, &mut keys);
                current = Some((
                    GpgKey { fingerprint: String::new(), key_id: field(4).to_owned(), user_id: String::new(), on_card: on_card(field(14)) },
                    field(11).contains('s') || field(11).contains('S'),
                ));
            }
            "ssb" => {
                if let Some((key, can_sign)) = &mut current {
                    if field(11).contains('s') || field(11).contains('S') {
                        *can_sign = true;
                    }
                    if on_card(field(14)) {
                        key.on_card = true;
                    }
                }
            }
            "fpr" => {
                if let Some((key, _)) = &mut current
                    && key.fingerprint.is_empty()
                {
                    key.fingerprint = field(9).to_owned();
                }
            }
            "uid" => {
                if let Some((key, _)) = &mut current
                    && key.user_id.is_empty()
                {
                    key.user_id = unescape_colon(field(9));
                }
            }
            _ => {}
        }
    }
    flush(&mut current, &mut keys);
    Ok(keys)
}

/// Whether a colon format's token serial field says the key is on a smartcard. `+` means the
/// secret key is available locally; `#` means it is an offline stub; a serial number means a card.
fn on_card(serial: &str) -> bool {
    !serial.is_empty() && serial != "+" && serial != "#"
}

/// GnuPG's colon format escapes some bytes in user ids. Decode the escapes used there.
fn unescape_colon(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if index + 3 < bytes.len() && bytes[index + 1] == b'x' => {
                let hex = |byte: u8| match byte {
                    b'0'..=b'9' => Some(byte - b'0'),
                    b'a'..=b'f' => Some(byte - b'a' + 10),
                    b'A'..=b'F' => Some(byte - b'A' + 10),
                    _ => None,
                };
                match (hex(bytes[index + 2]), hex(bytes[index + 3])) {
                    (Some(high), Some(low)) => {
                        out.push((high << 4) | low);
                        index += 4;
                    }
                    _ => {
                        out.push(b'\\');
                        index += 1;
                    }
                }
            }
            b'\\' if index + 1 < bytes.len() => {
                out.push(bytes[index + 1]);
                index += 2;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A temporary directory only SolidCraft may read, removed when it goes out of scope.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<TempDir, String> {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("solidcraft-gpg-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&path).map_err(|error| format!("could not make a temporary folder for GnuPG: {error}"))?;
        set_private(&path)?;
        Ok(TempDir(path))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("could not write {}: {error}", path.display()))?;
    set_private(path)
}

#[cfg(unix)]
fn set_private(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if path.is_dir() { 0o700 } else { 0o600 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|error| format!("could not protect {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_private(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_keys_are_read_with_capability_card_and_user_id() {
        // GnuPG 2.5 colon output for an Ed25519 key whose signing subkey is on a smartcard.
        let text = "sec:u:255:22:00187B908ED1DAB4:1794449107:::u:::scSC:::+::ed25519:::0:\nfpr:::::::::900D0FC44F3044C168362CB200187B908ED1DAB4:\ngrp:::::::::ECF979BF4322795687250A14E7507AE10F7156B5:\nuid:u::::1794449107::19106547CBF2D9D9DC1D2A547326233E21939A7F::Ada Lovelace <ada@example.com>::::::::::0:\nssb:u:255:22:1111222233334444:1794449107::::::sE:::0011223344556677::ed25519:::0:\nfpr:::::::::1111222233334444444444444444444444444444:\n";
        let keys = parse_secret_keys(text).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].fingerprint, "900D0FC44F3044C168362CB200187B908ED1DAB4");
        assert_eq!(keys[0].user_id, "Ada Lovelace <ada@example.com>");
        assert!(keys[0].on_card);
    }

    #[test]
    fn a_local_secret_key_is_not_reported_as_on_a_card() {
        let text = "sec:u:255:22:00187B908ED1DAB4:1794449107:::u:::scSC:::+::ed25519:::0:\nfpr:::::::::900D0FC44F3044C168362CB200187B908ED1DAB4:\n";
        let keys = parse_secret_keys(text).unwrap();
        assert_eq!(keys.len(), 1);
        assert!(!keys[0].on_card);
    }

    #[test]
    fn a_key_without_signing_capability_is_left_out() {
        let text = "sec:e:2048:1:AAAA:1:0::::e:::scESC::::::23:::::\nfpr:::::::::AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:\n";
        assert!(parse_secret_keys(text).unwrap().is_empty());
    }

    #[test]
    fn colon_escapes_in_a_user_id_are_decoded() {
        assert_eq!(unescape_colon("a\\x3ab\\\\c"), "a:b\\c");
    }
}
