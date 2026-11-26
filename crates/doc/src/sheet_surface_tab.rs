//! Tab on a sheet made from surfaces (#8325, #6748): the closed profiles of a sketch on a flat
//! panel's face join that panel's outline, in the panel's chart, through the thickness. It is a
//! change of the panel's outline, and of the sheet's cuts across joints where the tab fills what
//! they took (#22259): the panel lays flat with it, the flat pattern and the DXF follow, and no
//! joint changes.

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_develop::prelude::*;
use solidcraft_develop::{Cut, Development, JointKind, Surface};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::BoolOp;

use crate::sheet::{FlatWork, TAB_ADDS_NOTHING, TAB_APART, TAB_CUT_AWAY, TAB_IN_BEND, TAB_OFF_SHEET, TAB_OVERLAPS, TAB_PAST_BEND, TAB_SEVERAL};
use crate::sheet_surface::Piece;
use crate::{DocError, Result, SurfaceSheet};

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(DocError::Invalid(msg.into()))
}

/// The largest coordinate a tab's profile may reach, in mm: past it the planar booleans lose the
/// sheet's tolerance.
const FAR: f64 = 1e7;

fn area(r: &[Region2]) -> f64 {
    r.iter().map(|g| g.area().abs()).sum()
}

/// `r` with its outer loop counter-clockwise and its holes clockwise.
fn oriented(r: Region2) -> Region2 {
    Region2 { outer: r.outer.ccw(), holes: r.holes.iter().map(|h| h.ccw().reversed()).collect() }
}

impl SurfaceSheet {
    /// A tab: the closed `regions` of a sketch in `plane` join the flat panel whose face (either
    /// side) the plane lies in, through the thickness. Where the plane lies in the faces of several
    /// panels, the one panel that takes them; which is never a matter of the panels' order, so a
    /// tab more than one would take is refused. Returns that panel's index; an error leaves the
    /// sheet as it was.
    pub fn add_tab(&mut self, plane: &Plane, regions: &[Region2]) -> Result<usize> {
        if self.unfolded {
            return err("refold the sheet before adding a tab");
        }
        if regions.is_empty() {
            return err("the tab needs a closed profile");
        }
        let finite = |p: Vec2| p.x.is_finite() && p.y.is_finite() && p.x.abs() < FAR && p.y.abs() < FAR;
        let all_finite =
            regions.iter().all(|r| std::iter::once(&r.outer).chain(&r.holes).all(|l| (&l.polyline(1e-3)).into_iter().all(|p| finite(*p))));
        let plane_ok = [plane.origin, plane.x, plane.y].iter().all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite());
        if !all_finite || !plane_ok {
            return err(TAB_OFF_SHEET);
        }
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let frames = crate::sheet_surface_cuts::frames(self, &dev)?;
        let t = self.thickness;
        let mut first = None;
        let mut took: Vec<(usize, SurfaceSheet)> = Vec::new();
        for (i, (panel, frame)) in self.sheet.panels.iter().zip(&frames).enumerate() {
            let (Surface::Plane, Some(frame)) = (&panel.surface, frame) else { continue };
            let z = (plane.origin - frame.shift).dot(frame.z);
            if frame.z.cross(plane.normal()).len() > 1e-6 || !(z.abs() < 1e-6 || (z - t).abs() < 1e-6) {
                continue;
            }
            // The profile in the panel's chart.
            let back = frame.inverse();
            let map = |p: Vec2| {
                let q = back.point(plane.to_world(p));
                Vec2::new(q.x, q.y)
            };
            let profile: Vec<Region2> = regions.iter().map(|r| oriented(crate::sheet_surface_convert::rigid_region(r, map))).collect();
            let mut work = self.clone();
            match work.tab_on(i, &profile) {
                Ok(()) => took.push((i, work)),
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        match <[(usize, SurfaceSheet); 1]>::try_from(took) {
            Ok([(i, work)]) => {
                *self = work;
                Ok(i)
            }
            Err(took) if !took.is_empty() => err(TAB_SEVERAL),
            Err(_) => Err(first.unwrap_or_else(|| DocError::Invalid(TAB_OFF_SHEET.into()))),
        }
    }

    /// Joins `profile` (panel `i`'s chart) to panel `i`'s outline. Where it meets the sheet's
    /// cuts across joints (see [`Cut`]), it also gives back what they took there, as it fills a
    /// hole cut inside its panel (see [`Self::fill_cuts`]).
    fn tab_on(&mut self, i: usize, profile: &[Region2]) -> Result<()> {
        let region = self.sheet.panels.get(i).map(|p| p.region.clone()).ok_or_else(|| DocError::Invalid("tab".into()))?;
        // The sheet's cuts across joints that the profile meets, each in panel `i`'s chart:
        // through the flat, where every panel's chart has its place.
        let dev = if self.sheet.cuts.is_empty() { None } else { Some(self.develop().map_err(|e| DocError::Invalid(e.to_string()))?) };
        let mut met: Vec<(usize, Region2)> = Vec::new();
        if let Some((dev, back)) = dev.as_ref().and_then(|d| Some((d, d.placements.get(i).copied().flatten()?.inverse()))) {
            for (k, c) in self.sheet.cuts.iter().enumerate() {
                let Some(there) = dev.placements.get(c.panel).copied().flatten() else { continue };
                let cut = there.then(&back).region(&c.region);
                let inside = solidcraft_kernel::planar_regions_boolean(profile, std::slice::from_ref(&cut), BoolOp::Intersect, 1e-9)?;
                if area(&inside) > 1e-6 {
                    met.push((k, cut));
                }
            }
        }
        let met_cuts: Vec<Region2> = met.iter().map(|(_, c)| c.clone()).collect();
        // The panel's joints: each runs along a line of its chart, the panel's material on one
        // side (an `a` anchor's on its left, a `b` anchor's on its right, see
        // [`solidcraft_develop::Anchor`]). Across the line, along its length, is the bend's zone
        // or the next panel: no part of this one. Only what the cuts the profile meets leave of a
        // bend counts: where they took it away, the tab may fill the gap (see
        // [`Self::fill_cuts`], which checks what it fills).
        let reach = {
            let b = region.outer.polyline(1e-3).bounds();
            let p = profile.iter().map(|r| r.outer.polyline(1e-3).bounds()).fold(b, |x, y| x.union(&y));
            (p.size().len() + p.min.len() + p.max.len() + 1.0) * 4.0
        };
        for joint in &self.sheet.joints {
            let (length, bend) = match &joint.kind {
                JointKind::Bend(b) => (b.length, true),
                JointKind::Rip(r) => (r.edges.0.first().zip(r.edges.0.last()).map_or(0.0, |(p, q)| p.dist(*q)), false),
                JointKind::Seam => continue,
            };
            for (an, away) in [(&joint.a, -1.0), (&joint.b, 1.0)] {
                if an.panel != i {
                    continue;
                }
                let left = if bend && !met_cuts.is_empty() {
                    crate::sheet_surface_convert::left_along(an, length, &met_cuts, 1e-6)
                } else {
                    vec![(0.0, length)]
                };
                let across = an.dir.perp() * away;
                for (from, to) in left {
                    let (p, q) = (an.origin + an.dir * from, an.origin + an.dir * to);
                    let strip = Loop2::polygon(&[p, q, q + across * reach, p + across * reach]).ccw();
                    let inside =
                        solidcraft_kernel::planar_regions_boolean(profile, &[Region2 { outer: strip, holes: Vec::new() }], BoolOp::Intersect, 1e-9)?;
                    if area(&inside) > 1e-6 {
                        return err(TAB_IN_BEND);
                    }
                }
            }
        }
        let joined = solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(&region), profile, BoolOp::Union, 1e-9)?;
        let [one] = <[Region2; 1]>::try_from(joined).map_err(|_| DocError::Invalid(TAB_APART.into()))?;
        let one = oriented(one);
        match dev.as_ref().filter(|_| !met.is_empty()) {
            Some(dev) => self.fill_cuts(i, profile, one, dev, &met)?,
            None => {
                let added = solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(&one), std::slice::from_ref(&region), BoolOp::Cut, 1e-9)?;
                if area(&added) <= 1e-6 * area(std::slice::from_ref(&region)).max(1.0) {
                    return err(TAB_ADDS_NOTHING);
                }
                // Folded, what the tab adds must stay clear of the sheet's other pieces.
                let others: Vec<solidcraft_geom::Aabb3> = self
                    .pieces_in_space()
                    .map(|(_, boxes)| boxes)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|(panel, _)| *panel != Some(i))
                    .map(|(_, b)| b)
                    .collect();
                self.clear_folded(i, &added, &others)?;
                if let Some(p) = self.sheet.panels.get_mut(i) {
                    p.region = one;
                }
            }
        }
        // Flat, the panel must not run onto another.
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let pieces = crate::sheet_surface::pieces(&dev, &mut crate::sheet::FlatWork::new())?;
        let regions: Vec<Region2> = pieces.into_iter().map(|(_, r)| r).collect();
        let whole = crate::sheet_surface::planar_union(regions.clone(), &mut crate::sheet::FlatWork::new())?;
        if (area(&regions) - area(&whole)).abs() > 1e-6 * area(&regions).max(1.0) {
            return err(TAB_OVERLAPS);
        }
        Ok(())
    }

    /// Folded, `added` (panel `i`'s chart) must stay clear of every box of `others` (the body's
    /// coordinates).
    fn clear_folded(&self, i: usize, added: &[Region2], others: &[solidcraft_geom::Aabb3]) -> Result<()> {
        let t = self.thickness;
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let Some(frame) = crate::sheet_surface_cuts::frames(self, &dev)?.get(i).copied().flatten() else { return Ok(()) };
        let mut b = solidcraft_geom::Aabb3::EMPTY;
        for r in added {
            for p in &r.outer.polyline(1e-3) {
                for z in [0.0, t] {
                    b.add(frame.point(Vec3::new(p.x, p.y, z)));
                }
            }
        }
        let shrink = (t * 1e-3).max(1e-6);
        let overlaps = |o: &solidcraft_geom::Aabb3| {
            let part = |lo: f64, hi: f64, olo: f64, ohi: f64| hi.min(ohi) - lo.max(olo) > shrink;
            part(b.min.x, b.max.x, o.min.x, o.max.x) && part(b.min.y, b.max.y, o.min.y, o.max.y) && part(b.min.z, b.max.z, o.min.z, o.max.z)
        };
        if others.iter().any(overlaps) {
            return err(TAB_OVERLAPS);
        }
        Ok(())
    }

    /// The tab `profile` (panel `i`'s chart), which meets the sheet's cuts across joints `met`
    /// (each its place in the sheet's cuts and its region in panel `i`'s chart): panel `i`'s
    /// outline becomes `one`, and each of those cuts loses what the profile covers of it, so that
    /// a tab fills what an earlier cut across a bend took, as it fills a hole cut inside its
    /// panel. What it gives back of a bend's zone rides flat with the panel, as a piece of zone
    /// the cuts leave joined to that panel only does (see [`crate::sheet_surface::Piece::Tab`]).
    /// The flat it makes is checked against the one before, exactly, and refused unless:
    /// - the profile touches material of the panel's (its own or a piece of zone riding with
    ///   it), as it would touch the panel's outline;
    /// - no other piece of the flat changes: what the tab gives back must not bend with what is
    ///   left of a bend, nor join the next panel;
    /// - all of what it gives back rides with the panel, none of it cut away again;
    /// - it adds material;
    /// - folded, what it adds stays clear of the sheet's other pieces.
    fn fill_cuts(&mut self, i: usize, profile: &[Region2], one: Region2, dev: &Development, met: &[(usize, Region2)]) -> Result<()> {
        let here = dev.placements.get(i).copied().flatten().ok_or_else(|| DocError::Invalid(TAB_OFF_SHEET.into()))?;
        let tiny = 1e-6 * area(std::slice::from_ref(&one)).max(1.0);
        let others: Vec<solidcraft_geom::Aabb3> = self
            .piece_boxes_in_space()
            .map(|(_, boxes)| boxes)
            .unwrap_or_default()
            .into_iter()
            .filter(|(piece, _)| !matches!(piece, Some(Piece::Panel(p) | Piece::Tab(_, p)) if *p == i))
            .map(|(_, b)| b)
            .collect();
        let (riding, rest) = riding_with(dev, i)?;
        let before = crate::sheet_surface::planar_union(riding, &mut FlatWork::new())?;
        // The profile touches what the panel has built: no part of what it joins floats apart.
        let with = crate::sheet_surface::planar_union(before.iter().cloned().chain(profile.iter().cloned()).collect(), &mut FlatWork::new())?;
        for r in &with {
            let held = solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(r), &before, BoolOp::Intersect, 1e-9)?;
            if area(&held) <= tiny {
                return err(TAB_APART);
            }
        }
        let mut cuts = Vec::with_capacity(self.sheet.cuts.len());
        for (k, c) in self.sheet.cuts.iter().enumerate() {
            let there = dev.placements.get(c.panel).copied().flatten();
            let (true, Some(there)) = (met.iter().any(|(m, _)| *m == k), there) else {
                cuts.push(c.clone());
                continue;
            };
            let onto = here.then(&there.inverse());
            let cover: Vec<Region2> = profile.iter().map(|r| oriented(onto.region(r))).collect();
            let left = solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(&c.region), &cover, BoolOp::Cut, 1e-9)?;
            cuts.extend(left.into_iter().filter(|r| r.area().abs() > tiny).map(|r| Cut { panel: c.panel, region: oriented(r) }));
        }
        self.sheet.cuts = cuts;
        if let Some(p) = self.sheet.panels.get_mut(i) {
            p.region = one;
        }
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let (riding, rest_after) = riding_with(&dev, i)?;
        // No other piece changes.
        let changed = |past: bool| {
            rest.iter().chain(&rest_after).any(|(piece, _)| {
                let sum = |of: &[(Piece, f64)]| of.iter().filter(|(p, _)| p == piece).map(|(_, a)| a).sum::<f64>();
                matches!(piece, Piece::Zone(_)) != past && (sum(&rest) - sum(&rest_after)).abs() > tiny
            })
        };
        if changed(true) {
            return err(TAB_PAST_BEND);
        }
        if changed(false) {
            return err(TAB_IN_BEND);
        }
        // All it gives back rides with the panel.
        let after = crate::sheet_surface::planar_union(riding, &mut FlatWork::new())?;
        let taken = crate::sheet_surface::planar_union(met.iter().map(|(_, c)| c.clone()).collect(), &mut FlatWork::new())?;
        let back = solidcraft_kernel::planar_regions_boolean(profile, &taken, BoolOp::Intersect, 1e-9)?;
        let lost = solidcraft_kernel::planar_regions_boolean(&back, &after, BoolOp::Cut, 1e-9)?;
        if area(&lost) > tiny {
            return err(TAB_CUT_AWAY);
        }
        let added = solidcraft_kernel::planar_regions_boolean(&after, &before, BoolOp::Cut, 1e-9)?;
        if area(&added) <= tiny {
            return err(TAB_ADDS_NOTHING);
        }
        self.clear_folded(i, &added, &others)
    }
}

/// The pieces of `dev`'s flat that ride with panel `i` (its own and the pieces of zone left
/// joined to it only), in its chart; and the area of every other piece, by piece.
fn riding_with(dev: &Development, i: usize) -> Result<(Vec<Region2>, Vec<(Piece, f64)>)> {
    let back = dev.placements.get(i).copied().flatten().ok_or_else(|| DocError::Invalid(TAB_OFF_SHEET.into()))?.inverse();
    let mut riding = Vec::new();
    let mut rest: Vec<(Piece, f64)> = Vec::new();
    for (piece, r) in crate::sheet_surface::pieces(dev, &mut FlatWork::new())? {
        match piece {
            Piece::Panel(p) | Piece::Tab(_, p) if p == i => riding.push(back.region(&r)),
            _ => rest.push((piece, r.area().abs())),
        }
    }
    Ok((riding, rest))
}
