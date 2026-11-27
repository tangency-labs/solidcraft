//! Deterministic multi-core pre-evaluation for cold multi-component and multi-import rebuilds.
//!
//! Two complementary waves run ahead of [`Model::evaluate`]'s sequential timeline loop when a
//! document has multiple cold independent items on desktop (`#[cfg(not(target_arch = "wasm32"))]`),
//! while the main timeline loop commits every feature in exact document order:
//!
//! 1. [`prefetch_imports`]: decodes independent cold [`FeatureKind::Import`] payloads (STEP text
//!    and [`kernel::ImportRecords`]) in parallel across worker threads, deduplicating identical
//!    payloads and bounding nested solid-conversion workers so total concurrency stays capped.
//! 2. [`prefetch_components`]: pre-evaluates independent per-component feature chains whose
//!    features produce valid [`Relative`] changes (verified by [`EvalKey::relative`],
//!    [`Feature::reads`], and [`ModelState::with_relative`]), including per-feature SMLib checks
//!    under [`kernel::Validation::EachFeature`]. Any feature that fails, depends on another
//!    component, lacks pinned names, or cannot form a [`Relative`] change stops that component's
//!    speculative chain and falls back to exact inline evaluation in the sequential loop.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use solidcraft_kernel as kernel;

use super::{
    CachedEval, Change, ComponentDigests, Document, EvalCache, EvalKey, EvalValidationGuard, Feature, FeatureKind, FeatureResult, FrameBodies,
    IMPORT_HEALS, ImportDigests, ModelState, Primed, Value, absorb_sheet_cuts, append_warning, check_kept, copy_instances, eval_feature, fingerprint,
    naming_feature, note_cut_cavity, note_join_apart, now, overlap, own_copies, own_inputs, param_names, track_altered_sheets,
    with_named_face_points,
};

#[cfg(test)]
use super::cost_tests;

/// Maximum outer worker threads spawned by cold evaluation prefetch waves.
const MAX_WORKERS: usize = 16;

/// Pre-decoded payload for a single [`FeatureKind::Import`] feature: its shared [`kernel::StepImport`]
/// and SHA-256 payload digest for face certificates.
pub(super) type PreparedImport = (Arc<kernel::StepImport>, String);

/// Pre-decoded [`FeatureKind::Import`] payloads held alive across a single [`Model::evaluate`] call.
#[derive(Default)]
pub(super) struct PreparedImports {
    by_feature: HashMap<u64, PreparedImport>,
}

impl PreparedImports {
    pub(super) fn get(&self, feature: u64) -> Option<&PreparedImport> {
        self.by_feature.get(&feature)
    }
}

/// A pre-evaluated feature change from an independent component chain, ready to be spliced into
/// the sequential timeline via [`ModelState::with_relative`].
pub(super) struct PrefetchedChange {
    pub(super) cached: CachedEval<Change>,
    #[cfg(test)]
    pub(super) counts: cost_tests::Counts,
}

/// Pre-evaluated independent component feature changes, keyed by `(feature_id, relative_key)`.
#[derive(Default)]
pub(super) struct PrefetchedComponents {
    by_key: HashMap<(u64, u64), PrefetchedChange>,
}

impl PrefetchedComponents {
    pub(super) fn get(&self, feature: u64, relative_key: u64) -> Option<&PrefetchedChange> {
        self.by_key.get(&(feature, relative_key))
    }
}

#[cfg(test)]
thread_local! {
    /// Forces [`available_cores`] to return 1 on this thread so tests can compare parallel cold
    /// evaluation against an otherwise identical sequential run (with caching still enabled).
    static FORCE_SEQUENTIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn without_parallel<R>(f: impl FnOnce() -> R) -> R {
    let prev = FORCE_SEQUENTIAL.with(|flag| flag.replace(true));
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            FORCE_SEQUENTIAL.with(|flag| flag.set(self.0));
        }
    }
    let _restore = Restore(prev);
    f()
}

fn available_cores() -> usize {
    #[cfg(test)]
    if FORCE_SEQUENTIAL.with(std::cell::Cell::get) {
        return 1;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism().map(usize::from).unwrap_or(1)
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// Exact [`EvalKey::reads`] cache key for a non-empty [`FeatureKind::Import`] feature (when below
/// the naming depth limit, where `depth_digest == 0`).
fn import_reads_key(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, shared_ctx: u64, digests: &ImportDigests) -> u64 {
    let local = fingerprint(doc, 0, f, vals, false, digests);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "import-reads".hash(&mut h);
    own_inputs(doc, vals, f, local, shared_ctx, digests, &mut h);
    h.finish()
}

enum ImportPayload {
    Step { step: Arc<str>, digest: u64 },
    Records { records: Arc<kernel::ImportRecords>, digest: u64 },
}

impl ImportPayload {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Step { step: a, digest: da }, Self::Step { step: b, digest: db }) => {
                da == db && a.len() == b.len() && (Arc::ptr_eq(a, b) || a.as_ref() == b.as_ref())
            }
            (Self::Records { records: a, digest: da }, Self::Records { records: b, digest: db }) => da == db && (Arc::ptr_eq(a, b) || a == b),
            _ => false,
        }
    }
}

struct ImportJob {
    payload: ImportPayload,
    features: Vec<u64>,
}

/// Pre-decode independent cold [`FeatureKind::Import`] features in parallel across worker threads.
///
/// Skips features that are suppressed, rolled back, at or past `limit`, already cached in `cache`,
/// present in `primed`/`rolled_results`, or unchanged in `prev_results`. Identical STEP or
/// [`kernel::ImportRecords`] payloads are decoded once and shared across all matching features.
/// Errors and cancelled items are omitted so the main sequential loop handles them identically.
#[allow(clippy::too_many_arguments)]
pub(super) fn prefetch_imports(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    prev_results: &[FeatureResult],
    prev_import_digests: &ImportDigests,
    import_digests: &ImportDigests,
    cache: &Mutex<EvalCache>,
    primed: &[Primed],
    rolled_results: &HashMap<u64, (u64, Option<u64>, FeatureResult)>,
    ctx: u64,
    previous_context: u64,
    shared_ctx: u64,
    no_cache: bool,
    limit: usize,
    cancelled: bool,
) -> PreparedImports {
    if no_cache || cancelled || limit == 0 {
        return PreparedImports::default();
    }
    let cores = available_cores();
    if cores <= 1 {
        return PreparedImports::default();
    }
    let same_ctx = previous_context == ctx;
    let mut jobs: Vec<ImportJob> = Vec::new();
    let mut by_digest: HashMap<(u8, u64, usize), Vec<usize>> = HashMap::new();
    let cache_guard = cache.lock().ok();
    for (i, f) in doc.features.iter().enumerate().take(limit) {
        if f.suppressed {
            continue;
        }
        let FeatureKind::Import { step, records, .. } = &f.kind else {
            continue;
        };
        if records.is_none() && step.is_empty() {
            continue;
        }
        // Skip imports that are already reusable from the previous evaluation, primed results,
        // rolled-back results, or the change cache so warm edits do zero redundant decoding.
        let prev_reusable = same_ctx
            && prev_results.get(i).is_some_and(|old| old.id == f.id && !old.skipped && !old.stale && !old.unkept && old.error.is_none())
            && match records {
                Some(r) => prev_import_digests.records_digest(f.id, r).is_some(),
                None => prev_import_digests.get(f.id, step).is_some(),
            };
        if prev_reusable {
            continue;
        }
        if primed.iter().any(|p| p.results.contains_key(&f.id)) || rolled_results.get(&f.id).is_some_and(|(c, _, _)| *c == ctx) {
            continue;
        }
        let reads_key = import_reads_key(doc, vals, f, shared_ctx, import_digests);
        if cache_guard.as_ref().is_some_and(|m| m.changes.map.contains_key(&reads_key)) {
            continue;
        }
        let (bucket_key, payload) = match records {
            Some(r) => {
                let d = import_digests.records_digest(f.id, r).unwrap_or(0);
                ((1u8, d, r.bodies.len()), ImportPayload::Records { records: r.clone(), digest: d })
            }
            None => {
                let d = import_digests.get(f.id, step).unwrap_or(0);
                ((0u8, d, step.len()), ImportPayload::Step { step: step.clone(), digest: d })
            }
        };
        let existing = by_digest.get(&bucket_key).and_then(|indices| indices.iter().copied().find(|&idx| jobs[idx].payload.same(&payload)));
        if let Some(idx) = existing {
            jobs[idx].features.push(f.id);
        } else {
            let idx = jobs.len();
            jobs.push(ImportJob { payload, features: vec![f.id] });
            by_digest.entry(bucket_key).or_default().push(idx);
        }
    }
    drop(cache_guard);
    // Single unique import uses full inner parallelism directly in the main loop (unless shared by
    // multiple features, in which case decoding once avoids duplicate payload hashing).
    let total_features: usize = jobs.iter().map(|j| j.features.len()).sum();
    if jobs.is_empty() || (jobs.len() == 1 && total_features <= 1) {
        return PreparedImports::default();
    }
    let outer = cores.min(MAX_WORKERS).min(jobs.len());
    let inner = (cores / outer.max(1)).clamp(1, 8);
    let decoded = kernel::map_on_threads(&jobs, outer, |job| -> Option<PreparedImport> {
        if kernel::watch::cancelled() {
            return None;
        }
        match &job.payload {
            ImportPayload::Step { step, .. } => {
                let imp = kernel::step_import_shared_with_workers(step, inner).ok()?;
                if kernel::watch::cancelled() {
                    return None;
                }
                let digest = crate::config::payload_digest(step);
                Some((imp, digest))
            }
            ImportPayload::Records { records, .. } => {
                let imp = kernel::records_import_shared_with_workers(records, inner).ok()?;
                if kernel::watch::cancelled() {
                    return None;
                }
                let text = kernel::records_text(records).ok()?;
                let digest = crate::config::payload_digest(&text);
                Some((imp, digest))
            }
        }
    });
    let mut by_feature = HashMap::with_capacity(total_features);
    for (job, res) in jobs.iter().zip(decoded) {
        let Some(Some(prepared)) = res else { continue };
        for &fid in &job.features {
            by_feature.insert(fid, prepared.clone());
        }
    }
    PreparedImports { by_feature }
}

/// Whether `kind` is one of the feature kinds that [`Feature::reads`] can isolate for relative
/// replay (or [`FeatureKind::Import`], which can seed a component's isolated chain).
fn relative_chain_candidate(kind: &FeatureKind) -> bool {
    matches!(
        kind,
        FeatureKind::Import { .. }
            | FeatureKind::Box { .. }
            | FeatureKind::Cylinder { .. }
            | FeatureKind::Sphere { .. }
            | FeatureKind::Torus { .. }
            | FeatureKind::Coil { sketch: None, axis_ref: None, .. }
            | FeatureKind::Sketch { .. }
            | FeatureKind::Extrude { .. }
            | FeatureKind::Revolve { .. }
            | FeatureKind::Fillet { .. }
            | FeatureKind::Chamfer { .. }
            | FeatureKind::Combine { .. }
            | FeatureKind::Move { rotate_about: None, .. }
            | FeatureKind::Pattern { .. }
            | FeatureKind::SurfaceBoundary { .. }
    )
}

struct ComponentStep<'a> {
    feature: &'a Feature,
    reserved: BTreeSet<String>,
}

struct ComponentChain<'a> {
    steps: Vec<ComponentStep<'a>>,
}

/// Pre-evaluate independent per-component parametric feature chains in parallel during a cold
/// rebuild, emitting verified [`Relative`] changes keyed by `(feature_id, relative_key)`.
///
/// Only runs when at least two distinct components have eligible cold non-import features, all
/// features stay below the naming depth limit, and no sheet-metal or body-transfer features are
/// present. Each chain runs on an isolated `ModelState` and stops at the first feature that fails,
/// fails SMLib validation, or cannot form a [`Relative`] change.
#[allow(clippy::too_many_arguments)]
pub(super) fn prefetch_components(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    param_errors: &BTreeMap<String, String>,
    prev_results: &[FeatureResult],
    empty: &Arc<ModelState>,
    frame_bodies: &Arc<Mutex<FrameBodies>>,
    cache: &Mutex<EvalCache>,
    import_digests: &ImportDigests,
    constructions: &[usize],
    component_digests: &ComponentDigests,
    envelopes: &HashSet<u64>,
    carried: Option<&overlap::Carried>,
    prepared_imports: &PreparedImports,
    validation: kernel::Validation,
    heal: bool,
    ctx: u64,
    previous_context: u64,
    shared_ctx: u64,
    no_cache: bool,
    limit: usize,
    cancelled: bool,
) -> PrefetchedComponents {
    if no_cache || cancelled || limit == 0 || doc.components.len() <= 1 || limit >= crate::naming::MAX_DEPTH {
        return PrefetchedComponents::default();
    }
    let cores = available_cores();
    if cores <= 1 {
        return PrefetchedComponents::default();
    }
    // Sheet metal features, body transfers, and capture-position timelines can alter global sheet
    // state, cross-component ownership, or mid-timeline occurrence transforms; leave those
    // timelines to sequential evaluation.
    if doc.has_capture_position_timeline()
        || doc.features.iter().take(limit).any(|f| !f.suppressed && (f.kind.is_sheet() || matches!(f.kind, FeatureKind::BodyTransfer { .. })))
    {
        return PrefetchedComponents::default();
    }
    // Find the earliest potentially cold feature index.
    let same_ctx = previous_context == ctx;
    let first_cold = if !same_ctx || prev_results.is_empty() {
        0
    } else {
        doc.features
            .iter()
            .enumerate()
            .take(limit)
            .position(|(i, f)| prev_results.get(i).is_none_or(|old| old.id != f.id || old.stale || old.unkept))
            .unwrap_or(limit)
    };
    if first_cold >= limit {
        return PrefetchedComponents::default();
    }
    let carried_component = carried.and_then(|c| component_digests.feature_component.get(&c.id).copied());
    let mut blocked_components: HashSet<u64> = HashSet::new();
    if let Some(c) = carried_component {
        blocked_components.insert(c);
    }
    let mut reserved = doc.reserved_body_names.clone();
    let mut by_comp: BTreeMap<u64, Vec<ComponentStep<'_>>> = BTreeMap::new();
    for (i, f) in doc.features.iter().enumerate().take(limit) {
        if f.suppressed {
            reserved.extend(f.body_names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).map(str::to_string));
            continue;
        }
        if i < first_cold {
            // Components that already have prefix state before `first_cold` are excluded so every
            // worker chain starts cleanly from an empty component state.
            blocked_components.insert(f.component);
            continue;
        }
        if same_ctx && prev_results.get(i).is_some_and(|old| old.id == f.id && !old.skipped && !old.stale && !old.unkept && old.error.is_none()) {
            // Components whose results are already cached from the previous evaluation (such as
            // valid components after a failing feature in a partial model cache) are skipped.
            blocked_components.insert(f.component);
            continue;
        }
        if blocked_components.contains(&f.component) {
            continue;
        }
        if !relative_chain_candidate(&f.kind) {
            // Stop collecting this component once it hits an unsupported feature; earlier features
            // already collected in this component can still pre-evaluate.
            blocked_components.insert(f.component);
            continue;
        }
        by_comp.entry(f.component).or_default().push(ComponentStep { feature: f, reserved: reserved.clone() });
    }
    let chains: Vec<ComponentChain<'_>> = by_comp
        .into_values()
        .filter(|steps| steps.iter().any(|s| !matches!(s.feature.kind, FeatureKind::Import { .. })))
        .map(|steps| ComponentChain { steps })
        .collect();
    if chains.len() < 2 {
        return PrefetchedComponents::default();
    }
    let progress = crate::progress::current();
    let outer = cores.min(MAX_WORKERS).min(chains.len());
    // Workers start with no file scope: they run in the caller's, so the reads a chain makes
    // (a sketch text's font file) stay inside it.
    let carried = crate::worker_scope::capture();
    let results = kernel::map_on_threads(&chains, outer, |chain| {
        crate::worker_scope::within(&carried, || {
            crate::progress::watch_nested(progress.clone(), || {
                eval_component_chain(
                    doc,
                    vals,
                    param_errors,
                    empty,
                    frame_bodies,
                    cache,
                    import_digests,
                    constructions,
                    component_digests,
                    envelopes,
                    prepared_imports,
                    validation,
                    heal,
                    ctx,
                    shared_ctx,
                    chain,
                )
            })
        })
    });
    let mut by_key = HashMap::new();
    for entries in results.into_iter().flatten().flatten() {
        for (k, v) in entries {
            by_key.insert(k, v);
        }
    }
    PrefetchedComponents { by_key }
}

#[allow(clippy::too_many_arguments)]
fn eval_component_chain(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    param_errors: &BTreeMap<String, String>,
    empty: &Arc<ModelState>,
    frame_bodies: &Arc<Mutex<FrameBodies>>,
    cache: &Mutex<EvalCache>,
    import_digests: &ImportDigests,
    constructions: &[usize],
    component_digests: &ComponentDigests,
    envelopes: &HashSet<u64>,
    prepared_imports: &PreparedImports,
    validation: kernel::Validation,
    heal: bool,
    ctx: u64,
    shared_ctx: u64,
    chain: &ComponentChain<'_>,
) -> Vec<((u64, u64), PrefetchedChange)> {
    let mut emitted = Vec::new();
    let feature_component = &component_digests.feature_component;
    let mut state = Arc::new(ModelState { reserved_body_names: doc.reserved_body_names.clone(), ..(**empty).clone() });
    for step in &chain.steps {
        if kernel::watch::cancelled() || crate::progress::cancelled() {
            break;
        }
        let f = step.feature;
        if param_names(f).iter().any(|n| param_errors.contains_key(n)) {
            break;
        }
        if state.reserved_body_names != step.reserved {
            Arc::make_mut(&mut state).reserved_body_names.clone_from(&step.reserved);
        }
        let key = EvalKey::new(doc, vals, f, 0, ctx, shared_ctx, &state, import_digests, constructions, component_digests);
        let Some((reads_key, read)) = key.reads.as_ref() else {
            break;
        };
        if !read.import && key.relative.is_none() {
            break;
        }
        // If `cache` already holds a reusable change for this step, advance our isolated state
        // without recomputing or perturbing `EvalCache::hits` so later cold steps in the same
        // component can still run in parallel.
        let cached_change = if read.import {
            cache.lock().ok().and_then(|m| m.changes.map.get(reads_key).map(|s| s.value.clone()))
        } else {
            key.relative.and_then(|k| cache.lock().ok().and_then(|m| m.changes.map.get(&k).map(|s| s.value.clone())))
        };
        if let Some(c) = cached_change {
            let replayed = if read.import { state.with_change(&c.state, f.id) } else { state.with_relative(&c.state, f.id) };
            let Some(mut next) = replayed else { break };
            let stamp_key = if read.import { *reads_key } else { key.relative.unwrap_or(*reads_key) };
            stamp_local(doc, vals, &state, &mut next, f, stamp_key, envelopes, feature_component);
            state = Arc::new(next);
            continue;
        }
        let Ok(None) = with_named_face_points(doc, vals, &state, f) else {
            break;
        };
        let mut next = (*state).clone();
        next.reserved_body_names.clone_from(&step.reserved);
        next.frame_bodies = Arc::downgrade(frame_bodies);
        for b in &mut next.bodies {
            b.made = crate::lineage::Made::Before;
        }
        let mut warning = None;
        let mut import_report = None;
        let mut repair = None;
        let t0 = now();
        let run_step = || {
            let r = kernel::guard(&f.name, || {
                let _val_guard = EvalValidationGuard::new(validation);
                let (r, deferred) = kernel::validating(validation, || {
                    IMPORT_HEALS.with(|h| *h.borrow_mut() = import_digests.heals(f.id).cloned());
                    let r =
                        eval_feature(doc, vals, f, &mut next, &mut warning, &mut import_report, &mut repair, heal, None, prepared_imports.get(f.id));
                    IMPORT_HEALS.with(|h| *h.borrow_mut() = None);
                    r
                });
                r.map_err(|e| kernel::KernelError::Failed(e.in_feature(&f.name).to_string()))?;
                if deferred {
                    check_kept(&state, &next)?;
                }
                Ok(())
            });
            if r.is_ok() {
                let thin = matches!(&f.kind, FeatureKind::Extrude { thin: Some(_), .. });
                for (sketch, sel) in f.kind.profile_sels() {
                    if let (false, Some(ss)) = (thin, state.sketch(sketch)) {
                        for note in crate::profile_fallback_notes(ss, sel) {
                            append_warning(&mut warning, note);
                        }
                    }
                }
                track_altered_sheets(&state, &mut next, f, &mut warning);
                note_join_apart(doc, &state, &next, f, &mut warning);
                note_cut_cavity(&state, &next, f, &mut warning);
            }
            r.and_then(|()| {
                absorb_sheet_cuts(doc, &state, &mut next, f, validation, &mut warning).map_err(|e| kernel::KernelError::Failed(e.to_string()))
            })
        };
        #[cfg(test)]
        let (r, counts) = cost_tests::measure(run_step);
        #[cfg(not(test))]
        let r = {
            let mut run_step = run_step;
            run_step()
        };
        if r.is_err() || kernel::watch::cancelled() || crate::progress::cancelled() {
            break;
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let Some(change) = next.change_from(&state, f, read) else {
            break;
        };
        if read.import {
            // Stage 1 already decoded the import payload; advance local state so subsequent
            // parametric features in this component can pre-evaluate, and let the main loop apply
            // the import itself in microseconds.
            stamp_local(doc, vals, &state, &mut next, f, *reads_key, envelopes, feature_component);
            state = Arc::new(next);
            continue;
        }
        if change.relative().is_none() {
            break;
        }
        let Some(rel_key) = key.relative else {
            break;
        };
        let cached = CachedEval { error: None, error_code: None, warning, import_report, repair, lost_projections: None, ms, state: change };
        emitted.push((
            (f.id, rel_key),
            PrefetchedChange {
                cached,
                #[cfg(test)]
                counts,
            },
        ));
        stamp_local(doc, vals, &state, &mut next, f, rel_key, envelopes, feature_component);
        state = Arc::new(next);
    }
    emitted
}

#[allow(clippy::too_many_arguments)]
fn stamp_local(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    before: &Arc<ModelState>,
    next: &mut ModelState,
    f: &Feature,
    stamp_key: u64,
    envelopes: &HashSet<u64>,
    feature_component: &HashMap<u64, u64>,
) {
    let mats = own_copies(doc, vals, before, f).unwrap_or_default();
    let mats = match &f.kind {
        FeatureKind::Pattern { features, bodies, components, .. } if bodies.is_empty() && components.is_empty() => {
            copy_instances(doc, vals, before, f, features, &mats).unwrap_or(mats)
        }
        FeatureKind::Mirror { features, bodies, .. } if bodies.is_empty() => copy_instances(doc, vals, before, f, features, &mats).unwrap_or(mats),
        _ => mats,
    };
    let named = naming_feature(vals, f);
    next.stamp(doc, before, &named, stamp_key, mats);
    for b in &mut next.bodies {
        b.envelope = envelopes.contains(&b.feature);
        if !b.transferred {
            b.component = match doc.body_components.get(&b.name) {
                Some(c) => *c,
                None => feature_component.get(&b.feature).copied().unwrap_or(0),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BodyName, Model, Operation, Position, ThreadInfo};
    use solidcore_record::Vec3;

    type BodySnap = (String, u64, u64, usize, u64, u64, Vec<String>);
    type ResultSnap = (u64, Option<String>, Option<String>, bool);

    fn eval_snapshot(model: &Model) -> (Vec<ResultSnap>, Vec<BodySnap>, Vec<ThreadInfo>, usize) {
        let results = model.results.iter().map(|r| (r.id, r.error.clone(), r.warning.clone(), r.skipped)).collect();
        let st = model.state();
        let bodies = st
            .bodies
            .iter()
            .map(|b| {
                let m = kernel::measure(&b.body).unwrap();
                (b.name.clone(), b.key, b.component, m.faces, m.volume.to_bits(), m.area.to_bits(), (*crate::naming::face_names(b)).clone())
            })
            .collect();
        (results, bodies, st.threads.clone(), model.last_recomputed)
    }

    fn step_box(origin: Vec3, size: Vec3, name: &str) -> Arc<str> {
        let solid = kernel::box_solid(origin, origin + size).unwrap();
        kernel::step_export(&[&solid], name).unwrap().into()
    }

    fn records_box(origin: Vec3, size: Vec3, name: &str) -> Arc<kernel::ImportRecords> {
        let brep = kernel::box_solid(origin, origin + size).unwrap().record().clone();
        let face_origin = (0..brep.faces.len() as u64).collect();
        Arc::new(kernel::ImportRecords {
            bodies: vec![kernel::RecordBody {
                name: name.into(),
                path: Vec::new(),
                instance: None,
                color: None,
                paint: None,
                hidden: false,
                placement: None,
                brep: Some(brep),
                face_origin,
                sheet: false,
                copy_of: None,
                mesh: None,
            }],
            tree: vec![kernel::StepNode {
                name: name.into(),
                transform: crate::IDENTITY,
                bodies: vec![0],
                items: Vec::new(),
                left_out: Vec::new(),
                children: Vec::new(),
            }],
            warnings: Vec::new(),
            system: "test".into(),
        })
    }

    /// Cold multi-component assembly combining STEP imports (including duplicate payloads, name
    /// collisions, carried face names, cosmetic threads), ImportRecords, and independent
    /// parametric component chains (Box -> Cylinder Cut -> Chamfer / Fillet): parallel cold
    /// evaluation must match sequential (`no_cache`) bit-for-bit across validation modes, and
    /// `Model::saved` + `Model::adopt` must round-trip identically.
    #[test]
    fn parallel_cold_assembly_eval_is_bit_exact_with_sequential_and_roundtrips_saved() {
        let mut doc = Document::new("parallel assembly parity");
        let shared_step = step_box(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0), "SharedBlock");

        // 4 STEP-imported components (two sharing the exact same payload and colliding body name).
        for k in 0..4 {
            let comp = doc.add_component(Some(&format!("ImpComp{k}")), 0).unwrap();
            let step = if k < 2 {
                shared_step.clone()
            } else {
                step_box(Vec3::new(k as f64 * 25.0, 0.0, 0.0), Vec3::new(10.0 + k as f64, 12.0, 8.0), &format!("Imp{k}"))
            };
            let kind = FeatureKind::Import { file: format!("imp_{k}.step"), step, components: Vec::new(), skip: Vec::new(), records: None };
            let fid = doc.add_feature(kind, Some(&format!("Import{k}"))).unwrap();
            let f = doc.feature_mut(fid).unwrap();
            f.component = comp;
            f.body_names = vec![if k < 2 { "SharedBlock".into() } else { format!("ImpBody{k}") }];
            if k == 2 {
                f.link_faces = vec![vec!["F1:top".into(), "F1:bottom".into()]];
                f.link_threads = vec![vec![ThreadInfo {
                    feature: 0,
                    body: String::new(),
                    designation: "M6".into(),
                    major_diameter: 6.0,
                    radius: 3.0,
                    pitch: 1.0,
                    internal: true,
                    axis_point: Vec3::new(55.0, 5.0, 0.0),
                    axis: solidcraft_geom::UnitVec3::new(Vec3::Z).unwrap(),
                    start: 0.0,
                    end: 8.0,
                    form: crate::thread_forms::ThreadForm::default(),
                    hand: crate::thread_forms::ThreadHand::default(),
                    starts: 1,
                }]];
            }
        }

        // 2 ImportRecords components.
        for k in 0..2 {
            let comp = doc.add_component(Some(&format!("RecComp{k}")), 0).unwrap();
            let rec = records_box(Vec3::new(120.0 + k as f64 * 25.0, 0.0, 0.0), Vec3::new(12.0, 8.0 + k as f64, 6.0), &format!("Rec{k}"));
            let kind = FeatureKind::Import {
                file: format!("rec_{k}.cadir"),
                step: Arc::from(""),
                components: Vec::new(),
                skip: Vec::new(),
                records: Some(rec),
            };
            let fid = doc.add_feature(kind, Some(&format!("RecImport{k}"))).unwrap();
            let f = doc.feature_mut(fid).unwrap();
            f.component = comp;
            f.body_names = vec![format!("RecBody{k}")];
        }

        // 4 independent parametric components: Box -> Cylinder Cut -> Chamfer (pinned body names).
        for k in 0..4 {
            let comp = doc.add_component(Some(&format!("ParamComp{k}")), 0).unwrap();
            let x0 = 200.0 + k as f64 * 40.0;
            let body_name = format!("ParamBody{k}");
            let b_id = doc
                .add_feature(
                    FeatureKind::Box {
                        corner: crate::point_expr(Vec3::new(x0, 0.0, 0.0)),
                        length: "20".into(),
                        width: "20".into(),
                        height: format!("{}", 10 + k * 2).into(),
                        operation: Operation::NewBody,
                        centred: false,
                    },
                    Some(&format!("Box{k}")),
                )
                .unwrap();
            let bf = doc.feature_mut(b_id).unwrap();
            bf.component = comp;
            bf.body_names = vec![body_name.clone()];

            let c_id = doc
                .add_feature(
                    FeatureKind::Cylinder {
                        base: crate::point_expr(Vec3::new(x0 + 10.0, 10.0, -1.0)),
                        axis: crate::point_expr(Vec3::Z),
                        radius: "3".into(),
                        height: "30".into(),
                        operation: Operation::Cut,
                        references: Vec::new(),
                    },
                    Some(&format!("HoleCut{k}")),
                )
                .unwrap();
            let cf = doc.feature_mut(c_id).unwrap();
            cf.component = comp;
            cf.participants = vec![body_name.clone()];

            let ch_id = doc
                .add_feature(
                    FeatureKind::Chamfer {
                        edges: vec![Position(Vec3::new(x0 + 10.0, 0.0, (10 + k * 2) as f64))],
                        distance: "1".into(),
                        distance2: None,
                        angle: None,
                        flip: false,
                        body: Some(BodyName(body_name)),
                        tangent_chain: false,
                    },
                    Some(&format!("Chamfer{k}")),
                )
                .unwrap();
            doc.feature_mut(ch_id).unwrap().component = comp;
        }

        for validation in [kernel::Validation::EachStep, kernel::Validation::EachFeature] {
            // Sequential baseline with caching enabled (identical cache keys & Relative stamping).
            let mut sequential = Model::new();
            sequential.set_validation(validation);
            let seq_counts = cost_tests::counted(|| without_parallel(|| sequential.evaluate(&doc)));

            // Full from-scratch baseline (`no_cache = true`, skips cache & Relative stamping).
            let mut no_cache = Model::new();
            no_cache.set_validation(validation);
            no_cache.no_cache = true;
            no_cache.evaluate(&doc);

            let mut parallel = Model::new();
            parallel.set_validation(validation);
            let par_counts = cost_tests::counted(|| parallel.evaluate(&doc));

            assert!(parallel.results.iter().all(|r| r.error.is_none()), "{validation:?}: {:?}", parallel.results.iter().find(|r| r.error.is_some()));
            assert_eq!(eval_snapshot(&parallel), eval_snapshot(&sequential), "parallel vs sequential mismatch under {validation:?}");
            assert_eq!(par_counts.imports_read, seq_counts.imports_read, "{validation:?}");
            assert_eq!(par_counts.blends, seq_counts.blends, "{validation:?}");
            assert_eq!(par_counts.cut, seq_counts.cut, "{validation:?}");

            // Geometry, naming, threads, results, and recomputed count also match `no_cache` bit-for-bit.
            let strip_keys = |(r, b, t, n): (Vec<ResultSnap>, Vec<BodySnap>, Vec<ThreadInfo>, usize)| {
                let b: Vec<_> = b.into_iter().map(|(name, _key, comp, faces, vol, area, fnm)| (name, comp, faces, vol, area, fnm)).collect();
                (r, b, t, n)
            };
            assert_eq!(
                strip_keys(eval_snapshot(&parallel)),
                strip_keys(eval_snapshot(&no_cache)),
                "parallel vs no_cache mismatch under {validation:?}"
            );

            // Saved model round-trip matches sequential bit-for-bit.
            let saved = parallel.saved(&doc).unwrap();
            let adopted = Model::adopt(&doc, &saved).unwrap();
            let (par_res, par_bodies, par_threads, _) = eval_snapshot(&parallel);
            let (ad_res, ad_bodies, ad_threads, _) = eval_snapshot(&adopted);
            assert_eq!(ad_res, par_res, "adopted results mismatch under {validation:?}");
            assert_eq!(ad_bodies, par_bodies, "adopted bodies mismatch under {validation:?}");
            assert_eq!(ad_threads, par_threads, "adopted threads mismatch under {validation:?}");

            // Warm re-evaluation with no changes must recompute zero features and read zero imports.
            let warm = cost_tests::counted(|| parallel.evaluate(&doc));
            assert_eq!(parallel.last_recomputed, 0, "warm re-eval should recompute nothing");
            assert_eq!(warm.imports_read, 0, "warm re-eval should read no imports");
        }
    }

    /// Cooperative cancellation (`progress::watch` + `Progress::cancel`) stops parallel import and
    /// component prefetch without executing features or poisoning the shared evaluation cache.
    #[test]
    fn cancelled_watch_skips_parallel_prefetch_and_leaves_cache_clean() {
        let mut doc = Document::new("cancel parallel");
        for k in 0..4 {
            let comp = doc.add_component(Some(&format!("C{k}")), 0).unwrap();
            let step = step_box(Vec3::new(k as f64 * 20.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0), &format!("P{k}"));
            let fid = doc
                .add_feature(FeatureKind::Import { file: format!("p{k}.step"), step, components: Vec::new(), skip: Vec::new(), records: None }, None)
                .unwrap();
            let f = doc.feature_mut(fid).unwrap();
            f.component = comp;
            f.body_names = vec![format!("Body{k}")];
        }

        let mut model = Model::new();
        // A cancelled evaluation (e.g. a discarded preview sharing `model.cache`) skips all work
        // and leaves the shared cache clean.
        let mut cancelled_run = model.clone();
        let progress = Arc::new(crate::progress::Progress::default());
        progress.cancel();
        let work = cost_tests::counted(|| crate::progress::watch(progress, || cancelled_run.evaluate(&doc)));
        assert_eq!(work.imports_read, 0, "cancelled eval must not read imports");
        assert!(cancelled_run.results.iter().all(|r| r.skipped), "all features should be skipped when cancelled");
        assert!(model.cache.lock().unwrap().is_empty(), "cancelled eval must not poison shared cache");

        // Subsequent uncancelled evaluation on `model` (sharing that cache) completes cleanly.
        model.evaluate(&doc);
        assert_eq!(model.state().bodies.len(), 4);
        assert!(model.results.iter().all(|r| !r.skipped && r.error.is_none()));
    }
}
