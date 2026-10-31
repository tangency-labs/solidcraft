//! Safe native fixtures and `SmFeatureExecutive` for `blend.feature_executive`.
//!
//! Shim: `harness/cpp/shim/blend_feature_executive.cpp`. Upstream: `SmFeatureExecutive.cpp`.
//! The B-rep block is the shared `topology.edgeuse_p1` block (an upstream `.smb` file, a
//! wire, a face or the wire network). [`eval`] builds an `SmFeatureExecutive` over the
//! B-rep and runs a script on it:
//!
//! ```text
//! n_attributes
//!   kind index id behavior     kind 0 face, 1 edge: SmLongAttribute(id, 7, behavior) on it
//! n_features
//!   kind base_rb feat_rb ...   kind 0: n_faces face*            AddFeature(faces, base_rb, feat_rb)
//!                              kind 1: face n_edges edge*       AddFeature(face, edges, base_rb, feat_rb)
//!                              kind 2: face n_loops             AddFeature(face, loops) (base_rb and feat_rb unused)
//!                                      (base_rb feat_rb n_edges edge*)*   one SmFeatureLoop each
//! n_ops
//!   op ...                     0 DoDefeature, 1 t SetBaseRBType, 2 t SetFeatureRBType,
//!                              3 d SetBaseExtDist, 4 d SetFeatureExtDist, 5 RestoreFeatures
//! ```
//!
//! Faces and edges are indices of the fixture's `GetFaces` / `GetEdges` before the
//! script; rebuild types are `SmRebuildBrepType` values 0..=3.
//!
//! It returns `record_len record*`, then the base B-rep and, per entry of
//! `GetFeatureBreps`, `0` (none) or `1` and that B-rep; a B-rep is `len geometry*
//! graph_len graph*` (edge-curve and face-surface samples, then the raw graph export). The
//! record is one flag per `AddFeature` (a feature was made), per op `DoDefeature`'s
//! `status base_is_input n_feature_breps` or `RestoreFeatures`' status, then the
//! features' states, then per attribute the numbers of faces and edges carrying its id in
//! the base B-rep and in each feature B-rep (`-1 -1` for none). A failed fixture build returns `-999 status`; an index past the
//! fixture's faces or edges `-998`; a `RestoreFeatures` with two or more restorable
//! features `-997` (upstream then deletes a feature it keeps in its list); a `DoDefeature`
//! whose features keep a loop edge an applied patch's stitch deleted `-996` (upstream
//! reads it); a `DoDefeature` that may extend a surface without an extension `-995`
//! (upstream dereferences the NULL surface). The script runs in a fresh child process
//! ([`crate::isolate::fresh_process_within`] [`CHILD_SECONDS`]): any other native crash,
//! or a script still running then, is reported as
//! [`FfiError::Status`]`(`[`SC_ERR_CHILD_CRASHED`](crate::SC_ERR_CHILD_CRASHED)`)`.

use super::topology_edgeuse_p1::{brep_block, Reader};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;

mod sys {
    extern "C" {
        pub fn sc_blend_feature_executive_eval(
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            script: *const f64,
            ns: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The child's run time in seconds: the slowest fixture script takes a few seconds, and
/// some scripts never end upstream (reported as a crash).
pub const CHILD_SECONDS: u32 = 60;

/// Largest count (features, faces, edges, loops, ops) a script may give.
const MAX_COUNT: f64 = 100_000.0;

struct Script<'a> {
    s: &'a [f64],
    at: usize,
}

impl Script<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = *self
            .s
            .get(self.at)
            .ok_or_else(|| FfiError::InvalidArgument("short script".into()))?;
        self.at += 1;
        Ok(v)
    }
    /// A non-negative integer up to [`MAX_COUNT`].
    fn count(&mut self) -> Result<usize, FfiError> {
        let v = self.num()?;
        if !(0.0..=MAX_COUNT).contains(&v) || v.fract() != 0.0 {
            return Err(FfiError::InvalidArgument("script count or index".into()));
        }
        Ok(v as usize)
    }
    /// An `SmRebuildBrepType` value (0..=3; others are outside the enum's range).
    fn rebuild_type(&mut self) -> Result<(), FfiError> {
        if self.count()? > 3 {
            return Err(FfiError::InvalidArgument("rebuild type".into()));
        }
        Ok(())
    }
    fn indices(&mut self) -> Result<(), FfiError> {
        let n = self.count()?;
        for _ in 0..n {
            self.count()?;
        }
        Ok(())
    }
}

/// Checks the script's shape (see the module docs); index bounds are the shim's (`-998`).
fn validate(script: &[f64]) -> Result<(), FfiError> {
    let mut s = Script { s: script, at: 0 };
    let n_attributes = s.count()?;
    for _ in 0..n_attributes {
        if s.count()? > 1 {
            return Err(FfiError::InvalidArgument("attribute kind".into()));
        }
        s.count()?;
        let id = s.num()?;
        if !(0.0..=f64::from(u32::MAX)).contains(&id) || id.fract() != 0.0 {
            return Err(FfiError::InvalidArgument("attribute id".into()));
        }
        if s.count()? > 4 {
            return Err(FfiError::InvalidArgument("attribute behaviour".into()));
        }
    }
    let n_features = s.count()?;
    for _ in 0..n_features {
        let kind = s.count()?;
        s.rebuild_type()?;
        s.rebuild_type()?;
        match kind {
            0 => s.indices()?,
            1 => {
                s.count()?;
                s.indices()?;
            }
            2 => {
                s.count()?;
                let n_loops = s.count()?;
                for _ in 0..n_loops {
                    s.rebuild_type()?;
                    s.rebuild_type()?;
                    s.indices()?;
                }
            }
            _ => return Err(FfiError::InvalidArgument("feature kind".into())),
        }
    }
    let n_ops = s.count()?;
    for _ in 0..n_ops {
        match s.count()? {
            0 | 5 => {}
            1 | 2 => s.rebuild_type()?,
            3 | 4 => {
                if !s.num()?.is_finite() {
                    return Err(FfiError::InvalidArgument("extension distance".into()));
                }
            }
            _ => return Err(FfiError::InvalidArgument("op".into())),
        }
    }
    if s.at != script.len() {
        return Err(FfiError::InvalidArgument("trailing script".into()));
    }
    Ok(())
}

/// Runs `script` on an `SmFeatureExecutive` over the B-rep that `block` builds, in a fresh
/// child process (see the module docs).
pub fn eval(path: &str, block: &[f64], script: &[f64]) -> Result<Vec<f64>, FfiError> {
    let bad = |m: &str| FfiError::InvalidArgument(m.into());
    let mut rd = Reader {
        input: block,
        at: 0,
    };
    brep_block(&mut rd, path)?;
    rd.done()?;
    validate(script)?;
    if script.len() < 2 {
        return Err(bad("short script"));
    }
    let path = CString::new(path).map_err(|_| bad("path"))?;
    crate::isolate::fresh_process_within(CHILD_SECONDS, || run(&path, block, script))
}

fn run(path: &CString, block: &[f64], script: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(1 << 22, |out, len| {
        // SAFETY: the validated block and script stay live through the call; the output
        // and length pointers come from call_with_buffer.
        unsafe {
            sys::sc_blend_feature_executive_eval(
                path.as_ptr(),
                block.as_ptr(),
                block.len() as i64,
                script.as_ptr(),
                script.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
