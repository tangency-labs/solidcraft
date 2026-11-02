//! Network folders for Send to Slicer (#2304): add, remove and test the folders a mesh can be
//! sent to (`crate::remote_slicers`), on this computer or on an SFTP server (`crate::remote_sftp`). They are the session's (`Session::network_folders`); the
//! desktop app stores them in the user's preferences when these commands succeed and puts them back
//! at the next start. Run without an app (CLI, scripts), a folder lasts as long as the session.

use serde_json::{Value, json};

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::remote_slicers::{self, RemoteTarget, SftpAddress};
use crate::schema::file_remote::{AddRemoteParams, RemoteFolderReply, RemoveRemoteParams, RemoveRemoteReply, TestRemoteParams, TestRemoteReply};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("file.slicers.add_remote", "Add Network Folder", crate::checked!("file.slicers.add_remote", add_remote))
        .icon("network_folder")
        .noundo()
        .doc_only()
        .enabled(reaches_folders)
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<RemoteFolderReply>)
        .params("name: 1..64 characters (shown in the Send to Slicer list); folder: the folder's whole path, such as a mounted SMB or NFS share that a slicer on another computer reads, or with sftp the folder on that server (a whole path, or one from the login folder); sftp?: user@host or user@host:port, for a folder on an SFTP server; key?: the whole path of a private key file to log in with (Ed25519 or ECDSA, without a passphrase; default: the keys of the user's ssh-agent; no password is ever taken); host_key?: the SHA256:… fingerprint of the server's key to trust when ~/.ssh/known_hosts doesn't list it (file.slicers.test shows it; check it on the server) → {id: remote:<name>, name, kind: folder|sftp, folder, sftp?, key?, host_key?, replaced}: adds a network folder to Send to Slicer, replacing one of the same name. Nothing is written until a mesh is sent; file.slicers.test checks it. The app keeps it in the user's preferences; without an app it lasts as long as the session. An agent limited to folders (--allow-path) can't add an SFTP server"),
    CommandSpec::new("file.slicers.remove_remote", "Remove Network Folder", crate::checked!("file.slicers.remove_remote", remove_remote))
        .icon("delete")
        .noundo()
        .doc_only()
        .enabled(reaches_folders)
        .contract(&REMOVE_CONTRACT)
        .output(output_schema_for::<RemoveRemoteReply>)
        .params("name: a network folder's name or id (file.slicers) → {name, removed: true}; the files sent there stay"),
    CommandSpec::new("file.slicers.test", "Test Network Folder", crate::checked!("file.slicers.test", test))
        .icon("inspect")
        .noundo()
        .doc_only()
        .enabled(reaches_folders)
        .contract(&TEST_CONTRACT)
        .output(output_schema_for::<TestRemoteReply>)
        .params("name?: a network folder's name or id, or folder?: a folder's whole path, or with sftp?, key? and host_key? (as file.slicers.add_remote) a folder on an SFTP server (one of name and folder); confirm?: the SHA256:… fingerprint of an SFTP server's key the user checked, trusted for this test only → {ok, folder, name?, sftp?, error?, host_key?: {fingerprint, algorithm, trusted, changed}}: writes a small file in the folder and deletes it again, each step giving up after 30 s; error says what went wrong (not there, no permission, full, no answer, a server not trusted yet, a key refused). A server neither ~/.ssh/known_hosts lists nor the user trusted is never written to: host_key says its fingerprint, to show the user before passing it as host_key or confirm; changed: true means the server's key is not the one trusted (set up again, or someone in between)"),
];

const ADD_CONTRACT: TypedContract<AddRemoteParams> = TypedContract::new();
const REMOVE_CONTRACT: TypedContract<RemoveRemoteParams> = TypedContract::new();
const TEST_CONTRACT: TypedContract<TestRemoteParams> = TypedContract::new();

/// The browser build has no folders to write to.
fn reaches_folders(_: &Session) -> std::result::Result<(), String> {
    if cfg!(target_arch = "wasm32") { Err("network folders can't be reached from the browser".into()) } else { Ok(()) }
}

/// A folder an agent's commands may use: one in the folders it was allowed (`--allow-path`). The
/// app's own calls have no such limit.
fn allowed(cmd: &str, folder: &str) -> Result<()> {
    solidcraft_io::vfs::check(folder).map_err(|e| bad(cmd, e.to_string()))
}

/// An SFTP server sends files off this computer: an agent limited to folders (`--allow-path`)
/// can't name one; the user adds it in Send to Slicer.
fn off_this_computer(cmd: &str) -> Result<()> {
    if solidcraft_io::vfs::current_scope().is_some() {
        Err(bad(cmd, "an agent limited to folders (--allow-path) can't name an SFTP server; the user adds it in Send to Slicer"))
    } else {
        Ok(())
    }
}

/// The target `p` describes, named `name`: a folder, or with `sftp` a folder on an SFTP server.
fn target_of(cmd: &str, name: &str, p: &Value) -> Result<RemoteTarget> {
    let folder = str_(p, "folder").unwrap_or_default();
    let Some(server) = str_(p, "sftp") else {
        if str_(p, "key").is_some() || str_(p, "host_key").is_some() {
            return Err(bad(cmd, "`key` and `host_key` go with `sftp`"));
        }
        let folder = remote_slicers::check_folder(folder).map_err(|e| bad(cmd, e))?;
        allowed(cmd, folder)?;
        return Ok(RemoteTarget::folder_target(name, folder));
    };
    off_this_computer(cmd)?;
    let address = SftpAddress::parse(server).map_err(|e| bad(cmd, e))?;
    let folder = remote_slicers::check_remote_folder(folder).map_err(|e| bad(cmd, e))?;
    let key = str_(p, "key").map(remote_slicers::check_key_file).transpose().map_err(|e| bad(cmd, e))?;
    let host_key = str_(p, "host_key").map(remote_slicers::check_fingerprint).transpose().map_err(|e| bad(cmd, e))?;
    Ok(RemoteTarget::sftp_target(name, &address, folder, key, host_key))
}

fn add_remote(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.slicers.add_remote";
    reaches_folders(s).map_err(|e| bad(cmd, e))?;
    let name = remote_slicers::check_name(str_(p, "name").unwrap_or_default()).map_err(|e| bad(cmd, e))?;
    let target = target_of(cmd, name, p)?;
    let replaced = remote_slicers::add(&mut s.network_folders, target.clone()).map_err(|e| bad(cmd, e))?;
    let mut out = target.listing();
    if let Some(obj) = out.as_object_mut() {
        obj.remove("remote");
        obj.insert("replaced".into(), json!(replaced));
    }
    Ok(out)
}

fn remove_remote(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.slicers.remove_remote";
    reaches_folders(s).map_err(|e| bad(cmd, e))?;
    let spec = str_(p, "name").filter(|n| !n.trim().is_empty()).ok_or_else(|| bad(cmd, "give `name` (see file.slicers)"))?;
    let gone =
        remote_slicers::remove(&mut s.network_folders, spec).ok_or_else(|| bad(cmd, format!("no network folder `{spec}` (see file.slicers)")))?;
    Ok(json!({"name": gone.name, "removed": true}))
}

fn test(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.slicers.test";
    reaches_folders(s).map_err(|e| bad(cmd, e))?;
    let server_keys = ["sftp", "key", "host_key"].iter().any(|k| str_(p, k).is_some());
    let (name, target) = match (str_(p, "name"), str_(p, "folder")) {
        (Some(spec), None) if !server_keys => {
            let target = remote_slicers::resolve(&s.network_folders, spec)
                .ok_or_else(|| bad(cmd, format!("no network folder `{spec}` (see file.slicers)")))?;
            (Some(target.name.clone()), target.clone())
        }
        (None, Some(_)) => (None, target_of(cmd, "test", p)?),
        _ => return Err(bad(cmd, "give `name`, or `folder` (with `sftp` for a folder on an SFTP server), one of the two")),
    };
    let confirm = str_(p, "confirm").map(remote_slicers::check_fingerprint).transpose().map_err(|e| bad(cmd, e))?;
    if confirm.is_some() && target.address().is_none() {
        return Err(bad(cmd, "`confirm` goes with an SFTP server"));
    }
    let tested = remote_slicers::test_target(&target, confirm);
    let mut out = json!({"ok": tested.ok, "folder": target.folder()});
    if let Some(name) = name {
        out["name"] = json!(name);
    }
    if let Some(address) = target.address() {
        out["sftp"] = json!(address.to_string());
    }
    if let Some(e) = tested.error {
        out["error"] = json!(e);
    }
    if let Some(key) = tested.host_key {
        out["host_key"] = json!(key);
    }
    Ok(out)
}
