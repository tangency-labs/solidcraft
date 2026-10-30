//! The API object ([`Smlib`]) and what every call shares: the kernel, the log callback,
//! the dump directory for replaying failed calls, and the deep copy inputs are worked on.
//!
//! Source: tangency-labs/smlib `capi/src/smlib_shim.h` (`Call`, `copy_of`, `take`),
//! `capi/src/smlib_ops.cpp` (`finish`, `closed_nonmanifold`) and
//! `capi/src/smlib_c.cpp` (`smlib_abi_version`, `smlib_init`, `smlib_set_log`,
//! `smlib_set_dump_dir`, `smlib_current_call`, `smlib_work_counters*`), replay/1.0.1 at f8fd83e.
//!
//! | Rust | C API |
//! |------|-------|
//! | [`Smlib::abi_version`], [`ABI_VERSION`] | `smlib_abi_version`, `SMLIB_ABI_VERSION` |
//! | [`Smlib::init`] | `smlib_init` |
//! | [`Smlib::set_log`] | `smlib_set_log` |
//! | [`Smlib::set_dump_dir`] | `smlib_set_dump_dir` |
//! | [`Smlib::current_call`] | `smlib_current_call` |
//! | [`Smlib::work_counters`], [`Smlib::work_counters_reset`] | `smlib_work_counters`, `smlib_work_counters_reset` |
//! | `run` (private) | `smlib_shim::Call::run` |
//! | `copy` (private) | `copy_of` (`SmApiBrepCopy`) |
//! | [`fork_kernel`] | (none: the fork's SMLib build) |
//! | `take`, `finish`, `closed_nonmanifold` (crate) | `smlib_shim::take`, `finish`, `closed_nonmanifold` (`smlib_ops.cpp`) |
//!
//! Deviations: state is per [`Smlib`] value, not process-global; the log line has no
//! duration (no clock on wasm32); SMLib's own warnings (`smos_ErrorMessage`) are not
//! forwarded (the port reports none); the work counters are zero (the port does not count
//! that work); a dump writes the call's line and its B-rep inputs as ASCII `.smb` files,
//! best effort, as the C API does.

use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::p2::shell_vertices;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::edge;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::Kernel;

use crate::status::{Result, Status};

/// `SMLIB_ABI_VERSION` of the C API this crate follows.
pub const ABI_VERSION: i32 = 4;

/// The log callback (`smlib_log_fn`): called with level 0 for a call that succeeded, 1
/// for one that failed, and one line naming the call and its status, such as
/// `smlib_make_box(origin=[0,0,0], size=[10,20,30]) -> 0`.
pub type LogFn = Box<dyn FnMut(i32, &str) + Send>;

/// The API: one kernel and the diagnostics settings, used through `&mut self`.
pub struct Smlib {
    kernel: Kernel,
    log: Option<LogFn>,
    dump_dir: Option<String>,
    dump_seq: u64,
    last_error: Option<SolidError>,
    failed_point: Option<usize>,
    failed_corner: Option<usize>,
}

/// The kernel the C API's results come from: solidcore's default with the SolidCraft
/// preset of fixes (`UpstreamFixes::solidcraft`), the fixes of the SMLib fork the C API
/// is built on and the port's own for cases SolidCraft reported. solidcore's own default
/// stays upstream SMLib 1.0.1.
#[must_use]
pub fn fork_kernel() -> Kernel {
    Kernel {
        fixes: UpstreamFixes::solidcraft(),
        ..Kernel::default()
    }
}

impl Default for Smlib {
    /// The API over [`fork_kernel`].
    fn default() -> Self {
        Self::new(fork_kernel())
    }
}

impl core::fmt::Debug for Smlib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Smlib")
            .field("dump_dir", &self.dump_dir)
            .field("last_error", &self.last_error)
            .finish_non_exhaustive()
    }
}

impl Smlib {
    /// The API over `kernel`: its tolerances, random state and `UpstreamFixes` (the C API
    /// runs the fork's SMLib, whose fixes are opt-in flags here).
    #[must_use]
    pub fn new(kernel: Kernel) -> Self {
        Self {
            kernel,
            log: None,
            dump_dir: None,
            dump_seq: 0,
            last_error: None,
            failed_point: None,
            failed_corner: None,
        }
    }

    /// The kernel the calls run on.
    #[must_use]
    pub const fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    /// The kernel, to set options or fixes between calls.
    pub fn kernel_mut(&mut self) -> &mut Kernel {
        &mut self.kernel
    }

    /// The solidcore error behind the last failed call's status, when it came from the
    /// kernel (an argument check has none).
    #[must_use]
    pub const fn last_error(&self) -> Option<&SolidError> {
        self.last_error.as_ref()
    }

    /// The C API's `out_failed_point` of the last call: the index of the selection point
    /// that picked nothing or could not decide, when the call failed with
    /// `SMLIB_ERR_PICK_NONE` or `SMLIB_ERR_PICK_AMBIGUOUS`; `None` otherwise.
    #[must_use]
    pub const fn last_failed_point(&self) -> Option<usize> {
        self.failed_point
    }

    /// The C API's `out_failed_corner` of the last call: the index of the corner a
    /// setback blend refused, when the call reports one; `None` otherwise.
    #[must_use]
    pub const fn last_failed_corner(&self) -> Option<usize> {
        self.failed_corner
    }

    /// Records the call's `out_failed_point` (from inside `run`).
    #[allow(dead_code)] // the families that pick (blends, offsets) set it
    pub(crate) fn set_failed_point(&mut self, index: usize) {
        self.failed_point = Some(index);
    }

    /// Records the call's `out_failed_corner` (from inside `run`).
    #[allow(dead_code)] // the setback blends set it
    pub(crate) fn set_failed_corner(&mut self, index: usize) {
        self.failed_corner = Some(index);
    }

    /// `smlib_abi_version`.
    #[must_use]
    pub const fn abi_version() -> i32 {
        ABI_VERSION
    }

    /// `smlib_init`: nothing to set up (the kernel exists with the value); kept for the
    /// C API's call sequence.
    pub fn init(&mut self) -> Result<()> {
        self.run(|| "smlib_init()".to_string(), &[], |_| Ok(()))
    }

    /// `smlib_set_log`: sets (or, with `None`, clears) the log callback.
    pub fn set_log(&mut self, log: Option<LogFn>) {
        self.log = log;
    }

    /// `smlib_set_dump_dir`: a directory where every failing call leaves `<n>-<function>.txt`
    /// (its line) and `<n>-<function>-<input>.smb` (each B-rep it was given, ASCII); `None`
    /// or `""` turns this off.
    #[allow(clippy::unnecessary_wraps)] // the C API's status, kept for its callers
    pub fn set_dump_dir(&mut self, dir: Option<&str>) -> Result<()> {
        self.dump_dir = dir.filter(|d| !d.is_empty()).map(ToString::to_string);
        Ok(())
    }

    /// `smlib_current_call`: the call running right now. A call holds `&mut self`, so no
    /// other code can ask while one runs: always empty.
    #[must_use]
    #[allow(clippy::unused_self)] // the C API's call, kept for its callers
    pub const fn current_call(&self) -> &'static str {
        ""
    }

    /// `smlib_work_counters_reset`.
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)] // the C API's call, kept for its callers
    pub fn work_counters_reset(&mut self) -> Result<()> {
        Ok(())
    }

    /// `smlib_work_counters`: the fork's boolean work counters (closure faceuses, face
    /// pairs, faceuses moved). The port does not count them: zeros.
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)] // the C API's call, kept for its callers
    pub fn work_counters(&self) -> Result<[i64; 3]> {
        Ok([0; 3])
    }

    /// `Call::run`: runs `f` on the kernel, logs the call's line (built only when there is
    /// a log) with its status, records the solidcore error of a failure and, when a dump
    /// directory is set, dumps a failed call's inputs.
    pub(crate) fn run<T>(
        &mut self,
        line: impl FnOnce() -> String,
        inputs: &[(&str, &Brep)],
        f: impl FnOnce(&mut Self) -> core::result::Result<T, Fail>,
    ) -> Result<T> {
        self.last_error = None;
        self.failed_point = None;
        self.failed_corner = None;
        let result = f(self).map_err(|fail| match fail {
            Fail::Status(s) => s,
            Fail::Solid(e) => {
                let s = Status::from_solid(&e);
                self.last_error = Some(e);
                s
            }
        });
        let status = result.as_ref().err().copied().unwrap_or(Status::OK);
        if self.log.is_some() || (status != Status::OK && self.dump_dir.is_some()) {
            let name_line = line();
            let name = name_line.split('(').next().unwrap_or("smlib").to_string();
            let text = format!("{name_line} -> {}", status.code());
            if let Some(log) = self.log.as_mut() {
                log(i32::from(status != Status::OK), &text);
            }
            if status != Status::OK {
                self.dump(&name, &text, inputs);
            }
        }
        result
    }

    /// Writes the call's line and its B-rep inputs into the dump directory. Best effort: a
    /// dump that cannot be written never changes the call's status.
    fn dump(&mut self, name: &str, line: &str, inputs: &[(&str, &Brep)]) {
        let Some(dir) = self.dump_dir.clone() else {
            return;
        };
        self.dump_seq += 1;
        let stem = format!("{dir}/{}-{name}", self.dump_seq);
        let _ = std::fs::write(format!("{stem}.txt"), format!("{line}\n"));
        for (input, b) in inputs {
            if let Ok(bytes) = self.smb_bytes(b, true) {
                let _ = std::fs::write(format!("{stem}-{input}.smb"), bytes);
            }
        }
    }

    /// `SmBrep::WriteToFile` of `b` into bytes (ASCII or binary).
    pub(crate) fn smb_bytes(&self, b: &Brep, ascii: bool) -> solidcore::Result<Vec<u8>> {
        let store = AttributeStore::new();
        let records = NotifyRecords::new();
        // the body's own naming, which its geometry owners are recorded under
        let key_of = |e| b.owner_identity(e);
        let env = FromBrepEnv {
            attributes: &store,
            key_of: &key_of,
            records: &records,
            curve_key: &|_: EdgeId| ObjectKey(0),
            uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
            surface_key: &|_: FaceId| ObjectKey(0),
        };
        solidcore::topology::api_files::write_brep_to_file(&self.kernel, Some(b), &env, ascii)
    }

    /// `copy_of` (`SmApiBrepCopy`): a deep copy that owns its geometry. A plain `Clone`
    /// keeps the source's geometry owners.
    pub(crate) fn copy(&mut self, b: &Brep) -> core::result::Result<Brep, Fail> {
        // Every kernel call names a body by its own naming (`Brep::owner_identity`): the
        // source by its own, the new copy by a new B-rep's.
        let fresh = Brep::new();
        let target_key = |entity| fresh.owner_identity(entity);
        let mut state = ArenaNotifyState::new(&fresh);
        let mut env = state.env(None);
        env.key_of = &target_key;
        let copy = p3::api_brep_copy(&mut self.kernel, &mut env, Some(b), &|entity| {
            b.owner_identity(entity)
        })?;
        Ok(copy)
    }
}

/// How a call's body fails: a status decided by the glue, or a kernel error mapped by
/// [`Status::from_solid`].
#[derive(Debug)]
pub(crate) enum Fail {
    Status(Status),
    Solid(SolidError),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        Self::Solid(e)
    }
}

impl From<Status> for Fail {
    fn from(s: Status) -> Self {
        Self::Status(s)
    }
}

/// `smlib_shim::take`: a result must have faces, or the call is `SMLIB_ERR_EMPTY`.
pub(crate) fn take(b: Brep) -> core::result::Result<Brep, Fail> {
    if b.face_list().is_empty() {
        return Err(Fail::Status(Status::ERR_EMPTY));
    }
    Ok(b)
}

/// `finish`: a finished operation's body must have faces, and a solid input must give a
/// closed manifold solid, or (`nonmanifold`) a closed one whose solids touch along an edge
/// or at a vertex (`SMLIB_ERR_EMPTY`, `SMLIB_ERR_NOT_SOLID`).
pub(crate) fn finish(
    r: Brep,
    solid_in: bool,
    nonmanifold: bool,
) -> core::result::Result<Brep, Fail> {
    if r.face_list().is_empty() {
        return Err(Fail::Status(Status::ERR_EMPTY));
    }
    if solid_in && !brep_is_manifold_solid(&r)? && !(nonmanifold && closed_nonmanifold(&r)?) {
        return Err(Fail::Status(Status::ERR_NOT_SOLID));
    }
    take(r)
}

/// `closed_nonmanifold`: whether `r` is closed though not manifold: bounded regions, no
/// wire edges or shell vertices, and every edge shared by its faces in pairs (four edge
/// uses per pair of face sides).
pub(crate) fn closed_nonmanifold(r: &Brep) -> solidcore::Result<bool> {
    if regions(r, None)?.len() < 2 {
        return Ok(false);
    }
    if !shell_vertices(r, None)?.is_empty() {
        return Ok(false);
    }
    for &e in r.edge_list() {
        if edge::is_wire(r, e)? {
            return Ok(false);
        }
        // (a failed `GetEdgeuses` answers no)
        let Ok(uses) = edge::edgeuses(r, e, None, None) else {
            return Ok(false);
        };
        if uses.len() < 4 || uses.len() % 4 != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `smlib_shim::finite`.
pub(crate) fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// The C API's `num` (`%.17g`) for log lines: the shortest form that reads back, a whole
/// number without its `.0` (`1`, `0.25`, `1e200`).
pub(crate) fn num(x: f64) -> String {
    let s = format!("{x:?}");
    s.strip_suffix(".0")
        .map_or_else(|| s.clone(), ToString::to_string)
}

/// The C API's `vec` for log lines.
pub(crate) fn vec(v: &[f64]) -> String {
    let parts: Vec<String> = v.iter().map(|x| num(*x)).collect();
    format!("[{}]", parts.join(","))
}
