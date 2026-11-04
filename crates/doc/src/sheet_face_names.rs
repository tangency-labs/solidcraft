//! Readable names of a sheet metal body's faces, from the sheet itself: `<panel>:top`,
//! `<panel>:bottom` and `<panel>:rim.<edge>` (the names sketches on sheet faces keep, see
//! [`SheetBody::panel_face_at`]); a bend's curved faces, `<flange>:bend.inner` and
//! `<flange>:bend.outer`; and where a flange's panel runs on past its bend's ends (toward a
//! neighbouring flange), the rim along the bend there, `<flange>:rim.ext0` and `rim.ext1`. A panel is `base` or a flange's tag (`F<feature>.<k>`), so the names
//! say what each face is and stay with it when sizes change or other flanges come and go.

use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use solidcore_record::Region2;
use solidcraft_develop::{Bent, JointKind, Rigid3, Surface, develop};

use super::SheetBody;
use crate::{ModelBody, ModelState, SurfaceSheet, apply_point, mat_inverse};

/// The share of a face's sample points that must agree on a name for the face to take it.
const AGREE: f64 = 0.8;

/// The sine of the angle within which a flat face's plane faces along a triangle's normal.
const FACING: f64 = 1e-3;

impl SheetBody {
    /// The readable name of the face of this sheet a point lies on (sheet coordinates, within
    /// `tol`, kept below a quarter of the thickness): a flat face's (see
    /// [`panel_face_at`](Self::panel_face_at)), else a bend's inner or outer face.
    pub fn face_name_at(&self, p: Vec3, tol: f64) -> Option<String> {
        self.panel_face_at(p, tol).map(|(name, _)| name).or_else(|| self.bend_face_at(p, tol)).or_else(|| self.ext_rim_at(p, tol))
    }

    /// [`face_name_at`](Self::face_name_at) for a point on a face whose normal is `normal` (a
    /// mesh triangle's, either way round): a flat face's name is taken only where its plane faces
    /// along `normal`. A rim is as narrow as the sheet is thick, so the centres of its triangles
    /// can lie within `tol` of the panel's top or bottom face beside it.
    fn face_name_facing(&self, p: Vec3, normal: Vec3, tol: f64) -> Option<String> {
        let facing = |plane: &Plane| plane.normal().cross(normal).len() < FACING;
        match self.panel_face_at(p, tol) {
            Some((name, plane)) if facing(&plane) => Some(name),
            Some(_) => self
                .rim_face_at(p, tol)
                .filter(|(_, plane)| facing(plane))
                .map(|(name, _)| name)
                .or_else(|| self.bend_face_at(p, tol))
                .or_else(|| self.ext_rim_at(p, tol)),
            None => self.bend_face_at(p, tol).or_else(|| self.ext_rim_at(p, tol)),
        }
    }

    /// The rim of a flange's panel along its bend line, where the panel runs on past the
    /// bend's start (`<flange>:rim.ext0`) or end (`rim.ext1`).
    fn ext_rim_at(&self, p: Vec3, tol: f64) -> Option<String> {
        if self.flat {
            return None;
        }
        let tol = tol.min(self.t * 0.25);
        for (pi, region) in self.panels() {
            let Some(i) = pi else { continue };
            let Some(f) = self.flanges.get(i) else { continue };
            let Some(inverse) = mat_inverse(&self.world(pi)) else { continue };
            let q = apply_point(&inverse, p);
            if q.z < -tol || q.z > self.t + tol {
                continue;
            }
            let q2 = Vec2::new(q.x, q.y);
            let pts = region.outer.ccw().polyline(1e-3);
            let m = pts.len();
            for k in 0..m {
                let (Some(&a), Some(&b)) = (pts.get(k), pts.get((k + 1) % m)) else { continue };
                let d = b - a;
                if d.len2() < 1e-18 || self.rim_edge(pi, a, b, k).as_deref() != Some("bend") {
                    continue;
                }
                let along = (q2 - a).dot(d) / d.len2();
                if !(-1e-9..=1.0 + 1e-9).contains(&along) || ((a + d * along) - q2).len() > tol {
                    continue;
                }
                let end = if (q2 - f.p).dot(f.d) < f.len / 2.0 { "ext0" } else { "ext1" };
                return Some(format!("{}:rim.{end}", self.panel_name(pi)));
            }
        }
        None
    }

    /// The curved face of a bend a point lies on: `<flange>:bend.inner` (the bend's radius) or
    /// `<flange>:bend.outer` (the radius plus the thickness).
    fn bend_face_at(&self, p: Vec3, tol: f64) -> Option<String> {
        if self.flat {
            return None;
        }
        let tol = tol.min(self.t * 0.25);
        for (i, f) in self.flanges.iter().enumerate() {
            let Some((world, o, d, start, toward)) = self.bend_frame(f) else { continue };
            let Some(inverse) = mat_inverse(&world) else { continue };
            let q = apply_point(&inverse, p) - o;
            let u = q.dot(d);
            let (lo, hi) = self.zone_reach(f);
            if u < lo - tol || u > hi + tol {
                continue;
            }
            let radial = q - d * u;
            let rho = radial.len();
            let side = if (rho - f.radius).abs() <= tol {
                "inner"
            } else if (rho - f.radius - self.t).abs() <= tol {
                "outer"
            } else {
                continue;
            };
            let phi = radial.dot(toward).atan2(radial.dot(start));
            if phi < -1e-6 || phi > f.angle.abs() + 1e-6 {
                continue;
            }
            return Some(format!("{}:bend.{side}", self.panel_name(Some(i))));
        }
        None
    }
}

/// The readable sheet names of a body's faces (by B-rep face index), as
/// [`sheet_face_name`](super::sheet_face_name)s: `sheet:<body>:<panel>:<side>`. `None` for a face
/// the sheet does not name (a hole's wall, a pocket's floor) and for every face of a body that is
/// no folded sheet of `st`.
pub fn body_sheet_face_names(st: &ModelState, b: &ModelBody) -> Vec<Option<String>> {
    // A sheet made from surfaces names each face by an exact point well inside it and its
    // normal there: no mesh.
    if let Some(sk) = st.surface_sheets.iter().find(|s| s.body == b.name) {
        let n = b.body.face_count();
        let Some(faces) = SurfaceFaces::of(sk).filter(|_| !sk.unfolded) else { return vec![None; n] };
        let q = solidcraft_geom::brep::query::Query::new(b.body.record());
        let tol = (sk.thickness * 1e-3).max(b.body.record().tolerance * 10.0);
        return (0..n)
            .map(|f| q.face_sample(f).and_then(|(p, normal)| faces.name_facing(p, normal, tol)).map(|name| super::sheet_face_name(&b.name, &name)))
            .collect();
    }
    let m = b.mesh();
    let nf = m.tri_face.iter().map(|f| *f as usize + 1).max().unwrap_or(0);
    let Some(sheet) = st.sheets.iter().find(|s| s.body == b.name && !s.flat) else { return vec![None; nf] };
    // Triangle centres lie inside a face, off a curved one by its facets' sag: a fifth of the
    // thickness takes them and still tells a flat face from the rim beside it.
    voted(&m, nf, sheet.t * 0.2, &b.name, |p, n, tol| sheet.face_name_facing(p, n, tol))
}

/// Each face's name, by the vote of up to 48 of its triangles' centres: the name `name_at`
/// gives a point with the triangle's normal, kept where at least [`AGREE`] of them agree.
fn voted<M: std::ops::Deref<Target = solidcraft_geom::Mesh>>(
    m: &M,
    nf: usize,
    tol: f64,
    body: &str,
    name_at: impl Fn(Vec3, Vec3, f64) -> Option<String>,
) -> Vec<Option<String>> {
    let mut votes: Vec<Vec<(String, usize)>> = vec![Vec::new(); nf];
    let mut total = vec![0usize; nf];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let (Some(v), Some(n), Some([a, b2, c])) = (votes.get_mut(*f as usize), total.get_mut(*f as usize), m.tri(t)) else { continue };
        let Some(normal) = (b2 - a).cross(c - a).normalized() else { continue };
        // At most a few dozen points a face: enough to tell its name, cheap on a fine mesh.
        if *n >= 48 {
            continue;
        }
        *n += 1;
        if let Some(name) = name_at((a + b2 + c) * (1.0 / 3.0), normal, tol) {
            match v.iter_mut().find(|(x, _)| *x == name) {
                Some(x) => x.1 += 1,
                None => v.push((name, 1)),
            }
        }
    }
    votes
        .into_iter()
        .zip(total)
        .map(|(v, n)| {
            v.into_iter()
                .max_by_key(|x| x.1)
                .filter(|(_, k)| *k as f64 >= AGREE * n.max(1) as f64)
                .map(|(name, _)| super::sheet_face_name(body, &name))
        })
        .collect()
}

/// What names the faces of a folded sheet made from surfaces (#6748), as [`SheetBody`] names
/// its own: each flat panel's two faces (`<panel>:top` on the sheet's surface, `<panel>:bottom`),
/// the rims along the straight edges of its outline, and each bend's curved faces, named after
/// the panel the bend makes (`<panel>:bend.inner`, `bend.outer`). A panel's rims are `far`,
/// `side0` and `side1` on a panel a bend makes (as a flange's), `ext0` and `ext1` where it runs
/// on past its bend's ends along it, and `edge<k>` (the edge's place in its outline) on any
/// other.
struct SurfaceFaces {
    t: f64,
    panels: Vec<(Rigid3, Region2, String)>,
    rims: Vec<(Rigid3, Vec2, Vec2, String)>,
    bends: Vec<(Bent, bool, Region2, String)>,
}

impl SurfaceFaces {
    fn of(sk: &SurfaceSheet) -> Option<SurfaceFaces> {
        let dev = develop(&sk.sheet, 0).ok()?;
        let fold = crate::sheet_surface_convert::fold_onto(&sk.sheet, &dev, &sk.places).ok()?;
        let frames = crate::sheet_surface_cuts::frames(sk, &dev).ok()?;
        let pieces = crate::sheet_surface::pieces(&dev, &mut crate::sheet::FlatWork::new()).ok()?;
        let mut panels = Vec::new();
        let mut rims = Vec::new();
        for (i, (panel, frame)) in sk.sheet.panels.iter().zip(&frames).enumerate() {
            let (Surface::Plane, Some(frame), Some(Some(at))) = (&panel.surface, frame, dev.placements.get(i)) else { continue };
            panels.push((*frame, panel.region.clone(), panel.name.clone()));
            // Its material as the flat has it, after the bends' trims, in its chart.
            let back = at.inverse();
            let own: Vec<Region2> =
                pieces.iter().filter(|(p, _)| matches!(p, crate::sheet_surface::Piece::Panel(k) if *k == i)).map(|(_, r)| back.region(r)).collect();
            rims.extend(rim_names(sk, i, &own).into_iter().map(|(a, b, name)| (*frame, a, b, format!("{}:rim.{name}", panel.name))));
        }
        let mut bends = Vec::new();
        for z in &dev.zones {
            let Some(joint) = sk.sheet.joints.get(z.joint) else { continue };
            let (JointKind::Bend(b), Some(Some(bent))) = (&joint.kind, fold.bends.get(z.joint)) else { continue };
            let Some(made) = sk.sheet.panels.get(joint.b.panel) else { continue };
            bends.push((*bent, b.toward_material, z.region.clone(), made.name.clone()));
        }
        Some(SurfaceFaces { t: sk.thickness, panels, rims, bends })
    }

    /// The name of the face a point lies on (within `tol`) whose normal is `normal`, either way
    /// round.
    fn name_facing(&self, p: Vec3, normal: Vec3, tol: f64) -> Option<String> {
        let facing = |n: Vec3| n.cross(normal).len() < FACING;
        let t = self.t;
        for (frame, region, name) in &self.panels {
            let q = frame.inverse().point(p);
            let top = q.z.abs() < (q.z - t).abs();
            let off = if top { q.z.abs() } else { (q.z - t).abs() };
            if off <= tol && facing(frame.z) && region.contains(Vec2::new(q.x, q.y)) {
                return Some(format!("{name}:{}", if top { "top" } else { "bottom" }));
            }
        }
        for (bent, toward, zone, name) in &self.bends {
            let Some((flat, depth, deeper)) = bent.unwrapped(p) else { continue };
            let near = zone.contains(flat)
                || std::iter::once(&zone.outer).chain(&zone.holes).flat_map(|l| &l.segs).any(|s| s.point_at(s.closest_param(flat).clamp(0.0, 1.0)).dist(flat) <= tol);
            if !near || depth < -tol || depth > t + tol {
                continue;
            }
            // Its normal along the axis: one of the bend's ends, its start or its end along the
            // joint. Round the axis: its inner or outer face.
            let (_, axis) = bent.axis();
            if normal.dot(axis).abs() > 0.5 {
                let (o, e) = zone_ends(zone, bent);
                // Numbered along the flat as it is read, which the charts see mirrored.
                let side = if (flat - o).dot(e - o) < (e - o).dot(e - o) / 2.0 { "end1" } else { "end0" };
                return Some(format!("{name}:bend.{side}"));
            }
            if (depth.abs() <= tol || (depth - t).abs() <= tol) && normal.dot(deeper).abs() > 0.9 {
                // Where the bend turns toward the material, the sheet's surface runs round its
                // outside.
                let surface = depth.abs() <= tol;
                return Some(format!("{name}:bend.{}", if surface == *toward { "outer" } else { "inner" }));
            }
        }
        // A rim: the nearest straight edge of a panel whose way out of the panel the face's
        // normal runs along. A rim cut at a slant (a mitered corner) leans off the panel's
        // normal, so a point of it lies up to a thickness off its edge in the panel's chart.
        let mut rim: Option<(f64, &String)> = None;
        for (frame, a, b, name) in &self.rims {
            let back = frame.inverse();
            let q = back.point(p);
            if q.z < -tol || q.z > t + tol {
                continue;
            }
            let (s, q2) = (solidcore_record::Seg2::Line { a: *a, b: *b }, Vec2::new(q.x, q.y));
            let Some(d) = (*b - *a).normalized() else { continue };
            let reach = (t + tol) / a.dist(*b).max(1e-9);
            let u = s.closest_param(q2);
            let off = s.point_at(u.clamp(0.0, 1.0)).dist(q2);
            let n = back.vector(normal);
            let Some(across) = Vec2::new(n.x, n.y).normalized() else { continue };
            if (-reach..=1.0 + reach).contains(&u) && off <= t + tol && across.dot(Vec2::new(d.y, -d.x)).abs() > 0.5 && rim.is_none_or(|(x, _)| off < x) {
                rim = Some((off, name));
            }
        }
        if let Some((_, name)) = rim {
            return Some(name.clone());
        }
        None
    }
}

impl SurfaceSheet {
    /// Each straight rim of the sheet's flat panels: the panel, its edge in the panel's chart and
    /// its name there (see [`SurfaceFaces`]).
    pub(crate) fn rims(&self) -> Vec<(usize, Vec2, Vec2, String)> {
        let Ok(dev) = develop(&self.sheet, 0) else { return Vec::new() };
        let Ok(pieces) = crate::sheet_surface::pieces(&dev, &mut crate::sheet::FlatWork::new()) else { return Vec::new() };
        let mut out = Vec::new();
        for (i, panel) in self.sheet.panels.iter().enumerate() {
            let (Surface::Plane, Some(Some(at))) = (&panel.surface, dev.placements.get(i)) else { continue };
            let back = at.inverse();
            let own: Vec<Region2> =
                pieces.iter().filter(|(p, _)| matches!(p, crate::sheet_surface::Piece::Panel(k) if *k == i)).map(|(_, r)| back.region(r)).collect();
            out.extend(rim_names(self, i, &own).into_iter().map(|(a, b, name)| (i, a, b, name)));
        }
        out
    }
}

/// The straight edges of the outlines `own` of panel `i`'s material (in its chart), with the
/// names of their rims (see [`SurfaceFaces`]); a name two edges would share takes the edge's
/// place too. On a panel a bend makes, an edge along the bend farthest from it is `far`, one
/// nearer it past the bend's ends `ext0` or `ext1` (none within them: the bend runs on there).
fn rim_names(sk: &SurfaceSheet, i: usize, own: &[Region2]) -> Vec<(Vec2, Vec2, String)> {
    let made = sk.sheet.joints.iter().find_map(|j| match &j.kind {
        JointKind::Bend(b) if j.b.panel == i => Some((j.b, b.ends.far)),
        _ => None,
    });
    let lines: Vec<(usize, Vec2, Vec2)> = own
        .iter()
        .flat_map(|r| &r.outer.segs)
        .enumerate()
        .filter_map(|(k, s)| match *s {
            solidcore_record::Seg2::Line { a, b } if a.dist(b) > 1e-9 => Some((k, a, b)),
            _ => None,
        })
        .collect();
    let small = sk.thickness * 1e-3;
    // How far into the panel from its bend an edge along the bend lies.
    let off = |a: Vec2, b: Vec2| made.map_or(0.0, |(an, _)| ((a + b) * 0.5 - an.origin).cross(an.dir));
    let parallel = |a: Vec2, b: Vec2| made.is_some_and(|(an, _)| (b - a).normalized().is_some_and(|d| d.cross(an.dir).abs() < 1e-6));
    let farthest = lines.iter().filter(|(_, a, b)| parallel(*a, *b)).map(|(_, a, b)| off(*a, *b)).fold(f64::MIN, f64::max);
    let mut out: Vec<(Vec2, Vec2, String)> = Vec::new();
    for &(k, a, b) in &lines {
        let Some((an, (lo, hi))) = made else {
            out.push((a, b, format!("edge{k}")));
            continue;
        };
        let along = |p: Vec2| (p - an.origin).dot(an.dir);
        if !parallel(a, b) {
            // Numbered along the flat as it is read, which the charts see mirrored.
            out.push((a, b, if along((a + b) * 0.5) < (lo + hi) / 2.0 { "side1" } else { "side0" }.into()));
        } else if off(a, b) >= farthest - small && off(a, b) > small {
            out.push((a, b, "far".into()));
        } else {
            // Nearer the bend: its stretches past the bend's ends (the bend runs on within them).
            let at = |s: f64| a + (b - a) * ((s - along(a)) / (along(b) - along(a)));
            let (s0, s1) = (along(a).min(along(b)), along(a).max(along(b)));
            if s0 < lo - small {
                out.push((at(s0), at(s1.min(lo)), "ext1".into()));
            }
            if s1 > hi + small {
                out.push((at(s0.max(hi)), at(s1), "ext0".into()));
            }
        }
    }
    let names: Vec<String> = out.iter().map(|x| x.2.clone()).collect();
    for (k, x) in out.iter_mut().enumerate() {
        if names.iter().filter(|n| **n == x.2).count() > 1 {
            x.2 = format!("{}{k}", x.2);
        }
    }
    out
}

/// Where a bend's flat `zone` starts and ends along its joint: its extreme points along the
/// joint's way in the flat.
fn zone_ends(zone: &Region2, bent: &Bent) -> (Vec2, Vec2) {
    let pts = zone.outer.polyline(1e-3);
    let way = bent.flat_way();
    let along = |p: &Vec2| p.dot(way);
    let lo = (&pts).into_iter().copied().min_by(|a, b| along(a).total_cmp(&along(b))).unwrap_or_default();
    let hi = (&pts).into_iter().copied().max_by(|a, b| along(a).total_cmp(&along(b))).unwrap_or_default();
    (lo, hi)
}
