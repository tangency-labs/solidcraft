//! Contracts and result types for `cmd/search.rs` (`engine.search`).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use crate::search::{MAX_LIMIT, MAX_QUERY_CHARS, SearchResult, SearchScope, SearchSurface};

/// Query text for `engine.search`: a string of at most 256 characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryText(pub String);

impl<'de> Deserialize<'de> for QueryText {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(text) if text.chars().count() > MAX_QUERY_CHARS => {
                Err(D::Error::custom(whole(format!("`query` is longer than {MAX_QUERY_CHARS} characters"))))
            }
            Value::String(text) => Ok(QueryText(text)),
            _ => Err(D::Error::custom(whole("`query` must be a string"))),
        }
    }
}

impl JsonSchema for QueryText {
    fn schema_name() -> Cow<'static, str> {
        "SearchQueryText".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Search text (at most 256 characters). Case- and accent-folded; matches exact, prefix, word-start, substring, subsequence, and typo-tolerant near matches (non-digit words of 5+ letters when nothing else matches).",
            "type": "string",
            "maxLength": MAX_QUERY_CHARS
        })
    }
}

/// Optional list of search scopes for `engine.search`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeList(pub Vec<SearchScope>);

impl<'de> Deserialize<'de> for ScopeList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(items) if items.len() <= SearchScope::ALL.len() => items
                .iter()
                .map(|v| v.as_str().and_then(SearchScope::from_id))
                .collect::<Option<Vec<SearchScope>>>()
                .map(ScopeList)
                .ok_or_else(|| {
                    D::Error::custom(whole(
                        "`scopes` takes commands, design, features, bodies, sketches, components, parameters, joints, named_views, parts, samples and plugins",
                    ))
                }),
            _ => Err(D::Error::custom(whole("`scopes` must be a list of valid search scope names"))),
        }
    }
}

impl JsonSchema for ScopeList {
    fn schema_name() -> Cow<'static, str> {
        "SearchScopeList".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let ids: Vec<&str> = SearchScope::ALL.iter().map(|s| s.id()).collect();
        json_schema!({
            "description": "Optional list of search scopes to restrict which sources run. Empty or omitted searches all sources for the chosen surface.",
            "type": "array",
            "items": {"type": "string", "enum": ids},
            "maxItems": SearchScope::ALL.len()
        })
    }
}

/// Surface parameter for `engine.search`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceParam(pub SearchSurface);

impl<'de> Deserialize<'de> for SurfaceParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => SearchSurface::from_id(&s)
                .map(SurfaceParam)
                .ok_or_else(|| D::Error::custom(whole("`surface` must be global, commands, design, parts, preferences or browser"))),
            _ => Err(D::Error::custom(whole("`surface` must be global, commands, design, parts, preferences or browser"))),
        }
    }
}

impl JsonSchema for SurfaceParam {
    fn schema_name() -> Cow<'static, str> {
        "SearchSurfaceParam".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let ids: Vec<&str> = SearchSurface::ALL.iter().map(|s| s.id()).collect();
        json_schema!({
            "description": "Search surface controlling which registered sources answer when `scopes` is omitted (default: global).",
            "type": "string",
            "enum": ids
        })
    }
}

/// Bounded result limit for `engine.search`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchLimit(pub usize);

impl<'de> Deserialize<'de> for SearchLimit {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => {
                let value = n
                    .as_u64()
                    .filter(|&v| v >= 1 && v <= MAX_LIMIT as u64)
                    .ok_or_else(|| D::Error::custom(whole(format!("`limit` must be a whole number from 1 to {MAX_LIMIT}"))))?;
                Ok(SearchLimit(value as usize))
            }
            _ => Err(D::Error::custom(whole(format!("`limit` must be a whole number from 1 to {MAX_LIMIT}")))),
        }
    }
}

impl JsonSchema for SearchLimit {
    fn schema_name() -> Cow<'static, str> {
        "SearchLimit".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Maximum number of ranked hits to return (1 to 500; default 50). `total` still counts every match.",
            "type": "integer",
            "minimum": 1,
            "maximum": MAX_LIMIT
        })
    }
}

/// Search across commands, design entities, standard parts, samples and installed plugins.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SearchParams {
    /// Text to search for.
    pub query: QueryText,
    /// Optional scope filter restricting which sources run.
    #[serde(default)]
    pub scopes: Option<ScopeList>,
    /// Optional surface (`global`, `commands`, `design`, `parts`, `preferences`, `browser`).
    #[serde(default)]
    pub surface: Option<SurfaceParam>,
    /// Maximum number of hits returned (1–500; default 50).
    #[serde(default)]
    pub limit: Option<SearchLimit>,
    /// Whether to include currently disabled commands (default true).
    #[serde(default)]
    pub include_disabled: Option<bool>,
}

/// What `engine.search` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchCommandResult {
    #[serde(flatten)]
    pub result: SearchResult,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::Session;
    use crate::schema::{ParamCheck, check::schema_keys, input_schema_for};

    #[test]
    fn search_schema_keys_and_strict_contract_agree() {
        let schema = input_schema_for::<SearchParams>();
        let keys: Vec<String> = schema_keys(schema.as_value()).into_iter().collect();
        assert_eq!(keys, ["include_disabled", "limit", "query", "scopes", "surface"]);

        let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
        let ok = s.execute("engine.search", &json!({"query": "extrude", "scopes": ["commands"], "limit": 5})).unwrap();
        assert!(ok["hits"].as_array().is_some_and(|hits| !hits.is_empty()), "{ok}");
        assert!(s.execute("engine.search", &json!({"query": "extrude", "bogus": true})).is_err());
        assert!(s.execute("engine.search", &json!({})).is_err());
        assert!(s.execute("engine.search", &json!({"query": 42})).is_err());
        assert!(s.execute("engine.search", &json!({"query": "x", "limit": 0})).is_err());
        assert!(s.execute("engine.search", &json!({"query": "x", "scopes": ["unknown_scope"]})).is_err());
        assert!(s.execute("engine.search", &json!({"query": "x", "surface": "unknown_surface"})).is_err());
    }
}
