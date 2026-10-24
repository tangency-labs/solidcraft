//! OpenSSH invokes this same executable to retrieve a remembered key passphrase.
//! Only a hashed keychain identity is inherited through the process environment.
use std::io::Write;

pub(super) fn run() -> Option<i32> {
    let id = std::env::var_os("SOLIDCRAFT_CLOUD_SSH_ASKPASS")?;
    let result = (|| -> Result<(), ()> {
        let id = id.to_str().ok_or(())?;
        let path = std::env::var("SOLIDCRAFT_CLOUD_SSH_KEY").map_err(|_| ())?;
        let prompt = std::env::args().nth(1).ok_or(())?;
        if !valid_prompt(id, &path, &prompt) {
            return Err(());
        }
        let secret = zeroize::Zeroizing::new(super::access::entry(id).map_err(|_| ())?.get_password().map_err(|_| ())?);
        // An askpass response is one line. Validate even an externally changed entry.
        if secret.len() > 1024 || secret.chars().any(char::is_control) {
            return Err(());
        }
        let mut output = std::io::stdout().lock();
        output.write_all(secret.as_bytes()).and_then(|()| output.write_all(b"\n")).and_then(|()| output.flush()).map_err(|_| ())
    })();
    Some(if result.is_ok() { 0 } else { 1 })
}

fn valid_prompt(id: &str, path: &str, prompt: &str) -> bool {
    let Some(hash) = id.strip_prefix("ssh-key:") else { return false };
    hash.len() == 64
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
        && !path.is_empty()
        && !path.chars().any(char::is_control)
        && prompt == format!("Enter passphrase for key '{path}': ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_answers_only_the_selected_keys_unlock_prompt() {
        let path = "/keys/a 'quoted' key";
        let id = solidcraft_cloud::jobs::ssh_credential(path);
        assert!(valid_prompt(&id, path, &format!("Enter passphrase for key '{path}': ")));
        for prompt in [
            "git@example.test's password: ",
            "Enter passphrase for key '/other/key': ",
            "Are you sure you want to continue connecting (yes/no)?",
            "Password: ",
        ] {
            assert!(!valid_prompt(&id, path, prompt));
        }
        assert!(!valid_prompt("cloud:github", path, &format!("Enter passphrase for key '{path}': ")));
    }
}
