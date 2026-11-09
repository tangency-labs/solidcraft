//! Sheet metal features on a sheet made from surfaces (#6748): each changes the sheet's panels and
//! joints, and the body is built again from them. An edge flange is a new panel bent from a free
//! edge of the sheet; a hem, a new panel turned back over the sheet by half a turn; a fold, a flat
//! panel cut in two along a line and bent there, its bend zone taken from the panel's own
//! material so that the flat stays as drawn. Where two
//! flanges meet at a corner of the panel they stand on, their surfaces meet along a line, and a
//! rip parts them there: the corner is the sheet's like any other, its bends ending with the
//! corner's relief.
//!
//! Not built yet: flanges on part of an edge, and flanges on a curved edge or a roll's edge.

use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_develop::Cut;
use solidcraft_develop::{Anchor, Bend, Datum, Joint, JointKind, Lap, Panel, Relief, Rigid3, Rip, Sheet, Surface, Tail, ZoneEnds, develop};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::BoolOp;

use crate::sheet::HeightFrom;
use crate::{DocError, FoldSide, Result, SurfaceSheet};

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(DocError::Invalid(msg.into()))
}

/// A free edge of the sheet, where a pick found it: the panel, the edge's ends in the panel's
/// chart (its material on the left running from `a` to `b`), and whether the pick was on the
/// sheet's other face (the one `thickness` into the material).
#[derive(Clone, Copy, Debug)]
pub struct FreeEdge {
    pub panel: usize,
    pub a: Vec2,
    pub b: Vec2,
    pub far_face: bool,
}

/// A straight free edge to bend from: its ends and its way along it.
fn straight(e: &FreeEdge) -> Result<(Vec2, Vec2, Vec2)> {
    let d = (e.b - e.a).normalized().ok_or_else(|| DocError::Invalid("the edge has no length".into()))?;
    Ok((e.a, e.b, d))
}

impl SurfaceSheet {
    /// The free edge of the sheet nearest the point `p` (the body's coordinates), on a face of a
    /// planar panel or its rim: one no joint runs along. `None` when no such edge is within twice
    /// the thickness of `p`; refused where a bend's edge is nearer than any free one.
    pub fn free_edge_at(&self, p: Vec3) -> Result<Option<FreeEdge>> {
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let folded = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        let t = self.sheet.thickness;
        let mut best: Option<(f64, FreeEdge)> = None;
        let mut bent = f64::INFINITY;
        for (i, (panel, place)) in self.sheet.panels.iter().zip(&folded.places).enumerate() {
            let (Surface::Plane, Some(place)) = (&panel.surface, place) else { continue };
            let q = place.inverse().point(p);
            if !(q.z >= -t - 1e-6 && q.z <= 2.0 * t + 1e-6) {
                continue;
            }
            let at = Vec2::new(q.x, q.y);
            for l in std::iter::once(&panel.region.outer).chain(&panel.region.holes) {
                for s in &l.segs {
                    let Seg2::Line { a, b } = *s else { continue };
                    let d = s.point_at(s.closest_param(at).clamp(0.0, 1.0)).dist(at);
                    let off = (q.z.clamp(0.0, t) - q.z).abs();
                    let dist = (d * d + off * off).sqrt();
                    if let Some(j) = self.joint_along(i, a, b) {
                        if matches!(j.kind, JointKind::Bend(_)) {
                            bent = bent.min(dist);
                        }
                        continue;
                    }
                    if dist <= t * 2.0 + 1e-3 && best.as_ref().is_none_or(|(x, _)| dist < *x) {
                        best = Some((dist, FreeEdge { panel: i, a, b, far_face: q.z > t / 2.0 }));
                    }
                }
            }
        }
        if bent <= t * 2.0 + 1e-3 && best.as_ref().is_none_or(|(x, _)| bent < *x) {
            return err("that is a bend line, not a free edge of the sheet: pick a free edge");
        }
        Ok(best.map(|(_, e)| e))
    }

    /// Keeps the flanges of the bends `made` (a feature's, its corners closed) clear of the panel
    /// beside their own at each end no other flange closed: where a rip parts their panel from
    /// another at that end, and the flange lies on that panel's side of its own. A wall leaning in
    /// over the base leans in over a return on the wall beside it. The wall's own end keeps half
    /// the gap off the mitre between the two, which leaves it |n_q − n_p|·gap/2 from the other's
    /// face (n_p, n_q: the faces' normals toward the flange); the flange is cut back until it keeps
    /// that much too. Its bend stops square, and its panel's side becomes one line in the flat
    /// through what the panel's start and its far edge need, neither less than nothing; it is
    /// never run on. A flange of an earlier feature is left as it is.
    pub fn clear_of_neighbours(&mut self, made: &[usize]) -> Result<()> {
        let t = self.sheet.thickness;
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let places = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?.places;
        let place = |i: usize| places.get(i).copied().flatten();
        let plane = |i: usize| matches!(self.sheet.panels.get(i).map(|x| &x.surface), Some(Surface::Plane));
        let size = self.sheet.panels.iter().map(|x| x.region.outer.polyline(1e-3).bounds().size().len()).fold(1.0, f64::max);
        let tol = size * 1e-7;
        let mut cuts = Vec::new();
        for &j in made {
            let Some(Joint { a: on_p, b: on_f, kind: JointKind::Bend(nb) }) = self.sheet.joints.get(j).cloned() else { continue };
            let (p, f) = (on_p.panel, on_f.panel);
            let (Some(pp), Some(pf)) = (place(p), place(f)) else { continue };
            if !(plane(p) && plane(f)) {
                continue;
            }
            for end in [0.0, nb.length] {
                let (vp, vf) = (on_p.origin + on_p.dir * end, on_f.origin + on_f.dir * end);
                // Closed: a rip parts the flange from another there.
                let ends_at = |side: &[Vec2], v: Vec2| side.len() > 1 && [side.first(), side.last()].into_iter().flatten().any(|e| e.dist(v) <= tol);
                let closed = self.sheet.joints.iter().any(|x| match &x.kind {
                    JointKind::Rip(r) => (x.a.panel == f && ends_at(&r.edges.0, vf)) || (x.b.panel == f && ends_at(&r.edges.1, vf)),
                    _ => false,
                });
                if closed {
                    continue;
                }
                // The panel beside, parted from the flange's own by a rip that passes the bend's
                // end within its reach (its sharp may lie inside the edge it stands on).
                let reach = (nb.radius + t) * (nb.angle / 2.0).tan() + t;
                let passes = |side: &[Vec2]| side.windows(2).any(|w| dist_to_segment(vp, w[0], w[1]) <= reach);
                let beside = self.sheet.joints.iter().find_map(|x| match &x.kind {
                    JointKind::Rip(r) if x.a.panel == p && passes(&r.edges.0) => Some(x.b.panel),
                    JointKind::Rip(r) if x.b.panel == p && passes(&r.edges.1) => Some(x.a.panel),
                    _ => None,
                });
                let Some(q) = beside else { continue };
                let Some(pq) = place(q) else { continue };
                if q == f || !plane(q) {
                    continue;
                }
                // Which side of a panel's material a point lies on: 1 past its face `t` in, -1
                // before its surface, 0 within.
                let side = |pl: &Rigid3, x: Vec3| {
                    let z = (x - pl.shift).dot(pl.z);
                    if z > t {
                        1.0
                    } else if z < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                };
                let height = self.sheet.panels.get(f).map_or(0.0, |x| reach_from(&x.region, (on_f.origin, -on_f.dir.perp())));
                let mid_f = pf.point((on_f.origin + on_f.dir * (nb.length / 2.0) - on_f.dir.perp() * (height / 2.0)).extend(0.0));
                let q_box = self.sheet.panels.get(q).map(|x| x.region.outer.polyline(1e-3).bounds());
                let Some(mid_q) = q_box.map(|b| pq.point(((b.min + b.max) * 0.5).extend(0.0))) else { continue };
                // Only a flange on the other panel's side of its own runs into it.
                let (sp, sq) = (
                    side(&pp, mid_f),
                    side(&pq, pp.point(on_p.origin.extend(0.0)).lerp(pp.point((on_p.origin + on_p.dir * nb.length).extend(0.0)), 0.5)),
                );
                if sp == 0.0 || sq == 0.0 || side(&pp, mid_q) != sp {
                    continue;
                }
                let (n_p, n_q) = (pp.z * sp, pq.z * sq);
                let face_q = pq.shift + if sq > 0.0 { pq.z * t } else { Vec3::ZERO };
                let keep = (n_q - n_p).len() * self.reliefs.gap.max(0.0) / 2.0;
                let clear = |x: Vec3| n_q.dot(x - face_q) - keep;
                // The bend, cut back square along its edge from the corner.
                let way = if end == 0.0 { on_p.dir } else { -on_p.dir };
                let v = pp.point(vp.extend(0.0));
                let d = pp.vector(way.extend(0.0));
                let Some(u_bend) =
                    crate::sheet_surface_convert::bend_cut(v, d, n_q, (-pp.z, -pf.z), nb.toward_material, nb.radius, t, 2.0 * -clear(v))
                else {
                    continue;
                };
                // The panel: how far back each of its faces is clear, at its start (the bend's
                // tangent line) and at its far edge.
                let (along, into) = (if end == 0.0 { on_f.dir } else { -on_f.dir }, -on_f.dir.perp());
                let at = |u: f64, y: f64| {
                    let x = vf + along * u + into * y;
                    let (c0, c1) = (clear(pf.point(x.extend(0.0))), clear(pf.point(x.extend(t))));
                    c0.min(c1)
                };
                let per_u = at(1.0, 0.0) - at(0.0, 0.0);
                if !(per_u > 1e-9) {
                    continue;
                }
                let start = nb.setback(t).min(height);
                let need = |y: f64| (-at(0.0, y) / per_u).max(0.0);
                let (u0, u1) = (need(start), need(height));
                if !(u_bend > 1e-9 || u0 > 1e-9 || u1 > 1e-9) {
                    continue;
                }
                // Panel and bend ending together stay one straight edge.
                let u0 = if (u0 - u_bend).abs() < 1e-9 { u_bend } else { u0 };
                let side = (u0 > 1e-9 || u1 > 1e-9).then(|| (vf + along * u0 + into * start, vf + along * u1 + into * height));
                cuts.push((j, end, u_bend, side));
            }
        }
        // The panels first, along their anchors as they are; then the bends.
        for &(j, end, _, side) in &cuts {
            let (Some((a, b)), Some(Joint { b: on_f, .. })) = (side, self.sheet.joints.get(j).cloned()) else { continue };
            let Some(m) = (b - a).normalized() else { continue };
            let Some(s) = cross_lines((on_f.origin, on_f.dir), (a, m)).map(|c| (c - on_f.origin).dot(on_f.dir)) else { continue };
            let Some(region) = self.sheet.panels.get(on_f.panel).map(|x| x.region.clone()) else { continue };
            let Some((region, _)) = to_line(&region, &on_f, -1.0, end, s, m)? else {
                return err("a flange runs into the panel beside its own, and its side can't be cut back");
            };
            if let Some(x) = self.sheet.panels.get_mut(on_f.panel) {
                x.region = region;
            }
        }
        for &(j, end, u, _) in cuts.iter().rev() {
            if let Some(Joint { a, b, kind: JointKind::Bend(bend) }) = self.sheet.joints.get_mut(j) {
                if !(bend.length - u > 1e-6) {
                    return err("a flange runs into the panel beside its own: keeping clear of it would cut its bend away");
                }
                let at = if end == 0.0 { u } else { bend.length - u };
                to_corner(a, b, bend, at, end);
            }
        }
        Ok(())
    }

    /// The joint that runs along the edge `a`–`b` of panel `i`: its anchor line holds both ends,
    /// or, a bend's, the edge lies beside the bend in the strip its band takes, up to a thickness
    /// past its anchor away from the panel's material (where the bend's sharp was drawn inside the
    /// panel's edge).
    fn joint_along(&self, i: usize, a: Vec2, b: Vec2) -> Option<&Joint> {
        let t = self.sheet.thickness;
        self.sheet.joints.iter().find(|j| {
            let (band, length) = match &j.kind {
                JointKind::Bend(bend) => (t, Some(bend.length)),
                _ => (0.0, None),
            };
            // The `a` anchor's material lies on its left, the `b` anchor's on its right.
            [(&j.a, 1.0), (&j.b, -1.0)].into_iter().any(|(an, side)| {
                let off = |p: Vec2| -side * an.dir.cross(p - an.origin);
                let (oa, ob) = (off(a), off(b));
                let along = (a.lerp(b, 0.5) - an.origin).dot(an.dir);
                let beside = length.is_none_or(|l| along >= -1e-6 && along <= l + 1e-6);
                an.panel == i && (oa - ob).abs() <= 1e-6 && oa >= -1e-6 && oa <= band + 1e-6 && beside
            })
        })
    }

    /// Moves the edge `a`–`b` of panel `i` `by` mm outward (inward where negative): the panel
    /// gains, or loses, the strip between. At each end the line of the outline beside the edge
    /// is followed to the edge's new line (on a slanted edge the strip is a parallelogram); where
    /// that line is round, or runs nearly along the edge, the end moves square to it.
    fn move_edge(&mut self, i: usize, (a, b, d): (Vec2, Vec2, Vec2), by: f64) -> Result<()> {
        if by.abs() <= 1e-9 {
            return Ok(());
        }
        let (p, q) = self.moved_ends(i, (a, b, d), by);
        let panel = self.sheet.panels.get(i).ok_or_else(|| DocError::Invalid("panel".into()))?;
        let strip = if by > 0.0 { [a, b, q, p] } else { [p, q, b, a] };
        let strip = Region2 { outer: Loop2::polygon(&strip).ccw(), holes: Vec::new() };
        let op = if by > 0.0 { BoolOp::Union } else { BoolOp::Cut };
        let mut out =
            crate::sheet_surface::boolean_past_holes(std::slice::from_ref(&panel.region), &[strip], op, &mut crate::sheet::FlatWork::new())?;
        if out.len() != 1 {
            return err("the flange's bend would cut its panel apart");
        }
        if let (Some(r), Some(panel)) = (out.pop(), self.sheet.panels.get_mut(i)) {
            panel.region = r;
        }
        Ok(())
    }

    /// Where the ends of the edge `a`–`b` of panel `i` land with the edge moved `by` mm outward
    /// (inward where negative): at each end the line of the outline beside the edge is followed
    /// to the edge's new line; where that line is round, or runs nearly along the edge, the end
    /// moves square to it.
    fn moved_ends(&self, i: usize, (a, b, d): (Vec2, Vec2, Vec2), by: f64) -> (Vec2, Vec2) {
        let out = Vec2::new(d.y, -d.x);
        let (k0, k1) = self.edge_slants(i, (a, b, d));
        (a + d * (k0 * by) + out * by, b + d * (k1 * by) + out * by)
    }

    /// How far along the edge `a`–`b` (way `d`) of panel `i` the lines of the outline beside it
    /// run for each mm out from it, at its start and its end: 0 where they are square to it,
    /// round, or run nearly along it.
    fn edge_slants(&self, i: usize, (a, b, d): (Vec2, Vec2, Vec2)) -> (f64, f64) {
        let out = Vec2::new(d.y, -d.x);
        let Some(panel) = self.sheet.panels.get(i) else { return (0.0, 0.0) };
        let segs = &panel.region.outer.segs;
        let n = segs.len();
        let Some(k) = segs.iter().position(|s| matches!(*s, Seg2::Line { a: x, b: y } if x.dist(a) <= 1e-9 && y.dist(b) <= 1e-9)) else {
            return (0.0, 0.0);
        };
        let slant = |s: Option<&Seg2>| -> f64 {
            let Some(Seg2::Line { a: x, b: y }) = s.copied() else { return 0.0 };
            let Some(w) = (y - x).normalized() else { return 0.0 };
            let across = w.dot(out);
            if across.abs() < 0.1 { 0.0 } else { w.dot(d) / across }
        };
        (slant(segs.get((k + n - 1) % n)), slant(segs.get((k + 1) % n)))
    }

    /// A new flat panel `len` along and `depth` deep, joined to a free edge by `bend` with its
    /// anchors at `origin` along `d`: the panel's own chart puts the joint on its x axis, its
    /// material below.
    fn add_panel(&mut self, kind: &str, edge: &FreeEdge, origin: Vec2, d: Vec2, len: f64, depth: f64, bend: Bend) -> usize {
        let n = self.sheet.panels.len();
        let rect = [Vec2::new(0.0, 0.0), Vec2::new(0.0, -depth), Vec2::new(len, -depth), Vec2::new(len, 0.0)];
        let region = Region2 { outer: Loop2::polygon(&rect).ccw(), holes: Vec::new() };
        self.sheet.panels.push(Panel { name: format!("{kind} {n}"), surface: Surface::Plane, region });
        self.places.push(None);
        self.sheet.joints.push(Joint {
            a: Anchor { panel: edge.panel, origin, dir: d },
            b: Anchor { panel: n, origin: Vec2::ZERO, dir: Vec2::new(1.0, 0.0) },
            kind: JointKind::Bend(bend),
        });
        n
    }

    /// An edge flange on the free edge at `pick`: `angle` (radians, more than 0 and less than a
    /// half turn) at inside radius `radius`, its height measured from `from`, the bend placed on
    /// the edge as `position` says (as a flange-built sheet places it: inside, outside, middle,
    /// material_outside or tangent). It rises on the side of the face picked: away from the
    /// sheet's material when the pick is on the sheet's surface (as a flange-built sheet's
    /// flanges rise from its top), toward it when the pick is on the face a thickness in; `flip`
    /// turns it the other way. Where it meets another flange at a corner of the panel, the two close
    /// the corner (see [`SurfaceSheet::close_corners`]). Its bend's ends are settled when the
    /// sheet is built. The new bend's joint.
    #[allow(clippy::too_many_arguments)]
    pub fn add_flange(&mut self, pick: Vec3, height: f64, angle: f64, radius: f64, position: &str, from: HeightFrom, flip: bool) -> Result<usize> {
        let edge = self
            .free_edge_at(pick)?
            .ok_or_else(|| DocError::Invalid("pick a free edge of the sheet (one no bend, seam or cut runs along)".into()))?;
        let joint = self.add_flange_on(edge, height, angle, radius, position, from, flip)?;
        self.clear_of_neighbours(&[joint])?;
        Ok(joint)
    }

    /// [`SurfaceSheet::add_flange`] on an edge already found.
    #[allow(clippy::too_many_arguments)]
    pub fn add_flange_on(
        &mut self,
        edge: FreeEdge,
        height: f64,
        angle: f64,
        radius: f64,
        position: &str,
        from: HeightFrom,
        flip: bool,
    ) -> Result<usize> {
        let joint = self.flange_on(edge, height, angle, radius, position, from, flip)?;
        self.close_corners(joint)?;
        Ok(joint)
    }

    /// Closes the corners of the bends `made`, a feature's flanges all added first (see
    /// [`SurfaceSheet::flange_on`]), each against the bends before it (see `close_corners`).
    pub fn close_flange_corners(&mut self, made: &[usize]) -> Result<()> {
        for &j in made {
            self.close_corners(j)?;
        }
        Ok(())
    }

    /// The flange of [`SurfaceSheet::add_flange_on`] with its corners not closed yet. A feature on
    /// several edges adds all its flanges first and then closes their corners
    /// ([`SurfaceSheet::close_flange_corners`]), so a flange that stands on another's side edge
    /// (side flanges across a bend) is there when the corner the three bends make is closed.
    #[allow(clippy::too_many_arguments)]
    pub fn flange_on(&mut self, edge: FreeEdge, height: f64, angle: f64, radius: f64, position: &str, from: HeightFrom, flip: bool) -> Result<usize> {
        if !(angle > 1e-6 && angle <= std::f64::consts::PI - 1e-6) {
            return err("the flange angle must be between 0 and 180°");
        }
        let (a, b, d) = straight(&edge)?;
        let on = |l: &Loop2| l.segs.iter().any(|s| matches!(*s, Seg2::Line { a: x, b: y } if x.dist(a) <= 1e-9 && y.dist(b) <= 1e-9));
        if self.sheet.panels.get(edge.panel).is_some_and(|x| x.region.holes.iter().any(on)) {
            return err("not supported yet: a flange on the edge of a hole");
        }
        let t = self.sheet.thickness;
        let half = (angle / 2.0).tan();
        // How far inside the edge the bend starts, as a flange-built sheet has it.
        let start = match position {
            "inside" | "" => (radius + t) * half,
            "outside" => 0.0,
            "middle" | "mid" => (radius + t) * half / 2.0,
            "material_outside" => radius * half,
            "tangent" if angle < std::f64::consts::FRAC_PI_2 => (radius + t) * angle.sin(),
            "tangent" => radius + t,
            other => return err(format!("unknown bend position `{other}` (inside, outside, middle, material_outside or tangent)")),
        };
        // On the side of the face picked: toward the material from the face a thickness in.
        let toward = edge.far_face != flip;
        // The panels' surface is the bend's outside where it turns toward the material: the
        // anchors mark that surface's virtual sharp, this far past the bend's start.
        let sharp = if toward { (radius + t) * half } else { radius * half };
        let leg = height - from.before_leg(angle, radius, t);
        if !(leg >= -1e-9) {
            let datum = match from {
                HeightFrom::Outer => String::new(),
                other => format!(" from {}", other.datum()),
            };
            return err(format!("the flange height{datum} must be at least {:.3} mm for this bend", from.before_leg(angle, radius, t)));
        }
        let by = sharp - start;
        // The anchor runs where the edge's neighbours meet its line moved to the sharp (on a
        // slanted edge, not square across from its ends).
        let (from, to) = self.moved_ends(edge.panel, (a, b, d), by);
        // A sharp inside the edge by no more than the thickness needs no cut: the bend's band runs
        // a thickness past its anchor and takes that strip, leaving the panel's other edges (and
        // the rips along them) as they are.
        if by > 0.0 || by < -t - 1e-9 {
            self.move_edge(edge.panel, (a, b, d), by)?;
        }
        let len = (to - from).dot(d);
        if !(len > 1e-9) {
            return err("the edge's neighbours leave its flange no length at its bend");
        }
        let bend = Bend {
            angle,
            radius,
            toward_material: toward,
            datum: Datum::Sharp,
            length: len,
            ends: ZoneEnds { near: (0.0, len), far: (0.0, len) },
            tails: (Tail::Keep, Tail::Keep),
            relief: (Relief::None, Relief::None),
        };
        // Its sides carry on the edges beside it, as laid out in the flat with the bend: through
        // the anchor's ends on the plate, `slant` along for each mm out (square where those
        // edges are). The flange's own anchor lies `allowance - 2·setback` out in the flat.
        let (k0, k1) = self.edge_slants(edge.panel, (a, b, d));
        let depth = leg.max(0.0) + sharp;
        let base = bend.allowance(t, self.sheet.k_factor) - 2.0 * bend.setback(t);
        let at = |y: f64| (k0 * (base - y), len + k1 * (base - y));
        let ((p0, p1), (q0, q1)) = (at(0.0), at(-depth));
        if !(p1 - p0 > 1e-9 && q1 - q0 > 1e-9) {
            return err("the edges beside it close the flange before it reaches its height");
        }
        let n = self.add_panel("flange", &edge, from, d, len, depth, bend);
        if let Some(panel) = self.sheet.panels.get_mut(n) {
            let corners = [Vec2::new(p0, 0.0), Vec2::new(q0, -depth), Vec2::new(q1, -depth), Vec2::new(p1, 0.0)];
            panel.region = Region2 { outer: Loop2::polygon(&corners).ccw(), holes: Vec::new() };
        }
        Ok(self.sheet.joints.len() - 1)
    }

    /// Closes the corners the bend `j`, a flange's just added, makes with the other bends standing
    /// on its panel, or on a panel its panel meets at a rip (a return beside another, on two walls
    /// mitred at a corner). Two bends whose anchors cross near an end of each, at a corner the
    /// panel's material fills (on one panel; across a rip, where their lines meet in space), with
    /// their flanges rising to the same side, share that corner: their flanges' surfaces meet along
    /// the line where their planes cross. Each bend's edge then runs
    /// to the corner and each flange to that line (see `to_line`), and a rip parts the two flanges
    /// along it, as far as both reach. Whatever made the other bend (Convert, Thicken or another
    /// flange), its panel is met alike. A panel whose side at the corner is no straight edge from
    /// its bend keeps it, and the corner stays open.
    ///
    /// Three bends can meet at one corner: a wall's bend on a panel, a flange on the panel's edge
    /// beside it, and a flange on the wall's end edge (side flanges across a bend). There neither
    /// new flange is closed against the other's panel, whose side at the corner the third bend
    /// holds; the two new flanges are closed against each other, across the wall's bend, and that
    /// bend runs to the corner too. Only bends before `j` are met: a later flange of the same
    /// feature meets `j` when its own corners are closed.
    fn close_corners(&mut self, j: usize) -> Result<()> {
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let places = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?.places;
        close_corners_on(&mut self.sheet, &places, self.reliefs.gap, j, None)
    }

    /// A hem on the free edge at `pick`: half a turn at the edge back over the face opposite the
    /// one picked (over it when `flip`), its leg `gap` off that face (the bend's inside radius is
    /// half the gap), `length` to its outer face. The new panel's index.
    /// Folds the sheet along the line from `a` to `b` (the body's coordinates) on a face of one of
    /// its flat panels: the panel is cut in two there, its bend zone where `position` puts it from
    /// the line (`centerline`, `start`, `end` or `mould`, its outside mould line), and the side
    /// `stays` (left or right of the line as drawn, seen from the sheet's surface: see
    /// [`FoldSide`]) stays put while the other bends by `angle` at `radius`, toward the material
    /// or, `flip`, away from it, with every panel joined beyond it. The zone is the panel's own
    /// material, so the flat stays as drawn: where the outline crosses the zone on a slanted or a
    /// curved edge, the zone's end follows it, and where a rip of the panel (a wall mitred to the
    /// next) crosses the zone, it follows the rip's cut; a fold that would turn the wall into the
    /// next one is refused. On the side that moves, only the pieces the drawn line reaches fold
    /// (a tab beside them on the same line stays). A fold that would leave the side that stays in
    /// pieces, or join the side that moves to it again beyond the bend, is refused: the side was
    /// chosen when the fold was made ([`SurfaceSheet::fold_side`]) so that it does not, and it is
    /// never chosen again. Returns the fold's joint.
    #[allow(clippy::too_many_arguments)]
    pub fn add_fold(&mut self, a: Vec3, b: Vec3, angle: f64, radius: f64, position: &str, flip: bool, stays: FoldSide) -> Result<usize> {
        let f = self.fold_setup(a, b, angle, radius, flip)?;
        let n = if stays == FoldSide::Left { f.across } else { -f.across };
        match self.plan_fold(f.i, (&f.place, f.places.as_slice()), &f.region, f.line, n, &f.bend, position)? {
            Ok(plan) => Ok(self.apply_fold(f.i, plan)),
            Err(why) => err(why),
        }
    }

    /// Which side of the fold line from `a` to `b` stays put (see [`SurfaceSheet::add_fold`]),
    /// decided once, when a fold is made: the side `fixed` lies on; else the side with more of the
    /// sheet joined to it, the panel's own material there and every panel bends and seams join to
    /// the panel on that side (on a panel bent from another, the side toward that bend; on a lone
    /// panel, the larger side), a tie keeping the line's left. Where that side would fall apart,
    /// or the side that moves would join it again beyond the bend (a tab on an inside edge of an
    /// L, the fixed point on the far leg), the other side's pieces the line reaches fold instead:
    /// the other side is the one that stays, so long as the fixed point's material does (#639).
    #[allow(clippy::too_many_arguments)]
    pub fn fold_side(&self, a: Vec3, b: Vec3, angle: f64, radius: f64, position: &str, flip: bool, fixed: Option<Vec3>) -> Result<FoldSide> {
        let f = self.fold_setup(a, b, angle, radius, flip)?;
        let from = f.line[0].dot(f.across);
        let fixed = fixed.map(|p| {
            let q = f.place.inverse().point(p);
            Vec2::new(q.x, q.y)
        });
        // The side that moves lies along `n` from the line.
        let moves_across = match fixed {
            Some(q) => q.dot(f.across) < from,
            None => {
                let (along, against) = self.held_beside(&f, from)?;
                along <= against
            }
        };
        let n = if moves_across { f.across } else { -f.across };
        let at = (&f.place, f.places.as_slice());
        let n = match self.plan_fold(f.i, at, &f.region, f.line, n, &f.bend, position)? {
            Ok(_) => n,
            Err(why) => match self.plan_fold(f.i, at, &f.region, f.line, -n, &f.bend, position) {
                Ok(Ok(plan)) if fixed.is_none_or(|q| !plan.moves.iter().any(|m| m.region.contains(q))) => -n,
                _ => return err(why),
            },
        };
        // The side that stays lies against `n`; the line's left, seen from the surface (against
        // the panel's material normal), lies against `across`.
        Ok(if n.dot(f.across) > 0.0 { FoldSide::Left } else { FoldSide::Right })
    }

    /// What a fold works on, whichever side stays (see [`SurfaceSheet::add_fold`]).
    fn fold_setup(&self, a: Vec3, b: Vec3, angle: f64, radius: f64, flip: bool) -> Result<FoldSetup> {
        if !(angle > 1e-6 && angle <= std::f64::consts::PI - 1e-6) {
            return err("the fold angle must be between 0 and 180°");
        }
        if !(radius >= 0.0) {
            return err("the bend radius must not be negative");
        }
        let t = self.sheet.thickness;
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let folded = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        // The panel the line lies on: both its ends on one face of the panel, the line across it.
        let on_face = |z: f64| z.abs() < 1e-4 || (z - t).abs() < 1e-4;
        // Where the line lies across panels in one plane (coplanar panels side by side), the one
        // its drawn stretch crosses; which to fold is never a matter of the panels' order, so
        // more than one is refused.
        let mut found = Vec::new();
        for (i, (panel, place)) in self.sheet.panels.iter().zip(&folded.places).enumerate() {
            let (Surface::Plane, Some(place)) = (&panel.surface, place) else { continue };
            let back = place.inverse();
            let (qa, qb) = (back.point(a), back.point(b));
            if !(on_face(qa.z) && on_face(qb.z) && (qa.z - qb.z).abs() < 1e-4) {
                continue;
            }
            let (a2, b2) = (Vec2::new(qa.x, qa.y), Vec2::new(qb.x, qb.y));
            let Some(d) = (b2 - a2).normalized() else { return err("the fold line has no length") };
            if let Some((lo, hi)) = crate::sheet_surface_convert::extent_across(&panel.region, (a2, d)) {
                let drawn_across = hi >= -1e-9 && lo <= a2.dist(b2) + 1e-9;
                found.push(((i, *place, [a2, b2], d), drawn_across));
            }
        }
        if found.len() > 1 {
            found.retain(|(_, drawn_across)| *drawn_across);
        }
        let (i, place, line, d) = match found.as_slice() {
            [(one, _)] => *one,
            [] => return err("the fold line must lie on a flat face of the sheet"),
            _ => return err("not supported yet: a fold line drawn across more than one panel"),
        };
        let region = self.sheet.panels[i].region.clone();
        let bend = Bend {
            angle,
            radius,
            // Up from the sheet's surface, its top, as a flange rises; toward the material when
            // flipped.
            toward_material: flip,
            datum: Datum::Tangent,
            length: 0.0,
            ends: ZoneEnds { near: (0.0, 0.0), far: (0.0, 0.0) },
            tails: (Tail::Keep, Tail::Keep),
            relief: (Relief::None, Relief::None),
        };
        let flat_area = dev.panels.iter().map(|p| p.as_ref().map_or(0.0, |p| p.region.area())).collect();
        Ok(FoldSetup { i, place, places: folded.places, region, line, across: d.perp(), bend, flat_area })
    }

    /// How much of the sheet lies on either side of a fold's line `x · across = from` in its
    /// panel's chart: the panel's own material there, and the flat material of every panel that
    /// bends and seams join to the panel on that side, through any panels beyond it (a rip holds
    /// nothing). Along `across` first.
    fn held_beside(&self, f: &FoldSetup, from: f64) -> Result<(f64, f64)> {
        let n = f.across;
        let mut held = [area(&half(&f.region, n, from)?), area(&half(&f.region, -n, -from)?)];
        for (slot, sign) in held.iter_mut().zip([1.0, -1.0]) {
            let mut queue: Vec<usize> = Vec::new();
            for joint in &self.sheet.joints {
                let length = match &joint.kind {
                    JointKind::Bend(b) => b.length,
                    JointKind::Seam => 0.0,
                    JointKind::Rip(_) => continue,
                };
                for (an, other) in [(&joint.a, joint.b.panel), (&joint.b, joint.a.panel)] {
                    let mid = an.origin + an.dir * (length / 2.0);
                    if an.panel == f.i && other != f.i && sign * (mid.dot(n) - from) > 0.0 {
                        queue.push(other);
                    }
                }
            }
            let mut reached = std::collections::BTreeSet::new();
            while let Some(p) = queue.pop() {
                if p == f.i || !reached.insert(p) {
                    continue;
                }
                for joint in self.sheet.joints.iter().filter(|j| !matches!(j.kind, JointKind::Rip(_))) {
                    if joint.a.panel == p {
                        queue.push(joint.b.panel);
                    } else if joint.b.panel == p {
                        queue.push(joint.a.panel);
                    }
                }
            }
            *slot += reached.iter().filter_map(|p| f.flat_area.get(*p)).sum::<f64>();
        }
        let [along, against] = held;
        Ok((along, against))
    }

    /// How a fold folds panel `i` (its region `region`; where it and every panel lie in space,
    /// `at`) along `line` (in the panel's chart), the side along `n` moving: worked out before
    /// anything of the sheet changes (see [`SurfaceSheet::add_fold`]). `Ok(Err(why))` where that
    /// side can't be the one that moves, the other side perhaps can: the side that stays falls
    /// apart, or a piece that would fold hangs from two pieces of the zone (it joins the rest
    /// again beyond the bend).
    #[allow(clippy::too_many_arguments)]
    fn plan_fold(
        &self,
        i: usize,
        (here, places): (&Rigid3, &[Option<Rigid3>]),
        region: &Region2,
        line: [Vec2; 2],
        n: Vec2,
        bend: &Bend,
        position: &str,
    ) -> Result<std::result::Result<FoldPlan, &'static str>> {
        let t = self.sheet.thickness;
        let ba = bend.allowance(t, self.sheet.k_factor);
        let l = line[0].dot(n);
        let s0 = match position {
            "centerline" | "center" | "centre" => l - ba / 2.0,
            "start" => l,
            "end" => l - ba,
            "mould" | "mold" => l - (bend.radius + t) * (bend.angle / 2.0).tan(),
            o => return err(format!("unknown fold position `{o}` (centerline, start, end or mould)")),
        };
        // The zone runs from the line `x · n = s0` (on the side that stays) to `s0 + ba`; its
        // anchors run along `u`, the side that stays on their left.
        let u = n.perp();
        let tol = (ba.max(t) * 1e-6).max(1e-9);
        let side = |q: Vec2| q.dot(n) - s0;
        let moving = half(region, n, s0 + ba)?;
        if moving.is_empty() {
            return err("the fold leaves nothing on its moving side");
        }
        // The zone, piece by piece: what the outline holds between the zone's lines (a cut-out
        // across it is cut from it below, and parts nothing). A piece of no width, where an edge
        // of the panel runs along a line of the zone, is none.
        let outline = Region2 { outer: region.outer.clone(), holes: Vec::new() };
        let parts: Vec<Region2> = band(&outline, n, s0, s0 + ba)?
            .into_iter()
            .filter(|p| {
                let (lo, hi) = extent(p, n);
                hi - lo > tol
            })
            .collect();
        if parts.is_empty() {
            return err("the fold's bend zone runs off its panel");
        }
        let near: Vec<Vec<(f64, f64)>> = parts.iter().map(|p| edges_on(p, (n, u), s0, tol)).collect();
        let far: Vec<Vec<(f64, f64)>> = parts.iter().map(|p| edges_on(p, (n, u), s0 + ba, tol)).collect();
        if near.iter().chain(&far).any(Vec::is_empty) {
            return err("not supported yet: material in the fold's bend zone that only one side of the bend holds");
        }
        // Which piece of the zone each piece beyond it hangs from.
        let mut hangs: Vec<Option<usize>> = vec![None; parts.len()];
        for (k, m) in moving.iter().enumerate() {
            let ends = edges_on(m, (n, u), s0 + ba, tol);
            let from: Vec<usize> = (0..parts.len()).filter(|p| far.get(*p).is_some_and(|f| overlap(f, &ends, tol))).collect();
            match from.as_slice() {
                [] => return err("the fold's bend zone runs off its panel"),
                [p] => match hangs.get_mut(*p) {
                    Some(slot) if slot.is_none() => *slot = Some(k),
                    _ => return err("not supported yet: a fold whose bend zone holds two pieces beyond it"),
                },
                _ => return Ok(Err(FOLD_TABS_REJOIN)),
            }
        }
        if hangs.iter().any(Option::is_none) {
            return err("not supported yet: a fold whose bend zone holds nothing beyond it");
        }
        // The pieces the drawn line reaches fold, each on its own bend; a tab beside them on the
        // same line, the line not drawn across it, stays (#264).
        let drawn = {
            let (p, q) = (line[0].dot(u), line[1].dot(u));
            (p.min(q), p.max(q))
        };
        let spans: Vec<(f64, f64)> = parts.iter().map(|p| extent(p, u)).collect();
        let piece_spans: Vec<(f64, f64)> = moving.iter().map(|m| extent(m, u)).collect();
        // The pieces in their order along the line as drawn (from its first point to its second),
        // never as the planar booleans list them: each new panel's place among them is its ordinal.
        let way = line[1] - line[0];
        let mut order: Vec<usize> = (0..parts.len()).collect();
        order.sort_by(|&x, &y| {
            let at = |p: usize| spans.get(p).map_or(0.0, |s| if way.dot(u) >= 0.0 { s.0 } else { -s.1 });
            at(x).total_cmp(&at(y))
        });
        let first = self.sheet.panels.len();
        let mut moves: Vec<FoldPiece> = Vec::new();
        for p in order {
            let (Some(part), Some(&(lo, hi))) = (parts.get(p), spans.get(p)) else { continue };
            if !(lo <= drawn.1 + 1e-9 && hi >= drawn.0 - 1e-9) {
                continue;
            }
            let Some(&Some(k)) = hangs.get(p) else { continue };
            // The zone's reach along its line: the piece's, and past a curved end a little more,
            // so that the curve's bulge between the chords it was measured on lies inside; what
            // lies past the outline is cut off again below.
            let past = if part.outer.segs.iter().all(|s| matches!(s, Seg2::Line { .. })) { 0.0 } else { CURVE_REACH };
            let (lo, hi) = (lo - past, hi + past);
            let (plo, phi) = piece_spans.get(k).copied().unwrap_or((lo, hi));
            let span = (lo.min(plo), hi.max(phi));
            // Nothing else of the panel lies beside it along the line.
            let clash = |s: &(f64, f64)| s.0 < span.1 - tol && s.1 > span.0 + tol;
            if spans.iter().enumerate().any(|(q, s)| q != p && clash(s)) || piece_spans.iter().enumerate().any(|(q, s)| q != k && clash(s)) {
                return err("not supported yet: a fold whose bend zone's pieces lie beside each other along it");
            }
            let mut bend = bend.clone();
            bend.length = hi - lo;
            bend.ends = ZoneEnds { near: (0.0, hi - lo), far: (0.0, hi - lo) };
            let piece = moving.get(k).cloned().unwrap_or_default();
            moves.push(FoldPiece { panel: first + moves.len(), region: piece, bend, start: lo, span });
        }
        if moves.is_empty() {
            return err("the fold leaves nothing on its moving side");
        }
        let one = |r: Vec<Region2>| -> Result<std::result::Result<Region2, &'static str>> {
            match <[Region2; 1]>::try_from(r) {
                Ok([one]) => Ok(Ok(one)),
                Err(r) if r.is_empty() => err("the fold leaves nothing on its fixed side"),
                Err(_) => Ok(Err(FOLD_FIXED_APART)),
            }
        };
        let stays = if moves.len() == parts.len() {
            one(half(region, -n, -s0)?)?
        } else {
            // What the line does not reach stays with the fixed side, its zone too.
            let big = {
                let bb = region.outer.polyline(1e-3).bounds();
                (bb.size().len() + bb.min.len() + bb.max.len() + 1.0) * 4.0
            };
            let boxes: Vec<Region2> = moves
                .iter()
                .map(|m| {
                    let (lo, hi) = m.span;
                    let corners = [n * s0 + u * lo, n * s0 + u * hi, n * (s0 + big) + u * hi, n * (s0 + big) + u * lo];
                    Region2 { outer: Loop2::polygon(&corners).ccw(), holes: Vec::new() }
                })
                .collect();
            one(solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(region), &boxes, BoolOp::Cut, 1e-9)?)?
        };
        let stays = match stays {
            Ok(stays) => stays,
            Err(why) => return Ok(Err(why)),
        };
        // What each zone lacks of its reach (the outline past a slanted or a curved end, a cut-out
        // across it) is cut from the flat: the zone is the panel's own material, the flat as drawn.
        // A sliver of no width, where an end of the reach lies on a straight edge, is none.
        let mut cuts = Vec::new();
        for m in &moves {
            let (lo, hi) = (m.start, m.start + m.bend.length);
            let reach = [n * s0 + u * lo, n * s0 + u * hi, n * (s0 + ba) + u * hi, n * (s0 + ba) + u * lo];
            let reach = Region2 { outer: Loop2::polygon(&reach).ccw(), holes: Vec::new() };
            let lacks = solidcraft_kernel::planar_regions_boolean(&[reach], std::slice::from_ref(region), BoolOp::Cut, 1e-9)?;
            cuts.extend(lacks.into_iter().filter(|g| width(g) > SLIVER));
        }
        // What lay on the moving side goes with it: joints, cut-outs across joints, recesses and
        // loose pieces there; one in the zone itself has no one place, but for a rip, which parts
        // where the zone crosses it. What lies on a piece the line does not reach stays.
        let piece_at = |q: Vec2| moves.iter().find(|m| q.dot(u) >= m.span.0 - tol && q.dot(u) <= m.span.1 + tol).map(|m| m.panel);
        let mut anchors = Vec::new();
        // The rips the zone parts: the joint, its two stretches, and the panel beyond it with the
        // panel's cut edge on the piece that folds (a point of it and the way into the material).
        let mut rips = Vec::new();
        let mut beside = Vec::new();
        for (k, joint) in self.sheet.joints.iter().enumerate() {
            match &joint.kind {
                JointKind::Rip(rip) => {
                    for (on_a, an, edge) in [(true, &joint.a, &rip.edges.0), (false, &joint.b, &rip.edges.1)] {
                        if an.panel != i {
                            continue;
                        }
                        let s: Vec<f64> = edge.iter().map(|q| side(*q)).collect();
                        if s.iter().all(|x| *x >= ba - tol) {
                            if let Some(p) = edge.first().and_then(|q| piece_at(*q)) {
                                anchors.push((k, on_a, p));
                            }
                        } else if !s.iter().all(|x| *x <= tol) {
                            let folds = edge.iter().find(|q| side(**q) >= ba - tol).and_then(|q| piece_at(*q));
                            let parted =
                                folds.filter(|_| joint.a.panel != joint.b.panel).and_then(|p| part_rip(joint, rip, on_a, p, (n, s0, ba), t, tol));
                            let (Some(p), Some((kept, moved, lap, cut_edge))) = (folds, parted) else { return err(ZONE_ON_JOINT) };
                            rips.push((k, kept, moved));
                            cuts.extend(lap);
                            beside.push((p, if on_a { joint.b.panel } else { joint.a.panel }, cut_edge));
                        }
                    }
                }
                kind => {
                    let length = match kind {
                        JointKind::Bend(b) => b.length,
                        _ => 0.0,
                    };
                    for (on_a, an) in [(true, &joint.a), (false, &joint.b)] {
                        if an.panel != i {
                            continue;
                        }
                        let (s, e) = (side(an.origin), side(an.origin + an.dir * length));
                        if s.min(e) >= ba - tol {
                            if let Some(p) = piece_at(an.origin) {
                                anchors.push((k, on_a, p));
                            }
                        } else if s.max(e) > tol {
                            return err(if matches!(kind, JointKind::Bend(_)) { FOLD_THROUGH_BEND } else { ZONE_ON_JOINT });
                        }
                    }
                }
            }
        }
        // A piece that folds must keep clear of the panel beyond each rip it parts from (a wall
        // mitred to the next): turning into it is refused. Where that panel is joined to the piece
        // by bends beyond it, it turns with the piece, and what stays turns the other way against
        // both: so the side that turns against the other panel is the piece's, or else the side
        // that stays.
        for &(p, other, _) in &beside {
            let Some(m) = moves.iter().find(|m| m.panel == p) else { continue };
            let mut rides = std::collections::BTreeSet::new();
            let mut queue: Vec<usize> = anchors
                .iter()
                .filter(|(_, _, q)| *q == p)
                .filter_map(|&(k, on_a, _)| {
                    let j = self.sheet.joints.get(k).filter(|j| !matches!(j.kind, JointKind::Rip(_)))?;
                    Some(if on_a { j.b.panel } else { j.a.panel })
                })
                .collect();
            while let Some(q) = queue.pop() {
                if q == i || !rides.insert(q) {
                    continue;
                }
                for j in self.sheet.joints.iter().filter(|j| !matches!(j.kind, JointKind::Rip(_))) {
                    if j.a.panel == q {
                        queue.push(j.b.panel);
                    } else if j.b.panel == q {
                        queue.push(j.a.panel);
                    }
                }
            }
            let turns_piece = !rides.contains(&other);
            // What turns, as the rips' laps leave it, and the zone less what is cut from it.
            let mut turning = vec![if turns_piece { m.region.clone() } else { stays.clone() }];
            for &(_, _, (at, into)) in beside.iter().filter(|b| b.0 == p) {
                turning = turning.iter().map(|r| half(r, into, at.dot(into))).collect::<Result<Vec<_>>>()?.concat();
            }
            let (lo, hi) = (m.start, m.start + m.bend.length);
            let reach = [n * s0 + u * lo, n * s0 + u * hi, n * (s0 + ba) + u * hi, n * (s0 + ba) + u * lo];
            let mut zone = vec![Region2 { outer: Loop2::polygon(&reach).ccw(), holes: Vec::new() }];
            for c in &cuts {
                zone = solidcraft_kernel::planar_regions_boolean(&zone, std::slice::from_ref(c), BoolOp::Cut, 1e-9)?;
            }
            let joint = Joint {
                a: Anchor { panel: i, origin: n * s0 + u * m.start, dir: u },
                b: Anchor { panel: p, origin: n * (s0 + ba) + u * m.start, dir: u },
                kind: JointKind::Bend(m.bend.clone()),
            };
            let there = places.get(other).copied().flatten().zip(self.sheet.panels.get(other)).filter(|(_, x)| x.surface == Surface::Plane);
            let clear = match (folded_joint(t, self.sheet.k_factor, &joint, *here, turns_piece), there) {
                (Some((turned, bent)), Some((there, x))) => {
                    turning.iter().all(|r| keeps_clear(&zone, r, (&bent, turned, *here), bend.toward_material, &x.region, there, t) == Some(true))
                }
                _ => false,
            };
            if !clear {
                return err(FOLD_INTO_NEIGHBOR);
            }
        }
        // Each outline of `of` (its points, where it is the panel's) that lies past the zone, by
        // its index: on the piece it goes with. One across the zone stays, or is refused as
        // `across` says.
        let beyond = |of: Vec<Option<Vec<Vec2>>>, across: Option<&str>| -> Result<Vec<(usize, usize)>> {
            let mut out = Vec::new();
            for (k, pts) in of.into_iter().enumerate() {
                let Some(pts) = pts else { continue };
                let s: Vec<f64> = pts.iter().map(|q| side(*q)).collect();
                if s.iter().all(|x| *x >= ba - tol) {
                    if let Some(p) = pts.first().and_then(|q| piece_at(*q)) {
                        out.push((k, p));
                    }
                } else if let Some(what) = across.filter(|_| !s.iter().all(|x| *x <= tol)) {
                    return err(format!("the fold's bend zone runs through {what}: not supported yet"));
                }
            }
            Ok(out)
        };
        let starts = |mine: bool, l: &Loop2| mine.then(|| l.segs.iter().map(|g| g.start()).collect::<Vec<Vec2>>());
        let moved_cuts = beyond(self.sheet.cuts.iter().map(|c| starts(c.panel == i, &c.region.outer)).collect(), None)?;
        let recesses = beyond(self.recesses.iter().map(|r| starts(r.panel == i, &r.region.outer)).collect(), Some("a recess"))?;
        let loose = beyond(self.loose.iter().map(|l| starts(l.panel == i, &l.region.outer)).collect(), Some("a loose piece of the sheet"))?;
        Ok(Ok(FoldPlan { n, u, s0, ba, stays, moves, cuts, anchors, rips, moved_cuts, recesses, loose }))
    }

    /// Makes the fold `plan` of panel `i` (see [`SurfaceSheet::add_fold`]). Returns the fold's
    /// first joint.
    fn apply_fold(&mut self, i: usize, plan: FoldPlan) -> usize {
        let FoldPlan { n, u, s0, ba, stays, moves, cuts, anchors, rips, moved_cuts, recesses, loose } = plan;
        for (k, on_a, p) in anchors {
            if let Some(joint) = self.sheet.joints.get_mut(k) {
                if on_a {
                    joint.a.panel = p;
                } else {
                    joint.b.panel = p;
                }
            }
        }
        for (k, kept, _) in &rips {
            if let Some(joint) = self.sheet.joints.get_mut(*k) {
                *joint = kept.clone();
            }
        }
        for (k, p) in moved_cuts {
            if let Some(c) = self.sheet.cuts.get_mut(k) {
                c.panel = p;
            }
        }
        for (k, p) in recesses {
            if let Some(r) = self.recesses.get_mut(k) {
                r.panel = p;
            }
        }
        for (k, p) in loose {
            if let Some(l) = self.loose.get_mut(k) {
                l.panel = p;
            }
        }
        self.sheet.cuts.extend(cuts.into_iter().map(|region| Cut { panel: i, region }));
        if let Some(panel) = self.sheet.panels.get_mut(i) {
            panel.region = stays;
        }
        let first = self.sheet.panels.len();
        let j = self.sheet.joints.len();
        for (k, m) in moves.into_iter().enumerate() {
            // Named by its place along the fold's line (see `plan_fold`); the feature names it.
            self.sheet.panels.push(Panel { name: format!("fold.{k}"), surface: Surface::Plane, region: m.region });
            self.places.push(None);
            self.sheet.joints.push(Joint {
                a: Anchor { panel: i, origin: n * s0 + u * m.start, dir: u },
                b: Anchor { panel: m.panel, origin: n * (s0 + ba) + u * m.start, dir: u },
                kind: JointKind::Bend(m.bend),
            });
        }
        let moved_all: Vec<usize> = (first..self.sheet.panels.len()).collect();
        let made: Vec<usize> = (j..self.sheet.joints.len()).collect();
        // A rip's stretch beyond the zone goes with the piece it lies on.
        self.sheet.joints.extend(rips.into_iter().map(|(_, _, moved)| moved));
        // Every panel joined beyond the moving side follows the fold: by bends and seams, as a rip
        // holds nothing (the panel beyond a mitre stays where its own bend holds it).
        let mut reached: std::collections::BTreeSet<usize> = moved_all.iter().copied().collect();
        let mut queue = moved_all.clone();
        while let Some(p) = queue.pop() {
            for (k, joint) in self.sheet.joints.iter().enumerate() {
                if made.contains(&k) || matches!(joint.kind, JointKind::Rip(_)) {
                    continue;
                }
                let other = if joint.a.panel == p {
                    joint.b.panel
                } else if joint.b.panel == p {
                    joint.a.panel
                } else {
                    continue;
                };
                if other != i && reached.insert(other) {
                    queue.push(other);
                }
            }
        }
        for p in reached {
            if let Some(place) = self.places.get_mut(p) {
                *place = None;
            }
        }
        j
    }

    /// The sheet of a copy of its body, named `body`, by `m`: moved, or mirrored into the opposite
    /// hand. A mirror turns each panel's chart over (v → −v) so that its frame stays
    /// right-handed with the material where it was: every outline, anchor, cut and record in the
    /// charts and the flat is mirrored with it, and each joint runs the other way. None for a
    /// transform that is no rigid motion or mirror, and for a mirror of a sheet with a roll.
    pub fn copied(&self, body: &str, m: &crate::Mat) -> Option<SurfaceSheet> {
        let axis = |i: usize| m.get(i).map(|c| Vec3::new(c[0], c[1], c[2]));
        let (x, y, z) = (axis(0)?, axis(1)?, axis(2)?);
        let unit = |v: Vec3| (v.dot(v) - 1.0).abs() < 1e-6;
        let isometry = m.iter().flatten().all(|v| v.is_finite())
            && unit(x)
            && unit(y)
            && unit(z)
            && x.dot(y).abs() < 1e-6
            && x.dot(z).abs() < 1e-6
            && y.dot(z).abs() < 1e-6;
        if !isometry {
            return None;
        }
        let mirror = x.cross(y).dot(z) < 0.0;
        if mirror && self.sheet.panels.iter().any(|p| !matches!(p.surface, Surface::Plane)) {
            return None;
        }
        let place = |r: Rigid3| {
            let (x, y, z) = (crate::apply_vector(m, r.x), crate::apply_vector(m, r.y), crate::apply_vector(m, r.z));
            Rigid3 { x, y: if mirror { y * -1.0 } else { y }, z, shift: crate::apply_point(m, r.shift) }
        };
        let mut out = self.clone();
        out.body = body.to_string();
        out.places = self.places.iter().map(|p| p.map(place)).collect();
        out.flat = place(self.flat);
        if mirror {
            out.turn_charts_over();
        }
        Some(out)
    }

    /// Every chart and the flat turned over (v → −v): see [`Self::copied`].
    fn turn_charts_over(&mut self) {
        let f = |p: Vec2| Vec2::new(p.x, -p.y);
        let r = |g: &Region2| crate::sheet_surface_convert::rigid_region(g, f);
        let seg =
            |s: &Seg2| crate::sheet_surface_convert::rigid_region(&Region2 { outer: Loop2 { segs: vec![*s] }, holes: Vec::new() }, f).outer.segs;
        for p in &mut self.sheet.panels {
            p.region = r(&p.region);
        }
        for j in &mut self.sheet.joints {
            // Each joint runs the other way, from its far end, so each panel's material stays on
            // its side of it.
            let turn = |a: &mut Anchor, length: f64| {
                a.origin = f(a.origin + a.dir * length);
                a.dir = Vec2::new(-a.dir.x, a.dir.y);
            };
            match &mut j.kind {
                JointKind::Bend(b) => {
                    let l = b.length;
                    turn(&mut j.a, l);
                    turn(&mut j.b, l);
                    b.ends = ZoneEnds { near: (l - b.ends.near.1, l - b.ends.near.0), far: (l - b.ends.far.1, l - b.ends.far.0) };
                    b.tails = (b.tails.1, b.tails.0);
                    b.relief = (b.relief.1, b.relief.0);
                }
                JointKind::Seam => {
                    turn(&mut j.a, 0.0);
                    turn(&mut j.b, 0.0);
                }
                JointKind::Rip(rip) => {
                    let along = |a: &Anchor, last: Option<&Vec2>| last.map_or(0.0, |q| (*q - a.origin).dot(a.dir));
                    let (la, lb) = (along(&j.a, rip.edges.0.last()), along(&j.b, rip.edges.1.last()));
                    turn(&mut j.a, la);
                    turn(&mut j.b, lb);
                    for e in [&mut rip.edges.0, &mut rip.edges.1] {
                        *e = e.iter().rev().map(|p| f(*p)).collect();
                    }
                    for c in [&mut rip.curves.0, &mut rip.curves.1] {
                        *c = c.iter().rev().flat_map(|s| seg(&s.reversed())).collect();
                    }
                    rip.turns.reverse();
                }
            }
        }
        for c in &mut self.sheet.cuts {
            c.region = r(&c.region);
        }
        for x in &mut self.recesses {
            x.region = r(&x.region);
        }
        for l in &mut self.loose {
            l.region = r(&l.region);
        }
        // The flat lies in the first panel's chart: it turns over with it.
        self.regions = self.regions.iter().map(r).collect();
        for b in &mut self.bends {
            b.zone = r(&b.zone);
            (b.start, b.end) = (f(b.start), f(b.end));
            b.tabs = b.tabs.iter().map(r).collect();
        }
        for l in &mut self.roll_lines {
            (l.start, l.end) = (f(l.start), f(l.end));
        }
        for p in &mut self.pockets {
            p.region = r(&p.region);
        }
        self.labels = self.labels.iter().map(r).collect();
        for p in &mut self.perforations {
            if let Some(o) = &mut p.outline {
                *o = o.iter().map(r).collect();
            }
        }
    }

    /// Each flat panel's material as the flat has it (after its bends' trims), in its chart, with
    /// its frame in the body; and a box round every piece of the sheet in the body (each flat
    /// panel's material, each bend's zone, each loose piece), the panel's index for a panel's.
    #[allow(clippy::type_complexity)]
    pub(crate) fn pieces_in_space(&self) -> Option<(Vec<(usize, Rigid3, Region2)>, Vec<(Option<usize>, solidcraft_geom::Aabb3)>)> {
        let dev = self.develop().ok()?;
        let frames = crate::sheet_surface_cuts::frames(self, &dev).ok()?;
        let fold = if self.unfolded { None } else { Some(crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places).ok()?) };
        let pieces = crate::sheet_surface::pieces(&dev, &mut crate::sheet::FlatWork::new()).ok()?;
        let t = self.thickness;
        let mut panels = Vec::new();
        let mut boxes = Vec::new();
        let slab = |frame: &Rigid3, r: &Region2| {
            let mut b = solidcraft_geom::Aabb3::EMPTY;
            for p in &r.outer.polyline(1e-3) {
                for z in [0.0, t] {
                    b.add(frame.point(Vec3::new(p.x, p.y, z)));
                }
            }
            b
        };
        for (piece, region) in &pieces {
            match *piece {
                crate::sheet_surface::Piece::Panel(i) | crate::sheet_surface::Piece::Tab(_, i) => {
                    let (Some(frame), Some(Some(at))) = (frames.get(i).copied().flatten(), dev.placements.get(i)) else { return None };
                    let own = at.inverse().region(region);
                    boxes.push((matches!(piece, crate::sheet_surface::Piece::Panel(_)).then_some(i), slab(&frame, &own)));
                    if matches!(piece, crate::sheet_surface::Piece::Panel(_)) && matches!(self.sheet.panels.get(i)?.surface, Surface::Plane) {
                        panels.push((i, frame, own));
                    }
                }
                crate::sheet_surface::Piece::Zone(z) => {
                    // The zone's outline, finely: its arc bulges between its corners.
                    let zone = dev.zones.get(z)?;
                    let mut b = solidcraft_geom::Aabb3::EMPTY;
                    let pts = region.outer.polyline(1e-3);
                    for (p, q) in pts.edges() {
                        for k in 0..=16 {
                            let x = p.lerp(*q, k as f64 / 16.0);
                            for depth in [0.0, t] {
                                match fold.as_ref() {
                                    Some(fold) => b.add(fold.bends.get(zone.joint).copied().flatten()?.point(x, depth)),
                                    None => b.add(self.flat.point(Vec3::new(x.x, x.y, depth))),
                                }
                            }
                        }
                    }
                    boxes.push((None, b));
                }
            }
        }
        for l in &self.loose {
            let frame = frames.get(l.panel).copied().flatten()?;
            boxes.push((None, slab(&frame, &l.region)));
        }
        Some((panels, boxes))
    }

    /// Whether point `p` (the body's coordinates) touches the sheet's material, within `tol`: in a
    /// flat panel's material through its thickness, in a bend, or in a loose piece.
    pub(crate) fn touches(&self, p: Vec3, tol: f64) -> bool {
        let near = |r: &Region2, q: Vec2| {
            r.contains(q)
                || std::iter::once(&r.outer)
                    .chain(&r.holes)
                    .flat_map(|l| &l.segs)
                    .any(|s| s.point_at(s.closest_param(q).clamp(0.0, 1.0)).dist(q) <= tol)
        };
        let t = self.thickness;
        let Some((panels, _)) = self.pieces_in_space() else { return true };
        for (_, frame, own) in &panels {
            let q = frame.inverse().point(p);
            if q.z >= -tol && q.z <= t + tol && near(own, Vec2::new(q.x, q.y)) {
                return true;
            }
        }
        let Ok(dev) = self.develop() else { return true };
        let Ok(frames) = crate::sheet_surface_cuts::frames(self, &dev) else { return true };
        for l in &self.loose {
            let Some(frame) = frames.get(l.panel).copied().flatten() else { continue };
            let q = frame.inverse().point(p);
            if q.z >= -tol && q.z <= t + tol && near(&l.region, Vec2::new(q.x, q.y)) {
                return true;
            }
        }
        if self.unfolded {
            return false;
        }
        let Ok(fold) = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places) else { return true };
        dev.zones.iter().any(|z| {
            fold.bends
                .get(z.joint)
                .copied()
                .flatten()
                .and_then(|bent| bent.unwrapped(p))
                .is_some_and(|(flat, depth, _)| depth >= -tol && depth <= t + tol && near(&z.region, flat))
        })
    }

    /// Whether the line from `a` to `b` lies on a face of one of the sheet's flat panels, across
    /// it: what [`Self::add_fold`] folds.
    pub fn fold_panel_at(&self, a: Vec3, b: Vec3) -> bool {
        let t = self.sheet.thickness;
        let Ok(dev) = self.develop() else { return false };
        let Ok(folded) = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places) else { return false };
        let on_face = |z: f64| z.abs() < 1e-4 || (z - t).abs() < 1e-4;
        self.sheet.panels.iter().zip(&folded.places).any(|(panel, place)| {
            let (Surface::Plane, Some(place)) = (&panel.surface, place) else { return false };
            let back = place.inverse();
            let (qa, qb) = (back.point(a), back.point(b));
            let (a2, b2) = (Vec2::new(qa.x, qa.y), Vec2::new(qb.x, qb.y));
            on_face(qa.z)
                && on_face(qb.z)
                && (b2 - a2).normalized().is_some_and(|d| crate::sheet_surface_convert::extent_across(&panel.region, (a2, d)).is_some())
        })
    }

    /// The far edge of the flange whose bend is joint `j`: its panel's straight edge farthest from
    /// the bend, run with the panel's material on its left; where a further bend starts.
    pub fn far_edge(&self, j: usize) -> Result<FreeEdge> {
        let joint = self.sheet.joints.get(j).ok_or_else(|| DocError::Invalid("the flange has no bend".into()))?;
        let an = joint.b;
        let panel = self.sheet.panels.get(an.panel).ok_or_else(|| DocError::Invalid("the flange has no panel".into()))?;
        // How far into the flange's material from its bend (its material lies right of its anchor).
        let into = |q: Vec2| (q - an.origin).cross(an.dir);
        panel
            .region
            .outer
            .segs
            .iter()
            .filter_map(|s| match *s {
                Seg2::Line { a, b } => Some((into(a).min(into(b)), a, b)),
                _ => None,
            })
            .max_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, a, b)| FreeEdge { panel: an.panel, a, b, far_face: false })
            .ok_or_else(|| DocError::Invalid("the flange has no far edge to bend again".into()))
    }

    /// The middle of the far edge (see [`Self::far_edge`]) of the flange on panel `panel`, on the
    /// sheet's surface, where its body has it: a pick for a further bend. `None` for a panel no
    /// bend stands on.
    pub fn far_edge_point(&self, panel: usize) -> Option<Vec3> {
        let j = self.sheet.joints.iter().position(|jt| jt.b.panel == panel && matches!(jt.kind, JointKind::Bend(_)))?;
        let e = self.far_edge(j).ok()?;
        let dev = self.develop().ok()?;
        let folded = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places).ok()?;
        let m = (e.a + e.b) * 0.5;
        Some(folded.places.get(panel).copied().flatten()?.point(Vec3::new(m.x, m.y, 0.0)))
    }

    /// A path flange: a chain of edge flanges on the free edge at `pick` following `path`, an open
    /// chain of points (the body's coordinates) drawn on a plane across the edge, from the end
    /// nearer the edge. Each line is one flange, its length the flange's height to the outer face
    /// from the virtual sharp, and each corner one bend: the first line's angle to the sheet makes
    /// the first bend, the turn between two lines the next, which way it turns which way it
    /// bends. Returns each flange's bend joint, in order.
    pub fn add_path_flange(&mut self, pick: Vec3, path: &[Vec3], radius: f64) -> Result<Vec<usize>> {
        let max = crate::sheet::MAX_PATH_LINES;
        if path.len() < 2 || path.len() > max + 1 {
            return err(format!("a path flange's path must have 1…{max} lines"));
        }
        if !path.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
            return err("the path flange's path is not finite");
        }
        let edge = self.free_edge_at(pick)?.ok_or_else(|| DocError::Invalid("pick an edge of the sheet's top or bottom face".into()))?;
        // The path says which way each bend turns, from the surface, whichever face was picked.
        let edge = FreeEdge { far_face: false, ..edge };
        let (a, _, d) = straight(&edge)?;
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let folded = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        let place = folded.places.get(edge.panel).copied().flatten().ok_or_else(|| DocError::Invalid("the edge's panel has no place".into()))?;
        // The edge's frame: along it, out of the sheet past it, and up from its top (the surface).
        let at = place.point(Vec3::new(a.x, a.y, 0.0));
        let dir = |v: Vec3| place.vector(v);
        let (along, out, up) = (dir(Vec3::new(d.x, d.y, 0.0)), dir(Vec3::new(d.y, -d.x, 0.0)), dir(Vec3::new(0.0, 0.0, -1.0)));
        let off = |p: Vec3| {
            let q = p - at;
            (q - along * q.dot(along)).len()
        };
        let path: Vec<Vec3> = match (path.first(), path.last()) {
            (Some(&f), Some(&l)) if off(l) < off(f) => path.iter().rev().copied().collect(),
            _ => path.to_vec(),
        };
        // Each line's length and its heading in the plane across the edge (out, up).
        let mut lines: Vec<(f64, f64)> = Vec::with_capacity(path.len() - 1);
        for (k, pair) in path.windows(2).enumerate() {
            let [p, q] = pair else { continue };
            let v = *q - *p;
            let len = v.len();
            if !(len > 1e-6) {
                return err(format!("line {} of the path flange's path has no length", k + 1));
            }
            if v.dot(along).abs() > len * 1e-4 {
                return err("draw the path flange's path on a plane across the edge (square to it)");
            }
            let heading = v.dot(up).atan2(v.dot(out));
            // A line straight on from the one before lengthens it.
            match lines.last_mut() {
                Some((l, h)) if (heading - *h).abs() < 1e-9 => *l += len,
                _ => lines.push((len, heading)),
            }
        }
        // Each bend's turn: the first from the sheet, the others from the line before; up from the
        // surface is positive.
        let mut turns = Vec::with_capacity(lines.len());
        let mut before = 0.0;
        for (k, &(len, heading)) in lines.iter().enumerate() {
            let mut turn = heading - before;
            while turn > std::f64::consts::PI {
                turn -= std::f64::consts::TAU;
            }
            while turn <= -std::f64::consts::PI {
                turn += std::f64::consts::TAU;
            }
            if !(turn.abs() > 1e-6 && turn.abs() < std::f64::consts::PI - 1e-6) {
                let what = if k == 0 { "the sheet" } else { "the line before it" };
                return err(format!("line {} of the path flange's path runs straight on from {what} or back over it: each line must turn", k + 1));
            }
            turns.push((len, turn));
            before = heading;
        }
        let mut made: Vec<usize> = Vec::with_capacity(turns.len());
        let mut next = edge;
        for (k, &(height, turn)) in turns.iter().enumerate() {
            let line = |e: DocError| match e {
                DocError::Invalid(m) => DocError::Invalid(format!("line {}: {m}", k + 1)),
                e => e,
            };
            // Each bend turns up from its panel's surface (away from the material), or toward
            // the material where the path turns down.
            let flip = turn < 0.0;
            let j = self.add_flange_on(next, height, turn.abs(), radius, "inside", HeightFrom::Outer, flip).map_err(line)?;
            made.push(j);
            if k + 1 < turns.len() {
                next = self.far_edge(j).map_err(line)?;
            }
        }
        Ok(made)
    }

    pub fn add_hem(&mut self, pick: Vec3, length: f64, gap: f64, flip: bool) -> Result<usize> {
        let edge = self
            .free_edge_at(pick)?
            .ok_or_else(|| DocError::Invalid("pick a free edge of the sheet (one no bend, seam or cut runs along)".into()))?;
        self.add_hem_on(edge, length, gap, flip)
    }

    /// [`SurfaceSheet::add_hem`] on an edge already found.
    pub fn add_hem_on(&mut self, edge: FreeEdge, length: f64, gap: f64, flip: bool) -> Result<usize> {
        let (a, b, d) = straight(&edge)?;
        let t = self.sheet.thickness;
        if gap < 0.0 {
            return err("the hem gap must not be negative");
        }
        let radius = gap / 2.0;
        let leg = length - (radius + t);
        if leg < 0.0 {
            return err("the hem is shorter than its bend");
        }
        // Over the face opposite the pick: toward the material from the sheet's surface.
        let toward = edge.far_face == flip;
        let len = a.dist(b);
        let bend = Bend {
            angle: std::f64::consts::PI,
            radius,
            toward_material: toward,
            datum: Datum::Tangent,
            length: len,
            ends: ZoneEnds { near: (0.0, len), far: (0.0, len) },
            tails: (Tail::Keep, Tail::Keep),
            relief: (Relief::None, Relief::None),
        };
        Ok(self.add_panel("hem", &edge, a, d, len, leg, bend))
    }
}

/// [`SurfaceSheet::close_corners`] on `sheet`, its panels where `places` put them in space, its
/// rips parting the flanges by `gap`. Convert closes a converted sheet's corners with it too,
/// before it settles them, with `open_in` the sheet's joints as the body gave them: only a
/// corner the body left open closes, where neither flange is joined there to any of those. (Read
/// on the joints as they were, not as earlier closings left them, so that which corner closes
/// does not depend on the order the bends are met in.)
pub(crate) fn close_corners_on(sheet: &mut Sheet, places: &[Option<Rigid3>], gap: f64, j: usize, open_in: Option<&[Joint]>) -> Result<()> {
    let t = sheet.thickness;
    let reach_of = |b: &Bend| (b.radius + t) * (b.angle / 2.0).tan();
    for k in 0..sheet.joints.len() {
        // The new bend as the corners closed so far left it.
        let Some(Joint { a: on_p, b: on_f, kind: JointKind::Bend(nb) }) = sheet.joints.get(j).cloned() else { return Ok(()) };
        let p = on_p.panel;
        let Some(Joint { a, b, kind: JointKind::Bend(ob) }) = sheet.joints.get(k).cloned() else { continue };
        if k >= j || ob.datum != Datum::Sharp {
            continue;
        }
        // The other bend's anchor on the panel and on its flange, and the side of each its
        // material lies on (1 on the left). Across a rip, or across a bend (side flanges across
        // a bend), the bend that stands on the panel beside.
        let (on_q, on_w, side_q) = if a.panel == p {
            (a, b, 1.0)
        } else if b.panel == p {
            (b, a, -1.0)
        } else if ripped(sheet, p, a.panel) || bridge(sheet, p, a.panel).is_some() {
            (a, b, 1.0)
        } else if bridge(sheet, p, b.panel).is_some() {
            (b, a, -1.0)
        } else {
            continue;
        };
        let across = on_q.panel != p;
        let (w, f) = (on_w.panel, on_f.panel);
        if ripped(sheet, f, w) {
            continue;
        }
        // Across a bend, that bend: it ends at the corner with the other two.
        let bridged = if across && !ripped(sheet, p, on_q.panel) { bridge(sheet, p, on_q.panel) } else { None };
        if !matches!(sheet.panels.get(w).map(|x| &x.surface), Some(Surface::Plane)) {
            continue;
        }
        let (Some(Some(pf)), Some(Some(pw))) = (places.get(f), places.get(w)) else { continue };
        // The corner: where the anchors' lines cross on the panel, or across the rip where they
        // meet in space, in each one's chart.
        let (c1, c2) = if across {
            let (Some(Some(pp)), Some(Some(pq))) = (places.get(p), places.get(on_q.panel)) else { continue };
            let line = |place: &Rigid3, an: &Anchor| (place.point(an.origin.extend(0.0)), place.vector(an.dir.extend(0.0)));
            let Some((m1, m2)) = nearest(line(pp, &on_p), line(pq, &on_q)) else { continue };
            if m1.dist(m2) > t * 4.0 {
                continue;
            }
            let chart = |place: &Rigid3, m: Vec3| {
                let q = place.inverse().point(m);
                Vec2::new(q.x, q.y)
            };
            (chart(pp, m1), chart(pq, m2))
        } else {
            let Some(c) = cross_lines((on_p.origin, on_p.dir), (on_q.origin, on_q.dir)) else { continue };
            (c, c)
        };
        let (s1, s2) = ((c1 - on_p.origin).dot(on_p.dir), (c2 - on_q.origin).dot(on_q.dir));
        let nearer = |s: f64, len: f64| if s < len / 2.0 { 0.0 } else { len };
        let (e1, e2) = (nearer(s1, nb.length), nearer(s2, ob.length));
        let reach = reach_of(&nb) + reach_of(&ob) + t;
        if (s1 - e1).abs() > reach || (s2 - e2).abs() > reach {
            continue;
        }
        // A corner the panel fills: each bend runs off into the panel's side of the other. (Across
        // a rip the walls' own corner holds them; the flanges' meeting line is checked below.)
        let (into1, into2) = (if e1 == 0.0 { on_p.dir } else { -on_p.dir }, if e2 == 0.0 { on_q.dir } else { -on_q.dir });
        if !across && !(into2.dot(on_p.dir.perp()) > 1e-9 && into1.dot(on_q.dir.perp() * side_q) > 1e-9) {
            continue;
        }
        // A side at the corner held by a third bend (a flange on a panel's end edge): the flanges
        // close against that bend's flange, across the other bend, instead. Looked for on both
        // panels, so that it does not matter which of the two bends was made first.
        let (at_f, at_w) = (on_f.origin + on_f.dir * s1, on_w.origin + on_w.dir * s2);
        if !across && (held_at(sheet, w, at_w, on_w.dir, [j, k], reach) || held_at(sheet, f, at_f, on_f.dir, [j, k], reach)) {
            continue;
        }
        // A corner the body left open, and only that, closes on a converted sheet: neither flange is
        // joined there to anything but its own bend (where five faces meet round an icosahedron's
        // corner, the body's rips and bends there stay as they are).
        if let Some(joints) = open_in
            && (meets(joints, f, at_f, [j, k], reach) || meets(joints, w, at_w, [j, k], reach))
        {
            continue;
        }
        // The bend across, from its anchor on the panel: where the corner lies along it, and its
        // end there.
        let bridged = match bridged {
            Some(m) => {
                let Some(Joint { a: ma, b: mb, kind: JointKind::Bend(mbend) }) = sheet.joints.get(m) else { continue };
                let on = if ma.panel == p { ma } else { mb };
                let at = (c1 - on.origin).dot(on.dir);
                let end = nearer(at, mbend.length);
                if (at - end).abs() > reach {
                    continue;
                }
                Some((m, at, end))
            }
            None => None,
        };
        // The line the two flanges' planes meet along, from the corner into the new flange's
        // material (on the right of its anchor), and into the other's too. Flanges in one plane
        // (returns on two walls) meet on the line halving the corner between their bends.
        let line = match pf.z.cross(pw.z).normalized() {
            Some(line) => line,
            None if pf.z.dot(pw.z) > 0.0 && (pw.shift - pf.shift).dot(pf.z).abs() < t * 0.5 => {
                let away = |place: &Rigid3, an: &Anchor, end: f64| {
                    let d = if end == 0.0 { an.dir } else { -an.dir };
                    place.vector(d.extend(0.0))
                };
                let Some(half) = (away(pf, &on_f, e1) + away(pw, &on_w, e2)).normalized() else { continue };
                half
            }
            None => continue,
        };
        let flat = |v: Vec3| Vec2::new(v.x, v.y);
        let in_f = |l: Vec3| flat(pf.inverse().vector(l));
        let line = if in_f(line).dot(-on_f.dir.perp()) < 0.0 { -line } else { line };
        let side_w = -side_q;
        let (mf, mw) = (in_f(line), flat(pw.inverse().vector(line)));
        if !(mw.dot(on_w.dir.perp() * side_w) > 1e-9) {
            continue;
        }
        let (Some(rf), Some(rw)) = (sheet.panels.get(f).map(|x| x.region.clone()), sheet.panels.get(w).map(|x| x.region.clone())) else {
            continue;
        };
        // Converting, a relief the body cut at this corner leaves its panels first (see
        // `fill_bites`): the rule's is cut afresh once the corner is settled.
        let (rf, rw) = if open_in.is_some() { (fill_bites(&rf, at_f, reach), fill_bites(&rw, at_w, reach)) } else { (rf, rw) };
        let (Some((rf, hf)), Some((rw, hw))) = (to_line(&rf, &on_f, -1.0, e1, s1, mf)?, to_line(&rw, &on_w, side_w, e2, s2, mw)?) else {
            continue;
        };
        let h = hf.min(hw);
        if !(h > 1e-9) {
            continue;
        }
        let (cf, cw) = (on_f.origin + on_f.dir * s1, on_w.origin + on_w.dir * s2);
        // The rip runs with the new flange's material on its left.
        let up = mf.perp().dot(if e1 == 0.0 { on_f.dir } else { -on_f.dir }) > 0.0;
        let way = if up { line } else { -line };
        let ends = |x: Vec2, m: Vec2| if up { [x, x + m * h] } else { [x + m * h, x] };
        let (ef, ew) = (ends(cf, mf), ends(cw, mw));
        // How far the sheet turns across the edge, from the new flange to the other (as
        // Convert reads a rip's turn).
        let (y, into_w) = (pf.z, (-pw.z).cross(way));
        let turn = into_w.dot(y).atan2(into_w.dot(way.cross(y)));
        // The flange that reaches past the other keeps the mitre's gap on up its side: set back
        // past the rip's end as far as the rip's lap cuts it below, its side stays one line.
        let lap = Lap::Miter { gap };
        let (cut_f, cut_w) = lap.cuts(turn, t);
        let inward = |m: Vec2, an: &Anchor, end: f64| {
            let along = if end == 0.0 { an.dir } else { -an.dir };
            (along - m * along.dot(m)).normalized()
        };
        let rf = match inward(mf, &on_f, e1) {
            Some(into) if hf > h + 1e-9 && cut_f > 1e-12 => set_back_past(&rf, cf, mf, hf, h, into, cut_f).unwrap_or(rf),
            _ => rf,
        };
        let rw = match inward(mw, &on_w, e2) {
            Some(into) if hw > h + 1e-9 && cut_w > 1e-12 => set_back_past(&rw, cw, mw, hw, h, into, cut_w).unwrap_or(rw),
            _ => rw,
        };
        if let Some(x) = sheet.panels.get_mut(f) {
            x.region = rf;
        }
        if let Some(x) = sheet.panels.get_mut(w) {
            x.region = rw;
        }
        if open_in.is_some()
            && let Some(x) = sheet.panels.get_mut(p)
        {
            x.region = fill_bites(&x.region, c1, reach);
        }
        // Each bend's edge runs to the corner.
        if let Some(Joint { a, b, kind: JointKind::Bend(bend) }) = sheet.joints.get_mut(j) {
            to_corner(a, b, bend, s1, e1);
        }
        if let Some(Joint { a, b, kind: JointKind::Bend(bend) }) = sheet.joints.get_mut(k) {
            to_corner(a, b, bend, s2, e2);
        }
        if let Some((m, at, end)) = bridged
            && let Some(Joint { a, b, kind: JointKind::Bend(bend) }) = sheet.joints.get_mut(m)
        {
            to_corner(a, b, bend, at, end);
        }
        let [f0, f1] = ef;
        let [w0, w1] = ew;
        let (df, dw) = ((f1 - f0) * (1.0 / h), (w1 - w0) * (1.0 / h));
        sheet.joints.push(Joint {
            a: Anchor { panel: f, origin: f0, dir: df },
            b: Anchor { panel: w, origin: w0, dir: dw },
            kind: JointKind::Rip(Rip {
                edges: (vec![f0, f1], vec![w0, w1]),
                curves: (vec![Seg2::Line { a: f0, b: f1 }], vec![Seg2::Line { a: w0, b: w1 }]),
                turns: vec![turn, turn],
                lap: Lap::Miter { gap },
            }),
        });
    }
    Ok(())
}

/// Whether panels `i` and `k` meet at a rip (two walls mitred at a corner).
fn ripped(sheet: &Sheet, i: usize, k: usize) -> bool {
    i != k
        && sheet
            .joints
            .iter()
            .any(|j| matches!(j.kind, JointKind::Rip(_)) && ((j.a.panel == i && j.b.panel == k) || (j.a.panel == k && j.b.panel == i)))
}

/// The bend anchored at its sharps that joins panels `i` and `k`, if one does.
fn bridge(sheet: &Sheet, i: usize, k: usize) -> Option<usize> {
    if i == k {
        return None;
    }
    sheet.joints.iter().position(|j| {
        matches!(&j.kind, JointKind::Bend(b) if b.datum == Datum::Sharp) && ((j.a.panel == i && j.b.panel == k) || (j.a.panel == k && j.b.panel == i))
    })
}

/// Whether a bend other than `skip`'s stands on panel `w` across the line through `at` along
/// `along` (in the panel's chart), its anchor passing within `reach` of `at`: there the
/// panel's side is that bend's, as where a flange stands on a wall's end edge beside the
/// wall's own bend. A bend running along the line (a hem along the wall's far edge) holds no
/// side there.
fn held_at(sheet: &Sheet, w: usize, at: Vec2, along: Vec2, skip: [usize; 2], reach: f64) -> bool {
    sheet.joints.iter().enumerate().any(|(m, j)| {
        let JointKind::Bend(bend) = &j.kind else { return false };
        !skip.contains(&m)
            && [&j.a, &j.b].into_iter().any(|an| {
                an.panel == w && an.dir.cross(along).abs() > 0.1 && dist_to_segment(at, an.origin, an.origin + an.dir * bend.length) <= reach
            })
    })
}

/// Whether a joint of `joints` other than `skip`'s runs on panel `x` within `reach` of the point
/// `at` of its chart: a bend along its anchor, a rip along its edge on `x`, a seam on its anchor's line.
fn meets(joints: &[Joint], x: usize, at: Vec2, skip: [usize; 2], reach: f64) -> bool {
    let near = |pts: &[Vec2]| pts.windows(2).any(|w| dist_to_segment(at, w[0], w[1]) <= reach);
    joints.iter().enumerate().any(|(m, jt)| {
        !skip.contains(&m)
            && match &jt.kind {
                JointKind::Bend(bend) => {
                    [&jt.a, &jt.b].into_iter().any(|an| an.panel == x && dist_to_segment(at, an.origin, an.origin + an.dir * bend.length) <= reach)
                }
                JointKind::Rip(rip) => (jt.a.panel == x && near(&rip.edges.0)) || (jt.b.panel == x && near(&rip.edges.1)),
                JointKind::Seam => [&jt.a, &jt.b].into_iter().any(|an| an.panel == x && (at - an.origin).cross(an.dir).abs() <= reach),
            }
    })
}

/// Why a fold's side can't be the one that moves, where the other side perhaps can (see
/// [`SurfaceSheet::add_fold`]): the side that stays would fall apart, or a piece that folds would
/// join the rest again beyond the bend.
const FOLD_FIXED_APART: &str = "the fold cuts its panel into more than one piece on its fixed side: not supported yet";
const FOLD_TABS_REJOIN: &str = "not supported yet: folding tabs that join again beyond the bend";

/// A fold's zone reaching a joint of its panel that can't go with either side: a bend, or
/// another joint; and a fold across a rip (a wall mitred to the next) that would turn the wall
/// into the next one.
const FOLD_THROUGH_BEND: &str = "not supported yet: a fold through a flange's bend";
const ZONE_ON_JOINT: &str = "the fold's bend zone reaches or crosses another joint of its panel: not supported yet";
const FOLD_INTO_NEIGHBOR: &str =
    "not supported yet: a fold across a mitred corner that turns its wall into the next one, across the rip between them";

/// How far (mm) a rip's cut may differ along it and still be one cut, as the development takes it.
const RIP_EVEN: f64 = 1e-9;

/// How far (mm) a fold's zone reaches along its line past a curved end of the outline: past the
/// sag of the chords the end was measured on (1e-4 mm), so that the zone holds the curve's bulge.
/// What lies past the curve is cut off again.
const CURVE_REACH: f64 = 1e-3;

/// How wide (mm) a piece the planar booleans leave must be to be one: where two of their edges
/// coincide they leave slivers of no width, about their tolerance (1e-9 mm) across.
const SLIVER: f64 = 1e-8;

/// A fold worked out before anything of the sheet changes (see [`SurfaceSheet::add_fold`]).
struct FoldPlan {
    /// The way to the side that moves, the way along the fold's line (the side that stays on its
    /// left), and the zone's start line `x · n = s0` and its width.
    n: Vec2,
    u: Vec2,
    s0: f64,
    ba: f64,
    /// What of the panel stays.
    stays: Region2,
    moves: Vec<FoldPiece>,
    /// What is cut from the flat in the zones, in the panel's chart.
    cuts: Vec<Region2>,
    /// The anchors on the panel that go with a piece that folds: the joint, whether it is its `a`
    /// anchor, and the piece's panel.
    anchors: Vec<(usize, bool, usize)>,
    /// The rips the zone parts: the joint, its stretch that stays (in its place) and its stretch
    /// that folds (a joint of its own).
    rips: Vec<(usize, Joint, Joint)>,
    /// The sheet's cuts, recesses and loose pieces that go with a piece that folds: each one's
    /// index, and the piece's panel.
    moved_cuts: Vec<(usize, usize)>,
    recesses: Vec<(usize, usize)>,
    loose: Vec<(usize, usize)>,
}

/// A piece of a panel that a fold folds: its panel to be, its material, its bend, where its
/// anchors start along the fold's line, and how far along the line it and its zone reach.
struct FoldPiece {
    panel: usize,
    region: Region2,
    bend: Bend,
    start: f64,
    span: (f64, f64),
}

/// What a fold works on, whichever side stays (see [`SurfaceSheet::add_fold`]): the panel the
/// line lies on and where it lies in space, where every panel lies, the panel's material, the
/// line in its chart (as drawn) and the way across it to its right seen from the sheet's surface
/// (`d.perp()`, `d` the line's way: the chart's `z` runs into the material), the bend, and each
/// panel's flat area.
struct FoldSetup {
    i: usize,
    place: Rigid3,
    places: Vec<Option<Rigid3>>,
    region: Region2,
    line: [Vec2; 2],
    across: Vec2,
    bend: Bend,
    flat_area: Vec<f64>,
}

/// What of `region` lies between the lines `x · n = from` and `x · n = to` (`n` a unit vector).
fn band(region: &Region2, n: Vec2, from: f64, to: f64) -> Result<Vec<Region2>> {
    let b = region.outer.polyline(1e-3).bounds();
    let big = (b.size().len() + b.min.len() + b.max.len() + 1.0) * 4.0;
    let d = n.perp();
    let corners = [n * from - d * big, n * from + d * big, n * to + d * big, n * to - d * big];
    let strip = Region2 { outer: Loop2::polygon(&corners).ccw(), holes: Vec::new() };
    let tol = (b.size().len() * 1e-9).max(1e-9);
    Ok(solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(region), std::slice::from_ref(&strip), BoolOp::Intersect, tol)?)
}

/// The least and the greatest of `x · w` over a region's outline (on its chords, 1e-4 mm in).
fn extent(r: &Region2, w: Vec2) -> (f64, f64) {
    (&r.outer.polyline(1e-4)).into_iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.dot(w)), hi.max(q.dot(w))))
}

/// Where the straight edges of a region's outline run along the line `x · n = level`, within
/// `tol` of it: each one's stretch along `u`.
fn edges_on(r: &Region2, (n, u): (Vec2, Vec2), level: f64, tol: f64) -> Vec<(f64, f64)> {
    r.outer
        .segs
        .iter()
        .filter_map(|s| match *s {
            Seg2::Line { a, b } if (a.dot(n) - level).abs() <= tol && (b.dot(n) - level).abs() <= tol && a.dist(b) > tol => {
                Some((a.dot(u).min(b.dot(u)), a.dot(u).max(b.dot(u))))
            }
            _ => None,
        })
        .collect()
}

/// Whether any of the stretches `a` overlaps any of `b` by more than `tol`.
fn overlap(a: &[(f64, f64)], b: &[(f64, f64)], tol: f64) -> bool {
    a.iter().any(|x| b.iter().any(|y| x.1.min(y.1) - x.0.max(y.0) > tol))
}

/// How wide a region is on average: twice its area over the length of its loops.
fn width(r: &Region2) -> f64 {
    let around: f64 = std::iter::once(&r.outer).chain(&r.holes).flat_map(|l| &l.segs).map(|s| s.length()).sum();
    if around > 0.0 { 2.0 * r.area() / around } else { 0.0 }
}

/// A rip of a panel that a fold's zone crosses (the zone along `n` from the line `x · n = s0`, a
/// width `ba` across), parted where the zone's lines cross it: its stretch on the side that
/// stays, which keeps the other panel's edge across the zone too, so that the other panel's cut
/// runs on unbroken; its stretch on the side that folds, moved to panel `folds`; what the rip's
/// cut takes from the zone (the panel's edge moved into its material, as the development cuts
/// it), so that the zone's end runs on along the panel's cut edge from the corner the cut leaves
/// on one side of the zone to the one it leaves on the other; and that cut edge, a point of it
/// and the way into the panel's material. `on_a`: the panel is the rip's `a` side. None where the
/// rip isn't one straight edge cut alike all along, has an end in the zone, or runs so nearly
/// along the zone that the panels' cut corners would not lie where its cut edge meets the zone's
/// lines.
#[allow(clippy::too_many_arguments)]
fn part_rip(
    joint: &Joint,
    rip: &Rip,
    on_a: bool,
    folds: usize,
    (n, s0, ba): (Vec2, f64, f64),
    t: f64,
    tol: f64,
) -> Option<(Joint, Joint, Option<Region2>, (Vec2, Vec2))> {
    let (mine, other) = if on_a { (&rip.edges.0, &rip.edges.1) } else { (&rip.edges.1, &rip.edges.0) };
    let ([e0, e1], [o0, o1], [t0, t1]) = (mine.as_slice(), other.as_slice(), rip.turns.as_slice()) else { return None };
    let (e0, e1, o0, o1) = (*e0, *e1, *o0, *o1);
    let straight = |c: &[Seg2]| c.is_empty() || matches!(c, [Seg2::Line { .. }]);
    let cut = |turn: f64| {
        let (a, b) = rip.lap.cuts(turn, t);
        if on_a { a } else { b }
    };
    let c = cut(*t0);
    if !(straight(&rip.curves.0) && straight(&rip.curves.1)) || (cut(*t1) - c).abs() > RIP_EVEN {
        return None;
    }
    let side = |q: Vec2| q.dot(n) - s0;
    let (se0, se1) = (side(e0), side(e1));
    let dir = (e1 - e0).normalized()?;
    // Past the zone at both ends, and across it steeply enough that the development's joins of the
    // panels' cut edges to the zone's lines come out where the lines cross.
    if !(se0.min(se1) < -tol && se0.max(se1) > ba + tol) || dir.dot(n).abs() < 0.5 {
        return None;
    }
    let at = |level: f64| (level - se0) / (se1 - se0);
    let on = |p: Vec2, q: Vec2, x: f64| p + (q - p) * x;
    let (tn, tf) = (at(0.0), at(ba));
    let (pn, pf, qf) = (on(e0, e1, tn), on(e0, e1, tf), on(o0, o1, tf));
    let (stay, fold) = if se0 < se1 { (([e0, pn], [o0, qf]), ([pf, e1], [qf, o1])) } else { (([pn, e1], [qf, o1]), ([e0, pf], [o0, qf])) };
    let part = |(mine, other): ([Vec2; 2], [Vec2; 2]), panel: Option<usize>| {
        let (ea, eb) = if on_a { (mine, other) } else { (other, mine) };
        let line = |e: [Vec2; 2], had: &[Seg2]| if had.is_empty() { Vec::new() } else { vec![Seg2::Line { a: e[0], b: e[1] }] };
        let (mut a, mut b) = (joint.a, joint.b);
        (a.origin, b.origin) = (ea[0], eb[0]);
        if let Some(p) = panel {
            if on_a {
                a.panel = p;
            } else {
                b.panel = p;
            }
        }
        let edges = (ea.to_vec(), eb.to_vec());
        let curves = (line(ea, &rip.curves.0), line(eb, &rip.curves.1));
        Joint { a, b, kind: JointKind::Rip(Rip { edges, curves, turns: rip.turns.clone(), lap: rip.lap }) }
    };
    // The panel's cut edge: its edge moved into its material, on the left of the rip's way on its
    // `a` panel and on the right on its `b` one.
    let into = if on_a { dir.perp() } else { -dir.perp() };
    let from = e0 + into * c;
    let lap = (c > RIP_EVEN).then(|| {
        let x = |level: f64| from + dir * ((level - side(from)) / dir.dot(n));
        Region2 { outer: Loop2::polygon(&[pn, pf, x(ba), x(0.0)]).ccw(), holes: Vec::new() }
    });
    Some((part(stay, None), part(fold, Some(folds)), lap, (from, into)))
}

/// Where a fold's `joint` puts one of its panels in space with the other at `at` (the `a` panel
/// held where `held_a`, else the `b` panel), and its bend there: as the sheet's fold carries one
/// from the other. Both panels' charts are the flat's.
fn folded_joint(t: f64, k_factor: f64, joint: &Joint, at: Rigid3, held_a: bool) -> Option<(Rigid3, solidcraft_develop::Bent)> {
    let mut pair = joint.clone();
    (pair.a.panel, pair.b.panel) = (0, 1);
    let sheet = solidcraft_develop::Sheet {
        thickness: t,
        k_factor,
        panels: vec![Panel::default(), Panel::default()],
        joints: vec![pair],
        ..Default::default()
    };
    let (held, other) = if held_a { (0, 1) } else { (1, 0) };
    let dev = develop(&sheet, &[(held, solidcraft_develop::Rigid2::IDENTITY)]).ok()?;
    let fold = solidcraft_develop::fold(&sheet, &dev, &[at]).ok()?;
    Some((fold.places.get(other).copied().flatten()?, fold.bends.first().copied().flatten()?))
}

/// Whether what a fold moves keeps clear of the panel beyond a rip the fold parts (`other`, at
/// `there`, a plane `t` thick): its zone's pieces `zone` (in the flat, which `bent` folds) and its
/// panel's piece `piece` (in its chart, at `moved`) lie wholly on the far side of the other
/// panel's surface from its material; or, where the fold turns toward its own panel's material
/// (`toward`), wholly on that side of its panel's surface (at `here`, where nothing has turned,
/// as a bend toward the material never comes back across it) with the other panel wholly on the
/// other side. Both are planes that part the two. `None` where an edge isn't straight, so that
/// this can't be told exactly.
#[allow(clippy::too_many_arguments)]
fn keeps_clear(
    zone: &[Region2],
    piece: &Region2,
    (bent, moved, here): (&solidcraft_develop::Bent, Rigid3, Rigid3),
    toward: bool,
    other: &Region2,
    there: Rigid3,
    t: f64,
) -> Option<bool> {
    let lines = |r: &Region2| -> Option<Vec<(Vec2, Vec2)>> {
        r.outer.segs.iter().map(|s| if let Seg2::Line { a, b } = *s { Some((a, b)) } else { None }).collect()
    };
    // How far past the other panel's surface, into its material, a point lies.
    let past = |x: Vec3| there.inverse().point(x).z;
    let mut most = f64::NEG_INFINITY;
    for (a, _) in lines(piece)? {
        for depth in [0.0, t] {
            most = most.max(past(moved.point(Vec3::new(a.x, a.y, depth))));
        }
    }
    for r in zone {
        for seg in lines(r)? {
            for depth in [0.0, t] {
                most = most.max(bent_max(bent, &past, seg, depth));
            }
        }
    }
    if most <= 0.0 {
        return Some(true);
    }
    if !toward {
        return Some(false);
    }
    let above = |x: Vec3| here.inverse().point(x).z;
    Some(lines(other)?.iter().all(|(a, _)| [0.0, t].iter().all(|d| above(there.point(Vec3::new(a.x, a.y, *d))) <= 0.0)))
}

/// The greatest of the linear function `f` over the straight flat edge `(p, q)` of a bend's zone,
/// `depth` into the material, as `bent` folds it. Folded, `f` is `k + c·along + a·cos φ + b·sin φ`
/// in the length `along` the bend and the turn `φ` round it, both linear along the edge: its
/// greatest is at an end, or where its rate along the edge is nothing, found in closed form.
fn bent_max(bent: &solidcraft_develop::Bent, f: &dyn Fn(Vec3) -> f64, (p, q): (Vec2, Vec2), depth: f64) -> f64 {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let at = |along: f64, phi: f64| f(bent.at(along, phi, depth));
    let (f0, f_half, f_pi) = (at(0.0, 0.0), at(0.0, FRAC_PI_2), at(0.0, PI));
    let (k, a) = ((f0 + f_pi) / 2.0, (f0 - f_pi) / 2.0);
    let (b, c) = (f_half - k, at(1.0, 0.0) - f0);
    let ((l0, p0), (l1, p1)) = (bent.wrapped(p), bent.wrapped(q));
    let value = |s: f64| {
        let (along, phi) = (l0 + (l1 - l0) * s, p0 + (p1 - p0) * s);
        k + c * along + a * phi.cos() + b * phi.sin()
    };
    let mut most = value(0.0).max(value(1.0));
    // c·(l1 - l0) + (p1 - p0)·(b cos φ - a sin φ) = 0, where b cos φ - a sin φ = m cos(φ + δ).
    let (dl, dp, m) = (l1 - l0, p1 - p0, a.hypot(b));
    if dp.abs() > 0.0 && m > 0.0 {
        let r = -c * dl / (dp * m);
        if r.abs() <= 1.0 {
            let delta = a.atan2(b);
            for root in [r.acos(), -r.acos()] {
                for turns in -2..=2 {
                    let s = (root - delta + f64::from(turns) * TAU - p0) / dp;
                    if s > 0.0 && s < 1.0 {
                        most = most.max(value(s));
                    }
                }
            }
        }
    }
    most
}

/// The point where two lines, each a point and a direction, cross.
/// What of `region` lies along `n` from the line `x . n = from` (n a unit vector).
fn half(region: &Region2, n: Vec2, from: f64) -> Result<Vec<Region2>> {
    let b = region.outer.polyline(1e-3).bounds();
    let big = (b.size().len() + b.min.len() + b.max.len() + 1.0) * 4.0;
    let (o, d) = (n * from, n.perp());
    let box_ = [o - d * big, o + d * big, o + d * big + n * big, o - d * big + n * big];
    let half = Region2 { outer: Loop2::polygon(&box_).ccw(), holes: Vec::new() };
    let tol = (b.size().len() * 1e-9).max(1e-9);
    Ok(solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(region), std::slice::from_ref(&half), BoolOp::Intersect, tol)?)
}

fn area(r: &[Region2]) -> f64 {
    r.iter().map(Region2::area).sum()
}

/// `region` with its side from `from` along `m` for `rise` (as `to_line` leaves it) set back `by`
/// toward `into`, square to it, past `h` along it: the side steps in at `h` and runs on parallel,
/// and the edge past the side runs on along its line to meet it. `None` where the side or that
/// edge isn't a straight line of the outline.
fn set_back_past(region: &Region2, from: Vec2, m: Vec2, rise: f64, h: f64, into: Vec2, by: f64) -> Option<Region2> {
    let segs = &region.outer.segs;
    let n = segs.len();
    let top = from + m * rise;
    let tol = rise.max(1.0) * 1e-9;
    let same = |p: Vec2, q: Vec2| p.dist(q) <= tol;
    let k = segs.iter().position(|g| matches!(*g, Seg2::Line { a, b } if (same(a, from) && same(b, top)) || (same(a, top) && same(b, from))))?;
    let up = matches!(segs[k], Seg2::Line { a, .. } if same(a, from));
    // The edge past the side, at its top.
    let past_at = if up { (k + 1) % n } else { (k + n - 1) % n };
    let Seg2::Line { a: pa, b: pb } = segs[past_at] else { return None };
    let (p, q) = (from + m * h, from + m * h + into * by);
    let w = pb - pa;
    let cross = m.cross(w);
    if cross.abs() <= 1e-12 {
        return None;
    }
    let new_top = q + m * ((pa - q).cross(w) / cross);
    let mut out = segs.clone();
    let side = if up {
        vec![Seg2::Line { a: from, b: p }, Seg2::Line { a: p, b: q }, Seg2::Line { a: q, b: new_top }]
    } else {
        vec![Seg2::Line { a: new_top, b: q }, Seg2::Line { a: q, b: p }, Seg2::Line { a: p, b: from }]
    };
    out[past_at] = if up { Seg2::Line { a: new_top, b: pb } } else { Seg2::Line { a: pa, b: new_top } };
    out.splice(k..=k, side);
    Some(Region2 { outer: Loop2 { segs: out }, holes: region.holes.clone() })
}

/// How far a point lies from the segment `a`–`b`.
fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let len2 = d.dot(d);
    let s = if len2 > 0.0 { ((p - a).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + d * s)
}

/// How far a region reaches from the line through `o`, along its normal `n`.
fn reach_from(region: &Region2, (o, n): (Vec2, Vec2)) -> f64 {
    (&region.outer.polyline(1e-4)).into_iter().map(|p| (*p - o).dot(n)).fold(0.0, f64::max)
}

/// `region` with the bites a relief took out of its corners near `at` put back (Convert, #6748):
/// an arc of its outline between two straight edges, about a centre outside its material and
/// within `reach` of `at` past its radius, is a disc's bite out of the corner the two edges' lines
/// make, and the edges run on to that corner instead. So a converted panel loses the body's
/// relief there, and the rule's is cut afresh where the corner is settled: the flat holds one
/// exact circle at the corner, never the body's beside the rule's, nearly on it (their crossings
/// leave slivers, and pieces of curve the flat's walls cannot be swept from).
fn fill_bites(region: &Region2, at: Vec2, reach: f64) -> Region2 {
    let mut segs = region.outer.ccw().segs;
    loop {
        let n = segs.len();
        let found = (0..n).find_map(|i| {
            let Seg2::Arc { center, radius, .. } = *segs.get(i)? else { return None };
            let (Seg2::Line { a: a1, b: b1 }, Seg2::Line { a: a2, b: b2 }) = (*segs.get((i + n - 1) % n)?, *segs.get((i + 1) % n)?) else {
                return None;
            };
            if n < 4 || region.contains(center) || center.dist(at) > radius + reach {
                return None;
            }
            let (d1, d2) = ((b1 - a1).normalized()?, (b2 - a2).normalized()?);
            let x = cross_lines((a1, d1), (a2, d2))?;
            // The corner lies on past the edge before the arc, and before the edge after it.
            ((x - b1).dot(d1) >= 0.0 && (a2 - x).dot(d2) >= 0.0).then_some((i, a1, x, b2))
        });
        let Some((i, a1, x, b2)) = found else { break };
        let (before, after) = ((i + n - 1) % n, (i + 1) % n);
        if let Some(s) = segs.get_mut(before) {
            *s = Seg2::Line { a: a1, b: x };
        }
        if let Some(s) = segs.get_mut(after) {
            *s = Seg2::Line { a: x, b: b2 };
        }
        segs.remove(i);
    }
    Region2 { outer: Loop2 { segs }, holes: region.holes.clone() }
}

fn cross_lines((p, u): (Vec2, Vec2), (q, w): (Vec2, Vec2)) -> Option<Vec2> {
    let cross = u.cross(w);
    (cross.abs() > 1e-12).then(|| p + u * ((q - p).cross(w) / cross))
}

/// Moves a bend's edge end `end` (0 or its length) to `at`, along its anchors.
fn to_corner(a: &mut Anchor, b: &mut Anchor, bend: &mut Bend, at: f64, end: f64) {
    if end == 0.0 {
        a.origin += a.dir * at;
        b.origin += b.dir * at;
        bend.length -= at;
    } else {
        bend.length = at;
    }
    bend.ends = ZoneEnds { near: (0.0, bend.length), far: (0.0, bend.length) };
}

/// The points where two lines in space, each a point and a direction, come nearest each other;
/// none for parallel lines.
fn nearest((o1, d1): (Vec3, Vec3), (o2, d2): (Vec3, Vec3)) -> Option<(Vec3, Vec3)> {
    let (a, b, c) = (d1.dot(d1), d1.dot(d2), d2.dot(d2));
    let w = o1 - o2;
    let (d, e) = (d1.dot(w), d2.dot(w));
    let den = a * c - b * b;
    if !(den.abs() > 1e-12 * a * c) {
        return None;
    }
    let (s, u) = ((b * e - c * d) / den, (a * e - b * d) / den);
    Some((o1 + d1 * s, o2 + d2 * u))
}

/// A panel's region with its side at a corner moved onto the line where it meets another flange:
/// the line through `at` along the panel's anchor `an` (lengths from the anchor's origin),
/// running `m` into the material, which lies on the `side` of the anchor (1 on the left). The
/// side is the straight edge of the outline from the anchor's end `end` (0 or the bend's length);
/// as a drawing's trim and extend would, it becomes the line, the anchor's edge runs on or stops
/// where the line meets it, and the edge past the side runs on along its own line or circle, or
/// stops, where the line meets that. The new region and how far the side runs along the line;
/// `None` where the side is not a straight edge from the anchor, the edge past it neither a line
/// nor an arc, or the line meets either of them nowhere ahead.
fn to_line(region: &Region2, an: &Anchor, side: f64, end: f64, at: f64, m: Vec2) -> Result<Option<(Region2, f64)>> {
    let (o, d, n) = (an.origin, an.dir, an.dir.perp() * side);
    let into = |p: Vec2| Vec2::new((p - o).dot(d), (p - o).dot(n));
    let segs = &region.outer.segs;
    let count = segs.len();
    let size = region.outer.polyline(1e-3).bounds().size().len().max(1.0);
    let tol = size * 1e-9;
    let on_anchor = |s: &Seg2| matches!(*s, Seg2::Line { a, b } if into(a).y.abs() <= tol && into(b).y.abs() <= tol);
    let corner = o + d * end;
    // The vertex at the anchor's end, between the anchor's edge and the side: `before` and
    // `after` the vertex in the loop's order. A panel whose side carries on a slanted edge beside
    // it in the flat has it where its edge along the anchor ends nearest there.
    let at_end = (0..count).find(|k| segs.get(*k).is_some_and(|s| s.end().dist(corner) <= tol));
    let nearest = || {
        let ends = segs.iter().enumerate().filter(|(_, s)| on_anchor(s)).flat_map(|(k, s)| [((k + count - 1) % count, s.start()), (k, s.end())]);
        ends.min_by(|x, y| x.1.dist(corner).total_cmp(&y.1.dist(corner))).map(|(k, _)| k)
    };
    let Some(k) = at_end.or_else(nearest) else { return Ok(None) };
    let (before, after) = (k, (k + 1) % count);
    let (Some(sb), Some(sa)) = (segs.get(before), segs.get(after)) else { return Ok(None) };
    // Which of the two is the side, and the edge past it.
    let (side_at, past_at, side_first) = match (on_anchor(sb), on_anchor(sa)) {
        (true, false) => (after, (after + 1) % count, true),
        (false, true) => (before, (before + count - 1) % count, false),
        _ => return Ok(None),
    };
    let (Some(Seg2::Line { .. }), Some(past)) = (segs.get(side_at), segs.get(past_at).copied()) else { return Ok(None) };
    let from = o + d * at;
    let m = m.normalized().ok_or_else(|| DocError::Invalid("the corner's line has no way".into()))?;
    if !(into(from + m).y > 0.0) {
        return Ok(None);
    }
    // Where the line meets the edge past the side: on its line, or its circle, nearest the
    // side's old end there.
    let old = if side_first { past.start() } else { past.end() };
    let top = match past {
        Seg2::Line { a, b } => {
            let w = b - a;
            let cross = m.cross(w);
            if cross.abs() <= 1e-12 {
                return Ok(None);
            }
            from + m * ((a - from).cross(w) / cross)
        }
        Seg2::Arc { center, radius, .. } => {
            let f = from - center;
            let (b, c) = (m.dot(f), f.dot(f) - radius * radius);
            let disc = b * b - c;
            if disc < 0.0 {
                return Ok(None);
            }
            let r = disc.sqrt();
            let (p, q) = (from + m * (-b - r), from + m * (-b + r));
            if p.dist(old) <= q.dist(old) { p } else { q }
        }
        _ => return Ok(None),
    };
    let rise = (top - from).dot(m);
    if !(rise > tol) {
        return Ok(None);
    }
    // The edge past the side, from (or to) its new end at `top`.
    let moved = |s: Seg2, at_start: bool, p: Vec2| -> Seg2 {
        match s {
            Seg2::Line { a, b } => {
                if at_start {
                    Seg2::Line { a: p, b }
                } else {
                    Seg2::Line { a, b: p }
                }
            }
            Seg2::Arc { center, radius, start, sweep } => {
                let near = |angle: f64, to: f64| angle + ((to - angle) / std::f64::consts::TAU).round() * std::f64::consts::TAU;
                let ang = (p - center).y.atan2((p - center).x);
                if at_start {
                    let a = near(ang, start);
                    Seg2::Arc { center, radius, start: a, sweep: sweep + (start - a) }
                } else {
                    let a = near(ang, start + sweep);
                    Seg2::Arc { center, radius, start, sweep: a - start }
                }
            }
            other => other,
        }
    };
    let mut out = segs.clone();
    let anchor_at = if side_first { before } else { after };
    if let Some(slot) = out.get_mut(anchor_at) {
        *slot = match *slot {
            Seg2::Line { a, .. } if side_first => Seg2::Line { a, b: from },
            Seg2::Line { b, .. } => Seg2::Line { a: from, b },
            other => other,
        };
    }
    if let Some(slot) = out.get_mut(side_at) {
        *slot = if side_first { Seg2::Line { a: from, b: top } } else { Seg2::Line { a: top, b: from } };
    }
    if let Some(slot) = out.get_mut(past_at) {
        *slot = moved(past, side_first, top);
    }
    // Nothing may run backwards or cross: the anchor's edge and the edge past the side keep their
    // way, and the outline stays one simple loop.
    let kept = |s: Option<&Seg2>, was: Option<&Seg2>| match (s, was) {
        (Some(Seg2::Line { a, b }), Some(Seg2::Line { a: p, b: q })) => (*b - *a).dot(*q - *p) > tol * tol,
        (Some(Seg2::Arc { sweep, .. }), Some(Seg2::Arc { sweep: was, .. })) => sweep * was > 0.0,
        _ => true,
    };
    if !kept(out.get(anchor_at), segs.get(anchor_at)) || !kept(out.get(past_at), segs.get(past_at)) {
        return Ok(None);
    }
    let outer = Loop2 { segs: out };
    if crossed(&outer) {
        return Ok(None);
    }
    Ok(Some((Region2 { outer, holes: region.holes.clone() }, rise)))
}

/// Whether a loop crosses itself: two of its edges, not neighbours, meeting (on their polylines).
fn crossed(l: &Loop2) -> bool {
    let pieces: Vec<Vec<Vec2>> = l.segs.iter().map(|s| s.polyline(1e-4)).collect();
    let n = pieces.len();
    let hit = |p: &[Vec2], q: &[Vec2]| {
        p.windows(2).any(|a| {
            q.windows(2).any(|b| {
                let (Some(&a0), Some(&a1), Some(&b0), Some(&b1)) = (a.first(), a.get(1), b.first(), b.get(1)) else { return false };
                let side = |o: Vec2, x: Vec2, y: Vec2| (x - o).cross(y - o);
                side(a0, a1, b0) * side(a0, a1, b1) < 0.0 && side(b0, b1, a0) * side(b0, b1, a1) < 0.0
            })
        })
    };
    pieces.iter().enumerate().any(|(i, p)| pieces.iter().enumerate().skip(i + 2).filter(|(j, _)| !(i == 0 && *j == n - 1)).any(|(_, q)| hit(p, q)))
}
