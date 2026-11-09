//! Readable names of a sheet metal body's faces, from the sheet itself: `<panel>:top`,
//! `<panel>:bottom` and `<panel>:rim.<edge>` (the names sketches on sheet faces keep, see
//! [`SheetBody::panel_face_at`]); a bend's curved faces, `<flange>:bend.inner` and
//! `<flange>:bend.outer`; and where a flange's panel runs on past its bend's ends (toward a
//! neighbouring flange), the rim along the bend there, `<flange>:rim.ext0` and `rim.ext1`. A panel
//! is `base`, or is named after the feature that made it and the edge it was made on
//! (`F12.base.l2`, a flange on the base's sketch line `l2`; `F15.F12.base.l2.far`, a return on
//! that flange's far edge; see `tag_panels` in `eval_more`), so the names say what each face is
//! and stay with it when sizes change, other flanges come and go, or a feature's picks are
//! reordered. These are the flat model's (`SheetBody`).
//!
//! A sheet made from surfaces has no names of its own: it reports to the model what each face of
//! its body was made for (`FaceNamer::part`, a [`SheetPart`]), and the model names the face from
//! that, as it names every feature's faces (#7990, see `lineage::sheet_lineage`).

use solidcore_record::{Loop2, Plane, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use solidcore_record::Region2;
use solidcraft_develop::{CornerRelief, Development, JointKind, Sheet, Surface};

use crate::sheet_surface::Piece;
use crate::sheet_surface_convert::{HoleRecord, PanelMade, RimEdge, SheetFace, SheetPart};

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
    // A sheet made from surfaces names none: it reports what each face was made for, and the model
    // names its faces (#7990).
    if st.surface_sheets.iter().any(|s| s.body == b.name) {
        return vec![None; b.body.face_count()];
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

/// What the builder of a folded sheet made from surfaces sews a face for (see
/// `sheet_surface_fold::solid`): one of a piece's two caps, on the sheet's surface or the other
/// face, `at` the middle of the piece, or a wall along the run of its outline from `a` through
/// `mid` to `b` (flat coordinates).
#[derive(Clone, Copy, Debug)]
pub(crate) enum FaceRole {
    Cap { surface: bool, at: Vec2 },
    Wall { a: Vec2, b: Vec2, mid: Vec2 },
}

/// What each face of a folded sheet made from surfaces (#6748) was made for, as its builder makes
/// it, read from the sheet itself (see [`SheetPart`]): never a name, which the model gives from it
/// (#7990). A face's part is what it was made for, never read back from where it lies.
pub(crate) struct FaceNamer<'a> {
    sheet: &'a Sheet,
    dev: &'a Development,
    /// What made each panel, by its key (see [`PanelMade`]).
    recipes: &'a [(String, PanelMade)],
    /// Each panel's rims in the flat, and what made each (see [`rim_edges`]).
    rims: Vec<Vec<(Vec2, Vec2, Option<RimEdge>)>>,
    /// Each corner relief cut from the flat (see [`Development::cuts`]), with the panels the two
    /// bends whose corner it is make: the corner whose bends' center lines cross inside it.
    reliefs: Vec<(&'a Region2, [String; 2])>,
}

impl<'a> FaceNamer<'a> {
    pub(crate) fn new(
        sheet: &'a Sheet,
        dev: &'a Development,
        pieces: &[(Piece, Region2)],
        curves: &[(String, String, Vec2, Vec2)],
        recipes: &'a [(String, PanelMade)],
    ) -> FaceNamer<'a> {
        let rims = (0..sheet.panels.len())
            .map(|i| {
                let (Some(panel), Some(Some(at))) = (sheet.panels.get(i), dev.placements.get(i)) else { return Vec::new() };
                if !matches!(panel.surface, Surface::Plane) {
                    return Vec::new();
                }
                let back = at.inverse();
                let own: Vec<Region2> = pieces.iter().filter(|(p, _)| matches!(p, Piece::Panel(k) if *k == i)).map(|(_, r)| back.region(r)).collect();
                let zones: Vec<Region2> = dev.zones.iter().map(|z| back.region(&z.region)).collect();
                rim_edges(sheet, i, &own, &zones, curves, recipes).into_iter().map(|(a, b, edge, _)| (at.point(a), at.point(b), edge)).collect()
            })
            .collect();
        let key = |j: usize| sheet.joints.get(j).and_then(|j| sheet.panels.get(j.b.panel)).map(|p| p.name.clone());
        let centers: Vec<(Vec2, [String; 2])> = sheet
            .corners
            .iter()
            .filter(|c| matches!(c.relief, CornerRelief::Round | CornerRelief::Square))
            .filter_map(|c| {
                let mut panels = [key(c.joints[0])?, key(c.joints[1])?];
                panels.sort();
                Some((corner_center(dev, c.joints)?, panels))
            })
            .collect();
        let reliefs = dev
            .cuts
            .iter()
            .filter_map(|cut| match centers.iter().filter(|(c, _)| cut.contains(*c)).collect::<Vec<_>>().as_slice() {
                [(_, panels)] => Some((cut, panels.clone())),
                _ => None,
            })
            .collect();
        FaceNamer { sheet, dev, recipes, rims, reliefs }
    }

    /// What the face made for `role` of `piece` was made for, as the sheet reports it to the model
    /// (see [`SheetPart`]), and where it starts along that part's definition where faces made for
    /// one part need telling apart (see [`ranked`]): a panel's cap, or a rim by what made its edge;
    /// a bend's curved face or end by the panel the bend makes; a tab's cap, along its bend; a
    /// corner relief's wall, along the relief's outline; a hole's wall (see [`HoleRecord`],
    /// `holes`), along its sketch curve. None for a wall nothing says what made (the model names
    /// it from geometry). A fold's piece's caps and rims are its panel's (see [`root`]): pieces of
    /// the faces the fold parted.
    pub(crate) fn part(&self, piece: Piece, role: FaceRole, holes: &[HoleRecord]) -> Option<(SheetPart, Option<f64>)> {
        let key = |i: usize| self.sheet.panels.get(i).map(|p| p.name.clone());
        let parted = |i: usize| key(root(self.sheet, self.recipes, i));
        let on = |m: Vec2, p: Vec2, q: Vec2| {
            let s = Seg2::Line { a: p, b: q };
            s.point_at(s.closest_param(m).clamp(0.0, 1.0)).dist(m) <= 1e-6
        };
        let small = self.sheet.thickness * 1e-3;
        // A wall nothing else made: a corner relief's, else a hole's.
        let cut = |run: [Vec2; 3]| -> Option<(SheetPart, Option<f64>)> {
            for (region, panels) in &self.reliefs {
                let segs = &region.outer.segs;
                let place =
                    |x: Vec2| segs.iter().enumerate().find(|(_, g)| on_seg(g, x, small)).map(|(k, g)| k as f64 + g.closest_param(x).clamp(0.0, 1.0));
                if let [Some(a), Some(_), Some(b)] = run.map(place) {
                    return Some((SheetPart::Relief { panels: panels.clone() }, Some(a.min(b))));
                }
            }
            let (feature, curve, along) = hole_at(self.sheet, self.dev, holes, run)?;
            Some((SheetPart::Hole { feature, curve }, Some(along)))
        };
        match (piece, role) {
            (Piece::Panel(i), FaceRole::Cap { surface, .. }) => Some((SheetPart::Cap { panel: parted(i)?, surface }, None)),
            (Piece::Panel(i), FaceRole::Wall { a, b, mid }) => {
                let m = (a + b) * 0.5;
                if let Some(relief) = cut([a, mid, b]).filter(|(p, _)| matches!(p, SheetPart::Relief { .. })) {
                    return Some(relief);
                }
                match self.rims.get(i).and_then(|r| r.iter().find(|(p, q, _)| on(m, *p, *q))) {
                    Some((_, _, Some(edge))) => Some((SheetPart::Rim { panel: parted(i)?, edge: edge.clone() }, None)),
                    Some((_, _, None)) => None,
                    None => cut([a, mid, b]),
                }
            }
            (Piece::Zone(z), role) => {
                let zone = self.dev.zones.get(z)?;
                let joint = self.sheet.joints.get(zone.joint)?;
                let JointKind::Bend(bend) = &joint.kind else { return None };
                let made = key(joint.b.panel)?;
                match role {
                    // Where the bend turns toward the material, the sheet's surface runs round its
                    // outside.
                    FaceRole::Cap { surface, .. } => Some((SheetPart::Bend { panel: made, outer: surface == bend.toward_material }, None)),
                    FaceRole::Wall { a, b, mid } => {
                        let m = (a + b) * 0.5;
                        match (0..2).find(|&k| on(m, zone.near[k], zone.far[k])) {
                            Some(k) => Some((SheetPart::BendEnd { panel: made, at_origin: k == 0 }, None)),
                            None => cut([a, mid, b]),
                        }
                    }
                }
            }
            (Piece::Tab(z, _), FaceRole::Cap { surface, at }) => {
                let zone = self.dev.zones.get(z)?;
                let joint = self.sheet.joints.get(zone.joint)?;
                // Along the bend, from its anchors' origin.
                let along = (zone.near[1] - zone.near[0]).normalized().map(|d| (at - zone.near[0]).dot(d));
                Some((SheetPart::Tab { panel: key(joint.b.panel)?, surface }, along))
            }
            (Piece::Tab(..), FaceRole::Wall { a, b, mid }) => cut([a, mid, b]),
        }
    }
}

/// Where the center lines of the zones of the bends `joints` cross in the flat of `dev`: the
/// middle of their corner's relief (see `solidcraft_develop`'s corner cut). None where either
/// bend has no zone, or the two run alike.
fn corner_center(dev: &Development, joints: [usize; 2]) -> Option<Vec2> {
    let line = |j: usize| {
        let z = dev.zones.iter().find(|z| z.joint == j)?;
        let (a, b) = ((z.near[0] + z.far[0]) * 0.5, (z.near[1] + z.far[1]) * 0.5);
        Some((a, (b - a).normalized()?))
    };
    let ((p, u), (q, w)) = (line(joints[0])?, line(joints[1])?);
    let cross = u.cross(w);
    if cross.abs() < 1e-12 {
        return None;
    }
    Some(p + u * ((q - p).cross(w) / cross))
}

/// Each face's part (see [`FaceNamer::part`]) with its place among the faces made for the same
/// part: its rank, from 1, by where each starts along the part's definition, where it says. Never
/// the measure itself, which moves when sizes change: only the order is kept.
pub(crate) fn ranked(made: Vec<Option<(SheetPart, Option<f64>)>>) -> Vec<Option<SheetFace>> {
    let mut starts: std::collections::HashMap<SheetPart, Vec<f64>> = std::collections::HashMap::new();
    for (part, start) in made.iter().flatten() {
        if let Some(start) = start {
            starts.entry(part.clone()).or_default().push(*start);
        }
    }
    for v in starts.values_mut() {
        v.sort_by(f64::total_cmp);
        v.dedup();
    }
    made.into_iter()
        .map(|m| {
            m.map(|(part, start)| {
                let place = start.and_then(|s| starts.get(&part)?.iter().position(|x| *x == s)).map(|k| k + 1);
                SheetFace { part, place }
            })
        })
        .collect()
}

/// The hole of `holes` whose wall the run `run` (from its start through its middle to its end,
/// the flat's coordinates) of a sheet developed as `dev` lies along, wherever the hole now is: the
/// cut that made it, the sketch curve under the run, and where along that curve's drawn segment
/// the run starts (see [`starts_at`]). None for a run along no recorded hole, or along records
/// that disagree.
fn hole_at(sheet: &Sheet, dev: &Development, holes: &[HoleRecord], run: [Vec2; 3]) -> Option<(u64, String, f64)> {
    let small = sheet.thickness * 1e-3;
    let mut found: Option<(u64, String, f64)> = None;
    for h in holes {
        // The hole's chart is its panel's (by key), which a fold's pieces keep.
        let Some(Some(at)) = sheet.panels.iter().position(|p| p.name == h.panel).and_then(|k| dev.placements.get(k)) else { continue };
        let back = at.inverse();
        let chart = run.map(|x| back.point(x));
        let Some((_, curve, drawn)) = h.segs.iter().find(|(seg, curve, _)| !curve.is_empty() && chart.iter().all(|x| on_seg(seg, *x, small))) else {
            continue;
        };
        if let Some((feature, named, _)) = &found {
            if (*feature, named) != (h.feature, curve) {
                return None;
            }
            continue;
        }
        let start = drawn.as_ref().map_or(0.0, |g| starts_at(g, chart, small));
        found = Some((h.feature, curve.clone(), start));
    }
    found
}

/// Where along `g` (a segment of a sketch, by its own parameter from 0 to 1) the piece of it from
/// `run[0]` through `run[1]` to `run[2]` starts, going the way `g` runs: a piece of a closed curve
/// may run over its start, and starts where it does going round.
fn starts_at(g: &Seg2, run: [Vec2; 3], small: f64) -> f64 {
    let closed = g.start().dist(g.end()) <= small;
    let at = |x: Vec2| {
        let t = g.closest_param(x).clamp(0.0, 1.0);
        if closed && (t >= 1.0 || g.start().dist(x) <= small) { 0.0 } else { t }
    };
    let [a, m, b] = run.map(at);
    if !closed {
        return a.min(b);
    }
    let round = |t: f64| (t - a).rem_euclid(1.0);
    if round(m) <= round(b) { a } else { b }
}

/// Whether `p` lies on segment `s`, within `tol`.
fn on_seg(s: &Seg2, p: Vec2, tol: f64) -> bool {
    s.point_at(s.closest_param(p).clamp(0.0, 1.0)).dist(p) <= tol
}

/// The names of `body`'s faces from those of the body its operation's first operand was
/// (`before`), through the operation's history: a face kept, trimmed or split from a named face
/// keeps its name; a face the operation made has none. All none without a history.
pub(crate) fn carried<T: Clone>(before: &[Option<T>], body: &solidcraft_kernel::Body) -> Vec<Option<T>> {
    let n = body.face_count();
    let Some(h) = body.history().filter(|h| h.faces.len() == n) else { return vec![None; n] };
    h.faces
        .iter()
        .map(|p| match p.origins.as_slice() {
            [solidcore_record::brep::history::FaceOrigin::Face { operand: 0, face }] => before.get(*face).cloned().flatten(),
            _ => None,
        })
        .collect()
}

/// Each straight rim of the flat panels of `sheet`, developed as `dev` into `pieces`: the panel,
/// its edge in the panel's chart and its name there (see [`FaceNamer`]). `curves` are the sketch
/// lines that made panels' edges (see [`SurfaceSheet::sketch_curves`]).
pub(crate) fn rims(
    sheet: &Sheet,
    dev: &Development,
    pieces: &[(Piece, Region2)],
    curves: &[(String, String, Vec2, Vec2)],
    recipes: &[(String, PanelMade)],
) -> Vec<(usize, Vec2, Vec2, String)> {
    let mut out = Vec::new();
    for (i, panel) in sheet.panels.iter().enumerate() {
        let (Surface::Plane, Some(Some(at))) = (&panel.surface, dev.placements.get(i)) else { continue };
        let back = at.inverse();
        let own: Vec<Region2> = pieces.iter().filter(|(p, _)| matches!(p, Piece::Panel(k) if *k == i)).map(|(_, r)| back.region(r)).collect();
        let zones: Vec<Region2> = dev.zones.iter().map(|z| back.region(&z.region)).collect();
        out.extend(rim_names(sheet, i, &own, &zones, curves, recipes).into_iter().map(|(a, b, name, _)| (i, a, b, name)));
    }
    out
}

/// The straight edges of the outlines `own` of panel `i`'s material (in its chart), with what
/// made each (see [`rim_edges`]) and the name of its rim (see [`FaceNamer`]); a name two edges
/// would share takes the edge's place too. `far`, `side0`, `side1`, `ext0` and `ext1` on a panel a
/// bend makes, the sketch line's id on one a sketch made, `edge3` (its place in the outline) where
/// nothing says what made the edge.
fn rim_names(
    sheet: &Sheet,
    i: usize,
    own: &[Region2],
    zones: &[Region2],
    curves: &[(String, String, Vec2, Vec2)],
    recipes: &[(String, PanelMade)],
) -> Vec<(Vec2, Vec2, String, Option<RimEdge>)> {
    let edges = rim_edges(sheet, i, own, zones, curves, recipes);
    let mut out: Vec<(Vec2, Vec2, String, Option<RimEdge>)> = edges
        .into_iter()
        .map(|(a, b, edge, k)| {
            let name = match &edge {
                Some(RimEdge::Sketch { curve }) => curve.clone(),
                Some(RimEdge::Far) => "far".into(),
                Some(RimEdge::End { at_origin }) => if *at_origin { "side1" } else { "side0" }.into(),
                Some(RimEdge::Past { at_origin }) => if *at_origin { "ext1" } else { "ext0" }.into(),
                None => format!("edge{k}"),
            };
            (a, b, name, edge)
        })
        .collect();
    let names: Vec<String> = out.iter().map(|x| x.2.clone()).collect();
    for (k, x) in out.iter_mut().enumerate() {
        if names.iter().filter(|n| **n == x.2).count() > 1 {
            x.2 = format!("{}{k}", x.2);
        }
    }
    out
}

/// What made each straight edge of the outlines `own` of panel `i`'s material (in its chart), and
/// the edge's place in them. On a panel a bend makes, an edge along the bend farthest from it is
/// [`RimEdge::Far`], one across the bend's ends [`RimEdge::End`], and the stretches of one nearer
/// it past the bend's ends [`RimEdge::Past`] (none within them: the bend runs on there). On a
/// panel no bend makes, an edge lying on one of the sketch lines `curves` that made that panel's
/// edges is that line's ([`RimEdge::Sketch`]); none says what made any other. A fold's piece's
/// edges are read as its panel's (see [`root`]), in the chart it keeps.
fn rim_edges(
    sheet: &Sheet,
    i: usize,
    own: &[Region2],
    zones: &[Region2],
    curves: &[(String, String, Vec2, Vec2)],
    recipes: &[(String, PanelMade)],
) -> Vec<(Vec2, Vec2, Option<RimEdge>, usize)> {
    let r = root(sheet, recipes, i);
    let lines: Vec<(usize, Vec2, Vec2)> = own
        .iter()
        .flat_map(|r| &r.outer.segs)
        .enumerate()
        .filter_map(|(k, s)| match *s {
            Seg2::Line { a, b } if a.dist(b) > 1e-9 => Some((k, a, b)),
            _ => None,
        })
        .collect();
    let small = sheet.thickness * 1e-3;
    // An edge where a bend starts (on its zone's outline) has no rim: the bend runs on there.
    let on_zone = |a: Vec2, b: Vec2| {
        let m = (a + b) * 0.5;
        zones.iter().any(|z| {
            z.contains(m)
                || std::iter::once(&z.outer)
                    .chain(&z.holes)
                    .flat_map(|l| &l.segs)
                    .any(|s| s.point_at(s.closest_param(m).clamp(0.0, 1.0)).dist(m) <= small)
        })
    };
    let outline: Vec<(Vec2, Vec2)> = lines.iter().map(|&(_, a, b)| (a, b)).collect();
    let mut out: Vec<(Vec2, Vec2, Option<RimEdge>, usize)> = Vec::new();
    for &(k, a, b) in &lines {
        match bend_making(sheet, r) {
            None => out.push((a, b, sketch_line(sheet, curves, r, a, b), k)),
            Some((an, (lo, hi))) => {
                let along = |p: Vec2| (p - an.origin).dot(an.dir);
                match edge_of_bend(sheet, &outline, an, (lo, hi), a, b) {
                    // Nearer the bend: its stretches past the bend's ends (the bend runs on within them).
                    Some(RimEdge::Past { .. }) => {
                        let at = |s: f64| a + (b - a) * ((s - along(a)) / (along(b) - along(a)));
                        let (s0, s1) = (along(a).min(along(b)), along(a).max(along(b)));
                        if s0 < lo - small {
                            out.push((at(s0), at(s1.min(lo)), Some(RimEdge::Past { at_origin: true }), k));
                        }
                        if s1 > hi + small {
                            out.push((at(s0.max(hi)), at(s1), Some(RimEdge::Past { at_origin: false }), k));
                        }
                    }
                    Some(edge) => out.push((a, b, Some(edge), k)),
                    None => {}
                }
            }
        }
    }
    // Each stretch on its own: the stretches of an edge past a bend's ends are rims.
    out.retain(|(a, b, _, _)| !on_zone(*a, *b));
    out
}

/// The panel whose faces panel `i`'s caps and rims are pieces of: the panel a fold parted it from
/// (by its key, through every fold), whose chart a fold's piece keeps, else `i` itself.
pub(crate) fn root(sheet: &Sheet, recipes: &[(String, PanelMade)], i: usize) -> usize {
    let mut at = i;
    // Each step goes to another panel; never more steps than panels (a damaged record cannot loop).
    for _ in 0..sheet.panels.len() {
        let Some(name) = sheet.panels.get(at).map(|p| &p.name) else { break };
        let Some((_, PanelMade::Fold { from, .. })) = recipes.iter().find(|(k, _)| k == name) else { break };
        let Some(next) = sheet.panels.iter().position(|p| &p.name == from) else { break };
        at = next;
    }
    at
}

/// The anchor on panel `i` of the bend that makes it, and the span along the anchor the bend's
/// far side runs; none for a panel no bend makes.
fn bend_making(sheet: &Sheet, i: usize) -> Option<(solidcraft_develop::Anchor, (f64, f64))> {
    sheet.joints.iter().find_map(|j| match &j.kind {
        JointKind::Bend(b) if j.b.panel == i => Some((j.b, b.ends.far)),
        _ => None,
    })
}

/// The sketch line of `curves` that made the edge from `a` to `b` of panel `i` (its chart): one
/// of that panel's own (by its key) the edge lies on, within its ends.
fn sketch_line(sheet: &Sheet, curves: &[(String, String, Vec2, Vec2)], i: usize, a: Vec2, b: Vec2) -> Option<RimEdge> {
    let small = sheet.thickness * 1e-3;
    let on = |p: Vec2, s: Vec2, e: Vec2| {
        let t = e - s;
        let l = t.len();
        l > 1e-9 && ((p - s).cross(t) / l).abs() <= small && (p - s).dot(t) >= -small * l && (p - e).dot(t) <= small * l
    };
    let mine = sheet.panels.get(i).map(|p| p.name.as_str());
    curves
        .iter()
        .find(|(panel, _, s, e)| Some(panel.as_str()) == mine && on(a, *s, *e) && on(b, *s, *e))
        .map(|(_, id, _, _)| RimEdge::Sketch { curve: id.clone() })
}

/// What made the edge from `a` to `b` of a panel the bend anchored at `an` makes (its chart),
/// among the straight edges `outline` of the panel's material, the bend's far side running
/// `(lo, hi)` along the anchor: [`RimEdge::Far`] for the farthest edge along the bend,
/// [`RimEdge::End`] for one across it (at the anchors' origin where it lies nearer the origin
/// than the middle of the bend), [`RimEdge::Past`] for a nearer one reaching past the bend's
/// ends (which end the caller tells by the stretch). None for one within them.
fn edge_of_bend(sheet: &Sheet, outline: &[(Vec2, Vec2)], an: solidcraft_develop::Anchor, (lo, hi): (f64, f64), a: Vec2, b: Vec2) -> Option<RimEdge> {
    let small = sheet.thickness * 1e-3;
    let along = |p: Vec2| (p - an.origin).dot(an.dir);
    // How far into the panel from its bend an edge along the bend lies.
    let off = |a: Vec2, b: Vec2| ((a + b) * 0.5 - an.origin).cross(an.dir);
    let parallel = |a: Vec2, b: Vec2| (b - a).normalized().is_some_and(|d| d.cross(an.dir).abs() < 1e-6);
    if !parallel(a, b) {
        // Numbered along the flat as it is read, which the charts see mirrored.
        return Some(RimEdge::End { at_origin: along((a + b) * 0.5) < (lo + hi) / 2.0 });
    }
    let farthest = outline.iter().filter(|(p, q)| parallel(*p, *q)).map(|(p, q)| off(*p, *q)).fold(f64::MIN, f64::max);
    if off(a, b) >= farthest - small && off(a, b) > small {
        return Some(RimEdge::Far);
    }
    let (s0, s1) = (along(a).min(along(b)), along(a).max(along(b)));
    (s0 < lo - small || s1 > hi + small).then_some(RimEdge::Past { at_origin: s0 < lo - small })
}

impl SurfaceSheet {
    /// The rim of panel `panel` along the line through `origin` along the unit `dir` (its chart),
    /// or beside it within `band` (a bend's anchor lies inside the edge its flange was drawn on,
    /// by its position's setback), as the sheet reports it ([`SheetPart::Rim`]): the nearest
    /// straight edge of the panel's outline parallel to the line, by what made it as
    /// [`rim_edges`] tells it, read from the panel itself (no lay-out). None where no such edge
    /// is, or nothing says what made it.
    pub(crate) fn rim_part_along(&self, panel: usize, origin: Vec2, dir: Vec2, band: f64) -> Option<SheetPart> {
        let r = root(&self.sheet, &self.recipes, panel);
        let key = self.sheet.panels.get(r)?.name.clone();
        let small = (self.thickness * 1e-3).max(1e-6);
        let region = &self.sheet.panels.get(panel)?.region;
        let outline: Vec<(Vec2, Vec2)> = region
            .outer
            .segs
            .iter()
            .filter_map(|s| match *s {
                Seg2::Line { a, b } if a.dist(b) > 1e-9 => Some((a, b)),
                _ => None,
            })
            .collect();
        let off = |p: Vec2| (p - origin).cross(dir);
        let &(a, b) = outline
            .iter()
            .filter(|(a, b)| (off(*a) - off(*b)).abs() <= small && off(*a).abs() <= band + small)
            .min_by(|x, y| off(x.0).abs().total_cmp(&off(y.0).abs()))?;
        let edge = match bend_making(&self.sheet, r) {
            None => sketch_line(&self.sheet, &self.sketch_curves, r, a, b),
            Some((an, span)) => edge_of_bend(&self.sheet, &outline, an, span, a, b),
        }?;
        Some(SheetPart::Rim { panel: key, edge })
    }

    /// Keeps what names the walls of a hole the cut `feature` makes in the panel with key `panel`
    /// (see [`HoleRecord`]): `hole` is its loop in the panel's chart, cut along the profiles `drawn`
    /// of the cut's sketch `ss` (in the sketch's coordinates, which `chart` maps into the panel's
    /// chart). Each segment of the hole keeps the sketch curve its segment of `drawn` was drawn on,
    /// and that segment in the chart, which the wall's pieces are ordered along.
    pub(crate) fn name_hole(
        &mut self,
        panel: &str,
        hole: &Loop2,
        feature: u64,
        ss: &crate::SolvedSketch,
        drawn: &[Region2],
        chart: &dyn Fn(Vec2) -> Vec2,
    ) {
        let small = (self.thickness * 1e-3).max(1e-6);
        let curves = crate::config::ExactSketchCurves::new(ss);
        // Each drawn segment, and where the chart has it, running the way it was drawn: a mirrored
        // chart must not turn it round, or the wall's pieces would be ordered backward.
        let in_chart = crate::sheet_surface_convert::rigid_seg(chart);
        let mapped: Vec<(&Seg2, Seg2)> =
            drawn.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).flat_map(|l| &l.segs).map(|d| (d, in_chart(d))).collect();
        let segs = hole
            .segs
            .iter()
            .map(|h| {
                let lies = |g: &Seg2| [h.start(), h.point_at(0.5), h.end()].iter().all(|x| on_seg(g, *x, small));
                let found = mapped.iter().find_map(|(d, g)| {
                    let g = lies(g).then_some(*g)?;
                    Some((curves.find_all_along(d).into_iter().next()?, g))
                });
                match found {
                    Some((curve, g)) => (*h, curve, Some(g)),
                    None => (*h, String::new(), None),
                }
            })
            .collect();
        self.holes.push(HoleRecord { panel: panel.to_string(), feature, segs });
    }
}
