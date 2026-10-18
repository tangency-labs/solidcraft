//! FFI wrappers for unit `surface.surf_of_extrusion_p2`.
//!
//! Shim: `harness/cpp/shim/surface_surf_of_extrusion_p2.cpp`
//! (`sc_surface_surf_of_extrusion_p2_eval`).
//! Upstream: `source/SMLib/src/SmSurfOfExtrusion.cpp`.
//!
//! [`surface_op`] makes a surface of extrusion from a [`Src`] (built as in
//! `surface.surf_of_extrusion_p1`, or read with `SmSurfOfExtrusion::ReadFromDB` from a
//! database record) and runs one [`Op`] on it; [`is_nurb`] runs
//! `IsNurbSurfaceSurfOfExtrusion` on a [`TestSurf`]. Both return the shim's flat
//! record. Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`.
//!
//! * [`surface_op`]: the source status (construction or read), then on success the op's
//!   record: a target status where the op makes a second object (`Cylinder`,
//!   `Extrusion`, `Global`), then the op status and, on success, its outputs. `State`:
//!   `0` and the state. `Plane`, `Cylinder`, `Extrusion`: an intersection `more n
//!   curves ntypes types nuv1 has_uv1... nuv2 has_uv2... ndev devs`. `Global`: the same
//!   (`more` always `0`), or the status `-3` when upstream would run the general
//!   intersector (the special case needs more, or the other surface has none).
//!   `Trim`: the state. A state is the `surface.surf_of_extrusion_p1` state, a curve
//!   its curve layout.
//! * [`is_nurb`]: the source status (`0` for an inline NURB), the statuses of the
//!   `SwapUV` and `Reparameterize` edits asked for, then the result: `0 found
//!   [state]`, or the failing status upstream returns as its boolean.
//!
//! The wrappers accept only what upstream handles without undefined behaviour: built
//! surfaces as `surface.surf_of_extrusion_p1` accepts them; records that the strict
//! stream simulator of `topology.brep_data` reads completely (every extraction well
//! formed, no short binary read: upstream then reads uninitialised values) with a
//! B-spline generator (`SmCurve_TYPE` 14033: upstream casts other kinds unchecked) and
//! B-spline headers and knots it accepts; finite plane, cone and placement data with
//! independent axes, planes with positive UV scales and ordered domains (a plane normal
//! longer than `1e-6`), ordered edit domains with positive extents, clamped inline test
//! NURBs with `1 <= degree <= n` and at most [`MAX_INDEX`] control points per direction
//! and finite tolerances. The shim answers `SC_ERR_ARG` for a control point to perturb
//! outside the test NURB and for a cylinder whose bottom iso circle upstream would make
//! from unset values (as the `surface.cone_p1` shim tests it).

use crate::units::surface_surf_of_extrusion_p1::{
    axes, check_knots, check_points, domain, finite, push_surf, Surf,
};
use crate::units::topology_brep_data::Sim;
use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest control point index of an inline test NURB.
pub const MAX_INDEX: usize = 64;
/// Largest degree of an inline test NURB.
pub const MAX_DEGREE: usize = 9;

/// `SmBSplineCurve_TYPE`: the only generator kind a record may hold.
const B_SPLINE_CURVE_TYPE: i64 = 14033;

mod sys {
    extern "C" {
        pub fn sc_surface_surf_of_extrusion_p2_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            bytes: *const u8,
            n_bytes: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Where the surface of extrusion comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Src {
    /// Built as in `surface.surf_of_extrusion_p1`.
    Built(Surf),
    /// `SmSurfOfExtrusion::ReadFromDB` of a database record (the bytes after the
    /// surface's type header).
    Record {
        /// ASCII (else binary) stream.
        ascii: bool,
        /// Database version.
        version: u64,
        /// The record.
        bytes: Vec<u8>,
    },
}

/// A plane.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaneIn {
    /// `SmPlane(O, X, Y, scale, domain)`.
    Bounded {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// UV scales.
        scale: [f64; 2],
        /// UV domain `umin vmin umax vmax`.
        dom: [f64; 4],
    },
    /// `SmPlane(O, N)` (infinite).
    Normal {
        /// Origin.
        o: [f64; 3],
        /// Normal.
        n: [f64; 3],
    },
}

/// How a cone is made.
#[derive(Debug, Clone, PartialEq)]
pub enum ConeKind {
    /// `SmCone::CreateCanonical(O, bot, top, height, swap, inside)`.
    Canonical {
        /// Bottom radius.
        bot: f64,
        /// Top radius (the semi-angle for `height <= 0`).
        top: f64,
        /// Height.
        height: f64,
        /// `bSwapUV`.
        swap: bool,
        /// `bInsideOut`.
        inside: bool,
    },
    /// `SmCone::CreateCanonical(O, bot, semi, pOptHeight, swap, inside)`.
    Semi {
        /// Bottom radius.
        bot: f64,
        /// Semi-angle in degrees.
        semi: f64,
        /// Optional height.
        height: Option<f64>,
        /// `bSwapUV`.
        swap: bool,
        /// `bInsideOut`.
        inside: bool,
    },
    /// `SmCylinder::CreateCanonical(O, radius)`.
    Cylinder {
        /// Radius.
        radius: f64,
    },
}

/// A cone or cylinder.
#[derive(Debug, Clone, PartialEq)]
pub struct ConeIn {
    /// Construction.
    pub kind: ConeKind,
    /// Placement origin.
    pub o: [f64; 3],
    /// Placement X axis.
    pub x: [f64; 3],
    /// Placement Y axis.
    pub y: [f64; 3],
}

/// The other surface of `GlobalSurfaceIntersect`.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// An `SmPlane`.
    Plane(PlaneIn),
    /// An `SmCone` / `SmCylinder`.
    Cone(ConeIn),
    /// An `SmSurfOfExtrusion`.
    Extrusion(Src),
    /// The NURB of a surface of extrusion as an `SmBSplineSurface`.
    BSpline(Src),
}

/// An inline NURB surface: control points `[x, y, z, w]` (`w` = `NL_NOW` for
/// non-rational), `v` fastest.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbIn {
    /// Highest U index.
    pub n: usize,
    /// Highest V index.
    pub m: usize,
    /// U degree.
    pub p: usize,
    /// V degree.
    pub q: usize,
    /// `(n + 1) (m + 1)` control points.
    pub cpts: Vec<[f64; 4]>,
    /// `n + p + 2` U knots.
    pub u: Vec<f64>,
    /// `m + q + 2` V knots.
    pub v: Vec<f64>,
}

/// The source of a test surface.
#[derive(Debug, Clone, PartialEq)]
pub enum TestSource {
    /// `SmBSplineSurface(sur, FALSE, ctx)` of an inline NURB.
    Raw(NurbIn),
    /// `SmBSplineSurface(src->GetGwNurbPointer())`.
    Src(Src),
}

/// The test surface of `IsNurbSurfaceSurfOfExtrusion` and its edits.
#[derive(Debug, Clone, PartialEq)]
pub struct TestSurf {
    /// The surface.
    pub source: TestSource,
    /// `SwapUV()` it.
    pub transpose: bool,
    /// `Reparameterize(domain)` it.
    pub reparam: Option<[f64; 4]>,
    /// Add `d` to the stored `x y z` of control point `(i, j)`.
    pub perturb: Option<(usize, usize, [f64; 3])>,
}

/// The member run on the surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The state only.
    State,
    /// `IntersectWithPlane` (domains default to the natural ones).
    Plane {
        /// The plane.
        plane: PlaneIn,
        /// This surface's NURB domain.
        dom: Option<[f64; 4]>,
        /// The plane's domain.
        other_dom: Option<[f64; 4]>,
        /// `pdOptApproxTol3d`.
        tol: Option<f64>,
    },
    /// `IntersectWithCylinder`.
    Cylinder {
        /// The cone.
        cone: ConeIn,
        /// `pdOptApproxTol3d`.
        tol: Option<f64>,
    },
    /// `IntersectWithSurfOfExtrusion`.
    Extrusion {
        /// The other surface.
        other: Src,
        /// `pdOptApproxTol3d`.
        tol: Option<f64>,
    },
    /// `GlobalSurfaceIntersect`.
    Global {
        /// The other surface.
        target: Target,
        /// This surface's NURB domain.
        dom: Option<[f64; 4]>,
        /// The other surface's domain.
        other_dom: Option<[f64; 4]>,
        /// `pdOptApproxTol3d`.
        tol: Option<f64>,
        /// `pdOptAngTolRad`.
        ang: Option<f64>,
    },
    /// `TrimWithDomain`.
    Trim {
        /// The new NURB domain.
        dom: [f64; 4],
    },
}

impl Op {
    const fn code(&self) -> i32 {
        match self {
            Self::State => 0,
            Self::Plane { .. } => 1,
            Self::Cylinder { .. } => 2,
            Self::Extrusion { .. } => 3,
            Self::Global { .. } => 4,
            Self::Trim { .. } => 6,
        }
    }
}

/// `OP_IS_NURB`.
const OP_IS_NURB: i32 = 5;

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn len(v: [f64; 3]) -> f64 {
    let [x, y, z] = v;
    (x * x + y * y + z * z).sqrt()
}

/// Checks a record with the strict stream simulator.
fn check_record(ascii: bool, version: u64, bytes: &[u8]) -> Result<(), FfiError> {
    if i64::try_from(version).is_err() {
        return Err(bad("record: version too large"));
    }
    let mut sim = Sim::for_record(bytes, !ascii);
    let doubles = |sim: &mut Sim<'_>, n: usize| -> Result<(), FfiError> {
        for _ in 0..n {
            sim.double()?;
        }
        sim.lines(1);
        Ok(())
    };
    doubles(&mut sim, 3)?; // origin
    doubles(&mut sim, 3)?; // extrusion vector
    doubles(&mut sim, 4)?; // analytic domain
    sim.ulong()?; // orientation
    sim.lines(1);
    sim.boolean()?; // swap
    sim.lines(1);
    doubles(&mut sim, 3)?; // curve point
    doubles(&mut sim, 3)?; // curve vector
    sim.lines(1);
    let (ty, _dim) = sim.read_type(true)?;
    if ty != B_SPLINE_CURVE_TYPE {
        return Err(bad("record: generator is not an SmBSplineCurve record"));
    }
    sim.bspline_curve(version)?;
    sim.bspline_surface(version)
}

fn check_plane(p: &PlaneIn) -> Result<(), FfiError> {
    match p {
        PlaneIn::Bounded {
            o,
            x,
            y,
            scale,
            dom,
        } => {
            finite(o, "plane origin")?;
            axes(*x, *y, "plane axes")?;
            finite(scale, "plane scale")?;
            if !(scale[0] > 0.0 && scale[1] > 0.0) {
                return Err(bad("plane scale: not positive"));
            }
            domain(*dom, true, "plane domain")
        }
        PlaneIn::Normal { o, n } => {
            finite(o, "plane origin")?;
            finite(n, "plane normal")?;
            if len(*n) < 1e-6 {
                return Err(bad("plane normal: too short"));
            }
            Ok(())
        }
    }
}

fn check_nurb(s: &NurbIn) -> Result<(), FfiError> {
    if s.n == 0 || s.m == 0 || s.n > MAX_INDEX || s.m > MAX_INDEX {
        return Err(bad("test NURB: bad control point counts"));
    }
    if s.p == 0 || s.q == 0 || s.p > s.n || s.q > s.m || s.p > MAX_DEGREE || s.q > MAX_DEGREE {
        return Err(bad("test NURB: need 1 <= degree <= n"));
    }
    if s.cpts.len() != (s.n + 1) * (s.m + 1)
        || s.u.len() != s.n + s.p + 2
        || s.v.len() != s.m + s.q + 2
    {
        return Err(bad("test NURB: wrong sizes"));
    }
    check_points(&s.cpts, "test NURB")?;
    check_knots(&s.u, s.p, "test NURB U")?;
    check_knots(&s.v, s.q, "test NURB V")
}

fn opt(o: &mut Vec<f64>, v: Option<f64>, what: &str) -> Result<(), FfiError> {
    let x = v.unwrap_or(0.0);
    finite(&[x], what)?;
    o.extend([flag(v.is_some()), x]);
    Ok(())
}

fn opt_dom(o: &mut Vec<f64>, d: Option<[f64; 4]>, what: &str) -> Result<(), FfiError> {
    if let Some(d) = d {
        domain(d, false, what)?;
    }
    o.push(flag(d.is_some()));
    o.extend(d.unwrap_or([0.0; 4]));
    Ok(())
}

/// Encodes inputs and the record bytes.
struct Enc {
    o: Vec<f64>,
    bytes: Vec<u8>,
}

impl Enc {
    fn src(&mut self, s: &Src) -> Result<(), FfiError> {
        match s {
            Src::Built(b) => {
                self.o.push(0.0);
                push_surf(&mut self.o, b)
            }
            Src::Record {
                ascii,
                version,
                bytes,
            } => {
                check_record(*ascii, *version, bytes)?;
                let start = self.bytes.len();
                self.bytes.extend_from_slice(bytes);
                self.o.extend([
                    1.0,
                    flag(*ascii),
                    *version as f64,
                    start as f64,
                    bytes.len() as f64,
                ]);
                Ok(())
            }
        }
    }

    fn plane(&mut self, p: &PlaneIn) -> Result<(), FfiError> {
        check_plane(p)?;
        match p {
            PlaneIn::Bounded {
                o,
                x,
                y,
                scale,
                dom,
            } => {
                self.o.push(0.0);
                self.o.extend_from_slice(o);
                self.o.extend_from_slice(x);
                self.o.extend_from_slice(y);
                self.o.extend_from_slice(scale);
                self.o.extend_from_slice(dom);
            }
            PlaneIn::Normal { o, n } => {
                self.o.push(1.0);
                self.o.extend_from_slice(o);
                self.o.extend_from_slice(n);
            }
        }
        Ok(())
    }

    fn cone(&mut self, c: &ConeIn) -> Result<(), FfiError> {
        finite(&c.o, "cone origin")?;
        axes(c.x, c.y, "cone axes")?;
        let (kind, p): (f64, [f64; 6]) = match c.kind {
            ConeKind::Canonical {
                bot,
                top,
                height,
                swap,
                inside,
            } => (0.0, [bot, top, height, flag(swap), flag(inside), 0.0]),
            ConeKind::Semi {
                bot,
                semi,
                height,
                swap,
                inside,
            } => (
                1.0,
                [
                    bot,
                    semi,
                    flag(height.is_some()),
                    height.unwrap_or(0.0),
                    flag(swap),
                    flag(inside),
                ],
            ),
            ConeKind::Cylinder { radius } => (2.0, [radius, 0.0, 0.0, 0.0, 0.0, 0.0]),
        };
        finite(&p, "cone")?;
        self.o.push(kind);
        self.o.extend_from_slice(&c.o);
        self.o.extend_from_slice(&c.x);
        self.o.extend_from_slice(&c.y);
        self.o.extend_from_slice(&p);
        Ok(())
    }

    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::State => {}
            Op::Plane {
                plane,
                dom,
                other_dom,
                tol,
            } => {
                self.plane(plane)?;
                opt_dom(&mut self.o, *dom, "domain")?;
                opt_dom(&mut self.o, *other_dom, "plane domain")?;
                opt(&mut self.o, *tol, "tolerance")?;
            }
            Op::Cylinder { cone, tol } => {
                self.cone(cone)?;
                opt(&mut self.o, *tol, "tolerance")?;
            }
            Op::Extrusion { other, tol } => {
                self.src(other)?;
                opt(&mut self.o, *tol, "tolerance")?;
            }
            Op::Global {
                target,
                dom,
                other_dom,
                tol,
                ang,
            } => {
                match target {
                    Target::Plane(p) => {
                        self.o.push(0.0);
                        self.plane(p)?;
                    }
                    Target::Cone(c) => {
                        self.o.push(1.0);
                        self.cone(c)?;
                    }
                    Target::Extrusion(s) => {
                        self.o.push(2.0);
                        self.src(s)?;
                    }
                    Target::BSpline(s) => {
                        self.o.push(3.0);
                        self.src(s)?;
                    }
                }
                opt_dom(&mut self.o, *dom, "domain")?;
                opt_dom(&mut self.o, *other_dom, "other domain")?;
                opt(&mut self.o, *tol, "tolerance")?;
                opt(&mut self.o, *ang, "angle tolerance")?;
            }
            Op::Trim { dom } => {
                domain(*dom, true, "trim domain")?;
                self.o.extend_from_slice(dom);
            }
        }
        Ok(())
    }

    fn test(&mut self, t: &TestSurf) -> Result<(), FfiError> {
        match &t.source {
            TestSource::Raw(s) => {
                check_nurb(s)?;
                self.o.push(1.0);
                self.o.extend([s.n, s.m, s.p, s.q].map(|x| x as f64));
                for c in &s.cpts {
                    self.o.extend_from_slice(c);
                }
                self.o.extend_from_slice(&s.u);
                self.o.extend_from_slice(&s.v);
            }
            TestSource::Src(s) => {
                self.o.push(0.0);
                self.src(s)?;
            }
        }
        self.o.push(flag(t.transpose));
        if let Some(d) = t.reparam {
            domain(d, true, "reparameterisation domain")?;
        }
        self.o.push(flag(t.reparam.is_some()));
        self.o.extend(t.reparam.unwrap_or([0.0; 4]));
        let (i, j, d) = t.perturb.unwrap_or((0, 0, [0.0; 3]));
        finite(&d, "perturbation")?;
        if i > MAX_INDEX * 4 || j > MAX_INDEX * 4 {
            return Err(bad("perturbation index too large"));
        }
        self.o.push(flag(t.perturb.is_some()));
        self.o.extend([i as f64, j as f64]);
        self.o.extend_from_slice(&d);
        Ok(())
    }
}

fn call(op: i32, input: &[f64], bytes: &[u8]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (built and validated above: p1 surface blocks, records the strict
        // stream simulator reads completely, finite planes, cones and domains, clamped
        // inline NURBs); bytes is valid for bytes.len() reads and every record block names
        // a range inside it; buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_surface_surf_of_extrusion_p2_eval(
                op,
                input.as_ptr(),
                input.len() as i64,
                bytes.as_ptr(),
                bytes.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Makes the surface from `src` and runs `op` on it; returns the shim's record (layout
/// in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn surface_op(src: &Src, op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc {
        o: Vec::new(),
        bytes: Vec::new(),
    };
    e.src(src)?;
    e.op(op)?;
    call(op.code(), &e.o, &e.bytes)
}

/// Runs `IsNurbSurfaceSurfOfExtrusion(test, scale)`; returns the shim's record (layout
/// in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn is_nurb(test: &TestSurf, scale: f64) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc {
        o: Vec::new(),
        bytes: Vec::new(),
    };
    e.test(test)?;
    finite(&[scale], "tolerance scale")?;
    e.o.push(scale);
    call(OP_IS_NURB, &e.o, &e.bytes)
}
