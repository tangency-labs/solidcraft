//! Untrim (Surface › Untrim): a face restored towards the surface it lies on. Picked inner
//! loops (holes), or all of them, are taken away; optionally the outer boundary goes back to
//! the surface's natural parameter bounds: a free-form surface's own parameter box, and for an
//! analytic surface (a plane, cylinder, cone, sphere or torus, which has no natural bounds of
//! its own, or bounds that go all the way round) the box of the face's own span. The restored
//! boundary runs along the surface's parameter lines.
//!
//! SMLib does it (`smlib_untrim_face`): holes are deleted from the face, the outer boundary is
//! the face SMLib makes from the surface over that box, the holes kept rebuilt in it from their
//! own edges. The body goes to SMLib tagged as operand 0, so the result's history comes from
//! SMLib's tags: the untrimmed face is the face it was, trimmed differently.

use solidcraft_geom::Vec3;
use solidcraft_smlib::trim::Holes;

use crate::body::Body;
use crate::history::{Change, FaceOrigin};
use crate::native::{body_from_smlib_ordered, smlib_body_ordered};
use crate::{KernelError, Result};

/// Most hole picks.
const MAX_PICKS: usize = 10_000;

/// The inner loops Untrim takes away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UntrimHoles<'a> {
    /// Every inner loop of the face.
    All,
    /// The inner loops nearest these points (on or near their edges); none keeps every hole.
    At(&'a [Vec3]),
}

/// Face `face` of `body` (in its face order) untrimmed: the inner loops `holes` names taken
/// away and, with `outer`, the outer boundary restored to the surface's natural bounds (an
/// analytic surface's: the face's own span). The result is a surface body: the untrimmed face on
/// its own, or, with `in_place` and a surface `body`, the body with that face untrimmed (a
/// solid's face is always copied out). Its history is over `[body]`: the untrimmed face is the
/// face it came from, trimmed differently, so it keeps that face's name.
pub fn untrim(body: &Body, face: usize, holes: UntrimHoles, outer: bool, in_place: bool) -> Result<Body> {
    body.require_brep("untrim")?;
    if let UntrimHoles::At(points) = holes {
        if points.len() > MAX_PICKS {
            return Err(KernelError::Invalid("too many holes picked".into()));
        }
        if points.iter().any(|p| !p.is_finite()) {
            return Err(KernelError::Invalid("a hole point is not finite".into()));
        }
    }
    let record = crate::native::brep_record(body)?;
    let f = record.faces.get(face).ok_or_else(|| KernelError::Invalid(format!("the body has no face {face}")))?;
    let inner = f.loops.len().saturating_sub(1);
    let removes = match holes {
        UntrimHoles::All => inner > 0,
        UntrimHoles::At(points) => !points.is_empty(),
    };
    if !outer && !removes {
        return Err(KernelError::Invalid(if inner == 0 {
            "the face has no holes to remove (restore its outer boundary instead)".into()
        } else {
            "nothing to untrim: pick holes to remove or restore the outer boundary".into()
        }));
    }
    if matches!(holes, UntrimHoles::At(p) if !p.is_empty()) && inner == 0 {
        return Err(KernelError::Invalid("the face has no holes".into()));
    }
    let size = body.size().max(1e-6);
    let at = face_point(body, face)?;
    let picks: Vec<[f64; 3]> = match holes {
        UntrimHoles::All => Vec::new(),
        UntrimHoles::At(points) => points.iter().map(|p| [p.x, p.y, p.z]).collect(),
    };
    let which = match holes {
        UntrimHoles::All => Holes::All,
        UntrimHoles::At(_) => Holes::At(&picks),
    };
    let in_place = in_place && body.is_surface();
    let (sb, ob) = smlib_body_ordered(body)?;
    let tagged = sb.tagged(0).map_err(|e| failed("tag", e))?;
    // A hole is picked within a twentieth of the body's size of its edges, the face by a point
    // inside it.
    let reach = size * 0.05 + 1e-6;
    let r = tagged.untrim_face([at.x, at.y, at.z], reach, which, outer, in_place).map_err(|e| match e {
        solidcraft_smlib::Error::NoMatch { .. } => KernelError::Invalid("no hole of the face at a picked point".into()),
        solidcraft_smlib::Error::Unsupported => {
            KernelError::Failed("not supported yet: untrimming this face in place (another face uses its hole's edges)".into())
        }
        e => failed("untrim", e),
    })?;
    let (out, order) = body_from_smlib_ordered(&r)?;
    let history = match (ob, order) {
        (Some(ob), Some(or)) => r.history().map_err(|e| failed("history", e))?.history(&[ob], &or).ok().map(|mut h| {
            // The untrimmed face is the face it was, bounded differently.
            for p in &mut h.faces {
                if p.origins.iter().any(|o| *o == FaceOrigin::Face { operand: 0, face }) {
                    p.change = Change::Modified;
                }
            }
            h
        }),
        _ => None,
    };
    Ok(out.with_checked_history(history, &crate::record::counts(&[body])))
}

fn failed(what: &str, e: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("SMLib untrim: {what}: {e}"))
}

/// A point inside face `face` of `body`: the centre of its largest mesh triangle.
pub(crate) fn face_point(body: &Body, face: usize) -> Result<Vec3> {
    let m = body.mesh(&crate::MeshTolerance::chord((body.size() * 1e-3).max(1e-3)))?;
    let area = |[a, b, c]: &[Vec3; 3]| (*b - *a).cross(*c - *a).len();
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, f)| **f as usize == face)
        .filter_map(|(t, _)| m.tri(t))
        .max_by(|x, y| area(x).total_cmp(&area(y)))
        .map(|[a, b, c]| (a + b + c) / 3.0)
        .ok_or_else(|| KernelError::Failed(format!("untrim: face {face} has no triangles")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{extrude, measure};
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    use std::f64::consts::PI;

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }

    fn inner_loops(b: &Body) -> usize {
        crate::native::brep_record(b).unwrap().faces.iter().map(|f| f.loops.len().saturating_sub(1)).sum()
    }

    /// A 40 × 20 × 2 plate with three holes of radius 2 at x = 10, 20, 30 on y = 10.
    fn plate() -> Body {
        let outer = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(40.0, 20.0), Vec2::new(0.0, 20.0)]);
        let holes = [10.0, 20.0, 30.0].iter().map(|x| Loop2::circle(Vec2::new(*x, 10.0), 2.0).reversed()).collect();
        extrude(&Plane::XY, &[Region2 { outer, holes }], 0.0, 2.0).unwrap().pop().unwrap()
    }

    fn face_at(b: &Body, p: Vec3) -> usize {
        crate::surfaces::nearest_face(b, p).unwrap()
    }

    /// The plate's top with every hole taken away is the whole 40 × 20 rectangle, one face with
    /// no inner loops; one hole taken away leaves two (area 800 − 2 · 4π); the face's own loops
    /// are kept by identity. Nothing to take away is refused.
    #[test]
    fn plate_with_three_holes_untrimmed_to_its_rectangle() {
        let b = plate();
        let top = face_at(&b, Vec3::new(5.0, 5.0, 2.0));
        let all = untrim(&b, top, UntrimHoles::All, false, false).unwrap();
        assert!(all.is_surface() && all.face_count() == 1 && inner_loops(&all) == 0);
        assert!(rel(measure(&all).unwrap().area, 800.0) < 1e-9, "{}", measure(&all).unwrap().area);
        let one = untrim(&b, top, UntrimHoles::At(&[Vec3::new(22.0, 10.0, 2.0)]), false, false).unwrap();
        assert_eq!(inner_loops(&one), 2);
        let want = 800.0 - 2.0 * 4.0 * PI;
        assert!(rel(measure(&one).unwrap().area, want) < 1e-3, "{} vs {want}", measure(&one).unwrap().area); // (measured on meshes)
        // The plate is untouched (three holes through its top and bottom); the history says the
        // face is the top, trimmed differently.
        assert_eq!(inner_loops(&b), 6);
        let h = all.history().unwrap();
        assert_eq!(h.faces.first().map(|f| f.change), Some(crate::history::Change::Modified));
        // Nothing to do: a face without holes, a point on no hole, an empty pick.
        let side = crate::native::brep_record(&b).unwrap().faces.iter().position(|f| f.loops.len() == 1).unwrap();
        assert!(untrim(&b, side, UntrimHoles::All, false, false).is_err());
        assert!(untrim(&b, top, UntrimHoles::At(&[Vec3::new(5.0, 5.0, 2.0)]), false, false).is_err());
        assert!(untrim(&b, top, UntrimHoles::At(&[]), false, false).is_err());
        assert!(untrim(&b, 99, UntrimHoles::All, false, false).is_err());
    }

    /// A half cylinder sheet (radius 10, angles 0…π, height 20) trimmed by the slanted plane
    /// z = 15 − y/2: its top edge runs from 15 down to 10 and back, its area ∫ (15 − 5 sin θ) r dθ
    /// = 150π − 100. Restored, it fills its own span again, angles 0…π and heights 0…15: 150π,
    /// on the same cylinder.
    #[test]
    fn a_trimmed_cylinder_face_is_restored_to_its_full_span() {
        let arc = vec![Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 10.0, start: 0.0, sweep: PI }];
        let sheet = crate::extrude_sheet(&Plane::XY, &[arc], 0.0, 20.0).unwrap();
        let tool = crate::SplitTool::Plane(Plane::from_normal(Vec3::new(0.0, 0.0, 15.0), Vec3::new(0.0, 0.5, 1.0)).unwrap());
        let cut = crate::trim(&sheet, &tool, Vec3::new(0.0, 10.0, 1.0)).unwrap();
        assert_eq!(cut.face_count(), 1);
        let trimmed = measure(&cut).unwrap().area;
        assert!(rel(trimmed, 150.0 * PI - 100.0) < 1e-3, "{trimmed}"); // (measured on meshes)
        // Nothing to take away and the outer boundary kept: nothing to do.
        assert!(untrim(&cut, 0, UntrimHoles::All, false, false).is_err());
        let restored = untrim(&cut, 0, UntrimHoles::All, true, false).unwrap();
        assert!(restored.face_count() == 1 && inner_loops(&restored) == 0);
        let a = measure(&restored).unwrap().area;
        assert!(rel(a, 150.0 * PI) < 1e-3, "{a} vs {}", 150.0 * PI); // (measured on meshes)
        let bb = measure(&restored).unwrap().bbox;
        assert!(bb.min.z.abs() < 1e-9 && (bb.max.z - 15.0).abs() < 1e-6, "{bb:?}");
        assert!(matches!(
            crate::face_surface(&restored, 0),
            Some(crate::FaceSurface::Cylinder { radius, .. }) if (radius - 10.0).abs() < 1e-6
        ));
    }

    /// A revolved face: the cone frustum swept by the line from radius 5 at z = 0 to radius 10
    /// at z = −10 (slant √125), its first face (angles 0…½), trimmed by the plane z = −8 + y/2.
    /// The cut reaches lowest (profile parameter 0.8) at angle 0, so the face's own span is
    /// angles 0…½ by parameters 0…0.8, restored with arcs about the axis and straight sides:
    /// ½ · √125 · ∫₀^0.8 (5 + 5t) dt = ½ · √125 · 5.6.
    #[test]
    fn a_trimmed_cone_face_is_restored_to_its_span() {
        let line = vec![Seg2::Line { a: Vec2::new(5.0, 0.0), b: Vec2::new(10.0, 10.0) }];
        let sheet = crate::revolve_sheet(&Plane::XZ, &[line], Vec2::ZERO, Vec2::Y, 0.0, 1.0).unwrap();
        let tool = crate::SplitTool::Plane(Plane::from_normal(Vec3::new(0.0, 0.0, -8.0), Vec3::new(0.0, -0.5, 1.0)).unwrap());
        let (c, s) = (0.25f64.cos(), 0.25f64.sin());
        let cut = crate::trim(&sheet, &tool, Vec3::new(5.0 * c, 5.0 * s, 0.0)).unwrap();
        let face = face_at(&cut, Vec3::new(5.5 * c, 5.5 * s, -1.0));
        let restored = untrim(&cut, face, UntrimHoles::All, true, false).unwrap();
        let want = 0.5 * 125f64.sqrt() * 5.6;
        let a = measure(&restored).unwrap().area;
        assert!(rel(a, want) < 1e-3, "{a} vs {want}"); // (measured on meshes)
        let bb = measure(&restored).unwrap().bbox;
        assert!((bb.min.z + 8.0).abs() < 1e-6 && bb.max.z.abs() < 1e-6, "{bb:?}");
        assert!(matches!(crate::face_surface(&restored, 0), Some(crate::FaceSurface::Cone { .. })));
    }

    /// A torus face with a piece of its outer equator cut off by the plane x = 12: the cut
    /// touches one side of the face only, so its span is the face's whole span and the restored
    /// face is the face as it was before the cut.
    #[test]
    fn a_torus_face_cut_at_one_side_is_restored_whole() {
        let torus = crate::torus(Vec3::ZERO, 10.0, 3.0).unwrap();
        let face = crate::copy_faces(&torus, &[Vec3::new(13.0, 0.0, 0.0)]).unwrap();
        let before = measure(&face).unwrap().area;
        let tool = crate::SplitTool::Plane(Plane::from_normal(Vec3::new(12.0, 0.0, 0.0), Vec3::X).unwrap());
        let cut = crate::trim(&face, &tool, Vec3::new(0.0, 13.0, 0.0)).unwrap();
        let trimmed = measure(&cut).unwrap().area;
        assert!(trimmed < before * 0.99, "{trimmed} vs {before}");
        let restored = untrim(&cut, 0, UntrimHoles::All, true, false).unwrap();
        let a = measure(&restored).unwrap().area;
        assert!(rel(a, before) < 1e-3, "{a} vs {before}"); // (measured on meshes)
        assert!(matches!(
            crate::face_surface(&restored, 0),
            Some(crate::FaceSurface::Torus { major, minor, .. }) if (major - 10.0).abs() < 1e-6 && (minor - 3.0).abs() < 1e-6
        ));
    }

    /// A free-form face goes back to its surface's own parameter box: the saddle z = x y / 10
    /// over 0…10 × 0…10 (ruled between two straight lines, sketched on planes y = 0 and 10 whose sketch y is −z: a
    /// bilinear patch), cut at x = 5 and
    /// restored to the whole patch: x and y 0…10, z 0…10.
    #[test]
    fn a_free_form_face_is_restored_to_its_surfaces_bounds() {
        let front = (Plane::XZ, vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }]);
        let back = (Plane { origin: Vec3::new(0.0, 10.0, 0.0), ..Plane::XZ }, vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, -10.0) }]);
        let saddle = crate::loft_sheet(&[front, back]).unwrap();
        let tool = crate::SplitTool::Plane(Plane::from_normal(Vec3::new(5.0, 0.0, 0.0), Vec3::X).unwrap());
        let half = crate::trim(&saddle, &tool, Vec3::new(1.0, 5.0, 0.5)).unwrap();
        assert!(crate::face_surface(&half, 0).is_none(), "not analytic");
        assert!((measure(&half).unwrap().bbox.max.x - 5.0).abs() < 1e-9);
        let whole = untrim(&half, 0, UntrimHoles::All, true, false).unwrap();
        let bb = measure(&whole).unwrap().bbox;
        assert!(bb.min.x.abs() < 1e-9 && bb.min.y.abs() < 1e-9 && bb.min.z.abs() < 1e-9, "{bb:?}");
        assert!((bb.max.x - 10.0).abs() < 1e-9 && (bb.max.y - 10.0).abs() < 1e-9 && (bb.max.z - 10.0).abs() < 1e-9, "{bb:?}");
        let record = crate::native::brep_record(&whole).unwrap();
        assert!(matches!(record.faces.first().and_then(|f| f.loops.first()), Some(solidcraft_geom::brep::Loop::Edges { edges }) if edges.len() == 4));
    }

    /// The plate exported to STEP and read back: its imported top face untrims to the whole
    /// rectangle as the plate's own did.
    #[test]
    fn an_imported_step_face_is_untrimmed() {
        let text = crate::step_export(&[&plate()], "untrim test").unwrap();
        let imported = crate::step_import(&text).unwrap();
        let b = &imported.bodies.first().unwrap().body;
        let top = face_at(b, Vec3::new(5.0, 5.0, 2.0));
        assert_eq!(crate::native::brep_record(b).unwrap().faces.get(top).unwrap().loops.len(), 4);
        let all = untrim(b, top, UntrimHoles::All, false, false).unwrap();
        assert!(all.face_count() == 1 && inner_loops(&all) == 0);
        assert!(rel(measure(&all).unwrap().area, 800.0) < 1e-9);
        let restored = untrim(b, top, UntrimHoles::All, true, false).unwrap();
        assert!(rel(measure(&restored).unwrap().area, 800.0) < 1e-9);
    }

    /// In place: a surface body of the plate's top and bottom keeps its bottom (and its three
    /// holes), the top replaced by the whole rectangle: 800 + 800 − 3 · 4π. A solid's face is
    /// copied out whatever `in_place` says.
    #[test]
    fn in_place_untrims_a_surface_and_copies_a_solids_face() {
        let b = plate();
        let top = face_at(&b, Vec3::new(5.0, 5.0, 2.0));
        let copied = untrim(&b, top, UntrimHoles::All, false, true).unwrap();
        assert_eq!(copied.face_count(), 1);
        let sheet = crate::copy_faces(&b, &[Vec3::new(5.0, 5.0, 2.0), Vec3::new(5.0, 5.0, 0.0)]).unwrap();
        assert!(sheet.is_surface() && sheet.face_count() == 2 && inner_loops(&sheet) == 6);
        let top = face_at(&sheet, Vec3::new(5.0, 5.0, 2.0));
        let whole = untrim(&sheet, top, UntrimHoles::All, false, true).unwrap();
        assert!(whole.is_surface());
        assert_eq!((whole.face_count(), inner_loops(&whole)), (2, 3));
        let want = 1600.0 - 3.0 * 4.0 * PI;
        assert!(rel(measure(&whole).unwrap().area, want) < 1e-3, "{} vs {want}", measure(&whole).unwrap().area); // (measured on meshes)
    }

    /// Points that are not finite and too many picks are refused, never panic.
    #[test]
    fn hostile_inputs_are_refused() {
        let b = plate();
        let top = face_at(&b, Vec3::new(5.0, 5.0, 2.0));
        assert!(untrim(&b, top, UntrimHoles::At(&[Vec3::new(f64::NAN, 0.0, 0.0)]), false, false).is_err());
        let many = vec![Vec3::ZERO; MAX_PICKS + 1];
        assert!(untrim(&b, top, UntrimHoles::At(&many), false, false).is_err());
    }
}
