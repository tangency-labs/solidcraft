//! Picking and drawing numbered copies while editing a pattern's skip list.

use std::collections::BTreeSet;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{FontId, Pos2, Rect, Stroke, Vec2};
use serde_json::Value;
use solidcraft_engine::doc::{
    PatternAddress, PatternGroupChoice, PatternMarker, PatternMarkerInfo, PatternMember, PatternMemberIdentity, PatternMemberKey,
};
use solidcraft_engine::geom::prelude::*;
use solidcraft_theme::Tokens;

use crate::{SolveApp, dialogs::Dialog, selection::BoxSel, viewport::Proj};

const MARKER_RADIUS: f32 = 7.0;

#[derive(Clone, Debug)]
struct Key {
    session: u64,
    revision: u64,
    component: u64,
    frame: [[f64; 4]; 4],
    command: &'static str,
    params: Value,
    dialog: Arc<()>,
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.session == other.session
            && self.revision == other.revision
            && self.component == other.component
            && self.frame == other.frame
            && self.command == other.command
            && self.params == other.params
            && Arc::ptr_eq(&self.dialog, &other.dialog)
    }
}

pub(crate) struct Cache {
    key: Key,
    groups: Vec<PatternGroupChoice>,
    markers: Vec<PatternMarker>,
    labels: Vec<String>,
    unavailable_members: usize,
    error: Option<String>,
}

impl Cache {
    fn failed(key: Key, error: String) -> Self {
        Self { key, groups: Vec::new(), markers: Vec::new(), labels: Vec::new(), unavailable_members: 0, error: Some(error) }
    }
}

fn key_for_dialog(app: &SolveApp, dialog: &Dialog) -> Option<Key> {
    if !dialog.pattern_skip_active() && !dialog.has_pattern_skips() {
        return None;
    }
    let (command, params) = crate::dialogs::pattern_skip_params(app, dialog)?;
    let session = &app.session;
    let component = dialog.editing.and_then(|(id, _)| session.doc.feature(id)).map_or(session.active_component, |feature| feature.component);
    Some(Key {
        session: session.uid,
        revision: session.revision,
        component,
        frame: crate::dialogs::feature_frame(session, component),
        command,
        params,
        dialog: dialog.pattern_marker_owner.clone(),
    })
}

fn cache_for_dialog<'a>(app: &'a SolveApp, dialog: &Dialog) -> Option<&'a Cache> {
    let key = key_for_dialog(app, dialog)?;
    app.viewport.pattern_skip_cache.as_ref().filter(|cache| cache.key == key)
}

fn compute(key: Key, session: solidcraft_engine::Session) -> Cache {
    match session.pattern_marker_info(key.command, &key.params) {
        Ok(PatternMarkerInfo { groups, markers, unavailable_members }) => {
            let labels = marker_labels(&session.doc, &key.params, &markers);
            Cache { key, groups, markers, labels, unavailable_members, error: None }
        }
        Err(error) => Cache::failed(key, error.to_string()),
    }
}

/// Keep one worker across dialog and document changes. New inputs replace the desired job,
/// and wait for the running job to finish instead of leaving detached computations behind.
#[derive(Default)]
pub(crate) struct Jobs {
    #[cfg(not(target_arch = "wasm32"))]
    running: Option<Running>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Running {
    key: Key,
    result: Receiver<Cache>,
}

impl Jobs {
    fn busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.running.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    fn collect(&mut self, wanted: Option<&Key>) -> Option<Cache> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let running = self.running.as_ref()?;
            let completed = match running.result.try_recv() {
                Ok(cache) => cache,
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => Cache::failed(running.key.clone(), "the pattern marker computation stopped".into()),
            };
            self.running = None;
            wanted.filter(|key| **key == completed.key).map(|_| completed)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = wanted;
            None
        }
    }

    fn start(&mut self, key: Key, session: solidcraft_engine::Session, ctx: &egui::Context) -> Option<Cache> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (send, result) = std::sync::mpsc::channel();
            let worker_key = key.clone();
            let repaint = ctx.clone();
            match std::thread::Builder::new().name("pattern-markers".into()).spawn(move || {
                let _ = send.send(compute(worker_key, session));
                repaint.request_repaint();
            }) {
                Ok(_) => {
                    self.running = Some(Running { key, result });
                    None
                }
                Err(error) => Some(Cache::failed(key, error.to_string())),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = ctx;
            Some(compute(key, session))
        }
    }
}

/// Runs before layout even when the model viewport is hidden, so cancelled jobs are drained.
pub(crate) fn update(app: &mut SolveApp, ctx: &egui::Context) {
    let wanted = app.dialog.as_ref().and_then(|dialog| key_for_dialog(app, dialog));
    if let Some(cache) = app.viewport.pattern_marker_jobs.collect(wanted.as_ref()) {
        app.viewport.pattern_skip_cache = Some(cache);
    }
    if app.viewport.pattern_skip_cache.as_ref().is_some_and(|cache| Some(&cache.key) != wanted.as_ref()) {
        app.viewport.pattern_skip_cache = None;
    }
    if !app.viewport.pattern_marker_jobs.busy()
        && app.viewport.pattern_skip_cache.is_none()
        && let Some(key) = wanted
    {
        let mut session = app.session.scratch();
        if key.component != session.active_component {
            session.active_component = key.component;
            session.active_occurrence_path = None;
        }
        app.viewport.pattern_skip_cache = app.viewport.pattern_marker_jobs.start(key, session, ctx);
    }
    if app.viewport.pattern_marker_jobs.busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

pub(crate) fn busy(app: &SolveApp) -> bool {
    app.viewport.pattern_marker_jobs.busy()
        || app
            .dialog
            .as_ref()
            .and_then(|dialog| key_for_dialog(app, dialog))
            .is_some_and(|key| app.viewport.pattern_skip_cache.as_ref().is_none_or(|cache| cache.key != key))
}

pub(crate) fn show_status(app: &SolveApp, dialog: &Dialog, ui: &mut egui::Ui) {
    if let Some(cache) = cache_for_dialog(app, dialog) {
        if let Some(error) = &cache.error {
            let message = solidcraft_ui_text::format("ui.pattern_skip_failed", &[("error", error)]);
            crate::ui_kit::text(ui, &message, crate::ui_kit::Text::Error("pattern_markers"));
        } else if cache.unavailable_members > 0 {
            crate::ui_kit::text(ui, solidcraft_ui_text::tr("ui.pattern_skip_unavailable"), crate::ui_kit::Text::Hint("pattern_markers"));
            if cache.groups.iter().any(|group| dialog.pattern_skipped().0.binary_search(&group.address).is_ok()) {
                crate::ui_kit::text(
                    ui,
                    solidcraft_ui_text::tr("ui.pattern_skip_restore_whole_note"),
                    crate::ui_kit::Text::Hint("pattern_group_restore"),
                );
            }
        }
    } else if key_for_dialog(app, dialog).is_some() {
        crate::ui_kit::text(ui, solidcraft_ui_text::tr("ui.pattern_skip_computing"), crate::ui_kit::Text::Hint("pattern_markers"));
    }
}

pub(crate) fn active(app: &SolveApp) -> bool {
    app.dialog.as_ref().is_some_and(Dialog::pattern_skip_active)
}

pub(crate) fn project(app: &mut SolveApp, proj: &Proj) -> Vec<(usize, Pos2)> {
    if !active(app) {
        return Vec::new();
    }
    let Some(cache) = app.dialog.as_ref().and_then(|dialog| cache_for_dialog(app, dialog)) else { return Vec::new() };
    let back = proj.cam.back();
    let eye = proj.cam.eye();
    cache
        .markers
        .iter()
        .enumerate()
        .filter_map(|(index, marker)| {
            let [first, second] = marker.positions;
            let towards_eye = if proj.cam.fov > 0.0 { eye - (first + second) * 0.5 } else { back };
            let world = if (second - first).dot(towards_eye) > 0.0 { second } else { first };
            let point = proj.to_screen(world)?;
            (point.is_finite() && proj.rect.contains(point) && !crate::shell::covered(app, point)).then_some((index + 2, point))
        })
        .collect()
}

pub(crate) fn skipped_handles(app: &SolveApp, dialog: &Dialog) -> Vec<usize> {
    let (slots, members) = dialog.pattern_skipped();
    let Some(cache) = cache_for_dialog(app, dialog) else { return Vec::new() };
    cache.markers.iter().enumerate().filter_map(|(i, marker)| is_skipped(marker, slots, members).then_some(i + 2)).collect()
}

pub(crate) fn skipped_choices<'a>(app: &'a SolveApp, dialog: &Dialog) -> Vec<(usize, &'a str, bool)> {
    let Some(cache) = cache_for_dialog(app, dialog) else { return Vec::new() };
    let (slots, members) = dialog.pattern_skipped();
    cache
        .markers
        .iter()
        .zip(&cache.labels)
        .enumerate()
        .filter_map(|(index, (marker, label))| {
            let can_restore = cache.unavailable_members == 0 || skipped_slot(marker, slots).is_none();
            is_skipped(marker, slots, members).then_some((index + 2, label.as_str(), can_restore))
        })
        .collect()
}

pub(crate) fn whole_choices(app: &SolveApp, dialog: &Dialog) -> Vec<(PatternAddress, usize)> {
    let Some(cache) = cache_for_dialog(app, dialog) else { return Vec::new() };
    let (groups, _) = dialog.pattern_skipped();
    cache.groups.iter().filter(|group| groups.binary_search(&group.address).is_ok()).map(|group| (group.address.clone(), group.number)).collect()
}

/// Opaque hit handles stay separate from the instance numbers shown to a person.
#[cfg(test)]
pub(crate) fn marker_label(app: &SolveApp, dialog: &Dialog, handle: usize) -> String {
    cache_for_dialog(app, dialog).and_then(|cache| handle.checked_sub(2).and_then(|index| cache.labels.get(index))).cloned().unwrap_or_default()
}

fn marker_labels(doc: &solidcraft_engine::doc::Document, params: &Value, markers: &[PatternMarker]) -> Vec<String> {
    let mut counts = std::collections::BTreeMap::<_, usize>::new();
    for marker in markers {
        *counts.entry(marker.slot).or_default() += 1;
    }
    let (values, _) = doc.param_values();
    markers
        .iter()
        .map(|marker| {
            let mut number = marker.slot.to_string();
            if let solidcraft_engine::doc::PatternAddress::Steps(steps) = &marker.member.0 {
                for step in steps.iter().skip(1) {
                    let columns = doc
                        .feature(step.0.0)
                        .and_then(|feature| match &feature.kind {
                            solidcraft_engine::doc::FeatureKind::Pattern { pattern, .. } => pattern.rectangular_columns(&values).ok(),
                            _ => None,
                        })
                        .unwrap_or(1);
                    number.push_str(&format!(".{}", step.1.saturating_mul(columns).saturating_add(step.2).saturating_add(1)));
                }
            }
            if counts.get(&marker.slot) == Some(&1) {
                return number;
            }
            let source = doc.feature(marker.member.1.0).map(|f| f.name.as_str()).or_else(|| match &marker.member.2 {
                PatternMemberKey::Stable(PatternMemberIdentity::Body { body, .. }) => Some(body.0.as_str()),
                _ => params.get("bodies")?.as_array()?.get(marker.index)?.as_str(),
            });
            source.map_or_else(|| format!("{number} [{}]", marker.index), |source| format!("{number} · {source} [{}]", marker.index))
        })
        .collect()
}

fn skipped_slot(marker: &PatternMarker, slots: &[PatternAddress]) -> Option<PatternAddress> {
    if slots.is_empty() {
        return None;
    }
    let PatternAddress::Steps(steps) = &marker.member.0 else { return None };
    let address = PatternAddress::Steps(vec![*steps.first()?]);
    slots.binary_search(&address).ok().map(|_| address)
}

fn is_skipped(marker: &PatternMarker, slots: &[PatternAddress], members: &[PatternMember]) -> bool {
    skipped_slot(marker, slots).is_some() || members.binary_search(&marker.member).is_ok()
}

pub(crate) fn paint(app: &SolveApp, painter: &egui::Painter, markers: &[(usize, Pos2)]) {
    let Some(dialog) = app.dialog.as_ref() else { return };
    let Some(cache) = cache_for_dialog(app, dialog) else { return };
    let skipped = skipped_handles(app, dialog);
    let t = Tokens::view();
    let font = FontId::monospace(t.font_small);
    for &(number, at) in markers {
        let text = number.checked_sub(2).and_then(|index| cache.labels.get(index)).cloned().unwrap_or_default();
        let label = painter.layout_no_wrap(text.clone(), font.clone(), t.nav_tip_ink);
        let label_at = at + Vec2::new(MARKER_RADIUS + t.space_4, -label.size().y * 0.5);
        let badge = Rect::from_min_size(label_at, label.size()).expand2(Vec2::new(t.space_2, 0.0));
        painter.rect_filled(badge, t.radius_sm, t.nav_tip);
        painter.galley(label_at, label, t.nav_tip_ink);
        crate::scenario::publish_handle(&format!("pattern:label:{text}"), badge.center());
    }
    // Leave the marker itself visible and clickable if a nearby marker's label overlaps it.
    for &(number, at) in markers {
        let skip = skipped.binary_search(&number).is_ok();
        let fill = if skip { t.error } else { t.agent_ok };
        painter.circle(at, MARKER_RADIUS, fill, Stroke::new(1.5, t.body_edge));
        if skip {
            painter.line_segment([at - Vec2::new(3.5, 0.0), at + Vec2::new(3.5, 0.0)], Stroke::new(1.6, t.body_edge));
        }
        if let Some(label) = number.checked_sub(2).and_then(|index| cache.labels.get(index)) {
            crate::scenario::publish_handle(&format!("pattern:instance:{label}"), at);
        }
    }
}

pub(crate) fn click(app: &mut SolveApp, markers: &[(usize, Pos2)], at: Pos2, toggle: bool) {
    let closest =
        markers.iter().filter(|(_, p)| p.distance(at) <= MARKER_RADIUS + 3.0).min_by(|(_, a), (_, b)| a.distance(at).total_cmp(&b.distance(at)));
    if let Some(&(number, _)) = closest {
        select(app, std::iter::once(number), toggle);
    }
}

pub(crate) fn window(app: &mut SolveApp, markers: &[(usize, Pos2)], region: BoxSel, toggle: bool) {
    let enclosed = markers.iter().filter_map(|&(number, p)| {
        let corners = Rect::from_center_size(p, Vec2::splat(2.0 * MARKER_RADIUS));
        region.takes(&[corners.left_top(), corners.right_top(), corners.right_bottom(), corners.left_bottom(), corners.left_top()]).then_some(number)
    });
    select(app, enclosed, toggle);
}

fn select(app: &mut SolveApp, numbers: impl Iterator<Item = usize>, toggle: bool) {
    // OK has already captured the commands while its result is computing.
    if app.applying.is_some() {
        return;
    }
    let Some(key) = app.dialog.as_ref().and_then(|dialog| key_for_dialog(app, dialog)) else { return };
    let Some(cache) = app.viewport.pattern_skip_cache.as_ref().filter(|cache| cache.key == key) else { return };
    let Some(dialog) = app.dialog.as_mut() else { return };
    apply_selection(cache, dialog, numbers, if toggle { SkipChange::Toggle } else { SkipChange::Add });
}

pub(crate) fn restore(app: &SolveApp, dialog: &mut Dialog, number: usize) {
    if app.applying.is_some() {
        return;
    }
    if let Some(cache) = cache_for_dialog(app, dialog) {
        apply_selection(cache, dialog, std::iter::once(number), SkipChange::Restore);
    }
}

pub(crate) fn restore_whole(app: &SolveApp, dialog: &mut Dialog, address: &PatternAddress) {
    if app.applying.is_some() {
        return;
    }
    let Some(cache) = cache_for_dialog(app, dialog) else { return };
    let (groups, members) = dialog.pattern_skipped();
    if !cache.groups.iter().any(|group| &group.address == address) || groups.binary_search(address).is_err() {
        return;
    }
    let remaining = groups.iter().filter(|group| *group != address).cloned().collect();
    let members = members
        .iter()
        .filter(|member| match (&member.0, address) {
            (PatternAddress::Steps(steps), PatternAddress::Steps(group)) => !steps.starts_with(group),
            _ => &member.0 != address,
        })
        .cloned()
        .collect();
    dialog.set_pattern_skipped(remaining, members);
}

enum SkipChange {
    Add,
    Toggle,
    Restore,
}

fn apply_selection(cache: &Cache, dialog: &mut Dialog, numbers: impl Iterator<Item = usize>, change: SkipChange) {
    let (slots, members) = dialog.pattern_skipped();
    let picked: Vec<_> = numbers
        .filter_map(|n| cache.markers.get(n.checked_sub(2)?))
        .filter_map(|marker| {
            let was_skipped = is_skipped(marker, slots, members);
            let skip = match change {
                SkipChange::Add => true,
                SkipChange::Toggle => !was_skipped,
                SkipChange::Restore => false,
            };
            let blocked = !skip && cache.unavailable_members > 0 && skipped_slot(marker, slots).is_some();
            (skip != was_skipped && !blocked).then_some((marker, skip))
        })
        .collect();
    if picked.is_empty() {
        return;
    }
    let expanded: BTreeSet<_> = picked.iter().filter(|(_, skip)| !*skip).filter_map(|(marker, _)| skipped_slot(marker, slots)).collect();
    let mut members: BTreeSet<_> = members.iter().cloned().collect();
    let mut remaining: BTreeSet<_> = slots.iter().cloned().collect();
    // Only an explicit partial restore turns its group into individual exclusions.
    // Untouched groups keep covering members added to their sources later.
    if !expanded.is_empty() {
        for marker in &cache.markers {
            if skipped_slot(marker, slots).is_some_and(|group| expanded.contains(&group)) {
                members.insert(marker.member.clone());
            }
        }
    }
    for group in &expanded {
        remaining.remove(group);
    }
    for (marker, skip) in picked {
        if skip {
            members.insert(marker.member.clone());
        } else {
            members.remove(&marker.member);
        }
    }
    dialog.set_pattern_skipped(remaining.into_iter().collect(), members.into_iter().collect());
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use serde_json::json;
    use solidcore_record::Vec3;

    fn pattern_dialog() -> Result<crate::scenario::Harness, Box<dyn std::error::Error>> {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Seed"}))?;
        h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Seed".into() }];
        h.app.start("solid.pattern.rectangular");
        let dialog = h.app.dialog.as_mut().ok_or("rectangular pattern dialog did not open")?;
        dialog.inputs[1].items = vec![solidcraft_engine::Sel::Axis { name: "X".into() }];
        dialog.set_pattern_skipped(
            vec![PatternAddress::Steps(vec![solidcraft_engine::doc::PatternStep(solidcraft_engine::doc::FeatureId::DANGLING, 2, 0)])],
            Vec::new(),
        );
        Ok(h)
    }

    fn hold_job(app: &mut SolveApp, key: Key) -> std::sync::mpsc::Sender<Cache> {
        let (send, result) = std::sync::mpsc::channel();
        app.viewport.pattern_marker_jobs.running = Some(Running { key, result });
        send
    }

    #[test]
    fn marker_jobs_wait_for_the_running_job_and_then_use_the_latest_inputs() {
        let mut h = pattern_dialog().unwrap();
        let original = key_for_dialog(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let send = hold_job(&mut h.app, original.clone());
        for quantity in [4, 5] {
            let Kind::PatternRect { count, .. } = &mut h.app.dialog.as_mut().unwrap().kind else { panic!("rectangular pattern") };
            *count = quantity.to_string();
            update(&mut h.app, &h.ctx);
            assert_eq!(h.app.viewport.pattern_marker_jobs.running.as_ref().unwrap().key, original);
            assert!(h.app.viewport.pattern_skip_cache.is_none());
            assert_eq!(
                h.app.dialog.as_ref().unwrap().pattern_skipped().0,
                [PatternAddress::Steps(vec![solidcraft_engine::doc::PatternStep(solidcraft_engine::doc::FeatureId::DANGLING, 2, 0)])]
            );
        }
        let latest = key_for_dialog(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_ne!(original, latest);
        assert!(send.send(Cache::failed(original, "obsolete failure".into())).is_ok());
        update(&mut h.app, &h.ctx);
        assert!(h.app.viewport.pattern_skip_cache.is_none(), "the obsolete failure cannot replace the current job");
        assert_eq!(h.app.viewport.pattern_marker_jobs.running.as_ref().unwrap().key, latest);
        h.settle();
        let cache = h.app.viewport.pattern_skip_cache.as_ref().unwrap();
        assert_eq!(cache.key, latest);
        assert!(cache.error.is_none());
        assert_eq!(cache.markers.len(), 4);
        assert_eq!(
            h.app.dialog.as_ref().unwrap().pattern_skipped().0,
            [PatternAddress::Steps(vec![solidcraft_engine::doc::PatternStep(solidcraft_engine::doc::FeatureId::DANGLING, 2, 0)])]
        );
    }

    #[test]
    fn marker_jobs_reject_changed_sessions_revisions_frames_sources_and_dialogs() {
        let mut h = pattern_dialog().unwrap();
        let original = key_for_dialog(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let mut changed = vec![original.clone(); 7];
        changed[0].session += 1;
        changed[1].revision += 1;
        changed[2].component += 1;
        changed[3].frame[0][0] += 1.0;
        changed[4].params["bodies"] = json!(["Other"]);
        changed[5].command = "solid.pattern.circular";
        changed[6].dialog = Arc::new(());
        for current in changed {
            let send = hold_job(&mut h.app, original.clone());
            assert!(send.send(Cache::failed(original.clone(), "obsolete failure".into())).is_ok());
            assert!(h.app.viewport.pattern_marker_jobs.collect(Some(&current)).is_none());
            assert!(!h.app.viewport.pattern_marker_jobs.busy());
        }
        let send = hold_job(&mut h.app, original.clone());
        h.app.dialog = None;
        update(&mut h.app, &h.ctx);
        assert!(h.app.viewport.pattern_marker_jobs.busy(), "closing the dialog retains the running worker");
        assert!(send.send(Cache::failed(original, "closed dialog failure".into())).is_ok());
        update(&mut h.app, &h.ctx);
        assert!(!h.app.viewport.pattern_marker_jobs.busy());
        assert!(h.app.viewport.pattern_skip_cache.is_none());
    }

    #[test]
    fn only_current_marker_results_can_supply_restore_targets_and_diagnostics() {
        let mut h = pattern_dialog().unwrap();
        let key = key_for_dialog(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let mut cache = compute(key.clone(), h.app.session.scratch());
        assert!(!cache.markers.is_empty());
        cache.unavailable_members = 2;
        h.app.viewport.pattern_skip_cache = Some(cache);
        let mut dialog = h.app.dialog.take().unwrap();
        assert_eq!(cache_for_dialog(&h.app, &dialog).unwrap().unavailable_members, 2);
        assert!(!skipped_handles(&h.app, &dialog).is_empty());
        let Kind::PatternRect { count, .. } = &mut dialog.kind else { panic!("rectangular pattern") };
        *count = "4".into();
        assert!(skipped_handles(&h.app, &dialog).is_empty());
        restore(&h.app, &mut dialog, 3);
        assert_eq!(
            dialog.pattern_skipped().0,
            [PatternAddress::Steps(vec![solidcraft_engine::doc::PatternStep(solidcraft_engine::doc::FeatureId::DANGLING, 2, 0)])]
        );
        h.app.viewport.pattern_skip_cache = Some(Cache::failed(key, "marker geometry failed".into()));
        assert!(cache_for_dialog(&h.app, &dialog).is_none(), "an error also belongs to its original inputs");
    }

    fn instance(number: usize) -> PatternAddress {
        PatternAddress::Steps(vec![solidcraft_engine::doc::PatternStep(solidcraft_engine::doc::FeatureId::DANGLING, number - 1, 0)])
    }

    #[test]
    fn selecting_another_member_keeps_whole_exclusions_for_later_source_additions() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        for (body, y) in [("Seed", 0), ("Extra", 10)] {
            h.app.run("solid.box", json!({"length":4,"width":4,"height":4,"corner":[0,y,0],"body_name":body})).unwrap();
        }
        let pattern = h
            .app
            .run(
                "solid.pattern.rectangular",
                json!({
                    "bodies":["Seed"],"dir1":[1,0,0],"count1":3,"spacing1":20,"skip":[2],"name":"Copies"
                }),
            )
            .unwrap()["feature"]
            .as_u64()
            .unwrap();
        h.call("ui.editFeature", json!({"feature":"Copies"}));
        h.settle();
        let at = egui::pos2(100.0, 100.0);
        click(&mut h.app, &[(3, at)], at, true);
        let dialog = h.app.dialog.as_ref().unwrap();
        assert_eq!(dialog.pattern_skipped().0, [instance(2)], "editing copy 3 keeps copy 2 excluded as a whole");
        assert_eq!(
            dialog.pattern_skipped().1,
            [PatternMember(
                instance(3),
                solidcraft_engine::doc::FeatureId::DANGLING,
                PatternMemberKey::Stable(PatternMemberIdentity::Body { body: "Seed".into(), path: Vec::new() }),
            )]
        );
        h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
        for _ in 0..500 {
            if h.app.applying.is_none() {
                break;
            }
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        h.frames(1);
        assert!(h.app.dialog.is_none());
        h.app.run("timeline.edit", json!({"feature":pattern,"set":{"bodies":["Seed","Extra"]}})).unwrap();
        let state = h.app.session.model.state();
        assert_eq!(state.bodies.len(), 3, "the two originals and only Extra's third instance remain");
        let copies: Vec<_> = state.bodies.iter().filter(|body| body.feature == pattern).collect();
        assert_eq!(copies.len(), 1);
        // Extra is a 4 mm cube at y=10. Its third instance moves 40 mm along X.
        let centre = copies[0].body.exact_bounds().unwrap().center();
        assert!(centre.dist(Vec3::new(42.0, 12.0, 2.0)) < 1e-9, "{centre:?}");
    }

    #[test]
    fn unavailable_members_require_whole_restore_and_keep_inactive_exclusions() {
        use solidcraft_engine::doc::{FeatureId, PatternStep};

        let mut h = pattern_dialog().unwrap();
        let key = key_for_dialog(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let mut cache = compute(key, h.app.session.scratch());
        cache.unavailable_members = 1;
        let nested = PatternMember(
            PatternAddress::Steps(vec![PatternStep(FeatureId::DANGLING, 1, 0), PatternStep(FeatureId(77), 1, 0)]),
            FeatureId(88),
            PatternMemberKey::Stable(PatternMemberIdentity::Single),
        );
        let other = PatternMember(instance(3), FeatureId(88), PatternMemberKey::Stable(PatternMemberIdentity::Single));
        let inactive = PatternMember(instance(9), FeatureId(99), PatternMemberKey::Stable(PatternMemberIdentity::Single));
        let mut dialog = h.app.dialog.take().unwrap();
        dialog.set_pattern_skipped(vec![instance(2), instance(9)], vec![nested.clone(), other.clone(), inactive.clone()]);
        let before = (dialog.pattern_skipped().0.to_vec(), dialog.pattern_skipped().1.to_vec());
        apply_selection(&cache, &mut dialog, std::iter::once(2), SkipChange::Toggle);
        assert_eq!(dialog.pattern_skipped().0, before.0, "a partial restore must not uncover an unidentifiable source");
        assert_eq!(dialog.pattern_skipped().1, before.1);
        h.app.viewport.pattern_skip_cache = Some(cache);
        assert!(!skipped_choices(&h.app, &dialog)[0].2, "the individual Restore button is disabled");
        restore(&h.app, &mut dialog, 2);
        assert_eq!(dialog.pattern_skipped().0, [instance(2), instance(9)]);
        assert!(dialog.pattern_skipped().1.contains(&nested));
        // A source with no marker still exposes its active whole-instance restore action.
        h.app.viewport.pattern_skip_cache.as_mut().unwrap().markers.clear();
        assert_eq!(whole_choices(&h.app, &dialog), [(instance(2), 2)]);
        restore_whole(&h.app, &mut dialog, &instance(2));
        assert_eq!(dialog.pattern_skipped().0, [instance(9)]);
        assert_eq!(dialog.pattern_skipped().1, [other, inactive]);
    }

    use crate::dialogs::Kind;
}
