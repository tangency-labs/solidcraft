//! The Workshop window (Plugins › Browse the Workshop…): what the Workshop's signed index lists,
//! with search, a kind filter and the tier badges, read in the background.
//!
//! The list comes from the same checked index as a Workshop link (`index`): fetched from
//! workshop.solidcraft.io and used only once its Ed25519 signature verifies with a key built in,
//! its sequence hasn't gone back and it hasn't expired. A failed read keeps the last good list and
//! says why. Choosing an item hands it to the link's question (`super::show`), so it opens or
//! installs only after the same ask; nothing downloads from the list itself.
//!
//! The session reads the same list through `workshop.list` and starts a read with
//! `workshop.refresh` ([`Host`]).

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use solidcraft_engine::workshop::{self as catalog, Catalog, Filter, WorkshopHost};
use solidcraft_ui_text::{format, tr};

use super::HttpGet;
use super::index::{self, Refusal};
use super::link::{Kind, Link, Target};
use crate::{SolveApp, ui_kit};

/// The most items read from one index: more is not a list anyone scrolls.
pub(crate) const MAX_ITEMS: usize = 5000;
/// The most rows the window draws at once; the search narrows the rest.
const MAX_ROWS: usize = 200;

/// The window's state, and the list the worker fills.
pub(crate) struct Browser {
    pub(crate) open: bool,
    pub(crate) query: String,
    /// 0: every kind; then plugins, models, parts libraries ([`catalog::Kind::ALL`]).
    pub(crate) kind: usize,
    pub(crate) shared: Arc<Mutex<Shared>>,
}

impl Default for Browser {
    fn default() -> Self {
        Browser { open: false, query: String::new(), kind: 0, shared: Arc::new(Mutex::new(Shared::default())) }
    }
}

/// The checked list, shared with the worker that reads it and with the session.
#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) catalog: Catalog,
    /// The index the list came from, for resolving the chosen item.
    pub(crate) index: Option<Arc<index::Index>>,
    loading: bool,
    /// The window's context, to repaint it when a read finishes.
    ctx: Option<egui::Context>,
}

pub(crate) fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The session's way to the list: `workshop.list` reads it, `workshop.refresh` starts a read.
struct Host {
    shared: Arc<Mutex<Shared>>,
    get: HttpGet,
}

impl WorkshopHost for Host {
    fn snapshot(&self) -> Catalog {
        lock(&self.shared).catalog.clone()
    }

    fn refresh(&self) -> Result<(), String> {
        start(&self.shared, self.get.clone())
    }
}

/// Give the session the Workshop, where this build has one (call after the services are set).
pub fn install(app: &mut SolveApp) {
    if !super::available(app) {
        return;
    }
    let Some(get) = app.services.workshop_get.clone() else { return };
    app.session.workshop = Some(Arc::new(Host { shared: app.workshop.browser.shared.clone(), get }));
}

/// Whether the Plugins menu offers the window.
pub(crate) fn offered(app: &SolveApp) -> bool {
    super::available(app)
}

/// Plugins › Browse the Workshop…: open the window, reading the list if it hasn't been read.
pub(crate) fn open(app: &mut SolveApp) {
    if !offered(app) {
        return;
    }
    app.workshop.browser.open = true;
    let unread = lock(&app.workshop.browser.shared).catalog.state == catalog::State::Empty;
    if unread {
        refresh(app);
    }
}

/// The kind filter's ids, in the window's order (0 is every kind).
const KIND_IDS: &[&str] = &["all", "plugin", "model", "part"];

/// `ui.workshop {open?, query?, kind?, choose?}`: the Workshop window as a click would drive it.
/// `open` shows or hides it (opening reads the list when it hasn't been read), `query` and `kind`
/// (`all`, `plugin`, `model`, `part`) set its search and filter, and `choose` (a listed id) asks
/// the item's question, as Open… or Install… does. Returns the window's state and the shown ids.
pub(crate) fn control(app: &mut SolveApp, p: &serde_json::Value) -> Result<serde_json::Value, String> {
    if !offered(app) {
        return Err("not supported yet: this app has no way to reach the Workshop (the desktop app has)".into());
    }
    if let Some(query) = p.get("query") {
        let query = query.as_str().filter(|q| q.chars().count() <= catalog::MAX_QUERY).ok_or("`query` must be text of at most 200 characters")?;
        app.workshop.browser.query = query.to_string();
    }
    if let Some(kind) = p.get("kind") {
        let at = kind.as_str().and_then(|k| KIND_IDS.iter().position(|id| *id == k)).ok_or("`kind` must be all, plugin, model or part")?;
        app.workshop.browser.kind = at;
    }
    match p.get("open").map(serde_json::Value::as_bool) {
        Some(Some(true)) => open(app),
        Some(Some(false)) => app.workshop.browser.open = false,
        Some(None) => return Err("`open` must be true or false".into()),
        None => {}
    }
    if let Some(id) = p.get("choose") {
        let id = id.as_str().ok_or("`choose` must be a listed item's id")?;
        let kind = lock(&app.workshop.browser.shared).catalog.items.iter().find(|i| i.id == id).map(|i| i.kind);
        let kind = kind.ok_or_else(|| std::format!("no listed item `{id}`"))?;
        choose(app, kind, id);
    }
    let browser = &app.workshop.browser;
    let s = lock(&browser.shared);
    let shown: Vec<&str> = filter_of(browser).apply(&s.catalog.items).take(MAX_ROWS).map(|i| i.id.as_str()).collect();
    Ok(serde_json::json!({
        "open": browser.open,
        "query": browser.query,
        "kind": KIND_IDS.get(browser.kind).copied().unwrap_or("all"),
        "state": s.catalog.state,
        "error": s.catalog.error,
        "shown": shown,
        "asking": app.workshop.ask.as_ref().map(|a| a.id.clone()),
    }))
}

/// What the window's search and kind filter keep.
fn filter_of(browser: &Browser) -> Filter {
    Filter { kind: browser.kind.checked_sub(1).and_then(|k| catalog::Kind::ALL.get(k).copied()), tier: None, query: browser.query.clone() }
}

/// Read the list again in the background.
pub(crate) fn refresh(app: &mut SolveApp) {
    let Some(get) = app.services.workshop_get.clone() else { return };
    if let Err(error) = start(&app.workshop.browser.shared, get) {
        app.set_status(format("workshop.unreachable", &[("error", &error)]), true);
    }
}

/// Start a read on a worker unless one is running.
fn start(shared: &Arc<Mutex<Shared>>, get: HttpGet) -> Result<(), String> {
    {
        let mut s = lock(shared);
        if s.loading {
            return Ok(());
        }
        s.loading = true;
        s.catalog.state = catalog::State::Loading;
    }
    let worker = shared.clone();
    let seen = super::sequence_file();
    let spawned = std::thread::Builder::new().name("workshop-index".into()).spawn(move || {
        let outcome = read(index::KEYS, &|url: &str| get(url), seen.as_deref(), super::unix_now(), &app_version());
        let mut s = lock(&worker);
        settle(&mut s, outcome);
        if let Some(ctx) = &s.ctx {
            ctx.request_repaint();
        }
    });
    match spawned {
        Ok(_) => Ok(()),
        Err(error) => {
            settle(&mut lock(shared), Err(Refusal::Unreachable(error.to_string())));
            Err(error.to_string())
        }
    }
}

fn app_version() -> semver::Version {
    crate::update::parse_version(crate::help::VERSION).unwrap_or_else(|| semver::Version::new(0, 0, 0))
}

/// Fetch and check the index, then list it.
pub(crate) fn read(
    keys: &[(&str, &str)],
    get: &dyn Fn(&str) -> Result<super::Signed, String>,
    seen: Option<&std::path::Path>,
    now: i64,
    app_version: &semver::Version,
) -> Result<(index::Index, Vec<catalog::Item>), Refusal> {
    let index = super::fetch_index(keys, get, seen, now)?;
    let items = items(&index, app_version);
    Ok((index, items))
}

/// Take a finished read: a good one replaces the list, a failed one keeps the last good list and
/// says why.
pub(crate) fn settle(s: &mut Shared, outcome: Result<(index::Index, Vec<catalog::Item>), Refusal>) {
    s.loading = false;
    match outcome {
        Ok((index, items)) => {
            s.catalog = Catalog {
                state: catalog::State::Ready,
                error: None,
                sequence: index.sequence,
                issued_at: index.issued_at.clone(),
                expires_at: index.expires_at.clone(),
                items,
            };
            s.index = Some(Arc::new(index));
        }
        Err(refusal) => {
            s.catalog.state = catalog::State::Failed;
            s.catalog.error = Some(super::refusal_text(&refusal));
        }
    }
}

fn link_kind(kind: catalog::Kind) -> Kind {
    match kind {
        catalog::Kind::Model => Kind::Model,
        catalog::Kind::Plugin => Kind::Plugin,
        catalog::Kind::Part => Kind::Part,
    }
}

fn catalog_tier(tier: index::Tier) -> catalog::Tier {
    match tier {
        index::Tier::Official => catalog::Tier::Official,
        index::Tier::Community => catalog::Tier::Community,
        index::Tier::Unverified => catalog::Tier::Unverified,
    }
}

/// The item's page on the Workshop, `https://workshop.solidcraft.io/<kind>/<owner>/<slug>`, when
/// its address is one the site writes.
fn page_url(kind: catalog::Kind, owner: &str, slug: &str) -> String {
    if super::link::segment(owner) && super::link::segment(slug) {
        std::format!("https://{}/{}/{owner}/{slug}", index::HOST, kind.id())
    } else {
        String::new()
    }
}

/// What the index lists that this SolidCraft could open or install, in the index's order: the
/// newest version that runs here (the one a link without a version gets), or, when every
/// version needs a newer SolidCraft, no version and the one it needs. Delisted items, items
/// whose every version is blocked, unknown kinds and entries a link would refuse are left out.
pub(crate) fn items(index: &index::Index, app_version: &semver::Version) -> Vec<catalog::Item> {
    let mut out = Vec::new();
    for item in index.items.iter().take(MAX_ITEMS) {
        let kind = match item.kind.as_str() {
            "model" => catalog::Kind::Model,
            "plugin" => catalog::Kind::Plugin,
            "library" => catalog::Kind::Part,
            _ => continue,
        };
        let link = Link { kind: link_kind(kind), target: Target::Id(item.id.clone()), version: None };
        let mut listed = catalog::Item {
            id: item.id.clone(),
            kind,
            owner: item.owner.clone(),
            slug: item.slug.clone(),
            name: item.name.clone(),
            summary: item.summary.clone(),
            tier: catalog_tier(index::Tier::from_index(&item.tier)),
            author: index::author_of(item),
            licence: item.licence.clone(),
            version: None,
            needs_app: None,
            permissions: Vec::new(),
            size: 0,
            warning: None,
            page_url: page_url(kind, &item.owner, &item.slug),
        };
        match index::resolve(index, &link, app_version) {
            Ok(resolved) => {
                listed.version = Some(resolved.version.to_string());
                listed.permissions = resolved.permissions;
                listed.size = resolved.file.size;
                listed.warning = resolved.warning;
            }
            Err(Refusal::NeedsNewerApp(version)) => listed.needs_app = Some(version),
            Err(_) => continue,
        }
        out.push(listed);
    }
    out
}

/// Open or install the listed item `id`: resolve it again in the index it came from (still
/// fresh) and ask, as a link does.
pub(crate) fn choose(app: &mut SolveApp, kind: catalog::Kind, id: &str) {
    if app.workshop.job.is_some() || app.workshop.ask.is_some() {
        app.set_status(tr("workshop.busy").to_string(), true);
        return;
    }
    let Some(index) = lock(&app.workshop.browser.shared).index.clone() else {
        super::refuse(app, &Refusal::NotListed(id.to_string()));
        return;
    };
    let link = Link { kind: link_kind(kind), target: Target::Id(id.to_string()), version: None };
    match index::check_fresh(&index, None, super::unix_now()).and_then(|()| index::resolve(&index, &link, &app_version())) {
        Ok(item) => app.workshop.ask = Some(item),
        Err(refusal) => super::refuse(app, &refusal),
    }
}

fn kind_text(kind: catalog::Kind) -> &'static str {
    tr(match kind {
        catalog::Kind::Model => "workshop.kind_model",
        catalog::Kind::Plugin => "workshop.kind_plugin",
        catalog::Kind::Part => "workshop.kind_part",
    })
}

fn tier_text(tier: catalog::Tier) -> &'static str {
    super::tier_text(match tier {
        catalog::Tier::Official => index::Tier::Official,
        catalog::Tier::Community => index::Tier::Community,
        catalog::Tier::Unverified => index::Tier::Unverified,
    })
}

/// The line under an item's name: kind, tier, author, size.
pub(crate) fn meta_line(item: &catalog::Item) -> String {
    let mut parts = vec![kind_text(item.kind).to_string(), tier_text(item.tier).to_string()];
    if !item.author.is_empty() {
        parts.push(item.author.clone());
    }
    if item.size > 0 {
        parts.push(super::size_text(item.size));
    }
    parts.join(" · ")
}

/// The window, while it is open.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.workshop.browser.open {
        return;
    }
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let shared = app.workshop.browser.shared.clone();
    let mut s = lock(&shared);
    if s.ctx.is_none() {
        s.ctx = Some(ctx.clone());
    }
    let browser = &mut app.workshop.browser;
    let filter = filter_of(browser);
    let kinds = [tr("workshop.all"), tr("workshop.plugins"), tr("workshop.models"), tr("workshop.parts")];
    let mut chosen = None;
    let mut reread = false;
    let (closed, _) = ui_kit::Window::new(egui::Id::unique("sc_workshop_browser"), "import", tr("workshop.browser_title"))
        .fields()
        .actions("", "", None)
        .show(ctx, true, |ui| {
            ui_kit::search_field(ui, egui::Id::unique("sc_workshop_search"), &mut browser.query, tr("workshop.search_hint"));
            ui.horizontal(|ui| {
                ui_kit::segmented(
                    ui,
                    &mut browser.kind,
                    &kinds,
                    ui_kit::Segments { targets: &["workshop:all", "workshop:plugins", "workshop:models", "workshop:parts"], ..Default::default() },
                );
                let again = ui_kit::button(ui, tr("workshop.refresh"), ui_kit::Button::Enabled(!s.loading));
                crate::scenario::publish_handle("workshop:refresh", again.rect.center());
                reread = again.clicked();
            });
            match s.catalog.state {
                catalog::State::Empty | catalog::State::Loading => ui_kit::status_line(ui, tr("workshop.loading"), ui_kit::StatusTone::Dim),
                catalog::State::Failed => {
                    let error = s.catalog.error.clone().unwrap_or_default();
                    let line = if s.catalog.items.is_empty() { error } else { format("workshop.kept", &[("error", &error)]) };
                    ui_kit::status_line(ui, &line, ui_kit::StatusTone::Error)
                }
                catalog::State::Ready => {
                    let count = s.catalog.items.len().to_string();
                    ui_kit::status_line(ui, &format("workshop.count", &[("count", &count)]), ui_kit::StatusTone::Dim)
                }
            };
            let kept: Vec<&catalog::Item> = filter.apply(&s.catalog.items).collect();
            if kept.is_empty() && !s.catalog.items.is_empty() {
                ui_kit::text(ui, tr("workshop.no_match"), ui_kit::Text::Hint("workshop_no_match"));
            }
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.55).auto_shrink([false, true]).show(ui, |ui| {
                for (i, item) in kept.iter().take(MAX_ROWS).enumerate() {
                    ui.push_id(("workshop_item", i), |ui| {
                        let heading = match &item.version {
                            Some(version) => std::format!("{} {version}", item.name),
                            None => item.name.clone(),
                        };
                        ui_kit::text(ui, &heading, ui_kit::Text::Heading("workshop_item_name"));
                        ui_kit::text(ui, &meta_line(item), ui_kit::Text::Hint("workshop_item_meta"));
                        if !item.summary.is_empty() {
                            ui_kit::text(ui, &item.summary, ui_kit::Text::Note("workshop_item_summary"));
                        }
                        if let Some(warning) = &item.warning {
                            let reason = if warning.trim().is_empty() { tr("workshop.no_reason") } else { warning.as_str() };
                            ui_kit::status_line(ui, &format("workshop.warning", &[("reason", reason)]), ui_kit::StatusTone::Warning);
                        }
                        match &item.needs_app {
                            Some(version) => {
                                ui_kit::status_line(ui, &format("workshop.needs_newer", &[("version", version)]), ui_kit::StatusTone::Dim);
                            }
                            None => {
                                let label = if item.kind == catalog::Kind::Model { tr("workshop.open_item") } else { tr("workshop.install_item") };
                                let button = ui_kit::button(ui, label, ui_kit::Button::Normal);
                                crate::scenario::publish_handle(&std::format!("workshop:item:{}", item.id), button.rect.center());
                                if button.clicked() {
                                    chosen = Some((item.kind, item.id.clone()));
                                }
                            }
                        }
                        ui.separator();
                    });
                }
            });
        });
    drop(s);
    if closed {
        app.workshop.browser.open = false;
    }
    if reread {
        refresh(app);
    }
    if let Some((kind, id)) = chosen {
        choose(app, kind, &id);
    }
}
