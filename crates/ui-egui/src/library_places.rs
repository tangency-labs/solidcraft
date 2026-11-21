//! The library places (#12982): the app keeps them in its preferences (`library_places` and
//! `my_library_folder`), scans each place on a thread of its own, and gives every session one
//! shared value built from the scans found so far (`Session::libraries`). The `library.places.*`
//! commands check a change and echo it; [`command`] applies the echo.

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
}

impl Places {
    /// Forget what was found for `place`, or for every place: the next frame scans it again.
    fn forget(&mut self, place: Option<&Place>) {
        let keep = |p: &Place| place.is_some_and(|wanted| wanted != p);
        self.found.retain(|s| keep(s.place()));
        self.failed.retain(|(p, _)| keep(p));
        #[cfg(not(target_arch = "wasm32"))]
        self.running.retain(|(p, _)| keep(p));
    }

    /// Rebuild the value from `mine` and `rows` and the scans found for them.
    fn rebuild(&mut self, mine: Option<PlaceRow>, rows: Vec<PlaceRow>) {
        let listed = |p: &Place| mine.iter().chain(&rows).any(|r| r.place == *p);
        self.found.retain(|s| listed(s.place()));
        self.failed.retain(|(p, _)| listed(p));
        #[cfg(not(target_arch = "wasm32"))]
        self.running.retain(|(p, _)| listed(p));
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

/// Scan `place` as the app: only a place the user's preferences list reaches its own network
/// host, under the scope of whoever starts the scan.
fn scan(place: &Place) -> Scanned {
    let hosts: Vec<String> = place.path().and_then(vfs::network_host).into_iter().collect();
    vfs::allowing_hosts(&hosts, || library::scan(place))
}

#[cfg(not(target_arch = "wasm32"))]
fn start(place: &Place, ctx: &egui::Context) -> Result<crate::starting::Worker<Scanned>, String> {
    let scope = vfs::current_scope();
    let place = place.clone();
    crate::starting::Worker::spawn("library scan", ctx.clone(), move || vfs::scoped(scope, || scan(&place))).map_err(|e| e.to_string())
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
            match path {
                None => app.library_places.forget(None),
                Some(path) => {
                    let place =
                        app.session.libraries.listed(&path).map(|row| row.place.clone()).ok_or_else(|| format!("no place is listed at {path}"))?;
                    app.library_places.forget(Some(&place));
                }
            }
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

/// Wait for every scan under way, up to `limit` each (tests).
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
    app.library_places.rebuild_same();
    sync(app, ctx);
}
