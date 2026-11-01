//! The timeline: each modelling feature with its inputs evaluated, the key dimensions of the
//! kinds analysis tools reason about, the sketch profiles it uses, its copies' transforms and
//! the bodies it built.

use std::collections::{BTreeMap, BTreeSet};

use solidcraft_doc::expr::Kind;
use solidcraft_doc::{Document, FeatureKind as DocKind, ModelState, Operation as DocOperation, SolvedSketch, plain};
use solidcraft_geom::{Seg2, Vec3};

use crate::schema::{
    Body, BodyName, Direction, Feature, FeatureId, FeatureKind, Frame, Input, Mm, Operation, Point, Point2, Profile, Quantity, Radians, Sketch,
    SketchCurve, Transform,
};

/// Arcs whose centres and radii agree within this (mm) are of one circle.
const SAME_ARC: f64 = 1e-9;
/// Arcs that turn a full circle within this (radians) close it.
const FULL_TURN: f64 = 1e-6;
/// Most features listed (a design's timeline is capped far above any real part).
pub const MAX_FEATURES: usize = 100_000;

/// The features a persistent face name mentions (`F3:inner:F1:+y` → 3, 1), in text order: by
/// the name grammar, or the `F<n>` tokens of a name it does not read.
pub(crate) fn feature_ids(name: &str) -> Vec<FeatureId> {
    use solidcraft_doc::persistent_name::{Base, FaceName};
    match FaceName::parse(name).ok().filter(|f| !matches!(f.base, Base::Other(_))) {
        Some(f) => f.features().into_iter().map(|id| FeatureId(id.0)).collect(),
        None => feature_tokens(name).into_iter().map(FeatureId).collect(),
    }
}

/// The `F<n>` tokens of `name` not inside a word.
fn feature_tokens(name: &str) -> Vec<u64> {
    let bytes = name.as_bytes();
    (0..bytes.len())
        .filter_map(|i| {
            let starts = bytes.get(i) == Some(&b'F') && (i == 0 || !bytes.get(i - 1).is_some_and(|b| b.is_ascii_alphanumeric()));
            if !starts {
                return None;
            }
            let digits: &str = name.get(i + 1..)?.split(|c: char| !c.is_ascii_digit()).next()?;
            if digits.is_empty() { None } else { digits.parse().ok() }
        })
        .collect()
}

fn operation(op: DocOperation) -> Operation {
    match op {
        DocOperation::NewBody => Operation::NewBody,
        DocOperation::Join => Operation::Join,
        DocOperation::Cut => Operation::Cut,
        DocOperation::Intersect => Operation::Intersect,
    }
}

fn point(v: Vec3) -> Point {
    Point([v.x + 0.0, v.y + 0.0, v.z + 0.0])
}

fn direction(v: Vec3) -> Option<Direction> {
    Direction::new([v.x, v.y, v.z])
}

fn names(bodies: &[String]) -> Vec<BodyName> {
    bodies.iter().map(|b| BodyName(b.clone())).collect()
}

fn curves(segs: &[Seg2]) -> Vec<SketchCurve> {
    // A loop of arcs about one centre and radius that turns a full circle is a circle.
    let arcs: Vec<(solidcraft_geom::Vec2, f64, f64)> =
        segs.iter().filter_map(|s| if let Seg2::Arc { center, radius, sweep, .. } = s { Some((*center, *radius, *sweep)) } else { None }).collect();
    if let Some(&(c0, r0, _)) = arcs.first()
        && arcs.len() == segs.len()
    {
        let same = arcs.iter().all(|(c, r, _)| (c.x - c0.x).abs() < SAME_ARC && (c.y - c0.y).abs() < SAME_ARC && (r - r0).abs() < SAME_ARC);
        let turn: f64 = arcs.iter().map(|a| a.2).sum();
        if same && (turn.abs() - std::f64::consts::TAU).abs() < FULL_TURN {
            return vec![SketchCurve::Circle { center: Point2([c0.x, c0.y]), radius: Mm(r0) }];
        }
    }
    segs.iter()
        .map(|s| match s {
            Seg2::Line { a, b } => SketchCurve::Line { a: Point2([a.x, a.y]), b: Point2([b.x, b.y]) },
            Seg2::Arc { center, radius, start, sweep } => {
                SketchCurve::Arc { center: Point2([center.x, center.y]), radius: Mm(*radius), start: Radians(*start), sweep: Radians(*sweep) }
            }
            _ => SketchCurve::Other,
        })
        .collect()
}

fn sketch(ss: &SolvedSketch, sel: &solidcraft_doc::ProfileSel) -> Sketch {
    let indices = solidcraft_doc::profile_selection_indices(ss, sel).unwrap_or_else(|_| (0..ss.profiles.len()).collect());
    let profiles = indices
        .into_iter()
        .filter_map(|i| {
            let p = ss.profiles.get(i)?;
            Some(Profile {
                index: u32::try_from(i).ok()?,
                area: p.area,
                outer: curves(&p.region.outer.segs),
                holes: p.region.holes.iter().map(|h| curves(&h.segs)).collect(),
            })
        })
        .collect();
    let frame = Frame {
        origin: point(ss.plane.origin),
        x: direction(ss.plane.x).unwrap_or(Direction::X),
        y: direction(ss.plane.y).unwrap_or(Direction::Y),
        normal: direction(ss.plane.normal()).unwrap_or(Direction::Z),
    };
    Sketch { id: FeatureId(ss.feature), name: ss.name.clone(), frame, profiles }
}

/// Whether a feature only builds reference geometry (no faces of a body).
fn reference_only(kind: &DocKind) -> bool {
    matches!(kind, DocKind::Sketch { .. } | DocKind::ConstructionPlane { .. } | DocKind::ConstructionAxis { .. } | DocKind::ConstructionPoint { .. })
}

/// The timeline's modelling features, described against the snapshot's `bodies`.
pub(crate) fn features(doc: &Document, state: &ModelState, bodies: &[Body]) -> Vec<Feature> {
    // The bodies each feature made faces of.
    let mut made: BTreeMap<FeatureId, BTreeSet<&BodyName>> = BTreeMap::new();
    for b in bodies {
        for f in &b.faces {
            for id in &f.features {
                made.entry(*id).or_default().insert(&b.name);
            }
        }
    }
    let (vals, _) = doc.param_values();
    let eval = |e: &str, k: Kind| Document::eval_in(&vals, e, k).ok().filter(|v| v.is_finite());
    let at = |p: &[solidcraft_doc::Expr; 3]| Some(Point([eval(&p[0], Kind::Length)?, eval(&p[1], Kind::Length)?, eval(&p[2], Kind::Length)?]));
    let solved = |id: solidcraft_doc::SketchId| state.sketch(*id);
    doc.features
        .iter()
        .filter(|f| !reference_only(&f.kind))
        .take(MAX_FEATURES)
        .map(|f| {
            let inputs: Vec<Input> = f
                .kind
                .inputs()
                .into_iter()
                .filter_map(|(label, e, k)| Some(Input { label: label.to_string(), value: super::quantity(k, eval(&e, k)?) }))
                .collect();
            // The first input of this label that is a length, or an angle.
            let length = |label: &str| {
                inputs.iter().find_map(|i| match i.value {
                    Quantity::Length(l) if i.label == label => Some(l),
                    _ => None,
                })
            };
            let angle = |label: &str| {
                inputs.iter().find_map(|i| match i.value {
                    Quantity::Angle(a) if i.label == label => Some(a),
                    _ => None,
                })
            };
            let kind = match &f.kind {
                DocKind::Extrude { sketch: id, profiles, extent, operation: op, targets, .. } => {
                    let ss = solved(*id);
                    let negative = matches!(extent.direction, solidcraft_doc::Direction::Negative);
                    FeatureKind::Extrude {
                        operation: operation(*op),
                        direction: ss.and_then(|ss| direction(if negative { -ss.plane.normal() } else { ss.plane.normal() })),
                        sketch: ss.map(|ss| sketch(ss, profiles)),
                        distance: length("Distance"),
                        two_sided: matches!(extent.direction, solidcraft_doc::Direction::Symmetric) || extent.distance2.is_some(),
                        targets: names(&plain(targets)),
                    }
                }
                DocKind::Revolve { sketch: id, profiles, operation: op, targets, .. } => FeatureKind::Revolve {
                    operation: operation(*op),
                    sketch: solved(*id).map(|ss| sketch(ss, profiles)),
                    angle: angle("Angle"),
                    targets: names(&plain(targets)),
                },
                DocKind::Sweep { sketch: id, profiles, operation: op, targets, .. } => FeatureKind::Sweep {
                    operation: operation(*op),
                    sketch: solved(*id).map(|ss| sketch(ss, profiles)),
                    targets: names(&plain(targets)),
                },
                DocKind::Loft { operation: op, targets, .. } => FeatureKind::Loft { operation: operation(*op), targets: names(&plain(targets)) },
                DocKind::Box { corner, operation: op, .. } => FeatureKind::Box {
                    operation: operation(*op),
                    corner: at(corner),
                    size: length("Length").zip(length("Width")).zip(length("Height")).map(|((l, w), h)| [l, w, h]),
                },
                DocKind::Cylinder { base, axis, operation: op, .. } => FeatureKind::Cylinder {
                    operation: operation(*op),
                    base: at(base),
                    axis: direction(**axis),
                    radius: length("Radius"),
                    height: length("Height"),
                },
                DocKind::Sphere { center, operation: op, .. } => {
                    FeatureKind::Sphere { operation: operation(*op), center: at(center), radius: length("Radius") }
                }
                DocKind::Torus { center, operation: op, .. } => FeatureKind::Torus { operation: operation(*op), center: at(center) },
                DocKind::Hole { position, direction: d, .. } => {
                    FeatureKind::Hole { position: point(**position), direction: direction(**d), diameter: length("Diameter"), depth: length("Depth") }
                }
                DocKind::Combine { target, tools, operation: op, .. } => {
                    FeatureKind::Combine { operation: operation(*op), target: BodyName(target.to_string()), tools: names(&plain(tools)) }
                }
                DocKind::Fillet { body, .. } => {
                    FeatureKind::Fillet { bodies: body.iter().map(|b| BodyName(b.to_string())).collect(), radius: length("Radius") }
                }
                DocKind::Chamfer { body, .. } => {
                    FeatureKind::Chamfer { bodies: body.iter().map(|b| BodyName(b.to_string())).collect(), distance: length("Distance") }
                }
                DocKind::Shell { body, .. } => {
                    FeatureKind::Shell { bodies: body.iter().map(|b| BodyName(b.to_string())).collect(), thickness: length("Thickness") }
                }
                DocKind::Split { body, .. } => FeatureKind::Split { body: BodyName(body.to_string()) },
                DocKind::Pattern { pattern, bodies: copied, .. } => FeatureKind::Pattern {
                    bodies: names(&plain(copied)),
                    // SolidCraft's matrices are column-major (`m[3]` the translation).
                    transforms: solidcraft_doc::pattern_matrices(doc, state, pattern)
                        .map(|ms| ms.into_iter().filter(|m| *m != solidcraft_doc::IDENTITY).map(|m| Transform(transpose(m))).collect())
                        .unwrap_or_default(),
                },
                DocKind::Mirror { plane, bodies: copied, .. } => FeatureKind::Mirror {
                    bodies: names(&plain(copied)),
                    transform: doc.resolve_plane_in(&vals, plane, 0, Some(state)).ok().and_then(|p| mirror(p.origin, p.normal())),
                },
                _ => FeatureKind::Other,
            };
            let kind_name =
                serde_json::to_value(&f.kind).ok().and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string)).unwrap_or_default();
            // The bodies it made faces of, and the snapshot's bodies it names.
            let named: Vec<&BodyName> = match &kind {
                FeatureKind::Extrude { targets, .. }
                | FeatureKind::Revolve { targets, .. }
                | FeatureKind::Sweep { targets, .. }
                | FeatureKind::Loft { targets, .. } => targets.iter().collect(),
                FeatureKind::Combine { target, tools, .. } => std::iter::once(target).chain(tools).collect(),
                FeatureKind::Fillet { bodies: b, .. }
                | FeatureKind::Chamfer { bodies: b, .. }
                | FeatureKind::Shell { bodies: b, .. }
                | FeatureKind::Pattern { bodies: b, .. }
                | FeatureKind::Mirror { bodies: b, .. } => b.iter().collect(),
                FeatureKind::Split { body } => vec![body],
                _ => Vec::new(),
            };
            let creates = names(&f.body_names);
            let mut on: BTreeSet<&BodyName> = made.get(&FeatureId(f.id)).cloned().unwrap_or_default();
            on.extend(named);
            on.extend(&creates);
            let bodies_on = bodies.iter().map(|b| &b.name).filter(|n| on.contains(n)).cloned().collect();
            Feature { id: FeatureId(f.id), name: f.name.clone(), kind_name, suppressed: f.suppressed, kind, inputs, creates, bodies: bodies_on }
        })
        .collect()
}

fn transpose(m: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    std::array::from_fn(|i| std::array::from_fn(|j| m[j][i]))
}

/// The reflection in the plane through `o` with normal `n`: x' = x − 2 ((x − o)·n) n.
fn mirror(o: Vec3, n: Vec3) -> Option<Transform> {
    let n = direction(n)?.get();
    let o = [o.x, o.y, o.z];
    let d = 2.0 * (o[0] * n[0] + o[1] * n[1] + o[2] * n[2]);
    let mut m = Transform::default().0;
    for (i, row) in m.iter_mut().take(3).enumerate() {
        for (j, c) in row.iter_mut().take(3).enumerate() {
            *c -= 2.0 * n[i] * n[j];
        }
        row[3] = d * n[i];
    }
    Some(Transform(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_ids_come_from_every_name_part() {
        let ids = |n: &str| feature_ids(n).into_iter().map(|f| f.0).collect::<Vec<_>>();
        assert_eq!(ids("F3:inner:F2:blend:F1:+y|F1:-x"), vec![3, 2, 1, 1]);
        assert_eq!(ids("F44:side:l12"), vec![44]);
        assert_eq!(ids("OFF7"), Vec::<u64>::new());
        assert_eq!(ids("F"), Vec::<u64>::new());
    }

    #[test]
    fn a_mirror_reflects_across_its_plane() {
        let m = mirror(Vec3::new(10.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)).unwrap();
        assert_eq!(m.0, [[-1.0, 0.0, 0.0, 20.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
    }
}
