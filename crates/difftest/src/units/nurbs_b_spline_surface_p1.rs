//! Runner for unit `nurbs.b_spline_surface_p1`: the `SmBSplineSurface` object (an NLib
//! surface with STEP flags), its constructors and copies, the bilinear, canonical, cone
//! patch and linear sweep factories, queries (counts, domain, knots, Greville abscissae,
//! canonical data, control net), bounding boxes, parallelism, transformation, STEP
//! inversion and reading from a database stream.
//!
//! C++: `solidcore_ffi::units::nurbs_b_spline_surface_p1` (upstream
//! `source/SMLib/src/SmBSplineSurface.cpp`). Rust: `solidcore::nurbs::b_spline_surface`.
//!
//! A case is `this` (how the surface the op runs on is built: `ctor` `empty`,
//! `fast_owned`, `fast_borrowed`, `data`, `gw_copy` or `copy`, with `oob`
//! (out-of-bounds evaluation), `form` and an inline or `.sms` surface), absent for the
//! static factories, plus `op` and its parameters. Both sides produce the same flat
//! record, split into keys by [`to_output`]: `status` (`0` = `SM_SUCCESS`, else the
//! `SmStatus`: `1001` `SM_ERR`, `1006` `SM_ERR_NULL_POINTER`, `1007`
//! `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot
//! produce), the op's outputs on success, the state of a surface the op created
//! (`new_*`) and the state of `this` after the op (`this_*`). A state is `flags`
//! (`has_nurb oob borrowed knot_type form`) and, with a NURB, `dims` (`n m p q r s`),
//! `cpts` (x y z w as stored), `u` and `v`.
//!
//! Cases whose Rust side reaches an unported `SmSurface` method (`IsKindOf` for other
//! types, hence `CalculateBoundingBox` and `IsParallelToVector`; `CreateOffsetSurface`,
//! `IsDegenerateCurve`, the point solves of `STEPInversion`) are deferred. Every run
//! has at least [`MIN_GENERATED`] generated cases.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::kernel::Kernel;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::polar_box::PolarBox;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, KnotType, NurbCircleParam};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, BoundingBoxRequest, CanonicalSurfaceData, SurfParamType,
};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::nurbs::srf_basic::Surface;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::nurbs_b_spline_surface_p1::{self as ffi, Ctor, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nurbs.b_spline_surface_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// An inline surface: control net as stored (`[x, y, z, w]`, rows in u), degrees,
/// knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// A surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    /// Inline data.
    Inline(InlineSrf),
}

/// An inline curve: control points as stored, degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineCrv {
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// How `this` is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CtorIn {
    Empty,
    FastOwned,
    FastBorrowed,
    Data,
    GwCopy,
    Copy,
}

impl CtorIn {
    fn ffi(self) -> Ctor {
        match self {
            CtorIn::Empty => Ctor::Empty,
            CtorIn::FastOwned => Ctor::FastOwned,
            CtorIn::FastBorrowed => Ctor::FastBorrowed,
            CtorIn::Data => Ctor::Data,
            CtorIn::GwCopy => Ctor::GwCopy,
            CtorIn::Copy => Ctor::Copy,
        }
    }
}

/// A surface block.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct This {
    ctor: CtorIn,
    #[serde(default)]
    oob: bool,
    #[serde(default = "default_form")]
    form: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    srf: Option<SrfIn>,
}

fn default_form() -> u8 {
    BSplineSurfaceForm::Unspecified as u8
}

/// A frame `[origin, x axis, y axis]` (`SetCanonical`).
type Frame = [[f64; 3]; 3];

/// A database stream: an upstream text file, or a header and values the runner
/// writes as ASCII lines or native binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum DbIn {
    /// An upstream text file (absolute path after fixture expansion), read as is.
    File { file: String },
    /// Header and values.
    Data(DbData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbData {
    n: i64,
    m: i64,
    p: i16,
    q: i16,
    rat: i16,
    #[serde(default = "one")]
    kt: u64,
    #[serde(default = "nine")]
    form: u64,
    /// Control points (3 or 4 values each), u knots, v knots, in reading order.
    vals: Vec<f64>,
}

fn one() -> u64 {
    1
}

fn nine() -> u64 {
    9
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    State,
    Counts,
    Domain,
    Knots {
        v_dir: bool,
        #[serde(default)]
        ivl: Option<[f64; 2]>,
    },
    Greville {
        v_dir: bool,
        index: usize,
    },
    GetCanonical,
    Net,
    ControlPoints,
    Parallel {
        vec: [f64; 3],
    },
    BBox {
        domain: [f64; 4],
        normal: bool,
        pseudo: bool,
        polar: bool,
        #[serde(default)]
        pseudo_guess: Option<Frame>,
        #[serde(default)]
        polar_guess: Option<Vec<[f64; 3]>>,
        expand: bool,
    },
    Transform {
        frame: Frame,
        #[serde(default)]
        scale: Option<[f64; 3]>,
    },
    Eq {
        other: This,
    },
    IsKindOf {
        t: i64,
    },
    Type,
    MakeNurb,
    Copy,
    Offset {
        dist: f64,
        tol: f64,
    },
    DegenCurve {
        angle: f64,
        #[serde(default)]
        domain: Option<[f64; 4]>,
        tol: f64,
    },
    StepInv {
        domain: [f64; 4],
        point: [f64; 3],
        tol: f64,
        #[serde(default)]
        guess: Option<[f64; 2]>,
    },
    SetForm {
        form: u8,
    },
    Bilinear {
        p00: [f64; 3],
        p10: [f64; 3],
        p01: [f64; 3],
        p11: [f64; 3],
    },
    Canonical {
        ud: usize,
        vd: usize,
        cps: Vec<[f64; 3]>,
        form: u8,
        uk: Vec<f64>,
        um: Vec<usize>,
        vk: Vec<f64>,
        vm: Vec<usize>,
        kt: u8,
        #[serde(default)]
        weights: Option<Vec<f64>>,
        #[serde(default)]
        domain: Option<[f64; 4]>,
    },
    Cone {
        frame: Frame,
        rb: f64,
        rt: f64,
        a0: f64,
        a1: f64,
        h: f64,
        quintic: bool,
    },
    Sweep {
        dim: u64,
        crv: InlineCrv,
        vec: [f64; 3],
    },
    ReadDb {
        binary: bool,
        version: u64,
        db: DbIn,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    this: Option<This>,
    #[serde(flatten)]
    op: OpIn,
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::State => Op::State,
            OpIn::Counts => Op::Counts,
            OpIn::Domain => Op::Domain,
            OpIn::Knots { .. } => Op::Knots,
            OpIn::Greville { .. } => Op::Greville,
            OpIn::GetCanonical => Op::GetCanonical,
            OpIn::Net => Op::Net,
            OpIn::ControlPoints => Op::ControlPoints,
            OpIn::Parallel { .. } => Op::Parallel,
            OpIn::BBox { .. } => Op::BBox,
            OpIn::Transform { .. } => Op::Transform,
            OpIn::Eq { .. } => Op::Eq,
            OpIn::IsKindOf { .. } => Op::IsKindOf,
            OpIn::Type => Op::Type,
            OpIn::MakeNurb => Op::MakeNurb,
            OpIn::Copy => Op::Copy,
            OpIn::Offset { .. } => Op::Offset,
            OpIn::DegenCurve { .. } => Op::DegenCurve,
            OpIn::StepInv { .. } => Op::StepInv,
            OpIn::SetForm { .. } => Op::SetForm,
            OpIn::Bilinear { .. } => Op::Bilinear,
            OpIn::Canonical { .. } => Op::Canonical,
            OpIn::Cone { .. } => Op::Cone,
            OpIn::Sweep { .. } => Op::Sweep,
            OpIn::ReadDb { .. } => Op::ReadDb,
        }
    }

    /// Whether a successful op is followed by the state of a new surface.
    fn creates(&self) -> bool {
        matches!(
            self,
            OpIn::Copy
                | OpIn::Bilinear { .. }
                | OpIn::Canonical { .. }
                | OpIn::Cone { .. }
                | OpIn::Sweep { .. }
                | OpIn::ReadDb { .. }
        )
    }
}

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let mut it = text.split_whitespace();
    let mut num = || -> Result<f64, HarnessError> {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))?
            .parse::<f64>()
            .map_err(|e| harness(format!("{path}: {e}")))
    };
    let int = |v: f64| -> Result<usize, HarnessError> {
        if v.fract() != 0.0 || v < 0.0 {
            return Err(harness(format!("{path}: bad count {v}")));
        }
        Ok(v as usize)
    };
    let n = int(num()?)?;
    let m = int(num()?)?;
    let p = int(num()?)?;
    let q = int(num()?)?;
    let rat = num()? != 0.0;
    let mut net = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let mut row = Vec::with_capacity(m + 1);
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(InlineSrf { net, p, q, u, v })
}

fn resolve(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

fn shape(s: &InlineSrf) -> Result<(usize, usize), HarnessError> {
    let n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols.checked_sub(1).ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// The database bytes of a `ReadDb` input.
fn db_bytes(binary: bool, version: u64, db: &DbIn) -> Result<Vec<u8>, HarnessError> {
    match db {
        DbIn::File { file } => std::fs::read(file).map_err(|e| harness(format!("{file}: {e}"))),
        DbIn::Data(d) => {
            let mut b = Vec::new();
            let header_ok = d.n >= 0 && d.m >= 0 && d.p >= 0 && d.q >= 0;
            let per = if d.rat == 1 { 4 } else { 3 };
            let n_pts = if header_ok && (d.rat == 0 || d.rat == 1) {
                ((d.n + 1) * (d.m + 1)) as usize
            } else {
                0
            };
            if binary {
                b.extend_from_slice(&d.n.to_ne_bytes());
                b.extend_from_slice(&d.m.to_ne_bytes());
                b.extend_from_slice(&d.p.to_ne_bytes());
                b.extend_from_slice(&d.q.to_ne_bytes());
                b.extend_from_slice(&d.rat.to_ne_bytes());
                if version > 32 {
                    b.extend_from_slice(&d.kt.to_ne_bytes());
                    b.extend_from_slice(&d.form.to_ne_bytes());
                }
                for x in &d.vals {
                    b.extend_from_slice(&x.to_ne_bytes());
                }
            } else {
                let mut t = format!("{} {}\n{} {}\n{}\n", d.n, d.m, d.p, d.q, d.rat);
                if version > 32 {
                    let _ = write!(t, "{}\n{}\n", d.kt, d.form);
                }
                let (pts, knots) = d.vals.split_at((n_pts * per).min(d.vals.len()));
                for c in pts.chunks(per) {
                    let s: Vec<String> = c.iter().map(|x| format!("{x:.17e}")).collect();
                    t.push_str(&s.join(" "));
                    t.push('\n');
                }
                for x in knots {
                    let _ = writeln!(t, "{x:.17e}");
                }
                b = t.into_bytes();
            }
            Ok(b)
        }
    }
}

// ---- C++ side ---------------------------------------------------------------------

fn encode_srf(s: &InlineSrf, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend([
        n as f64,
        m as f64,
        s.p as f64,
        s.q as f64,
        s.u.len().saturating_sub(1) as f64,
        s.v.len().saturating_sub(1) as f64,
    ]);
    o.extend(s.net.iter().flatten().flatten());
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

fn encode_crv(c: &InlineCrv, o: &mut Vec<f64>) {
    let n = c.cpts.len().saturating_sub(1);
    o.extend([n as f64, c.p as f64, c.u.len().saturating_sub(1) as f64]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
}

fn encode_this(t: &This, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    o.extend([t.ctor.ffi().code(), flag(t.oob), f64::from(t.form)]);
    if t.ctor != CtorIn::Empty {
        let s = t
            .srf
            .as_ref()
            .ok_or_else(|| harness("this: ctor needs a surface"))?;
        encode_srf(&resolve(s)?, o)?;
    }
    Ok(())
}

fn encode_frame(f: &Frame, o: &mut Vec<f64>) {
    for v in f {
        o.extend_from_slice(v);
    }
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match &i.this {
        Some(t) => encode_this(t, &mut o)?,
        None => o.push(Ctor::None.code()),
    }
    match &i.op {
        OpIn::State
        | OpIn::Counts
        | OpIn::Domain
        | OpIn::GetCanonical
        | OpIn::Net
        | OpIn::ControlPoints
        | OpIn::Type
        | OpIn::MakeNurb
        | OpIn::Copy => {}
        OpIn::Knots { v_dir, ivl } => {
            let [a, b] = ivl.unwrap_or([0.0, 0.0]);
            o.extend([flag(*v_dir), flag(ivl.is_some()), a, b]);
        }
        OpIn::Greville { v_dir, index } => o.extend([flag(*v_dir), *index as f64]),
        OpIn::Parallel { vec } => o.extend_from_slice(vec),
        OpIn::BBox {
            domain,
            normal,
            pseudo,
            polar,
            pseudo_guess,
            polar_guess,
            expand,
        } => {
            o.extend_from_slice(domain);
            o.extend([flag(*normal), flag(*pseudo), flag(*polar), flag(pseudo_guess.is_some())]);
            encode_frame(
                &pseudo_guess.unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
                &mut o,
            );
            let vs = polar_guess.as_deref().unwrap_or(&[]);
            o.extend([flag(polar_guess.is_some()), vs.len() as f64]);
            for v in vs {
                o.extend_from_slice(v);
            }
            o.push(flag(*expand));
        }
        OpIn::Transform { frame, scale } => {
            encode_frame(frame, &mut o);
            o.push(flag(scale.is_some()));
            o.extend_from_slice(&scale.unwrap_or([1.0, 1.0, 1.0]));
        }
        OpIn::Eq { other } => encode_this(other, &mut o)?,
        OpIn::IsKindOf { t } => o.push(*t as f64),
        OpIn::Offset { dist, tol } => o.extend([*dist, *tol]),
        OpIn::DegenCurve { angle, domain, tol } => {
            o.extend([*angle, flag(domain.is_some())]);
            o.extend_from_slice(&domain.unwrap_or([0.0; 4]));
            o.push(*tol);
        }
        OpIn::StepInv {
            domain,
            point,
            tol,
            guess,
        } => {
            o.extend_from_slice(domain);
            o.extend_from_slice(point);
            o.extend([*tol, flag(guess.is_some())]);
            o.extend_from_slice(&guess.unwrap_or([0.0, 0.0]));
        }
        OpIn::SetForm { form } => o.push(f64::from(*form)),
        OpIn::Bilinear { p00, p10, p01, p11 } => {
            for p in [p00, p10, p01, p11] {
                o.extend_from_slice(p);
            }
        }
        OpIn::Canonical {
            ud,
            vd,
            cps,
            form,
            uk,
            um,
            vk,
            vm,
            kt,
            weights,
            domain,
        } => {
            if uk.len() != um.len() || vk.len() != vm.len() {
                return Err(harness("canonical: one multiplicity per knot"));
            }
            o.extend([*ud as f64, *vd as f64, cps.len() as f64]);
            for p in cps {
                o.extend_from_slice(p);
            }
            o.push(f64::from(*form));
            o.push(uk.len() as f64);
            o.extend_from_slice(uk);
            o.extend(um.iter().map(|&m| m as f64));
            o.push(vk.len() as f64);
            o.extend_from_slice(vk);
            o.extend(vm.iter().map(|&m| m as f64));
            o.push(f64::from(*kt));
            o.push(flag(weights.is_some()));
            if let Some(w) = weights {
                o.push(w.len() as f64);
                o.extend_from_slice(w);
            }
            o.push(flag(domain.is_some()));
            o.extend_from_slice(&domain.unwrap_or([0.0; 4]));
        }
        OpIn::Cone {
            frame,
            rb,
            rt,
            a0,
            a1,
            h,
            quintic,
        } => {
            encode_frame(frame, &mut o);
            o.extend([*rb, *rt, *a0, *a1, *h, flag(*quintic)]);
        }
        OpIn::Sweep { dim, crv, vec } => {
            o.push(*dim as f64);
            encode_crv(crv, &mut o);
            o.extend_from_slice(vec);
        }
        OpIn::ReadDb {
            binary,
            version,
            db,
        } => {
            let bytes = db_bytes(*binary, *version, db)?;
            o.extend([flag(*binary), *version as f64, bytes.len() as f64]);
            o.extend(bytes.iter().map(|&b| f64::from(b)));
        }
    }
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let input = cpp_input(i)?;
    match ffi::eval(i.op.ffi(), &input) {
        Ok(flat) => to_output(i, &flat),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- records ----------------------------------------------------------------------

struct Split<'a> {
    flat: &'a [f64],
    at: usize,
    out: Output,
}

impl Split<'_> {
    fn take(&mut self, k: usize) -> Result<Vec<f64>, HarnessError> {
        let part = self
            .at
            .checked_add(k)
            .and_then(|end| self.flat.get(self.at..end))
            .ok_or_else(|| harness(format!("record too short ({} values)", self.flat.len())))?;
        self.at += k;
        Ok(part.to_vec())
    }

    fn count(&mut self) -> Result<usize, HarnessError> {
        let x = self.take(1)?.first().copied().unwrap_or(f64::NAN);
        if x.fract() != 0.0 || !(0.0..=1e7).contains(&x) {
            return Err(harness(format!("bad count {x} in record")));
        }
        Ok(x as usize)
    }

    fn put(&mut self, key: &str, v: Vec<f64>) {
        let out = std::mem::take(&mut self.out);
        self.out = out.values(key, v);
    }

    fn vals(&mut self, key: &str, k: usize) -> Result<(), HarnessError> {
        let v = self.take(k)?;
        self.put(key, v);
        Ok(())
    }

    /// A counted array: `count`, then `width * count` values.
    fn counted(&mut self, key: &str, width: usize) -> Result<usize, HarnessError> {
        let k = self.count()?;
        let out = std::mem::take(&mut self.out);
        self.out = out.int(&format!("{key}_count"), k as i64);
        self.vals(key, width * k)?;
        Ok(k)
    }

    /// Distinct knots and multiplicities: `count knots mults`.
    fn knots(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let k = self.counted(&format!("{pfx}knots"), 1)?;
        self.vals(&format!("{pfx}mults"), k)
    }

    fn state(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let flags = self.take(5)?;
        let has = flags.first().copied() == Some(1.0);
        self.put(&format!("{pfx}flags"), flags);
        if has {
            let dims = self.take(6)?;
            let cnt = |i: usize| dims.get(i).copied().unwrap_or(f64::NAN);
            let ok = |x: f64| x.fract() == 0.0 && (0.0..=1e6).contains(&x);
            let (n, m, r, s) = (cnt(0), cnt(1), cnt(4), cnt(5));
            if !(ok(n) && ok(m) && ok(r) && ok(s)) {
                return Err(harness(format!("bad surface dims {dims:?}")));
            }
            self.put(&format!("{pfx}dims"), dims);
            self.vals(&format!("{pfx}cpts"), 4 * (n as usize + 1) * (m as usize + 1))?;
            self.vals(&format!("{pfx}u"), r as usize + 1)?;
            self.vals(&format!("{pfx}v"), s as usize + 1)?;
        }
        Ok(())
    }
}

/// Splits a flat record into keys (layout in the module docs).
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut s = Split {
        flat,
        at: 0,
        out: Output::new(),
    };
    let status = s.take(1)?;
    let ok = status.first().copied() == Some(0.0);
    s.put("status", status);
    if ok {
        match &i.op {
            OpIn::Counts => s.vals("counts", 10)?,
            OpIn::Domain => s.vals("domain", 4)?,
            OpIn::Knots { .. } => s.knots("")?,
            OpIn::Greville { .. }
            | OpIn::Parallel { .. }
            | OpIn::IsKindOf { .. }
            | OpIn::Offset { .. }
            | OpIn::DegenCurve { .. } => s.vals("value", 1)?,
            OpIn::Eq { .. } | OpIn::Type => s.vals("value", 2)?,
            OpIn::GetCanonical => {
                s.vals("head", 4)?;
                s.knots("u_")?;
                s.knots("v_")?;
                s.counted("pts", 3)?;
                s.counted("w", 1)?;
            }
            OpIn::Net => {
                s.vals("uv_count", 2)?;
                s.counted("pts", 3)?;
                s.counted("w", 1)?;
            }
            OpIn::ControlPoints => {
                let uc = s.count()?;
                let vc = s.count()?;
                s.put("uv_count", vec![uc as f64, vc as f64]);
                s.vals("cp", 4 * uc * vc)?;
            }
            OpIn::BBox {
                normal,
                pseudo,
                polar,
                ..
            } => {
                if *normal {
                    s.vals("normal_box", 6)?;
                }
                if *pseudo {
                    s.vals("pseudo_box", 15)?;
                }
                if *polar {
                    s.vals("polar_box", 14)?;
                }
            }
            OpIn::StepInv { .. } => s.vals("inversion", 3)?,
            _ => {}
        }
        if i.op.creates() {
            s.state("new_")?;
        }
    }
    if let OpIn::Sweep { .. } = i.op {
        let n = s.count()?;
        s.vals("swept_curve", 4 * (n + 1))?;
    }
    if i.this.is_some() {
        s.state("this_")?;
    }
    if s.at != flat.len() {
        return Err(harness(format!(
            "record has {} trailing values",
            flat.len() - s.at
        )));
    }
    Ok(s.out)
}

// ---- Rust side --------------------------------------------------------------------

fn to_surface(s: &InlineSrf) -> Result<Surface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|row| row.iter().map(|c| CPoint::new(c[0], c[1], c[2], c[3])).collect())
        .collect();
    Surface::new(net, s.p, s.q, s.u.clone(), s.v.clone()).map_err(|e| harness(format!("surface: {e}")))
}

fn to_curve(c: &InlineCrv) -> Result<Curve, HarnessError> {
    let n = c.cpts.len().checked_sub(1).ok_or_else(|| harness("empty curve"))?;
    let m = c.u.len().checked_sub(1).ok_or_else(|| harness("empty knots"))?;
    let mut cur = allocate_nurb_curve(n, c.p, m).map_err(|e| harness(format!("curve: {e}")))?;
    for (d, s) in cur.cpts_mut().iter_mut().zip(&c.cpts) {
        d.x = s[0];
        d.y = s[1];
        d.z = s[2];
        d.w = s[3];
    }
    cur.knots_mut().copy_from_slice(&c.u);
    Ok(cur)
}

fn form_of(v: u8) -> Result<BSplineSurfaceForm, HarnessError> {
    BSplineSurfaceForm::from_code(u64::from(v)).ok_or_else(|| harness(format!("bad form {v}")))
}

fn knot_type_of(v: u8) -> Result<KnotType, HarnessError> {
    Ok(match v {
        0 => KnotType::UniformKnots,
        1 => KnotType::Unspecified,
        2 => KnotType::QuasiUniformKnots,
        3 => KnotType::PiecewiseBezierKnots,
        4 => KnotType::Unknown,
        _ => return Err(harness(format!("bad knot type {v}"))),
    })
}

fn build_this(t: &This) -> Result<BSplineSurface, HarnessError> {
    let srf = || {
        t.srf
            .as_ref()
            .ok_or_else(|| harness("this: ctor needs a surface"))
            .and_then(resolve)
    };
    let form = form_of(t.form)?;
    let mut s = match t.ctor {
        CtorIn::Empty => BSplineSurface::new(),
        CtorIn::FastOwned => BSplineSurface::from_nurb(to_surface(&srf()?)?, false),
        CtorIn::FastBorrowed => BSplineSurface::from_nurb(to_surface(&srf()?)?, true),
        CtorIn::Data => {
            let src = to_surface(&srf()?)?;
            let ix = src.indices();
            BSplineSurface::from_data(
                ix.n,
                ix.m,
                ix.p,
                ix.q,
                ix.r,
                ix.s,
                &src.cpts(),
                src.u_knots(),
                src.v_knots(),
            )
            .map_err(|e| harness(format!("from_data: {e}")))?
        }
        CtorIn::GwCopy => BSplineSurface::from_gw_nurb(&to_surface(&srf()?)?),
        CtorIn::Copy => {
            let mut orig = BSplineSurface::from_nurb(to_surface(&srf()?)?, false);
            *orig.out_of_bounds_enabled_mut() = t.oob;
            orig.set_b_spline_surface_form(form);
            orig.copy()
        }
    };
    *s.out_of_bounds_enabled_mut() = t.oob;
    s.set_b_spline_surface_form(form);
    Ok(s)
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn dump_state(o: &mut Vec<f64>, s: &BSplineSurface) {
    o.extend([
        flag(s.gw_nurb().is_some()),
        flag(s.is_out_of_bounds_enabled()),
        flag(s.is_nurb_borrowed()),
        f64::from(s.knot_type() as i32),
        f64::from(s.form() as i32),
    ]);
    if let Some(sur) = s.gw_nurb() {
        let ix = sur.indices();
        o.extend([ix.n, ix.m, ix.p, ix.q, ix.r, ix.s].map(|x| x as f64));
        for row in sur.rows() {
            for p in row {
                o.extend([p.x, p.y, p.z, p.w]);
            }
        }
        o.extend_from_slice(sur.u_knots());
        o.extend_from_slice(sur.v_knots());
    }
}

fn push_knots(o: &mut Vec<f64>, k: &[f64], m: &[usize]) {
    o.push(k.len() as f64);
    o.extend_from_slice(k);
    o.extend(m.iter().map(|&x| x as f64));
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn frame_of(f: &Frame) -> Result<Axis2Placement, SolidError> {
    let mut a = Axis2Placement::default();
    a.set_canonical(v3(f[0]), v3(f[1]), v3(f[2]))?;
    Ok(a)
}

fn domain_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn dir(v_dir: bool) -> SurfParamType {
    if v_dir {
        SurfParamType::V
    } else {
        SurfParamType::U
    }
}

/// The outcome of a Rust op: success (outputs pushed), an upstream status, or a
/// harness problem.
enum Fail {
    Solid(SolidError),
    Harness(HarnessError),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        Fail::Solid(e)
    }
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// Runs a static op; pushes its outputs (after the status) on success. `tail` gets
/// what follows the status-dependent part (the swept curve).
fn rust_static(op: &OpIn, o: &mut Vec<f64>, tail: &mut Vec<f64>) -> Result<(), Fail> {
    let created = match op {
        OpIn::Bilinear { p00, p10, p01, p11 } => {
            BSplineSurface::create_bilinear_surface(v3(*p00), v3(*p10), v3(*p01), v3(*p11))?
        }
        OpIn::Canonical {
            ud,
            vd,
            cps,
            form,
            uk,
            um,
            vk,
            vm,
            kt,
            weights,
            domain,
        } => {
            let pts: Vec<Point3> = cps.iter().map(|&p| v3(p)).collect();
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: *ud,
                v_degree: *vd,
                control_points: &pts,
                form: form_of(*form)?,
                u_multiplicities: um,
                v_multiplicities: vm,
                u_knots: uk,
                v_knots: vk,
                knot_type: knot_type_of(*kt)?,
                weights: weights.as_deref(),
                uv_domain: domain.map(domain_of),
            })?
        }
        OpIn::Cone {
            frame,
            rb,
            rt,
            a0,
            a1,
            h,
            quintic,
        } => {
            let f = frame_of(frame)?;
            let prm = if *quintic {
                NurbCircleParam::Quintic
            } else {
                NurbCircleParam::Quadratic
            };
            BSplineSurface::create_cone_patch(&f, *rb, *rt, *a0, *a1, *h, prm)?
        }
        OpIn::Sweep { dim, crv, vec } => {
            let mut c = BSplineCurve::from_nurb(to_curve(crv)?, *dim, false);
            let res = BSplineSurface::create_linear_sweep(&mut c, v3(*vec));
            if let Some(cur) = c.gw_nurb() {
                tail.push(cur.n() as f64);
                for p in cur.cpts() {
                    tail.extend([p.x, p.y, p.z, p.w]);
                }
            }
            res?
        }
        OpIn::ReadDb {
            binary,
            version,
            db,
        } => {
            let bytes = db_bytes(*binary, *version, db)?;
            let mut r = DatabaseReader::new();
            r.open_for_read(bytes, if *binary { FileType::Binary } else { FileType::Ascii });
            BSplineSurface::read_from_db(&mut r, *version)?
        }
        _ => return Err(harness("not a static op").into()),
    };
    dump_state(o, &created);
    Ok(())
}

/// Runs a surface op on `s`; pushes its outputs (after the status) on success.
fn rust_surface(k: &Kernel, op: &OpIn, s: &mut BSplineSurface, o: &mut Vec<f64>) -> Result<(), Fail> {
    match op {
        OpIn::State => {}
        OpIn::Counts => {
            o.extend([
                s.degree(SurfParamType::U) as f64,
                s.degree(SurfParamType::V) as f64,
                s.number_natural_knots(SurfParamType::U) as f64,
                s.number_natural_knots(SurfParamType::V) as f64,
                s.number_control_points(SurfParamType::U) as f64,
                s.number_control_points(SurfParamType::V) as f64,
                s.number_control_points(SurfParamType::Both) as f64,
                flag(s.is_rational()),
                flag(*s.out_of_bounds_enabled_mut()),
                f64::from(s.form() as i32),
            ]);
        }
        OpIn::Domain => {
            let d = s.natural_uv_domain();
            o.extend([d.u_min(), d.v_min(), d.u_max(), d.v_max()]);
        }
        OpIn::Knots { v_dir, ivl } => {
            let e = ivl.map(|[a, b]| Extent1d::from_min_max(a, b));
            let (k, m) = s.knots(dir(*v_dir), e.as_ref())?;
            push_knots(o, &k, &m);
        }
        OpIn::Greville { v_dir, index } => o.push(s.greville_abscissa(dir(*v_dir), *index)?),
        OpIn::GetCanonical => {
            let c = s.canonical()?;
            o.extend([
                c.u_degree as f64,
                c.v_degree as f64,
                f64::from(c.form as i32),
                f64::from(c.knot_type as i32),
            ]);
            push_knots(o, &c.u_knots, &c.u_multiplicities);
            push_knots(o, &c.v_knots, &c.v_multiplicities);
            o.push(c.control_points.len() as f64);
            for p in c.control_points {
                push3(o, p);
            }
            o.push(c.weights.len() as f64);
            o.extend_from_slice(&c.weights);
        }
        OpIn::Net => {
            let net = s.control_point_net()?;
            o.extend([net.u_count as f64, net.v_count as f64]);
            o.push(net.points.len() as f64);
            for p in net.points {
                push3(o, p);
            }
            o.push(net.weights.len() as f64);
            o.extend_from_slice(&net.weights);
        }
        OpIn::ControlPoints => {
            let (uc, vc, cps) = s.control_points()?;
            o.extend([uc as f64, vc as f64]);
            for p in cps {
                o.extend([p.x, p.y, p.z, p.w]);
            }
        }
        OpIn::Parallel { vec } => o.push(flag(s.is_parallel_to_vector(v3(*vec))?)),
        OpIn::BBox {
            domain,
            normal,
            pseudo,
            polar,
            pseudo_guess,
            polar_guess,
            expand,
        } => {
            let pg = pseudo_guess.map(|[b1, b2, b3]| {
                let mut g = PseudoBox::new();
                let _ = g.set_basis(v3(b1), v3(b2), v3(b3));
                g
            });
            let plg = polar_guess.as_ref().map(|vs| {
                let mut g = PolarBox::new();
                for v in vs {
                    g.add_vector(v3(*v), None);
                }
                g
            });
            let req = BoundingBoxRequest {
                normal: *normal,
                pseudo: *pseudo,
                polar: *polar,
                pseudo_guess: pg.as_ref(),
                polar_guess: plg.as_ref(),
                expand_by_zone_tol: *expand,
            };
            let b = s.calculate_bounding_box(k, &domain_of(*domain), &req)?;
            if let Some(nb) = b.normal {
                o.extend((0..3).map(|i| nb.min_at(i)));
                o.extend((0..3).map(|i| nb.max_at(i)));
            }
            if let Some(pb) = b.pseudo {
                for v in pb.basis() {
                    push3(o, *v);
                }
                for ivl in pb.intervals() {
                    o.extend([ivl.min(), ivl.max()]);
                }
            }
            if let Some(pl) = b.polar {
                o.push(f64::from(pl.state().code()));
                for v in pl.basis() {
                    push3(o, *v);
                }
                let d = pl.domain();
                o.extend([d.u_min(), d.v_min(), d.u_max(), d.v_max()]);
            }
        }
        OpIn::Transform { frame, scale } => {
            let f = frame_of(frame)?;
            s.transform(&f, scale.map(v3))?;
        }
        OpIn::Eq { other } => {
            let d = build_this(other)?;
            o.extend([flag(s.approx_eq(&d)), flag(s.approx_eq(s))]);
        }
        OpIn::IsKindOf { t } => o.push(flag(s.is_kind_of(*t)?)),
        OpIn::Type => o.extend([s.type_code() as f64, BSplineSurface::TYPE as f64]),
        OpIn::MakeNurb => s.make_nurb()?,
        OpIn::Copy => dump_state(o, &s.copy()),
        OpIn::Offset { dist, tol } => {
            let r = s.create_offset_surface(&Kernel::default(), *dist, *tol)?;
            o.push(r.type_code() as f64);
        }
        OpIn::DegenCurve { angle, domain, tol } => {
            let d = domain.map(domain_of);
            o.push(flag(s.is_degenerate_curve(k, *angle, d.as_ref(), *tol)?));
        }
        OpIn::StepInv {
            domain,
            point,
            tol,
            guess,
        } => {
            let g = guess.map(|[u, v]| Point2::new(u, v));
            let (uv, loc) = s.step_inversion(&Kernel::default(), &domain_of(*domain), v3(*point), *tol, g)?;
            o.extend([uv.x, uv.y, f64::from(loc as i32)]);
        }
        OpIn::SetForm { form } => {
            s.set_b_spline_surface_form(form_of(*form)?);
            s.toggle_swap_uv_bit();
            s.rebuild_step_from_nurb_parameters()?;
        }
        _ => return Err(harness("not a surface op").into()),
    }
    Ok(())
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let mut this = i.this.as_ref().map(build_this).transpose()?;
    let mut o = vec![0.0];
    let mut tail = Vec::new();
    let res = match this.as_mut() {
        Some(s) => rust_surface(k, &i.op, s, &mut o),
        None => rust_static(&i.op, &mut o, &mut tail),
    };
    match res {
        Ok(()) => {}
        Err(Fail::Harness(e)) => return Err(e),
        Err(Fail::Solid(e)) => {
            if let Some(d) = Output::unported(&e) {
                return Ok(d);
            }
            o = vec![status(&e)];
        }
    }
    o.extend(tail);
    if let Some(s) = &this {
        dump_state(&mut o, s);
    }
    to_output(i, &o)
}

// ---- generated cases --------------------------------------------------------------

/// A random clamped knot vector of degree `p` on `[a, a + len]` with up to `extra`
/// interior knots (multiplicities up to `p`).
fn gen_knots(rng: &mut Rng, p: usize, extra: usize) -> Vec<f64> {
    let (a, len) = if rng.chance(0.3) {
        (rng.below(5) as f64 - 2.0, 1.0 + rng.below(6) as f64)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    };
    let b = a + len;
    let integral = rng.chance(0.3);
    let mut inner: Vec<f64> = Vec::new();
    let mut used: Vec<f64> = Vec::new();
    let mut remaining = extra;
    while remaining > 0 {
        let mut v = a + 0.5 * len;
        for attempt in 0..32 {
            v = if integral && attempt < 16 {
                a + len * (1.0 + rng.below(15) as f64) / 16.0
            } else {
                rng.uniform(a, b)
            };
            if v > a && v < b && !used.contains(&v) {
                break;
            }
        }
        if v <= a || v >= b || used.contains(&v) {
            break;
        }
        used.push(v);
        let mult = if rng.chance(0.25) {
            1 + rng.below(p.min(remaining) as u64) as usize
        } else {
            1
        };
        inner.extend(std::iter::repeat_n(v, mult));
        remaining -= mult;
    }
    inner.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(std::iter::repeat_n(b, p + 1));
    u
}

fn gen_degree(rng: &mut Rng) -> usize {
    if rng.chance(0.06) {
        5 + rng.below(3) as usize
    } else {
        1 + rng.below(4) as usize
    }
}

/// A random clamped surface, 3D (rarely 2D), rational or not; sometimes planar or
/// with a collapsed row / column.
fn gen_srf(rng: &mut Rng, allow_2d: bool) -> InlineSrf {
    let p = gen_degree(rng);
    let q = gen_degree(rng);
    let extra_u = rng.below(5) as usize;
    let u = gen_knots(rng, p, extra_u);
    let extra_v = rng.below(5) as usize;
    let v = gen_knots(rng, q, extra_v);
    let n = u.len() - p - 2;
    let m = v.len() - q - 2;
    let two_d = allow_2d && rng.chance(0.08);
    let rational = rng.chance(0.3);
    let shape = rng.below(10);
    let collapsed_row = rng.below(n as u64 + 1) as usize;
    let base = rng.point3(-10.0, 10.0);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let [x, y, z] = match shape {
                        // planar grid
                        0 => [i as f64 + rng.uniform(-0.2, 0.2), j as f64 * 1.5, 2.0],
                        // ruled along a direction: parallel to (0, 0, 1) in v
                        1 => [i as f64, (i * i) as f64 * 0.3, j as f64 * 2.0],
                        // a collapsed row (pole)
                        2 if i == collapsed_row => base,
                        _ => rng.point3(-10.0, 10.0),
                    };
                    let w = if rational { rng.uniform(0.3, 3.0) } else { 1.0 };
                    let z = if two_d { NOZ } else { z * w };
                    [x * w, y * w, z, if rational { w } else { NOW }]
                })
                .collect()
        })
        .collect();
    InlineSrf { net, p, q, u, v }
}

fn gen_ctor(rng: &mut Rng) -> CtorIn {
    match rng.below(10) {
        0..=3 => CtorIn::FastOwned,
        4 => CtorIn::FastBorrowed,
        5 | 6 => CtorIn::Data,
        7 | 8 => CtorIn::GwCopy,
        _ => CtorIn::Copy,
    }
}

fn gen_vec(rng: &mut Rng, lo: f64, hi: f64) -> [f64; 3] {
    rng.point3(lo, hi)
}

/// A frame: random (orthogonalised by `SetCanonical`), axis-aligned, identity, or
/// (rarely) degenerate.
fn gen_frame(rng: &mut Rng) -> Frame {
    match rng.below(10) {
        0 => [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        1 => [gen_vec(rng, -5.0, 5.0), [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
        2 if rng.chance(0.3) => [gen_vec(rng, -5.0, 5.0), [1.0, 1.0, 0.0], [2.0, 2.0, 0.0]],
        _ => [
            gen_vec(rng, -5.0, 5.0),
            gen_vec(rng, -1.0, 1.0),
            gen_vec(rng, -1.0, 1.0),
        ],
    }
}

fn ends(k: &[f64]) -> (f64, f64) {
    (k.first().copied().unwrap_or(0.0), k.last().copied().unwrap_or(1.0))
}

fn gen_domain(rng: &mut Rng, s: &InlineSrf) -> [f64; 4] {
    let (u0, u1) = ends(&s.u);
    let (v0, v1) = ends(&s.v);
    if rng.chance(0.5) {
        [u0, v0, u1, v1]
    } else {
        let a = rng.uniform(u0, u1);
        let b = rng.uniform(v0, v1);
        [a, b, rng.uniform(a, u1), rng.uniform(b, v1)]
    }
}

fn this_of(rng: &mut Rng, s: &InlineSrf) -> This {
    This {
        ctor: gen_ctor(rng),
        oob: rng.chance(0.3),
        form: if rng.chance(0.7) { 9 } else { rng.below(12) as u8 },
        srf: Some(SrfIn::Inline(s.clone())),
    }
}

/// Perturbs one control point coordinate of `s` by `eps`.
fn perturb(rng: &mut Rng, s: &InlineSrf, eps: f64) -> InlineSrf {
    let mut d = s.clone();
    let i = rng.below(d.net.len() as u64) as usize;
    if let Some(row) = d.net.get_mut(i) {
        let j = rng.below(row.len() as u64) as usize;
        if let Some(p) = row.get_mut(j) {
            p[0] += eps;
        }
    }
    d
}

fn gen_surface_op(rng: &mut Rng, s: &InlineSrf, this: &This) -> OpIn {
    let three_d = s.net.first().and_then(|r| r.first()).is_some_and(|p| p[2] != NOZ);
    let n_rows = s.net.len();
    let n_cols = s.net.first().map_or(1, Vec::len);
    match rng.below(24) {
        0 => OpIn::State,
        1 | 2 => OpIn::Counts,
        3 => OpIn::Domain,
        4 | 5 => OpIn::Knots {
            v_dir: rng.chance(0.5),
            ivl: if rng.chance(0.5) {
                let (a, b) = if rng.chance(0.5) { ends(&s.u) } else { ends(&s.v) };
                let x = rng.uniform(a - 0.1, b);
                Some([x, rng.uniform(x, b + 0.1)])
            } else {
                None
            },
        },
        6 | 7 => {
            let v_dir = rng.chance(0.5);
            let k = if v_dir { n_cols } else { n_rows };
            OpIn::Greville {
                v_dir,
                index: rng.below(k as u64 + 3) as usize,
            }
        }
        8 | 9 => OpIn::GetCanonical,
        10 => OpIn::Net,
        11 => OpIn::ControlPoints,
        12 => OpIn::Parallel {
            vec: match rng.below(4) {
                0 => [0.0, 0.0, 0.0],
                1 => [0.0, 0.0, 1.0],
                _ => gen_vec(rng, -1.0, 1.0),
            },
        },
        13 if three_d => OpIn::BBox {
            domain: gen_domain(rng, s),
            normal: rng.chance(0.7),
            pseudo: rng.chance(0.5),
            polar: rng.chance(0.5),
            pseudo_guess: if rng.chance(0.3) {
                Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
            } else {
                None
            },
            polar_guess: if rng.chance(0.3) {
                Some((0..3).map(|_| gen_vec(rng, -1.0, 1.0)).collect())
            } else {
                None
            },
            expand: rng.chance(0.5),
        },
        14 | 15 => OpIn::Transform {
            frame: gen_frame(rng),
            scale: match rng.below(4) {
                0 => None,
                1 => Some([1.0, 1.0, 1.0]),
                2 => {
                    let k = rng.uniform(0.1, 4.0);
                    Some([k, k, k])
                }
                _ => Some(gen_vec(rng, -3.0, 3.0)),
            },
        },
        16 | 17 => {
            let other = match rng.below(5) {
                0 => This {
                    ctor: CtorIn::Empty,
                    srf: None,
                    ..this.clone()
                },
                1 => This {
                    srf: Some(SrfIn::Inline(perturb(rng, s, 1e-15))),
                    ..this.clone()
                },
                2 => This {
                    srf: Some(SrfIn::Inline(perturb(rng, s, 1e-3))),
                    ..this.clone()
                },
                3 => This {
                    ctor: gen_ctor(rng),
                    ..this.clone()
                },
                _ => This {
                    ctor: gen_ctor(rng),
                    srf: Some(SrfIn::Inline(gen_srf(rng, true))),
                    ..this.clone()
                },
            };
            OpIn::Eq { other }
        }
        18 => OpIn::IsKindOf {
            t: if rng.chance(0.8) {
                BSplineSurface::TYPE
            } else {
                *rng.choose(&[15001, 15011, 15012, 1]).unwrap_or(&1)
            },
        },
        19 => {
            if rng.chance(0.5) {
                OpIn::Type
            } else {
                OpIn::MakeNurb
            }
        }
        20 => OpIn::SetForm {
            form: rng.below(12) as u8,
        },
        21 if three_d && rng.chance(0.3) => {
            let d = gen_domain(rng, s);
            let c = s.net.first().and_then(|r| r.first()).copied().unwrap_or([0.0; 4]);
            OpIn::StepInv {
                domain: d,
                point: [c[0], c[1], c[2]],
                tol: 1e-6,
                guess: None,
            }
        }
        22 if three_d && rng.chance(0.3) => OpIn::DegenCurve {
            angle: 1e-10,
            domain: None,
            tol: 1e-6,
        },
        _ => OpIn::Copy,
    }
}

/// The surface op a NURB-less surface can run.
fn gen_empty_op(rng: &mut Rng) -> OpIn {
    match rng.below(12) {
        0 => OpIn::State,
        1 => OpIn::Counts,
        2 => OpIn::Domain,
        3 => OpIn::Knots {
            v_dir: rng.chance(0.5),
            ivl: None,
        },
        4 => OpIn::GetCanonical,
        5 => OpIn::Parallel {
            vec: [0.0, 0.0, 0.0],
        },
        6 => OpIn::Eq {
            other: This {
                ctor: CtorIn::Empty,
                oob: false,
                form: 9,
                srf: None,
            },
        },
        7 => OpIn::IsKindOf {
            t: BSplineSurface::TYPE,
        },
        8 => OpIn::Type,
        9 => OpIn::MakeNurb,
        10 => OpIn::SetForm {
            form: rng.below(12) as u8,
        },
        _ => OpIn::Copy,
    }
}

fn gen_curve(rng: &mut Rng) -> InlineCrv {
    let p = 1 + rng.below(4) as usize;
    let extra_u = rng.below(5) as usize;
    let u = gen_knots(rng, p, extra_u);
    let n = u.len() - p - 2;
    let three_d = rng.chance(0.75);
    let rational = rng.chance(0.3);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y, z] = rng.point3(-10.0, 10.0);
            let w = if rational { rng.uniform(0.3, 3.0) } else { 1.0 };
            let z = if three_d { z * w } else { NOZ };
            [x * w, y * w, z, if rational { w } else { NOW }]
        })
        .collect();
    InlineCrv { cpts, p, u }
}

fn gen_canonical(rng: &mut Rng) -> OpIn {
    let gen_dir = |rng: &mut Rng| {
        let deg = 1 + rng.below(4) as usize;
        let n_inner = rng.below(4) as usize;
        let mut knots = vec![rng.uniform(-3.0, 3.0)];
        for _ in 0..=n_inner {
            let last = knots.last().copied().unwrap_or(0.0);
            knots.push(last + rng.uniform(0.1, 2.0));
        }
        let mut mults = vec![deg + 1];
        for _ in 0..n_inner {
            mults.push(1 + rng.below(deg as u64) as usize);
        }
        mults.push(deg + 1);
        (deg, knots, mults)
    };
    let (ud, uk, um) = gen_dir(rng);
    let (vd, vk, vm) = gen_dir(rng);
    let nu = um.iter().sum::<usize>() - ud - 1;
    let nv = vm.iter().sum::<usize>() - vd - 1;
    let ncp = nu * nv;
    let cps: Vec<[f64; 3]> = (0..ncp).map(|_| gen_vec(rng, -10.0, 10.0)).collect();
    let mut weights = match rng.below(4) {
        0 => Some(vec![1.0; ncp]),
        1 => Some((0..ncp).map(|_| rng.uniform(0.3, 3.0)).collect::<Vec<_>>()),
        2 if rng.chance(0.2) => Some(Vec::new()),
        _ => None,
    };
    let domain = if rng.chance(0.2) {
        let (u0, u1) = ends(&uk);
        let (v0, v1) = ends(&vk);
        Some([u0, v0, u1, v1])
    } else {
        None
    };
    let (mut ud_out, mut vd_out) = (ud, vd);
    let mut cps = cps;
    match rng.below(25) {
        0 => ud_out = 0,
        1 => vd_out = 33,
        2 => weights = Some(vec![1.0; ncp + 1]),
        3 => {
            cps.pop();
        }
        4 => {
            if let Some(w) = weights.as_mut().and_then(|w| w.first_mut()) {
                *w = -1.0;
            }
        }
        _ => {}
    }
    OpIn::Canonical {
        ud: ud_out,
        vd: vd_out,
        cps,
        form: rng.below(12) as u8,
        uk,
        um,
        vk,
        vm,
        kt: rng.below(5) as u8,
        weights,
        domain,
    }
}

fn gen_db(rng: &mut Rng) -> OpIn {
    let s = gen_srf(rng, false);
    let n = s.net.len() as i64 - 1;
    let m = s.net.first().map_or(0, Vec::len) as i64 - 1;
    let rational = s.net.first().and_then(|r| r.first()).is_some_and(|p| p[3] != NOW);
    let mut vals = Vec::new();
    for row in &s.net {
        for p in row {
            vals.extend_from_slice(&p[..3]);
            if rational {
                vals.push(p[3]);
            }
        }
    }
    vals.extend_from_slice(&s.u);
    vals.extend_from_slice(&s.v);
    let rat = if rng.chance(0.05) { 2 } else { i16::from(rational) };
    let version = if rng.chance(0.5) { 30 } else { 33 };
    OpIn::ReadDb {
        binary: rng.chance(0.5),
        version,
        db: DbIn::Data(DbData {
            n,
            m,
            p: s.p as i16,
            q: s.q as i16,
            rat,
            kt: rng.below(5),
            form: rng.below(12),
            vals,
        }),
    }
}

fn gen_static_op(rng: &mut Rng) -> OpIn {
    match rng.below(10) {
        0..=2 => gen_canonical(rng),
        3 => OpIn::Bilinear {
            p00: gen_vec(rng, -10.0, 10.0),
            p10: gen_vec(rng, -10.0, 10.0),
            p01: gen_vec(rng, -10.0, 10.0),
            p11: gen_vec(rng, -10.0, 10.0),
        },
        4 | 5 => {
            let rb = if rng.chance(0.1) { 0.0 } else { rng.uniform(0.1, 10.0) };
            OpIn::Cone {
                frame: gen_frame(rng),
                rb,
                rt: match rng.below(5) {
                    0 => rb,
                    1 if rng.chance(0.3) => 0.0,
                    _ => rng.uniform(0.1, 10.0),
                },
                a0: if rng.chance(0.05) { 400.0 } else { rng.uniform(0.0, 360.0) },
                a1: match rng.below(6) {
                    0 => 360.0,
                    1 => 0.0,
                    _ => rng.uniform(0.0, 360.0),
                },
                h: if rng.chance(0.1) { -rng.uniform(0.1, 5.0) } else { rng.uniform(0.1, 10.0) },
                quintic: rng.chance(0.3),
            }
        }
        6 | 7 => {
            let crv = gen_curve(rng);
            let three_d = crv.cpts.first().is_some_and(|p| p[2] != NOZ);
            OpIn::Sweep {
                dim: if three_d { 3 } else { 2 },
                crv,
                vec: if rng.chance(0.05) {
                    [0.0, 0.0, 1e-13]
                } else {
                    gen_vec(rng, -5.0, 5.0)
                },
            }
        }
        _ => gen_db(rng),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.3) {
        return Inputs {
            this: None,
            op: gen_static_op(rng),
        };
    }
    if rng.chance(0.05) {
        return Inputs {
            this: Some(This {
                ctor: CtorIn::Empty,
                oob: rng.chance(0.3),
                form: rng.below(12) as u8,
                srf: None,
            }),
            op: gen_empty_op(rng),
        };
    }
    let s = gen_srf(rng, true);
    let this = this_of(rng, &s);
    let op = gen_surface_op(rng, &s, &this);
    Inputs {
        this: Some(this),
        op,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nurbs.b_spline_surface_p1"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        rust(k, &case.parse()?)
    }
}
