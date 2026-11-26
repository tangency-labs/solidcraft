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
    /// hole cut inside its panel (see [`Self::fill_cuts`]), or it is refused, with the reason.
    /// Before tabs could fill such cuts, a tab over one joined the outline and the cut kept what
    /// it took under the tab, without saying so. So a saved design with such a tab rebuilds
    /// differently: where the tab can fill the cut, with that material too (a tab over a cut at a
    /// bend's end: +140 mm3 before, +190 now); where it cannot, refused, not built with part of
    /// it missing.
    fn tab_on(&mut self, i: usize, profile: &[Region2]) -> Result<()> {
        let region = self.sheet.panels.get(i).map(|p| p.region.clone()).ok_or_else(|| DocError::Invalid("tab".into()))?;
        // The sheet's cuts across joints that the profile meets, each in panel `i`'s chart:
        // through the flat, where every panel's chart has its place.
        let dev = if self.sheet.cuts.is_empty() { None } else { Some(self.develop().map_err(|e| DocError::Invalid(e.to_string()))?) };
        let mut met: Vec<(usize, Region2)> = Vec::new();
        if let Some((dev, back)) = dev.as_ref().and_then(|d| Some((d, d.placements.get(i).copied().flatten()?.inverse()))) {
            for (k, c) in self.sheet.cuts.iter().enumerate() {
                // Every cut's panel has its place: the development refuses a sheet with a cut on
                // a panel it does not lay out.
                let Some(there) = dev.placements.get(c.panel).copied().flatten() else { continue };
                let cut = there.then(&back).region(&c.region);
                let inside = solidcraft_kernel::planar_regions_boolean(profile, std::slice::from_ref(&cut), BoolOp::Intersect, 1e-9)?;
                if area(&inside) > 1e-6 {
                    met.push((k, cut));
                }
            }
        }
        // The panel's joints: each runs along a line of its chart, the panel's material on one
        // side (an `a` anchor's on its left, a `b` anchor's on its right, see
        // [`solidcraft_develop::Anchor`]). Across the line, along its length, is the bend's zone
        // or the next panel: no part of this one. These are the strips across them, along what
        // `cuts` (panel `i`'s chart) leave of each bend: where a cut took a bend away, the tab
        // may fill the gap (see [`Self::fill_cuts`], which checks what it fills).
        let reach = {
            let b = region.outer.polyline(1e-3).bounds();
            let p = profile.iter().map(|r| r.outer.polyline(1e-3).bounds()).fold(b, |x, y| x.union(&y));
            (p.size().len() + p.min.len() + p.max.len() + 1.0) * 4.0
        };
        let across = |cuts: &[Region2]| {
            let mut strips = Vec::new();
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
                    let left =
                        if bend && !cuts.is_empty() { crate::sheet_surface_convert::left_along(an, length, cuts, 1e-6) } else { vec![(0.0, length)] };
                    let across = an.dir.perp() * away;
                    for (from, to) in left {
                        let (p, q) = (an.origin + an.dir * from, an.origin + an.dir * to);
                        strips.push(Region2 { outer: Loop2::polygon(&[p, q, q + across * reach, p + across * reach]).ccw(), holes: Vec::new() });
                    }
                }
            }
            strips
        };
        let whole = across(&[]);
        match dev.as_ref().filter(|_| !met.is_empty()) {
            None => self.join_outline(i, profile, &region, &whole),
            Some(dev) => {
                let met_cuts: Vec<Region2> = met.iter().map(|(_, c)| c.clone()).collect();
                self.fill_cuts(i, profile, &region, (&across(&met_cuts), &whole), dev, &met)
            }
        }
    }

    /// Joins `profile` (panel `i`'s chart) to panel `i`'s outline `region`, as it is: no part of
    /// it in the `strips` across the panel's joints (see [`Self::tab_on`]).
    fn join_outline(&mut self, i: usize, profile: &[Region2], region: &Region2, strips: &[Region2]) -> Result<()> {
        clear_of(profile, strips)?;
        let one = joined(region, profile)?;
        let added = solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(&one), std::slice::from_ref(region), BoolOp::Cut, 1e-9)?;
        if area(&added) <= 1e-6 * area(std::slice::from_ref(region)).max(1.0) {
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
        self.flat_apart()
    }

    /// Flat, no piece of the sheet may run onto another.
    fn flat_apart(&self) -> Result<()> {
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

    /// The tab `profile` (panel `i`'s chart, outline `region`), which meets the sheet's cuts
    /// across joints `met` (each its place in the sheet's cuts and its region in panel `i`'s
    /// chart): each of those cuts loses what the profile covers of it, so that a tab fills what an
    /// earlier cut across a bend took, as it fills a hole cut inside its panel. What it gives back
    /// of a bend's zone rides flat with the panel, as a piece of zone the cuts leave joined to that
    /// panel only does (see [`crate::sheet_surface::Piece::Tab`]); the rest of the profile joins
    /// the outline. `strips` are the strips across the panel's joints (see [`Self::tab_on`]):
    /// along what the met cuts leave of each bend, where no part of the profile may lie, and along
    /// each whole joint, past which what the profile covers of a cut is given back to the cut's
    /// pieces only, never to the outline as well. The profile must touch or overlap the outline,
    /// as any tab's must. The flat it makes is checked against the one before, exactly, and
    /// refused unless:
    /// - what it joins touches material the panel has built (its own, or a piece of zone riding
    ///   with it): no part of it floats in a gap;
    /// - what it gives back ends before the far side of each bend it fills, and no other piece of
    ///   the flat changes: what it gives back must not bend with what is left of a bend, nor join
    ///   the next panel;
    /// - all of what it gives back rides with the panel, none of it cut away again;
    /// - it adds material;
    /// - folded, what it adds stays clear of the sheet's other pieces, and flat, no piece runs
    ///   onto another.
    fn fill_cuts(
        &mut self,
        i: usize,
        profile: &[Region2],
        region: &Region2,
        strips: (&[Region2], &[Region2]),
        dev: &Development,
        met: &[(usize, Region2)],
    ) -> Result<()> {
        clear_of(profile, strips.0)?;
        let tiny = 1e-6 * area(std::slice::from_ref(&joined(region, profile)?)).max(1.0);
        let here = dev.placements.get(i).copied().flatten().ok_or_else(|| DocError::Invalid(TAB_OFF_SHEET.into()))?;
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
        let taken = crate::sheet_surface::planar_union(met.iter().map(|(_, c)| c.clone()).collect(), &mut FlatWork::new())?;
        let back = solidcraft_kernel::planar_regions_boolean(profile, &taken, BoolOp::Intersect, 1e-9)?;
        // What it gives back past a joint's line belongs to the pieces the cuts took it from:
        // the outline there would lie on them in the flat (past the trim, which ends a thickness
        // past the line, short of a bend's far side). A profile that runs on past what the cuts
        // took there lies on what is left past the line: it joins the outline whole, and the
        // checks below refuse it, as they refuse whatever runs past a cut.
        let past = solidcraft_kernel::planar_regions_boolean(&back, strips.1, BoolOp::Intersect, 1e-9)?;
        let outline =
            if area(&past) > tiny { solidcraft_kernel::planar_regions_boolean(profile, &past, BoolOp::Cut, 1e-9)? } else { profile.to_vec() };
        let one = match outline.is_empty() {
            true => region.clone(),
            false => joined(region, &outline).or_else(|_| joined(region, profile))?,
        };
        // Past the far side of a bend it fills, what it gives back runs along the line where the
        // bend's other panel begins in the flat.
        for zone in &dev.zones {
            let far = match zone.panels {
                [a, _] if a == i => zone.far,
                [_, b] if b == i => zone.near,
                _ => continue,
            };
            if back.iter().any(|r| crate::sheet_surface::along(&here.region(r), far)) {
                return err(TAB_PAST_BEND);
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
            // What is left of the cut, however small, stays cut; only the booleans' slivers of no
            // width, where the profile's edge lies along the cut's, are none.
            cuts.extend(
                left.into_iter()
                    .map(oriented)
                    .filter(|r| crate::sheet_surface_features::width(r) > crate::sheet_surface_features::SLIVER)
                    .map(|r| Cut { panel: c.panel, region: r }),
            );
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
        let lost = solidcraft_kernel::planar_regions_boolean(&back, &after, BoolOp::Cut, 1e-9)?;
        if area(&lost) > tiny {
            return err(TAB_CUT_AWAY);
        }
        let added = solidcraft_kernel::planar_regions_boolean(&after, &before, BoolOp::Cut, 1e-9)?;
        if area(&added) <= tiny {
            return err(TAB_ADDS_NOTHING);
        }
        self.clear_folded(i, &added, &others)?;
        self.flat_apart()
    }
}

/// No part of `profile` lies in `strips`, the strips across its panel's joints (see
/// [`SurfaceSheet::tab_on`]).
fn clear_of(profile: &[Region2], strips: &[Region2]) -> Result<()> {
    for strip in strips {
        let inside = solidcraft_kernel::planar_regions_boolean(profile, std::slice::from_ref(strip), BoolOp::Intersect, 1e-9)?;
        if area(&inside) > 1e-6 {
            return err(TAB_IN_BEND);
        }
    }
    Ok(())
}

/// `region` joined with `profile`, which must make one region: the profile touches or overlaps
/// the outline.
fn joined(region: &Region2, profile: &[Region2]) -> Result<Region2> {
    let joined = solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(region), profile, BoolOp::Union, 1e-9)?;
    let [one] = <[Region2; 1]>::try_from(joined).map_err(|_| DocError::Invalid(TAB_APART.into()))?;
    Ok(oriented(one))
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
