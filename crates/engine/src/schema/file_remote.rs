//! Contracts and result types for the commands in `cmd/file_remote.rs`: the network folders of
//! Send to Slicer (#2304).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Add a network folder to Send to Slicer, replacing one of the same name: a folder on this
/// computer, or with `sftp` a folder on an SFTP server. Nothing is written until a mesh is sent there.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AddRemoteParams {
    /// The name shown in the Send to Slicer list: 1 to 64 characters.
    pub name: String,
    /// The folder's whole path, such as a mounted SMB or NFS share that a slicer on another computer
    /// reads; with `sftp`, the folder on that server (a whole path, or one from the login folder).
    /// It need not be there yet; `file.slicers.test` checks it.
    pub folder: String,
    #[serde(flatten)]
    pub server: SftpParams,
}

/// An SFTP server: logged in to with the user's SSH keys, never a password.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SftpParams {
    /// `user@host`, or `user@host:port`: the folder is on this SFTP server.
    #[serde(default)]
    pub sftp: Option<String>,
    /// The whole path of a private key file to log in with (Ed25519 or ECDSA, without a passphrase).
    /// Without it, the keys of the user's ssh-agent.
    #[serde(default)]
    pub key: Option<String>,
    /// The `SHA256:…` fingerprint of the server's key to trust when `~/.ssh/known_hosts` doesn't
    /// list the server; `file.slicers.test` shows it. Check it on the server first.
    #[serde(default)]
    pub host_key: Option<String>,
}

/// What `file.slicers.add_remote` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RemoteFolderReply {
    /// The id to pass as `slicer`: `remote:<name>`.
    pub id: String,
    /// The name, trimmed.
    pub name: String,
    /// `folder`, or `sftp` for a folder on an SFTP server.
    pub kind: String,
    /// The folder, trimmed (on the server, for SFTP).
    pub folder: String,
    /// The SFTP server: `user@host`, with `:port` when it isn't 22.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sftp: Option<String>,
    /// The key file logged in with; none for the ssh-agent's keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The fingerprint of the server key trusted for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_key: Option<String>,
    /// A network folder of that name was there and is replaced.
    pub replaced: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Remove a network folder from Send to Slicer. The files sent there stay.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RemoveRemoteParams {
    /// The network folder's name or id (`file.slicers`).
    pub name: String,
}

/// What `file.slicers.remove_remote` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RemoveRemoteReply {
    /// The name of the network folder removed.
    pub name: String,
    /// Always true: a name that matches nothing is refused.
    pub removed: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Check that a mesh can be sent to a folder: write a small file there and delete it again,
/// each step giving up after 30 s. Give one of `name` and `folder`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TestRemoteParams {
    /// A network folder's name or id (`file.slicers`).
    #[serde(default)]
    pub name: Option<String>,
    /// A folder's whole path, to check before adding it; with `sftp`, the folder on that server.
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(flatten)]
    pub server: SftpParams,
    /// The `SHA256:…` fingerprint of an SFTP server's key that the user checked, trusted for this
    /// test only.
    #[serde(default)]
    pub confirm: Option<String>,
}

/// What `file.slicers.test` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TestRemoteReply {
    /// The file could be written there.
    pub ok: bool,
    /// The folder checked (on the server, for SFTP).
    pub folder: String,
    /// The network folder's name, when one was named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The SFTP server checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sftp: Option<String>,
    /// What went wrong, in words, when `ok` is false: not there, no permission, full, no answer, a
    /// server not trusted yet, a key refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The SFTP server's key, once it answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_key: Option<HostKeyReply>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// An SFTP server's key, as a test saw it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HostKeyReply {
    /// `SHA256:…`, as `ssh-keygen -lf` prints it on the server: what the user compares.
    pub fingerprint: String,
    /// The key's type, such as `ssh-ed25519`.
    pub algorithm: String,
    /// `~/.ssh/known_hosts` lists it, or it is the fingerprint trusted for the target or confirmed.
    pub trusted: bool,
    /// The key is not the one `~/.ssh/known_hosts` or the target trusts: the server was set up
    /// again, or someone is in between.
    pub changed: bool,
    /// `~/.ssh/known_hosts` lists the server: with this key when `trusted`, else with another one,
    /// which `host_key` and `confirm` never override.
    pub known_hosts: bool,
}

#[cfg(test)]
mod tests {
    use super::{AddRemoteParams, RemoveRemoteParams, TestRemoteParams};
    use crate::schema::input_schema_for;
    use crate::schema::tests::assert_keys;

    #[test]
    fn schemas_and_the_key_check_agree() {
        assert_keys(input_schema_for::<AddRemoteParams>(), &["folder", "host_key", "key", "name", "sftp"]);
        assert_keys(input_schema_for::<RemoveRemoteParams>(), &["name"]);
        assert_keys(input_schema_for::<TestRemoteParams>(), &["confirm", "folder", "host_key", "key", "name", "sftp"]);
    }
}
