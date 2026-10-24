//! Local Ed25519 generation. Exclusive files in a private, randomly named directory.
//! Keys are never repository objects; only their path and public half leave this module.
use std::{fs, io::Write, path::Path};

use rand_chacha::{
    ChaCha20Rng,
    rand_core::{OsRng, RngCore, SeedableRng},
};
use solidcraft_cloud::{
    Error, Result,
    jobs::{Job, SshKey, SshKeyInput},
};
use ssh_key::{Algorithm, LineEnding, PrivateKey};
use zeroize::Zeroizing;

fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}
fn check(job: &Job) -> Result<()> {
    if job.cancelled()? { Err(storage("SSH key generation cancelled")) } else { Ok(()) }
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(storage)?;
    file.write_all(bytes).map_err(storage)?;
    file.sync_all().map_err(storage)
}

// Windows files inherit the new directory's owner-only ACL. Restrict it before any
// key bytes are written; a failure leaves the empty temporary directory disposable.
#[cfg(windows)]
fn private_directory(path: &Path) -> Result<()> {
    let system = std::env::var_os("SystemRoot").ok_or_else(|| storage("Windows system directory unavailable"))?;
    let system = std::path::PathBuf::from(system).join("System32");
    let mut who = std::process::Command::new(system.join("whoami.exe"));
    who.args(["/user", "/fo", "csv", "/nh"]);
    let output = super::process::run(who, &[], || false, std::time::Duration::from_secs(10), 8192).map_err(storage)?;
    let tail = output.rsplit(|b| *b == b',').next().unwrap_or_default();
    let sid = std::str::from_utf8(tail).map_err(storage)?.trim().trim_matches('"');
    if !sid.starts_with("S-1-") || !sid.chars().all(|c| c == 'S' || c == '-' || c.is_ascii_digit()) {
        return Err(storage("Windows user identity unavailable"));
    }
    let mut acl = std::process::Command::new(system.join("icacls.exe"));
    acl.arg(path).args(["/inheritance:r", "/grant:r", &format!("*{sid}:(OI)(CI)F")]);
    super::process::run(acl, &[], || false, std::time::Duration::from_secs(10), 8192).map_err(storage)?;
    Ok(())
}

pub(super) fn generate(root: &Path, input: SshKeyInput, job: &Job, access: Option<&dyn super::NativeAccess>) -> Result<SshKey> {
    input.validate()?;
    check(job)?;
    crate::vfs::check(root.to_str().ok_or_else(|| storage("SSH storage path is not UTF-8"))?).map_err(storage)?;
    // A fallible OS entropy read precedes the infallible CSPRNG used by ssh-key's
    // rand_core 0.6 interface. Entropy failures return errors instead of panicking.
    let mut seed = Zeroizing::new([0u8; 32]);
    OsRng.try_fill_bytes(seed.as_mut()).map_err(|_| storage("system randomness is unavailable"))?;
    let mut rng = ChaCha20Rng::from_seed(*seed);
    let mut key = PrivateKey::random(&mut rng, Algorithm::Ed25519).map_err(storage)?;
    key.set_comment("SolidCraft cloud saves");
    let public_key = key.public_key().to_openssh().map_err(storage)?;
    let encrypted = !input.passphrase.is_empty();
    if encrypted {
        key = key.encrypt(&mut rng, input.passphrase.as_bytes()).map_err(storage)?;
    }
    check(job)?;
    let keys = root.join("ssh");
    fs::create_dir_all(&keys).map_err(storage)?;
    let mut builder = tempfile::Builder::new();
    builder.prefix("key-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let directory = builder.tempdir_in(&keys).map_err(storage)?;
    #[cfg(windows)]
    private_directory(directory.path())?;
    let private = directory.path().join("id_ed25519");
    let path = private.to_str().ok_or_else(|| storage("SSH key path is not UTF-8"))?.to_owned();
    write(&private, key.to_openssh(LineEnding::LF).map_err(storage)?.as_bytes())?;
    write(&directory.path().join("id_ed25519.pub"), format!("{public_key}\n").as_bytes())?;
    #[cfg(unix)]
    fs::File::open(directory.path()).and_then(|file| file.sync_all()).map_err(storage)?;
    check(job)?;
    if input.remember {
        access.ok_or_else(|| storage("system credential store unavailable"))?.store_ssh_passphrase(&path, &input.passphrase)?;
    }
    // Keep before reporting success. Cancellation never overwrites or removes a user's
    // existing keys; cancelling after this point may leave this unused pair locally.
    let _ = directory.keep();
    #[cfg(unix)]
    fs::File::open(&keys).and_then(|file| file.sync_all()).map_err(storage)?;
    Ok(SshKey { path, public_key, encrypted, remembered: input.remember })
}

#[cfg(test)]
mod tests;
