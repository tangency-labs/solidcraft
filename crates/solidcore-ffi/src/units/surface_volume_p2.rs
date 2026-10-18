//! FFI wrappers for unit `surface.volume_p2`.
//!
//! Shim: `harness/cpp/shim/surface_volume_p2.cpp` (`sc_surface_volume_p2_eval`).
//! Upstream: `source/SMLib/src/SmVolume.cpp`, `source/SMLib/inc/SmVolume.h`.
//!
//! Volume ops start with a volume block: `n` maps (`1..=MAX_MAPS`), each `kind data`
//! then (except for a twist) `has_orient [O(12)]`. Kinds: `0` a plain `SmVolume`
//! (no data), `1` an `SmTransform(ctx, origin, X, Y, &Z)` from twelve numbers, `2`
//! `SmTwistVolume(ctx, origin, axis, mid, period, min)` (eleven numbers; the twist
//! frame is its orient map), `3` an `SmBSplineVolume` from `l m n p q r`, the knots
//! `U V W` and the control points `x y z w` (`w` index fastest), `4` a default
//! `SmUnbendVolume`, `5` `SmUnbendVolume(ctx, center, axis, mid, neutral, tol)` (eleven
//! numbers; its frame is its orient map, so no orient flag follows), `6` a default
//! `SmBendVolume`, `7` `SmBendVolume(ctx, center, axis, mid, neutral, tol)` (eleven
//! numbers, no orient flag). `O` becomes the orient map through `SetOrientMap(new
//! SmTransform, 2)`; map `i > 0` is appended with `AddNextMap(map, 2)`. A box is `min
//! max` (`SmExtent3d(min, max)`), a pseudo box `X Y Z a0 b0 a1 b1 a2 b2`
//! (`SmPseudoBox(X, Y, Z, SmExtent1d(a, b) x3)`). Every output starts with the status
//! (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]          | upstream                     | parameters                         | outputs |
//! |-----------------|------------------------------|------------------------------------|---------|
//! | `PtInParam`     | `IsPointInParamDomain`       | `P comp`                           | flag |
//! | `LineInInSpace` | `IsLineInInSpaceDomain`      | `A B comp`                         | flag, both parameter points |
//! | `BoxInParam`    | `IsBoxInParamDomain`         | box `comp`                         | flag |
//! | `PBoxInInSpace` | `IsPseudoBoxInInSpaceDomain` | pseudo box `comp`                  | flag |
//! | `EvalBBox`      | `EvaluateBoundingBox`        | box `has_ppb [ppb] ob opb ib ipb comp` | requested `box`/`pbox` outputs in that order |
//! | `MapPBox`       | `MapPseudoBox`               | pseudo box `want_box want_pb comp trim` | requested outputs |
//! | `EvalPoint`     | `EvaluatePoint`              | `P comp`                           | point |
//! | `Eval1st`       | `Evaluate1stDerivatives`     | `P l0 l1 l2 nz comp`               | `P Du Dv Dw` |
//! | `InvEval`       | `InvEvaluatePoint`           | `P has_guess [G] has_dom [box] comp` | inverse record |
//! | `InvMap`        | `InvMapPoint`                | `P has_guess [G] has_dom [pseudo box] comp` | inverse record |
//! | `Global`        | `GlobalPointSolve`           | `P has_dom [box] comp`             | solutions record |
//! | `HasDisc`       | `HasDiscontinuities` of a `SetUnaligned` set | `want geometric`   | flag, discontinuities |
//! | `BBoxSimple`    | `EvaluateBoundingBoxSimple`  | box `has_ppb [ppb] want_box want_pb` | requested outputs |
//! | `PBoxSimple`    | `EvaluatePseudoBoxSimple`    | pseudo box `want_box want_pb`      | requested outputs |
//! | `InvEvalSimple` | `InvEvaluatePointSimple`     | `P has_guess [G] has_dom [box]`    | inverse record |
//! | `GlobalSimple`  | `GlobalPointSolveSimple`     | `P has_dom [box]`                  | solutions record |
//! | `LocalSimple`   | `LocalPointSolveSimple`      | `P G has_dom [box]`                | `found` solution |
//! | `ReadDb` (no volume block) | `SmVolume::ReadFromDB(type, ASCII data, ctx, NULL, 30)` | `type` | `has_volume [state]` |
//!
//! Box outputs are `min max`, pseudo boxes `basis[3] (min max)[3]`. An inverse record
//! is `success n points[n] n gaps[n]`; a solutions record `found n` then per solution
//! `type n_vars value u v w n_objects`. A discontinuity set is `has min` then per
//! direction `unaligned min n_params`. A state is `n` then per map `type [matrix(16) |
//! period min | neutral tol] has_orient [O(16)] has_inv [I(16)]`. Outputs after a failed
//! status are omitted.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour (see
//! [`check`]): finite values of at most [`MAX_COORD`], B-spline records within
//! [`MAX_CP`] / [`MAX_DEGREE`] with non-decreasing knots, no call path that hands a
//! transform's `EvaluateBoundingBoxSimple` a null parameter pseudo box or a twist's an
//! unbounded box without a box output, no call path that may hand an unbend's
//! `EvaluateBoundingBoxSimple` a bounded box with a side shorter than [`MIN_SIDE`]
//! (see [`check`]), and `ReadDb` text in a strict token grammar without the reader's
//! null or uninitialised cases.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Most maps in a volume chain.
pub const MAX_MAPS: usize = 6;
/// Most control points per direction of a B-spline map.
pub const MAX_CP: usize = 6;
/// Highest degree per direction of a B-spline map.
pub const MAX_DEGREE: usize = 4;
/// Largest value magnitude.
pub const MAX_COORD: f64 = 1e8;
/// Most nested volume reads in a `ReadDb` text (the head included), as
/// `solidcore::topology::database_io::MAX_READ_NESTING`.
pub const MAX_READ_NESTING: usize = 64;
/// Longest database text.
pub const MAX_DATA: usize = 1 << 16;
/// Shortest side of a bounded box handed to an unbend's `EvaluateBoundingBoxSimple`:
/// `SmLine::CreateLineSegment` fails on a zero-length box edge and upstream then uses
/// a null or deleted line (as in unit `surface.unbend_volume`).
pub const MIN_SIDE: f64 = 1e-9;

/// `SmVolume_TYPE`.
pub const VOLUME_TYPE: i64 = 22001;
/// `SmBSplineVolume_TYPE`.
pub const B_SPLINE_VOLUME_TYPE: i64 = 22004;
/// `SmBendVolume_TYPE`.
pub const BEND_VOLUME_TYPE: i64 = 22006;
/// `SmTransform_TYPE`.
pub const TRANSFORM_TYPE: i64 = 22008;
/// `SmUnbendVolume_TYPE`.
pub const UNBEND_VOLUME_TYPE: i64 = 22010;
/// `SmTwistVolume_TYPE`.
pub const TWIST_VOLUME_TYPE: i64 = 22011;

/// `SM_INFINITE_PARAMETER` and its square root, the unbounded interval markers.
const INFINITE_MARKERS: [f64; 4] = [1_234_567.0, -1_234_567.0, 1111.111, -1111.111];

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    PtInParam,
    LineInInSpace,
    BoxInParam,
    PBoxInInSpace,
    EvalBBox,
    MapPBox,
    EvalPoint,
    Eval1st,
    InvEval,
    InvMap,
    Global,
    HasDisc,
    BBoxSimple,
    PBoxSimple,
    InvEvalSimple,
    GlobalSimple,
    LocalSimple,
    ReadDb,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::PtInParam => 0,
            Op::LineInInSpace => 1,
            Op::BoxInParam => 2,
            Op::PBoxInInSpace => 3,
            Op::EvalBBox => 4,
            Op::MapPBox => 5,
            Op::EvalPoint => 6,
            Op::Eval1st => 7,
            Op::InvEval => 8,
            Op::InvMap => 9,
            Op::Global => 10,
            Op::HasDisc => 11,
            Op::BBoxSimple => 12,
            Op::PBoxSimple => 13,
            Op::InvEvalSimple => 14,
            Op::GlobalSimple => 15,
            Op::LocalSimple => 16,
            Op::ReadDb => 20,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_volume_p2_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            data: *const u8,
            data_len: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The class of one map of a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Base,
    Transform,
    Twist,
    BSpline,
    Unbend,
    Bend,
}

impl Kind {
    fn is_unbend(self) -> bool {
        self == Kind::Unbend
    }
}

/// One map of a chain: its class and whether it has an orient map.
#[derive(Debug, Clone, Copy)]
struct Map {
    kind: Kind,
    orient: bool,
}

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("value not finite or too large"));
        }
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v < 0.0 || v.fract() != 0.0 || v > max as f64 {
            return Err(bad(format!("index {v} outside 0..={max}")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.num()? != 0.0)
    }

    fn nums(&mut self, n: usize) -> Result<(), FfiError> {
        for _ in 0..n {
            self.num()?;
        }
        Ok(())
    }

    /// A box; returns whether it has any bounded end (`AnyBounds`).
    fn ext(&mut self) -> Result<bool, FfiError> {
        Ok(self.ext_flat()?.0)
    }

    /// A box; returns whether it has any bounded end (`AnyBounds`) and whether it is
    /// bounded with a side shorter than [`MIN_SIDE`] (an unbend's box edges then
    /// include a zero-length line).
    fn ext_flat(&mut self) -> Result<(bool, bool), FfiError> {
        let mut v = [0.0; 6];
        for x in &mut v {
            *x = self.num()?;
        }
        let inf = |x: f64| INFINITE_MARKERS.contains(&x);
        // SmExtent3d(min, max) only swaps; the markers are symmetric
        let (lo, hi) = v.split_at(3);
        let any_bounds = lo.iter().zip(hi).any(|(&a, &b)| !(inf(a) && inf(b)));
        let bounded = !v.iter().any(|&x| inf(x));
        let flat = lo.iter().zip(hi).any(|(&a, &b)| (b - a).abs() < MIN_SIDE);
        Ok((any_bounds, bounded && flat))
    }

    fn pbox(&mut self) -> Result<(), FfiError> {
        self.nums(15)
    }

    fn bspline(&mut self) -> Result<(), FfiError> {
        let mut hi = [0usize; 3];
        for h in &mut hi {
            *h = self.index(MAX_CP - 1)?;
        }
        let mut deg = [0usize; 3];
        for d in &mut deg {
            *d = self.index(MAX_DEGREE)?;
        }
        for (h, d) in hi.iter().zip(deg) {
            let mut prev = f64::NEG_INFINITY;
            for _ in 0..h + d + 2 {
                let k = self.num()?;
                if k < prev {
                    return Err(bad("knots must be non-decreasing"));
                }
                prev = k;
            }
        }
        self.nums(4 * (hi[0] + 1) * (hi[1] + 1) * (hi[2] + 1))
    }

    fn volume(&mut self) -> Result<Vec<Map>, FfiError> {
        let n = self.index(MAX_MAPS)?;
        if n == 0 {
            return Err(bad("a volume needs at least one map"));
        }
        let mut chain = Vec::with_capacity(n);
        for _ in 0..n {
            let code = self.index(7)?;
            let kind = match code {
                0 => Kind::Base,
                1 => {
                    self.nums(12)?;
                    Kind::Transform
                }
                2 => {
                    self.nums(11)?;
                    Kind::Twist
                }
                3 => {
                    self.bspline()?;
                    Kind::BSpline
                }
                4 => Kind::Unbend,
                5 => {
                    self.nums(11)?;
                    Kind::Unbend
                }
                6 => Kind::Bend,
                _ => {
                    self.nums(11)?;
                    Kind::Bend
                }
            };
            // a twist's and a component unbend's or bend's frame is their orient map
            let orient = if kind == Kind::Twist || code == 5 || code == 7 {
                true
            } else {
                let o = self.flag()?;
                if o {
                    self.nums(12)?;
                }
                o
            };
            chain.push(Map { kind, orient });
        }
        Ok(chain)
    }
}

// ---- undefined call paths -------------------------------------------------------------

/// Whether the domain check of a line or box (`IsLineInParamDomain` with
/// compounding, as `IsBoxInParamDomain` and `IsPseudoBoxInInSpaceDomain` run it) on
/// map `i` passes: `Some(false)` when a later map is a plain `SmVolume` (its
/// `IsCurveInParamDomainSimple` is `FALSE`), `None` when a B-spline map decides it
/// from data.
fn domain_check(chain: &[Map], i: usize) -> Option<bool> {
    if chain.get(i).is_none_or(|m| m.kind == Kind::BSpline) {
        return None;
    }
    for m in chain.iter().skip(i + 1) {
        match m.kind {
            Kind::Base => return Some(false),
            Kind::BSpline => return None,
            _ => {}
        }
    }
    Some(true)
}

/// Which box outputs a call requests.
#[derive(Debug, Clone, Copy)]
struct Wants {
    /// The axis-aligned box.
    bx: bool,
    /// The pseudo box.
    pb: bool,
}

impl Wants {
    fn any(self) -> bool {
        self.bx || self.pb
    }
}

/// Whether `EvaluateBoundingBox` on map `i` with the out-space outputs `out` and
/// the in-space outputs `inside` may hand a transform's `EvaluateBoundingBoxSimple`
/// a null parameter pseudo box (data-dependent domain checks count as passing).
fn ebb_undefined(
    chain: &[Map],
    i: usize,
    has_ppb: bool,
    (out, inside): (Wants, Wants),
    comp: bool,
) -> bool {
    let Some(m) = chain.get(i) else {
        return false;
    };
    if (!out.any() && !inside.any()) || domain_check(chain, i) == Some(false) {
        return false;
    }
    if inside.any() && m.orient && !has_ppb {
        return true;
    }
    if out.any() {
        if m.kind == Kind::Transform && !has_ppb {
            return true;
        }
        if comp && i + 1 < chain.len() {
            let next_out = Wants {
                bx: out.bx,
                pb: true,
            };
            return mpb_undefined(chain, i + 1, next_out, true, false);
        }
    }
    false
}

/// Whether `MapPseudoBox` on map `i` with the outputs `outs` may reach a
/// transform's `EvaluateBoundingBoxSimple` with a null parameter pseudo box.
fn mpb_undefined(chain: &[Map], i: usize, outs: Wants, comp: bool, trim: bool) -> bool {
    let Some(m) = chain.get(i) else {
        return false;
    };
    if !trim && domain_check(chain, i) == Some(false) {
        return false;
    }
    let next = comp && i + 1 < chain.len();
    let has_this = outs.pb || next;
    let here = if m.orient {
        m.kind == Kind::Transform && !has_this
    } else {
        trim && m.kind == Kind::Transform && (outs.bx || has_this)
    };
    let next_outs = Wants {
        bx: outs.bx,
        pb: true,
    };
    here || (next && mpb_undefined(chain, i + 1, next_outs, comp, trim))
}

/// Whether `InvMapPoint` on map `i` may reach an undefined box mapping.
fn inv_map_undefined(chain: &[Map], i: usize, has_dom: bool, comp: bool) -> bool {
    if !comp || i + 1 >= chain.len() {
        return false;
    }
    (has_dom
        && mpb_undefined(
            chain,
            i,
            Wants {
                bx: false,
                pb: true,
            },
            false,
            true,
        ))
        || inv_map_undefined(chain, i + 1, has_dom, true)
}

/// Whether `InvEvaluatePoint` / `GlobalPointSolve` on map `0` may reach an undefined
/// box mapping.
fn inv_eval_undefined(chain: &[Map], has_dom: bool, comp: bool) -> bool {
    if !comp || chain.len() < 2 {
        return false;
    }
    let out = Wants { bx: true, pb: true };
    let inside = Wants {
        bx: false,
        pb: false,
    };
    (has_dom && ebb_undefined(chain, 0, false, (out, inside), false))
        || inv_map_undefined(chain, 1, has_dom, true)
}

/// Whether `EvaluateBoundingBox` on map `i` with the out-space outputs `out` and
/// the in-space outputs `inside` may reach an unbend's `EvaluateBoundingBoxSimple`
/// with a too-flat box: on map `i` itself with the caller's box (`flat`), on a later
/// map with a computed box (rejected whatever its size).
fn ebb_unbend(
    chain: &[Map],
    i: usize,
    (out, inside): (Wants, Wants),
    comp: bool,
    flat: bool,
) -> bool {
    let Some(m) = chain.get(i) else {
        return false;
    };
    if (!out.any() && !inside.any()) || domain_check(chain, i) == Some(false) {
        return false;
    }
    if out.any() {
        if m.kind.is_unbend() && flat {
            return true;
        }
        if comp && i + 1 < chain.len() {
            let next_out = Wants {
                bx: out.bx,
                pb: true,
            };
            return mpb_unbend(chain, i + 1, next_out, true, false);
        }
    }
    false
}

/// Whether `MapPseudoBox` on map `i` with the outputs `outs` may reach an unbend's
/// `EvaluateBoundingBoxSimple`. Its box there is computed from the in-space pseudo
/// box (sampled through the inverse orient map, or its axis-aligned extent when
/// trimming), so any such call is rejected.
fn mpb_unbend(chain: &[Map], i: usize, outs: Wants, comp: bool, trim: bool) -> bool {
    let Some(m) = chain.get(i) else {
        return false;
    };
    if !trim && domain_check(chain, i) == Some(false) {
        return false;
    }
    let next = comp && i + 1 < chain.len();
    let has_this = outs.pb || next;
    let here = m.kind.is_unbend() && (m.orient || (trim && (outs.bx || has_this)));
    let next_outs = Wants {
        bx: outs.bx,
        pb: true,
    };
    here || (next && mpb_unbend(chain, i + 1, next_outs, comp, trim))
}

/// Whether `InvMapPoint` on map `i` may reach an unbend's `EvaluateBoundingBoxSimple`.
fn inv_map_unbend(chain: &[Map], i: usize, has_dom: bool, comp: bool) -> bool {
    if !comp || i + 1 >= chain.len() {
        return false;
    }
    (has_dom
        && mpb_unbend(
            chain,
            i,
            Wants {
                bx: false,
                pb: true,
            },
            false,
            true,
        ))
        || inv_map_unbend(chain, i + 1, has_dom, true)
}

/// Whether `InvEvaluatePoint` / `GlobalPointSolve` on map `0` may reach an unbend's
/// `EvaluateBoundingBoxSimple` (`flat`: the caller's domain box is too flat).
fn inv_eval_unbend(chain: &[Map], has_dom: bool, comp: bool, flat: bool) -> bool {
    if !comp || chain.len() < 2 {
        return false;
    }
    let out = Wants { bx: true, pb: true };
    let inside = Wants {
        bx: false,
        pb: false,
    };
    (has_dom && ebb_unbend(chain, 0, (out, inside), false, flat))
        || inv_map_unbend(chain, 1, has_dom, true)
}

// ---- database text ----------------------------------------------------------------------

/// A strict tokenizer over the ASCII database text, modelling the `istream`
/// extractions and `GoToNextLine` of the readers on text it accepts.
struct Text<'a> {
    b: &'a [u8],
    at: usize,
    failed: bool,
}

/// What an extraction found.
enum Tok<'a> {
    /// The stream is at its end (or failed before): the value stays unchanged.
    End,
    /// A token.
    Word(&'a str),
}

impl<'a> Text<'a> {
    fn token(&mut self) -> Result<Tok<'a>, FfiError> {
        if self.failed {
            return Ok(Tok::End);
        }
        while self.b.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
        let start = self.at;
        while self
            .b
            .get(self.at)
            .is_some_and(|c| !c.is_ascii_whitespace())
        {
            self.at += 1;
        }
        if start == self.at {
            self.failed = true;
            return Ok(Tok::End);
        }
        let w = self
            .b
            .get(start..self.at)
            .and_then(|s| core::str::from_utf8(s).ok())
            .ok_or_else(|| bad("database text is not ASCII"))?;
        if w.len() > 200 {
            return Err(bad("database token too long"));
        }
        Ok(Tok::Word(w))
    }

    fn next_line(&mut self) {
        if self.failed {
            return;
        }
        while let Some(&c) = self.b.get(self.at) {
            self.at += 1;
            if c == b'\n' {
                break;
            }
        }
    }

    fn integer(&mut self) -> Result<Option<i64>, FfiError> {
        match self.token()? {
            Tok::End => Ok(None),
            Tok::Word(w) => {
                let digits = w.strip_prefix(['+', '-']).unwrap_or(w);
                if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(bad("database integer token malformed"));
                }
                let v: i64 = w
                    .parse()
                    .map_err(|_| bad("database integer out of range"))?;
                if v.unsigned_abs() > i32::MAX as u64 {
                    return Err(bad("database integer out of range"));
                }
                Ok(Some(v))
            }
        }
    }

    fn double(&mut self) -> Result<(), FfiError> {
        if let Tok::Word(w) = self.token()? {
            let body = w.strip_prefix(['+', '-']).unwrap_or(w);
            let (mantissa, exp) = match body.split_once(['e', 'E']) {
                Some((m, e)) => (m, Some(e)),
                None => (body, None),
            };
            let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
            let digits = |s: &str| s.bytes().all(|c| c.is_ascii_digit());
            let exp_ok = exp.is_none_or(|e| {
                let e = e.strip_prefix(['+', '-']).unwrap_or(e);
                !e.is_empty() && digits(e)
            });
            if (int.is_empty() && frac.is_empty()) || !digits(int) || !digits(frac) || !exp_ok {
                return Err(bad("database number token malformed"));
            }
            let v: f64 = w.parse().map_err(|_| bad("database number malformed"))?;
            if !v.is_finite() || v.abs() > 1e300 {
                return Err(bad("database number out of range"));
            }
        }
        Ok(())
    }

    /// `>> word >> word >> long; GoToNextLine` after a "has map" flag.
    fn map_type(&mut self) -> Result<i64, FfiError> {
        self.token()?;
        self.token()?;
        let t = self
            .integer()?
            .ok_or_else(|| bad("map type missing (uninitialised type upstream)"))?;
        self.next_line();
        Ok(t)
    }

    /// The derived data and base data of a volume of type `t`; `Ok(false)` when the
    /// read fails without reading anything.
    fn volume(&mut self, t: i64, depth: usize) -> Result<bool, FfiError> {
        // the Rust reader's documented bound (`MAX_READ_NESTING`, 64 nested reads):
        // a deeper stream is a deliberate deviation (Rust fails, C++ recurses on)
        if depth >= MAX_READ_NESTING {
            return Err(bad("database volumes nested past the Rust reader's bound"));
        }
        let (lines, per_line) = match t {
            TRANSFORM_TYPE => (4, 4),
            TWIST_VOLUME_TYPE | UNBEND_VOLUME_TYPE | BEND_VOLUME_TYPE => (2, 1),
            VOLUME_TYPE => return Err(bad("SmVolume_TYPE has no volume to fill upstream")),
            _ => return Ok(false),
        };
        for _ in 0..lines {
            for _ in 0..per_line {
                self.double()?;
            }
            self.next_line();
        }
        // base data: next map
        if self.integer()?.is_some_and(|f| f != 0) {
            self.next_line();
            let nt = self.map_type()?;
            self.volume(nt, depth + 1)?;
        } else {
            self.next_line();
        }
        // orient map
        if self.integer()?.is_some_and(|f| f != 0) {
            self.next_line();
            let ot = self.map_type()?;
            if ot != TRANSFORM_TYPE {
                return Err(bad(
                    "orient map not a transform (null or mistyped upstream)",
                ));
            }
            self.volume(ot, depth + 1)?;
        } else {
            self.next_line();
        }
        Ok(true)
    }
}

/// Checks the ASCII text of a `ReadFromDB` of type `t` (here `ReadDb`; also the
/// `ReadDb` guards of `surface.bend_volume` and `surface.unbend_volume`, whose
/// readers end in the base read): a strict token grammar without the reader's null
/// or uninitialised cases.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] naming the first problem.
pub fn check_text(t: i64, data: &[u8]) -> Result<(), FfiError> {
    if data.len() > MAX_DATA {
        return Err(bad("database text too long"));
    }
    if data.split(|&c| c == b'\n').any(|l| l.len() > 200) {
        return Err(bad("database line too long"));
    }
    let mut text = Text {
        b: data,
        at: 0,
        failed: false,
    };
    text.volume(t, 0).map(|_| ())
}

/// Checks that `input` (and `data` for [`Op::ReadDb`]) is a well-formed record for
/// `op` that upstream handles without undefined behaviour (see the module docs).
/// A call path whose undefined step depends on a B-spline map's domain test is
/// rejected as if the test passed. An unbend's `EvaluateBoundingBoxSimple` is
/// allowed only on the caller's own box (`EvalBBox`, `BBoxSimple`, and the domain box
/// of `InvEval` / `Global` on map `0`) when that box is unbounded or has every side
/// at least [`MIN_SIDE`]; a call path that hands it a computed box (a later map, or
/// `MapPseudoBox`) is rejected whatever that box's size.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] naming the first problem.
pub fn check(op: Op, input: &[f64], data: &[u8]) -> Result<(), FfiError> {
    let mut r = Reader::new(input);
    if op == Op::ReadDb {
        let t = r.num()?;
        if t.fract() != 0.0 {
            return Err(bad("type must be an integer"));
        }
        check_text(t as i64, data)?;
    } else {
        let chain = r.volume()?;
        let undefined = match op {
            Op::PtInParam | Op::EvalPoint => {
                r.nums(3)?;
                r.flag()?;
                false
            }
            Op::LineInInSpace => {
                r.nums(6)?;
                r.flag()?;
                false
            }
            Op::BoxInParam => {
                r.ext()?;
                r.flag()?;
                false
            }
            Op::PBoxInInSpace => {
                r.pbox()?;
                r.flag()?;
                false
            }
            Op::EvalBBox => {
                let (_, flat) = r.ext_flat()?;
                let has_ppb = r.flag()?;
                if has_ppb {
                    r.pbox()?;
                }
                let out = Wants {
                    bx: r.flag()?,
                    pb: r.flag()?,
                };
                let inside = Wants {
                    bx: r.flag()?,
                    pb: r.flag()?,
                };
                let comp = r.flag()?;
                ebb_undefined(&chain, 0, has_ppb, (out, inside), comp)
                    || ebb_unbend(&chain, 0, (out, inside), comp, flat)
            }
            Op::MapPBox => {
                r.pbox()?;
                let outs = Wants {
                    bx: r.flag()?,
                    pb: r.flag()?,
                };
                let (comp, trim) = (r.flag()?, r.flag()?);
                mpb_undefined(&chain, 0, outs, comp, trim)
                    || mpb_unbend(&chain, 0, outs, comp, trim)
            }
            Op::Eval1st => {
                r.nums(3)?;
                r.nums(5)?;
                false
            }
            Op::InvEval | Op::InvMap => {
                r.nums(3)?;
                if r.flag()? {
                    r.nums(3)?;
                }
                let has_dom = r.flag()?;
                let mut flat = false;
                if has_dom {
                    if op == Op::InvEval {
                        flat = r.ext_flat()?.1;
                    } else {
                        r.pbox()?;
                    }
                }
                let comp = r.flag()?;
                if op == Op::InvEval {
                    inv_eval_undefined(&chain, has_dom, comp)
                        || inv_eval_unbend(&chain, has_dom, comp, flat)
                } else {
                    inv_map_undefined(&chain, 0, has_dom, comp)
                        || inv_map_unbend(&chain, 0, has_dom, comp)
                }
            }
            Op::Global => {
                r.nums(3)?;
                let has_dom = r.flag()?;
                let mut flat = false;
                if has_dom {
                    flat = r.ext_flat()?.1;
                }
                let comp = r.flag()?;
                inv_eval_undefined(&chain, has_dom, comp)
                    || inv_eval_unbend(&chain, has_dom, comp, flat)
            }
            Op::HasDisc => {
                r.flag()?;
                r.flag()?;
                false
            }
            Op::BBoxSimple => {
                let (any_bounds, flat) = r.ext_flat()?;
                let has_ppb = r.flag()?;
                if has_ppb {
                    r.pbox()?;
                }
                let (want_box, want_pb) = (r.flag()?, r.flag()?);
                let kind = chain.first().map(|m| m.kind);
                (want_box || want_pb)
                    && ((kind == Some(Kind::Transform) && !has_ppb)
                        || (kind == Some(Kind::Twist) && !want_box && !any_bounds)
                        || (kind == Some(Kind::Unbend) && flat))
            }
            Op::PBoxSimple => {
                r.pbox()?;
                r.flag()?;
                r.flag()?;
                false
            }
            Op::InvEvalSimple | Op::GlobalSimple => {
                r.nums(3)?;
                if op == Op::InvEvalSimple && r.flag()? {
                    r.nums(3)?;
                }
                if r.flag()? {
                    r.ext()?;
                }
                false
            }
            Op::LocalSimple => {
                r.nums(6)?;
                if r.flag()? {
                    r.ext()?;
                }
                false
            }
            Op::ReadDb => false,
        };
        if undefined {
            return Err(bad(
                "call path reaches an undefined box mapping upstream (null pseudo box)",
            ));
        }
    }
    if r.at != input.len() {
        return Err(bad(format!(
            "input has {} values, op reads {}",
            input.len(),
            r.at
        )));
    }
    Ok(())
}

/// Runs `op` of the shim on `input` (and the database text `data` for
/// [`Op::ReadDb`]; layouts in the module docs) and returns its outputs, starting
/// with the status.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`check`] rejects; the shim's status.
pub fn eval(op: Op, input: &[f64], data: &[u8]) -> Result<Vec<f64>, FfiError> {
    check(op, input, data)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op`, data for data.len() reads (both validated above: finite
        // values, record sizes within the limits, no undefined call path or reader
        // case); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_surface_volume_p2_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                data.as_ptr(),
                data.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
