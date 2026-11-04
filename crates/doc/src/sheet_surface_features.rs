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

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_develop::Cut;
use solidcraft_develop::{Anchor, Bend, Datum, Joint, JointKind, Lap, Panel, Relief, Rigid3, Rip, Surface, Tail, ZoneEnds, develop};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::BoolOp;

use crate::sheet::HeightFrom;
use crate::{DocError, Result, SurfaceSheet};

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
    /// the thickness of `p`.
    pub fn free_edge_at(&self, p: Vec3) -> Result<Option<FreeEdge>> {
        let dev = develop(&self.sheet, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
        let folded = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        let t = self.sheet.thickness;
        let mut best: Option<(f64, FreeEdge)> = None;
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
                    if self.joined(i, a, b) {
                        continue;
                    }
                    let d = s.point_at(s.closest_param(at).clamp(0.0, 1.0)).dist(at);
                    let off = (q.z.clamp(0.0, t) - q.z).abs();
                    let dist = (d * d + off * off).sqrt();
                    if dist <= t * 2.0 + 1e-3 && best.as_ref().is_none_or(|(x, _)| dist < *x) {
                        best = Some((dist, FreeEdge { panel: i, a, b, far_face: q.z > t / 2.0 }));
                    }
                }
            }
        }
        Ok(best.map(|(_, e)| e))
    }

    /// Whether a joint runs along the edge `a`–`b` of panel `i` (its anchor line holds both ends).
    fn joined(&self, i: usize, a: Vec2, b: Vec2) -> bool {
        let on = |an: &Anchor| {
            let off = |p: Vec2| (p - an.origin).cross(an.dir).abs();
            an.panel == i && off(a) <= 1e-6 && off(b) <= 1e-6
        };
        self.sheet.joints.iter().any(|j| on(&j.a) || on(&j.b))
    }

    /// Moves the edge `a`–`b` of panel `i` `by` mm outward (inward where negative): the panel
    /// gains, or loses, the strip between.
    fn move_edge(&mut self, i: usize, (a, b, d): (Vec2, Vec2, Vec2), by: f64) -> Result<()> {
        if by.abs() <= 1e-9 {
            return Ok(());
        }
        let out = Vec2::new(d.y, -d.x);
        let (p, q) = (a + out * by, b + out * by);
        let strip = if by > 0.0 { [a, b, q, p] } else { [p, q, b, a] };
        let strip = Region2 { outer: Loop2::polygon(&strip).ccw(), holes: Vec::new() };
        let panel = self.sheet.panels.get(i).ok_or_else(|| DocError::Invalid("panel".into()))?;
        let op = if by > 0.0 { BoolOp::Union } else { BoolOp::Cut };
        let mut out = solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(&panel.region), &[strip], op, 1e-9)?;
        if out.len() != 1 {
            return err("the flange's bend would cut its panel apart");
        }
        if let (Some(r), Some(panel)) = (out.pop(), self.sheet.panels.get_mut(i)) {
            panel.region = r;
        }
        Ok(())
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
    /// material_outside or tangent). It bends away from the sheet's material, to the side of
    /// the sheet's surface as a flange-built sheet's flanges rise from its top; toward the
    /// material when `flip`. Where it meets another flange at a corner of the panel, the two close
    /// the corner (see [`SurfaceSheet::close_corners`]). Its bend's ends are settled when the
    /// sheet is built. The new bend's joint.
    #[allow(clippy::too_many_arguments)]
    pub fn add_flange(&mut self, pick: Vec3, height: f64, angle: f64, radius: f64, position: &str, from: HeightFrom, flip: bool) -> Result<usize> {
        let edge = self
            .free_edge_at(pick)?
            .ok_or_else(|| DocError::Invalid("pick a free edge of the sheet (one no bend, seam or cut runs along)".into()))?;
        self.add_flange_on(edge, height, angle, radius, position, from, flip)
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
        if !(angle > 1e-6 && angle <= std::f64::consts::PI - 1e-6) {
            return err("the flange angle must be between 0 and 180°");
        }
        let (a, b, d) = straight(&edge)?;
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
        let toward = flip;
        // The panels' surface is the bend's outside where it turns toward the material: the
        // anchors mark that surface's virtual sharp, this far past the bend's start.
        let sharp = if toward { (radius + t) * half } else { radius * half };
        let leg = height - from.before_leg(angle, radius, t);
        if !(leg >= -1e-9) {
            return err(format!("the flange height must be at least {:.3} mm for this bend", from.before_leg(angle, radius, t)));
        }
        let by = sharp - start;
        // A sharp inside the edge by no more than the thickness needs no cut: the bend's band runs
        // a thickness past its anchor and takes that strip, leaving the panel's other edges (and
        // the rips along them) as they are.
        if by > 0.0 || by < -t - 1e-9 {
            self.move_edge(edge.panel, (a, b, d), by)?;
        }
        let out = Vec2::new(d.y, -d.x);
        let len = a.dist(b);
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
        self.add_panel("flange", &edge, a + out * by, d, len, leg.max(0.0) + sharp, bend);
        let joint = self.sheet.joints.len() - 1;
        self.close_corners(joint)?;
        Ok(joint)
    }

    /// Closes the corners the bend `j`, a flange's just added, makes with the other bends standing
    /// on its panel. Two bends whose anchors cross near an end of each, at a corner the panel's
    /// material fills, with their flanges rising to the same side, share that corner: their
    /// flanges' surfaces meet along the line where their planes cross. Each bend's edge then runs
    /// to the corner and each flange to that line (see `to_line`), and a rip parts the two flanges
    /// along it, as far as both reach. Whatever made the other bend (Convert, Thicken or another
    /// flange), its panel is met alike. A panel whose side at the corner is no straight edge from
    /// its bend keeps it, and the corner stays open.
    fn close_corners(&mut self, j: usize) -> Result<()> {
        let t = self.sheet.thickness;
        let dev = develop(&self.sheet, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
        let places = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?.places;
        let reach_of = |b: &Bend| (b.radius + t) * (b.angle / 2.0).tan();
        for k in 0..self.sheet.joints.len() {
            // The new bend as the corners closed so far left it.
            let Some(Joint { a: on_p, b: on_f, kind: JointKind::Bend(nb) }) = self.sheet.joints.get(j).cloned() else { return Ok(()) };
            let p = on_p.panel;
            let Some(Joint { a, b, kind: JointKind::Bend(ob) }) = self.sheet.joints.get(k).cloned() else { continue };
            if k == j || ob.datum != Datum::Sharp {
                continue;
            }
            // The other bend's anchor on the panel and on its flange, and the side of each its
            // material lies on (1 on the left).
            let (on_q, on_w, side_q) = if a.panel == p {
                (a, b, 1.0)
            } else if b.panel == p {
                (b, a, -1.0)
            } else {
                continue;
            };
            let (w, f) = (on_w.panel, on_f.panel);
            if !matches!(self.sheet.panels.get(w).map(|x| &x.surface), Some(Surface::Plane)) {
                continue;
            }
            let (Some(Some(pf)), Some(Some(pw))) = (places.get(f), places.get(w)) else { continue };
            let Some(c) = cross_lines((on_p.origin, on_p.dir), (on_q.origin, on_q.dir)) else { continue };
            let (s1, s2) = ((c - on_p.origin).dot(on_p.dir), (c - on_q.origin).dot(on_q.dir));
            let nearer = |s: f64, len: f64| if s < len / 2.0 { 0.0 } else { len };
            let (e1, e2) = (nearer(s1, nb.length), nearer(s2, ob.length));
            let reach = reach_of(&nb) + reach_of(&ob) + t;
            if (s1 - e1).abs() > reach || (s2 - e2).abs() > reach {
                continue;
            }
            // A corner the panel fills: each bend runs off into the panel's side of the other.
            let (into1, into2) = (if e1 == 0.0 { on_p.dir } else { -on_p.dir }, if e2 == 0.0 { on_q.dir } else { -on_q.dir });
            if !(into2.dot(on_p.dir.perp()) > 1e-9 && into1.dot(on_q.dir.perp() * side_q) > 1e-9) {
                continue;
            }
            // The line the two flanges' planes meet along, from the corner into the new flange's
            // material (on the right of its anchor), and into the other's too.
            let Some(line) = pf.z.cross(pw.z).normalized() else { continue };
            let flat = |v: Vec3| Vec2::new(v.x, v.y);
            let in_f = |l: Vec3| flat(pf.inverse().vector(l));
            let line = if in_f(line).dot(-on_f.dir.perp()) < 0.0 { -line } else { line };
            let side_w = -side_q;
            let (mf, mw) = (in_f(line), flat(pw.inverse().vector(line)));
            if !(mw.dot(on_w.dir.perp() * side_w) > 1e-9) {
                continue;
            }
            let (Some(rf), Some(rw)) = (self.sheet.panels.get(f).map(|x| x.region.clone()), self.sheet.panels.get(w).map(|x| x.region.clone()))
            else {
                continue;
            };
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
            if let Some(x) = self.sheet.panels.get_mut(f) {
                x.region = rf;
            }
            if let Some(x) = self.sheet.panels.get_mut(w) {
                x.region = rw;
            }
            // Each bend's edge runs to the corner.
            if let Some(Joint { a, b, kind: JointKind::Bend(bend) }) = self.sheet.joints.get_mut(j) {
                to_corner(a, b, bend, s1, e1);
            }
            if let Some(Joint { a, b, kind: JointKind::Bend(bend) }) = self.sheet.joints.get_mut(k) {
                to_corner(a, b, bend, s2, e2);
            }
            let [f0, f1] = ef;
            let [w0, w1] = ew;
            let (df, dw) = ((f1 - f0) * (1.0 / h), (w1 - w0) * (1.0 / h));
            self.sheet.joints.push(Joint {
                a: Anchor { panel: f, origin: f0, dir: df },
                b: Anchor { panel: w, origin: w0, dir: dw },
                kind: JointKind::Rip(Rip {
                    edges: (vec![f0, f1], vec![w0, w1]),
                    curves: (vec![Seg2::Line { a: f0, b: f1 }], vec![Seg2::Line { a: w0, b: w1 }]),
                    turns: vec![turn, turn],
                    lap: Lap::Miter { gap: self.reliefs.gap },
                }),
            });
        }
        Ok(())
    }

    /// A hem on the free edge at `pick`: half a turn at the edge back over the face opposite the
    /// one picked (over it when `flip`), its leg `gap` off that face (the bend's inside radius is
    /// half the gap), `length` to its outer face. The new panel's index.
    /// Folds the sheet along the line from `a` to `b` (the body's coordinates) on a face of one of
    /// its flat panels: the panel is cut in two there, its bend zone where `position` puts it from
    /// the line (`centerline`, `start`, `end` or `mould`, its outside mould line), and the side
    /// that moves (the side away from `fixed`, or else the smaller) bends by `angle` at `radius`,
    /// toward the material or, `flip`, away from it, with every panel joined beyond it. The zone is
    /// the panel's own material, so the flat stays as drawn. Returns the fold's joint.
    #[allow(clippy::too_many_arguments)]
    pub fn add_fold(&mut self, a: Vec3, b: Vec3, angle: f64, radius: f64, position: &str, flip: bool, fixed: Option<Vec3>) -> Result<usize> {
        if !(angle > 1e-6 && angle <= std::f64::consts::PI - 1e-6) {
            return err("the fold angle must be between 0 and 180°");
        }
        if !(radius >= 0.0) {
            return err("the bend radius must not be negative");
        }
        let t = self.sheet.thickness;
        let dev = develop(&self.sheet, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
        let folded = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        // The panel the line lies on: both its ends on one face of the panel, the line across it.
        let on_face = |z: f64| z.abs() < 1e-4 || (z - t).abs() < 1e-4;
        let mut found = None;
        for (i, (panel, place)) in self.sheet.panels.iter().zip(&folded.places).enumerate() {
            let (Surface::Plane, Some(place)) = (&panel.surface, place) else { continue };
            let back = place.inverse();
            let (qa, qb) = (back.point(a), back.point(b));
            if !(on_face(qa.z) && on_face(qb.z) && (qa.z - qb.z).abs() < 1e-4) {
                continue;
            }
            let (a2, b2) = (Vec2::new(qa.x, qa.y), Vec2::new(qb.x, qb.y));
            let Some(d) = (b2 - a2).normalized() else { return err("the fold line has no length") };
            if crate::sheet_surface_convert::extent_across(&panel.region, (a2, d)).is_some() {
                found = Some((i, back, a2, d));
                break;
            }
        }
        let (i, back, a2, d) = found.ok_or_else(|| DocError::Invalid("the fold line must lie on a flat face of the sheet".into()))?;
        let region = self.sheet.panels[i].region.clone();
        // The side that moves lies along `n` from the line.
        let mut n = d.perp();
        let beside = |n: Vec2, from: f64| half(&region, n, from);
        let moves_n = match fixed {
            Some(p) => {
                let q = back.point(p);
                Vec2::new(q.x, q.y).dot(n) < a2.dot(n)
            }
            None => area(&beside(n, a2.dot(n))?) <= area(&beside(-n, -a2.dot(n))?),
        };
        if !moves_n {
            n = -n;
        }
        let l = a2.dot(n);
        let mut bend = Bend {
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
        let ba = bend.allowance(t, self.sheet.k_factor);
        let s0 = match position {
            "centerline" | "center" | "centre" => l - ba / 2.0,
            "start" => l,
            "end" => l - ba,
            "mould" | "mold" => l - (radius + t) * (angle / 2.0).tan(),
            o => return err(format!("unknown fold position `{o}` (centerline, start, end or mould)")),
        };
        // The panel up to the zone stays; past it, it moves.
        let one = |r: Vec<Region2>, what: &str| -> Result<Region2> {
            match <[Region2; 1]>::try_from(r) {
                Ok([one]) => Ok(one),
                Err(r) if r.is_empty() => err(format!("the fold leaves nothing on its {what} side")),
                Err(_) => err(format!("the fold cuts its panel into more than one piece on its {what} side: not supported yet")),
            }
        };
        let stays = one(beside(-n, -s0)?, "fixed")?;
        let moves = one(beside(n, s0 + ba)?, "moving")?;
        // The zone along the line, from the anchors' origin: `a`'s material (the side that stays,
        // against `n`) on the left of its way.
        let u = n.perp();
        let reach = |s: f64| crate::sheet_surface_convert::extent_across(&region, (n * s, u));
        let (Some(near), Some(far)) = (reach(s0), reach(s0 + ba)) else { return err("the fold's bend zone runs off its panel") };
        let start = near.0.min(far.0);
        bend.ends = ZoneEnds { near: (near.0 - start, near.1 - start), far: (far.0 - start, far.1 - start) };
        bend.length = near.1.max(far.1) - start;
        let moved = self.sheet.panels.len();
        // What lay on the moving side goes with it: joints, cut-outs across joints and recesses
        // there; one in the zone itself has no one place.
        let side = |p: Vec2| p.dot(n) - s0;
        let tol = (ba.max(t) * 1e-6).max(1e-9);
        for joint in &mut self.sheet.joints {
            for an in [&mut joint.a, &mut joint.b] {
                if an.panel != i {
                    continue;
                }
                let s = side(an.origin);
                if s >= ba - tol {
                    an.panel = moved;
                } else if s > tol {
                    return err("the fold's bend zone reaches another joint of its panel: not supported yet");
                }
            }
        }
        for c in &mut self.sheet.cuts {
            if c.panel == i && c.region.outer.segs.iter().all(|g| side(g.start()) >= ba - tol) {
                *c = Cut { panel: moved, region: c.region.clone() };
            }
        }
        for r in &mut self.recesses {
            if r.panel != i {
                continue;
            }
            let s: Vec<f64> = r.region.outer.segs.iter().map(|g| side(g.start())).collect();
            if s.iter().all(|x| *x >= ba - tol) {
                r.panel = moved;
            } else if !s.iter().all(|x| *x <= tol) {
                return err("the fold's bend zone runs through a recess: not supported yet");
            }
        }
        for l in &mut self.loose {
            if l.panel != i {
                continue;
            }
            let s: Vec<f64> = l.region.outer.segs.iter().map(|g| side(g.start())).collect();
            if s.iter().all(|x| *x >= ba - tol) {
                l.panel = moved;
            } else if !s.iter().all(|x| *x <= tol) {
                return err("the fold's bend zone runs through a loose piece of the sheet: not supported yet");
            }
        }
        self.sheet.panels[i].region = stays;
        let name = format!("fold {}", self.sheet.joints.len() + 1);
        self.sheet.panels.push(Panel { name, surface: Surface::Plane, region: moves });
        self.places.push(None);
        let j = self.sheet.joints.len();
        self.sheet.joints.push(Joint {
            a: Anchor { panel: i, origin: n * s0 + u * start, dir: u },
            b: Anchor { panel: moved, origin: n * (s0 + ba) + u * start, dir: u },
            kind: JointKind::Bend(bend),
        });
        // Every panel joined beyond the moving side follows the fold.
        let mut reached = std::collections::BTreeSet::from([moved]);
        let mut queue = vec![moved];
        while let Some(p) = queue.pop() {
            for (k, joint) in self.sheet.joints.iter().enumerate() {
                if k == j {
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
        Ok(j)
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
        if mirror {
            out.turn_charts_over();
        }
        Some(out)
    }

    /// Every chart and the flat turned over (v → −v): see [`Self::copied`].
    fn turn_charts_over(&mut self) {
        let f = |p: Vec2| Vec2::new(p.x, -p.y);
        let r = |g: &Region2| crate::sheet_surface_convert::rigid_region(g, f);
        let seg = |s: &Seg2| crate::sheet_surface_convert::rigid_region(&Region2 { outer: Loop2 { segs: vec![*s] }, holes: Vec::new() }, f).outer.segs;
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

    /// The flat face of the sheet a point lies on (the body's coordinates, within `tol`, kept
    /// below a quarter of the thickness): its persistent name, `<panel>:top` for its face on the
    /// sheet's surface and `<panel>:bottom` for the other (`flat:…` unfolded), and its plane (see
    /// [`Self::panel_face`]).
    pub fn panel_face_at(&self, p: Vec3, tol: f64) -> Option<(String, Plane)> {
        let t = self.thickness;
        let tol = tol.min(t * 0.25);
        let dev = develop(&self.sheet, 0).ok()?;
        let frames = crate::sheet_surface_cuts::frames(self, &dev).ok()?;
        for (panel, frame) in self.sheet.panels.iter().zip(&frames) {
            let (Surface::Plane, Some(frame)) = (&panel.surface, frame) else { continue };
            let q = frame.inverse().point(p);
            let top = q.z.abs() < (q.z - t).abs();
            if (if top { q.z.abs() } else { (q.z - t).abs() }) > tol || !panel.region.contains(Vec2::new(q.x, q.y)) {
                continue;
            }
            let name = format!("{}:{}", if self.unfolded { "flat" } else { panel.name.as_str() }, if top { "top" } else { "bottom" });
            let plane = self.panel_face(&name)?;
            return Some((name, plane));
        }
        None
    }

    /// The plane of the flat face named `name` (see [`Self::panel_face_at`]), facing out of the
    /// sheet. Its coordinates are the flat pattern's as it is read (see [`SurfaceSheet::shown`]),
    /// the other face's mirrored, so a sketch on it measures as the flat pattern does.
    pub fn panel_face(&self, name: &str) -> Option<Plane> {
        let (panel, side) = name.rsplit_once(':')?;
        let top = match side {
            "top" => true,
            "bottom" => false,
            _ => return None,
        };
        let i = match panel {
            "flat" => 0,
            name => self.sheet.panels.iter().position(|p| p.name == name)?,
        };
        let dev = develop(&self.sheet, 0).ok()?;
        let frames = crate::sheet_surface_cuts::frames(self, &dev).ok()?;
        let frame = frames.get(i).copied().flatten()?;
        let back = dev.placements.get(i).copied().flatten()?.inverse();
        let o = back.point(Vec2::new(0.0, 0.0));
        let (x, y) = (back.vector(Vec2::new(1.0, 0.0)), back.vector(Vec2::new(0.0, if top { -1.0 } else { 1.0 })));
        let z = if top { 0.0 } else { self.thickness };
        Plane::new(frame.point(Vec3::new(o.x, o.y, z)), frame.vector(Vec3::new(x.x, x.y, 0.0)), frame.vector(Vec3::new(y.x, y.y, 0.0)))
    }

    /// Whether the line from `a` to `b` lies on a face of one of the sheet's flat panels, across
    /// it: what [`Self::add_fold`] folds.
    pub fn fold_panel_at(&self, a: Vec3, b: Vec3) -> bool {
        let t = self.sheet.thickness;
        let Ok(dev) = develop(&self.sheet, 0) else { return false };
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
        let dev = develop(&self.sheet, 0).ok()?;
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
        let (a, _, d) = straight(&edge)?;
        let dev = develop(&self.sheet, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
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
    // `after` the vertex in the loop's order.
    let Some(k) = (0..count).find(|k| segs.get(*k).is_some_and(|s| s.end().dist(corner) <= tol)) else { return Ok(None) };
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
