//! SolidCraft's control channel identity for the shared token files (`solidcraft_control::auth`):
//! its name, data folder, `SOLIDCRAFT_` environment and default port (docs/control-protocol.md).

use solidcraft_control::auth::App;

/// SolidCraft's control channel: token files in `<data folder>/control` (`SOLIDCRAFT_CONTROL_DIR`
/// replaces it), `SOLIDCRAFT_CONTROL_TOKEN`, and 27878 first when the launch names no port.
pub const APP: App =
    App { name: "SolidCraft", data_dir: crate::recovery::data_dir, env: solidcraft_geom::env::var_os, utc: crate::crash::utc, default_port: 27878 };

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_control::auth::{ClientToken, TokenSource};
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;

    /// A refused token names its file and when it was written, never the token.
    #[test]
    fn unauthorized_error_names_the_token_file() {
        let path = PathBuf::from("control/7878.token");
        let modified = Some(UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000));
        let client = ClientToken { token: "secret-token".into(), file: Some(TokenSource { path: path.clone(), modified, writer: None }), app: APP };
        let error = client.explain_error("unauthorized: send the control token");
        assert!(error.contains(&format!("token from {}, written 2023-11-14T22:13:20Z (", path.display())), "{error}");
        assert!(error.contains("restart it or delete that file") && !error.contains("secret-token"), "{error}");
        assert_eq!(client.explain_error("unknown method"), "unknown method");
        let explicit = APP.client_token(0, Some("explicit-token")).unwrap();
        assert!(explicit.token == "explicit-token" && explicit.file.is_none());
        assert_eq!(explicit.explain_error("unauthorized"), "unauthorized");
    }
}
