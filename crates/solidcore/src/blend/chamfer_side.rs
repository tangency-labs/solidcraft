//! Which face of an edge a chamfer's first size is along: the outward normals of the edge's
//! two faces at its middle, as how far each leans along a direction, for an application that
//! chooses the face (a chamfer whose first distance is along the face facing up most).
//!
//! Not upstream (solidcore's). The normals are the faces' surfaces' at the binormal points of
//! the edge's blend edge use and its radial mate at the middle of the edge's interval, signed
//! by the face's outer face use (its side facing the body's outside). Evaluated, not
//! certified: the leans are for a preference between the two faces, not a decision at a
//! tolerance. No recursion.

use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::evaluate_surface_normal;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::face::p2::outer_faceuse;
use crate::topology::{edge, EdgeId, EdgeuseId, FaceId};
use crate::{Brep, Kernel, Result, SolidError, Vec3};

const OP: &str = "blend::chamfer_side::faces_along";

fn invalid(why: &'static str) -> SolidError {
    SolidError::invalid_input(OP, why)
}

/// The face edge use `u` bounds and its outward unit normal at parameter `t` of its edge.
fn outward(k: &Kernel, b: &mut Brep, u: EdgeuseId, t: f64) -> Result<(FaceId, Vec3)> {
    let face = match eu1::faceuse(b, u)? {
        Some(fu) => b.faceuse(fu)?.face,
        None => None,
    }
    .ok_or_else(|| invalid("an edge use of the edge bounds no face"))?;
    let fu =
        outer_faceuse(b, face)?.ok_or_else(|| invalid("a face of the edge has no face use"))?;
    let at = evaluate_binormal(k, b, u, t, false, &BinormalOptions::default())?;
    let normal = evaluate_surface_normal(k, b.face(face)?.surface.as_ref(), at.uv)?;
    let len = normal.length();
    if !(len > 0.0 && len.is_finite()) {
        return Err(invalid("a face's normal vanishes at the edge's middle"));
    }
    let sign = if b.faceuse(fu)?.orientation == OrientType::Same {
        1.0
    } else {
        -1.0
    };
    Ok((face, normal * (sign / len)))
}

/// The two faces of `e`, the face of its blend edge use first (a chamfer's support 1) and its
/// radial mate's second, each with how far its outward unit normal at the middle of the edge
/// leans along `dir` (the normal's dot with `dir`).
///
/// # Errors
/// [`SolidError::InvalidInput`] for a direction that is not finite, an edge without a blend
/// edge use or its mate (not between two faces), or a face whose normal vanishes there; the
/// evaluators' errors.
pub fn faces_along(k: &Kernel, b: &mut Brep, e: EdgeId, dir: Vec3) -> Result<[(FaceId, f64); 2]> {
    if !(dir.x.is_finite() && dir.y.is_finite() && dir.z.is_finite()) {
        return Err(invalid("the direction is finite"));
    }
    let u0 =
        edge::blend_edgeuse(k, b, e)?.ok_or_else(|| invalid("the edge has no blend edge use"))?;
    let u1 = eu1::radial(b, u0)?.ok_or_else(|| invalid("the edge lies on one face only"))?;
    let t = b.edge(e)?.interval.evaluate(0.5);
    let (f0, n0) = outward(k, b, u0, t)?;
    let (f1, n1) = outward(k, b, u1, t)?;
    Ok([(f0, n0.dot(dir)), (f1, n1.dot(dir))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives::create_box;
    use crate::query::api_queries::edge_closest_point;

    /// A unit box's top front edge: its top face leans 1 along +z, its front face 0.
    #[test]
    fn the_top_face_of_a_box_edge_leans_up() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Vec3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let p = Vec3::new(0.5, 0.0, 1.0);
        let e = b
            .edge_list()
            .iter()
            .copied()
            .find(|&e| edge_closest_point(&k, &b, e, p).unwrap().closest.distance < 1e-12)
            .unwrap();
        let leans = faces_along(&k, &mut b, e, Vec3::new(0.0, 0.0, 1.0)).unwrap();
        let mut ups: Vec<f64> = leans.iter().map(|(_, l)| *l).collect();
        ups.sort_by(f64::total_cmp);
        assert!(
            ups[0].abs() < 1e-12 && (ups[1] - 1.0).abs() < 1e-12,
            "{ups:?}"
        );
        assert_ne!(leans[0].0, leans[1].0);
        assert!(faces_along(&k, &mut b, e, Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
    }
}
