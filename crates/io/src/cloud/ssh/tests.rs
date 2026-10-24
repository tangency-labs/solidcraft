use super::*;
use std::sync::Mutex;

#[derive(Default)]
struct Access(Mutex<Vec<(String, String)>>);
impl crate::cloud::NativeAccess for Access {
    fn credential(&self, _: &str) -> Result<Option<String>> {
        Ok(None)
    }
    fn store_credential(&self, _: &str, _: &str) -> Result<()> {
        Err(storage("unexpected credential operation"))
    }
    fn prepare(&self, _: solidcraft_cloud::Connection, _: &Job) -> Result<solidcraft_cloud::Connection> {
        Err(storage("unexpected credential operation"))
    }
    fn store_ssh_passphrase(&self, path: &str, passphrase: &str) -> Result<()> {
        self.0.lock().unwrap().push((path.into(), passphrase.into()));
        Ok(())
    }
}

#[test]
fn generated_pairs_are_private_unique_and_openssh_compatible() {
    let root = tempfile::Builder::new().prefix("SSH keys with 'quotes' ").tempdir().unwrap();
    let first = generate(root.path(), SshKeyInput::default(), &Job::new(String::new()).unwrap(), None).unwrap();
    let second = generate(root.path(), SshKeyInput::default(), &Job::new(String::new()).unwrap(), None).unwrap();
    assert_ne!(first.path, second.path);
    assert_ne!(first.public_key, second.public_key);
    let bytes = fs::read(&first.path).unwrap();
    let key = PrivateKey::from_openssh(&bytes).unwrap();
    assert!(!key.is_encrypted());
    assert_eq!(key.public_key().to_openssh().unwrap(), first.public_key);
    assert_eq!(fs::read_to_string(format!("{}.pub", first.path)).unwrap().trim(), first.public_key);
    assert_eq!(fs::read(&first.path).unwrap(), bytes, "second generation never overwrites the first");
    let output = std::process::Command::new("ssh-keygen").args(["-y", "-P", "", "-f", &first.path]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), first.public_key);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&first.path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(Path::new(&first.path).parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    }
}

#[test]
fn encryption_remembering_and_failed_storage_preserve_existing_keys() {
    let root = tempfile::tempdir().unwrap();
    let access = Access::default();
    let job = Job::new(String::new()).unwrap();
    for remember in [false, true] {
        let result =
            generate(root.path(), SshKeyInput { passphrase: Zeroizing::new("secret with 'spaces'".into()), remember }, &job, Some(&access)).unwrap();
        let key = PrivateKey::from_openssh(fs::read(&result.path).unwrap()).unwrap();
        assert!(result.encrypted);
        assert_eq!(result.remembered, remember);
        assert!(key.decrypt("wrong passphrase").is_err());
        assert_eq!(key.decrypt("secret with 'spaces'").unwrap().public_key().to_openssh().unwrap(), result.public_key);
        let saved = access.0.lock().unwrap();
        assert_eq!(saved.len(), usize::from(remember));
        if remember {
            assert_eq!(saved[0], (result.path.clone(), "secret with 'spaces'".into()));
        }
        let output = std::process::Command::new("ssh-keygen").args(["-y", "-P", "secret with 'spaces'", "-f", &result.path]).output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), result.public_key);
    }
    let count = fs::read_dir(root.path().join("ssh")).unwrap().count();
    assert!(generate(root.path(), SshKeyInput { passphrase: Zeroizing::new("secret".into()), remember: true }, &job, None).is_err());
    assert_eq!(fs::read_dir(root.path().join("ssh")).unwrap().count(), count);
    let cancelled = Job::new(String::new()).unwrap();
    cancelled.cancel().unwrap();
    assert!(generate(root.path(), SshKeyInput::default(), &cancelled, Some(&access)).is_err());
    assert_eq!(fs::read_dir(root.path().join("ssh")).unwrap().count(), count);
    assert!(generate(root.path(), SshKeyInput { passphrase: Zeroizing::new("x".repeat(1025)), remember: false }, &job, None).is_err());
    assert!(generate(root.path(), SshKeyInput { passphrase: Zeroizing::new("line\nbreak".into()), remember: false }, &job, None).is_err());
}
