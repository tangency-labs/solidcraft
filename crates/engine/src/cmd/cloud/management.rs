//! Repository management shares the existing asynchronous command and durability contract.
use super::*;
use crate::schema::cloud::{DiscardParams as Discard, ManagementParams as Input, OpenParams as Target, ResolveParams as Resolve};
use solidcraft_cloud::management::Change;

const RESOLVE_CONTRACT: crate::schema::TypedContract<crate::schema::cloud::ResolveParams> = crate::schema::TypedContract::new();

pub(crate) static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cloud.resolve", "Resolve conflict", crate::checked!("cloud.resolve", resolve)).icon("settings").noundo().doc_only()
        .contract(&RESOLVE_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::cloud::OperationResult>)
        .params("destination: conflict copy; revision: observed copy revision; remote_revision: observed original revision; keep: local or remote; atomically publish the choice and move the conflict copy to recoverable Trash"),
    CommandSpec::new("cloud.discard", "Discard queued action", crate::checked!("cloud.discard", discard))
        .contract(&crate::schema::TypedContract::<crate::schema::cloud::DiscardParams>::new())
        .output(crate::schema::output_schema_for::<solidcraft_cloud::jobs::JobStatus>).icon("stop").noundo().doc_only()
        .params("destination, revision: exact queued management id; verify repository state before discarding only that intent; saved designs and assets are preserved"),
    CommandSpec::new("cloud.website", "Open on GitHub", crate::checked!("cloud.website", website))
        .contract(&crate::schema::TypedContract::<crate::schema::cloud::ConnectionParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::cloud::WebsiteResult>).icon("open").noundo().doc_only()
        .params("connection?: selected GitHub connection id; returns canonical repository URL"),
    CommandSpec::new("cloud.history", "Version history", crate::checked!("cloud.history", history))
        .contract(&crate::schema::TypedContract::<crate::schema::cloud::OpenParams>::new())
        .output(crate::schema::output_schema_for::<solidcraft_cloud::jobs::JobStatus>).icon("inspect").noundo().doc_only()
        .params("destination: {connection,project,file}; returns operation id, then revision and up to 50 historical versions"),
    CommandSpec::new("cloud.trash", "Trash", crate::checked!("cloud.trash", trash))
        .contract(&crate::schema::TypedContract::<crate::schema::cloud::ConnectionParams>::new())
        .output(crate::schema::output_schema_for::<solidcraft_cloud::jobs::JobStatus>).icon("open").noundo().doc_only()
        .params("connection?: connection id; returns operation id, then recoverable deleted designs"),
    CommandSpec::new("cloud.rename", "Rename design", crate::checked!("cloud.rename", rename))
        .contract(&crate::schema::TypedContract::<crate::schema::cloud::ManagementParams>::new())
        .output(crate::schema::output_schema_for::<solidcraft_cloud::jobs::JobStatus>).icon("settings").noundo().doc_only()
        .params("destination, revision: observed revision or null, file: new project-relative design path; durable rename with collision checks"),
    CommandSpec::new("cloud.delete", "Move to Trash", crate::checked!("cloud.delete", delete))
        .contract(&crate::schema::TypedContract::<crate::schema::cloud::ManagementParams>::new())
        .output(crate::schema::output_schema_for::<solidcraft_cloud::jobs::JobStatus>).icon("delete").noundo().doc_only()
        .params("destination, revision: observed revision or null; durably queue recoverable deletion"),
    CommandSpec::new("cloud.restore", "Restore revision", crate::checked!("cloud.restore", restore))
        .contract(&crate::schema::TypedContract::<crate::schema::cloud::ManagementParams>::new())
        .output(crate::schema::output_schema_for::<solidcraft_cloud::jobs::JobStatus>).icon("save").noundo().doc_only()
        .params("destination, revision: observed current revision or null, commit: full Git SHA, file: historical project-relative path; publish old contents as a new revision"),
];

fn discard(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    let input: Discard = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.discard", e.to_string()))?;
    input.destination.validate().map_err(error)?;
    if input.revision.len() != 64 || !input.revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("cloud.discard", "exact queued revision required"));
    }
    let connection = super::connection(session, &json!({"connection":input.destination.connection}))?;
    submit(session, Operation::Discard { connection, destination: input.destination, revision: input.revision })
}

fn website(session: &mut Session, params: &Value) -> Result<Value> {
    let connection = super::connection(session, params)?;
    Ok(json!({"url":solidcraft_cloud::setup::github_website(&connection).map_err(error)?}))
}

fn history(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    let input: Target = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.history", e.to_string()))?;
    input.destination.validate().map_err(error)?;
    let connection = super::connection(session, &json!({"connection":input.destination.connection}))?;
    submit(session, Operation::History { connection, destination: input.destination })
}

fn trash(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    submit(session, Operation::Trash(super::connection(session, params)?))
}

fn manage(session: &mut Session, params: &Value, command: &str) -> Result<Value> {
    session.poll_cloud()?;
    let input: Input = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.revision.as_ref().is_some_and(|r| r.is_empty() || r.len() > 128 || r.chars().any(char::is_control)) {
        return Err(bad(command, "invalid observed revision"));
    }
    let change = match command {
        "cloud.rename" if input.commit.is_none() => Change::Rename { file: input.file.ok_or_else(|| bad(command, "new design path required"))? },
        "cloud.delete" if input.file.is_none() && input.commit.is_none() => Change::Delete,
        "cloud.restore" => Change::Restore {
            commit: input.commit.ok_or_else(|| bad(command, "historical commit required"))?,
            file: input.file.ok_or_else(|| bad(command, "historical design path required"))?,
        },
        _ => return Err(bad(command, "unexpected management parameters")),
    };
    change.validate(&input.destination).map_err(error)?;
    let connection = super::connection(session, &json!({"connection":input.destination.connection}))?;
    submit(session, Operation::Manage { connection, destination: input.destination, expected_revision: input.revision, change })
}

fn rename(session: &mut Session, params: &Value) -> Result<Value> {
    manage(session, params, "cloud.rename")
}
fn delete(session: &mut Session, params: &Value) -> Result<Value> {
    manage(session, params, "cloud.delete")
}
fn restore(session: &mut Session, params: &Value) -> Result<Value> {
    manage(session, params, "cloud.restore")
}

fn resolve(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    let input: Resolve = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.resolve", e.to_string()))?;
    let original =
        solidcraft_cloud::conflicts::original(&input.destination).map_err(error)?.ok_or_else(|| bad("cloud.resolve", "choose a conflict copy"))?;
    if input.revision.as_ref().is_none_or(|r| r.is_empty() || r.len() > 128 || r.chars().any(char::is_control)) {
        return Err(bad("cloud.resolve", "observed conflict revision required"));
    }
    let change = Change::Resolve { file: original.file, revision: input.remote_revision, keep: input.keep };
    change.validate(&input.destination).map_err(error)?;
    let connection = super::connection(session, &json!({"connection":input.destination.connection}))?;
    let reply = submit(session, Operation::Manage { connection, destination: input.destination, expected_revision: input.revision, change })?;
    let reply = crate::schema::cloud::OperationResult {
        operation: reply.get("operation").and_then(Value::as_str).ok_or_else(|| bad("cloud.resolve", "missing operation"))?.into(),
        connection: reply.get("connection").and_then(Value::as_str).ok_or_else(|| bad("cloud.resolve", "missing connection"))?.into(),
    };
    serde_json::to_value(reply).map_err(|e| bad("cloud.resolve", e.to_string()))
}
