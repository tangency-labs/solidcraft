//! Relations: joints that tie only some directions or points of two parts (B parallel to A, B's
//! point on A's plane…), so several of them between one pair of parts build up the placement the
//! way a designer thinks of it: on that face, square to that edge, 40 mm from that end. The
//! solver treats them as joints with more values; a second relation between the same parts closes
//! a loop, and the loop solver meets every relation at once (or reports the one that can't be).
//!
//! A relation is made where the parts are: its values start as the nearest that meet it, so B
//! moves no further than the relation needs.

use super::{Constraint, Joint, Snap, component_bodies, component_of, least_squares, link_of, occurrence_world, resolve_origin};
use crate::{Document, IDENTITY, Mat, ModelState, mat_inverse, mat_mul};

/// Starting turns tried for each angle value: the nearest values may lie a half turn away.
const TURNS: [f64; 4] = [0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI, -std::f64::consts::FRAC_PI_2];

/// Starts tried at most (angle values beyond the first three start at 0).
const MAX_STARTS: usize = 64;

/// The values of joint `j` that move B least from where it is now: B's joint frame as it sits
/// in A's, met as nearly as the joint allows (its origin as near as it can be to where it is,
/// and its axes, each weighed as a point a part's size along it). Used for relations and for
/// typed joints that close a loop. `None` when the origins or finite values can't be found.
pub fn relation_values(doc: &Document, st: &ModelState, j: &Joint) -> Option<Vec<f64>> {
    let world = |occ: u64| if occ == 0 { IDENTITY } else { occurrence_world(doc, occ) };
    let fa = mat_mul(&world(j.a.occurrence), &j.offset_frame_a(resolve_origin(doc, st, &j.a).ok()?));
    let fb = mat_mul(&world(j.b.occurrence), &resolve_origin(doc, st, &j.b).ok()?);
    // B's joint frame in A's as it is.
    let now = mat_mul(&mat_inverse(&fa)?, &fb);
    // A turn weighs as far as it moves a point B's size away.
    let size = component_of(doc, j.b.part()).map(|c| component_bodies(doc, st, c).iter().map(|b| b.body.size()).fold(0.0, f64::max));
    let scale = size.unwrap_or(0.0).clamp(1.0, 1e5);
    let n = j.kind.dofs().len();
    let angles: Vec<usize> = (0..n).filter(|i| j.kind.is_angle(*i)).collect();
    let mut gaps = |q: &[f64]| -> Option<Vec<f64>> {
        let m: Mat = link_of(j, q);
        let mut out = Vec::with_capacity(12);
        for (k, (row, want)) in m.iter().zip(&now).enumerate() {
            let w = if k == 3 { 1.0 } else { scale };
            out.extend(row.iter().zip(want).take(3).map(|(a, b)| (a - b) * w));
        }
        out.iter().all(|v| v.is_finite()).then_some(out)
    };
    let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    let bounds = vec![(f64::NEG_INFINITY, f64::INFINITY); n];
    let mut best: Option<(f64, Vec<f64>)> = None;
    let tried = angles.len().min(3);
    for k in 0..TURNS.len().pow(tried as u32).min(MAX_STARTS) {
        let mut q = vec![0.0; n];
        let mut code = k;
        for i in angles.iter().take(tried) {
            if let (Some(slot), Some(t)) = (q.get_mut(*i), TURNS.get(code % TURNS.len())) {
                *slot = *t;
            }
            code /= TURNS.len();
        }
        // Tolerance 0: as near as it gets; `q` keeps the best values reached.
        least_squares(&mut gaps, &mut q, &bounds, 0.0);
        let Some(c) = gaps(&q).map(|r| cost(&r)) else { continue };
        if best.as_ref().is_none_or(|(b, _)| c < *b - 1e-12) {
            best = Some((c, q));
        }
    }
    // Angles within a half turn of 0.
    let mut q = best.map(|(_, mut q)| {
        for i in &angles {
            if let Some(v) = q.get_mut(*i) {
                *v = (*v + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
            }
        }
        q
    })?;
    // A mate's faces are coincident: the two faces meet, so the lateral offsets start at zero
    // and the faces' centres come together, as the Planar joint puts them. The other rules and
    // the relations keep the parts where they are (#14939). A mate of a design saved before
    // that change keeps the placement it was saved with.
    if mate_of_faces(j) {
        for (i, name) in j.kind.dofs().iter().enumerate() {
            if matches!(*name, "x" | "y")
                && let Some(v) = q.get_mut(i)
            {
                *v = 0.0;
            }
        }
    }
    Some(q)
}

/// A mate between two planar faces: the rule then makes them coincident, so the faces' centres
/// meet and the faces touch. A datum side has no face centre to meet, so the part keeps its
/// place on the datum. A mate of a design saved before the change keeps the placement it was
/// saved with, gap and all.
fn mate_of_faces(j: &Joint) -> bool {
    j.constraint == Some(Constraint::Mate)
        && !j.old_placement
        && matches!(j.a.snap, Snap::FaceCenter { .. })
        && matches!(j.b.snap, Snap::FaceCenter { .. })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::joints::{Constraint, JointKind, JointOrigin, Snap};
    use crate::{FeatureKind, Model};
    use solidcore_record::Vec3;

    /// A plate in one component, and a block 130 mm along x and 40 mm along y from it, 40 mm
    /// above its top face: two parts whose top and bottom faces do not touch.
    fn plate_and_block() -> (Document, std::sync::Arc<ModelState>, u64, u64) {
        let mut doc = Document::new("mate");
        let plate = doc.add_component(Some("Plate"), 0).unwrap();
        let block = doc.add_component(Some("Block"), 0).unwrap();
        for (component, name, corner) in [(plate, "Plate", ["0", "0", "0"]), (block, "Block", ["130", "40", "40"])] {
            let kind: FeatureKind = serde_json::from_value(serde_json::json!({
                "type": "box", "corner": corner, "length": "100", "width": "100", "height": "10", "operation": "new_body"
            }))
            .unwrap();
            let id = doc.add_feature(kind, Some(name)).unwrap();
            doc.feature_mut(id).unwrap().component = component;
        }
        let a = doc.add_occurrence(plate, 0, IDENTITY).unwrap();
        let b = doc.add_occurrence(block, 0, IDENTITY).unwrap();
        let mut model = Model::new();
        model.evaluate(&doc);
        let st = model.state();
        (doc, st, a, b)
    }

    fn joint(kind: JointKind) -> Joint {
        let at = |occurrence| JointOrigin::new(occurrence, Snap::Point { point: Vec3::ZERO, z: None, x: None });
        Joint {
            id: 1,
            name: "R".into(),
            kind,
            a: at(0),
            b: at(1),
            values: Vec::new(),
            limits: Vec::new(),
            flip: false,
            offset: 0.0,
            angle: 0.6,
            suppressed: false,
            exprs: Default::default(),
            offset_xy: [0.0; 2],
            captured: None,
            constraint: None,
            old_placement: false,
        }
    }

    fn z_of(m: &Mat) -> Vec3 {
        Vec3::new(m[2][0], m[2][1], m[2][2])
    }

    /// Where the joints put the occurrence `id`, as the design solves them.
    fn place_of(doc: &Document, st: &ModelState, id: u64) -> Mat {
        crate::joints::solve(doc, st).world.get(&id).copied().unwrap_or(IDENTITY)
    }

    /// Where a world transform puts the point `p`.
    fn at_of(m: &Mat, p: Vec3) -> Vec3 {
        Vec3::new(m[3][0] + p.x, m[3][1] + p.y, m[3][2] + p.z)
    }

    /// A mate's two faces are coincident: the lateral offsets start at zero, so the faces'
    /// centres meet and the faces touch. The part is not left beside the other one, level with
    /// it and a gap away from it (#14939).
    #[test]
    fn a_mate_starts_with_its_faces_together() {
        let (doc, st, a, b) = plate_and_block();
        let face = |pick| Snap::FaceCenter { pick, face: None, x: None };
        let mut j = joint(JointKind::Planar);
        j.angle = 0.0;
        j.constraint = Some(Constraint::Mate);
        j.a = JointOrigin::new(a, face(Vec3::new(50.0, 50.0, 10.0)));
        j.b = JointOrigin::new(b, face(Vec3::new(180.0, 90.0, 40.0)));
        let q = relation_values(&doc, &st, &j).expect("the joint's origins resolve");
        // The faces' centres meet: the lateral offsets are zero, not the 130 mm and 40 mm the
        // parts stand apart.
        assert!(q[0].abs() < 1e-9 && q[1].abs() < 1e-9, "the mate's lateral offsets: {q:?}");
    }

    /// A design saved before the mate brought the two faces together opens with its parts where
    /// they were: the mate of that design keeps the placement it was saved with, gap and all
    /// (#14939).
    #[test]
    fn a_saved_design_with_a_mate_opens_with_its_parts_where_they_were() {
        let (mut doc, st, a, b) = plate_and_block();
        let face = |pick| Snap::FaceCenter { pick, face: None, x: None };
        let mut j = joint(JointKind::Planar);
        j.angle = 0.0;
        j.constraint = Some(Constraint::Mate);
        j.a = JointOrigin::new(a, face(Vec3::new(50.0, 50.0, 10.0)));
        j.b = JointOrigin::new(b, face(Vec3::new(180.0, 90.0, 40.0)));
        // The values the mate of an older design was saved with: the block's own lateral place,
        // so the two faces are level and a gap apart.
        j.values = vec![40.0, -130.0, std::f64::consts::PI];
        doc.assembly.joints.push(j);
        // Where the block is as the design is saved: the mate's own placement.
        let saved = place_of(&doc, &st, b);
        // The design as an older build saved it: the format number before this change and no
        // list of the changes it has, so the step that marks its mates runs as it opens.
        let mut old: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
        old["format"] = serde_json::json!(crate::format::format_string(crate::format::FORMAT - 1));
        old.as_object_mut().unwrap().remove("format_pending");
        let opened = crate::Document::from_json(&old.to_string()).unwrap();
        // The mate is marked: it keeps the placement it was saved with.
        assert!(opened.assembly.joints[0].old_placement, "the mate of the older design is marked");
        let mut model = Model::new();
        model.evaluate(&opened);
        let st = model.state();
        // The block is where it was: its bottom face's centre is where the mate left it.
        let now = place_of(&opened, &st, b);
        let moved = at_of(&now, Vec3::new(180.0, 90.0, 40.0)) - at_of(&saved, Vec3::new(180.0, 90.0, 40.0));
        assert!(moved.x.abs() < 1e-9 && moved.y.abs() < 1e-9 && moved.z.abs() < 1e-9, "the block is where it was: {moved:?}");
        // And the mate keeps that placement: its lateral offsets are the ones it was saved with,
        // not the zeros a new mate starts with.
        let q = relation_values(&opened, &st, &opened.assembly.joints[0]).expect("the joint's origins resolve");
        assert!((q[0] - 40.0).abs() < 1e-6 && (q[1] + 130.0).abs() < 1e-6, "the mate keeps its lateral offsets: {q:?}");
        // A design saved now keeps its mate unmarked: the mate of a new design brings the two
        // faces together.
        let now = crate::Document::from_json(&doc.to_json()).unwrap();
        assert!(!now.assembly.joints[0].old_placement, "the mate of a design saved now is not marked");
    }

    /// Whatever its values, each relation keeps what it ties.
    #[test]
    fn relations_keep_what_they_tie() {
        let q = [13.0, -7.0, 21.0, 0.7, -1.9];
        let at = |kind| {
            let j = joint(kind);
            let n = kind.dofs().len();
            link_of(&j, q.get(..n).unwrap())
        };
        // Parallel: z against A's.
        assert!((z_of(&at(JointKind::Parallel)).z + 1.0).abs() < 1e-12);
        // Perpendicular: z square to A's.
        assert!(z_of(&at(JointKind::Perpendicular)).z.abs() < 1e-12);
        // Angle: 0.6 from mated.
        assert!((z_of(&at(JointKind::Angle)).z + 0.6f64.cos()).abs() < 1e-12);
        // Point on plane: the origin on A's plane.
        assert!(at(JointKind::PointOnPlane)[3][2].abs() < 1e-12);
        // Point on line: on A's z axis.
        let m = at(JointKind::PointOnLine);
        assert!(m[3][0].abs() < 1e-12 && m[3][1].abs() < 1e-12);
        // Line on plane: the origin on the plane and z along it.
        let m = at(JointKind::LineOnPlane);
        assert!(m[3][2].abs() < 1e-12 && z_of(&m).z.abs() < 1e-12);
    }
}
