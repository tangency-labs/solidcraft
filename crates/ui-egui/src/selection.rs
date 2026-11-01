//! Selection inputs and the geometry behind picking: what a command input accepts, the origin
//! widget (a fixed size on screen), ray tests against planar quads, and window/crossing box
//! selection. The pure parts are unit tested; the viewport uses them.

use egui::{Pos2, Rect};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

use crate::viewport::Hit;

/// Model picks retain an on-surface point, but their identity is the body and topology index.
pub(crate) fn same_item(a: &Sel, b: &Sel) -> bool {
    match (a, b) {
        (Sel::Edge { body: a, index: i, .. }, Sel::Edge { body: b, index: j, .. })
        | (Sel::Face { body: a, index: i, .. }, Sel::Face { body: b, index: j, .. }) => a == b && i == j,
        _ => a == b,
    }
}

/// Two distinct point intents for the alignment command, without creating sketch points.
pub(crate) fn alignment_points(selection: &[Sel]) -> Option<[serde_json::Value; 2]> {
    let [a, b] = selection else { return None };
    let point = |s: &Sel| match s {
        Sel::SketchPoint { id } => Some(serde_json::json!(id)),
        Sel::SketchMidpoint { id } => Some(serde_json::json!({"midpoint": id})),
        _ => None,
    };
    let (a, b) = (point(a)?, point(b)?);
    (a != b).then_some([a, b])
}

/// The current middle of a finite straight sketch edge.
pub(crate) fn line_midpoint(sk: &solidcraft_engine::sketch::Sketch, line: usize) -> Option<solidcraft_engine::geom::Vec2> {
    let curve = sk.curves.get(line)?;
    if curve.infinite {
        return None;
    }
    let solidcraft_engine::sketch::CurveKind::Line { a, b } = &curve.kind else { return None };
    Some((sk.point(*a)? + sk.point(*b)?) * 0.5)
}

/// What a selection input accepts (bit set).
pub type Accept = u16;
pub const PROFILES: Accept = 1;
pub const EDGES: Accept = 2;
pub const FACES: Accept = 4;
/// Planar faces only.
pub const PLANAR_FACES: Accept = 8;
pub const BODIES: Accept = 16;
/// Origin and construction planes.
pub const PLANES: Accept = 32;
/// Origin axes and straight sketch curves.
pub const AXES: Accept = 64;
pub const VERTICES: Accept = 128;
/// Any sketch curve (a sweep path).
pub const CURVES: Accept = 256;
/// Sketch points of any visible sketch (hole centres); picked as "<sketch id>:<point id>".
pub const POINTS: Accept = 512;
/// Timeline features (pattern objects), picked in the timeline.
pub const FEATURES: Accept = 1024;
/// Points of the sketch being modified; unlike POINTS these never enable cross-sketch picking.
pub const SKETCH_POINTS: Accept = 2048;
/// The origin point, picked as a vertex of no body (in the viewport or the browser's Origin).
pub const ORIGIN: Accept = 4096;
/// Curves of any sketch, picked with their sketch (`ExternalSketchCurve`): curve ids are a
/// sketch's own, so two sketches can both have a `w2` (frame member lines).
pub const SKETCH_LINES: Accept = 8192;
/// Sketch points with an explicit source sketch; only for commands that consume external references.
pub const EXTERNAL_POINTS: Accept = 16384;
/// Named construction points.
pub const CONSTRUCTION_POINTS: Accept = 32768;
/// What a 3D axis input takes (Revolve, Circular Pattern, Plane at Angle, Move's Rotate): an
/// origin or construction axis, a straight sketch line, and an edge or face of the model that
/// has an axis (a straight or circular edge, a cylinder, cone or torus face; the dialog takes
/// only those, `Dialog::candidate_on`).
pub const AXIS_PICKS: Accept = AXES | EDGES | FACES;

/// One selection input of a command dialog ("Profiles", "Edges", "Plane", ...).
#[derive(Clone, Debug, PartialEq)]
pub struct SelInput {
    pub label: &'static str,
    pub accept: Accept,
    /// Many items (clicks toggle them), or one (a click replaces it).
    pub multi: bool,
    /// Face picks are places on the face, not the face: several on one face add up (a click on
    /// the very spot of one takes it out). For points that pick pieces a face is cut into.
    pub places: bool,
    pub items: Vec<Sel>,
}

impl SelInput {
    pub fn new(label: &'static str, accept: Accept, multi: bool) -> SelInput {
        SelInput { label, accept, multi, places: false, items: Vec::new() }
    }

    /// This input with face picks as places (see [`SelInput::places`]).
    pub fn places(mut self) -> SelInput {
        self.places = true;
        self
    }

    /// The selection a hit stands for, if this input accepts it. A curve of another sketch than
    /// the `active` one names its sketch: curve ids repeat from one sketch to the next (#3726).
    pub fn accepts(&self, h: &Hit, active: Option<u64>, planar: impl Fn(&str, usize) -> bool) -> Option<Sel> {
        let a = self.accept;
        match h {
            Hit::Profile { sketch, index } if a & PROFILES != 0 => Some(Sel::Profile { sketch: *sketch, index: *index }),
            Hit::Edge { body, index, mid, .. } if a & EDGES != 0 => Some(Sel::Edge { body: body.clone(), index: *index, point: *mid }),
            Hit::Face { body, index, point } if a & FACES != 0 || (a & PLANAR_FACES != 0 && planar(body, *index)) => {
                Some(Sel::Face { body: body.clone(), index: *index, point: *point })
            }
            Hit::Face { body, .. } if a & BODIES != 0 => Some(Sel::Body { name: body.clone() }),
            Hit::Plane { name, .. } if a & PLANES != 0 => Some(Sel::Plane { name: name.clone() }),
            Hit::Axis { name } if a & AXES != 0 => Some(Sel::Axis { name: name.clone() }),
            Hit::ConstructionPoint { name } if a & CONSTRUCTION_POINTS != 0 => Some(Sel::ConstructionPoint { name: name.clone() }),
            // A straight sketch line stands for an axis only in an input that takes axes for
            // what they are: an input that takes curves too (Measure) takes the line as its curve.
            Hit::SketchCurve { sketch, id, straight: true, .. } if a & AXES != 0 && a & CURVES == 0 => {
                Some(curve_pick(id, *sketch, active))
            }
            Hit::SketchCurve { sketch, id, .. } if a & SKETCH_LINES != 0 => Some(Sel::ExternalSketchCurve { sketch: *sketch, id: id.clone() }),
            Hit::SketchCurve { sketch, id, .. } if a & CURVES != 0 => Some(curve_pick(id, *sketch, active)),
            Hit::SketchPoint { sketch, id, .. } if a & EXTERNAL_POINTS != 0 => Some(Sel::ExternalSketchPoint { sketch: *sketch, id: id.clone() }),
            Hit::SketchPoint { id, .. } if a & SKETCH_POINTS != 0 => Some(Sel::SketchPoint { id: id.clone() }),
            Hit::SketchPoint { sketch, id, .. } if a & POINTS != 0 => Some(Sel::SketchPoint { id: format!("{sketch}:{id}") }),
            Hit::Vertex { body, point } if body.is_empty() && a & ORIGIN != 0 => Some(Sel::Vertex { body: String::new(), point: *point }),
            Hit::Vertex { body, point } if !body.is_empty() && a & VERTICES != 0 => Some(Sel::Vertex { body: body.clone(), point: *point }),
            _ => None,
        }
    }

    /// Add or remove items: in a multi input a pick toggles, otherwise it replaces.
    pub fn toggle(&mut self, picked: Vec<Sel>) {
        if picked.is_empty() {
            return;
        }
        if !self.multi {
            self.items = picked.into_iter().take(1).collect();
            return;
        }
        // A pick of something already in (or a chain fully in) takes it out.
        let places = self.places;
        // Whether two picks are the same item of this input.
        let same = |a: &Sel, b: &Sel| match (a, b) {
            (Sel::Face { point: p, .. }, Sel::Face { point: q, .. }) if places => same_item(a, b) && p.dist(*q) <= 1e-9,
            _ => same_item(a, b),
        };
        if picked.iter().all(|p| self.items.iter().any(|x| same(x, p))) {
            self.items.retain(|x| !picked.iter().any(|p| same(x, p)));
        } else {
            for p in picked {
                if !self.items.iter().any(|x| same(x, &p)) {
                    self.items.push(p);
                }
            }
        }
    }
}

/// A sketch curve as a dialog's pick. A curve of another sketch than the `active` one names its
/// sketch: curve ids repeat from one sketch to the next (#3726).
pub(crate) fn curve_pick(id: &str, sketch: u64, active: Option<u64>) -> Sel {
    Sel::SketchCurve { id: id.to_string(), sketch: (active != Some(sketch)).then_some(sketch) }
}

/// Projections preserve pick order and repetitions; callers own any deduplication or frames.
pub(crate) trait SelectionExt {
    fn body_names(&self) -> Vec<String>;
    fn curve_ids(&self) -> Vec<String>;
    /// The sketch every curve pick names, when they all name the same one (#3726).
    fn named_sketch(&self) -> Option<u64>;
    fn profiles(&self) -> Vec<(u64, usize)>;
    fn edge_points(&self) -> Vec<Vec3>;
    fn face_points(&self) -> Vec<Vec3>;
}

impl SelectionExt for [Sel] {
    fn body_names(&self) -> Vec<String> {
        self.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.clone()) } else { None }).collect()
    }
    fn curve_ids(&self) -> Vec<String> {
        self.iter().filter_map(|x| if let Sel::SketchCurve { id, .. } = x { Some(id.clone()) } else { None }).collect()
    }
    fn named_sketch(&self) -> Option<u64> {
        let mut named = self.iter().filter_map(|x| if let Sel::SketchCurve { sketch, .. } = x { Some(*sketch) } else { None });
        let first = named.next()??;
        named.all(|k| k == Some(first)).then_some(first)
    }
    fn profiles(&self) -> Vec<(u64, usize)> {
        self.iter().filter_map(|x| if let Sel::Profile { sketch, index } = x { Some((*sketch, *index)) } else { None }).collect()
    }
    fn edge_points(&self) -> Vec<Vec3> {
        self.iter().filter_map(|x| if let Sel::Edge { point, .. } = x { Some(*point) } else { None }).collect()
    }
    fn face_points(&self) -> Vec<Vec3> {
        self.iter().filter_map(|x| if let Sel::Face { point, .. } = x { Some(*point) } else { None }).collect()
    }
}

pub(crate) fn items(inputs: &[SelInput], i: usize) -> &[Sel] {
    inputs.get(i).map(|x| x.items.as_slice()).unwrap_or(&[])
}

pub(crate) fn point_value(v: Vec3) -> serde_json::Value {
    serde_json::json!([v.x, v.y, v.z])
}

pub(crate) fn polyline_mid(pts: &[Vec3]) -> Vec3 {
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= len * 0.5 && l > 0.0 {
            return w[0].lerp(w[1], (len * 0.5 - acc) / l);
        }
        acc += l;
    }
    pts.first().copied().unwrap_or_default()
}

/// World size of the origin widget: a fixed fraction of the viewport height, whatever the zoom
/// (`half_height` is half the visible height in mm at the target).
pub fn origin_size(half_height: f64) -> f64 {
    (half_height * 2.0 * 0.075).max(1e-6)
}

/// The three origin plane squares (positive quadrant): name, plane normal and corners.
pub fn origin_quads(size: f64) -> [(&'static str, Vec3, [Vec3; 4]); 3] {
    let s = size;
    [
        ("XY", Vec3::Z, [Vec3::ZERO, Vec3::new(s, 0.0, 0.0), Vec3::new(s, s, 0.0), Vec3::new(0.0, s, 0.0)]),
        ("XZ", Vec3::new(0.0, -1.0, 0.0), [Vec3::ZERO, Vec3::new(s, 0.0, 0.0), Vec3::new(s, 0.0, s), Vec3::new(0.0, 0.0, s)]),
        ("YZ", Vec3::X, [Vec3::ZERO, Vec3::new(0.0, s, 0.0), Vec3::new(0.0, s, s), Vec3::new(0.0, 0.0, s)]),
    ]
}

/// Ray parameter where the ray (o, d) hits the planar quad (corners in order), if it does.
pub fn ray_quad(o: Vec3, d: Vec3, q: &[Vec3; 4]) -> Option<f64> {
    let [a, b, _, dd] = *q;
    let (u, v) = (b - a, dd - a);
    let n = u.cross(v);
    let den = n.dot(d);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = n.dot(a - o) / den;
    if !(t.is_finite() && t >= 0.0) {
        return None;
    }
    let p = o + d * t - a;
    // Coordinates in the (u, v) frame (the quad is a parallelogram).
    let (uu, uv, vv) = (u.dot(u), u.dot(v), v.dot(v));
    let det = uu * vv - uv * uv;
    if det.abs() < 1e-300 {
        return None;
    }
    let (pu, pv) = (p.dot(u), p.dot(v));
    let s = (pu * vv - pv * uv) / det;
    let r = (pv * uu - pu * uv) / det;
    ((0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&r)).then_some(t)
}

/// A rectangle dragged in the viewport selects by window (dragged left to right: items fully
/// inside) or by crossing (right to left: items touching it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxSel {
    pub rect: Rect,
    pub crossing: bool,
}

impl BoxSel {
    pub fn from_drag(start: Pos2, end: Pos2) -> BoxSel {
        BoxSel { rect: Rect::from_two_pos(start, end), crossing: end.x < start.x }
    }

    /// Does the polyline (screen points) count as selected?
    pub fn takes(&self, pts: &[Pos2]) -> bool {
        if pts.is_empty() {
            return false;
        }
        if !self.crossing {
            return pts.iter().all(|p| self.rect.contains(*p));
        }
        pts.iter().any(|p| self.rect.contains(*p)) || pts.windows(2).any(|w| segment_hits_rect(w[0], w[1], self.rect))
    }
}

/// Does the segment a–b cross the rectangle's boundary or lie inside it?
pub fn segment_hits_rect(a: Pos2, b: Pos2, r: Rect) -> bool {
    if r.contains(a) || r.contains(b) {
        return true;
    }
    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    (0..4).any(|i| segments_cross(a, b, c[i], c[(i + 1) % 4]))
}

fn segments_cross(p: Pos2, q: Pos2, r: Pos2, s: Pos2) -> bool {
    let o = |a: Pos2, b: Pos2, c: Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let (d1, d2, d3, d4) = (o(r, s, p), o(r, s, q), o(p, q, r), o(p, q, s));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;
    use solidcraft_engine::render::{Camera, StandardView};

    #[test]
    fn origin_widget_keeps_its_screen_size() {
        let (w, h) = (1200.0, 800.0);
        let mut sizes = Vec::new();
        for distance in [20.0, 200.0, 5000.0] {
            let mut cam = Camera { distance, ..Default::default() };
            cam.set_view(StandardView::Front);
            let s = origin_size(cam.half_height());
            let a = cam.to_screen(Vec3::ZERO, w, h, distance).unwrap();
            let b = cam.to_screen(Vec3::new(s, 0.0, 0.0), w, h, distance).unwrap();
            sizes.push(b.0 - a.0);
        }
        assert!(sizes.iter().all(|x| (x - sizes[0]).abs() < 1e-6 && *x > 40.0), "{sizes:?}");
    }

    #[test]
    fn rays_hit_origin_quads() {
        let q = origin_quads(10.0);
        // Straight down onto XY at (3, 4).
        let t = ray_quad(Vec3::new(3.0, 4.0, 50.0), Vec3::new(0.0, 0.0, -1.0), &q[0].2).unwrap();
        assert!((t - 50.0).abs() < 1e-12);
        // Outside the quadrant: misses.
        assert!(ray_quad(Vec3::new(-3.0, 4.0, 50.0), Vec3::new(0.0, 0.0, -1.0), &q[0].2).is_none());
        // Parallel: misses.
        assert!(ray_quad(Vec3::new(3.0, 4.0, 50.0), Vec3::X, &q[0].2).is_none());
        // From the front onto XZ.
        assert!(ray_quad(Vec3::new(5.0, -100.0, 5.0), Vec3::Y, &q[1].2).is_some());
        // Behind the ray origin: misses.
        assert!(ray_quad(Vec3::new(3.0, 4.0, -50.0), Vec3::new(0.0, 0.0, -1.0), &q[0].2).is_none());
    }

    #[test]
    fn window_and_crossing_selection() {
        let inside = [pos2(20.0, 20.0), pos2(40.0, 30.0)];
        let partly = [pos2(20.0, 20.0), pos2(140.0, 30.0)];
        let through = [pos2(-10.0, 50.0), pos2(200.0, 50.0)];
        let away = [pos2(300.0, 300.0), pos2(320.0, 330.0)];
        // Left to right: window.
        let w = BoxSel::from_drag(pos2(0.0, 0.0), pos2(100.0, 100.0));
        assert!(!w.crossing);
        assert!(w.takes(&inside) && !w.takes(&partly) && !w.takes(&through) && !w.takes(&away));
        // Right to left: crossing.
        let c = BoxSel::from_drag(pos2(100.0, 100.0), pos2(0.0, 0.0));
        assert!(c.crossing);
        assert!(c.takes(&inside) && c.takes(&partly) && c.takes(&through) && !c.takes(&away));
    }

    #[test]
    fn inputs_toggle_and_filter() {
        let mut inp = SelInput::new("Edges", EDGES, true);
        let e = |i: usize| Sel::Edge { body: "B".into(), index: i, point: Vec3::new(i as f64, 0.0, 0.0) };
        inp.toggle(vec![e(1)]);
        inp.toggle(vec![e(2), e(3)]);
        assert_eq!(inp.items.len(), 3);
        inp.toggle(vec![e(2), e(3)]);
        assert_eq!(inp.items, vec![e(1)]);
        let face = Hit::Face { body: "B".into(), index: 0, point: Vec3::ZERO };
        assert!(inp.accepts(&face, None, |_, _| true).is_none());
        let planes = SelInput::new("Plane", PLANES | PLANAR_FACES, false);
        assert!(planes.accepts(&face, None, |_, _| true).is_some());
        assert!(planes.accepts(&face, None, |_, _| false).is_none());
        let mut one = SelInput::new("Plane", PLANES, false);
        one.toggle(vec![Sel::Plane { name: "XY".into() }]);
        one.toggle(vec![Sel::Plane { name: "YZ".into() }]);
        assert_eq!(one.items, vec![Sel::Plane { name: "YZ".into() }]);
    }
}

#[cfg(test)]
mod shared_geometry_baseline_tests {
    use super::*;

    #[test]
    fn polyline_mid_preserves_degenerate_and_unequal_segments() {
        let p = Vec3::new(10.0, -3.0, 7.0);
        assert_eq!(polyline_mid(&[]), Vec3::ZERO);
        assert_eq!(polyline_mid(&[p]), p);
        assert_eq!(polyline_mid(&[p, p, p]), p);
        assert_eq!(polyline_mid(&[p, p + Vec3::X, p + Vec3::X * 10.0]), p + Vec3::X * 5.0);
        assert_eq!(polyline_mid(&[p, p, p + Vec3::X * 2.0, p + Vec3::X * 2.0 + Vec3::Y * 6.0]), p + Vec3::X * 2.0 + Vec3::Y * 2.0);
        assert_eq!(polyline_mid(&[p, Vec3::new(f64::NAN, 0.0, 0.0)]), p);
    }
}
