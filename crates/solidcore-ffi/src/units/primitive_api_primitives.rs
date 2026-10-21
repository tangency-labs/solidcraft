//! FFI wrappers for unit `primitive.api_primitives`.
//!
//! Shim: `harness/cpp/shim/primitive_api_primitives.cpp` (`sc_primitive_api_primitives_run`).
//! Upstream: `source/SM_API/src/SmApiPrimitives.cpp` (`SmApiCreatePlane` (both overloads),
//! `SmApiCreatePlanarCircle`, `SmApiCreateBox`, `SmApiCreateSphere`, `SmApiCreateCylinder`
//! (both), `SmApiCreateCone` (both), `SmApiCreateTorus` (both), `SmApiCreatePyramid`,
//! `SmApiCreateCylindricalBox`, `SmApiCreatePartialSphere`, `SmApiCreateSphereNoPole`,
//! `SmApiCreatePartialCone`, `SmApiCreateConeEllipticEnds`, `SmApiCreatePartialCylinder`,
//! `SmApiCreatePartialTorus`, `SmApiCreateSwungPrimitive`).
//!
//! A call is `op args...` with the arguments in the API's order (`O[3]` = the origin,
//! center or base center):
//!
//! | [`Op`]              | op | args                                   |
//! |---------------------|----|----------------------------------------|
//! | `PlaneSurface`      | 1  | `O[3] length width`                    |
//! | `CylinderSurface`   | 2  | `O[3] radius height 0`                 |
//! | `ConeSurface`       | 3  | `O[3] radius_base radius_top height`   |
//! | `TorusSurface`      | 4  | `O[3] major minor 0`                   |
//! | `PlaneBrep`         | 10 | `O[3] x y`                             |
//! | `PlanarCircle`      | 11 | `O[3] radius`                          |
//! | `Box`               | 12 | `O[3] length width height`             |
//! | `Sphere`            | 13 | `O[3] radius`                          |
//! | `Cylinder`          | 14 | `O[3] radius height`                   |
//! | `Cone`              | 15 | `O[3] radius_base radius_top height`   |
//! | `Torus`             | 16 | `O[3] major minor`                     |
//! | `Pyramid`           | 17 | `O[3] length height`                   |
//! | `CylindricalBox`    | 18 | `O[3] length inside outside start end` |
//! | `PartialSphere`     | 19 | `O[3] radius start end`                |
//! | `SphereNoPole`      | 20 | `O[3] radius`                          |
//! | `PartialCone`       | 21 | `O[3] radius_base radius_top height start end` |
//! | `ConeEllipticEnds`  | 22 | `front rear height fyt ryt fxt rxt caps` |
//! | `PartialCylinder`   | 23 | `O[3] radius height start end`         |
//! | `PartialTorus`      | 24 | `O[3] major minor start end`           |
//! | `Swung`             | 25 | `k` x `C[3] r a0 a1` (XY arcs), `k` x arcs (XZ), `scale` |
//!
//! The swung curves are `SmApiCreateArc(C, r, a0, a1)` B-splines.
//!
//! The output is the API status (`0` = `SM_SUCCESS`, else the `SmStatus`), then on success
//! `has` and the result record. A B-rep's record is its face, edge and vertex counts. A
//! surface's record is its `GetType`, position (`O X Y Z`), STEP UV domain, the kind's
//! values (plane: the UV scale; cylinder / cone: bottom and top radius, inside-out and
//! swap-UV flags; torus: major and minor radius, inside-out and swap-UV flags), its NURB
//! (`has n m p q r s`, the `Pw` net row by row, the U and V knots; the plane's made by
//! `GetOrCreateGwNurbPointer`), then `finite` and, for a finite STEP domain, a 5 x 5 grid
//! over it of `status [x y z]` (`EvaluatePoint` for the plane, else `EvaluateSTEPPoint`).
//!
//! [`run`] checks the program's shape (argument counts, the `caps` flag, at most
//! [`MAX_ARCS`] arcs per list; the swung arcs must be ones `SmApiCreateArc` accepts, a
//! failing constructor is `SC_ERR_ARG`) and, with [`guard`], fences off the inputs on which
//! upstream crashes or does not return (`plan/upstream-issues.md`). Inputs an entry point
//! rejects itself (`SM_ERR_INVALID_INPUT`) always pass: the guard applies only past the
//! API's own validation, where every B-rep entry point calls the unported
//! `SmPrimitiveCreation` (the Rust side is deferred, so nothing compared is lost):
//! - `PlaneSurface`: a NaN length or width reaches `SmExtent2d::SetMinMax` (which keeps it)
//!   and then the NLib clamp of the plane's NURB, which indexes outside its knot arrays
//!   (a NaN length segfaults; a NaN width returns a plane built from garbage);
//! - `TorusSurface`: with both radii above `SM_EFF_ZERO`, a non-finite origin, an infinite
//!   radius, radii whose sum overflows or a minor radius within `1e-8` (relative) of
//!   `DBL_MAX` segfault in `SmTorus::CreateCanonical` (a radius not above `SM_EFF_ZERO`,
//!   NaN included, is rejected there first and always passes);
//! - `Sphere`: a non-finite center, or a radius magnitude above [`MAX_SPHERE_RADIUS`]
//!   (about `7e9` and up segfault in `SmPrimitiveCreation::CreateSphere`);
//! - `SphereNoPole`: radius magnitudes below [`MIN_NO_POLE_RADIUS`] (crashes or hangs,
//!   erratically, from about `1e-4` down) or above [`MAX_NO_POLE_RADIUS`] (segfaults from
//!   about `4e8`) in `SmPrimitiveCreation::CreateSphereNoPole`;
//! - `CylindricalBox`: `SmPrimitiveCreation::CreateCylindricalBox` hangs or segfaults when
//!   its arcs nearly degenerate: a non-finite origin or one farther than
//!   [`MAX_KERNEL_COORD`], an inside arc whose chord `ri * sweep` (radians) is below
//!   [`MIN_ARC_CHORD`] or whose sagitta `ri * sweep^2 / 8` is below [`MIN_ARC_SAGITTA`], or
//!   a sweep within [`MIN_FULL_TURN_GAP`] degrees of a full turn (e.g. `0 .. 360`,
//!   `-90 .. 270`, `0 .. 360 - 1e-12` never return; `0 .. 0.01` at radius 1 and
//!   `0 .. 0.2` at radius `1e-3` never return);
//! - the entry points without validation (`PlaneBrep`, `PlanarCircle`, `Torus`,
//!   `PartialSphere`, `PartialCone`, `ConeEllipticEnds`, `PartialCylinder`,
//!   `PartialTorus`, `Swung`): every argument (and arc value) finite and at most
//!   [`MAX_KERNEL_COORD`] in magnitude (a non-finite origin segfaults `CreateTorus`).
//!
//! The kernel-side bounds are conservative where the crashes are erratic: they may also
//! reject some inputs upstream survives, all of them deferred on the Rust side.

use crate::{call_with_buffer, FfiError};

/// Most arcs in one curve list of `Swung`.
pub const MAX_ARCS: usize = 16;
/// Largest sphere radius magnitude passed on to `SmPrimitiveCreation::CreateSphere`.
pub const MAX_SPHERE_RADIUS: f64 = 6.0e9;
/// Smallest radius magnitude passed on to `SmPrimitiveCreation::CreateSphereNoPole`.
pub const MIN_NO_POLE_RADIUS: f64 = 3.0e-4;
/// Largest radius magnitude passed on to `SmPrimitiveCreation::CreateSphereNoPole`.
pub const MAX_NO_POLE_RADIUS: f64 = 3.0e8;
/// Largest coordinate or dimension magnitude passed on to the kernel by the entry points
/// without validation, and largest origin coordinate of `CylindricalBox`.
pub const MAX_KERNEL_COORD: f64 = 1.0e6;
/// Smallest inside-arc chord (`ri * sweep`, sweep in radians) of `CylindricalBox`.
pub const MIN_ARC_CHORD: f64 = 1.0e-4;
/// Smallest inside-arc sagitta (`ri * sweep^2 / 8`) of `CylindricalBox`.
pub const MIN_ARC_SAGITTA: f64 = 1.0e-8;
/// Smallest gap (degrees) between a `CylindricalBox` sweep and a full turn.
pub const MIN_FULL_TURN_GAP: f64 = 1.0e-8;

/// `SM_EFF_ZERO`.
const EFF_ZERO: f64 = 1.0e-12;

/// `SM_IS_VALID_DOUBLE`: not NaN, finite and within `SM_BIG_DOUBLE / 100`.
fn valid_double(a: f64) -> bool {
    (-1.0e18..=1.0e18).contains(&a)
}

fn bad(m: &str) -> FfiError {
    FfiError::InvalidArgument(m.into())
}

fn bounded(xs: &[f64], what: &str) -> Result<(), FfiError> {
    if xs
        .iter()
        .all(|x| x.is_finite() && x.abs() <= MAX_KERNEL_COORD)
    {
        Ok(())
    } else {
        Err(bad(what))
    }
}

/// Rejects the inputs on which upstream crashes or does not return (see the module docs).
/// `args` as for [`run`]; `arcs` the swung arc lists.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] naming the fenced-off input.
pub fn guard(op: Op, args: &[f64], arcs: Option<(&[Arc], &[Arc])>) -> Result<(), FfiError> {
    let a = |i: usize| args.get(i).copied().unwrap_or(f64::NAN);
    let origin = [a(0), a(1), a(2)];
    match op {
        Op::PlaneSurface => {
            if a(3).is_nan() || a(4).is_nan() {
                return Err(bad("plane surface: NaN length or width"));
            }
        }
        Op::TorusSurface => {
            let (major, minor) = (a(3), a(4));
            // `SmTorus::CreateCanonical` rejects (`SM_ERR`) any radius not above
            // `SM_EFF_ZERO` (NaN included) before it touches the origin or builds the NURB.
            if major > EFF_ZERO && minor > EFF_ZERO {
                if !origin.iter().all(|x| x.is_finite())
                    || major == f64::INFINITY
                    || minor == f64::INFINITY
                {
                    return Err(bad("torus surface: non-finite origin or infinite radius"));
                }
                if (major + minor).is_infinite() || minor > f64::MAX * (1.0 - 1.0e-8) {
                    return Err(bad("torus surface: radii sum near DBL_MAX"));
                }
            }
        }
        Op::Sphere => {
            let r = a(3);
            let valid = valid_double(r) && r.abs() > EFF_ZERO;
            if valid && (!origin.iter().all(|x| x.is_finite()) || r.abs() > MAX_SPHERE_RADIUS) {
                return Err(bad("sphere: kernel crash domain"));
            }
        }
        Op::SphereNoPole => {
            let r = a(3).abs();
            let valid = valid_double(a(3)) && r > EFF_ZERO;
            if valid && !(MIN_NO_POLE_RADIUS..=MAX_NO_POLE_RADIUS).contains(&r) {
                return Err(bad("sphere no pole: kernel crash domain"));
            }
        }
        Op::CylindricalBox => {
            let (len, ri, ro, a0, a1) = (a(3), a(4), a(5), a(6), a(7));
            let positive = |v: f64| valid_double(v) && v > EFF_ZERO;
            let valid = positive(len)
                && positive(ri)
                && valid_double(ro)
                && ro > ri + EFF_ZERO
                && valid_double(a0)
                && valid_double(a1)
                && (-360.0..=360.0).contains(&a0)
                && a1 > a0 + EFF_ZERO
                && a1 - a0 <= 360.0;
            if valid {
                let sweep = a1 - a0;
                let theta = sweep.to_radians();
                if !origin
                    .iter()
                    .all(|x| x.is_finite() && x.abs() <= MAX_KERNEL_COORD)
                    || ri * theta < MIN_ARC_CHORD
                    || ri * theta * theta / 8.0 < MIN_ARC_SAGITTA
                    || 360.0 - sweep < MIN_FULL_TURN_GAP
                {
                    return Err(bad("cylindrical box: kernel hang domain"));
                }
            }
        }
        Op::PlaneBrep
        | Op::PlanarCircle
        | Op::Torus
        | Op::PartialSphere
        | Op::PartialCone
        | Op::ConeEllipticEnds
        | Op::PartialCylinder
        | Op::PartialTorus => bounded(args, "unvalidated entry point: argument out of bounds")?,
        Op::Swung => {
            bounded(args, "swung: scale out of bounds")?;
            if let Some((xy, xz)) = arcs {
                for c in xy.iter().chain(xz) {
                    bounded(&c.center, "swung: arc out of bounds")?;
                    bounded(
                        &[c.radius, c.start_deg, c.end_deg],
                        "swung: arc out of bounds",
                    )?;
                }
            }
        }
        Op::CylinderSurface | Op::ConeSurface | Op::Box | Op::Cylinder | Op::Cone | Op::Pyramid => {
        }
    }
    Ok(())
}

mod sys {
    extern "C" {
        pub fn sc_primitive_api_primitives_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The API entry point of a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SmApiCreatePlane(O, length, width, SmPlane*&)`.
    PlaneSurface = 1,
    /// `SmApiCreateCylinder(O, radius, height, SmSurface*&)`.
    CylinderSurface = 2,
    /// `SmApiCreateCone(O, radius_base, radius_top, height, SmSurface*&)`.
    ConeSurface = 3,
    /// `SmApiCreateTorus(O, major, minor, SmSurface*&)`.
    TorusSurface = 4,
    /// `SmApiCreatePlane(O, x, y, SmBrep*&)`.
    PlaneBrep = 10,
    /// `SmApiCreatePlanarCircle(O, radius)`.
    PlanarCircle = 11,
    /// `SmApiCreateBox(O, length, width, height)`.
    Box = 12,
    /// `SmApiCreateSphere(O, radius)`.
    Sphere = 13,
    /// `SmApiCreateCylinder(O, radius, height, SmBrep*&)`.
    Cylinder = 14,
    /// `SmApiCreateCone(O, radius_base, radius_top, height, SmBrep*&)`.
    Cone = 15,
    /// `SmApiCreateTorus(O, major, minor, SmBrep*&)`.
    Torus = 16,
    /// `SmApiCreatePyramid(O, length, height)`.
    Pyramid = 17,
    /// `SmApiCreateCylindricalBox(O, length, inside, outside, start, end)`.
    CylindricalBox = 18,
    /// `SmApiCreatePartialSphere(O, radius, start, end)`.
    PartialSphere = 19,
    /// `SmApiCreateSphereNoPole(O, radius)`.
    SphereNoPole = 20,
    /// `SmApiCreatePartialCone(O, radius_base, radius_top, height, start, end)`.
    PartialCone = 21,
    /// `SmApiCreateConeEllipticEnds(front, rear, height, fyt, ryt, fxt, rxt, caps)`.
    ConeEllipticEnds = 22,
    /// `SmApiCreatePartialCylinder(O, radius, height, start, end)`.
    PartialCylinder = 23,
    /// `SmApiCreatePartialTorus(O, major, minor, start, end)`.
    PartialTorus = 24,
    /// `SmApiCreateSwungPrimitive(xy, xz, scale)`.
    Swung = 25,
}

impl Op {
    /// The number of `f64` arguments after the op (for `Swung`: the scale only).
    #[must_use]
    pub const fn arity(self) -> usize {
        match self {
            Self::CylinderSurface
            | Self::ConeSurface
            | Self::TorusSurface
            | Self::Box
            | Self::Cone
            | Self::PartialSphere => 6,
            Self::PlaneSurface | Self::PlaneBrep | Self::Cylinder | Self::Torus | Self::Pyramid => {
                5
            }
            Self::PlanarCircle | Self::Sphere | Self::SphereNoPole => 4,
            Self::CylindricalBox | Self::PartialCone | Self::ConeEllipticEnds => 8,
            Self::PartialCylinder | Self::PartialTorus => 7,
            Self::Swung => 1,
        }
    }
}

/// An `SmApiCreateArc(center, radius, start_deg, end_deg)` curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    /// Center.
    pub center: [f64; 3],
    /// Radius.
    pub radius: f64,
    /// Start angle (degrees).
    pub start_deg: f64,
    /// End angle (degrees).
    pub end_deg: f64,
}

fn push_arcs(prog: &mut Vec<f64>, arcs: &[Arc]) {
    prog.push(arcs.len() as f64);
    for a in arcs {
        prog.extend_from_slice(&a.center);
        prog.extend([a.radius, a.start_deg, a.end_deg]);
    }
}

/// Runs `op` on `args` (the arguments in the table's order; for `Swung` the scale) and,
/// for `Swung`, the XY and XZ arc lists. Returns the output record (status first).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a wrong argument count, a `caps` flag not `0`/`1`,
/// arcs given to another op or missing for `Swung`, more than [`MAX_ARCS`] arcs in a
/// list, or an input [`guard`] rejects; [`FfiError::Status`] for a shim error (`SC_ERR_ARG` when an arc constructor
/// fails).
pub fn run(op: Op, args: &[f64], arcs: Option<(&[Arc], &[Arc])>) -> Result<Vec<f64>, FfiError> {
    if args.len() != op.arity() {
        return Err(FfiError::InvalidArgument(format!(
            "{op:?} takes {} arguments, got {}",
            op.arity(),
            args.len()
        )));
    }
    if op == Op::ConeEllipticEnds {
        let caps = args.get(7).copied().unwrap_or(f64::NAN);
        if caps != 0.0 && caps != 1.0 {
            return Err(FfiError::InvalidArgument("caps".into()));
        }
    }
    guard(op, args, arcs)?;
    let mut prog = vec![f64::from(op as i32)];
    match (op, arcs) {
        (Op::Swung, Some((xy, xz))) => {
            if xy.len() > MAX_ARCS || xz.len() > MAX_ARCS {
                return Err(FfiError::InvalidArgument("too many arcs".into()));
            }
            push_arcs(&mut prog, xy);
            push_arcs(&mut prog, xz);
        }
        (Op::Swung, None) | (_, Some(_)) => {
            return Err(FfiError::InvalidArgument("arcs".into()));
        }
        (_, None) => {}
    }
    prog.extend_from_slice(args);
    call_with_buffer(4096, |buf, len| {
        // SAFETY: prog is valid for prog.len() reads; buf valid for buf.len() writes; len
        // valid.
        unsafe {
            sys::sc_primitive_api_primitives_run(
                prog.as_ptr(),
                prog.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
