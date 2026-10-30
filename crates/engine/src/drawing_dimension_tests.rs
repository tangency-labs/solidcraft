//! Dimensions on drawing views found again in the model (W19b, #4896): an 80 x 40 x 30 box from
//! the origin (length along X, width along Y, height along Z), every value worked by hand.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::ModelBody;
use solidcraft_doc::appearance::face_index_at;
use solidcraft_doc::drawing::dimension::{
    Candidate, Dimension, DimensionKind, ModelEdge, ModelFace, ModelPoint, Resolved, Silhouette, Status, TextPlace,
};
use solidcraft_doc::drawing::view::{View, ViewKind, ViewPlace};
use solidcraft_doc::drawing::{Drawing, Source, Units};
use solidcraft_doc::drawing_view::{ViewDirection, ViewOverrides};
use solidcraft_doc::ids::IdSource;
use solidcraft_doc::naming::{edge_names, face_names};
use solidcraft_geom::Vec3;
use solidcraft_geom::drawing_measure::{LinearDirection, Reading, SilhouetteSide};
use solidcraft_geom::drawing_sheet::{Orientation, PaperSize, Sheet};
use solidcraft_geom::drawing_view::{NamedView, ProjectedSide};

use crate::Session;

const EPS: f64 = 1e-9;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// The box, a drawing of it with its front view at 1:2 and the right view lined up with it, and
/// the box feature's id. The drawing isn't open, so the model can change.
struct Fixture {
    s: Session,
    d: Drawing,
    ids: IdSource,
    front: u64,
    right: u64,
    feature: u64,
}

fn boxed() -> Fixture {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 40, "height": 30, "body_name": "Box"}));
    let feature = s.doc.features.last().unwrap().id;
    let mut ids = IdSource::seeded(4896);
    let a3 = Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Box", "en", "Sheet 1", a3, None, &BTreeSet::new(), &mut ids).unwrap();
    let source = d.allocate_id(&mut ids).unwrap();
    d.sources.push(Source { id: source });
    let sheet = d.sheets[0].id;
    let (front, right) = (d.allocate_id(&mut ids).unwrap(), d.allocate_id(&mut ids).unwrap());
    let half = ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() };
    let base = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let side = ViewKind::Projected { parent: front, side: ProjectedSide::Right };
    d.add_views([
        Arc::new(View { id: front, sheet, kind: base, place: ViewPlace::At([150.0, 150.0]), settings: half, reference_margin_mm: 0.0 }),
        Arc::new(View {
            id: right,
            sheet,
            kind: side,
            place: ViewPlace::Aligned(300.0),
            settings: ViewOverrides::default(),
            reference_margin_mm: 0.0,
        }),
    ]);
    d.validate().unwrap();
    Fixture { s, d, ids, front, right, feature }
}

impl Fixture {
    /// The box's body as the model has it now.
    fn body(&self) -> ModelBody {
        self.s.model.state().bodies.iter().find(|b| b.name == "Box").unwrap().clone()
    }

    /// The box's edges with an end at `p`: each one's name and ends.
    fn edges_at(&self, p: [f64; 3]) -> Vec<(String, Vec3, Vec3)> {
        let b = self.body();
        let p = Vec3::new(p[0], p[1], p[2]);
        let near = |q: Vec3| q.dist(p) < 1e-6;
        let lines = b.mesh().edges.clone();
        lines
            .iter()
            .zip(edge_names(&b))
            .filter_map(|(line, name)| Some((name, *line.first()?, *line.last()?)))
            .filter(|(_, a, z)| near(*a) || near(*z))
            .collect()
    }

    /// The box's edge from `a` to `b` (either way), as a dimension saves it.
    fn edge(&self, a: [f64; 3], b: [f64; 3]) -> ModelEdge {
        let to = Vec3::new(b[0], b[1], b[2]);
        let (name, ..) = self.edges_at(a).into_iter().find(|(_, p, q)| p.dist(to) < 1e-6 || q.dist(to) < 1e-6).unwrap();
        ModelEdge { body: "Box".into(), origin_feature: self.feature, name, path: Vec::new() }
    }

    /// The box's edge whose every point is `radius` from `centre`: one of a circle's or an arc's.
    fn round(&self, centre: [f64; 3], radius: f64) -> ModelEdge {
        let b = self.body();
        let c = Vec3::new(centre[0], centre[1], centre[2]);
        let lines = b.mesh().edges.clone();
        let (_, name) = lines.iter().zip(edge_names(&b)).find(|(line, _)| line.iter().all(|p| (p.dist(c) - radius).abs() < 1e-6)).unwrap();
        ModelEdge { body: "Box".into(), origin_feature: self.feature, name, path: Vec::new() }
    }

    /// The box's face at `p`, by its name.
    fn face(&self, p: [f64; 3]) -> ModelFace {
        let b = self.body();
        let index = face_index_at(&b, Vec3::new(p[0], p[1], p[2])).unwrap();
        ModelFace { body: "Box".into(), origin_feature: self.feature, name: face_names(&b)[index].clone(), path: Vec::new() }
    }

    /// The box's corner at `p`, by the names of the edges that meet there.
    fn corner(&self, p: [f64; 3]) -> ModelPoint {
        let edges = self.edges_at(p).into_iter().map(|(name, ..)| name).collect();
        ModelPoint { body: "Box".into(), origin_feature: self.feature, edges, centre: false, path: Vec::new() }
    }

    /// Adds a dimension on `view` (2 decimals, its text centred) and gives its id.
    fn add(&mut self, view: u64, kind: DimensionKind, override_text: Option<&str>) -> u64 {
        let id = self.d.allocate_id(&mut self.ids).unwrap();
        let override_text = override_text.map(str::to_string);
        self.d.add_dimensions([Arc::new(Dimension {
            id,
            view,
            kind,
            offset_mm: 10.0,
            text: TextPlace::Centred,
            decimals: 2,
            override_text,
            last_good: None,
        })]);
        self.d.validate().unwrap();
        id
    }

    fn linear(&mut self, view: u64, a: [f64; 3], b: [f64; 3], direction: LinearDirection, reading: Reading) -> u64 {
        let kind = DimensionKind::Linear { a: self.corner(a), b: self.corner(b), direction, reading };
        self.add(view, kind, None)
    }

    fn resolve(&self, id: u64) -> Resolved {
        self.s.doc.resolve_dimensions(&self.s.model, &self.d, &[id]).unwrap().remove(0)
    }
}

/// `r` was measured, reading `value` (millimetres or radians).
fn assert_reads(r: &Resolved, value: f64, foreshortened: bool) {
    assert!(matches!(r.status, Status::Ok | Status::Overridden), "{r:?}");
    let m = r.measured.unwrap();
    assert!((m.value - value).abs() < EPS, "{} against {value}: {r:?}", m.value);
    assert_eq!(m.foreshortened, foreshortened, "{r:?}");
}

/// In the front view, between the two bottom front corners horizontally 80, from the bottom to
/// the top vertically 30, aligned from (0, 0, 0) to (80, 0, 30) √7300, and the top front edge's
/// length 80: none foreshortened, every name exact. In the right view, from (0, 0, 0) to
/// (80, 40, 0): projected horizontally 40, its true length √8000, foreshortened. The text is the
/// number in the design's millimetres with the dimension's decimals, or in the drawing's own unit.
/// Two names that share both ends of one edge name no one corner.
#[test]
fn a_box_measures_as_its_views_show_it() {
    let mut f = boxed();
    let (front, right) = (f.front, f.right);
    let wide = f.linear(front, [0.0, 0.0, 0.0], [80.0, 0.0, 0.0], LinearDirection::Horizontal, Reading::Projected);
    let tall = f.linear(front, [0.0, 0.0, 0.0], [0.0, 0.0, 30.0], LinearDirection::Vertical, Reading::Projected);
    let slant = f.linear(front, [0.0, 0.0, 0.0], [80.0, 0.0, 30.0], LinearDirection::Aligned, Reading::Projected);
    let top = f.edge([0.0, 0.0, 30.0], [80.0, 0.0, 30.0]);
    let length = f.add(front, DimensionKind::Length { edge: top, direction: LinearDirection::Aligned, reading: Reading::True }, None);
    let across = f.linear(right, [0.0, 0.0, 0.0], [80.0, 40.0, 0.0], LinearDirection::Horizontal, Reading::Projected);
    let diagonal = f.linear(right, [0.0, 0.0, 0.0], [80.0, 40.0, 0.0], LinearDirection::Aligned, Reading::True);
    for (id, value, foreshortened) in [
        (wide, 80.0, false),
        (tall, 30.0, false),
        (slant, 7300f64.sqrt(), false),
        (length, 80.0, false),
        (across, 40.0, true),
        (diagonal, 8000f64.sqrt(), true),
    ] {
        let r = f.resolve(id);
        assert_reads(&r, value, foreshortened);
        assert_eq!(r.status, Status::Ok);
        assert!(r.exact, "{r:?}");
        assert_eq!(r.points.len(), 2);
    }
    assert_eq!(f.resolve(wide).text.as_deref(), Some("80.00"));
    let ends = f.resolve(wide).points;
    assert!(ends[0].dist(Vec3::ZERO) < EPS && ends[1].dist(Vec3::new(80.0, 0.0, 0.0)) < EPS, "{ends:?}");
    f.d.units = Units::Fixed { unit: "cm".into() };
    assert_eq!(f.resolve(wide).text.as_deref(), Some("8.00"), "the drawing's own unit");
    // Every dimension at once, in the order asked for; an id the drawing doesn't have is refused.
    let all = f.s.doc.resolve_dimensions(&f.s.model, &f.d, &[tall, wide]).unwrap();
    let values: Vec<f64> = all.iter().map(|r| r.measured.unwrap().value).collect();
    assert!((values[0] - 30.0).abs() < EPS && (values[1] - 80.0).abs() < EPS, "{values:?}");
    let refused = f.s.doc.resolve_dimensions(&f.s.model, &f.d, &[wide, 99]).unwrap_err().to_string();
    assert!(refused.contains(&solidcraft_doc::ids::encode(99)), "{refused}");
    // Two names for one edge share both its ends: they name no one corner, and neither is chosen.
    let top = f.edge([0.0, 0.0, 30.0], [80.0, 0.0, 30.0]).name;
    let twice = ModelPoint { edges: vec![top.clone(), top], ..f.corner([0.0, 0.0, 30.0]) };
    let kind = DimensionKind::Linear { a: twice, b: f.corner([0.0, 0.0, 0.0]), direction: LinearDirection::Vertical, reading: Reading::Projected };
    let both = f.add(front, kind, None);
    let r = f.resolve(both);
    let Status::Ambiguous(corners) = &r.status else { panic!("{r:?}") };
    assert_eq!(corners.len(), 2, "{r:?}");
    assert!(corners.iter().all(|c| matches!(c, Candidate::Corner(_))));
}

/// A dimension follows the model: the length changed from 80 to 100 reads 100, and undone, 80 again,
/// exact. Working it out never changes the drawing.
#[test]
fn a_dimension_follows_a_change_and_its_undo() {
    let mut f = boxed();
    let wide = f.linear(f.front, [0.0, 0.0, 0.0], [80.0, 0.0, 0.0], LinearDirection::Horizontal, Reading::Projected);
    let saved = f.d.to_json().unwrap();
    let list = run(&mut f.s, "parameters.list", json!({"filter": "feature"}));
    let length = list["parameters"].as_array().unwrap().iter().find(|p| p["value"].as_f64() == Some(80.0)).unwrap()["name"].clone();
    run(&mut f.s, "parameters.change", json!({"name": length, "expression": "100 mm"}));
    assert_reads(&f.resolve(wide), 100.0, false);
    assert_eq!(f.resolve(wide).text.as_deref(), Some("100.00"));
    run(&mut f.s, "edit.undo", json!({}));
    let back = f.resolve(wide);
    assert_reads(&back, 80.0, false);
    assert!(back.exact);
    assert_eq!(f.d.to_json().unwrap(), saved, "the record is what it was");
}

/// After an equal 10 mm chamfer on the top front edge, the chamfer's edge on the right face meets
/// that face's top edge at 45° in the right view, and at 135° on the other side. The front view
/// sees the top edge end-on, so the angle can't be measured there.
#[test]
fn an_angle_between_two_edges() {
    let mut f = boxed();
    run(&mut f.s, "solid.chamfer", json!({"edges": [[40, 0, 30]], "distance": 10, "body": "Box"}));
    let chamfer = f.edge([80.0, 0.0, 20.0], [80.0, 10.0, 30.0]);
    let top = f.edge([80.0, 10.0, 30.0], [80.0, 40.0, 30.0]);
    let angle =
        |a: &ModelEdge, b: &ModelEdge, other_side| DimensionKind::Angle { a: a.clone(), b: b.clone(), reading: Reading::Projected, other_side };
    let (right, front) = (f.right, f.front);
    let near = f.add(right, angle(&chamfer, &top, false), None);
    let far = f.add(right, angle(&chamfer, &top, true), None);
    let end_on = f.add(front, angle(&chamfer, &top, false), None);
    let r = f.resolve(near);
    assert_reads(&r, 45f64.to_radians(), false);
    assert_eq!(r.text.as_deref(), Some("45.00°"));
    assert_eq!(r.points.len(), 4, "the two lines");
    assert_reads(&f.resolve(far), 135f64.to_radians(), false);
    let r = f.resolve(end_on);
    assert!(matches!(&r.status, Status::Unmeasurable(why) if why.contains("end-on")), "{r:?}");
    assert_eq!((r.measured, r.text), (None, None));
}

/// A hole 12 across along Y through the box at (40, 15), and a 5 mm fillet on its top right edge
/// (along Y): the front view sees both face on, so the hole's front edge reads 6 as a radius and 12
/// as a diameter, centred on the hole's axis, and the fillet's front arc R5 from the right face to
/// the top. The right view sees the hole's edge end-on, and a straight edge isn't round: neither
/// can be measured. The hole's back edge reads where it is, 40 behind. The hole made 16 across
/// reads Ø16, and undone Ø12 again; deleted, it is lost, and undone found again.
#[test]
fn a_circle_and_an_arc_read_their_radius_and_diameter() {
    let mut f = boxed();
    run(
        &mut f.s,
        "solid.cylinder",
        json!({"radius": 6, "height": 42, "base": [40, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"], "name": "Hole"}),
    );
    run(&mut f.s, "solid.fillet", json!({"edges": [[80, 20, 30]], "radius": 5, "body": "Box"}));
    let (front, right) = (f.front, f.right);
    let radial = |edge: &ModelEdge, diameter| DimensionKind::Radial { edge: edge.clone(), diameter };
    let (hole, arc) = (f.round([40.0, 0.0, 15.0], 6.0), f.round([75.0, 0.0, 25.0], 5.0));
    let across = f.add(front, radial(&hole, true), None);
    let half = f.add(front, radial(&hole, false), None);
    let fillet = f.add(front, radial(&arc, false), None);
    let r = f.resolve(across);
    assert_reads(&r, 12.0, false);
    assert_eq!(r.text.as_deref(), Some("Ø12.00"));
    assert!(r.exact, "{r:?}");
    let centre = Vec3::new(40.0, 0.0, 15.0);
    assert!(r.points[0].dist(centre) < EPS, "the centre: {r:?}");
    assert!(r.points[1..].iter().all(|p| (p.dist(centre) - 6.0).abs() < EPS && p.y.abs() < EPS), "on the edge: {r:?}");
    let back_rim = f.round([40.0, 40.0, 15.0], 6.0);
    let back = f.add(front, radial(&back_rim, true), None);
    let r = f.resolve(back);
    assert_reads(&r, 12.0, false);
    assert!(r.points.iter().all(|p| (p.y - 40.0).abs() < EPS), "on the back face: {r:?}");
    let r = f.resolve(half);
    assert_reads(&r, 6.0, false);
    assert_eq!(r.text.as_deref(), Some("R6.00"));
    let r = f.resolve(fillet);
    assert_reads(&r, 5.0, false);
    assert_eq!(r.text.as_deref(), Some("R5.00"));
    let ends = [r.points[1], r.points[3]];
    let at = |p: Vec3| ends.iter().any(|q| q.dist(p) < EPS);
    assert!(at(Vec3::new(80.0, 0.0, 25.0)) && at(Vec3::new(75.0, 0.0, 30.0)), "from the right face to the top: {r:?}");
    let diagonal = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert!(r.points[2].dist(Vec3::new(75.0 + diagonal, 0.0, 25.0 + diagonal)) < EPS, "its middle: {r:?}");
    // End-on, and straight.
    let end_on = f.add(right, radial(&hole, true), None);
    let r = f.resolve(end_on);
    assert!(matches!(&r.status, Status::Unmeasurable(why) if why.ends_with("(it isn't parallel to it)")), "{r:?}");
    assert_eq!((r.measured, r.text, r.points.is_empty()), (None, None, true));
    let left = f.edge([0.0, 0.0, 0.0], [0.0, 0.0, 30.0]);
    let straight = f.add(front, radial(&left, false), None);
    let r = f.resolve(straight);
    assert!(matches!(&r.status, Status::Unmeasurable(why) if why.ends_with("isn't a circle or an arc")), "{r:?}");
    // The hole made 16 across, and undone.
    let list = run(&mut f.s, "parameters.list", json!({"filter": "feature"}));
    let size = list["parameters"].as_array().unwrap().iter().find(|p| p["value"].as_f64() == Some(6.0)).unwrap()["name"].clone();
    run(&mut f.s, "parameters.change", json!({"name": size, "expression": "8 mm"}));
    assert_reads(&f.resolve(across), 16.0, false);
    run(&mut f.s, "edit.undo", json!({}));
    assert_reads(&f.resolve(across), 12.0, false);
    run(&mut f.s, "timeline.delete", json!({"feature": "Hole"}));
    let r = f.resolve(across);
    assert!(matches!(&r.status, Status::Lost(why) if why.contains("no longer exists")), "{r:?}");
    run(&mut f.s, "edit.undo", json!({}));
    assert_reads(&f.resolve(across), 12.0, false);
    // A slot across the rim splits it: which piece to measure isn't one answer, and none is chosen.
    run(&mut f.s, "solid.box", json!({"length": 2, "width": 5, "height": 20, "corner": [39, -1, 5], "operation": "cut", "targets": ["Box"]}));
    let r = f.resolve(across);
    assert!(matches!(&r.status, Status::Ambiguous(pieces) if pieces.len() >= 2), "{r:?}");
    assert_eq!(r.measured, None);
}

/// What a dimension measures is gone when its edge is filleted away, and found again on undo; a
/// new box with the same body's name isn't the box it measured. A suppressed or rolled-back
/// feature is said so, and found again when it is back. An occurrence path that leads nowhere is
/// lost.
#[test]
fn lost_and_suppressed_say_so_and_never_move_to_other_geometry() {
    let mut f = boxed();
    let top = f.edge([0.0, 0.0, 30.0], [80.0, 0.0, 30.0]);
    let length = f.add(f.front, DimensionKind::Length { edge: top.clone(), direction: LinearDirection::Aligned, reading: Reading::True }, None);
    run(&mut f.s, "solid.fillet", json!({"edges": [[40, 0, 30]], "radius": 5, "body": "Box"}));
    let r = f.resolve(length);
    assert!(matches!(&r.status, Status::Lost(why) if why.contains("no longer exists")), "{r:?}");
    assert_eq!((r.measured, r.text.as_deref(), r.points.is_empty()), (None, None, true));
    run(&mut f.s, "edit.undo", json!({}));
    assert_reads(&f.resolve(length), 80.0, false);
    // Suppressed, then back.
    let name = f.s.doc.features.last().unwrap().name.clone();
    run(&mut f.s, "timeline.suppress", json!({"feature": name, "suppressed": true}));
    assert_eq!(f.resolve(length).status, Status::Suppressed);
    run(&mut f.s, "timeline.suppress", json!({"feature": name, "suppressed": false}));
    assert_reads(&f.resolve(length), 80.0, false);
    // Through an occurrence that isn't there.
    let placed = ModelEdge { path: vec![999], ..top };
    let through = f.add(f.front, DimensionKind::Length { edge: placed, direction: LinearDirection::Aligned, reading: Reading::True }, None);
    assert!(matches!(f.resolve(through).status, Status::Lost(_)));
    // Deleted, and a new box made with the same body's name.
    run(&mut f.s, "timeline.delete", json!({"feature": name}));
    run(&mut f.s, "solid.box", json!({"length": 80, "width": 40, "height": 30, "body_name": "Box"}));
    let r = f.resolve(length);
    assert!(matches!(&r.status, Status::Lost(why) if why.contains("another feature")), "{r:?}");
}

/// A notch across the top front edge splits it in two: its length is no longer one number, so the
/// dimension lists both pieces and chooses neither, while the corners at its ends are still
/// found (through a piece, so not exact) 80 apart. The pieces still lie on one line, square to the
/// left front edge, found through them, so not exact either.
#[test]
fn a_split_edge_is_ambiguous_for_its_length_and_its_ends_still_meet() {
    let mut f = boxed();
    let top = f.edge([0.0, 0.0, 30.0], [80.0, 0.0, 30.0]);
    let length = f.add(f.front, DimensionKind::Length { edge: top.clone(), direction: LinearDirection::Aligned, reading: Reading::True }, None);
    let left = f.edge([0.0, 0.0, 0.0], [0.0, 0.0, 30.0]);
    let square = f.add(f.front, DimensionKind::Angle { a: top, b: left, reading: Reading::Projected, other_side: false }, None);
    let ends = f.linear(f.front, [0.0, 0.0, 30.0], [80.0, 0.0, 30.0], LinearDirection::Horizontal, Reading::Projected);
    run(&mut f.s, "solid.box", json!({"length": 10, "width": 5, "height": 5, "corner": [35, -1, 26], "operation": "cut", "targets": ["Box"]}));
    let r = f.resolve(length);
    let Status::Ambiguous(candidates) = &r.status else { panic!("{r:?}") };
    assert_eq!(candidates.len(), 2, "{r:?}");
    assert!(candidates.iter().all(|c| matches!(c, Candidate::Edge { body, .. } if body == "Box")));
    assert_eq!(r.measured, None);
    let r = f.resolve(ends);
    assert_reads(&r, 80.0, false);
    assert!(!r.exact, "found through a piece");
    let r = f.resolve(square);
    assert_reads(&r, std::f64::consts::FRAC_PI_2, false);
    assert!(!r.exact, "found through its pieces");
}

/// A hole's centre is found through its rim, exactly: 40 along and 15 up from the box's bottom
/// front left corner. A slot across the rim leaves it several arcs, so the centre is ambiguous and
/// none is chosen; with the hole suppressed its centre is lost.
#[test]
fn a_holes_centre_is_found_through_its_rim() {
    let mut f = boxed();
    run(&mut f.s, "solid.cylinder", json!({"radius": 6, "height": 42, "base": [40, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"]}));
    let hole = f.s.doc.features.last().unwrap().name.clone();
    let rim = f.round([40.0, 0.0, 15.0], 6.0);
    let centre = ModelPoint { body: rim.body, origin_feature: rim.origin_feature, edges: vec![rim.name], centre: true, path: Vec::new() };
    let to_centre = |f: &mut Fixture, direction| {
        let kind = DimensionKind::Linear { a: f.corner([0.0, 0.0, 0.0]), b: centre.clone(), direction, reading: Reading::Projected };
        f.add(f.front, kind, None)
    };
    let (across, up) = (to_centre(&mut f, LinearDirection::Horizontal), to_centre(&mut f, LinearDirection::Vertical));
    let r = f.resolve(across);
    assert_reads(&r, 40.0, false);
    assert!(r.exact, "{r:?}");
    assert_reads(&f.resolve(up), 15.0, false);
    run(&mut f.s, "solid.box", json!({"length": 2, "width": 5, "height": 30, "corner": [39, -1, 0], "operation": "cut", "targets": ["Box"]}));
    let r = f.resolve(across);
    assert!(matches!(&r.status, Status::Ambiguous(c) if c.len() >= 2) && r.measured.is_none(), "{r:?}");
    run(&mut f.s, "timeline.suppress", json!({"feature": hole, "suppressed": true}));
    let r = f.resolve(across);
    assert!(matches!(&r.status, Status::Lost(why) if why.contains("no longer exists")), "{r:?}");
}

/// A boss of radius 5 along X on the box's right face reads Ø10 between its silhouettes in the front
/// view, exactly, with its silhouettes 10 apart from one end of it to the other. A notch across its
/// top cuts its face into pieces, all on the one cylinder, so it still reads Ø10 (found through
/// pieces, so not exactly). The right view looks along its axis, so it can't be measured there;
/// with the boss suppressed it is lost.
#[test]
fn a_cylinders_diameter_is_found_through_all_its_faces() {
    let mut f = boxed();
    run(&mut f.s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [80, 20, 15], "axis": "X", "operation": "join", "targets": ["Box"]}));
    let boss = f.s.doc.features.last().unwrap().name.clone();
    let face = f.face([90.0, 22.5, 15.0 + 5.0 * 30f64.to_radians().cos()]);
    let (front, right) =
        (f.add(f.front, DimensionKind::Cylinder { face: face.clone() }, None), f.add(f.right, DimensionKind::Cylinder { face }, None));
    let r = f.resolve(front);
    assert_reads(&r, 10.0, false);
    assert!(r.exact, "{r:?}");
    // Each silhouette from x = 80 to 100, one at z = 20 and the other at z = 10 (either way round).
    let [a, b, c, d] = r.points.as_slice() else { panic!("{r:?}") };
    let ends = |p: &Vec3, q: &Vec3| (p.x - 80.0).abs() < 1e-6 && (q.x - 100.0).abs() < 1e-6 && (p.z - q.z).abs() < 1e-9 && (p.y - 20.0).abs() < 1e-9;
    assert!(ends(a, b) && ends(c, d) && ((a.z - c.z).abs() - 10.0).abs() < 1e-9 && (a.z + c.z - 30.0).abs() < 1e-9, "{:?}", r.points);
    let r = f.resolve(right);
    assert!(matches!(&r.status, Status::Unmeasurable(why) if why.contains("end-on")), "{r:?}");
    run(&mut f.s, "solid.box", json!({"length": 2, "width": 50, "height": 3, "corner": [89, -1, 18], "operation": "cut", "targets": ["Box"]}));
    let r = f.resolve(front);
    assert_reads(&r, 10.0, false);
    assert!(!r.exact, "found through pieces: {r:?}");
    run(&mut f.s, "timeline.suppress", json!({"feature": boss, "suppressed": true}));
    let r = f.resolve(front);
    assert!(matches!(&r.status, Status::Lost(why) if why.contains("no longer exists")), "{r:?}");
}

/// Two bosses along X on the box's right face, radius 5 at z = 15 and radius 3 at z = 25: in the
/// front view the big one's near silhouette (z = 20) is 8 from the small one's far one (z = 28),
/// exactly, each silhouette from x = 80 to 100. The right view looks along their axes. A flat cut
/// along the small boss's top takes its far silhouette away; with the small boss suppressed it is
/// lost.
#[test]
fn two_cylinders_silhouettes_are_found_on_their_faces() {
    let mut f = boxed();
    for (radius, z) in [(5, 15), (3, 25)] {
        run(
            &mut f.s,
            "solid.cylinder",
            json!({"radius": radius, "height": 20, "base": [80, 20, z], "axis": "X", "operation": "join", "targets": ["Box"]}),
        );
    }
    let small = f.s.doc.features.last().unwrap().name.clone();
    let below = |r: f64, z: f64| [90.0, 20.0 - r * 0.5, z - r * 30f64.to_radians().cos()];
    let a = Silhouette { face: f.face(below(5.0, 15.0)), side: SilhouetteSide::Near };
    let b = Silhouette { face: f.face(below(3.0, 25.0)), side: SilhouetteSide::Far };
    let kind = DimensionKind::Silhouettes { a, b, direction: LinearDirection::Aligned };
    let (front, right) = (f.add(f.front, kind.clone(), None), f.add(f.right, kind, None));
    let r = f.resolve(front);
    assert_reads(&r, 8.0, false);
    assert!(r.exact, "{r:?}");
    let [a, b, c, d] = r.points.as_slice() else { panic!("{r:?}") };
    // Each from one end of the faces along the axis to the other, whichever way the axis runs.
    let at = |p: &Vec3, x: f64, z: f64| (p.x - x).abs() < 1e-6 && (p.y - 20.0).abs() < 1e-9 && (p.z - z).abs() < 1e-9;
    let runs = |p, q, z| (at(p, 80.0, z) && at(q, 100.0, z)) || (at(p, 100.0, z) && at(q, 80.0, z));
    assert!(runs(a, b, 20.0) && runs(c, d, 28.0), "{:?}", r.points);
    let r = f.resolve(right);
    assert!(matches!(&r.status, Status::Unmeasurable(why) if why.contains("end-on")), "{r:?}");
    run(&mut f.s, "solid.box", json!({"length": 31, "width": 50, "height": 10, "corner": [79, -1, 27], "operation": "cut", "targets": ["Box"]}));
    let r = f.resolve(front);
    assert!(matches!(&r.status, Status::Unmeasurable(why) if why.contains("far silhouette in this view isn't on it")), "{r:?}");
    run(&mut f.s, "timeline.suppress", json!({"feature": small, "suppressed": true}));
    let r = f.resolve(front);
    assert!(matches!(&r.status, Status::Lost(_) | Status::Suppressed), "{r:?}");
}

/// A slot through the front face splits it, and the top front edge with it; an angle is put on the
/// piece left of the slot, square to the back left edge. With the slot suppressed the edge is whole
/// again, and the piece's name finds it only through the face the piece was cut from: still square,
/// but found through a piece, so not exact.
#[test]
fn a_pieces_angle_on_the_whole_edge_again_is_not_exact() {
    let mut f = boxed();
    run(&mut f.s, "solid.box", json!({"length": 10, "width": 5, "height": 32, "corner": [35, -1, -1], "operation": "cut", "targets": ["Box"]}));
    let piece = f.edge([0.0, 0.0, 30.0], [35.0, 0.0, 30.0]);
    assert!(piece.name.contains('#'), "a piece's name: {}", piece.name);
    let back = f.edge([0.0, 40.0, 0.0], [0.0, 40.0, 30.0]);
    let square = f.add(f.front, DimensionKind::Angle { a: piece, b: back, reading: Reading::Projected, other_side: false }, None);
    let slot = f.s.doc.features.last().unwrap().name.clone();
    run(&mut f.s, "timeline.suppress", json!({"feature": slot, "suppressed": true}));
    let r = f.resolve(square);
    assert_reads(&r, std::f64::consts::FRAC_PI_2, false);
    assert!(!r.exact, "found through a piece");
}

/// Text given instead of the number is shown, and the number is still worked out: after the
/// length changes to 100 it reads 100 and shows the same text.
#[test]
fn an_override_shows_its_text_and_still_reads_the_model() {
    let mut f = boxed();
    let kind = DimensionKind::Linear {
        a: f.corner([0.0, 0.0, 0.0]),
        b: f.corner([80.0, 0.0, 0.0]),
        direction: LinearDirection::Horizontal,
        reading: Reading::Projected,
    };
    let id = f.add(f.front, kind, Some("≈80"));
    let r = f.resolve(id);
    assert_eq!((&r.status, r.text.as_deref()), (&Status::Overridden, Some("≈80")));
    assert_reads(&r, 80.0, false);
    let list = run(&mut f.s, "parameters.list", json!({"filter": "feature"}));
    let length = list["parameters"].as_array().unwrap().iter().find(|p| p["value"].as_f64() == Some(80.0)).unwrap()["name"].clone();
    run(&mut f.s, "parameters.change", json!({"name": length, "expression": "100 mm"}));
    let r = f.resolve(id);
    assert_eq!((&r.status, r.text.as_deref()), (&Status::Overridden, Some("≈80")));
    assert_reads(&r, 100.0, false);
}
