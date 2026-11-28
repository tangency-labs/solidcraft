//! What each face of a folded sheet made from surfaces (#6748) was made for. A sheet has no names
//! of its own: it reports to the model what each face of its body was made for
//! (`FaceNamer::part`, a [`SheetPart`]), and the model names the face from that, as it names
//! every feature's faces (#7990, see `lineage::sheet_lineage`).

use solidcore_record::{Loop2, Seg2, Vec2};
use solidcraft_develop::prelude::*;
use solidcraft_geom::prelude::*;

use solidcore_record::Region2;
use solidcraft_develop::{CornerRelief, Development, JointKind, Sheet, Surface};

use crate::sheet_surface::Piece;
use crate::sheet_surface_convert::{HoleRecord, PanelMade, RimEdge, SheetFace, SheetPart};

use crate::SurfaceSheet;

/// What the builder of a folded sheet made from surfaces sews a face for (see
/// `sheet_surface_fold::solid`): one of a piece's two caps, on the sheet's surface or the other
/// face, `at` the middle of the piece, a wall along the run of its outline from `a` through
/// `mid` to `b` (flat coordinates), or a face of the sheet's `bead`th bead (see
/// [`crate::sheet_surface_bead::Form`]).
#[derive(Clone, Copy, Debug)]
pub(crate) enum FaceRole {
    Cap { surface: bool, at: Vec2 },
    Wall { a: Vec2, b: Vec2, mid: Vec2 },
    Bead { bead: usize, face: solidcraft_develop::BeadFace },
}

/// What each face of a folded sheet made from surfaces (#6748) was made for, as its builder makes
/// it, read from the sheet itself (see [`SheetPart`]): never a name, which the model gives from it
/// (#7990). A face's part is what it was made for, never read back from where it lies.
pub(crate) struct FaceNamer<'a> {
    sheet: &'a Sheet,
    dev: &'a Development,
    /// What made each panel, by its key (see [`PanelMade`]).
    recipes: &'a [(String, PanelMade)],
    /// The sheet's beads, which name their own faces.
    beads: &'a [crate::SurfaceBead],
    /// Each panel's rims in the flat, and what made each (see [`rim_edges`]).
    rims: Vec<Vec<(Vec2, Vec2, Option<RimEdge>)>>,
    /// Each corner relief cut from the flat (see [`Development::cuts`]), with the panels the two
    /// bends whose corner it is make (the corner whose bends' center lines cross inside it), and
    /// where along its outline its walls are ranked from (see [`relief_start`]).
    reliefs: Vec<(&'a Region2, [String; 2], Option<(f64, f64)>)>,
}

impl<'a> FaceNamer<'a> {
    pub(crate) fn new(
        sheet: &'a Sheet,
        dev: &'a Development,
        pieces: &[(Piece, Region2)],
        curves: &[(String, String, Vec2, Vec2)],
        recipes: &'a [(String, PanelMade)],
        beads: &'a [crate::SurfaceBead],
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
        // Each relieved corner's middle, and its two bends, by the panels they make and their
        // joints, in the order of the panels' keys.
        let centers: Vec<(Vec2, [String; 2], [usize; 2])> = sheet
            .corners
            .iter()
            .filter(|c| matches!(c.relief, CornerRelief::Round | CornerRelief::Square))
            .filter_map(|c| {
                let mut bends = [(key(c.joints[0])?, c.joints[0]), (key(c.joints[1])?, c.joints[1])];
                bends.sort();
                let [(ka, ja), (kb, jb)] = bends;
                Some((corner_center(dev, c.joints)?, [ka, kb], [ja, jb]))
            })
            .collect();
        let reliefs = dev
            .cuts
            .iter()
            .filter_map(|cut| match centers.iter().filter(|(c, ..)| cut.contains(*c)).collect::<Vec<_>>().as_slice() {
                [(_, panels, joints)] => Some((cut, panels.clone(), relief_start(dev, &cut.outer.segs, *joints))),
                _ => None,
            })
            .collect();
        FaceNamer { sheet, dev, recipes, beads, rims, reliefs }
    }

    /// What the face made for `role` of `piece` was made for, as the sheet reports it to the model
    /// (see [`SheetPart`]), and where it starts along that part's definition where faces made for
    /// one part need telling apart (see [`ranked`]): a panel's cap, or a rim by what made its edge;
    /// a bend's curved face or end by the panel the bend makes; a tab's cap, along its bend; a
    /// corner relief's wall by its corner's two bends, along the relief's outline from where it
    /// meets the first bend's zone (see [`relief_start`]); a hole's wall (see [`HoleRecord`],
    /// `holes`), along its sketch curve; a bead's face by its feature and centreline (see
    /// [`crate::sheet_surface_bead::face_role`]). None for a wall nothing says what made (the model
    /// names it from geometry). A fold's piece's caps and rims are its panel's (see [`root`]): pieces of
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
            for (region, panels, start) in &self.reliefs {
                let segs = &region.outer.segs;
                let place =
                    |x: Vec2| segs.iter().enumerate().find(|(_, g)| on_seg(g, x, small)).map(|(k, g)| k as f64 + g.closest_param(x).clamp(0.0, 1.0));
                if let [Some(_), Some(m), Some(_)] = run.map(place) {
                    // Its place along the outline from where the corner's walls are ranked, the way
                    // they run (see `relief_start`), by the wall's middle: no wall runs across
                    // that start.
                    let n = segs.len() as f64;
                    let along = start.map_or(m, |(from, way)| ((m - from) * way).rem_euclid(n));
                    return Some((SheetPart::Relief { panels: panels.clone() }, Some(along)));
                }
            }
            let (part, along) = hole_at(self.sheet, self.dev, holes, run)?;
            Some((part, Some(along)))
        };
        match (piece, role) {
            (_, FaceRole::Bead { bead, face }) => {
                let b = self.beads.get(bead)?;
                Some((SheetPart::Bead { feature: b.feature, curve: b.curve.clone(), face: crate::sheet_surface_bead::face_role(face) }, None))
            }
            // A cap's pieces (a fold parts the panel) in the order of their panels' keys, which
            // say what made each (see `PanelMade`): exact, never a build order. It finds each
            // piece in the body before, and the folds' roles that name it (see `fold_cap_roles`),
            // for pieces nothing around them tells apart (a round panel's, #21300).
            (Piece::Panel(i), FaceRole::Cap { surface, .. }) => {
                let (r, mine) = (root(self.sheet, self.recipes, i), key(i)?);
                let pieces = self.sheet.panels.iter().enumerate().filter(|(k, _)| root(self.sheet, self.recipes, *k) == r);
                let before = pieces.filter(|(_, p)| p.name < mine).count();
                Some((SheetPart::Cap { panel: key(r)?, surface }, Some(before as f64)))
            }
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
                    FaceRole::Bead { .. } => None,
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

/// Where along the outline `segs` of the relief of a corner of the bends `joints` (in the order of
/// the keys of the panels they make) its walls are ranked from, a segment's index and its
/// parameter there, and which way (1 the way the outline runs, -1 against it): where the outline
/// meets the zone of the first bend in the flat of `dev`, on its edge on the panel the bend stands
/// on, going away from the zone; else on its edge on the panel it makes; else the second bend's
/// alike. Where the outline meets a zone's edge, one wall ends and the next starts, so no wall runs
/// across it, and the place is the corner's own: never the flat's axes, where a round relief's
/// outline starts. None where the outline crosses none of those edges just once.
fn relief_start(dev: &Development, segs: &[Seg2], joints: [usize; 2]) -> Option<(f64, f64)> {
    for j in joints {
        let Some(zone) = dev.zones.iter().find(|z| z.joint == j) else { continue };
        for (edge, other) in [(zone.near, zone.far), (zone.far, zone.near)] {
            let [p, q] = edge;
            let Some(along) = (q - p).normalized() else { continue };
            // Square to the edge, away from the zone (from its other edge).
            let away = along.perp() * (p - other[0]).dot(along.perp()).signum();
            let met: Vec<(usize, f64)> = segs.iter().enumerate().flat_map(|(k, g)| meets(g, p, q).into_iter().map(move |t| (k, t))).collect();
            let [(k, t)] = met.as_slice() else { continue };
            let Some(tangent) = segs.get(*k).map(|g| g.derivative(*t)) else { continue };
            // Crossing it, not touching it.
            let way = tangent.dot(away);
            if way.abs() > 1e-6 * tangent.len() {
                return Some((*k as f64 + *t, way.signum()));
            }
        }
    }
    None
}

/// Where `g`, a line or an arc (a corner relief's outline is drawn in them), crosses the segment
/// from `p` to `q`, in closed form: the parameter along `g` of each crossing, from 0 and short of
/// 1 (a crossing at a corner of the outline is the next segment's). No crossing for any other
/// curve.
fn meets(g: &Seg2, p: Vec2, q: Vec2) -> Vec<f64> {
    let d = q - p;
    let on_g = |s: f64| (0.0..1.0).contains(&s);
    match *g {
        Seg2::Line { a, b } => {
            let e = b - a;
            let den = e.cross(d);
            if den == 0.0 {
                return Vec::new();
            }
            let (s, t) = ((p - a).cross(d) / den, (p - a).cross(e) / den);
            if on_g(s) && (0.0..=1.0).contains(&t) { vec![s] } else { Vec::new() }
        }
        Seg2::Arc { center, radius, start, sweep } => {
            // The segment's points `radius` from the center: |p − center + d t|² = radius².
            let w = p - center;
            let (qa, qb, qc) = (d.len2(), 2.0 * d.dot(w), w.len2() - radius * radius);
            let disc = qb * qb - 4.0 * qa * qc;
            if !(qa > 0.0) || disc < 0.0 {
                return Vec::new();
            }
            let tau = std::f64::consts::TAU;
            [(-qb - disc.sqrt()) / (2.0 * qa), (-qb + disc.sqrt()) / (2.0 * qa)]
                .into_iter()
                .filter(|t| (0.0..=1.0).contains(t))
                .map(|t| (w + d * t).angle())
                .map(|at| if sweep > 0.0 { (at - start).rem_euclid(tau) / sweep } else { (start - at).rem_euclid(tau) / -sweep })
                .filter(|s| on_g(*s))
                .collect()
        }
        _ => Vec::new(),
    }
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
/// feature and curve or drill that made it, and where along its recorded segment the run starts
/// (see [`starts_at`]). None for a run along no recorded hole, or along records that disagree.
fn hole_at(sheet: &Sheet, dev: &Development, holes: &[HoleRecord], run: [Vec2; 3]) -> Option<(SheetPart, f64)> {
    let small = sheet.thickness * 1e-3;
    let mut found: Option<(SheetPart, f64)> = None;
    for h in holes {
        // The hole's chart is its panel's (by key), which a fold's pieces keep.
        let Some(Some(at)) = sheet.panels.iter().position(|p| p.name == h.panel).and_then(|k| dev.placements.get(k)) else { continue };
        let back = at.inverse();
        let chart = run.map(|x| back.point(x));
        let Some((_, curve, drawn)) =
            h.segs.iter().find(|(seg, curve, _)| (h.drill_role.is_some() || !curve.is_empty()) && chart.iter().all(|x| on_seg(seg, *x, small)))
        else {
            continue;
        };
        let part = match &h.drill_role {
            Some(role) => SheetPart::Drill { feature: h.feature, role: role.clone() },
            None => SheetPart::Hole { feature: h.feature, curve: curve.clone() },
        };
        if let Some((named, _)) = &found {
            if *named != part {
                return None;
            }
            continue;
        }
        let start = drawn.as_ref().map_or(0.0, |g| starts_at(g, chart, small));
        found = Some((part, start));
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
    carried_from(before, &[], body)
}

/// Carry parts from both operands when a result face has exactly one source.
pub(crate) fn carried_from<T: Clone>(before: &[Option<T>], tool: &[Option<T>], body: &solidcraft_kernel::Body) -> Vec<Option<T>> {
    let n = body.face_count();
    let Some(h) = body.history().filter(|h| h.faces.len() == n) else { return vec![None; n] };
    h.faces
        .iter()
        .map(|p| match p.origins.as_slice() {
            [solidcore_record::brep::history::FaceOrigin::Face { operand: 0, face }] => before.get(*face).cloned().flatten(),
            [solidcore_record::brep::history::FaceOrigin::Face { operand: 1, face }] => tool.get(*face).cloned().flatten(),
            _ => None,
        })
        .collect()
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
        match bend_making(sheet, r).or_else(|| contour_bend(sheet, recipes, r)) {
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

/// The folds' roles of each face of `sheet`'s body that is a piece of a panel's cap folds parted
/// (see `Lineage::cap_pieces`), `None` for any other face: for each fold that parted the panel the
/// piece is on or a panel it came from, `F<fold>:fold:<n>` where the piece is on the `n`th part
/// that fold folded (see `PanelMade::Fold`), else `F<fold>:fold:stay` (the piece was still on the
/// panel it folded, and stayed). What made each panel, never
/// where it lies. The cap's place (see [`FaceNamer::part`]) is its piece's rank among its panel's
/// pieces by key: where it can't be read (a piece without its cap), no piece of that cap has roles.
pub(crate) fn fold_cap_roles(sheet: &crate::SurfaceSheet) -> Vec<Option<Vec<String>>> {
    let recipes = &sheet.recipes;
    let panels = &sheet.sheet.panels;
    let made = |key: &str| recipes.iter().find(|(k, _)| k == key).map(|(_, m)| m);
    // The keys of each cap's pieces, in order: the pieces of the panel the cap is named after.
    let pieces_of = |root_key: &str| -> Vec<&str> {
        let Some(r) = panels.iter().position(|p| p.name == root_key) else { return Vec::new() };
        let mut keys: Vec<&str> =
            panels.iter().enumerate().filter(|(k, _)| root(&sheet.sheet, recipes, *k) == r).map(|(_, p)| p.name.as_str()).collect();
        keys.sort_unstable();
        keys
    };
    // The places each cap has, which must be one per piece.
    let mut places: std::collections::HashMap<&SheetPart, std::collections::BTreeSet<usize>> = std::collections::HashMap::new();
    for x in sheet.parts.iter().flatten() {
        if let (SheetPart::Cap { .. }, Some(place)) = (&x.part, x.place) {
            places.entry(&x.part).or_default().insert(place);
        }
    }
    // The roles of the piece on panel `key`: the panel and every panel it came from, through folds.
    let roles = |key: &str| -> Vec<String> {
        let mut chain: Vec<&str> = vec![key];
        for _ in 0..panels.len() {
            let Some(PanelMade::Fold { from, .. }) = chain.last().and_then(|k| made(k)) else { break };
            chain.push(from.as_str());
        }
        let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut folded: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
        for (k, m) in recipes {
            if let PanelMade::Fold { feature, place, .. } = m
                && chain.contains(&k.as_str())
            {
                folded.insert(*feature);
                out.insert(format!("F{feature}:{FOLD_ROLE}{place}"));
            }
        }
        // A fold of a panel the piece is on, or came from, left it where it was, unless the piece
        // had left that panel before (a fold made earlier parted it off): recipes are in the order
        // they were made.
        let made_at = |key: &str| recipes.iter().position(|(k, _)| k == key);
        for (at, (_, m)) in recipes.iter().enumerate() {
            let PanelMade::Fold { feature, from, .. } = m else { continue };
            if folded.contains(feature) {
                continue;
            }
            let Some(k) = chain.iter().position(|c| c == from) else { continue };
            let left_after = match k.checked_sub(1).and_then(|c| chain.get(c)) {
                None => true,
                Some(child) => made_at(child).is_some_and(|c| c > at),
            };
            if left_after {
                out.insert(format!("F{feature}:{FOLD_ROLE}stay"));
            }
        }
        out.into_iter().collect()
    };
    sheet
        .parts
        .iter()
        .map(|x| {
            let x = x.as_ref()?;
            let (SheetPart::Cap { panel, .. }, Some(place)) = (&x.part, x.place) else { return None };
            let keys = pieces_of(panel);
            if keys.len() < 2 || places.get(&x.part).map(std::collections::BTreeSet::len) != Some(keys.len()) {
                return None;
            }
            let roles = roles(keys.get(place.checked_sub(1)?)?);
            (!roles.is_empty()).then_some(roles)
        })
        .collect()
}

/// What starts a fold's role of a piece of a panel's cap (`F30:fold:1`, see [`fold_cap_roles`]):
/// no face has it.
pub(crate) const FOLD_ROLE: &str = "fold:";

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

/// On a contour flange's first panel, which no bend makes, the anchor on it of the bend into the
/// contour's next panel, and the span along the anchor the bend's near side runs: its rims are what
/// that bend leaves, as a later panel's are what the bend that makes it leaves (its far edge, its
/// two ends), so none is left for the model to name from geometry. None for any other panel, a
/// later flange's bend on the panel never counting (a rim keeps its name when one is added).
fn contour_bend(sheet: &Sheet, recipes: &[(String, PanelMade)], i: usize) -> Option<(solidcraft_develop::Anchor, (f64, f64))> {
    let made = |k: usize| sheet.panels.get(k).and_then(|p| recipes.iter().find(|(key, _)| *key == p.name)).map(|(_, m)| m);
    let Some(PanelMade::Contour { feature, .. }) = made(i) else { return None };
    sheet.joints.iter().find_map(|j| match &j.kind {
        JointKind::Bend(b) if j.a.panel == i && matches!(made(j.b.panel), Some(PanelMade::Contour { feature: f, .. }) if f == feature) => {
            Some((j.a, b.ends.near))
        }
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
    // How far into the panel from its bend an edge along the bend lies: the panel's material lies
    // on one side of its anchor (a bend's second panel's on the right, its first's on the left).
    let side = |a: Vec2, b: Vec2| ((a + b) * 0.5 - an.origin).cross(an.dir);
    let sign = if outline.iter().map(|(p, q)| side(*p, *q)).sum::<f64>() < 0.0 { -1.0 } else { 1.0 };
    let off = |a: Vec2, b: Vec2| side(a, b) * sign;
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
        let edge = match bend_making(&self.sheet, r).or_else(|| contour_bend(&self.sheet, &self.recipes, r)) {
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
        self.holes.push(HoleRecord { panel: panel.to_string(), feature, segs, drill_role: None });
    }
}
