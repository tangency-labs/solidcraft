//! Extrusion, revolution and sweeps on solidcore. The profile's curve list becomes `solidcore` curves
//! (lines, arcs and circles placed on their planes, NURBS curves); a drafted extrusion is built
//! only from one whole circle or a polygon of lines (not `Round`), any other is refused ([`ERR_UNSUPPORTED`]).
use solidcore::curve::curve::Curve;
use solidcore::curve::curve::p2::CurveKind;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, BSplineCurveForm};
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::srf_adv::p4::create_b_vectors;
use solidcore::primitive::api_curves::{CanonicalCurveParams, create_analytic_circle, create_arc, create_canonical_curve, create_line_segment};
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::primitive::primitive_creation::p3::prepare_curve_sweep;
use solidcore::sweep::api_brep::{create_curve_sweep, create_linear_sweep, create_rotational_sweep, create_taper_extrude};
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p2::{ArenaNotifyState, is_manifold_solid};
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p5::{StitchAndOrientOptions, stitch_and_orient};
use solidcore::topology::brep::p8::{FaceBuildEnv, create_planar_face_with_3d_curves, create_planar_faces_with_3d_curves};
use solidcore::topology::proto_topology::p1::api_transform::{TransformObject, transform_independent_object};
use solidcore::topology::topology_traverser::Marks;
use solidcore::{EntityRef, FaceId};

use super::ops::{ERR_NOT_SOLID, ERR_UNSUPPORTED};
use super::rs::{state, status};
use super::sweeps::{CURVE_ARC, CURVE_LINE, CURVE_NURBS, DRAFT_CORNER_ROUND, List};
use super::{ERR_ARG, Handle};

fn point(a: &[f64]) -> Result<Point3, i32> {
    match a {
        [x, y, z, ..] => Ok(Point3::new(*x, *y, *z)),
        _ => Err(ERR_ARG),
    }
}

fn vector(a: &[f64]) -> Result<Vec3, i32> {
    point(a).map(|p| Vec3::new(p.x, p.y, p.z))
}

/// An arc about `axis` (unit) from angle `start` to `end` (radians), angle 0 along `reference`:
/// built about +Z from +X at the origin over the span, then placed with its X turned to the
/// start (`create_arc` takes angles within a turn only, which `start` and `end` need not be).
fn arc(k: &mut Kernel, a: &[f64]) -> Result<Curve, i32> {
    let (center, axis, reference) = (point(a)?, vector(a.get(3..).ok_or(ERR_ARG)?)?, vector(a.get(6..).ok_or(ERR_ARG)?)?);
    let &[radius, start, end] = a.get(9..12).ok_or(ERR_ARG)? else { return Err(ERR_ARG) };
    let origin = Point3::new(0.0, 0.0, 0.0);
    let span = end - start;
    let mut c = if span >= std::f64::consts::TAU - 1e-12 {
        Curve::from(create_analytic_circle(k, origin, radius).map_err(|e| status(&e))?)
    } else {
        Curve::from(create_arc(origin, radius, 0.0, span.to_degrees()).map_err(|e| status(&e))?)
    };
    let x = reference * start.cos() + axis.cross(reference) * start.sin();
    let placement = Axis2Placement::new(center, x, axis.cross(x));
    transform_independent_object(k, Some(TransformObject::Curve(&mut c)), &placement, None).map_err(|e| status(&e))?;
    Ok(c)
}

fn curves(k: &mut Kernel, list: &List) -> Result<Vec<Curve>, i32> {
    if list.reals().iter().any(|v| !v.is_finite()) {
        return Err(ERR_ARG);
    }
    let (mut kinds, mut reals, mut out) = (list.kinds().iter(), list.reals(), Vec::new());
    while let Some(&kind) = kinds.next() {
        match kind {
            CURVE_LINE => {
                let (a, b) = (point(reals)?, point(reals.get(3..).ok_or(ERR_ARG)?)?);
                out.push(Curve::from(create_line_segment(a, b).map_err(|e| status(&e))?));
                reals = reals.get(6..).ok_or(ERR_ARG)?;
            }
            CURVE_ARC => {
                out.push(arc(k, reals)?);
                reals = reals.get(12..).ok_or(ERR_ARG)?;
            }
            CURVE_NURBS => {
                let (&degree, &count) = (kinds.next().ok_or(ERR_ARG)?, kinds.next().ok_or(ERR_ARG)?);
                let (degree, count) = (usize::try_from(degree).map_err(|_| ERR_ARG)?, usize::try_from(count).map_err(|_| ERR_ARG)?);
                let used = count.checked_mul(5).and_then(|n| n.checked_add(degree + 1)).ok_or(ERR_ARG)?;
                out.push(nurbs(degree, count, reals.get(..used).ok_or(ERR_ARG)?)?);
                reals = &reals[used..];
            }
            _ => return Err(ERR_ARG),
        }
    }
    Ok(out)
}

/// A NURBS curve: `count` control points (x, y, z, weight), then `count + degree + 1` clamped,
/// non-decreasing knots.
fn nurbs(degree: usize, count: usize, r: &[f64]) -> Result<Curve, i32> {
    if degree < 1 || count < degree + 1 {
        return Err(ERR_ARG);
    }
    let (pts, knots) = r.split_at(4 * count);
    let (quads, _) = pts.as_chunks::<4>();
    let points: Vec<Point3> = quads.iter().map(|p| Point3::new(p[0], p[1], p[2])).collect();
    let weights: Vec<f64> = quads.iter().map(|p| p[3]).collect();
    if weights.iter().any(|w| !(*w > 1e-12 && *w < 1e12)) || knots.windows(2).any(|w| w[1] < w[0]) {
        return Err(ERR_ARG);
    }
    let (mut unique, mut mult): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for &x in knots {
        match unique.last() {
            Some(&u) if u == x => *mult.last_mut().ok_or(ERR_ARG)? += 1,
            _ => {
                unique.push(x);
                mult.push(1);
            }
        }
    }
    let n = unique.len();
    if n < 2
        || mult[0] != degree + 1
        || mult[n - 1] != degree + 1
        || !(unique[n - 1] - unique[0] > 1e-12)
        || mult[1..n - 1].iter().any(|m| *m > degree)
    {
        return Err(ERR_ARG);
    }
    let rational = weights.iter().any(|w| *w != 1.0);
    let c = create_canonical_curve(&CanonicalCurveParams {
        points: &points,
        knots: &unique,
        knot_multiplicities: &mult,
        weights: rational.then_some(weights.as_slice()),
        degree,
        form: BSplineCurveForm::Unspecified,
    })
    .map_err(|e| status(&e))?;
    Ok(Curve::from(c))
}

/// A capped sweep must be one closed manifold solid.
fn finish(b: solidcore::Brep, cap: bool) -> Result<Handle, i32> {
    if cap && !is_manifold_solid(&b).map_err(|e| status(&e))? {
        return Err(ERR_NOT_SOLID);
    }
    Ok(Handle::new(b))
}

pub fn extrude(profile: &List, direction: &[f64; 3], distance: f64, draft: f64, corner: i32, cap: bool, _at: &mut [f64; 3]) -> Result<Handle, i32> {
    if !distance.is_finite() || direction.iter().any(|v| !v.is_finite()) || !draft.is_finite() {
        return Err(ERR_ARG);
    }
    if draft != 0.0 {
        // A taper needs the native shim's corner refusals (a corner that cannot close) and its
        // curvature rule, which solidcore's taper extrusion lacks. Both are moot for one whole
        // circle, which has no corner, and for a polygon of lines, whose corners all close by
        // continuing the lines (or by an arc for `Round`): those are built, any other profile
        // is refused.
        let records = records(profile);
        let circle = match records.as_slice() {
            [(CURVE_ARC, r)] if r[11] - r[10] >= std::f64::consts::TAU - 1e-12 => Some(r),
            rs if rs.iter().all(|(k, _)| *k == CURVE_LINE) => None,
            _ => return Err(ERR_UNSUPPORTED),
        };
        let dir = Vec3::new(direction[0], direction[1], direction[2]);
        // The normal the extrusion computes: a circle's axis, a polygon's winding (Newell).
        let axis = match circle {
            Some(r) => Vec3::new(r[3], r[4], r[5]),
            None => records.iter().try_fold(Vec3::ZERO, |n, (_, r)| {
                let (a, b) = (point(r)?, point(&r[3..])?);
                Ok::<_, i32>(n + Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y)))
            })?,
        };
        if !(axis.length() > 0.0) {
            return Err(ERR_ARG);
        }
        if !(draft.abs() < std::f64::consts::FRAC_PI_2 - 1e-6) || !(distance > 1e-9) {
            return Err(ERR_ARG);
        }
        // A draft extrusion runs square to the profile; its height's sign picks the side.
        let along = axis.dot(dir) / (axis.length() * dir.length());
        if along.abs() < 1.0 - 1e-9 {
            return Err(ERR_ARG);
        }
        if circle.is_none() {
            if corner == DRAFT_CORNER_ROUND || !cap {
                return Err(ERR_UNSUPPORTED);
            }
            let ring = records.iter().map(|(_, r)| point(r)).collect::<Result<Vec<_>, i32>>()?;
            let (dir, n) = (dir / dir.length(), axis / axis.length());
            let top = taper_ring(&ring, n, -distance * draft.tan()).ok_or(ERR_UNSUPPORTED)?;
            let lift = dir * distance;
            let top: Vec<Point3> = top.iter().map(|p| *p + lift).collect();
            let mut loops = vec![ring.clone(), top.clone()];
            loops.extend((0..ring.len()).map(|i| {
                let j = (i + 1) % ring.len();
                vec![ring[i], ring[j], top[j], top[i]]
            }));
            return stitched_solid(&loops);
        }
        let mut s = state();
        let cs = curves(&mut s.kernel, profile)?;
        let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
        let height = if along > 0.0 { distance } else { -distance };
        let b = create_taper_extrude(&mut s.kernel, &refs, height, draft.to_degrees(), if cap { 3 } else { 0 }, corner == DRAFT_CORNER_ROUND)
            .map_err(|e| status(&e))?;
        return finish(b, cap);
    }
    let mut s = state();
    let cs = curves(&mut s.kernel, profile)?;
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let b = create_linear_sweep(&mut s.kernel, &refs, Vec3::new(direction[0], direction[1], direction[2]), distance, cap).map_err(|e| status(&e))?;
    finish(b, cap)
}

/// A loft (skin) through the sections, `sizes` curves each; the caller's degree across them.
pub fn loft(sections: &List, sizes: &[i64], degree: i32, cap: bool) -> Result<Handle, i32> {
    if sections.reals().iter().any(|v| !v.is_finite()) {
        return Err(ERR_ARG);
    }
    let degree = usize::try_from(degree).map_err(|_| ERR_ARG)?;
    let mut s = state();
    let cs = curves(&mut s.kernel, sections)?;
    let sizes = sizes.iter().map(|&n| usize::try_from(n).map_err(|_| ERR_ARG)).collect::<Result<Vec<_>, i32>>()?;
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let b = create_skin_primitive(&mut s.kernel, &sizes, &refs, degree, cap).map_err(|e| status(&e))?.brep;
    finish(b, cap)
}

/// `SmApiCreatePlanarFaces`: a new B-rep with the faces bounded by the closed loops of the
/// coplanar `loops`, built on its infinite region with editing on.
pub fn planar_faces(loops: &List) -> Result<Handle, i32> {
    if loops.reals().iter().any(|v| !v.is_finite()) {
        return Err(ERR_ARG);
    }
    let mut s = state();
    let cs = curves(&mut s.kernel, loops)?;
    let mut b = new_brep(&mut s.kernel, tol::USE_DEFAULT, tol::USE_DEFAULT).map_err(|e| status(&e))?;
    b.set_editing_enabled(true);
    let mut notify = ArenaNotifyState::new(&b);
    let mut env = notify.env(None);
    let key_of: &dyn Fn(EntityRef) -> ObjectRef = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv { env: &mut env, marks: &mut marks, this_face: &this_face };
    // The tolerance argument is not used (the B-rep's is).
    let made = create_planar_faces_with_3d_curves(&mut s.kernel, &mut b, None, &cs, 0.01, &mut fenv);
    made.map_err(|e| status(&e))?;
    b.set_editing_enabled(false);
    b.refresh_cached_topology();
    Ok(Handle::new(b))
}

pub fn revolve(profile: &List, origin: &[f64; 3], direction: &[f64; 3], angle: f64, cap: bool) -> Result<Handle, i32> {
    if !(angle > 0.0 && angle <= std::f64::consts::TAU) || origin.iter().chain(direction).any(|v| !v.is_finite()) {
        return Err(ERR_ARG);
    }
    let mut s = state();
    let cs = curves(&mut s.kernel, profile)?;
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let b = create_rotational_sweep(
        &mut s.kernel,
        &refs,
        Point3::new(origin[0], origin[1], origin[2]),
        Vec3::new(direction[0], direction[1], direction[2]),
        angle.to_degrees(),
        cap,
    )
    .map_err(|e| status(&e))?;
    finish(b, cap)
}

/// The point of an arc record (12 numbers) at angle `t`.
fn arc_at(a: &[f64], t: f64) -> Vec3 {
    let (axis, reference) = (Vec3::new(a[3], a[4], a[5]), Vec3::new(a[6], a[7], a[8]));
    let (axis, reference) = (axis / axis.length(), reference / reference.length());
    Vec3::new(a[0], a[1], a[2]) + (reference * t.cos() + axis.cross(reference) * t.sin()) * a[9]
}

/// The curve records of `list`, one slice of numbers each (a NURBS curve's with its kinds).
fn records<'a>(list: &'a List) -> Vec<(i64, &'a [f64])> {
    let (mut kinds, mut reals, mut out) = (list.kinds().iter(), list.reals(), Vec::new());
    while let Some(&kind) = kinds.next() {
        let n = match kind {
            CURVE_LINE => 6,
            CURVE_ARC => 12,
            _ => match (kinds.next(), kinds.next()) {
                (Some(&d), Some(&c)) => (5 * c + d + 1) as usize,
                _ => return out,
            },
        };
        let (now, rest) = reals.split_at(n.min(reals.len()));
        out.push((kind, now));
        reals = rest;
    }
    out
}

/// A path of lines end to end along one direction: the unit direction and the length.
fn straight(path: &[(i64, &[f64])]) -> Option<(Vec3, f64)> {
    let (mut dir, mut length, mut prev) = (Vec3::ZERO, 0.0, Vec3::ZERO);
    for (i, &(kind, r)) in path.iter().enumerate() {
        if kind != CURVE_LINE {
            return None;
        }
        let (a, b) = (Vec3::new(r[0], r[1], r[2]), Vec3::new(r[3], r[4], r[5]));
        let l = (b - a).length();
        if (i > 0 && (a - prev).length() > 1e-9) || !(l > 1e-12) {
            return None;
        }
        let d = (b - a) / l;
        if i == 0 {
            dir = d;
        } else if d.dot(dir) < 1.0 - 1e-12 {
            return None;
        }
        (length, prev) = (length + l, b);
    }
    (!path.is_empty()).then_some((dir, length))
}

/// A path of arcs end to end about one axis: the centre, the unit axis they turn about and the
/// total angle.
fn circular(path: &[(i64, &[f64])]) -> Option<(Vec3, Vec3, f64)> {
    let (mut head, mut angle, mut prev) = (None, 0.0, Vec3::ZERO);
    for &(kind, r) in path {
        if kind != CURVE_ARC {
            return None;
        }
        let (c, k) = (Vec3::new(r[0], r[1], r[2]), Vec3::new(r[3], r[4], r[5]));
        let k = k / k.length();
        let (a, b) = (arc_at(r, r[10]), arc_at(r, r[11]));
        match head {
            None => head = Some((c, k, r[9])),
            Some((c0, k0, r0)) => {
                if (c - c0).length() > 1e-9 || k.dot(k0) < 1.0 - 1e-12 || (r[9] - r0).abs() > 1e-9 || (a - prev).length() > 1e-9 {
                    return None;
                }
            }
        }
        (prev, angle) = (b, angle + r[11] - r[10]);
    }
    head.filter(|_| angle <= std::f64::consts::TAU * (1.0 + 1e-12)).map(|(c, k, _)| (c, k, angle))
}

/// The point a curve record starts at.
fn start_of(kind: i64, r: &[f64]) -> Vec3 {
    match kind {
        CURVE_ARC => arc_at(r, r[10]),
        _ => Vec3::new(r[0], r[1], r[2]),
    }
}

/// The curve as a B-spline, as SMLib's sweep takes its path.
fn bspline(c: Curve) -> Result<BSplineCurve, i32> {
    match c.into_kind() {
        CurveKind::BSpline(b) => Ok(b),
        CurveKind::Line(l) => l.base_with_nurb().map(|b| b.into_owned()).map_err(|e| status(&e)),
        CurveKind::Ellipse(e) => Ok(e.base().clone()),
        _ => Err(ERR_ARG),
    }
}

/// The motion `k` (0 first, 1 middle, 2 last) of a sweep that turns by `angle` about `axis`
/// through `centre`: the columns, then the translation.
fn put_motion(placed: &mut [f64; 36], k: usize, turn: f64, centre: Vec3, axis: Vec3) {
    let turned = |v: Vec3| v * turn.cos() + axis.cross(v) * turn.sin() + axis * (axis.dot(v) * (1.0 - turn.cos()));
    let t = centre - turned(centre);
    let m = placed.get_mut(12 * k..12 * k + 12).unwrap_or_default();
    for (i, e) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
        let c = turned(e);
        m[3 * i..3 * i + 3].copy_from_slice(&[c.x, c.y, c.z]);
    }
    m[9..].copy_from_slice(&[t.x, t.y, t.z]);
}

/// `smlib_sweep` and, with `placed`, `smlib_sweep_placed`. Along a straight path the sweep is an
/// extrusion, about one axis a revolution, as in the native shim (SM_API's linear and rotational
/// sweeps build the walls exactly); any other path is joined into one B-spline for the curve
/// sweep. The curve sweep puts the profile at the path's start as the shim asks it to (the
/// profile keeps its place relative to the start), which solidcore's options express where the
/// profile starts at the start or is round it; any other place is [`ERR_UNSUPPORTED`]. Its
/// motions are rebuilt from the sweep's own frames ([`curve_sweep_motions`]), as a closed path is
/// [`ERR_UNSUPPORTED`] (the shim stitches those).
fn sweep_with(profile: &List, path: &List, frame: i32, cap: bool, placed: Option<&mut [f64; 36]>) -> Result<Handle, i32> {
    if !(0..=1).contains(&frame) || path.reals().iter().chain(profile.reals()).any(|v| !v.is_finite()) {
        return Err(ERR_ARG);
    }
    let route = records(path);
    let mut s = state();
    let pr = curves(&mut s.kernel, profile)?;
    let refs: Vec<Option<&Curve>> = pr.iter().map(Some).collect();
    if let Some((dir, length)) = straight(&route) {
        let b = create_linear_sweep(&mut s.kernel, &refs, dir, length, cap).map_err(|e| status(&e))?;
        if let Some(p) = placed {
            for k in 0..3 {
                let t = dir * (length * 0.5 * k as f64);
                p[12 * k..12 * k + 12].copy_from_slice(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, t.x, t.y, t.z]);
            }
        }
        return finish(b, cap);
    }
    if let Some((centre, axis, angle)) = circular(&route).filter(|_| frame == 0) {
        let degrees = angle.to_degrees().min(360.0);
        let b = create_rotational_sweep(&mut s.kernel, &refs, centre, axis, degrees, cap).map_err(|e| status(&e))?;
        if let Some(p) = placed {
            for k in 0..3 {
                put_motion(p, k, degrees.to_radians() * 0.5 * k as f64, centre, axis);
            }
        }
        return finish(b, cap);
    }
    let pc = curves(&mut s.kernel, path)?;
    let first = records(profile);
    let mut parts = pc.into_iter().map(bspline).collect::<Result<Vec<_>, i32>>()?;
    let ends = (parts.first().and_then(|b| b.ends().ok()), parts.last().and_then(|b| b.ends().ok()));
    let (Some(head), Some(tail)) = ends else { return Err(ERR_ARG) };
    let start = Vec3::new(head.start.x, head.start.y, head.start.z);
    if (Vec3::new(tail.end.x, tail.end.y, tail.end.z) - start).length() <= 1e-6 {
        return Err(ERR_UNSUPPORTED);
    }
    let &(pk, pre) = first.first().ok_or(ERR_ARG)?;
    // Where the profile sits against the path's start: its own start there, or its curves whole
    // circles about it (a circle's box is centred on it).
    let circles = first.iter().all(|&(k, r)| {
        k == CURVE_ARC
            && r[11] - r[10] >= std::f64::consts::TAU - 1e-12
            && (Vec3::new(r[0], r[1], r[2]) - Vec3::new(pre[0], pre[1], pre[2])).length() <= 1e-9
    });
    let centred = circles && (Vec3::new(pre[0], pre[1], pre[2]) - start).length() <= 1e-9;
    if !centred && (start_of(pk, pre) - start).length() > 1e-9 {
        return Err(ERR_UNSUPPORTED);
    }
    let joined = if parts.len() == 1 { parts.remove(0) } else { BSplineCurve::create_by_joining(&mut parts, None).map_err(|e| status(&e))? };
    let options = SweepOptions { cap_ends: cap, translational_sweep: frame == 1, move_profile_center_to_path: centred, ..SweepOptions::default() };
    let was = std::mem::replace(&mut s.kernel.fixes.sweep_frame_on_planar_path_normal, true);
    let motions = placed.map(|p| {
        let profiles: Vec<Option<Curve>> = pr.iter().cloned().map(Some).collect();
        curve_sweep_motions(&s.kernel, &profiles, &joined, options, p)
    });
    let motions = motions.transpose();
    let done =
        if motions.is_ok() { Some(create_curve_sweep(&mut s.kernel, &refs, Some(&Curve::from(joined)), None, None, Some(&options))) } else { None };
    s.kernel.fixes.sweep_frame_on_planar_path_normal = was;
    motions?;
    finish(done.ok_or(ERR_ARG)?.map_err(|e| status(&e))?.brep, cap)
}

pub fn sweep_placed(profile: &List, path: &List, frame: i32, cap: bool, placed: &mut [f64; 36]) -> Result<Handle, i32> {
    sweep_with(profile, path, frame, cap, Some(placed))
}

pub fn sweep(profile: &List, path: &List, frame: i32, cap: bool) -> Result<Handle, i32> {
    sweep_with(profile, path, frame, cap, None)
}

/// The rigid motions (12 numbers each: a matrix by columns, then a translation) that carry the
/// profile as given onto its first, a middle and its last section in a curve sweep, as
/// `smlib_sweep_placed` reports them. Solidcore's curve sweep does not return them, so they are
/// rebuilt from what it works from: `prepare_curve_sweep` (the same preparation), and the frame
/// `create_sweep_scale` puts each section in, `x` along the path, `z` carried along it by
/// `create_b_vectors` from the start frame's, `y = z × x`; a translational sweep only moves the
/// profile by the path's displacement. The last section is at the path's end parameter, exactly;
/// the middle one is the path's parameter midpoint, which is all the kernel needs of it (it
/// places probes on the walls there, and any section lies on them). The frames are carried over
/// a thousand steps, where `create_sweep_scale` carries them from the samples it takes of the
/// path, so a section may differ from the surface's own by what that sampling does.
fn curve_sweep_motions(
    k: &Kernel,
    profiles: &[Option<Curve>],
    path: &BSplineCurve,
    options: SweepOptions,
    placed: &mut [f64; 36],
) -> Result<(), i32> {
    const STEPS: usize = 1000;
    let prepared = prepare_curve_sweep(k, profiles, path, None, None, options, true).map_err(|e| status(&e))?;
    let nurb = prepared.path.gw_nurb().ok_or(ERR_ARG)?;
    let ivl = prepared.path.natural_interval();
    let (a, b) = (ivl.min(), ivl.max());
    let (to_origin, rotation) = (prepared.profile_back.invert(), prepared.rotation_back.invert());
    let params: Vec<f64> = (0..=STEPS).map(|i| if i == STEPS { b } else { a + (b - a) * i as f64 / STEPS as f64 }).collect();
    let carried = if options.translational_sweep {
        Vec::new()
    } else {
        let tangent = prepared.path.evaluate(a, 1, true, false).map_err(|e| status(&e))?.get(1).copied().ok_or(ERR_ARG)?;
        let [_, _, z0] = tangent.make_unit_ortho_vectors(None).map_err(|e| status(&e))?;
        create_b_vectors(nurb, z0, &params).map_err(|e| status(&e))?
    };
    let motion = |i: usize| -> Result<[f64; 12], i32> {
        let (o, x) = nurb.eval_tangent(*params.get(i).ok_or(ERR_ARG)?, Side::Left).map_err(|e| status(&e))?;
        let (y, z) = match carried.get(i) {
            Some(&z) => (z.cross(x), z),
            None => (Vec3::ZERO, Vec3::ZERO),
        };
        let at = |v: Vec3| {
            let p = rotation.transform_point(to_origin.transform_point(v));
            let q = if options.translational_sweep { p + o } else { o + x * p.x + y * p.y + z * p.z };
            prepared.path_back.transform_point(prepared.rotation_back.transform_point(q))
        };
        let t = at(Vec3::ZERO);
        let col = |e: Vec3| at(e) - t;
        let (cx, cy, cz) = (col(Vec3::X), col(Vec3::Y), col(Vec3::Z));
        Ok([cx.x, cx.y, cx.z, cy.x, cy.y, cy.z, cz.x, cz.y, cz.z, t.x, t.y, t.z])
    };
    for (slot, i) in [0, STEPS / 2, STEPS].into_iter().enumerate() {
        placed.get_mut(12 * slot..12 * slot + 12).ok_or(ERR_ARG)?.copy_from_slice(&motion(i)?);
    }
    Ok(())
}

/// A pyramid: the base is the planar closed loop of lines `section` (in order), a triangle runs
/// from each line to `apex`, as `create_pyramid` builds its faces and stitches them into a solid.
/// A curved or open base, or an apex in the base's plane, is an argument error.
pub fn loft_to_point(section: &List, apex: &[f64; 3]) -> Result<Handle, i32> {
    if section.reals().iter().chain(apex).any(|v| !v.is_finite()) || section.kinds().iter().any(|&k| k != CURVE_LINE) {
        return Err(ERR_ARG);
    }
    let lines = records(section);
    let ends: Vec<(Point3, Point3)> = lines.iter().map(|(_, r)| Ok((point(r)?, point(&r[3..])?))).collect::<Result<_, i32>>()?;
    let scale = ends.iter().fold(0.0_f64, |m, (a, b)| m.max(a.length()).max(b.length())).max(apex.iter().fold(0.0_f64, |m, v| m.max(v.abs())));
    let closed = ends.len() >= 3 && ends.iter().zip(ends.iter().cycle().skip(1)).all(|((_, e), (s, _))| (*e - *s).length() <= 1e-9 * scale.max(1.0));
    if !closed {
        return Err(ERR_ARG);
    }
    // Newell's normal of the loop; every corner and not the apex must lie in its plane.
    let (mut n, mut o) = (Vec3::ZERO, Vec3::ZERO);
    for (a, b) in &ends {
        n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
        o += Vec3::new(a.x, a.y, a.z);
    }
    let len = n.length();
    let o = o / ends.len() as f64;
    let eps = 1e-9 * scale.max(1.0);
    if !(len > 0.0) || ends.iter().any(|(a, _)| (Vec3::new(a.x, a.y, a.z) - o).dot(n / len).abs() > eps) {
        return Err(ERR_ARG);
    }
    let top = Point3::new(apex[0], apex[1], apex[2]);
    if (Vec3::new(top.x, top.y, top.z) - o).dot(n / len).abs() <= eps {
        return Err(ERR_ARG);
    }
    let mut loops = vec![ends.iter().map(|(a, _)| *a).collect::<Vec<_>>()];
    loops.extend(ends.iter().map(|(a, b)| vec![*a, *b, top]));
    stitched_solid(&loops)
}

/// A solid from planar polygons (corner lists, closed): a planar face on each, stitched and oriented.
fn stitched_solid(loops: &[Vec<Point3>]) -> Result<Handle, i32> {
    let mut s = state();
    let mut b = new_brep(&mut s.kernel, tol::USE_DEFAULT, tol::USE_DEFAULT).map_err(|e| status(&e))?;
    let region = b.infinite_region();
    let mut notify = ArenaNotifyState::new(&b);
    let mut env = notify.env(None);
    let key_of: &dyn Fn(EntityRef) -> ObjectRef = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv { env: &mut env, marks: &mut marks, this_face: &this_face };
    for corners in loops {
        let lines = (0..corners.len())
            .map(|i| {
                let (a, b) = (corners.get(i).ok_or(ERR_ARG)?, corners.get((i + 1) % corners.len()).ok_or(ERR_ARG)?);
                Ok(Curve::from(create_line_segment(*a, *b).map_err(|e| status(&e))?))
            })
            .collect::<Result<Vec<_>, i32>>()?;
        create_planar_face_with_3d_curves(&mut s.kernel, &mut b, region, &lines, 1.0e-5, &mut fenv).map_err(|e| status(&e))?;
    }
    stitch_and_orient(&mut s.kernel, &mut b, &StitchAndOrientOptions::default()).map_err(|e| status(&e))?;
    b.refresh_cached_topology();
    finish(b, true)
}

/// The polygon `ring` (in order, normal `n` by its winding) offset by `d` in its plane, outward when
/// positive, each corner moved to where the offset edges cross. `None` where an edge would
/// collapse or turn round (an inward offset past what the shape allows) or a corner is a reversal.
fn taper_ring(ring: &[Point3], n: Vec3, d: f64) -> Option<Vec<Point3>> {
    let m = ring.len();
    let edge = |i: usize| -> Option<Vec3> {
        let (a, b) = (ring.get(i)?, ring.get((i + 1) % m)?);
        let e = Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        (e.length() > 0.0).then(|| e / e.length())
    };
    let out = |i: usize| edge(i).map(|e| e.cross(n));
    let mut top = Vec::with_capacity(m);
    for i in 0..m {
        let (o0, o1) = (out((i + m - 1) % m)?, out(i)?);
        let k = 1.0 + o0.dot(o1);
        if k < 1e-9 {
            return None;
        }
        let (p, s) = (ring.get(i)?, (o0 + o1) * (d / k));
        top.push(Point3::new(p.x + s.x, p.y + s.y, p.z + s.z));
    }
    // Every offset edge must still run the way its edge does.
    for i in 0..m {
        let (a, b) = (top.get(i)?, top.get((i + 1) % m)?);
        let e = Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        if e.dot(edge(i)?) <= 1e-9 {
            return None;
        }
    }
    Some(top)
}

/// `v` turned by `angle` about the unit `axis` (Rodrigues).
fn turned(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

/// The offset at turn `t` of the piecewise linear `law` of (turn, offset) pairs, held at its ends.
fn offset_at(law: &[f64], t: f64) -> f64 {
    let pairs: Vec<(f64, f64)> = law.as_chunks::<2>().0.iter().map(|&[t, o]| (t, o)).collect();
    let (Some(&(t0, o0)), Some(&(t1, o1))) = (pairs.first(), pairs.last()) else { return 0.0 };
    if t <= t0 {
        return o0;
    }
    if t >= t1 {
        return o1;
    }
    pairs.windows(2).find(|w| t <= w[1].0).map_or(o1, |w| w[0].1 + (w[1].1 - w[0].1) * (t - w[0].0) / (w[1].0 - w[0].0))
}

/// Screw sweep: the profile placed at stations along its screw motion, skinned through them
/// (cubic across) and capped. The stations are close enough that the cubic skin stays about
/// `tolerance` from the true helicoidal walls (a cubic's error over a step `d` of a circle of
/// radius `R` is about `R d^4 / 384`), between 2 and 15 degrees apart. `law` is the radial offset
/// per turn of [`screw_sweep_radial`](super::sweeps::screw_sweep_radial), empty for none.
fn screw(profile: &List, origin: &[f64; 3], direction: &[f64; 3], pitch: f64, turns: f64, law: &[f64], tolerance: f64) -> Result<Handle, i32> {
    let axis_len = Vec3::new(direction[0], direction[1], direction[2]).length();
    if profile.reals().iter().chain(origin).chain(direction).any(|v| !v.is_finite()) || !(axis_len > 1e-12) {
        return Err(ERR_ARG);
    }
    if !(pitch.is_finite() && pitch != 0.0 && turns.is_finite() && turns > 0.0 && tolerance.is_finite() && tolerance > 0.0) {
        return Err(ERR_ARG);
    }
    let pairs: Vec<(f64, f64)> = law.as_chunks::<2>().0.iter().map(|&[t, o]| (t, o)).collect();
    if pairs.iter().any(|&(t, o)| !(t >= 0.0 && t <= turns && o.is_finite())) || pairs.windows(2).any(|w| !(w[0].0 < w[1].0)) {
        return Err(ERR_ARG);
    }
    let o = Point3::new(origin[0], origin[1], origin[2]);
    let axis = Vec3::new(direction[0], direction[1], direction[2]) / axis_len;
    let rel = |p: Vec3| p - Vec3::new(o.x, o.y, o.z);
    // Points that bound the profile's distance from the axis and give its half-plane's radial.
    let mut marks: Vec<Vec3> = Vec::new();
    let mut reach = 0.0_f64;
    for (kind, r) in records(profile) {
        match kind {
            CURVE_LINE => marks.extend([vector(r)?, vector(&r[3..])?]),
            CURVE_ARC => {
                let c = vector(r)?;
                marks.push(c);
                reach = reach.max(r[9].abs());
            }
            // ponytail: a NURBS curve's first control point only; a long one may reach further.
            _ => marks.push(vector(r)?),
        }
    }
    let radial_of = |p: Vec3| {
        let d = rel(p);
        d - axis * axis.dot(d)
    };
    let radial = marks.iter().map(|&p| radial_of(p)).find(|d| d.length() > 1e-9).map(|d| d / d.length()).unwrap_or(Vec3::ZERO);
    let reach = reach + marks.iter().map(|&p| radial_of(p).length()).fold(0.0, f64::max);
    let total = turns * std::f64::consts::TAU;
    let step = (384.0 * tolerance / reach.max(1e-9)).powf(0.25).clamp(2.0_f64.to_radians(), 15.0_f64.to_radians());
    let n = ((total / step).ceil() as usize).clamp(2, 4096);
    let mut s = state();
    let base = curves(&mut s.kernel, profile)?;
    let mut all: Vec<Curve> = Vec::with_capacity(base.len() * (n + 1));
    for i in 0..=n {
        let f = i as f64 / n as f64;
        let (angle, t) = (total * f, turns * f);
        let (shift, lift) = (radial * offset_at(law, t), axis * (pitch * t));
        // Where the motion sends the origin and the unit vectors (the shift comes first, then the turn).
        let at = |p: Vec3| {
            let q = turned(rel(p) + shift, axis, angle) + lift;
            Vec3::new(o.x, o.y, o.z) + q
        };
        let zero = at(Vec3::ZERO);
        let placement = Axis2Placement::new(Point3::new(zero.x, zero.y, zero.z), at(Vec3::X) - zero, at(Vec3::Y) - zero);
        for c in &base {
            let mut c = c.clone();
            transform_independent_object(&mut s.kernel, Some(TransformObject::Curve(&mut c)), &placement, None).map_err(|e| status(&e))?;
            all.push(c);
        }
    }
    let refs: Vec<Option<&Curve>> = all.iter().map(Some).collect();
    let sizes = vec![base.len(); n + 1];
    let b = create_skin_primitive(&mut s.kernel, &sizes, &refs, 3, true).map_err(|e| status(&e))?.brep;
    finish(b, true)
}

pub fn screw_sweep(profile: &List, origin: &[f64; 3], direction: &[f64; 3], pitch: f64, turns: f64, tolerance: f64) -> Result<Handle, i32> {
    screw(profile, origin, direction, pitch, turns, &[], tolerance)
}

/// `law` holds (turn, offset) pairs.
pub fn screw_sweep_radial(
    profile: &List,
    origin: &[f64; 3],
    direction: &[f64; 3],
    pitch: f64,
    turns: f64,
    law: &[f64],
    tolerance: f64,
) -> Result<Handle, i32> {
    if !law.len().is_multiple_of(2) {
        return Err(ERR_ARG);
    }
    screw(profile, origin, direction, pitch, turns, law, tolerance)
}
