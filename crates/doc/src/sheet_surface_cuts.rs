//! Cuts on a sheet made from surfaces (#6748). What a feature that is not sheet metal cuts from a
//! sheet's body, folded or unfolded, is read back onto the sheet's flat panels, so that the sheet
//! stays the master and the folded part, its flat and its DXF are all built from it alike:
//!
//! - where the cut goes through, the panel's outline loses it, as if the panel had been drawn so:
//!   no record of the cut is kept. Where it reaches an edge a rip cut the panel back to (the gap
//!   that closes a corner), it runs on across the rip's lap to the rip's line: the panel's
//!   outline runs to that line, past its material, and the rip cuts back what is left of it;
//! - where it stops inside the thickness on a floor parallel to the panel, the panel keeps a
//!   recess, from its own face or the face one thickness into the material, to the floor's depth;
//!   a later cut through a recess takes it with the material.
//!
//! The panels' faces are read before and after the feature, exactly, from the bodies' B-rep
//! records. A cut through a roll, or with walls not square to a flat panel (a hole at a slant),
//! goes through the panel as its shadow: everything whose fiber through the thickness meets
//! what the cut took, as a shop cuts it in the flat ([`crate::sheet_surface_shadow`]). Whether
//! walls are square is read from their surfaces too, not only from what the panel's faces show
//! of them: a bore lying in a panel (a cylinder along it) runs face to face, but its faces show it
//! narrower than it is at its axis. Its shadow is read in closed form, a band as wide as the bore
//! is where it is widest within the thickness, between its ends (see [`lying`]). A cut whose
//! walls cross a joint (a bend, a seam) is read the same way across every piece it
//! reaches at once, its outline in the flat, and kept as a cut of the sheet's: what it leaves
//! of a bend bends, or is a tab of the one panel it still joins. Unfolded, the sheet lies in one
//! plane, and such a cut with walls square to it is what the flat's faces lost, read over the
//! whole flat at once, up to the sheet's outline where it reaches it. Every face the cut made
//! must be read; checking each step, the sheet built again must also weigh what the cut body
//! does, or, where a shadow was cut, no more (a tab weighs flat). Refused: walls that don't run
//! through the sheet from face to face (a countersink, a rounded or chamfered face edge, a cut
//! that stops inside a bend, a bore lying in the sheet that reaches one face or none), a recess in
//! a roll, a bore lying in a panel that reaches its edge or ends other than square to its axis,
//! and, folded, a cut through the sheet's outline at a slant, on a roll or across a bend (not
//! supported yet).
//! A piece a cut parts from a panel that holds none of its joints (a letter's counter, the end
//! of a flange cut off beside its bend) stays where it was, a loose piece of the sheet: in the
//! folded body, in the flat and in its DXF. A cut that parts a panel into pieces that each hold
//! some of its joints leaves the panel whole and is kept as a cut of the sheet's, in the panel's
//! chart: each piece folds and lies flat where the panel does. A cut that leaves the sheet in
//! several pieces says nothing of it: every piece is as much the sheet as any other.

use solidcraft_geom::prelude::*;
use std::collections::BTreeSet;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_record::brep::Loop as Bounds;
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_develop::{Cut, Development, EdgeFrom, Fold, Joint, JointKind, Query, Rigid2, Rigid3, Surface};
use solidcraft_kernel::{Body, BoolOp, FaceSurface, face_surface, planar_regions_boolean};

use crate::sheet::FlatWork;
use crate::sheet_surface::{Piece, holds_clear};
use crate::sheet_surface_shadow::{Carrier, Folded, Slab, around, components, reached, shadow, walls as made, within};
use crate::{DocError, Feature, FeatureKind, ModelState, Result, SurfaceSheet};

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(DocError::Invalid(msg.into()))
}

/// A recess in a flat panel of a sheet made from surfaces: the panel's material within `region`
/// (in the panel's chart) taken out from its own face (the sheet's surface), or from its other
/// face when `far`, to `depth`, short of the thickness.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Recess {
    pub panel: usize,
    pub region: Region2,
    pub far: bool,
    pub depth: f64,
}

/// What a body has of a flat panel, each in the panel's chart: the material of the panel's own
/// face (`near`) and of its face one thickness into the material (`far`), and the floors between,
/// with their depths from the face they are seen from.
#[derive(Default)]
struct OnPanel {
    near: Vec<Region2>,
    far: Vec<Region2>,
    near_floors: Vec<(f64, Region2)>,
    far_floors: Vec<(f64, Region2)>,
}

/// An extrude as it was drawn: its sketch plane and the interval along the plane's normal it ran
/// over (see `ModelState::cut_extents`).
pub(crate) type Drawn = (Plane, (f64, f64));

/// The depth of a floor through the point `floor` from the panel's face through `face`, the panel's
/// material toward `z`, `measured` in the panel's frame. Where the extrude `drawn` that cut it ran
/// from that face to the floor (each an end of its interval, to the `rounding` of the body's
/// numbers, see [`rounding`]), the pocket is as deep as the extrude ran, exactly: a folded
/// panel's frame is where its fold puts it, to a rounding, and the floor's plane measured in it is
/// that rounding off what was asked for. A floor any farther off is another floor.
fn drawn_depth(measured: f64, face: Vec3, floor: Vec3, z: Vec3, drawn: Option<&Drawn>, rounding: f64) -> f64 {
    let Some((plane, (a, b))) = drawn else { return measured };
    let n = plane.normal();
    if z.cross(n).len() > 64.0 * f64::EPSILON {
        return measured;
    }
    let at = |p: Vec3| (p - plane.origin).dot(n);
    let (from, to) = (at(face), at(floor));
    match [(*a, *b), (*b, *a)].into_iter().find(|(s, e)| (from - s).abs() <= rounding && (to - e).abs() <= rounding) {
        Some((s, e)) if ((e - s).abs() - measured).abs() <= rounding => (e - s).abs(),
        _ => measured,
    }
}

/// The planar faces of `b` parallel to a panel whose frame is `frame` (its material toward
/// `z`, `t` deep), within its region `own` (or, unfolded, the whole flat's regions). A floor the
/// extrude `drawn` made is as deep as it ran (see [`drawn_depth`]).
fn on_panel(b: &Body, q: &Query, frame: &Rigid3, t: f64, own: &[Region2], tol: f64, drawn: Option<&Drawn>) -> Result<OnPanel> {
    let mut out = OnPanel::default();
    for f in 0..q.record().faces.len() {
        let Some(FaceSurface::Plane { normal, .. }) = face_surface(b, f) else { continue };
        // Parallel to the panel within the body's tolerance across it: a body built again can
        // lean its planes by rounding.
        if normal.cross(frame.z).len() * b.size() > tol {
            continue;
        }
        let Some((plane, face_out)) = crate::sheet_surface_convert::planar_face(b, q, f, tol) else { continue };
        let depth = (plane.origin - frame.shift).dot(frame.z);
        if !(depth > -tol && depth < t + tol) {
            continue;
        }
        // The panel's chart, at the face's depth.
        let Some(at) = Plane::new(frame.shift + frame.z * depth, frame.x, frame.y) else { continue };
        let Some(region) = crate::sheet_surface_convert::face_region(b, q, &at, face_out, f, tol) else {
            return err(format!("a face of the sheet's body at depth {depth:.3} can't be read"));
        };
        // A face that is the panel's own outline with holes in it needs no boolean to keep to it.
        let mine = if inside_by_loops(std::slice::from_ref(&region), own, tol) {
            vec![region]
        } else {
            planar_regions_boolean(&[region], own, BoolOp::Intersect, tol)?
        };
        if mine.is_empty() {
            continue;
        }
        let toward = face_out.dot(frame.z) > 0.0;
        if depth.abs() <= tol && !toward {
            out.near.extend(mine);
        } else if (depth - t).abs() <= tol && toward {
            out.far.extend(mine);
        } else if !toward && depth > tol {
            let depth = drawn_depth(depth, frame.shift, plane.origin, frame.z, drawn, rounding(b));
            out.near_floors.extend(mine.into_iter().map(|r| (depth, r)));
        } else if toward && depth < t - tol {
            let depth = drawn_depth(t - depth, frame.shift + frame.z * t, plane.origin, frame.z, drawn, rounding(b));
            out.far_floors.extend(mine.into_iter().map(|r| (depth, r)));
        }
    }
    Ok(out)
}

/// How a wall of a cut lies to a flat panel, read exactly from its surface (see [`lean`]).
enum Lean {
    /// Square to the panel all over (a plane square to it, a cylinder whose axis runs through the
    /// thickness), or a floor parallel to it: the panel's faces show what it took. A wall of no
    /// analytic kind is read so too, as it was.
    Square,
    /// On a cylinder lying along the panel, its axis square to the fibers: a point of its axis,
    /// the axis, and its radius.
    Lying(Vec3, Vec3, f64),
    /// Leaning otherwise: a plane or a cylinder at a slant, a cone, a ball, a torus.
    Leaning,
}

/// How face `f` of `b` lies to a flat panel whose material lies toward `z` (see [`Lean`]): a
/// direction is along `z`, or square to it, within the body's tolerance across it, as
/// [`on_panel`] reads a face parallel to the panel.
fn lean(b: &Body, f: usize, z: Vec3, tol: f64) -> Lean {
    let size = b.size();
    let along = |v: Vec3| v.cross(z).len() * size <= tol;
    let across = |v: Vec3| v.dot(z).abs() * size <= tol;
    match face_surface(b, f) {
        Some(FaceSurface::Plane { normal, .. }) if along(normal.vec()) || across(normal.vec()) => Lean::Square,
        Some(FaceSurface::Cylinder { axis, .. }) if along(axis.vec()) => Lean::Square,
        Some(FaceSurface::Cylinder { origin, axis, radius }) if across(axis.vec()) => Lean::Lying(origin, axis.vec(), radius),
        Some(_) => Lean::Leaning,
        None => Lean::Square,
    }
}

/// What the cut takes through flat panel `name` (its frame `frame`, `t` thick) with each bore
/// lying in it, and the walls of `here` (faces of `now`, the cut's in the panel) it is read from.
/// A bore lying in the panel is a cylinder along it, ended by planes square to its axis. Its walls
/// run from face to face but lean into the fibers, so what it takes is its shadow, not what the
/// panel's faces show of it: the section of its material through the thickness is widest at its
/// axis, or, where its axis lies past a face, at that face, and every fiber through that widest
/// section meets the bore. So its shadow is a band as wide as that, along its axis between its
/// ends, in closed form. Refused, where walls lie on a cylinder along the panel: walls that don't
/// run through the sheet (a bore that reaches one face or none, a rounded face edge), as the
/// shadow refuses them; and a bore that reaches the panel's edge or any face but its own and the
/// panel's two, or with walls of other kinds or ends not square to its axis (not supported yet).
fn lying(
    now: &Body,
    q: &Query,
    frame: &Rigid3,
    t: f64,
    here: &[usize],
    name: &str,
    tol: f64,
) -> std::result::Result<(Vec<Region2>, BTreeSet<usize>), String> {
    let (mut bands, mut read) = (Vec::new(), BTreeSet::new());
    if !here.iter().any(|f| matches!(lean(now, *f, frame.z, tol), Lean::Lying(..))) {
        return Ok((bands, read));
    }
    let size = now.size();
    let parallel = |u: Vec3, v: Vec3| u.cross(v).len() * size <= tol;
    for group in components(q, here) {
        let Some((o, a, r)) = group.iter().find_map(|f| match lean(now, *f, frame.z, tol) {
            Lean::Lying(o, a, r) => Some((o, a, r)),
            _ => None,
        }) else {
            continue;
        };
        // Which of the panel's two faces it reaches, and whether it reaches any other face but
        // its own walls.
        let (mut near, mut far, mut stray) = (false, false, false);
        for &f in &group {
            let Some(face) = q.record().faces.get(f) else { continue };
            for l in &face.loops {
                let Bounds::Edges { edges } = l else { continue };
                for oe in edges {
                    for g in q.edge_faces(oe.edge as usize) {
                        if g == f || group.contains(&g) {
                            continue;
                        }
                        let depth = match face_surface(now, g) {
                            Some(FaceSurface::Plane { normal, point }) if parallel(normal.vec(), frame.z) => Some((point - frame.shift).dot(frame.z)),
                            _ => None,
                        };
                        match depth {
                            Some(d) if d.abs() <= tol => near = true,
                            Some(d) if (d - t).abs() <= tol => far = true,
                            _ => stray = true,
                        }
                    }
                }
            }
        }
        if !(near && far) {
            return Err(format!(
                "leaves walls that are not square to the sheet and don't run through it on {name}: not supported yet (a countersink, a rounded or chamfered face edge)"
            ));
        }
        if stray {
            return Err(format!("reaches the edge of {name} along a bore lying in it: not supported yet"));
        }
        let other = || format!("leaves a bore lying in {name} with walls other than its own and two ends square to its axis: not supported yet");
        // Its walls: the one cylinder, and the planes square to its axis that end it, where they
        // lie along it.
        let mut ends: Vec<f64> = Vec::new();
        for &f in &group {
            match face_surface(now, f) {
                Some(FaceSurface::Cylinder { origin, axis, radius }) => {
                    let off = origin - o;
                    if !(parallel(axis.vec(), a) && (radius - r).abs() <= tol && (off - a * off.dot(a)).len() <= tol) {
                        return Err(other());
                    }
                }
                Some(FaceSurface::Plane { normal, point }) if parallel(normal.vec(), a) => {
                    let s = (point - o).dot(a);
                    if !ends.iter().any(|e| (e - s).abs() <= tol) {
                        ends.push(s);
                    }
                }
                _ => return Err(other()),
            }
        }
        let (lo, hi) = match ends.as_slice() {
            [x, y] => (x.min(*y), x.max(*y)),
            _ => return Err(other()),
        };
        // Where its section through the thickness is widest: at its axis, or at the face nearer
        // it where the axis lies past that face. Half as wide as that, either side of the axis.
        let h = (o - frame.shift).dot(frame.z);
        let off = h.clamp(0.0, t) - h;
        let half = (r * r - off * off).max(0.0).sqrt();
        let c = o - frame.shift;
        let at = Vec2::new(c.dot(frame.x), c.dot(frame.y));
        let Some(way) = Vec2::new(a.dot(frame.x), a.dot(frame.y)).normalized() else { return Err(other()) };
        let side = way.perp() * half;
        let corners = [at + way * lo - side, at + way * hi - side, at + way * hi + side, at + way * lo + side];
        bands.push(Region2 { outer: Loop2::polygon(&corners).ccw(), holes: Vec::new() });
        read.extend(group);
    }
    Ok((bands, read))
}

/// The regions' union, their loops closed within `tol`. The regions are faces of a body, which
/// never overlap: the same region twice (a panel's two faces) is once, and regions apart from
/// each other (a piece inside another's hole) are their own union.
fn union(regions: Vec<Region2>, tol: f64) -> Result<Vec<Region2>> {
    let mut kept: Vec<Region2> = Vec::new();
    for r in regions {
        if !kept.iter().any(|k| same_regions(std::slice::from_ref(k), std::slice::from_ref(&r), tol)) {
            kept.push(r);
        }
    }
    if kept.len() <= 1 || apart(&kept, tol) {
        return Ok(kept);
    }
    let mut it = kept.into_iter();
    let Some(first) = it.next() else { return Ok(Vec::new()) };
    let rest: Vec<Region2> = it.collect();
    Ok(planar_regions_boolean(&[first], &rest, BoolOp::Union, tol)?)
}

/// Whether no two of `regions` come within `tol` of each other by their boxes (see [`loop_box`]):
/// each pair's outlines' boxes are apart, or one lies inside the box of a hole of the other made
/// of lines and arcs. A hole with any other curve has a box that may reach past it, which says
/// nothing of what lies inside the hole.
fn apart(regions: &[Region2], tol: f64) -> bool {
    let bounds = |l: &Loop2| loop_box(l).inflated(tol);
    let hole = |l: &Loop2| l.segs.iter().all(|s| matches!(s, Seg2::Line { .. } | Seg2::Arc { .. })).then(|| bounds(l));
    let boxes: Vec<(solidcraft_geom::Aabb2, Vec<solidcraft_geom::Aabb2>)> =
        regions.iter().map(|r| (bounds(&r.outer), r.holes.iter().filter_map(hole).collect())).collect();
    let clear =
        |a: &solidcraft_geom::Aabb2, b: &solidcraft_geom::Aabb2| a.max.x < b.min.x || b.max.x < a.min.x || a.max.y < b.min.y || b.max.y < a.min.y;
    let within =
        |a: &solidcraft_geom::Aabb2, b: &solidcraft_geom::Aabb2| b.min.x < a.min.x && a.max.x < b.max.x && b.min.y < a.min.y && a.max.y < b.max.y;
    boxes
        .iter()
        .enumerate()
        .all(|(i, (a, ah))| boxes.iter().skip(i + 1).all(|(b, bh)| clear(a, b) || bh.iter().any(|h| within(a, h)) || ah.iter().any(|h| within(b, h))))
}

fn area(regions: &[Region2]) -> f64 {
    regions.iter().map(Region2::area).sum()
}

fn perimeter(regions: &[Region2]) -> f64 {
    regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).flat_map(|l| &l.segs).map(|s| s.length()).sum()
}

/// Whether loops `a` and `b` are one loop, within `tol`: as many pieces, starting at the same
/// points, as long.
fn same_loop(a: &Loop2, b: &Loop2, tol: f64) -> bool {
    if a.segs.len() != b.segs.len() {
        return false;
    }
    let length = |l: &Loop2| l.segs.iter().map(|s| s.length()).sum::<f64>();
    (length(a) - length(b)).abs() <= tol * a.segs.len().max(1) as f64
        && a.segs.iter().all(|s| b.segs.iter().any(|t| t.start().dist(s.start()) <= tol || t.end().dist(s.start()) <= tol))
}

/// Whether regions `a` and `b` are the same regions, loop for loop.
fn same_regions(a: &[Region2], b: &[Region2], tol: f64) -> bool {
    let same = |x: &Region2, y: &Region2| {
        x.holes.len() == y.holes.len() && same_loop(&x.outer, &y.outer, tol) && x.holes.iter().all(|h| y.holes.iter().any(|k| same_loop(h, k, tol)))
    };
    a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| same(x, y)))
}

/// Whether the recesses `a` read from a panel are the ones `b` it has, to `tol`: as many, and each
/// from the same face, as deep, over the same region.
fn same_recesses(a: &[Recess], b: Vec<&Recess>, tol: f64) -> bool {
    a.len() == b.len()
        && a.iter().all(|x| {
            b.iter().any(|y| {
                x.far == y.far
                    && (x.depth - y.depth).abs() <= tol
                    && same_regions(std::slice::from_ref(&x.region), std::slice::from_ref(&y.region), tol)
            })
        })
}

/// Whether every region of `a` lies in a region of `b` by their loops (so `a` ∩ `b` is `a`): its
/// outline is that region's and it has each of that region's holes, or it lies in a new hole of
/// one that does (a piece a cut left inside it, as faces of one body never overlap).
fn inside_by_loops(a: &[Region2], b: &[Region2], tol: f64) -> bool {
    let holds = |x: &Region2, y: &Region2| same_loop(&x.outer, &y.outer, tol) && y.holes.iter().all(|k| x.holes.iter().any(|h| same_loop(h, k, tol)));
    let new_hole_at = |p: Vec2| {
        a.iter().any(|x| b.iter().any(|y| holds(x, y) && x.holes.iter().any(|h| self::holds(h, p) && !y.holes.iter().any(|k| same_loop(h, k, tol)))))
    };
    a.iter().all(|x| b.iter().any(|y| holds(x, y)) || x.outer.segs.first().is_some_and(|s| new_hole_at(s.start())))
}

/// What `footprint` loses to `near` (`footprint` − `near`), read off their loops where `near` is
/// `footprint` with holes cut in it and pieces left inside them: each new hole is a region gone,
/// less the pieces inside it. None where the loops don't say so (a cut through an outline).
fn gone_by_loops(footprint: &[Region2], near: &[Region2], tol: f64) -> Option<Vec<Region2>> {
    let mut gone: Vec<Region2> = Vec::new();
    let mut islands: Vec<&Region2> = Vec::new();
    let mut matched = vec![false; footprint.len()];
    for x in near {
        let Some(k) = footprint.iter().position(|y| same_loop(&x.outer, &y.outer, tol)) else {
            islands.push(x);
            continue;
        };
        let y = footprint.get(k)?;
        if !y.holes.iter().all(|k| x.holes.iter().any(|h| same_loop(h, k, tol))) {
            return None;
        }
        matched[k] = true;
        gone.extend(x.holes.iter().filter(|h| !y.holes.iter().any(|k| same_loop(h, k, tol))).map(|h| Region2 { outer: h.ccw(), holes: Vec::new() }));
    }
    if !matched.iter().all(|m| *m) {
        return None;
    }
    // Each piece left inside a new hole is a hole of what is gone there.
    for island in islands {
        let at = island.outer.segs.first()?.start();
        let g = gone.iter_mut().find(|g| holds(&g.outer, at))?;
        g.holes.push(island.outer.ccw().reversed());
    }
    Some(gone)
}

/// Whether region `g` reaches within `near` of the outline of any of `of`.
fn touches(g: &Region2, of: &[Region2], near: f64) -> bool {
    let pts = g.outer.polyline(near.max(1e-6));
    of.iter().flat_map(|r| &r.outer.segs).any(|s| (&pts).into_iter().any(|p| s.point_at(s.closest_param(*p).clamp(0.0, 1.0)).dist(*p) <= near))
}

/// The edges the rips give panel `p` of the development `dev`, in the panel's chart, each with its
/// rip's joint: where a rip cut the panel back from its own edge along it (see
/// [`EdgeFrom::Rip`]), as a corner closed with a gap has it. None for a panel no rip cuts.
fn rip_edges(dev: &Development, p: usize) -> Vec<(usize, Seg2)> {
    let (Some(Some(laid)), Some(Some(at))) = (dev.panels.get(p), dev.placements.get(p)) else { return Vec::new() };
    let back = at.inverse();
    std::iter::once(&laid.region.outer)
        .chain(&laid.region.holes)
        .zip(&laid.origins)
        .flat_map(|(l, from)| l.segs.iter().zip(from))
        .filter_map(|(s, from)| match from {
            EdgeFrom::Rip(j) => Some((*j, back.seg(s))),
            EdgeFrom::Own(_) => None,
        })
        .collect()
}

/// `g`, a region a cut took through panel `p` of `sk` (in its chart), run on across the lap of
/// each rip whose new edge it reaches (`rips`, see [`rip_edges`]) to the rip's line: the band's
/// rectangle over the stretch where `g` reaches the band's inner line, from that line out to the
/// rip's (see [`crate::sheet_surface_features::reaches`]), joined to `g` by the planar booleans.
/// The lap is never material, so this takes no more of the panel than `g` does; it puts the cut
/// in the panel's outline as drawn. Where `g` only touches the edge, at a point, nothing runs on
/// (it stays a hole of the panel). Refused where the rip's edge on the panel isn't one straight
/// edge cut alike all along (see [`crate::sheet_surface_features::lap_band`]).
fn across_laps(sk: &SurfaceSheet, p: usize, g: Region2, rips: &[(usize, Seg2)], tol: f64) -> std::result::Result<Region2, String> {
    let t = sk.sheet.thickness;
    let mut laps: Vec<Region2> = Vec::new();
    for (j, joint) in sk.sheet.joints.iter().enumerate() {
        let JointKind::Rip(rip) = &joint.kind else { continue };
        let edges: Vec<Seg2> = rips.iter().filter(|(k, _)| *k == j).map(|(_, s)| *s).collect();
        if edges.is_empty() || !reaches(&g, &edges, tol)? {
            continue;
        }
        for (panel, on_a) in [(joint.a.panel, true), (joint.b.panel, false)] {
            if panel != p {
                continue;
            }
            let Some(band) = crate::sheet_surface_features::lap_band(rip, on_a, t) else {
                let name = sk.sheet.panels.get(p).map_or("a panel", |x| x.name.as_str());
                return Err(format!("reaches the edge a rip left on {name}, which isn't one straight edge cut alike all along: not supported yet"));
            };
            for (lo, hi) in crate::sheet_surface_features::reaches(&g, &band) {
                let corners = [band.start + band.way * lo, band.start + band.way * hi, band.inner(hi), band.inner(lo)];
                laps.push(Region2 { outer: Loop2::polygon(&corners).ccw(), holes: Vec::new() });
            }
        }
    }
    if laps.is_empty() {
        return Ok(g);
    }
    let joined = planar_regions_boolean(std::slice::from_ref(&g), &laps, BoolOp::Union, tol).map_err(|e| e.to_string())?;
    match <[Region2; 1]>::try_from(joined) {
        Ok([one]) => Ok(one),
        Err(_) => Err("can't be kept in the sheet yet: run on across a rip's lap, it doesn't join up".into()),
    }
}

/// Whether region `g` reaches any of `edges`: its outline comes within `near` of one (crosses it,
/// runs along it or touches it, if only at a point), or it holds one. Points within `near` are
/// one point, as the body reads them; nothing else is close. Read exactly for each kind of curve
/// (see [`near_each_other`]), never from points along them; refused for a pair of curves no exact
/// form is known for.
fn reaches(g: &Region2, edges: &[Seg2], near: f64) -> std::result::Result<bool, String> {
    for e in edges {
        for s in &g.outer.segs {
            if near_each_other(e, s, near)? {
                return Ok(true);
            }
        }
    }
    // Clear of the outline, an edge lies all inside it or all outside it.
    Ok(edges.iter().any(|e| holds(&g.outer, e.start())))
}

/// What kind of curve `s` is, for a refusal.
fn kind(s: &Seg2) -> String {
    match s {
        Seg2::Line { .. } => "line".into(),
        Seg2::Arc { .. } => "arc".into(),
        Seg2::Cubic { .. } => "cubic".into(),
        Seg2::Conic { .. } => "conic".into(),
        Seg2::Bezier { n, .. } => format!("curve of degree {n}"),
    }
}

/// The points of segment `s` that reach furthest along each of `ways` and against it: a line's
/// ends; an arc's ends and each point of its circle furthest along a way or against it that lies
/// within its sweep (see [`on_arc`]); a Bézier's or a conic's control points, which hold the curve
/// (a conic's weight is above 0) but may lie off it. Read along any of `ways`, the least and the
/// greatest of them are the segment's own for a line and an arc, and reach at least as far as the
/// segment does for any other curve.
pub(crate) fn extremes(s: &Seg2, ways: &[Vec2]) -> Vec<Vec2> {
    match *s {
        Seg2::Line { a, b } => vec![a, b],
        Seg2::Arc { center, radius, start, sweep } => {
            let mut pts = vec![s.start(), s.end()];
            // Any way of some length, however small: along one of none, the ends answer.
            for w in ways.iter().filter(|w| w.len() > 0.0).map(|w| *w / w.len()) {
                pts.extend([w, -w].into_iter().filter(|d| on_arc(*d, start, sweep)).map(|d| center + d * radius));
            }
            pts
        }
        Seg2::Cubic { p0, p1, p2, p3 } => vec![p0, p1, p2, p3],
        Seg2::Conic { a, apex, b, .. } => vec![a, apex, b],
        Seg2::Bezier { n, p } => p.iter().take(usize::from(n) + 1).copied().collect(),
    }
}

/// A box round `s` that holds it: a line's and an arc's own, exactly (an arc's ends and each of
/// its circle's four points center ± radius along x and along y that lies within its sweep, see
/// [`extremes`]); for a cubic, a conic or a Bézier the box of its control points, a sure box that
/// holds the curve and may reach past it.
pub(crate) fn seg_box(s: &Seg2) -> (Vec2, Vec2) {
    let pts = extremes(s, &[Vec2::X, Vec2::Y]);
    let lo = pts.iter().fold(Vec2::new(f64::INFINITY, f64::INFINITY), |m, q| Vec2::new(m.x.min(q.x), m.y.min(q.y)));
    let hi = pts.iter().fold(Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |m, q| Vec2::new(m.x.max(q.x), m.y.max(q.y)));
    (lo, hi)
}

/// The box of loop `l`, its segments' boxes joined (see [`seg_box`]): the loop's own where it is
/// made of lines and arcs, and a sure box that holds it where it has any other curve. Empty for a
/// loop of no segments.
pub(crate) fn loop_box(l: &Loop2) -> solidcraft_geom::Aabb2 {
    l.segs.iter().map(seg_box).fold(solidcraft_geom::Aabb2::EMPTY, |b, (min, max)| b.union(&solidcraft_geom::Aabb2 { min, max }))
}

/// The box of loop `l` carried into space by `to`, an affine map of its plane (a plane's or a
/// frame's placing, say): each segment's points furthest along each axis of space and against it
/// (see [`extremes`]), carried. The carried loop's own box where it is made of lines and arcs; a
/// sure box where it has any other curve, whose control points an affine map carries onto the
/// carried curve's.
pub(crate) fn loop_box_in(l: &Loop2, to: impl Fn(Vec2) -> Vec3) -> solidcraft_geom::Aabb3 {
    let o = to(Vec2::ZERO);
    let (x, y) = (to(Vec2::X) - o, to(Vec2::Y) - o);
    // Along each axis of space a carried point moves its x times that axis's share of `x`, and
    // its y times its share of `y`.
    let ways = [Vec2::new(x.x, y.x), Vec2::new(x.y, y.y), Vec2::new(x.z, y.z)];
    let mut b = solidcraft_geom::Aabb3::EMPTY;
    for p in l.segs.iter().flat_map(|s| extremes(s, &ways)) {
        b.add(to(p));
    }
    b
}

/// Whether segments `x` and `y` come within `near` of each other, exactly: a line with any curve
/// (see [`line_near`]), two arcs (see [`arcs_near`]). Two curves of other kinds whose boxes meet
/// are refused: no exact form is known for them here.
pub(crate) fn near_each_other(x: &Seg2, y: &Seg2, near: f64) -> std::result::Result<bool, String> {
    let ((xl, xh), (yl, yh)) = (seg_box(x), seg_box(y));
    if xl.x - yh.x > near || yl.x - xh.x > near || xl.y - yh.y > near || yl.y - xh.y > near {
        return Ok(false);
    }
    match (x, y) {
        (Seg2::Line { a, b }, other) | (other, Seg2::Line { a, b }) => Ok(line_near(*a, *b, other, near)),
        (Seg2::Arc { .. }, Seg2::Arc { .. }) => Ok(arcs_near(x, y, near)),
        _ => Err(format!("reaches the {} edge a rip left along a {} of its own: not supported yet", kind(x), kind(y))),
    }
}

/// How far point `p` lies from the segment from `a` to `b`.
pub(crate) fn to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let t = if d.dot(d) > 0.0 { ((p - a).dot(d) / d.dot(d)).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + d * t)
}

/// Whether the way `v` from an arc's center lies within the arc, from angle `start` round
/// `sweep` (counter-clockwise where above 0).
pub(crate) fn on_arc(v: Vec2, start: f64, sweep: f64) -> bool {
    sweep.abs() >= TAU || ((v.y.atan2(v.x) - start) * sweep.signum()).rem_euclid(TAU) <= sweep.abs()
}

/// How far point `p` lies from the arc round `center`, `radius` from it, from `start` round
/// `sweep`, which ends at `ends`: square to it where the way to `p` lies within it, else from an
/// end.
pub(crate) fn to_arc(p: Vec2, (center, radius, start, sweep): (Vec2, f64, f64, f64), ends: [Vec2; 2]) -> f64 {
    let v = p - center;
    let square = if v.len() > 0.0 && on_arc(v, start, sweep) { (v.len() - radius).abs() } else { f64::INFINITY };
    square.min(p.dist(ends[0])).min(p.dist(ends[1]))
}

/// Whether segment `s` comes within `near` of the segment from `a` to `b`: where it crosses the
/// line within the segment, or where it comes nearest the line, or the segment, or the line's
/// ends, at their ends. A line in closed form, an arc by its circle's center, any other curve by
/// the roots of polynomials in its own parameter (see [`curve_line_near`]).
fn line_near(a: Vec2, b: Vec2, s: &Seg2, near: f64) -> bool {
    match *s {
        Seg2::Line { a: c, b: d } => {
            let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
            let (s1, s2, s3, s4) = (side(a, b, c), side(a, b, d), side(c, d, a), side(c, d, b));
            let apart = |u: f64, v: f64| (u > 0.0 && v < 0.0) || (u < 0.0 && v > 0.0);
            // Crossing, or one's end within `near` of the other (which takes in lines that
            // overlap along one line).
            (apart(s1, s2) && apart(s3, s4))
                || to_segment(c, a, b) <= near
                || to_segment(d, a, b) <= near
                || to_segment(a, c, d) <= near
                || to_segment(b, c, d) <= near
        }
        Seg2::Arc { center, radius, start, sweep } => {
            let arc = (center, radius, start, sweep);
            let ends = [s.start(), s.end()];
            if ends.iter().any(|e| to_segment(*e, a, b) <= near) || [a, b].iter().any(|p| to_arc(*p, arc, ends) <= near) {
                return true;
            }
            let Some(u) = (b - a).normalized() else { return false };
            let (n, length) = (u.perp(), (b - a).len());
            let (along, across) = ((center - a).dot(u), (center - a).dot(n));
            if across.abs() >= radius {
                // Clear of the line: the circle's point nearest it, square to it from the center.
                return across.abs() - radius <= near && (0.0..=length).contains(&along) && on_arc(n * -across.signum(), start, sweep);
            }
            // The line cuts the circle: where.
            let half = (radius * radius - across * across).sqrt();
            [along - half, along + half].iter().any(|f| (0.0..=length).contains(f) && on_arc(a + u * *f - center, start, sweep))
        }
        _ => rational(s).is_some_and(|curve| curve_line_near(a, b, s, &curve, near)),
    }
}

/// Whether arcs `x` and `y` come within `near` of each other: where their circles cross, within
/// both; where they come nearest, on the line through their centers; or at their ends.
fn arcs_near(x: &Seg2, y: &Seg2, near: f64) -> bool {
    let (Seg2::Arc { center: c1, radius: r1, start: s1, sweep: w1 }, Seg2::Arc { center: c2, radius: r2, start: s2, sweep: w2 }) = (*x, *y) else {
        return false;
    };
    let (e1, e2) = ([x.start(), x.end()], [y.start(), y.end()]);
    if e1.iter().any(|p| to_arc(*p, (c2, r2, s2, w2), e2) <= near) || e2.iter().any(|p| to_arc(*p, (c1, r1, s1, w1), e1) <= near) {
        return true;
    }
    // About one center, every point of either is as far from the other: their ends answer.
    let Some(u) = (c2 - c1).normalized() else { return false };
    let d = (c2 - c1).len();
    if d <= r1 + r2 && d >= (r1 - r2).abs() {
        let along = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
        let half = (r1 * r1 - along * along).max(0.0).sqrt();
        for side in [-1.0, 1.0] {
            let p = c1 + u * along + u.perp() * (half * side);
            if on_arc(p - c1, s1, w1) && on_arc(p - c2, s2, w2) {
                return true;
            }
        }
    }
    [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)]
        .iter()
        .any(|(k1, k2)| (c1 + u * (r1 * k1)).dist(c2 + u * (r2 * k2)) <= near && on_arc(u * *k1, s1, w1) && on_arc(u * *k2, s2, w2))
}

/// A polynomial's coefficients, of 1, t, t², ….
type Poly = Vec<f64>;

fn eval(c: &[f64], t: f64) -> f64 {
    c.iter().rev().fold(0.0, |acc, k| acc * t + k)
}

/// `a` + `k` `b`.
fn plus(a: &[f64], b: &[f64], k: f64) -> Poly {
    (0..a.len().max(b.len())).map(|i| a.get(i).copied().unwrap_or(0.0) + k * b.get(i).copied().unwrap_or(0.0)).collect()
}

fn times(a: &[f64], b: &[f64]) -> Poly {
    let mut out = vec![0.0; (a.len() + b.len()).saturating_sub(1)];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            if let Some(o) = out.get_mut(i + j) {
                *o += x * y;
            }
        }
    }
    out
}

fn derivative(c: &[f64]) -> Poly {
    c.iter().enumerate().skip(1).map(|(i, k)| k * i as f64).collect()
}

/// The polynomial in t of a Bézier's coordinate, from its control values `ctrl`.
fn power_basis(ctrl: &[f64]) -> Poly {
    let choose = |n: usize, k: usize| (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64);
    let n = ctrl.len().saturating_sub(1);
    (0..=n)
        .map(|k| {
            let sign = |i: usize| if (k - i).is_multiple_of(2) { 1.0 } else { -1.0 };
            let sum: f64 = (0..=k).map(|i| sign(i) * choose(k, i) * ctrl.get(i).copied().unwrap_or(0.0)).sum();
            choose(n, k) * sum
        })
        .collect()
}

/// A Bézier or a conic as polynomials in its own parameter: its point is (x, y) / w, w being 1
/// but for a conic. None for an arc.
fn rational(s: &Seg2) -> Option<[Poly; 3]> {
    let (pts, weights): (Vec<Vec2>, Vec<f64>) = match *s {
        Seg2::Line { a, b } => (vec![a, b], vec![1.0; 2]),
        Seg2::Cubic { p0, p1, p2, p3 } => (vec![p0, p1, p2, p3], vec![1.0; 4]),
        Seg2::Conic { a, apex, b, w } => (vec![a, apex, b], vec![1.0, w, 1.0]),
        Seg2::Bezier { n, p } => {
            let pts: Vec<Vec2> = p.iter().take(usize::from(n) + 1).copied().collect();
            let k = pts.len();
            (pts, vec![1.0; k])
        }
        Seg2::Arc { .. } => return None,
    };
    let weighted = |f: fn(&Vec2) -> f64| power_basis(&pts.iter().zip(&weights).map(|(p, w)| f(p) * w).collect::<Vec<f64>>());
    Some([weighted(|p| p.x), weighted(|p| p.y), power_basis(&weights)])
}

/// The point of `curve` (see [`rational`]) at `t`, its ends exactly those of `s`.
fn curve_at(s: &Seg2, [x, y, w]: &[Poly; 3], t: f64) -> Vec2 {
    if t <= 0.0 {
        return s.start();
    }
    if t >= 1.0 {
        return s.end();
    }
    let k = eval(w, t);
    Vec2::new(eval(x, t) / k, eval(y, t) / k)
}

/// The real roots in [0, 1] of the polynomial `c`, each to the last bit: between the roots of its
/// derivative it only rises or only falls, so each stretch holds one root at most, found by
/// halving it where its ends differ in sign. A root where it touches 0 without crossing is a root
/// of its derivative, which callers read too.
fn roots01(c: &[f64]) -> Vec<f64> {
    let n = c.iter().rposition(|k| *k != 0.0).map_or(0, |i| i + 1);
    let c = c.get(..n).unwrap_or_default();
    match c {
        [] | [_] => Vec::new(),
        [c0, c1] => {
            let t = -c0 / c1;
            if (0.0..=1.0).contains(&t) { vec![t] } else { Vec::new() }
        }
        _ => {
            let mut stops = vec![0.0];
            stops.extend(roots01(&derivative(c)));
            stops.push(1.0);
            stops.sort_by(f64::total_cmp);
            let mut out = Vec::new();
            for w in stops.windows(2) {
                let &[mut lo, mut hi] = w else { continue };
                let (flo, fhi) = (eval(c, lo), eval(c, hi));
                if flo == 0.0 || fhi == 0.0 {
                    out.push(if flo == 0.0 { lo } else { hi });
                    continue;
                }
                if (flo < 0.0) == (fhi < 0.0) {
                    continue;
                }
                loop {
                    let mid = 0.5 * (lo + hi);
                    if mid <= lo || mid >= hi {
                        break;
                    }
                    let f = eval(c, mid);
                    if f == 0.0 {
                        (lo, hi) = (mid, mid);
                        break;
                    }
                    if (f < 0.0) == (flo < 0.0) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                out.push(0.5 * (lo + hi));
            }
            out
        }
    }
}

/// Whether the Bézier or conic `s`, as `curve` (see [`rational`]), comes within `near` of the
/// segment from `a` to `b`: at its ends; where it comes nearest either end of the segment (the
/// roots of its distance's derivative); where it crosses the segment's line within the segment,
/// or comes nearest the line there (the roots of its signed distance from the line, and of that
/// distance's derivative): every polynomial in the curve's own parameter.
fn curve_line_near(a: Vec2, b: Vec2, s: &Seg2, curve: &[Poly; 3], near: f64) -> bool {
    let [x, y, w] = curve;
    let at = |t: f64| curve_at(s, curve, t);
    if [0.0, 1.0].iter().any(|t| to_segment(at(*t), a, b) <= near) {
        return true;
    }
    for q in [a, b] {
        // |p − q|² is |m|² / w², m = (x − q.x w, y − q.y w): its derivative is 0 where
        // (m · m′) w − |m|² w′ is.
        let (mx, my) = (plus(x, w, -q.x), plus(y, w, -q.y));
        let dot = plus(&times(&mx, &derivative(&mx)), &times(&my, &derivative(&my)), 1.0);
        let square = plus(&times(&mx, &mx), &times(&my, &my), 1.0);
        let turns = plus(&times(&dot, w), &times(&square, &derivative(w)), -1.0);
        if roots01(&turns).into_iter().any(|t| at(t).dist(q) <= near) {
            return true;
        }
    }
    let Some(u) = (b - a).normalized() else { return false };
    let (n, length) = (u.perp(), (b - a).len());
    // The signed distance from the line is h / w, h = (x − a.x w) n.x + (y − a.y w) n.y; its
    // derivative is 0 where h′ w − h w′ is.
    let h = plus(&plus(x, w, -a.x).iter().map(|k| k * n.x).collect::<Poly>(), &plus(y, w, -a.y), n.y);
    let turns = plus(&times(&derivative(&h), w), &times(&h, &derivative(w)), -1.0);
    roots01(&h).into_iter().chain(roots01(&turns)).any(|t| {
        let p = at(t);
        (0.0..=length).contains(&(p - a).dot(u)) && (p - a).dot(n).abs() <= near
    })
}

/// Whether point `p` lies inside loop `l`, exactly: a ray from it toward +x crosses the loop an
/// odd number of times. Each segment is taken in pieces along which y only rises or only falls,
/// each holding its lower end and not its upper one, so that a ray through a corner counts once.
/// A corner is one point of the loop, read once: each segment ends where the next one starts (as
/// a loop's segments do), never at its own end reckoned again. An arc's end is reckoned from its
/// angle, and lies a rounding off the start it meets where the two angles differ by a turn (a
/// whole circle's end, at 2π, and its start, at 0): read twice, a ray level with that corner
/// would cross it on one side and not on the other.
pub(crate) fn holds(l: &Loop2, p: Vec2) -> bool {
    let ends = l.segs.iter().skip(1).chain(l.segs.first()).map(Seg2::start);
    l.segs.iter().zip(ends).map(|(s, end)| ray_crossings(s, end, p)).sum::<usize>() % 2 == 1
}

/// Whether point `p` lies in region `r`'s material, exactly: inside its outline and inside none
/// of its holes (see [`holds`]). A point on a loop of it lies inside or outside as the ray from
/// it reads that loop.
pub(crate) fn region_holds(r: &Region2, p: Vec2) -> bool {
    holds(&r.outer, p) && !r.holes.iter().any(|h| holds(h, p))
}

/// How many times a ray from `p` toward +x crosses segment `s`, which ends at `end`, the start of
/// the next segment of its loop (see [`holds`]).
fn ray_crossings(s: &Seg2, end: Vec2, p: Vec2) -> usize {
    let counts = |y0: f64, y1: f64| (y0 > p.y) != (y1 > p.y);
    match *s {
        Seg2::Line { a, .. } => usize::from(counts(a.y, end.y) && a.x + (p.y - a.y) * (end.x - a.x) / (end.y - a.y) > p.x),
        Seg2::Arc { center, radius, start, sweep } => {
            // Pieces between the arc's ends and its circle's top and bottom.
            let (lo, hi) = (start.min(start + sweep), start.max(start + sweep));
            let mut stops = vec![0.0];
            let mut turn = FRAC_PI_2 + PI * ((lo - FRAC_PI_2) / PI).ceil();
            // A top or a bottom each half turn on. An arc whose angles are not finite has none
            // to stop at, nor one whose angles are so large that a half turn no longer moves
            // them: past either, the loop would never end.
            while hi.is_finite() && turn < hi && turn + PI > turn {
                if turn > lo {
                    stops.push((turn - start) / sweep);
                }
                turn += PI;
            }
            stops.push(1.0);
            stops.sort_by(f64::total_cmp);
            let y_at = |t: f64| match t {
                t if t <= 0.0 => s.start().y,
                t if t >= 1.0 => end.y,
                t => center.y + radius * (start + sweep * t).sin(),
            };
            stops
                .windows(2)
                .filter(|w| {
                    let &[t0, t1] = *w else { return false };
                    // Along a piece the way across from the center keeps its side: x is on it.
                    let side = (start + sweep * 0.5 * (t0 + t1)).cos().signum();
                    let x = center.x + side * (radius * radius - (p.y - center.y).powi(2)).max(0.0).sqrt();
                    counts(y_at(t0), y_at(t1)) && x > p.x
                })
                .count()
        }
        _ => {
            let Some(curve) = rational(s) else { return 0 };
            let [x, y, w] = &curve;
            // Pieces between the curve's ends and where y turns: (y / w)′ is 0 where y′ w − y w′ is.
            let mut stops = vec![0.0];
            stops.extend(roots01(&plus(&times(&derivative(y), w), &times(y, &derivative(w)), -1.0)));
            stops.push(1.0);
            stops.sort_by(f64::total_cmp);
            let level = plus(y, w, -p.y);
            let at = |t: f64| if t >= 1.0 { end } else { curve_at(s, &curve, t) };
            stops
                .windows(2)
                .filter(|win| {
                    let &[t0, t1] = *win else { return false };
                    let (y0, y1) = (at(t0).y, at(t1).y);
                    if !counts(y0, y1) {
                        return false;
                    }
                    // y only rises or only falls along the piece: one root of y − p.y w in it, at
                    // its lower end where it holds that.
                    if eval(&level, t0) == 0.0 {
                        return at(t0).x > p.x;
                    }
                    let (mut lo, mut hi) = (t0, t1);
                    let below = eval(&level, lo) < 0.0;
                    loop {
                        let mid = 0.5 * (lo + hi);
                        if mid <= lo || mid >= hi {
                            break;
                        }
                        if (eval(&level, mid) < 0.0) == below {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    let t = 0.5 * (lo + hi);
                    eval(x, t) / eval(w, t) > p.x
                })
                .count()
        }
    }
}

/// A panel's frame in its body: where the fold puts it, or in the flat, where the development
/// lays its chart in the sheet's flat (see [`SurfaceSheet::flat`]).
pub(crate) fn frames(sk: &SurfaceSheet, dev: &Development) -> Result<Vec<Option<Rigid3>>> {
    if !sk.unfolded {
        // A panel with a place of its own (the face it was made from) is read where that face is:
        // folding reaches it only to within the body's tolerance (see `fold_onto`).
        let folded = crate::sheet_surface_convert::fold_onto(&sk.sheet, dev, &sk.places)?.places;
        return Ok(folded.into_iter().enumerate().map(|(i, got)| sk.places.get(i).copied().flatten().or(got)).collect());
    }
    Ok(dev.placements.iter().map(|p| p.map(|r| lift(&sk.flat, &r))).collect())
}

/// The frame `r` puts a chart in, within the plane of `base`.
pub(crate) fn lift(base: &Rigid3, r: &Rigid2) -> Rigid3 {
    let (x, y) = (base.x * r.cos + base.y * r.sin, base.y * r.cos - base.x * r.sin);
    Rigid3 { x, y, z: base.z, shift: base.shift + base.x * r.shift.x + base.y * r.shift.y }
}

/// What feature `f` cut from each sheet made from surfaces whose body it changed, read back onto
/// the sheet (see the module docs): the sheet's index, the sheet, and its body built again.
/// Features that are sheet metal build their sheets themselves, and moves leave a sheet as it is.
/// Checking each step (`weigh`), each body built again is weighed against the body cut.
/// What [`absorb`] made of a sheet: the sheet read with its body built again, or why the sheet
/// can't hold the change.
pub(crate) enum Absorbed {
    Read(Box<SurfaceSheet>, Body),
    Refused(String),
}

pub(crate) fn absorb(before: &ModelState, after: &ModelState, f: &Feature, weigh: bool, drawn: Option<&Drawn>) -> Result<Vec<(usize, Absorbed)>> {
    if f.kind.is_sheet()
        || matches!(
            f.kind,
            FeatureKind::Move { .. }
                | FeatureKind::Align { .. }
                | FeatureKind::Rest { .. }
                | FeatureKind::BodyTransfer { .. }
                | FeatureKind::SplitFace { .. }
                | FeatureKind::Thread { modeled: false, .. }
        )
    {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for i in 0..after.surface_sheets.len() {
        let Some(sk) = after.surface_sheets.get(i).cloned() else { continue };
        // A sheet the feature changed itself (a cut read straight into it) has its body already.
        if before.surface_sheets.iter().any(|b| b.body == sk.body && (b.sheet != sk.sheet || b.loose != sk.loose || b.bores != sk.bores)) {
            continue;
        }
        let (Some(was), Some(now)) = (before.body(&sk.body), after.body(&sk.body)) else { continue };
        if was.body.same_shape(&now.body) {
            continue;
        }
        let (was, now) = (was.body.clone(), now.body.clone());
        // A sheet's body that keeps a change the sheet can't hold has drifted from the sheet:
        // what later features change of it stays on it.
        if sk.altered_by.is_some() {
            continue;
        }
        match cut(&sk, &was, &now, weigh, drawn) {
            Ok((cut, built)) => out.push((i, Absorbed::Read(Box::new(cut), built))),
            Err(e) => out.push((i, Absorbed::Refused(format!("`{}`: what {} cut {e}", sk.body, f.name)))),
        }
    }
    Ok(out)
}

/// The sheet `sk` with what was cut from its body, `was` before and `now` after, read onto its
/// flat panels, and its body built again. Every face the cut made must lie in a panel or a piece
/// of the flat it was read in. Weighed (`weigh`), the body built again must also weigh what `now`
/// does: a check of the reading itself, which costs more than the rest of it. The extrude `drawn`
/// that made the cut, if one did, gives its pockets their depths.
pub(crate) fn cut(
    sk: &SurfaceSheet,
    was: &Body,
    now: &Body,
    weigh: bool,
    drawn: Option<&Drawn>,
) -> std::result::Result<(SurfaceSheet, Body), String> {
    let walls = made(was, now, tolerance(was));
    let (mut out, read) = onto_panels(sk, was, now, Some(&walls), drawn)?;
    // Nothing else may have been cut: a bend's material, or anything the panels' faces don't show.
    if walls.iter().any(|f| !read.claimed.contains(f)) {
        return Err("can't be kept in the sheet yet: it reaches a bend or a seam".into());
    }
    let built = out.build().map_err(|e| e.to_string())?;
    if weigh {
        weighed(sk, &out, &built, now, read.shadowed)?;
    }
    Ok((out, built))
}

/// Whether `built`, the sheet `out` that the cut of `sk` to `now` was read into, weighs what `now`
/// does. A shadow takes more than the cut did, and all the cut made lies in panels read.
fn weighed(sk: &SurfaceSheet, out: &SurfaceSheet, built: &Body, now: &Body, shadowed: bool) -> std::result::Result<(), String> {
    let weigh = |b: &Body| solidcraft_kernel::measure_mass(b).map(|m| m.volume).map_err(|e| e.to_string());
    let (v, w) = (weigh(built)?, weigh(now)?);
    // What the cut's new tabs weigh more flat than they did bent. Unfolded, a tab lay flat
    // before the cut as it does after it.
    let (t, k) = (sk.sheet.thickness, sk.sheet.k_factor);
    let tabs = |b: &crate::SurfaceBendLine| b.tabs.iter().map(Region2::area).sum::<f64>();
    let flat_more: f64 = if sk.unfolded {
        0.0
    } else {
        out.bends
            .iter()
            .enumerate()
            .map(|(j, b)| (tabs(b) - sk.bends.get(j).map_or(0.0, tabs)) * (t - t * (b.radius + t / 2.0) / (b.radius + k * t)))
            .sum()
    };
    let v = v - flat_more;
    if shadowed {
        if v > w + 1e-6 * w.abs().max(1.0) {
            return Err(format!("can't be kept in the sheet yet: its shadow keeps {:.4} mm³ the cut took", v - w));
        }
    } else if (v - w).abs() > 1e-6 * w.abs().max(1.0) {
        return Err(format!("can't be kept in the sheet yet: it reaches a bend or a seam ({:.4} mm³ the sheet's panels don't show)", (v - w).abs()));
    }
    Ok(())
}

/// A converted sheet `sk`, built as `built`, with the recesses of the faces it was converted
/// from (of `body`) read onto its flat panels, and its body built again with them. Where the
/// body's two faces of a panel end apart at its outline (as a bent part drawn with sharp
/// corners has them), that is how it was drawn, not a cut.
pub(crate) fn engraved(sk: &SurfaceSheet, built: &Body, body: &Body) -> Result<(SurfaceSheet, Body)> {
    let (mut out, _) = onto_panels(sk, built, body, None, None).map_err(DocError::Invalid)?;
    if out.recesses.is_empty() && out.sheet == sk.sheet {
        return Ok((out, built.clone()));
    }
    let solid = out.build()?;
    Ok((out, solid))
}

/// How near two points of `b` are one: as near as the body's exact edges close its faces' loops.
fn tolerance(b: &Body) -> f64 {
    (b.size() * 1e-7).clamp(1e-7, 1e-3)
}

/// How far apart two workings of one length from the coordinates of body `b` can come out: 64
/// units in the last place of its largest coordinate and size. A folded panel's frame is where
/// its fold puts it, to a rounding, so a length measured in it differs by that much from the same
/// length worked out from the numbers a feature was given; two lengths any farther apart are
/// different lengths (far below the extrusion resolution, 1e-6 mm).
fn rounding(b: &Body) -> f64 {
    let o = b.outline_bounds();
    let far = [o.min.x, o.min.y, o.min.z, o.max.x, o.max.y, o.max.z].into_iter().map(f64::abs).fold(0.0, f64::max);
    64.0 * f64::EPSILON * (if far.is_finite() { far } else { 0.0 } + b.size())
}

/// A plane: the slab of a sheet laid out flat.
static FLAT: Surface = Surface::Plane;

/// The flat's `pieces` (of the development `dev` of the sheet `sk`) as `fold` carries them into
/// space, or, unfolded (no `fold`), as they lie in the sheet's flat (see [`SurfaceSheet::flat`]).
fn carriers<'a>(
    sk: &'a SurfaceSheet,
    dev: &Development,
    fold: Option<&'a Fold>,
    pieces: &[(Piece, Region2)],
) -> std::result::Result<Vec<(Carrier<'a>, Region2)>, String> {
    let t = sk.sheet.thickness;
    let lost = || "a piece of the sheet has no place".to_string();
    pieces
        .iter()
        .map(|(piece, region)| {
            let carrier = match (fold, *piece) {
                (None, _) => Carrier::Flat(Slab::new(&FLAT, sk.flat, t)),
                (Some(f), Piece::Panel(i) | Piece::Tab(_, i)) => {
                    let place = f.places.get(i).copied().flatten().ok_or_else(lost)?;
                    Carrier::Panel(i, Slab::new(&sk.sheet.panels.get(i).ok_or_else(lost)?.surface, place, t))
                }
                (Some(f), Piece::Zone(z)) => {
                    Carrier::Bend(dev.zones.get(z).and_then(|zone| f.bends.get(zone.joint)).and_then(Option::as_ref).ok_or_else(lost)?)
                }
            };
            Ok((carrier, region.clone()))
        })
        .collect()
}

/// The names of the panels and bends of `pieces` that `reach` takes: "Plate, bend 1 and Flange".
fn names(sk: &SurfaceSheet, pieces: &[(Piece, Region2)], reach: &BTreeSet<usize>) -> String {
    let mut names: Vec<String> = Vec::new();
    for k in reach {
        let name = match pieces.get(*k).map(|p| p.0) {
            Some(Piece::Panel(i) | Piece::Tab(_, i)) => sk.sheet.panels.get(i).map_or_else(|| format!("panel {i}"), |p| p.name.clone()),
            Some(Piece::Zone(z)) => format!("bend {}", z + 1),
            None => continue,
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    match names.as_slice() {
        [] => "the sheet".into(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What [`onto_panels`] read besides the sheet: whether it cut a panel's shadow, and the faces
/// the cut made that lie in the panels it read.
#[derive(Default)]
struct Read {
    shadowed: bool,
    claimed: BTreeSet<usize>,
}

/// A piece of a panel a cut parted from it, holding none of its joints: it stays where it was, in
/// the panel's chart (see the module docs).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Loose {
    pub panel: usize,
    pub region: Region2,
}

/// What [`left`] leaves of a panel's region.
enum Left {
    /// The piece that holds the panel's joints, and the pieces parted from it, which hold none.
    Kept(Region2, Vec<Region2>),
    /// Pieces that each hold some of the panel's joints, or nothing at all: the panel stays whole
    /// and the cut is a cut of the sheet's, so that each piece folds and lies flat where the panel
    /// does, and a panel cut away keeps its record, its joints and its place.
    Apart,
}

/// What is left of a panel's `region` without `through`: the piece that holds the
/// panel's joints (the points `holders` on them; the largest piece where it has none), and the
/// pieces parted from it; or none, where pieces each hold some of them or nothing is left. What
/// goes through clear of the region's outline and holes is a hole of it as it is, its curves kept.
fn left(region: &Region2, through: &[Region2], holders: &[Vec2], tol: f64) -> std::result::Result<Left, String> {
    let outline = |t: &Region2| Region2 { outer: t.outer.clone(), holes: Vec::new() };
    if !through.is_empty() && through.iter().all(|t| holds_clear(region, &outline(t))) {
        let mut out = region.clone();
        out.holes.extend(through.iter().map(|t| t.outer.ccw().reversed()));
        // What a cut goes round (a letter's counter) is parted from the panel, with any other
        // cut inside it a hole of it.
        let parted = through
            .iter()
            .flat_map(|t| &t.holes)
            .map(|h| {
                let outer = h.ccw();
                let holes = through
                    .iter()
                    .filter(|o| o.outer.segs.first().is_some_and(|g| holds(&outer, g.start())))
                    .map(|o| o.outer.ccw().reversed())
                    .collect();
                Region2 { outer, holes }
            })
            .collect();
        return Ok(Left::Kept(out, parted));
    }
    let mut left = planar_regions_boolean(std::slice::from_ref(region), through, BoolOp::Cut, tol).map_err(|e| e.to_string())?;
    if left.len() <= 1 {
        return Ok(left.pop().map_or(Left::Apart, |one| Left::Kept(one, Vec::new())));
    }
    // The piece nearest each joint's point holds it.
    let gap = |r: &Region2, p: Vec2| {
        if region_holds(r, p) {
            return 0.0;
        }
        std::iter::once(&r.outer)
            .chain(&r.holes)
            .flat_map(|l| &l.segs)
            .map(|s| s.point_at(s.closest_param(p).clamp(0.0, 1.0)).dist(p))
            .fold(f64::MAX, f64::min)
    };
    let holder = |p: Vec2| (0..left.len()).min_by(|&i, &j| gap(&left[i], p).total_cmp(&gap(&left[j], p)));
    let held: BTreeSet<usize> = holders.iter().filter_map(|p| holder(*p)).collect();
    let keep = match held.len() {
        0 => (0..left.len()).max_by(|&i, &j| left[i].area().total_cmp(&left[j].area())).unwrap_or(0),
        1 => held.first().copied().unwrap_or(0),
        _ => return Ok(Left::Apart),
    };
    let kept = left.swap_remove(keep);
    Ok(Left::Kept(kept, left))
}

/// What is left of panel `p` of `sk` without `through` (in the panel's chart): the piece that
/// holds its joints, and the pieces parted from it, as [`left`] reads them. Pieces that each hold
/// some of its joints, or nothing left, are an error here: the body's cut keeps that panel whole
/// (see [`Left::Apart`]).
pub(crate) fn left_of_panel(sk: &SurfaceSheet, p: usize, through: &[Region2], tol: f64) -> std::result::Result<(Region2, Vec<Region2>), String> {
    let panel = sk.sheet.panels.get(p).ok_or_else(|| format!("the sheet has no panel {p}"))?;
    match left(&panel.region, through, &holders(sk, p), tol)? {
        Left::Kept(kept, parted) => Ok((kept, parted)),
        Left::Apart => Err(format!("cuts {} apart into pieces that each hold a joint, or leaves nothing of it", panel.name)),
    }
}

/// Where joint `j` holds its `a` panel (`a`) or its `b` panel: the middle of a bend's zone along
/// the anchor, else the anchor's origin.
fn holds_at(j: &Joint, a: bool) -> Vec2 {
    let an = if a { &j.a } else { &j.b };
    let along = match &j.kind {
        JointKind::Bend(b) => {
            let (lo, hi) = if a { b.ends.near } else { b.ends.far };
            (lo + hi) / 2.0
        }
        _ => 0.0,
    };
    an.origin + an.dir * along
}

/// The points where the joints of `sk`'s panel `p` hold it (see [`holds_at`]).
fn holders(sk: &SurfaceSheet, p: usize) -> Vec<Vec2> {
    let mut out = Vec::new();
    for j in &sk.sheet.joints {
        if j.a.panel == p {
            out.push(holds_at(j, true));
        }
        if j.b.panel == p {
            out.push(holds_at(j, false));
        }
    }
    out
}

/// The sheet `sk` with what its body lost between `was` and `now` read onto its flat panels: what
/// went through is out of their outlines, and their recesses are the floors `now` has in them.
/// Reading a cut, `walls` are the faces of `now` it made, and where a panel's walls are not
/// square (by their surfaces, see [`lean`], or by what its faces show), or the panel is a roll,
/// what went through is the shadow they cast: of a bore lying in a flat panel, its band (see
/// [`lying`]). Reading a converted body's engravings (no `walls`), a panel's walls must be
/// square, away from its outline. The
/// extrude `drawn` that made the cut gives the floors it made their depths.
fn onto_panels(
    sk: &SurfaceSheet,
    was: &Body,
    now: &Body,
    walls: Option<&[usize]>,
    drawn: Option<&Drawn>,
) -> std::result::Result<(SurfaceSheet, Read), String> {
    let strict = walls.is_some();
    let dev = sk.develop().map_err(|e| e.to_string())?;
    let frames = frames(sk, &dev).map_err(|e| e.to_string())?;
    let (qa, qb) = (Query::new(was.record()), Query::new(now.record()));
    let t = sk.sheet.thickness;
    let tol = tolerance(was);
    let sag = (was.size() * 1e-4).max(1e-4);
    let mut out = sk.clone();
    let mut taken = Read::default();
    let mut recesses: Vec<Recess> = sk.recesses.clone();
    // A cut whose walls cross a joint is read once across every piece it reaches, its shadow in
    // the flat, and kept as a cut of the sheet's, drawn in the flat chart of a panel it reaches;
    // the panels' own reading leaves it be. Unfolded, where the cut's walls are square to the
    // flat, what it took is what the flat's faces lost (see [`flat_gone`]), and a region of that
    // crosses a joint where it takes material from a bend or from more than one panel (see
    // [`spans_joint`]), wherever its walls lie: a box over a panel, its bend and the next panel's
    // end has walls on that panel alone, and takes the bend whole.
    let mut crossing: BTreeSet<usize> = BTreeSet::new();
    let mut across: Vec<Region2> = Vec::new();
    // Folded, the fold and the flat's pieces, for the bends a cut may have taken whole (below),
    // and what each panel's own reading takes from it, in its chart.
    let mut whole: Option<(Fold, Vec<(Piece, Region2)>)> = None;
    let mut gone_from: Vec<(usize, Vec<Region2>)> = Vec::new();
    // Folded, the pieces of the flat (by their places in its list) that some wall of the cut
    // reaches.
    let mut touched: BTreeSet<usize> = BTreeSet::new();
    if let Some(walls) = walls.filter(|w| !w.is_empty()) {
        let fold =
            if sk.unfolded { None } else { Some(crate::sheet_surface_convert::fold_onto(&sk.sheet, &dev, &sk.places).map_err(|e| e.to_string())?) };
        let pieces = crate::sheet_surface::pieces(&dev, &mut FlatWork::new()).map_err(|e| e.to_string())?;
        let flat = if sk.unfolded { flat_gone(sk, (was, &qa), (now, &qb), walls, tol)? } else { None };
        if let Some(gone) = &flat {
            for g in gone {
                if !spans_joint(&pieces, g, tol)? {
                    continue;
                }
                let anchor = holder(&pieces, g).ok_or("leaves no panel of the sheet any material: not supported yet")?;
                let back = dev.placements.get(anchor).copied().flatten().ok_or("a panel of the sheet has no place in the flat")?.inverse();
                out.sheet.cuts.push(Cut { panel: anchor, region: back.region(g) });
                across.push(g.clone());
                // Its walls are read with it.
                crossing.extend(walls.iter().copied().filter(|f| !bounded(&qb, &sk.flat, &[*f], std::slice::from_ref(g), tol * 100.0).is_empty()));
                taken.shadowed = true;
            }
            taken.claimed.extend(crossing.iter().copied());
        }
        let folded = Folded::new(carriers(sk, &dev, fold.as_ref(), &pieces)?, fold.as_ref(), t);
        for group in components(&qb, walls).into_iter().filter(|_| flat.is_none()) {
            let reach = reached(&qb, &folded, &group, sag);
            touched.extend(reach.iter().copied());
            let mut panels = reach.iter().map(|k| match pieces.get(*k).map(|p| p.0) {
                Some(Piece::Panel(i)) => Some(i),
                _ => None,
            });
            let first = panels.next().flatten();
            if first.is_some() && panels.all(|i| i == first) {
                continue;
            }
            let through = shadow(now, &qb, &folded, &group, &names(sk, &pieces, &reach), 0.0, tol, sag)?;
            // Each region is kept in the chart of the panel where it lies (see `holder`).
            for r in &through {
                let anchor = holder(&pieces, r).ok_or("leaves no panel of the sheet any material: not supported yet")?;
                let back = dev.placements.get(anchor).copied().flatten().ok_or("a panel of the sheet has no place in the flat")?.inverse();
                out.sheet.cuts.push(Cut { panel: anchor, region: back.region(r) });
            }
            across.extend(through);
            crossing.extend(group.iter().copied());
            taken.claimed.extend(group);
            taken.shadowed = true;
        }
        drop(folded);
        whole = fold.map(|f| (f, pieces));
    }
    let rest: Option<Vec<usize>> = walls.map(|w| w.iter().copied().filter(|f| !crossing.contains(f)).collect());
    for (p, (panel, frame)) in sk.sheet.panels.iter().zip(&frames).enumerate() {
        let Some(frame) = frame else { continue };
        let plane = matches!(panel.surface, Surface::Plane);
        // Unfolded, a roll lies flat: its slab is not its surface's.
        if sk.unfolded && !plane {
            continue;
        }
        let slab = Slab::new(&panel.surface, *frame, t);
        let here = rest.as_deref().map(|w| within(&qb, &slab, &panel.region, w, tol * 100.0, sag)).unwrap_or_default();
        taken.claimed.extend(here.iter().copied());
        if !plane {
            // A roll: what went through it is its walls' shadow.
            if here.is_empty() {
                continue;
            }
            let through = shadow(now, &qb, &slab, &here, &panel.name, around(&panel.region), tol, sag)?;
            taken.shadowed = true;
            let Left::Kept(kept, parted) = left(&panel.region, &through, &holders(sk, p), tol)? else {
                return Err(format!("cuts {} apart: not supported yet on a roll", panel.name));
            };
            if !parted.is_empty() {
                return Err(format!("cuts {} apart: not supported yet on a roll", panel.name));
            }
            if let Some(x) = out.sheet.panels.get_mut(p) {
                x.region = kept;
            }
            continue;
        }
        // A bore lying in the panel takes its shadow, read in closed form (see [`lying`]); the
        // cut's other walls here are read as before. Whether they are square is read from their
        // surfaces too: a wall that leans is not square to the panel, whatever its faces show.
        let (bands, bored) = lying(now, &qb, frame, t, &here, &panel.name, tol)?;
        let others: Vec<usize> = here.iter().copied().filter(|f| !bored.contains(f)).collect();
        let leaning = others.iter().any(|f| !matches!(lean(now, *f, frame.z, tol), Lean::Square));
        let read = |b: &Body, q: &Query| on_panel(b, q, frame, t, std::slice::from_ref(&panel.region), tol, drawn).map_err(|e| e.to_string());
        let (before, after) = (read(was, &qa)?, read(now, &qb)?);
        // The panel's flat material before the cut, and each face's after it (its own face, or
        // a floor seen from it).
        let all = |o: &OnPanel| {
            o.near.iter().chain(&o.far).cloned().chain(o.near_floors.iter().chain(&o.far_floors).map(|x| x.1.clone())).collect::<Vec<_>>()
        };
        let footprint = union(all(&before), tol).map_err(|e| e.to_string())?;
        // What the cuts across joints took here is theirs, read with them, not here: a region
        // that lies within theirs, by the planar booleans (none of its area left outside them).
        // So is what a bore lying in the panel took, which its band holds, whatever the panel's
        // faces show of it.
        let mut theirs: Vec<Region2> = dev
            .placements
            .get(p)
            .copied()
            .flatten()
            .map(|pl| pl.inverse())
            .map(|back| across.iter().map(|r| back.region(r)).collect())
            .unwrap_or_default();
        theirs.extend(bands.iter().cloned());
        let ours = |g: &Region2| -> std::result::Result<bool, String> {
            if theirs.is_empty() {
                return Ok(true);
            }
            let outside = planar_regions_boolean(std::slice::from_ref(g), &theirs, BoolOp::Cut, tol).map_err(|e| e.to_string())?;
            // A zero test on a computed area (a sliver the booleans leave along a shared edge),
            // not a geometric tolerance.
            Ok(area(&outside) > tol * 10.0 * perimeter(std::slice::from_ref(g)).max(1.0))
        };
        let seen = |own: &[Region2], floors: &[(f64, Region2)]| union(own.iter().cloned().chain(floors.iter().map(|x| x.1.clone())).collect(), tol);
        // Within the flat material only: past it (toward a sharp corner the body was drawn with)
        // the two faces of a bend's panels end apart.
        let within = |r: Vec<Region2>| {
            if inside_by_loops(&r, &footprint, tol) {
                return Ok(r);
            }
            planar_regions_boolean(&r, &footprint, BoolOp::Intersect, tol).map_err(|e| e.to_string())
        };
        let near = within(seen(&after.near, &after.near_floors).map_err(|e| e.to_string())?)?;
        let far = within(seen(&after.far, &after.far_floors).map_err(|e| e.to_string())?)?;
        // Square walls: each face sees material wherever the other does.
        let apart = |a: &[Region2], b: &[Region2]| planar_regions_boolean(a, b, BoolOp::Cut, tol).map(|r| area(&r)).map_err(|e| e.to_string());
        let slack = tol * 10.0 * perimeter(&footprint).max(1.0);
        let off_outline = |a: &[Region2], b: &[Region2]| -> std::result::Result<f64, String> {
            let gaps = planar_regions_boolean(a, b, BoolOp::Cut, tol).map_err(|e| e.to_string())?;
            let mut off = 0.0;
            for g in &gaps {
                if ours(g)? && (strict || !touches(g, &footprint, tol * 100.0)) {
                    off += g.area();
                }
            }
            Ok(off)
        };
        // A recess's floor lies in the flat material: one reaching into a bend has no one place.
        let floors = |list: Vec<(f64, Region2)>| -> std::result::Result<Vec<(f64, Region2)>, String> {
            let mut kept = Vec::new();
            for (depth, r) in list {
                if apart(std::slice::from_ref(&r), &footprint)? > slack {
                    return Err(format!("leaves a recess reaching into a bend from {}: not supported yet", panel.name));
                }
                kept.extend(within(vec![r])?.into_iter().map(|r| (depth, r)));
            }
            Ok(kept)
        };
        let (near_floors, far_floors) = (floors(after.near_floors)?, floors(after.far_floors)?);
        // Faces with the same loops see material at the same places: square walls, unless one
        // leans.
        let square = !leaning && (same_regions(&near, &far, tol) || (off_outline(&near, &far)? <= slack && off_outline(&far, &near)? <= slack));
        // Through: the flat material no face sees any longer, or, where the walls are not
        // square, their shadow; and each bore's band.
        let through = if square {
            let gone = match gone_by_loops(&footprint, &near, tol) {
                Some(gone) => gone,
                None => planar_regions_boolean(&footprint, &near, BoolOp::Cut, tol).map_err(|e| e.to_string())?,
            };
            let mut kept = Vec::with_capacity(gone.len());
            for g in gone {
                if ours(&g)? {
                    kept.push(g);
                }
            }
            kept
        } else if strict && !others.is_empty() {
            taken.shadowed = true;
            shadow(now, &qb, &slab, &others, &panel.name, 0.0, tol, sag)?
        } else if strict {
            // Its walls here run on past the panel.
            return Err("can't be kept in the sheet yet: it reaches a bend or a seam".into());
        } else {
            return Err(format!(
                "leaves walls that are not square to the sheet on {}: not supported yet (a countersink, a rounded or chamfered face edge, a hole at a slant)",
                panel.name
            ));
        };
        let through = if bands.is_empty() {
            through
        } else {
            taken.shadowed = true;
            union(through.into_iter().chain(bands.iter().cloned()).collect(), tol).map_err(|e| e.to_string())?
        };
        if area(&through) > slack {
            if whole.is_some() {
                gone_from.push((p, through.clone()));
            }
            // A panel's region runs on past its material to each rip's line, for the rip to cut
            // it back (a corner closed with a gap). What went through ends at the rip's new
            // edge, inside the region: taken from the region as it is, a cut that reaches that
            // edge would be a hole the rip leaves on the outline, or would leave a strip as wide
            // as the rip's cut, which the rip cuts back across along the region's own edges. The
            // strip is the rip's lap, never material: the cut runs on across it to the rip's line
            // (see [`across_laps`]), so that the panel's outline holds the cut as drawn, and the
            // rip cuts back what is left of its edge.
            let rips = rip_edges(&dev, p);
            let mut own = Vec::with_capacity(through.len());
            for g in through {
                own.push(across_laps(sk, p, g, &rips, tol)?);
            }
            let through = own;
            if !through.is_empty() {
                match left(&panel.region, &through, &holders(sk, p), tol)? {
                    Left::Kept(kept, parted) => {
                        if let Some(x) = out.sheet.panels.get_mut(p) {
                            x.region = kept;
                        }
                        out.loose.extend(parted.into_iter().map(|region| Loose { panel: p, region }));
                    }
                    Left::Apart => {
                        // The panel stays whole: what went through is a cut of the sheet's, in the
                        // panel's chart.
                        out.sheet.cuts.extend(through.iter().map(|r| Cut { panel: p, region: r.clone() }));
                    }
                }
            }
        }
        // The panel's recesses are its floors as they are now. Floors read again as they were (a
        // cut that changed nothing here), to the rounding of the body's numbers, keep their
        // records: each recess's depth stays as it was recorded, not as its floor's plane measures
        // now. A floor any farther off is another floor, and the panel's records are read anew.
        let fresh: Vec<Recess> = near_floors
            .into_iter()
            .map(|(depth, region)| Recess { panel: p, region, far: false, depth })
            .chain(far_floors.into_iter().map(|(depth, region)| Recess { panel: p, region, far: true, depth }))
            .collect();
        if !same_recesses(&fresh, recesses.iter().filter(|r| r.panel == p).collect(), rounding(now)) {
            recesses.retain(|r| r.panel != p);
            recesses.extend(fresh);
        }
    }
    out.recesses = recesses;
    // Folded, a stretch of a bend beside which the cut took both panels' material, with no wall
    // of its own (the cut went through it whole), would come back with the sheet. A stretch that
    // a wall of the cut reaches was read with its walls (above). Whether a panel still holds one
    // that no wall reaches is read by region (see [`held`]); where neither does, whether the body
    // still has it is read at one point of it; where it has not, the stretch is a cut of the
    // sheet's.
    if let Some((fold, pieces)) = &whole {
        for (k, (piece, region)) in pieces.iter().enumerate() {
            let (Piece::Zone(z), false, false) = (*piece, region.outer.segs.is_empty(), touched.contains(&k)) else { continue };
            let Some(zone) = dev.zones.get(z) else { continue };
            if held(&dev, zone, region, pieces, &gone_from, 0, tol)? || held(&dev, zone, region, pieces, &gone_from, 1, tol)? {
                continue;
            }
            let Some(bent) = fold.bends.get(zone.joint).copied().flatten() else { continue };
            // One point answers for the whole stretch, exactly: no wall of the cut reaches it, so
            // no face of the body runs through its material, which the cut took all of or none of.
            // The point is inside it by construction, not by a chosen offset: halfway through the
            // thickness, at the region's inner point (its centroid, or the middle of a span across
            // it), so no face of the body lies at it.
            let middle = bent.point(region.interior_point(), t / 2.0);
            if now.classify_point(middle).map_err(|e| e.to_string())? != solidcraft_kernel::PointClass::Outside {
                continue;
            }
            let anchor = holder(pieces, region).ok_or("leaves no panel of the sheet any material: not supported yet")?;
            let back = dev.placements.get(anchor).copied().flatten().ok_or("a panel of the sheet has no place in the flat")?.inverse();
            out.sheet.cuts.push(Cut { panel: anchor, region: back.region(region) });
        }
    }
    Ok((out, taken))
}

/// Whether the panel on side `k` of a bend's `zone` (0 its `a` panel, 1 its `b` one) still holds
/// the zone's piece `zr`, of the flat's `pieces` (both as the development `dev` lays them out),
/// once its own reading took `gone` (each panel's, in its chart) from it. Read by region, exactly:
/// the panel's pieces less what was taken, on its side of its tangent line, are joined to the
/// zone's piece by the planar booleans, as the flat joins its pieces; the panel holds it where
/// they share a stretch of the line, half of what the two outlines lose to the join. A panel
/// nothing was taken from holds it as it did.
#[allow(clippy::too_many_arguments)]
fn held(
    dev: &Development,
    zone: &solidcraft_develop::Zone,
    zr: &Region2,
    pieces: &[(Piece, Region2)],
    gone: &[(usize, Vec<Region2>)],
    k: usize,
    tol: f64,
) -> std::result::Result<bool, String> {
    let (Some(&p), Some(&[a, b]), Some(&[c, _])) = (zone.panels.get(k), [zone.near, zone.far].get(k), [zone.far, zone.near].get(k)) else {
        return Ok(false);
    };
    let taken = gone.iter().find(|(i, _)| *i == p).map(|(_, g)| g);
    let (Some(taken), Some(at)) = (taken, dev.placements.get(p).copied().flatten()) else { return Ok(true) };
    let own: Vec<Region2> = pieces.iter().filter(|(piece, _)| *piece == Piece::Panel(p)).map(|(_, r)| r.clone()).collect();
    let taken: Vec<Region2> = taken.iter().map(|r| at.region(r)).collect();
    let kept = planar_regions_boolean(&own, &taken, BoolOp::Cut, tol).map_err(|e| e.to_string())?;
    let Some(d) = (b - a).normalized() else { return Ok(false) };
    // The panel's side of its tangent line: away from the zone's other line. What lies past the
    // line, beyond the zone's end, is no hold of the bend's.
    let across = Vec2::new(-d.y, d.x);
    let outward = if (c - a).dot(across) > 0.0 { across * -1.0 } else { across };
    // Far enough that the region below holds all of the panel's pieces on that side.
    let bounds = std::iter::once(zr).chain(&kept).map(|r| loop_box(&r.outer)).reduce(|x, y| x.union(&y));
    let reach = bounds.map_or(0.0, |x| x.size().len()) + (b - a).len() + (c - a).len() + 1.0;
    let side = Region2 {
        outer: Loop2::polygon(&[a - d * reach, b + d * reach, b + d * reach + outward * reach, a - d * reach + outward * reach]).ccw(),
        holes: Vec::new(),
    };
    let kept = planar_regions_boolean(&kept, std::slice::from_ref(&side), BoolOp::Intersect, tol).map_err(|e| e.to_string())?;
    if kept.is_empty() {
        return Ok(false);
    }
    let joined = planar_regions_boolean(std::slice::from_ref(zr), &kept, BoolOp::Union, tol).map_err(|e| e.to_string())?;
    let shared = (perimeter(std::slice::from_ref(zr)) + perimeter(&kept) - perimeter(&joined)) / 2.0;
    // A zero test on a computed length, not a geometric tolerance.
    Ok(shared > tol * 10.0)
}

/// Whether region `g` of the flat takes material from a bend's zone (or a tab of one) or from more
/// than one panel, of the flat's `pieces`: a cut across a joint.
fn spans_joint(pieces: &[(Piece, Region2)], g: &Region2, tol: f64) -> std::result::Result<bool, String> {
    let slack = tol * 10.0 * perimeter(std::slice::from_ref(g)).max(1.0);
    // Boxes that hold the loops (see [`loop_box`]): a pair whose boxes are apart has nothing in
    // common.
    let gb = loop_box(&g.outer);
    let mut panels = BTreeSet::new();
    for (piece, region) in pieces {
        let b = loop_box(&region.outer);
        if b.max.x < gb.min.x || gb.max.x < b.min.x || b.max.y < gb.min.y || gb.max.y < b.min.y {
            continue;
        }
        let shared =
            planar_regions_boolean(std::slice::from_ref(g), std::slice::from_ref(region), BoolOp::Intersect, tol).map_err(|e| e.to_string())?;
        if area(&shared) <= slack {
            continue;
        }
        match *piece {
            Piece::Zone(_) | Piece::Tab(..) => return Ok(true),
            Piece::Panel(i) => {
                panels.insert(i);
            }
        }
    }
    Ok(panels.len() > 1)
}

/// Unfolded, what went through the sheet, read over the whole flat at once in its chart (see
/// [`SurfaceSheet::flat`]): the flat material that no face of `now` sees any
/// longer, as a panel's is read (see [`onto_panels`]), through bends and up to the sheet's outline.
/// None where its two faces don't see material alike, or where one of the cut's `walls` leans or
/// bores along the flat, whatever the faces show of it (walls not square to the flat, see
/// [`lean`]): there their shadow is read instead.
fn flat_gone(
    sk: &SurfaceSheet,
    (was, qa): (&Body, &Query),
    (now, qb): (&Body, &Query),
    walls: &[usize],
    tol: f64,
) -> std::result::Result<Option<Vec<Region2>>, String> {
    if walls.iter().any(|f| !matches!(lean(now, *f, sk.flat.z, tol), Lean::Square)) {
        return Ok(None);
    }
    let t = sk.sheet.thickness;
    let read = |b: &Body, q: &Query| on_panel(b, q, &sk.flat, t, &sk.regions, tol, None).map_err(|e| e.to_string());
    let (before, after) = (read(was, qa)?, read(now, qb)?);
    let seen = |own: &[Region2], floors: &[(f64, Region2)]| {
        union(own.iter().cloned().chain(floors.iter().map(|x| x.1.clone())).collect(), tol).map_err(|e| e.to_string())
    };
    let footprint = seen(&[before.near, before.far].concat(), &[before.near_floors, before.far_floors].concat())?;
    let (near, far) = (seen(&after.near, &after.near_floors)?, seen(&after.far, &after.far_floors)?);
    // Square walls: each face sees material wherever the other does.
    let apart = |a: &[Region2], b: &[Region2]| planar_regions_boolean(a, b, BoolOp::Cut, tol).map(|r| area(&r)).map_err(|e| e.to_string());
    let slack = tol * 10.0 * perimeter(&footprint).max(1.0);
    if !(same_regions(&near, &far, tol) || (apart(&near, &far)? <= slack && apart(&far, &near)? <= slack)) {
        return Ok(None);
    }
    Ok(Some(match gone_by_loops(&footprint, &near, tol) {
        Some(gone) => gone,
        None => planar_regions_boolean(&footprint, &near, BoolOp::Cut, tol).map_err(|e| e.to_string())?,
    }))
}

/// The panel a region `r` of the flat a cut took (the development's `pieces` lying as the flat has
/// them) is kept in: the one whose material lies nearest the region's middle, so that a bend's zone
/// goes with the panel across its nearer tangent line; between two as near, the one whose material
/// lies lower in the flat, then more to the left. Taken from where the cut and the material lie,
/// never from the order of the panels or the pieces. None where no panel has material left.
fn holder(pieces: &[(Piece, Region2)], r: &Region2) -> Option<usize> {
    let x = r.interior_point();
    let near = |g: &Region2| if region_holds(g, x) { 0.0 } else { outline_gap(g, x) };
    pieces
        .iter()
        .filter_map(|(p, g)| match *p {
            Piece::Panel(i) | Piece::Tab(_, i) => Some((i, near(g), g.interior_point())),
            Piece::Zone(_) => None,
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.y.total_cmp(&b.2.y)).then(a.2.x.total_cmp(&b.2.x)))
        .map(|(i, ..)| i)
}

/// How far point `p` lies from the outline of region `g`, its holes' too.
fn outline_gap(g: &Region2, p: Vec2) -> f64 {
    std::iter::once(&g.outer)
        .chain(&g.holes)
        .flat_map(|l| &l.segs)
        .map(|s| s.point_at(s.closest_param(p).clamp(0.0, 1.0)).dist(p))
        .fold(f64::INFINITY, f64::min)
}

/// Those of `gone` (regions in the chart of `frame`, the flat's) that the walls `group` of `q`'s
/// body run round: the middle of an edge of theirs lies on its outline, within `near`.
fn bounded(q: &Query, frame: &Rigid3, group: &[usize], gone: &[Region2], near: f64) -> Vec<usize> {
    let from = frame.inverse();
    let rec = q.record();
    let middles: Vec<Vec2> = group
        .iter()
        .filter_map(|f| rec.faces.get(*f))
        .flat_map(|face| &face.loops)
        .filter_map(|l| match l {
            Bounds::Edges { edges } => Some(edges),
            _ => None,
        })
        .flatten()
        .filter_map(|oe| {
            let e = oe.edge as usize;
            let [t0, t1] = q.edge_interval(e)?;
            let (p, _) = q.edge_at(e, 0.5 * (t0 + t1))?;
            let c = from.point(p);
            Some(Vec2::new(c.x, c.y))
        })
        .collect();
    (0..gone.len()).filter(|k| gone.get(*k).is_some_and(|g| middles.iter().any(|m| outline_gap(g, *m) <= near))).collect()
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{PI, SQRT_2};

    use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
    use solidcraft_geom::prelude::*;

    use super::{holds, loop_box_in, reaches, seg_box};

    /// How near points are one, as a body reads them.
    const NEAR: f64 = 1e-6;

    /// A rip's edge as a wall's end has it in the wall's chart: x = 1, y 0..−34.
    fn edge() -> Seg2 {
        Seg2::Line { a: Vec2::new(1.0, 0.0), b: Vec2::new(1.0, -34.0) }
    }

    fn reached(g: &Region2) -> bool {
        match reaches(g, &[edge()], NEAR) {
            Ok(r) => r,
            Err(e) => panic!("{e}"),
        }
    }

    fn region(segs: Vec<Seg2>) -> Region2 {
        Region2 { outer: Loop2 { segs }.ccw(), holes: Vec::new() }
    }

    fn polygon(pts: &[(f64, f64)]) -> Region2 {
        let pts: Vec<Vec2> = pts.iter().map(|(x, y)| Vec2::new(*x, *y)).collect();
        Region2 { outer: Loop2::polygon(&pts).ccw(), holes: Vec::new() }
    }

    /// Lines: along the edge, across it, touching it at a corner, or round it whole, a region
    /// reaches it; 0.01 short of it, it does not.
    #[test]
    fn lines_reach_an_edge_along_it_across_it_at_a_corner_or_round_it() {
        assert!(reached(&polygon(&[(1.0, -11.0), (50.0, -11.0), (50.0, -24.0), (1.0, -24.0)])));
        assert!(!reached(&polygon(&[(1.01, -11.0), (50.0, -11.0), (50.0, -24.0), (1.01, -24.0)])));
        assert!(reached(&polygon(&[(0.5, -11.0), (50.0, -11.0), (50.0, -24.0), (0.5, -24.0)])));
        assert!(reached(&polygon(&[(1.0, -10.0), (5.0, -8.0), (5.0, -12.0)])));
        assert!(reached(&polygon(&[(-5.0, 5.0), (5.0, 5.0), (5.0, -40.0), (-5.0, -40.0)])));
        assert!(!reached(&polygon(&[(2.0, 5.0), (5.0, 5.0), (5.0, -40.0), (2.0, -40.0)])));
    }

    /// A circle of radius 4 whose center lies 4 from the edge touches it at one point, as one arc
    /// or two; 4.01 from it, it is clear. Half of it turned away from the edge is clear too.
    #[test]
    fn an_arc_reaches_an_edge_where_it_touches_it() {
        let arc = |x: f64, start: f64, sweep: f64| Seg2::Arc { center: Vec2::new(x, -19.0), radius: 4.0, start, sweep };
        for x in [5.0, 5.01] {
            let whole = region(vec![arc(x, 0.0, 2.0 * PI)]);
            let halves = region(vec![arc(x, 0.5 * PI, PI), arc(x, 1.5 * PI, PI)]);
            assert_eq!((reached(&whole), reached(&halves)), (x == 5.0, x == 5.0), "center {x}");
        }
        let away = region(vec![arc(5.0, -0.5 * PI, PI), Seg2::Line { a: Vec2::new(5.0, -15.0), b: Vec2::new(5.0, -23.0) }]);
        assert!(!reached(&away));
    }

    /// A cubic that dips to the edge, x = 1 + (2t − 1)² (its control values 2, 2/3, 2/3, 2), and a
    /// conic that is a quarter of a circle of radius 4 about (5, −19) round its point nearest the
    /// edge: each touches it at one point, inside the curve; moved 0.01 away, each is clear.
    #[test]
    fn a_cubic_and_a_conic_reach_an_edge_where_they_touch_it() {
        for off in [0.0, 0.01] {
            let p = |x: f64, y: f64| Vec2::new(x + off, y);
            let dip = Seg2::Cubic { p0: p(2.0, -10.0), p1: p(2.0 / 3.0, -10.0 - 4.0 / 3.0), p2: p(2.0 / 3.0, -10.0 - 8.0 / 3.0), p3: p(2.0, -14.0) };
            let lines = [p(2.0, -14.0), p(6.0, -14.0), p(6.0, -10.0), p(2.0, -10.0)];
            let mut segs = vec![dip];
            segs.extend(lines.windows(2).map(|w| Seg2::Line { a: w[0], b: w[1] }));
            assert_eq!(reached(&region(segs)), off == 0.0, "cubic {off}");
            let r = 4.0 / SQRT_2;
            let (a, b) = (p(5.0 - r, -19.0 + r), p(5.0 - r, -19.0 - r));
            let quarter = Seg2::Conic { a, apex: p(5.0 - 4.0 * SQRT_2, -19.0), b, w: 0.5 * SQRT_2 };
            assert_eq!(reached(&region(vec![quarter, Seg2::Line { a: b, b: a }])), off == 0.0, "conic {off}");
        }
    }

    /// A curved cut that comes near a rip's curved edge has no exact reading here: refused.
    #[test]
    fn a_curve_near_a_curved_rip_edge_is_refused() {
        let curved = Seg2::Cubic { p0: Vec2::new(1.0, 0.0), p1: Vec2::new(0.5, -10.0), p2: Vec2::new(0.5, -24.0), p3: Vec2::new(1.0, -34.0) };
        let g = region(vec![
            Seg2::Cubic { p0: Vec2::new(2.0, -10.0), p1: Vec2::new(0.0, -11.0), p2: Vec2::new(0.0, -13.0), p3: Vec2::new(2.0, -14.0) },
            Seg2::Line { a: Vec2::new(2.0, -14.0), b: Vec2::new(2.0, -10.0) },
        ]);
        let e = reaches(&g, &[curved], NEAR).err().unwrap_or_default();
        assert!(e.contains("not supported yet"), "{e}");
    }

    /// A circle of radius 2 round (15, 6), as one arc and as two halves, each either way round (a
    /// hole runs clockwise): a point is held only inside it, level with its arcs' ends too. A whole
    /// turn's end, at 2π, can read a rounding off its start, at 0; a corner read at two heights let
    /// a ray level with it cross the circle's left side and not its right, so a point left of the
    /// circle was inside it (a carried cut-out of the flat lost its piece, beside another in the
    /// same row). Each corner is now read once, from the next edge's start.
    #[test]
    fn a_point_level_with_a_circles_ends_is_held_only_inside_it() {
        let c = Vec2::new(15.0, 6.0);
        let one = Loop2::circle(c, 2.0);
        let arc = |start: f64| Seg2::Arc { center: c, radius: 2.0, start, sweep: PI };
        let halves = Loop2 { segs: vec![arc(0.0), arc(PI)] };
        for l in [one.clone(), one.reversed(), halves.clone(), halves.reversed()] {
            let levels: Vec<f64> = l.segs.iter().flat_map(|s| [s.start().y, s.end().y]).chain([6.0]).collect();
            for y in levels {
                for (x, inside) in [(8.0, false), (15.0, true), (20.0, false)] {
                    assert_eq!(holds(&l, Vec2::new(x, y)), inside, "({x}, {y:e}) in {l:?}");
                }
            }
        }
    }

    /// An arc whose angles are not finite, or so large that a half turn no longer moves them, has
    /// no top or bottom of its circle to stop at, and is read to an end. Each arc round (15, 6),
    /// radius 2, is a loop of its own, so its end is its start: the ray from the center, level or
    /// not with them, crosses it nowhere between them, and the loop holds no point. A sweep of +∞
    /// stepped toward its end forever; one of −∞ put its first stop at −∞, which a half turn left
    /// there; and from 1e17 (where a double's step is 16) the half turns past it, 1e3 on, rounded
    /// back to where they were.
    #[test]
    fn an_arc_whose_angles_are_not_finite_is_read_to_an_end() {
        let c = Vec2::new(15.0, 6.0);
        for (start, sweep) in [(0.0, f64::INFINITY), (0.0, f64::NEG_INFINITY), (1e17, 1e3)] {
            let l = Loop2 { segs: vec![Seg2::Arc { center: c, radius: 2.0, start, sweep }] };
            assert!(!holds(&l, c), "start {start}, sweep {sweep}");
        }
    }

    /// An arc's box is its own: its ends, and each of its circle's points center ± radius along x
    /// and along y that it passes. Round (1, 1), radius 1, a quarter turn clockwise from 0 runs
    /// from (2, 1) to (1, 0) past neither (1, 2) nor (0, 1): x 1..2, y 0..1, a quarter of its
    /// circle's box (x 0..2, y 0..2, what the box was before). Round the origin, radius 2, a
    /// quarter turn counter-clockwise from −π/4 runs from (√2, −√2) to (√2, √2) past (2, 0):
    /// x √2..2, y −√2..√2. A line's box is its ends' (3, −1) and (−2, 4): x −2..3, y −1..4. A
    /// cubic's is its control points' (0, 0), (0, 3), (4, 3), (4, 0): x 0..4, y 0..3, which holds
    /// the curve, whose top is 9 t (1 − t) = 2.25 at t = ½.
    #[test]
    fn a_segments_box_is_its_own_for_a_line_or_an_arc_and_holds_any_other_curve() {
        let near = |a: Vec2, b: Vec2| a.dist(b) < 1e-12;
        let quarter = Seg2::Arc { center: Vec2::new(1.0, 1.0), radius: 1.0, start: 0.0, sweep: -PI / 2.0 };
        let (lo, hi) = seg_box(&quarter);
        assert!(near(lo, Vec2::new(1.0, 0.0)) && near(hi, Vec2::new(2.0, 1.0)), "{lo:?} {hi:?}");
        let across = Seg2::Arc { center: Vec2::ZERO, radius: 2.0, start: -PI / 4.0, sweep: PI / 2.0 };
        let (lo, hi) = seg_box(&across);
        assert!(near(lo, Vec2::new(SQRT_2, -SQRT_2)) && near(hi, Vec2::new(2.0, SQRT_2)), "{lo:?} {hi:?}");
        let line = Seg2::Line { a: Vec2::new(3.0, -1.0), b: Vec2::new(-2.0, 4.0) };
        assert_eq!(seg_box(&line), (Vec2::new(-2.0, -1.0), Vec2::new(3.0, 4.0)));
        let cubic = Seg2::Cubic { p0: Vec2::ZERO, p1: Vec2::new(0.0, 3.0), p2: Vec2::new(4.0, 3.0), p3: Vec2::new(4.0, 0.0) };
        assert_eq!(seg_box(&cubic), (Vec2::ZERO, Vec2::new(4.0, 3.0)));
    }

    /// A circle of radius 1 round the origin, as two halves, carried into space with its x along
    /// (√3/2, 0, ½) (tilted 30° about y), its y along y, and moved 5 along x. Its box in space is
    /// the tilted circle's own: x 5 ± √3/2 (4.1340…, 5.8660…), y −1..1, z −½..½. Its chords'
    /// box would be smaller wherever no chord ends at those points.
    #[test]
    fn a_loops_box_in_space_is_the_carried_loops_own() {
        let half = 3f64.sqrt() / 2.0;
        let arc = |start: f64| Seg2::Arc { center: Vec2::ZERO, radius: 1.0, start, sweep: PI };
        let circle = Loop2 { segs: vec![arc(0.0), arc(PI)] };
        let b = loop_box_in(&circle, |q| Vec3::new(5.0 + q.x * half, q.y, q.x * 0.5));
        let near = |a: Vec3, b: Vec3| a.dist(b) < 1e-12;
        assert!(near(b.min, Vec3::new(5.0 - half, -1.0, -0.5)) && near(b.max, Vec3::new(5.0 + half, 1.0, 0.5)), "{b:?}");
    }
}
