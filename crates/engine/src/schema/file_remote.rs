//! Contracts and result types for the commands in `cmd/file_remote.rs`: the network folders of
//! Send to Slicer (#2304).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Add a network folder to Send to Slicer, replacing one of the same name. Nothing is written
/// until a mesh is sent there.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AddRemoteParams {
    /// The name shown in the Send to Slicer list: 1 to 64 characters.
    pub name: String,
    /// The folder's whole path, such as a mounted SMB or NFS share that a slicer on another computer
    /// reads. It need not be there yet; `file.slicers.test` checks it.
    pub folder: String,
}

/// What `file.slicers.add_remote` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RemoteFolderReply {
    /// The id to pass as `slicer`: `remote:<name>`.
    pub id: String,
    /// The name, trimmed.
    pub name: String,
    /// Always `folder`.
    pub kind: String,
    /// The folder, trimmed.
    pub folder: String,
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
/// giving up after 30 s. Give one of `name` and `folder`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TestRemoteParams {
    /// A network folder's name or id (`file.slicers`).
    #[serde(default)]
    pub name: Option<String>,
    /// A folder's whole path, to check before adding it.
    #[serde(default)]
    pub folder: Option<String>,
}

/// What `file.slicers.test` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TestRemoteReply {
    /// The file could be written there.
    pub ok: bool,
    /// The folder checked.
    pub folder: String,
    /// The network folder's name, when one was named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What went wrong, in words, when `ok` is false: not there, no permission, full, no answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{AddRemoteParams, RemoveRemoteParams, TestRemoteParams};
    use crate::schema::input_schema_for;
    use crate::schema::tests::assert_keys;

    #[test]
    fn schemas_and_the_key_check_agree() {
        assert_keys(input_schema_for::<AddRemoteParams>(), &["folder", "name"]);
        assert_keys(input_schema_for::<RemoveRemoteParams>(), &["name"]);
        assert_keys(input_schema_for::<TestRemoteParams>(), &["folder", "name"]);
    }
}
