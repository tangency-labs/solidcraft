//! The library places (#12982): the app keeps them in its preferences (`library_places` and
//! `my_library_folder`), scans each place on a thread of its own, and gives every session one
//! shared value built from the scans found so far (`Session::libraries`). The `library.places.*`
//! commands check a change and echo it; [`command`] applies the echo. A library write leaves the
//! library it read again in the session; [`adopt`] keeps it.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_engine::io::library::{self, Libraries, Place, PlaceEdit, PlaceRow, Scanned};
use solidcraft_engine::io::vfs;

use crate::SolveApp;
use crate::prefs::Prefs;

/// The app's library places: the value every session gets, and the scans it is built from.
#[derive(Default)]
pub struct Places {
    /// Built from the preferences and the scans; `None` before the first frame.
    current: Option<Arc<Libraries>>,
    built_in: Option<Arc<Scanned>>,
    /// The latest scan of each place.
    found: Vec<Arc<Scanned>>,
    /// Places whose scan couldn't run, and why.
    failed: Vec<(Place, String)>,
    /// Scans under way. A rescan drops its place's worker, so a result that comes too late is
    /// never taken.
    #[cfg(not(target_arch = "wasm32"))]
    running: Vec<(Place, crate::starting::Worker<Scanned>)>,
    /// Places a changed-only refresh asked about, checked from the next frame.
    to_check: Vec<Place>,
    /// Checks under way ([`library::rescan_if_changed`]): a new scan when the place's files
    /// changed. Dropped like `running`, and by a write, whose place they may have listed before it.
    #[cfg(not(target_arch = "wasm32"))]
    checking: Vec<(Place, crate::starting::Worker<Option<Scanned>>)>,
}

impl Places {
    /// Forget what was found for `place`, or for every place: the next frame scans it again.
    fn forget(&mut self, place: Option<&Place>) {
        let keep = |p: &Place| place.is_some_and(|wanted| wanted != p);
        self.found.retain(|s| keep(s.place()));
        self.failed.retain(|(p, _)| keep(p));
        self.to_check.retain(keep);
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.running.retain(|(p, _)| keep(p));
            self.checking.retain(|(p, _)| keep(p));
        }
    }

    /// Ask whether `place`'s files, or every place's, changed since its scan: only a scanned
    /// place is checked, and one already being checked isn't asked twice.
    fn check(&mut self, place: Option<&Place>) {
        for scan in &self.found {
            let wanted = place.is_none_or(|p| p == scan.place());
            #[cfg(not(target_arch = "wasm32"))]
            let wanted = wanted && !self.checking.iter().any(|(p, _)| p == scan.place());
            if wanted && !self.to_check.contains(scan.place()) {
                self.to_check.push(scan.place().clone());
            }
        }
    }

    /// Take a check's new scan in its place's.
    fn replace(&mut self, scan: Scanned) {
        if let Some(old) = self.found.iter_mut().find(|s| s.place() == scan.place()) {
            *old = Arc::new(scan);
        }
    }

    /// Rebuild the value from `mine` and `rows` and the scans found for them.
    fn rebuild(&mut self, mine: Option<PlaceRow>, rows: Vec<PlaceRow>) {
        let listed = |p: &Place| mine.iter().chain(&rows).any(|r| r.place == *p);
        self.found.retain(|s| listed(s.place()));
        self.failed.retain(|(p, _)| listed(p));
        self.to_check.retain(listed);
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.running.retain(|(p, _)| listed(p));
            self.checking.retain(|(p, _)| listed(p));
        }
        let built_in = Arc::clone(self.built_in.get_or_insert_with(|| Arc::new(library::scan(&Place::BuiltIn))));
        self.current = Some(Arc::new(Libraries::new(built_in, mine, rows, &self.found, &self.failed)));
    }

    /// Rebuild with the rows the value already has (after a scan finished or was forgotten).
    fn rebuild_same(&mut self) {
        if let Some(current) = self.current.clone() {
            self.rebuild(current.my_library().cloned(), current.rows().to_vec());
        }
    }
}

/// My Library and the user's places, as the preferences hold them.
fn wanted(prefs: &Prefs) -> (Option<PlaceRow>, &[PlaceRow]) {
    let folder = prefs.my_library_folder.trim();
    let mine = (!folder.is_empty()).then(|| PlaceRow { place: Place::Folder(folder.to_string()), read_only: false });
    (mine, &prefs.library_places)
}

/// Give the session the places the preferences hold now, without starting a scan: the echo hook
/// calls it, so the next command in the same batch sees the change.
pub fn sync_rows(app: &mut SolveApp) {
    let (mine, rows) = wanted(&app.preferences);
    let places = &mut app.library_places;
    // Compared in place: the rows are copied only when they changed, not every frame.
    if !places.current.as_ref().is_some_and(|c| c.my_library() == mine.as_ref() && c.rows() == rows) {
        places.rebuild(mine, rows.to_vec());
    }
    if let Some(current) = &places.current
        && !Arc::ptr_eq(&app.session.libraries, current)
    {
        app.session.libraries = Arc::clone(current);
    }
}

/// Once per frame: follow the preferences, take the scans that finished and start one for each
/// place that has none.
pub fn sync(app: &mut SolveApp, ctx: &egui::Context) {
    sync_rows(app);
    let places = &mut app.library_places;
    let Some(current) = places.current.clone() else { return };
    let mut changed = false;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use crate::starting::Poll;
        for (place, worker) in std::mem::take(&mut places.running) {
            match worker.poll() {
                Poll::Running => places.running.push((place, worker)),
                Poll::Done(scan) => {
                    places.found.push(Arc::new(scan));
                    changed = true;
                }
                Poll::Failed => {
                    places.failed.push((place, "the scan stopped before it finished".into()));
                    changed = true;
                }
            }
        }
        // A check that found nothing new, or stopped, leaves the place's scan as it is.
        for (place, worker) in std::mem::take(&mut places.checking) {
            match worker.poll() {
                Poll::Running => places.checking.push((place, worker)),
                Poll::Done(Some(scan)) => {
                    places.replace(scan);
                    changed = true;
                }
                Poll::Done(None) | Poll::Failed => {}
            }
        }
    }
    for place in std::mem::take(&mut places.to_check) {
        let Some(scan) = places.found.iter().find(|s| *s.place() == place).cloned() else { continue };
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(worker) = start_check(scan, ctx) {
            places.checking.push((place, worker));
        }
        // As a scan there: in the frame.
        #[cfg(target_arch = "wasm32")]
        if let Some(fresh) = library::rescan_if_changed(&scan) {
            places.replace(fresh);
            changed = true;
        }
    }
    for (row, state) in current.places() {
        if *state != library::RowState::Scanning {
            continue;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !places.running.iter().any(|(p, _)| *p == row.place) {
            match start(&row.place, ctx) {
                Ok(worker) => places.running.push((row.place.clone(), worker)),
                Err(why) => {
                    places.failed.push((row.place.clone(), why));
                    changed = true;
                }
            }
        }
        // The browser's files are in memory: a scan there is quick and runs in the frame.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = ctx;
            places.found.push(Arc::new(scan(&row.place)));
            changed = true;
        }
    }
    if changed {
        places.rebuild_same();
        sync_rows(app);
    }
}

/// Read `place` as the app: only a place the user's preferences list reaches its own network
/// host, under the scope of whoever starts the read.
fn as_the_app<T>(place: &Place, read: impl FnOnce() -> T) -> T {
    let hosts: Vec<String> = place.path().and_then(vfs::network_host).into_iter().collect();
    vfs::allowing_hosts(&hosts, read)
}

fn scan(place: &Place) -> Scanned {
    as_the_app(place, || library::scan(place))
}

#[cfg(not(target_arch = "wasm32"))]
fn start(place: &Place, ctx: &egui::Context) -> Result<crate::starting::Worker<Scanned>, String> {
    let scope = vfs::current_scope();
    let place = place.clone();
    crate::starting::Worker::spawn("library scan", ctx.clone(), move || vfs::scoped(scope, || scan(&place))).map_err(|e| e.to_string())
}

/// A changed-only check of `scan`'s place on a thread of its own, as [`start`] scans one. A check
/// that can't start leaves the scan as it is: the next refresh asks again.
#[cfg(not(target_arch = "wasm32"))]
fn start_check(scan: Arc<Scanned>, ctx: &egui::Context) -> std::io::Result<crate::starting::Worker<Option<Scanned>>> {
    let scope = vfs::current_scope();
    crate::starting::Worker::spawn("library check", ctx.clone(), move || {
        vfs::scoped(scope, || as_the_app(scan.place(), || library::rescan_if_changed(&scan)))
    })
}

/// The rest of a `library.places.*` command, after the engine checked it (`v`, its echo): the
/// change is applied to the preferences by the same rule the engine checked it with.
pub fn command(app: &mut SolveApp, id: &str, v: &Value) -> Result<Value, String> {
    let text = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    let at = v.get("at").and_then(Value::as_u64).and_then(|at| usize::try_from(at).ok());
    let read_only = v.get("read_only").and_then(Value::as_bool);
    sync_rows(app);
    let edit = match (id, text("path")) {
        ("library.places.add", Some(path)) => {
            let place = if text("kind").as_deref() == Some("folder") { Place::Folder(path) } else { Place::Library(path) };
            PlaceEdit::Add { row: PlaceRow { place, read_only: read_only.unwrap_or(false) }, at }
        }
        ("library.places.remove", Some(path)) => PlaceEdit::Remove { path },
        ("library.places.set", Some(path)) => PlaceEdit::Set { path, read_only, at },
        ("library.places.refresh", path) => {
            let place = match path {
                None => None,
                Some(path) => {
                    Some(app.session.libraries.listed(&path).map(|row| row.place.clone()).ok_or_else(|| format!("no place is listed at {path}"))?)
                }
            };
            // Window open and focus ask for changed places only; the Refresh button scans in full.
            if v.get("changed_only").and_then(Value::as_bool) == Some(true) {
                app.library_places.check(place.as_ref());
                return Ok(v.clone());
            }
            app.library_places.forget(place.as_ref());
            app.library_places.rebuild_same();
            sync_rows(app);
            return Ok(v.clone());
        }
        _ => return Ok(v.clone()),
    };
    app.preferences.library_places = app.session.libraries.edit(&edit)?;
    sync_rows(app);
    Ok(v.clone())
}

/// Whether `id` writes a library: its command leaves the libraries it read again in the session.
pub fn writes(id: &str) -> bool {
    id == "library.create" || (id.starts_with("library.entry.") && id != "library.entry.list")
}

/// What a library write changes, for the question the Assistant asks before it: the place and the
/// new library's name, the library and the new entry's name, the entry (and its new name), or the
/// entry and the library it is copied to. Each value as given, without control characters and at
/// most 200 characters, as the gate shows a file. `None` when `id` writes no library or names nothing.
pub fn write_names(id: &str, params: &Value) -> Option<String> {
    if !writes(id) {
        return None;
    }
    let text = |v: Option<&Value>| {
        v.and_then(Value::as_str).map(|s| s.chars().filter(|c| !c.is_control()).take(200).collect::<String>()).filter(|s| !s.is_empty())
    };
    let named = |key: &str| text(params.get(key));
    let entry_name = text(params.pointer("/definition/value/name"));
    let names = match id {
        "library.create" => [named("place"), named("name")],
        "library.entry.add" => [named("library"), entry_name],
        "library.entry.edit" => [named("entry"), entry_name],
        "library.entry.copy" => [named("entry"), named("to")],
        _ => [named("entry"), None],
    };
    let names: Vec<String> = names.into_iter().flatten().collect();
    (!names.is_empty()).then(|| names.join(", "))
}

/// After a library write: take the session's libraries as they are, so the next frame keeps the
/// write. Only a scanned place can be written, and a place being scanned has no scan in the set, so
/// no scan still running can bring the old files back.
pub fn adopt(app: &mut SolveApp) {
    let fresh = Arc::clone(&app.session.libraries);
    let places = &mut app.library_places;
    // A check may have listed the place before the write: its answer is out of date.
    #[cfg(not(target_arch = "wasm32"))]
    places.checking.clear();
    for scan in fresh.set().places().iter().skip(1) {
        if let Some(old) = places.found.iter_mut().find(|s| s.place() == scan.place()) {
            *old = Arc::clone(scan);
        }
    }
    places.current = Some(fresh);
}

/// Wait for every scan and check under way, up to `limit` each (tests).
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn finish(app: &mut SolveApp, ctx: &egui::Context, limit: std::time::Duration) {
    use crate::starting::Poll;
    for (place, worker) in std::mem::take(&mut app.library_places.running) {
        match worker.wait_for(limit) {
            Poll::Done(scan) => app.library_places.found.push(Arc::new(scan)),
            Poll::Running => app.library_places.running.push((place, worker)),
            Poll::Failed => app.library_places.failed.push((place, "the scan stopped before it finished".into())),
        }
    }
    for (place, worker) in std::mem::take(&mut app.library_places.checking) {
        match worker.wait_for(limit) {
            Poll::Done(Some(scan)) => app.library_places.replace(scan),
            Poll::Running => app.library_places.checking.push((place, worker)),
            Poll::Done(None) | Poll::Failed => {}
        }
    }
    app.library_places.rebuild_same();
    sync(app, ctx);
}
