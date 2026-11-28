//! Native camera queue preparation. Runtime command metadata is distinct from
//! funded renderer storage: the long-lived shared request is an exact Vec arena.
use super::{
    Kind,
    batch_payload::{self, Payload},
};
use crate::{EngineError, Result, Session};
use serde_json::{Value, json};
use solidcraft_io::vfs::PathScope;
use solidcraft_render::job::{CancelToken, Phase, RetainedMetadata};
use solidcraft_viewport::Camera;
use std::mem::size_of;

fn bad(message: &str) -> EngineError {
    EngineError::Other(message.into())
}
fn check(cancel: &CancelToken) -> Result<()> {
    cancel.check().map_err(crate::render_jobs::job_error)
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ticket {
    uid: u64,
    owner: u64,
    epoch: u64,
}
impl Ticket {
    pub(crate) fn matches(self, s: &Session) -> bool {
        self.uid == s.uid && self.owner == s.render_jobs.owner_key()
    }
}
pub(crate) struct NativeState {
    pub ticket: Ticket,
    pub next: usize,
    pub total: usize,
    pub claimed: bool,
    pub cancel: CancelToken,
}
struct Row {
    name: String,
    camera: Camera,
}
/// Opaque persistent shared request. Backing precedes its independent lease.
pub struct Plan {
    ticket: Ticket,
    payload: Payload,
    rows: Vec<Row>,
    scope: Option<PathScope>,
    next: usize,
    lease: RetainedMetadata,
}
impl Plan {
    pub fn owner_matches(&self, s: &Session) -> bool {
        self.ticket.matches(s)
    }
    pub fn retained_bytes(&self) -> u64 {
        self.lease.bytes()
    }
}
/// One exact source/front claim; a failed channel delivery retains this owner.
pub struct Claim {
    ticket: Ticket,
    front: usize,
    first: bool,
    total: usize,
    source: Session,
    params: Option<Value>,
    scope: Option<PathScope>,
    plan: Option<Plan>,
    cancel: CancelToken,
}
impl Claim {
    pub fn owner_matches(&self, s: &Session) -> bool {
        self.ticket.matches(s)
    }
    pub fn is_first(&self) -> bool {
        self.first
    }
}
/// Success or failure owns its complete retirement path. There is no heavy Err
/// payload implicitly destroyed on the window.
pub struct Prepared {
    ticket: Ticket,
    front: usize,
    first: bool,
    total: usize,
    plan: Option<Plan>,
    inner: Option<super::Prepared>,
    // Exact first caller input is command metadata, not persistent Plan backing.
    // Keep it through current-owner policy validation and worker retirement.
    first_params: Option<Value>,
    error: Option<String>,
    cancel: CancelToken,
    committed: bool,
    finished: bool,
}
impl Prepared {
    pub fn owner_matches(&self, s: &Session) -> bool {
        self.ticket.matches(s)
    }
    pub fn is_first(&self) -> bool {
        self.first
    }
    pub fn committed(&self) -> bool {
        self.committed
    }
}

/// Caller starts its empty bounded worker first. Metadata refusal takes neither
/// owned input nor scope. No source capture, resource IO or job starts here.
pub fn claim(s: &mut Session, params: &mut Option<Value>, scope: &mut Option<PathScope>, cancel: CancelToken) -> Result<Claim> {
    check(&cancel)?;
    let input = params.as_ref().ok_or_else(|| bad("batch request was already claimed"))?;
    solidcraft_io::vfs::scoped(scope.clone(), || super::validate_command_id(s, "render.batch", input))?;
    s.render_batch_ready().map_err(EngineError::Other)?;
    if s.model.is_stale() {
        return Err(bad("finish the modelling gesture before rendering shots"));
    }
    let epoch = s.render_jobs.native_epoch.checked_add(1).ok_or_else(|| bad("native shot queue identity exhausted"))?;
    let owner = s.render_jobs.owner_key();
    if owner == 0 {
        return Err(bad("render session identity exhausted"));
    }
    let ticket = Ticket { uid: s.uid, owner, epoch };
    let total = s.doc.render.setups.len();
    let source = s.scratch();
    let params = params.take().ok_or_else(|| bad("batch request vanished before claim"))?;
    s.render_jobs.native_epoch = epoch;
    s.render_jobs.batch_error = None;
    s.render_jobs.native_batch = Some(NativeState { ticket, next: 0, total, claimed: true, cancel: cancel.clone() });
    Ok(Claim { ticket, front: 0, first: true, total, source, params: Some(params), scope: scope.take(), plan: None, cancel })
}
/// None means the actual current job is still running or its front is claimed.
/// Refusal preserves the plan; the consumer retires it on its worker.
pub fn claim_next(s: &mut Session, slot: &mut Option<Plan>, cancel: CancelToken) -> Result<Option<Claim>> {
    check(&cancel)?;
    let plan = slot.as_ref().ok_or_else(|| bad("batch plan was already claimed"))?;
    if !plan.owner_matches(s) {
        return Err(bad("batch plan belongs to another real session owner"));
    }
    // Verify the exact real owner/epoch/front before metadata errors may fail
    // this queue. A stale Plan is not authority to alter a replacement ticket.
    let native =
        s.render_jobs.native_batch.as_ref().filter(|n| n.ticket == plan.ticket).ok_or_else(|| bad("native shot queue is no longer current"))?;
    if native.claimed {
        return Ok(None);
    }
    if native.next != plan.next || native.total != plan.rows.len() || native.next >= native.total {
        return Err(bad("native shot queue front changed"));
    }
    let ticket = plan.ticket;
    let ready = (|| {
        s.advance_render_batch()?;
        if !s.render_jobs.native_batch.as_ref().is_some_and(|n| n.ticket == ticket) {
            return Err(bad("native shot queue is no longer current"));
        }
        if let Some(job) = s.render_jobs.batch_job {
            let status = s.render_jobs.peek_status(job).map_err(crate::render_jobs::job_error)?;
            if !status.progress.phase.terminal() {
                return Ok(false);
            }
            if status.progress.phase != Phase::Completed {
                return Err(bad("previous native shot did not complete"));
            }
        }
        Ok(true)
    })();
    match ready {
        Ok(true) => {}
        Ok(false) => return Ok(None),
        Err(error) => {
            // Fixed metadata only: retain Plan/source backing for its original
            // worker retirement and never touch a newer queue on this owner.
            fail(s, ticket, &error.to_string());
            return Err(error);
        }
    }
    let source = s.scratch();
    let front = plan.next;
    let total = plan.rows.len();
    let ticket = plan.ticket;
    let plan = slot.take().ok_or_else(|| bad("batch plan vanished before claim"))?;
    if let Some(native) = &mut s.render_jobs.native_batch {
        native.claimed = true;
        native.cancel = cancel.clone();
    }
    Ok(Some(Claim { ticket, front, first: false, total, source, params: None, scope: None, plan: Some(plan), cancel }))
}

fn shot_params(mut value: Value, camera: Camera) -> Result<Value> {
    value
        .get_mut("settings")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| bad("queued render settings are missing"))?
        .insert("camera".into(), json!(camera));
    Ok(value)
}
fn plan_bytes(source: &Session, payload: batch_payload::Counts, scope: Option<&PathScope>) -> Result<u64> {
    let rows = source.doc.render.setups.len();
    let mut bytes = (size_of::<Plan>() as u64).checked_add(payload.bytes()?).ok_or_else(|| bad("batch plan byte overflow"))?;
    bytes = bytes
        .checked_add(rows.checked_mul(size_of::<Row>()).ok_or_else(|| bad("batch row byte overflow"))? as u64)
        .ok_or_else(|| bad("batch plan byte overflow"))?;
    for name in source.doc.render.setups.keys() {
        bytes = bytes.checked_add(name.len() as u64).ok_or_else(|| bad("batch name byte overflow"))?;
    }
    bytes
        .checked_add(scope.map_or(Ok(0), PathScope::retained_heap_bytes).map_err(|e| bad(&e.to_string()))?)
        .ok_or_else(|| bad("batch scope byte overflow"))
}
fn first(claim: Claim, out: &mut Prepared) -> Result<()> {
    out.first_params = claim.params;
    let input = out.first_params.as_ref().ok_or_else(|| bad("initial batch input missing"))?;
    let empty = Value::Object(Default::default());
    let input = if input.is_null() { &empty } else { input };
    // Both the original Batch and every nested Start retain their exact guard
    // inputs for current-owner validation before the first real enqueue.
    let common = crate::cmd::batch_request(&claim.source, input)?;
    let (name, camera) = claim.source.doc.render.setups.iter().next().map(|(n, s)| (n.clone(), s.camera)).ok_or_else(|| bad("no saved shots"))?;
    let params = shot_params(common.clone(), camera)?;
    let mut inner = super::prepare(claim.source, Kind::Start, params, claim.cancel.clone())?;
    inner.shot = Some(name);
    out.inner = Some(inner);
    let counts = batch_payload::count(&common, &claim.cancel)?;
    let source = &out.inner.as_ref().ok_or_else(|| bad("missing first prepared owner"))?.source;
    let bytes = plan_bytes(source, counts, claim.scope.as_ref())?;
    let lease = super::submission(&out.inner.as_ref().ok_or_else(|| bad("missing first preparation"))?.capture)?
        .reserve_retained_metadata(bytes)
        .map_err(crate::render_jobs::job_error)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(claim.total).map_err(|_| bad("could not allocate batch rows"))?;
    if rows.capacity() != claim.total {
        return Err(bad("batch row capacity exceeds reservation"));
    }
    for (name, setup) in &source.doc.render.setups {
        check(&claim.cancel)?;
        let mut copied = String::new();
        copied.try_reserve_exact(name.len()).map_err(|_| bad("could not retain batch name"))?;
        if copied.capacity() != name.len() {
            return Err(bad("batch name capacity exceeds reservation"));
        }
        copied.push_str(name);
        rows.push(Row { name: copied, camera: setup.camera });
    }
    let payload = Payload::new(common, counts, &claim.cancel)?;
    // No normalized Arc<Value> remains alongside this arena. The exact original
    // Batch input belongs only to this first Prepared command metadata owner,
    // never to the persistent Plan, and retires on the same worker after finish.
    let mut plan = Plan { ticket: claim.ticket, payload, rows, scope: claim.scope, next: 0, lease };
    if plan.actual_bytes()? != plan.lease.bytes() {
        return Err(bad("batch arena actual backing disagrees with reservation"));
    }
    plan.lease.settle().map_err(crate::render_jobs::job_error)?;
    out.plan = Some(plan);
    Ok(())
}
fn next(mut claim: Claim, out: &mut Prepared) -> Result<()> {
    out.plan = claim.plan.take();
    let plan = out.plan.as_ref().ok_or_else(|| bad("missing next-shot plan"))?;
    let row = plan.rows.get(claim.front).ok_or_else(|| bad("batch camera row missing"))?;
    let params = shot_params(plan.payload.value(&claim.cancel)?, row.camera)?;
    let mut inner = super::prepare_with_metadata(claim.source, Kind::Start, params, claim.cancel.clone(), Some(&plan.lease))?;
    inner.shot = Some(row.name.clone());
    out.inner = Some(inner);
    Ok(())
}
impl Plan {
    fn actual_bytes(&self) -> Result<u64> {
        let mut bytes = (size_of::<Self>() as u64).checked_add(self.payload.heap_bytes()?).ok_or_else(|| bad("batch actual byte overflow"))?;
        bytes = bytes
            .checked_add(self.rows.capacity().checked_mul(size_of::<Row>()).ok_or_else(|| bad("batch actual row overflow"))? as u64)
            .ok_or_else(|| bad("batch actual row overflow"))?;
        for row in &self.rows {
            bytes = bytes.checked_add(row.name.capacity() as u64).ok_or_else(|| bad("batch actual name overflow"))?;
        }
        bytes
            .checked_add(self.scope.as_ref().map_or(Ok(0), PathScope::retained_heap_bytes).map_err(|e| bad(&e.to_string()))?)
            .ok_or_else(|| bad("batch actual scope overflow"))
    }
}
/// Always returns the exact owner, including failures. Caller must retire it on
/// this same worker after finish/stale/cancel, and wait for acknowledgement.
pub fn prepare(claim: Claim) -> Prepared {
    let mut out = Prepared {
        ticket: claim.ticket,
        front: claim.front,
        first: claim.first,
        total: claim.total,
        plan: None,
        inner: None,
        first_params: None,
        error: None,
        cancel: claim.cancel.clone(),
        committed: false,
        finished: false,
    };
    let scope = claim.scope.clone().or_else(|| claim.plan.as_ref().and_then(|p| p.scope.clone()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        solidcraft_io::vfs::scoped(scope, || {
            check(&claim.cancel)?;
            if claim.first { first(claim, &mut out) } else { next(claim, &mut out) }
        })
    }));
    out.error = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.to_string()),
        Err(_) => Some("native batch preparation failed internally".into()),
    };
    out
}
fn fail(s: &mut Session, ticket: Ticket, message: &str) {
    if s.render_jobs.native_batch.as_ref().is_some_and(|n| n.ticket == ticket) {
        if let Some(native) = s.render_jobs.native_batch.take() {
            native.cancel.cancel();
        }
        s.render_jobs.batch_job = None;
        s.render_jobs.batch_error = Some(message.into());
    }
}
/// The real owner commits; a different current tab cannot adopt its jobs. All
/// error and successful source backing remains in `out` for worker retirement.
pub fn finish_retained(s: &mut Session, out: &mut Prepared) -> Result<Value> {
    if !out.owner_matches(s) {
        return Err(bad("batch completion belongs to another real session owner"));
    }
    if out.finished {
        return Err(bad("batch preparation was already completed"));
    }
    let native = s
        .render_jobs
        .native_batch
        .as_ref()
        .filter(|n| n.ticket == out.ticket && n.claimed && n.next == out.front && n.total == out.total)
        .ok_or_else(|| bad("native shot queue claim is no longer current"))?;
    if let Err(error) = check(&native.cancel) {
        out.finished = true;
        fail(s, out.ticket, &error.to_string());
        return Err(error);
    }
    let result = (|| {
        check(&out.cancel)?;
        if let Some(error) = &out.error {
            return Err(bad(error));
        }
        let scope = out.plan.as_ref().and_then(|p| p.scope.clone());
        let inner = out.inner.as_mut().ok_or_else(|| bad("batch preparation has no proved job"))?;
        let reply = solidcraft_io::vfs::scoped(scope, || {
            if out.first {
                let input = out.first_params.as_ref().ok_or_else(|| bad("first batch guard input missing"))?;
                let spec = crate::find_command("render.batch").ok_or_else(|| EngineError::UnknownCommand("render.batch".into()))?;
                // The ordinary enablement shares this readiness predicate. Only
                // this exact first claimed front is exempt from its own busy bit.
                s.render_batch_ready_for_first(out.ticket).map_err(|why| EngineError::Disabled(spec.id.into(), why))?;
                s.check_command(spec, input)?;
            }
            super::finish_inner(s, inner, Some(out.ticket))
        })?;
        out.committed = true;
        let plan = out.plan.as_mut().ok_or_else(|| bad("batch completion has no plan"))?;
        plan.next = out.front + 1;
        Ok(if out.first { json!({"queued":out.total}) } else { reply })
    })();
    out.finished = true;
    if let Err(error) = &result
        && !out.committed
    {
        fail(s, out.ticket, &error.to_string());
    }
    result
}
pub fn take_plan(out: &mut Prepared) -> Result<Option<Plan>> {
    if !out.committed {
        return Err(bad("batch plan cannot advance before its actual job"));
    }
    if out.plan.as_ref().is_some_and(|p| p.next < p.rows.len()) { Ok(out.plan.take()) } else { Ok(None) }
}
/// Metadata-only cancellation, including a first claim which has no fabricated
/// job ID. Idle/closed plans still require their separate worker retirement.
pub fn cancel(s: &mut Session) -> Result<bool> {
    let Some(native) = s.render_jobs.native_batch.take() else { return Ok(false) };
    native.cancel.cancel();
    if let Some(job) = s.render_jobs.batch_job.take() {
        // A discarded completed front can survive only in the bounded status
        // history. It needs no live handle or additional cancellation; looking
        // it up must not collect or evict another retained result here.
        let status = s.render_jobs.peek_status(job).map_err(crate::render_jobs::job_error)?;
        if !status.progress.phase.terminal() {
            let _ = s.render_jobs.handle(job).map_err(crate::render_jobs::job_error)?.cancel().map_err(crate::render_jobs::job_error)?;
        }
    }
    Ok(true)
}

/// Cleanup for a particular idle plan. Owner equality alone is deliberately
/// insufficient: an identical replacement queue has a different epoch.
pub fn cancel_plan(s: &mut Session, plan: &Plan) -> Result<bool> {
    if !plan.owner_matches(s) || !s.render_jobs.native_batch.as_ref().is_some_and(|n| n.ticket == plan.ticket) {
        return Ok(false);
    }
    cancel(s)
}

/// The native worker wraps its entire send/return/drop lifecycle here. Send the
/// release acknowledgement only after this returns, so both captured backing
/// and its temporary caller permission context have really retired. The plan
/// itself still owns/funds exactly one persistent scope between jobs.
pub fn with_scope<T>(claim: Claim, f: impl FnOnce(Claim) -> T) -> T {
    let scope = claim.scope.clone().or_else(|| claim.plan.as_ref().and_then(|p| p.scope.clone()));
    solidcraft_io::vfs::scoped(scope, || f(claim))
}
/// Idle, closed or rejected plan-only messages need no second stored scope.
/// Hold the lease until original scope storage and all backing have dropped.
pub fn retire_plan(plan: Plan) {
    let Plan { payload, rows, scope, lease, .. } = plan;
    solidcraft_io::vfs::scoped(scope, || drop((payload, rows)));
    drop(lease);
}
/// Pure ordinary cancellation enablement/policy/typed-argument check. A pending
/// UI input has no invented job: None may cancel that input; Some must follow
/// the ordinary actual-job cancellation route, including ownership validation.
pub fn check_cancel(s: &Session, params: &Value) -> Result<Option<solidcraft_render::job::JobId>> {
    super::validate_command_id(s, "render.cancel", params)?;
    let job = crate::cmd::render_cancel_params(params)?;
    if let Some(job) = job {
        s.render_jobs.check_retained_or_historical_job(job).map_err(crate::render_jobs::job_error)?;
    }
    Ok(job)
}
