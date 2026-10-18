//! FFI wrappers for unit `intersect.ray_tracer`.
//!
//! Shim: `harness/cpp/shim/intersect_ray_tracer.cpp` (`sc_intersect_ray_tracer_run`).
//! Upstream: `source/SMLib/src/SmRayTracer.cpp`, `source/SMLib/inc/SmRayTracer.h`
//! (`SmRayTracer`, `SmLSI3LocalSolver`).
//!
//! [`run`] builds real `SmPlane` / `SmBSplineSurface` surfaces, an `SmGrid` and an
//! `SmRayTracer` owning it, applies the [`Op`]s in order and returns one flat record: per
//! `Fire` and `Local` op
//!
//! * `Fire`: `status hits <solution> n_elements ids(n) mins(n) maxs(n)`;
//! * `Local`: `status found needs_subdivision <solution>`;
//!
//! where `<solution>` is `type num_variables num_objects solution_value p[0..10] surface
//! object1 node0` (`surface` the surface index or `-1`, `object1` `-1` for null, `node0` `1`
//! when `m_apNodes[0]` is set). `SolverTol`, `AddLeaf` and `AddSurface` add nothing.
//!
//! | [`Op`]       | upstream                                                                    |
//! |--------------|-----------------------------------------------------------------------------|
//! | `SolverTol`  | `SmRayTracer::SetSolverTolerance`                                           |
//! | `AddLeaf`    | `SmGrid::AddElement(SM_GE_SURFACE)` with `SmSurfaceGridElement::Init` (the box data given, the mid-point data computed from the surface) |
//! | `Fire`       | `SmRayTracer::FireRay`                                                      |
//! | `Local`      | `SmLSI3LocalSolver::LocalSolve` on a leaf                                   |
//! | `AddSurface` | `SmRayTracer::AddSurfaceToGrid` (not run: needs the surface cache)          |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`] (ray parameters and
//! tolerances: any finite value); planes with independent axes and positive scales; Bezier
//! B-spline patches of degree 1..=3 with positive weights or none; grids of positive size with
//! at most [`MAX_AXIS`] voxels per axis; leaf domains of positive extent inside the surface's
//! natural domain with non-inverted bounding boxes (a pseudo box may be inverted) and non-zero polar vectors; ray vectors of `Local`
//! ops that are zero or at least [`MIN_LOCAL_VECTOR`] (the Newton iteration divides by it); at most
//! [`MAX_OPS`] ops, [`MAX_LEAVES`] leaves, [`MAX_SURFACES`] surfaces and [`MAX_POLAR`] polar
//! vectors per leaf.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_ray_tracer_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest voxel count per axis.
pub const MAX_AXIS: usize = 20;
/// Largest input magnitude (coordinates).
pub const MAX_COORD: f64 = 1e9;
/// Most ops per program.
pub const MAX_OPS: usize = 64;
/// Most leaves per program.
pub const MAX_LEAVES: usize = 64;
/// Most surfaces per program.
pub const MAX_SURFACES: usize = 4;
/// Most polar vectors per leaf.
pub const MAX_POLAR: usize = 8;
/// Shortest non-zero ray vector of a `Local` op (a zero vector is allowed: `LocalSolve` handles it).
pub const MIN_LOCAL_VECTOR: f64 = 1e-6;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;

/// A surface.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceIn {
    /// A Bezier-knot B-spline surface of degree `p` x `q` (clamped knots `0..0 1..1`);
    /// `net[i][j]` is the control point `x y z w` (`w` = [`NOW`] when non-rational) with `i`
    /// in `0..=p` and `j` in `0..=q`.
    Bezier {
        /// u degree.
        p: usize,
        /// v degree.
        q: usize,
        /// Control net.
        net: Vec<Vec<[f64; 4]>>,
    },
    /// `SmPlane(o, x, y, SmVector2d(scale), SmExtent2d(domain), ctx)`.
    Plane {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// u and v scales.
        scale: [f64; 2],
        /// `umin vmin umax vmax`.
        domain: [f64; 4],
    },
}

/// The grid and tracer constructor arguments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridIn {
    /// `xmin ymin zmin xmax ymax zmax`.
    pub bbox: [f64; 6],
    /// Voxels along x, y, z.
    pub size: [usize; 3],
    /// `dRayTolerance`.
    pub ray_tolerance: f64,
    /// `dRayAccuracy`.
    pub ray_accuracy: f64,
}

/// One surface subdivision leaf added to the grid.
#[derive(Debug, Clone, PartialEq)]
pub struct LeafIn {
    /// Index of the surface.
    pub surface: usize,
    /// The leaf's parameter domain `umin vmin umax vmax`.
    pub uv_domain: [f64; 4],
    /// The leaf's bounding box (`SmSurfaceGridElement::m_sBBox`).
    pub bbox: [f64; 6],
    /// The leaf's pseudo box, `SmPseudoBox(SmExtent3d)` (may be inverted); with
    /// `pseudo_basis` set, the intervals along the basis directions instead
    /// (`xmin ymin zmin xmax ymax zmax` read as one interval per axis).
    pub pseudo_box: [f64; 6],
    /// An orthonormal basis for a rotated pseudo box, `SmPseudoBox(x, y, z, xIvl, yIvl,
    /// zIvl)`; `None` for the axis-aligned `SmPseudoBox(SmExtent3d)`.
    pub pseudo_basis: Option<[[f64; 3]; 3]>,
    /// The polar box vectors: the first is the constructor's, the others `AddVector`.
    pub polar: Vec<[f64; 3]>,
    /// The box the element is registered with in the grid voxels.
    pub elem_box: [f64; 6],
}

/// A `FireRay` call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FireIn {
    /// Ray start.
    pub point: [f64; 3],
    /// Ray direction.
    pub vector: [f64; 3],
    /// `dMaxRayParameter`.
    pub max_t: f64,
    /// `m_bHitAnyThing`.
    pub hit_any: bool,
    /// `m_pSkipElement`: a leaf index.
    pub skip: Option<usize>,
}

/// A `LocalSolve` call on a leaf with a solver built by the full constructor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalIn {
    /// Leaf index.
    pub leaf: usize,
    /// `crLinePoint`.
    pub point: [f64; 3],
    /// `crLineVector`.
    pub vector: [f64; 3],
    /// `m_d3dTolerance`.
    pub d3d_tolerance: f64,
    /// `dDistanceTolerance`.
    pub distance_tolerance: f64,
    /// `dRayAccuracy`.
    pub ray_accuracy: f64,
    /// `bRayFire`.
    pub ray_fire: bool,
    /// `m_bHaveTMinTMax`.
    pub have_t_min_t_max: bool,
    /// `m_dTMinOfBBox`.
    pub t_min: f64,
    /// `m_dTMaxOfBBox`.
    pub t_max: f64,
    /// `cpLineInterval` (`None` = null).
    pub interval: Option<[f64; 2]>,
}

/// One operation (see the module table).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `SetSolverTolerance`.
    SolverTol(f64),
    /// Adds a leaf.
    AddLeaf(LeafIn),
    /// `FireRay`.
    Fire(FireIn),
    /// `LocalSolve`.
    Local(LocalIn),
    /// `AddSurfaceToGrid` of a surface index (not run).
    AddSurface(usize),
    /// `SmRayRenderer(tracer, SmAxis2Placement(origin, x, y), view, nx, ny)` taking over the
    /// tracer, then `SetSampleRate`; once per program.
    Renderer(RendererIn),
    /// `AddLightSource(new SmLightSource(vector, is_point))`.
    Light {
        /// Direction or origin.
        vector: [f64; 3],
        /// Point source.
        is_point: bool,
    },
    /// `DoRender(shadows, background, object colour)`: record `status pixels_shaded`.
    DoRender {
        /// Cast shadows.
        shadows: bool,
        /// Background colour.
        background: [f64; 3],
        /// Object colour.
        object: [f64; 3],
    },
    /// `RenderPixel(x, y)` (after `DoRender`): record `status r g b pixels_shaded`.
    RenderPixel(usize, usize),
    /// `RenderRay(point, vector)` (after `DoRender`): record `status r g b pixels_shaded`.
    RenderRay([f64; 3], [f64; 3]),
}

/// The renderer constructor arguments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RendererIn {
    /// Origin of the geometry transform.
    pub origin: [f64; 3],
    /// Unit X axis.
    pub x: [f64; 3],
    /// Unit Y axis, perpendicular to X.
    pub y: [f64; 3],
    /// The view volume box.
    pub view: [f64; 6],
    /// Pixels in x (1..=8).
    pub nx: usize,
    /// Pixels in y (1..=8).
    pub ny: usize,
    /// Samples per pixel direction (1..=3).
    pub rate: usize,
}

/// A whole case.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The surfaces.
    pub surfaces: Vec<SurfaceIn>,
    /// The grid and tracer.
    pub grid: GridIn,
    /// The ops, in order.
    pub ops: Vec<Op>,
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("value not finite or too large"))
    }
}

/// Any finite value (tolerances, ray parameters).
fn any_finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(bad("value not finite"))
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The `(min, max)` of each axis of a box `xmin ymin zmin xmax ymax zmax`.
fn axes(b: &[f64; 6]) -> [(f64, f64); 3] {
    let [x0, y0, z0, x1, y1, z1] = *b;
    [(x0, x1), (y0, y1), (z0, z1)]
}

struct Enc(Vec<f64>);

impl Enc {
    fn num(&mut self, x: f64) -> Result<(), FfiError> {
        self.0.push(finite(x)?);
        Ok(())
    }
    fn any(&mut self, x: f64) -> Result<(), FfiError> {
        self.0.push(any_finite(x)?);
        Ok(())
    }
    fn int(&mut self, x: usize) {
        self.0.push(x as f64);
    }
    fn flag(&mut self, b: bool) {
        self.0.push(if b { 1.0 } else { 0.0 });
    }
    fn nums(&mut self, v: &[f64]) -> Result<(), FfiError> {
        v.iter().try_for_each(|&x| self.num(x))
    }

    /// A box `min max` that must not be inverted; its coordinates may be huge (grid boxes).
    fn bbox(&mut self, b: &[f64; 6]) -> Result<(), FfiError> {
        self.nums(b)?;
        if axes(b).iter().any(|&(lo, hi)| lo > hi) {
            return Err(bad("inverted box"));
        }
        Ok(())
    }

    /// The surface block; returns its natural domain.
    fn surface(&mut self, s: &SurfaceIn) -> Result<[f64; 4], FfiError> {
        match s {
            SurfaceIn::Bezier { p, q, net } => {
                if !(1..=3).contains(p) || !(1..=3).contains(q) {
                    return Err(bad("degree"));
                }
                if net.len() != p + 1 || net.iter().any(|r| r.len() != q + 1) {
                    return Err(bad("control net size"));
                }
                let rational = net.first().and_then(|r| r.first()).map(|c| c[3] != NOW);
                let ok_w = |w: f64| match rational {
                    Some(true) => w > 0.0 && w <= MAX_COORD,
                    Some(false) => w == NOW,
                    None => false,
                };
                self.0.push(0.0);
                self.int(*p);
                self.int(*q);
                for c in net.iter().flatten() {
                    if !ok_w(c[3]) {
                        return Err(bad("weights"));
                    }
                    self.nums(&c[..3])?;
                    self.0.push(c[3]);
                }
                Ok([0.0, 0.0, 1.0, 1.0])
            }
            SurfaceIn::Plane {
                o,
                x,
                y,
                scale,
                domain,
            } => {
                let c = norm(cross(*x, *y));
                let independent = c > 1e-6 * norm(*x) * norm(*y);
                if !independent || norm(*x) < 1e-9 || norm(*y) < 1e-9 {
                    return Err(bad("plane axes"));
                }
                if !(scale[0] > 1e-9 && scale[1] > 1e-9) {
                    return Err(bad("plane scale"));
                }
                if !(domain[0] < domain[2] && domain[1] < domain[3]) {
                    return Err(bad("plane domain"));
                }
                self.0.push(1.0);
                self.nums(o)?;
                self.nums(x)?;
                self.nums(y)?;
                self.nums(scale)?;
                self.nums(domain)?;
                Ok(*domain)
            }
        }
    }
}

/// How far the renderer ops of a program have got.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Render {
    None,
    Built,
    Done,
}

fn encode(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc(Vec::new());
    let ns = prog.surfaces.len();
    if !(1..=MAX_SURFACES).contains(&ns) {
        return Err(bad("surface count"));
    }
    e.int(ns);
    let mut naturals = Vec::new();
    for s in &prog.surfaces {
        naturals.push(e.surface(s)?);
    }

    let g = &prog.grid;
    e.bbox(&g.bbox)?;
    for (lo, hi) in axes(&g.bbox) {
        // GetVoxelAddress converts NaN or a negative value to an index otherwise
        let size = hi - lo;
        if size <= 0.0 || size * 1e-12 <= 0.0 {
            return Err(bad("grid box must have positive size"));
        }
    }
    for &n in &g.size {
        if !(1..=MAX_AXIS).contains(&n) {
            return Err(bad("voxel counts"));
        }
        e.int(n);
    }
    e.any(g.ray_tolerance)?;
    e.any(g.ray_accuracy)?;

    if prog.ops.len() > MAX_OPS {
        return Err(bad("too many ops"));
    }
    e.int(prog.ops.len());
    let mut leaves = 0_usize;
    let mut rendered = Render::None;
    for op in &prog.ops {
        match op {
            Op::SolverTol(t) => {
                e.int(1);
                e.any(*t)?;
            }
            Op::AddLeaf(l) => {
                if leaves >= MAX_LEAVES {
                    return Err(bad("too many leaves"));
                }
                leaves += 1;
                let nat = naturals
                    .get(l.surface)
                    .ok_or_else(|| bad("leaf surface index"))?;
                let d = &l.uv_domain;
                if !(d[0] < d[2] && d[1] < d[3])
                    || d[0] < nat[0]
                    || d[1] < nat[1]
                    || d[2] > nat[2]
                    || d[3] > nat[3]
                {
                    return Err(bad("leaf domain"));
                }
                if l.polar.is_empty() || l.polar.len() > MAX_POLAR {
                    return Err(bad("polar vectors"));
                }
                if l.polar.iter().any(|v| norm(*v) < 1e-6) {
                    return Err(bad("zero polar vector"));
                }
                e.int(2);
                e.int(l.surface);
                e.nums(d)?;
                e.bbox(&l.bbox)?;
                e.nums(&l.pseudo_box)?;
                match &l.pseudo_basis {
                    Some(b) => {
                        if axes(&l.pseudo_box).iter().any(|&(lo, hi)| lo > hi) {
                            return Err(bad("inverted rotated pseudo box"));
                        }
                        let dot =
                            |a: [f64; 3], c: [f64; 3]| a[0] * c[0] + a[1] * c[1] + a[2] * c[2];
                        for (i, v) in b.iter().enumerate() {
                            if (norm(*v) - 1.0).abs() > 1e-9 {
                                return Err(bad("pseudo basis must be orthonormal"));
                            }
                            if let Some(w) = b.get(i + 1..) {
                                if w.iter().any(|w| dot(*v, *w).abs() > 1e-9) {
                                    return Err(bad("pseudo basis must be orthonormal"));
                                }
                            }
                        }
                        if dot(cross(b[0], b[1]), b[2]) <= 0.0 {
                            return Err(bad("pseudo basis must be right-handed"));
                        }
                        e.flag(true);
                        for v in b {
                            e.nums(v)?;
                        }
                    }
                    None => e.flag(false),
                }
                e.int(l.polar.len());
                for v in &l.polar {
                    e.nums(v)?;
                }
                e.bbox(&l.elem_box)?;
            }
            Op::Fire(f) => {
                if let Some(s) = f.skip {
                    if s >= leaves {
                        return Err(bad("skip index"));
                    }
                }
                e.int(3);
                e.nums(&f.point)?;
                e.nums(&f.vector)?;
                e.any(f.max_t)?;
                e.flag(f.hit_any);
                match f.skip {
                    Some(s) => e.int(s),
                    None => e.0.push(-1.0),
                }
            }
            Op::Local(l) => {
                if l.leaf >= leaves {
                    return Err(bad("leaf index"));
                }
                let len = norm(l.vector);
                if len != 0.0 && len < MIN_LOCAL_VECTOR {
                    return Err(bad("ray vector too short"));
                }
                e.int(4);
                e.int(l.leaf);
                e.nums(&l.point)?;
                e.nums(&l.vector)?;
                e.any(l.d3d_tolerance)?;
                e.any(l.distance_tolerance)?;
                e.any(l.ray_accuracy)?;
                e.flag(l.ray_fire);
                e.flag(l.have_t_min_t_max);
                e.any(l.t_min)?;
                e.any(l.t_max)?;
                e.flag(l.interval.is_some());
                let [i0, i1] = l.interval.unwrap_or([0.0, 0.0]);
                e.any(i0)?;
                e.any(i1)?;
            }
            Op::AddSurface(s) => {
                if *s >= ns {
                    return Err(bad("surface index"));
                }
                e.int(5);
                e.int(*s);
            }
            Op::Renderer(r) => {
                if rendered != Render::None {
                    return Err(bad("one renderer per program"));
                }
                rendered = Render::Built;
                let dot = r.x[0] * r.y[0] + r.x[1] * r.y[1] + r.x[2] * r.y[2];
                if (norm(r.x) - 1.0).abs() > 1e-9
                    || (norm(r.y) - 1.0).abs() > 1e-9
                    || dot.abs() > 1e-9
                {
                    return Err(bad("renderer axes must be orthonormal"));
                }
                if !(1..=8).contains(&r.nx)
                    || !(1..=8).contains(&r.ny)
                    || !(1..=3).contains(&r.rate)
                {
                    return Err(bad("renderer size"));
                }
                e.int(6);
                e.nums(&r.origin)?;
                e.nums(&r.x)?;
                e.nums(&r.y)?;
                e.bbox(&r.view)?;
                e.int(r.nx);
                e.int(r.ny);
                e.int(r.rate);
            }
            Op::Light { vector, is_point } => {
                if rendered == Render::None {
                    return Err(bad("light before renderer"));
                }
                e.int(7);
                e.nums(vector)?;
                e.flag(*is_point);
            }
            Op::DoRender {
                shadows,
                background,
                object,
            } => {
                if rendered == Render::None {
                    return Err(bad("render before renderer"));
                }
                rendered = Render::Done;
                e.int(8);
                e.flag(*shadows);
                e.nums(background)?;
                e.nums(object)?;
            }
            Op::RenderPixel(x, y) => {
                if rendered != Render::Done || *x > 7 || *y > 7 {
                    return Err(bad("pixel render needs DoRender first"));
                }
                e.int(9);
                e.int(*x);
                e.int(*y);
            }
            Op::RenderRay(p, v) => {
                if rendered != Render::Done {
                    return Err(bad("ray render needs DoRender first"));
                }
                e.int(10);
                e.nums(p)?;
                e.nums(v)?;
            }
        }
    }
    Ok(e.0)
}

/// Runs a program through upstream and returns its record.
pub fn run(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let input = encode(prog)?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: input valid for input.len() reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_intersect_ray_tracer_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
