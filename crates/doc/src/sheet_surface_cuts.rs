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
//! what the cut took, as a shop cuts it in the flat ([`crate::sheet_surface_shadow`]). The sheet
//! built again must weigh what the cut body does, or, where a shadow was cut, no more, with
//! every face the cut made lying in a panel it was read on. Refused: walls that don't run
//! through the sheet from face to face (a countersink, a rounded or chamfered face edge), a
//! recess in a roll, a cut that reaches a bend or a seam (not supported yet), or splits a panel
//! in two.

use std::collections::BTreeSet;

use solidcraft_develop::{Development, Rigid2, Rigid3, Surface, crossings, develop};
use solidcraft_geom::brep::query::Query;
use solidcraft_geom::{Plane, Region2, Seg2};
use solidcraft_kernel::{Body, BoolOp, FaceSurface, face_surface, planar_regions_boolean};

use crate::sheet_surface_shadow::{Slab, around, shadow, walls as made, within};
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
/// `z`, `t` deep), within its region `own`.
fn on_panel(b: &Body, q: &Query, frame: &Rigid3, t: f64, own: &Region2, tol: f64) -> Result<OnPanel> {
    let mut out = OnPanel::default();
    for f in 0..q.record().faces.len() {
        let Some(FaceSurface::Plane { normal, .. }) = face_surface(b, f) else { continue };
        if normal.cross(frame.z).len() > 1e-9 {
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
        let mine = planar_regions_boolean(&[region], std::slice::from_ref(own), BoolOp::Intersect, tol)?;
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

/// The regions' union, their loops closed within `tol`.
fn union(regions: Vec<Region2>, tol: f64) -> Result<Vec<Region2>> {
    let mut it = regions.into_iter();
    let Some(first) = it.next() else { return Ok(Vec::new()) };
    let rest: Vec<Region2> = it.collect();
    Ok(planar_regions_boolean(&[first], &rest, BoolOp::Union, tol)?)
}

fn area(regions: &[Region2]) -> f64 {
    regions.iter().map(Region2::area).sum()
}

fn perimeter(regions: &[Region2]) -> f64 {
    regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).flat_map(|l| &l.segs).map(|s| s.length()).sum()
}

/// Whether region `g` reaches within `near` of the outline of any of `of`.
fn touches(g: &Region2, of: &[Region2], near: f64) -> bool {
    let pts = g.outer.polyline(near.max(1e-6));
    of.iter().flat_map(|r| &r.outer.segs).any(|s| (&pts).into_iter().any(|p| s.point_at(s.closest_param(*p).clamp(0.0, 1.0)).dist(*p) <= near))
}

/// A panel's frame in its body: where the fold puts it, or in the flat, where the development
/// lays its chart in the plane of the first panel's face.
fn frames(sk: &SurfaceSheet, dev: &Development) -> Result<Vec<Option<Rigid3>>> {
    if !sk.unfolded {
        return Ok(crate::sheet_surface_convert::fold_onto(&sk.sheet, dev, &sk.places)?.places);
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
pub(crate) fn absorb(before: &ModelState, after: &ModelState, f: &Feature) -> Result<Vec<(usize, SurfaceSheet, Body)>> {
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
        let (Some(was), Some(now)) = (before.body(&sk.body), after.body(&sk.body)) else { continue };
        if was.body.same_shape(&now.body) {
            continue;
        }
        let (was, now) = (was.body.clone(), now.body.clone());
        let (sk, built) = cut(&sk, &was, &now).map_err(|e| DocError::Invalid(format!("`{}`: what {} cut {e}", sk.body, f.name)))?;
        out.push((i, sk, built));
    }
    Ok(out)
}

/// The sheet `sk` with what was cut from its body, `was` before and `now` after, read onto its
/// flat panels, and its body built again, which must weigh what `now` does.
pub(crate) fn cut(sk: &SurfaceSheet, was: &Body, now: &Body) -> std::result::Result<(SurfaceSheet, Body), String> {
    let walls = made(was, now, tolerance(was));
    let (mut out, read) = onto_panels(sk, was, now, Some(&walls))?;
    let built = out.build().map_err(|e| e.to_string())?;
    // Nothing else may have been cut: a bend's material, or anything the panels' faces don't
    // show. A shadow takes more than the cut did, and all the cut made lies in panels read.
    let weigh = |b: &Body| solidcraft_kernel::measure_mass(b).map(|m| m.volume).map_err(|e| e.to_string());
    let (v, w) = (weigh(&built)?, weigh(now)?);
    if read.shadowed {
        if walls.iter().any(|f| !read.claimed.contains(f)) {
            return Err("can't be kept in the sheet yet: it reaches a bend or a seam".into());
        }
        if v > w + 1e-6 * w.abs().max(1.0) {
            return Err(format!("can't be kept in the sheet yet: its shadow keeps {:.4} mm³ the cut took", v - w));
        }
    } else if (v - w).abs() > 1e-6 * w.abs().max(1.0) {
        return Err(format!("can't be kept in the sheet yet: it reaches a bend or a seam ({:.4} mm³ the sheet's panels don't show)", (v - w).abs()));
    }
    Ok((out, built))
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

/// What [`onto_panels`] read besides the sheet: whether it cut a panel's shadow, and the faces
/// the cut made that lie in the panels it read.
#[derive(Default)]
struct Read {
    shadowed: bool,
    claimed: BTreeSet<usize>,
}

/// What is left of a panel's `region` named `name` without `through`: one piece. What goes
/// through clear of the region's outline and holes is a hole of it as it is, its curves kept.
fn left(region: &Region2, through: &[Region2], name: &str, tol: f64) -> std::result::Result<Region2, String> {
    let loops = || std::iter::once(&region.outer).chain(&region.holes);
    let clear = |t: &Region2| {
        let p = t.outer.segs.first().map(Seg2::start);
        t.holes.is_empty()
            && p.is_some_and(|p| region.contains(p))
            && region.holes.iter().all(|h| h.segs.first().is_none_or(|s| !t.outer.contains(s.start())))
            && loops().all(|l| crossings(&t.outer.segs, &l.segs).is_empty())
    };
    if !through.is_empty() && through.iter().all(clear) {
        let mut out = region.clone();
        out.holes.extend(through.iter().map(|t| t.outer.ccw().reversed()));
        return Ok(out);
    }
    let left = planar_regions_boolean(std::slice::from_ref(region), through, BoolOp::Cut, tol).map_err(|e| e.to_string())?;
    match <[Region2; 1]>::try_from(left) {
        Ok([one]) => Ok(one),
        Err(_) => Err(format!("cuts {name} apart: not supported yet")),
    }
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
    for (p, (panel, frame)) in sk.sheet.panels.iter().zip(&frames).enumerate() {
        let Some(frame) = frame else { continue };
        let plane = matches!(panel.surface, Surface::Plane);
        // Unfolded, a roll lies flat: its slab is not its surface's.
        if sk.unfolded && !plane {
            continue;
        }
        let slab = Slab::new(&panel.surface, *frame, t);
        let here = walls.map(|w| within(&qb, &slab, &panel.region, w, tol * 100.0, sag)).unwrap_or_default();
        taken.claimed.extend(here.iter().copied());
        if !plane {
            // A roll: what went through it is its walls' shadow.
            if here.is_empty() {
                continue;
            }
            let through = shadow(now, &qb, &slab, &here, &panel.name, around(&panel.region), tol, sag)?;
            taken.shadowed = true;
            let kept = left(&panel.region, &through, &panel.name, tol)?;
            if let Some(x) = out.sheet.panels.get_mut(p) {
                x.region = kept;
            }
            continue;
        }
        let read = |b: &Body, q: &Query| on_panel(b, q, frame, t, &panel.region, tol).map_err(|e| e.to_string());
        let (before, after) = (read(was, &qa)?, read(now, &qb)?);
        // The panel's flat material before the cut, and each face's after it (its own face, or
        // a floor seen from it).
        let all = |o: &OnPanel| {
            o.near.iter().chain(&o.far).cloned().chain(o.near_floors.iter().chain(&o.far_floors).map(|x| x.1.clone())).collect::<Vec<_>>()
        };
        let footprint = union(all(&before), tol).map_err(|e| e.to_string())?;
        let seen = |own: &[Region2], floors: &[(f64, Region2)]| union(own.iter().cloned().chain(floors.iter().map(|x| x.1.clone())).collect(), tol);
        // Within the flat material only: past it (toward a sharp corner the body was drawn with)
        // the two faces of a bend's panels end apart.
        let within = |r: Vec<Region2>| planar_regions_boolean(&r, &footprint, BoolOp::Intersect, tol).map_err(|e| e.to_string());
        let near = within(seen(&after.near, &after.near_floors).map_err(|e| e.to_string())?)?;
        let far = within(seen(&after.far, &after.far_floors).map_err(|e| e.to_string())?)?;
        // Square walls: each face sees material wherever the other does.
        let apart = |a: &[Region2], b: &[Region2]| planar_regions_boolean(a, b, BoolOp::Cut, tol).map(|r| area(&r)).map_err(|e| e.to_string());
        let slack = tol * 10.0 * perimeter(&footprint).max(1.0);
        let off_outline = |a: &[Region2], b: &[Region2]| -> std::result::Result<f64, String> {
            let gaps = planar_regions_boolean(a, b, BoolOp::Cut, tol).map_err(|e| e.to_string())?;
            Ok(gaps.iter().filter(|g| strict || !touches(g, &footprint, tol * 100.0)).map(Region2::area).sum())
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
        let square = off_outline(&near, &far)? <= slack && off_outline(&far, &near)? <= slack;
        // Through: the flat material no face sees any longer, or, where the walls are not
        // square, their shadow.
        let through = if square {
            planar_regions_boolean(&footprint, &near, BoolOp::Cut, tol).map_err(|e| e.to_string())?
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
            let kept = left(&panel.region, &through, &panel.name, tol)?;
            if let Some(x) = out.sheet.panels.get_mut(p) {
                x.region = kept;
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
