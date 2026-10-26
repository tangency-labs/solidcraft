//! Connection-scoped HTTPS authentication. Git's credential protocol keeps secrets out of
//! argv, config files and remote URLs, and never offers them to another host or repository.
use solidcraft_cloud::{Connection, Error, Result};
use std::process::Command;

/// Native app supplies credentials and hosting APIs. IO owns durable queues and standard Git.
pub trait NativeAccess: Send + Sync {
    fn check_repository(
        &self,
        _connection: &Connection,
        _input: solidcraft_cloud::jobs::RepositoryCheckInput,
        _job: &solidcraft_cloud::jobs::Job,
    ) -> Result<bool> {
        Err(Error::Storage("repository checking is unavailable".into()))
    }
    fn uses_provider_api(&self, _connection: &Connection) -> bool {
        false
    }
    fn provider_repository(&self, _connection: &Connection, _job: &solidcraft_cloud::jobs::Job) -> Result<Box<dyn super::ProviderRepository>> {
        Err(Error::Storage("provider repository API is unavailable".into()))
    }
    fn credential(&self, id: &str) -> Result<Option<String>>;
    fn store_credential(&self, id: &str, token: &str) -> Result<()>;
    fn store_ssh_passphrase(&self, _path: &str, _passphrase: &str) -> Result<()> {
        Err(Error::Storage("SSH passphrase storage is unavailable".into()))
    }
    /// Path to a platform askpass helper for a remembered key. Only an opaque keychain
    /// id enters the subprocess environment; the helper retrieves the secret itself.
    fn ssh_askpass(&self, _path: &str) -> Result<Option<std::path::PathBuf>> {
        Ok(None)
    }
    fn prepare(&self, connection: Connection, job: &solidcraft_cloud::jobs::Job) -> Result<Connection>;
}

pub(super) fn apply(command: &mut Command, connection: &Connection, token: &str) -> Result<()> {
    if connection.provider.is_empty() {
        return apply_git(command, connection, token);
    }
    solidcraft_cloud::setup::check_token(token)?;
    let repository = solidcraft_cloud::setup::github_repository(&connection.repository)?;
    if connection.provider != "github" {
        return Err(Error::Invalid("reconnect this repository to use its GitHub token".into()));
    }
    apply_context(command, &format!("https://github.com/{repository}.git"), "x-access-token", token);
    Ok(())
}

/// Credential contexts use Git's decoded path and include an explicit port, when present.
fn context(endpoint: &str) -> Result<String> {
    let invalid = || Error::Invalid("choose an HTTP(S) Git remote without credentials, query or fragment".into());
    let url = url::Url::parse(endpoint).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    url.host().ok_or_else(invalid)?;
    // Retain explicit default ports too: Git includes the literal port in its context.
    let authority = endpoint.split_once("://").map(|(_, rest)| rest.split('/').next().unwrap_or_default()).ok_or_else(invalid)?;
    let path = percent_encoding::percent_decode_str(url.path().strip_prefix('/').unwrap_or(url.path())).decode_utf8().map_err(|_| invalid())?;
    if path.is_empty() || path.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(format!("{}://{authority}/{path}", url.scheme()))
}

pub(super) fn check_git_destinations(connection: &Connection) -> Result<String> {
    connection.validate()?;
    let remote = connection.git_remote.as_ref().ok_or_else(|| Error::Invalid("verify the Git remote before supplying credentials".into()))?;
    let contexts = std::iter::once(&remote.fetch)
        .chain(&remote.push)
        .filter(|url| url.split_once("://").is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")))
        .map(|url| context(url))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    if contexts.is_empty() {
        return Err(Error::Invalid("choose an HTTP(S) Git remote for username/password authentication".into()));
    }
    Ok(contexts.into_iter().collect::<Vec<_>>().join("\n"))
}

fn apply_git(command: &mut Command, connection: &Connection, secret: &str) -> Result<()> {
    if connection.https_username.is_empty() {
        return Err(Error::Invalid("enter the Git HTTPS username".into()));
    }
    let contexts = check_git_destinations(connection)?;
    let password = solidcraft_cloud::credentials::decode_password(secret)?;
    apply_context(command, &contexts, &connection.https_username, &password);
    Ok(())
}

fn apply_context(command: &mut Command, contexts: &str, username: &str, password: &str) {
    // Endpoint and credential values are environment data, never interpolated shell code.
    let helper = "!f() { test \"$1\" = get || return 0; protocol= host= path=; while IFS= read -r line && test -n \"$line\"; do case \"$line\" in protocol=*) protocol=${line#protocol=};; host=*) host=${line#host=};; path=*) path=${line#path=};; esac; done; IFS='\n'; set -f; for allowed in $SOLIDCRAFT_GIT_DESTINATIONS; do if test \"$allowed\" = \"$protocol://$host/$path\"; then printf 'username=%s\\npassword=%s\\n' \"$SOLIDCRAFT_GIT_USERNAME\" \"$SOLIDCRAFT_GIT_PASSWORD\"; return 0; fi; done; }; f";
    command.args([
        "-c",
        "credential.helper=",
        "-c",
        &format!("credential.helper={helper}"),
        "-c",
        "credential.useHttpPath=true",
        "-c",
        "http.followRedirects=false",
    ]);
    command
        .env("SOLIDCRAFT_GIT_DESTINATIONS", contexts)
        .env("SOLIDCRAFT_GIT_USERNAME", username)
        .env("SOLIDCRAFT_GIT_PASSWORD", password)
        .env("GIT_TERMINAL_PROMPT", "0");
    for key in ["GIT_TRACE", "GIT_TRACE_PACKET", "GIT_TRACE_CURL", "GIT_CURL_VERBOSE", "GIT_TRACE2", "GIT_TRACE2_EVENT", "GIT_TRACE2_PERF"] {
        command.env_remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standalone_git_credentials_match_only_verified_endpoints_and_support_passwords() {
        let endpoint = "https://git.example.test:8443/team/design%20files.git";
        let password = "a password with spaces '$(not-shell)' ü";
        let encoded = solidcraft_cloud::credentials::encode_password(password).unwrap();
        let connection = Connection {
            provider: String::new(),
            https_username: "CAD user".into(),
            credential: "opaque-reference".into(),
            git_remote: Some(solidcraft_cloud::GitRemote { fetch: endpoint.into(), push: vec![endpoint.into()] }),
            ..Default::default()
        };
        for (url, expected) in [
            (endpoint, true),
            ("https://git.example.test:8443/team/other.git", false),
            ("https://git.example.test/team/design%20files.git", false),
            ("https://other.test:8443/team/design%20files.git", false),
            ("http://git.example.test:8443/team/design%20files.git", false),
        ] {
            let mut command = Command::new("git");
            apply(&mut command, &connection, &encoded).unwrap();
            let args = format!("{:?}", command.get_args().collect::<Vec<_>>());
            assert!(!args.contains(password) && !args.contains(&encoded));
            command.args(["credential", "fill"]).env("GIT_ASKPASS", "false");
            let input = format!("url={url}\n\n");
            let result = super::super::process::run(command, input.as_bytes(), || false, std::time::Duration::from_secs(10), 8192);
            if expected {
                let output = String::from_utf8(result.unwrap()).unwrap();
                assert!(output.contains("username=CAD user\n"));
                assert!(output.contains(&format!("password={password}\n")));
            } else {
                assert!(result.is_err());
            }
        }
        for endpoint in ["ssh://git@example.test/team/designs.git", "https://example.test/repo?query=1", "https://example.test/repo%0Aother"] {
            let mut invalid = connection.clone();
            invalid.git_remote = Some(solidcraft_cloud::GitRemote { fetch: endpoint.into(), push: vec![endpoint.into()] });
            assert!(apply(&mut Command::new("git"), &invalid, &encoded).is_err());
        }
        assert!(solidcraft_cloud::credentials::decode_password("not-a-credential").is_err());
        assert!(solidcraft_cloud::credentials::encode_password("password\ninjection").is_err());
    }

    #[test]
    fn git_credential_protocol_limits_token_to_the_selected_repository() {
        let connection = solidcraft_cloud::setup::github_connection("owner/repo").unwrap();
        for (context, expected) in [
            ("protocol=https\nhost=github.com\npath=owner/repo.git\n\n", true),
            ("protocol=https\nhost=github.com\npath=owner/other.git\n\n", false),
            ("protocol=https\nhost=evil.test\npath=owner/repo.git\n\n", false),
            ("protocol=http\nhost=github.com\npath=owner/repo.git\n\n", false),
        ] {
            let mut command = Command::new("git");
            apply(&mut command, &connection, "test-token").unwrap();
            assert!(!format!("{:?}", command.get_args().collect::<Vec<_>>()).contains("test-token"));
            command.args(["credential", "fill"]).env("GIT_ASKPASS", "false");
            let result = super::super::process::run(command, context.as_bytes(), || false, std::time::Duration::from_secs(10), 8192);
            if expected {
                assert!(String::from_utf8(result.unwrap()).unwrap().contains("password=test-token"));
            } else {
                assert!(result.is_err());
            }
        }
    }
}
