//! Git-based version control for design histories (`vcs.*`).

use serde_json::{Value, json};

use super::CommandSpec;
use crate::schema::vcs::{
    VcsCommitParams, VcsCommitResult, VcsInitParams, VcsInitResult, VcsLogParams, VcsLogResult, VcsStatusParams, VcsStatusResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const INIT: TypedContract<VcsInitParams> = TypedContract::new();
const COMMIT: TypedContract<VcsCommitParams> = TypedContract::new();
const STATUS: TypedContract<VcsStatusParams> = TypedContract::new();
const LOG: TypedContract<VcsLogParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("vcs.init", "Initialize History", crate::checked!("vcs.init", vcs_init))
        .icon("git_history")
        .noundo()
        .any_part()
        .contract(&INIT)
        .output(output_schema_for::<VcsInitResult>)
        .params("author_name?: str; author_email?: str → repo: path, created: bool"),
    CommandSpec::new("vcs.commit", "Commit Design", crate::checked!("vcs.commit", vcs_commit))
        .icon("git_commit")
        .noundo()
        .any_part()
        .contract(&COMMIT)
        .output(output_schema_for::<VcsCommitResult>)
        .params("message: str; author_name?: str; author_email?: str → commit: sha, message: str"),
    CommandSpec::new("vcs.status", "Design History Status", crate::checked!("vcs.status", vcs_status))
        .icon("git_status")
        .noundo()
        .read_only()
        .any_part()
        .contract(&STATUS)
        .output(output_schema_for::<VcsStatusResult>)
        .params("→ initialized: bool, changed: bool, branch?: str, head?: str"),
    CommandSpec::new("vcs.log", "Design History Log", crate::checked!("vcs.log", vcs_log))
        .icon("git_log")
        .noundo()
        .read_only()
        .any_part()
        .contract(&LOG)
        .output(output_schema_for::<VcsLogResult>)
        .params("limit?: int (default 20) → commits: [{commit, message, author, time}]"),
];

fn design_dir_and_stem(s: &Session) -> Result<(std::path::PathBuf, String)> {
    let path_str = s.path.as_ref().ok_or_else(|| EngineError::Other(solidcraft_vcs::VcsError::NoPath.to_string()))?;
    let path = std::path::Path::new(path_str);
    let dir = path.parent().ok_or_else(|| EngineError::Other("design file has no parent directory".into()))?.to_owned();
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    Ok((dir, stem))
}

fn vcs_err(e: solidcraft_vcs::VcsError) -> EngineError {
    EngineError::Other(e.to_string())
}

fn design_json(s: &Session) -> Result<String> {
    serde_json::to_string(&s.doc).map_err(|e| EngineError::Other(e.to_string()))
}

fn vcs_init(s: &mut Session, _p: &Value) -> Result<Value> {
    let (dir, _stem) = design_dir_and_stem(s)?;
    let (repo, created) = solidcraft_vcs::init(&dir).map_err(vcs_err)?;
    Ok(json!({ "repo": repo.display().to_string(), "created": created }))
}

fn vcs_commit(s: &mut Session, p: &Value) -> Result<Value> {
    let (dir, stem) = design_dir_and_stem(s)?;
    let json_str = design_json(s)?;
    let message = p.get("message").and_then(Value::as_str).ok_or_else(|| crate::params::bad("vcs.commit", "`message` is required"))?;
    let author_name = p.get("author_name").and_then(Value::as_str).unwrap_or("SolidCraft VCS");
    let author_email = p.get("author_email").and_then(Value::as_str).unwrap_or("vcs@solidcraft.local");
    let commit = solidcraft_vcs::commit(&dir, &stem, &json_str, message, author_name, author_email).map_err(vcs_err)?;
    Ok(json!({ "commit": commit, "message": message }))
}

fn vcs_status(s: &mut Session, _p: &Value) -> Result<Value> {
    let (dir, stem) = design_dir_and_stem(s)?;
    let json_str = design_json(s)?;
    let info = solidcraft_vcs::status(&dir, &stem, &json_str).map_err(vcs_err)?;
    Ok(json!({ "initialized": info.initialized, "changed": info.changed, "branch": info.branch, "head": info.head }))
}

fn vcs_log(s: &mut Session, p: &Value) -> Result<Value> {
    let (dir, _stem) = design_dir_and_stem(s)?;
    let limit = p.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
    let entries = solidcraft_vcs::log(&dir, limit).map_err(vcs_err)?;
    let commits: Vec<Value> = entries.iter().map(|c| json!({ "commit": c.id, "message": c.message, "author": c.author, "time": c.time })).collect();
    Ok(json!({ "commits": commits }))
}
