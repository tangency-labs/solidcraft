//! Application preferences around the existing bounded layout catalogue.
//! The runtime owns live selection; this record is only restart/rollback data.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcraft_engine::panels::*;

use super::*;

pub const PREFERENCES_KEY: &str = "panel_layout";
const RECORD_VERSION: u32 = 1;
const MAX_REJECTED_RECORD_BYTES: usize = 4 * commands::MAX_CATALOGUE_SNAPSHOT_BYTES + 4096;
const MAX_RECORD_BYTES: usize = 2 * MAX_REJECTED_RECORD_BYTES;
const MAX_PROFILE_LAYOUT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelLayoutPreferences {
    pub(super) record: Option<Value>,
    legacy: Option<LegacyPanelPlacements>,
    notice_reported: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreferenceRecord {
    pub version: u32,
    pub selected: WorkspaceId,
    #[serde(with = "catalogue_bytes")]
    pub catalogue: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy: Option<LegacyPanelPlacements>,
    /// Stored layouts that could not be restored, kept verbatim so a save never
    /// deletes them. Newest last; bounded by `MAX_REJECTED_RECORD_BYTES` in total.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unread: Vec<UnreadEntry>,
}

/// One stored layout that could not be restored, byte for byte.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UnreadEntry {
    /// `builtin`, `user`, `remembered` or `native`.
    pub kind: String,
    pub name: String,
    #[serde(with = "catalogue_bytes")]
    pub record: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
}

impl UnreadEntry {
    fn new(kind: &str, name: &str, record: &[u8], recovery: Option<&[u8]>) -> Self {
        Self { kind: kind.into(), name: name.into(), record: record.to_vec(), recovery: recovery.map(solidcraft_engine::doc::canvas::base64_encode) }
    }
    pub(super) fn from_raw(kind: &str, name: &str, raw: &commands::RawRecord) -> Self {
        Self::new(kind, name, &raw.bytes, raw.rejected.as_deref())
    }
    pub(super) fn native(workspace: &WorkspaceId, bytes: &[u8]) -> Self {
        Self::new("native", &workspace.0, bytes, None)
    }
    fn size(&self) -> usize {
        self.kind.len() + self.name.len() + self.record.len().div_ceil(3) * 4 + self.recovery.as_ref().map_or(0, String::len)
    }
}

/// Appends entries that are not already kept, newest last, and drops the oldest
/// when the total would pass the cap. Returns the names of newly kept entries and
/// how many entries could not be kept.
pub(super) fn merge_unread(kept: &mut Vec<UnreadEntry>, incoming: Vec<UnreadEntry>) -> (Vec<String>, usize) {
    let mut added = Vec::new();
    for entry in incoming {
        if kept.contains(&entry) {
            continue;
        }
        added.push(entry.name.clone());
        kept.push(entry);
    }
    let mut dropped = 0;
    while kept.iter().map(UnreadEntry::size).sum::<usize>() > MAX_REJECTED_RECORD_BYTES && !kept.is_empty() {
        kept.remove(0);
        dropped += 1;
    }
    (added, dropped)
}

fn dropped_note(dropped: usize) -> Option<String> {
    (dropped > 0).then(|| format!("{dropped} older kept"))
}

mod catalogue_bytes {
    use serde::Deserialize as _;
    use serde::de::Error as _;
    use solidcraft_engine::doc::canvas::{base64_decode, base64_encode};

    pub(super) const MAX_PACKED_BYTES: usize = super::commands::MAX_CATALOGUE_SNAPSHOT_BYTES.div_ceil(3) * 4;

    pub fn serialize<S: serde::Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64_encode(bytes))
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let packed = String::deserialize(deserializer)?;
        if packed.len() > MAX_PACKED_BYTES {
            return Err(D::Error::custom("panel catalogue packed byte cap"));
        }
        let bytes = base64_decode(&packed).ok_or_else(|| D::Error::custom("invalid packed panel catalogue"))?;
        if bytes.len() > super::commands::MAX_CATALOGUE_SNAPSHOT_BYTES || base64_encode(&bytes) != packed {
            return Err(D::Error::custom("invalid or oversized packed panel catalogue"));
        }
        Ok(bytes)
    }
}

pub(super) struct PreferenceSession {
    pub pending: Option<PreferenceRecord>,
    pub initial: PreferenceRecord,
    pub rejected: Option<Value>,
    pub failed: bool,
    pub legacy: Option<LegacyPanelPlacements>,
    pub failed_environment: Option<PreferenceEnvironment>,
    /// Entries reset at startup or first paint, for the precise notice.
    pub reset: Vec<String>,
    pub unread: Vec<UnreadEntry>,
}

pub(super) type PreferenceEnvironment = (LogicalRect, AllocationEnvironment, u64, RuntimeEnvironment, Vec<LogicalRect>, HostCapabilities);

/// Captured once when Preferences opens. Catalogue records share their existing
/// Arc payloads, so checking Apply never exports a catalogue or creates JSON.
#[derive(Clone)]
pub(crate) struct PanelLayoutSnapshot {
    catalogue: Option<commands::CatalogueState>,
    selected: Option<WorkspaceId>,
    pending: Option<PreferenceRecord>,
    rejected: Option<Value>,
    legacy: Option<LegacyPanelPlacements>,
    deferred: Option<Value>,
}

pub(crate) fn panel_layout_snapshot(app: &crate::SolveApp) -> PanelLayoutSnapshot {
    match app.panel_runtime.as_ref().and_then(|runtime| runtime.preferences.as_ref().map(|state| (runtime, state))) {
        Some((runtime, state)) => PanelLayoutSnapshot {
            catalogue: Some(runtime.preference_catalogue_state().clone()),
            selected: Some(runtime.runtime().layout().layout().workspace.clone()),
            pending: state.pending.clone(),
            rejected: state.rejected.clone(),
            legacy: state.legacy.clone(),
            deferred: None,
        },
        None => PanelLayoutSnapshot {
            catalogue: None,
            selected: None,
            pending: None,
            rejected: None,
            legacy: app.panel_layout_preferences.legacy.clone(),
            deferred: app.panel_layout_preferences.record.clone(),
        },
    }
}

pub(crate) fn panel_layout_matches(app: &crate::SolveApp, snapshot: &PanelLayoutSnapshot) -> bool {
    let live = app.panel_runtime.as_ref().and_then(|runtime| runtime.preferences.as_ref().map(|state| (runtime, state)));
    match (&snapshot.catalogue, live) {
        (None, None) => snapshot.deferred == app.panel_layout_preferences.record && snapshot.legacy == app.panel_layout_preferences.legacy,
        (Some(catalogue), Some((runtime, state))) => {
            catalogue == runtime.preference_catalogue_state()
                && snapshot.selected.as_ref() == Some(&runtime.runtime().layout().layout().workspace)
                && snapshot.pending == state.pending
                && snapshot.rejected == state.rejected
                && snapshot.legacy == state.legacy
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyPanelRole {
    Browser,
    History,
    Assistant,
    RenderSceneBrowser,
}

/// Only placement inputs are retained. Tree, chat and tool content stay owned by
/// their existing preferences. Explicit bindings avoid guessed product kind IDs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyPanelPlacements {
    browser_width: f32,
    browser_collapsed: bool,
    history_left: bool,
    history_under_browser: bool,
    history_width: f32,
    history_collapsed: bool,
    browser_share: f32,
    assistant_side: String,
    assistant_width: f32,
    assistant_collapsed: bool,
    assistant_popped: bool,
    render_side: String,
    render_width: f32,
    #[serde(default)]
    imported: BTreeSet<LegacyPanelRole>,
}
impl LegacyPanelPlacements {
    fn capture(app: &crate::SolveApp) -> Self {
        let prefs = &app.preferences;
        let vertical = app.ui.show_timeline && prefs.timeline_left;
        let stacked = vertical && prefs.timeline_under_browser;
        // Repair only the import snapshot; Classic keeps its saved preferences.
        let defaults = crate::prefs::Prefs::default();
        let width = |value: f32, fallback: f32| {
            if value.is_finite() && value > 0.0 && value <= MAX_LOGICAL { value } else { fallback }
        };
        Self {
            browser_width: width(prefs.browser_width, defaults.browser_width),
            browser_collapsed: if vertical { prefs.timeline_left_browser_collapsed } else { prefs.browser_collapsed },
            history_left: prefs.timeline_left,
            history_under_browser: stacked,
            history_width: width(prefs.timeline_left_width, defaults.timeline_left_width),
            history_collapsed: prefs.timeline_collapsed,
            browser_share: if prefs.browser_share.is_finite() && (0.0..=1.0).contains(&prefs.browser_share) {
                prefs.browser_share
            } else {
                defaults.browser_share
            },
            assistant_side: if prefs.agent_chat_side == "left" { "left" } else { "right" }.into(),
            assistant_width: width(prefs.agent_chat_width, defaults.agent_chat_width),
            assistant_collapsed: prefs.agent_chat_minimized,
            assistant_popped: prefs.agent_chat_popped,
            render_side: if prefs.render_browser_side == "right" { "right" } else { "left" }.into(),
            render_width: width(prefs.render_browser_width, defaults.render_browser_width),
            imported: BTreeSet::new(),
        }
    }
    fn validate(&self) -> PanelResult<()> {
        if !matches!(self.assistant_side.as_str(), "left" | "right")
            || !matches!(self.render_side.as_str(), "left" | "right")
            || [self.browser_width, self.history_width, self.assistant_width, self.render_width]
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0 || *value > MAX_LOGICAL)
            || !self.browser_share.is_finite()
            || !(0.0..=1.0).contains(&self.browser_share)
        {
            return Err(error(Rule::Bounds, "invalid retained legacy panel placement"));
        }
        Ok(())
    }
    fn placement(&self, role: LegacyPanelRole) -> Option<(Zone, f32, bool)> {
        Some(match role {
            LegacyPanelRole::Browser => (Zone::Left, self.browser_width, self.browser_collapsed),
            LegacyPanelRole::History => (if self.history_left { Zone::Left } else { Zone::Bottom }, self.history_width, self.history_collapsed),
            LegacyPanelRole::Assistant if self.assistant_popped => return None,
            LegacyPanelRole::Assistant => {
                (if self.assistant_side == "left" { Zone::Left } else { Zone::Right }, self.assistant_width, self.assistant_collapsed)
            }
            LegacyPanelRole::RenderSceneBrowser => (if self.render_side == "right" { Zone::Right } else { Zone::Left }, self.render_width, false),
        })
    }
}

pub struct LegacyPanelBinding {
    pub role: LegacyPanelRole,
    pub instance: InstanceId,
}

/// Staging checks the exact existing serializer before any live layout commits.
/// The outer preferences writer still owns disk/localStorage publication.
pub(super) struct PreferenceCatalogue(commands::CatalogueState);
impl commands::LayoutCatalogue for PreferenceCatalogue {
    fn state(&self) -> &commands::CatalogueState {
        &self.0
    }
    fn prepare(&mut self, next: commands::CatalogueState) -> PanelResult<commands::PreparedCatalogue> {
        next.export()?;
        commands::PreparedCatalogue::new(next)
    }
    fn publish(&mut self, prepared: commands::PreparedCatalogue) {
        self.0 = prepared.into_state();
    }
}

struct CountBytes(usize);
impl Write for CountBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > MAX_RECORD_BYTES {
            return Err(io::Error::other("panel preference record byte cap"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn bounded_record(value: &Value) -> PanelResult<()> {
    serde_json::to_writer(CountBytes(0), value).map_err(|_| error(Rule::CollectionLimit, "panel preference record byte cap"))
}
impl PreferenceRecord {
    pub(super) fn read(value: &Value, registry: &PanelRegistry) -> PanelResult<Self> {
        Ok(Self::read_with(value, registry, None)?.0)
    }
    /// Per-entry restore: a stored layout that cannot be read is reset to the
    /// matching entry of `defaults` (or dropped, for a user preset) and named in
    /// the returned list; the rest is kept. Nothing is written until the next
    /// save. A record that is unreadable as a whole still fails.
    pub(super) fn read_repairing(value: &Value, registry: &PanelRegistry, defaults: &commands::CatalogueState) -> PanelResult<(Self, Vec<String>)> {
        Self::read_with(value, registry, Some(defaults))
    }
    fn read_with(value: &Value, registry: &PanelRegistry, defaults: Option<&commands::CatalogueState>) -> PanelResult<(Self, Vec<String>)> {
        bounded_record(value)?;
        if value.get("version").and_then(Value::as_u64) != Some(u64::from(RECORD_VERSION)) {
            return Err(error(Rule::Version, "unsupported application panel preferences"));
        }
        let packed = value.get("catalogue").and_then(Value::as_str).ok_or_else(|| error(Rule::Schema, "packed panel catalogue required"))?;
        if packed.len() > catalogue_bytes::MAX_PACKED_BYTES {
            return Err(error(Rule::CollectionLimit, "panel catalogue snapshot byte cap"));
        }
        let mut record: Self = serde_json::from_value(value.clone()).map_err(|_| error(Rule::Schema, "invalid application panel preferences"))?;
        // Explicit JSON null is rejected source data, not an absent sidecar.
        record.rejected = value.get("rejected").cloned();
        if let Some(legacy) = &record.legacy {
            legacy.validate()?;
        }
        if let Some(rejected) = &record.rejected {
            let mut counter = CountBytes(0);
            serde_json::to_writer(&mut counter, rejected).map_err(|_| error(Rule::CollectionLimit, "rejected panel preference byte cap"))?;
            if counter.0 > MAX_REJECTED_RECORD_BYTES {
                return Err(error(Rule::CollectionLimit, "rejected panel preference byte cap"));
            }
        }
        let mut reset = Vec::new();
        let mut state = match defaults {
            Some(defaults) => {
                let (state, skipped) = commands::CatalogueState::hydrate_lenient(&record.catalogue, registry, defaults)?;
                let mut entries = Vec::new();
                for entry in skipped {
                    match entry {
                        commands::SkippedEntry::Builtin(name, raw) => entries.push(UnreadEntry::from_raw("builtin", &name, &raw)),
                        commands::SkippedEntry::User(name, raw) => entries.push(UnreadEntry::from_raw("user", &name, &raw)),
                        commands::SkippedEntry::Remembered(workspace, raw) => entries.push(UnreadEntry::from_raw("remembered", &workspace.0, &raw)),
                    }
                }
                if !entries.is_empty() {
                    let (added, dropped) = merge_unread(&mut record.unread, entries);
                    reset = added;
                    reset.extend(dropped_note(dropped));
                }
                state
            }
            None => commands::CatalogueState::hydrate(&record.catalogue, registry)?,
        };
        if defaults.is_some() && state.remembered(&record.selected).is_none() && state.summaries().iter().any(|item| item.id == record.selected) {
            // The selected workspace's saved layout was unreadable: its registered default stands in.
            state.reset_remembered_to_default(&record.selected, registry)?;
            if !reset.contains(&record.selected.0) {
                reset.push(record.selected.0.clone());
            }
        }
        if !state.summaries().iter().any(|item| item.id == record.selected) || state.remembered(&record.selected).is_none() {
            return Err(error(Rule::MissingTarget, "saved selected workspace has no remembered layout"));
        }
        if defaults.is_some() && (!reset.is_empty() || !record.unread.is_empty()) {
            record.catalogue = state.export()?;
        }
        Ok((record, reset))
    }
    pub(super) fn catalogue(&self, registry: &PanelRegistry) -> PanelResult<commands::CatalogueState> {
        commands::CatalogueState::hydrate(&self.catalogue, registry)
    }
    pub(super) fn value(&self) -> Value {
        let packed = solidcraft_engine::doc::canvas::base64_encode(&self.catalogue);
        let mut value = serde_json::json!({"version":self.version,"selected":self.selected,"catalogue":packed,"legacy":self.legacy});
        if !self.unread.is_empty()
            && let Some(object) = value.as_object_mut()
        {
            object.insert("unread".into(), serde_json::json!(self.unread));
        }
        if let Some(rejected) = &self.rejected
            && let Some(object) = value.as_object_mut()
        {
            object.insert("rejected".into(), rejected.clone());
        }
        value
    }
}

pub fn preferences_enabled(app: &crate::SolveApp) -> bool {
    app.panel_runtime.as_ref().is_some_and(|runtime| runtime.preferences.is_some())
}

pub fn preferences_value(app: &crate::SolveApp) -> Option<Value> {
    if let Some(runtime) = app.panel_runtime.as_ref().filter(|runtime| runtime.preferences.is_some()) {
        return runtime.preference_record().ok().map(|record| record.value());
    }
    app.panel_layout_preferences.record.clone()
}

/// Disk storage is portable; the exact in-process value remains the Cancel baseline.
pub fn persistent_preferences_value(app: &crate::SolveApp) -> PanelResult<Option<Value>> {
    portable_preferences_value(app, preferences_value(app))
}

/// Also used for the held Preferences baseline. Unreadable recovery stays exact.
pub fn portable_preferences_value(app: &crate::SolveApp, value: Option<Value>) -> PanelResult<Option<Value>> {
    let Some(value) = value else { return Ok(None) };
    bounded_record(&value)?;
    let empty = PanelRegistry::default();
    let registry = app.panel_runtime.as_ref().map_or(&empty, |runtime| runtime.runtime().layout().registry());
    let Ok(mut record) = PreferenceRecord::read(&value, registry) else { return Ok(Some(value)) };
    record.catalogue =
        record.catalogue(registry)?.map_envelopes(registry, |bytes, envelope| portable_envelope(bytes, envelope, registry))?.export()?;
    Ok(Some(record.value()))
}

fn portable_envelope(bytes: &[u8], envelope: LayoutEnvelope, registry: &PanelRegistry) -> PanelResult<LayoutEnvelope> {
    let native = envelope.layout.sections.iter().any(|section| section.location.zone() == Zone::Window)
        || envelope.layout.instances.iter().any(|instance| instance.memory.location.zone() == Zone::Window);
    if !native {
        return Ok(envelope);
    }
    // Current registered placements are portable. Opaque extension/recovery
    // data keeps its existing lossless representation until its owner returns.
    let projected = project_retained_record(bytes, registry, &envelope.layout.workspace)?;
    let layout = portable_profile_layout(&projected.layout, registry)?;
    with_live_layout(&projected, layout, registry)
}

/// A saved record for a host without native windows: every Window placement
/// falls back to its portable one, and each moved panel is named.
pub(super) fn redock_native_record(
    record: &PreferenceRecord,
    registry: &PanelRegistry,
) -> PanelResult<(PreferenceRecord, Vec<String>, Vec<UnreadEntry>)> {
    let mut names = Vec::new();
    let mut originals = Vec::new();
    let mut next = record.clone();
    next.catalogue = record
        .catalogue(registry)?
        .map_envelopes(registry, |bytes, envelope| {
            let before = names.len();
            for section in envelope.layout.sections.iter().filter(|section| section.location.zone() == Zone::Window) {
                names.extend(section.tabs.iter().map(|tab| format!("{} (Window)", tab.0)));
            }
            if names.len() > before {
                originals.push(UnreadEntry::native(&envelope.layout.workspace, bytes));
            }
            portable_envelope(bytes, envelope, registry)
        })?
        .export()?;
    Ok((next, names, originals))
}

/// Capture before the generic preferences reader can discard an unreadable value.
/// A present but corrupt/future record never falls through to legacy import.
pub fn load_preferences(app: &mut crate::SolveApp, saved: &Value) {
    if saved.get(PREFERENCES_KEY).is_some_and(|value| bounded_record(value).is_err()) {
        if !preferences_enabled(app) {
            app.panel_layout_preferences.record = Some(Value::Null);
            app.panel_layout_preferences.legacy = None;
        }
        report_notice(app);
        return;
    }
    let record = saved.get(PREFERENCES_KEY).cloned();
    if preferences_enabled(app) {
        let production = app.panel_runtime.as_ref().is_some_and(|runtime| runtime.production.is_some());
        if (record.is_some() || !production) && restore_preferences(app, record).is_err() {
            report_notice(app);
        }
        return;
    }
    app.panel_layout_preferences = PanelLayoutPreferences {
        record,
        legacy: (!saved.as_object().is_some_and(|object| object.contains_key(PREFERENCES_KEY))).then(|| LegacyPanelPlacements::capture(app)),
        notice_reported: false,
    };
}

pub(super) fn report_notice(app: &mut crate::SolveApp) {
    if !app.panel_layout_preferences.notice_reported {
        app.panel_layout_preferences.notice_reported = true;
        let reset = app.panel_runtime.as_ref().and_then(|runtime| runtime.preferences.as_ref()).map(|state| state.reset.clone()).unwrap_or_default();
        let text =
            if reset.is_empty() || app.panel_runtime.as_ref().and_then(|runtime| runtime.preferences.as_ref()).is_some_and(|state| state.failed) {
                app.tr("prefs.panel_layout.recovery").to_string()
            } else {
                reset_notice(&reset)
            };
        app.set_status(text, true);
    }
}

/// Names what was reset, at most five, then a count of the rest.
fn reset_notice(reset: &[String]) -> String {
    const SHOWN: usize = 5;
    let mut names = reset.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
    if reset.len() > SHOWN {
        names.push_str(&format!(", +{}", reset.len() - SHOWN));
    }
    solidcraft_ui_text::format("prefs.panel_layout.reset", &[("count", &reset.len().to_string()), ("names", &names)])
}

/// Opt in an already registered runtime. Production registrations supply their
/// own presets with `enable_preferences_with_catalogue`; specimens stay explicit.
/// Declare `HostCapabilities::persistence` when constructing its LayoutService
/// so discovery describes the caller-supplied preference storage truthfully.
pub fn enable_preferences(app: &mut crate::SolveApp) -> PanelResult<()> {
    let runtime = app.panel_runtime.as_ref().ok_or_else(|| error(Rule::Capability, "panel runtime is not enabled"))?;
    let layout = runtime.runtime().layout().layout();
    let registry = runtime.runtime().layout().registry();
    let mut catalogue = commands::InMemoryCatalogue::default();
    catalogue.register_builtin("Default", &LayoutEnvelope::new(layout.clone()), registry)?;
    catalogue.register_workspace(commands::WorkspaceDescriptor {
        id: layout.workspace.clone(),
        title_key: "ui.panel_layout.current".into(),
        default_preset: "Default".into(),
    })?;
    enable_preferences_with_catalogue(app, catalogue)
}

/// Install caller-registered defaults on a runtime whose LayoutService declares
/// `HostCapabilities::persistence`; the native adapter does not own storage.
pub fn enable_preferences_with_catalogue(app: &mut crate::SolveApp, mut defaults: commands::InMemoryCatalogue) -> PanelResult<()> {
    let integration = app.panel_runtime.as_mut().ok_or_else(|| error(Rule::Capability, "panel runtime is not enabled"))?;
    let repaired = configure_preferences(integration, &app.panel_layout_preferences, &mut defaults)?;
    if repaired {
        report_notice(app);
    }
    Ok(())
}

pub(super) fn configure_preferences(
    integration: &mut AppPanelRuntime,
    preferences: &PanelLayoutPreferences,
    defaults: &mut commands::InMemoryCatalogue,
) -> PanelResult<bool> {
    use commands::LayoutCatalogue as _;
    if integration.preferences.is_some() {
        return Err(error(Rule::Lifecycle, "panel preferences are already enabled"));
    }
    let layout = integration.runtime().layout().layout().clone();
    let registry = integration.runtime().layout().registry();
    defaults.remember(&LayoutEnvelope::new(layout.clone()), None, registry)?;
    let legacy = preferences.legacy.clone();
    if let Some(legacy) = &legacy {
        legacy.validate()?;
    }
    let initial = PreferenceRecord {
        version: RECORD_VERSION,
        selected: layout.workspace,
        catalogue: defaults.state().export()?,
        rejected: None,
        legacy: legacy.clone(),
        unread: Vec::new(),
    };
    let mut pending = None;
    let mut rejected = None;
    let mut reset = Vec::new();
    let mut unread = Vec::new();
    if let Some(raw) = &preferences.record {
        match PreferenceRecord::read_repairing(raw, registry, defaults.state()) {
            Ok((record, names)) => {
                reset = names;
                unread = record.unread.clone();
                pending = Some(record);
            }
            Err(_) => {
                let mut counter = CountBytes(0);
                if serde_json::to_writer(&mut counter, raw).is_ok() && counter.0 <= MAX_REJECTED_RECORD_BYTES {
                    rejected = Some(raw.clone());
                } else {
                    rejected = Some(Value::Null);
                }
            }
        }
    }
    integration.install_preference_catalogue(
        PreferenceCatalogue(defaults.state().clone()),
        PreferenceSession { pending, initial, rejected, failed: false, legacy, failed_environment: None, reset, unread },
    )?;
    Ok(integration.preferences.as_ref().is_some_and(|state| state.rejected.is_some() || !state.reset.is_empty()))
}

/// Preferences Cancel uses the same admitted retained-controller transaction as
/// commands. Failure leaves both the current catalogue and record untouched.
pub fn restore_preferences(app: &mut crate::SolveApp, value: Option<Value>) -> PanelResult<()> {
    let settled =
        app.panel_runtime.as_ref().and_then(|runtime| runtime.preferences.as_ref()).is_none_or(|state| state.pending.is_none() && !state.failed);
    if settled && preferences_value(app) == value {
        return Ok(());
    }
    if let Some(raw) = &value {
        bounded_record(raw)?;
    }
    if let Some(integration) = app.panel_runtime.as_mut().filter(|runtime| runtime.preferences.is_some()) {
        let record = match &value {
            Some(raw) => {
                let (record, reset) =
                    PreferenceRecord::read_repairing(raw, integration.runtime().layout().registry(), integration.preference_catalogue_state())?;
                if let Some(state) = &mut integration.preferences {
                    state.reset = reset;
                }
                record
            }
            None => integration.preferences.as_ref().ok_or_else(|| error(Rule::Lifecycle, "panel preferences disappeared"))?.initial.clone(),
        };
        integration.restore_preference_record(&app.session, record)?;
        if value.is_none() && integration.production.is_none() {
            integration.preferences = None;
        }
    }
    app.panel_layout_preferences.record = value;
    app.panel_layout_preferences.legacy = None;
    Ok(())
}

pub fn preset_names(app: &crate::SolveApp) -> Vec<String> {
    app.panel_runtime.as_ref().filter(|runtime| runtime.preferences.is_some()).map_or_else(Vec::new, |runtime| runtime.preference_preset_names())
}

pub fn user_preset_names(app: &crate::SolveApp) -> Vec<String> {
    app.panel_runtime.as_ref().filter(|runtime| runtime.preferences.is_some()).map_or_else(Vec::new, |runtime| runtime.preference_user_preset_names())
}

#[derive(Clone)]
pub struct LayoutPreferenceWorkspace {
    pub id: WorkspaceId,
    pub title_key: String,
}

#[derive(Clone)]
pub struct LayoutPreferenceEntry {
    pub name: String,
    pub user: bool,
    pub layout: Layout,
    pub matches: bool,
    pub selected: bool,
}

#[derive(Clone)]
pub struct LayoutPreferencesView {
    pub entries: Vec<LayoutPreferenceEntry>,
    pub builtin_names: Vec<String>,
    pub workspaces: Vec<LayoutPreferenceWorkspace>,
    pub selected_workspace: WorkspaceId,
    pub default_matches: bool,
    pub current_layout: Layout,
}

/// Placements for settings tiles; no controller, allocator or command runs here.
pub fn layout_preferences_view(app: &crate::SolveApp) -> PanelResult<LayoutPreferencesView> {
    let runtime = app
        .panel_runtime
        .as_ref()
        .filter(|runtime| runtime.preferences.is_some())
        .ok_or_else(|| error(Rule::Capability, "panel preferences are not enabled"))?;
    let current_layout = runtime.runtime().layout().layout().clone();
    let registry = runtime.runtime().layout().registry();
    let catalogue = runtime.preference_catalogue_state();
    let summaries = catalogue.summaries();
    let default = summaries
        .iter()
        .find(|workspace| workspace.id == current_layout.workspace)
        .ok_or_else(|| error(Rule::MissingTarget, "current layout workspace has no default"))?;
    let mut entries = Vec::new();
    let mut matched = false;
    let mut default_matches = false;
    for (name, stored, user) in catalogue
        .builtin_presets()
        .map(|(name, record)| (name, record, false))
        .chain(catalogue.user_presets().map(|(name, record)| (name, record, true)))
    {
        let raw = decode_retained_record(stored.bytes(), registry)?;
        if !user && raw.layout.workspace != current_layout.workspace && name != default.default_preset {
            continue;
        }
        let mut envelope = stored.project_envelope(registry, &raw.layout.workspace)?;
        restore_quarantine(&mut envelope, registry)?;
        envelope.layout.workspace = current_layout.workspace.clone();
        let exact = arrangements_equal(&envelope.layout, &current_layout);
        if !user && name == default.default_preset {
            default_matches = exact;
        }
        let selected = exact && !matched;
        matched |= selected;
        entries.push(LayoutPreferenceEntry { name: name.into(), user, layout: envelope.layout, matches: exact, selected });
    }
    Ok(LayoutPreferencesView {
        entries,
        builtin_names: catalogue.builtin_presets().map(|(name, _)| name.to_owned()).collect(),
        workspaces: summaries.into_iter().map(|workspace| LayoutPreferenceWorkspace { id: workspace.id, title_key: workspace.title_key }).collect(),
        selected_workspace: current_layout.workspace.clone(),
        default_matches,
        current_layout,
    })
}

/// Product adapters explicitly identify their corresponding legacy placements.
/// Unmapped roles survive fixture use and later restarts; only a successful
/// admitted transaction marks a role imported. No host or content is registered.
pub fn migrate_legacy_placements(app: &mut crate::SolveApp, bindings: &[LegacyPanelBinding]) -> PanelResult<()> {
    if bindings.len() > 4 {
        return Err(error(Rule::CollectionLimit, "legacy panel mapping count cap"));
    }
    let integration = app.panel_runtime.as_mut().ok_or_else(|| error(Rule::Capability, "panel runtime is not enabled"))?;
    let state = integration.preferences.as_ref().ok_or_else(|| error(Rule::Capability, "panel preferences are not enabled"))?;
    if state.pending.is_some() {
        return Err(error(Rule::Availability, "restore the saved panel layout before legacy mapping"));
    }
    let Some(mut legacy) = state.legacy.clone() else { return Ok(()) };
    let layout = integration.runtime().layout().layout();
    let registry = integration.runtime().layout().registry();
    let mut roles = BTreeSet::new();
    let mut instances = BTreeSet::new();
    let mut sections = integration.preference_identity_reservations()?;
    let mut mutations = Vec::new();
    let mut widths = Vec::new();
    let mut ordered: Vec<_> = bindings.iter().collect();
    ordered.sort_by_key(|binding| binding.role);
    let mut browser_section = bindings
        .iter()
        .find(|binding| binding.role == LegacyPanelRole::Browser)
        .and_then(|binding| layout.containing(&binding.instance).ok())
        .map(|section| section.id.clone());
    for binding in ordered {
        if !roles.insert(binding.role) || !instances.insert(binding.instance.clone()) {
            return Err(error(Rule::DuplicateId, "duplicate legacy panel mapping"));
        }
        if legacy.imported.contains(&binding.role) {
            continue;
        }
        let Some((zone, width, collapsed)) = legacy.placement(binding.role) else { continue };
        let instance = layout.instance(&binding.instance)?;
        let existing = layout.containing(&binding.instance)?;
        let spec = registry.get(&instance.kind)?;
        if !existing.location.zone().docked() || existing.tabs.len() != 1 {
            return Err(error(Rule::Capability, "legacy mapping requires one currently docked panel"));
        }
        if !width.is_finite() || !legacy.browser_share.is_finite() {
            return Err(error(Rule::Bounds, "legacy panel size must be finite"));
        }
        let mut size = existing.size;
        if zone.side() {
            size.width = width.clamp(spec.min_size.width, spec.max_size.width);
            if binding.role != LegacyPanelRole::History || !legacy.history_under_browser {
                widths.push((binding.instance.clone(), size.width));
            }
        }
        let section = next_section_id_with_reservations(layout, &sections)?;
        sections.insert(section.clone());
        let destination = if binding.role == LegacyPanelRole::History && legacy.history_under_browser {
            if let Some(anchor) = &browser_section {
                Destination::Relative {
                    section: anchor.clone(),
                    placement: RelativePlacement::Below,
                    new_section: section.clone(),
                    new_column: None,
                    size,
                }
            } else {
                return Err(error(Rule::MissingTarget, "shared-column legacy History requires a Browser mapping"));
            }
        } else {
            Destination::NewSection { section: section.clone(), index: 0, location: SectionLocation::Dock { zone }, size }
        };
        mutations.push(Mutation::Move { instance: binding.instance.clone(), destination });
        if binding.role == LegacyPanelRole::Browser {
            browser_section = Some(section.clone());
        }
        if collapsed {
            mutations.push(Mutation::Collapse { section, collapsed });
        }
        legacy.imported.insert(binding.role);
    }
    if mutations.is_empty() {
        return Ok(());
    }
    let weights = if legacy.history_under_browser {
        let share = legacy.browser_share.clamp(0.01, 0.99);
        bindings
            .iter()
            .filter_map(|binding| match binding.role {
                LegacyPanelRole::Browser => Some((binding.instance.clone(), share)),
                LegacyPanelRole::History => Some((binding.instance.clone(), 1.0 - share)),
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };
    integration.apply_legacy_preferences(&app.session, mutations, weights, widths, legacy)
}

/// Profiles contain the selected workspace and every workspace's current known
/// arrangement, without the named library or physical native window geometry.
pub fn profile_value(app: &crate::SolveApp) -> Option<Value> {
    let runtime = app.panel_runtime.as_ref().filter(|runtime| runtime.preferences.is_some())?;
    match capture_workspace_profiles(runtime) {
        Ok(value) => Some(value),
        // An enabled but unrepresentable layout is refused by profile sanitation,
        // rather than silently making a profile with missing workspaces.
        Err(_) => Some(Value::Null),
    }
}

fn capture_workspace_profiles(runtime: &AppPanelRuntime) -> PanelResult<Value> {
    let registry = runtime.runtime().layout().registry();
    let current = runtime.runtime().layout().layout();
    let catalogue = runtime.preference_catalogue_state();
    let mut workspaces = BTreeMap::new();
    for workspace in catalogue.summaries() {
        let source = if workspace.id == current.workspace {
            runtime.current_preference_envelope()?
        } else {
            let stored = catalogue.remembered(&workspace.id).map(Ok).unwrap_or_else(|| catalogue.preset(&workspace.default_preset))?;
            let raw = decode_retained_record(stored.bytes(), registry)?;
            let mut projected = stored.project_envelope(registry, &raw.layout.workspace)?;
            projected.layout.workspace = workspace.id.clone();
            projected
        };
        if !source.quarantine.is_empty() || !source.retained_data.is_empty() || !source.section_snapshots.is_empty() || source.recovery_wire.is_some()
        {
            return Err(error(Rule::Capability, "profile workspace contains extension recovery data; keep its arrangement in preferences"));
        }
        let portable = portable_profile_layout(&source.layout, registry)?;
        let envelope = with_live_layout(&source, portable, registry)?;
        let bytes = encode(&envelope, registry)?;
        let text = String::from_utf8(bytes).map_err(|_| error(Rule::Schema, "profile layout is not UTF-8"))?;
        workspaces.insert(workspace.id, text);
    }
    let value = serde_json::json!({"version":2,"selected":current.workspace,"workspaces":workspaces});
    validate_profile_value(&value)?;
    Ok(value)
}

pub(super) struct ProfileLayouts {
    pub selected: Option<WorkspaceId>,
    pub workspaces: BTreeMap<WorkspaceId, LayoutEnvelope>,
}

fn checked_profile_envelope(text: &str) -> PanelResult<LayoutEnvelope> {
    if text.len() > MAX_PROFILE_LAYOUT_BYTES {
        return Err(error(Rule::CollectionLimit, "profile panel layout byte cap"));
    }
    let envelope = decode_retained_record(text.as_bytes(), &PanelRegistry::default())?;
    if envelope.layout.sections.iter().any(|section| section.location.zone() == Zone::Window)
        || envelope.layout.instances.iter().any(|instance| instance.memory.location.zone() == Zone::Window)
        || !envelope.quarantine.is_empty()
        || !envelope.retained_data.is_empty()
        || !envelope.section_snapshots.is_empty()
        || envelope.recovery_wire.is_some()
    {
        return Err(error(Rule::Capability, "profile panel layout cannot contain native or recovery data"));
    }
    Ok(envelope)
}

fn read_profile_layouts(value: &Value) -> PanelResult<ProfileLayouts> {
    let object = value.as_object().ok_or_else(|| error(Rule::Schema, "profile panel layout must be an object"))?;
    if object.len() == 2 && object.get("version").and_then(Value::as_u64) == Some(1) {
        let text = object.get("envelope").and_then(Value::as_str).ok_or_else(|| error(Rule::Schema, "profile envelope must be text"))?;
        let envelope = checked_profile_envelope(text)?;
        return Ok(ProfileLayouts { selected: None, workspaces: [(envelope.layout.workspace.clone(), envelope)].into_iter().collect() });
    }
    if object.len() != 3 || object.get("version").and_then(Value::as_u64) != Some(2) {
        return Err(error(Rule::Version, "unsupported profile panel layout"));
    }
    let selected = object.get("selected").and_then(Value::as_str).ok_or_else(|| error(Rule::Schema, "selected profile workspace required"))?;
    valid_id(selected)?;
    let selected = WorkspaceId::new(selected)?;
    let records = object.get("workspaces").and_then(Value::as_object).ok_or_else(|| error(Rule::Schema, "profile workspaces must be an object"))?;
    if records.is_empty() || records.len() > commands::MAX_WORKSPACES {
        return Err(error(Rule::CollectionLimit, "profile workspace count cap"));
    }
    let mut bytes = 0_usize;
    let mut workspaces = BTreeMap::new();
    for (id, record) in records {
        valid_id(id)?;
        let workspace = WorkspaceId::new(id)?;
        let text = record.as_str().ok_or_else(|| error(Rule::Schema, "profile workspace envelope must be text"))?;
        bytes = bytes.saturating_add(id.len()).saturating_add(text.len());
        if bytes > MAX_PROFILE_LAYOUT_BYTES {
            return Err(error(Rule::CollectionLimit, "profile workspace aggregate byte cap"));
        }
        let envelope = checked_profile_envelope(text)?;
        if envelope.layout.workspace != workspace {
            return Err(error(Rule::Schema, "profile workspace identity mismatch"));
        }
        workspaces.insert(workspace, envelope);
    }
    if !workspaces.contains_key(&selected) {
        return Err(error(Rule::MissingTarget, "selected profile workspace has no arrangement"));
    }
    Ok(ProfileLayouts { selected: Some(selected), workspaces })
}

pub fn validate_profile_value(value: &Value) -> PanelResult<()> {
    read_profile_layouts(value).map(|_| ())
}

pub fn apply_profile_value(app: &mut crate::SolveApp, value: &Value) -> PanelResult<()> {
    let profile = read_profile_layouts(value)?;
    let integration = app
        .panel_runtime
        .as_mut()
        .filter(|runtime| runtime.preferences.is_some())
        .ok_or_else(|| error(Rule::Capability, "panel preferences are not enabled"))?;
    integration.restore_profile_layouts(&app.session, profile)
}

fn portable_profile_layout(layout: &Layout, registry: &PanelRegistry) -> PanelResult<Layout> {
    match redocked_portable_layout(layout, registry) {
        Ok(layout) => Ok(layout),
        // A saved return tab can become full or incompatible. Every registration
        // allows Floating; use logical defaults rather than leaking an OS frame.
        Err(_) => floating_portable_layout(layout, registry),
    }
}

fn floating_portable_layout(layout: &Layout, registry: &PanelRegistry) -> PanelResult<Layout> {
    let mut service = LayoutService::new(
        registry.clone(),
        layout.clone(),
        HostCapabilities { native_windows: true, persistence: false, max_native_hosts: MAX_SECTIONS },
    )?;
    let mut portable = layout.clone();
    for (index, section) in portable.sections.iter_mut().enumerate() {
        if section.location.zone() != Zone::Window {
            continue;
        }
        let (min, max) = tab_bounds(layout, registry, section)?;
        let preferred = registry.get(&layout.instance(&section.active)?.kind)?.default_size;
        section.location = SectionLocation::Floating { origin: Point { x: 0.0, y: 0.0 } };
        section.size = Size { width: preferred.width.clamp(min.width, max.width), height: preferred.height.clamp(min.height, max.height) };
        section.column = None;
        for (tab, instance) in section.tabs.iter().enumerate() {
            let memory = &mut portable
                .instances
                .iter_mut()
                .find(|known| known.id == *instance)
                .ok_or_else(|| error(Rule::MissingTarget, "native profile tab has no instance"))?
                .memory;
            memory.section = section.id.clone();
            memory.location = section.location.clone();
            memory.section_index = index;
            memory.tab_index = tab;
            memory.size = section.size;
            memory.collapsed = section.collapsed;
            memory.column = None;
            memory.column_index = 0;
            memory.column_width = None;
            memory.weight = section.weight;
            memory.preferred_extent = section.preferred_extent;
            memory.extent_priority = section.extent_priority;
        }
    }
    for instance in &mut portable.instances {
        if !instance.requested_open && instance.memory.location.zone() == Zone::Window {
            instance.memory.location = SectionLocation::Floating { origin: Point { x: 0.0, y: 0.0 } };
            instance.memory.size = registry.get(&instance.kind)?.default_size;
            instance.memory.column = None;
            instance.memory.column_index = 0;
            instance.memory.column_width = None;
        }
    }
    let columns: BTreeSet<_> = portable
        .sections
        .iter()
        .filter_map(|section| section.column.clone())
        .chain(portable.instances.iter().filter_map(|instance| instance.memory.column.clone()))
        .collect();
    portable.columns.retain(|column| columns.contains(&column.id));
    let prepared = service.prepare(LayoutAction::Replace { expected_revision: None, layout: portable })?;
    service.commit(prepared)?;
    Ok(service.layout().clone())
}

fn redocked_portable_layout(layout: &Layout, registry: &PanelRegistry) -> PanelResult<Layout> {
    // Pure reducer projection reuses the exact column/order/tab return policy.
    // No runtime, native resource owner, service or document is reachable here.
    let mut service = LayoutService::new(
        registry.clone(),
        layout.clone(),
        HostCapabilities { native_windows: true, persistence: false, max_native_hosts: MAX_SECTIONS },
    )?;
    let hosts: Vec<_> = layout
        .sections
        .iter()
        .filter_map(|section| match &section.location {
            SectionLocation::Window { host, .. } => Some(host.clone()),
            _ => None,
        })
        .collect();
    for host in hosts {
        let prepared = service.prepare(LayoutAction::Apply { expected_revision: None, mutations: vec![Mutation::NativeClose { host }] })?;
        service.commit(prepared)?;
    }
    let closed: Vec<_> = service
        .layout()
        .instances
        .iter()
        .filter(|instance| !instance.requested_open && instance.memory.location.zone() == Zone::Window)
        .cloned()
        .collect();
    for instance in closed {
        let SectionLocation::Window { host, .. } = &instance.memory.location else { continue };
        for mutation in [
            Mutation::Open { kind: instance.kind, instance: instance.id.clone(), destination: None },
            Mutation::NativeClose { host: host.clone() },
            Mutation::Close { instance: instance.id },
        ] {
            let prepared = service.prepare(LayoutAction::Apply { expected_revision: None, mutations: vec![mutation] })?;
            service.commit(prepared)?;
        }
    }
    Ok(service.layout().clone())
}
