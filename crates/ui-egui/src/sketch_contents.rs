//! A sketch's contents as rows a browser can show: its points and curves, the constraints and
//! dimensions that drive them, and the patterns, offsets, links, text and 3D curves stored beside
//! them.
//!
//! The model is built from the stored [`Sketch`] and, when it has been evaluated, the solved one,
//! so it costs no more than a walk over the sketch. It is pure data: no egui, no selection and no
//! command is involved; the browser decides what a row does.

// TODO(stage 2): remove when the browser uses these
#![allow(dead_code)]

use solidcraft_engine::{
    doc::{Document, ModelState},
    geom::Seg2,
    sketch::{Constraint, ConstraintKind, CurveKind, LinkKind, LinkSource, Sketch, SketchPatternKind},
};

/// Everything the browser shows below one sketch row.
///
/// `editable` says the sketch is the active one; `evaluated` says it has a solved sketch in the
/// state it was built from (an inactive or rolled-back sketch may not). Groups are in
/// [`GroupKind`] order and only non-empty ones are present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SketchContents {
    pub sketch: u64,
    pub editable: bool,
    pub evaluated: bool,
    pub groups: Vec<ContentsGroup>,
}

/// One group of contents rows, e.g. all the lines of a sketch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContentsGroup {
    pub kind: GroupKind,
    pub rows: Vec<ContentsRow>,
}

/// The groups a sketch's contents are split into, in the order the browser shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroupKind {
    Points,
    Lines,
    Arcs,
    Circles,
    Splines,
    Conics,
    Constraints,
    Dimensions,
    Patterns,
    Offsets,
    Links,
    Text,
    Other,
}

impl GroupKind {
    /// Every group, in display order.
    pub(crate) const ALL: [GroupKind; 13] = [
        GroupKind::Points,
        GroupKind::Lines,
        GroupKind::Arcs,
        GroupKind::Circles,
        GroupKind::Splines,
        GroupKind::Conics,
        GroupKind::Constraints,
        GroupKind::Dimensions,
        GroupKind::Patterns,
        GroupKind::Offsets,
        GroupKind::Links,
        GroupKind::Text,
        GroupKind::Other,
    ];

    /// Position in [`GroupKind::ALL`], for bucketing rows in display order.
    fn index(self) -> usize {
        match self {
            GroupKind::Points => 0,
            GroupKind::Lines => 1,
            GroupKind::Arcs => 2,
            GroupKind::Circles => 3,
            GroupKind::Splines => 4,
            GroupKind::Conics => 5,
            GroupKind::Constraints => 6,
            GroupKind::Dimensions => 7,
            GroupKind::Patterns => 8,
            GroupKind::Offsets => 9,
            GroupKind::Links => 10,
            GroupKind::Text => 11,
            GroupKind::Other => 12,
        }
    }
}

/// What kind of entity a row is, for its icon and its actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowKind {
    Point,
    Line,
    Arc,
    Circle,
    Spline,
    Conic,
    Constraint,
    Dimension,
    Pattern,
    Offset,
    Link,
    Text,
    Other,
}

/// One row below a sketch: an entity id, what it is, its label and a short summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContentsRow {
    /// The sketch entity's id (a sketch pattern's id, a link's id, a wire's id).
    pub id: String,
    pub kind: RowKind,
    /// The type word and the id, e.g. `Line l3` (the browser translates the word in stage 7).
    pub label: String,
    /// A short, unlocalised detail: a length, a radius, a value, a source.
    pub summary: String,
}

/// Build the contents of one sketch.
///
/// `doc` supplies the stored sketch, `state` (when the design has been evaluated) its solved
/// copy; solved geometry is preferred because dimension edits and dragging only appear there.
/// `active` says whether this is the sketch being edited, which is the only one rows may edit.
pub(crate) fn build(doc: &Document, state: Option<&ModelState>, sketch: u64, active: bool) -> SketchContents {
    let solved = state.and_then(|state| state.sketch(sketch));
    let evaluated = solved.is_some();
    let source = solved.map(|solved| &solved.sketch).or_else(|| doc.sketch(sketch).ok());
    let mut buckets: Vec<Vec<ContentsRow>> = GroupKind::ALL.iter().map(|_| Vec::new()).collect();
    let Some(source) = source else {
        return SketchContents { sketch, editable: active, evaluated, groups: Vec::new() };
    };

    for (index, point) in source.points.iter().enumerate() {
        // Point 0 is always the fixed origin, not something the sketch drew: no row for it.
        if index == 0 && point.id == "origin" {
            continue;
        }
        let mut summary = format!("{}, {}", num(point.pos.x), num(point.pos.y));
        if point.fixed {
            summary.push_str(" · fixed");
        }
        buckets[GroupKind::Points.index()].push(contents_row(&point.id, RowKind::Point, format!("Point {}", point.id), summary));
    }

    for (index, curve) in source.curves.iter().enumerate() {
        let (group, kind, word) = match &curve.kind {
            CurveKind::Line { .. } => (GroupKind::Lines, RowKind::Line, "Line"),
            CurveKind::Arc { .. } => (GroupKind::Arcs, RowKind::Arc, "Arc"),
            CurveKind::Circle { .. } => (GroupKind::Circles, RowKind::Circle, "Circle"),
            CurveKind::Spline { .. } => (GroupKind::Splines, RowKind::Spline, "Spline"),
            CurveKind::Ellipse { .. } => (GroupKind::Conics, RowKind::Conic, "Ellipse"),
            CurveKind::Conic { .. } => (GroupKind::Conics, RowKind::Conic, "Conic"),
        };
        let summary = curve_summary(doc, source, index, &curve.kind);
        buckets[group.index()].push(contents_row(&curve.id, kind, format!("{word} {}", curve.id), summary));
    }

    for constraint in &source.constraints {
        if constraint.kind.is_dimension() {
            let label = format!("{} {}", dimension_word(&constraint.kind), constraint.id);
            let summary = dimension_summary(doc, constraint);
            buckets[GroupKind::Dimensions.index()].push(contents_row(&constraint.id, RowKind::Dimension, label, summary));
        } else {
            let label = format!("{} {}", constraint.kind.name(), constraint.id);
            let summary = constraint_entities(source, &constraint.kind);
            buckets[GroupKind::Constraints.index()].push(contents_row(&constraint.id, RowKind::Constraint, label, summary));
        }
    }

    for pattern in &source.patterns {
        let (word, summary) = pattern_summary(&pattern.kind);
        let label = format!("{word} {}", pattern.id);
        buckets[GroupKind::Patterns.index()].push(contents_row(&pattern.id, RowKind::Pattern, label, summary));
    }

    for (index, offset) in source.offsets.iter().enumerate() {
        // An offset chain has no id of its own: its first output curve names it, so a pick can
        // select it. A chain without outputs still gets a stable row.
        let id = offset.outputs.first().cloned().unwrap_or_else(|| format!("offset{}", index + 1));
        let summary = length(doc, offset.distance);
        buckets[GroupKind::Offsets.index()].push(contents_row(&id, RowKind::Offset, format!("Offset {id}"), summary));
    }
    for offset in &source.freeform_offsets {
        let summary = format!("{} · from {}", length(doc, offset.distance), offset.source);
        buckets[GroupKind::Offsets.index()].push(contents_row(&offset.output, RowKind::Offset, format!("Offset {}", offset.output), summary));
    }

    for link in &source.links {
        if link.kind == LinkKind::Text {
            let text = match &link.source {
                LinkSource::Text { text, .. } => excerpt(text, 40),
                _ => String::new(),
            };
            buckets[GroupKind::Text.index()].push(contents_row(&link.id, RowKind::Text, format!("Text {}", link.id), text));
        } else {
            let mut summary = link.source.describe();
            if link.lost {
                summary.push_str(" · lost");
            }
            buckets[GroupKind::Links.index()].push(contents_row(&link.id, RowKind::Link, format!("Link {}", link.id), summary));
        }
    }

    for wire in &source.wires {
        let summary = format!("{} points", wire.pts.len());
        buckets[GroupKind::Other.index()].push(contents_row(&wire.id, RowKind::Other, format!("3D Curve {}", wire.id), summary));
    }
    for equation in &source.equations {
        let summary = format!("y = {}", excerpt(&equation.y, 40));
        buckets[GroupKind::Other.index()].push(contents_row(&equation.curve, RowKind::Other, format!("Equation Curve {}", equation.curve), summary));
    }

    let groups = GroupKind::ALL
        .into_iter()
        .filter_map(|kind| {
            let rows = std::mem::take(&mut buckets[kind.index()]);
            (!rows.is_empty()).then_some(ContentsGroup { kind, rows })
        })
        .collect();
    SketchContents { sketch, editable: active, evaluated, groups }
}

/// A row with an id of `id`.
fn contents_row(id: &str, kind: RowKind, label: String, summary: String) -> ContentsRow {
    ContentsRow { id: id.to_string(), kind, label, summary }
}

/// What a curve measures: line length, arc radius and sweep, circle radius, ellipse radii,
/// spline point count, conic shape.
fn curve_summary(doc: &Document, sketch: &Sketch, index: usize, kind: &CurveKind) -> String {
    match kind {
        CurveKind::Line { a, b } => match (sketch.point(*a), sketch.point(*b)) {
            (Some(a), Some(b)) => length(doc, a.dist(b)),
            _ => String::new(),
        },
        CurveKind::Circle { r, .. } => format!("r {}", num(*r)),
        CurveKind::Arc { .. } => {
            let radius = sketch.radius(index).unwrap_or(0.0);
            let sweep = sketch.segs(index).into_iter().find_map(|seg| match seg {
                Seg2::Arc { sweep, .. } => Some(degrees(sweep)),
                _ => None,
            });
            match sweep {
                Some(sweep) => format!("r {} · {sweep}", num(radius)),
                None => format!("r {}", num(radius)),
            }
        }
        CurveKind::Ellipse { c, m, r } => {
            let major = match (sketch.point(*c), sketch.point(*m)) {
                (Some(c), Some(m)) => c.dist(m),
                _ => 0.0,
            };
            format!("{} × {}", num(major), num(*r))
        }
        CurveKind::Spline { pts, .. } => format!("{} points", pts.len()),
        CurveKind::Conic { rho, .. } => format!("{} · ρ {}", conic_name(*rho), num(*rho)),
    }
}

/// The geometric name of a conic's shape from its rho (0.5 is a parabola).
fn conic_name(rho: f64) -> &'static str {
    if rho < 0.5 {
        "Ellipse"
    } else if rho > 0.5 {
        "Hyperbola"
    } else {
        "Parabola"
    }
}

/// The type word of a dimension, instead of [`ConstraintKind::name`]'s long form.
fn dimension_word(kind: &ConstraintKind) -> &'static str {
    use ConstraintKind::*;
    match kind {
        Distance { .. }
        | DistanceX { .. }
        | DistanceY { .. }
        | PointLineDistance { .. }
        | LineCircleDistance { .. }
        | PointCircleDistance { .. }
        | CircleCircleDistance { .. }
        | Length { .. }
        | SlotOverallLength { .. } => "Distance",
        Radius { .. } => "Radius",
        Diameter { .. } | LinearDiameter { .. } => "Diameter",
        Angle { .. } | LineAngle { .. } | ArcSweep { .. } => "Angle",
        ArcLength { .. } => "Arc Length",
        OffsetDistance { .. } => "Offset",
        _ => "Dimension",
    }
}

/// A dimension's value in the design's units (degrees for angles), then its parameter or that it
/// is driven.
fn dimension_summary(doc: &Document, constraint: &Constraint) -> String {
    let Some(value) = constraint.kind.value() else {
        return String::new();
    };
    let mut summary = if constraint.kind.is_angle() { degrees(value) } else { length(doc, value) };
    if let Some(param) = constraint.param.as_ref().filter(|param| !param.is_empty()) {
        summary.push_str(" · ");
        summary.push_str(param);
    }
    if constraint.driven {
        summary.push_str(" · driven");
    }
    summary
}

/// The ids of the points and curves a geometric constraint ties together.
fn constraint_entities(sketch: &Sketch, kind: &ConstraintKind) -> String {
    let (points, curves) = kind.indices();
    let ids: Vec<&str> = points
        .iter()
        .filter_map(|index| sketch.points.get(*index).map(|point| point.id.as_str()))
        .chain(curves.iter().filter_map(|index| sketch.curves.get(*index).map(|curve| curve.id.as_str())))
        .collect();
    ids.join(", ")
}

/// A pattern's type word and how many copies it makes.
fn pattern_summary(kind: &SketchPatternKind) -> (&'static str, String) {
    match kind {
        // A circular pattern keeps its angle in degrees.
        SketchPatternKind::Circular { count, angle, .. } => ("Circular Pattern", format!("{count} copies · {}°", num(*angle))),
        SketchPatternKind::Rectangular { count, count2, .. } => {
            let summary = if *count2 > 1 { format!("{count} × {count2}") } else { format!("{count} copies") };
            ("Rectangular Pattern", summary)
        }
    }
}

/// A length in the design's unit, e.g. `20 mm`.
fn length(doc: &Document, value: f64) -> String {
    crate::length_input::shown(doc, value)
}

/// An angle in degrees, e.g. `90°`.
fn degrees(radians: f64) -> String {
    format!("{}°", num(radians.to_degrees()))
}

/// A plain number with at most three decimals and no trailing zeros.
fn num(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-0" { "0".to_string() } else { text.to_string() }
}

/// The first `max` characters of a text, with an ellipsis when it was cut.
fn excerpt(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let mut out: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::{
        doc::{FeatureKind, Model, PlaneRef},
        geom::{Vec2, Vec3},
        sketch::{EquationCurve, FreeformOffset, Link, OffsetChain, PatternAlignment, SketchPattern, TextExprs, TextStyle, Wire, WireSpace},
    };

    /// A document holding one sketch feature, named `Sketch1`.
    fn document(sketch: Sketch) -> (Document, u64) {
        let mut doc = Document::new("contents");
        let id = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch }, Some("Sketch1")).unwrap();
        (doc, id)
    }

    /// Points, a line, an arc, a circle, a spline, an ellipse, a conic, a geometric constraint and
    /// three dimensions.
    fn mixed_sketch() -> Sketch {
        let mut sk = Sketch::new();
        let line = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0), None, None, None).unwrap();
        let arc = sk.add_arc(Vec2::new(40.0, 0.0), Vec2::new(45.0, 0.0), Vec2::new(40.0, 5.0), [None; 3], None).unwrap();
        let circle = sk.add_circle(Vec2::new(60.0, 0.0), 4.0, None, None).unwrap();
        let spline_points: Vec<usize> = [Vec2::new(0.0, 20.0), Vec2::new(5.0, 25.0), Vec2::new(10.0, 20.0), Vec2::new(15.0, 25.0)]
            .into_iter()
            .map(|p| sk.add_point(p, None).unwrap())
            .collect();
        sk.add_curve(CurveKind::Spline { pts: spline_points, control: false, degree: 3, periodic: false, nurbs: None }, None).unwrap();
        let ellipse_centre = sk.add_point(Vec2::new(80.0, 0.0), None).unwrap();
        let ellipse_major = sk.add_point(Vec2::new(90.0, 0.0), None).unwrap();
        sk.add_curve(CurveKind::Ellipse { c: ellipse_centre, m: ellipse_major, r: 4.0 }, None).unwrap();
        let conic_start = sk.add_point(Vec2::new(0.0, 40.0), None).unwrap();
        let conic_end = sk.add_point(Vec2::new(10.0, 40.0), None).unwrap();
        let conic_apex = sk.add_point(Vec2::new(5.0, 45.0), None).unwrap();
        sk.add_curve(CurveKind::Conic { a: conic_start, b: conic_end, apex: conic_apex, rho: 0.5 }, None).unwrap();
        sk.add_constraint(ConstraintKind::Horizontal { l: line }, None).unwrap();
        sk.add_constraint(ConstraintKind::Coincident { p: 2, q: 6 }, None).unwrap();
        sk.add_constraint(ConstraintKind::Length { l: line, value: 20.0 }, Some("d1".into())).unwrap();
        sk.add_constraint(ConstraintKind::Radius { c: circle, value: 4.0 }, None).unwrap();
        sk.add_constraint(ConstraintKind::ArcSweep { c: arc, value: std::f64::consts::FRAC_PI_2 }, None).unwrap();
        sk
    }

    fn group(contents: &SketchContents, kind: GroupKind) -> &ContentsGroup {
        contents.groups.iter().find(|group| group.kind == kind).unwrap_or_else(|| panic!("missing {kind:?}"))
    }

    fn row<'a>(contents: &'a SketchContents, kind: GroupKind, id: &str) -> &'a ContentsRow {
        group(contents, kind).rows.iter().find(|row| row.id == id).unwrap_or_else(|| panic!("missing row {id} in {kind:?}"))
    }

    fn kinds(contents: &SketchContents) -> Vec<GroupKind> {
        contents.groups.iter().map(|group| group.kind).collect()
    }

    #[test]
    fn an_empty_sketch_has_no_groups_and_reports_its_flags() {
        let (doc, sketch) = document(Sketch::new());
        let contents = build(&doc, None, sketch, true);
        assert_eq!(contents.sketch, sketch);
        assert!(contents.editable);
        assert!(!contents.evaluated);
        assert!(contents.groups.is_empty(), "the fixed origin is not a row: {:?}", contents.groups);

        let inactive = build(&doc, None, sketch, false);
        assert!(!inactive.editable);
        assert!(!inactive.evaluated);
    }

    #[test]
    fn curves_constraints_and_dimensions_are_grouped_in_display_order() {
        let (doc, sketch) = document(mixed_sketch());
        let contents = build(&doc, None, sketch, true);

        assert_eq!(
            kinds(&contents),
            vec![
                GroupKind::Points,
                GroupKind::Lines,
                GroupKind::Arcs,
                GroupKind::Circles,
                GroupKind::Splines,
                GroupKind::Conics,
                GroupKind::Constraints,
                GroupKind::Dimensions,
            ],
            "empty groups are omitted and the rest keep display order"
        );

        // 1 origin (omitted) + 2 line points + 3 arc points + 1 circle centre + 4 spline points
        // + 2 ellipse points + 3 conic points.
        assert_eq!(group(&contents, GroupKind::Points).rows.len(), 15);
        let start = row(&contents, GroupKind::Points, "l1.start");
        assert_eq!(start.label, "Point l1.start");
        assert_eq!(start.summary, "0, 0");

        let line = row(&contents, GroupKind::Lines, "l1");
        assert_eq!((line.kind, line.label.as_str(), line.summary.as_str()), (RowKind::Line, "Line l1", "20 mm"));
        assert_eq!(row(&contents, GroupKind::Arcs, "a1").summary, "r 5 · 90°");
        assert_eq!(row(&contents, GroupKind::Circles, "c1").summary, "r 4");
        assert_eq!(row(&contents, GroupKind::Splines, "s1").summary, "4 points");
        assert_eq!(row(&contents, GroupKind::Conics, "e1").summary, "10 × 4");
        assert_eq!(row(&contents, GroupKind::Conics, "n1").summary, "Parabola · ρ 0.5");

        // Geometric constraints keep their type and name the entities they tie.
        let horizontal = row(&contents, GroupKind::Constraints, "k1");
        assert_eq!((horizontal.kind, horizontal.label.as_str()), (RowKind::Constraint, "Horizontal k1"));
        assert_eq!(horizontal.summary, "l1");
        let coincident = row(&contents, GroupKind::Constraints, "k2");
        assert_eq!(coincident.label, "Coincident k2");
        assert_eq!(coincident.summary, "l1.end, c1.center");

        // Dimensions keep their value, unit and parameter.
        let distance = row(&contents, GroupKind::Dimensions, "k3");
        assert_eq!((distance.kind, distance.label.as_str()), (RowKind::Dimension, "Distance k3"));
        assert_eq!(distance.summary, "20 mm · d1");
        assert_eq!(row(&contents, GroupKind::Dimensions, "k4").summary, "4 mm");
        assert_eq!(row(&contents, GroupKind::Dimensions, "k5").summary, "90°");
    }

    #[test]
    fn rows_keep_the_sketch_order_within_a_group() {
        let mut sk = Sketch::new();
        sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), None, None, Some("first")).unwrap();
        sk.add_line(Vec2::new(0.0, 10.0), Vec2::new(1.0, 10.0), None, None, Some("second")).unwrap();
        sk.add_line(Vec2::new(0.0, 20.0), Vec2::new(1.0, 20.0), None, None, Some("third")).unwrap();
        let (doc, sketch) = document(sk);
        let contents = build(&doc, None, sketch, false);
        let ids: Vec<&str> = group(&contents, GroupKind::Lines).rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["first", "second", "third"]);
    }

    #[test]
    fn patterns_offsets_links_text_and_three_d_curves_have_their_own_groups() {
        let mut sk = Sketch::new();
        sk.patterns.push(SketchPattern {
            id: "pat1".into(),
            entities: Vec::new(),
            kind: SketchPatternKind::Rectangular {
                dir: Vec2::new(1.0, 0.0),
                dir2: None,
                direction: None,
                direction2: None,
                count: 4,
                spacing: 10.0,
                count2: 2,
                spacing2: 5.0,
                alignment: PatternAlignment::default(),
                alignment2: PatternAlignment::default(),
                reversed: false,
                reversed2: false,
            },
            count_expression: None,
            instances: Vec::new(),
            expressions: Default::default(),
        });
        sk.patterns.push(SketchPattern {
            id: "pat2".into(),
            entities: Vec::new(),
            kind: SketchPatternKind::Circular {
                center: "origin".into(),
                count: 6,
                angle: 360.0,
                alignment: PatternAlignment::default(),
                reversed: false,
            },
            count_expression: None,
            instances: Vec::new(),
            expressions: Default::default(),
        });
        sk.offsets.push(OffsetChain {
            sources: vec![("l1".into(), false)],
            outputs: vec!["o1".into()],
            distance: 2.0,
            closed: false,
            topology: None,
        });
        sk.freeform_offsets.push(FreeformOffset {
            source: "s1".into(),
            output: "s2".into(),
            forward: true,
            distance: 1.5,
            divisions: 1,
            pieces: 1,
            closed: false,
            tolerance: 0.002,
            certificate: 1,
        });
        let plain_link = |id: &str, kind: LinkKind, source: LinkSource| Link {
            id: id.into(),
            kind,
            source,
            face_name: None,
            edge_name: None,
            auto_face: false,
            lost: false,
        };
        sk.links.push(plain_link("link1", LinkKind::Project, LinkSource::SketchCurve { sketch: 99, curve: "x1".into() }));
        sk.links.push(plain_link(
            "link2",
            LinkKind::Text,
            LinkSource::Text {
                text: "HELLO".into(),
                at: Vec2::ZERO,
                height: 5.0,
                angle: 0.0,
                font: None,
                style: TextStyle::default(),
                exprs: TextExprs::default(),
            },
        ));
        sk.wires.push(Wire {
            id: "w1".into(),
            pts: vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 0.0)],
            space: WireSpace::default(),
            link: None,
            fit: Vec::new(),
            arc: None,
            exact: Vec::new(),
        });
        sk.equations.push(EquationCurve {
            curve: "s1".into(),
            x: None,
            y: "sin(x)".into(),
            start: "0".into(),
            end: "360".into(),
            tolerance: 0.1,
            variable_unit: Default::default(),
        });
        let (doc, sketch) = document(sk);
        let contents = build(&doc, None, sketch, true);

        assert_eq!(
            kinds(&contents),
            vec![GroupKind::Patterns, GroupKind::Offsets, GroupKind::Links, GroupKind::Text, GroupKind::Other],
            "only the groups that hold something appear"
        );
        let rectangular = row(&contents, GroupKind::Patterns, "pat1");
        assert_eq!((rectangular.kind, rectangular.label.as_str()), (RowKind::Pattern, "Rectangular Pattern pat1"));
        assert_eq!(rectangular.summary, "4 × 2");
        assert_eq!(row(&contents, GroupKind::Patterns, "pat2").summary, "6 copies · 360°");
        assert_eq!(row(&contents, GroupKind::Offsets, "o1").summary, "2 mm");
        assert_eq!(row(&contents, GroupKind::Offsets, "s2").summary, "1.5 mm · from s1");
        assert_eq!(row(&contents, GroupKind::Links, "link1").summary, "sketch curve x1");
        assert_eq!(row(&contents, GroupKind::Text, "link2").summary, "HELLO");
        assert_eq!(row(&contents, GroupKind::Other, "w1").summary, "3 points");
        assert_eq!(row(&contents, GroupKind::Other, "s1").summary, "y = sin(x)");
    }

    #[test]
    fn long_text_is_summarised_with_an_ellipsis() {
        assert_eq!(excerpt("short", 40), "short");
        let long = "x".repeat(50);
        assert_eq!(excerpt(&long, 40), format!("{}…", "x".repeat(40)));
        assert_eq!(excerpt("héllo wörld", 5), "héllo…", "the cut counts characters, not bytes");
    }

    #[test]
    fn an_evaluated_sketch_prefers_its_solved_geometry() {
        let mut sk = Sketch::new();
        let line = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(18.0, 0.0), None, None, None).unwrap();
        sk.add_constraint(ConstraintKind::Horizontal { l: line }, None).unwrap();
        sk.add_constraint(ConstraintKind::Length { l: line, value: 20.0 }, None).unwrap();
        let (doc, sketch) = document(sk);

        let mut model = Model::new();
        model.evaluate(&doc);
        let state = model.state();
        let contents = build(&doc, Some(state.as_ref()), sketch, false);
        assert!(contents.evaluated);
        assert!(!contents.editable);
        assert_eq!(row(&contents, GroupKind::Lines, "l1").summary, "20 mm", "the solved length, not the drawn one");
    }

    #[test]
    fn a_large_sketch_lists_every_row() {
        let mut sk = Sketch::new();
        for index in 0..372 {
            let x = index as f64 * 10.0;
            sk.add_line(Vec2::new(x, 0.0), Vec2::new(x + 5.0, 5.0), None, None, None).unwrap();
        }
        let (doc, sketch) = document(sk);
        let contents = build(&doc, None, sketch, true);

        assert_eq!(kinds(&contents), vec![GroupKind::Points, GroupKind::Lines]);
        assert_eq!(group(&contents, GroupKind::Points).rows.len(), 744, "two points per line, origin left out");
        assert_eq!(group(&contents, GroupKind::Lines).rows.len(), 372);
        let rows: usize = contents.groups.iter().map(|group| group.rows.len()).sum();
        assert_eq!(rows, 744 + 372);
        assert!(group(&contents, GroupKind::Lines).rows.iter().all(|row| row.label.starts_with("Line l")));
    }

    #[test]
    fn num_formats_without_float_noise() {
        assert_eq!(num(20.0), "20");
        assert_eq!(num(12.5), "12.5");
        assert_eq!(num(12.3456), "12.346");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(f64::NAN), "NaN");
    }
}
