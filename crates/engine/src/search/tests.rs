use std::time::Duration;

use serde_json::json;

use super::matcher::{damerau_levenshtein, fold_str, match_query_against_fields, score_text};
use super::{
    ItemVisibility, MatchConfidence, MatchKind, SearchContext, SearchField, SearchGroup, SearchItem, SearchQuery, SearchRegistry, SearchScope,
    SearchSource, SearchSurfaces, SearchViewer, global_search,
};
use crate::Session;

#[test]
fn matcher_ranks_exact_over_prefix_word_start_substring_and_subsequence() {
    let viewer = SearchViewer::local_user();
    let score_for = |query: &str, title: &str| {
        let fields = [SearchField::title(title)];
        match_query_against_fields(query, &fields, &viewer).expect("expected match")
    };

    let exact = score_for("extrude", "Extrude");
    let prefix = score_for("ext", "Extrude");
    let word_start = score_for("ext", "Offset Extrude");
    let substring = score_for("ext", "Offset Text");
    let subseq = score_for("xtrd", "Extrude");

    assert_eq!(exact.match_kind, MatchKind::Exact);
    assert_eq!(exact.confidence, MatchConfidence::Exact);
    assert_eq!(prefix.match_kind, MatchKind::Prefix);
    assert_eq!(word_start.match_kind, MatchKind::WordStart);
    assert_eq!(substring.match_kind, MatchKind::Substring);
    assert_eq!(subseq.match_kind, MatchKind::Subsequence);

    assert!(exact.score > prefix.score);
    assert!(prefix.score > word_start.score);
    assert!(word_start.score > substring.score);
    assert!(substring.score > subseq.score);

    // Highlight byte spans stay valid UTF-8 ranges inside the original title.
    for (title, hit) in [("Extrude", &exact), ("Extrude", &prefix), ("Offset Extrude", &word_start), ("Offset Text", &substring)] {
        assert!(!hit.highlights.is_empty(), "{title}: {hit:?}");
        for span in &hit.highlights {
            assert!(title.is_char_boundary(span.start) && title.is_char_boundary(span.end) && span.start < span.end);
        }
    }
}

#[test]
fn matcher_folds_case_and_accents_and_preserves_utf8_highlight_spans() {
    assert_eq!(fold_str("Linearführung", 64), "linearfuhrung");
    assert_eq!(fold_str("Straße", 64), "strasse");

    let title = "Linearführung MGN";
    let fields = [SearchField::title(title)];
    let hit = match_query_against_fields("linearfuhrung", &fields, &SearchViewer::local_user()).expect("accent-folded match");
    assert_eq!(hit.match_kind, MatchKind::Prefix);
    assert!(!hit.near_match);
    assert_eq!(hit.highlights.len(), 1);
    let span = hit.highlights[0];
    assert_eq!(&title[span.start..span.end], "Linearführung");
}

#[test]
fn matcher_supports_multi_token_queries_and_short_token_exact_substring_rule() {
    let viewer = SearchViewer::local_user();
    let fields = [SearchField::title("Extrude"), SearchField::identifier("solid.extrude"), SearchField::keywords("boss cut join")];

    let multi = match_query_against_fields("solid extrude", &fields, &viewer).expect("multi-token match");
    assert!(!multi.near_match);
    assert_eq!(multi.confidence, MatchConfidence::Prefix);

    // Tokens shorter than 3 characters match contiguous substrings only (never subsequences).
    assert!(match_query_against_fields("ex", &[SearchField::title("Extrude")], &viewer).is_some());
    assert!(match_query_against_fields("ed", &[SearchField::title("Extrude")], &viewer).is_none());

    // `score_text` compatibility with palette expectations.
    assert!(score_text("ext", "Extrude") > score_text("ext", "Offset Text"));
    assert!(score_text("pres", "Press Pull") > score_text("ppl", "Press Pull"));
    assert!(score_text("ppl", "Press Pull") > 0);
    assert_eq!(score_text("xyz", "Extrude"), 0);
}

#[test]
fn typo_matcher_handles_one_and_two_char_typos_and_never_touches_digit_tokens() {
    let viewer = SearchViewer::local_user();

    // 1-char transposition typo (5..=7 chars)
    let hit = match_query_against_fields("exturde", &[SearchField::title("Extrude")], &viewer).expect("1-char transposition");
    assert_eq!(hit.match_kind, MatchKind::Typo);
    assert_eq!(hit.confidence, MatchConfidence::Possible);
    assert!(hit.near_match);

    // 1-char deletion typo
    let hit2 = match_query_against_fields("chmafer", &[SearchField::title("Chamfer")], &viewer).expect("chamfer transposition");
    assert!(hit2.near_match);

    // 2-char typo on 8+ char word
    let hit3 = match_query_against_fields("rectngluar", &[SearchField::title("Rectangular Pattern")], &viewer).expect("2-char typo on long word");
    assert_eq!(hit3.match_kind, MatchKind::Typo);
    assert!(hit3.near_match);

    // RFC 0019 §3.1: typo tolerance NEVER touches a token containing a digit.
    assert!(match_query_against_fields("MGN12", &[SearchField::title("MGN15")], &viewer).is_none());
    assert!(match_query_against_fields("MGN12H2R300", &[SearchField::title("MGN15H2R300")], &viewer).is_none());
    assert!(match_query_against_fields("M5", &[SearchField::title("M6")], &viewer).is_none());
    assert!(match_query_against_fields("608", &[SearchField::title("609")], &viewer).is_none());

    // Direct Damerau-Levenshtein check
    let a: Vec<char> = "exturde".chars().collect();
    let b: Vec<char> = "extrude".chars().collect();
    assert_eq!(damerau_levenshtein(&a, &b, 1), Some(1));
    assert_eq!(damerau_levenshtein(&a, &b, 0), None);
}

#[test]
fn registry_surfaces_typo_near_matches_only_when_nothing_else_matched() {
    let session = Session::default();
    let viewer = SearchViewer::local_user();

    // Exact/prefix query: near_matches must be false and no typo hits leak in.
    let clean = global_search(&session, &SearchQuery::new("extrude").with_scopes(vec![SearchScope::Commands]), &viewer);
    assert!(!clean.near_matches);
    assert!(clean.hits.iter().all(|h| !h.near_match));
    assert_eq!(clean.hits.first().map(|h| h.id.as_str()), Some("solid.extrude"));
    assert_eq!(clean.hits.first().map(|h| h.group), Some(SearchGroup::BestMatch));

    // Typo query with no exact/substring hit: falls back to near_matches with confidence Possible.
    let typo = global_search(&session, &SearchQuery::new("exturde").with_scopes(vec![SearchScope::Commands]), &viewer);
    assert!(typo.near_matches, "{typo:?}");
    assert!(typo.hits.iter().all(|h| h.near_match && h.confidence == MatchConfidence::Possible));
    assert!(typo.hits.iter().any(|h| h.id == "solid.extrude"), "{typo:?}");

    // Deterministic across repeated runs.
    let typo_again = global_search(&session, &SearchQuery::new("exturde").with_scopes(vec![SearchScope::Commands]), &viewer);
    assert_eq!(typo.hits, typo_again.hits);
}

#[test]
fn multi_source_search_finds_commands_features_bodies_sketches_components_parameters_views_parts_and_samples() {
    let mut session = Session::default();
    session.execute("parameters.change", &json!({"name": "bracket_width", "expression": "75 mm", "comment": "main flange width"})).unwrap();
    session.execute("sketch.create", &json!({"plane": "XY", "name": "BracketProfile"})).unwrap();
    session.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [40, 20]})).unwrap();
    session.execute("sketch.finish", &json!({})).unwrap();
    session.execute("solid.extrude", &json!({"distance": 10, "name": "BracketBase", "body_name": "BracketBody"})).unwrap();
    session.execute("component.create", &json!({"name": "MotorMount"})).unwrap();
    session.execute("view.save", &json!({"name": "BracketIso", "camera": {"target": [0, 0, 0], "yaw": 0.5, "pitch": 0.4, "distance": 120}})).unwrap();

    let viewer = SearchViewer::local_user();

    // Global query "bracket" spans features, bodies, sketches, parameters, named views, and samples.
    let res = global_search(&session, &SearchQuery::new("bracket"), &viewer);
    assert!(res.sources_failed.is_empty());
    let kinds: Vec<&str> = res.hits.iter().map(|h| h.kind.as_str()).collect();
    assert!(kinds.contains(&"feature"), "missing feature in {kinds:?}");
    assert!(kinds.contains(&"body"), "missing body in {kinds:?}");
    assert!(kinds.contains(&"sketch"), "missing sketch in {kinds:?}");
    assert!(kinds.contains(&"parameter"), "missing parameter in {kinds:?}");
    assert!(kinds.contains(&"named_view"), "missing named_view in {kinds:?}");
    assert!(kinds.contains(&"sample"), "missing sample in {kinds:?}");

    // Scope filtering isolates only the requested source.
    let params_only = global_search(&session, &SearchQuery::new("bracket").with_scopes(vec![SearchScope::Parameters]), &viewer);
    assert_eq!(params_only.sources_searched, vec!["design.parameters"]);
    assert!(!params_only.hits.is_empty());
    assert!(params_only.hits.iter().all(|h| h.kind == "parameter"));

    let comps_only = global_search(&session, &SearchQuery::new("MotorMount").with_scopes(vec![SearchScope::Components]), &viewer);
    assert!(comps_only.hits.iter().any(|h| h.title == "MotorMount"));

    // Standard parts search finds families & sized fasteners without false digit matches.
    let shcs = global_search(&session, &SearchQuery::new("M5x12 SHCS").with_scopes(vec![SearchScope::Parts]), &viewer);
    assert!(shcs.hits.iter().any(|h| h.id == "socket_head_cap_screw:M5x12"), "{shcs:?}");

    let iso = global_search(&session, &SearchQuery::new("ISO 4762 M6").with_scopes(vec![SearchScope::Parts]), &viewer);
    assert!(iso.hits.iter().any(|h| h.id == "socket_head_cap_screw"), "{iso:?}");

    let nonexistent_size = global_search(&session, &SearchQuery::new("M99x12 SHCS").with_scopes(vec![SearchScope::Parts]), &viewer);
    assert!(nonexistent_size.hits.is_empty(), "digit token M99 must never typo-match another size: {nonexistent_size:?}");

    // Limit truncates returned hits while preserving true total and group totals.
    let limited = global_search(&session, &SearchQuery::new("bracket").with_limit(2), &viewer);
    assert_eq!(limited.hits.len(), 2);
    assert!(limited.total >= 6, "total must reflect all matches before limit: {}", limited.total);
    let returned_sum: usize = limited.groups.iter().map(|g| g.returned).sum();
    let total_sum: usize = limited.groups.iter().map(|g| g.total).sum();
    assert_eq!(returned_sum, 2);
    assert_eq!(total_sum, limited.total);
}

struct FailingSource;
impl SearchSource for FailingSource {
    fn id(&self) -> &'static str {
        "test.failing"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Parts]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Parts
    }
    fn search(&self, _ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        Err("library index unavailable".into())
    }
}

struct SlowSource;
impl SearchSource for SlowSource {
    fn id(&self) -> &'static str {
        "test.slow"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Parts]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Parts
    }
    fn budget(&self) -> Duration {
        Duration::ZERO
    }
    fn search(&self, _ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        std::thread::sleep(Duration::from_millis(2));
        Ok(vec![SearchItem {
            id: "slow-item".into(),
            kind: "standard_part".into(),
            group: SearchGroup::Parts,
            title: "Slow Part".into(),
            subtitle: None,
            detail: None,
            icon: None,
            installed: true,
            enabled: true,
            disabled_reason: None,
            action: None,
            metadata: None,
            visibility: ItemVisibility::Public,
            fields: vec![SearchField::title("Slow Part")],
        }])
    }
}

struct VisibilityAndDupSource(&'static str, u32);
impl SearchSource for VisibilityAndDupSource {
    fn id(&self) -> &'static str {
        self.0
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Design]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Design
    }
    fn search(&self, _ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        Ok(vec![
            SearchItem {
                id: "dup-1".into(),
                kind: "feature".into(),
                group: SearchGroup::Design,
                title: "Shared Item".into(),
                subtitle: None,
                detail: Some(format!("weight {}", self.1)),
                icon: None,
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: None,
                metadata: None,
                visibility: ItemVisibility::Public,
                fields: vec![SearchField {
                    text: "Shared Item".into(),
                    weight: self.1,
                    is_identifier: false,
                    is_title: true,
                    visibility: ItemVisibility::Public,
                }],
            },
            SearchItem {
                id: "secret-item".into(),
                kind: "feature".into(),
                group: SearchGroup::Design,
                title: "Public Handle".into(),
                subtitle: None,
                detail: None,
                icon: None,
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: None,
                metadata: None,
                visibility: ItemVisibility::Public,
                fields: vec![
                    SearchField::title("Public Handle"),
                    SearchField::keywords("classified_token").with_visibility(ItemVisibility::Restricted),
                ],
            },
        ])
    }
}

#[test]
fn registry_reports_source_errors_timeouts_deduplication_and_viewer_privacy() {
    let mut reg = SearchRegistry::new();
    reg.register(Box::new(FailingSource))
        .register(Box::new(SlowSource))
        .register(Box::new(VisibilityAndDupSource("src.a", 80)))
        .register(Box::new(VisibilityAndDupSource("src.b", 100)));

    let session = Session::default();
    let res = reg.search(&session, &SearchQuery::new("Shared"), &SearchViewer::local_user());

    // Failing source and over-budget source are both named in `sources_failed` and contribute 0 hits.
    assert!(res.sources_failed.iter().any(|f| f.source == "test.failing" && !f.timed_out));
    assert!(res.sources_failed.iter().any(|f| f.source == "test.slow" && f.timed_out));
    assert!(!res.hits.iter().any(|h| h.id == "slow-item"));

    // Duplicate (kind, id) across sources collapses to one hit keeping the higher-ranked copy.
    let shared: Vec<_> = res.hits.iter().filter(|h| h.id == "dup-1").collect();
    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0].source, "src.b");

    // Viewer cannot match a restricted field (RFC 0019 §3.6).
    let restricted = reg.search(&session, &SearchQuery::new("classified_token"), &SearchViewer::local_user());
    assert!(restricted.hits.is_empty(), "viewer must never match unreadable field: {restricted:?}");
    let allowed = reg.search(&session, &SearchQuery::new("classified_token"), &SearchViewer { allow_local: true, allow_restricted: true });
    assert_eq!(allowed.hits.len(), 1);
    assert_eq!(allowed.hits[0].id, "secret-item");
}

#[test]
fn engine_search_command_returns_structured_hits_and_leaves_design_unchanged() {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "AnchorPlate"})).unwrap();
    let rev_before = session.revision;
    let undo_before = session.undo.len();

    let reply = session.execute("engine.search", &json!({"query": "AnchorPlate", "scopes": ["bodies"]})).unwrap();
    assert_eq!(session.revision, rev_before);
    assert_eq!(session.undo.len(), undo_before);
    assert_eq!(reply["total"], 1);
    assert_eq!(reply["near_matches"], false);
    assert_eq!(reply["hits"][0]["kind"], "body");
    assert_eq!(reply["hits"][0]["title"], "AnchorPlate");
    assert_eq!(reply["hits"][0]["group"], "best_match");
    assert_eq!(reply["sources_failed"], json!([]));
}

#[test]
fn phase1_typo_ranking_keeps_extrude_first() {
    let session = Session::default();
    let viewer = SearchViewer::local_user();
    let typo = global_search(&session, &SearchQuery::new("exturde").with_scopes(vec![SearchScope::Commands]), &viewer);
    assert!(typo.near_matches, "{typo:?}");
    assert_eq!(typo.hits.first().map(|h| h.id.as_str()), Some("solid.extrude"), "{typo:?}");
}

#[test]
fn document_entity_search_indexes_component_paths_occurrences_linked_sources_construction_and_formulas() {
    let mut session = Session::default();

    // Parameters: plain user parameter, formula parameter, and text parameter.
    session.execute("parameters.change", &json!({"name": "flange_span", "expression": "80 mm", "comment": "overall flange span"})).unwrap();
    session.execute("parameters.change", &json!({"name": "half_span", "expression": "flange_span / 2"})).unwrap();

    // Construction plane in root.
    session.execute("construct.plane.offset", &json!({"base": "XY", "offset": 15, "name": "MountingDatum"})).unwrap();

    // Component with a body, a sketch, and a linked source record.
    let comp = session.execute("component.create", &json!({"name": "DriveHousing"})).unwrap();
    let comp_id = comp["component"].as_u64().unwrap();
    session.execute("component.activate", &json!({"component": comp_id})).unwrap();
    session.execute("sketch.create", &json!({"plane": "XY", "name": "HousingProfile"})).unwrap();
    session.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [30, 20]})).unwrap();
    session.execute("sketch.finish", &json!({})).unwrap();
    session.execute("solid.extrude", &json!({"distance": 12, "name": "HousingBoss", "body_name": "HousingShell"})).unwrap();
    session.execute("component.activate", &json!({"component": 0})).unwrap();

    // Attach a DesignLink metadata record to test linked-source indexing without disk I/O.
    session.doc_mut().links.insert(comp_id, solidcraft_doc::DesignLink { path: "vendor/nema23_drive.scad".into(), ..Default::default() });

    // Place a second instance of DriveHousing so multi-instance body indexing is covered.
    session.execute("occurrence.copy", &json!({"component": comp_id, "translate": [60, 0, 0]})).unwrap();

    let viewer = SearchViewer::local_user();

    // 1. Searching by component name finds component, occurrences, placed bodies, sketch, and feature!
    let by_comp = global_search(&session, &SearchQuery::new("DriveHousing").with_surface(super::SearchSurface::Design), &viewer);
    let kinds: Vec<&str> = by_comp.hits.iter().map(|h| h.kind.as_str()).collect();
    assert!(kinds.contains(&"component"), "missing component in {by_comp:?}");
    assert!(kinds.contains(&"occurrence"), "missing occurrence in {by_comp:?}");
    assert!(kinds.contains(&"body"), "missing body by component path in {by_comp:?}");
    assert!(kinds.contains(&"sketch"), "missing sketch by component path in {by_comp:?}");
    assert!(kinds.contains(&"feature"), "missing feature by component path in {by_comp:?}");

    // 2. Both placed instances of HousingShell are indexed as distinct bodies with occurrence_path metadata.
    let bodies = global_search(&session, &SearchQuery::new("HousingShell").with_scopes(vec![SearchScope::Bodies]), &viewer);
    assert_eq!(bodies.hits.len(), 2, "expected 2 placed bodies: {bodies:?}");
    assert!(
        bodies.hits.iter().all(|h| h.metadata.as_ref().and_then(|m| m["occurrence_path"].as_array()).is_some_and(|p| !p.is_empty())),
        "each placed body carries its occurrence_path: {bodies:?}"
    );

    // 3. Linked source query finds the linked component and its occurrences.
    let linked = global_search(&session, &SearchQuery::new("nema23_drive").with_scopes(vec![SearchScope::Components]), &viewer);
    assert!(linked.hits.iter().any(|h| h.kind == "component" && h.title == "DriveHousing"), "{linked:?}");
    assert!(linked.hits.iter().any(|h| h.kind == "occurrence"), "{linked:?}");
    let comp_hit = linked.hits.iter().find(|h| h.kind == "component").unwrap();
    assert_eq!(comp_hit.metadata.as_ref().unwrap()["linked_source"], "vendor/nema23_drive.scad");

    // 4. Construction scope & kind filter find the offset construction plane.
    let datum = global_search(&session, &SearchQuery::new("MountingDatum").with_scopes(vec![SearchScope::Construction]), &viewer);
    assert_eq!(datum.sources_searched, vec!["design.construction"]);
    assert_eq!(datum.hits.len(), 1, "{datum:?}");
    assert_eq!(datum.hits[0].kind, "construction");
    assert_eq!(datum.hits[0].metadata.as_ref().unwrap()["construction_kind"], "plane");

    // 5. Parameter formula shows both evaluated value and expression, and is searchable by referenced parameter.
    let params = global_search(&session, &SearchQuery::new("flange_span").with_scopes(vec![SearchScope::Parameters]), &viewer);
    assert_eq!(params.hits.first().map(|h| h.title.as_str()), Some("flange_span"));
    let formula_hit = params.hits.iter().find(|h| h.title == "half_span").expect("formula parameter found by expression");
    assert!(
        formula_hit.detail.as_deref().is_some_and(|d| d.contains("40 mm") && d.contains("flange_span / 2")),
        "expected value + expression in detail: {formula_hit:?}"
    );

    // 6. Kind filtering narrows mixed design results cleanly.
    let only_body_and_param =
        global_search(&session, &SearchQuery::new("Housing").with_surface(super::SearchSurface::Design).with_kinds(vec!["body".into()]), &viewer);
    assert!(!only_body_and_param.hits.is_empty());
    assert!(only_body_and_param.hits.iter().all(|h| h.kind == "body"), "{only_body_and_param:?}");

    // 7. Feature search matches human kind label ("Extrude") as well as feature name.
    let by_kind = global_search(&session, &SearchQuery::new("Extrude").with_scopes(vec![SearchScope::Features]), &viewer);
    assert!(by_kind.hits.iter().any(|h| h.title == "HousingBoss"), "{by_kind:?}");
}

#[test]
fn document_entity_search_handles_empty_and_large_documents_cleanly() {
    let viewer = SearchViewer::local_user();

    // Empty document produces zero design hits and no errors.
    let empty = Session::default();
    let empty_res = global_search(&empty, &SearchQuery::new("anything").with_surface(super::SearchSurface::Design), &viewer);
    assert!(empty_res.hits.is_empty());
    assert_eq!(empty_res.total, 0);
    assert!(empty_res.sources_failed.is_empty());

    // Large document with 120 parameters stays bounded, deterministic, and preserves true `total`.
    let mut large = Session::default();
    for i in 0..120 {
        large.doc_mut().params.push(solidcraft_doc::Param {
            name: format!("rib_spacing_{i:03}"),
            expr: format!("{} mm", 10 + i),
            unit: "mm".into(),
            comment: String::new(),
            model: false,
        });
    }
    let res = global_search(&large, &SearchQuery::new("rib_spacing").with_scopes(vec![SearchScope::Parameters]).with_limit(15), &viewer);
    assert!(res.sources_failed.is_empty());
    assert_eq!(res.hits.len(), 15);
    assert_eq!(res.total, 120);
    assert_eq!(res.hits[0].title, "rib_spacing_000");

    // Exact match ranks #1 in BestMatch ahead of prefix hits.
    let exact = global_search(&large, &SearchQuery::new("rib_spacing_099").with_scopes(vec![SearchScope::Parameters]), &viewer);
    assert_eq!(exact.hits.first().map(|h| h.title.as_str()), Some("rib_spacing_099"));
    assert_eq!(exact.hits.first().map(|h| h.group), Some(SearchGroup::BestMatch));
}
