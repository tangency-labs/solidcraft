//! `engine.search`: unified global search command backed by [`crate::search`] (RFC 0019 Phase 1).

use serde_json::Value;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::search::{SearchCommandResult, SearchParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::search::{DEFAULT_LIMIT, MAX_LIMIT, MAX_QUERY_CHARS, SearchQuery, SearchScope, SearchSurface, SearchViewer, global_search};
use crate::{Result, Session};

const SEARCH_CONTRACT: TypedContract<SearchParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("engine.search", "Search", crate::checked!("engine.search", search))
    .any_part()
    .icon("list")
    .noundo()
    .read_only()
    .contract(&SEARCH_CONTRACT)
    .output(output_schema_for::<SearchCommandResult>)
    .params(
        "query: text to search for (up to 256 characters); \
         scopes?: [commands|design|features|bodies|sketches|components|parameters|joints|named_views|parts|samples|plugins] (default: all sources for the surface); \
         surface?: global|commands|design|parts|preferences|browser (default global); \
         limit?: 1..500 (default 50); \
         include_disabled?: bool (default true) \
         → {query, surface, hits, groups, total, near_matches, sources_searched, sources_failed}. Read-only",
    )];

fn search(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "engine.search";
    let query_text = match p.get("query") {
        Some(Value::String(text)) if text.chars().count() > MAX_QUERY_CHARS => {
            return Err(bad(cmd, format!("`query` is longer than {MAX_QUERY_CHARS} characters")));
        }
        Some(Value::String(text)) => text.clone(),
        _ => return Err(bad(cmd, "`query` must be a string")),
    };
    let scopes = match p.get("scopes") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) if items.len() <= SearchScope::ALL.len() => items
            .iter()
            .map(|v| v.as_str().and_then(SearchScope::from_id))
            .collect::<Option<Vec<SearchScope>>>()
            .ok_or_else(|| {
                bad(
                    cmd,
                    "`scopes` takes commands, design, features, bodies, sketches, components, parameters, joints, named_views, parts, samples and plugins",
                )
            })?,
        Some(_) => return Err(bad(cmd, "`scopes` must be a list of valid search scope names")),
    };
    let surface = match p.get("surface") {
        None | Some(Value::Null) => SearchSurface::Global,
        Some(Value::String(name)) => {
            SearchSurface::from_id(name).ok_or_else(|| bad(cmd, "`surface` must be global, commands, design, parts, preferences or browser"))?
        }
        Some(_) => return Err(bad(cmd, "`surface` must be global, commands, design, parts, preferences or browser")),
    };
    let limit = match p.get("limit") {
        None | Some(Value::Null) => DEFAULT_LIMIT,
        Some(Value::Number(n)) => n
            .as_u64()
            .filter(|&v| v >= 1 && v <= MAX_LIMIT as u64)
            .map(|v| v as usize)
            .ok_or_else(|| bad(cmd, format!("`limit` must be a whole number from 1 to {MAX_LIMIT}")))?,
        Some(_) => return Err(bad(cmd, format!("`limit` must be a whole number from 1 to {MAX_LIMIT}"))),
    };
    let include_disabled = match p.get("include_disabled") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return Err(bad(cmd, "`include_disabled` must be true or false")),
    };

    let query = SearchQuery::new(query_text).with_surface(surface).with_scopes(scopes).with_limit(limit).with_include_disabled(include_disabled);
    let result = global_search(s, &query, &SearchViewer::agent());
    serde_json::to_value(SearchCommandResult { result, param_warnings: Vec::new() }).map_err(|e| bad(cmd, e.to_string()))
}
