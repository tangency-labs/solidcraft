//! Connection-scoped HTTPS authentication. Git's credential protocol keeps secrets out of
//! argv, config files and remote URLs, and never offers them to another host or repository.
use solidcraft_cloud::{Connection, Error, Result};
use std::process::Command;

/// Native app supplies its OS credential store and provider HTTP client. IO owns Git only.
pub trait NativeAccess: Send + Sync {
    fn credential(&self, id: &str) -> Result<Option<String>>;
    fn store_credential(&self, id: &str, token: &str) -> Result<()>;
    fn prepare(&self, connection: Connection, job: &solidcraft_cloud::jobs::Job) -> Result<Connection>;
}

pub(super) fn apply(command: &mut Command, connection: &Connection, token: &str) -> Result<()> {
    solidcraft_cloud::setup::check_token(token)?;
    let repository = solidcraft_cloud::setup::github_repository(&connection.repository)?;
    if connection.provider != "github" || connection.remote != format!("https://github.com/{repository}.git") {
        return Err(Error::Invalid("reconnect this repository to use its GitHub token".into()));
    }
    // No interpolation into the shell snippet. Values read from Git are compared as data.
    // Git for Windows ships sh for credential helpers as well.
    let helper = "!f() { test \"$1\" = get || return 0; protocol= host= path=; while IFS= read -r line && test -n \"$line\"; do case \"$line\" in protocol=*) protocol=${line#protocol=};; host=*) host=${line#host=};; path=*) path=${line#path=};; esac; done; test \"$protocol\" = https && test \"$host\" = github.com && test \"$path\" = \"$SOLIDCRAFT_GIT_REPOSITORY\" || return 0; printf 'username=x-access-token\\npassword=%s\\n' \"$SOLIDCRAFT_GIT_TOKEN\"; }; f";
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
    command.env("SOLIDCRAFT_GIT_TOKEN", token).env("SOLIDCRAFT_GIT_REPOSITORY", format!("{repository}.git"));
    command.env("GIT_TERMINAL_PROMPT", "0");
    for key in ["GIT_TRACE", "GIT_TRACE_PACKET", "GIT_TRACE_CURL", "GIT_CURL_VERBOSE", "GIT_TRACE2", "GIT_TRACE2_EVENT", "GIT_TRACE2_PERF"] {
        command.env_remove(key);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
