//! A one-face sheet on a surface's whole natural box ([`sheet_on`]), the kernel's one
//! arena-notify `CreateFaceFromSurface` wrapper, a face on a box of a surface's parameters
//! ([`face_on`]), a sheet turned over ([`turn_over_shell`], [`turn_over_sheet`]), a planar
//! face rebuilt as the Coons spline of its edges ([`face_spline_sheet`]), and any face made a
//! sheet with a control net ([`editable_sheet`]: an analytic face as its exact NURBS form).
//!
//! Not upstream: the composition an operation that rebuilds a face on a new surface (a sheet
//! extended past a side) makes of `new_brep` and `SmBrep::CreateFaceFromSurface`, kept in the
//! kernel so the modelling layer holds no notification plumbing; and the turn-over of every
//! face of an open shell ([`crate::topology::face::p1::turn_over`]); and a planar face's spline
//! sheet, so the control-net edits of the modelling layer have a net on a flat face; and an
//! analytic face's exact-form sheet, so they have one on a cylinder, a sphere or a torus.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::collections::BTreeSet;

use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::USE_DEFAULT;
use crate::kernel::Kernel;
use crate::surface::surface::Surface;
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use crate::topology::face::p1::turn_over;
use crate::topology::topology_traverser::Marks;
use crate::topology::{Brep, FaceId, ShellId};
use crate::EntityRef;

use crate::curve::curve::CurveKind;
use crate::geom::tol::{context_zone_tol_3d, SmallTopoType};
use crate::math::Point3;
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::primitive::api_curve_ops::make_curves_compatible;
use crate::surface::surface::SurfaceKind;
use crate::topology::edge::create_trimmed_nurbs_curve;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as fp2;
use crate::topology::loopuse;

/// A new body of one face on `surface` over its whole natural box, at `tolerance`, and that
/// face.
///
/// # Errors
///
/// Those of the box query, `new_brep` and `CreateFaceFromSurface`.
pub fn sheet_on(k: &mut Kernel, surface: Surface, tolerance: f64) -> Result<(Brep, FaceId)> {
    let bx = surface.natural_uv_domain_with_nurb(k)?;
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    b.set_tolerance(tolerance);
    let f = face_on(k, &mut b, surface, &bx)?;
    b.refresh_cached_topology();
    Ok((b, f))
}

/// `SmBrep::CreateFaceFromSurface(surface, bx, face)`: a new face of `b` on `surface` over the
/// box `bx` of its parameters, bounded by one loop of the box's sides, notifying through an
/// arena-only store (default records, no attributes, no context), the new face owning its
/// surface under its arena identity. The kernel's one such wrapper; the caller refreshes `b`'s
/// cached topology when it reads it next.
///
/// # Errors
///
/// Those of `CreateFaceFromSurface`.
pub fn face_on(k: &mut Kernel, b: &mut Brep, surface: Surface, bx: &Extent2d) -> Result<FaceId> {
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    create_face_from_surface(k, b, Some(surface), bx, &mut fenv)
}

const OP_TURN: &str = "topology::brep::sheet::turn_over_shell";

/// Turns the open shell `s` over: each of its faces ([`turn_over`], `SmFace::SwapUV`), so each
/// face's side is the other one, its edges as they were; the body stays valid. Turning a shell
/// over twice gives it back.
///
/// A loop of [`turn_over`], not `SmBrep::SwapUV` or `OrientSheetSurfaces(same = false)`: those
/// ignore a face's failed swap (upstream's `sm_SwapFaceUV` is void), and a turn-over that stops
/// part way must say so and leave the shell as it was to the caller's rollback.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a closed shell (one whose faces' other uses lie in another
/// shell: turned over, the body would be inside out), before anything changes;
/// [`SolidError::InvalidInput`] for a shell of no faces; those of [`turn_over`].
pub fn turn_over_shell(k: &Kernel, b: &mut Brep, s: ShellId) -> Result<()> {
    let faces = open_shell_faces(b, s)?;
    for f in faces {
        turn_over(k, b, f)?;
    }
    b.refresh_cached_topology();
    Ok(())
}

/// Turns every shell of faces of `b` over ([`turn_over_shell`]): a sheet turned over.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a body with a closed shell (a solid), before anything
/// changes; those of [`turn_over`].
pub fn turn_over_sheet(k: &Kernel, b: &mut Brep) -> Result<()> {
    let mut faces = Vec::new();
    let shells: Vec<ShellId> = b.shells().ids().collect();
    for s in shells {
        if b.shell(s)?.is_faceuse_shell() {
            faces.extend(open_shell_faces(b, s)?);
        }
    }
    for f in faces {
        turn_over(k, b, f)?;
    }
    b.refresh_cached_topology();
    Ok(())
}

/// The faces of the open shell `s`, each once, in the order of their uses.
fn open_shell_faces(b: &Brep, s: ShellId) -> Result<Vec<FaceId>> {
    let shell = b.shell(s)?;
    if !shell.is_faceuse_shell() {
        return Err(SolidError::invalid_input(OP_TURN, "a shell of no faces"));
    }
    if shell.is_closed(b, s)? {
        return Err(SolidError::Unsupported {
            operation: OP_TURN,
            reason: "a closed shell: turned over, the body would be inside out",
        });
    }
    let mut seen = BTreeSet::new();
    let mut faces = Vec::new();
    for fu in shell.get_faceuses()? {
        if let Some(f) = b.faceuse(fu)?.face {
            if seen.insert(f) {
                faces.push(f);
            }
        }
    }
    Ok(faces)
}

const OP_SPLINE: &str = "topology::brep::sheet::face_spline_sheet";

fn not_rebuilt(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP_SPLINE,
        reason,
    }
}

/// The curve that stays at `p`: degree 1 on `[0, 1]`, both control points `p` (a Coons side
/// collapsed to a corner).
fn at_point(p: Point3) -> Result<BSplineCurve> {
    let points = [p, p];
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[2, 2],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
}

/// `c` running the other way.
fn reversed(mut c: BSplineCurve) -> Result<BSplineCurve> {
    let interval = c.natural_interval();
    c.reverse_parameterization(&interval)?;
    Ok(c)
}

/// `a` and `b` made compatible, exactly (one degree, parameter range and knot vector).
fn compatible(a: BSplineCurve, b: BSplineCurve) -> Result<[BSplineCurve; 2]> {
    let mut pair = [a, b];
    make_curves_compatible(&mut pair)?;
    Ok(pair)
}

/// The bilinear Coons patch bounded by `sides`, three or four B-spline curves running end to end
/// round one loop in their order (`c0` from `P0` to `P1`, `c1` on to `P2`, `c2` on to `P3` and
/// `c3` back to `P0`; with three, `c2` runs back to `P0` and the fourth side stays at `P0`, a
/// degenerate patch). Its sides in the patch's order are `P0` to `P1` and `P3` to `P2` (in u, at
/// v min and max) and `P0` to `P3` and `P1` to `P2` (in v, at u min and max); opposite sides are
/// made compatible first, exactly (one degree, parameter range and knot vector each pair), so
/// the patch's sides are the curves themselves (`create_coons_patch` with `tol`). Not upstream:
/// the one sequence the modelling layer's boundary patch and [`face_spline_sheet`] share.
///
/// # Errors
///
/// `InvalidInput` for other than three or four curves; those of the reversal, the
/// compatibility and the patch.
pub fn coons_patch_of_loop(sides: Vec<BSplineCurve>, tol: f64) -> Result<BSplineSurface> {
    let mut sides = sides.into_iter();
    let (Some(c0), Some(c1), Some(c2)) = (sides.next(), sides.next(), sides.next()) else {
        return Err(SolidError::invalid_input(
            OP_SPLINE,
            "a Coons patch of other than three or four sides",
        ));
    };
    let c3 = match sides.next() {
        Some(c3) => c3,
        None => at_point(c0.ends()?.start)?,
    };
    if sides.next().is_some() {
        return Err(SolidError::invalid_input(
            OP_SPLINE,
            "a Coons patch of other than three or four sides",
        ));
    }
    let [u0, u1] = compatible(c0, reversed(c2)?)?;
    let [v0, v1] = compatible(reversed(c3)?, c1)?;
    let mut quad = [u0, u1, v0, v1];
    BSplineSurface::create_coons_patch(tol, true, &mut quad, &mut [])
}

/// Face `f` of `b`, when its surface is a plane, rebuilt as a one-face sheet on the bilinear
/// Coons patch of its edges: a spline surface whose whole box is the face. The edges are taken
/// round the face's one loop as its upward use runs it, each as a B-spline copy of its curve
/// over its interval (`CreateTrimmedNURBSCurve`), and bound the patch [`coons_patch_of_loop`]
/// makes (three edges: its fourth side collapsed to the corner where the third meets the
/// first). The patch's sides are the edges themselves,
/// and each of its points is an affine combination of points of the edges, so it lies in their
/// plane; its degrees are the edges' own (a skin of two cubic sections gives (3, 1), its wall).
/// `None` for a face that is not on a plane.
///
/// # Errors
///
/// `Unsupported` for a planar face with holes or with fewer than three or more than four edges,
/// an edge with no B-spline form, or a patch that folds (its normal not the face's at every
/// point of a 5 by 5 grid inside its box: a dart-shaped outline); those of the curves, the
/// compatibility, the patch and the sheet.
pub fn face_spline_sheet(k: &mut Kernel, b: &Brep, f: FaceId) -> Result<Option<Brep>> {
    let face = b.face(f)?;
    let surface = face
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP_SPLINE, "a face without a surface"))?;
    let SurfaceKind::Plane(plane) = surface.kind() else {
        return Ok(None);
    };
    let normal = plane.position().z_axis();
    if fp2::loops(b, f, false, None)?.len() != 1 {
        return Err(not_rebuilt("a planar face with holes"));
    }
    let fu = fp2::upward_faceuse(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP_SPLINE, "a face without its upward use"))?;
    let lu = eu1::faceuse_loopuses(b, fu, false, None)?
        .first()
        .copied()
        .ok_or_else(|| SolidError::invalid_input(OP_SPLINE, "a face without its loop"))?;
    let mut sides = Vec::new();
    for eu in loopuse::edgeuses(b, lu, None, None)? {
        let e = eu1::edge(b, eu)?
            .ok_or_else(|| SolidError::invalid_input(OP_SPLINE, "an edge use without its edge"))?;
        let c = create_trimmed_nurbs_curve(k, b, e)?
            .ok_or_else(|| not_rebuilt("an edge with no B-spline form"))?;
        let CurveKind::BSpline(c) = c.into_kind() else {
            return Err(not_rebuilt("an edge with no B-spline form"));
        };
        // as the loop runs
        sides.push(if eu1::orientation(b, eu)? == OrientType::Same {
            c
        } else {
            reversed(c)?
        });
    }
    if !(3..=4).contains(&sides.len()) {
        return Err(not_rebuilt(
            "a planar face of other than three or four edges",
        ));
    }
    let tol = context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
    let patch = coons_patch_of_loop(sides, tol)?;
    let patch = Surface::from(patch);
    // no fold: the patch faces the face's way inside its box
    let bx = patch.natural_uv_domain_with_nurb(k)?;
    for i in 1..=5 {
        for j in 1..=5 {
            let uv = bx.evaluate(f64::from(i) / 6.0, f64::from(j) / 6.0);
            let n = patch.evaluate_normal(k, uv, Side::Left, Side::Left)?;
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
            if !(n.dot(normal) > 0.0) {
                return Err(not_rebuilt(
                    "a planar face whose Coons patch folds (its outline is not convex enough)",
                ));
            }
        }
    }
    let (sheet, _) = sheet_on(k, patch, b.tolerance())?;
    Ok(Some(sheet))
}

const OP_EDITABLE: &str = "topology::brep::sheet::editable_sheet";

/// Face `f` of `b` as a one-face sheet whose surface has a control net to edit, its whole box
/// the face; `None` for a face on a B-spline surface (it has one already).
/// - A plane: [`face_spline_sheet`], the Coons spline of its edges.
/// - A cone (a cylinder too), a sphere, a torus, a surface of revolution or of extrusion: its
///   surface's exact NURBS form, the B-spline the kernel keeps for it (its circles rational
///   quadratics), on a sheet over its whole box. Only for a face that is its surface's whole box,
///   bounded by the box's sides (`FindIfRectangularTrim`'s natural trim): a face trimmed inside
///   it would lose its trims to a whole-box sheet. A closed surface's two seam sides are one
///   edge used twice and a pole is a point (`CreateFaceFromSurface` shares coincident sides and
///   drops degenerate ones).
///
/// `b` is unchanged.
///
/// # Errors
///
/// `Unsupported` for a surface of another kind (an offset, a blend, a ball patch: no exact
/// NURBS form here) and for a face that is not its surface's whole box; those of
/// [`face_spline_sheet`], the copy, the trim query and the sheet.
pub fn editable_sheet(k: &mut Kernel, b: &Brep, f: FaceId) -> Result<Option<Brep>> {
    let surface = b
        .face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP_EDITABLE, "a face without a surface"))?;
    match surface.kind() {
        SurfaceKind::BSpline(_) => Ok(None),
        SurfaceKind::Plane(_) => face_spline_sheet(k, b, f),
        SurfaceKind::Cone(_)
        | SurfaceKind::Sphere(_)
        | SurfaceKind::Torus(_)
        | SurfaceKind::SurfOfRevolution(_)
        | SurfaceKind::SurfOfExtrusion(_) => exact_form_sheet(k, b, f, surface).map(Some),
        _ => Err(SolidError::Unsupported {
            operation: OP_EDITABLE,
            reason: "a face with no exact NURBS form",
        }),
    }
}

/// The sheet on `surface`'s exact NURBS form over its whole box, when face `f` of `b` is that
/// whole box ([`editable_sheet`]).
fn exact_form_sheet(k: &mut Kernel, b: &Brep, f: FaceId, surface: &Surface) -> Result<Brep> {
    use crate::geom::attribute::ObjectKey;
    use crate::geom::object::ObjectRef;
    use crate::topology::brep::p3::{brep_copy_mapped, with_nurb};
    use crate::topology::face::p1::find_if_rectangular_trim;

    // the trim query builds the face's trim boundary: on a copy, so `b` is only read
    let (mut c, map) = brep_copy_mapped(k, b)?;
    let Some(EntityRef::Face(cf)) = map.get_at(Some(EntityRef::Face(f)))? else {
        return Err(SolidError::invalid_input(
            OP_EDITABLE,
            "a face without its copy",
        ));
    };
    let key_of = |e: EntityRef| ObjectRef {
        type_code: 0,
        key: ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    if !find_if_rectangular_trim(k, &mut c, cf, &key_of)?.natural {
        return Err(SolidError::Unsupported {
            operation: OP_EDITABLE,
            reason: "a face that is not its surface's whole box",
        });
    }
    let form = with_nurb(k, surface)?
        .ok_or_else(|| SolidError::invalid_input(OP_EDITABLE, "a surface with no NURBS form"))?;
    let nurb = form
        .gw_nurb()
        .ok_or_else(|| SolidError::invalid_input(OP_EDITABLE, "a surface with no NURBS form"))?;
    let (sheet, _) = sheet_on(
        k,
        Surface::from(BSplineSurface::from_gw_nurb(nurb)),
        b.tolerance(),
    )?;
    Ok(sheet)
}
