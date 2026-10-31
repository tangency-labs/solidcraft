//! Extrusions, revolutions, sweeps, lofts and planar faces on solidcore: the C API's logic round
//! SM_API's sweeps, ported from the fork's `capi/src/smlib_sweeps.cpp` (`coord_ok`, `unit`,
//! `line`, `arc`, `nurbs`, `parse`, `finish`, `one_loop`, `FlatArc`, `one_circle`, `angle_on`,
//! `one_wall_per_circle`, `straight`, `circular`, `joined`,
//! `put_motion`, `turned`, `Rigid`, `Segment`, `segmented`, `carried`, `piecewise`, `maps_ok`, `placed`, `put_curve`, and the entry points
//! `smlib_planar_faces`, `smlib_extrude`, `smlib_revolve`, `smlib_sweep_placed`, `smlib_loft`,
//! `smlib_loft_to_point`, `smlib_sweep_stations`, `smlib_project_curves`, `smlib_face_section`,
//! and `smlib_screw_sweep` and `smlib_screw_sweep_radial` with `Screw`, `Stations`,
//! `lift_error`, `offset_error`, `path`, `path_curve`, `axial_radius`, `cylinder_piece`,
//! `screw_wall` and `screw_sweep`).
//!
//! As there, each call builds its own curves from the curve list, checks its numbers before the
//! kernel sees them, runs SM_API's sweep and hands the result over only when it is what was asked
//! for: a closed manifold solid when the ends are capped ([`Error::NotSolid`] otherwise), never a
//! body without faces ([`Error::Empty`]).
//!
//! The screw sweeps are the capi's own: it builds their walls from SMLib's B-spline surfaces,
//! cylinders and face builders (`Stations`, `screw_wall`, `cylinder_piece`), with no SM_API
//! sweep behind them, and solidcore leaves them to the capi (solidcore PR #1326, closed as
//! fork-only; #1162 and #1163 triaged not applicable). So they are ported here.

use std::f64::consts::{PI, TAU};

use solidcore::curve::curve::p3::compute_crvs_normal;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::{SmallTopoType, USE_DEFAULT, context_zone_tol_3d};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::heal::stitch::p1::api_stitch_into_solid;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, CurveCanonical, KnotType};
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::surface::cone::Cone;
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep::{
    CornerRefusal, OffsetCornerType, TaperCornerRefusal, TaperExtrude, create_curve_sweep_with_placements, create_linear_sweep,
    create_rotational_sweep, create_taper_extrude_with_corner,
};
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::p1::{ParallelProjectionOptions, create_parallel_projection_curves};
use solidcore::topology::brep::p2::{ArenaNotifyState, is_manifold_solid};
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p4::stitch_faces;
use solidcore::topology::brep::p5::{StitchAndOrientOptions, stitch_and_orient};
use solidcore::topology::brep::p7::copy_faces;
use solidcore::topology::brep::p8::{
    FaceBuildEnv, create_face_from_surface, create_faces_with_curves, create_planar_face_with_3d_curves, create_planar_faces_with_3d_curves,
};
use solidcore::topology::brep::planar_section::{PlanarSectionOptions, SectionOutputs, create_planar_section_curves};
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, FaceId};
use solidcore::{EntityRef, SolidError};

use super::blend::status;
use super::offset::{face_miss, pick_faces};
use super::{Body, run, with_body};
use crate::sweeps::{Affine, Curve as Profile, DraftCorner, Ends, MAX_CURVES, MAX_STATION_CURVES, MAX_STATIONS, SweepFrame};
use crate::{Error, Result, ffi};

/// The capi's statuses as SolidCraft errors: an operation's own result.
type Out<T> = std::result::Result<T, Error>;

/// A coordinate no larger than this (mm, 1000 km), so that SMLib's tolerances stay meaningful.
const MAX_COORD: f64 = 1e9;

/// `SMLIB_FLAT_MAX_LEN`: most reals a curve list may hold.
const FLAT_MAX_LEN: usize = 1 << 27;

/// `SMLIB_SCREW_MAX_TURNS`.
const SCREW_MAX_TURNS: f64 = 200.0;

/// `SMLIB_SCREW_MAX_LAW`.
const SCREW_MAX_LAW: usize = 1024;

/// `coord_ok`.
fn coord_ok(p: &[f64]) -> bool {
    p.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD)
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// `unit`: a unit vector from `v`, or `None` when it has no usable length.
fn unit(v: [f64; 3]) -> Option<Vec3> {
    if !v.iter().all(|x| x.is_finite()) {
        return None;
    }
    let u = v3(v);
    let n = u.length();
    (n > 1e-12 && n.is_finite()).then(|| u * (1.0 / n))
}

/// An SMLib call's own status passed on (`status(...)` in the capi), else the value.
fn called<T>(r: solidcore::Result<T>) -> Out<T> {
    r.map_err(status)
}

/// `line`.
fn line(start: [f64; 3], end: [f64; 3]) -> Out<Curve> {
    if !coord_ok(&start) || !coord_ok(&end) {
        return Err(Error::Argument);
    }
    let (a, b) = (v3(start), v3(end));
    if !((b - a).length() > 1e-9) {
        return Err(Error::Argument);
    }
    called(Line::create_line_segment(3, a, b, None)).map(Curve::from)
}

/// `arc`: centre, unit axis, unit reference direction (square to the axis), radius, start and
/// end angles (radians, counter-clockwise about the axis from the reference).
fn arc(k: &Kernel, center: [f64; 3], axis: [f64; 3], reference: [f64; 3], radius: f64, start: f64, end: f64) -> Out<Curve> {
    if !coord_ok(&center) || ![radius, start, end].iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    let (Some(axis), Some(r)) = (unit(axis), unit(reference)) else { return Err(Error::Argument) };
    if axis.dot(r).abs() > 1e-9 {
        return Err(Error::Argument);
    }
    let sweep = end - start;
    if !(radius > 1e-9 && radius < MAX_COORD) || start.abs() > 1e6 || !(sweep > 1e-9 && sweep <= TAU * (1.0 + 1e-12)) {
        return Err(Error::Argument);
    }
    // The circle's own parameter starts at the arc's start: SMLib takes intervals in degrees
    // within [-360, 360].
    let y = axis.cross(r);
    let x0 = r * start.cos() + y * start.sin();
    let y0 = axis.cross(x0);
    let frame = Axis2Placement::new(v3(center), x0, y0);
    let degrees = Extent1d::from_min_max(0.0, (sweep * 180.0 / PI).min(360.0));
    called(Ellipse::create_canonical_circle(k, &frame, radius, Some(&degrees), None)).map(Curve::from)
}

/// `nurbs`: control points (x, y, z, weight), then points + degree + 1 knots, clamped (end
/// multiplicity degree + 1, inner ones at most degree), non-decreasing.
fn nurbs(degree: usize, points: &[[f64; 4]], knots: &[f64]) -> Out<Curve> {
    let count = points.len();
    if !(1..=ffi::sweeps::CURVE_MAX_DEGREE).contains(&degree) || count < degree + 1 || count > ffi::sweeps::CURVE_MAX_POINTS {
        return Err(Error::Argument);
    }
    let mut ps = Vec::with_capacity(count);
    let mut weights = Vec::with_capacity(count);
    let mut rational = false;
    for p in points {
        if !coord_ok(&p[..3]) || !p[3].is_finite() || !(p[3] > 1e-12 && p[3] < 1e12) {
            return Err(Error::Argument);
        }
        ps.push(Vec3::new(p[0], p[1], p[2]));
        weights.push(p[3]);
        rational = rational || p[3] != 1.0;
    }
    if knots.len() != count + degree + 1 || !knots.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    let (mut unique, mut mult): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for (i, &u) in knots.iter().enumerate() {
        match (i.checked_sub(1).and_then(|j| knots.get(j)), mult.last_mut()) {
            (Some(&prev), _) if u < prev => return Err(Error::Argument),
            (Some(&prev), Some(m)) if u == prev => *m += 1,
            _ => {
                unique.push(u);
                mult.push(1);
            }
        }
    }
    let (Some(&first), Some(&last), Some(&m0), Some(&m1)) = (unique.first(), unique.last(), mult.first(), mult.last()) else {
        return Err(Error::Argument);
    };
    if unique.len() < 2 || m0 != degree + 1 || m1 != degree + 1 || !(last - first > 1e-12) {
        return Err(Error::Argument);
    }
    if mult.iter().skip(1).take(mult.len() - 2).any(|&m| m > degree) {
        return Err(Error::Argument);
    }
    let data = CanonicalData {
        dim: 3,
        degree,
        control_points: &ps,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mult,
        knots: &unique,
        knot_type: KnotType::Unspecified,
        weights: rational.then_some(weights.as_slice()),
        trim: None,
    };
    called(BSplineCurve::create_canonical(&data)).map(Curve::from)
}

/// `parse`: the curves of a curve list. A list of none, more than [`MAX_CURVES`] or more reals
/// than `SMLIB_FLAT_MAX_LEN` is [`Error::Argument`].
fn parse(k: &Kernel, curves: &[Profile]) -> Out<Vec<Curve>> {
    if curves.is_empty() || curves.len() > MAX_CURVES {
        return Err(Error::Argument);
    }
    let mut reals = 0usize;
    let mut out = Vec::with_capacity(curves.len());
    for c in curves {
        let made = match c {
            Profile::Line { start, end } => {
                reals += 6;
                line(*start, *end)?
            }
            Profile::Arc { center, axis, reference, radius, start, end } => {
                reals += 12;
                arc(k, *center, *axis, *reference, *radius, *start, *end)?
            }
            Profile::Nurbs { degree, points, knots } => {
                // `parse` reads the kinds first: at least two points.
                if *degree < 1 || *degree > ffi::sweeps::CURVE_MAX_DEGREE || points.len() < 2 || points.len() > ffi::sweeps::CURVE_MAX_POINTS {
                    return Err(Error::Argument);
                }
                reals = reals.saturating_add(4 * points.len() + points.len() + degree + 1);
                if reals > FLAT_MAX_LEN || knots.len() != points.len() + degree + 1 {
                    return Err(Error::Argument);
                }
                nurbs(*degree, points, knots)?
            }
        };
        if reals > FLAT_MAX_LEN {
            return Err(Error::Argument);
        }
        out.push(made);
    }
    Ok(out)
}

/// `finish`: the result must have faces, and be a closed manifold solid when `solid`.
fn finish(b: Brep, solid: bool) -> solidcore::Result<Out<Brep>> {
    if b.face_list().is_empty() {
        return Ok(Err(Error::Empty));
    }
    if solid && !is_manifold_solid(&b)? {
        return Ok(Err(Error::NotSolid));
    }
    Ok(Ok(b))
}

/// Both ends of `c` (`GetEnds`).
fn ends(k: &Kernel, c: &Curve) -> solidcore::Result<(Point3, Point3)> {
    let e = c.ends(k)?;
    Ok((e.start, e.end))
}

/// `one_loop`: whether the curves, in order, run end to end round one closed loop (within `tol`).
fn one_loop(k: &Kernel, curves: &[Curve], tol: f64) -> solidcore::Result<bool> {
    let (mut first, mut prev) = (None, None);
    for c in curves {
        let (a, b) = ends(k, c)?;
        match prev {
            None => first = Some(a),
            Some(p) if (a - p).length() > tol => return Ok(false),
            Some(_) => {}
        }
        prev = Some(b);
    }
    Ok(matches!((first, prev), (Some(f), Some(p)) if (p - f).length() <= tol))
}

/// `FlatArc`: an arc of a curve list as numbers, its frame, radius and angles.
#[derive(Clone, Copy)]
struct FlatArc {
    centre: Vec3,
    axis: Vec3,
    reference: Vec3,
    radius: f64,
    start: f64,
    sweep: f64,
}

impl FlatArc {
    /// `flat_arc`: the arc's numbers, its axis and reference made unit (`parse` has checked
    /// that they can be); `None` for a curve that is no arc.
    fn of(c: &Profile) -> Option<FlatArc> {
        let Profile::Arc { center, axis, reference, radius, start, end } = c else { return None };
        Some(FlatArc {
            centre: v3(*center),
            axis: unit(*axis).unwrap_or_else(|| v3(*axis)),
            reference: unit(*reference).unwrap_or_else(|| v3(*reference)),
            radius: *radius,
            start: *start,
            sweep: end - start,
        })
    }

    /// The point at angle `t` (radians from the reference, counter-clockwise about the axis).
    fn at(&self, t: f64) -> Point3 {
        let y = self.axis.cross(self.reference);
        self.centre + self.reference * (self.radius * t.cos()) + y * (self.radius * t.sin())
    }

    /// `angle_on`: the angle of `p` on this arc's circle, in its frame (radians, in [0, 2π)).
    fn angle_on(&self, p: Point3) -> f64 {
        let d = p - self.centre;
        let y = self.axis.cross(self.reference);
        let t = d.dot(y).atan2(d.dot(self.reference));
        if t < 0.0 { t + TAU } else { t }
    }
}

/// `one_circle`: whether arcs `a` and `b` lie on one circle to `tol`: centres and radii within
/// it, and the axes alike (the same way round) to within it at the rim.
fn one_circle(a: &FlatArc, b: &FlatArc, tol: f64) -> bool {
    (a.centre - b.centre).length() <= tol
        && (a.radius - b.radius).abs() <= tol
        && a.axis.dot(b.axis) > 0.0
        && a.axis.cross(b.axis).length() * a.radius.max(b.radius) <= tol
}

/// `one_wall_per_circle` (tangency-labs/smlib 7dee48f): the profile `curves` (parsed from
/// `given`) with each run of consecutive arcs on one circle within a loop made one arc, so that
/// the run's wall is one face as a single arc's is: a circle drawn in pieces (split where another
/// sketch curve ends on it) sweeps the one cylinder it lies on, not a face per piece meeting the
/// next along a line that is no edge of the shape. A run that is its whole loop is the full
/// circle, swept as a plain circle is: one periodic face, its seam the one edge it uses twice.
/// The seam goes where a plain circle has it: when the arcs are pieces of one drawn circle (they
/// share its frame, the reference direction their angles are measured from), at that circle's
/// own start, angle 0 of that frame; otherwise at the middle of the run's longest arc (the first
/// of equal ones, in profile order). `tol` is the model's zone tolerance, within which
/// consecutive curves already meet.
fn one_wall_per_circle(k: &Kernel, given: &[Profile], curves: Vec<Curve>, tol: f64) -> solidcore::Result<Out<Vec<Curve>>> {
    let n = curves.len();
    if given.len() != n {
        return Ok(Err(Error::Argument));
    }
    let arcs: Vec<Option<FlatArc>> = given.iter().map(FlatArc::of).collect();
    let (mut head, mut tail) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for c in &curves {
        let (a, b) = ends(k, c)?;
        head.push(a);
        tail.push(b);
    }
    // Whether the end of curve `i` meets the start of curve `j`; whether arc `j` goes on from
    // arc `i`, on its circle.
    let meets = |i: usize, j: usize| matches!((tail.get(i), head.get(j)), (Some(t), Some(h)) if (*t - *h).length() <= tol);
    let arc_of = |i: usize| arcs.get(i).copied().flatten();
    let joins = |i: usize, j: usize| match (arc_of(i), arc_of(j)) {
        (Some(a), Some(b)) => meets(i, j) && one_circle(&a, &b, tol),
        _ => false,
    };
    let mut taken: Vec<Option<Curve>> = curves.into_iter().map(Some).collect();
    let mut take = |i: usize| taken.get_mut(i).and_then(Option::take).ok_or(Error::Argument);
    let mut out = Vec::with_capacity(n);
    let mut first = 0;
    while first < n {
        // The loop from `first`: its curves run end to end until one ends at its start.
        let mut last = first;
        while last + 1 < n && !meets(last, first) && meets(last, last + 1) {
            last += 1;
        }
        let len = last - first + 1;
        let closed = meets(last, first);
        // Whether two of the loop's arcs go on on one circle.
        let mut any = false;
        let mut j = 0;
        while j + 1 < len || (closed && len > 1 && j < len) {
            any = any || joins(first + j, first + (j + 1) % len);
            j += 1;
        }
        if !any {
            for i in first..=last {
                match take(i) {
                    Ok(c) => out.push(c),
                    Err(e) => return Ok(Err(e)),
                }
            }
            first = last + 1;
            continue;
        }
        // A run starts where its curve does not go on from the one before it round the loop.
        let mut begin = 0;
        if closed {
            while begin < len && joins(first + (begin + len - 1) % len, first + begin) {
                begin += 1;
            }
        }
        if begin == len {
            // The whole loop is arcs of one circle: the full circle, its seam at the drawn
            // circle's start when the arcs share its frame, else at the middle of the longest arc.
            let Some(a) = arc_of(first) else { return Ok(Err(Error::Argument)) };
            let mut one_frame = true;
            let mut longest = a;
            for i in first..=last {
                let Some(b) = arc_of(i) else { return Ok(Err(Error::Argument)) };
                one_frame = one_frame && (b.reference - a.reference).length() * a.radius <= tol;
                if b.sweep > longest.sweep {
                    longest = b;
                }
            }
            let seam = if one_frame { 0.0 } else { a.angle_on(longest.at(longest.start + 0.5 * longest.sweep)) };
            let p = |v: Vec3| [v.x, v.y, v.z];
            match arc(k, p(a.centre), p(a.axis), p(a.reference), a.radius, seam, seam + TAU) {
                Ok(c) => out.push(c),
                Err(e) => return Ok(Err(e)),
            }
        } else {
            let mut kk = 0;
            while kk < len {
                let i = first + (begin + kk) % len;
                // The arcs i, i + 1, ... round the loop that go on on its circle.
                let mut m = 1;
                while kk + m < len && joins(first + (begin + kk + m - 1) % len, first + (begin + kk + m) % len) {
                    m += 1;
                }
                if m == 1 {
                    match take(i) {
                        Ok(c) => out.push(c),
                        Err(e) => return Ok(Err(e)),
                    }
                } else {
                    let (Some(a), Some(&h)) = (arc_of(i), head.get(i)) else { return Ok(Err(Error::Argument)) };
                    let sweep: f64 = (0..m).filter_map(|j| arc_of(first + (begin + kk + j) % len)).map(|b| b.sweep).sum();
                    let start = a.angle_on(h);
                    let p = |v: Vec3| [v.x, v.y, v.z];
                    match arc(k, p(a.centre), p(a.axis), p(a.reference), a.radius, start, start + sweep) {
                        Ok(c) => out.push(c),
                        Err(e) => return Ok(Err(e)),
                    }
                }
                kk += m;
            }
        }
        first = last + 1;
    }
    Ok(Ok(out))
}

/// Runs a call that makes a new body: the kernel locked, its own status as the capi passes it.
fn making(call: &'static str, f: impl FnOnce(&mut Kernel) -> solidcore::Result<Out<Brep>>) -> Result<Body> {
    run(call, f)?.map(Body::new)
}

/// `smlib_planar_faces`: a sheet of planar faces bounded by the closed `loops`
/// (`SmApiCreatePlanarFaces`: `CreatePlanarFacesWith3DCurves` into a new B-rep's infinite region).
pub(crate) fn planar_faces(loops: &[Profile]) -> Result<Body> {
    making("smlib_planar_faces", |k| {
        let curves = match parse(k, loops) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        let region = b.infinite_region();
        let made = {
            let mut state = ArenaNotifyState::new(&b);
            let mut env = state.env(None);
            let key_of = env.key_of;
            let this_face = |f: FaceId| key_of(EntityRef::Face(f));
            let mut marks = Marks::default();
            let mut fenv = FaceBuildEnv { env: &mut env, marks: &mut marks, this_face: &this_face };
            // (the tolerance is not used: CreatePlanarFacesWith3DCurves takes the B-rep's)
            create_planar_faces_with_3d_curves(k, &mut b, region, &curves, 0.01, &mut fenv)
        };
        if let Err(e) = made {
            return Ok(Err(status(e)));
        }
        b.refresh_cached_topology();
        finish(b, false)
    })
}

/// `smlib_extrude`: the planar `profile` extruded `distance` along `direction`, straight
/// (`SmApiCreateLinearSweep`) or drafted (`SmApiCreateTaperExtrude`).
pub(crate) fn extrude(profile: &[Profile], direction: [f64; 3], distance: f64, draft: f64, corner: DraftCorner, cap: bool) -> Result<Body> {
    if !distance.is_finite() || !(distance > 1e-9 && distance < MAX_COORD) || !draft.is_finite() || draft.abs() >= PI / 2.0 - 1e-6 {
        return Err(Error::Argument);
    }
    making("smlib_extrude", |k| {
        let Some(dir) = unit(direction) else { return Ok(Err(Error::Argument)) };
        let curves = match parse(k, profile) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        if draft == 0.0 {
            // The fork's capi (7dee48f): arcs of a loop on one circle sweep one wall.
            let tol = context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
            let walls = match one_wall_per_circle(k, profile, curves, tol)? {
                Ok(w) => w,
                Err(e) => return Ok(Err(e)),
            };
            let refs: Vec<Option<&Curve>> = walls.iter().map(Some).collect();
            return match create_linear_sweep(k, &refs, dir, distance, cap) {
                Ok(r) => finish(r, cap),
                Err(e) => Ok(Err(status(e))),
            };
        }
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        // SMLib's draft extrusion takes a single chain of curves, square to the extrusion: the
        // height's sign picks the side, matched to `direction` here.
        if !one_loop(k, &curves, 1e-6)? {
            return Ok(Err(Error::Argument));
        }
        let Ok(normal) = compute_crvs_normal(k, &curves) else { return Ok(Err(Error::Argument)) };
        let along = normal.dot(dir);
        if along.abs() < 1.0 - 1e-9 {
            return Ok(Err(Error::Argument));
        }
        let height = if along > 0.0 { distance } else { -distance };
        let angle = draft * 180.0 / PI;
        // The fork's corner rules (54148db, e8f23e0): the curvature rule continues each wall as
        // itself, the tangent rule a profile of lines and arcs as their tangent planes
        // (`fixes.taper_tangent_corner_planes`), and a corner either cannot close is refused,
        // with why and where (`SmTaperCornerRefusal`, `pOptRefusedCorner`), as the capi passes it on.
        let rule = match corner {
            DraftCorner::Curvature => OffsetCornerType::CurveExtension,
            DraftCorner::Tangent => OffsetCornerType::LinearExtension,
            DraftCorner::Round => OffsetCornerType::FilletCorner,
        };
        match create_taper_extrude_with_corner(k, &refs, height, angle, if cap { 3 } else { 0 }, rule) {
            Ok(TaperExtrude::Solid(r)) => finish(r, cap),
            Ok(TaperExtrude::Refused(r)) => Ok(Err(refused(r))),
            Err(e) => Ok(Err(status(e))),
        }
    })
}

/// A refused draft corner as the capi reports it: the reason's status with the corner's point,
/// [`Error::Corner`] when the point is finite and the status's own error otherwise.
fn refused(r: CornerRefusal) -> Error {
    let (why, code) = match r.reason {
        TaperCornerRefusal::Apart => (crate::sweeps::CornerRefusal::Apart, ffi::sweeps::ERR_CORNER_APART),
        TaperCornerRefusal::WallCloses => (crate::sweeps::CornerRefusal::WallCloses, ffi::sweeps::ERR_WALL_CLOSES),
        TaperCornerRefusal::NotExact => (crate::sweeps::CornerRefusal::NotExact, ffi::trim::ERR_NOT_EXACT),
    };
    let at = [r.corner.x, r.corner.y, r.corner.z];
    if at.iter().all(|x| x.is_finite()) { Error::Corner { why, at } } else { crate::error(code) }
}

/// `smlib_revolve`: the planar `profile` turned `angle` radians about the axis through `origin`
/// along `axis` (`SmApiCreateRotationalSweep`).
pub(crate) fn revolve(profile: &[Profile], origin: [f64; 3], axis: [f64; 3], angle: f64, cap: bool) -> Result<Body> {
    if !coord_ok(&origin) || !angle.is_finite() || !(angle > 1e-9 && angle <= TAU * (1.0 + 1e-12)) {
        return Err(Error::Argument);
    }
    making("smlib_revolve", |k| {
        let Some(dir) = unit(axis) else { return Ok(Err(Error::Argument)) };
        let curves = match parse(k, profile) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        let degrees = (angle * 180.0 / PI).min(360.0);
        match create_rotational_sweep(k, &refs, v3(origin), dir, degrees, cap) {
            Ok(r) => finish(r, cap),
            Err(e) => Ok(Err(status(e))),
        }
    })
}

/// `straight`: whether the path is lines end to end along one direction; then that unit
/// direction and the path's length.
fn straight(k: &Kernel, path: &[Curve]) -> solidcore::Result<Option<(Vec3, f64)>> {
    let (mut dir, mut distance, mut prev) = (None::<Vec3>, 0.0, None::<Point3>);
    for c in path {
        if !matches!(c.kind(), CurveKind::Line(_)) {
            return Ok(None);
        }
        let (a, b) = ends(k, c)?;
        if prev.is_some_and(|p| (a - p).length() > 1e-9) {
            return Ok(None);
        }
        prev = Some(b);
        let d = b - a;
        let l = d.length();
        if !(l > 1e-12) {
            return Ok(None);
        }
        let d = d * (1.0 / l);
        match dir {
            None => dir = Some(d),
            Some(u) if d.dot(u) < 1.0 - 1e-12 => return Ok(None),
            Some(_) => {}
        }
        distance += l;
    }
    Ok(dir.map(|d| (d, distance)))
}

/// `circular`: whether the path is arcs end to end about one axis; then the centre, the unit axis
/// they turn about counter-clockwise and the total angle (radians).
fn circular(k: &Kernel, given: &[Profile], path: &[Curve]) -> solidcore::Result<Option<(Point3, Vec3, f64)>> {
    if given.len() != path.len() || path.is_empty() {
        return Ok(None);
    }
    let (mut first, mut prev, mut angle) = (None::<(Point3, Vec3, f64)>, None::<Point3>, 0.0);
    for (g, c) in given.iter().zip(path) {
        let Profile::Arc { center, axis, radius, start, end, .. } = g else { return Ok(None) };
        let (ctr, kv) = (v3(*center), v3(*axis));
        let kv = kv * (1.0 / kv.length());
        let (a, b) = ends(k, c)?;
        match first {
            None => first = Some((ctr, kv, *radius)),
            Some((c0, k0, r0)) => {
                if (ctr - c0).length() > 1e-9
                    || kv.dot(k0) < 1.0 - 1e-12
                    || (radius - r0).abs() > 1e-9
                    || prev.is_none_or(|p| (a - p).length() > 1e-9)
                {
                    return Ok(None);
                }
            }
        }
        prev = Some(b);
        angle += end - start;
    }
    Ok(first.filter(|_| angle <= TAU * (1.0 + 1e-12)).map(|(c, kv, _)| (c, kv, angle)))
}

/// A curve's B-spline (`SM_CAST_PTR(SmBSplineCurve, c)`: SMLib's lines and circles are
/// B-splines too); `None` for another kind.
fn bspline(c: &Curve) -> solidcore::Result<Option<BSplineCurve>> {
    Ok(match c.kind() {
        CurveKind::BSpline(b) => Some(b.clone()),
        CurveKind::Line(l) => Some(l.base_with_nurb()?.into_owned()),
        CurveKind::Ellipse(e) => Some(e.base().clone()),
        _ => None,
    })
}

/// `joined`: a path of end-to-end curves as one B-spline (`CreateCurveSweep` takes one curve).
fn joined(path: &[Curve]) -> solidcore::Result<Out<BSplineCurve>> {
    let mut parts = Vec::with_capacity(path.len());
    for c in path {
        let Some(b) = bspline(c)? else { return Ok(Err(Error::Argument)) };
        parts.push(b);
    }
    if parts.len() == 1 {
        return Ok(parts.pop().ok_or(Error::Argument));
    }
    Ok(called(BSplineCurve::create_by_joining(&mut parts, None)))
}

/// `turned`: `v` turned `angle` right-handed about the unit `k` (Rodrigues).
fn turned(v: Vec3, k: Vec3, angle: f64) -> Vec3 {
    v * angle.cos() + k.cross(v) * angle.sin() + k * (k.dot(v) * (1.0 - angle.cos()))
}

/// `put_motion`: a placement as its columns and translation.
fn motion(x: Vec3, y: Vec3, z: Vec3, t: Vec3) -> Affine {
    Affine { m: [[x.x, x.y, x.z], [y.x, y.y, y.z], [z.x, z.y, z.z]], t: [t.x, t.y, t.z] }
}

/// A rigid motion p -> R p + t, R by its columns (`Rigid`).
#[derive(Clone, Copy)]
struct Rigid {
    x: Vec3,
    y: Vec3,
    z: Vec3,
    t: Vec3,
}

impl Rigid {
    const IDENTITY: Rigid =
        Rigid { x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 1.0, 0.0), z: Vec3::new(0.0, 0.0, 1.0), t: Vec3::new(0.0, 0.0, 0.0) };

    /// This motion, then a turn of `angle` right-handed about the unit `k` through `c`.
    fn turned_by(&self, c: Vec3, k: Vec3, angle: f64) -> Rigid {
        Rigid { x: turned(self.x, k, angle), y: turned(self.y, k, angle), z: turned(self.z, k, angle), t: c + turned(self.t - c, k, angle) }
    }

    /// This motion, then a move by `d`.
    fn moved_by(&self, d: Vec3) -> Rigid {
        Rigid { t: self.t + d, ..*self }
    }

    /// As the placement `SmCurve::Transform` takes: the world frame's image.
    fn placement(&self) -> Axis2Placement {
        Axis2Placement::new(self.t, self.x, self.y)
    }

    fn motion(&self) -> Affine {
        motion(self.x, self.y, self.z, self.t)
    }
}

/// One segment of a path of lines and arcs (`Segment`): a line from `a` along the unit `dir` for
/// `length`, or an arc turning `angle` (radians) right-handed about the unit `k` through `c`,
/// `length` along.
#[derive(Clone, Copy)]
struct Segment {
    arc: bool,
    dir: Vec3,
    c: Vec3,
    k: Vec3,
    angle: f64,
    length: f64,
}

impl Segment {
    /// The motion `m` carried `s` of the way along (0 <= s <= length).
    fn carry(&self, m: &Rigid, s: f64) -> Rigid {
        if self.arc { m.turned_by(self.c, self.k, self.angle * s / self.length) } else { m.moved_by(self.dir * s) }
    }
}

/// `segmented`: whether `path` (parsed from `given`) is two or more lines and arcs end to end,
/// each leaving in the direction the one before arrives in, and not closed: then its segments.
fn segmented(k: &Kernel, given: &[Profile], path: &[Curve]) -> solidcore::Result<Option<Vec<Segment>>> {
    if given.len() != path.len() || path.len() < 2 {
        return Ok(None);
    }
    let unit = |v: Vec3| v * (1.0 / v.length());
    let mut out = Vec::with_capacity(path.len());
    // The direction the path arrives in at the end of the segment before, and where it ends.
    let (mut before, mut first, mut prev) = (None::<Vec3>, None::<Point3>, None::<Point3>);
    for (g, c) in given.iter().zip(path) {
        let (a, b) = ends(k, c)?;
        let (seg, leaving, arriving) = match g {
            Profile::Line { .. } => {
                let length = (b - a).length();
                if !(length > 1e-12) {
                    return Ok(None);
                }
                let dir = (b - a) * (1.0 / length);
                (Segment { arc: false, dir, c: Vec3::new(0.0, 0.0, 0.0), k: Vec3::new(0.0, 0.0, 0.0), angle: 0.0, length }, dir, dir)
            }
            Profile::Arc { center, axis, radius, start, end, .. } => {
                let (ctr, kv) = (v3(*center), unit(v3(*axis)));
                let angle = end - start;
                let seg = Segment { arc: true, dir: Vec3::new(0.0, 0.0, 0.0), c: ctr, k: kv, angle, length: radius * angle };
                (seg, unit(kv.cross(a - ctr)), unit(kv.cross(b - ctr)))
            }
            _ => return Ok(None),
        };
        match (first, prev, before) {
            (None, ..) => first = Some(a),
            (Some(_), Some(p), Some(d)) => {
                if (a - p).length() > 1e-9 || leaving.dot(d) < 1.0 - 1e-12 {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        }
        prev = Some(b);
        before = Some(arriving);
        out.push(seg);
    }
    match (first, prev) {
        (Some(f), Some(p)) if (f - p).length() > 1e-6 => Ok(Some(out)),
        _ => Ok(None),
    }
}

/// `carried`: copies of the profile's curves carried by `m`.
fn carried(k: &Kernel, profile: &[Curve], m: &Rigid) -> solidcore::Result<Vec<Curve>> {
    let at = m.placement();
    let mut out = Vec::with_capacity(profile.len());
    for c in profile {
        let mut copy = c.clone();
        copy.transform(k, &at, None).map_err(|_| SolidError::sm_err("SmCurve::Transform"))?;
        out.push(copy);
    }
    Ok(out)
}

/// `piecewise`: the profile swept along a path of lines and arcs, turning with it: each segment
/// swept on its own, exactly (a line by SM_API's linear sweep, an arc by its rotational sweep), the
/// profile carried from each to the next, and the walls sewn along the sections they share; with
/// the motions to the first section, the one half-way along and the last.
fn piecewise(k: &mut Kernel, profile: &[Curve], segs: &[Segment], cap: bool) -> solidcore::Result<Out<(Brep, [Affine; 3])>> {
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    let mut m = Rigid::IDENTITY;
    let mut starts = Vec::with_capacity(segs.len());
    let mut total = 0.0;
    for g in segs {
        starts.push(m);
        total += g.length;
        let section = carried(k, profile, &m)?;
        let refs: Vec<Option<&Curve>> = section.iter().map(Some).collect();
        let piece = if g.arc {
            create_rotational_sweep(k, &refs, g.c, g.k, g.angle * 180.0 / PI, false)
        } else {
            create_linear_sweep(k, &refs, g.dir, g.length, false)
        };
        let mut piece = match piece {
            Ok(p) => p,
            Err(e) => return Ok(Err(status(e))),
        };
        let faces = brep_p1::faces(&piece, None)?;
        let mut state = ArenaNotifyState::new(&piece);
        state.seed(&b);
        let mut env = state.env(None);
        let key_of = env.key_of;
        let this_face = |f: FaceId| key_of(EntityRef::Face(f));
        match copy_faces(k, &mut piece, &faces, &mut b, false, &this_face, &mut env) {
            Ok((_, true)) => {}
            Ok((_, false)) => return Ok(Err(status(SolidError::sm_err("CopyFaces")))),
            Err(e) => return Ok(Err(status(e))),
        }
        m = g.carry(&m, g.length);
    }
    let Some(&start) = starts.first() else { return Ok(Err(Error::Argument)) };
    if cap {
        // The caps: the profile where the path starts and where it ends.
        for at in [start, m] {
            let section = carried(k, profile, &at)?;
            let (region, tol) = (b.infinite_region(), b.tolerance());
            if let Err(e) = build_faces(&mut b, |b, fenv| create_planar_faces_with_3d_curves(k, b, region, &section, tol, fenv)) {
                return Ok(Err(status(e)));
            }
        }
    }
    if let Err(e) = stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()) {
        return Ok(Err(status(e)));
    }
    b.refresh_cached_topology();
    let mut half = 0.5 * total;
    let mut middle = m;
    for (i, (g, from)) in segs.iter().zip(&starts).enumerate() {
        if half <= g.length || i + 1 == segs.len() {
            middle = g.carry(from, half.min(g.length));
            break;
        }
        half -= g.length;
    }
    Ok(Ok((b, [start.motion(), middle.motion(), m.motion()])))
}

/// `smlib_sweep_placed`: `profile` swept along `path`, with the rigid motions that placed it at
/// the start, half way and the end. Along a straight path the sweep is SM_API's linear sweep, and
/// turning with arcs about one axis its rotational sweep: both build the walls exactly, as the
/// sweep piece by piece does along lines and arcs that turn with the path ([`piecewise`]).
pub(crate) fn sweep_placed(profile: &[Profile], path: &[Profile], frame: SweepFrame, cap: bool) -> Result<(Body, [Affine; 3])> {
    let mut placed = [motion(Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 0.0)); 3];
    let body = making("smlib_sweep", |k| {
        let (curves, along) = match (parse(k, profile), parse(k, path)) {
            (Ok(c), Ok(p)) => (c, p),
            (Err(e), _) | (_, Err(e)) => return Ok(Err(e)),
        };
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        if let Some((dir, distance)) = straight(k, &along)? {
            let r = match create_linear_sweep(k, &refs, dir, distance, cap) {
                Ok(r) => r,
                Err(e) => return Ok(Err(status(e))),
            };
            for (j, m) in placed.iter_mut().enumerate() {
                let t = dir * (distance * 0.5 * j as f64);
                m.t = [t.x, t.y, t.z];
            }
            return finish(r, cap);
        }
        if frame == SweepFrame::Follow
            && let Some((centre, axis, angle)) = circular(k, path, &along)?
        {
            let degrees = (angle * 180.0 / PI).min(360.0);
            let r = match create_rotational_sweep(k, &refs, centre, axis, degrees, cap) {
                Ok(r) => r,
                Err(e) => return Ok(Err(status(e))),
            };
            let len = axis.length();
            if !(len > 0.0) {
                return Ok(Err(Error::Argument));
            }
            let kv = axis * (1.0 / len);
            let turn = degrees * PI / 180.0;
            for (j, m) in placed.iter_mut().enumerate() {
                let a = turn * 0.5 * j as f64;
                let t = centre - turned(centre, kv, a);
                let [x, y, z] = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)].map(|e| turned(e, kv, a));
                *m = motion(x, y, z, t);
            }
            return finish(r, cap);
        }
        // Along lines and arcs, turning with the path, the sweep is exact piece by piece.
        if frame == SweepFrame::Follow
            && let Some(segs) = segmented(k, path, &along)?
        {
            return Ok(match piecewise(k, &curves, &segs, cap)? {
                Ok((b, motions)) => {
                    placed = motions;
                    finish(b, cap)?
                }
                Err(e) => Err(e),
            });
        }
        let path_curve = match joined(&along)? {
            Ok(b) => Curve::from(b),
            Err(e) => return Ok(Err(e)),
        };
        // Round a closed path the profile comes back onto itself: the walls close on each other
        // there, where caps would lie on top of each other inside the solid.
        let (Some(head), Some(tail)) = (along.first(), along.last()) else { return Ok(Err(Error::Argument)) };
        let closed = (ends(k, head)?.0 - ends(k, tail)?.1).length() <= 1e-6;
        // `SmApiCreateCurveSweep` with `bProfileAtPathStart` (fork 2dd71a0), so the profile is
        // swept where it lies, and the placements it reports (fork 5d3939e).
        let options = SweepOptions {
            cap_ends: cap && !closed,
            translational_sweep: frame == SweepFrame::Translate,
            profile_at_path_start: true,
            ..SweepOptions::default()
        };
        let r = match create_curve_sweep_with_placements(k, &refs, Some(&path_curve), None, None, Some(&options)) {
            Ok(r) => r,
            Err(e) => return Ok(Err(status(e))),
        };
        for (m, p) in placed.iter_mut().zip(&r.placements) {
            *m = motion(p.x_axis(), p.y_axis(), p.z_axis(), p.origin());
        }
        let mut b = r.sweep.brep;
        if closed {
            let tol = b.tolerance();
            match stitch_faces(k, &mut b, tol, None, false) {
                Ok(s) if s.lamina_edges != 0 => return Ok(Err(Error::NotSolid)),
                Ok(_) => b.refresh_cached_topology(),
                Err(e) => return Ok(Err(status(e))),
            }
        }
        finish(b, cap)
    })?;
    Ok((body, placed))
}

/// `smlib_loft`: a skin through `sections` (`SmApiCreateSkinPrimitive`), `degree` across them.
pub(crate) fn loft(sections: &[Vec<Profile>], degree: u32, cap: bool) -> Result<Body> {
    if sections.len() < 2 || sections.len() > MAX_CURVES || !(1..=3).contains(&degree) {
        return Err(Error::Argument);
    }
    let all: Vec<Profile> = sections.iter().flatten().cloned().collect();
    making("smlib_loft", |k| {
        let curves = match parse(k, &all) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        if sections.iter().any(|s| s.is_empty() || s.len() > MAX_CURVES) {
            return Ok(Err(Error::Argument));
        }
        let counts: Vec<usize> = sections.iter().map(Vec::len).collect();
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        match create_skin_primitive(k, &counts, &refs, degree as usize, cap) {
            Ok(r) => finish(r.brep, cap),
            Err(e) => Ok(Err(status(e))),
        }
    })
}

/// Each face's identity, for the face builders: their arena identity.
pub(super) fn build_faces<T>(b: &mut Brep, f: impl FnOnce(&mut Brep, &mut FaceBuildEnv<'_, '_>) -> T) -> T {
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv { env: &mut env, marks: &mut marks, this_face: &this_face };
    f(b, &mut fenv)
}

/// `smlib_loft_to_point`: a pyramid from the planar `section` (one closed loop of lines, in
/// order) to `apex`: the base and a triangle on each line, each `CreatePlanarFaceWith3DCurves` of
/// new lines, then `StitchAndOrient`.
pub(crate) fn loft_to_point(section: &[Profile], apex: [f64; 3]) -> Result<Body> {
    if !coord_ok(&apex) {
        return Err(Error::Argument);
    }
    making("smlib_loft_to_point", |k| {
        let curves = match parse(k, section) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        if curves.len() < 3 || !one_loop(k, &curves, 1e-6)? {
            return Ok(Err(Error::Argument));
        }
        // The base's corners, each line's start.
        let mut corners = Vec::with_capacity(curves.len());
        for c in &curves {
            if !matches!(c.kind(), CurveKind::Line(_)) {
                return Ok(Err(Error::Argument));
            }
            corners.push(ends(k, c)?.0);
        }
        let Ok(normal) = compute_crvs_normal(k, &curves) else { return Ok(Err(Error::Argument)) };
        let top = v3(apex);
        let Some(&c0) = corners.first() else { return Ok(Err(Error::Argument)) };
        let size = corners.iter().map(|q| (*q - c0).length()).fold(0.0, f64::max);
        if !((top - c0).dot(normal).abs() > 1e-9 * size.max(1.0)) {
            return Ok(Err(Error::Argument));
        }
        let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        let region = b.infinite_region();
        let tol = 1e-6;
        let made = build_faces(&mut b, |b, fenv| -> Out<()> {
            let mut face = |b: &mut Brep, fenv: &mut FaceBuildEnv<'_, '_>, pts: &[Point3]| -> Out<()> {
                let mut edges = Vec::with_capacity(pts.len());
                for (i, &p) in pts.iter().enumerate() {
                    let q = pts.get((i + 1) % pts.len()).copied().unwrap_or(p);
                    edges.push(Curve::from(called(Line::create_line_segment(3, p, q, None))?));
                }
                called(create_planar_face_with_3d_curves(k, b, region, &edges, tol, fenv)).map(|_| ())
            };
            face(b, fenv, &corners)?;
            for (i, &p) in corners.iter().enumerate() {
                let q = corners.get((i + 1) % corners.len()).copied().unwrap_or(p);
                face(b, fenv, &[p, q, top])?;
            }
            Ok(())
        });
        if let Err(e) = made {
            return Ok(Err(e));
        }
        if let Err(e) = stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()) {
            return Ok(Err(status(e)));
        }
        b.refresh_cached_topology();
        finish(b, true)
    })
}

/// `maps_ok`: each station map finite, of bounded size and far from singular, its translation
/// within `MAX_COORD`.
fn maps_ok(stations: &[Affine]) -> bool {
    stations.iter().all(|s| {
        let m = &s.m;
        if !m.iter().flatten().chain(&s.t).all(|x| x.is_finite()) || !coord_ok(&s.t) {
            return false;
        }
        let len = |c: &[f64; 3]| (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        let (c0, c1, c2) = (len(&m[0]), len(&m[1]), len(&m[2]));
        if !(c0 < 1e6 && c1 < 1e6 && c2 < 1e6) {
            return false;
        }
        let det = m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2]) - m[1][0] * (m[0][1] * m[2][2] - m[2][1] * m[0][2])
            + m[2][0] * (m[0][1] * m[1][2] - m[1][1] * m[0][2]);
        det.abs() > 1e-9 * c0 * c1 * c2
    })
}

/// `placed`: a copy of `c` as a plain B-spline with every control point carried by `s`, the
/// weights unchanged (an affine map moves a NURBS curve exactly by its control points).
fn placed(c: &BSplineCurve, s: &Affine) -> Out<Curve> {
    let can = called(c.canonical())?;
    if can.dim != 3 {
        return Err(Error::Status(1001));
    }
    let m = &s.m;
    let points: Vec<Point3> = can
        .control_points
        .iter()
        .map(|p| {
            Vec3::new(
                m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + s.t[0],
                m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + s.t[1],
                m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + s.t[2],
            )
        })
        .collect();
    let data = CanonicalData {
        dim: 3,
        degree: can.degree,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &can.knot_multiplicities,
        knots: &can.knots,
        knot_type: KnotType::Unspecified,
        weights: (!can.weights.is_empty()).then_some(can.weights.as_slice()),
        trim: None,
    };
    called(BSplineCurve::create_canonical(&data)).map(Curve::from)
}

/// `smlib_sweep_stations`: the profile placed by each station, its sections skinned through
/// (`SmApiCreateSkinPrimitive`, cubic across them or as high as their number allows), the ends
/// capped, or joined by `SmApiStitchIntoSolid`.
pub(crate) fn sweep_stations(profile: &[Profile], stations: &[Affine], end: Ends) -> Result<Body> {
    if !(2..=MAX_STATIONS).contains(&stations.len()) {
        return Err(Error::Argument);
    }
    making("smlib_sweep_stations", |k| {
        if !maps_ok(stations) {
            return Ok(Err(Error::Argument));
        }
        let curves = match parse(k, profile) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        if curves.len() as f64 * stations.len() as f64 > MAX_STATION_CURVES as f64 {
            return Ok(Err(Error::Argument));
        }
        // Every section is the profile carried by its station's map: the sections share the
        // profile's degrees and knots, which the skin interpolates control point by control point.
        let mut sections = Vec::with_capacity(curves.len() * stations.len());
        let mut counts = Vec::with_capacity(stations.len());
        for s in stations {
            for c in &curves {
                let Some(b) = bspline(c)? else { return Ok(Err(Error::Argument)) };
                match placed(&b, s) {
                    Ok(p) => sections.push(p),
                    Err(e) => return Ok(Err(e)),
                }
            }
            counts.push(curves.len());
        }
        let degree = (stations.len() - 1).min(3);
        let refs: Vec<Option<&Curve>> = sections.iter().map(Some).collect();
        let mut r = match create_skin_primitive(k, &counts, &refs, degree, end == Ends::Capped) {
            Ok(r) => r.brep,
            Err(e) => return Ok(Err(status(e))),
        };
        if end == Ends::Joined {
            // The last section lies on the first: the walls' ends are glued to their starts.
            match api_stitch_into_solid(k, Some(&mut r)) {
                Ok(s) if s.produces_a_solid => {}
                Ok(_) => return Ok(Err(Error::NotSolid)),
                Err(e) => return Ok(Err(status(e))),
            }
        }
        finish(r, end != Ends::Open)
    })
}

/// The entry checks of `smlib_screw_sweep` and `smlib_screw_sweep_radial`.
fn screw_ok(origin: [f64; 3], pitch: f64, turns: f64, tolerance: f64) -> bool {
    coord_ok(&origin)
        && pitch.is_finite()
        && pitch.abs() > 1e-6
        && pitch.abs() < MAX_COORD
        && turns.is_finite()
        && (1e-3..=SCREW_MAX_TURNS).contains(&turns)
        && tolerance.is_finite()
        && (1e-6..1e3).contains(&tolerance)
}

/// `SMLIB_SCREW_MAX_STATIONS`: most spans of one screw sweep.
const SCREW_MAX_STATIONS: f64 = 12000.0;

/// `Screw`: the screw motion about the axis through `o` along the unit `k`, rising `pitch` per
/// turn, the section carried by angle a (radians, right-handed about k) and first moved off the
/// axis by offset(a) along `e` (the radial sweep: the unit direction from the axis into the
/// profile's half-plane), offset(a) piecewise linear through (`angles`, `offsets`) and held
/// beyond them. No offsets: none.
struct Screw {
    o: Point3,
    k: Vec3,
    e: Vec3,
    pitch: f64,
    angles: Vec<f64>,
    offsets: Vec<f64>,
}

impl Screw {
    fn turn(&self, v: Vec3, a: f64) -> Vec3 {
        turned(v, self.k, a)
    }

    fn lift(&self, a: f64) -> f64 {
        self.pitch * a / TAU
    }

    fn offset(&self, a: f64) -> f64 {
        let (Some(&a0), Some(&o0), Some(&on)) = (self.angles.first(), self.offsets.first(), self.offsets.last()) else { return 0.0 };
        if !(a > a0) {
            return o0;
        }
        for i in 1..self.angles.len() {
            let (Some(&ai), Some(&ap), Some(&oi), Some(&op)) =
                (self.angles.get(i), self.angles.get(i - 1), self.offsets.get(i), self.offsets.get(i - 1))
            else {
                break;
            };
            if a <= ai {
                return op + (oi - op) * (a - ap) / (ai - ap);
            }
        }
        on
    }

    /// The move off the axis at angle a, before the turn.
    fn shift(&self, a: f64) -> Vec3 {
        if self.offsets.is_empty() { Vec3::new(0.0, 0.0, 0.0) } else { self.e * self.offset(a) }
    }

    /// Where p goes at angle a.
    fn at(&self, p: Point3, a: f64) -> Point3 {
        self.o + self.turn(p - self.o + self.shift(a), a) + self.k * self.lift(a)
    }

    /// How far p lies from the axis.
    fn reach(&self, p: Point3) -> f64 {
        let w = p - self.o;
        (w - self.k * w.dot(self.k)).length()
    }
}

/// `Stations`: n spans of h radians each, from t[0] to t[n]. Each span carries the profile round
/// as an exact rational quadratic arc carries a point (its middle control point at the span's
/// middle angle, out by 1 / cos(h / 2), weighted cos(h / 2)), and lifts it by the same rational
/// quadratic of the lifts at the span's ends and middle.
struct Stations {
    t: Vec<f64>,
    /// h / 2.
    half: f64,
}

impl Stations {
    fn new(from: f64, to: f64, n: usize) -> Stations {
        let t = (0..=n).map(|i| if i == n { to } else { from + (to - from) * i as f64 / n as f64 }).collect();
        Stations { t, half: 0.5 * (to - from) / n as f64 }
    }

    fn spans(&self) -> usize {
        self.t.len() - 1
    }

    fn first(&self) -> f64 {
        self.t.first().copied().unwrap_or(0.0)
    }

    fn last(&self) -> f64 {
        self.t.last().copied().unwrap_or(0.0)
    }

    /// The knots across the stations: double inside (the arcs meet tangent), triple at the ends.
    fn knots(&self) -> (Vec<f64>, Vec<usize>) {
        let n = self.t.len();
        (self.t.clone(), (0..n).map(|i| if i == 0 || i + 1 == n { 3 } else { 2 }).collect())
    }

    /// The weight of control point j (of 2 spans + 1) across the stations.
    fn weight(&self, j: usize) -> f64 {
        if j.is_multiple_of(2) { 1.0 } else { self.half.cos() }
    }
}

/// `lift_error`: how far, in radians of turn, the rational quadratic lift of a span of
/// half-angle `half` leads or lags the true lift, at most (about 0.008 h³).
fn lift_error(half: f64) -> f64 {
    let mut worst: f64 = 0.0;
    for i in 0..=2048 {
        let s = i as f64 / 2048.0;
        let lift = half * s / (1.0 - (1.0 - s * s) * (1.0 - half.cos()) / 2.0);
        worst = worst.max((lift - 2.0 * ((half / 2.0).tan() * s).atan()).abs());
    }
    worst
}

/// `offset_error`: how far, per unit change of offset across a span of half-angle `half`, the
/// rational quadratic offset strays from the true offset turned with the profile, at most
/// (about h / 8).
fn offset_error(half: f64) -> f64 {
    let (c, h) = (half.cos(), 2.0 * half);
    let mut worst: f64 = 0.0;
    for i in 0..=4096 {
        let s = -1.0 + i as f64 / 2048.0;
        let t = 0.5 * (s + 1.0);
        let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
        let den = b0 + c * b1 + b2;
        let (ax, ay) = (0.5 * (b2 - b0) * c / den, 0.5 * (b2 + b0) * half.sin() / den);
        let phi = 2.0 * ((half / 2.0).tan() * s).atan();
        worst = worst.max((ax - phi / h * phi.cos()).hypot(ay - phi / h * phi.sin()));
    }
    worst
}

/// `path`: the control points of p's path under the screw motion over `st` (2 spans + 1).
fn screw_path(m: &Screw, p: Point3, st: &Stations) -> Vec<Point3> {
    let w = p - m.o;
    let axial = w.dot(m.k);
    let radial = w - m.k * axial;
    let mut d = Vec::with_capacity(2 * st.spans() + 1);
    for (i, &ti) in st.t.iter().enumerate() {
        d.push(m.at(p, ti));
        if let Some(&tn) = st.t.get(i + 1) {
            let mid = 0.5 * (ti + tn);
            d.push(m.o + m.turn(radial + m.shift(mid), mid) * (1.0 / st.half.cos()) + m.k * (axial + m.lift(mid)));
        }
    }
    d
}

/// `path_curve`: the path of p over `st` as a curve, exactly as the walls either side have it.
fn path_curve(m: &Screw, p: Point3, st: &Stations) -> Out<Curve> {
    let points = screw_path(m, p, st);
    let weights: Vec<f64> = (0..points.len()).map(|j| st.weight(j)).collect();
    let (knots, mult) = st.knots();
    let data = CanonicalData {
        dim: 3,
        degree: 2,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mult,
        knots: &knots,
        knot_type: KnotType::Unspecified,
        weights: Some(&weights),
        trim: None,
    };
    called(BSplineCurve::create_canonical(&data)).map(Curve::from)
}

/// `axial_radius`: the radius of the cylinder a profile line parallel to the axis sweeps (within
/// a thousandth of `tolerance`, at least 1e-6 off it); 0 for any other curve.
fn axial_radius(k: &Kernel, m: &Screw, c: &Curve, tolerance: f64) -> solidcore::Result<f64> {
    if !matches!(c.kind(), CurveKind::Line(_)) {
        return Ok(0.0);
    }
    let (a, b) = ends(k, c)?;
    // How far a point of the line strays from the axis lies between the ends' and the nearest
    // point's (the distance is convex along the line).
    let (w, d) = (a - m.o, b - a);
    let (wp, dp) = (w - m.k * w.dot(m.k), d - m.k * d.dot(m.k));
    let t = if dp.dot(dp) > 0.0 { (-wp.dot(dp) / dp.dot(dp)).clamp(0.0, 1.0) } else { 0.0 };
    let (ra, rb, nearest) = (m.reach(a), m.reach(b), (wp + dp * t).length());
    let (far, near) = (ra.max(rb), nearest.min(ra.min(rb)));
    Ok(if near >= 1e-6 && far - near <= 1e-3 * tolerance { 0.5 * (ra + rb) } else { 0.0 })
}

/// `cylinder_piece`: the piece of the cylinder of `radius` the profile line from `a` to `b`
/// sweeps over `st`: an exact cylinder about the axis, bounded by the line placed at both ends of
/// the piece and its ends' paths (`CreateFacesWithCurves`). The cylinder's own turn starts a
/// quarter turn before the piece and its height runs past it.
fn cylinder_piece(k: &mut Kernel, b: &mut Brep, m: &Screw, a: Point3, e: Point3, radius: f64, st: &Stations) -> solidcore::Result<Out<()>> {
    let (a0, a1) = (st.first(), st.last());
    let w = a - m.o;
    let r = w - m.k * w.dot(m.k);
    let r = r * (1.0 / r.length());
    let x = m.turn(r, a0 - 0.25 * TAU);
    let y = m.k.cross(x);
    let (za, zb) = ((a - m.o).dot(m.k), (e - m.o).dot(m.k));
    let z0 = za.min(zb) + m.lift(a0).min(m.lift(a1));
    let z1 = za.max(zb) + m.lift(a0).max(m.lift(a1));
    let margin = 0.25 * (z1 - z0) + 1e-3;
    let base = m.o + m.k * (z0 - margin);
    let cylinder = Cone::new_cylinder_bounded(base, x, y, radius, 0.0, 360.0, z1 - z0 + 2.0 * margin, false, false, false);
    let edge = |at: f64| called(Line::create_line_segment(3, m.at(a, at), m.at(e, at), None)).map(Curve::from);
    let lp = || -> Out<Vec<Curve>> { Ok(vec![edge(a0)?, path_curve(m, e, st)?, edge(a1)?, path_curve(m, a, st)?]) };
    let curves = match lp() {
        Ok(c) => c,
        Err(err) => return Ok(Err(err)),
    };
    let orient = [OrientType::Same, OrientType::Same, OrientType::Opposite, OrientType::Opposite];
    let region = b.infinite_region();
    let made = build_faces(b, |b, fenv| {
        create_faces_with_curves(k, b, region, &[4], Some(curves.into_iter().map(Some).collect()), &orient, Some(Surface::from(cylinder)), fenv)
    });
    Ok(match made {
        Ok(f) if f.faces.len() == 1 => Ok(()),
        Ok(_) => Err(Error::Status(1001)),
        Err(err) => Err(status(err)),
    })
}

/// `screw_wall`: the wall one profile curve sweeps over `st`: across the curve its NURBS, across
/// the turns each control point's path, the weights the curve's times the arcs'.
fn screw_wall(m: &Screw, c: &CurveCanonical, st: &Stations) -> Out<Surface> {
    let across = 2 * st.spans() + 1;
    let mut net = Vec::with_capacity(c.control_points.len() * across);
    let mut weights = Vec::with_capacity(net.capacity());
    for (l, &p) in c.control_points.iter().enumerate() {
        let w = c.weights.get(l).copied().unwrap_or(1.0);
        for (j, q) in screw_path(m, p, st).into_iter().enumerate() {
            net.push(q);
            weights.push(w * st.weight(j));
        }
    }
    let (vknots, vmult) = st.knots();
    let data = CanonicalSurfaceData {
        u_degree: c.degree,
        v_degree: 2,
        control_points: &net,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &c.knot_multiplicities,
        v_multiplicities: &vmult,
        u_knots: &c.knots,
        v_knots: &vknots,
        knot_type: KnotType::Unspecified,
        weights: Some(&weights),
        uv_domain: None,
    };
    called(BSplineSurface::create_canonical(&data)).map(Surface::from)
}

/// The profile curve `c` carried by the screw motion's placement at angle `a` (`Copy`, then
/// `Transform`): a rigid motion, so a line stays a line and an arc an arc.
fn screw_placed(m: &Screw, c: &Profile, a: f64) -> Profile {
    let p = |q: [f64; 3]| {
        let r = m.at(v3(q), a);
        [r.x, r.y, r.z]
    };
    let d = |q: [f64; 3]| {
        let r = m.turn(v3(q), a);
        [r.x, r.y, r.z]
    };
    match c {
        Profile::Line { start, end } => Profile::Line { start: p(*start), end: p(*end) },
        Profile::Arc { center, axis, reference, radius, start, end } => {
            Profile::Arc { center: p(*center), axis: d(*axis), reference: d(*reference), radius: *radius, start: *start, end: *end }
        }
        Profile::Nurbs { degree, points, knots } => Profile::Nurbs {
            degree: *degree,
            points: points
                .iter()
                .map(|q| {
                    let r = p([q[0], q[1], q[2]]);
                    [r[0], r[1], r[2], q[3]]
                })
                .collect(),
            knots: knots.clone(),
        },
    }
}

/// `screw_sweep` (smlib_sweeps.cpp): the profile's walls built from the screw motion itself, span
/// by span (SM_API's curve sweep carries the profile on its own frame, which along a helix turns
/// it away from the screw motion), lines parallel to the axis as exact cylinder pieces, the caps
/// where the motion starts and ends, all stitched and oriented.
fn screw(k: &mut Kernel, profile: &[Profile], mut m: Screw, turns: f64, tolerance: f64) -> solidcore::Result<Out<Brep>> {
    let curves = match parse(k, profile) {
        Ok(c) => c,
        Err(e) => return Ok(Err(e)),
    };
    let mut walls = Vec::with_capacity(curves.len());
    let (mut reach, mut farthest) = (0.0, Vec3::new(0.0, 0.0, 0.0));
    for c in &curves {
        let Some(b) = bspline(c)? else { return Ok(Err(Error::Status(1001))) };
        let can = b.canonical()?;
        if can.dim != 3 {
            return Ok(Err(Error::Status(1001)));
        }
        for &p in &can.control_points {
            let v = p - m.o;
            let v = v - m.k * v.dot(m.k);
            if v.length() > reach {
                reach = v.length();
                farthest = v;
            }
        }
        walls.push(can);
    }
    if !(reach > 1e-6) {
        return Ok(Err(Error::Argument));
    }
    // An offset moves the profile within its half-plane: every control point must lie in the one
    // the farthest of them spans with the axis, and stay off the axis moved.
    if !m.offsets.is_empty() {
        let mut nearest = reach;
        m.e = farthest * (1.0 / reach);
        for p in walls.iter().flat_map(|w| &w.control_points) {
            let v = *p - m.o;
            let v = v - m.k * v.dot(m.k);
            let along = v.dot(m.e);
            if !((v - m.e * along).length() <= 1e-9 * (1.0 + reach) && along >= -1e-9 * (1.0 + reach)) {
                return Ok(Err(Error::Argument));
            }
            nearest = nearest.min(along);
        }
        if m.offsets.iter().any(|d| !(nearest + d > 1e-6)) {
            return Ok(Err(Error::Argument));
        }
    }
    // Walls of lines parallel to the axis are exact cylinders, which cannot turn on themselves:
    // then every wall comes in pieces of half a turn (the last up to an eighth longer).
    let mut radius = Vec::with_capacity(curves.len());
    for c in &curves {
        radius.push(axial_radius(k, &m, c, tolerance)?);
    }
    let cylinders = radius.iter().any(|&r| r > 0.0);
    // The offset changes slope only at its own angles: they bound pieces too, and a half turn
    // within a sixteenth of a turn of one is left out.
    let total = TAU * turns;
    let mut breaks = vec![0.0];
    breaks.extend(m.angles.iter().copied().filter(|&a| a > 0.0 && a < total));
    let mut half = 0.5 * TAU;
    while cylinders && half < total - TAU / 16.0 {
        if !m.angles.iter().any(|a| (a - half).abs() < TAU / 16.0) {
            breaks.push(half);
        }
        half += 0.5 * TAU;
    }
    breaks.push(total);
    breaks.sort_by(f64::total_cmp);
    let rise = m.pitch.abs() / TAU;
    let mut h = (TAU / 8.0).min((tolerance / (0.008 * rise)).cbrt());
    // (bounded: a span this narrow would need more than SMLIB_SCREW_MAX_STATIONS)
    let mut guard = 0;
    while rise * lift_error(0.5 * h) > tolerance && guard < 10_000 {
        h *= 0.99;
        guard += 1;
    }
    let mut pieces = Vec::with_capacity(breaks.len());
    let mut spans = 0.0;
    for pair in breaks.windows(2) {
        let &[from, to] = pair else { continue };
        // Where the offset changes, its spans are narrower still.
        let rate = (m.offset(to) - m.offset(from)).abs() / (to - from);
        let mut piece = h;
        while rise * lift_error(0.5 * piece) + rate * piece * offset_error(0.5 * piece) > tolerance && piece > 1e-4 {
            piece *= 0.99;
        }
        let n = ((to - from) / piece).ceil();
        spans += n;
        if !(spans <= SCREW_MAX_STATIONS) || !(n >= 1.0) {
            return Ok(Err(Error::Argument));
        }
        pieces.push(Stations::new(from, to, n as usize));
    }
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    for st in &pieces {
        for (i, (c, wall)) in curves.iter().zip(&walls).enumerate() {
            let r = radius.get(i).copied().unwrap_or(0.0);
            if r > 0.0 && m.offset(st.first()) == m.offset(st.last()) {
                let (a, e) = ends(k, c)?;
                if let Err(err) = cylinder_piece(k, &mut b, &m, a, e, r + m.offset(st.first()), st)? {
                    return Ok(Err(err));
                }
                continue;
            }
            let surface = match screw_wall(&m, wall, st) {
                Ok(s) => s,
                Err(err) => return Ok(Err(err)),
            };
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            if let Err(err) = build_faces(&mut b, |b, fenv| create_face_from_surface(k, b, Some(surface), &domain, fenv)) {
                return Ok(Err(status(err)));
            }
        }
    }
    // The caps: the profile where the motion starts and where it ends, placed exactly.
    for a in [0.0, total] {
        let placed: Vec<Profile> = profile.iter().map(|c| screw_placed(&m, c, a)).collect();
        let cap = match parse(k, &placed) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        let (region, tol) = (b.infinite_region(), b.tolerance());
        if let Err(err) = build_faces(&mut b, |b, fenv| create_planar_faces_with_3d_curves(k, b, region, &cap, tol, fenv)) {
            return Ok(Err(status(err)));
        }
    }
    if let Err(e) = stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()) {
        return Ok(Err(status(e)));
    }
    b.refresh_cached_topology();
    finish(b, true)
}

/// `smlib_screw_sweep` and `smlib_screw_sweep_radial` (`law`: the radial offset law, as
/// (turn, offset) pairs).
pub(crate) fn screw_sweep(
    profile: &[Profile],
    origin: [f64; 3],
    axis: [f64; 3],
    pitch: f64,
    turns: f64,
    law: Option<&[(f64, f64)]>,
    tolerance: f64,
) -> Result<Body> {
    if !screw_ok(origin, pitch, turns, tolerance) {
        return Err(Error::Argument);
    }
    if let Some(law) = law {
        if law.is_empty() || law.len() > SCREW_MAX_LAW || !law.iter().all(|(t, o)| t.is_finite() && o.is_finite()) {
            return Err(Error::Argument);
        }
        for (i, (t, o)) in law.iter().enumerate() {
            let ascending = i == 0 || law.get(i - 1).is_some_and(|(p, _)| t > p);
            if !(*t >= 0.0 && *t <= turns) || !ascending || !(o.abs() < MAX_COORD) {
                return Err(Error::Argument);
            }
        }
    }
    let call = if law.is_some() { "smlib_screw_sweep_radial" } else { "smlib_screw_sweep" };
    making(call, |k| {
        let Some(axis) = unit(axis) else { return Ok(Err(Error::Argument)) };
        let law = law.unwrap_or_default();
        let m = Screw {
            o: v3(origin),
            k: axis,
            e: Vec3::new(0.0, 0.0, 0.0),
            pitch,
            angles: law.iter().map(|(t, _)| TAU * t).collect(),
            offsets: law.iter().map(|(_, o)| *o).collect(),
        };
        screw(k, profile, m, turns, tolerance)
    })
}

/// `put_curve`: `c` as the NURBS it is, with its interval; [`Error::Unsupported`] for a curve that
/// is no B-spline or whose knots are not clamped.
fn put_curve(c: &Curve) -> solidcore::Result<Out<(Profile, [f64; 2])>> {
    let Some(b) = bspline(c)? else { return Ok(Err(Error::Unsupported)) };
    let can = b.canonical()?;
    let degree = can.degree;
    let count = can.control_points.len();
    let mut knots = Vec::with_capacity(count + degree + 1);
    for (&u, &m) in can.knots.iter().zip(&can.knot_multiplicities) {
        knots.extend(std::iter::repeat_n(u, m));
    }
    let clamped = can.knot_multiplicities.first().is_some_and(|&m| m > degree) && can.knot_multiplicities.last().is_some_and(|&m| m > degree);
    if !(1..=ffi::sweeps::CURVE_MAX_DEGREE).contains(&degree)
        || count < degree + 1
        || count > ffi::sweeps::CURVE_MAX_POINTS
        || knots.len() != count + degree + 1
        || !clamped
    {
        return Ok(Err(Error::Unsupported));
    }
    let points = can.control_points.iter().enumerate().map(|(i, p)| [p.x, p.y, p.z, can.weights.get(i).copied().unwrap_or(1.0)]).collect();
    let ivl = c.natural_interval();
    Ok(Ok((Profile::Nurbs { degree, points, knots }, [ivl.min(), ivl.max()])))
}

/// The pick tolerance and the curve tolerance of `smlib_project_curves` and `smlib_face_section`.
fn tolerances_ok(pick: f64, tolerance: f64) -> bool {
    pick.is_finite() && pick > 0.0 && pick < 1e6 && tolerance.is_finite() && (1e-9..1e3).contains(&tolerance)
}

/// The face of `b` nearest `point` (`smlib_pick::pick_faces` of one point).
fn picked_face(k: &mut Kernel, b: &mut Brep, point: [f64; 3], tol: f64) -> solidcore::Result<Out<FaceId>> {
    Ok(match pick_faces(k, b, &[point], tol)? {
        Ok(f) => f.first().copied().ok_or(Error::NoMatch { point: 0 }),
        Err(m) => Err(face_miss(m)),
    })
}

/// `smlib_project_curves`: `curves` projected along `direction` onto the face nearest
/// `face_point` (`SmBrep::CreateParallelProjectionCurves` on that face), as NURBS with their
/// intervals; none is [`Error::Empty`].
pub(crate) fn project_curves(
    body: &Body,
    face_point: [f64; 3],
    pick_tolerance: f64,
    curves: &[Profile],
    direction: [f64; 3],
    tolerance: f64,
) -> Result<Vec<(Profile, [f64; 2])>> {
    if !face_point.iter().chain(&direction).all(|x| x.is_finite()) || !tolerances_ok(pick_tolerance, tolerance) {
        return Err(Error::Argument);
    }
    let len = v3(direction).length();
    if !(len > 1e-12) {
        return Err(Error::Argument);
    }
    with_body("smlib_project_curves", body, |k, b| {
        let given = match parse(k, curves) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        let face = match picked_face(k, b, face_point, pick_tolerance)? {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        let dir = v3(direction) * (1.0 / len);
        let faces = [face];
        let opts = ParallelProjectionOptions {
            approx_tol_3d: Some(tolerance),
            subset_faces: Some(&faces),
            want_3d: true,
            ..ParallelProjectionOptions::default()
        };
        let mut out = Vec::new();
        for c in &given {
            // curve->GetRootCurve(): a line, an arc and a B-spline are their own root, and
            // go in as themselves
            if !matches!(c.kind(), CurveKind::BSpline(_) | CurveKind::Line(_) | CurveKind::Ellipse(_)) {
                return Ok(Err(Error::Argument));
            }
            let made = match create_parallel_projection_curves(k, b, c, dir, &opts) {
                Ok(m) => m.curves_3d.unwrap_or_default(),
                Err(e) => return Ok(Err(status(e))),
            };
            for m in &made {
                match put_curve(m)? {
                    Ok(x) => out.push(x),
                    Err(e) => return Ok(Err(e)),
                }
            }
        }
        Ok(if out.is_empty() { Err(Error::Empty) } else { Ok(out) })
    })?
}

/// `smlib_face_section`: the section of the face nearest `face_point` by the plane through
/// `plane_point` square to `plane_normal` (`SmBrep::CreatePlanarSectionCurves` over the whole
/// B-rep, keeping the curves of the picked face), as NURBS with their intervals; none is
/// [`Error::Empty`].
pub(crate) fn face_section(
    body: &Body,
    face_point: [f64; 3],
    pick_tolerance: f64,
    plane_point: [f64; 3],
    plane_normal: [f64; 3],
    tolerance: f64,
) -> Result<Vec<(Profile, [f64; 2])>> {
    if !face_point.iter().chain(&plane_point).chain(&plane_normal).all(|x| x.is_finite()) || !tolerances_ok(pick_tolerance, tolerance) {
        return Err(Error::Argument);
    }
    let Some(normal) = unit(plane_normal) else { return Err(Error::Argument) };
    with_body("smlib_face_section", body, |k, b| {
        let face = match picked_face(k, b, face_point, pick_tolerance)? {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        // UV curves are asked for, so the classify is forced: an analytic face's intersector
        // makes none, and without it the trim drops the whole section (SmBrep.h documents the
        // flag for this). The capi leaves it FALSE, so the C++ kernel answers Empty on planes,
        // cylinders, cones and spheres (solidcore#1769).
        let opts = PlanarSectionOptions {
            approx_tol_3d: Some(tolerance),
            outputs: SectionOutputs { curves_3d: true, uv_curves: true, faces: true },
            force_uv_classify: true,
            ..PlanarSectionOptions::default()
        };
        let made = match create_planar_section_curves(k, b, v3(plane_point), normal, &opts) {
            Ok(m) => m,
            Err(e) => return Ok(Err(status(e))),
        };
        let curves = made.curves_3d.unwrap_or_default();
        let faces = made.faces.unwrap_or_default();
        // (each curve's face: without one apiece the picked face's curves are not known)
        if faces.len() != curves.len() {
            return Ok(Err(Error::Unsupported));
        }
        let mut out = Vec::new();
        for (c, &f) in curves.iter().zip(&faces) {
            if f == face {
                match put_curve(c)? {
                    Ok(x) => out.push(x),
                    Err(e) => return Ok(Err(e)),
                }
            }
        }
        Ok(if out.is_empty() { Err(Error::Empty) } else { Ok(out) })
    })?
}
