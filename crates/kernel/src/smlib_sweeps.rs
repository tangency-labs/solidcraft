//! Extrusions, revolutions, sweeps, lofts and screw sweeps through SMLib ([`crate::backend`]
//! picks them): the profile's segments go to SMLib as 3D curves (lines, circular arcs, Béziers
//! and conics as NURBS), SMLib sweeps them, and the result comes back through its schema 4
//! record as a kernel body. A result SMLib's own check refuses is an error, never a body. An
//! option SMLib has no sweep for is "not supported yet", never truck's sweep in its place.
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
pub(crate) use imp::{extrude, loft, revolve, screw, sweep};

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
}
#[cfg(not(feature = "smlib"))]
pub(crate) use stubs::{extrude, loft, revolve, screw, sweep};

/// Refuses what no SMLib sweep does, before any work.
pub(crate) fn refuse(what: &str) -> KernelError {
    not_supported(&format!("SMLib {what}"))
}

#[cfg(feature = "smlib")]
mod imp {
    use std::collections::BTreeSet;

    use solidcraft_smlib::sweeps::{Curve, SweepFrame};
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
        let brep = Brep::loft(&curves, degree, true).map_err(|e| failed("loft", e))?;
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
