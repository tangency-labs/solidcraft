use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use super::{WorkshopListParams, WorkshopListResult, WorkshopRefreshParams, WorkshopRefreshResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};
use crate::workshop::{Catalog, Item, Kind, State, Tier, WorkshopHost};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<WorkshopListParams>(), &["kind", "limit", "query", "tier"]);
    assert_keys(input_schema_for::<WorkshopRefreshParams>(), &[]);
}

/// A host that answers with a fixed catalog and counts the refreshes asked for.
struct Fake {
    catalog: Catalog,
    refreshes: AtomicUsize,
    refuse: bool,
}

impl WorkshopHost for Fake {
    fn snapshot(&self) -> Catalog {
        self.catalog.clone()
    }
    fn refresh(&self) -> Result<(), String> {
        if self.refuse {
            return Err("a read is starting".into());
        }
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn item(id: &str, kind: Kind, tier: Tier, name: &str) -> Item {
    Item {
        id: id.into(),
        kind,
        owner: "maker".into(),
        slug: id.into(),
        name: name.into(),
        summary: format!("{name} for tests."),
        tier,
        author: "A Maker (@maker)".into(),
        licence: "MIT".into(),
        version: Some("1.2.0".into()),
        needs_app: None,
        permissions: if kind == Kind::Plugin { vec!["commands:sketch".into()] } else { Vec::new() },
        size: 48_211,
        warning: None,
        page_url: format!("https://workshop.solidcraft.io/{}/maker/{id}", kind.id()),
    }
}

fn catalog() -> Catalog {
    let mut old = item("old-gears", Kind::Plugin, Tier::Unverified, "Old gears");
    old.version = None;
    old.needs_app = Some("9.0.0".into());
    old.size = 0;
    Catalog {
        state: State::Ready,
        error: None,
        sequence: 42,
        issued_at: "2026-11-26T03:00:00.000Z".into(),
        expires_at: "2026-12-03T03:00:00.000Z".into(),
        items: vec![
            item("gears", Kind::Plugin, Tier::Community, "Gears"),
            item("shelf", Kind::Model, Tier::Unverified, "Wall shelf"),
            item("tangency-labs.fasteners", Kind::Part, Tier::Official, "Fasteners"),
            old,
        ],
    }
}

fn session(fake: &Arc<Fake>) -> Session {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    s.workshop = Some(fake.clone());
    s
}

fn fake(catalog: Catalog) -> Arc<Fake> {
    Arc::new(Fake { catalog, refreshes: AtomicUsize::new(0), refuse: false })
}

fn matches_list(reply: &Value) {
    let schema = output_schema_for::<WorkshopListResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), reply), "{reply} doesn't match {schema:?}");
}

fn ids(reply: &Value) -> Vec<String> {
    reply["items"].as_array().map(|a| a.iter().filter_map(|i| i["id"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// Without a host (the command line, the browser build) both commands say so and nothing else
/// happens.
#[test]
fn without_a_workshop_both_commands_refuse() {
    let mut s = Session::default();
    for id in ["workshop.list", "workshop.refresh"] {
        let error = s.execute(id, &json!({})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("no way to reach the Workshop"), "{id}: {error}");
    }
}

/// The list filters by kind, tier and words, counts every match in `total`, keeps the index's
/// order, and each reply matches the output schema. An item every version of which needs a newer
/// SolidCraft is listed with no version and the version it needs.
#[test]
fn the_list_filters_and_matches_its_schema() {
    let fake = fake(catalog());
    let mut s = session(&fake);
    let all = s.execute("workshop.list", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    matches_list(&all);
    assert_eq!(all["state"], "ready");
    assert_eq!(all["sequence"], 42);
    assert_eq!(all["total"], 4);
    assert_eq!(ids(&all), ["gears", "shelf", "tangency-labs.fasteners", "old-gears"]);
    assert_eq!(all["items"][0]["permissions"], json!(["commands:sketch"]));
    assert_eq!(all["items"][3]["version"], Value::Null);
    assert_eq!(all["items"][3]["needs_app"], "9.0.0");
    assert!(all["items"][0].get("needs_app").is_none(), "{all}");

    let plugins = s.execute("workshop.list", &json!({"kind": "plugin"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(ids(&plugins), ["gears", "old-gears"]);
    let official = s.execute("workshop.list", &json!({"tier": "official"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(ids(&official), ["tangency-labs.fasteners"]);
    let words = s.execute("workshop.list", &json!({"query": "WALL shelf"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(ids(&words), ["shelf"]);
    let one = s.execute("workshop.list", &json!({"limit": 1})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((ids(&one).len(), one["total"].as_u64()), (1, Some(4)));
    matches_list(&one);

    // A misspelt key runs in warn, says so, and the reply still matches.
    let warned = s.execute("workshop.list", &json!({"knd": "plugin"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    matches_list(&warned);
    assert_eq!(issues("workshop.list", &json!({"kind": "model", "limit": 5})).len(), 0);
}

/// Before the first read, and after a failed one, the list says where it is.
#[test]
fn an_empty_or_failed_catalog_says_so() {
    let fake = fake(Catalog::default());
    let mut s = session(&fake);
    let empty = s.execute("workshop.list", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((empty["state"].as_str(), empty["total"].as_u64()), (Some("empty"), Some(0)));
    matches_list(&empty);
    let failed = self::fake(Catalog { state: State::Failed, error: Some("the signature doesn't verify".into()), ..catalog() });
    let mut s = session(&failed);
    let reply = s.execute("workshop.list", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reply["state"], "failed");
    assert_eq!(reply["error"], "the signature doesn't verify");
    // The last good read's items stay listed.
    assert_eq!(reply["total"], 4);
    matches_list(&reply);
}

/// Bad values are refused with a message, never a panic, and refuse in every check level.
#[test]
fn bad_parameters_are_refused() {
    let fake = fake(catalog());
    for level in [ParamCheck::Off, ParamCheck::Warn, ParamCheck::Strict] {
        let mut s = session(&fake);
        s.param_check = level;
        for p in [
            json!({"limit": 0}),
            json!({"limit": 501}),
            json!({"limit": -1}),
            json!({"limit": 1e300}),
            json!({"limit": "ten"}),
            json!({"kind": "theme"}),
            json!({"kind": 3}),
            json!({"tier": "verified"}),
            json!({"query": "x".repeat(201)}),
            json!({"query": ["gears"]}),
            json!([1, 2]),
            json!("workshop"),
        ] {
            assert!(s.execute("workshop.list", &p).is_err(), "{level:?} {p}");
        }
        // A long query of short words is read, but no more than its limit.
        assert!(s.execute("workshop.list", &json!({"query": "a ".repeat(100)})).is_ok());
    }
}

/// Refresh asks the host once per call and says it started; a host that can't start says why.
#[test]
fn refresh_asks_the_host() {
    let fake = fake(catalog());
    let mut s = session(&fake);
    let reply = s.execute("workshop.refresh", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reply, json!({"refreshing": true}));
    let schema = output_schema_for::<WorkshopRefreshResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1);
    s.param_check = ParamCheck::Strict;
    assert!(s.execute("workshop.refresh", &json!({"now": true})).is_err());
    assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1, "strict refuses before the host is asked");

    let busy = Arc::new(Fake { catalog: catalog(), refreshes: AtomicUsize::new(0), refuse: true });
    let mut s = session(&busy);
    let error = s.execute("workshop.refresh", &json!({})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("a read is starting"), "{error}");
}

/// The host is the session's, like `param_check`: it survives opening and starting designs.
#[test]
fn the_workshop_survives_a_new_design() {
    let fake = fake(catalog());
    let mut s = session(&fake);
    assert!(s.execute("file.new", &json!({})).is_ok());
    assert!(s.workshop.is_some());
    assert_eq!(s.execute("workshop.list", &json!({})).unwrap_or_else(|e| panic!("{e}"))["total"], 4);
}
