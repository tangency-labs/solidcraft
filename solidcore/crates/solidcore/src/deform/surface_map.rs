//! Surface-to-surface space deformation ([`SurfaceToSurfaceMap`]).
//!
//! Given a base surface `S_b(u, v)` and a control (target) surface `S_c(u, v)`, each
//! normalized to `[0, 1] x [0, 1]`:
//! 1. Project `P` onto `S_b` (either by closest point / normal or along a fixed unit
//!    direction) to get `(u_b, v_b)`, foot point `B = S_b(u_b, v_b)`, and unit normal `n_b`.
//! 2. Map the normalized `(u, v)` (with optional `swap_uv`, `flip_u`, `flip_v`) to `(u_c, v_c)`
//!    on `S_c`, obtaining `C = S_c(u_c, v_c)` and unit normal `n_c`.
//! 3. Compute the mapped point `P'` either by [`MapMode::NormalToNormal`]
//!    (`C + h * n_c`) or [`MapMode::Translate`] (`C + (P - B)`), then blend with `scale`:
//!    `P_out = P + scale * (P' - P)`.

use crate::deform::SpaceMap;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::intersect::adv_surface_intersector::NoSurfaceHandle;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::surface::Surface;

const OP_NEW: &str = "deform::surface_map::SurfaceToSurfaceMap::new";
const OP_MAP: &str = "deform::surface_map::SurfaceToSurfaceMap::map_point";

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
        validate_domain(&base_domain, "base")?;
        validate_domain(&control_domain, "control")?;
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
                let len = d.length();
                if !len.is_finite() || len <= 1e-12 {
                    return Err(SolidError::invalid_input(
                        OP_NEW,
                        "projection direction must be non-zero and finite",
                    ));
                }
                let unit_d = d / len;
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
        })
    }

    fn project_normal(&self, k: &Kernel, p: Point3) -> Result<Point2> {
        if let Ok(solutions) = self.base.global_point_solve::<NoSurfaceHandle>(
            k,
            None,
            &self.base_domain,
            SolverOperation::Minimize,
            p,
            self.options.tolerance,
            None,
            SolutionRequested::Single,
        ) {
            if let Some(sol) = solutions.get(0) {
                if let (Ok(u), Ok(v)) = (sol.start.param(0), sol.start.param(1)) {
                    if u.is_finite() && v.is_finite() {
                        return Ok(clamp_uv(Point2::new(u, v), &self.base_domain));
                    }
                }
            }
        }
        self.project_normal_fallback(k, p)
    }

    fn project_normal_fallback(&self, k: &Kernel, p: Point3) -> Result<Point2> {
        let u0 = self.base_domain.u_min();
        let u1 = self.base_domain.u_max();
        let v0 = self.base_domain.v_min();
        let v1 = self.base_domain.v_max();
        let du = u1 - u0;
        let dv = v1 - v0;

        let mut best_uv = Point2::new(0.5 * (u0 + u1), 0.5 * (v0 + v1));
        let mut best_d2 = f64::INFINITY;
        const N: usize = 9;
        for iu in 0..=N {
            let u = u0 + du * (iu as f64 / N as f64);
            for iv in 0..=N {
                let v = v0 + dv * (iv as f64 / N as f64);
                let pt = self.base.evaluate_point(k, Point2::new(u, v))?;
                let diff = pt - p;
                let d2 = diff.dot(diff);
                if d2 < best_d2 {
                    best_d2 = d2;
                    best_uv = Point2::new(u, v);
                }
            }
        }

        let mut uv = best_uv;
        let flags = EvaluateFlags::default();
        for _ in 0..16 {
            let eval = self.base.evaluate(k, uv, 1, 1, flags)?;
            let Some(row0) = eval.first() else { break };
            let Some(row1) = eval.get(1) else { break };
            let (Some(&s), Some(&sv), Some(&su)) = (row0.first(), row0.get(1), row1.first()) else {
                break;
            };
            let r = s - p;
            let a11 = su.dot(su);
            let a12 = su.dot(sv);
            let a22 = sv.dot(sv);
            let b1 = -r.dot(su);
            let b2 = -r.dot(sv);
            let det = a11 * a22 - a12 * a12;
            if det.abs() <= 1e-24 {
                break;
            }
            let step_u = (a22 * b1 - a12 * b2) / det;
            let step_v = (a11 * b2 - a12 * b1) / det;
            let next = clamp_uv(Point2::new(uv.x + step_u, uv.y + step_v), &self.base_domain);
            let moved = (next.x - uv.x).abs() + (next.y - uv.y).abs();
            uv = next;
            if moved <= 1e-12 * (du.abs() + dv.abs() + 1.0) {
                break;
            }
        }
        Ok(uv)
    }

    fn project_direction(&self, k: &Kernel, p: Point3, e1: Vec3, e2: Vec3) -> Result<Point2> {
        let u0 = self.base_domain.u_min();
        let u1 = self.base_domain.u_max();
        let v0 = self.base_domain.v_min();
        let v1 = self.base_domain.v_max();
        let du = u1 - u0;
        let dv = v1 - v0;

        let mut best_uv = Point2::new(0.5 * (u0 + u1), 0.5 * (v0 + v1));
        let mut best_e2 = f64::INFINITY;
        const N: usize = 9;
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
        for _ in 0..20 {
            let eval = self.base.evaluate(k, uv, 1, 1, flags)?;
            let Some(row0) = eval.first() else { break };
            let Some(row1) = eval.get(1) else { break };
            let (Some(&s), Some(&sv), Some(&su)) = (row0.first(), row0.get(1), row1.first()) else {
                break;
            };
            let r = s - p;
            let f1 = r.dot(e1);
            let f2 = r.dot(e2);
            if f1 * f1 + f2 * f2 <= self.options.tolerance * self.options.tolerance * 1e-4 {
                break;
            }
            let j11 = su.dot(e1);
            let j12 = sv.dot(e1);
            let j21 = su.dot(e2);
            let j22 = sv.dot(e2);
            let det = j11 * j22 - j12 * j21;
            if det.abs() <= 1e-24 {
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
}

impl SpaceMap for SurfaceToSurfaceMap<'_> {
    fn map_point(&self, k: &Kernel, p: Point3) -> Result<Point3> {
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
}

fn validate_domain(d: &Extent2d, _name: &str) -> Result<()> {
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
