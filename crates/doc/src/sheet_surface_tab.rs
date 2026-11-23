//! Tab on a sheet made from surfaces (#8325, #6748): the closed profiles of a sketch on a flat
//! panel's face join that panel's outline, in the panel's chart, through the thickness. It is a
//! change of the panel's outline only: the panel lays flat with it, the flat pattern and the DXF
//! follow, and no joint changes.

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_develop::prelude::*;
use solidcraft_develop::{JointKind, Surface};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::BoolOp;

use crate::sheet::{TAB_ADDS_NOTHING, TAB_APART, TAB_IN_BEND, TAB_OFF_SHEET, TAB_OVERLAPS, TAB_SEVERAL};
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

    /// Joins `profile` (panel `i`'s chart) to panel `i`'s outline.
    fn tab_on(&mut self, i: usize, profile: &[Region2]) -> Result<()> {
        let region = self.sheet.panels.get(i).map(|p| p.region.clone()).ok_or_else(|| DocError::Invalid("tab".into()))?;
        let t = self.thickness;
        // The panel's joints: each runs along a line of its chart, the panel's material on one
        // side (an `a` anchor's on its left, a `b` anchor's on its right, see
        // [`solidcraft_develop::Anchor`]). Across the line, along its length, is the bend's zone
        // or the next panel: no part of this one.
        let reach = {
            let b = region.outer.polyline(1e-3).bounds();
            let p = profile.iter().map(|r| r.outer.polyline(1e-3).bounds()).fold(b, |x, y| x.union(&y));
            (p.size().len() + p.min.len() + p.max.len() + 1.0) * 4.0
        };
        for joint in &self.sheet.joints {
            let length = match &joint.kind {
                JointKind::Bend(b) => b.length,
                JointKind::Rip(r) => r.edges.0.first().zip(r.edges.0.last()).map_or(0.0, |(p, q)| p.dist(*q)),
                JointKind::Seam => continue,
            };
            for (an, away) in [(&joint.a, -1.0), (&joint.b, 1.0)] {
                if an.panel != i {
                    continue;
                }
                let across = an.dir.perp() * away;
                let (p, q) = (an.origin, an.origin + an.dir * length);
                let strip = Loop2::polygon(&[p, q, q + across * reach, p + across * reach]).ccw();
                let inside =
                    solidcraft_kernel::planar_regions_boolean(profile, &[Region2 { outer: strip, holes: Vec::new() }], BoolOp::Intersect, 1e-9)?;
                if area(&inside) > 1e-6 {
                    return err(TAB_IN_BEND);
                }
            }
        }
        let joined = solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(&region), profile, BoolOp::Union, 1e-9)?;
        let [one] = <[Region2; 1]>::try_from(joined).map_err(|_| DocError::Invalid(TAB_APART.into()))?;
        let one = oriented(one);
        let added = solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(&one), std::slice::from_ref(&region), BoolOp::Cut, 1e-9)?;
        if area(&added) <= 1e-6 * area(std::slice::from_ref(&region)).max(1.0) {
            return err(TAB_ADDS_NOTHING);
        }
        // Folded, what the tab adds must stay clear of the sheet's other pieces.
        let others = self.pieces_in_space().map(|(_, boxes)| boxes).unwrap_or_default();
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        if let Some(frame) = crate::sheet_surface_cuts::frames(self, &dev)?.get(i).copied().flatten() {
            let mut b = solidcraft_geom::Aabb3::EMPTY;
            for r in &added {
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
            if others.iter().any(|(panel, o)| *panel != Some(i) && overlaps(o)) {
                return err(TAB_OVERLAPS);
            }
        }
        if let Some(p) = self.sheet.panels.get_mut(i) {
            p.region = one;
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
}
