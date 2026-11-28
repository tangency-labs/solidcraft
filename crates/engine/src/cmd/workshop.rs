//! `workshop.list` and `workshop.refresh`: read the Workshop catalog (workshop.solidcraft.io) the
//! host fetched and checked (see [`crate::workshop`]). They never download, open or install an
//! item: the app's Workshop window does that, and asks first.

use serde_json::Value;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::workshop::{WorkshopListParams, WorkshopListResult, WorkshopRefreshParams, WorkshopRefreshResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::workshop::{DEFAULT_LIMIT, Filter, MAX_LIMIT, MAX_QUERY};
use crate::{Result, Session};

const NO_WORKSHOP: &str = "not supported yet: this app has no way to reach the Workshop (the desktop app has)";

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("workshop.list", "List Workshop Items", crate::checked!("workshop.list", list))
        .icon("list")
        .noundo()
        .read_only()
        .doc_only()
        .contract(&TypedContract::<WorkshopListParams>::new())
        .output(output_schema_for::<WorkshopListResult>)
        .params(
            "kind?: plugin|model|part; tier?: official|community|unverified; query?: words (each must match the name, summary, owner, author, slug or id); limit?: 1..500 (default 100) → state: empty|loading|ready|failed, error?, sequence, issued_at, expires_at, total, items: [{id, kind, owner, slug, name, summary, tier, author, licence, version (null: needs a newer SolidCraft), needs_app?, permissions, size, warning?, page_url}]: the Workshop's signed index as the app last checked it; run workshop.refresh first when state is empty. Installs nothing",
        ),
    CommandSpec::new("workshop.refresh", "Refresh Workshop", crate::checked!("workshop.refresh", refresh))
        .icon("update")
        .noundo()
        .doc_only()
        .contract(&TypedContract::<WorkshopRefreshParams>::new())
        .output(output_schema_for::<WorkshopRefreshResult>)
        .params("→ refreshing: true: the app reads the Workshop's signed index again in the background and checks its signature; workshop.list shows loading, then the result. Installs nothing"),
];

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "workshop.list";
    let p: WorkshopListParams =
        if p.is_null() { WorkshopListParams::default() } else { serde_json::from_value(p.clone()).map_err(|e| bad(CMD, e.to_string()))? };
    let query = p.query.unwrap_or_default();
    if query.chars().count() > MAX_QUERY {
        return Err(bad(CMD, format!("`query` is longer than {MAX_QUERY} characters")));
    }
    let limit = match p.limit {
        None => DEFAULT_LIMIT,
        Some(n) if (1..=MAX_LIMIT as u64).contains(&n) => usize::try_from(n).unwrap_or(MAX_LIMIT),
        Some(n) => return Err(bad(CMD, format!("`limit` must be 1 to {MAX_LIMIT}, not {n}"))),
    };
    let workshop = s.workshop.clone().ok_or_else(|| bad(CMD, NO_WORKSHOP))?;
    let catalog = workshop.snapshot();
    let filter = Filter { kind: p.kind, tier: p.tier, query };
    let kept: Vec<_> = filter.apply(&catalog.items).collect();
    let result = WorkshopListResult {
        state: catalog.state,
        error: catalog.error,
        sequence: catalog.sequence,
        issued_at: catalog.issued_at,
        expires_at: catalog.expires_at,
        total: kept.len() as u64,
        items: kept.into_iter().take(limit).cloned().collect(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(result).map_err(|e| bad(CMD, e.to_string()))
}

fn refresh(s: &mut Session, _: &Value) -> Result<Value> {
    const CMD: &str = "workshop.refresh";
    let workshop = s.workshop.clone().ok_or_else(|| bad(CMD, NO_WORKSHOP))?;
    workshop.refresh().map_err(|e| bad(CMD, e))?;
    serde_json::to_value(WorkshopRefreshResult { refreshing: true, param_warnings: Vec::new() }).map_err(|e| bad(CMD, e.to_string()))
}
