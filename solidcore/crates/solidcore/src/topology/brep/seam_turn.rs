//! (not upstream) A face's seam turned about its surface's axis on an existing body: the face
//! given its own surface placed again about that axis, its seam edges taken out and the seam
//! of the placed surface put in.
//!
//! Upstream: none (#19505). A seam is the surface's parameterisation, not a feature of the body
//! (#2864): the same face on the same point set, its seam on another ruling. The surface comes
//! from [`crate::surface::step_seam::seam_into_widest_gap`], which turns a closed surface about
//! its own axis and keeps its kind.
//!
//! | Rust          | What                                                              |
//! |---------------|-------------------------------------------------------------------|
//! | [`turn_seam`] | the face's seam edges out, the turned surface in, its seam made   |
//!
//! Deviations: none (not a port). No recursion.

use crate::error::{EntityRef, Result, SolidError};
use crate::kernel::Kernel;
use crate::surface::surface::Surface;
use crate::topology::brep::{p2, p3, p4, p5};
use crate::topology::face::{p1 as face1, p3 as face3, p4 as face4};
use crate::topology::{edge, vertex, Brep, EdgeId, FaceId, VertexId};

const OP: &str = "topology::brep::seam_turn::turn_seam";

/// Gives face `f` the surface `turned`, which is `f`'s own surface placed again about its axis
/// (the same point set, its seam on another ruling), in one edit ([`p3::edit_brep`], so the
/// body's attributes follow the splits and joins):
///
/// 1. the face's seam edges (both sides on `f`, [`edge::is_seam`]) are deleted, the face keeping
///    its surface ([`p4::delete_edge`]);
/// 2. the surface is replaced and the face's UV trim curves dropped ([`p2::replace_surface`]);
/// 3. the seam of `turned` is completed ([`face1::complete_merged_face_seam`]: the edges it
///    crosses are split at new vertices);
/// 4. each vertex the old seam ended at that now only joins two edges between the same faces,
///    G1 there (within the kernel's angular tolerance), is taken out and the two edges joined
///    ([`p5::delete_topological_vertex_joining`]): they are pieces of the two faces' one
///    intersection curve, which only the seam split. `IsTopologicalVertex` is not asked: it
///    wants the two curves' parameters equal at the vertex, and two arcs a seam split are two
///    curve objects with their own parameters;
/// 5. the face's UV trim curves are made again on the turned parameterisation
///    ([`face3::create_uv_trim_curves_notified`]).
///
/// The face keeps its id and its attributes. The caller answers for `turned` being `f`'s
/// surface turned. A face with no seam edge is given the surface and its seam all the same.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a face without a surface; those of the steps. On an error
/// the body is left as the failing step left it: the caller works on a copy.
pub fn turn_seam(k: &mut Kernel, b: &mut Brep, f: FaceId, turned: Surface) -> Result<()> {
    if b.face(f)?.surface.is_none() {
        return Err(SolidError::invalid_input(OP, "a face without a surface"));
    }
    let mut seams: Vec<EdgeId> = Vec::new();
    for e in face4::edges(b, f, None, false, None)? {
        if seams.contains(&e) {
            continue;
        }
        if edge::faces(b, e, None)?.as_slice() == [f] && edge::is_seam(k, b, e)? {
            seams.push(e);
        }
    }
    let mut ends: Vec<VertexId> = Vec::new();
    for &e in &seams {
        let (a, z) = edge::start_end_vertices(b, e)?;
        for v in [a, z].into_iter().flatten() {
            if !ends.contains(&v) {
                ends.push(v);
            }
        }
    }
    p3::edit_brep(k, b, |k, b, scope| {
        for &e in &seams {
            let mut env = p4::DeleteEnv::from_notify(scope.env);
            p4::delete_edge(k, b, &mut env, e, Some(f))?;
        }
        let face_ref = (scope.env.key_of)(EntityRef::Face(f));
        p2::replace_surface(
            k,
            b,
            scope.env,
            f,
            turned,
            face_ref,
            p2::OldSurface::Delete,
            false,
        )?;
        face1::complete_merged_face_seam(k, b, Some(&mut *scope.env), f)?;
        for &v in &ends {
            if !b.vertex_list().contains(&v) {
                continue;
            }
            let at = vertex::edges(b, v, None)?;
            let [a, z] = at.as_slice() else {
                continue;
            };
            if a == z {
                continue;
            }
            let (mut fa, mut fz) = (edge::faces(b, *a, None)?, edge::faces(b, *z, None)?);
            fa.sort();
            fz.sort();
            if fa != fz {
                continue;
            }
            let (g1, _) = crate::blend::fillet_corner::p1::test_g1_continuity(
                k,
                b,
                Some(v),
                Some(*a),
                Some(*z),
                k.tolerances.ang_tol_deg,
            )?;
            if !g1 {
                continue;
            }
            p5::delete_topological_vertex_joining(
                k,
                b,
                true,
                Some(v),
                None,
                Some(&mut *scope.env),
            )?;
        }
        face1::remove_uv_trim_curves_with_notifications(b, f, scope.env)?;
        face3::create_uv_trim_curves_notified(k, b, f, true, None, Some(&mut *scope.env))?;
        Ok(())
    })
}
