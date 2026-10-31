//! Implicit iso-parametric curves: the constant-`u` or constant-`v` curve of a surface,
//! evaluated as `C(s) = S(C_uv(s))` without building a curve.
//!
//! Upstream: `source/SMLib/src/SmIsoCurve.cpp`, `source/SMLib/inc/SmIsoCurve.h`.
//!
//! | Rust                                   | Upstream                                       |
//! |----------------------------------------|------------------------------------------------|
//! | [`IsoCurve`]                           | `SmIsoCurve`                                   |
//! | [`IsoCurve::new`], [`IsoCurve::from_surface`] | `SmIsoCurve(crSurface, eSurfParam, dIsoParameter, bFromLeft, bClampInput)` |
//! | `Drop` (implicit)                      | `~SmIsoCurve`                                  |
//! | [`IsoCurve::approx_eq`]                | `SmIsoCurve::operator==`                       |
//! | [`IsoCurve::create_uv_iso_line`], [`IsoCurve::create_uv_iso_line_with_nurb`] | `SmIsoCurve::CreateUVIsoLine` |
//! | [`IsoCurve::calculate_continuities`]   | `SmIsoCurve::CalculateContinuities`            |
//! | [`IsoCurve::natural_interval`]         | `SmIsoCurve::GetNaturalInterval`               |
//! | [`IsoCurve::evaluate`]                 | `SmIsoCurve::Evaluate`                         |
//! | [`IsoCurve::evaluate_point`]           | `SmIsoCurve::EvaluatePoint`                    |
//! | [`IsoCurve::knots`]                    | `SmIsoCurve::GetKnots`                         |
//! | [`IsoCurve::read_from_db`]             | `SmIsoCurve::ReadFromDB`                       |
//! | [`IsoCurve::TYPE`], [`IsoCurve::type_code`], [`IsoCurve::type_string`], [`IsoCurve::class_string`] | `GetType`, `GetTypeString`, `GetClassString` (`SM_COMMON`) |
//! | [`IsoCurve::is_kind_of`]               | `SmIsoCurve::IsKindOf`                         |
//!
//! Deviations:
//! - **Surface.** The borrowed C++ surface is an owned, boxed [`Surface`] copy.
//!   [`IsoCurve::new`] retains the B-spline constructor; [`IsoCurve::from_surface`]
//!   also supports the other concrete surface kinds through the same virtual dispatch.
//!   This extension is needed by `surface.surface_point_solve_p3` boundary solves.
//!   The general constructor bounds owned geometry nesting to `MAX_GEOMETRY_NESTING` before
//!   any recursive surface dispatch; the B-spline constructor cannot nest.
//! - **Natural domain.** Upstream's `GetNaturalUVDomain` makes a lazy analytic's
//!   missing NURB first. [`IsoCurve::from_surface`] (its clamp) and
//!   [`IsoCurve::create_uv_iso_line_with_nurb`] read the domain so, with the kernel.
//!   [`IsoCurve::new`] holds a B-spline, which is never lazy. The kernel-free
//!   [`IsoCurve::create_uv_iso_line`] and [`IsoCurve::natural_interval`] read the
//!   surface's domain as far as it is built
//!   ([`Surface::natural_uv_domain_if_built`]), which is the same for every surface
//!   but a lazy analytic.
//! - **Equality.** `operator==` accepts identical surface pointers before comparing the
//!   surfaces (`SmBSplineSurface::operator==`); the port always compares the surfaces
//!   ([`Surface::approx_eq`]; comparison failures propagate).
//!   `SmCurve::operator==` (same type and dimension) always holds between two iso
//!   curves (dimension 3).
//! - **Flags.** `m_bFromLeft` is a `bool` (upstream `SmBoolean`, `UNSURE` only in the
//!   empty I/O constructor, whose state is never observable here). `SetContext` and the
//!   allocation-only `SmContext` arguments are omitted. Evaluation and continuity
//!   queries retain the caller's Kernel through the concrete surface dispatch;
//!   `CalculateContinuities`' unused angle tolerance is omitted.
//! - **`CreateUVIsoLine`** keeps upstream's branches by default: a constant-`u` curve
//!   gets a line at `x = iso` along `(0, 1, 0)` over the *u* interval of the domain,
//!   from the domain's minimum v; the second branch repeats the `SM_SP_U` test, so
//!   every other parameter type is `SM_ERR_INVALID_INPUT`
//!   ([`SolidError::InvalidInput`]). With
//!   [`Kernel::fixes.iso_curve_uv_line_both_params`](crate::kernel::UpstreamFixes::iso_curve_uv_line_both_params) (opt-in, default off) the
//!   kernel-threaded entry point builds both directions over the varying coordinate's
//!   interval from a zero varying origin: `UV(t) = (iso, t, 0)` for constant `u`,
//!   `UV(t) = (t, iso, 0)` for constant `v` (WR-30). The kernel-free
//!   [`IsoCurve::create_uv_iso_line`] always keeps the pinned body.
//! - **`ReadFromDB`** always builds a new curve (the "fill the curve passed in" branch
//!   and its `IsKindOf` check have no counterpart; the type and dimension arguments are
//!   unused upstream and dropped). It reads the iso data and the surface type, then
//!   needs `SmSurface::ReadFromDB`. An ASCII surface type whose extraction fails
//!   (upstream reads an uninitialised `SM_TYPE`) is [`SolidError::InvalidInput`].
//! - `Dump` (print only) and `GetMemoryUsed` are not ported; `WriteToDB` belongs to
//!   another unit.

use crate::curve::curve::p2::{self as curve_base, ISO_CURVE_TYPE};
use crate::curve::curve::Curve;
use crate::curve::line::{BoundedLineOptions, Line};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::tol::is_zero;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::surface::p2::Surface;
use crate::topology::database_io::{DatabaseReader, FileType, InputStream};

const OP_UV_ISO_LINE: &str = "curve::iso_curve::IsoCurve::create_uv_iso_line";
const OP_READ: &str = "curve::iso_curve::IsoCurve::read_from_db";

/// An iso-parametric curve of a surface (`SmIsoCurve`): `S(iso, s)` for a constant-`u`
/// curve ([`SurfParamType::U`]), `S(s, iso)` otherwise. Image dimension 3.
#[derive(Debug, Clone)]
pub struct IsoCurve {
    /// `m_cpSurface`.
    surface: Box<Surface>,
    /// `m_eSurfParam`: constant `u` or `v`.
    surf_param: SurfParamType,
    /// `m_dIsoParameter`: the constant `u` or `v` value.
    iso_parameter: f64,
    /// `m_bFromLeft`: side of a knot the iso parameter is evaluated on.
    from_left: bool,
}

impl IsoCurve {
    /// Exact evaluation copy after the caller validates the geometry depth.
    pub(crate) fn clone_exact_bounded(&self) -> Self {
        Self {
            surface: Box::new(self.surface.clone_exact_bounded()),
            surf_param: self.surf_param,
            iso_parameter: self.iso_parameter,
            from_left: self.from_left,
        }
    }

    /// An exact evaluation copy without re-clamping or resetting surface owners.
    /// `depth` is the surface's depth in the shared `MAX_GEOMETRY_NESTING` (32) geometry traversal.
    pub(crate) fn clone_for_evaluation(&self, depth: usize) -> Result<Self> {
        Ok(Self {
            surface: Box::new(self.surface.clone_for_evaluation_depth(depth)?),
            surf_param: self.surf_param,
            iso_parameter: self.iso_parameter,
            from_left: self.from_left,
        })
    }

    /// `SmIsoCurve_TYPE` (`CURV_BASE_TYPE + 45`).
    pub const TYPE: i64 = ISO_CURVE_TYPE;

    /// The iso curve of `surface` at `iso_parameter` in direction `surf_param`
    /// (`SM_SP_U` = constant `u`, anything else constant `v`), evaluated from the
    /// left (`from_left`) at a knot of the iso direction. With `clamp_input` the iso
    /// parameter is clamped to the surface's natural domain.
    #[must_use]
    pub fn new(
        surface: BSplineSurface,
        surf_param: SurfParamType,
        iso_parameter: f64,
        from_left: bool,
        clamp_input: bool,
    ) -> Self {
        let surface = Surface::from(surface);
        // (a B-spline is never lazy: its domain is built)
        let clamp_domain = clamp_input.then(|| surface.natural_uv_domain_if_built());
        Self::from_surface_inner(surface, surf_param, iso_parameter, from_left, clamp_domain)
    }

    /// The implicit iso curve of any concrete surface, preserving its evaluation dispatch.
    /// With `clamp_input` the iso parameter is clamped to the surface's natural domain
    /// (`GetNaturalUVDomain`, which makes a lazy analytic's missing NURB).
    ///
    /// # Errors
    ///
    /// `InvalidInput` when `surface` nests deeper than
    /// [`MAX_GEOMETRY_NESTING`](crate::surface::surface::p2::MAX_GEOMETRY_NESTING) - 1;
    /// [`SolidError::Unported`] from making the NURB.
    pub fn from_surface(
        k: &Kernel,
        surface: Surface,
        surf_param: SurfParamType,
        iso_parameter: f64,
        from_left: bool,
        clamp_input: bool,
    ) -> Result<Self> {
        surface.check_evaluation_copy_depth(1)?;
        let clamp_domain = if clamp_input {
            Some(surface.natural_uv_domain_with_nurb(k)?)
        } else {
            None
        };
        Ok(Self::from_surface_inner(
            surface,
            surf_param,
            iso_parameter,
            from_left,
            clamp_domain,
        ))
    }

    /// The constructor body; `clamp_domain` is the surface's natural domain when the
    /// input is clamped (`bClampInput`).
    fn from_surface_inner(
        surface: Surface,
        surf_param: SurfParamType,
        iso_parameter: f64,
        from_left: bool,
        clamp_domain: Option<Extent2d>,
    ) -> Self {
        let mut c = Self {
            surface: Box::new(surface),
            surf_param,
            iso_parameter,
            from_left,
        };
        let Some(uv_domain) = clamp_domain else {
            return c;
        };

        if surf_param == SurfParamType::U {
            if uv_domain.min().x > iso_parameter {
                c.iso_parameter = uv_domain.min().x;
            }
            if iso_parameter > uv_domain.max().x {
                c.iso_parameter = uv_domain.max().x;
            }
        } else {
            if uv_domain.min().y > iso_parameter {
                c.iso_parameter = uv_domain.min().y;
            }
            if iso_parameter > uv_domain.max().y {
                c.iso_parameter = uv_domain.max().y;
            }
        }
        c
    }

    /// The base surface (`m_cpSurface`).
    #[must_use]
    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    /// Constant `u` or `v` (`m_eSurfParam`).
    #[must_use]
    pub const fn surf_param(&self) -> SurfParamType {
        self.surf_param
    }

    /// The constant parameter value (`m_dIsoParameter`).
    #[must_use]
    pub const fn iso_parameter(&self) -> f64 {
        self.iso_parameter
    }

    /// Whether the iso parameter is evaluated from the left (`m_bFromLeft`).
    #[must_use]
    pub const fn from_left(&self) -> bool {
        self.from_left
    }

    /// Image-space dimension (`SmCurve(3)`).
    #[must_use]
    pub const fn dim(&self) -> u64 {
        3
    }

    /// `operator==`: equal surfaces, the same direction and flag, and iso parameters
    /// within `SM_IS_ZERO`.
    pub fn approx_eq(&self, k: &Kernel, other: &Self) -> Result<bool> {
        // low work
        if core::ptr::eq(self, other) {
            return Ok(true);
        }
        // first check the base: same type and dimension (always for two iso curves)
        Ok(self.surface.approx_eq(k, &other.surface)?
            && self.surf_param == other.surf_param
            && is_zero(self.iso_parameter - other.iso_parameter)
            && self.from_left == other.from_left)
    }

    /// The UV line of the `C_uv(s)` part of `S(C_uv(s))` (`CreateUVIsoLine`), a 2D
    /// `SmLine`, over the surface's domain as far as it is built
    /// ([`Surface::natural_uv_domain_if_built`]: an iso curve of a lazy analytic uses
    /// [`IsoCurve::create_uv_iso_line_with_nurb`]). Always the pinned v1.0.1 body,
    /// whatever the kernel flag: only constant `u`, over the u interval and from the
    /// domain-minimum origin.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] unless the curve is constant `u` (upstream tests
    /// `SM_SP_U` twice).
    pub fn create_uv_iso_line(&self) -> Result<Curve> {
        self.uv_iso_line(self.surface.natural_uv_domain_if_built(), false)
    }

    /// [`IsoCurve::create_uv_iso_line`] over the surface's natural domain as upstream
    /// reads it (`GetNaturalUVDomain`, which makes a lazy analytic's missing NURB).
    /// With [`Kernel::fixes.iso_curve_uv_line_both_params`](crate::kernel::UpstreamFixes::iso_curve_uv_line_both_params) both directions are built
    /// over the varying coordinate's interval from a zero varying origin; without it
    /// the pinned body is used.
    ///
    /// # Errors
    ///
    /// Those of [`IsoCurve::create_uv_iso_line`]; [`SolidError::Unported`] from making
    /// the NURB.
    pub fn create_uv_iso_line_with_nurb(&self, k: &Kernel) -> Result<Curve> {
        self.uv_iso_line(
            self.surface.natural_uv_domain_with_nurb(k)?,
            k.fixes.iso_curve_uv_line_both_params,
        )
    }

    /// The body of `CreateUVIsoLine` over the surface's natural `domain`. With
    /// `both_params` (the opt-in WR-30 fix) both directions are built; without it the
    /// body is pinned v1.0.1's: the `SM_SP_U` test is repeated, so only constant `u`
    /// succeeds, over the u interval and from the domain-minimum origin.
    fn uv_iso_line(&self, domain: Extent2d, both_params: bool) -> Result<Curve> {
        // locals
        let mut line_point = Point3::new(domain.min().x, domain.min().y, 0.0);
        let (line_ivl, line_unit_vec) = if self.surf_param == SurfParamType::U {
            if both_params {
                // UV(t) = (iso, t, 0): the varying coordinate starts at zero. The
                // line is P + s*V, so the domain minimum must not be added twice.
                line_point = Point3::new(self.iso_parameter, 0.0, 0.0);
                (domain.v_interval(), Vec3::new(0.0, 1.0, 0.0))
            } else {
                line_point.x = self.iso_parameter;
                (domain.u_interval(), Vec3::new(0.0, 1.0, 0.0))
            }
        } else if both_params && self.surf_param == SurfParamType::V {
            // UV(t) = (t, iso, 0), likewise from a zero varying origin.
            line_point = Point3::new(0.0, self.iso_parameter, 0.0);
            (domain.u_interval(), Vec3::new(1.0, 0.0, 0.0))
        } else {
            // the `else if` branch repeats the SM_SP_U test and is never taken
            return Err(SolidError::InvalidInput {
                operation: OP_UV_ISO_LINE,
                reason: "Bad value for m_eSurfParam",
            });
        };

        // create IsoLine
        Ok(new_line(line_point, line_unit_vec, &line_ivl, 1.0, 2))
    }

    /// The minimum continuity and the continuities at the knots, inherited from the
    /// surface in the iso direction (`CalculateContinuities`).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] (`SmSurface::CalculateContinuities`).
    pub fn calculate_continuities(
        &self,
        k: &Kernel,
    ) -> Result<(ContinuityType, Vec<ContinuityType>)> {
        // pass the call along
        surface_calculate_continuities(k, &self.surface, self.surf_param)
    }

    /// The natural interval: the surface domain across the iso direction
    /// (`GetNaturalInterval`).
    #[must_use]
    pub fn natural_interval(&self) -> Extent1d {
        // (kernel-free: a lazy analytic answers its NURB-less base's empty domain)
        let uv_domain = self.surface.natural_uv_domain_if_built();
        if self.surf_param == SurfParamType::U {
            return Extent1d::from_min_max(uv_domain.min().y, uv_domain.max().y);
        }
        Extent1d::from_min_max(uv_domain.min().x, uv_domain.max().x)
    }

    /// The surface parameter of curve parameter `t`.
    fn uv(&self, t: f64) -> Point2 {
        if self.surf_param == SurfParamType::U {
            Point2::new(self.iso_parameter, t)
        } else {
            Point2::new(t, self.iso_parameter)
        }
    }

    /// Position and derivatives `0..=num_derivs` at `t` (`Evaluate`): the surface's
    /// derivatives along the curve direction, with `from_left` for the curve parameter
    /// and the stored flag for the iso parameter.
    ///
    /// # Errors
    ///
    /// Those of the surface evaluation ([`BSplineSurface::evaluate`]).
    pub fn evaluate(
        &self,
        k: &Kernel,
        t: f64,
        num_derivs: usize,
        from_left: bool,
        non_zero_tangents: bool,
    ) -> Result<Vec<Vec3>> {
        let side = |left: bool| if left { Side::Left } else { Side::Right };
        let uv = self.uv(t);
        let (num_u, num_v, u_left, v_left) = if self.surf_param == SurfParamType::U {
            // Constant U curve
            (0, num_derivs, self.from_left, from_left)
        } else {
            // Constant V curve
            (num_derivs, 0, from_left, self.from_left)
        };
        let d = self.surface.evaluate(
            k,
            uv,
            num_u,
            num_v,
            EvaluateFlags {
                u_side: side(u_left),
                v_side: side(v_left),
                only_upper_half: false,
                non_zero_tangents,
            },
        )?;
        // one of the orders is zero: the flat `[nu + 1][nv + 1]` array is the curve's
        Ok(d.into_iter().flatten().collect())
    }

    /// The point at `t` (`EvaluatePoint`).
    ///
    /// # Errors
    ///
    /// Those of [`BSplineSurface::evaluate_point`] (`SM_ERR` without NURB).
    pub fn evaluate_point(&self, k: &Kernel, t: f64) -> Result<Point3> {
        self.surface.evaluate_point(k, self.uv(t))
    }

    /// The distinct knots and multiplicities of the surface across the iso direction,
    /// optionally only those in `interval` (`GetKnots`).
    ///
    /// # Errors
    ///
    /// Those of [`BSplineSurface::knots`] (`SM_ERR` without NURB).
    pub fn knots(&self, interval: Option<&Extent1d>) -> Result<(Vec<f64>, Vec<usize>)> {
        let sp = if self.surf_param == SurfParamType::U {
            SurfParamType::V
        } else {
            SurfParamType::U
        };
        self.surface.knots(sp, interval)
    }

    /// Reads an iso curve from `db` (`ReadFromDB`): direction, iso parameter and flag,
    /// then the surface (type header and data). `version` is passed to the surface
    /// reader.
    ///
    /// # Errors
    ///
    /// Those of the stream reads; [`SolidError::InvalidInput`] for an unreadable ASCII
    /// surface type; [`SolidError::Unported`] (`SmSurface::ReadFromDB`).
    pub fn read_from_db(k: &Kernel, db: &mut DatabaseReader, version: u64) -> Result<Self> {
        // curve locals
        let mut surf_param: u64 = 0;
        let mut iso_parameter = 0.0;
        let mut from_left: i32 = 0;

        if db.file_type() == FileType::Ascii {
            if let Some(v) = db.in_stream_mut().and_then(InputStream::extract_ulong) {
                surf_param = v;
            }
            db.go_to_next_line();
            if let Some(v) = db.in_stream_mut().and_then(InputStream::extract_double) {
                iso_parameter = v;
            }
            db.go_to_next_line();
            if let Some(v) = db.in_stream_mut().and_then(InputStream::extract_int) {
                from_left = v;
            }
            db.go_to_next_line();
        } else {
            surf_param = db.read_ulong()?;
            iso_parameter = db.read_double()?;
            from_left = db.read_boolean()?;
        }

        // compounding surface
        if db.file_type() == FileType::Ascii {
            db.go_to_next_line();
        }
        let Some(surface_type) = db.read_type(false)?.type_code else {
            return Err(SolidError::InvalidInput {
                operation: OP_READ,
                reason: "surface type not read",
            });
        };
        let surface = surface_read_from_db(k, surface_type, db, version)?;

        // load obj
        Self::from_surface(
            k,
            surface,
            // a code outside the enum behaves as a non-`SM_SP_U` value everywhere here
            SurfParamType::from_code(surf_param).unwrap_or(SurfParamType::Unknown),
            iso_parameter,
            from_left != 0,
            false,
        )
    }

    /// `SmIsoCurve_TYPE` (`GetType`).
    #[must_use]
    pub const fn type_code(&self) -> i64 {
        Self::TYPE
    }

    /// `"SmIsoCurve_TYPE"` (`GetTypeString`).
    #[must_use]
    pub const fn type_string(&self) -> &'static str {
        "SmIsoCurve_TYPE"
    }

    /// `"SmIsoCurve"` (`GetClassString`).
    #[must_use]
    pub const fn class_string(&self) -> &'static str {
        "SmIsoCurve"
    }

    /// `IsKindOf`: `SmIsoCurve_TYPE` or a base type of `SmCurve`.
    #[must_use]
    pub const fn is_kind_of(t: i64) -> bool {
        t == Self::TYPE || curve_base::is_kind_of(t)
    }
}

// ---- unported callees -------------------------------------------------------------

/// `SmSurface::CalculateContinuities(eSurfParam, ..)` (not overridden by
/// `SmBSplineSurface`).
fn surface_calculate_continuities(
    k: &Kernel,
    srf: &Surface,
    surf_param: SurfParamType,
) -> Result<(ContinuityType, Vec<ContinuityType>)> {
    srf.calculate_continuities(k, surf_param)
}

/// `SmSurface::ReadFromDB(type, db, .., version)`: the concrete surface of the record.
fn surface_read_from_db(
    k: &Kernel,
    surface_type: i64,
    db: &mut DatabaseReader,
    version: u64,
) -> Result<Surface> {
    Surface::read_from_db(k, surface_type, db, version)
}

/// `new SmLine(point, unit_vec, interval, scale, dim, context)`.
fn new_line(point: Point3, unit_vec: Vec3, interval: &Extent1d, scale: f64, dim: u64) -> Curve {
    Curve::from(Line::bounded(
        point,
        unit_vec,
        interval,
        &BoundedLineOptions {
            scale,
            dim,
            ..BoundedLineOptions::default()
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::math::CPoint;
    use crate::nurbs::srf_basic::Surface;

    /// A bilinear patch over `[0, 2] x [-1, 3]`.
    fn patch() -> BSplineSurface {
        let net = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, 1.0),
                CPoint::new(0.0, 1.0, 0.0, 1.0),
            ],
            vec![
                CPoint::new(1.0, 0.0, 1.0, 1.0),
                CPoint::new(1.0, 1.0, 2.0, 1.0),
            ],
        ];
        let s = Surface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 2.0, 2.0],
            vec![-1.0, -1.0, 3.0, 3.0],
        )
        .expect("surface");
        BSplineSurface::from_nurb(s, false)
    }

    /// The same bilinear net as [`patch`] over `[u_min, u_min + 2] x [v_min, v_min + 4]`.
    fn patch_with_domain(u_min: f64, v_min: f64) -> BSplineSurface {
        let net = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, 1.0),
                CPoint::new(0.0, 1.0, 0.0, 1.0),
            ],
            vec![
                CPoint::new(1.0, 0.0, 1.0, 1.0),
                CPoint::new(1.0, 1.0, 2.0, 1.0),
            ],
        ];
        let s = Surface::new(
            net,
            1,
            1,
            vec![u_min, u_min, u_min + 2.0, u_min + 2.0],
            vec![v_min, v_min, v_min + 4.0, v_min + 4.0],
        )
        .expect("surface");
        BSplineSurface::from_nurb(s, false)
    }

    /// The same bilinear net as [`patch`] over `[0, 2] x [0, 2]` (equal u and v
    /// intervals, zero minima: the ordinary success control).
    fn equal_domain_patch() -> BSplineSurface {
        let net = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, 1.0),
                CPoint::new(0.0, 1.0, 0.0, 1.0),
            ],
            vec![
                CPoint::new(1.0, 0.0, 1.0, 1.0),
                CPoint::new(1.0, 1.0, 2.0, 1.0),
            ],
        ];
        let s = Surface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 2.0, 2.0],
            vec![0.0, 0.0, 2.0, 2.0],
        )
        .expect("surface");
        BSplineSurface::from_nurb(s, false)
    }

    fn kernel_with_uv_iso_line_fix() -> Kernel {
        let mut k = Kernel::default();
        k.fixes.iso_curve_uv_line_both_params = true;
        k
    }

    fn assert_point(actual: Point3, expected: Point3) {
        let d = actual.distance(expected);
        assert!(d < 1e-12, "point {actual:?} != {expected:?} (distance {d})");
    }

    /// Checks an opt-in UV line against its independent UV map (`uv_of`), its lift
    /// through the iso curve's own surface accessor and the same iso curve evaluated
    /// at the same parameter, and an independent bilinear formula (`lift_of`).
    fn assert_uv_line(
        line: &Curve,
        k: &Kernel,
        iso: &IsoCurve,
        samples: &[f64],
        uv_of: impl Fn(f64) -> Point3,
        lift_of: impl Fn(f64) -> Point3,
    ) {
        assert_eq!(line.dim(), 2);
        assert_eq!(line.natural_interval(), iso.natural_interval());
        for &t in samples {
            let p = line.evaluate_point(k, t).expect("uv line point");
            assert_point(p, uv_of(t));
            let lifted = iso
                .surface()
                .evaluate_point(k, Point2::new(p.x, p.y))
                .expect("lifted surface point");
            assert_point(lifted, iso.evaluate_point(k, t).expect("iso point"));
            assert_point(lifted, lift_of(t));
        }
    }

    #[test]
    fn clamps_to_domain() {
        let c = IsoCurve::new(patch(), SurfParamType::U, 5.0, true, true);
        assert_eq!(c.iso_parameter(), 2.0);
        let c = IsoCurve::new(patch(), SurfParamType::V, -5.0, true, true);
        assert_eq!(c.iso_parameter(), -1.0);
        let c = IsoCurve::new(patch(), SurfParamType::V, -5.0, true, false);
        assert_eq!(c.iso_parameter(), -5.0);
    }

    #[test]
    fn interval_and_knots() {
        let c = IsoCurve::new(patch(), SurfParamType::U, 1.0, false, true);
        let i = c.natural_interval();
        assert_eq!((i.min(), i.max()), (-1.0, 3.0));
        let (k, m) = c.knots(None).expect("knots");
        assert_eq!(k, vec![-1.0, 3.0]);
        assert_eq!(m, vec![2, 2]);
        let c = IsoCurve::new(patch(), SurfParamType::V, 1.0, false, true);
        let i = c.natural_interval();
        assert_eq!((i.min(), i.max()), (0.0, 2.0));
    }

    #[test]
    fn equality_and_types() {
        let k = &Kernel::default();
        let a = IsoCurve::new(patch(), SurfParamType::U, 1.0, false, true);
        let b = IsoCurve::new(patch(), SurfParamType::U, 1.0 + 1e-15, false, true);
        assert!(a.approx_eq(k, &b).unwrap());
        let c = IsoCurve::new(patch(), SurfParamType::U, 1.0, true, true);
        assert!(!a.approx_eq(k, &c).unwrap());
        assert!(IsoCurve::is_kind_of(IsoCurve::TYPE));
        assert!(IsoCurve::is_kind_of(curve_base::TYPE));
        assert_eq!(a.type_string(), "SmIsoCurve_TYPE");
    }

    #[test]
    fn evaluation_and_errors() {
        let k = &Kernel::default();
        let a = IsoCurve::new(patch(), SurfParamType::V, 1.0, false, true);
        assert!(matches!(
            a.create_uv_iso_line(),
            Err(SolidError::InvalidInput { .. })
        ));
        // constant v = 1 on the patch: x = u / 2, y = (v + 1) / 4
        let p = a.evaluate_point(k, 0.5).expect("point");
        assert!((p.y - 0.5).abs() < 1e-12);
        let d = a.evaluate(k, 0.5, 1, false, true).expect("evaluate");
        assert_eq!(d.len(), 2);
        assert!(d[0].distance(p) < 1e-12);
        assert!((d[1].x - 0.5).abs() < 1e-12 && d[1].y.abs() < 1e-12);
    }

    #[test]
    fn uv_iso_line_default_is_pinned() {
        let k = &Kernel::default();
        let fixed = &kernel_with_uv_iso_line_fix();
        let iso = 1.0;

        // Constant u: pinned v1.0.1 uses the u interval and the domain-minimum
        // origin (the line is P + s*V, so that minimum is added to every point).
        let u = IsoCurve::new(patch(), SurfParamType::U, iso, false, true);
        let line = u.create_uv_iso_line_with_nurb(k).expect("default U line");
        assert_eq!(line.dim(), 2);
        assert_eq!(line.natural_interval(), Extent1d::from_min_max(0.0, 2.0));
        assert_point(
            line.evaluate_point(k, 0.0).expect("start"),
            Point3::new(iso, -1.0, 0.0),
        );
        assert_point(
            line.evaluate_point(k, 2.0).expect("end"),
            Point3::new(iso, 1.0, 0.0),
        );

        // The kernel-free entry point stays pinned even with the flag on.
        let kernel_free = u.create_uv_iso_line().expect("kernel-free U line");
        assert_eq!(kernel_free.natural_interval(), line.natural_interval());
        assert_point(
            kernel_free.evaluate_point(fixed, 0.0).expect("start"),
            Point3::new(iso, -1.0, 0.0),
        );
        assert_point(
            kernel_free.evaluate_point(fixed, 2.0).expect("end"),
            Point3::new(iso, 1.0, 0.0),
        );

        // Constant v is exactly the pinned refusal by default; the kernel-free
        // entry point keeps it even while a kernel with the flag on exists.
        let v = IsoCurve::new(patch(), SurfParamType::V, iso, false, true);
        for line in [v.create_uv_iso_line(), v.create_uv_iso_line_with_nurb(k)] {
            match line {
                Err(SolidError::InvalidInput { operation, reason }) => {
                    assert_eq!(operation, OP_UV_ISO_LINE);
                    assert_eq!(reason, "Bad value for m_eSurfParam");
                }
                _ => panic!("expected InvalidInput"),
            }
        }
    }

    #[test]
    fn uv_iso_line_opt_in_uses_each_varying_interval_and_origin() {
        let k = &kernel_with_uv_iso_line_fix();
        let iso = 1.0;

        // Constant u: UV(t) = (iso, t, 0) over the v interval.
        let u = IsoCurve::new(patch(), SurfParamType::U, iso, false, true);
        let line = u.create_uv_iso_line_with_nurb(k).expect("U line");
        assert_uv_line(
            &line,
            k,
            &u,
            &[-1.0, 0.5, 3.0],
            |t| Point3::new(iso, t, 0.0),
            // Bilinear patch: x = u/2, y = (v + 1)/4, z = u (v + 5)/8.
            |t| Point3::new(iso / 2.0, (t + 1.0) / 4.0, iso * (t + 5.0) / 8.0),
        );

        // Constant v: UV(t) = (t, iso, 0) over the u interval.
        let v = IsoCurve::new(patch(), SurfParamType::V, iso, false, true);
        let line = v.create_uv_iso_line_with_nurb(k).expect("V line");
        assert_uv_line(
            &line,
            k,
            &v,
            &[0.0, 0.5, 2.0],
            |t| Point3::new(t, iso, 0.0),
            |t| Point3::new(t / 2.0, (iso + 1.0) / 4.0, t * (iso + 5.0) / 8.0),
        );
    }

    #[test]
    fn uv_iso_line_opt_in_equal_domain_control() {
        let k = &kernel_with_uv_iso_line_fix();
        let off = &Kernel::default();
        let iso = 1.0;

        // With zero minima and equal intervals the opt-in U line is the pinned one
        // bit for bit, so this ordinary case cannot hide an origin shift.
        let u = IsoCurve::new(equal_domain_patch(), SurfParamType::U, iso, false, true);
        let on = u.create_uv_iso_line_with_nurb(k).expect("U line with fix");
        let old = u.create_uv_iso_line().expect("pinned U line");
        assert_eq!(on.natural_interval(), old.natural_interval());
        for t in [0.0, 0.5, 2.0] {
            let opt_in = on.evaluate_point(k, t).expect("opt-in point");
            let pinned = old.evaluate_point(off, t).expect("pinned point");
            assert_eq!(opt_in, pinned);
            let lifted = u
                .surface()
                .evaluate_point(off, Point2::new(pinned.x, pinned.y))
                .expect("pinned lift");
            assert_point(lifted, u.evaluate_point(off, t).expect("pinned iso point"));
        }
        assert_uv_line(
            &on,
            k,
            &u,
            &[0.0, 0.5, 2.0],
            |t| Point3::new(iso, t, 0.0),
            // x = u/2, y = v/2, z = u (v + 2)/4.
            |t| Point3::new(iso / 2.0, t / 2.0, iso * (t + 2.0) / 4.0),
        );

        // Constant v succeeds only with the flag.
        let v = IsoCurve::new(equal_domain_patch(), SurfParamType::V, iso, false, true);
        assert!(matches!(
            v.create_uv_iso_line_with_nurb(off),
            Err(SolidError::InvalidInput { .. })
        ));
        let line = v.create_uv_iso_line_with_nurb(k).expect("V line with fix");
        assert_uv_line(
            &line,
            k,
            &v,
            &[0.0, 0.5, 2.0],
            |t| Point3::new(t, iso, 0.0),
            |t| Point3::new(t / 2.0, iso / 2.0, t * (iso + 2.0) / 4.0),
        );
    }

    #[test]
    fn uv_iso_line_opt_in_translated_domain_origin() {
        let k = &kernel_with_uv_iso_line_fix();
        let (u_min, v_min) = (1.0, 2.0);
        let (iso_u, iso_v) = (1.5, 3.0);
        let patch = || patch_with_domain(u_min, v_min);

        // Constant u over the translated v interval: the varying origin is zero, not
        // the domain minimum (which P + s*V would add to every returned UV).
        let u = IsoCurve::new(patch(), SurfParamType::U, iso_u, false, true);
        let line = u.create_uv_iso_line_with_nurb(k).expect("U line");
        assert_uv_line(
            &line,
            k,
            &u,
            &[v_min, 4.0, v_min + 4.0],
            |t| Point3::new(iso_u, t, 0.0),
            |t| {
                let lu = iso_u - u_min;
                let lv = t - v_min;
                Point3::new(lu / 2.0, lv / 4.0, (lu / 2.0) * (1.0 + lv / 4.0))
            },
        );

        // Constant v over the translated u interval, likewise from a zero origin.
        let v = IsoCurve::new(patch(), SurfParamType::V, iso_v, false, true);
        let line = v.create_uv_iso_line_with_nurb(k).expect("V line");
        assert_uv_line(
            &line,
            k,
            &v,
            &[u_min, 2.0, u_min + 2.0],
            |t| Point3::new(t, iso_v, 0.0),
            |t| {
                let lu = t - u_min;
                let lv = iso_v - v_min;
                Point3::new(lu / 2.0, lv / 4.0, (lu / 2.0) * (1.0 + lv / 4.0))
            },
        );
    }
}
