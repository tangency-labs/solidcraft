//! Exact printable strokes from one resolved, explicitly owned sheet sketch.
//!
//! This adapter neither resolves expressions nor solves or publishes a sketch. Its input must
//! come from the drawing's read-only solve boundary. The public resolved type can also be built
//! by callers, so geometry sizes, references and auxiliary families are checked before using
//! the shared geometry helpers. These checks are not a substitute for drawing validation.

use std::collections::BTreeSet;
use std::f64::consts::TAU;

use solidcraft_doc::drawing::SolvedSheetSketch;
use solidcraft_doc::ids;
use solidcraft_geom::paper::{Group, Item, MAX_LINE_MM, MAX_RECORDS, ObjectKind, Scene};
use solidcraft_geom::{EPS, Vec2};
use solidcraft_sketch::{Curve, CurveKind, MAX_CONSTRAINTS, MAX_COORD, MAX_CURVES, MAX_POINTS, MAX_SPLINE_POINTS, Sketch};

use crate::Result;
use crate::params::bad;

/// Printable curves in authored order, in physical paper millimetres. Construction geometry
/// (including infinite construction lines) is excluded independently of editor visibility.
/// Centerline styling is not representable by a paper stroke yet, and is explicitly refused.
/// The caller supplies the paper line width; no drawing or model display scale is applied.
pub fn paper_group(solved: &SolvedSheetSketch, width_mm: f64) -> Result<Group> {
    if !width_mm.is_finite() || width_mm <= 0.0 || width_mm > MAX_LINE_MM {
        return Err(error(solved, None, "paper line width must be finite, above zero and at most 10 mm"));
    }
    let printable = check(solved)?;
    let mut group = Group { object: solved.id, kind: ObjectKind::Sketch, items: Vec::with_capacity(printable.len()) };
    for (index, count) in printable {
        let curve = solved.sketch.curves.get(index).ok_or_else(|| error(solved, None, "printable curve disappeared"))?;
        check_arc_sweep(solved, curve)?;
        let mut path = match curve.kind {
            CurveKind::Line { .. } | CurveKind::Circle { .. } | CurveKind::Arc { .. } => solved.sketch.exact_pieces(index),
            CurveKind::Ellipse { .. } | CurveKind::Spline { .. } | CurveKind::Conic { .. } => {
                solved.sketch.exact_segs(index).ok_or_else(|| error(solved, Some(&curve.id), "exact curve pieces are unavailable"))?
            }
        };
        // The shared control-spline conversion filters unrepresentable Béziers. Require every
        // expected span, rather than allowing a missing piece or a future fallback to disappear.
        if path.len() != count || path.is_empty() {
            return Err(error(solved, Some(&curve.id), "exact conversion did not retain every curve piece"));
        }
        if let CurveKind::Arc { b, .. } = curve.kind {
            let end = solved.sketch.point(b).ok_or_else(|| error(solved, Some(&curve.id), "arc references an unknown end point"))?;
            if !path.last().is_some_and(|piece| piece.end().dist(end) <= EPS) {
                return Err(error(solved, Some(&curve.id), "analytic arc does not retain its authored end within the paper endpoint tolerance"));
            }
        }
        if curve.reversed {
            path.reverse();
            path.iter_mut().for_each(|piece| *piece = piece.reversed());
        }
        let same_ends = || curve.kind.ends().and_then(|(a, b)| solved.sketch.point(a).zip(solved.sketch.point(b))).is_some_and(|(a, b)| a == b);
        let closed = match curve.kind {
            CurveKind::Line { .. } => false,
            CurveKind::Circle { .. } | CurveKind::Ellipse { .. } => true,
            CurveKind::Spline { periodic, .. } => periodic || same_ends(),
            CurveKind::Arc { .. } | CurveKind::Conic { .. } => same_ends(),
        };
        let item = Item::Stroke { path, closed, width_mm, pattern: None };
        // Validate each curve separately so range, degeneracy and continuity errors name its
        // local identity. Moving its checked items out avoids cloning its exact geometry.
        let mut alone = Scene::new(Vec2::new(1.0, 1.0), vec![Group { object: solved.id, kind: ObjectKind::Sketch, items: vec![item] }]);
        alone.validate().map_err(|why| error(solved, Some(&curve.id), why.to_string()))?;
        let checked = alone.groups.pop().ok_or_else(|| error(solved, Some(&curve.id), "checked curve disappeared"))?;
        group.items.extend(checked.items);
    }
    // Paper has no public group-validation API. A legal dummy sheet checks this standalone
    // group without translating, scaling, clipping or otherwise changing its paper geometry.
    let mut scene = Scene::new(Vec2::new(1.0, 1.0), vec![group]);
    scene.validate().map_err(|why| error(solved, None, why.to_string()))?;
    scene.groups.pop().ok_or_else(|| error(solved, None, "checked sketch disappeared"))
}

/// Refuse the tiny nonzero sweep that the shared analytic helper widens to a full turn.
/// Exact coincident endpoints still intentionally describe a complete circle. The check is
/// only reached for printable curves, after their points and references have been checked.
fn check_arc_sweep(solved: &SolvedSheetSketch, curve: &Curve) -> Result<()> {
    let CurveKind::Arc { c, a, b } = curve.kind else { return Ok(()) };
    let point = |index| solved.sketch.point(index).ok_or_else(|| error(solved, Some(&curve.id), "arc references an unknown point"));
    let (center, start, end) = (point(c)?, point(a)?, point(b)?);
    if start != end {
        let sweep = ((end - center).angle() - (start - center).angle()).rem_euclid(TAU);
        // Sketch::segs adds a full turn at this cutoff, including across atan2's branch seam.
        if sweep <= 1e-12 {
            return Err(error(solved, Some(&curve.id), "noncoincident arc sweep at or below 1e-12 radians cannot be converted exactly"));
        }
    }
    Ok(())
}

fn error(solved: &SolvedSheetSketch, curve: Option<&str>, why: impl std::fmt::Display) -> crate::EngineError {
    let local = curve.map_or_else(String::new, |id| format!(", curve `{id:.512}`"));
    bad(
        super::CMD,
        format!(
            "sheet sketch: drawing {}, sheet {}, sketch {}{local}: {why}",
            ids::encode(solved.drawing),
            ids::encode(solved.sheet),
            ids::encode(solved.id)
        ),
    )
}

fn identity<'a>(solved: &SolvedSheetSketch, ids: &mut BTreeSet<&'a str>, id: &'a str) -> Result<()> {
    if id.len() > 512 || id.trim().is_empty() || id.chars().any(char::is_control) || !ids.insert(id) {
        return Err(error(solved, Some(id), "local identity is empty, excessive, contains controls or is duplicated"));
    }
    Ok(())
}

/// Bound all input-dependent work and output records before allocating exact curve pieces.
fn check(solved: &SolvedSheetSketch) -> Result<Vec<(usize, usize)>> {
    let sk = &solved.sketch;
    if solved.drawing == 0 || solved.sheet == 0 || solved.id == 0 {
        return Err(error(solved, None, "drawing, sheet and sketch identities must be nonzero"));
    }
    if !solved.report.keeps_orientation() {
        return Err(error(solved, None, "the sheet sketch has no accepted solved state"));
    }
    if sk.points.len() > MAX_POINTS || sk.curves.len() > MAX_CURVES || sk.constraints.len() > MAX_CONSTRAINTS {
        return Err(error(solved, None, "sheet sketch array limit exceeded"));
    }
    if !sk.links.is_empty()
        || !sk.patterns.is_empty()
        || !sk.offsets.is_empty()
        || !sk.freeform_offsets.is_empty()
        || !sk.wires.is_empty()
        || !sk.equations.is_empty()
    {
        return Err(error(solved, None, "not supported yet: sheet sketch links, patterns, offsets, 3D wires or equation curves"));
    }
    if sk.view.three_d || sk.view.slice {
        return Err(error(solved, None, "model-only sketch view is not a sheet sketch"));
    }
    let origin = sk.points.first().ok_or_else(|| error(solved, None, "missing fixed local origin"))?;
    if origin.id != "origin" || origin.pos != Vec2::ZERO || !origin.fixed || origin.link.is_some() || origin.exprs.is_some() {
        return Err(error(solved, None, "point zero must be the fixed local origin"));
    }
    let mut ids = BTreeSet::new();
    for point in &sk.points {
        identity(solved, &mut ids, &point.id)?;
        if !point.pos.is_finite() || point.pos.x.abs() >= MAX_COORD || point.pos.y.abs() >= MAX_COORD || point.link.is_some() {
            return Err(error(solved, Some(&point.id), "point is nonfinite, out of range or linked"));
        }
    }
    for constraint in &sk.constraints {
        identity(solved, &mut ids, &constraint.id)?;
        if constraint.kind.value().is_some_and(|value| !value.is_finite()) || constraint.text.is_some_and(|at| !at.is_finite()) {
            return Err(error(solved, Some(&constraint.id), "constraint has a nonfinite value or label position"));
        }
    }
    let mut entities = sk.points.len().saturating_add(sk.curves.len()).saturating_add(sk.constraints.len());
    let mut records = 1usize; // The group itself, plus one item and its pieces per printable curve.
    let mut printable = Vec::new();
    for (index, curve) in sk.curves.iter().enumerate() {
        identity(solved, &mut ids, &curve.id)?;
        if curve.link.is_some() {
            return Err(error(solved, Some(&curve.id), "not supported yet: linked sheet sketch curve"));
        }
        if curve.centerline {
            return Err(error(solved, Some(&curve.id), "not supported yet: paper centerline styling"));
        }
        if curve.infinite && !(curve.construction && matches!(curve.kind, CurveKind::Line { .. })) {
            return Err(error(solved, Some(&curve.id), "only a construction line can be infinite"));
        }
        let count = pieces(solved, curve, sk, &mut entities)?;
        if entities > MAX_POINTS + MAX_CURVES + MAX_CONSTRAINTS {
            return Err(error(solved, Some(&curve.id), "sheet sketch entity limit exceeded"));
        }
        if curve.construction {
            continue;
        }
        if let CurveKind::Conic { rho, .. } = curve.kind
            && 1.0 - rho < 1e-9
        {
            return Err(error(solved, Some(&curve.id), "conic rho is too close to one for exact paper conversion"));
        }
        records = records.saturating_add(1).saturating_add(count);
        if records > MAX_RECORDS {
            return Err(error(solved, Some(&curve.id), format!("exact strokes exceed the paper scene record limit of {MAX_RECORDS}")));
        }
        printable.push((index, count));
    }
    Ok(printable)
}

fn pieces(solved: &SolvedSheetSketch, curve: &Curve, sk: &Sketch, entities: &mut usize) -> Result<usize> {
    let fail = |why| error(solved, Some(&curve.id), why);
    if let CurveKind::Spline { pts, degree, control, periodic } = &curve.kind {
        if !(2..=MAX_SPLINE_POINTS).contains(&pts.len()) || !(1..=5).contains(degree) || (*control && pts.len() <= usize::from(*degree)) {
            return Err(fail("invalid sheet spline size or degree"));
        }
        if *periodic && (*control || *degree != 3 || pts.len() < 4) {
            return Err(fail("periodic sheet spline must be a cubic fit spline"));
        }
        *entities = entities.saturating_add(pts.len());
    }
    if curve.kind.point_ids().iter().any(|&index| sk.points.get(index).is_none()) {
        return Err(fail("curve references an unknown point"));
    }
    match &curve.kind {
        CurveKind::Line { a, b } => {
            if a == b {
                return Err(fail("line needs distinct point identities"));
            }
            Ok(1)
        }
        CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. } => {
            if !r.is_finite() || *r <= 0.0 || *r >= MAX_COORD {
                return Err(fail("curve radius is nonfinite or out of range"));
            }
            Ok(if matches!(curve.kind, CurveKind::Ellipse { .. }) { 4 } else { 2 })
        }
        CurveKind::Arc { .. } => Ok(1),
        CurveKind::Conic { rho, .. } => {
            if !rho.is_finite() || *rho <= 0.0 || *rho >= 1.0 {
                return Err(fail("conic rho must be finite and between zero and one"));
            }
            Ok(1)
        }
        CurveKind::Spline { pts, degree, control, .. } => Ok(pts.len() - if *control { usize::from(*degree) } else { 1 }),
    }
}
