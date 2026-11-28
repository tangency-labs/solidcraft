//! Surface-to-surface space deformation ([`SurfaceToSurfaceMap`]).
//!
//! Given a base surface `S_b(u, v)` and a control (target) surface `S_c(u, v)`, each
//! normalized to `[0, 1] x [0, 1]`:
//! 1. Project `P` onto `S_b` (either by closest point / normal or along a fixed unit
//!    direction) to get `(u_b, v_b)`, foot point `B = S_b(u_b, v_b)`, and unit normal `n_b`.
//! 2. Map the normalized `(u, v)` (with optional `swap_uv`, `flip_u`, `flip_v`) to `(u_c, v_c)`
//!    on `S_c`, obtaining `C = S_c(u_c, v_c)` and unit normal `n_c`.
//! 3. Compute the mapped point `P'` either by [`MapMode::NormalToNormal`]
//!    (`C + h * n_c + residual`) or [`MapMode::Translate`] (`C + (P - B)`), then blend with
//!    `scale`: `P_out = P + scale * (P' - P)`.

use crate::blend::fillet_section::certificate::nurbs_partials_over_box;
use crate::blend::fillet_section::interval::{round_down, round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{as_b_spline, Nurbs};
use crate::deform::{Box3, Jacobian3, JetEnclosure, SpaceMap};
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::krawczyk::{contraction, inverse_of_columns2, k_operator};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::intersect::adv_surface_intersector::NoSurfaceHandle;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::nurbs::math::{gamma, NOW};
use crate::query::distance_bound::pieces_about;
use crate::surface::implicit::AnalyticSupport;
use crate::surface::plane::Plane;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::surface_cache::p3::definition_bound;

const OP_NEW: &str = "deform::surface_map::SurfaceToSurfaceMap::new";
const OP_MAP: &str = "deform::surface_map::SurfaceToSurfaceMap::map_point";

const IZ: Ivl = Ivl::new(0.0, 0.0);
const VZ: V3<Ivl> = V3 {
    x: IZ,
    y: IZ,
    z: IZ,
};

/// Whether `d` defines a valid finite non-zero projection direction, independent of scale.
#[must_use]
pub fn is_valid_direction(d: Vec3) -> bool {
    normalize_direction(d).is_some()
}

/// Scale-safe normalization of a finite non-zero direction vector.
fn normalize_direction(d: Vec3) -> Option<Vec3> {
    if !(d.x.is_finite() && d.y.is_finite() && d.z.is_finite()) {
        return None;
    }
    let m = d.x.abs().max(d.y.abs()).max(d.z.abs());
    if !m.is_finite() || m == 0.0 {
        return None;
    }
    let s = Vec3::new(d.x / m, d.y / m, d.z / m);
    let len = (s.x * s.x + s.y * s.y + s.z * s.z).sqrt();
    if !len.is_finite() || len == 0.0 {
        return None;
    }
    let unit = Vec3::new(s.x / len, s.y / len, s.z / len);
    (unit.x.is_finite() && unit.y.is_finite() && unit.z.is_finite()).then_some(unit)
}

/// How points in 3D space are projected onto the base surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectionMode {
    /// Closest-point (normal) projection onto the base surface.
    Normal,
    /// Parallel projection along a fixed non-zero direction vector.
    Direction(Vec3),
}

/// How the offset from the base surface is carried onto the control surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapMode {
    /// Preserve signed normal distance along the control surface's unit normal.
    NormalToNormal,
    /// Preserve the 3D displacement vector `P - S_b(u, v)` directly (`P + (S_c - S_b)`).
    Translate,
}

/// Options controlling a [`SurfaceToSurfaceMap`].
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceToSurfaceOptions {
    /// Projection onto the base surface.
    pub projection: ProjectionMode,
    /// How the offset from the base surface is mapped onto the control surface.
    pub map_mode: MapMode,
    /// Deformation scale factor (`1.0` = full target, `0.0` = identity, `> 1.0` = over-crown).
    pub scale: f64,
    /// Reverse the normalized `u` parameter between base and control surfaces.
    pub flip_u: bool,
    /// Reverse the normalized `v` parameter between base and control surfaces.
    pub flip_v: bool,
    /// Swap normalized `(u, v)` between base and control surfaces.
    pub swap_uv: bool,
    /// Whether the base surface's logical normal matches its natural normal.
    pub base_forward: bool,
    /// Whether the control surface's logical normal matches its natural normal.
    pub control_forward: bool,
    /// Solver tolerance for point projection onto the base surface.
    pub tolerance: f64,
}

impl Default for SurfaceToSurfaceOptions {
    fn default() -> Self {
        Self {
            projection: ProjectionMode::Normal,
            map_mode: MapMode::NormalToNormal,
            scale: 1.0,
            flip_u: false,
            flip_v: false,
            swap_uv: false,
            base_forward: true,
            control_forward: true,
            tolerance: 1e-6,
        }
    }
}

/// A 3D space deformation mapping a base surface onto a control (target) surface.
pub struct SurfaceToSurfaceMap<'a> {
    base: &'a Surface,
    base_domain: Extent2d,
    control: &'a Surface,
    control_domain: Extent2d,
    options: SurfaceToSurfaceOptions,
    /// Orthonormal frame `(e1, e2, d)` used when `options.projection` is `Direction`.
    dir_frame: Option<(Vec3, Vec3, Vec3)>,
    base_prep: std::sync::OnceLock<Option<PreparedSurface<'a>>>,
    control_prep: std::sync::OnceLock<Option<PreparedSurface<'a>>>,
}

/// Enclosed base-domain UV projection over a 3D box `X`.
struct BaseUvEnclosure {
    u: Ivl,
    v: Ivl,
    gu: V3<Ivl>,
    gv: V3<Ivl>,
    rho_u: Ivl,
    rho_v: Ivl,
}

/// Enclosed surface evaluation up to second partial derivatives.
pub(crate) struct SurfaceJetIvl {
    pub(crate) p: V3<Ivl>,
    pub(crate) su: V3<Ivl>,
    pub(crate) sv: V3<Ivl>,
    pub(crate) suu: V3<Ivl>,
    pub(crate) suv: V3<Ivl>,
    pub(crate) svv: V3<Ivl>,
}

impl<'a> SurfaceToSurfaceMap<'a> {
    /// Construct a validated surface-to-surface deformation map.
    ///
    /// # Errors
    ///
    /// Returns [`SolidError::InvalidInput`] if either domain is empty/non-finite,
    /// `scale` or `tolerance` is invalid, or a direction projection vector is zero.
    pub fn new(
        base: &'a Surface,
        base_domain: Extent2d,
        control: &'a Surface,
        control_domain: Extent2d,
        options: SurfaceToSurfaceOptions,
    ) -> Result<Self> {
        validate_domain(&base_domain)?;
        validate_domain(&control_domain)?;
        if !options.scale.is_finite() {
            return Err(SolidError::invalid_input(OP_NEW, "scale must be finite"));
        }
        if !options.tolerance.is_finite() || options.tolerance <= 0.0 {
            return Err(SolidError::invalid_input(
                OP_NEW,
                "tolerance must be positive and finite",
            ));
        }
        let dir_frame = match options.projection {
            ProjectionMode::Normal => None,
            ProjectionMode::Direction(d) => {
                let unit_d = normalize_direction(d).ok_or_else(|| {
                    SolidError::invalid_input(
                        OP_NEW,
                        "projection direction must be non-zero and finite",
                    )
                })?;
                let (e1, e2) = orthonormal_basis(unit_d);
                Some((e1, e2, unit_d))
            }
        };
        Ok(Self {
            base,
            base_domain,
            control,
            control_domain,
            options,
            dir_frame,
            base_prep: std::sync::OnceLock::new(),
            control_prep: std::sync::OnceLock::new(),
        })
    }

    fn base_jet(&self, k: &Kernel, u: Ivl, v: Ivl, order: usize) -> Result<Option<SurfaceJetIvl>> {
        if let Some(cached) = self.base_prep.get() {
            return match cached {
                Some(p) => p.jet_ivl(u, v, order),
                None => Ok(None),
            };
        }
        let prep = PreparedSurface::of(k, self.base)?;
        let res = match prep.as_ref() {
            Some(p) => p.jet_ivl(u, v, order),
            None => Ok(None),
        };
        let _ = self.base_prep.set(prep);
        res
    }

    fn control_jet(
        &self,
        k: &Kernel,
        u: Ivl,
        v: Ivl,
        order: usize,
    ) -> Result<Option<SurfaceJetIvl>> {
        if let Some(cached) = self.control_prep.get() {
            return match cached {
                Some(p) => p.jet_ivl(u, v, order),
                None => Ok(None),
            };
        }
        let prep = PreparedSurface::of(k, self.control)?;
        let res = match prep.as_ref() {
            Some(p) => p.jet_ivl(u, v, order),
            None => Ok(None),
        };
        let _ = self.control_prep.set(prep);
        res
    }

    /// The scale factor configured on this map.
    #[must_use]
    pub const fn scale(&self) -> f64 {
        self.options.scale
    }

    /// When this map acts as an exact rigid motion `p' = p · A + t` over `body_box`
    /// (for instance, zero scale, identical base and control surfaces, or an isometric
    /// planar-to-planar map without boundary clamping), returns its 4×4 row-vector matrix.
    pub(crate) fn exact_rigid_matrix(
        &self,
        k: &Kernel,
        body_box: Box3,
    ) -> Result<Option<[[f64; 4]; 4]>> {
        let identity = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        if self.options.scale == 0.0 {
            return Ok(Some(identity));
        }

        // Exact identity when base and control surfaces and UV orientations match,
        // provided every point in `body_box` has a valid projection on `base`.
        let same_uv = self.base_domain.u_min() == self.control_domain.u_min()
            && self.base_domain.u_max() == self.control_domain.u_max()
            && self.base_domain.v_min() == self.control_domain.v_min()
            && self.base_domain.v_max() == self.control_domain.v_max()
            && !self.options.flip_u
            && !self.options.flip_v
            && !self.options.swap_uv
            && self.options.base_forward == self.options.control_forward;
        if same_uv && same_surface(self.base, self.control) {
            let valid_identity = match self.dir_frame {
                None => surface_jet_ivl(
                    k,
                    self.base,
                    Ivl::new(self.base_domain.u_min(), self.base_domain.u_max()),
                    Ivl::new(self.base_domain.v_min(), self.base_domain.v_max()),
                    1,
                )?
                .is_some_and(|j| {
                    self.options.map_mode == MapMode::Translate
                        || unit_normal_jet_ivl(&j, self.options.base_forward, false).is_some()
                }),
                Some(_) => self.enclose_box(k, body_box)?.is_some(),
            };
            if valid_identity {
                return Ok(Some(identity));
            }
        }

        // Exact planar-to-planar rigid motion when `body_box` projects inside `base_domain`
        // without boundary clamping.
        let (SurfaceKind::Plane(pb), SurfaceKind::Plane(pc)) =
            (self.base.kind(), self.control.kind())
        else {
            return Ok(None);
        };
        let x = body_box.map(|(lo, hi)| Ivl::new(lo, hi));
        let Some(proj) = self.enclose_base_plane(pb, x) else {
            return Ok(None);
        };
        if proj.rho_u != IZ || proj.rho_v != IZ {
            return Ok(None);
        }
        // Check that `proj.u` and `proj.v` are not stuck on a single clamped boundary value.
        let u_span = self.base_domain.u_max() - self.base_domain.u_min();
        let v_span = self.base_domain.v_max() - self.base_domain.v_min();
        if proj.u.lo <= self.base_domain.u_min() + 1e-12 * u_span
            || proj.u.hi >= self.base_domain.u_max() - 1e-12 * u_span
            || proj.v.lo <= self.base_domain.v_min() + 1e-12 * v_span
            || proj.v.hi >= self.base_domain.v_max() - 1e-12 * v_span
        {
            // Only reject if raw projection actually left the domain; Verify raw directly:
            if !self.plane_raw_inside_domain(pb, x) {
                return Ok(None);
            }
        }

        let b00 = pb.evaluate_point_fast(Point2::new(
            self.base_domain.u_min(),
            self.base_domain.v_min(),
        ));
        let b10 = pb.evaluate_point_fast(Point2::new(
            self.base_domain.u_max(),
            self.base_domain.v_min(),
        ));
        let b01 = pb.evaluate_point_fast(Point2::new(
            self.base_domain.u_min(),
            self.base_domain.v_max(),
        ));
        let c00 = pc.evaluate_point_fast(self.map_uv(Point2::new(
            self.base_domain.u_min(),
            self.base_domain.v_min(),
        )));
        let c10 = pc.evaluate_point_fast(self.map_uv(Point2::new(
            self.base_domain.u_max(),
            self.base_domain.v_min(),
        )));
        let c01 = pc.evaluate_point_fast(self.map_uv(Point2::new(
            self.base_domain.u_min(),
            self.base_domain.v_max(),
        )));

        let du_b = b10 - b00;
        let dv_b = b01 - b00;
        let du_c = c10 - c00;
        let dv_c = c01 - c00;

        let mut nb = pb.position().z_axis();
        if !self.options.base_forward {
            nb = -nb;
        }
        let mut nc = pc.position().z_axis();
        if !self.options.control_forward {
            nc = -nc;
        }
        let w = match self.dir_frame {
            None => nb,
            Some((_, _, unit_d)) => unit_d,
        };

        // Pure translation (works for any `scale` when tangent frames and normal match).
        let scale_ref = 1.0 + du_b.length() + dv_b.length();
        let same_tangents = (du_c - du_b).length() <= 1e-13 * scale_ref
            && (dv_c - dv_b).length() <= 1e-13 * scale_ref;
        let same_normal = match self.options.map_mode {
            MapMode::Translate => true,
            MapMode::NormalToNormal => (nc - w).length() <= 1e-13,
        };
        if same_tangents && same_normal {
            let t = (c00 - b00) * self.options.scale;
            if t.x.is_finite() && t.y.is_finite() && t.z.is_finite() {
                return Ok(Some([
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [t.x, t.y, t.z, 1.0],
                ]));
            }
        }

        // Rigid rotation + translation when `scale == 1.0`, `Normal` projection, `NormalToNormal`.
        if (self.options.scale - 1.0).abs() <= 1e-15
            && self.dir_frame.is_none()
            && self.options.map_mode == MapMode::NormalToNormal
            && (du_c.length() - du_b.length()).abs() <= 1e-13 * scale_ref
            && (dv_c.length() - dv_b.length()).abs() <= 1e-13 * scale_ref
            && (du_c.dot(dv_c) - du_b.dot(dv_b)).abs() <= 1e-13 * scale_ref * scale_ref
        {
            if let (Ok(eu_b), Ok(eu_c)) = (du_b.unitize(), du_c.unitize()) {
                let ev_b = nb.cross(eu_b);
                let ev_c = nc.cross(eu_c);
                // Verify `dv_b` and `dv_c` have the same components in `(eu, ev)`.
                if (dv_b.dot(ev_b) - dv_c.dot(ev_c)).abs() <= 1e-13 * scale_ref {
                    // Row-vector rotation R sending eu_b -> eu_c, ev_b -> ev_c, nb -> nc:
                    // R = eu_b^T ⊗ eu_c + ev_b^T ⊗ ev_c + nb^T ⊗ nc.
                    let row = |comp_b: [f64; 3]| -> [f64; 4] {
                        let v = eu_c * comp_b[0] + ev_c * comp_b[1] + nc * comp_b[2];
                        [v.x, v.y, v.z, 0.0]
                    };
                    let r0 = row([eu_b.x, ev_b.x, nb.x]);
                    let r1 = row([eu_b.y, ev_b.y, nb.y]);
                    let r2 = row([eu_b.z, ev_b.z, nb.z]);
                    let rot_b00 = Vec3::new(
                        b00.x * r0[0] + b00.y * r1[0] + b00.z * r2[0],
                        b00.x * r0[1] + b00.y * r1[1] + b00.z * r2[1],
                        b00.x * r0[2] + b00.y * r1[2] + b00.z * r2[2],
                    );
                    let t = c00 - rot_b00;
                    if [r0[0], r1[1], r2[2], t.x, t.y, t.z]
                        .iter()
                        .all(|v| v.is_finite())
                    {
                        return Ok(Some([r0, r1, r2, [t.x, t.y, t.z, 1.0]]));
                    }
                }
            }
        }

        Ok(None)
    }

    fn plane_raw_inside_domain(&self, pl: &Plane, x: [Ivl; 3]) -> bool {
        let Some((u_raw, v_raw, _, _)) = self.plane_raw_projection(pl, x) else {
            return false;
        };
        u_raw.lo >= self.base_domain.u_min()
            && u_raw.hi <= self.base_domain.u_max()
            && v_raw.lo >= self.base_domain.v_min()
            && v_raw.hi <= self.base_domain.v_max()
    }

    fn project_normal(&self, k: &Kernel, p: Point3) -> Result<Point2> {
        let solutions = self.base.global_point_solve::<NoSurfaceHandle>(
            k,
            None,
            &self.base_domain,
            SolverOperation::Minimize,
            p,
            self.options.tolerance,
            None,
            SolutionRequested::Single,
        )?;
        if let Some(sol) = solutions.get(0) {
            if let (Ok(u), Ok(v)) = (sol.start.param(0), sol.start.param(1)) {
                if u.is_finite() && v.is_finite() {
                    return Ok(clamp_uv(Point2::new(u, v), &self.base_domain));
                }
            }
        }
        Err(SolidError::invalid_input(
            OP_MAP,
            "normal projection found no closest point on the base surface",
        ))
    }

    fn project_direction(&self, k: &Kernel, p: Point3, e1: Vec3, e2: Vec3) -> Result<Point2> {
        const N: usize = 9;
        let u0 = self.base_domain.u_min();
        let u1 = self.base_domain.u_max();
        let v0 = self.base_domain.v_min();
        let v1 = self.base_domain.v_max();
        let du = u1 - u0;
        let dv = v1 - v0;

        let mut best_uv = Point2::new(0.5 * (u0 + u1), 0.5 * (v0 + v1));
        let mut best_e2 = f64::INFINITY;
        for iu in 0..=N {
            let u = u0 + du * (iu as f64 / N as f64);
            for iv in 0..=N {
                let v = v0 + dv * (iv as f64 / N as f64);
                let pt = self.base.evaluate_point(k, Point2::new(u, v))?;
                let r = pt - p;
                let c1 = r.dot(e1);
                let c2 = r.dot(e2);
                let err2 = c1 * c1 + c2 * c2;
                if err2 < best_e2 {
                    best_e2 = err2;
                    best_uv = Point2::new(u, v);
                }
            }
        }

        let mut uv = best_uv;
        let flags = EvaluateFlags::default();
        for _ in 0..24 {
            let eval = self.base.evaluate(k, uv, 1, 1, flags)?;
            let Some(row0) = eval.first() else { break };
            let Some(row1) = eval.get(1) else { break };
            let (Some(&s), Some(&sv), Some(&su)) = (row0.first(), row0.get(1), row1.first()) else {
                break;
            };
            let r = s - p;
            let f1 = r.dot(e1);
            let f2 = r.dot(e2);
            if f1.hypot(f2) <= self.options.tolerance * 1e-3 {
                break;
            }
            let j11 = su.dot(e1);
            let j12 = sv.dot(e1);
            let j21 = su.dot(e2);
            let j22 = sv.dot(e2);
            let det = j11 * j22 - j12 * j21;
            if !det.is_finite() || det.abs() <= 1e-24 {
                break;
            }
            let step_u = -(j22 * f1 - j12 * f2) / det;
            let step_v = -(-j21 * f1 + j11 * f2) / det;
            let next = clamp_uv(Point2::new(uv.x + step_u, uv.y + step_v), &self.base_domain);
            let moved = (next.x - uv.x).abs() + (next.y - uv.y).abs();
            uv = next;
            if moved <= 1e-12 * (du.abs() + dv.abs() + 1.0) {
                break;
            }
        }

        // Verify the final perpendicular residual so clamped misses, singular steps, or
        // unconverged iterations never return a bogus hit.
        let hit = self.base.evaluate_point(k, uv)?;
        let r = hit - p;
        let residual = r.dot(e1).hypot(r.dot(e2));
        if !residual.is_finite() || residual > self.options.tolerance {
            return Err(SolidError::invalid_input(
                OP_MAP,
                "directional projection missed the base surface",
            ));
        }
        Ok(uv)
    }

    fn map_uv(&self, uv_b: Point2) -> Point2 {
        let u0 = self.base_domain.u_min();
        let u1 = self.base_domain.u_max();
        let v0 = self.base_domain.v_min();
        let v1 = self.base_domain.v_max();

        let mut nu = ((uv_b.x - u0) / (u1 - u0)).clamp(0.0, 1.0);
        let mut nv = ((uv_b.y - v0) / (v1 - v0)).clamp(0.0, 1.0);
        if self.options.swap_uv {
            std::mem::swap(&mut nu, &mut nv);
        }
        if self.options.flip_u {
            nu = 1.0 - nu;
        }
        if self.options.flip_v {
            nv = 1.0 - nv;
        }

        let cu0 = self.control_domain.u_min();
        let cu1 = self.control_domain.u_max();
        let cv0 = self.control_domain.v_min();
        let cv1 = self.control_domain.v_max();
        Point2::new(cu0 + nu * (cu1 - cu0), cv0 + nv * (cv1 - cv0))
    }

    /// Map an enclosed base UV box `(u_b, v_b)` to `control_domain` in outward-rounded
    /// intervals, returning `(u_c, v_c, [[∂u_c/∂u_b, ∂u_c/∂v_b], [∂v_c/∂u_b, ∂v_c/∂v_b]])`.
    fn map_uv_ivl(&self, ub: Ivl, vb: Ivl) -> Option<(Ivl, Ivl, [[Ivl; 2]; 2])> {
        let u0 = Ivl::from(self.base_domain.u_min());
        let u1 = Ivl::from(self.base_domain.u_max());
        let v0 = Ivl::from(self.base_domain.v_min());
        let v1 = Ivl::from(self.base_domain.v_max());
        let du_b = u1 - u0;
        let dv_b = v1 - v0;
        if !(du_b.lo > 0.0 && dv_b.lo > 0.0) {
            return None;
        }

        let clamp01 = |x: Ivl| Ivl::new(x.lo.clamp(0.0, 1.0), x.hi.clamp(0.0, 1.0));
        let mut nu = clamp01((ub - u0) / du_b);
        let mut nv = clamp01((vb - v0) / dv_b);
        let mut dnu = [Ivl::from(1.0) / du_b, IZ];
        let mut dnv = [IZ, Ivl::from(1.0) / dv_b];

        if self.options.swap_uv {
            std::mem::swap(&mut nu, &mut nv);
            std::mem::swap(&mut dnu, &mut dnv);
        }
        if self.options.flip_u {
            nu = Ivl::from(1.0) - nu;
            dnu = [IZ - dnu[0], IZ - dnu[1]];
        }
        if self.options.flip_v {
            nv = Ivl::from(1.0) - nv;
            dnv = [IZ - dnv[0], IZ - dnv[1]];
        }

        let cu0 = Ivl::from(self.control_domain.u_min());
        let cu1 = Ivl::from(self.control_domain.u_max());
        let cv0 = Ivl::from(self.control_domain.v_min());
        let cv1 = Ivl::from(self.control_domain.v_max());
        let du_c = cu1 - cu0;
        let dv_c = cv1 - cv0;

        let uc = Ivl::new(
            (cu0 + nu * du_c).lo.max(self.control_domain.u_min()),
            (cu0 + nu * du_c).hi.min(self.control_domain.u_max()),
        );
        let vc = Ivl::new(
            (cv0 + nv * dv_c).lo.max(self.control_domain.v_min()),
            (cv0 + nv * dv_c).hi.min(self.control_domain.v_max()),
        );
        let jac = [
            [du_c * dnu[0], du_c * dnu[1]],
            [dv_c * dnv[0], dv_c * dnv[1]],
        ];
        (uc.is_finite() && vc.is_finite()).then_some((uc, vc, jac))
    }

    /// Raw (unclamped) projection onto a planar base surface `pl` in rounded intervals.
    fn plane_raw_projection(
        &self,
        pl: &Plane,
        x: [Ivl; 3],
    ) -> Option<(Ivl, Ivl, V3<Ivl>, V3<Ivl>)> {
        let o = v3_pt(pl.position().origin());
        let au = v3_scale(v3_vec(pl.position().x_axis()), Ivl::from(pl.uv_scale().x));
        let av = v3_scale(v3_vec(pl.position().y_axis()), Ivl::from(pl.uv_scale().y));
        let rel = V3 {
            x: x[0] - o.x,
            y: x[1] - o.y,
            z: x[2] - o.z,
        };

        let (r1, r2, m11, m12, m21, m22) = match self.dir_frame {
            None => (
                au,
                av,
                v3_dot(au, au),
                v3_dot(au, av),
                v3_dot(av, au),
                v3_dot(av, av),
            ),
            Some((e1, e2, _)) => {
                let e1v = v3_vec(e1);
                let e2v = v3_vec(e2);
                (
                    e1v,
                    e2v,
                    v3_dot(au, e1v),
                    v3_dot(av, e1v),
                    v3_dot(au, e2v),
                    v3_dot(av, e2v),
                )
            }
        };
        let inv = invert_2x2_ivl([[m11, m12], [m21, m22]])?;
        let rhs1 = v3_dot(rel, r1);
        let rhs2 = v3_dot(rel, r2);
        let u_raw = inv[0][0] * rhs1 + inv[0][1] * rhs2;
        let v_raw = inv[1][0] * rhs1 + inv[1][1] * rhs2;
        let gu = v3_add(v3_scale(r1, inv[0][0]), v3_scale(r2, inv[0][1]));
        let gv = v3_add(v3_scale(r1, inv[1][0]), v3_scale(r2, inv[1][1]));
        (u_raw.is_finite() && v_raw.is_finite()).then_some((u_raw, v_raw, gu, gv))
    }

    fn enclose_base_plane(&self, pl: &Plane, x: [Ivl; 3]) -> Option<BaseUvEnclosure> {
        let (u_raw, v_raw, raw_gu, raw_gv) = self.plane_raw_projection(pl, x)?;
        let u0 = self.base_domain.u_min();
        let u1 = self.base_domain.u_max();
        let v0 = self.base_domain.v_min();
        let v1 = self.base_domain.v_max();

        let (u, gu, rho_u) = clamp_linear_ivl(u_raw, raw_gu, u0, u1);
        let (v, gv, rho_v) = clamp_linear_ivl(v_raw, raw_gv, v0, v1);

        // In Direction mode, every point in `x` must hit `base_domain` within `options.tolerance`.
        if let Some((e1, e2, _)) = self.dir_frame {
            if rho_u != IZ || rho_v != IZ {
                let o = v3_pt(pl.position().origin());
                let au = v3_scale(v3_vec(pl.position().x_axis()), Ivl::from(pl.uv_scale().x));
                let av = v3_scale(v3_vec(pl.position().y_axis()), Ivl::from(pl.uv_scale().y));
                let s_clamped = v3_add(o, v3_add(v3_scale(au, u), v3_scale(av, v)));
                let diff = V3 {
                    x: s_clamped.x - x[0],
                    y: s_clamped.y - x[1],
                    z: s_clamped.z - x[2],
                };
                let f1 = v3_dot(diff, v3_vec(e1));
                let f2 = v3_dot(diff, v3_vec(e2));
                let resid = (f1 * f1 + f2 * f2).sqrt();
                if !resid.is_finite() || resid.hi > self.options.tolerance {
                    return None;
                }
            }
        }

        Some(BaseUvEnclosure {
            u,
            v,
            gu,
            gv,
            rho_u,
            rho_v,
        })
    }

    fn enclose_base_uv(&self, k: &Kernel, x: [Ivl; 3]) -> Result<Option<BaseUvEnclosure>> {
        if let SurfaceKind::Plane(pl) = self.base.kind() {
            return Ok(self.enclose_base_plane(pl, x));
        }

        let p_mid = Point3::new(x[0].mid(), x[1].mid(), x[2].mid());
        let uv_seed = match self.dir_frame {
            None => match self.project_normal(k, p_mid) {
                Ok(uv) => uv,
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => return Ok(None),
            },
            Some((e1, e2, _)) => match self.project_direction(k, p_mid, e1, e2) {
                Ok(uv) => uv,
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => return Ok(None),
            },
        };

        let u0 = self.base_domain.u_min();
        let u1 = self.base_domain.u_max();
        let v0 = self.base_domain.v_min();
        let v1 = self.base_domain.v_max();
        let order = if self.dir_frame.is_some() { 1 } else { 2 };

        let Some(jet_mid) = surface_jet_ivl(
            k,
            self.base,
            Ivl::from(uv_seed.x),
            Ivl::from(uv_seed.y),
            order,
        )?
        else {
            return Ok(None);
        };

        // Build approximate 2x2 Jacobian columns at `(uv_seed, p_mid)`.
        let x_pt = V3 {
            x: x[0],
            y: x[1],
            z: x[2],
        };
        let (f_at_seed, cols_mid) = if let Some((e1, e2, _)) = self.dir_frame {
            let e1v = v3_vec(e1);
            let e2v = v3_vec(e2);
            let r = v3_sub(jet_mid.p, x_pt);
            let f = [v3_dot(r, e1v), v3_dot(r, e2v)];
            let cols = [
                [v3_dot(jet_mid.su, e1v).mid(), v3_dot(jet_mid.su, e2v).mid()],
                [v3_dot(jet_mid.sv, e1v).mid(), v3_dot(jet_mid.sv, e2v).mid()],
            ];
            (f, cols)
        } else {
            let r = v3_sub(jet_mid.p, x_pt);
            let f = [v3_dot(r, jet_mid.su), v3_dot(r, jet_mid.sv)];
            let j11 = v3_dot(jet_mid.su, jet_mid.su) + v3_dot(r, jet_mid.suu);
            let j12 = v3_dot(jet_mid.sv, jet_mid.su) + v3_dot(r, jet_mid.suv);
            let j21 = v3_dot(jet_mid.su, jet_mid.sv) + v3_dot(r, jet_mid.suv);
            let j22 = v3_dot(jet_mid.sv, jet_mid.sv) + v3_dot(r, jet_mid.svv);
            (f, [[j11.mid(), j21.mid()], [j12.mid(), j22.mid()]])
        };

        let Some(c_inv) = inverse_of_columns2(&cols_mid) else {
            return Ok(None);
        };
        let mid = [uv_seed.x, uv_seed.y];
        let step0 = [
            (Ivl::from(c_inv[0][0]) * f_at_seed[0] + Ivl::from(c_inv[0][1]) * f_at_seed[1]).mag(),
            (Ivl::from(c_inv[1][0]) * f_at_seed[0] + Ivl::from(c_inv[1][1]) * f_at_seed[1]).mag(),
        ];
        let mut rad = [
            round_up(2.0 * step0[0] + 1e-11 * (1.0 + mid[0].abs())),
            round_up(2.0 * step0[1] + 1e-11 * (1.0 + mid[1].abs())),
        ];

        for _ in 0..8 {
            if !rad[0].is_finite() || !rad[1].is_finite() {
                return Ok(None);
            }
            let y = [
                Ivl::new(mid[0] - rad[0], mid[0] + rad[0]),
                Ivl::new(mid[1] - rad[1], mid[1] + rad[1]),
            ];
            // Require the uniqueness box `y` to stay inside `base_domain`.
            if y[0].lo < u0 || y[0].hi > u1 || y[1].lo < v0 || y[1].hi > v1 {
                return Ok(None);
            }
            let Some(jet) = surface_jet_ivl(k, self.base, y[0], y[1], order)? else {
                return Ok(None);
            };
            let jac_rows = if let Some((e1, e2, _)) = self.dir_frame {
                let e1v = v3_vec(e1);
                let e2v = v3_vec(e2);
                [
                    [v3_dot(jet.su, e1v), v3_dot(jet.sv, e1v)],
                    [v3_dot(jet.su, e2v), v3_dot(jet.sv, e2v)],
                ]
            } else {
                let r = v3_sub(jet.p, x_pt);
                [
                    [
                        v3_dot(jet.su, jet.su) + v3_dot(r, jet.suu),
                        v3_dot(jet.sv, jet.su) + v3_dot(r, jet.suv),
                    ],
                    [
                        v3_dot(jet.su, jet.sv) + v3_dot(r, jet.suv),
                        v3_dot(jet.sv, jet.sv) + v3_dot(r, jet.svv),
                    ],
                ]
            };
            let (cm, q) = contraction(&c_inv, &jac_rows);
            let kop = k_operator(&mid, &c_inv, &f_at_seed, &cm, &y);
            if q < 1.0
                && kop[0].lo > y[0].lo
                && kop[0].hi < y[0].hi
                && kop[1].lo > y[1].lo
                && kop[1].hi < y[1].hi
            {
                let root_u = Ivl::new(kop[0].lo.max(y[0].lo), kop[0].hi.min(y[0].hi));
                let root_v = Ivl::new(kop[1].lo.max(y[1].lo), kop[1].hi.min(y[1].hi));

                // For closest-point projection, prove no point of `base_domain \ y` is closer.
                if self.dir_frame.is_none()
                    && !self.prove_global_closest_in_box(k, x, p_mid, root_u, root_v, y)?
                {
                    return Ok(None);
                }

                let Some(jet_root) = surface_jet_ivl(k, self.base, root_u, root_v, order)? else {
                    return Ok(None);
                };
                let (jac_root, r1, r2) = if let Some((e1, e2, _)) = self.dir_frame {
                    let e1v = v3_vec(e1);
                    let e2v = v3_vec(e2);
                    (
                        [
                            [v3_dot(jet_root.su, e1v), v3_dot(jet_root.sv, e1v)],
                            [v3_dot(jet_root.su, e2v), v3_dot(jet_root.sv, e2v)],
                        ],
                        e1v,
                        e2v,
                    )
                } else {
                    let r = v3_sub(jet_root.p, x_pt);
                    (
                        [
                            [
                                v3_dot(jet_root.su, jet_root.su) + v3_dot(r, jet_root.suu),
                                v3_dot(jet_root.sv, jet_root.su) + v3_dot(r, jet_root.suv),
                            ],
                            [
                                v3_dot(jet_root.su, jet_root.sv) + v3_dot(r, jet_root.suv),
                                v3_dot(jet_root.sv, jet_root.sv) + v3_dot(r, jet_root.svv),
                            ],
                        ],
                        jet_root.su,
                        jet_root.sv,
                    )
                };
                let Some(inv_j) = invert_2x2_ivl(jac_root) else {
                    return Ok(None);
                };
                let gu = v3_add(v3_scale(r1, inv_j[0][0]), v3_scale(r2, inv_j[0][1]));
                let gv = v3_add(v3_scale(r1, inv_j[1][0]), v3_scale(r2, inv_j[1][1]));
                return Ok(Some(BaseUvEnclosure {
                    u: root_u,
                    v: root_v,
                    gu,
                    gv,
                    rho_u: IZ,
                    rho_v: IZ,
                }));
            }
            let reach0 = (kop[0].lo - mid[0]).abs().max((kop[0].hi - mid[0]).abs());
            rad[0] = round_up(1.5 * reach0.max(rad[0]) + 1e-11 * (1.0 + mid[0].abs()));
            let reach1 = (kop[1].lo - mid[1]).abs().max((kop[1].hi - mid[1]).abs());
            rad[1] = round_up(1.5 * reach1.max(rad[1]) + 1e-11 * (1.0 + mid[1].abs()));
        }
        Ok(None)
    }

    /// Prove that for every `P ∈ x`, the stationary point in `(root_u, root_v)` is strictly
    /// closer to `P` than every point of `base_domain` outside the Krawczyk uniqueness box `y`.
    fn prove_global_closest_in_box(
        &self,
        k: &Kernel,
        x: [Ivl; 3],
        p_mid: Point3,
        root_u: Ivl,
        root_v: Ivl,
        y: [Ivl; 2],
    ) -> Result<bool> {
        let Some(jet_r) = surface_jet_ivl(k, self.base, root_u, root_v, 0)? else {
            return Ok(false);
        };
        let dx = x[0] - jet_r.p.x;
        let dy = x[1] - jet_r.p.y;
        let dz = x[2] - jet_r.p.z;
        let upper_d = (dx * dx + dy * dy + dz * dz).sqrt().hi;
        if !upper_d.is_finite() {
            return Ok(false);
        }
        // Radius of `x` around `p_mid`.
        let rx = round_up((x[0].hi - p_mid.x).abs().max((p_mid.x - x[0].lo).abs()));
        let ry = round_up((x[1].hi - p_mid.y).abs().max((p_mid.y - x[1].lo).abs()));
        let rz = round_up((x[2].hi - p_mid.z).abs().max((p_mid.z - x[2].lo).abs()));
        let box_rad = (Ivl::from(rx) * Ivl::from(rx)
            + Ivl::from(ry) * Ivl::from(ry)
            + Ivl::from(rz) * Ivl::from(rz))
        .sqrt()
        .hi;
        let need_from_mid = round_up(upper_d + box_rad);
        if !need_from_mid.is_finite() {
            return Ok(false);
        }

        let (pieces, slack) = match pieces_about(k, self.base, Some(&self.base_domain), p_mid) {
            Ok(pair) => pair,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(false),
        };
        let mut work = pieces;
        let mut budget: usize = 512;
        while let Some(pc) = work.pop() {
            let [pu0, pu1, pv0, pv1] = pc.uv;
            // Any sub-patch wholly inside `y` cannot contain a competing point outside `y`.
            if pu0 >= y[0].lo && pu1 <= y[0].hi && pv0 >= y[1].lo && pv1 <= y[1].hi {
                continue;
            }
            let lower = round_down((pc.lower - slack).max(0.0));
            if lower > need_from_mid {
                continue;
            }
            let Some(rem) = budget.checked_sub(4) else {
                return Ok(false);
            };
            budget = rem;
            match pc.quarters() {
                Ok(qs) => work.extend(qs),
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => return Ok(false),
            }
        }
        Ok(true)
    }
}

impl SpaceMap for SurfaceToSurfaceMap<'_> {
    fn map_point(&self, k: &Kernel, p: Point3) -> Result<Point3> {
        if !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() {
            return Err(SolidError::invalid_input(
                OP_MAP,
                "input point must be finite",
            ));
        }
        if self.options.scale == 0.0 {
            return Ok(p);
        }

        let uv_b = match self.dir_frame {
            None => self.project_normal(k, p)?,
            Some((e1, e2, _)) => self.project_direction(k, p, e1, e2)?,
        };
        let uv_c = self.map_uv(uv_b);

        let b_pt = self.base.evaluate_point(k, uv_b)?;
        let c_pt = self.control.evaluate_point(k, uv_c)?;

        let mapped = match self.options.map_mode {
            MapMode::Translate => c_pt + (p - b_pt),
            MapMode::NormalToNormal => {
                let mut n_b = unit_normal(self.base, k, uv_b)?;
                if !self.options.base_forward {
                    n_b = -n_b;
                }
                let mut n_c = unit_normal(self.control, k, uv_c)?;
                if !self.options.control_forward {
                    n_c = -n_c;
                }
                let h = match self.dir_frame {
                    None => (p - b_pt).dot(n_b),
                    Some((_, _, unit_d)) => (p - b_pt).dot(unit_d),
                };
                // Preserve tangential residual if point was clamped to base boundary.
                let residual = match self.dir_frame {
                    None => (p - b_pt) - n_b * h,
                    Some((_, _, unit_d)) => (p - b_pt) - unit_d * h,
                };
                c_pt + n_c * h + residual
            }
        };

        let out = p + (mapped - p) * self.options.scale;
        if !out.x.is_finite() || !out.y.is_finite() || !out.z.is_finite() {
            return Err(SolidError::invalid_input(
                OP_MAP,
                "deformation produced a non-finite point",
            ));
        }
        Ok(out)
    }

    fn enclose_box(&self, k: &Kernel, p_box: Box3) -> Result<Option<Box3>> {
        let x = p_box.map(|(lo, hi)| Ivl::new(lo, hi));
        if !x.iter().all(|i| i.is_finite()) {
            return Ok(None);
        }
        if self.options.scale == 0.0 {
            return Ok(Some(p_box));
        }
        let Some(proj) = self.enclose_base_uv(k, x)? else {
            return Ok(None);
        };
        let Some((uc, vc, _)) = self.map_uv_ivl(proj.u, proj.v) else {
            return Ok(None);
        };
        let need = usize::from(self.options.map_mode == MapMode::NormalToNormal);
        let Some(jb) = self.base_jet(k, proj.u, proj.v, need)? else {
            return Ok(None);
        };
        let Some(jc) = self.control_jet(k, uc, vc, need)? else {
            return Ok(None);
        };

        let xp = V3 {
            x: x[0],
            y: x[1],
            z: x[2],
        };
        let delta_s = v3_sub(jc.p, jb.p);
        let disp = match self.options.map_mode {
            MapMode::Translate => delta_s,
            MapMode::NormalToNormal => {
                let Some((nc, _, _)) =
                    unit_normal_jet_ivl(&jc, self.options.control_forward, false)
                else {
                    return Ok(None);
                };
                let w = if let Some((_, _, unit_d)) = self.dir_frame {
                    v3_vec(unit_d)
                } else {
                    let Some((nb, _, _)) =
                        unit_normal_jet_ivl(&jb, self.options.base_forward, false)
                    else {
                        return Ok(None);
                    };
                    nb
                };
                let h = v3_dot(v3_sub(xp, jb.p), w);
                v3_add(delta_s, v3_scale(v3_sub(nc, w), h))
            }
        };
        let out = v3_add(xp, v3_scale(disp, Ivl::from(self.options.scale)));
        if !(out.x.is_finite() && out.y.is_finite() && out.z.is_finite()) {
            return Ok(None);
        }
        Ok(Some([
            (out.x.lo, out.x.hi),
            (out.y.lo, out.y.hi),
            (out.z.lo, out.z.hi),
        ]))
    }

    fn enclose_jet(&self, k: &Kernel, p_box: Box3) -> Result<Option<JetEnclosure>> {
        let x = p_box.map(|(lo, hi)| Ivl::new(lo, hi));
        if !x.iter().all(|i| i.is_finite()) {
            return Ok(None);
        }
        if self.options.scale == 0.0 {
            let id = [
                [(1.0, 1.0), (0.0, 0.0), (0.0, 0.0)],
                [(0.0, 0.0), (1.0, 1.0), (0.0, 0.0)],
                [(0.0, 0.0), (0.0, 0.0), (1.0, 1.0)],
            ];
            return Ok(Some((id, [(0.0, 0.0); 3])));
        }

        let Some(proj) = self.enclose_base_uv(k, x)? else {
            return Ok(None);
        };
        let Some((uc, vc, uv_jac)) = self.map_uv_ivl(proj.u, proj.v) else {
            return Ok(None);
        };
        let order = if self.options.map_mode == MapMode::NormalToNormal {
            2
        } else {
            1
        };
        let Some(jb) = self.base_jet(k, proj.u, proj.v, order)? else {
            return Ok(None);
        };
        let Some(jc) = self.control_jet(k, uc, vc, order)? else {
            return Ok(None);
        };

        let s = Ivl::from(self.options.scale);
        let dc_dub = v3_add(v3_scale(jc.su, uv_jac[0][0]), v3_scale(jc.sv, uv_jac[1][0]));
        let dc_dvb = v3_add(v3_scale(jc.su, uv_jac[0][1]), v3_scale(jc.sv, uv_jac[1][1]));

        let (dpsi_dp, dpsi_du, dpsi_dv) = match self.options.map_mode {
            MapMode::Translate => {
                let eye = [
                    [Ivl::from(1.0), IZ, IZ],
                    [IZ, Ivl::from(1.0), IZ],
                    [IZ, IZ, Ivl::from(1.0)],
                ];
                (
                    eye,
                    v3_scale(v3_sub(dc_dub, jb.su), s),
                    v3_scale(v3_sub(dc_dvb, jb.sv), s),
                )
            }
            MapMode::NormalToNormal => {
                let Some((nc, ncu, ncv)) =
                    unit_normal_jet_ivl(&jc, self.options.control_forward, true)
                else {
                    return Ok(None);
                };
                let dnc_dub = v3_add(v3_scale(ncu, uv_jac[0][0]), v3_scale(ncv, uv_jac[1][0]));
                let dnc_dvb = v3_add(v3_scale(ncu, uv_jac[0][1]), v3_scale(ncv, uv_jac[1][1]));

                let (w, wu, wv) = if let Some((_, _, unit_d)) = self.dir_frame {
                    (v3_vec(unit_d), VZ, VZ)
                } else {
                    let Some((nb, nbu, nbv)) =
                        unit_normal_jet_ivl(&jb, self.options.base_forward, true)
                    else {
                        return Ok(None);
                    };
                    (nb, nbu, nbv)
                };
                let xp = V3 {
                    x: x[0],
                    y: x[1],
                    z: x[2],
                };
                let delta = v3_sub(xp, jb.p);
                let h = v3_dot(delta, w);
                let m = v3_sub(nc, w);
                let hu = v3_dot(delta, wu) - v3_dot(jb.su, w);
                let hv = v3_dot(delta, wv) - v3_dot(jb.sv, w);
                let mu = v3_sub(dnc_dub, wu);
                let mv = v3_sub(dnc_dvb, wv);

                let one = Ivl::from(1.0);
                let dp = [
                    [one + s * m.x * w.x, s * m.x * w.y, s * m.x * w.z],
                    [s * m.y * w.x, one + s * m.y * w.y, s * m.y * w.z],
                    [s * m.z * w.x, s * m.z * w.y, one + s * m.z * w.z],
                ];
                let du = v3_scale(
                    v3_add(
                        v3_sub(dc_dub, jb.su),
                        v3_add(v3_scale(m, hu), v3_scale(mu, h)),
                    ),
                    s,
                );
                let dv = v3_scale(
                    v3_add(
                        v3_sub(dc_dvb, jb.sv),
                        v3_add(v3_scale(m, hv), v3_scale(mv, h)),
                    ),
                    s,
                );
                (dp, du, dv)
            }
        };

        let du_arr = [dpsi_du.x, dpsi_du.y, dpsi_du.z];
        let dv_arr = [dpsi_dv.x, dpsi_dv.y, dpsi_dv.z];
        let gu_arr = [proj.gu.x, proj.gu.y, proj.gu.z];
        let gv_arr = [proj.gv.x, proj.gv.y, proj.gv.z];

        let mut jac_out: Jacobian3 = [[(0.0, 0.0); 3]; 3];
        for ((row_out, row_dp), (&du_r, &dv_r)) in jac_out
            .iter_mut()
            .zip(dpsi_dp.iter())
            .zip(du_arr.iter().zip(dv_arr.iter()))
        {
            for ((cell_out, &dp_rc), (&gu_c, &gv_c)) in row_out
                .iter_mut()
                .zip(row_dp.iter())
                .zip(gu_arr.iter().zip(gv_arr.iter()))
            {
                let entry = dp_rc + du_r * gu_c + dv_r * gv_c;
                if !entry.is_finite() {
                    return Ok(None);
                }
                *cell_out = (entry.lo, entry.hi);
            }
        }
        let mut slack_out: Box3 = [(0.0, 0.0); 3];
        for (slot, (&du_r, &dv_r)) in slack_out.iter_mut().zip(du_arr.iter().zip(dv_arr.iter())) {
            let sl = du_r * proj.rho_u + dv_r * proj.rho_v;
            if !sl.is_finite() {
                return Ok(None);
            }
            *slot = (sl.lo, sl.hi);
        }
        Ok(Some((jac_out, slack_out)))
    }
}

fn same_surface(a: &Surface, b: &Surface) -> bool {
    if core::ptr::eq(a, b) {
        return true;
    }
    match (a.kind(), b.kind()) {
        (SurfaceKind::Plane(pa), SurfaceKind::Plane(pb)) => {
            pa.position().origin() == pb.position().origin()
                && pa.position().x_axis() == pb.position().x_axis()
                && pa.position().y_axis() == pb.position().y_axis()
                && pa.uv_scale() == pb.uv_scale()
        }
        (SurfaceKind::BSpline(sa), SurfaceKind::BSpline(sb)) => {
            match (sa.gw_nurb(), sb.gw_nurb()) {
                (Some(na), Some(nb)) => {
                    na.degrees() == nb.degrees()
                        && na.u_knots() == nb.u_knots()
                        && na.v_knots() == nb.v_knots()
                        && na.n() == nb.n()
                        && na.m() == nb.m()
                        && na.rows().zip(nb.rows()).all(|(ra, rb)| {
                            ra.len() == rb.len()
                                && ra.iter().zip(rb).all(|(p, q)| {
                                    p.x == q.x && p.y == q.y && p.z == q.z && p.w == q.w
                                })
                        })
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Pre-extracted surface data for fast, allocation-free interval jet evaluation.
pub(crate) enum PreparedSurface<'a> {
    Plane(&'a Plane),
    Nurbs {
        nurbs: Nurbs,
        form_slack: f64,
        min_span: f64,
    },
}

impl<'a> PreparedSurface<'a> {
    pub(crate) fn of(k: &Kernel, surf: &'a Surface) -> Result<Option<Self>> {
        if let SurfaceKind::Plane(pl) = surf.kind() {
            return Ok(Some(Self::Plane(pl)));
        }
        if definition_bound(surf) > 0.0 || surf.procedural_sweep().is_some() {
            return Ok(None);
        }
        let Some(form) = as_b_spline(k, surf)? else {
            return Ok(None);
        };
        let Ok(nurbs) = Nurbs::of(&form) else {
            return Ok(None);
        };
        let Some(form_slack) = analytic_nurb_slack(k, surf, &form)? else {
            return Ok(None);
        };
        let min_span = min_positive_knot_span(&nurbs.u).min(min_positive_knot_span(&nurbs.v));
        Ok(Some(Self::Nurbs {
            nurbs,
            form_slack,
            min_span,
        }))
    }

    pub(crate) fn jet_ivl(&self, u: Ivl, v: Ivl, order: usize) -> Result<Option<SurfaceJetIvl>> {
        if !(u.is_finite() && v.is_finite()) {
            return Ok(None);
        }
        match self {
            Self::Plane(pl) => {
                let o = v3_pt(pl.position().origin());
                let su = v3_scale(v3_vec(pl.position().x_axis()), Ivl::from(pl.uv_scale().x));
                let sv = v3_scale(v3_vec(pl.position().y_axis()), Ivl::from(pl.uv_scale().y));
                let p = v3_add(o, v3_add(v3_scale(su, u), v3_scale(sv, v)));
                Ok(Some(SurfaceJetIvl {
                    p,
                    su,
                    sv,
                    suu: VZ,
                    suv: VZ,
                    svv: VZ,
                }))
            }
            Self::Nurbs {
                nurbs,
                form_slack,
                min_span,
            } => {
                let continuity = usize::from(order >= 2);
                let partials =
                    match nurbs_partials_over_box(nurbs, u.lo, u.hi, v.lo, v.hi, order, continuity)
                    {
                        Ok(pt) => pt,
                        Err(e) if e.is_unported_or_cancelled() => return Err(e),
                        Err(_) => return Ok(None),
                    };
                let mut p = partials.at(0, 0);
                let mut su = partials.at(1, 0);
                let mut sv = partials.at(0, 1);
                let mut suu = partials.at(2, 0);
                let mut suv = partials.at(1, 1);
                let mut svv = partials.at(0, 2);

                if *form_slack > 0.0 {
                    let pad0 = Ivl::new(-*form_slack, *form_slack);
                    p = v3_add(
                        p,
                        V3 {
                            x: pad0,
                            y: pad0,
                            z: pad0,
                        },
                    );
                    if order >= 1 {
                        if !(min_span.is_finite() && *min_span > 0.0) {
                            return Ok(None);
                        }
                        let deg = nurbs.p.max(nurbs.q) as f64;
                        let s1 = round_up(2.0 * deg * *form_slack / *min_span);
                        let pad1 = Ivl::new(-s1, s1);
                        let p1 = V3 {
                            x: pad1,
                            y: pad1,
                            z: pad1,
                        };
                        su = v3_add(su, p1);
                        sv = v3_add(sv, p1);
                        if order >= 2 {
                            let s2 =
                                round_up(4.0 * deg * deg * *form_slack / (*min_span * *min_span));
                            let pad2 = Ivl::new(-s2, s2);
                            let p2 = V3 {
                                x: pad2,
                                y: pad2,
                                z: pad2,
                            };
                            suu = v3_add(suu, p2);
                            suv = v3_add(suv, p2);
                            svv = v3_add(svv, p2);
                        }
                    }
                }
                let ok = v3_finite(p)
                    && (order < 1 || (v3_finite(su) && v3_finite(sv)))
                    && (order < 2 || (v3_finite(suu) && v3_finite(suv) && v3_finite(svv)));
                Ok(ok.then_some(SurfaceJetIvl {
                    p,
                    su,
                    sv,
                    suu,
                    suv,
                    svv,
                }))
            }
        }
    }
}

/// Outward-rounded enclosure of `S` and its partials up to `order` (`0`, `1`, or `2`)
/// over `[u.lo, u.hi] x [v.lo, v.hi]`.
pub(crate) fn surface_jet_ivl(
    k: &Kernel,
    surf: &Surface,
    u: Ivl,
    v: Ivl,
    order: usize,
) -> Result<Option<SurfaceJetIvl>> {
    let Some(prep) = PreparedSurface::of(k, surf)? else {
        return Ok(None);
    };
    prep.jet_ivl(u, v, order)
}

fn min_positive_knot_span(knots: &[f64]) -> f64 {
    let mut best = f64::INFINITY;
    for w in knots.windows(2) {
        if let &[a, b] = w {
            if b > a {
                best = best.min(b - a);
            }
        }
    }
    best
}

fn analytic_nurb_slack(
    k: &Kernel,
    surf: &Surface,
    form: &crate::nurbs::b_spline_surface::BSplineSurface,
) -> Result<Option<f64>> {
    let kind = surf.kind_with_nurb(k)?;
    let origin = match kind {
        SurfaceKind::BSpline(_) | SurfaceKind::BallPatch(_) => return Ok(Some(0.0)),
        SurfaceKind::Plane(p) => Some(p.position().origin()),
        SurfaceKind::Cone(c) => Some(c.canonical().origin.origin()),
        SurfaceKind::Sphere(_) | SurfaceKind::Torus(_) => match AnalyticSupport::of(surf) {
            Some(
                AnalyticSupport::Sphere { placement, .. }
                | AnalyticSupport::Torus { placement, .. },
            ) => Some(placement.origin()),
            _ => return Ok(None),
        },
        SurfaceKind::SurfOfRevolution(r) => Some(r.position.origin()),
        SurfaceKind::SurfOfExtrusion(e) => Some(e.origin),
        _ => return Ok(None),
    };
    let Some(nurb) = form.gw_nurb() else {
        return Ok(None);
    };
    let mut net_size = 0.0_f64;
    for row in nurb.rows() {
        for q in row {
            let w = if q.w == NOW { 1.0 } else { q.w };
            net_size = net_size
                .max(q.x.abs())
                .max(q.y.abs())
                .max(q.z.abs())
                .max(w.abs());
        }
    }
    let far = origin.map_or(0.0, |o| o.x.abs().max(o.y.abs()).max(o.z.abs()));
    let slack = round_up(gamma(32) * (2.0 * net_size + far));
    Ok(slack.is_finite().then_some(slack))
}

fn unit_normal_jet_ivl(
    jet: &SurfaceJetIvl,
    forward: bool,
    with_partials: bool,
) -> Option<(V3<Ivl>, V3<Ivl>, V3<Ivl>)> {
    let mut n_raw = v3_cross(jet.su, jet.sv);
    if !forward {
        n_raw = v3_neg(n_raw);
    }
    let len2 = v3_dot(n_raw, n_raw);
    if !(len2.is_finite() && len2.lo > 1e-24) {
        return None;
    }
    let len = len2.sqrt();
    if !(len.is_finite() && len.lo > 0.0) {
        return None;
    }
    let inv_len = Ivl::from(1.0) / len;
    let n = v3_scale(n_raw, inv_len);
    if !v3_finite(n) {
        return None;
    }
    if !with_partials {
        return Some((n, VZ, VZ));
    }
    let mut nu_raw = v3_add(v3_cross(jet.suu, jet.sv), v3_cross(jet.su, jet.suv));
    let mut nv_raw = v3_add(v3_cross(jet.suv, jet.sv), v3_cross(jet.su, jet.svv));
    if !forward {
        nu_raw = v3_neg(nu_raw);
        nv_raw = v3_neg(nv_raw);
    }
    let nu = v3_scale(v3_sub(nu_raw, v3_scale(n, v3_dot(n, nu_raw))), inv_len);
    let nv = v3_scale(v3_sub(nv_raw, v3_scale(n, v3_dot(n, nv_raw))), inv_len);
    (v3_finite(nu) && v3_finite(nv)).then_some((n, nu, nv))
}

fn clamp_linear_ivl(raw: Ivl, g: V3<Ivl>, lo: f64, hi: f64) -> (Ivl, V3<Ivl>, Ivl) {
    if raw.hi <= lo {
        return (Ivl::from(lo), VZ, IZ);
    }
    if raw.lo >= hi {
        return (Ivl::from(hi), VZ, IZ);
    }
    let clamped = Ivl::new(raw.lo.max(lo), raw.hi.min(hi));
    let under = round_up((lo - raw.lo).max(0.0));
    let over = round_up((raw.hi - hi).max(0.0));
    let rho = Ivl::new(-over, under);
    (clamped, g, rho)
}

fn invert_2x2_ivl(m: [[Ivl; 2]; 2]) -> Option<[[Ivl; 2]; 2]> {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    if !det.is_finite() || (det.lo <= 0.0 && det.hi >= 0.0) {
        return None;
    }
    let inv_det = Ivl::from(1.0) / det;
    let out = [
        [m[1][1] * inv_det, (IZ - m[0][1]) * inv_det],
        [(IZ - m[1][0]) * inv_det, m[0][0] * inv_det],
    ];
    out.iter().flatten().all(|v| v.is_finite()).then_some(out)
}

fn v3_pt(p: Point3) -> V3<Ivl> {
    V3 {
        x: Ivl::from(p.x),
        y: Ivl::from(p.y),
        z: Ivl::from(p.z),
    }
}

fn v3_vec(v: Vec3) -> V3<Ivl> {
    V3 {
        x: Ivl::from(v.x),
        y: Ivl::from(v.y),
        z: Ivl::from(v.z),
    }
}

fn v3_finite(v: V3<Ivl>) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

fn v3_add(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn v3_sub(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn v3_neg(a: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: IZ - a.x,
        y: IZ - a.y,
        z: IZ - a.z,
    }
}

fn v3_scale(a: V3<Ivl>, s: Ivl) -> V3<Ivl> {
    V3 {
        x: a.x * s,
        y: a.y * s,
        z: a.z * s,
    }
}

fn v3_dot(a: V3<Ivl>, b: V3<Ivl>) -> Ivl {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn v3_cross(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

fn validate_domain(d: &Extent2d) -> Result<()> {
    let du = d.u_max() - d.u_min();
    let dv = d.v_max() - d.v_min();
    if !du.is_finite() || !dv.is_finite() || du <= 1e-12 || dv <= 1e-12 {
        return Err(SolidError::invalid_input(
            OP_NEW,
            "reference surface has a degenerate UV domain",
        ));
    }
    Ok(())
}

fn clamp_uv(uv: Point2, d: &Extent2d) -> Point2 {
    let x = if uv.x < d.u_min() {
        d.u_min()
    } else if uv.x > d.u_max() {
        d.u_max()
    } else {
        uv.x
    };
    let y = if uv.y < d.v_min() {
        d.v_min()
    } else if uv.y > d.v_max() {
        d.v_max()
    } else {
        uv.y
    };
    Point2::new(x, y)
}

fn unit_normal(surface: &Surface, k: &Kernel, uv: Point2) -> Result<Vec3> {
    let n = surface.evaluate_normal(k, uv, Side::Right, Side::Right)?;
    let len = n.length();
    if !len.is_finite() || len <= 1e-12 {
        return Err(SolidError::invalid_input(
            OP_MAP,
            "surface has a degenerate normal at projected point",
        ));
    }
    Ok(n / len)
}

fn orthonormal_basis(unit_d: Vec3) -> (Vec3, Vec3) {
    let helper = if unit_d.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let e1_raw = unit_d.cross(helper);
    let e1 = e1_raw / e1_raw.length();
    let e2 = unit_d.cross(e1);
    (e1, e2)
}
