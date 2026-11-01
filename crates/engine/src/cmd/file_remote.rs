//! Network folders for Send to Slicer (#2304): add, remove and test the folders a mesh can be
//! sent to (`crate::remote_slicers`). They are the session's (`Session::network_folders`); the
//! desktop app stores them in the user's preferences when these commands succeed and puts them back
//! at the next start. Run without an app (CLI, scripts), a folder lasts as long as the session.

use serde_json::{Value, json};

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::remote_slicers::{self, RemoteTarget};
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
        .params("name: 1..64 characters (shown in the Send to Slicer list); folder: the folder's whole path, such as a mounted SMB or NFS share that a slicer on another computer reads → {id: remote:<name>, name, kind: folder, folder, replaced}: adds a network folder to Send to Slicer, replacing one of the same name. Nothing is written until a mesh is sent; file.slicers.test checks the folder. The app keeps it in the user's preferences; without an app it lasts as long as the session"),
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
        .params("name?: a network folder's name or id, or folder?: a folder's whole path (one of the two) → {ok, folder, name?, error?}: writes a small file in the folder and deletes it again, giving up after 30 s; error says what went wrong (not there, no permission, full, no answer)"),
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

fn add_remote(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.slicers.add_remote";
    reaches_folders(s).map_err(|e| bad(cmd, e))?;
    let name = remote_slicers::check_name(str_(p, "name").unwrap_or_default()).map_err(|e| bad(cmd, e))?;
    let folder = remote_slicers::check_folder(str_(p, "folder").unwrap_or_default()).map_err(|e| bad(cmd, e))?;
    allowed(cmd, folder)?;
    let target = RemoteTarget::folder_target(name, folder);
    let replaced = remote_slicers::add(&mut s.network_folders, target.clone()).map_err(|e| bad(cmd, e))?;
    Ok(json!({"id": target.id(), "name": target.name, "kind": target.kind(), "folder": target.folder(), "replaced": replaced}))
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
    let (name, folder) = match (str_(p, "name"), str_(p, "folder")) {
        (Some(spec), None) => {
            let target = remote_slicers::resolve(&s.network_folders, spec)
                .ok_or_else(|| bad(cmd, format!("no network folder `{spec}` (see file.slicers)")))?;
            (Some(target.name.clone()), target.folder().to_string())
        }
        (None, Some(folder)) => {
            let folder = remote_slicers::check_folder(folder).map_err(|e| bad(cmd, e))?;
            allowed(cmd, folder)?;
            (None, folder.to_string())
        }
        _ => return Err(bad(cmd, "give `name` or `folder`, one of the two")),
    };
    let mut out = json!({"ok": true, "folder": folder});
    if let Some(name) = name {
        out["name"] = json!(name);
    }
    if let Err(e) = remote_slicers::probe(&folder) {
        out["ok"] = json!(false);
        out["error"] = json!(e);
    }
    Ok(out)
}
