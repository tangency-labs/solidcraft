//! Flat arrays as a solidcore B-rep (`smlib_brep_import`): the arrays are validated, built into
//! SMLib's own serialised form (`BrepData`) and made into topology by `make_topology_from_data`,
//! which is what reading a native file does. Every malformed count, index or kind is
//! `SM_ERR_INVALID_INPUT` and builds nothing.
//!
//! Not read yet: the UV trims (`EDGEUSE_NURBS_*`; they are made from the 3D edges instead),
//! NURBS forms (a face or edge given both ways is built from its NURBS block alone), wire and
//! point shells.
use std::sync::Mutex;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::topology::Brep;
use solidcore::topology::brep::p10::{HEALER_VERSION, MakeTopologyOptions, TopologyEnv, make_topology_from_data};
use solidcore::topology::brep_data::{
    BrepData, DataSourceType, EdgeData, EuData, FaceData, FaceuseData, Keyed, LoopData, NO_OBJECT, RegionData, ShellData, VertexData,
};
use solidcore::topology::trimming_tools::create_uv_trim_curves;

use super::Handle;
use super::rs::{SM_ERR_INVALID_INPUT, State, named_ref, state, status};
use super::rs_flat::FlatHandle;
use crate::flat::*;
use crate::import::Arrays;

type R<T> = Result<T, i32>;

fn bad<T>() -> R<T> {
    Err(SM_ERR_INVALID_INPUT)
}

fn ok_or_bad<T>(x: Option<T>) -> R<T> {
    x.ok_or(SM_ERR_INVALID_INPUT)
}

/// The most items of one kind (the consumer's bound).
const MAX_ITEMS: usize = 1 << 24;

struct View<'a>(&'a Arrays);

impl View<'_> {
    fn i(&self, id: i32) -> &[i64] {
        self.0.ints.get(&id).map_or(&[], Vec::as_slice)
    }
    fn f(&self, id: i32) -> &[f64] {
        self.0.reals.get(&id).map_or(&[], Vec::as_slice)
    }
}

fn at<T: Copy>(a: &[T], k: usize) -> R<T> {
    ok_or_bad(a.get(k).copied())
}

/// `x` as an index below `n`.
fn idx(x: i64, n: usize) -> R<usize> {
    ok_or_bad(usize::try_from(x).ok().filter(|i| *i < n))
}

fn count(x: i64) -> R<usize> {
    ok_or_bad(usize::try_from(x).ok().filter(|n| *n <= MAX_ITEMS))
}

fn v3(a: &[f64], k: usize) -> R<Vec3> {
    Ok(Vec3::new(at(a, 3 * k)?, at(a, 3 * k + 1)?, at(a, 3 * k + 2)?))
}

/// Consecutive blocks of `counts` covering exactly `0..total`.
fn blocks(counts: &[i64], total: usize) -> R<Vec<std::ops::Range<usize>>> {
    let mut start = 0usize;
    let mut out = Vec::with_capacity(counts.len());
    for &c in counts {
        let end = ok_or_bad(start.checked_add(count(c)?).filter(|e| *e <= total))?;
        out.push(start..end);
        start = end;
    }
    if start != total {
        return bad();
    }
    Ok(out)
}

/// A placement from an origin, z axis and x axis (y = z × x).
fn placement(origin: Vec3, axis: Vec3, x: Vec3) -> Axis2Placement {
    Axis2Placement::new(Point3::new(origin.x, origin.y, origin.z), x, axis.cross(x))
}

/// Unique knots and their multiplicities of a full knot vector.
fn unique(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let (mut u, mut m): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for &k in knots {
        match (u.last(), m.last_mut()) {
            (Some(l), Some(c)) if *l == k => *c += 1,
            _ => {
                u.push(k);
                m.push(1);
            }
        }
    }
    (u, m)
}

/// The face's STEP parameters as the NURBS-side domain: both corners converted, then the box.
fn nurbs_domain(k: &Kernel, surface: &Surface, step: [f64; 4], angles: [bool; 2], scale: [f64; 2]) -> R<Extent2d> {
    let conv = |i: usize, x: f64| if angles[i] { x.to_degrees() } else { x / scale[i] };
    let to_nurbs = |p: Point2| -> R<Point2> {
        let c = surface.convert_uv_from_step_to_nurbs(k, p).map_err(|e| status(&e))?;
        // A torus's converter is the identity: see `rs_export::torus_step`, whose inverse this is.
        let (SurfaceKind::Torus(t), natural) = (surface.kind(), surface.natural_uv_domain_if_built()) else { return Ok(c) };
        let step = t.base().step_uv_domain();
        let (su, sv) = (step.u_max() - step.u_min(), step.v_max() - step.v_min());
        let (lu, lv) = (natural.u_max() - natural.u_min(), natural.v_max() - natural.v_min());
        if c.x != p.x || c.y != p.y || !(su > 0.0 && sv > 0.0) || (lu == su && lv == sv) {
            return Ok(c);
        }
        Ok(Point2::new(natural.u_min() + (p.x - step.u_min()) / su * lu, natural.v_min() + (p.y - step.v_min()) / sv * lv))
    };
    let (a, z) = (Point2::new(conv(0, step[0]), conv(1, step[1])), Point2::new(conv(0, step[2]), conv(1, step[3])));
    let (a, z) = (to_nurbs(a)?, to_nurbs(z)?);
    Ok(Extent2d::from_bounds(a.x.min(z.x), a.y.min(z.y), a.x.max(z.x), a.y.max(z.y)))
}

struct Faces {
    surfaces: Vec<Keyed<Surface>>,
    data: Vec<(u64, Extent2d)>,
}

/// Each face's surface (one data surface per face) and its domain in the NURBS parameters.
fn faces(k: &Kernel, flat: &View, n_faces: usize) -> R<Faces> {
    let kinds = flat.i(FACE_SURFACE_TYPE);
    let range = flat.f(FACE_RANGE);
    if kinds.len() != n_faces || range.len() != 4 * n_faces {
        return bad();
    }
    let forms = flat.i(FACE_NURBS_FORM);
    if !forms.is_empty() && forms.len() != n_faces {
        return bad();
    }
    let mut cursor = [0usize; 6];
    let (mut nj, mut cv, mut uk, mut vk, mut wt) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut out = Faces { surfaces: Vec::with_capacity(n_faces), data: Vec::with_capacity(n_faces) };
    for (f, &kind) in kinds.iter().enumerate() {
        let slot = ok_or_bad(usize::try_from(kind).ok().filter(|s| *s < 6))?;
        let i = at(&cursor, slot)?;
        if let Some(c) = cursor.get_mut(slot) {
            *c += 1;
        }
        let step = [at(range, 4 * f)?, at(range, 4 * f + 1)?, at(range, 4 * f + 2)?, at(range, 4 * f + 3)?];
        let form = kind != i64::from(SURFACE_NURBS) && forms.get(f) == Some(&1);
        let mut nurbs = |nj: &mut usize| -> R<(Surface, Extent2d)> {
            let j = *nj;
            let nu = count(at(flat.i(FACE_NURBS_U_VERTEX_COUNT), j)?)?;
            let nv = count(at(flat.i(FACE_NURBS_V_VERTEX_COUNT), j)?)?;
            let (ou, ov) = (count(at(flat.i(FACE_NURBS_U_ORDER), j)?)?, count(at(flat.i(FACE_NURBS_V_ORDER), j)?)?);
            let n = ok_or_bad(nu.checked_mul(nv).filter(|n| *n <= MAX_ITEMS))?;
            if ou < 2 || ov < 2 || ou > nu || ov > nv {
                return bad();
            }
            let take = |v: &[f64], from: usize, len: usize| -> R<Vec<f64>> {
                {
                    let end = ok_or_bad(from.checked_add(len))?;
                    ok_or_bad(v.get(from..end).map(<[f64]>::to_vec))
                }
            };
            let pts = take(flat.f(FACE_NURBS_CVS), 3 * cv, 3 * n)?;
            let u_full = take(flat.f(FACE_NURBS_U_KNOTS), uk, nu + ou)?;
            let v_full = take(flat.f(FACE_NURBS_V_KNOTS), vk, nv + ov)?;
            let weights = if flat.f(FACE_NURBS_WEIGHTS).is_empty() { None } else { Some(take(flat.f(FACE_NURBS_WEIGHTS), wt, n)?) };
            if weights.as_ref().is_some_and(|w| w.iter().any(|w| !(*w > 0.0))) {
                return bad();
            }
            cv += n;
            uk += nu + ou;
            vk += nv + ov;
            wt += n;
            *nj += 1;
            let points: Vec<Point3> = pts.as_chunks::<3>().0.iter().map(|p| Point3::new(p[0], p[1], p[2])).collect();
            let ((uu, um), (vu, vm)) = (unique(&u_full), unique(&v_full));
            let domain = Extent2d::from_bounds(step[0], step[1], step[2], step[3]);
            let s = BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: ou - 1,
                v_degree: ov - 1,
                control_points: &points,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &um,
                v_multiplicities: &vm,
                u_knots: &uu,
                v_knots: &vu,
                knot_type: KnotType::Unspecified,
                weights: weights.as_deref(),
                uv_domain: None,
            })
            .map_err(|e| status(&e))?;
            Ok((Surface::from(s), domain))
        };
        let analytic = |step: [f64; 4], step_angles: [bool; 2], scale: [f64; 2], surface: Surface| -> R<(Surface, Extent2d)> {
            let mut surface = surface;
            // The STEP domain the face covers (degrees; plane distances), so that the NURB spans it.
            let deg = |i: usize, x: f64| if step_angles[i] { x.to_degrees() } else { x / scale[i] };
            let (u0, u1) = (deg(0, step[0]).min(deg(0, step[2])), deg(0, step[0]).max(deg(0, step[2])));
            let (v0, v1) = (deg(1, step[1]).min(deg(1, step[3])), deg(1, step[1]).max(deg(1, step[3])));
            if u1 > u0 && v1 > v0 {
                surface.adjust_step_uv_domain(k, &Extent2d::from_bounds(u0, v0, u1, v1)).map_err(|e| status(&e))?;
            }
            let domain = nurbs_domain(k, &surface, step, step_angles, scale)?;
            Ok((surface, domain))
        };
        let (surface, domain) = if kind == i64::from(SURFACE_NURBS) {
            nurbs(&mut nj)?
        } else if form {
            // unconfirmed: a face given as analytic and NURBS is built from its NURBS block.
            nurbs(&mut nj)?
        } else {
            let o = |id: i32| -> R<Vec3> { v3(flat.f(id), i) };
            let one = |id: i32| -> R<f64> { at(flat.f(id), i) };
            match kind {
                SURFACE_PLANE_I => {
                    let pl = placement(o(FACE_PLANE_ORIGIN)?, o(FACE_PLANE_AXIS)?, o(FACE_PLANE_REF_DIRECTION)?);
                    analytic(step, [false, false], [1.0, 1.0], Surface::from(Plane::create_canonical(&pl).map_err(|e| status(&e))?))?
                }
                SURFACE_CYLINDER_I => {
                    let pl = placement(o(FACE_CYLINDER_ORIGIN)?, o(FACE_CYLINDER_AXIS)?, o(FACE_CYLINDER_REF_DIRECTION)?);
                    let c = Cone::create_canonical_cylinder(k, &pl, one(FACE_CYLINDER_RADIUS)?).map_err(|e| status(&e))?;
                    analytic(step, [true, false], [1.0, 1.0], Surface::from(c))?
                }
                SURFACE_CONE_I => {
                    let pl = placement(o(FACE_CONE_ORIGIN)?, o(FACE_CONE_AXIS)?, o(FACE_CONE_REF_DIRECTION)?);
                    let (radius, semi) = (one(FACE_CONE_RADIUS)?, one(FACE_CONE_SEMI_ANGLE)?);
                    // A bounded cone over the face's heights [v0, v1], its placement moved up to v0 (as
                    // a primitive's is, from its base): an unbounded one has its NURB over a default span.
                    let (v0, v1) = (step[1].min(step[3]), step[1].max(step[3]));
                    let up = pl.z_axis();
                    let moved = Axis2Placement::new(
                        Point3::new(pl.origin().x + v0 * up.x, pl.origin().y + v0 * up.y, pl.origin().z + v0 * up.z),
                        pl.x_axis(),
                        pl.y_axis(),
                    );
                    let c =
                        Cone::create_canonical_with_semi_angle(k, &moved, radius + v0 * semi.tan(), semi.to_degrees(), Some(v1 - v0), false, false)
                            .map_err(|e| status(&e))?;
                    analytic([step[0], step[1] - v0, step[2], step[3] - v0], [true, false], [1.0, 1.0], Surface::from(c))?
                }
                SURFACE_SPHERE_I => {
                    let pl = placement(o(FACE_SPHERE_CENTER)?, o(FACE_SPHERE_AXIS)?, o(FACE_SPHERE_REF_DIRECTION)?);
                    let s = Sphere::create_canonical(k, &pl, one(FACE_SPHERE_RADIUS)?).map_err(|e| status(&e))?;
                    analytic(step, [true, true], [1.0, 1.0], Surface::from(s))?
                }
                SURFACE_TORUS_I => {
                    let pl = placement(o(FACE_TORUS_ORIGIN)?, o(FACE_TORUS_AXIS)?, o(FACE_TORUS_REF_DIRECTION)?);
                    let t =
                        Torus::create_canonical(k, &pl, one(FACE_TORUS_MAJOR_RADIUS)?, one(FACE_TORUS_MINOR_RADIUS)?, TorusOptions::default(), None)
                            .map_err(|e| status(&e))?;
                    analytic(step, [true, true], [1.0, 1.0], Surface::from(t))?
                }
                _ => return bad(),
            }
        };
        out.surfaces.push(Keyed { key: ObjectKey((1 << 49) + f as u64), object: surface });
        out.data.push((f as u64, domain));
    }
    Ok(out)
}

const SURFACE_PLANE_I: i64 = SURFACE_PLANE as i64;
const SURFACE_CYLINDER_I: i64 = SURFACE_CYLINDER as i64;
const SURFACE_CONE_I: i64 = SURFACE_CONE as i64;
const SURFACE_SPHERE_I: i64 = SURFACE_SPHERE as i64;
const SURFACE_TORUS_I: i64 = SURFACE_TORUS as i64;
const CURVE_LINE_I: i64 = CURVE_LINE as i64;
const CURVE_CIRCLE_I: i64 = CURVE_CIRCLE as i64;
const CURVE_ELLIPSE_I: i64 = CURVE_ELLIPSE as i64;
/// The most of a record's tolerance (mm) that becomes a zone tolerance here. A STEP file's stated
/// uncertainty is the record's tolerance (the importer's default for a file that states none is
/// 1e-3), and some files state far more than their geometry needs (a FreeCAD insert: 0.1); as a
/// zone tolerance that makes every feature smaller than it a point, and its faces collapse. A tolerance
/// the geometry really needs (a vertex off its curve) is the importer's, per item, and a gap
/// wider than this is left open for the checks to say so.
const MAX_TOLERANCE: f64 = 1e-3;

const CURVE_NURBS_I: i64 = CURVE_NURBS as i64;

struct Edges {
    curves: Vec<Keyed<Curve>>,
    edges: Vec<EdgeData>,
    /// The edge's NURBS direction is the arrays' reverse: its uses turn over.
    flipped: Vec<bool>,
}

fn edges(k: &Kernel, flat: &View, n_edges: usize, n_vertices: usize) -> R<Edges> {
    let kinds = flat.i(EDGE_CURVE_TYPE);
    let range = flat.f(EDGE_RANGE);
    if kinds.len() != n_edges || range.len() != 2 * n_edges || flat.i(EDGE_VERTICES).len() != 2 * n_edges {
        return bad();
    }
    let forms = flat.i(EDGE_NURBS_FORM);
    if !forms.is_empty() && forms.len() != n_edges {
        return bad();
    }
    let tolerances = match flat.f(EDGE_TOLERANCE) {
        [] => None,
        t if t.len() == n_edges => Some(t),
        _ => return bad(),
    };
    let (mut lines, mut circles, mut ellipses, mut nj, mut cv, mut kn) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut out = Edges { curves: Vec::with_capacity(n_edges), edges: Vec::with_capacity(n_edges), flipped: Vec::with_capacity(n_edges) };
    for (e, &kind) in kinds.iter().enumerate() {
        let (a, z) = (at(range, 2 * e)?, at(range, 2 * e + 1)?);
        if !(a.is_finite() && z.is_finite()) || a == z {
            return bad();
        }
        let (lo, hi) = (a.min(z), a.max(z));
        let form = matches!(kind, CURVE_CIRCLE_I | CURVE_ELLIPSE_I) && forms.get(e) == Some(&1);
        let mut nurbs = |nj: &mut usize| -> R<Curve> {
            let n = count(at(flat.i(EDGE_NURBS_VERTEX_COUNT), *nj)?)?;
            let o = count(at(flat.i(EDGE_NURBS_ORDER), *nj)?)?;
            if o < 2 || o > n {
                return bad();
            }
            let cvs = ok_or_bad(flat.f(EDGE_NURBS_CVS).get(3 * cv..3 * (cv + n)))?;
            let knots = ok_or_bad(flat.f(EDGE_NURBS_KNOTS).get(kn..kn + n + o))?;
            let weights = match flat.f(EDGE_NURBS_WEIGHTS) {
                [] => None,
                w => Some(ok_or_bad(w.get(cv..cv + n))?),
            };
            if weights.is_some_and(|w| w.iter().any(|w| !(*w > 0.0))) {
                return bad();
            }
            cv += n;
            kn += n + o;
            *nj += 1;
            let points: Vec<Point3> = cvs.as_chunks::<3>().0.iter().map(|p| Point3::new(p[0], p[1], p[2])).collect();
            let (uk, um) = unique(knots);
            let c = BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: o - 1,
                control_points: &points,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &um,
                knots: &uk,
                knot_type: KnotType::Unspecified,
                weights,
                trim: None,
            })
            .map_err(|e| status(&e))?;
            Ok(Curve::from(c))
        };
        let interval_deg = Extent1d::from_min_max(lo.to_degrees(), hi.to_degrees());
        // `raw`: the range is already the curve's own parameter (a NURBS).
        let (curve, raw) = match kind {
            CURVE_LINE_I => {
                let (o, d) = (v3(flat.f(EDGE_LINE_ORIGIN), lines)?, v3(flat.f(EDGE_LINE_DIRECTION), lines)?);
                lines += 1;
                let speed = d.length();
                if !(speed > 0.0) {
                    return bad();
                }
                let line = Line::bounded(
                    Point3::new(o.x, o.y, o.z),
                    d,
                    &Extent1d::from_min_max(lo, hi),
                    &BoundedLineOptions { scale: speed, dim: 3, ..BoundedLineOptions::default() },
                );
                (Curve::from(line), false)
            }
            CURVE_CIRCLE_I if form => {
                circles += 1;
                (nurbs(&mut nj)?, true)
            }
            CURVE_ELLIPSE_I if form => {
                ellipses += 1;
                (nurbs(&mut nj)?, true)
            }
            CURVE_CIRCLE_I => {
                let pl = placement(
                    v3(flat.f(EDGE_CIRCLE_CENTER), circles)?,
                    v3(flat.f(EDGE_CIRCLE_AXIS), circles)?,
                    v3(flat.f(EDGE_CIRCLE_REF_DIRECTION), circles)?,
                );
                let r = at(flat.f(EDGE_CIRCLE_RADIUS), circles)?;
                circles += 1;
                (Curve::from(Ellipse::create_canonical_circle(k, &pl, r, Some(&interval_deg), None).map_err(|e| status(&e))?), false)
            }
            CURVE_ELLIPSE_I => {
                let pl = placement(
                    v3(flat.f(EDGE_ELLIPSE_CENTER), ellipses)?,
                    v3(flat.f(EDGE_ELLIPSE_AXIS), ellipses)?,
                    v3(flat.f(EDGE_ELLIPSE_REF_DIRECTION), ellipses)?,
                );
                let (rx, ry) = (at(flat.f(EDGE_ELLIPSE_X_RADIUS), ellipses)?, at(flat.f(EDGE_ELLIPSE_Y_RADIUS), ellipses)?);
                ellipses += 1;
                (Curve::from(Ellipse::create_canonical(k, &pl, rx, ry, Some(&interval_deg), None).map_err(|e| status(&e))?), false)
            }
            CURVE_NURBS_I => (nurbs(&mut nj)?, true),
            _ => return bad(),
        };
        // The NURBS parameters of the range ends.
        let (n0, n1) = if raw {
            (lo, hi)
        } else {
            let deg = matches!(kind, CURVE_CIRCLE_I | CURVE_ELLIPSE_I);
            let p = |t: f64| curve.convert_t_from_step_to_nurbs(k, if deg { t.to_degrees() } else { t }).map_err(|e| status(&e));
            let (p0, p1) = (p(lo)?, p(hi)?);
            (p0.min(p1), p0.max(p1))
        };
        // The arrays' start vertex is at `a`: the NURBS start unless the range runs backwards.
        let flipped = a > z;
        let (s, t) = (idx(at(flat.i(EDGE_VERTICES), 2 * e)?, n_vertices)?, idx(at(flat.i(EDGE_VERTICES), 2 * e + 1)?, n_vertices)?);
        let (start, end) = if flipped { (t, s) } else { (s, t) };
        out.curves.push(Keyed { key: ObjectKey((1 << 48) + e as u64), object: curve });
        let ci = out.curves.len() as u64 - 1;
        out.edges.push(EdgeData {
            zone_tol_3d: tolerances.map_or(0.0, |t| t.get(e).copied().unwrap_or(0.0).min(MAX_TOLERANCE) / 2.0),
            interval: Extent1d::from_min_max(n0, n1),
            curve: ci,
            start_vertex: start as u64,
            end_vertex: end as u64,
            prim_eu: 0,
            ..EdgeData::default()
        });
        out.flipped.push(flipped);
    }
    Ok(out)
}

/// The arrays as a B-rep (the kernel locked by `s`) and the key domain of its objects.
fn build(s: &mut State, a: &Arrays) -> R<(Brep, u64)> {
    let flat = View(a);
    let n_faceuses = flat.i(FACEUSE_FACE).len();
    let n_faces = flat.i(FACE_LOOP_COUNT).len();
    let n_loops = flat.i(LOOP_EDGEUSE_COUNT).len();
    let n_eu = flat.i(EDGEUSE_EDGE).len();
    let n_edges = flat.i(EDGE_CURVE_TYPE).len();
    let n_vertices = flat.f(VERTEX_POSITION).len() / 3;
    if flat.f(VERTEX_POSITION).len() != 3 * n_vertices
        || flat.i(FACEUSE_ORIENTATION).len() != n_faceuses
        || flat.i(EDGEUSE_ORIENTATION).len() != n_eu
        || flat.i(LOOP_VERTEX).len() != n_loops
        || [n_faceuses, n_faces, n_loops, n_eu, n_edges, n_vertices].iter().any(|n| *n > MAX_ITEMS)
    {
        return bad();
    }
    let tolerance = match flat.f(TOLERANCE) {
        [t] if t.is_finite() && *t > 0.0 => *t,
        _ => return bad(),
    };
    let n_shells = flat.i(SHELL_FACEUSE_COUNT).len();
    if flat.i(SHELL_WIRE_EDGE_COUNT).len() != n_shells
        || flat.i(SHELL_POINT_TYPE).len() != n_shells
        || flat.i(SHELL_WIRE_EDGE_COUNT).iter().chain(flat.i(SHELL_POINT_TYPE)).any(|c| *c != 0)
    {
        return bad();
    }
    let region_shells = blocks(flat.i(REGION_SHELL_COUNT), n_shells)?;
    if region_shells.is_empty() || flat.i(REGION_TYPE).len() != region_shells.len() {
        return bad();
    }
    let shell_faceuses = blocks(flat.i(SHELL_FACEUSE_COUNT), n_faceuses)?;
    let face_loops = blocks(flat.i(FACE_LOOP_COUNT), n_loops)?;
    let loop_eus = blocks(flat.i(LOOP_EDGEUSE_COUNT), n_eu)?;

    let k = &s.kernel;
    let face_geometry = faces(k, &flat, n_faces)?;
    let edge_geometry = edges(k, &flat, n_edges, n_vertices)?;
    let radial = radial_arrays(&flat, n_eu, n_edges)?;

    // Radial order of each edge's records, the data's own (`manifold = 0`), or else the order of
    // the records themselves, which is right where every edge has two.
    let mut records_of: Vec<Vec<usize>> = vec![Vec::new(); n_edges];
    for u in 0..n_eu {
        records_of.get_mut(idx(at(flat.i(EDGEUSE_EDGE), u)?, n_edges)?).ok_or(SM_ERR_INVALID_INPUT)?.push(u);
    }
    if radial.is_none() && records_of.iter().any(|r| r.len() != 2) {
        return bad();
    }

    let mut data = BrepData::new(if radial.is_some() { 0 } else { 1 }, DataSourceType::SmLib);
    data.zone_tol_3d = tolerance.min(MAX_TOLERANCE) / 2.0;
    data.start_region = 0;
    data.num_regions = region_shells.len() as u64;
    // Trusted arrays skip SMLib's healer.
    data.healer_version = if flat.i(SOURCE).first() == Some(&0) { 0 } else { HEALER_VERSION };

    for (r, block) in region_shells.iter().enumerate() {
        data.regions.push(RegionData {
            start_shell: block.start as u64,
            num_shells: block.len() as u64,
            is_void: if r == 0 { 1 } else { i32::from(at(flat.i(REGION_TYPE), r)? != 0) },
            ..RegionData::default()
        });
    }
    for block in &shell_faceuses {
        data.shells.push(ShellData { shell_type: 0, faceuse_start: block.start as u64, num_faceuses: block.len() as u64, ..ShellData::default() });
    }
    for fu in 0..n_faceuses {
        data.faceuses.push(FaceuseData {
            face: idx(at(flat.i(FACEUSE_FACE), fu)?, n_faces)? as u64,
            orientation: i32::from(at(flat.i(FACEUSE_ORIENTATION), fu)? == 1),
            ..FaceuseData::default()
        });
    }
    let trim_types = flat.i(FACE_TRIM_TYPE);
    for (f, block) in face_loops.iter().enumerate() {
        let (_, domain) = at_ref(&face_geometry.data, f)?;
        data.faces.push(FaceData {
            zone_tol_3d: 0.0,
            surface: f as u64,
            start_loop: block.start as u64,
            num_loops: block.len() as u64,
            uv_domain: *domain,
            rectangular_trim: i32::from(trim_types.get(f) == Some(&1)),
            ..FaceData::default()
        });
    }
    data.surfaces = face_geometry.surfaces;
    data.curves_3d = edge_geometry.curves;

    // Edge uses: the records, then one per vertex loop.
    let mut vertex_loops: Vec<EuData> = Vec::new();
    for (l, uses) in loop_eus.iter().enumerate() {
        if uses.is_empty() {
            let v = idx(at(flat.i(LOOP_VERTEX), l)?, n_vertices)? as u64;
            data.loops.push(LoopData { loop_type: 1, vertex: v, start_eu: (n_eu + vertex_loops.len()) as u64, num_eu: 1, ..LoopData::default() });
            vertex_loops.push(EuData { eu_type: 1, vertex: v, loop_: l as u64, orientation: 1, uv_curve: NO_OBJECT, ..EuData::default() });
        } else {
            data.loops.push(LoopData { loop_type: 0, start_eu: uses.start as u64, num_eu: uses.len() as u64, ..LoopData::default() });
        }
    }
    let mut loop_of = vec![0usize; n_eu];
    for (l, uses) in loop_eus.iter().enumerate() {
        for u in uses.clone() {
            if let Some(slot) = loop_of.get_mut(u) {
                *slot = l;
            }
        }
    }
    // Each record's first use runs along its edge (in the edge's NURBS direction) or not.
    let same_flags = (0..n_eu)
        .map(|u| {
            let e = idx(at(flat.i(EDGEUSE_EDGE), u)?, n_edges)?;
            Ok((at(flat.i(EDGEUSE_ORIENTATION), u)? == 1) != *at_ref(&edge_geometry.flipped, e)?)
        })
        .collect::<R<Vec<bool>>>()?;
    for (u, &same) in same_flags.iter().enumerate() {
        let e = idx(at(flat.i(EDGEUSE_EDGE), u)?, n_edges)?;
        let (next_eu, mate_next_eu) = match &radial {
            Some(r) => r.links(u, &same_flags)?,
            None => (0, 0),
        };
        data.edgeuses.push(EuData {
            eu_type: 0,
            edge: e as u64,
            loop_: loop_of.get(u).copied().ok_or(SM_ERR_INVALID_INPUT)? as u64,
            orientation: i32::from(same),
            uv_curve: NO_OBJECT,
            next_eu,
            mate_next_eu,
            ..EuData::default()
        });
    }
    data.edgeuses.extend(vertex_loops);
    data.edges = edge_geometry.edges;
    for (e, edge) in data.edges.iter_mut().enumerate() {
        // The primary use is the first record's member that runs along the edge.
        let first = *records_of.get(e).and_then(|r| r.first()).ok_or(SM_ERR_INVALID_INPUT)?;
        edge.prim_eu = if data.edgeuses.get(first).is_some_and(|u| u.orientation != 0) { first as i64 } else { -(first as i64) - 1 };
    }
    let vertex_tolerances = match flat.f(VERTEX_TOLERANCE) {
        [] => None,
        t if t.len() == n_vertices => Some(t),
        _ => return bad(),
    };
    for v in 0..n_vertices {
        let p = v3(flat.f(VERTEX_POSITION), v)?;
        if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
            return bad();
        }
        data.vertices.push(VertexData {
            zone_tol_3d: vertex_tolerances.map_or(0.0, |t| t.get(v).copied().unwrap_or(0.0).min(MAX_TOLERANCE) / 2.0),
            point: Point3::new(p.x, p.y, p.z),
            ..VertexData::default()
        });
    }

    // The B-rep: its objects named in a fresh key domain, as a copy's are.
    s.copies += 1;
    let domain = s.copies * 100_000_000;
    let mut brep = Brep::new_with_infinite_region().map_err(|e| status(&e))?;
    let (mut records, mut attributes) = (solidcore::topology::topology::NotifyRecords::new(), AttributeStore::default());
    let key_of = |entity| named_ref(entity, domain);
    let mut env =
        TopologyEnv { all_attributes: &[], attributes: &mut attributes, key_of: &key_of, records: &mut records, next_key: (1 << 50) + domain };
    let options = MakeTopologyOptions { healer_enabled: data.healer_version < HEALER_VERSION, ..MakeTopologyOptions::default() };
    make_topology_from_data(&mut s.kernel, &mut brep, &mut data, &mut env, options).map_err(|e| status(&e))?;
    if options.healer_enabled {
        // The healer made the UV trims.
    } else {
        for f in brep.face_list().to_vec() {
            create_uv_trim_curves(&s.kernel, &mut brep, f).map_err(|e| status(&e))?;
        }
    }
    Ok((brep, domain))
}

fn at_ref<T>(a: &[T], k: usize) -> R<&T> {
    ok_or_bad(a.get(k))
}

/// The radial arrays, checked: in range, entries 0 or 1.
struct Radial {
    next: Vec<usize>,
}

fn radial_arrays(flat: &View, n_eu: usize, n_edges: usize) -> R<Option<Radial>> {
    let (next, entry) = (flat.i(EDGEUSE_NEXT_RADIAL), flat.i(EDGEUSE_RADIAL_ENTRY));
    if next.is_empty() && entry.is_empty() {
        return Ok(None);
    }
    if next.len() != n_eu || entry.len() != n_eu {
        return bad();
    }
    let edge = flat.i(EDGEUSE_EDGE).iter().map(|e| idx(*e, n_edges)).collect::<R<Vec<_>>>()?;
    let next = next.iter().map(|n| idx(*n, n_eu)).collect::<R<Vec<_>>>()?;
    for (u, n) in next.iter().enumerate() {
        if edge.get(u) != edge.get(*n) {
            return bad();
        }
    }
    // The entry flags are checked but not kept (see `Radial::links`).
    if entry.iter().any(|x| !matches!(x, 0 | 1)) {
        return bad();
    }
    Ok(Some(Radial { next }))
}

impl Radial {
    /// `(next_eu, mate_next_eu)` of record `u`, `same[j]` saying whether record `j`'s first use
    /// (`eu1`, on the face's primary side) runs along its edge. SMLib's list reads, round the edge,
    /// each record as its pair `[use along the edge, its mate]` (the mate of a use is its list
    /// neighbour: the next for the one with the primary's orientation, else the last), so the pair's
    /// order is fixed by the orientations and the records follow each other as `next` says. The
    /// entry flags are checked but not needed: they say which side of the face the traversal enters
    /// from, which these orientations already settle.
    fn links(&self, u: usize, same: &[bool]) -> R<(i64, i64)> {
        let index = |j: usize| i64::try_from(j).map_err(|_| SM_ERR_INVALID_INPUT);
        // The first of record j's pair: its use along the edge (`eu1` is `j`, `eu2` is `-j - 1`).
        let first = |j: usize| -> R<i64> { Ok(if *at_ref(same, j)? { index(j)? } else { -index(j)? - 1 }) };
        let after = first(*at_ref(&self.next, u)?)?;
        let me = index(u)?;
        Ok(if *at_ref(same, u)? {
            // eu1 -> eu2 -> next record's first
            (-me - 1, after)
        } else {
            // eu2 -> eu1 -> next record's first
            (after, me)
        })
    }
}

pub fn import(f: &FlatHandle) -> R<Handle> {
    let mut s = state();
    let (b, domain) = build(&mut s, f.arrays())?;
    Ok(Handle { brep: Mutex::new(b), domain })
}
