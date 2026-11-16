//! The original Mecha navigator: a small, articulated right hand, built once from analytic
//! bevelled solids. It is a display mesh, never a design body. The middle finger follows the
//! world's up axis; its nail is on the back of the fingertip, opposite the palm.
//!
//! Canonical coordinates are X right, Y into the palm, Z up. The thumb folds across +Y, and
//! remains on the left in the front (dorsal) view. Three curled fingers and the thumb have
//! their own distal nails; no nail is painted onto a knuckle or a fingertip pad.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use solidcore_record::Vec3;
use solidcraft_engine::geom::{Mesh, Vec3Geom};
use solidcraft_viewport::{Camera, UpAxis};

use crate::view_gizmo::Shape;

#[path = "../../../assets/appearances/mecha-navigator.rs"]
mod material;

/// The immutable model in the world's up frame. Each cached shape has a stable identity for
/// the navigator's raster cache; changing the design never rebuilds this mesh.
pub(crate) fn shape(up: UpAxis) -> Option<Shape> {
    static Z_UP: OnceLock<Option<Shape>> = OnceLock::new();
    static Y_UP: OnceLock<Option<Shape>> = OnceLock::new();
    let cache = match up {
        UpAxis::Z => &Z_UP,
        UpAxis::Y => &Y_UP,
    };
    cache
        .get_or_init(|| {
            let mut model = authored();
            for point in &mut model.mesh.positions {
                *point = up.from_frame(*point);
            }
            for normal in &mut model.mesh.normals {
                *normal = up.from_frame(*normal);
            }
            for line in &mut model.mesh.edges {
                for point in line {
                    *point = up.from_frame(*point);
                }
            }
            Shape::of("Mecha", &model.mesh, Some(material::GRAPHITE), &model.colours)
        })
        .clone()
}

/// Preferences uses the real mesh and the same occlusion and shading as the live navigator.
pub(crate) fn paint_preview(painter: &egui::Painter, rect: egui::Rect) {
    if let Some(shape) = shape(UpAxis::Z) {
        let camera = Camera { yaw: 18.0_f64.to_radians(), pitch: 9.0_f64.to_radians(), ..Camera::default() };
        crate::viewport::paint_navigator_preview(painter, rect, &shape, &camera);
    }
}

#[derive(Default)]
struct Model {
    mesh: Mesh,
    colours: BTreeMap<u32, [u8; 3]>,
    next_face: u32,
}

impl Model {
    /// Add one convex, flat-shaded polygon. Winding follows the outside of the authored solid,
    /// so the renderer can cull its back faces from every view, including underneath the hand.
    fn face(&mut self, points: &[Vec3], outside: Vec3, colour: [u8; 3], outline: bool) {
        let [a, b, c, ..] = points else { return };
        let Some(mut normal) = (*b - *a).cross(*c - *a).normalized() else { return };
        let mut points = points.to_vec();
        if normal.dot(outside) < 0.0 {
            points.reverse();
            normal = -normal;
        }
        let Ok(base) = u32::try_from(self.mesh.positions.len()) else { return };
        let face = self.next_face;
        self.next_face = face.saturating_add(1);
        self.colours.insert(face, colour);
        self.mesh.positions.extend(points.iter().copied());
        self.mesh.normals.extend(std::iter::repeat_n(normal, points.len()));
        for i in 1..points.len().saturating_sub(1) {
            let Some(b) = u32::try_from(i).ok().and_then(|i| base.checked_add(i)) else { continue };
            let Some(c) = b.checked_add(1) else { continue };
            self.mesh.triangles.push([base, b, c]);
            self.mesh.tri_face.push(face);
        }
        if outline && let Some(first) = points.first().copied() {
            points.push(first);
            self.mesh.edges.push(points);
            self.mesh.edge_faces.push(vec![face]);
        }
    }

    /// A closed solid through corresponding convex rings. All callers use small, fixed rings;
    /// there is no unbounded tessellation or input-dependent allocation in the asset.
    fn loft(&mut self, rings: &[Vec<Vec3>], sides: &[[u8; 3]], cap: [u8; 3], outline_caps: bool) {
        let (Some(first), Some(last)) = (rings.first(), rings.last()) else { return };
        let centre = average(&rings.iter().flatten().copied().collect::<Vec<_>>());
        self.face(first, average(first) - centre, cap, outline_caps);
        self.face(last, average(last) - centre, cap, outline_caps);
        for (section, pair) in rings.windows(2).enumerate() {
            let [lower, upper] = pair else { continue };
            let n = lower.len();
            if n < 3 || upper.len() != n {
                continue;
            }
            for i in 0..n {
                let j = (i + 1) % n;
                let (Some(a), Some(b), Some(c), Some(d)) = (lower.get(i), lower.get(j), upper.get(j), upper.get(i)) else { continue };
                let face = [*a, *b, *c, *d];
                self.face(&face, average(&face) - centre, sides.get(section).copied().unwrap_or(cap), false);
            }
        }
    }

    /// A chamfered phalanx. The local cross-section's depth points toward its dorsal surface.
    fn bone(&mut self, start: Vec3, end: Vec3, width: f64, depth: f64, taper: f64) {
        let Some(direction) = (end - start).normalized() else { return };
        let Some(dorsal) = Vec3::X.cross(direction).normalized() else { return };
        let right = direction.cross(dorsal);
        let ring = |t: f64, inset: f64| {
            let size = (1.0 + (taper - 1.0) * t) * inset;
            octagon(start + (end - start) * t, right, dorsal, width * 0.5 * size, depth * 0.5 * size)
        };
        self.loft(
            &[ring(0.0, 0.82), ring(0.12, 1.0), ring(0.88, 1.0), ring(1.0, 0.80)],
            &[material::BEVEL, material::ARMOUR, material::BEVEL],
            material::GRAPHITE,
            false,
        );
    }

    /// A transverse hinge with two small metal end caps. Eight sides are enough at gizmo size.
    fn hinge(&mut self, at: Vec3, width: f64, radius: f64) {
        self.cylinder(at, Vec3::X, width * 0.93, radius, material::JOINT);
        for side in [-1.0, 1.0] {
            self.cylinder(at + Vec3::X * (side * width * 0.51), Vec3::X, width * 0.10, radius * 0.86, material::TITANIUM);
            let inset: Vec<_> = (0..8)
                .map(|i| {
                    let angle = f64::from(i) * std::f64::consts::TAU / 8.0;
                    at + Vec3::X * (side * width * 0.562) + (Vec3::Y * angle.cos() + Vec3::Z * angle.sin()) * (radius * 0.55)
                })
                .collect();
            self.face(&inset, Vec3::X * side, material::GRAPHITE, false);
        }
    }

    fn cylinder(&mut self, at: Vec3, axis: Vec3, length: f64, radius: f64, colour: [u8; 3]) {
        let reference = if axis.z.abs() < 0.8 { Vec3::Z } else { Vec3::Y };
        let Some(a) = axis.cross(reference).normalized() else { return };
        let b = axis.cross(a);
        let ring = |along: f64, radius: f64| {
            (0..8)
                .map(|i| {
                    let angle = f64::from(i) * std::f64::consts::TAU / 8.0;
                    at + axis * along + (a * angle.cos() + b * angle.sin()) * radius
                })
                .collect()
        };
        self.loft(
            &[ring(-length * 0.5, radius * 0.84), ring(-length * 0.32, radius), ring(length * 0.32, radius), ring(length * 0.5, radius * 0.84)],
            &[colour, colour, colour],
            colour,
            false,
        );
    }

    /// One raised plate with a broad bevel. The face is offset from the underlying body, so
    /// both its rim and the dark seam remain visible without a texture or a painted fake gap.
    fn plate(&mut self, points: &[Vec3], normal: Vec3, colour: [u8; 3], bevel: [u8; 3], outline: bool) {
        let centre = average(points);
        let back: Vec<_> = points.iter().map(|p| *p - normal * 0.012).collect();
        let front: Vec<_> = points.iter().map(|p| centre + (*p - centre) * 0.84 + normal * 0.024).collect();
        self.loft(&[back, points.to_vec(), front], &[material::GRAPHITE, bevel], colour, outline);
    }

    /// A real nail plate on the distal segment's dorsal surface, following that segment's
    /// bend. The palm-facing pad remains graphite even when the camera turns behind the hand.
    fn nail(&mut self, start: Vec3, end: Vec3, width: f64, depth: f64, dorsal: Vec3) {
        let Some(direction) = (end - start).normalized() else { return };
        let Some(right) = direction.cross(dorsal).normalized() else { return };
        let length = (end - start).len();
        let at = |x: f64, t: f64| start + direction * (length * t) + right * (width * x) + dorsal * (depth * 0.51);
        let points =
            [at(-0.34, 0.20), at(0.34, 0.20), at(0.43, 0.30), at(0.35, 0.88), at(0.24, 1.00), at(-0.24, 1.00), at(-0.35, 0.88), at(-0.43, 0.30)];
        self.plate(&points, dorsal, material::NAIL, material::NAIL_BEVEL, true);
        // A slim planar highlight gives the pink lacquer a readable shine at small sizes.
        let gleam = [at(-0.24, 0.35), at(-0.12, 0.32), at(-0.12, 0.84), at(-0.20, 0.87)].map(|p| p + dorsal * 0.027);
        self.face(&gleam, dorsal, material::NAIL_GLEAM, false);
    }
}

fn average(points: &[Vec3]) -> Vec3 {
    points.iter().copied().fold(Vec3::ZERO, |sum, p| sum + p) * (1.0 / points.len().max(1) as f64)
}

fn octagon(centre: Vec3, right: Vec3, dorsal: Vec3, half_width: f64, half_depth: f64) -> Vec<Vec3> {
    [(-0.70, -1.0), (0.70, -1.0), (1.0, -0.68), (1.0, 0.68), (0.70, 1.0), (-0.70, 1.0), (-1.0, 0.68), (-1.0, -0.68)]
        .into_iter()
        .map(|(x, y)| centre + right * (x * half_width) + dorsal * (y * half_depth))
        .collect()
}

fn palm_ring(z: f64, width: f64, depth: f64, y: f64) -> Vec<Vec3> {
    octagon(Vec3::new(0.0, y, z), Vec3::X, Vec3::Y, width * 0.5, depth * 0.5)
}

fn authored() -> Model {
    let mut model = Model::default();
    // A narrow cuff and tapered palm keep the silhouette feminine and distinct from a glove.
    model.loft(
        &[palm_ring(-0.99, 0.44, 0.31, 0.0), palm_ring(-0.96, 0.48, 0.35, 0.0), palm_ring(-0.79, 0.49, 0.36, 0.0), palm_ring(-0.76, 0.45, 0.33, 0.0)],
        &[material::BEVEL, material::GRAPHITE, material::JOINT],
        material::GRAPHITE,
        false,
    );
    model.loft(
        &[
            palm_ring(-0.77, 0.48, 0.34, 0.0),
            palm_ring(-0.63, 0.61, 0.42, 0.015),
            palm_ring(-0.18, 0.86, 0.46, 0.025),
            palm_ring(0.025, 0.82, 0.43, 0.045),
        ],
        &[material::GRAPHITE, material::GRAPHITE, material::ARMOUR],
        material::GRAPHITE,
        false,
    );
    // Four metacarpal plates converge toward the wrist. Narrow gaps expose the dark body.
    for outline in [
        vec![(-0.23, -0.67), (-0.38, -0.27), (-0.39, -0.015), (-0.28, 0.065), (-0.19, -0.11), (-0.15, -0.58)],
        vec![(-0.13, -0.68), (-0.185, -0.16), (-0.17, 0.015), (-0.075, 0.095), (0.02, 0.015), (0.015, -0.62)],
        vec![(0.045, -0.64), (0.045, 0.015), (0.14, 0.08), (0.24, 0.005), (0.255, -0.24), (0.18, -0.70)],
        vec![(0.205, -0.65), (0.275, -0.05), (0.365, 0.005), (0.435, -0.13), (0.395, -0.35), (0.28, -0.73)],
    ] {
        let points: Vec<_> = outline.into_iter().map(|(x, z)| Vec3::new(x, -0.225, z)).collect();
        model.plate(&points, -Vec3::Y, material::ARMOUR, material::BEVEL, true);
    }
    // Palm pads have different geometry, so front and back remain identifiable after orbiting.
    for outline in [
        vec![(-0.30, -0.61), (-0.39, -0.22), (-0.25, -0.04), (-0.06, -0.21), (-0.09, -0.50)],
        vec![(0.01, -0.52), (-0.015, -0.20), (0.23, -0.08), (0.37, -0.22), (0.28, -0.59)],
    ] {
        let points: Vec<_> = outline.into_iter().map(|(x, z)| Vec3::new(x, 0.245, z)).collect();
        model.plate(&points, Vec3::Y, material::GRAPHITE, material::ARMOUR, true);
    }
    for side in [-1.0, 1.0] {
        model.cylinder(Vec3::new(side * 0.25, 0.0, -0.75), Vec3::X, 0.035, 0.065, material::TITANIUM);
    }

    // Middle finger: three phalanges, two visible interphalangeal hinges, plus its base joint.
    let middle =
        [Vec3::new(-0.075, 0.0, 0.035), Vec3::new(-0.075, -0.008, 0.455), Vec3::new(-0.075, -0.014, 0.775), Vec3::new(-0.075, -0.010, 1.035)];
    for (i, pair) in middle.windows(2).enumerate() {
        let [a, b] = pair else { continue };
        let direction = *b - *a;
        let (width, depth) = [(0.185, 0.205), (0.161, 0.180), (0.144, 0.164)].get(i).copied().unwrap_or((0.144, 0.164));
        model.bone(*a + direction * 0.055, *b - direction * 0.055, width, depth, 0.91);
        model.hinge(*a, width, depth * 0.40);
        if i == 2 {
            let dorsal = Vec3::X.cross(direction).normalized().unwrap_or(-Vec3::Y);
            model.nail(*a + direction * 0.055, *b - direction * 0.04, width * 0.94, depth * 0.93, dorsal);
        }
    }

    // The other three digits bend into the palm. Their distal nail normals follow the curl,
    // rather than being copied onto the outward-facing knuckles.
    for (x, root, width, length) in [(-0.285, 0.005, 0.185, 1.0), (0.145, -0.005, 0.180, 0.96), (0.34, -0.075, 0.150, 0.79)] {
        let joints = [
            Vec3::new(x, 0.015, root),
            Vec3::new(x, 0.195 * length, root + 0.205 * length),
            Vec3::new(x, 0.385 * length, root + 0.095 * length),
            Vec3::new(x, 0.335 * length, root - 0.135 * length),
        ];
        for (i, pair) in joints.windows(2).enumerate() {
            let [a, b] = pair else { continue };
            let direction = *b - *a;
            let k = 1.0 - i as f64 * 0.09;
            model.bone(*a + direction * 0.07, *b - direction * 0.07, width * k, width * k, 0.93);
            model.hinge(*a, width * k, width * k * 0.40);
            if i == 2 {
                let dorsal = Vec3::X.cross(direction).normalized().unwrap_or(Vec3::Y);
                model.nail(*a + direction * 0.10, *b - direction * 0.08, width * k * 0.92, width * k * 0.93, dorsal);
            }
        }
    }

    // The thumb's two phalanges cross the PALM, leaving the metacarpal plates unobstructed.
    // Its base remains on the radial side (−X), and the distal nail is on its outer surface.
    let thumb = [Vec3::new(-0.37, 0.095, -0.43), Vec3::new(-0.43, 0.34, -0.14), Vec3::new(-0.12, 0.46, -0.10)];
    for (i, pair) in thumb.windows(2).enumerate() {
        let [a, b] = pair else { continue };
        let direction = *b - *a;
        // A thumb points across X, so author its section in a frame aligned to its own bone.
        let Some(along) = direction.normalized() else { continue };
        let reference = Vec3::new(0.0, 0.95, 0.31);
        let Some(right) = along.cross(reference).normalized() else { continue };
        let dorsal = right.cross(along);
        let width = if i == 0 { 0.205 } else { 0.18 };
        let ring = |t: f64, bevel: f64| octagon(*a + direction * t, right, dorsal, width * 0.5 * bevel, 0.085 * bevel);
        model.loft(
            &[ring(0.06, 0.80), ring(0.16, 1.0), ring(0.85, 0.93), ring(0.96, 0.74)],
            &[material::BEVEL, material::ARMOUR, material::BEVEL],
            material::GRAPHITE,
            false,
        );
        model.cylinder(*a, right, width * 1.04, 0.075, material::JOINT);
        for sign in [-1.0, 1.0] {
            model.cylinder(*a + right * (width * 0.50 * sign), right, 0.022, 0.057, material::TITANIUM);
        }
        if i == 1 {
            model.nail(*a + direction * 0.08, *b - direction * 0.04, width * 0.95, 0.16, dorsal);
        }
    }
    model
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::geom::MeshGeom;

    #[test]
    fn original_hand_is_a_small_finite_non_degenerate_mesh() {
        let hand = shape(UpAxis::Z).expect("the authored hand fits navigator limits");
        let mesh = &hand.mesh;
        assert!(mesh.triangles.len() < 5_000, "small enough to raster while dragging");
        assert_eq!(mesh.positions.len(), mesh.normals.len());
        assert_eq!(mesh.triangles.len(), mesh.tri_face.len());
        assert!(mesh.positions.iter().all(|p| p.is_finite()));
        assert!(mesh.normals.iter().all(|n| n.is_finite() && (n.len() - 1.0).abs() < 1e-6));
        for triangle in &mesh.triangles {
            let [a, b, c] = mesh.tri(triangle).expect("every triangle index exists");
            let normal = (b - a).cross(c - a);
            assert!(normal.len() > 1e-10, "no zero-area facets");
            for index in triangle {
                assert!(mesh.normals[*index as usize].dot(normal) > 0.0, "normals agree with winding");
            }
        }
        let size = mesh.bounds().size();
        assert!((size.z - 2.0).abs() < 1e-3);
        assert!(size.x > 0.7 && size.y > 0.4, "a recognisable hand from side views, {size:?}");
        assert!(Shape::from_json(&hand.to_json()).is_some(), "the normal shape validation accepts the model");
    }

    #[test]
    fn middle_nail_is_on_the_dorsal_tip_and_thumb_is_on_the_radial_side() {
        let model = authored();
        let mut middle_nail_points = Vec::new();
        let mut thumb_nail_points = Vec::new();
        for (triangle, face) in model.mesh.triangles.iter().zip(&model.mesh.tri_face) {
            if model.colours.get(face) != Some(&material::NAIL) {
                continue;
            }
            let points = model.mesh.tri(triangle).unwrap();
            let centre = average(&points);
            if centre.z > 0.8 {
                middle_nail_points.extend(points);
            } else if centre.x < -0.15 && centre.y > 0.50 {
                thumb_nail_points.extend(points);
            }
        }
        assert!(!middle_nail_points.is_empty());
        assert!(middle_nail_points.iter().all(|p| p.y < -0.06), "the pink nail never appears on the finger pad");
        assert!(middle_nail_points.iter().all(|p| p.z > 0.78 && p.x > -0.16 && p.x < 0.01));
        assert!(!thumb_nail_points.is_empty(), "the folded thumb has its own nail on the palm side");
        assert!(thumb_nail_points.iter().all(|p| p.x < 0.0 && p.z < 0.05));
    }

    #[test]
    fn changing_world_up_rotates_the_same_model_and_reuses_each_cached_shape() {
        let z = shape(UpAxis::Z).unwrap();
        let y = shape(UpAxis::Y).unwrap();
        assert_eq!(z.id, shape(UpAxis::Z).unwrap().id);
        assert_eq!(y.id, shape(UpAxis::Y).unwrap().id);
        assert_ne!(z.id, y.id);
        assert_eq!(z.mesh.triangles, y.mesh.triangles);
        assert_eq!(z.mesh.tri_face, y.mesh.tri_face);
        assert_eq!(z.face_colours, y.face_colours);
        for (a, b) in z.mesh.positions.iter().zip(&y.mesh.positions) {
            assert!(UpAxis::Y.from_frame(*a).dist(*b) < 2e-4);
        }
        let size = y.mesh.bounds().size();
        assert!((size.y - 2.0).abs() < 1e-3, "the finger follows Y-up");
    }
}
