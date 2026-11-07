//! Cuts on a sheet made from surfaces (#6748). What a feature that is not sheet metal cuts from a
//! sheet's body, folded or unfolded, is read back onto the sheet's flat panels, so that the sheet
//! stays the master and the folded part, its flat and its DXF are all built from it alike:
//!
//! - where the cut goes through, the panel's outline loses it, as if the panel had been drawn so:
//!   no record of the cut is kept;
//! - where it stops inside the thickness on a floor parallel to the panel, the panel keeps a
//!   recess, from its own face or the face one thickness into the material, to the floor's depth;
//!   a later cut through a recess takes it with the material.
//!
//! The panels' faces are read before and after the feature, exactly, from the bodies' B-rep
//! records. A cut through a roll, or with walls not square to a flat panel (a hole at a slant),
//! goes through the panel as its shadow: everything whose fiber through the thickness meets
//! what the cut took, as a shop cuts it in the flat ([`crate::sheet_surface_shadow`]). A cut
//! whose walls cross a joint (a bend, a seam) is read the same way across every piece it
//! reaches at once, its outline in the flat, and kept as a cut of the sheet's: what it leaves
//! of a bend bends, or is a tab of the one panel it still joins. Unfolded, the sheet lies in one
//! plane, and such a cut with walls square to it is what the flat's faces lost, read over the
//! whole flat at once, up to the sheet's outline where it reaches it. Every face the cut made
//! must be read; checking each step, the sheet built again must also weigh what the cut body
//! does, or, where a shadow was cut, no more (a tab weighs flat). Refused: walls that don't run
//! through the sheet from face to face (a countersink, a rounded or chamfered face edge, a cut
//! that stops inside a bend), a recess in a roll, and, folded, a cut through the sheet's outline
//! at a slant, on a roll or across a bend (not supported yet).
//! A piece a cut parts from a panel that holds none of its joints (a letter's counter, the end
//! of a flange cut off beside its bend) stays where it was, a loose piece of the sheet: in the
//! folded body, in the flat and in its DXF; the cut says so of each that lies outside the panel.
//! A cut that parts a panel into pieces that each hold some of its joints leaves the panel whole
//! and is kept as a cut of the sheet's, in the panel's chart: each piece folds and lies flat where
//! the panel does, and the cut says so of what it parted from the sheet's base.

use solidcraft_geom::prelude::*;
use std::collections::{BTreeSet, VecDeque};

use solidcore_record::brep::Loop as Bounds;
use solidcore_record::{Loop2, Plane, Region2, Vec2};
use solidcraft_develop::{Cut, Development, Fold, Joint, JointKind, Rigid2, Rigid3, Surface, develop};
use solidcraft_geom::brep::query::Query;
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

/// The planar faces of `b` parallel to a panel whose frame is `frame` (its material toward
/// `z`, `t` deep), within its region `own` (or, unfolded, the whole flat's regions).
fn on_panel(b: &Body, q: &Query, frame: &Rigid3, t: f64, own: &[Region2], tol: f64) -> Result<OnPanel> {
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
            out.near_floors.extend(mine.into_iter().map(|r| (depth, r)));
        } else if toward && depth < t - tol {
            out.far_floors.extend(mine.into_iter().map(|r| (t - depth, r)));
        }
    }
    Ok(out)
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

/// Whether no two of `regions` come within `tol` of each other by their boxes: each pair's
/// outlines' boxes are apart, or one lies inside the box of a hole of the other.
fn apart(regions: &[Region2], tol: f64) -> bool {
    let bounds = |l: &Loop2| l.polyline(1e-3).bounds().inflated(tol);
    let boxes: Vec<(solidcraft_geom::Aabb2, Vec<solidcraft_geom::Aabb2>)> =
        regions.iter().map(|r| (bounds(&r.outer), r.holes.iter().map(bounds).collect())).collect();
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

/// Whether every region of `a` lies in a region of `b` by their loops (so `a` ∩ `b` is `a`): its
/// outline is that region's and it has each of that region's holes, or it lies in a new hole of
/// one that does (a piece a cut left inside it, as faces of one body never overlap).
fn inside_by_loops(a: &[Region2], b: &[Region2], tol: f64) -> bool {
    let holds = |x: &Region2, y: &Region2| same_loop(&x.outer, &y.outer, tol) && y.holes.iter().all(|k| x.holes.iter().any(|h| same_loop(h, k, tol)));
    let new_hole_at = |p: Vec2| {
        a.iter()
            .any(|x| b.iter().any(|y| holds(x, y) && x.holes.iter().any(|h| h.ccw().contains(p) && !y.holes.iter().any(|k| same_loop(h, k, tol)))))
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
        let g = gone.iter_mut().find(|g| g.outer.contains(at))?;
        g.holes.push(island.outer.ccw().reversed());
    }
    Some(gone)
}

/// Whether region `g` reaches within `near` of the outline of any of `of`.
fn touches(g: &Region2, of: &[Region2], near: f64) -> bool {
    let pts = g.outer.polyline(near.max(1e-6));
    of.iter().flat_map(|r| &r.outer.segs).any(|s| (&pts).into_iter().any(|p| s.point_at(s.closest_param(*p).clamp(0.0, 1.0)).dist(*p) <= near))
}

/// A panel's frame in its body: where the fold puts it, or in the flat, where the development
/// lays its chart in the plane of the first panel's face.
pub(crate) fn frames(sk: &SurfaceSheet, dev: &Development) -> Result<Vec<Option<Rigid3>>> {
    if !sk.unfolded {
        // A panel with a place of its own (the face it was made from) is read where that face is:
        // folding reaches it only to within the body's tolerance (see `fold_onto`).
        let folded = crate::sheet_surface_convert::fold_onto(&sk.sheet, dev, &sk.places)?.places;
        return Ok(folded.into_iter().enumerate().map(|(i, got)| sk.places.get(i).copied().flatten().or(got)).collect());
    }
    let base = sk.places.first().copied().flatten().ok_or_else(|| DocError::Invalid("the sheet's first panel has no place".into()))?;
    Ok(dev.placements.iter().map(|p| p.map(|r| lift(&base, &r))).collect())
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
/// What [`absorb`] made of a sheet: the sheet read with its body built again and the cut's note,
/// or why the sheet can't hold the change.
pub(crate) enum Absorbed {
    Read(Box<SurfaceSheet>, Body, Option<String>),
    Refused(String),
}

pub(crate) fn absorb(before: &ModelState, after: &ModelState, f: &Feature, weigh: bool) -> Result<Vec<(usize, Absorbed)>> {
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
        match cut(&sk, &was, &now, weigh) {
            Ok((cut, built, parted)) => {
                let note = parted_note(&cut, sk.loose.len(), &parted);
                out.push((i, Absorbed::Read(Box::new(cut), built, note)));
            }
            Err(e) => out.push((i, Absorbed::Refused(format!("`{}`: what {} cut {e}", sk.body, f.name)))),
        }
    }
    Ok(out)
}

/// The sheet `sk` with what was cut from its body, `was` before and `now` after, read onto its
/// flat panels, and its body built again, with a point of each piece the cut parted from the
/// sheet's base while leaving its panel whole (see [`Left::Apart`]): the panel, and the point in
/// its chart. Every face the cut made must lie in a panel or a piece of the flat it was read in.
/// Weighed (`weigh`), the body built again must also weigh what `now` does: a check of the
/// reading itself, which costs more than the rest of it.
pub(crate) fn cut(sk: &SurfaceSheet, was: &Body, now: &Body, weigh: bool) -> std::result::Result<(SurfaceSheet, Body, Vec<(usize, Vec2)>), String> {
    let walls = made(was, now, tolerance(was));
    let (mut out, read) = onto_panels(sk, was, now, Some(&walls))?;
    // Nothing else may have been cut: a bend's material, or anything the panels' faces don't show.
    if walls.iter().any(|f| !read.claimed.contains(f)) {
        return Err("can't be kept in the sheet yet: it reaches a bend or a seam".into());
    }
    let built = out.build().map_err(|e| e.to_string())?;
    if weigh {
        weighed(sk, &out, &built, now, read.shadowed)?;
    }
    Ok((out, built, read.parted))
}

/// The cut's note on the loose pieces past the first `had` of `sk` that lie outside their
/// panel (a letter's counter lies in a hole of it), and on the pieces of its flat that hold the
/// points `parted` (each a panel and a point in its chart) of what a cut parted from its base
/// while leaving their panels whole: how many, their area and where they lie in the flat
/// pattern, as it is read (see [`SurfaceSheet::shown`]). They stay. None when there are none,
/// or their places can't be worked out: the note is no reason for a cut to fail.
fn parted_note(sk: &SurfaceSheet, had: usize, parted: &[(usize, Vec2)]) -> Option<String> {
    let dev = develop(&sk.sheet, 0).ok()?;
    let flipped = |r: &Region2| crate::sheet_surface_convert::rigid_region(r, |p| Vec2::new(p.x, -p.y));
    let mut pieces: Vec<Region2> = sk
        .loose
        .iter()
        .skip(had)
        .filter(|l| {
            let inside = sk.sheet.panels.get(l.panel).is_some_and(|p| {
                let x = l.region.interior_point();
                p.region.outer.ccw().contains(x)
            });
            !inside
        })
        .filter_map(|l| Some(flipped(&dev.placements.get(l.panel).copied().flatten()?.region(&l.region))))
        .collect();
    // Each piece of the flat that holds a parted point, once.
    let mut found = BTreeSet::new();
    for (panel, at) in parted {
        let Some(x) = dev.placements.get(*panel).copied().flatten().map(|r| r.point(*at)) else { continue };
        if let Some(k) = sk.regions.iter().position(|r| r.contains(x))
            && found.insert(k)
            && let Some(r) = sk.regions.get(k)
        {
            pieces.push(flipped(r));
        }
    }
    if pieces.is_empty() {
        return None;
    }
    let area: f64 = pieces.iter().map(Region2::area).sum();
    let at: Vec<String> = pieces
        .iter()
        .take(4)
        .map(|r| {
            let c = r.interior_point();
            format!("({:.1}, {:.1})", c.x, c.y)
        })
        .collect();
    let at = at.join(", ");
    let body = &sk.body;
    Some(match pieces.len() {
        1 => format!("1 piece of `{body}` is no longer joined to its base ({area:.2} mm², near {at} in the flat pattern); it is kept"),
        n => {
            let more = if n > 4 { format!(" and {} more", n - 4) } else { String::new() };
            format!(
                "{n} pieces of `{body}` are no longer joined to its base ({area:.2} mm² in all, near {at}{more} in the flat pattern); they are kept"
            )
        }
    })
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
    let (mut out, _) = onto_panels(sk, built, body, None).map_err(DocError::Invalid)?;
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

/// A plane: the slab of a sheet laid out flat.
static FLAT: Surface = Surface::Plane;

/// The flat's `pieces` (of the development `dev` of the sheet `sk`) as `fold` carries them into
/// space, or, unfolded (no `fold`), as they lie in the plane of the sheet's first panel.
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
                (None, _) => Carrier::Flat(Slab::new(&FLAT, sk.places.first().copied().flatten().ok_or_else(lost)?, t)),
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

/// A point inside region `g`: a hair inside the middle of an edge of its outline.
fn inner_point(g: &Region2) -> Option<Vec2> {
    let hair = g.outer.polyline(1e-3).bounds().size().len() * 1e-4;
    g.outer.segs.iter().find_map(|s| {
        let along = s.derivative(0.5).normalized()?;
        let side = Vec2::new(-along.y, along.x) * hair;
        let m = s.point_at(0.5);
        [m + side, m - side].into_iter().find(|x| g.contains(*x))
    })
}

/// What [`onto_panels`] read besides the sheet: whether it cut a panel's shadow, the faces the
/// cut made that lie in the panels it read, and a point of each piece it parted from the sheet's
/// base while leaving its panel whole (see [`Left::Apart`]): the panel, and the point in its chart.
#[derive(Default)]
struct Read {
    shadowed: bool,
    claimed: BTreeSet<usize>,
    parted: Vec<(usize, Vec2)>,
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
    /// Pieces that each hold some of the panel's joints. The panel stays whole and the cut is a
    /// cut of the sheet's, so that each piece folds and lies flat where the panel does. Given
    /// are the pieces parted from the sheet's base: all but the one that holds the joint toward
    /// it, and none where the panel is the base.
    Apart(Vec<Region2>),
}

/// What is left of a panel's `region` named `name` without `through`: the piece that holds the
/// panel's joints (the points `holders` on them; the largest piece where it has none), and the
/// pieces parted from it; or, where pieces each hold some of them, those pieces but the one that
/// holds the point `toward` (on the joint toward the sheet's base; none on the base). What goes
/// through clear of the region's outline and holes is a hole of it as it is, its curves kept.
fn left(region: &Region2, through: &[Region2], name: &str, holders: &[Vec2], toward: Option<Vec2>, tol: f64) -> std::result::Result<Left, String> {
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
                    .filter(|o| o.outer.segs.first().is_some_and(|g| outer.contains(g.start())))
                    .map(|o| o.outer.ccw().reversed())
                    .collect();
                Region2 { outer, holes }
            })
            .collect();
        return Ok(Left::Kept(out, parted));
    }
    let mut left = planar_regions_boolean(std::slice::from_ref(region), through, BoolOp::Cut, tol).map_err(|e| e.to_string())?;
    if left.len() <= 1 {
        return left.pop().map(|one| Left::Kept(one, Vec::new())).ok_or_else(|| format!("takes the whole of {name}: not supported yet"));
    }
    // The piece nearest each joint's point holds it.
    let gap = |r: &Region2, p: Vec2| {
        if r.contains(p) {
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
        _ => {
            let Some(base) = toward.and_then(holder) else { return Ok(Left::Apart(Vec::new())) };
            return Ok(Left::Apart(left.into_iter().enumerate().filter(|(i, _)| *i != base).map(|(_, r)| r).collect()));
        }
    };
    let kept = left.swap_remove(keep);
    Ok(Left::Kept(kept, left))
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

/// Where the joint that holds `sk`'s panel `p` toward the sheet's base (its first panel) holds
/// it: the joint by which a walk from the base through bends and seams first reaches `p`, as the
/// flat is laid out. None for the base, and for a panel no such walk reaches.
fn toward_base(sk: &SurfaceSheet, p: usize) -> Option<Vec2> {
    let joints = &sk.sheet.joints;
    let n = sk.sheet.panels.len();
    let mut via: Vec<Option<(usize, bool)>> = vec![None; n];
    let mut seen = vec![false; n];
    *seen.first_mut()? = true;
    let mut queue = VecDeque::from([0usize]);
    while let Some(i) = queue.pop_front() {
        for (k, j) in joints.iter().enumerate() {
            if matches!(j.kind, JointKind::Rip(_)) {
                continue;
            }
            // The panel across the joint, and whether it is the joint's `a` panel.
            let (other, a) = if j.a.panel == i {
                (j.b.panel, false)
            } else if j.b.panel == i {
                (j.a.panel, true)
            } else {
                continue;
            };
            if seen.get(other).is_some_and(|s| !*s) {
                seen[other] = true;
                via[other] = Some((k, a));
                queue.push_back(other);
            }
        }
    }
    let (k, a) = via.get(p).copied().flatten()?;
    joints.get(k).map(|j| holds_at(j, a))
}

/// The sheet `sk` with what its body lost between `was` and `now` read onto its flat panels: what
/// went through is out of their outlines, and their recesses are the floors `now` has in them.
/// Reading a cut, `walls` are the faces of `now` it made, and where a panel's walls are not
/// square, or the panel is a roll, what went through is the shadow they cast. Reading a converted
/// body's engravings (no `walls`), a panel's walls must be square, away from its outline.
fn onto_panels(sk: &SurfaceSheet, was: &Body, now: &Body, walls: Option<&[usize]>) -> std::result::Result<(SurfaceSheet, Read), String> {
    let strict = walls.is_some();
    let dev = develop(&sk.sheet, 0).map_err(|e| e.to_string())?;
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
    // flat, what it took is what the flat's faces lost (see [`flat_gone`]): each region of that
    // is the cut of the walls that run round it, up to the sheet's outline where it reaches it.
    let mut crossing: BTreeSet<usize> = BTreeSet::new();
    let mut across: Vec<Region2> = Vec::new();
    if let Some(walls) = walls.filter(|w| !w.is_empty()) {
        let fold =
            if sk.unfolded { None } else { Some(crate::sheet_surface_convert::fold_onto(&sk.sheet, &dev, &sk.places).map_err(|e| e.to_string())?) };
        let pieces = crate::sheet_surface::pieces(&dev, &mut FlatWork::new()).map_err(|e| e.to_string())?;
        let folded = Folded::new(carriers(sk, &dev, fold.as_ref(), &pieces)?, fold.as_ref(), t);
        let mut flat: Option<Option<Vec<Region2>>> = None;
        let mut kept: BTreeSet<usize> = BTreeSet::new();
        for group in components(&qb, walls) {
            let reach = reached(&qb, &folded, &group, sag);
            let mut panels = reach.iter().map(|k| match pieces.get(*k).map(|p| p.0) {
                Some(Piece::Panel(i)) => Some(i),
                _ => None,
            });
            let first = panels.next().flatten();
            if first.is_some() && panels.all(|i| i == first) {
                continue;
            }
            if sk.unfolded && flat.is_none() {
                flat = Some(flat_gone(sk, (was, &qa), (now, &qb), tol)?);
            }
            let flat_base = sk.places.first().copied().flatten();
            let theirs = match (&flat, flat_base) {
                (Some(Some(gone)), Some(base)) => Some(bounded(&qb, &base, &group, gone, tol * 100.0)).filter(|k| !k.is_empty()).map(|k| (gone, k)),
                _ => None,
            };
            let through: Vec<Region2> = match theirs {
                // A region two groups of walls run round (a slot across the flat) is the first's.
                Some((gone, k)) => k.into_iter().filter(|k| kept.insert(*k)).filter_map(|k| gone.get(k).cloned()).collect(),
                None => shadow(now, &qb, &folded, &group, &names(sk, &pieces, &reach), 0.0, tol, sag)?,
            };
            let anchor = reach
                .iter()
                .find_map(|k| match pieces.get(*k)?.0 {
                    Piece::Panel(i) | Piece::Tab(_, i) => Some(i),
                    Piece::Zone(z) => dev.zones.get(z).map(|zone| zone.panels[0]),
                })
                .unwrap_or(0);
            let back = dev.placements.get(anchor).copied().flatten().ok_or("a panel of the sheet has no place in the flat")?.inverse();
            out.sheet.cuts.extend(through.iter().map(|r| Cut { panel: anchor, region: back.region(r) }));
            across.extend(through);
            crossing.extend(group.iter().copied());
            taken.claimed.extend(group);
            taken.shadowed = true;
        }
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
            let Left::Kept(kept, parted) = left(&panel.region, &through, &panel.name, &holders(sk, p), None, tol)? else {
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
        let read = |b: &Body, q: &Query| on_panel(b, q, frame, t, std::slice::from_ref(&panel.region), tol).map_err(|e| e.to_string());
        let (before, after) = (read(was, &qa)?, read(now, &qb)?);
        // The panel's flat material before the cut, and each face's after it (its own face, or
        // a floor seen from it).
        let all = |o: &OnPanel| {
            o.near.iter().chain(&o.far).cloned().chain(o.near_floors.iter().chain(&o.far_floors).map(|x| x.1.clone())).collect::<Vec<_>>()
        };
        let footprint = union(all(&before), tol).map_err(|e| e.to_string())?;
        // What the cuts across joints took here is theirs, read with them, not here.
        let theirs: Vec<Region2> = dev
            .placements
            .get(p)
            .copied()
            .flatten()
            .map(|pl| pl.inverse())
            .map(|back| across.iter().map(|r| back.region(r)).collect())
            .unwrap_or_default();
        let ours = |g: &Region2| !inner_point(g).is_some_and(|x| theirs.iter().any(|t| t.contains(x)));
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
            Ok(gaps.iter().filter(|g| ours(g) && (strict || !touches(g, &footprint, tol * 100.0))).map(Region2::area).sum())
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
        // Faces with the same loops see material at the same places: square walls.
        let square = same_regions(&near, &far, tol) || (off_outline(&near, &far)? <= slack && off_outline(&far, &near)? <= slack);
        // Through: the flat material no face sees any longer, or, where the walls are not
        // square, their shadow.
        let through = if square {
            let gone = match gone_by_loops(&footprint, &near, tol) {
                Some(gone) => gone,
                None => planar_regions_boolean(&footprint, &near, BoolOp::Cut, tol).map_err(|e| e.to_string())?,
            };
            gone.into_iter().filter(|g| ours(g)).collect()
        } else if strict && !here.is_empty() {
            taken.shadowed = true;
            shadow(now, &qb, &slab, &here, &panel.name, 0.0, tol, sag)?
        } else if strict {
            // Its walls here run on past the panel.
            return Err("can't be kept in the sheet yet: it reaches a bend or a seam".into());
        } else {
            return Err(format!(
                "leaves walls that are not square to the sheet on {}: not supported yet (a countersink, a rounded or chamfered face edge, a hole at a slant)",
                panel.name
            ));
        };
        if area(&through) > slack {
            match left(&panel.region, &through, &panel.name, &holders(sk, p), toward_base(sk, p), tol)? {
                Left::Kept(kept, parted) => {
                    if let Some(x) = out.sheet.panels.get_mut(p) {
                        x.region = kept;
                    }
                    out.loose.extend(parted.into_iter().map(|region| Loose { panel: p, region }));
                }
                Left::Apart(parted) => {
                    // The panel stays whole: what went through is a cut of the sheet's, in the
                    // panel's chart.
                    out.sheet.cuts.extend(through.iter().map(|r| Cut { panel: p, region: r.clone() }));
                    taken.parted.extend(parted.iter().map(|r| (p, r.interior_point())));
                }
            }
        }
        // The panel's recesses are its floors as they are now.
        recesses.retain(|r| r.panel != p);
        recesses.extend(near_floors.into_iter().map(|(depth, region)| Recess { panel: p, region, far: false, depth }));
        recesses.extend(far_floors.into_iter().map(|(depth, region)| Recess { panel: p, region, far: true, depth }));
    }
    out.recesses = recesses;
    Ok((out, taken))
}

/// Unfolded, what went through the sheet, read over the whole flat at once in the plane of its
/// first panel, the chart the flat is drawn in: the flat material that no face of `now` sees any
/// longer, as a panel's is read (see [`onto_panels`]), through bends and up to the sheet's outline.
/// None where its two faces don't see material alike (walls not square to the flat): there their
/// shadow is read instead.
fn flat_gone(
    sk: &SurfaceSheet,
    (was, qa): (&Body, &Query),
    (now, qb): (&Body, &Query),
    tol: f64,
) -> std::result::Result<Option<Vec<Region2>>, String> {
    let base = sk.places.first().copied().flatten().ok_or("the sheet's first panel has no place")?;
    let t = sk.sheet.thickness;
    let read = |b: &Body, q: &Query| on_panel(b, q, &base, t, &sk.regions, tol).map_err(|e| e.to_string());
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
