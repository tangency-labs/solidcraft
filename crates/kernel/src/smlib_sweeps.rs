//! Extrusions, revolutions, sweeps, lofts and screw sweeps through SMLib ([`crate::backend`]
//! picks them): the profile's segments go to SMLib as 3D curves (lines, circular arcs, Béziers
//! and conics as NURBS), SMLib sweeps them, and the result comes back through its schema 4
//! record as a kernel body. A result SMLib's own check refuses is an error, never a body. An
//! option SMLib has no sweep for is "not supported yet", never truck's sweep in its place.
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
#[cfg_attr(not(feature = "smlib"), allow(dead_code))]
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
#[cfg_attr(not(feature = "smlib"), allow(dead_code))]
pub(crate) struct Draft {
    /// Radians; positive grows the profile (as [`crate::extrude_tapered`] takes it).
    pub taper: f64,
}

#[cfg(feature = "smlib")]
pub(crate) use imp::{extrude, loft, loft_rails, revolve, screw, sweep, sweep_controlled};

#[cfg(not(feature = "smlib"))]
mod stubs {
    use super::*;

    fn not_built() -> KernelError {
        not_supported("this build has no SMLib (feature `smlib`)")
    }
    pub(crate) fn extrude(_: &Plane, _: &[Piece], _: Option<&Region2>, _: f64, _: f64, _: f64, _: Option<Draft>) -> Result<Body> {
        Err(not_built())
    }
    pub(crate) fn revolve(_: &Plane, _: &[Piece], _: Vec3, _: Vec3, _: f64) -> Result<Body> {
        Err(not_built())
    }
    pub(crate) fn sweep(_: &Plane, _: &[Piece], _: &[crate::PathSeg], _: bool) -> Result<Body> {
        Err(not_built())
    }
    pub(crate) fn loft(_: &[(Plane, Vec<Piece>)], _: usize) -> Result<Body> {
        Err(not_built())
    }
    pub(crate) fn screw(_: &Plane, _: &[Piece], _: Vec3, _: Vec3, _: f64, _: f64) -> Result<Body> {
        Err(not_built())
    }
    pub(crate) fn sweep_controlled(_: &Plane, _: &NumberedRegion, _: &[crate::PathSeg], _: &crate::SweepControl) -> Result<Body> {
        Err(not_built())
    }
    pub(crate) fn loft_rails(_: &[(Plane, Vec<Piece>)], _: usize, _: &[Vec<Vec3>], _: crate::RailFit) -> Result<Body> {
        Err(not_built())
    }
}
#[cfg(not(feature = "smlib"))]
pub(crate) use stubs::{extrude, loft, loft_rails, revolve, screw, sweep, sweep_controlled};

/// Refuses what no SMLib sweep does, before any work.
pub(crate) fn refuse(what: &str) -> KernelError {
    not_supported(&format!("SMLib {what}"))
}

#[cfg(feature = "smlib")]
mod imp {
    use std::collections::BTreeSet;

    use solidcraft_smlib::sweeps::{Affine, Curve, Ends, SweepFrame};
    use solidcraft_smlib::{Brep, CheckLevel};

    use solidcraft_geom::Vec2;

    use super::*;
    use crate::history::FaceProv;
    use crate::provenance::{closest_on_tri, face_tris};
    use crate::record;

    fn failed(what: &str, e: impl std::fmt::Display) -> KernelError {
        KernelError::Failed(format!("SMLib {what}: {e}"))
    }

    fn p(v: Vec3) -> [f64; 3] {
        [v.x, v.y, v.z]
    }

    /// The segment in 3D on `plane`, as SMLib's curve.
    fn curve(plane: &Plane, s: &Seg2) -> Result<Curve> {
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

    /// The kernel body of an SMLib result SMLib's own check finds valid.
    fn body_of(brep: &Brep, what: &str) -> Result<Body> {
        let check = brep.check(CheckLevel::All).map_err(|e| failed(what, e))?;
        if !check.is_valid() {
            return Err(failed(what, format!("SMLib's check refuses the result: {check}")));
        }
        crate::native::body_from_smlib(brep)
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
        let m = body.tessellate(mesh_tol)?;
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
        record::attach(body, &[], Some(history(roles, pieces)), Default::default())
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
        // SMLib's draft is positive inward; the kernel's taper grows the profile.
        let lean = draft.map_or(0.0, |d| -d.taper);
        let brep = Brep::extrude(&c, p(dir), distance, lean, true).map_err(|e| failed("extrude", e))?;
        let body = body_of(&brep, "extrude")?;
        let edges = seeds(&base, pieces, |q| q);
        // Half-way along a straight wall lies over its segment; a drafted one has moved.
        let inside = if draft.is_none() { seeds(&base, pieces, |q| q + dir * (distance * 0.5)) } else { Vec::new() };
        let top = plane.offset(from + sign.signum() * distance);
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

    fn path_curve(s: &crate::PathSeg) -> Result<Curve> {
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
        })
    }

    /// The profile `pieces` on `plane` swept along `path`, turning with it, or only moving along
    /// it when `translate`.
    pub(crate) fn sweep(plane: &Plane, pieces: &[Piece], path: &[crate::PathSeg], translate: bool) -> Result<Body> {
        let c = curves(plane, pieces)?;
        let route = path.iter().map(path_curve).collect::<Result<Vec<_>>>()?;
        let frame = if translate { SweepFrame::Translate } else { SweepFrame::Follow };
        let brep = Brep::sweep(&c, &route, frame, true).map_err(|e| failed("sweep", e))?;
        let body = body_of(&brep, "sweep")?;
        let roles = roles(&body, plane, None, &seeds(plane, pieces, |q| q), &[], true)?;
        with_history(body, &roles, pieces)
    }

    /// The length of a path segment.
    fn seg_len(s: &crate::PathSeg) -> f64 {
        match *s {
            crate::PathSeg::Line { a, b } => a.dist(b),
            crate::PathSeg::Arc { a, center, angle, .. } => angle.abs() * a.dist(center),
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

    /// Fewest and most stations of a sweep that twists, scales or follows rails.
    const MIN_STATIONS: usize = 16;
    const MAX_STATIONS: usize = 2048;

    /// The profile `region` on `plane` swept along the smooth `path` the way `ctl` asks: twisted,
    /// scaled, kept at an orientation or guided by rails. The profile is placed at stations along
    /// the path by [`crate::sweep_path`]'s laws (the same the polyline sweep uses), each section
    /// exactly the profile's image, and SMLib skins the walls through them. Round a closed path the
    /// last section has to land on the first, and the ends are joined.
    pub(crate) fn sweep_controlled(plane: &Plane, region: &NumberedRegion, path: &[crate::PathSeg], ctl: &crate::SweepControl) -> Result<Body> {
        let pieces = pieces(region.loops());
        let c = curves(plane, &pieces)?;
        let start = match path.first() {
            Some(crate::PathSeg::Line { a, .. } | crate::PathSeg::Arc { a, .. }) => *a,
            None => return Err(KernelError::Invalid("the path needs 1…1000 segments".into())),
        };
        // Each point of the profile turns with the path and the twist, and SMLib's skin is a cubic
        // interpolation through the stations with free ends, whose error near the ends falls only
        // as the square of the step: a square of reach 7.1 twisted a quarter turn over 40 mm is
        // 2e-7 off in volume at 1.4 degree steps and 2e-6 at 2.8 (measured against V = A L). A
        // station per degree of turn.
        let turn: f64 = path.iter().map(|s| if let crate::PathSeg::Arc { angle, .. } = s { angle.abs() } else { 0.0 }).sum::<f64>() + ctl.twist.abs();
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

    /// A curve as a NURBS net: degree, control points (x, y, z) and weights, clamped knots. A line
    /// is of degree 1; an arc is rational quadratic, one span per quarter turn or less.
    struct Net {
        degree: usize,
        points: Vec<Vec3>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    }

    impl Net {
        fn of(c: &Curve) -> Option<Net> {
            let v = |q: [f64; 3]| Vec3::new(q[0], q[1], q[2]);
            Some(match c {
                Curve::Line { start, end } => {
                    Net { degree: 1, points: vec![v(*start), v(*end)], weights: vec![1.0; 2], knots: vec![0.0, 0.0, 1.0, 1.0] }
                }
                Curve::Arc { center, axis, reference, radius, start, end } => {
                    let (c, k, x) = (v(*center), v(*axis), v(*reference));
                    let y = k.cross(x);
                    let sweep = end - start;
                    let m = (sweep / (std::f64::consts::PI / 2.0) - 1e-9).ceil().max(1.0) as usize;
                    let d = sweep / m as f64;
                    let at = |a: f64, r: f64| c + (x * a.cos() + y * a.sin()) * r;
                    let (mut points, mut weights, mut knots) = (vec![at(*start, *radius)], vec![1.0], vec![0.0; 3]);
                    for j in 0..m {
                        let a0 = start + d * j as f64;
                        points.push(at(a0 + d / 2.0, radius / (d / 2.0).cos()));
                        weights.push((d / 2.0).cos());
                        points.push(at(a0 + d, *radius));
                        weights.push(1.0);
                        let u = (j + 1) as f64 / m as f64;
                        knots.extend(if j + 1 < m { vec![u, u] } else { vec![1.0; 3] });
                    }
                    Net { degree: 2, points, weights, knots }
                }
                Curve::Nurbs { degree, points, knots } => Net {
                    degree: *degree,
                    points: points.iter().map(|q| Vec3::new(q[0], q[1], q[2])).collect(),
                    weights: points.iter().map(|q| q[3]).collect(),
                    knots: knots.clone(),
                },
            })
        }

        /// The same curve with other control points.
        fn with(&self, points: Vec<Vec3>) -> Net {
            Net { degree: self.degree, points, weights: self.weights.clone(), knots: self.knots.clone() }
        }

        /// Whether `o` has the same degree, knots and weights (its control points pair up).
        fn pairs_with(&self, o: &Net) -> bool {
            let near = |a: &[f64], b: &[f64]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-9 * (1.0 + x.abs()));
            self.degree == o.degree && self.points.len() == o.points.len() && near(&self.knots, &o.knots) && near(&self.weights, &o.weights)
        }

        fn curve(&self) -> Curve {
            let points = self.points.iter().zip(&self.weights).map(|(q, w)| [q.x, q.y, q.z, *w]).collect();
            Curve::Nurbs { degree: self.degree, points, knots: self.knots.clone() }
        }

        /// The point `f` (0…1) of the way along the parameter range.
        fn at(&self, f: f64) -> Option<Vec3> {
            let ctrl = self.points.iter().zip(&self.weights).map(|(q, w)| [q.x * w, q.y * w, q.z * w, *w]).collect();
            let c = solidcraft_geom::bspline::CurveN::<4> { degree: self.degree, knots: self.knots.clone(), ctrl };
            let (t0, t1) = (*self.knots.first()?, *self.knots.last()?);
            let ([x, y, z, w], _) = c.eval(t0 + (t1 - t0) * f.clamp(0.0, 1.0))?;
            (w.abs() > 1e-300).then(|| Vec3::new(x / w, y / w, z / w))
        }
    }

    /// The point of a section (its curves' nets) nearest `p`: as a wire parameter (the curve's
    /// index plus how far along its parameter range), the point, and how far it is.
    fn nearest_on(section: &[Net], p: Vec3) -> Option<(f64, Vec3, f64)> {
        const SAMPLES: usize = 256;
        let mut best: Option<(f64, Vec3, f64)> = None;
        for (k, c) in section.iter().enumerate() {
            let d = |f: f64| c.at(f).map_or(f64::INFINITY, |q| q.dist(p));
            let Some(i) = (0..=SAMPLES).min_by(|a, b| d(*a as f64 / SAMPLES as f64).total_cmp(&d(*b as f64 / SAMPLES as f64))) else { continue };
            // Golden-section search about the nearest sample.
            let (mut lo, mut hi) = ((i as f64 - 1.0).max(0.0) / SAMPLES as f64, (i as f64 + 1.0).min(SAMPLES as f64) / SAMPLES as f64);
            let g = 0.5 * (5f64.sqrt() - 1.0);
            for _ in 0..80 {
                let (a, b) = (hi - g * (hi - lo), lo + g * (hi - lo));
                if d(a) < d(b) {
                    hi = b;
                } else {
                    lo = a;
                }
            }
            let f = 0.5 * (lo + hi);
            let q = c.at(f)?;
            if best.as_ref().is_none_or(|b| q.dist(p) < b.2) {
                best = Some((k as f64 + f, q, q.dist(p)));
            }
        }
        best
    }

    /// The point at wire parameter `g` of a section.
    fn wire_point(section: &[Net], g: f64) -> Result<Vec3> {
        let k = (g.floor().max(0.0) as usize).min(section.len().saturating_sub(1));
        section.get(k).and_then(|c| c.at(g - k as f64)).ok_or_else(|| KernelError::Failed("loft rails: section".into()))
    }

    fn centroid(section: &[Net]) -> Vec3 {
        let (sum, n) = section.iter().flat_map(|c| &c.points).fold((Vec3::ZERO, 0usize), |(s, n), q| (s + *q, n + 1));
        sum * (1.0 / n.max(1) as f64)
    }

    /// A loft through `sections` (as [`loft`] takes them) guided by one or two `rails` (polylines
    /// from the first section to the last, through every section's curve), by the law
    /// [`crate::loft_rails`] gives a loft's rails: between each two sections stations are the
    /// plain loft there (the sections' control points straight between two, the cubic through
    /// them across more) moved so that the points the rails guide land on the rails. Each section
    /// curve has to pair with the others' (the same kind, so their control points pair up).
    pub(crate) fn loft_rails(sections: &[(Plane, Vec<Piece>)], first: usize, rails: &[Vec<Vec3>], fit: crate::RailFit) -> Result<Body> {
        if rails.is_empty() || rails.len() > 2 {
            return Err(KernelError::Invalid("a loft takes one or two guide rails".into()));
        }
        let fail = |m: &str| KernelError::Failed(format!("loft rails: {m}"));
        let count = sections.len();
        if count < 2 {
            return Err(fail("sections"));
        }
        let curves: Vec<Vec<Curve>> = sections.iter().map(|(pl, ps)| curves(pl, ps)).collect::<Result<_>>()?;
        let nets: Vec<Vec<Net>> = curves
            .iter()
            .map(|cs| cs.iter().map(Net::of).collect::<Option<Vec<_>>>())
            .collect::<Option<_>>()
            .ok_or_else(|| fail("section curves"))?;
        let lead = nets.first().ok_or_else(|| fail("sections"))?;
        let n = lead.len();
        if n == 0 || nets.iter().any(|s| s.len() != n || s.iter().zip(lead).any(|(a, b)| !a.pairs_with(b))) {
            return Err(refuse("loft along guide rails between sections whose curves differ in kind (a line against an arc)"));
        }
        let mut bounds = solidcraft_geom::Aabb3::EMPTY;
        for q in nets.iter().flatten().flat_map(|c| &c.points) {
            bounds.add(*q);
        }
        let size = bounds.diagonal();
        let tol = (size * 1e-4).max(1e-6);
        // The sections' parameters along the loft: in proportion to the mean distance between
        // their curves' starts.
        let mut along = vec![0.0];
        for w in nets.windows(2) {
            let step = w[0].iter().zip(&w[1]).filter_map(|(a, b)| Some(a.points.first()?.dist(*b.points.first()?))).sum::<f64>() / n as f64;
            along.push(along.last().copied().unwrap_or(0.0) + step);
        }
        let total = along.last().copied().unwrap_or(0.0);
        if !(total > 1e-9) || along.windows(2).any(|w| !(w[1] - w[0] > 1e-9 * total)) {
            return Err(KernelError::Invalid("two loft sections sit on top of each other".into()));
        }
        along.iter_mut().for_each(|u| *u /= total);
        // The plain loft at v: each control point straight between two sections, the cubic (or
        // quadratic) through them across more.
        let degree_across = (count - 1).min(3);
        let across: Vec<Vec<solidcraft_geom::bspline::CurveN<3>>> = if count == 2 {
            Vec::new()
        } else {
            (0..n)
                .map(|k| {
                    (0..lead.get(k).map_or(0, |c| c.points.len()))
                        .map(|j| {
                            let pts: Vec<[f64; 3]> = nets.iter().filter_map(|s| s.get(k)?.points.get(j).map(|q| [q.x, q.y, q.z])).collect();
                            solidcraft_geom::bspline::CurveN::interpolate(&along, &pts, degree_across).map_err(|e| fail(&e.to_string()))
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<_>>()?
        };
        let plain = |v: f64| -> Result<Vec<Net>> {
            let (Some(a), Some(b)) = (nets.first(), nets.last()) else { return Err(fail("sections")) };
            if count == 2 {
                return Ok(a.iter().zip(b).map(|(ca, cb)| ca.with(ca.points.iter().zip(&cb.points).map(|(p, q)| p.lerp(*q, v)).collect())).collect());
            }
            a.iter()
                .zip(&across)
                .map(|(c, cs)| {
                    let pts = cs.iter().map(|x| x.eval(v).map(|([px, py, pz], _)| Vec3::new(px, py, pz))).collect::<Option<Vec<_>>>();
                    pts.map(|p| c.with(p)).ok_or_else(|| fail("the plain loft"))
                })
                .collect()
        };
        // The plain loft's middle and its direction there.
        let middle = |v: f64| -> Result<(Vec3, Vec3)> {
            let h = 1e-4;
            let (a, b) = ((v - h).max(0.0), (v + h).min(1.0));
            let d = centroid(&plain(b)?) - centroid(&plain(a)?);
            let d = d.normalized().ok_or_else(|| KernelError::Invalid("two loft sections sit on top of each other".into()))?;
            Ok((centroid(&plain(v)?), d))
        };
        // Each rail from its end nearer the first section's middle, and where it meets each
        // section: the guided point (wire parameter), that point, the nudge onto it from the
        // crossing, and how far along the rail the crossing is.
        let start = centroid(lead);
        type Anchor = (f64, Vec3, Vec3, (usize, f64));
        let mut guided: Vec<(Vec<Vec3>, Vec<Anchor>)> = Vec::with_capacity(rails.len());
        for r in rails {
            let mut pts =
                crate::sweep_path::clean(r).map_err(|_| KernelError::Invalid("a guide rail needs 2…100000 finite points along its length".into()))?;
            if let (Some(a), Some(b)) = (pts.first(), pts.last())
                && b.dist(start) < a.dist(start)
            {
                pts.reverse();
            }
            let mut cursor = (0usize, 0.0f64);
            let mut anchors = Vec::with_capacity(count);
            for (i, ((plane, _), section)) in sections.iter().zip(&nets).enumerate() {
                let Some((hit, at)) = crate::sweep_path::rail_crossing(&pts, plane.origin, plane.normal(), cursor, tol) else {
                    return Err(KernelError::Invalid(format!(
                        "a guide rail does not reach section {}: a rail has to run from the first section to the last, through every one",
                        i + 1
                    )));
                };
                cursor = at;
                let (g, on, d) = nearest_on(section, hit).ok_or_else(|| fail("section"))?;
                if d > tol {
                    return Err(KernelError::Invalid(format!(
                        "a guide rail crosses section {}'s plane {d:.3} mm from its curves: a rail has to pass through every section",
                        i + 1
                    )));
                }
                anchors.push((g, on, on - hit, at));
            }
            guided.push((pts, anchors));
        }
        if let [a, b] = &guided[..]
            && a.1.iter().zip(&b.1).any(|(p, q)| p.1.dist(q.1) <= tol)
        {
            return Err(KernelError::Invalid("the two guide rails meet a section at the same point".into()));
        }
        // The sections' normals, turned to agree with the first's.
        let mut normals: Vec<Vec3> = Vec::with_capacity(count);
        for (p, _) in sections {
            let nn = p.normal();
            let prev = normals.last().copied().unwrap_or(nn);
            normals.push(if nn.dot(prev) < 0.0 { nn * -1.0 } else { nn });
        }
        let spans = count - 1;
        let per = (crate::loft_rails::MAX_WIRES.saturating_sub(count) / spans).clamp(1, crate::loft_rails::MAX_PER_SPAN);
        let mut all: Vec<Vec<Curve>> = Vec::with_capacity(count + spans * per);
        for i in 0..spans {
            all.push(nets.get(i).ok_or_else(|| fail("sections"))?.iter().map(Net::curve).collect());
            let (Some(&v0), Some(&v1), Some(&n0), Some(&n1)) = (along.get(i), along.get(i + 1), normals.get(i), normals.get(i + 1)) else {
                return Err(fail("sections"));
            };
            let mut cursors: Vec<(usize, f64)> = guided.iter().map(|(_, a)| a.get(i).map_or((0, 0.0), |x| x.3)).collect();
            for q in 1..=per {
                let f = q as f64 / (per + 1) as f64;
                let v = v0 + f * (v1 - v0);
                let station = plain(v)?;
                let (mid, dir) = middle(v)?;
                let normal = n0.lerp(n1, f).normalized().ok_or_else(|| fail("section planes"))?;
                let mut pairs = Vec::with_capacity(guided.len());
                for ((pts, anchors), cursor) in guided.iter().zip(cursors.iter_mut()) {
                    let (Some(a0), Some(a1)) = (anchors.get(i), anchors.get(i + 1)) else { return Err(fail("rail")) };
                    let anchor = wire_point(&station, crate::loft_rails::interpolate_g(a0.0, a1.0, f, n, true))?;
                    let Some((hit, at)) = crate::sweep_path::rail_crossing(pts, anchor, normal, *cursor, tol) else {
                        return Err(KernelError::Invalid(format!(
                            "a guide rail turns back between sections {} and {}: it has to run on from one section to the next",
                            i + 1,
                            i + 2
                        )));
                    };
                    *cursor = at;
                    pairs.push((anchor, hit + a0.2 * (1.0 - f) + a1.2 * f));
                }
                let map = crate::loft_rails::station_map(&pairs, mid, dir, fit, size)?;
                all.push(station.iter().map(|c| c.with(c.points.iter().map(|q| map.apply(*q)).collect()).curve()).collect());
            }
        }
        all.push(nets.last().ok_or_else(|| fail("sections"))?.iter().map(Net::curve).collect());
        loft_through(sections, first, &all, 3)
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
}
