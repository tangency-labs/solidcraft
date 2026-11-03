//! The flat shape of a cut through a panel of a sheet made from surfaces whose walls are not
//! square to it (#6748): the region of the panel's parameters whose fibers, square through the
//! sheet's thickness, meet the material the cut took, read from the faces the cut left (its
//! walls), exactly. Its outline is made of:
//!
//! - a wall's edge on the panel's face, where the wall narrows the hole going into the material
//!   (a wall edge-on to the fibers counts as narrowing: its two edges lie on one line of the flat);
//! - a wall's edge on the face one thickness in, where the wall widens it;
//! - a wall's fold, where it turns edge-on to the fibers, followed on its own surface;
//! - the edge between two walls, where one narrows the hole and the other widens it.
//!
//! These meet end to end at corners found exactly on the edges, where a wall's leaning changes
//! along them, so the outline is put together from how its pieces meet, never by joining
//! overlapping shapes: a fold meets an edge tangent to it in the flat, where a union of the two
//! could not tell them apart. An edge piece on flat material is the line or the arc it is,
//! exactly; elsewhere a curve is put through its points.
//!
//! A cut whose walls cross a joint is read the same way in the sheet as a whole ([`Folded`]):
//! each point in whichever piece of the flat holds its fiber (a panel, a tab, or a bend's zone,
//! unwrapped round its axis), its outline drawn in the flat, one curve across tangent lines and
//! seams. Refused: walls that don't run through the thickness from face to face (a countersink, a
//! rounded or chamfered face edge, a recess, a cut that stops inside a bend), walls that reach the
//! sheet's outline, and walls whose shapes in the flat overlap.

use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};

use solidcraft_develop::{Bent, Fold, Rigid2, Rigid3, Surface, curve_through};
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::brep::query::Query;
use solidcraft_geom::brep::{Loop as Bounds, Shell, Surface as Record};
use solidcraft_geom::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::{Body, FaceSurface, face_surface};

/// A wall leaning less than this (the cosine of its normal's angle to the fibers) is edge-on to
/// them: above the rounding of a wall square to them.
const EDGE_ON: f64 = 1e-12;

/// A wall leaning more than this either way leans that way plainly, away from any fold.
const PLAIN: f64 = 1e-6;

/// Points of a fold followed at most.
const MAX_FOLD_POINTS: usize = 20_000;

/// A panel's material where the fold puts it: its surface at `place`, `t` deep.
pub(crate) struct Slab<'a> {
    surface: &'a Surface,
    place: Rigid3,
    from: Rigid3,
    t: f64,
}

/// A point of space in a panel's slab: how deep it lies into the material from the panel's face,
/// the way deeper (the fiber's way, a unit vector), and the panel's parameters its fiber starts
/// from (a roll's angle within half a turn of 0).
#[derive(Clone, Copy)]
pub(crate) struct Depth {
    w: f64,
    deeper: Vec3,
    at: Vec2,
}

/// The sheet's material as a shadow is read in it: for a point of space, how deep it lies into
/// the material, the way deeper, and where its fiber starts in the chart the shadow is drawn in.
pub(crate) trait Fibers {
    fn depth(&self, p: Vec3) -> Option<Depth>;
    /// Whether the chart's second coordinate is an angle round a roll.
    fn angular(&self) -> bool;
    fn thickness(&self) -> f64;
    /// Where the fiber through `p` starts in a plane the chart takes rigidly: which piece of the
    /// sheet's material (a key), the plane's frame in space, and where the chart lays the frame's
    /// `x`, `y`. `None` where the material there is curved.
    fn rigid(&self, p: Vec3) -> Option<(usize, Rigid3, Rigid2)>;
}

fn turned(v: f64, inward: bool) -> f64 {
    if inward { v } else { -v }
}

impl<'a> Slab<'a> {
    pub(crate) fn new(surface: &'a Surface, place: Rigid3, t: f64) -> Slab<'a> {
        Slab { surface, place, from: place.inverse(), t }
    }
}

impl Fibers for Slab<'_> {
    fn depth(&self, p: Vec3) -> Option<Depth> {
        let q = self.from.point(p);
        let side = |inward: bool| if inward { -1.0 } else { 1.0 };
        let rho = q.x.hypot(q.y);
        let radial = Vec3::new(q.x / rho, q.y / rho, 0.0);
        let (w, deeper, at) = match *self.surface {
            Surface::Plane => (q.z, Vec3::Z, Vec2::new(q.x, q.y)),
            Surface::Cylinder { radius, material_inward } => {
                let s = side(material_inward);
                ((rho - radius) * s, radial * s, Vec2::new(q.z, turned(q.y.atan2(q.x), material_inward)))
            }
            Surface::Cone { half_angle, material_inward } => {
                // In the half plane through the axis: along the ruling, and off it.
                let (sin, cos) = half_angle.sin_cos();
                let s = side(material_inward);
                (
                    (rho * cos - q.z * sin) * s,
                    (radial * cos - Vec3::Z * sin) * s,
                    Vec2::new(rho * sin + q.z * cos, turned(q.y.atan2(q.x), material_inward)),
                )
            }
            Surface::Spline(_) => return None,
        };
        (w.is_finite() && deeper.len().is_finite() && at.x.is_finite() && at.y.is_finite()).then(|| Depth {
            w,
            deeper: self.place.vector(deeper),
            at,
        })
    }

    fn angular(&self) -> bool {
        !matches!(self.surface, Surface::Plane)
    }

    fn thickness(&self) -> f64 {
        self.t
    }

    fn rigid(&self, _: Vec3) -> Option<(usize, Rigid3, Rigid2)> {
        matches!(self.surface, Surface::Plane).then_some((0, self.place, Rigid2::IDENTITY))
    }
}

/// What carries a piece of a folded sheet's flat material into space.
pub(crate) enum Carrier<'a> {
    /// Panel `.0`, its surface in the slab `.1`: its own material, or a tab riding with it.
    Panel(usize, Slab<'a>),
    /// A bend's zone.
    Bend(&'a Bent),
    /// The sheet laid out flat, the flat's frame in the slab's plane.
    Flat(Slab<'a>),
}

/// A sheet's material round a cut that crosses its joints, read as one: a point of space is read
/// in whichever piece of the flat holds its fiber (a panel, a tab with its panel, or a bend's
/// zone), and its fiber's start is given in the flat, so that one outline runs on across tangent
/// lines and seams.
pub(crate) struct Folded<'a> {
    /// Each carrier, and the piece's flat material.
    pieces: Vec<(Carrier<'a>, Region2)>,
    fold: Option<&'a Fold>,
    t: f64,
}

impl<'a> Folded<'a> {
    /// The sheet's pieces as `fold` carries them (`None`: laid out flat), `t` thick.
    pub(crate) fn new(pieces: Vec<(Carrier<'a>, Region2)>, fold: Option<&'a Fold>, t: f64) -> Folded<'a> {
        Folded { pieces, fold, t }
    }

    /// The piece that holds the fiber through `p`, and `p`'s depth in it: the piece whose flat
    /// material holds the fiber's start with `p` within the thickness, or else comes nearest.
    pub(crate) fn read(&self, p: Vec3) -> Option<(usize, Depth)> {
        let mut best: Option<(f64, usize, Depth)> = None;
        for (k, (carrier, region)) in self.pieces.iter().enumerate() {
            let got = match carrier {
                Carrier::Panel(i, slab) => slab.depth(p).and_then(|d| Some(Depth { at: self.fold?.flat_point(*i, d.at)?, ..d })),
                Carrier::Bend(bent) => bent.unwrapped(p).map(|(at, w, deeper)| Depth { w, deeper, at }),
                Carrier::Flat(slab) => slab.depth(p),
            };
            let Some(d) = got.filter(|d| d.at.x.is_finite() && d.at.y.is_finite() && d.w.is_finite()) else { continue };
            let off = if region.contains(d.at) { 0.0 } else { outline_distance(region, d.at) };
            let off = off + (-d.w).max(0.0) + (d.w - self.t).max(0.0);
            if best.as_ref().is_none_or(|b| off < b.0) {
                best = Some((off, k, d));
            }
        }
        best.map(|(_, k, d)| (k, d))
    }
}

impl Fibers for Folded<'_> {
    fn depth(&self, p: Vec3) -> Option<Depth> {
        self.read(p).map(|(_, d)| d)
    }

    fn angular(&self) -> bool {
        false
    }

    fn thickness(&self) -> f64 {
        self.t
    }

    fn rigid(&self, p: Vec3) -> Option<(usize, Rigid3, Rigid2)> {
        let (k, _) = self.read(p)?;
        match &self.pieces.get(k)?.0 {
            Carrier::Panel(i, slab) if matches!(slab.surface, Surface::Plane) => Some((k, slab.place, self.fold?.placement(*i)?)),
            Carrier::Flat(slab) => Some((k, slab.place, Rigid2::IDENTITY)),
            _ => None,
        }
    }
}

/// How far (mm) an edge's points may lie off the line or the circle it is taken for: a rounding.
const ON_FORM: f64 = 1e-9;

/// The flat segments of an edge's piece from its parameter `a` to `b`, where it lies in the plane
/// of `frame`, which the chart takes rigidly by `place`: a line or a circular arc where the piece
/// is one (its points within [`ON_FORM`] of it, whatever form its record holds it in), else none.
fn exact(curve: &CurveEval, a: f64, b: f64, frame: &Rigid3, place: &Rigid2) -> Vec<Seg2> {
    const N: usize = 16;
    let from = frame.inverse();
    let pts: Option<Vec<Vec2>> = (0..=N)
        .map(|k| {
            let q = from.point(curve.point(a + (b - a) * k as f64 / N as f64)?);
            Some(place.point(Vec2::new(q.x, q.y)))
        })
        .collect();
    let Some(pts) = pts else { return Vec::new() };
    let (s, e) = (pts[0], pts[N]);
    let on = ON_FORM + 1e-12 * s.dist(e);
    let off_line = |p: Vec2| {
        let d = e - s;
        let t = if d.len2() > 0.0 { ((p - s).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
        p.dist(s + d * t)
    };
    if s.dist(e) > on && pts.iter().all(|p| off_line(*p) <= on) {
        return vec![Seg2::Line { a: s, b: e }];
    }
    // The circle through three points a third of the piece apart.
    let (p, q, r) = (pts[0], pts[N / 3], pts[2 * N / 3]);
    let (u, v) = (q - p, r - p);
    let den = 2.0 * u.cross(v);
    if !(den.abs() > 1e-12) {
        return Vec::new();
    }
    let center = p + Vec2::new(v.y * u.len2() - u.y * v.len2(), u.x * v.len2() - v.x * u.len2()) * (1.0 / den);
    let radius = p.dist(center);
    if !pts.iter().all(|x| (x.dist(center) - radius).abs() <= on) {
        return Vec::new();
    }
    let angle = |x: Vec2| (x - center).y.atan2((x - center).x);
    let sweep: f64 = pts.windows(2).map(|w| (angle(w[1]) - angle(w[0]) + PI).rem_euclid(TAU) - PI).sum();
    // Past a half turn, in halves: an arc's ends stay well apart.
    let n = if sweep.abs() > PI { 2 } else { 1 };
    (0..n).map(|k| Seg2::Arc { center, radius, start: angle(s) + sweep * k as f64 / n as f64, sweep: sweep / n as f64 }).collect()
}

/// `walls` in groups that meet each other along edges: each the walls of one cut through the
/// sheet.
pub(crate) fn components(q: &Query, walls: &[usize]) -> Vec<Vec<usize>> {
    fn root(group: &mut [usize], mut i: usize) -> usize {
        while group[i] != i {
            group[i] = group[group[i]];
            i = group[i];
        }
        i
    }
    let mut group: Vec<usize> = (0..walls.len()).collect();
    for (i, &f) in walls.iter().enumerate() {
        for l in &q.record().faces[f].loops {
            let Bounds::Edges { edges } = l else { continue };
            for oe in edges {
                for g in q.edge_faces(oe.edge as usize) {
                    if let Some(j) = walls.iter().position(|w| *w == g && g != f) {
                        let (a, b) = (root(&mut group, i), root(&mut group, j));
                        group[a] = b;
                    }
                }
            }
        }
    }
    let mut out: Vec<(usize, Vec<usize>)> = Vec::new();
    for (i, &f) in walls.iter().enumerate() {
        let r = root(&mut group, i);
        match out.iter_mut().find(|(k, _)| *k == r) {
            Some((_, list)) => list.push(f),
            None => out.push((r, vec![f])),
        }
    }
    out.into_iter().map(|(_, list)| list).collect()
}

/// Which of `folded`'s pieces the edges of `walls` lie in, by their places in its list.
pub(crate) fn reached(q: &Query, folded: &Folded, walls: &[usize], sag: f64) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for &f in walls {
        for l in &q.record().faces[f].loops {
            let Bounds::Edges { edges } = l else { continue };
            for oe in edges {
                for (_, p) in q.edge_points(oe.edge as usize, sag).unwrap_or_default() {
                    if let Some((k, _)) = folded.read(p) {
                        out.insert(k);
                    }
                }
            }
        }
    }
    out
}

/// Whether faces `a` and `b` lie on one surface within `tol`: a cut leaves the faces it didn't
/// make on the surfaces they had.
fn same_surface(a: (Option<FaceSurface>, &Record), b: (Option<FaceSurface>, &Record), tol: f64) -> bool {
    let parallel = |x: Vec3, y: Vec3| x.cross(y).len() <= 1e-9;
    let off_line = |p: Vec3, o: Vec3, d: Vec3| ((p - o) - d * (p - o).dot(d)).len();
    match (a.0, b.0) {
        (Some(FaceSurface::Plane { normal: n, point: p }), Some(FaceSurface::Plane { normal: m, point: q })) => {
            parallel(*n, *m) && (q - p).dot(*n).abs() <= tol
        }
        (Some(FaceSurface::Cylinder { origin: o, axis: d, radius: r }), Some(FaceSurface::Cylinder { origin: p, axis: e, radius: s })) => {
            parallel(*d, *e) && (r - s).abs() <= tol && off_line(p, o, *d) <= tol
        }
        (Some(FaceSurface::Cone { apex: o, axis: d, half_angle: h }), Some(FaceSurface::Cone { apex: p, axis: e, half_angle: k })) => {
            d.dot(*e) > 0.0 && parallel(*d, *e) && (h - k).abs() <= 1e-9 && o.dist(p) <= tol
        }
        (Some(FaceSurface::Sphere { center: o, radius: r }), Some(FaceSurface::Sphere { center: p, radius: s })) => {
            o.dist(p) <= tol && (r - s).abs() <= tol
        }
        (
            Some(FaceSurface::Torus { center: o, axis: d, major: r, minor: m }),
            Some(FaceSurface::Torus { center: p, axis: e, major: s, minor: n }),
        ) => parallel(*d, *e) && o.dist(p) <= tol && (r - s).abs() <= tol && (m - n).abs() <= tol,
        (None, None) => a.1 == b.1,
        _ => false,
    }
}

/// The faces of `now` a cut made: those on no surface of `was`.
pub(crate) fn walls(was: &Body, now: &Body, tol: f64) -> Vec<usize> {
    let (ra, rb) = (was.record(), now.record());
    let old: Vec<(Option<FaceSurface>, &Record)> = (0..ra.faces.len()).map(|f| (face_surface(was, f), &ra.faces[f].surface)).collect();
    (0..rb.faces.len())
        .filter(|f| {
            let here = (face_surface(now, *f), &rb.faces[*f].surface);
            !old.iter().any(|o| same_surface(here, *o, tol))
        })
        .collect()
}

/// What an edge of a wall is to the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// On the panel's face (`far` false) or the face one thickness in, bounding this wall.
    Rim { far: bool, wall: usize },
    /// Between two walls.
    Between(usize, usize),
}

impl Kind {
    fn walls(&self) -> Vec<usize> {
        match *self {
            Kind::Rim { wall, .. } => vec![wall],
            Kind::Between(a, b) => vec![a, b],
        }
    }

    /// Whether a piece of the edge where its walls widen the hole as `wide` says lies on the
    /// shadow's outline.
    fn outline(&self, wide: &[bool]) -> bool {
        match (*self, wide) {
            (Kind::Rim { far, .. }, [w]) => *w == far,
            (Kind::Between(..), [a, b]) => a != b,
            _ => false,
        }
    }
}

/// An edge of the walls: its curve, its interval, and the corners found on it (each with the
/// wall whose leaning changes there).
struct Edge {
    e: usize,
    kind: Kind,
    curve: CurveEval,
    span: [f64; 2],
    corners: Vec<(f64, usize)>,
}

/// Where a fold ends.
enum End {
    /// At the corner of this index among those it was looking for.
    Known(usize),
    /// At a corner it found: on this edge (of the list), at this parameter.
    Found(usize, f64),
}

/// A wall's surface as its producer names it (a sphere's own, in closed form, not its NURBS
/// form, whose pole a projection can't settle near), and whether its natural normal turns
/// against the wall's out of the material.
struct Wall {
    surface: SurfaceEval,
    sign: f64,
}

/// Reads one panel's shadow from the body a cut left.
struct Reader<'a> {
    q: &'a Query<'a>,
    slab: &'a dyn Fibers,
    surfaces: Vec<Option<Wall>>,
    /// Points this near are one.
    tol: f64,
    /// Edges and folds are read within this of their chords.
    sag: f64,
    /// How far a fold steps at most.
    step: f64,
    name: &'a str,
}

impl Reader<'_> {
    /// The point of wall `f`'s surface nearest `p`, within `reach`, and the wall's normal there
    /// out of the material (into the hole).
    fn onto(&self, f: usize, p: Vec3, reach: f64) -> Option<(Vec3, Vec3)> {
        let w = self.surfaces.get(f)?.as_ref()?;
        let uv = w.surface.project(p, None, reach)?;
        let normal = |uv: [f64; 2]| w.surface.derivs(uv[0], uv[1]).and_then(|(_, su, sv)| su.cross(sv).normalized());
        let n = normal(uv).or_else(|| {
            // A pole: the normal a hair toward the middle of the surface's box.
            let r = w.surface.domain().range;
            let mid = [0, 1].map(|k| r[k].map_or(uv[k], |(lo, hi)| 0.5 * (lo + hi)));
            normal([uv[0] + (mid[0] - uv[0]) * 1e-7, uv[1] + (mid[1] - uv[1]) * 1e-7])
        })?;
        Some((w.surface.point(uv)?, n * w.sign))
    }

    /// Whether `p`, a step `step` on from a point of wall `f`, is still on it: on it as the
    /// record tells, or else too far from all its edges to have crossed one (a seam, which the
    /// wall has on both sides, it crosses staying on it).
    fn inside(&self, f: usize, p: Vec3, step: f64) -> bool {
        let seam = |e: usize| self.q.edge_faces(e).iter().all(|g| *g == f);
        self.q.face_contains(f, p, self.tol * 10.0)
            || self.q.record().faces.get(f).is_some_and(|face| {
                face.loops.iter().all(|l| match l {
                    Bounds::Edges { edges } => edges
                        .iter()
                        .map(|oe| oe.edge as usize)
                        .filter(|e| !seam(*e))
                        .all(|e| self.q.edge_distance(e, p, self.sag).is_some_and(|d| d > step * 2.0)),
                    Bounds::Vertex { .. } => true,
                })
            })
    }

    /// How wall `f` leans at its point `p`: its normal into the hole along the fiber, above
    /// [`EDGE_ON`] where the hole widens going deeper.
    fn lean(&self, f: usize, p: Vec3) -> Option<f64> {
        let (_, n) = self.onto(f, p, self.tol * 10.0)?;
        Some(n.dot(self.slab.depth(p)?.deeper) - EDGE_ON)
    }

    fn widens(&self, f: usize, p: Vec3) -> Result<bool, String> {
        self.lean(f, p).map(|h| h > 0.0).ok_or_else(|| format!("a wall of the cut on {} can't be read", self.name))
    }

    /// Where wall `f`'s leaning changes along `curve` between parameters `a`, where it widens
    /// the hole as `wa` says, and `b`.
    fn corner(&self, curve: &CurveEval, f: usize, (mut a, wa): (f64, bool), mut b: f64) -> Result<f64, String> {
        for _ in 0..64 {
            let m = 0.5 * (a + b);
            let p = curve.point(m).ok_or_else(|| format!("an edge of the cut on {} can't be read", self.name))?;
            if self.widens(f, p)? == wa {
                a = m;
            } else {
                b = m;
            }
        }
        Ok(0.5 * (a + b))
    }

    /// The corners along `edge`: wherever one of its walls' leaning changes between its points.
    fn corners(&self, edge: &mut Edge) -> Result<(), String> {
        let samples = self.q.edge_points(edge.e, self.sag).ok_or_else(|| format!("an edge of the cut on {} can't be read", self.name))?;
        for f in edge.kind.walls() {
            let wide: Vec<bool> = samples.iter().map(|(_, p)| self.widens(f, *p)).collect::<Result<_, _>>()?;
            let mut found: Vec<f64> = Vec::new();
            for (k, pair) in wide.windows(2).enumerate() {
                if let [a, b] = pair
                    && a != b
                {
                    found.push(self.corner(&edge.curve, f, (samples[k].0, *a), samples[k + 1].0)?);
                }
            }
            // Two corners nearer than the tolerance are a fold touching the edge there, not
            // crossing it: the wall's leaning dips past edge-on and back within the tolerance.
            let at = |t: f64| edge.curve.point(t);
            let mut kept: Vec<f64> = Vec::new();
            for t in found {
                match kept.last() {
                    Some(&last) if matches!((at(last), at(t)), (Some(p), Some(q)) if p.dist(q) <= self.tol * 10.0) => {
                        kept.pop();
                    }
                    _ => kept.push(t),
                }
            }
            edge.corners.extend(kept.into_iter().map(|t| (t, f)));
        }
        edge.corners.sort_by(|x, y| x.0.total_cmp(&y.0));
        Ok(())
    }

    /// Wall `f`'s surface at `p`: its normal into the hole, and the way along the surface the
    /// leaning grows fastest, scaled by how fast.
    fn slope(&self, f: usize, p: Vec3) -> Option<(Vec3, Vec3)> {
        let (_, n) = self.onto(f, p, self.tol * 10.0)?;
        let pick = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let e1 = (pick - n * pick.dot(n)).normalized()?;
        let e2 = n.cross(e1);
        let d = (self.step * 1e-4).max(self.tol);
        let h = |x: Vec3| self.onto(f, x, d * 4.0).and_then(|(on, _)| self.lean(f, on));
        let du = (h(p + e1 * d)? - h(p - e1 * d)?) / (2.0 * d);
        let dv = (h(p + e2 * d)? - h(p - e2 * d)?) / (2.0 * d);
        Some((n, e1 * du + e2 * dv))
    }

    /// `p` moved onto wall `f`'s fold, from near it (Newton's method across the fold).
    fn settle(&self, f: usize, mut p: Vec3) -> Option<Vec3> {
        for _ in 0..30 {
            let h = self.lean(f, p)?;
            if h.abs() <= 1e-12 {
                return Some(p);
            }
            let (_, g) = self.slope(f, p)?;
            let gg = g.dot(g);
            if !(gg > 1e-300) {
                return None;
            }
            p = self.onto(f, p - g * (h / gg), self.step * 4.0)?.0;
        }
        (self.lean(f, p)?.abs() <= 1e-9).then_some(p)
    }

    /// The way into wall `f` from its point `p` on `edge`, at the edge's parameter `t`: square to
    /// the edge on the wall, toward the wall, which lies on the left of its loop seen from its
    /// parameters' natural normal.
    fn into(&self, f: usize, edge: &Edge, t: f64, p: Vec3) -> Option<Vec3> {
        let rec = self.q.record();
        let forward = rec.faces.get(f)?.loops.iter().find_map(|l| match l {
            Bounds::Edges { edges } => edges.iter().find(|oe| oe.edge as usize == edge.e).map(|oe| oe.forward),
            Bounds::Vertex { .. } => None,
        })?;
        let used = rec.shells.iter().find_map(|sh| match sh {
            Shell::Faces { faces, .. } => faces.iter().find(|of| of.face as usize == f).map(|of| of.forward),
            _ => None,
        })?;
        let h = 1e-6 * (edge.span[1] - edge.span[0]).abs();
        let (a, b) = (edge.curve.point((t - h).max(edge.span[0]))?, edge.curve.point((t + h).min(edge.span[1]))?);
        let along = (b - a).normalized()? * if forward { 1.0 } else { -1.0 };
        let natural = self.q.face_normal(f, self.q.face_uv(f, p, self.tol * 10.0)?)? * if used { 1.0 } else { -1.0 };
        natural.cross(along).normalized()
    }

    /// Wall `f`'s fold from `from`, a corner on its edge `at` (an edge of `edges`, and its
    /// parameter there), to where it leaves the wall: at one of `ends`, or at a corner it finds on
    /// one of `edges`. A fold round the wall from a point inside it (no edge; `ends` that point
    /// alone) ends where it started.
    fn fold(&self, f: usize, from: Vec3, at: Option<(usize, f64)>, ends: &[Vec3], edges: &[Edge]) -> Result<(Vec<Vec3>, End), String> {
        let lost = || format!("a fold of a wall of the cut on {} can't be followed", self.name);
        let way = |p: Vec3, prev: Option<Vec3>| -> Option<Vec3> {
            let (n, g) = self.slope(f, p)?;
            let w = n.cross(g).normalized()?;
            Some(match prev {
                Some(v) if w.dot(v) < 0.0 => -w,
                _ => w,
            })
        };
        // Into the wall from the corner: toward the wall's side of its edge, where the fold leaves
        // it plainly; else (inside the wall, or leaving the edge along it) whichever way a step
        // stays on the wall.
        let w0 = way(from, None).ok_or_else(lost)?;
        let side = at.and_then(|(i, t)| self.into(f, edges.get(i)?, t, from)).map(|m| w0.dot(m)).filter(|d| d.abs() > 1e-9);
        let probe = |w: Vec3| self.onto(f, from + w * (self.step * 0.1), self.step).is_some_and(|(p, _)| self.q.face_contains(f, p, self.tol * 10.0));
        let mut dir = if let Some(d) = side {
            w0 * d.signum()
        } else if probe(w0) {
            w0
        } else if probe(-w0) {
            -w0
        } else {
            return Err(lost());
        };
        let (mut path, mut p, mut step) = (vec![from], from, self.step);
        while path.len() < MAX_FOLD_POINTS {
            // A corner it was looking for, close ahead (not the one it started at, at the start).
            let away = path.len() > 3;
            let close = |i: &usize| ends[*i].dist(p) <= step * 1.5 && (ends[*i] - p).dot(dir) > 0.0 && (away || ends[*i].dist(from) > self.tol);
            if let Some(i) = (0..ends.len()).filter(close).min_by(|a, b| ends[*a].dist(p).total_cmp(&ends[*b].dist(p))) {
                path.push(ends[i]);
                return Ok((path, End::Known(i)));
            }
            let next = self.onto(f, p + dir * step, step * 4.0).and_then(|(g, _)| self.settle(f, g));
            let turned = next.and_then(|x| way(x, Some(dir)).map(|w| (x, w)));
            let Some((x, w)) = turned.filter(|(_, w)| w.dot(dir) >= (TAU / 40.0).cos()) else {
                if step <= self.tol * 10.0 {
                    return Err(lost());
                }
                step *= 0.5;
                continue;
            };
            if !self.inside(f, x, step) {
                // Off the wall between `p` and `x`: the corner there, on the wall's edge nearest,
                // unless the wall leans one way all along there, where the fold only touches
                // the edge and goes on.
                let near = edges
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.kind.walls().contains(&f))
                    .filter_map(|(i, e)| self.q.edge_distance(e.e, x, self.sag).map(|d| (d, i)))
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                let Some((_, i)) = near else { return Err(lost()) };
                if let Some(t) = self.crossing(&edges[i], f, x)? {
                    let at = edges[i].curve.point(t).ok_or_else(lost)?;
                    path.push(at);
                    return Ok((path, End::Found(i, t)));
                }
            }
            path.push(x);
            if w.dot(dir) > (TAU / 160.0).cos() {
                step = (step * 1.5).min(self.step);
            }
            (p, dir) = (x, w);
        }
        Err(lost())
    }

    /// A point of wall `f`'s fold between its points `a` (where the wall widens the hole) and
    /// `b` (where it narrows it): along the chord between them laid on the wall.
    fn seed(&self, f: usize, a: Vec3, b: Vec3) -> Result<Vec3, String> {
        let lost = || format!("a fold of a wall of the cut on {} can't be found", self.name);
        let at = |s: f64| self.onto(f, a + (b - a) * s, a.dist(b)).map(|x| x.0);
        let n = 64;
        let mut prev: Option<(f64, bool)> = None;
        for k in 0..=n {
            let s = k as f64 / n as f64;
            let Some(p) = at(s).filter(|p| self.inside(f, *p, a.dist(b) / n as f64)) else { continue };
            let wide = self.widens(f, p)?;
            if let Some((s0, w0)) = prev
                && w0 != wide
            {
                let (mut lo, mut hi) = (s0, s);
                for _ in 0..64 {
                    let m = 0.5 * (lo + hi);
                    let x = at(m).ok_or_else(lost)?;
                    if self.widens(f, x)? == w0 {
                        lo = m;
                    } else {
                        hi = m;
                    }
                }
                return at(0.5 * (lo + hi)).and_then(|x| self.settle(f, x)).ok_or_else(lost);
            }
            prev = Some((s, wide));
        }
        Err(lost())
    }

    /// Where wall `f`'s leaning changes on `edge` near `x`: between the edge's points either side
    /// of its point nearest `x`; none where it leans one way all along there.
    fn crossing(&self, edge: &Edge, f: usize, x: Vec3) -> Result<Option<f64>, String> {
        let samples = self.q.edge_points(edge.e, self.sag).ok_or_else(|| format!("an edge of the cut on {} can't be read", self.name))?;
        let k = (0..samples.len()).min_by(|a, b| samples[*a].1.dist(x).total_cmp(&samples[*b].1.dist(x))).unwrap_or(0);
        let (lo, hi) = (k.saturating_sub(3), (k + 3).min(samples.len() - 1));
        let wide: Vec<bool> = (lo..=hi).map(|i| self.widens(f, samples[i].1)).collect::<Result<_, _>>()?;
        let found = (0..wide.len().saturating_sub(1)).find(|i| wide[*i] != wide[i + 1]);
        let Some(i) = found else { return Ok(None) };
        self.corner(&edge.curve, f, (samples[lo + i].0, wide[i]), samples[lo + i + 1].0).map(Some)
    }
}

/// A piece of the outline: its points in space, in order, and its exact segments in the chart,
/// where known (else none).
struct Piece(Vec<Vec3>, Vec<Seg2>);

/// The shadow on the panel `name` in `slab` of what a cut took through it, from the body `now`
/// it left and the faces it made there (`walls`, faces of `now`), as regions of the panel's
/// parameters within a half turn of `around` round a roll. `tol`: points this near are one;
/// `sag`: edges are read within this of their chords.
pub(crate) fn shadow(
    now: &Body,
    q: &Query,
    slab: &dyn Fibers,
    walls: &[usize],
    name: &str,
    around: f64,
    tol: f64,
    sag: f64,
) -> Result<Vec<Region2>, String> {
    let rec = q.record();
    let mine: BTreeSet<usize> = walls.iter().copied().collect();
    let wall = |f: usize| -> Option<Wall> {
        let surface = SurfaceEval::new(&rec.faces[f].surface).ok()?;
        // Its side from a point of its edges where the record's own normal is had.
        let sign = rec.faces[f].loops.iter().find_map(|l| {
            let Bounds::Edges { edges } = l else { return None };
            edges.iter().find_map(|oe| {
                q.edge_points(oe.edge as usize, sag)?.into_iter().find_map(|(_, p)| {
                    let given = q.face_normal(f, q.face_uv(f, p, tol * 10.0)?)?;
                    let uv = surface.project(p, None, tol * 10.0)?;
                    let (_, su, sv) = surface.derivs(uv[0], uv[1])?;
                    let natural = su.cross(sv).normalized()?;
                    (given.dot(natural).abs() > 0.5).then(|| given.dot(natural).signum())
                })
            })
        })?;
        Some(Wall { surface, sign })
    };
    let surfaces = (0..rec.faces.len()).map(|f| if mine.contains(&f) { wall(f) } else { None }).collect();
    let r = Reader { q, slab, surfaces, tol, sag, step: (now.size() * 0.01).clamp(sag * 10.0, 1.0), name };
    let unread = || format!("a wall of the cut on {name} can't be read");
    // The walls' edges, each once, and what each is.
    let mut edges: Vec<Edge> = Vec::new();
    let mut seen = BTreeSet::new();
    for &f in walls {
        let (mut near, mut far, mut edge, mut depths) = (false, false, false, Vec::new());
        for l in &rec.faces[f].loops {
            let Bounds::Edges { edges: used } = l else { continue };
            for oe in used {
                let e = oe.edge as usize;
                let Some(other) = q.edge_faces(e).into_iter().find(|g| *g != f) else { continue };
                let points = q.edge_points(e, sag).ok_or_else(unread)?;
                let ws: Vec<f64> = points.iter().map(|(_, p)| slab.depth(*p).map(|d| d.w)).collect::<Option<_>>().ok_or_else(unread)?;
                depths.extend(ws.iter().copied());
                let kind = if mine.contains(&other) {
                    Kind::Between(f.min(other), f.max(other))
                } else if ws.iter().all(|w| w.abs() <= tol * 10.0) {
                    near = true;
                    Kind::Rim { far: false, wall: f }
                } else if ws.iter().all(|w| (w - slab.thickness()).abs() <= tol * 10.0) {
                    far = true;
                    Kind::Rim { far: true, wall: f }
                } else {
                    edge = true;
                    continue;
                };
                if seen.insert(e) {
                    let edge = &rec.edges[e];
                    let curve = CurveEval::new(edge.parameters()).map_err(|_| unread())?;
                    edges.push(Edge { e, kind, curve, span: edge.interval, corners: Vec::new() });
                }
            }
        }
        if !(near && far) {
            let level = depths.iter().all(|w| (w - depths[0]).abs() <= tol * 10.0);
            return Err(if level {
                format!("leaves a recess in {name}: not supported yet at a slant, on a roll or in a bend")
            } else {
                format!(
                    "leaves walls that are not square to the sheet and don't run through it on {name}: not supported yet (a countersink, a rounded or chamfered face edge)"
                )
            });
        }
        if edge {
            return Err(format!("reaches the edge of {name}: not supported yet at a slant, on a roll or across a bend"));
        }
    }
    for edge in &mut edges {
        r.corners(edge)?;
    }
    // Each wall's folds, from the corners where its leaning changes, to the next such corner.
    let mut folds: Vec<Piece> = Vec::new();
    for &f in walls {
        let mut open: Vec<(Vec3, usize, f64)> = edges
            .iter()
            .enumerate()
            .flat_map(|(i, e)| e.corners.iter().filter(|c| c.1 == f).filter_map(move |c| e.curve.point(c.0).map(|p| (p, i, c.0))))
            .collect();
        while let Some((from, i, t)) = open.pop() {
            let ends: Vec<Vec3> = open.iter().map(|o| o.0).collect();
            let (path, end) = r.fold(f, from, Some((i, t)), &ends, &edges)?;
            match end {
                End::Known(i) => {
                    open.remove(i);
                }
                End::Found(i, t) => {
                    edges[i].corners.push((t, f));
                    edges[i].corners.sort_by(|x, y| x.0.total_cmp(&y.0));
                }
            }
            folds.push(Piece(path, Vec::new()));
        }
    }
    // A wall whose edges lean both ways with no corner on them has a fold all round inside it.
    for &f in walls {
        if edges.iter().any(|e| e.corners.iter().any(|c| c.1 == f)) {
            continue;
        }
        let (mut wide, mut narrow) = (Vec::new(), Vec::new());
        for e in edges.iter().filter(|e| e.kind.walls().contains(&f)) {
            for (_, p) in q.edge_points(e.e, sag).ok_or_else(unread)? {
                match r.lean(f, p).ok_or_else(unread)? {
                    h if h > PLAIN => wide.push(p),
                    h if h < -PLAIN => narrow.push(p),
                    _ => {}
                }
            }
        }
        // From the nearest two points that lean apart.
        let pair = wide.iter().flat_map(|a| narrow.iter().map(move |b| (*a, *b))).min_by(|x, y| x.0.dist(x.1).total_cmp(&y.0.dist(y.1)));
        if let Some((a, b)) = pair {
            let from = r.seed(f, a, b)?;
            let (path, _) = r.fold(f, from, None, &[from], &edges)?;
            folds.push(Piece(path, Vec::new()));
        }
    }
    // The edges' pieces on the outline: between corners, where their walls lean as the outline
    // asks.
    let mut pieces = folds;
    for edge in &edges {
        let samples = q.edge_points(edge.e, sag).ok_or_else(unread)?;
        let mut cuts: Vec<f64> = std::iter::once(edge.span[0]).chain(edge.corners.iter().map(|c| c.0)).chain(std::iter::once(edge.span[1])).collect();
        cuts.dedup_by(|a, b| (*a - *b).abs() <= 1e-12 * (edge.span[1] - edge.span[0]).abs());
        for pair in cuts.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            let mid = edge.curve.point(0.5 * (a + b)).ok_or_else(unread)?;
            let wide: Vec<bool> = edge.kind.walls().iter().map(|f| r.widens(*f, mid)).collect::<Result<_, _>>()?;
            if !edge.kind.outline(&wide) {
                continue;
            }
            let mut pts = vec![edge.curve.point(a).ok_or_else(unread)?];
            pts.extend(samples.iter().filter(|(t, _)| *t > a && *t < b).map(|(_, p)| *p));
            pts.push(edge.curve.point(b).ok_or_else(unread)?);
            // On flat material the chart takes rigidly, a line or a circle is one exactly.
            let inside = [0.25, 0.5, 0.75].map(|u| edge.curve.point(a + (b - a) * u).and_then(|p| r.slab.rigid(p)));
            let segs = match inside {
                [Some((k, frame, place)), Some((l, ..)), Some((m, ..))] if k == l && l == m => exact(&edge.curve, a, b, &frame, &place),
                _ => Vec::new(),
            };
            pieces.push(Piece(pts, segs));
        }
    }
    let loops = assemble(&r, pieces, around)?;
    let regions = nest(loops);
    // The holes the walls leave in both faces lie within the shadow.
    for edge in &edges {
        if let Kind::Rim { .. } = edge.kind {
            for (_, p) in q.edge_points(edge.e, sag).ok_or_else(unread)? {
                let at = slab.depth(p).ok_or_else(unread)?.at;
                let at = Vec2::new(at.x, at.y + if slab.angular() { ((around - at.y) / TAU).round() * TAU } else { 0.0 });
                let near = regions.iter().any(|g| g.contains(at) || outline_distance(g, at) <= tol * 100.0);
                if !near {
                    return Err(format!("its walls on {name} overlap in the flat: not supported yet"));
                }
            }
        }
    }
    Ok(regions)
}

/// The middle of a region's turn round a roll: the mean angle of its outline's points.
pub(crate) fn around(region: &Region2) -> f64 {
    let ys: Vec<f64> = region.outer.segs.iter().map(|s| s.start().y).collect();
    ys.iter().sum::<f64>() / ys.len().max(1) as f64
}

/// Those of `walls` that lie in the panel `region` of `slab`: every point of their edges within
/// the thickness and on the region, or on its outline within `near`.
pub(crate) fn within(q: &Query, slab: &Slab, region: &Region2, walls: &[usize], near: f64, sag: f64) -> Vec<usize> {
    let mid = around(region);
    let on = |p: Vec3| {
        slab.depth(p).is_some_and(|d| {
            let at = Vec2::new(d.at.x, d.at.y + if slab.angular() { ((mid - d.at.y) / TAU).round() * TAU } else { 0.0 });
            d.w >= -near && d.w <= slab.thickness() + near && (region.contains(at) || outline_distance(region, at) <= near)
        })
    };
    let rec = q.record();
    walls
        .iter()
        .copied()
        .filter(|f| {
            rec.faces[*f].loops.iter().all(|l| match l {
                Bounds::Edges { edges } => {
                    edges.iter().all(|oe| q.edge_points(oe.edge as usize, sag).is_some_and(|pts| pts.iter().all(|(_, p)| on(*p))))
                }
                Bounds::Vertex { .. } => true,
            })
        })
        .collect()
}

fn outline_distance(g: &Region2, p: Vec2) -> f64 {
    std::iter::once(&g.outer)
        .chain(&g.holes)
        .flat_map(|l| &l.segs)
        .map(|s| s.point_at(s.closest_param(p).clamp(0.0, 1.0)).dist(p))
        .fold(f64::INFINITY, f64::min)
}

/// The pieces joined end to end into closed outlines in the panel's parameters, each piece a
/// curve through its points.
fn assemble(r: &Reader, pieces: Vec<Piece>, around: f64) -> Result<Vec<Loop2>, String> {
    let name = r.name;
    let broken = || format!("the cut's outline on {name} can't be followed round");
    // The pieces' ends, as points one within the edges' tolerance; a piece all within it is a
    // point (an edge's bit between a vertex and a touch beside it).
    let join = r.tol.max(r.q.record().tolerance) * 10.0;
    let pieces: Vec<Piece> = pieces.into_iter().filter(|Piece(p, _)| p.iter().any(|x| x.dist(p[0]) > join)).collect();
    let mut nodes: Vec<Vec3> = Vec::new();
    let mut ends: Vec<[usize; 2]> = Vec::new();
    for Piece(p, _) in &pieces {
        let mut end = [0; 2];
        for (slot, x) in end.iter_mut().zip([p[0], p[p.len() - 1]]) {
            *slot = match nodes.iter().position(|n| n.dist(x) <= join) {
                Some(i) => i,
                None => {
                    nodes.push(x);
                    nodes.len() - 1
                }
            };
        }
        ends.push(end);
    }
    let mut degree = vec![0usize; nodes.len()];
    for [a, b] in &ends {
        degree[*a] += 1;
        degree[*b] += 1;
    }
    if degree.iter().any(|d| *d != 2) {
        return Err(broken());
    }
    let mut used = vec![false; pieces.len()];
    let (mut loops, mut junctions) = (Vec::new(), Vec::new());
    for start in 0..pieces.len() {
        if used[start] {
            continue;
        }
        // The cycle: each piece, and whether it runs from its last point to its first.
        let (mut cycle, mut k, mut at) = (Vec::new(), start, ends[start][0]);
        loop {
            used[k] = true;
            let back = ends[k][0] != at;
            cycle.push((k, back));
            at = if back { ends[k][0] } else { ends[k][1] };
            if at == ends[start][0] {
                break;
            }
            k = (0..pieces.len()).find(|j| !used[*j] && ends[*j].contains(&at)).ok_or_else(broken)?;
        }
        // Its points in the panel's parameters, a roll's angle unwrapped along it, or its exact
        // segments.
        let mut runs: Vec<Run> = Vec::new();
        let mut last: Option<Vec2> = None;
        for (k, back) in &cycle {
            let Piece(points, exact) = &pieces[*k];
            if !exact.is_empty() {
                let segs: Vec<Seg2> = if *back { exact.iter().rev().map(Seg2::reversed).collect() } else { exact.clone() };
                last = segs.last().map(Seg2::end);
                runs.push(Run::Exact(segs));
                continue;
            }
            let mut pts: Vec<Vec3> = points.clone();
            if *back {
                pts.reverse();
            }
            let mut run = Vec::new();
            for p in pts {
                let mut c = r.slab.depth(p).ok_or_else(broken)?.at;
                if r.slab.angular()
                    && let Some(prev) = last
                {
                    c.y += ((prev.y - c.y) / TAU).round() * TAU;
                }
                last = Some(c);
                run.push(c);
            }
            runs.push(Run::Points(run));
        }
        // Each piece starts where the one before it ends (an exact piece's ends stay, the points
        // beside them move); one that is a point in the flat (a fold along a fiber, a wall
        // edge-on to the fibers along a line) is left out.
        let scale = if r.slab.angular() { 1e-3 } else { 1.0 };
        runs.retain(|run| run.extent() > r.tol * 1e-3 * scale);
        for i in 1..runs.len() {
            let (end, start) = (runs[i - 1].end(), runs[i].start());
            let (before, after) = runs.split_at_mut(i);
            match (&mut before[i - 1], &mut after[0]) {
                (Run::Points(prev), Run::Exact(_)) => {
                    if let (Some(p), Some(s)) = (prev.last_mut(), start) {
                        *p = s;
                    }
                }
                (_, Run::Points(next)) => {
                    if let (Some(p), Some(e)) = (next.first_mut(), end) {
                        *p = e;
                    }
                }
                (Run::Exact(_), Run::Exact(_)) => {}
            }
        }
        let first = runs.first().and_then(Run::start).ok_or_else(broken)?;
        let finish = runs.last().and_then(Run::end).ok_or_else(broken)?;
        if r.slab.angular() && (first.y - finish.y).abs() > 1.0 {
            return Err(format!("cuts all round {name}: not supported yet"));
        }
        match runs.as_mut_slice() {
            [.., Run::Points(tail)] => {
                if let Some(l) = tail.last_mut() {
                    *l = first;
                }
            }
            [Run::Points(head), ..] => {
                if let Some(f) = head.first_mut() {
                    *f = finish;
                }
            }
            _ => {}
        }
        // Onto the panel's own turn round a roll.
        if r.slab.angular() {
            let pts = || runs.iter().filter_map(|run| if let Run::Points(v) = run { Some(v) } else { None }).flatten();
            let mean = pts().map(|p| p.y).sum::<f64>() / pts().count().max(1) as f64;
            let shift = ((around - mean) / TAU).round() * TAU;
            for run in &mut runs {
                if let Run::Points(v) = run {
                    for p in v {
                        p.y += shift;
                    }
                }
            }
        }
        // A run that closes on itself (a whole rim) as two halves: a curve through points runs
        // from one end to another.
        let runs: Vec<Run> = runs
            .into_iter()
            .flat_map(|run| match run {
                Run::Points(run) => {
                    let n = run.len();
                    if n > 2 && run[0].dist(run[n - 1]) <= r.tol * 1e-3 {
                        vec![Run::Points(run[..=n / 2].to_vec()), Run::Points(run[n / 2..].to_vec())]
                    } else {
                        vec![Run::Points(run)]
                    }
                }
                exact => vec![exact],
            })
            .collect();
        junctions.extend(runs.iter().filter_map(Run::start));
        let segs: Vec<Seg2> = runs
            .iter()
            .flat_map(|run| match run {
                Run::Points(v) => curve_through(v),
                Run::Exact(segs) => segs.clone(),
            })
            .collect();
        if segs.is_empty() {
            return Err(broken());
        }
        loops.push(Loop2 { segs });
    }
    crossed(&loops, &junctions, r.tol).map_or(Ok(loops), |_| Err(format!("its walls on {name} overlap in the flat: not supported yet")))
}

/// A run of an outline in the chart: points a curve is put through, or exact segments.
enum Run {
    Points(Vec<Vec2>),
    Exact(Vec<Seg2>),
}

impl Run {
    fn start(&self) -> Option<Vec2> {
        match self {
            Run::Points(v) => v.first().copied(),
            Run::Exact(segs) => segs.first().map(Seg2::start),
        }
    }

    fn end(&self) -> Option<Vec2> {
        match self {
            Run::Points(v) => v.last().copied(),
            Run::Exact(segs) => segs.last().map(Seg2::end),
        }
    }

    /// How far it reaches from its start: a run that reaches no further is a point.
    fn extent(&self) -> f64 {
        match self {
            Run::Points(v) => {
                if v.len() < 2 {
                    return 0.0;
                }
                v.iter().map(|p| p.dist(v[0])).fold(0.0, f64::max)
            }
            Run::Exact(segs) => segs.iter().map(Seg2::length).sum(),
        }
    }
}

/// A point where the outlines cross themselves or each other, away from the points where their
/// pieces meet (`joints`), which may touch there.
fn crossed(loops: &[Loop2], joints: &[Vec2], tol: f64) -> Option<Vec2> {
    let lines: Vec<Vec<Vec2>> = loops.iter().map(|l| (&l.polyline(tol.max(1e-6) * 10.0)).into_iter().copied().collect()).collect();
    let near_joint = |p: Vec2, reach: f64| joints.iter().any(|j| j.dist(p) <= reach);
    let seg = |l: &Vec<Vec2>, i: usize| (l[i], l[(i + 1) % l.len()]);
    for (a, la) in lines.iter().enumerate() {
        for (b, lb) in lines.iter().enumerate().skip(a) {
            for i in 0..la.len() {
                let (p0, p1) = seg(la, i);
                for j in 0..lb.len() {
                    if a == b && (j <= i + 1 || (i == 0 && j == la.len() - 1)) {
                        continue;
                    }
                    let (q0, q1) = seg(lb, j);
                    let (d1, d2) = (p1 - p0, q1 - q0);
                    let den = d1.cross(d2);
                    if den.abs() <= 1e-15 * d1.len() * d2.len() {
                        continue;
                    }
                    let (s, t) = ((q0 - p0).cross(d2) / den, (q0 - p0).cross(d1) / den);
                    if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t) {
                        let x = p0 + d1 * s;
                        let reach = 4.0 * d1.len().max(d2.len());
                        if !near_joint(x, reach) {
                            return Some(x);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Closed outlines as regions: each inside another is a hole of the smallest around it.
fn nest(loops: Vec<Loop2>) -> Vec<Region2> {
    let mut loops: Vec<Loop2> = loops.into_iter().map(|l| l.ccw()).collect();
    loops.sort_by(|a, b| b.signed_area().abs().total_cmp(&a.signed_area().abs()));
    let mut regions: Vec<Region2> = Vec::new();
    for l in loops {
        let p = l.segs[0].mid();
        match regions.iter_mut().rev().find(|g| g.outer.contains(p)) {
            Some(g) if !g.holes.iter().any(|h| h.contains(p)) => g.holes.push(l.reversed()),
            _ => regions.push(Region2 { outer: l, holes: Vec::new() }),
        }
    }
    regions
}
