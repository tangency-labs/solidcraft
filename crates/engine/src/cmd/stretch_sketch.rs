//! Stretch by editing the sketch (#20512): when the body was made by an Extrude of a sketch,
//! a stretch along the sketch's plane grows the sketch's dimensions that span the cut, and a
//! stretch along the Extrude's direction grows its depth, so the part rebuilds from its own
//! sketch with no Stretch step. Each changed value is a number grown by the distance, or a
//! parameter it names, changed the same way. Whatever does not fit is refused here, with why,
//! and the command adds a Stretch step instead.

use std::collections::HashSet;

use solidcore_ops::stretch::{self, Lies, SketchPlane};
use solidcore_record::{Vec2, Vec3};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{Document, FeatureKind, ModelState, StretchDirection};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch};

use crate::Session;

/// One axis of a stretch, worked out: the way the moving side goes, the cut's level along it
/// (world, in the body's frame) and the distance.
pub(super) struct Planned {
    pub direction: StretchDirection,
    /// A point on the cut plane.
    pub cut: Vec3,
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
    let (related, sketches) = depends_on(s, state, body, made_by, feature.component, base_sketch);
    let mut doc: Document = (*s.doc).clone();
    let mut out: Vec<(String, String)> = Vec::new();
    // Sketch points already moved by an earlier axis: a later axis adds to where they are now.
    let mut moved: HashSet<(u64, usize)> = HashSet::new();
    for p in planned {
        let n = arr(p.direction.vector());
        let cut = arr(p.cut);
        let lies = stretch::plane_lies(arr(normal), n);
        match lies {
            // Along the Extrude: its depth grows, when the side that moves is the far cap.
            Lies::Along(with_normal) => {
                let forward = match extent.direction {
                    solidcraft_doc::Direction::Positive => with_normal,
                    solidcraft_doc::Direction::Negative => !with_normal,
                    solidcraft_doc::Direction::Symmetric => return Err("the Extrude is symmetric".into()),
                };
                if !forward {
                    return Err("the side that moves is the sketch's own".into());
                }
                let name = input_param(feature, "Distance").ok_or("the Extrude's depth has no parameter")?;
                out.push(grown(s, &exprs, &name, p.distance)?);
            }
            Lies::InPlane => {}
            Lies::Oblique => return Err("the direction is neither in the sketch's plane nor along the Extrude".into()),
        }
        let mut base_spanned = false;
        for f in &mut doc.features {
            // Faces later features picked beyond the cut move with the part. Other bodies and
            // components, and the sketches only they use, stay as they are.
            if f.id > made_by && related.contains(&f.id) && !matches!(f.kind, FeatureKind::Sketch { .. }) {
                for point in f.kind.face_points_mut() {
                    // A picked point is an axis of no length: beyond the cut it moves with the part.
                    let (moved, ..) = stretch::moved_axis(arr(*point), n, 0.0, 0.0, cut, n, p.distance);
                    *point = Vec3::new(moved[0], moved[1], moved[2]);
                }
            }
            if !sketches.contains(&f.id) {
                continue;
            }
            let FeatureKind::Sketch { sketch, .. } = &mut f.kind else { continue };
            let Some(solved) = state.sketch(f.id) else { continue };
            let plane = solved.plane;
            if stretch::plane_lies(arr(plane.normal()), n) != Lies::InPlane {
                continue;
            }
            let on = SketchPlane { origin: arr(plane.origin), x_axis: arr(plane.x), y_axis: arr(plane.y) };
            let spanning = spanning_dimensions(&solved.sketch, &on, cut, n);
            if f.id == base_sketch && !spanning.is_empty() {
                base_spanned = true;
            }
            for name in spanning {
                out.push(grown(s, &exprs, &name, p.distance)?);
            }
            // Which side of the cut a point is on comes from the solved sketch; where it goes
            // adds this axis's move to any move an earlier axis made.
            for (i, (point, now)) in sketch.points.iter_mut().zip(&solved.sketch.points).enumerate() {
                if point.link.is_none() && on.past_cut(xy(now.pos), cut, n) > 0.0 {
                    let from = if moved.insert((f.id, i)) { now.pos } else { point.pos };
                    let [x, y] = on.moved(xy(from), n, p.distance);
                    point.pos = Vec2::new(x, y);
                }
            }
        }
        if lies == Lies::InPlane && !base_spanned {
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

/// The features `body` depends on after the one that made it, and the sketches they and that
/// feature are built from. A feature belongs to the body when it is in the body's component and
/// names it (a target, a face it picks), or names no body and makes none of its own (a join or
/// cut on every body it meets, a pattern). Features that make or work on other bodies do not.
fn depends_on(s: &Session, state: &ModelState, body: &str, made_by: u64, component: u64, base_sketch: u64) -> (HashSet<u64>, HashSet<u64>) {
    let mut related = HashSet::from([made_by]);
    let mut sketches = HashSet::from([base_sketch]);
    for f in s.doc.features.iter().filter(|f| f.id > made_by && f.component == component) {
        if matches!(f.kind, FeatureKind::Sketch { .. }) {
            continue;
        }
        let named = f.kind.input_bodies();
        let names_ours = named.iter().any(|b| b == body) || f.kind.face_body() == Some(body);
        let anonymous = named.is_empty() && f.kind.face_body().is_none() && !state.bodies.iter().any(|b| b.feature == f.id);
        if names_ours || anonymous {
            related.insert(f.id);
            sketches.extend(f.kind.input_sketches());
        }
    }
    (related, sketches)
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

/// The parameters of the driving dimensions of `sketch` (drawn `on` its plane) that measure
/// across the cut through `cut` along `n`: two points on either side, measured along `n`.
fn spanning_dimensions(sketch: &Sketch, on: &SketchPlane, cut: [f64; 3], n: [f64; 3]) -> Vec<String> {
    let point = |i: usize| sketch.points.get(i).map(|p| xy(p.pos));
    let side = |i: usize| point(i).map(|p| on.past_cut(p, cut, n));
    let line = |l: usize| match sketch.curves.get(l).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => Some((*a, *b)),
        _ => None,
    };
    let runs = |a: usize, b: usize| match (point(a), point(b)) {
        (Some(pa), Some(pb)) => on.runs(pa, pb, n),
        _ => None,
    };
    let across = |a: usize, b: usize| matches!((side(a), side(b)), (Some(x), Some(y)) if (x > 0.0) != (y > 0.0) && x != 0.0 && y != 0.0);
    // Whether the stretch runs along the sketch's own x or y axis.
    let ux = on.runs([0.0, 0.0], [1.0, 0.0], n) == Some(true);
    let uy = on.runs([0.0, 0.0], [0.0, 1.0], n) == Some(true);
    let mut out = Vec::new();
    for c in sketch.constraints.iter().filter(|c| !c.driven) {
        let Some(name) = c.param.clone() else { continue };
        let spans = match c.kind {
            ConstraintKind::DistanceX { p, q, .. } => ux && across(p, q),
            ConstraintKind::DistanceY { p, q, .. } => uy && across(p, q),
            ConstraintKind::Distance { p, q, .. } => runs(p, q) == Some(true) && across(p, q),
            ConstraintKind::Length { l, .. } => line(l).is_some_and(|(a, b)| runs(a, b) == Some(true) && across(a, b)),
            ConstraintKind::PointLineDistance { p, l, .. } => line(l).is_some_and(|(a, b)| runs(a, b) == Some(false) && across(p, a)),
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
    state.body(body).is_some_and(|b| solidcraft_kernel::same_stretched_part(&b.body, expected))
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn xy(p: Vec2) -> [f64; 2] {
    [p.x, p.y]
}
