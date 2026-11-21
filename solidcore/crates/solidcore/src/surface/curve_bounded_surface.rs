//! Curve bounded (trimmed) surfaces: a basis surface with 3D and UV boundary curves,
//! used for IGES I/O. Evaluation passes every call along to the basis surface.
//!
//!
//! Upstream: `source/SMLib/src/SmCurveBoundedSurface.cpp`,
//! `source/SMLib/inc/SmCurveBoundedSurface.h`.
//!
//! | Rust                                                  | Upstream                                       |
//! |-------------------------------------------------------|------------------------------------------------|
//! | [`CurveBoundedSurface`]                               | `SmCurveBoundedSurface`                        |
//! | [`CurveBoundedSurface::new`]                          | `SmCurveBoundedSurface(rBasisSurface, cp3DBoundaries, cpUVBoundaries, bImplicitOuter)` |
//! | `Drop` (implicit)                                     | `~SmCurveBoundedSurface`                       |
//! | [`CurveBoundedSurface::approx_eq`]                    | `operator==`                                   |
//! | [`CurveBoundedSurface::natural_uv_domain`]            | `GetNaturalUVDomain`                           |
//! | [`CurveBoundedSurface::evaluate_normal`]              | `EvaluateNormal`                               |
//! | [`CurveBoundedSurface::evaluate`]                     | `Evaluate`                                     |
//! | [`CurveBoundedSurface::evaluate_simple`]              | `EvaluateSimple`                               |
//! | [`CurveBoundedSurface::evaluate_normal_at_singularity`] | `EvaluateNormalAtSingularity`                |
//! | [`CurveBoundedSurface::evaluate_geometric`]           | `EvaluateGeometric`                            |
//! | [`CurveBoundedSurface::evaluate_normal_section`]      | `EvaluateNormalSection`                        |
//! | [`CurveBoundedSurface::evaluate_point`]               | `EvaluatePoint`                                |
//! | [`CurveBoundedSurface::evaluate_step`]                | `EvaluateSTEP`                                 |
//! | [`CurveBoundedSurface::evaluate_step_point`]          | `EvaluateSTEPPoint`                            |
//! | [`CurveBoundedSurface::set_context`]                  | `SetContext`                                   |
//! | [`TYPE`], [`TYPE_STRING`], [`CLASS_STRING`], [`CurveBoundedSurface::type_code`], [`CurveBoundedSurface::type_string`], [`CurveBoundedSurface::class_string`] | `GetType`, `GetTypeString`, `GetClassString` (`SM_COMMON`) |
//! | [`is_kind_of`], [`CurveBoundedSurface::is_kind_of`]   | `IsKindOf`                                     |
//! | [`CurveBoundedSurface::read_from_db`]                 | `ReadFromDB`                                   |
//! | [`CurveBoundedSurface::write_to_db`]                  | `WriteToDB`                                    |
//!
//! Deviations:
//! - **Nesting.** `new` returns `InvalidInput` when the basis or a boundary curve would nest
//!   deeper than [`MAX_GEOMETRY_NESTING`](crate::surface::surface::p2::MAX_GEOMETRY_NESTING)
//!   (32; upstream fixtures nest at most 3), which bounds the recursive dispatch,
//!   cloning and dropping of nested geometry (no limit upstream).
//! - Compound evaluation copies carry a depth counter checked by `Surface` against the same bound.
//! - **Ownership.** The surface owns its basis surface and boundary curves, as
//!   upstream (whose destructor deletes them): they are moved into
//!   [`CurveBoundedSurface::new`] (a missing upstream array is an empty `Vec`), and the
//!   boundary arrays hold curves, never null pointers. The accessors
//!   ([`CurveBoundedSurface::basis`], ...) are not upstream (the members are private).
//! - **Object model.** The surface is the [`SurfaceKind::CurveBounded`] kind of
//!   [`Surface`]. Its own overrides pass the call along to the basis surface; so do the
//!   geometry queries upstream leaves to the `SmSurface` defaults (next item). The
//!   other `SmSurface` virtuals it does not override, `Copy` among them, run the
//!   base-class defaults there.
//! - **Geometry queries answer the basis's** (not upstream, whose class overrides only
//!   the evaluators, the natural domain, `GetRootSurface`, `operator==`, `WriteToDB` and
//!   `AssertValid`). The surface is its basis over the same parameters: every evaluator
//!   passes `uv` along unchanged, and the edits that could move a parameterisation,
//!   `Reverse`, `Reparameterize`, `SwapUV`, `Transform`, `TrimWithDomain` and `SplitAt`,
//!   are refused for this kind. So every query whose answer is a function of the basis
//!   geometry over its parameters answers the basis's: knots, degrees, control-point
//!   counts, the analytic and STEP domains and conversions, STEP inversion, iso curves,
//!   point drop, plane and surface intersection, the bilinear, planar, closed, parallel,
//!   seam, singularity and degenerate point/curve tests, the surface's bounding boxes
//!   (upstream's `CalculateBoundingBox` is the box of the whole surface's cache tree,
//!   not of the bounded region), the cache-tree inputs ([`Surface::geometry`]) and the
//!   out-of-bounds evaluation switch. The basis takes the surface's owner, owning face
//!   and zone tolerance, as an offset surface's base does. Upstream answers `SM_ERR` or
//!   a non-B-spline default there, so every continuity query, `LiftCurve` and surface
//!   cache tree on the surface failed. The boundary curves bound nothing in these
//!   answers, as upstream's evaluators ignore them too.
//! - **Unported callees.** The basis surface's `EvaluateNormalAtSingularity`
//!   (`surface.surface_p5`) is not in the [`Surface`] dispatch yet: that call returns
//!   [`SolidError::Unported`]. The other basis-surface calls (`EvaluateSimple`,
//!   `EvaluateSTEP`, `EvaluateSTEPPoint`, `operator==`, `EvaluateGeometric`,
//!   `EvaluateNormalSection`) are wired.
//! - **`operator==`** keeps upstream's loop bound: the UV boundaries are compared over
//!   the number of 3D boundaries. Reading past the UV arrays (undefined upstream) ends
//!   the comparison with the elements compared so far.
//! - **`SetContext`** has nothing to set: the kernel is passed to the calls that need
//!   it.
//! - **`ReadFromDB`** (`WR-43`): each UV boundary is read with its own `lUVType`
//!   header and `bImplicitOuter` is forwarded to [`CurveBoundedSurface::new`]. Upstream
//!   reads the UV boundaries with the type of the last 3D boundary (`0`, not a curve
//!   type, when there is none) and drops the implicit-outer flag read from the stream
//!   (the surface gets `FALSE`). A failed ASCII extraction of a count or
//!   the flag stores `0` (C++11 `operator>>`). Not upstream: the read is bounded by
//!   [`DatabaseReader::enter_nested`] (a basis surface can be another curve bounded
//!   surface) and each boundary count by the bytes left in the stream
//!   ([`DatabaseReader::ensure_available`]); [`CurveBoundedSurface::write_to_db`] bounds
//!   nested basis and boundary writes by
//!   [`MAX_GEOMETRY_NESTING`](crate::surface::surface::p2::MAX_GEOMETRY_NESTING).

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::surface::p2::{
    self as surface_p2, check_geometry_nesting, GeometricProperties, NestedGeometry, Surface,
    SurfaceKind,
};
use crate::topology::database_io::{DatabaseReader, DatabaseWriter, FileType, InputStream};
use crate::topology::geometry_write::{write_curve_at, write_surface_at};

const OP_NEW: &str = "surface::curve_bounded_surface::CurveBoundedSurface::new";
const OP_READ: &str = "surface::curve_bounded_surface::CurveBoundedSurface::read_from_db";
const OP_WRITE: &str = "surface::curve_bounded_surface::CurveBoundedSurface::write_to_db";

/// `SmCurveBoundedSurface_TYPE`.
pub const TYPE: i64 = surface_p2::CURVE_BOUNDED_SURFACE_TYPE;

/// The owner a curve-bounded surface gives its basis (not upstream): a surface of this
/// type, through which the basis reaches the face the curve-bounded surface belongs to.
pub(crate) const BASIS_OWNER: crate::geom::object::ObjectRef = crate::geom::object::ObjectRef {
    type_code: TYPE,
    key: crate::geom::attribute::ObjectKey(0),
};
/// `GetTypeString()` of `SmCurveBoundedSurface`.
pub const TYPE_STRING: &str = "SmCurveBoundedSurface_TYPE";
/// `GetClassString()` of `SmCurveBoundedSurface`.
pub const CLASS_STRING: &str = "SmCurveBoundedSurface";

/// `SmCurveBoundedSurface::IsKindOf`: `SmCurveBoundedSurface_TYPE` or a base type.
#[must_use]
pub const fn is_kind_of(t: i64) -> bool {
    TYPE == t || surface_p2::is_kind_of(t)
}

/// The outputs of [`CurveBoundedSurface::evaluate_normal_section`]
/// (`EvaluateNormalSection`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalSection {
    /// `rTangentPlaneDirection`: the unit direction projected into the tangent plane.
    pub tangent_plane_direction: Vec3,
    /// `rdCurvatureValue`: the curvature of the surface curve in that direction.
    pub curvature: f64,
    /// `rSurfaceNormalVector`: the unit surface normal.
    pub normal: Vec3,
}

/// A trimmed surface (`SmCurveBoundedSurface`): a basis surface bounded by 3D curves
/// and their UV counterparts.
#[derive(Debug, Clone)]
pub struct CurveBoundedSurface {
    /// `m_pBasisSurface`.
    basis: Box<Surface>,
    /// `m_v3DBoundaries`.
    boundaries_3d: Vec<Curve>,
    /// `m_vUVBoundaries`.
    boundaries_uv: Vec<Curve>,
    /// `m_bImplicitOuter`.
    implicit_outer: bool,
}

impl From<CurveBoundedSurface> for Surface {
    fn from(s: CurveBoundedSurface) -> Self {
        Self::new(SurfaceKind::CurveBounded(s))
    }
}

impl CurveBoundedSurface {
    /// Exact evaluation copy; the caller validates the complete geometry graph first.
    pub(crate) fn clone_exact_bounded(&self) -> Self {
        Self {
            basis: Box::new(self.basis.clone_exact_bounded()),
            boundaries_3d: self
                .boundaries_3d
                .iter()
                .map(Curve::clone_exact_bounded)
                .collect(),
            boundaries_uv: self
                .boundaries_uv
                .iter()
                .map(Curve::clone_exact_bounded)
                .collect(),
            implicit_outer: self.implicit_outer,
        }
    }

    /// The basis surface, mutable, for [`Surface::detach_nurb_cells`].
    pub(crate) fn nested_surfaces_mut(&mut self) -> Vec<&mut Surface> {
        vec![self.basis.as_mut()]
    }

    /// The basis surface, mutable, for its owner ([`BASIS_OWNER`]).
    pub(crate) fn basis_mut(&mut self) -> &mut Surface {
        &mut self.basis
    }

    /// An evaluation copy retaining exact basis data, with Surface's `MAX_GEOMETRY_NESTING` bound.
    pub(crate) fn clone_for_evaluation(&self, depth: usize) -> Result<Self> {
        Ok(Self {
            basis: Box::new(self.basis.clone_for_evaluation_depth(depth)?),
            boundaries_3d: self
                .boundaries_3d
                .iter()
                .map(|c| c.clone_for_evaluation_depth(depth))
                .collect::<Result<_>>()?,
            boundaries_uv: self
                .boundaries_uv
                .iter()
                .map(|c| c.clone_for_evaluation_depth(depth))
                .collect::<Result<_>>()?,
            implicit_outer: self.implicit_outer,
        })
    }

    /// A surface owning `basis` and the boundary curves (the constructor; the curves
    /// are assumed valid).
    ///
    /// # Errors
    ///
    /// `InvalidInput` when the basis or a boundary curve nests deeper than
    /// [`MAX_GEOMETRY_NESTING`](crate::surface::surface::p2::MAX_GEOMETRY_NESTING) - 1.
    pub fn new(
        basis: Surface,
        boundaries_3d: Vec<Curve>,
        boundaries_uv: Vec<Curve>,
        implicit_outer: bool,
    ) -> Result<Self> {
        check_geometry_nesting(NestedGeometry::Surface(&basis), 1, OP_NEW)?;
        for c in boundaries_3d.iter().chain(&boundaries_uv) {
            check_geometry_nesting(NestedGeometry::Curve(c), 1, OP_NEW)?;
        }
        Ok(Self {
            basis: Box::new(basis),
            boundaries_3d,
            boundaries_uv,
            implicit_outer,
        })
    }

    /// The basis surface.
    #[must_use]
    pub fn basis(&self) -> &Surface {
        &self.basis
    }

    /// The 3D boundary curves.
    #[must_use]
    pub fn boundaries_3d(&self) -> &[Curve] {
        &self.boundaries_3d
    }

    /// The UV boundary curves.
    #[must_use]
    pub fn boundaries_uv(&self) -> &[Curve] {
        &self.boundaries_uv
    }

    /// Whether the outer boundary is implicit (the basis surface's boundary).
    #[must_use]
    pub const fn implicit_outer(&self) -> bool {
        self.implicit_outer
    }

    // ---- type queries ---------------------------------------------------------------

    /// `SmCurveBoundedSurface_TYPE` (`GetType`).
    #[must_use]
    pub const fn type_code(&self) -> i64 {
        TYPE
    }

    /// `"SmCurveBoundedSurface_TYPE"` (`GetTypeString`).
    #[must_use]
    pub const fn type_string(&self) -> &'static str {
        TYPE_STRING
    }

    /// `"SmCurveBoundedSurface"` (`GetClassString`).
    #[must_use]
    pub const fn class_string(&self) -> &'static str {
        CLASS_STRING
    }

    /// True for `SmCurveBoundedSurface_TYPE` and its base types (`IsKindOf`).
    #[must_use]
    pub const fn is_kind_of(&self, t: i64) -> bool {
        is_kind_of(t)
    }

    /// Sets the context of the surface and its curves (`SetContext`): nothing to set.
    pub const fn set_context(&self) {}

    // ---- equality -------------------------------------------------------------------

    /// `operator==`: the same object, or the base-class test and then equal basis
    /// surfaces, boundary counts, implicit-outer flags and boundary curves.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]: `SmSurface::operator==` (`surface.surface_p1`) for
    /// a different object; those of [`Curve::approx_eq`].
    pub fn approx_eq(&self, k: &Kernel, other: &Surface) -> Result<bool> {
        // low work
        if let SurfaceKind::CurveBounded(o) = other.kind() {
            if core::ptr::eq(self, o) {
                return Ok(true);
            }
        }

        // first check the base
        let mut rtn = surface_base_eq(self, other);

        if rtn {
            // OK to cast
            let SurfaceKind::CurveBounded(o) = other.kind() else {
                return Ok(false);
            };

            // check equivalence of these objects
            rtn = (core::ptr::eq(&raw const *self.basis, &raw const *o.basis)
                || surface_eq(k, &self.basis, &o.basis)?)
                && self.boundaries_3d.len() == o.boundaries_3d.len()
                && self.boundaries_uv.len() == o.boundaries_uv.len()
                && self.implicit_outer == o.implicit_outer;

            // check the array elements
            if rtn {
                for (a, b) in self.boundaries_3d.iter().zip(&o.boundaries_3d) {
                    if !rtn {
                        break;
                    }
                    rtn &= core::ptr::eq(a, b) || a.approx_eq(k, b)?;
                }

                // upstream bound: the number of 3D boundaries
                for ii in 0..self.boundaries_3d.len() {
                    if !rtn {
                        break;
                    }
                    let (Some(a), Some(b)) = (self.boundaries_uv.get(ii), o.boundaries_uv.get(ii))
                    else {
                        break;
                    };
                    rtn &= core::ptr::eq(a, b) || a.approx_eq(k, b)?;
                }
            }
        }

        // all done
        Ok(rtn)
    }

    // ---- evaluation: passed along to the basis surface ------------------------------

    /// The basis surface's natural parameter domain (`GetNaturalUVDomain`).
    #[must_use]
    pub fn natural_uv_domain(&self) -> Extent2d {
        // (kernel-free: a lazy analytic answers its NURB-less base's empty domain)
        self.basis.natural_uv_domain_if_built()
    }

    /// The basis surface's unit normal at `uv` (`EvaluateNormal`).
    ///
    /// # Errors
    ///
    /// Those of the basis surface's [`Surface::evaluate_normal`].
    pub fn evaluate_normal(
        &self,
        k: &Kernel,
        uv: Point2,
        u_side: Side,
        v_side: Side,
    ) -> Result<Vec3> {
        // pass the call along
        self.basis.evaluate_normal(k, uv, u_side, v_side)
    }

    /// The basis surface's point and partial derivatives at `uv` (`Evaluate`).
    ///
    /// # Errors
    ///
    /// Those of the basis surface's [`Surface::evaluate`].
    pub fn evaluate(
        &self,
        k: &Kernel,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
        flags: EvaluateFlags,
    ) -> Result<Vec<Vec<Vec3>>> {
        self.basis
            .evaluate(k, uv, highest_u_deriv, highest_v_deriv, flags)
    }

    /// The basis surface's derivatives at `uv` without zero-tangent adjustments
    /// (`EvaluateSimple`).
    ///
    /// # Errors
    ///
    /// Those of the basis surface's evaluation.
    pub fn evaluate_simple(
        &self,
        k: &Kernel,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
        u_side: Side,
        v_side: Side,
        only_upper_half: bool,
    ) -> Result<Vec<Vec<Vec3>>> {
        self.basis.evaluate_simple(
            k,
            uv,
            highest_u_deriv,
            highest_v_deriv,
            EvaluateFlags {
                u_side,
                v_side,
                only_upper_half,
                non_zero_tangents: false,
            },
        )
    }

    /// The basis surface's normal at a singular `uv` (`EvaluateNormalAtSingularity`;
    /// upstream's default `d3dTol` is [`EFF_ZERO`]).
    ///
    /// # Errors
    ///
    /// Those of [`Surface::evaluate_normal_at_singularity`] of the basis surface.
    pub fn evaluate_normal_at_singularity(
        &self,
        k: &Kernel,
        uv: Point2,
        tol_3d: f64,
    ) -> Result<Vec3> {
        self.basis().evaluate_normal_at_singularity(k, uv, tol_3d)
    }

    /// The default `d3dTol` of [`Self::evaluate_normal_at_singularity`].
    pub const SINGULARITY_TOL: f64 = EFF_ZERO;

    /// The basis surface's curvatures, fundamental forms and principal directions at
    /// `uv` (`EvaluateGeometric`).
    ///
    /// # Errors
    ///
    /// Those of the basis surface's [`Surface::evaluate_geometric`].
    pub fn evaluate_geometric(
        &self,
        k: &Kernel,
        uv: Point2,
        u_side: Side,
        v_side: Side,
    ) -> Result<GeometricProperties> {
        self.basis.evaluate_geometric(k, uv, u_side, v_side)
    }

    /// The basis surface's normal section in `direction` at `uv`
    /// (`EvaluateNormalSection`).
    ///
    /// # Errors
    ///
    /// Those of the basis surface's [`Surface::evaluate_normal_section`].
    pub fn evaluate_normal_section(
        &self,
        k: &Kernel,
        uv: Point2,
        u_side: Side,
        v_side: Side,
        direction: Vec3,
    ) -> Result<NormalSection> {
        let section = self
            .basis
            .evaluate_normal_section(k, uv, u_side, v_side, direction)?;
        Ok(NormalSection {
            tangent_plane_direction: section.tangent,
            curvature: section.curvature,
            normal: section.normal,
        })
    }

    /// The basis surface's point at `uv` (`EvaluatePoint`).
    ///
    /// # Errors
    ///
    /// Those of the basis surface's [`Surface::evaluate_point`].
    pub fn evaluate_point(&self, k: &Kernel, uv: Point2) -> Result<Point3> {
        self.basis.evaluate_point(k, uv)
    }

    /// The basis surface's derivatives at `uv` in its STEP parameterisation
    /// (`EvaluateSTEP`).
    ///
    /// # Errors
    ///
    /// Those of the basis surface's evaluation.
    pub fn evaluate_step(
        &self,
        k: &Kernel,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
        u_side: Side,
        v_side: Side,
        only_upper_half: bool,
    ) -> Result<Vec<Vec<Vec3>>> {
        self.basis.evaluate_step(
            k,
            uv,
            highest_u_deriv,
            highest_v_deriv,
            EvaluateFlags {
                u_side,
                v_side,
                only_upper_half,
                non_zero_tangents: true,
            },
        )
    }

    /// The basis surface's point at `uv` in its STEP parameterisation
    /// (`EvaluateSTEPPoint`).
    ///
    /// # Errors
    ///
    /// Those of the basis surface's evaluation.
    pub fn evaluate_step_point(&self, k: &Kernel, uv: Point2) -> Result<Point3> {
        self.basis.evaluate_step_point(k, uv)
    }

    // ---- database -------------------------------------------------------------------

    /// Reads a curve bounded surface written by [`Self::write_to_db`] (`ReadFromDB`):
    /// the boundary counts and implicit-outer flag, the basis surface (type header and
    /// record), then every 3D and every UV boundary curve (type and dimension header
    /// and record).
    ///
    /// Each UV boundary is dispatched on its own header type and the stream's
    /// `bImplicitOuter` flag is kept (`WR-43`). Upstream dispatches UV boundaries with
    /// the last 3D boundary's type (`0` when there are no 3D boundaries, failing with
    /// `SM_ERR`) and ignores the flag (`false`).
    ///
    /// # Errors
    ///
    /// Those of the stream reads, of [`Surface::read_from_db`] and of
    /// [`Curve::read_from_db`]; [`SolidError::InvalidInput`] for an unreadable type
    /// header (undefined upstream), nesting too deep or a boundary count larger than
    /// the input.
    pub fn read_from_db(k: &Kernel, db: &mut DatabaseReader, version: u64) -> Result<Self> {
        db.enter_nested(OP_READ)?;
        let result = read_body(k, db, version);
        db.leave_nested();
        result
    }

    /// Writes the surface in `SmCurveBoundedSurface::WriteToDB` format (`WriteToDB`):
    /// the 3D and UV boundary counts and implicit-outer flag, the basis surface (type
    /// header and record), then every 3D and every UV boundary curve (type and dimension
    /// header and record).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when nested deeper than
    /// [`MAX_GEOMETRY_NESTING`](crate::surface::surface::p2::MAX_GEOMETRY_NESTING); those
    /// of [`write_surface_at`] and [`write_curve_at`].
    pub fn write_to_db(&self, db: &mut DatabaseWriter, version: u64) -> Result<()> {
        self.write_to_db_at(db, version, 0)
    }

    /// [`Self::write_to_db`] for a surface nested `depth` levels deep (the call of
    /// [`write_surface_at`]).
    ///
    /// # Errors
    ///
    /// Those of [`Self::write_to_db`].
    pub(crate) fn write_to_db_at(
        &self,
        db: &mut DatabaseWriter,
        version: u64,
        depth: usize,
    ) -> Result<()> {
        if depth > surface_p2::MAX_GEOMETRY_NESTING {
            return Err(SolidError::invalid_input(
                OP_WRITE,
                "geometry nested too deeply",
            ));
        }
        let ascii = db.file_type() == FileType::Ascii;
        let implicit_outer = i32::from(self.implicit_outer);
        if ascii {
            db.out_u64(self.boundaries_3d.len() as u64);
            db.out_str(" SmCurveBoundedSurface Number of 3D Boundaries \n");
            db.out_u64(self.boundaries_uv.len() as u64);
            db.out_str(" SmCurveBoundedSurface Number of UV Boundaries \n");
            db.out_i64(implicit_outer.into());
            db.out_str(" SmCurveBoundedSurface Implicit Outer Flag \n");
        } else {
            db.write_long(self.boundaries_3d.len() as i64);
            db.write_long(self.boundaries_uv.len() as i64);
            db.write_boolean(implicit_outer);
        }

        // basis surface
        if ascii {
            db.out_str(" SmCurveBoundedSurface->BasisSurface \n");
        }
        db.write_type(self.basis.type_code(), None);
        write_surface_at(&self.basis, db, version, depth + 1)?;

        // every 3DBoundary curve
        for c in &self.boundaries_3d {
            db.write_type(c.type_code(), Some(c.dim()));
            write_curve_at(c, db, version, depth + 1)?;
        }

        // every UVBoundary curve
        for c in &self.boundaries_uv {
            db.write_type(c.type_code(), Some(c.dim()));
            write_curve_at(c, db, version, depth + 1)?;
        }

        // all done
        Ok(())
    }
}

/// `SmSurface::operator==(crOther)` on `this` (the base-class test).
fn surface_base_eq(this: &CurveBoundedSurface, other: &Surface) -> bool {
    // (the same object was handled by the caller; the same type)
    this.type_code() == other.type_code()
}

/// `*a == *b` for two surfaces (the virtual `operator==`).
fn surface_eq(k: &Kernel, a: &Surface, b: &Surface) -> Result<bool> {
    a.approx_eq(k, b)
}

/// An ASCII count or flag: `0` when the extraction fails (C++11 `operator>>`).
fn ascii_ulong(db: &mut DatabaseReader) -> u64 {
    let v = db
        .in_stream_mut()
        .and_then(InputStream::extract_ulong)
        .unwrap_or(0);
    db.go_to_next_line();
    v
}

/// A boundary count as a length, bounded by the bytes left (each curve takes at
/// least one).
fn boundary_count(db: &DatabaseReader, count: u64) -> Result<usize> {
    let n = usize::try_from(count)
        .map_err(|_| SolidError::invalid_input(OP_READ, "boundary count too large"))?;
    db.ensure_available(n, 1, OP_READ)?;
    Ok(n)
}

/// A curve type header (`ReadType(type, &dim)`).
fn read_curve_type(db: &mut DatabaseReader) -> Result<(i64, u64)> {
    let header = db.read_type(true)?;
    let ty = header
        .type_code
        .ok_or_else(|| SolidError::invalid_input(OP_READ, "curve type not read"))?;
    let dim = header
        .dim
        .ok_or_else(|| SolidError::invalid_input(OP_READ, "curve dimension not read"))?;
    Ok((ty, dim))
}

/// The body of [`CurveBoundedSurface::read_from_db`].
fn read_body(k: &Kernel, db: &mut DatabaseReader, version: u64) -> Result<CurveBoundedSurface> {
    // file type
    let ascii = db.file_type() == FileType::Ascii;

    // locals
    let (l3d_boundaries_size, luv_boundaries_size, implicit_outer) = if ascii {
        let a = ascii_ulong(db);
        let b = ascii_ulong(db);
        let implicit_outer = db
            .in_stream_mut()
            .and_then(InputStream::extract_int)
            .unwrap_or(0);
        db.go_to_next_line();
        (a, b, implicit_outer != 0)
    } else {
        let a = db.read_ulong()?;
        let b = db.read_ulong()?;
        let implicit_outer = db.read_boolean()?;
        (a, b, implicit_outer != 0)
    };

    // surface locals
    let mut s3d_curves = Vec::new();
    let mut suv_curves = Vec::new();

    // basis surface
    if ascii {
        db.go_to_next_line();
    }
    let basis_type = db
        .read_type(false)?
        .type_code
        .ok_or_else(|| SolidError::invalid_input(OP_READ, "basis surface type not read"))?;
    let basis_surface = Surface::read_from_db(k, basis_type, db, version)?;

    // for every 3d curve
    let n3d = boundary_count(db, l3d_boundaries_size)?;
    for _ in 0..n3d {
        let (l3d_type, l3d_dim) = read_curve_type(db)?;
        s3d_curves.push(Curve::read_from_db(k, l3d_type, db, l3d_dim, version)?);
    }

    // for every UV curve, with its own type (WR-43; upstream reuses the last 3D curve's)
    let nuv = boundary_count(db, luv_boundaries_size)?;
    for _ in 0..nuv {
        let (luv_type, luv_dim) = read_curve_type(db)?;
        suv_curves.push(Curve::read_from_db(k, luv_type, db, luv_dim, version)?);
    }

    // all done (WR-43: the implicit-outer flag read above is kept; upstream drops it)
    CurveBoundedSurface::new(basis_surface, s3d_curves, suv_curves, implicit_outer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::p2::BSPLINE_CURVE_TYPE;
    use crate::curve::curve::CurveKind;
    use crate::curve::line::Line;
    use crate::geom::assert_array::labels::FACE_TYPE;
    use crate::geom::attribute::ObjectKey;
    use crate::geom::extent1d::Extent1d;
    use crate::geom::geom_utility::p2::ContinuityType;
    use crate::geom::object::ObjectRef;
    use crate::geom::vector2d::Vec2;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::b_spline_surface::SurfParamType;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW, NOZ};

    fn patch() -> BSplineSurface {
        BSplineSurface::create_bilinear_surface(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
            Point3::new(2.0, 3.0, 1.0),
        )
        .unwrap()
    }

    fn line_curve(a: Point3, b: Point3) -> Curve {
        Curve::from(BSplineCurve::create_line_segment(3, a, b).unwrap())
    }

    fn cbs() -> CurveBoundedSurface {
        CurveBoundedSurface::new(
            Surface::from(patch()),
            vec![line_curve(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
            )],
            vec![line_curve(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
            )],
            true,
        )
        .unwrap()
    }

    #[test]
    fn types() {
        let s = Surface::from(cbs());
        assert_eq!(s.type_code(), TYPE);
        assert_eq!(s.type_string(), TYPE_STRING);
        assert_eq!(s.class_string(), CLASS_STRING);
        assert!(s.is_kind_of(TYPE).unwrap());
        assert!(s.is_kind_of(surface_p2::TYPE).unwrap());
        assert!(!s.is_kind_of(BSplineSurface::TYPE).unwrap());
        assert!(!is_kind_of(BSPLINE_CURVE_TYPE));
    }

    #[test]
    fn passes_evaluation_to_basis() {
        let k = &Kernel::default();
        let b = patch();
        let c = cbs();
        let s = Surface::from(c.clone());
        assert_eq!(s.natural_uv_domain_if_built(), b.natural_uv_domain());
        let uv = Point2::new(0.25, 0.5);
        assert_eq!(
            s.evaluate_point(k, uv).unwrap(),
            b.evaluate_point(uv).unwrap()
        );
        let f = EvaluateFlags::default();
        assert_eq!(
            s.evaluate(k, uv, 1, 1, f).unwrap(),
            b.evaluate(uv, 1, 1, f).unwrap()
        );
        // the basis's geometry queries (not upstream, whose base-class defaults answer
        // them), and the base-class default for the copy the class does not override
        assert_eq!(s.degree(SurfParamType::U), b.degree(SurfParamType::U));
        assert!(s.copy(&Kernel::default()).is_err());
        assert_eq!(
            c.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap(),
            Surface::from(b.clone())
                .evaluate_normal(k, uv, Side::Left, Side::Left)
                .unwrap()
        );
        assert_eq!(
            c.evaluate_step_point(k, uv).unwrap(),
            b.evaluate_point(uv).unwrap()
        );
    }

    #[test]
    fn equality() {
        let k = &Kernel::default();
        let c = cbs();
        let s = Surface::from(c.clone());
        let SurfaceKind::CurveBounded(inner) = s.kind() else {
            panic!("kind");
        };
        assert!(inner.approx_eq(k, &s).unwrap());
        assert!(c.approx_eq(k, &s).unwrap());
        assert_eq!(c.boundaries_3d().len(), 1);
        assert_eq!(c.boundaries_uv().len(), 1);
        assert!(c.implicit_outer());
        c.set_context();
    }

    fn surface_text() -> String {
        // bilinear patch, version 30 layout
        "1 1\n1 1\n0\n0 0 0\n0 3 0\n2 0 0\n2 3 1\n0\n0\n1\n1\n0\n0\n1\n1\n".to_string()
    }

    fn curve_text(dim: u64) -> String {
        let pts = if dim == 2 {
            "0 0\n1 0\n"
        } else {
            "0 0 0\n2 0 0\n"
        };
        format!(
            "Object Type {BSPLINE_CURVE_TYPE}\nCurve Dim {dim}\n1\n1\n0\n{dim}\n{pts}0\n0\n1\n1\n"
        )
    }

    fn reader(text: &str) -> DatabaseReader {
        let mut db = DatabaseReader::new();
        db.open_for_read(text.as_bytes().to_vec(), FileType::Ascii);
        db
    }

    #[test]
    fn read_ascii() {
        let k = Kernel::default();
        let text = format!(
            "1 n3d\n1 nuv\n1 implicit\n basis\nObject Type {}\n{}{}{}",
            BSplineSurface::TYPE,
            surface_text(),
            curve_text(3),
            curve_text(2)
        );
        let s = CurveBoundedSurface::read_from_db(&k, &mut reader(&text), 30).unwrap();
        assert_eq!(s.boundaries_3d().len(), 1);
        assert_eq!(s.boundaries_uv().len(), 1);
        assert_eq!(s.boundaries_uv()[0].dim(), 2);
        // the stream's implicit-outer line says 1 (WR-43: upstream drops it)
        assert!(s.implicit_outer());
        assert_eq!(s.basis().type_code(), BSplineSurface::TYPE);

        // through the surface dispatch
        let r = Surface::read_from_db(&k, TYPE, &mut reader(&text), 30).unwrap();
        assert_eq!(r.type_code(), TYPE);
    }

    /// WR-43: a UV boundary is read with its own type header, so a record without 3D
    /// boundaries reads (upstream reads it with type 0 and fails with `SM_ERR`).
    #[test]
    fn read_uv_without_3d_uses_its_own_type() {
        let k = Kernel::default();
        let text = format!(
            "0\n1\n0\n\nObject Type {}\n{}{}",
            BSplineSurface::TYPE,
            surface_text(),
            curve_text(2)
        );
        let s = CurveBoundedSurface::read_from_db(&k, &mut reader(&text), 30).unwrap();
        assert!(s.boundaries_3d().is_empty());
        assert_eq!(s.boundaries_uv().len(), 1);
        assert_eq!(s.boundaries_uv()[0].dim(), 2);
        assert!(!s.implicit_outer());
    }

    #[test]
    fn read_rejects_huge_counts_and_deep_nesting() {
        let k = Kernel::default();
        let text = format!(
            "99999999\n0\n0\n\nObject Type {}\n{}",
            BSplineSurface::TYPE,
            surface_text()
        );
        assert!(CurveBoundedSurface::read_from_db(&k, &mut reader(&text), 30).is_err());

        let deep = format!("0\n0\n0\n\nObject Type {TYPE}\n").repeat(200);
        assert!(CurveBoundedSurface::read_from_db(&k, &mut reader(&deep), 30).is_err());
    }

    fn analytic_line_2d(a: Point3, b: Point3) -> Curve {
        Curve::from(Line::create_line_segment(2, a, b, None).unwrap())
    }

    fn analytic_line_3d(a: Point3, b: Point3) -> Curve {
        Curve::from(Line::create_line_segment(3, a, b, None).unwrap())
    }

    fn quadratic_bspline_2d() -> Curve {
        let nurb = NurbCurve::new(
            vec![
                CPoint::new(0.125, 0.25, NOZ, NOW),
                CPoint::new(0.5, 0.75, NOZ, NOW),
                CPoint::new(0.875, 0.25, NOZ, NOW),
            ],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        Curve::from(BSplineCurve::from_nurb(nurb, 2, false))
    }

    fn write_cbs_bytes(cbs: &CurveBoundedSurface, file_type: FileType, version: u64) -> Vec<u8> {
        let mut db = DatabaseWriter::open_for_write(file_type);
        cbs.write_to_db(&mut db, version).unwrap();
        db.into_bytes()
    }

    fn write_polymorphic_surface_bytes(
        cbs: &CurveBoundedSurface,
        file_type: FileType,
        version: u64,
    ) -> Vec<u8> {
        let mut db = DatabaseWriter::open_for_write(file_type);
        db.write_type(cbs.type_code(), None);
        cbs.write_to_db(&mut db, version).unwrap();
        db.into_bytes()
    }

    fn reader_bytes(bytes: &[u8], file_type: FileType) -> DatabaseReader {
        let mut db = DatabaseReader::new();
        db.open_for_read(bytes.to_vec(), file_type);
        db
    }

    fn assert_curves_match_points(k: &Kernel, got: &Curve, want: &Curve) {
        assert_eq!(got.type_code(), want.type_code());
        assert_eq!(got.dim(), want.dim());
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let p_got = got.evaluate_point(k, t).unwrap();
            let p_want = want.evaluate_point(k, t).unwrap();
            assert!(
                (p_got - p_want).length() < 1.0e-12,
                "t={t}: got {p_got:?}, want {p_want:?}"
            );
        }
    }

    #[test]
    fn wr43_uv_only_and_implicit_outer_round_trip_ascii_and_binary() {
        let k_opt = Kernel::default();
        let uv_line = analytic_line_2d(Point3::new(0.25, 0.5, 0.0), Point3::new(0.75, 0.5, 0.0));
        let uv_bspline = quadratic_bspline_2d();

        for (is_line, uv_curve) in [(true, uv_line), (false, uv_bspline)] {
            let orig = CurveBoundedSurface::new(
                Surface::from(patch()),
                Vec::new(),
                vec![uv_curve.clone()],
                true,
            )
            .unwrap();

            for version in [30_u64, 42_u64] {
                for file_type in [FileType::Ascii, FileType::Binary] {
                    let raw = write_cbs_bytes(&orig, file_type, version);
                    let poly = write_polymorphic_surface_bytes(&orig, file_type, version);

                    // WR-43: the UV curve is dispatched on `luv_type` and `implicit_outer
                    // == true` is kept (pinned C++: 0 3D curves leave `l3d_type == 0`, so
                    // UV dispatch fails with `SM_ERR`).
                    let got = CurveBoundedSurface::read_from_db(
                        &k_opt,
                        &mut reader_bytes(&raw, file_type),
                        version,
                    )
                    .unwrap();
                    assert!(got.boundaries_3d().is_empty());
                    assert_eq!(got.boundaries_uv().len(), 1);
                    assert!(got.implicit_outer());
                    assert_eq!(got.basis().type_code(), BSplineSurface::TYPE);
                    assert_eq!(got.boundaries_uv()[0].dim(), 2);
                    if is_line {
                        assert!(matches!(got.boundaries_uv()[0].kind(), CurveKind::Line(_)));
                    } else {
                        assert!(matches!(
                            got.boundaries_uv()[0].kind(),
                            CurveKind::BSpline(_)
                        ));
                    }
                    assert_curves_match_points(&k_opt, &got.boundaries_uv()[0], &uv_curve);

                    // Polymorphic `Surface::read_from_db` forwards `k: &Kernel`.
                    let mut poly_opt_db = reader_bytes(&poly, file_type);
                    let ty_opt = poly_opt_db.read_type(false).unwrap().type_code.unwrap();
                    assert_eq!(ty_opt, TYPE);
                    let got_surface =
                        Surface::read_from_db(&k_opt, ty_opt, &mut poly_opt_db, version).unwrap();
                    let SurfaceKind::CurveBounded(inner) = got_surface.kind() else {
                        panic!("expected CurveBounded");
                    };
                    assert!(inner.boundaries_3d().is_empty());
                    assert_eq!(inner.boundaries_uv().len(), 1);
                    assert!(inner.implicit_outer());
                    assert_curves_match_points(&k_opt, &inner.boundaries_uv()[0], &uv_curve);
                }
            }
        }
    }

    #[test]
    fn wr43_mismatched_3d_and_uv_curve_types_and_implicit_false() {
        let k_opt = Kernel::default();

        // Case 1: 3D BSplineCurve + 2D Line UV boundary.
        let bspline_3d = line_curve(Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0));
        let line_2d = analytic_line_2d(Point3::new(0.25, 0.5, 0.0), Point3::new(0.75, 0.5, 0.0));
        for implicit_outer in [true, false] {
            let orig = CurveBoundedSurface::new(
                Surface::from(patch()),
                vec![bspline_3d.clone()],
                vec![line_2d.clone()],
                implicit_outer,
            )
            .unwrap();
            for file_type in [FileType::Ascii, FileType::Binary] {
                let raw = write_cbs_bytes(&orig, file_type, 30);
                let poly = write_polymorphic_surface_bytes(&orig, file_type, 30);

                // (pinned C++ reads the 2D `Line` record as `BSPLINE_CURVE_TYPE` and fails)
                let got = CurveBoundedSurface::read_from_db(
                    &k_opt,
                    &mut reader_bytes(&raw, file_type),
                    30,
                )
                .unwrap();
                assert_eq!(got.boundaries_3d().len(), 1);
                assert_eq!(got.boundaries_uv().len(), 1);
                assert!(matches!(
                    got.boundaries_3d()[0].kind(),
                    CurveKind::BSpline(_)
                ));
                assert!(matches!(got.boundaries_uv()[0].kind(), CurveKind::Line(_)));
                assert_curves_match_points(&k_opt, &got.boundaries_3d()[0], &bspline_3d);
                assert_curves_match_points(&k_opt, &got.boundaries_uv()[0], &line_2d);
                assert_eq!(got.implicit_outer(), implicit_outer);

                let mut poly_db = reader_bytes(&poly, file_type);
                let ty = poly_db.read_type(false).unwrap().type_code.unwrap();
                let got_poly = Surface::read_from_db(&k_opt, ty, &mut poly_db, 30).unwrap();
                let SurfaceKind::CurveBounded(inner) = got_poly.kind() else {
                    panic!("expected CurveBounded");
                };
                assert!(matches!(
                    inner.boundaries_uv()[0].kind(),
                    CurveKind::Line(_)
                ));
                assert_eq!(inner.implicit_outer(), implicit_outer);
            }
        }

        // Case 2: 3D Line + 2D quadratic BSplineCurve UV boundary.
        let line_3d = analytic_line_3d(Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0));
        let bspline_2d = quadratic_bspline_2d();
        for implicit_outer in [true, false] {
            let orig = CurveBoundedSurface::new(
                Surface::from(patch()),
                vec![line_3d.clone()],
                vec![bspline_2d.clone()],
                implicit_outer,
            )
            .unwrap();
            for file_type in [FileType::Ascii, FileType::Binary] {
                let raw = write_cbs_bytes(&orig, file_type, 30);
                let poly = write_polymorphic_surface_bytes(&orig, file_type, 30);

                // (pinned C++ misreads the 2D `BSplineCurve` record as `Line::TYPE`,
                // leaving its failed base 3D, and drops `implicit_outer`)
                let got = CurveBoundedSurface::read_from_db(
                    &k_opt,
                    &mut reader_bytes(&raw, file_type),
                    30,
                )
                .unwrap();
                assert_eq!(got.boundaries_3d().len(), 1);
                assert_eq!(got.boundaries_uv().len(), 1);
                assert!(matches!(got.boundaries_3d()[0].kind(), CurveKind::Line(_)));
                assert!(matches!(
                    got.boundaries_uv()[0].kind(),
                    CurveKind::BSpline(_)
                ));
                assert_eq!(got.boundaries_uv()[0].type_code(), BSPLINE_CURVE_TYPE);
                assert_eq!(got.boundaries_uv()[0].dim(), 2);
                assert_curves_match_points(&k_opt, &got.boundaries_3d()[0], &line_3d);
                assert_curves_match_points(&k_opt, &got.boundaries_uv()[0], &bspline_2d);
                assert!(
                    (got.boundaries_uv()[0].evaluate_point(&k_opt, 0.5).unwrap()
                        - Point3::new(0.5, 0.5, 0.0))
                    .length()
                        < 1.0e-12
                );
                assert_eq!(got.implicit_outer(), implicit_outer);

                let mut poly_db = reader_bytes(&poly, file_type);
                let ty = poly_db.read_type(false).unwrap().type_code.unwrap();
                let got_poly = Surface::read_from_db(&k_opt, ty, &mut poly_db, 30).unwrap();
                let SurfaceKind::CurveBounded(inner) = got_poly.kind() else {
                    panic!("expected CurveBounded");
                };
                assert!(matches!(
                    inner.boundaries_uv()[0].kind(),
                    CurveKind::BSpline(_)
                ));
                assert_eq!(inner.implicit_outer(), implicit_outer);
            }
        }
    }

    // ---- knots and continuities: the basis surface's ----------------------------------

    /// A B-spline with a crease and a reduced-continuity knot: degree 2 in `u` with the
    /// double knot `0.5` (C0: the net folds there, so the surface has a real crease
    /// along `u = 0.5`), degree 3 in `v` with the double knot `0.4` (C1).
    fn creased() -> BSplineSurface {
        let xs = [0.0, 0.25, 0.5, 0.75, 1.0];
        let tent = [0.0, 0.25, 0.5, 0.25, 0.0];
        let ys = [0.0, 0.15, 0.3, 0.5, 0.75, 1.0];
        let wave = [0.0, 0.1, 0.0, 0.2, 0.0, 0.1];
        let pw: Vec<Vec<CPoint>> = xs
            .iter()
            .zip(tent)
            .map(|(&x, z)| {
                ys.iter()
                    .zip(wave)
                    .map(|(&y, w)| CPoint::from_point(Vec3::new(x, y, z + w)))
                    .collect()
            })
            .collect();
        let u = [0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0];
        let v = [0.0, 0.0, 0.0, 0.0, 0.4, 0.4, 1.0, 1.0, 1.0, 1.0];
        BSplineSurface::from_data(4, 5, 2, 3, u.len() - 1, v.len() - 1, &pw, &u, &v).unwrap()
    }

    fn bounded(basis: Surface) -> Surface {
        Surface::from(CurveBoundedSurface::new(basis, Vec::new(), Vec::new(), true).unwrap())
    }

    /// A curve-bounded surface answers `GetKnots` with its basis's knots, in both
    /// directions, whole or limited to an interval (upstream's class does not override
    /// `GetKnots`, so it answers the base class's `SM_ERR`).
    #[test]
    fn knots_are_the_basis_knots() {
        let k = &Kernel::default();
        let basis = Surface::from(creased());
        let s = bounded(basis.clone());
        let ivl = Extent1d::from_min_max(0.3, 0.6);
        for param in [SurfParamType::U, SurfParamType::V] {
            for ivl in [None, Some(&ivl)] {
                assert_eq!(
                    s.knots(param, ivl).unwrap(),
                    basis.knots(param, ivl).unwrap()
                );
                assert_eq!(
                    s.knots_with_nurb(k, param, ivl).unwrap(),
                    basis.knots_with_nurb(k, param, ivl).unwrap()
                );
            }
        }
        // the creased net's own knots and multiplicities
        assert_eq!(
            s.knots(SurfParamType::U, None).unwrap(),
            (vec![0.0, 0.5, 1.0], vec![3, 2, 3])
        );
        assert_eq!(
            s.knots(SurfParamType::V, None).unwrap(),
            (vec![0.0, 0.4, 1.0], vec![4, 2, 4])
        );

        // a plane basis
        let plane = Surface::from(crate::surface::plane::Plane::from_normal(
            Point3::new(1.0, 2.0, 3.0),
            Vec3::Z,
        ));
        let s = bounded(plane.clone());
        for param in [SurfParamType::U, SurfParamType::V] {
            assert_eq!(
                s.knots(param, None).unwrap(),
                plane.knots(param, None).unwrap()
            );
        }

        // a cylinder basis without its NURB: the basis makes it to answer
        let cylinder = || {
            Surface::from(crate::surface::cone::Cone::new_cylinder_bounded(
                Point3::new(0.0, 0.0, 0.0),
                Vec3::X,
                Vec3::Y,
                5.0,
                0.0,
                360.0,
                12.0,
                false,
                false,
                false,
            ))
        };
        let s = bounded(cylinder());
        let alone = cylinder();
        for param in [SurfParamType::U, SurfParamType::V] {
            let got = s.knots_with_nurb(k, param, None).unwrap();
            assert!(got.0.len() >= 2, "{param:?}: {got:?}");
            assert_eq!(got, alone.knots_with_nurb(k, param, None).unwrap());
        }
    }

    /// The continuities across the knots, the continuous domain around a point next to
    /// the crease and an iso curve of a curve-bounded surface are its basis's.
    #[test]
    fn continuities_and_continuous_domains_are_the_basis_ones() {
        let k = &Kernel::default();
        let basis = Surface::from(creased());
        let s = bounded(basis.clone());
        for param in [SurfParamType::U, SurfParamType::V] {
            let got = s.calculate_continuities(k, param).unwrap();
            assert_eq!(got, basis.calculate_continuities(k, param).unwrap());
            let iso = s
                .create_iso_parametric_curve(k, param, 0.3, 0.0, None, true)
                .unwrap();
            let want = basis
                .create_iso_parametric_curve(k, param, 0.3, 0.0, None, true)
                .unwrap();
            assert_eq!(iso.max_gap_3d, want.max_gap_3d);
            for t in [0.0, 0.2, 0.5, 0.9, 1.0] {
                assert_eq!(
                    iso.curve.evaluate_point(k, t).unwrap(),
                    want.curve.evaluate_point(k, t).unwrap()
                );
            }
        }
        // the crease is C0 in u
        let (_, u_conts) = s.calculate_continuities(k, SurfParamType::U).unwrap();
        assert_eq!(u_conts.len(), 3);
        assert!(
            (u_conts[1] as i32) < (ContinuityType::C1 as i32),
            "{u_conts:?}"
        );

        // at the crease and on either side of it
        for (uv, dir) in [
            (Point2::new(0.3, 0.5), Vec2::new(1.0, 0.0)),
            (Point2::new(0.5, 0.5), Vec2::new(1.0, 0.0)),
            (Point2::new(0.5, 0.5), Vec2::new(-1.0, 0.0)),
            (Point2::new(0.7, 0.2), Vec2::new(0.0, 1.0)),
        ] {
            let got = s
                .domain_with_continuity(k, ContinuityType::C1, uv, dir)
                .unwrap();
            assert_eq!(
                got,
                basis
                    .domain_with_continuity(k, ContinuityType::C1, uv, dir)
                    .unwrap(),
                "{uv:?} {dir:?}"
            );
        }
        // left of the crease the C1 domain stops at it
        let left = s
            .domain_with_continuity(
                k,
                ContinuityType::C1,
                Point2::new(0.3, 0.5),
                Vec2::new(1.0, 0.0),
            )
            .unwrap();
        assert_eq!((left.min().x, left.max().x), (0.0, 0.5));
    }

    /// A UV line across the crease lifts on a curve-bounded surface as on its basis: the
    /// same curve, split at the crease.
    #[test]
    fn a_curve_across_the_crease_lifts_as_on_the_basis() {
        let k = &Kernel::default();
        let basis = Surface::from(creased());
        let s = bounded(basis.clone());
        let dom = basis.natural_uv_domain_with_nurb(k).unwrap();
        let uv = analytic_line_2d(Point3::new(0.2, 0.3, 0.0), Point3::new(0.8, 0.7, 0.0));
        let ivl = uv.natural_interval();
        let got = s.lift_curve(k, &dom, &uv, &ivl, 1e-6, false).unwrap();
        let want = basis.lift_curve(k, &dom, &uv, &ivl, 1e-6, false).unwrap();
        assert_eq!(got.max_approx_dev, want.max_approx_dev);
        assert_eq!(got.curve.natural_interval(), want.curve.natural_interval());
        for i in 0..=10 {
            let t = ivl.evaluate(f64::from(i) / 10.0);
            assert_eq!(
                got.curve.evaluate_point(k, t).unwrap(),
                want.curve.evaluate_point(k, t).unwrap()
            );
        }
        // the lifted curve passes the crease's point
        let mid = got.curve.evaluate_point(k, ivl.evaluate(0.5)).unwrap();
        let on = basis.evaluate_point(k, Point2::new(0.5, 0.5)).unwrap();
        assert!(mid.distance(on) <= 1e-6, "{mid:?} vs {on:?}");
    }

    /// `LiftCurve` without continuity checks on a curve-bounded surface during a Boolean
    /// operation (whose surface cache defers its tree) lifts the curve.
    #[test]
    fn a_lift_without_continuity_checks_during_a_boolean_succeeds() {
        let mut k = Kernel::default();
        k.context.set_doing_boolean(true);
        let basis = Surface::from(creased());
        let s = bounded(basis.clone());
        let dom = basis.natural_uv_domain_with_nurb(&k).unwrap();
        let uv = analytic_line_2d(Point3::new(0.2, 0.3, 0.0), Point3::new(0.8, 0.7, 0.0));
        let ivl = uv.natural_interval();
        // (no continuity checks: one segment across the crease, so the approximation does
        // not meet the tolerance there; it is still a curve on the surface)
        let lifted = s.lift_curve(&k, &dom, &uv, &ivl, 1e-6, true).unwrap();
        assert!(lifted.max_approx_dev < 1e-3, "{}", lifted.max_approx_dev);
        let lifted_ivl = lifted.curve.natural_interval();
        for (t, lt) in [(ivl.min(), lifted_ivl.min()), (ivl.max(), lifted_ivl.max())] {
            let at = uv.evaluate_point(&k, t).unwrap();
            let on = basis.evaluate_point(&k, Point2::new(at.x, at.y)).unwrap();
            let p = lifted.curve.evaluate_point(&k, lt).unwrap();
            assert!(p.distance(on) <= 1e-6, "{t}: {p:?} vs {on:?}");
        }
    }

    // ---- the basis takes the surface's owner, face and zone tolerance -----------------

    /// Every basis down a chain of curve-bounded surfaces, outermost first.
    fn bases(s: &Surface) -> Vec<&Surface> {
        let mut out = Vec::new();
        let mut cur = s;
        while let SurfaceKind::CurveBounded(c) = cur.kind() {
            cur = c.basis();
            out.push(cur);
        }
        out
    }

    /// Asserts each basis answers `face()` and `owner_zone_tol_3d()` as `s` does.
    fn assert_bases_match(s: &Surface, want_face: Option<ObjectRef>, want_tol: Option<f64>) {
        assert_eq!(s.face().unwrap(), want_face);
        assert_eq!(s.owner_zone_tol_3d(), want_tol);
        let bases = bases(s);
        assert!(!bases.is_empty());
        for (i, b) in bases.into_iter().enumerate() {
            assert_eq!(b.face().unwrap(), want_face, "basis {i}");
            assert_eq!(b.owner_zone_tol_3d(), want_tol, "basis {i}");
            if s.owner().is_some() {
                assert_eq!(b.owner(), Some(BASIS_OWNER), "basis {i}");
            }
        }
    }

    /// After `set_owner`, `set_owner_zone_tol_3d`, a clone and an evaluation copy, the
    /// basis of a curve-bounded surface (and each basis of a nested one) reaches the same
    /// face and zone tolerance as the surface.
    #[test]
    fn the_basis_reaches_the_surface_s_face_and_zone_tolerance() {
        let face = ObjectRef {
            type_code: FACE_TYPE,
            key: ObjectKey(7),
        };
        let other = ObjectRef {
            type_code: FACE_TYPE,
            key: ObjectKey(8),
        };
        let single = bounded(Surface::from(creased()));
        let nested = bounded(bounded(bounded(Surface::from(creased()))));
        for mut s in [single, nested] {
            // no owner
            assert_bases_match(&s, None, None);
            // a face owner, then its tolerance
            s.set_owner(Some(face));
            assert_bases_match(&s, Some(face), None);
            s.set_owner_zone_tol_3d(Some(1e-4));
            assert_bases_match(&s, Some(face), Some(1e-4));
            // the same owner again keeps the tolerance
            s.set_owner(Some(face));
            assert_bases_match(&s, Some(face), Some(1e-4));
            // the evaluation copy keeps the owner
            let eval = s.clone_for_evaluation().unwrap();
            assert_bases_match(&eval, Some(face), Some(1e-4));
            // a clone resets it (the copy constructor), the bases with it
            let copy = s.clone();
            assert_bases_match(&copy, None, None);
            // a copy keeping the owner
            let kept = s.clone_keeping_owner();
            assert_bases_match(&kept, Some(face), Some(1e-4));
            // a new owner drops the old face's tolerance
            s.set_owner(Some(other));
            assert_bases_match(&s, Some(other), None);
            s.set_owner_zone_tol_3d(Some(2e-5));
            assert_bases_match(&s, Some(other), Some(2e-5));
            // clearing the tolerance, then the owner
            s.set_owner_zone_tol_3d(None);
            assert_bases_match(&s, Some(other), None);
            s.set_owner_zone_tol_3d(Some(3e-5));
            s.set_owner(None);
            assert_bases_match(&s, None, None);
        }
    }

    // ---- every delegated geometry query answers the basis's --------------------------

    /// One delegated query, answered as text (its `Debug`, errors included) so that the
    /// table can compare answers of different types.
    type Query = (&'static str, fn(&Kernel, &Surface, &Extent2d) -> String);

    fn iso_points(k: &Kernel, s: &Surface, dom: &Extent2d, param: SurfParamType) -> String {
        let at = if param == SurfParamType::U {
            dom.min().x * 0.6 + dom.max().x * 0.4
        } else {
            dom.min().y * 0.6 + dom.max().y * 0.4
        };
        match s.create_iso_parametric_curve(k, param, at, 1e-6, Some(dom), true) {
            Ok(iso) => {
                let ivl = iso.curve.natural_interval();
                let pts: Vec<_> = (0..=4)
                    .map(|i| {
                        iso.curve
                            .evaluate_point(k, ivl.evaluate(f64::from(i) / 4.0))
                    })
                    .collect();
                format!("{:?} {ivl:?} {pts:?}", iso.max_gap_3d)
            }
            Err(e) => format!("{e:?}"),
        }
    }

    fn mid(dom: &Extent2d) -> Point2 {
        Point2::new(
            dom.min().x * 0.6 + dom.max().x * 0.4,
            dom.min().y * 0.3 + dom.max().y * 0.7,
        )
    }

    /// Makes a fresh surface for one row of the table.
    type MakeSurface = fn() -> Surface;

    const QUERIES: &[Query] = &[
        ("degree", |_, s, _| {
            format!(
                "{} {}",
                s.degree(SurfParamType::U),
                s.degree(SurfParamType::V)
            )
        }),
        ("degree_with_nurb", |k, s, _| {
            format!(
                "{:?} {:?}",
                s.degree_with_nurb(k, SurfParamType::U),
                s.degree_with_nurb(k, SurfParamType::V)
            )
        }),
        ("number_control_points", |k, s, _| {
            format!(
                "{} {}",
                s.number_control_points(k, SurfParamType::U),
                s.number_control_points(k, SurfParamType::V)
            )
        }),
        ("knots_with_nurb", |k, s, _| {
            format!(
                "{:?} {:?}",
                s.knots_with_nurb(k, SurfParamType::U, None),
                s.knots_with_nurb(k, SurfParamType::V, None)
            )
        }),
        ("natural_uv_domain_with_nurb", |k, s, _| {
            format!("{:?}", s.natural_uv_domain_with_nurb(k))
        }),
        ("step_uv_domain", |_, s, _| {
            format!("{:?}", s.step_uv_domain())
        }),
        ("max_analytic_domain", |k, s, _| {
            format!("{:?}", s.max_analytic_domain(k))
        }),
        ("convert_uv_from_step_to_nurbs", |k, s, dom| {
            format!("{:?}", s.convert_uv_from_step_to_nurbs(k, mid(dom)))
        }),
        ("is_bilinear", |k, s, _| format!("{}", s.is_bilinear(k))),
        ("is_planar", |k, s, _| format!("{:?}", s.is_planar(k, 1e-6))),
        ("is_parallel_to_vector", |k, s, _| {
            format!("{:?}", s.is_parallel_to_vector(k, Vec3::Z))
        }),
        ("is_on_seam", |k, s, dom| {
            format!(
                "{:?} {:?}",
                s.is_on_seam(k, mid(dom), None),
                s.is_on_seam(k, dom.min(), None)
            )
        }),
        ("is_degenerate_point", |k, s, dom| {
            format!("{:?}", s.is_degenerate_point(k, 1e-6, Some(dom)))
        }),
        ("is_singularity", |k, s, dom| {
            format!(
                "{:?} {:?}",
                s.is_singularity(k, mid(dom), 1e-6, false),
                s.is_singularity(k, dom.min(), 1e-6, false)
            )
        }),
        ("calculate_continuities", |k, s, _| {
            format!(
                "{:?} {:?}",
                s.calculate_continuities(k, SurfParamType::U),
                s.calculate_continuities(k, SurfParamType::V)
            )
        }),
        ("domain_with_continuity", |k, s, dom| {
            format!(
                "{:?}",
                s.domain_with_continuity(k, ContinuityType::C1, mid(dom), Vec2::new(1.0, 0.0))
            )
        }),
        ("create_iso_parametric_curve", |k, s, dom| {
            format!(
                "{} {}",
                iso_points(k, s, dom, SurfParamType::U),
                iso_points(k, s, dom, SurfParamType::V)
            )
        }),
        ("calculate_bounding_box", |k, s, dom| {
            format!(
                "{:?}",
                s.calculate_bounding_box(k, dom, [true, true, true], false)
            )
        }),
        ("calculate_tight_bounding_box", |k, s, dom| {
            format!("{:?}", s.calculate_tight_bounding_box(k, dom))
        }),
        ("certified_bounding_box", |k, s, dom| {
            format!("{:?}", s.certified_bounding_box(k, dom))
        }),
        ("drop_point", |k, s, dom| {
            let p = s
                .evaluate_point(k, mid(dom))
                .map(|p| p + Vec3::new(0.01, -0.02, 0.3));
            match p {
                Ok(p) => {
                    let opts = crate::surface::surface::p5::DropPointOptions::default();
                    match s.drop_point::<crate::surface::surface::p4::NoObject>(k, p, dom, &opts) {
                        Ok(d) => format!("{} {:?}", d.success, d.result_uv),
                        Err(e) => format!("{e:?}"),
                    }
                }
                Err(e) => format!("{e:?}"),
            }
        }),
        ("evaluate_point", |k, s, dom| {
            format!("{:?}", s.evaluate_point(k, mid(dom)))
        }),
        ("out_of_bounds_enabled", |_, s, _| {
            format!(
                "{:?}",
                crate::surface::surface::p3::out_of_bounds_enabled(s)
            )
        }),
    ];

    /// Every geometry query a curve-bounded surface passes to its basis answers what the
    /// basis answers, for a creased B-spline, a plane, a cylinder (whose NURB the query
    /// makes) and a curve-bounded surface nested in two more.
    #[test]
    fn geometry_queries_answer_the_basis_s() {
        let k = &Kernel::default();
        let bases: [(&str, MakeSurface); 4] = [
            ("creased B-spline", || Surface::from(creased())),
            ("plane", || {
                Surface::from(crate::surface::plane::Plane::new(
                    Point3::ZERO,
                    Vec3::new(1.0, 0.0, 1.0),
                    Vec3::new(0.0, 1.0, 1.0),
                    Vec2::new(1.0, 1.0),
                    &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
                ))
            }),
            ("cylinder", || {
                Surface::from(crate::surface::cone::Cone::new_cylinder_bounded(
                    Point3::new(0.0, 0.0, 0.0),
                    Vec3::X,
                    Vec3::Y,
                    5.0,
                    0.0,
                    360.0,
                    12.0,
                    false,
                    false,
                    false,
                ))
            }),
            ("nested", || bounded(bounded(Surface::from(creased())))),
        ];
        for (name, basis) in bases {
            // fresh surfaces for each query: a query may make and keep a NURB
            let dom = basis().natural_uv_domain_with_nurb(k).unwrap();
            for (query, ask) in QUERIES {
                // (the nested row's answers are the innermost basis's)
                let want = ask(k, basis().geometry(), &dom);
                let got = ask(k, &bounded(basis()), &dom);
                assert_eq!(got, want, "{name}: {query}");
            }
        }
    }
}
