//! Private keychain payloads for standalone Git. These never enter repository files.
use base64::{Engine, engine::general_purpose::STANDARD};
use zeroize::Zeroizing;

use crate::{Error, Result};

const PREFIX: &str = "git-https/1:";

pub fn check_password(password: &str) -> Result<()> {
    if password.is_empty() || password.len() > 1024 || password.chars().any(char::is_control) {
        return Err(Error::Invalid("enter a password or token of at most 1024 bytes without control characters".into()));
    }
    Ok(())
}

/// Encoding permits spaces and Unicode while retaining the host's bounded secret API.
/// The operating system keychain supplies encryption; this encoding is only a container.
pub fn encode_password(password: &str) -> Result<String> {
    check_password(password)?;
    Ok(format!("{PREFIX}{}", STANDARD.encode(password)))
}

pub fn decode_password(value: &str) -> Result<Zeroizing<String>> {
    let invalid = || Error::Invalid("invalid saved Git credential; enter its password again".into());
    if value.len() > 2048 {
        return Err(invalid());
    }
    let bytes = STANDARD.decode(value.strip_prefix(PREFIX).ok_or_else(invalid)?).map_err(|_| invalid())?;
    let password = Zeroizing::new(String::from_utf8(bytes).map_err(|_| invalid())?);
    check_password(&password)?;
    Ok(password)
}
