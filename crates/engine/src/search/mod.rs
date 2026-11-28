//! Unified global search registry, typed query/result model and built-in sources
//! (RFC 0019 Phase 1, issue #22427).
//!
//! Every searchable domain (commands, design features/bodies/sketches/components/parameters/
//! joints/named views, standard parts, samples and installed plugins) implements [`SearchSource`]
//! and registers with one [`SearchRegistry`]. The engine command (`engine.search`), the MCP
//! `search` tool and UI search surfaces share this registry and [`matcher`].

pub mod matcher;
pub mod sources;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Session;

pub use matcher::{MAX_QUERY_CHARS, MAX_QUERY_TOKENS};

/// Default number of hits returned when `limit` is omitted.
pub const DEFAULT_LIMIT: usize = 50;
/// Maximum number of hits returned by one query.
pub const MAX_LIMIT: usize = 500;
/// Default per-source wall-clock budget (RFC 0019 §3.1).
pub const DEFAULT_SOURCE_BUDGET: Duration = Duration::from_millis(250);

/// Where a search request originates: only sources serving that surface run (RFC 0019 §3.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchSurface {
    /// Global search (top-right box, Ctrl/Cmd+K, MCP `search`).
    #[default]
    Global,
    /// Commands-only surface.
    Commands,
    /// Open design items surface.
    Design,
    /// Parts library / Insert Part surface.
    Parts,
    /// Preferences / settings surface.
    Preferences,
    /// Browser tree filter surface.
    Browser,
}

impl SearchSurface {
    pub const ALL: &[SearchSurface] = &[
        SearchSurface::Global,
        SearchSurface::Commands,
        SearchSurface::Design,
        SearchSurface::Parts,
        SearchSurface::Preferences,
        SearchSurface::Browser,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SearchSurface::Global => "global",
            SearchSurface::Commands => "commands",
            SearchSurface::Design => "design",
            SearchSurface::Parts => "parts",
            SearchSurface::Preferences => "preferences",
            SearchSurface::Browser => "browser",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.id().eq_ignore_ascii_case(s))
    }

    const fn bit(self) -> u8 {
        match self {
            SearchSurface::Global => 1 << 0,
            SearchSurface::Commands => 1 << 1,
            SearchSurface::Design => 1 << 2,
            SearchSurface::Parts => 1 << 3,
            SearchSurface::Preferences => 1 << 4,
            SearchSurface::Browser => 1 << 5,
        }
    }
}

/// Bitmask of [`SearchSurface`] values a [`SearchSource`] serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SearchSurfaces(u8);

impl SearchSurfaces {
    pub const GLOBAL: Self = Self(SearchSurface::Global.bit());
    pub const COMMANDS: Self = Self(SearchSurface::Commands.bit());
    pub const DESIGN: Self = Self(SearchSurface::Design.bit());
    pub const PARTS: Self = Self(SearchSurface::Parts.bit());
    pub const PREFERENCES: Self = Self(SearchSurface::Preferences.bit());
    pub const BROWSER: Self = Self(SearchSurface::Browser.bit());
    pub const ALL: Self = Self(0b0011_1111);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, surface: SearchSurface) -> bool {
        (self.0 & surface.bit()) != 0
    }
}

/// Explicit scope filters a caller may pass to narrow which sources run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    Commands,
    Design,
    Features,
    Bodies,
    Sketches,
    Construction,
    Components,
    Parameters,
    Joints,
    NamedViews,
    Parts,
    Samples,
    Plugins,
}

impl SearchScope {
    pub const ALL: &[SearchScope] = &[
        SearchScope::Commands,
        SearchScope::Design,
        SearchScope::Features,
        SearchScope::Bodies,
        SearchScope::Sketches,
        SearchScope::Construction,
        SearchScope::Components,
        SearchScope::Parameters,
        SearchScope::Joints,
        SearchScope::NamedViews,
        SearchScope::Parts,
        SearchScope::Samples,
        SearchScope::Plugins,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SearchScope::Commands => "commands",
            SearchScope::Design => "design",
            SearchScope::Features => "features",
            SearchScope::Bodies => "bodies",
            SearchScope::Sketches => "sketches",
            SearchScope::Construction => "construction",
            SearchScope::Components => "components",
            SearchScope::Parameters => "parameters",
            SearchScope::Joints => "joints",
            SearchScope::NamedViews => "named_views",
            SearchScope::Parts => "parts",
            SearchScope::Samples => "samples",
            SearchScope::Plugins => "plugins",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.id().eq_ignore_ascii_case(s))
    }
}

/// Known item kind slugs emitted by built-in search sources (used for `kinds` filtering).
pub const KNOWN_KINDS: &[&str] = &[
    "command",
    "feature",
    "body",
    "sketch",
    "construction",
    "component",
    "occurrence",
    "parameter",
    "joint",
    "named_view",
    "standard_part",
    "sample",
    "plugin",
];

/// Normalize a caller-supplied kind filter slug if it matches a known kind.
pub fn normalize_kind(s: &str) -> Option<&'static str> {
    KNOWN_KINDS.iter().copied().find(|k| k.eq_ignore_ascii_case(s))
}

/// Result group in RFC 0019 §3.3 order (stated rank, never computed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchGroup {
    BestMatch,
    Design,
    Commands,
    Parts,
    Settings,
    Help,
    InstalledPlugins,
    Workshop,
}

impl SearchGroup {
    pub const fn rank(self) -> u8 {
        match self {
            SearchGroup::BestMatch => 0,
            SearchGroup::Design => 1,
            SearchGroup::Commands => 2,
            SearchGroup::Parts => 3,
            SearchGroup::Settings => 4,
            SearchGroup::Help => 5,
            SearchGroup::InstalledPlugins => 6,
            SearchGroup::Workshop => 7,
        }
    }
}

/// Confidence tier of a match (RFC 0019 §3.2 & §3.3).
/// A `Possible` match is a suggestion only and never executes automatically on Enter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchConfidence {
    Exact,
    Prefix,
    Possible,
}

impl MatchConfidence {
    pub const fn rank(self) -> u8 {
        match self {
            MatchConfidence::Exact => 0,
            MatchConfidence::Prefix => 1,
            MatchConfidence::Possible => 2,
        }
    }
}

/// How the query matched the hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    Exact,
    Prefix,
    WordStart,
    Substring,
    Subsequence,
    Typo,
}

impl MatchKind {
    pub const fn rank(self) -> u8 {
        match self {
            MatchKind::Exact => 0,
            MatchKind::Prefix => 1,
            MatchKind::WordStart => 2,
            MatchKind::Substring => 3,
            MatchKind::Subsequence => 4,
            MatchKind::Typo => 5,
        }
    }
}

/// Half-open UTF-8 byte range inside `SearchHit::title` to highlight in UIs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct MatchSpan {
    pub start: usize,
    pub end: usize,
}

/// Read-visibility level of an item or field (RFC 0019 §3.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ItemVisibility {
    /// Readable by any viewer.
    #[default]
    Public,
    /// Readable by the local user and local agent, hidden from restricted previews.
    LocalOnly,
    /// Restricted field (e.g. private notes or credentials); only readable when explicitly allowed.
    Restricted,
}

/// Who is asking for search results (RFC 0019 §3.6).
/// Sources and the matcher never match or emit fields the viewer cannot read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchViewer {
    pub allow_local: bool,
    pub allow_restricted: bool,
}

impl SearchViewer {
    pub const fn local_user() -> Self {
        Self { allow_local: true, allow_restricted: false }
    }

    pub const fn agent() -> Self {
        Self { allow_local: true, allow_restricted: false }
    }

    pub const fn public_only() -> Self {
        Self { allow_local: false, allow_restricted: false }
    }

    pub const fn can_read(&self, visibility: ItemVisibility) -> bool {
        match visibility {
            ItemVisibility::Public => true,
            ItemVisibility::LocalOnly => self.allow_local,
            ItemVisibility::Restricted => self.allow_restricted,
        }
    }
}

impl Default for SearchViewer {
    fn default() -> Self {
        Self::local_user()
    }
}

/// One searchable text field of a [`SearchItem`].
#[derive(Clone, Debug)]
pub struct SearchField {
    pub text: String,
    /// Relative weight in percent (`100` = primary title, `90` = id, `75` = keywords, `40` = category).
    pub weight: u32,
    pub is_identifier: bool,
    pub is_title: bool,
    pub visibility: ItemVisibility,
}

impl SearchField {
    pub fn title(text: impl Into<String>) -> Self {
        Self { text: text.into(), weight: 100, is_identifier: false, is_title: true, visibility: ItemVisibility::Public }
    }

    pub fn identifier(text: impl Into<String>) -> Self {
        Self { text: text.into(), weight: 95, is_identifier: true, is_title: false, visibility: ItemVisibility::Public }
    }

    pub fn keywords(text: impl Into<String>) -> Self {
        Self { text: text.into(), weight: 75, is_identifier: false, is_title: false, visibility: ItemVisibility::Public }
    }

    pub fn secondary(text: impl Into<String>) -> Self {
        Self { text: text.into(), weight: 50, is_identifier: false, is_title: false, visibility: ItemVisibility::Public }
    }

    pub fn with_visibility(mut self, visibility: ItemVisibility) -> Self {
        self.visibility = visibility;
        self
    }
}

/// A candidate item yielded by a [`SearchSource`] before ranking and deduplication.
#[derive(Clone, Debug)]
pub struct SearchItem {
    pub id: String,
    /// Item kind slug (`command`, `feature`, `body`, `sketch`, `component`, `parameter`, `joint`,
    /// `named_view`, `standard_part`, `sample`, `plugin`). Deduplication key is `(kind, id)`.
    pub kind: String,
    pub group: SearchGroup,
    pub title: String,
    pub subtitle: Option<String>,
    pub detail: Option<String>,
    pub icon: Option<String>,
    pub installed: bool,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    /// Suggested command to run when activating this hit, if any.
    pub action: Option<String>,
    /// Structured domain metadata (e.g. feature id, body name, parameter expression, standard part family/size).
    pub metadata: Option<Value>,
    pub visibility: ItemVisibility,
    pub fields: Vec<SearchField>,
}

/// One ranked search result returned to callers (UI, engine command, MCP `search`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHit {
    pub id: String,
    pub kind: String,
    pub group: SearchGroup,
    pub source: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub confidence: MatchConfidence,
    pub match_kind: MatchKind,
    pub score: u32,
    /// True when this hit came from typo-tolerant fallback ("near matches").
    pub near_match: bool,
    pub installed: bool,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<MatchSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// True total and returned count for one group in a [`SearchResult`] (RFC 0019 §3.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchGroupCount {
    pub group: SearchGroup,
    pub total: usize,
    pub returned: usize,
}

/// A source that errored or ran over its time budget (RFC 0019 §3.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceFailure {
    pub source: String,
    pub reason: String,
    pub timed_out: bool,
}

/// Parameters for a unified search request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub surface: SearchSurface,
    pub scopes: Vec<SearchScope>,
    /// Optional item-kind slugs (`body`, `component`, `occurrence`, `feature`, `sketch`,
    /// `construction`, `parameter`, `joint`, `named_view`, `command`, `standard_part`, `sample`,
    /// `plugin`). Empty matches every kind.
    pub kinds: Vec<String>,
    pub limit: usize,
    pub include_disabled: bool,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            surface: SearchSurface::Global,
            scopes: Vec::new(),
            kinds: Vec::new(),
            limit: DEFAULT_LIMIT,
            include_disabled: true,
        }
    }

    pub fn with_surface(mut self, surface: SearchSurface) -> Self {
        self.surface = surface;
        self
    }

    pub fn with_scopes(mut self, scopes: Vec<SearchScope>) -> Self {
        self.scopes = scopes;
        self
    }

    pub fn with_kinds(mut self, kinds: Vec<String>) -> Self {
        self.kinds = kinds;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.clamp(1, MAX_LIMIT);
        self
    }

    pub fn with_include_disabled(mut self, include_disabled: bool) -> Self {
        self.include_disabled = include_disabled;
        self
    }
}

/// Result of running a [`SearchQuery`] against a [`SearchRegistry`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchResult {
    pub query: String,
    pub surface: SearchSurface,
    pub hits: Vec<SearchHit>,
    pub groups: Vec<SearchGroupCount>,
    /// True total matching items across all sources before applying `limit`.
    pub total: usize,
    /// True when no exact/substring/subsequence hits matched and results are typo-tolerant near matches.
    pub near_matches: bool,
    pub sources_searched: Vec<String>,
    pub sources_failed: Vec<SourceFailure>,
}

/// Read-only context handed to every [`SearchSource`].
pub struct SearchContext<'a> {
    pub session: &'a Session,
    pub query: &'a SearchQuery,
    pub viewer: &'a SearchViewer,
}

/// One searchable source registered with [`SearchRegistry`] (RFC 0019 §3.1).
pub trait SearchSource: Send + Sync {
    /// Stable source identifier (`"commands"`, `"design.features"`, `"parts"`, `"plugins.installed"`, …).
    fn id(&self) -> &'static str;
    /// Which surfaces query this source.
    fn surfaces(&self) -> SearchSurfaces;
    /// Explicit scopes this source satisfies.
    fn scopes(&self) -> &'static [SearchScope];
    /// Default result group for items produced by this source.
    fn group(&self) -> SearchGroup;
    /// Maximum wall-clock duration allowed before the source is treated as timed out.
    fn budget(&self) -> Duration {
        DEFAULT_SOURCE_BUDGET
    }
    /// Gather candidate items from `ctx`. Returning `Err` records an entry in `sources_failed`.
    fn search(&self, ctx: &SearchContext<'_>) -> std::result::Result<Vec<SearchItem>, String>;
}

/// Registry holding every [`SearchSource`] for unified global search.
#[derive(Default)]
pub struct SearchRegistry {
    sources: Vec<Box<dyn SearchSource>>,
}

impl SearchRegistry {
    pub fn new() -> Self {
        Self { sources: Vec::new() }
    }

    pub fn register(&mut self, source: Box<dyn SearchSource>) -> &mut Self {
        self.sources.push(source);
        self
    }

    /// Build a registry pre-populated with all built-in Phase 1 sources.
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        sources::register_builtins(&mut reg);
        reg
    }

    pub fn sources(&self) -> &[Box<dyn SearchSource>] {
        &self.sources
    }

    /// Execute `query` against every applicable source in this registry.
    pub fn search(&self, session: &Session, query: &SearchQuery, viewer: &SearchViewer) -> SearchResult {
        let normalized_query: String = query.text.trim().chars().take(MAX_QUERY_CHARS).collect();
        let bounded_limit = query.limit.clamp(1, MAX_LIMIT);
        let bounded_query = SearchQuery {
            text: normalized_query.clone(),
            surface: query.surface,
            scopes: query.scopes.clone(),
            kinds: query.kinds.clone(),
            limit: bounded_limit,
            include_disabled: query.include_disabled,
        };
        let ctx = SearchContext { session, query: &bounded_query, viewer };

        let mut sources_searched = Vec::new();
        let mut sources_failed = Vec::new();
        let mut all_hits: Vec<SearchHit> = Vec::new();

        for source in &self.sources {
            if !source_matches_request(source.as_ref(), &bounded_query) {
                continue;
            }
            sources_searched.push(source.id().to_string());
            let started = web_time::Instant::now();
            let result = source.search(&ctx);
            let elapsed = started.elapsed();

            match result {
                Err(reason) => {
                    sources_failed.push(SourceFailure { source: source.id().to_string(), reason, timed_out: false });
                }
                Ok(_) if elapsed > source.budget() => {
                    sources_failed.push(SourceFailure {
                        source: source.id().to_string(),
                        reason: format!("source `{}` exceeded its {} ms search budget", source.id(), source.budget().as_millis()),
                        timed_out: true,
                    });
                }
                Ok(items) => {
                    for item in items {
                        if !viewer.can_read(item.visibility) {
                            continue;
                        }
                        if !bounded_query.include_disabled && !item.enabled {
                            continue;
                        }
                        if !bounded_query.kinds.is_empty() && !bounded_query.kinds.iter().any(|k| k.eq_ignore_ascii_case(&item.kind)) {
                            continue;
                        }
                        let Some(matched) = matcher::match_query_against_fields(&normalized_query, &item.fields, viewer) else {
                            continue;
                        };
                        let group = if !normalized_query.is_empty() && matched.exact_identifier && !matched.near_match {
                            SearchGroup::BestMatch
                        } else {
                            item.group
                        };
                        all_hits.push(SearchHit {
                            id: item.id,
                            kind: item.kind,
                            group,
                            source: source.id().to_string(),
                            title: item.title,
                            subtitle: item.subtitle,
                            detail: item.detail,
                            icon: item.icon,
                            confidence: matched.confidence,
                            match_kind: matched.match_kind,
                            score: matched.score,
                            near_match: matched.near_match,
                            installed: item.installed,
                            enabled: item.enabled,
                            disabled_reason: item.disabled_reason,
                            highlights: matched.highlights,
                            action: item.action,
                            metadata: item.metadata,
                        });
                    }
                }
            }
        }

        // RFC 0019 §3.1: typo-tolerant near matches only surface when nothing else matched.
        let has_strong = all_hits.iter().any(|h| !h.near_match);
        let near_matches = !has_strong && !all_hits.is_empty();
        if has_strong {
            all_hits.retain(|h| !h.near_match);
        }

        // Deduplicate by (kind, id), keeping the highest-ranked hit (RFC 0019 §3.3).
        let mut deduped: HashMap<(String, String), SearchHit> = HashMap::with_capacity(all_hits.len());
        for hit in all_hits {
            let key = (hit.kind.clone(), hit.id.clone());
            match deduped.get(&key) {
                Some(existing) if compare_hits(existing, &hit) != std::cmp::Ordering::Greater => {}
                _ => {
                    deduped.insert(key, hit);
                }
            }
        }
        let mut hits: Vec<SearchHit> = deduped.into_values().collect();
        hits.sort_by(compare_hits);

        // True totals per group before applying `limit` (RFC 0019 §3.3).
        let total = hits.len();
        let mut group_totals: BTreeMap<u8, (SearchGroup, usize, usize)> = BTreeMap::new();
        for hit in &hits {
            let entry = group_totals.entry(hit.group.rank()).or_insert((hit.group, 0, 0));
            entry.1 += 1;
        }
        hits.truncate(bounded_limit);
        for hit in &hits {
            if let Some(entry) = group_totals.get_mut(&hit.group.rank()) {
                entry.2 += 1;
            }
        }
        let groups =
            group_totals.into_values().map(|(group, group_total, returned)| SearchGroupCount { group, total: group_total, returned }).collect();

        SearchResult { query: normalized_query, surface: bounded_query.surface, hits, groups, total, near_matches, sources_searched, sources_failed }
    }
}

fn source_matches_request(source: &dyn SearchSource, query: &SearchQuery) -> bool {
    if !query.scopes.is_empty() {
        return source.scopes().iter().any(|scope| query.scopes.contains(scope));
    }
    source.surfaces().contains(query.surface)
}

/// Deterministic hit ordering per RFC 0019 §3.3:
/// group rank -> confidence -> installed -> enabled -> match score (desc) -> stable id -> kind.
fn compare_hits(a: &SearchHit, b: &SearchHit) -> std::cmp::Ordering {
    a.group
        .rank()
        .cmp(&b.group.rank())
        .then_with(|| a.confidence.rank().cmp(&b.confidence.rank()))
        .then_with(|| b.installed.cmp(&a.installed))
        .then_with(|| b.enabled.cmp(&a.enabled))
        .then_with(|| b.score.cmp(&a.score))
        .then_with(|| a.id.cmp(&b.id))
        .then_with(|| a.kind.cmp(&b.kind))
}

/// Shared process-wide default registry with all built-in Phase 1 search sources.
pub fn default_registry() -> &'static SearchRegistry {
    static REGISTRY: OnceLock<SearchRegistry> = OnceLock::new();
    REGISTRY.get_or_init(SearchRegistry::with_builtins)
}

/// Run a global search against the default built-in registry.
pub fn global_search(session: &Session, query: &SearchQuery, viewer: &SearchViewer) -> SearchResult {
    default_registry().search(session, query, viewer)
}

/// Extra search keywords for commands whose names do not contain common terms people type
/// (file formats, continuity levels, 3D printing). Shared by global search and the UI palette.
pub fn command_keywords(id: &str) -> &'static str {
    match id {
        "file.save_mesh" => "export stl 3mf mesh 3d print printing slicer",
        "file.export" => "export stl 3mf obj step urdf sdf gazebo mjcf mujoco mesh 3d print",
        "file.export_step" => "stp cad",
        "file.export_stl" | "file.export_3mf" => "mesh 3d print printing",
        "file.export_obj" => "mesh",
        "file.export_urdf" | "file.export_urdf_options" => "robot ros simulation",
        "file.export_sdf" => "gazebo model simulation world texture",
        "file.export_mjcf" => "mujoco robot simulation",
        "sketch.constraint.tangent" => "g1 tangency continuity smooth",
        "sketch.constraint.curvature" => "g2 curvature continuity smooth",
        "sketch.blend_curve" => "g1 g2 g3 g4 continuity tangent curvature bridge",
        "solid.fillet" => "g1 g2 continuity curvature blend round",
        _ => "",
    }
}
