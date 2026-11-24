//! Stretch by editing the sketch (#20512): when the body was made by an Extrude of a sketch,
//! a stretch along the sketch's plane grows the sketch's dimensions that span the cut, and a
//! stretch along the Extrude's direction grows its depth, so the part rebuilds from its own
//! sketch with no Stretch step. Each changed value is a number grown by the distance, or a
//! parameter it names, changed the same way. Whatever does not fit is refused here, with why,
//! and the command adds a Stretch step instead.

use solidcore_record::Vec2;
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{Document, FeatureKind, ModelState, StretchDirection};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch};

use crate::Session;

/// One axis of a stretch, worked out: the way the moving side goes, the cut's level along it
/// (world, in the body's frame) and the distance.
pub(super) struct Planned {
    pub direction: StretchDirection,
    pub level: f64,
    pub distance: f64,
}

/// The design changed so the sketches and Extrude that built `body` give the stretched part, and
/// the parameter changes to make on it, as `(name, new expression)`; or why they cannot.
///
/// Every sketch whose plane runs along a stretch's direction has the points beyond the cut moved
/// by the distance and the driving dimensions that span the cut grown by it, so later features
/// drawn on those sketches (holes, bosses, cuts) move with the part. Faces picked beyond the cut
/// by later features move too. Along the Extrude's own direction, its depth grows.
pub(super) fn changes(s: &Session, state: &ModelState, body: &str, planned: &[Planned]) -> Result<(Document, Vec<(String, String)>), String> {
    let source = state.body(body).ok_or("the body is gone")?;
    let made_by = source.feature;
    let feature = s.doc.feature(made_by).ok_or("the feature that made the body is gone")?;
    let FeatureKind::Extrude { sketch, extent, thin: None, .. } = &feature.kind else {
        return Err("the body was not made by an Extrude of a sketch".into());
    };
    if extent.distance2.is_some() {
        return Err("the Extrude goes both ways".into());
    }
    let base_sketch = sketch.0;
    let normal = state.sketch(base_sketch).ok_or("the Extrude's sketch is gone")?.plane.normal();
    let exprs = s.doc.all_param_exprs();
    let mut doc: Document = (*s.doc).clone();
    let mut out: Vec<(String, String)> = Vec::new();
    for p in planned {
        let n = p.direction.vector();
        if 1.0 - n.dot(normal).abs() < 1e-9 {
            // Along the Extrude: its depth grows, when the side that moves is the far cap.
            let extrudes_along = match extent.direction {
                solidcraft_doc::Direction::Positive => 1.0,
                solidcraft_doc::Direction::Negative => -1.0,
                solidcraft_doc::Direction::Symmetric => return Err("the Extrude is symmetric".into()),
            };
            if n.dot(normal) * extrudes_along < 0.0 {
                return Err("the side that moves is the sketch's own".into());
            }
            let name = input_param(feature, "Distance").ok_or("the Extrude's depth has no parameter")?;
            out.push(grown(s, &exprs, &name, p.distance)?);
        } else if n.dot(normal).abs() > 1e-9 {
            return Err("the direction is neither in the sketch's plane nor along the Extrude".into());
        }
        let mut base_spanned = false;
        for f in &mut doc.features {
            // Faces later features picked beyond the cut move with the part.
            if f.id > made_by && !matches!(f.kind, FeatureKind::Sketch { .. }) {
                for point in f.kind.face_points_mut() {
                    if point.dot(n) > p.level {
                        *point += n * p.distance;
                    }
                }
            }
            let FeatureKind::Sketch { sketch, .. } = &mut f.kind else { continue };
            let Some(solved) = state.sketch(f.id) else { continue };
            let plane = solved.plane;
            if plane.normal().dot(n).abs() > 1e-9 {
                continue;
            }
            // The direction and the cut in this sketch's coordinates.
            let u = Vec2::new(n.dot(plane.x), n.dot(plane.y));
            let level = p.level - plane.origin.dot(n);
            let spanning = spanning_dimensions(&solved.sketch, u, level);
            if f.id == base_sketch && !spanning.is_empty() {
                base_spanned = true;
            }
            for name in spanning {
                out.push(grown(s, &exprs, &name, p.distance)?);
            }
            for (point, now) in sketch.points.iter_mut().zip(&solved.sketch.points) {
                if point.link.is_none() && now.pos.dot(u) > level {
                    point.pos = now.pos + u * p.distance;
                }
            }
        }
        if 1.0 - n.dot(normal).abs() >= 1e-9 && !base_spanned {
            return Err("no dimension of the sketch spans the cut".into());
        }
    }
    // The same parameter twice (two dimensions that share one) cannot grow twice.
    let mut merged: Vec<(String, String)> = Vec::new();
    for (name, e) in out {
        if merged.iter().any(|(n, _)| *n == name) {
            return Err(format!("`{name}` sizes more than one stretched span"));
        }
        merged.push((name, e));
    }
    Ok((doc, merged))
}

/// The parameter of `feature`'s input labelled `label`.
fn input_param(feature: &solidcraft_doc::Feature, label: &str) -> Option<String> {
    feature.param_names.iter().zip(feature.kind.inputs()).find(|(n, (l, ..))| !n.is_empty() && *l == label).map(|(n, _)| n.clone())
}

/// `name`'s new expression, grown by `by` mm: a plain length gets the sum; an expression that is
/// just another parameter's name grows that parameter instead.
fn grown(s: &Session, exprs: &[(String, String, Kind)], name: &str, by: f64) -> Result<(String, String), String> {
    let (_, e, _) = exprs.iter().find(|(n, ..)| n == name).ok_or_else(|| format!("no parameter `{name}`"))?;
    let e = e.trim();
    if exprs.iter().any(|(n, ..)| n == e) && e != name {
        return grown(s, exprs, e, by);
    }
    if !plain(e) {
        return Err(format!("`{name}` is the formula `{e}`, which Stretch does not rewrite"));
    }
    let value = s.doc.eval(e, Kind::Length).map_err(|err| format!("`{name}`: {err}"))?;
    let next = value + by;
    if !(next > 0.0) {
        return Err(format!("`{name}` would be {next} mm"));
    }
    Ok((name.to_string(), format!("{} mm", (next * 1e6).round() / 1e6)))
}

/// A number, alone or with a length unit after it.
fn plain(e: &str) -> bool {
    let number = e.trim_end_matches(|c: char| c.is_ascii_alphabetic()).trim();
    let unit = e.get(number.len()..).unwrap_or("").trim();
    !number.is_empty() && number.parse::<f64>().is_ok_and(f64::is_finite) && matches!(unit, "" | "mm" | "cm" | "m" | "in" | "ft")
}

/// The parameters of the driving dimensions of `sketch` that measure across the cut at `level`
/// along unit `u` (sketch coordinates): two points on either side, measured along `u`.
fn spanning_dimensions(sketch: &Sketch, u: Vec2, level: f64) -> Vec<String> {
    let at = |i: usize| sketch.points.get(i).map(|p| p.pos.dot(u) - level);
    let line = |l: usize| match sketch.curves.get(l).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => Some((*a, *b)),
        _ => None,
    };
    let along = |a: usize, b: usize| -> bool {
        let (Some(pa), Some(pb)) = (sketch.points.get(a), sketch.points.get(b)) else { return false };
        (pb.pos - pa.pos).normalized().is_some_and(|d| d.dot(u).abs() > 1.0 - 1e-9)
    };
    let across = |a: usize, b: usize| matches!((at(a), at(b)), (Some(x), Some(y)) if x * y < 0.0);
    let ux = u.x.abs() > 1.0 - 1e-9;
    let uy = u.y.abs() > 1.0 - 1e-9;
    let mut out = Vec::new();
    for c in sketch.constraints.iter().filter(|c| !c.driven) {
        let Some(name) = c.param.clone() else { continue };
        let spans = match c.kind {
            ConstraintKind::DistanceX { p, q, .. } => ux && across(p, q),
            ConstraintKind::DistanceY { p, q, .. } => uy && across(p, q),
            ConstraintKind::Distance { p, q, .. } => along(p, q) && across(p, q),
            ConstraintKind::Length { l, .. } => line(l).is_some_and(|(a, b)| along(a, b) && across(a, b)),
            ConstraintKind::PointLineDistance { p, l, .. } => line(l).is_some_and(|(a, b)| {
                let square = matches!((sketch.points.get(a), sketch.points.get(b)), (Some(pa), Some(pb))
                    if (pb.pos - pa.pos).normalized().is_some_and(|d| d.dot(u).abs() < 1e-9));
                square && across(p, a)
            }),
            _ => false,
        };
        if spans {
            out.push(name);
        }
    }
    out
}

/// Does `body` in `state` have the box and volume `expected` has (within a millionth of its
/// size)? The check that the edited sketch made the part the stretch makes.
pub(super) fn same_part(state: &ModelState, body: &str, expected: &solidcraft_kernel::Body) -> bool {
    let Some(b) = state.body(body) else { return false };
    let (Ok(got), Ok(want)) = (solidcraft_kernel::measure(&b.body), solidcraft_kernel::measure(expected)) else { return false };
    let size = want.bbox.max.dist(want.bbox.min).max(1.0);
    let corners = got.bbox.min.dist(want.bbox.min) <= size * 1e-6 && got.bbox.max.dist(want.bbox.max) <= size * 1e-6;
    corners && (got.volume - want.volume).abs() <= want.volume.abs() * 1e-6
}
