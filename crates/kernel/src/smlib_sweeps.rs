//! Extrusions, revolutions, sweeps, lofts and screw sweeps through SMLib: the profile's
//! segments go to SMLib as 3D curves (lines, circular arcs, Béziers and conics as NURBS), SMLib
//! sweeps them, and the result comes back through its schema 4 record as a kernel body. A result
//! SMLib's own check refuses is an error, never a body. An option SMLib has no sweep for is "not
//! supported yet".
//!
//! A sweep that twists, scales, keeps an orientation direction or follows guide rails, and a
//! loft that follows guide rails, place the profile at stations by the laws the kernel's own
//! sweeps and lofts define ([`crate::sweep_path`], [`crate::loft_rails`]); each section is the
//! profile's exact image, and SMLib skins the walls through them.
//!
//! History is in the builders' vocabulary ([`FaceOrigin::Profile`], [`FaceOrigin::Cap`]), found
//! from the result's geometry ([`crate::provenance`] does the same for operations without exact
//! history): a wall comes from the profile segments whose curves its edge at the start runs
//! along (a wall SMLib makes over several tangent segments comes from all of them), and from
//! those whose middles it holds half-way along the sweep where that is known (extrusions,
//! revolutions); the face in the start plane is the start cap, the one in the end plane (or,
//! where the sweep has no end plane, the planar face left over) the end cap.

use solidcraft_geom::{Plane, Region2, Seg2, Vec3};

use crate::body::Body;
use crate::numbered::{NumberedLoop, NumberedRegion};
use crate::{KernelError, Result};

/// A profile segment as SMLib gets it, with the profile segments it stands for.
pub(crate) struct Piece {
    pub seg: Seg2,
    pub numbers: Vec<usize>,
    /// The segment as drawn that names it: itself, or for collinear lines joined into one, the
    /// first of them.
    pub drawn: Seg2,
}

/// The segments of `loops` as pieces, loop after loop.
pub(crate) fn pieces<'a>(loops: impl IntoIterator<Item = &'a NumberedLoop>) -> Vec<Piece> {
    loops.into_iter().flat_map(|l| l.lp.segs.iter().enumerate().map(|(i, s)| Piece { seg: *s, numbers: l.numbers(i).to_vec(), drawn: *s })).collect()
}

/// The pieces of `joined` (`drawn` with collinear lines joined): outer loop, then holes, each
/// joined line named by the first line drawn under it.
pub(crate) fn region_pieces(joined: &NumberedRegion, drawn: &NumberedRegion) -> Vec<Piece> {
    let firsts: Vec<Seg2> = drawn.loops().flat_map(|l| l.lp.segs.iter().copied()).collect();
    pieces(joined.loops())
        .into_iter()
        .map(|mut x| {
            if x.numbers.len() > 1
                && let Some(first) = firsts.iter().find(|d| d.start() == x.seg.start())
            {
                x.drawn = *first;
            }
            x
        })
        .collect()
}

fn not_supported(what: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {what}"))
}

/// How an extrusion's walls lean.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Draft {
    /// Radians; positive grows the profile (as [`crate::extrude_tapered`] takes it).
    pub taper: f64,
    /// A height (less than the distance) the walls first stand straight for, before they slope
    /// on to the same far end ([`crate::extrude_tapered_between`]'s foot).
    pub foot: f64,
    /// How the sharp corners close.
    pub corner: crate::DraftCorner,
}

pub(crate) use imp::{
    body_from_record, curve, extrude, face_section, loft, loft_rails, loft_to_point, path_curve, project_onto_face, revolve, screw, screw_radial, sphere, sweep,
    sweep_controlled,
};

/// Refuses what no SMLib sweep does, before any work.
pub(crate) fn refuse(what: &str) -> KernelError {
    not_supported(&format!("SMLib {what}"))
}

mod imp {
    use std::collections::BTreeSet;

    use solidcraft_smlib::sweeps::{Affine, Curve, DraftCorner, Ends, SweepFrame};
    use solidcraft_smlib::{Brep, CheckLevel};

    use solidcraft_geom::Vec2;

    use super::*;
    use crate::history::FaceProv;
    use crate::provenance::{closest_on_tri, face_tris};
    use crate::record;

    fn failed(what: &str, e: impl std::fmt::Display) -> KernelError {
        KernelError::Failed(format!("SMLib {what}: {e}"))
    }

    fn corner(c: crate::DraftCorner) -> DraftCorner {
        match c {
            crate::DraftCorner::PreserveCurvature => DraftCorner::Curvature,
            crate::DraftCorner::Tangent => DraftCorner::Tangent,
            crate::DraftCorner::Round => DraftCorner::Round,
        }
    }

    fn p(v: Vec3) -> [f64; 3] {
        [v.x, v.y, v.z]
    }

    /// The segment in 3D on `plane`, as SMLib's curve.
    pub(crate) fn curve(plane: &Plane, s: &Seg2) -> Result<Curve> {
        let w = |q: Vec2| p(plane.to_world(q));
        let n = plane.x.cross(plane.y).normalized().ok_or_else(|| KernelError::Invalid("profile plane".into()))?;
        let bezier = |pts: &[Vec2], weights: &[f64]| {
            let degree = pts.len().saturating_sub(1);
            let points = pts.iter().zip(weights).map(|(q, wt)| {
                let [x, y, z] = w(*q);
                [x, y, z, *wt]
            });
            let knots = std::iter::repeat_n(0.0, degree + 1).chain(std::iter::repeat_n(1.0, degree + 1)).collect();
            Curve::Nurbs { degree, points: points.collect(), knots }
        };
        Ok(match *s {
            Seg2::Line { a, b } => Curve::Line { start: w(a), end: w(b) },
            Seg2::Arc { center, radius, start, sweep } => {
                // A full circle re-split for a loft may come out a rounding over one turn.
                if !(sweep.abs() <= std::f64::consts::TAU * (1.0 + 1e-9)) {
                    return Err(KernelError::Invalid("an arc turns more than once".into()));
                }
                let sweep = sweep.clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
                // Clockwise in the plane is counter-clockwise about the other normal, where a
                // plane angle t is the angle -t.
                let (axis, a0) = if sweep >= 0.0 { (n, start) } else { (-n, -start) };
                Curve::Arc { center: w(center), axis: p(axis), reference: p(plane.x), radius, start: a0, end: a0 + sweep.abs() }
            }
            Seg2::Cubic { p0, p1, p2, p3 } => bezier(&[p0, p1, p2, p3], &[1.0; 4]),
            Seg2::Conic { a, apex, b, w: wt } => bezier(&[a, apex, b], &[1.0, wt, 1.0]),
            Seg2::Bezier { n: degree, p: pts } => {
                let pts = pts.get(..=usize::from(degree)).ok_or_else(|| KernelError::Invalid("Bézier degree".into()))?;
                bezier(pts, &vec![1.0; pts.len()])
            }
        })
    }

    fn curves(plane: &Plane, pieces: &[Piece]) -> Result<Vec<Curve>> {
        pieces.iter().map(|x| curve(plane, &x.seg)).collect()
    }

    /// A chain of sketch segments on `plane` as SMLib's curves, one each, exactly (lines, arcs,
    /// Béziers and conics as NURBS): what SMLib's sweeps take as a profile or a path.
    #[allow(dead_code)] // For the sheet builders, which still build their own wires.
    pub(crate) fn chain_curves(plane: &Plane, segs: &[Seg2]) -> Result<Vec<Curve>> {
        segs.iter().map(|s| curve(plane, s)).collect()
    }

    /// The kernel body of an SMLib result SMLib's own check finds valid.
    fn body_of(brep: &Brep, what: &str) -> Result<Body> {
        let check = brep.check(CheckLevel::All).map_err(|e| failed(what, e))?;
        if !check.is_valid() {
            return Err(failed(what, format!("SMLib's check refuses the result: {check}")));
        }
        crate::native::body_from_smlib(brep)
    }

    /// The body (a solid or a sheet) a schema 4 record describes, taken by SMLib as a
    /// modeller's own record (no healing) and checked there.
    pub(crate) fn body_from_record(record: &solidcraft_geom::brep::Brep, what: &str) -> Result<Body> {
        let brep = Brep::from_record(record, true).map_err(|e| failed(what, e))?;
        body_of(&brep, what)
    }

    /// The sketch segments `segs` on `plane` projected along `direction` onto the face of
    /// `body` at `face_at`: SMLib's projection (smlib_project_curves), each projected curve the
    /// B-spline SMLib made, as a record curve on its interval, within a millionth of the body's
    /// size of the true projection.
    pub(crate) fn project_onto_face(
        body: &Body,
        face_at: Vec3,
        plane: &Plane,
        segs: &[Seg2],
        direction: Vec3,
    ) -> Result<Vec<(solidcraft_geom::brep::Curve, [f64; 2])>> {
        let c = segs.iter().map(|s| curve(plane, s)).collect::<Result<Vec<_>>>()?;
        let out = body
            .smlib()?
            .project_curves(p(face_at), pick_reach(body), &c, p(direction), curve_tolerance(body))
            .map_err(|e| curves_error("curve projection", e))?;
        record_curves(out, "curve projection")
    }

    /// The face of `body` at `face_at` cut by the plane through `plane_point` square to `normal`,
    /// SMLib's section curves on that face, as record curves on their intervals.
    pub(crate) fn face_section(body: &Body, face_at: Vec3, plane_point: Vec3, normal: Vec3) -> Result<Vec<(solidcraft_geom::brep::Curve, [f64; 2])>> {
        let out = body
            .smlib()?
            .face_section(p(face_at), pick_reach(body), p(plane_point), p(normal), curve_tolerance(body))
            .map_err(|e| curves_error("face section", e))?;
        record_curves(out, "face section")
    }

    /// How far a point may be from the face it picks: the kernel's picking reach.
    fn pick_reach(body: &Body) -> f64 {
        body.size().max(1e-9) * 1e-3 + 1e-6
    }

    /// How close a curve SMLib makes on a body's faces keeps to the true one: a millionth of the
    /// body's size.
    fn curve_tolerance(body: &Body) -> f64 {
        (body.size() * 1e-6).max(1e-6)
    }

    fn curves_error(what: &str, e: solidcraft_smlib::Error) -> KernelError {
        match e {
            solidcraft_smlib::Error::Empty => KernelError::Invalid(format!("{what}: no curve on the face")),
            solidcraft_smlib::Error::NoMatch { .. } => KernelError::Invalid(format!("{what}: no face of the body at the point")),
            e => failed(what, e),
        }
    }

    /// SMLib's NURBS curves as record curves (rational splines) on their intervals.
    fn record_curves(out: Vec<(Curve, [f64; 2])>, what: &str) -> Result<Vec<(solidcraft_geom::brep::Curve, [f64; 2])>> {
        out.into_iter()
            .map(|(c, interval)| match c {
                Curve::Nurbs { points, knots, .. } => {
                    let homogeneous = points.iter().map(|q| [q[0] * q[3], q[1] * q[3], q[2] * q[3], q[3]]).collect();
                    Ok((solidcraft_geom::brep::Curve::RationalSpline { knots, homogeneous }, interval))
                }
                _ => Err(failed(what, "a curve SMLib made is not a B-spline")),
            })
            .collect()
    }

    /// SMLib's sphere about `center`, each face swept from the arc (segment 0) of the half disc
    /// a sphere revolves.
    pub(crate) fn sphere(center: Vec3, radius: f64) -> Result<Body> {
        let brep = Brep::make_sphere(p(center), radius).map_err(|e| failed("sphere", e))?;
        let body = body_of(&brep, "sphere")?;
        let faces = body.face_count();
        Ok(record::attach(body, &[], Some(vec![record::profile(&[0usize]); faces]), &[]))
    }

    /// A point a wall is known by: on its edge in the start plane, or inside it; and the piece
    /// (by index) that sweeps it.
    struct Seed {
        at: Vec3,
        piece: usize,
    }

    fn seeds(plane: &Plane, pieces: &[Piece], to: impl Fn(Vec3) -> Vec3) -> Vec<Seed> {
        pieces.iter().enumerate().map(|(piece, x)| Seed { at: to(plane.to_world(x.seg.point_at(0.5))), piece }).collect()
    }

    /// What a face of a swept body is.
    #[derive(Clone, Debug, PartialEq)]
    enum Role {
        Start,
        End,
        /// A wall, swept from these pieces.
        Wall(BTreeSet<usize>),
        Unknown,
    }

    /// The role of each face of `body` swept from the profile in `start` (see the module notes):
    /// `edges` lie on the walls' edges in the start plane, `inside` inside the walls; the face in
    /// `end` (or, with `end` unset and `rest_is_end`, a planar face nothing else claims) is the
    /// end cap.
    fn roles(body: &Body, start: &Plane, end: Option<&Plane>, edges: &[Seed], inside: &[Seed], rest_is_end: bool) -> Result<Vec<Role>> {
        let size = body.size().max(1e-9);
        let mesh_tol = (size * 2e-4).max(1e-6);
        let m = body.mesh(&crate::MeshTolerance::chord(mesh_tol))?;
        let faces = face_tris(&m);
        let n = body.face_count();
        let flat_tol = size * 1e-5 + 1e-9;
        let near = 3.0 * mesh_tol + flat_tol;
        let flat = |f: usize, pl: &Plane| {
            faces.get(f).is_some_and(|ft| !ft.tris.is_empty() && ft.tris.iter().all(|(t, _)| t.iter().all(|q| pl.height(*q).abs() <= flat_tol)))
        };
        let planar =
            |f: usize| faces.get(f).is_some_and(|ft| ft.tris.first().is_some_and(|(_, n0)| ft.tris.iter().all(|(_, nn)| nn.dot(*n0) > 1.0 - 1e-6)));
        // A face in the start plane is the start cap where the profile bounds it (a screw's far
        // end after whole turns lies in the same plane, elsewhere).
        let touches = |f: usize| {
            edges.is_empty()
                || faces.get(f).is_some_and(|ft| edges.iter().any(|s| ft.tris.iter().any(|(t, _)| closest_on_tri(s.at, *t).dist(s.at) <= near)))
        };
        let start_cap: Vec<bool> = (0..n).map(|f| flat(f, start) && touches(f)).collect();
        let end_cap: Vec<bool> = (0..n).map(|f| end.is_some_and(|e| flat(f, e))).collect();
        let cap = |f: usize| start_cap.get(f).copied().unwrap_or(false) || end_cap.get(f).copied().unwrap_or(false);
        let mut swept: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
        for (poly, fs) in m.edges.iter().zip(&m.edge_faces) {
            if poly.len() < 2 || !poly.iter().all(|q| start.height(*q).abs() <= flat_tol) {
                continue;
            }
            for s in edges {
                if poly.windows(2).any(|w| s.at.dist_to_segment(w[0], w[1]) <= near) {
                    for f in fs.iter().map(|f| *f as usize).filter(|f| !cap(*f)) {
                        if let Some(set) = swept.get_mut(f) {
                            set.insert(s.piece);
                        }
                    }
                }
            }
        }
        // Inside the walls: the face nearest each point (bounded work).
        let tris: usize = faces.iter().map(|f| f.tris.len()).sum();
        if tris.saturating_mul(inside.len()) <= 50_000_000 {
            for s in inside {
                let mut best = (f64::INFINITY, usize::MAX);
                for (fi, ft) in faces.iter().enumerate() {
                    if !ft.bounds.inflated(best.0.min(size)).contains(s.at) {
                        continue;
                    }
                    for (t, _) in &ft.tris {
                        let d = closest_on_tri(s.at, *t).dist(s.at);
                        if d < best.0 {
                            best = (d, fi);
                        }
                    }
                }
                if best.0 <= 5.0 * near
                    && !cap(best.1)
                    && let Some(set) = swept.get_mut(best.1)
                {
                    set.insert(s.piece);
                }
            }
        }
        Ok(swept
            .into_iter()
            .enumerate()
            .map(|(f, set)| {
                if start_cap.get(f).copied().unwrap_or(false) {
                    Role::Start
                } else if end_cap.get(f).copied().unwrap_or(false) {
                    Role::End
                } else if !set.is_empty() {
                    Role::Wall(set)
                } else if end.is_none() && rest_is_end && planar(f) {
                    Role::End
                } else {
                    log::debug!(target: "smlib", "sweep history: face {f} comes from no profile segment");
                    Role::Unknown
                }
            })
            .collect())
    }

    /// The history the roles give: caps, and walls from their pieces' profile segments.
    fn history(roles: &[Role], pieces: &[Piece]) -> Vec<FaceProv> {
        roles
            .iter()
            .map(|r| match r {
                Role::Start => record::cap(false),
                Role::End => record::cap(true),
                Role::Wall(set) => record::profile(set.iter().filter_map(|i| pieces.get(*i)).flat_map(|x| x.numbers.iter())),
                Role::Unknown => record::generated(Vec::new()),
            })
            .collect()
    }

    fn with_history(body: Body, roles: &[Role], pieces: &[Piece]) -> Result<Body> {
        Ok(record::attach(body, &[], Some(history(roles, pieces)), &[]))
    }

    /// The profile `pieces` on `plane`, moved `from` along its normal, extruded `distance`
    /// along `sign` (±1) times the normal, straight or drafted.
    pub(crate) fn extrude(
        plane: &Plane,
        pieces: &[Piece],
        region: Option<&Region2>,
        from: f64,
        distance: f64,
        sign: f64,
        draft: Option<Draft>,
    ) -> Result<Body> {
        let base = plane.offset(from);
        let n = plane.x.cross(plane.y).normalized().ok_or_else(|| KernelError::Invalid("profile plane".into()))?;
        let dir = n * sign.signum();
        let c = curves(&base, pieces)?;
        let top = plane.offset(from + sign.signum() * distance);
        let edges = seeds(&base, pieces, |q| q);
        let foot = draft.map_or(0.0, |d| d.foot);
        if let Some(d) = draft.filter(|d| d.foot > 0.0) {
            // A straight foot, then the walls sloping from its top to the far end the taper gives
            // from the base, joined face to face where they meet.
            if !(foot < distance) {
                return Err(KernelError::Invalid(format!("taper foot = {foot}")));
            }
            let knee = plane.offset(from + sign.signum() * foot);
            let lean = -(distance * d.taper.tan() / (distance - foot)).atan();
            let prism = Brep::extrude(&c, p(dir), foot, 0.0, DraftCorner::default(), true).map_err(|e| failed("extrude", e))?;
            let slope =
                Brep::extrude(&curves(&knee, pieces)?, p(dir), distance - foot, lean, corner(d.corner), true).map_err(|e| failed("extrude", e))?;
            let brep = prism.boolean(&slope, solidcraft_smlib::BooleanOp::Union).map_err(|e| failed("extrude", e))?;
            let brep = brep.ok_or_else(|| failed("extrude", "nothing is left"))?;
            let body = body_of(&brep, "extrude")?;
            // The caps and the foot's walls from the base; the sloping walls (and the foot's again)
            // from the edges at the knee, which both stand on.
            let low = roles(&body, &base, Some(&top), &edges, &[], false)?;
            let high = roles(&body, &knee, Some(&top), &seeds(&knee, pieces, |q| q), &[], false)?;
            let roles: Vec<Role> = low
                .into_iter()
                .zip(high)
                .map(|(a, b)| match (a, b) {
                    (Role::Wall(mut x), Role::Wall(y)) => {
                        x.extend(y);
                        Role::Wall(x)
                    }
                    (Role::Unknown, w @ Role::Wall(_)) => w,
                    (a, _) => a,
                })
                .collect();
            return with_history(body, &roles, pieces);
        }
        // SMLib's draft is positive inward; the kernel's taper grows the profile.
        let lean = draft.map_or(0.0, |d| -d.taper);
        let rule = draft.map_or(DraftCorner::default(), |d| corner(d.corner));
        let brep = Brep::extrude(&c, p(dir), distance, lean, rule, true).map_err(|e| failed("extrude", e))?;
        let body = body_of(&brep, "extrude")?;
        // Half-way along a straight wall lies over its segment; a drafted one has moved.
        let inside = if draft.is_none() { seeds(&base, pieces, |q| q + dir * (distance * 0.5)) } else { Vec::new() };
        let roles = roles(&body, &base, Some(&top), &edges, &inside, false)?;
        let sources = (draft.is_none() && sign > 0.0).then(|| extrude_sources(&roles, pieces, region, plane, &base, &top));
        let body = body.with_face_sources(sources.map(std::sync::Arc::new));
        with_history(body, &roles, pieces)
    }

    /// The construction sources of a straight extrusion's faces along the normal of `sketch`, as
    /// [`crate::extrude`] gives them: each cap the region at its end (both caps found once
    /// each), each wall swept from one piece that piece as drawn, a line's with the wall's plane.
    fn extrude_sources(
        roles: &[Role],
        pieces: &[Piece],
        region: Option<&Region2>,
        sketch: &Plane,
        base: &Plane,
        top: &Plane,
    ) -> Vec<Option<crate::FaceSource>> {
        let once = |role: &Role| roles.iter().filter(|r| *r == role).count() == 1;
        let caps = region.filter(|r| once(&Role::Start) && once(&Role::End) && numbered_segments(r) <= 4096);
        roles
            .iter()
            .map(|role| match role {
                Role::Start => caps.map(|r| crate::FaceSource::ExtrudeCap {
                    high: false,
                    region: r.clone(),
                    plane: Plane { origin: base.origin, x: sketch.x, y: -sketch.y },
                }),
                Role::End => caps.map(|r| crate::FaceSource::ExtrudeCap { high: true, region: r.clone(), plane: *top }),
                Role::Wall(set) if set.len() == 1 => set.first().and_then(|i| pieces.get(*i)).map(|x| {
                    let plane = match x.seg {
                        Seg2::Line { a, b } => (b - a).normalized().and_then(|d| {
                            let foot = a - d * a.dot(d);
                            Plane::new(sketch.to_world(foot), sketch.dir_to_world(d), sketch.normal())
                        }),
                        _ => None,
                    };
                    crate::FaceSource::ExtrudeSide { segment: x.drawn, plane, coalesced: false }
                }),
                _ => None,
            })
            .collect()
    }

    fn numbered_segments(r: &Region2) -> usize {
        r.outer.segs.len() + r.holes.iter().map(|h| h.segs.len()).sum::<usize>()
    }

    /// `v` turned by `a` right-handed about the unit `k` (Rodrigues).
    fn turned(v: Vec3, k: Vec3, a: f64) -> Vec3 {
        v * a.cos() + k.cross(v) * a.sin() + k * (k.dot(v) * (1.0 - a.cos()))
    }

    /// The profile `pieces` on `plane` turned `angle` radians (signed, right-handed about `axis`)
    /// about the axis through `origin` (both in the plane). Segments on the axis sweep nothing.
    pub(crate) fn revolve(plane: &Plane, pieces: &[Piece], origin: Vec3, axis: Vec3, angle: f64) -> Result<Body> {
        let k = axis.normalized().ok_or_else(|| KernelError::Invalid("revolve axis".into()))?;
        let (k, turn) = if angle < 0.0 { (-k, -angle) } else { (k, angle) };
        let turn = turn.min(std::f64::consts::TAU);
        let c = curves(plane, pieces)?;
        let brep = Brep::revolve(&c, p(origin), p(k), turn, true).map_err(|e| failed("revolve", e))?;
        let body = body_of(&brep, "revolve")?;
        let size = body.size().max(1e-9);
        let off_axis = |q: Vec3| {
            let w = q - origin;
            (w - k * w.dot(k)).len() > size * 1e-7
        };
        let walls: Vec<Piece> = pieces
            .iter()
            .filter(|x| [0.0, 0.5, 1.0].iter().any(|t| off_axis(plane.to_world(x.seg.point_at(*t)))))
            .map(|x| Piece { seg: x.seg, numbers: x.numbers.clone(), drawn: x.drawn })
            .collect();
        let about = |q: Vec3, a: f64| origin + turned(q - origin, k, a);
        let full = turn >= std::f64::consts::TAU - 1e-9;
        let edges = seeds(plane, &walls, |q| q);
        let inside = seeds(plane, &walls, |q| about(q, turn * 0.5));
        let end = (!full).then(|| Plane { origin: about(plane.origin, turn), x: turned(plane.x, k, turn), y: turned(plane.y, k, turn) });
        // A full turn has no caps, and its profile plane holds seams only.
        let roles = if full {
            roles(&body, &plane.offset(size * 10.0), None, &[], &inside, false)?
        } else {
            roles(&body, plane, end.as_ref(), &edges, &inside, false)?
        };
        with_history(body, &roles, &walls)
    }

    /// A path segment as SMLib's curve, exactly.
    pub(crate) fn path_curve(s: &crate::PathSeg) -> Result<Curve> {
        Ok(match *s {
            crate::PathSeg::Line { a, b } => Curve::Line { start: p(a), end: p(b) },
            crate::PathSeg::Arc { a, center, axis, angle } => {
                let k = axis.normalized().ok_or_else(|| KernelError::Invalid("path arc axis".into()))?;
                let (k, turn) = if angle < 0.0 { (-k, -angle) } else { (k, angle) };
                let r = a - center;
                let radius = r.len();
                let reference = r.normalized().ok_or_else(|| KernelError::Invalid("path arc radius".into()))?;
                Curve::Arc { center: p(center), axis: p(k), reference: p(reference), radius, start: 0.0, end: turn }
            }
            crate::PathSeg::Curve { plane, seg } => curve(&plane, &seg)?,
            crate::PathSeg::Bezier(b) => {
                let degree = b.degree();
                let points = b.weighted_points().ok_or_else(|| KernelError::Invalid("a path span does not evaluate".into()))?;
                Curve::Nurbs { degree, points, knots: std::iter::repeat_n(0.0, degree + 1).chain(std::iter::repeat_n(1.0, degree + 1)).collect() }
            }
        })
    }

    /// The profile `pieces` on `plane` swept along `path`, turning with it, or only moving along
    /// it when `translate`. SMLib's sweep follows a smooth path; at a corner (the direction
    /// jumping) the path is swept piece by piece ([`cornered`]).
    pub(crate) fn sweep(plane: &Plane, pieces: &[Piece], path: &[crate::PathSeg], translate: bool) -> Result<Body> {
        let runs = smooth_runs(path)?;
        if runs.len() > 1 {
            return cornered(plane, pieces, path, &runs, translate);
        }
        let (brep, _) = swept(plane, pieces, path, translate)?;
        let body = body_of(&brep, "sweep")?;
        let roles = roles(&body, plane, None, &seeds(plane, pieces, |q| q), &[], true)?;
        with_history(body, &roles, pieces)
    }

    /// The sweep of the profile on `plane` along `path`, with the rigid motions SMLib reports
    /// carrying the profile onto its first, middle and last sections.
    fn swept(plane: &Plane, pieces: &[Piece], path: &[crate::PathSeg], translate: bool) -> Result<(Brep, [Affine; 3])> {
        let c = curves(plane, pieces)?;
        let route = path.iter().map(path_curve).collect::<Result<Vec<_>>>()?;
        let frame = if translate { SweepFrame::Translate } else { SweepFrame::Follow };
        Brep::sweep_placed(&c, &route, frame, true).map_err(|e| failed("sweep", e))
    }

    /// `q` moved by the motion `a`.
    fn apply(a: &Affine, q: Vec3) -> Vec3 {
        let col = |i: usize| a.m.get(i).map_or(Vec3::ZERO, |c| Vec3::new(c[0], c[1], c[2]));
        col(0) * q.x + col(1) * q.y + col(2) * q.z + Vec3::new(a.t[0], a.t[1], a.t[2])
    }

    /// `pl` carried by the motion `a` (a rigid motion: its directions turn with it).
    fn placed(pl: &Plane, a: &Affine) -> Plane {
        let o = apply(a, Vec3::ZERO);
        Plane { origin: apply(a, pl.origin), x: apply(a, pl.x) - o, y: apply(a, pl.y) - o }
    }

    /// The path's segments in runs along which its direction carries on (within a thousandth of
    /// a radian across each joint): a new run starts at each corner.
    fn smooth_runs(path: &[crate::PathSeg]) -> Result<Vec<std::ops::Range<usize>>> {
        let ends: Vec<(Vec3, Vec3)> = path
            .iter()
            .map(crate::PathSeg::tangents)
            .collect::<Option<_>>()
            .ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))?;
        let mut runs = Vec::new();
        let mut from = 0;
        for (i, w) in ends.windows(2).enumerate() {
            if w[0].1.dot(w[1].0) < (1e-3f64).cos() {
                runs.push(from..i + 1);
                from = i + 1;
            }
        }
        runs.push(from..path.len());
        Ok(runs)
    }

    /// `pl` carried rigidly by a turn of `angle` about the unit `k` through `center`.
    fn turned_plane(pl: &Plane, center: Vec3, k: Vec3, angle: f64) -> Plane {
        Plane { origin: center + turned(pl.origin - center, k, angle), x: turned(pl.x, k, angle), y: turned(pl.y, k, angle) }
    }

    fn moved_plane(pl: &Plane, by: Vec3) -> Plane {
        Plane { origin: pl.origin + by, ..*pl }
    }

    /// Where `s` starts and ends.
    fn seg_ends(s: &crate::PathSeg) -> Result<(Vec3, Vec3)> {
        Ok(match *s {
            crate::PathSeg::Line { a, b } => (a, b),
            crate::PathSeg::Arc { a, center, axis, angle } => {
                let k = axis.normalized().ok_or_else(|| KernelError::Invalid("path arc axis".into()))?;
                (a, crate::build::rotate_about(a, center, k, angle))
            }
            crate::PathSeg::Curve { plane, seg } => (plane.to_world(seg.start()), plane.to_world(seg.end())),
            crate::PathSeg::Bezier(b) => (b.start(), b.end()),
        })
    }

    /// A box on one side of the plane through `at` square to the unit `normal`: a `size` square
    /// in the plane, `size` deep along `normal`.
    fn half_space(at: Vec3, normal: Vec3, size: f64) -> Result<Brep> {
        let u = if normal.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let u = normal.cross(u).normalized().ok_or_else(|| KernelError::Invalid("mitre plane".into()))?;
        let v = normal.cross(u);
        let h = size * 0.5;
        let c = [at + (u + v) * h, at + (v - u) * h, at - (u + v) * h, at + (u - v) * h];
        let square: Vec<Curve> = (0..4).map(|i| Curve::Line { start: p(c[i]), end: p(c[(i + 1) % 4]) }).collect();
        Brep::extrude(&square, p(normal), size, 0.0, DraftCorner::default(), true).map_err(|e| failed("mitre", e))
    }

    fn combined(a: &Brep, b: &Brep, op: solidcraft_smlib::BooleanOp, what: &str) -> Result<Brep> {
        a.boolean(b, op).map_err(|e| failed(what, e))?.ok_or_else(|| failed(what, "nothing is left"))
    }

    /// A sweep along a path with corners: each smooth run of it swept on its own, and the runs
    /// joined at each corner. Turning with the path, the profile turns at a corner about the
    /// line square to both directions, and the runs meet in a mitre: each run is swept on
    /// straight past the corner (along its direction there) and cut back to the plane that halves
    /// the corner, where the next run, cut back the same way, meets it face to face. Only moving
    /// along the path, the profile is the same at the corner in both runs, which meet there.
    ///
    /// History: the start cap is the face in the profile's plane, the end cap the face in its
    /// plane at the path's end; each wall comes from the profile segment it is swept from, found
    /// on the first run's start or half-way along each run.
    fn cornered(plane: &Plane, pieces: &[Piece], path: &[crate::PathSeg], runs: &[std::ops::Range<usize>], translate: bool) -> Result<Body> {
        let start = path.first().map(seg_ends).transpose()?.map(|(a, _)| a).ok_or_else(|| KernelError::Invalid("empty path".into()))?;
        // How far the profile reaches from the path.
        let reach = pieces
            .iter()
            .flat_map(|x| (0..=8).map(move |i| x.seg.point_at(f64::from(i) / 8.0)))
            .map(|q| plane.to_world(q).dist(start))
            .fold(0.0f64, f64::max);
        let mut length = 0.0;
        for s in path {
            let (a, b) = seg_ends(s)?;
            length += a.dist(b).max(match *s {
                crate::PathSeg::Arc { a, center, angle, .. } => (a - center).len() * angle.abs(),
                crate::PathSeg::Curve { seg, .. } => seg.length(),
                crate::PathSeg::Bezier(_) => seg_len(s),
                crate::PathSeg::Line { .. } => 0.0,
            });
        }
        let size = 4.0 * (length + reach) + 1.0;
        let tangent = |s: Option<&crate::PathSeg>| {
            s.and_then(crate::PathSeg::tangents).ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))
        };
        let (first_t, _) = tangent(path.first())?;
        let (_, last_t) = tangent(path.last())?;
        let end = path.last().map(seg_ends).transpose()?.map(|(_, b)| b).ok_or_else(|| KernelError::Invalid("empty path".into()))?;
        // A closed path ends where it starts, with no caps; at a corner there the last run and the
        // first meet in a mitre too.
        let closed = end.dist(start) <= 1e-6;
        let closing = (closed && last_t.dot(first_t) < (1e-3f64).cos()).then_some(first_t);
        let mut pose = *plane;
        let mut body: Option<Brep> = None;
        let mut inside = Vec::new();
        // The mitre the next run starts from: its point, its normal (along the run) and the turn.
        let mut entry: Option<(Vec3, Vec3, f64)> = match closing {
            Some(_) if !translate => {
                let (m, turn) = mitre_of(last_t, first_t)?;
                Some((start, m, turn))
            }
            _ => None,
        };
        for (ri, run) in runs.iter().enumerate() {
            let segs = path.get(run.clone()).ok_or_else(|| KernelError::Invalid("path run".into()))?;
            let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return Err(KernelError::Invalid("path run".into())) };
            let (t0, _) = first.tangents().ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))?;
            let (_, t1) = last.tangents().ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))?;
            let (a0, _) = seg_ends(first)?;
            let (_, b1) = seg_ends(last)?;
            let next = match runs.get(ri + 1).and_then(|r| path.get(r.start)) {
                Some(s) => Some(tangent(Some(s))?.0),
                None => closing,
            };
            // The plane halving the corner at the run's end, and the turn there.
            let exit = next.map(|t2| mitre_of(t1, t2)).transpose()?;
            // Each piece's sweep says where it left the profile: half-way along it each wall lies
            // over its piece, and at its end (less any straight lead-out past the corner) the
            // profile is where the next run takes it on.
            let (piece, at_end) = if translate {
                let (piece, [_, middle, last]) = swept(&pose, pieces, segs, true)?;
                inside.extend(seeds(&pose, pieces, |q| apply(&middle, q)));
                (piece, placed(&pose, &last))
            } else {
                // On past each mitre: the profile's reach times the tangent of half the turn takes
                // every point of it to the mitre's plane; a little further, so the cut back runs
                // through the walls, never along the run's own end.
                let over = |turn: f64| reach * (turn * 0.5).tan() * 1.1 + reach * 1e-3 + 1e-6;
                let pre = entry.map_or(0.0, |(_, _, turn)| over(turn));
                let post = exit.map(|(_, turn)| over(turn)).unwrap_or(0.0);
                let mut route = Vec::with_capacity(segs.len() + 2);
                if pre > 0.0 {
                    route.push(crate::PathSeg::Line { a: a0 - t0 * pre, b: a0 });
                }
                route.extend_from_slice(segs);
                if post > 0.0 {
                    route.push(crate::PathSeg::Line { a: b1, b: b1 + t1 * post });
                }
                let from = moved_plane(&pose, t0 * -pre);
                let (mut piece, [_, middle, last]) = swept(&from, pieces, &route, false)?;
                inside.extend(seeds(&from, pieces, |q| apply(&middle, q)));
                if let Some((at, toward, _)) = entry {
                    piece = combined(&piece, &half_space(at, -toward, size)?, solidcraft_smlib::BooleanOp::Subtract, "sweep mitre")?;
                }
                if let Some((m, _)) = exit {
                    piece = combined(&piece, &half_space(b1, m, size)?, solidcraft_smlib::BooleanOp::Subtract, "sweep mitre")?;
                }
                (piece, moved_plane(&placed(&from, &last), t1 * -post))
            };
            body = Some(match body {
                None => piece,
                Some(b) => combined(&b, &piece, solidcraft_smlib::BooleanOp::Union, "sweep corner")?,
            });
            // The profile at the run's end, then turned through the corner onto the next run.
            pose = at_end;
            entry = None;
            if let (Some(t2), Some((m, turn)), false) = (next, exit, translate) {
                let k = t1.cross(t2).normalized().ok_or_else(|| KernelError::Invalid("path corner".into()))?;
                pose = turned_plane(&pose, b1, k, turn);
                entry = Some((b1, m, turn));
            }
        }
        let brep = body.ok_or_else(|| KernelError::Invalid("empty path".into()))?;
        let body = body_of(&brep, "sweep")?;
        let roles = roles(&body, plane, (!closed).then_some(&pose), &seeds(plane, pieces, |q| q), &inside, false)?;
        with_history(body, &roles, pieces)
    }

    /// The length of a path segment.
    fn seg_len(s: &crate::PathSeg) -> f64 {
        match *s {
            crate::PathSeg::Line { a, b } => a.dist(b),
            crate::PathSeg::Arc { a, center, angle, .. } => angle.abs() * a.dist(center),
            crate::PathSeg::Curve { seg, .. } => seg.length(),
            crate::PathSeg::Bezier(b) => b.length().unwrap_or(f64::NAN),
        }
    }

    /// The point `f` (0…1) of the way along a path segment, and the unit direction of travel there.
    fn seg_at(s: &crate::PathSeg, f: f64) -> Option<(Vec3, Vec3)> {
        match *s {
            crate::PathSeg::Line { a, b } => Some((a.lerp(b, f), (b - a).normalized()?)),
            crate::PathSeg::Arc { a, center, axis, angle } => {
                let k = axis.normalized()?;
                let q = crate::build::rotate_about(a, center, k, angle * f);
                Some((q, (k.cross(q - center) * angle.signum()).normalized()?))
            }
            // The parameter whose length from the start is that share of the curve's.
            crate::PathSeg::Curve { plane, seg } => {
                let want = seg.length() * f;
                let (mut lo, mut hi) = (0.0f64, 1.0f64);
                for _ in 0..48 {
                    let t = 0.5 * (lo + hi);
                    if seg.split_at(t).0.length() < want {
                        lo = t;
                    } else {
                        hi = t;
                    }
                }
                let t = 0.5 * (lo + hi);
                Some((plane.to_world(seg.point_at(t)), plane.dir_to_world(seg.derivative(t)).normalized()?))
            }
            crate::PathSeg::Bezier(b) => b.along(f),
        }
    }

    /// `n + 1` points evenly spaced by length along `path` (exact on its lines and arcs), with the
    /// unit directions of travel there.
    fn path_samples(path: &[crate::PathSeg], n: usize) -> Result<(Vec<Vec3>, Vec<Vec3>)> {
        let lens: Vec<f64> = path.iter().map(seg_len).collect();
        let total: f64 = lens.iter().sum();
        if !(total > 1e-9 && total.is_finite()) || n == 0 {
            return Err(KernelError::Invalid("the sweep path has no length".into()));
        }
        let bad = || KernelError::Invalid("a path segment has no direction".into());
        let (mut pts, mut tangents) = (Vec::with_capacity(n + 1), Vec::with_capacity(n + 1));
        let (mut k, mut before) = (0usize, 0.0f64);
        for i in 0..=n {
            let at = total * i as f64 / n as f64;
            while k + 1 < path.len() && lens.get(k).is_some_and(|l| before + l < at) {
                before += lens.get(k).copied().unwrap_or(0.0);
                k += 1;
            }
            let (seg, len) = path.get(k).zip(lens.get(k)).ok_or_else(bad)?;
            let f = if *len > 0.0 { ((at - before) / len).clamp(0.0, 1.0) } else { 0.0 };
            let (q, t) = seg_at(seg, f).ok_or_else(bad)?;
            pts.push(q);
            tangents.push(t);
        }
        Ok((pts, tangents))
    }

    /// How far the direction of travel turns along a path segment (radians, all told).
    fn path_turn(s: &crate::PathSeg) -> f64 {
        match *s {
            crate::PathSeg::Line { .. } => 0.0,
            crate::PathSeg::Arc { angle, .. } => angle.abs(),
            crate::PathSeg::Curve { seg, .. } => {
                let pts = seg.polyline(1e-3);
                pts.windows(3)
                    .filter_map(|w| Some(((w[1] - w[0]).normalized()?, (w[2] - w[1]).normalized()?)))
                    .map(|(u, v)| u.cross(v).atan2(u.dot(v)).abs())
                    .sum()
            }
            crate::PathSeg::Bezier(b) => {
                let pts: Vec<Vec3> = std::iter::once(b.start()).chain(b.polyline(1e-3)).collect();
                pts.windows(3)
                    .filter_map(|w| Some(((w[1] - w[0]).normalized()?, (w[2] - w[1]).normalized()?)))
                    .map(|(u, v)| u.cross(v).len().atan2(u.dot(v)))
                    .sum()
            }
        }
    }

    /// Fewest and most stations of a sweep that twists, scales or follows rails.
    const MIN_STATIONS: usize = 16;
    const MAX_STATIONS: usize = 2048;

    /// The profile `region` on `plane` swept along the smooth `path` the way `ctl` asks: twisted,
    /// scaled, kept at an orientation or guided by rails. The profile is placed at stations along
    /// the path by [`crate::sweep_path`]'s laws (the same the polyline sweep uses), each section
    /// exactly the profile's image, and SMLib skins the walls through them. Round a closed path the
    /// last section has to land on the first, and the ends are joined.
    pub(crate) fn sweep_controlled(plane: &Plane, region: &NumberedRegion, path: &[crate::PathSeg], ctl: &crate::SweepControl) -> Result<Body> {
        let runs = smooth_runs(path)?;
        if runs.len() > 1 {
            return cornered_controlled(plane, region, path, &runs, ctl);
        }
        let pieces = pieces(region.loops());
        let c = curves(plane, &pieces)?;
        let start = path.first().map(crate::PathSeg::start).ok_or_else(|| KernelError::Invalid("the path needs 1…1000 segments".into()))?;
        // Each point of the profile turns with the path and the twist, and SMLib's skin is a cubic
        // interpolation through the stations with free ends, whose error near the ends falls only
        // as the square of the step: a square of reach 7.1 twisted a quarter turn over 40 mm is
        // 2e-7 off in volume at 1.4 degree steps and 2e-6 at 2.8 (measured against V = A L). A
        // station per degree of turn.
        let turn: f64 = path.iter().map(path_turn).sum::<f64>() + ctl.twist.abs();
        let step = std::f64::consts::PI / 180.0;
        let rail_points = ctl.rails.iter().map(Vec::len).max().unwrap_or(0);
        let n = ((turn / step).ceil() as usize).max(rail_points.saturating_sub(1)).clamp(MIN_STATIONS, MAX_STATIONS);
        let (pts, tangents) = path_samples(path, n)?;
        let st = crate::sweep_path::controlled_stations(plane, &pts, &tangents, &[], ctl)?;
        let (Some(first), Some(last)) = (st.first(), st.last()) else { return Err(KernelError::Invalid("sweep path".into())) };
        let s0 = first.at;
        let size = pts.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>() + region.outer.lp.signed_area().abs().sqrt();
        let closed = pts.last().is_some_and(|e| e.dist(start) <= 1e-7 * (1.0 + size));
        if closed {
            for l in region.loops() {
                crate::sweep_path::closing_offset(plane, l, &st)?;
            }
        }
        let maps: Vec<Affine> = st
            .iter()
            .map(|s| {
                let [a, b, c] = s.m;
                Affine { m: [p(a), p(b), p(c)], t: p(s.at - s.apply(s0)) }
            })
            .collect();
        let brep = Brep::sweep_stations(&c, &maps, if closed { Ends::Joined } else { Ends::Capped }).map_err(|e| failed("sweep", e))?;
        let body = body_of(&brep, "sweep")?;
        let mid = st.get(st.len() / 2).copied().unwrap_or(*last);
        let inside = seeds(plane, &pieces, |q| mid.at + mid.apply(q - s0));
        let roles = if closed {
            // Round a closed path there are no caps, and the start holds seams only.
            roles(&body, &plane.offset(size * 10.0), None, &[], &inside, false)?
        } else {
            let end = Plane::new(last.at + last.apply(plane.origin - s0), last.apply(plane.x), last.apply(plane.y))
                .ok_or_else(|| KernelError::Invalid("the sweep's end section is degenerate".into()))?;
            roles(&body, plane, Some(&end), &seeds(plane, &pieces, |q| q), &inside, false)?
        };
        with_history(body, &roles, &pieces)
    }

    /// `x` moved by `a`.
    fn affine_apply(a: &Affine, x: Vec3) -> Vec3 {
        let [c0, c1, c2] = a.m;
        let v = |q: [f64; 3]| Vec3::new(q[0], q[1], q[2]);
        v(c0) * x.x + v(c1) * x.y + v(c2) * x.z + v(a.t)
    }

    /// `v` turned by `a`'s linear part.
    fn affine_turn(a: &Affine, v: Vec3) -> Vec3 {
        affine_apply(a, v) - affine_apply(a, Vec3::ZERO)
    }

    /// `a` after `b`: x goes to a(b(x)).
    fn affine_after(a: &Affine, b: &Affine) -> Affine {
        let col = |c: [f64; 3]| p(affine_turn(a, Vec3::new(c[0], c[1], c[2])));
        Affine { m: [col(b.m[0]), col(b.m[1]), col(b.m[2])], t: p(affine_apply(a, Vec3::new(b.t[0], b.t[1], b.t[2]))) }
    }

    /// The rigid turn by `angle` about the unit `k` through `center`, as an affine map.
    fn affine_turned(center: Vec3, k: Vec3, angle: f64) -> Affine {
        let col = |e: Vec3| p(turned(e, k, angle));
        Affine { m: [col(Vec3::X), col(Vec3::Y), col(Vec3::Z)], t: p(center - turned(center, k, angle)) }
    }

    fn affine_moved(a: &Affine, by: Vec3) -> Affine {
        Affine { m: a.m, t: p(Vec3::new(a.t[0], a.t[1], a.t[2]) + by) }
    }

    const IDENTITY: Affine = Affine { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };

    /// A sweep that twists, scales, keeps an orientation direction or follows guide rails along
    /// a path with corners, by the corner model of the plain sweep ([`cornered`]): each smooth run
    /// of the path is swept through stations by the laws a smooth path has (the twist and the
    /// scale running with the length along the whole path, each run going on from the section the
    /// last one ended with), and the runs meet at each corner in a mitre. Turning with the path,
    /// each run is carried on straight past its corners as a prism of its section there (the laws
    /// hold still across a mitre: the two runs' sections meet on the plane halving the corner only
    /// when both are the corner's section), cut back to that plane, and the section turned through
    /// the corner about its binormal; only moving along the path, the runs meet at the corner's
    /// section.
    ///
    /// At a corner an orientation direction turns the section as the corner does, and along the
    /// next run it turns back about the path to the direction's frame, evenly along the run's
    /// length, as a twist. Guide rails must turn with the path: each rail has a corner of its own
    /// at every corner of the path, in the plane halving it and by the same turn, where it is cut
    /// into the runs' rails. Round a closed path the last section has to land on the first: a
    /// twist where the profile maps onto itself; an end scale never does.
    fn cornered_controlled(
        plane: &Plane,
        region: &NumberedRegion,
        path: &[crate::PathSeg],
        runs: &[std::ops::Range<usize>],
        ctl: &crate::SweepControl,
    ) -> Result<Body> {
        let translate = ctl.orientation == crate::SweepOrientation::Parallel;
        let pieces = pieces(region.loops());
        let c = curves(plane, &pieces)?;
        let bad = || KernelError::Invalid("a path segment has no direction".into());
        let start = path.first().map(crate::PathSeg::start).ok_or_else(|| KernelError::Invalid("empty path".into()))?;
        let end = path.last().map(crate::PathSeg::end).ok_or_else(|| KernelError::Invalid("empty path".into()))?;
        let lens: Vec<f64> = path.iter().map(seg_len).collect();
        let total: f64 = lens.iter().sum();
        if !(total > 1e-9 && total.is_finite()) {
            return Err(KernelError::Invalid("the sweep path has no length".into()));
        }
        let tangent = |s: Option<&crate::PathSeg>| s.and_then(crate::PathSeg::tangents).ok_or_else(bad);
        let (first_t, _) = tangent(path.first())?;
        let (_, last_t) = tangent(path.last())?;
        let closed = end.dist(start) <= 1e-6;
        if closed && ctl.scale != 1.0 {
            return Err(KernelError::Invalid(
                "a sweep with an end scale can't go round a closed path: its last section would have to land on its first".into(),
            ));
        }
        let closing = (closed && last_t.dot(first_t) < (1e-3f64).cos()).then_some(first_t);
        let reach = pieces
            .iter()
            .flat_map(|x| (0..=8).map(move |i| x.seg.point_at(f64::from(i) / 8.0)))
            .map(|q| plane.to_world(q).dist(start) * ctl.scale.max(1.0))
            .fold(0.0f64, f64::max);
        let size = 4.0 * (total + reach) + 1.0;
        let law = |u: f64| 1.0 + (ctl.scale - 1.0) * u;
        let n0 = plane.normal().normalized().ok_or_else(|| KernelError::Invalid("profile plane".into()))?;
        // Each run's rails, cut at the corners.
        let rails = run_rails(path, runs, &ctl.rails, closing)?;
        // The map carrying the profile as drawn onto the section where the next run starts, the
        // mitre that run starts from (its point, its normal along the run, its turn) and how far
        // along the path it starts.
        let mut at_start = IDENTITY;
        let mut entry: Option<(Vec3, Vec3, f64)> = match closing {
            Some(_) if !translate => {
                let (m, turn) = mitre_of(last_t, first_t)?;
                Some((start, m, turn))
            }
            _ => None,
        };
        let mut before = 0.0f64;
        let mut body: Option<Brep> = None;
        let mut inside = Vec::new();
        for (ri, run) in runs.iter().enumerate() {
            let segs = path.get(run.clone()).ok_or_else(|| KernelError::Invalid("path run".into()))?;
            let length: f64 = lens.get(run.clone()).ok_or_else(|| KernelError::Invalid("path run".into()))?.iter().sum();
            let (u0, u1) = (before / total, (before + length) / total);
            let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return Err(KernelError::Invalid("path run".into())) };
            let ((t0, _), (_, t1)) = (first.tangents().ok_or_else(bad)?, last.tangents().ok_or_else(bad)?);
            let (a0, b1) = (first.start(), last.end());
            let normal = affine_turn(&at_start, n0).normalized().ok_or_else(|| KernelError::Invalid("the sweep's section is degenerate".into()))?;
            let pose = Plane::from_normal(a0, normal).ok_or_else(|| KernelError::Invalid("the sweep's section is degenerate".into()))?;
            // The run's own laws on its section at the start: the twist over its length (less the
            // turn back to the orientation direction after a corner), the scale from where the
            // last run left it to where this one ends, the rails from corner to corner.
            let back = match ctl.orientation {
                crate::SweepOrientation::Direction(_) if ri > 0 => direction_offset(plane, start, first_t, a0, t0, u0, ctl, &at_start)?,
                _ => 0.0,
            };
            let run_ctl = crate::SweepControl {
                orientation: ctl.orientation,
                twist: ctl.twist * (u1 - u0) - back,
                scale: law(u1) / law(u0),
                rails: rails.get(ri).cloned().unwrap_or_default(),
                rail_fit: ctl.rail_fit,
            };
            // A station per degree the section turns, with the path and the twist.
            let turn: f64 = segs.iter().map(path_turn).sum::<f64>() + run_ctl.twist.abs();
            let n = ((turn / (std::f64::consts::PI / 180.0)).ceil() as usize).clamp(MIN_STATIONS, MAX_STATIONS);
            let (pts, tangents) = path_samples(segs, n)?;
            let st = crate::sweep_path::controlled_stations(&pose, &pts, &tangents, &[], &run_ctl)?;
            let maps: Vec<Affine> = st
                .iter()
                .map(|s| {
                    let [a, b, cc] = s.m;
                    affine_after(&Affine { m: [p(a), p(b), p(cc)], t: p(s.at - s.apply(a0)) }, &at_start)
                })
                .collect();
            let (Some(&first_map), Some(&end_map)) = (maps.first(), maps.last()) else { return Err(KernelError::Invalid("sweep path".into())) };
            let mid = maps.get(maps.len() / 2).copied().unwrap_or(end_map);
            inside.extend(seeds(plane, &pieces, |q| affine_apply(&mid, q)));
            let mut piece = Brep::sweep_stations(&c, &maps, Ends::Capped).map_err(|e| failed("sweep", e))?;
            let next = match runs.get(ri + 1).and_then(|r| path.get(r.start)) {
                Some(s) => Some(tangent(Some(s))?.0),
                None => closing,
            };
            let exit = next.map(|t2| mitre_of(t1, t2)).transpose()?;
            if !translate {
                // On past each mitre, the run's section there held still.
                let over = |turn: f64| reach * (turn * 0.5).tan() * 1.1 + reach * 1e-3 + 1e-6;
                let prism = |map: Affine, toward: Vec3, by: f64| -> Result<Brep> {
                    Brep::sweep_stations(&c, &[map, affine_moved(&map, toward * by)], Ends::Capped).map_err(|e| failed("sweep corner", e))
                };
                if let Some((at, m, turn)) = entry {
                    let lead = prism(affine_moved(&first_map, t0 * -over(turn)), t0, over(turn))?;
                    piece = combined(&piece, &lead, solidcraft_smlib::BooleanOp::Union, "sweep corner")?;
                    piece = combined(&piece, &half_space(at, -m, size)?, solidcraft_smlib::BooleanOp::Subtract, "sweep mitre")?;
                }
                if let Some((m, turn)) = exit {
                    let tail = prism(end_map, t1, over(turn))?;
                    piece = combined(&piece, &tail, solidcraft_smlib::BooleanOp::Union, "sweep corner")?;
                    piece = combined(&piece, &half_space(b1, m, size)?, solidcraft_smlib::BooleanOp::Subtract, "sweep mitre")?;
                }
            }
            body = Some(match body {
                None => piece,
                Some(b) => combined(&b, &piece, solidcraft_smlib::BooleanOp::Union, "sweep corner")?,
            });
            // The section at the run's end, turned through the corner onto the next run.
            at_start = end_map;
            entry = None;
            if let (Some(t2), Some((m, turn)), false) = (next, exit, translate) {
                let k = t1.cross(t2).normalized().ok_or_else(|| KernelError::Invalid("path corner".into()))?;
                at_start = affine_after(&affine_turned(b1, k, turn), &end_map);
                entry = Some((b1, m, turn));
            }
            before += length;
        }
        let brep = body.ok_or_else(|| KernelError::Invalid("empty path".into()))?;
        if closed {
            // Back at the start, the section has to be the profile again (a whole number of its
            // symmetries of twist).
            let to_station = |a: &Affine| {
                let v = |q: [f64; 3]| Vec3::new(q[0], q[1], q[2]);
                crate::sweep_path::Station { at: affine_apply(a, start), m: [v(a.m[0]), v(a.m[1]), v(a.m[2])] }
            };
            for l in region.loops() {
                crate::sweep_path::closing_offset(plane, l, &[to_station(&IDENTITY), to_station(&at_start)])?;
            }
        }
        let body = body_of(&brep, "sweep")?;
        let roles = if closed {
            roles(&body, &plane.offset(size * 10.0), None, &[], &inside, false)?
        } else {
            let end_plane = Plane::new(affine_apply(&at_start, plane.origin), affine_turn(&at_start, plane.x), affine_turn(&at_start, plane.y))
                .ok_or_else(|| KernelError::Invalid("the sweep's end section is degenerate".into()))?;
            roles(&body, plane, Some(&end_plane), &seeds(plane, &pieces, |q| q), &inside, false)?
        };
        with_history(body, &roles, &pieces)
    }

    /// After a corner, how far the section at `a0` (the profile carried there by `at_start`) is
    /// turned about its normal (toward the run's direction `t0`) from where the orientation direction's law puts it
    /// (the law of a smooth path from `start`, leaving along `first_t`, `u0` of the way along):
    /// the turn the next run takes back, as a twist. Radians, right-handed about `t0`.
    #[allow(clippy::too_many_arguments)]
    fn direction_offset(
        plane: &Plane,
        start: Vec3,
        first_t: Vec3,
        a0: Vec3,
        t0: Vec3,
        u0: f64,
        ctl: &crate::SweepControl,
        at_start: &Affine,
    ) -> Result<f64> {
        let law = crate::SweepControl { twist: ctl.twist * u0, scale: 1.0 + (ctl.scale - 1.0) * u0, rails: Vec::new(), ..ctl.clone() };
        // A two-station path from the start to here with the two directions: the law's section here.
        let st = crate::sweep_path::controlled_stations(plane, &[start, a0], &[first_t, t0], &[], &law)?;
        let want = st.last().map(|s| s.apply(plane.x)).ok_or_else(|| KernelError::Invalid("sweep path".into()))?;
        let have = affine_turn(at_start, plane.x);
        // About the section's normal, pointing the way the run leaves it (the twist's axis).
        let n = affine_turn(at_start, plane.normal());
        let axis = (if n.dot(t0) < 0.0 { n * -1.0 } else { n })
            .normalized()
            .ok_or_else(|| KernelError::Invalid("the sweep's section is degenerate".into()))?;
        let flat = |v: Vec3| (v - axis * v.dot(axis)).normalized();
        let (Some(w), Some(h)) = (flat(want), flat(have)) else {
            return Err(KernelError::Invalid("the profile lies along the sweep path at a corner".into()));
        };
        Ok(w.cross(h).dot(axis).atan2(w.dot(h)))
    }

    /// The guide rails cut at the path's corners, one set per run: each rail must turn with the
    /// path there, with a corner of its own in the plane halving the path's corner and by the
    /// same turn about the same axis.
    fn run_rails(
        path: &[crate::PathSeg],
        runs: &[std::ops::Range<usize>],
        rails: &[Vec<Vec3>],
        closing: Option<Vec3>,
    ) -> Result<Vec<Vec<Vec<Vec3>>>> {
        if rails.is_empty() {
            return Ok(vec![Vec::new(); runs.len()]);
        }
        if closing.is_some() {
            return Err(KernelError::Invalid("guide rails can't follow a closed path with corners: a rail runs from the start to the end".into()));
        }
        let refused = |i: usize| {
            KernelError::Invalid(format!(
                "at corner {} of the path the guide rails have to turn with it: each rail needs a corner of its own there, in the plane halving the path's corner and by the same turn",
                i + 1
            ))
        };
        let mut rest: Vec<Vec<Vec3>> = rails.iter().map(|r| crate::sweep_path::clean(r)).collect::<Result<_>>()?;
        let mut out = Vec::with_capacity(runs.len());
        for (ci, w) in runs.windows(2).enumerate() {
            let (Some(a), Some(b)) = (w[0].end.checked_sub(1).and_then(|i| path.get(i)), path.get(w[1].start)) else { return Err(refused(ci)) };
            let ((_, t1), (t2, _)) = (a.tangents().ok_or_else(|| refused(ci))?, b.tangents().ok_or_else(|| refused(ci))?);
            let (m, turn) = mitre_of(t1, t2)?;
            let at = a.end();
            let k = t1.cross(t2).normalized().ok_or_else(|| refused(ci))?;
            let mut here = Vec::with_capacity(rest.len());
            for rail in &mut rest {
                let size = rail.windows(2).map(|q| q[0].dist(q[1])).sum::<f64>().max(1e-9);
                let tol = (size * 1e-6).max(1e-6);
                let corner = (1..rail.len().saturating_sub(1)).find(|&j| {
                    let (Some(p), Some(q), Some(r)) = (rail.get(j - 1), rail.get(j), rail.get(j + 1)) else { return false };
                    let (Some(d1), Some(d2)) = ((*q - *p).normalized(), (*r - *q).normalized()) else { return false };
                    (*q - at).dot(m).abs() <= tol
                        && (d1.dot(d2).clamp(-1.0, 1.0).acos() - turn).abs() <= 1e-3
                        && d1.cross(d2).normalized().is_some_and(|kr| kr.dot(k) > 1.0 - 1e-6)
                });
                let j = corner.ok_or_else(|| refused(ci))?;
                let tail = rail.split_off(j);
                let mut head = std::mem::replace(rail, tail);
                head.extend(rail.first().copied());
                here.push(head);
            }
            out.push(here);
        }
        out.push(rest);
        Ok(out)
    }

    /// The plane halving the corner from direction `t1` to `t2` (its unit normal) and the turn
    /// there; a path that turns back on itself has none.
    fn mitre_of(t1: Vec3, t2: Vec3) -> Result<(Vec3, f64)> {
        let turn = t1.dot(t2).clamp(-1.0, 1.0).acos();
        if turn > std::f64::consts::PI - 1e-3 {
            return Err(KernelError::Invalid("the path turns back on itself at a corner".into()));
        }
        Ok(((t1 + t2).normalized().ok_or_else(|| KernelError::Invalid("path corner".into()))?, turn))
    }

    /// A loft through `sections` (each a closed loop of pieces, all re-split to the same count
    /// and lined up), `first` the index of the section given first (its cap is the start cap).
    pub(crate) fn loft(sections: &[(Plane, Vec<Piece>)], first: usize) -> Result<Body> {
        let curves: Vec<Vec<Curve>> = sections.iter().map(|(pl, ps)| curves(pl, ps)).collect::<Result<_>>()?;
        // Ruled between two sections, else smooth through them (cubic from four).
        let degree = match sections.len() {
            0..=2 => 1,
            3 => 2,
            _ => 3,
        };
        loft_through(sections, first, &curves, degree)
    }

    /// The loft skinned through `curves` (the sections', or with stations between them), `degree`
    /// across them; `sections` and `first` as [`loft`] takes them name the faces.
    fn loft_through(sections: &[(Plane, Vec<Piece>)], first: usize, curves: &[Vec<Curve>], degree: u32) -> Result<Body> {
        let brep = Brep::loft(curves, degree, true).map_err(|e| failed("loft", e))?;
        let body = body_of(&brep, "loft")?;
        let (Some((start, lead)), Some(last)) = (sections.get(first), sections.get(if first == 0 { sections.len().saturating_sub(1) } else { 0 }))
        else {
            return Err(KernelError::Invalid("loft sections".into()));
        };
        // Each wall comes from its column's segments in every section.
        let columns: Vec<Piece> = lead
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let mut all: BTreeSet<usize> = BTreeSet::new();
                for (_, ps) in sections {
                    if let Some(q) = ps.get(i) {
                        all.extend(q.numbers.iter().copied());
                    }
                }
                Piece { seg: x.seg, numbers: all.into_iter().collect(), drawn: x.drawn }
            })
            .collect();
        let roles = roles(&body, start, Some(&last.0), &seeds(start, &columns, |q| q), &[], false)?;
        with_history(body, &roles, &columns)
    }

    /// A loft through `sections` (as [`loft`] takes them) guided by one or two `rails` (polylines
    /// from the first section to the last, through every section's curve), by the law
    /// [`crate::loft_rails::railed`] gives a loft's rails: between each two sections stations are
    /// the plain loft there moved so that the points the rails guide land on the rails. Each
    /// section curve has to pair with the others' (the same kind, so their control points pair
    /// up).
    pub(crate) fn loft_rails(sections: &[(Plane, Vec<Piece>)], first: usize, rails: &[Vec<Vec3>], fit: crate::RailFit) -> Result<Body> {
        if rails.is_empty() || rails.len() > 2 {
            return Err(KernelError::Invalid("a loft takes one or two guide rails".into()));
        }
        let matched = sections
            .iter()
            .map(|(pl, ps)| {
                let segs: Vec<Seg2> = ps.iter().map(|x| x.seg).collect();
                crate::loft_rails::Section::of(pl, &segs, ps.iter().map(|x| x.numbers.clone()).collect())
            })
            .collect::<Result<Vec<_>>>()?;
        let all: Vec<Vec<Curve>> = crate::loft_rails::railed(&matched, true, rails, fit)?
            .iter()
            .map(|section| {
                section
                    .iter()
                    .map(|c| {
                        let points = c.ctrl.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3], h[3]]).collect();
                        Curve::Nurbs { degree: c.degree, points, knots: c.knots.clone() }
                    })
                    .collect()
            })
            .collect();
        loft_through(sections, first, &all, 3)
    }

    /// A loft from the section `pieces` (lines) on `plane` to `apex`: the base is the start cap,
    /// each triangle comes from the segment it stands on.
    pub(crate) fn loft_to_point(plane: &Plane, pieces: &[Piece], apex: Vec3) -> Result<Body> {
        let c = curves(plane, pieces)?;
        let brep = Brep::loft_to_point(&c, p(apex)).map_err(|e| failed("loft to a point", e))?;
        let body = body_of(&brep, "loft to a point")?;
        let roles = roles(&body, plane, None, &seeds(plane, pieces, |q| q), &[], false)?;
        with_history(body, &roles, pieces)
    }

    /// The profile `pieces` on `plane` carried by a screw motion about the axis through `origin`
    /// along `axis`: `turns` right-handed turns, rising `pitch` per turn.
    pub(crate) fn screw(plane: &Plane, pieces: &[Piece], origin: Vec3, axis: Vec3, pitch: f64, turns: f64) -> Result<Body> {
        let c = curves(plane, pieces)?;
        // Walls within a ten-thousandth of the pitch of the true screw surfaces.
        let tolerance = (pitch.abs() * 1e-4).clamp(1e-6, 1e-2);
        let brep = Brep::screw_sweep(&c, p(origin), p(axis), pitch, turns, tolerance).map_err(|e| failed("screw sweep", e))?;
        let body = body_of(&brep, "screw sweep")?;
        let roles = roles(&body, plane, None, &seeds(plane, pieces, |q| q), &[], true)?;
        with_history(body, &roles, pieces)
    }

    /// [`screw`] with the profile moved `offset(t)` mm off the axis at turn t (negative: towards
    /// it), piecewise linear through `law`'s (turn, offset) pairs: a groove fading over a runout.
    pub(crate) fn screw_radial(plane: &Plane, pieces: &[Piece], origin: Vec3, axis: Vec3, pitch: f64, turns: f64, law: &[(f64, f64)]) -> Result<Body> {
        let c = curves(plane, pieces)?;
        let tolerance = (pitch.abs() * 1e-4).clamp(1e-6, 1e-2);
        let brep =
            Brep::screw_sweep_radial(&c, p(origin), p(axis), pitch, turns, law, tolerance).map_err(|e| failed("screw sweep with a radial law", e))?;
        let body = body_of(&brep, "screw sweep")?;
        let roles = roles(&body, plane, None, &seeds(plane, pieces, |q| q), &[], true)?;
        with_history(body, &roles, pieces)
    }
}
