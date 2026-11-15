//! Whether a face of a body is a prism's planar cap: the kernel's
//! [`solidcore::geom::plane_height::is_square_plane_at`] on records' numbers. Ops converts.
//!
//! And whether a prism cut at a face of a target is kept ([`prism_part`]): the cap tests on
//! the part's planar faces, and the section's edges on the target's face, each curve lying on
//! the face's surface and inside its trim by the kernel's
//! [`solidcore::topology::face::curve_on::curve_lies_on_face`] (#16713).

use solidcore::geom::plane_height::is_square_plane_at;
use solidcore::geom::vector3d::Vec3;
use solidcore::topology::face::curve_on::curve_lies_on_face;

use crate::body::Body;
use crate::order::{self, Order};
use crate::session::with_body;
use crate::{Error, Result};

fn vec(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// Whether the plane through `face_origin` with unit normal `face_normal` is square to the
/// unit direction `normal` and lies at `height` along it from `origin`, within `tol` (the
/// kernel's [`is_square_plane_at`]). The application sequences it over a body's planar faces.
#[must_use]
pub fn is_square_plane_at_height(
    face_origin: [f64; 3],
    face_normal: [f64; 3],
    origin: [f64; 3],
    normal: [f64; 3],
    height: f64,
    tol: f64,
) -> bool {
    is_square_plane_at(
        vec(face_origin),
        vec(face_normal),
        vec(origin),
        vec(normal),
        height,
        tol,
    )
}

/// A planar face of a part: a point on it and its unit normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanarFace {
    /// A point on the face's plane.
    pub origin: [f64; 3],
    /// The plane's unit normal.
    pub normal: [f64; 3],
}

/// The prism a part was cut from and where it was cut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrismCut {
    /// A point on the prism's start plane.
    pub origin: [f64; 3],
    /// The unit direction the prism runs along.
    pub normal: [f64; 3],
    /// The height along `normal` of the start cap.
    pub start: f64,
    /// The height of the far end, where the unbounded prism was cut off.
    pub far: f64,
    /// How near a cap's plane must be to a height: the body's own, never over a quarter of the
    /// distance between the two heights.
    pub cap_tolerance: f64,
    /// How near a section curve must lie to the target's surface.
    pub section_tolerance: f64,
}

/// What becomes of one part of a prism cut at a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrismPart {
    /// The part does not start at the prism's start: not the piece asked for.
    Dropped,
    /// The part reaches past the face: its far cap is still there, or its section is not a
    /// piece of the face.
    ReachesPast,
    /// The part starts at the start and ends at the face.
    Kept,
}

/// Whether every one of `edges` (record edges of `part`, through `part_order`) lies on face
/// `face` (a record index) of `target`: each edge's own kernel curve and interval, as the
/// B-rep validator reads an edge (a traced surface-intersection curve too), on the face's
/// surface (checked at sampled points) and inside its trim (each part of the interval
/// classified against the face's boundary) ([`curve_lies_on_face`]), within
/// `tolerance`. No edges vacuously lie on any face. `target` may gain UV trim curves.
///
/// # Errors
/// [`Error::Argument`] for a face or edge past its body's, an edge without a curve, or a
/// tolerance not finite and positive; the kernel's errors.
pub fn edges_lie_on_face(
    target: &Body,
    order: &Order,
    face: usize,
    part: &Body,
    part_order: &Order,
    edges: &[usize],
    tolerance: f64,
) -> Result<bool> {
    let f = order::face(order, face)?;
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(Error::Argument);
    }
    let ids = edges
        .iter()
        .map(|e| order::edge(part_order, *e))
        .collect::<Result<Vec<_>>>()?;
    let curves = with_body("section_curves", part, |_, b| {
        let mut out = Vec::new();
        for e in &ids {
            let data = b.edge(*e)?;
            let interval = data.interval;
            let Some(curve) = data.curve.as_ref() else {
                return Ok(Err(Error::Argument));
            };
            out.push((curve.clone_for_evaluation()?, interval));
        }
        Ok(Ok(out))
    })??;
    with_body("edges_lie_on_face", target, |k, b| {
        for (curve, interval) in &curves {
            if !curve_lies_on_face(k, b, f, curve, *interval, tolerance)? {
                return Ok(false);
            }
        }
        Ok(true)
    })
}

/// What becomes of `part` (a body, its `planar` faces), one piece of the prism `cut` cut at face `face` of `target`:
/// [`PrismPart::Dropped`] unless one of its `planar` faces is the start cap; else
/// [`PrismPart::ReachesPast`] when one is the far cap, or when an edge of its `section` (record edges of
/// the face the cut made, which came from no face of the prism) is not on the target's face
/// ([`edges_lie_on_face`]), or when the section is empty (nothing shows the piece ends at the face);
/// else [`PrismPart::Kept`].
///
/// # Errors
/// As [`edges_lie_on_face`].
pub fn prism_part(
    target: &Body,
    order: &Order,
    face: usize,
    part: &Body,
    part_order: &Order,
    planar: &[PlanarFace],
    section: &[usize],
    cut: &PrismCut,
) -> Result<PrismPart> {
    let cap_at = |height: f64| {
        planar.iter().any(|p| {
            is_square_plane_at_height(
                p.origin,
                p.normal,
                cut.origin,
                cut.normal,
                height,
                cut.cap_tolerance,
            )
        })
    };
    Ok(if !cap_at(cut.start) {
        PrismPart::Dropped
    } else if cap_at(cut.far)
        || section.is_empty()
        || !edges_lie_on_face(
            target,
            order,
            face,
            part,
            part_order,
            section,
            cut.section_tolerance,
        )?
    {
        PrismPart::ReachesPast
    } else {
        PrismPart::Kept
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::{make_box, make_cylinder};

    /// A body with its record's face and edge counts, and its order.
    fn made(body: &Body) -> (Order, usize, usize) {
        let (record, order) = body.to_record().unwrap();
        (order, record.faces.len(), record.edges.len())
    }

    fn on(target: &Body, part: &Body, edge: usize, face: usize) -> bool {
        let (order, ..) = made(target);
        let (part_order, ..) = made(part);
        edges_lie_on_face(target, &order, face, part, &part_order, &[edge], 1.0e-6).unwrap()
    }

    #[test]
    fn every_edge_of_a_cylinder_lies_on_a_face_of_it_and_the_circles_on_two() {
        // the circles are where a plane meets the cylinder: intersection edges
        let body = make_cylinder([0.0; 3], 5.0, 12.0).unwrap();
        let (_, faces, edges) = made(&body);
        let mut circles = 0;
        for e in 0..edges {
            let held = (0..faces).filter(|f| on(&body, &body, e, *f)).count();
            assert!(held >= 1, "edge {e} lies on no face");
            circles += usize::from(held == 2);
        }
        assert_eq!(circles, 2, "the two circles");
    }

    #[test]
    fn an_edge_of_another_body_is_on_none_of_a_boxes_faces() {
        let cylinder = make_cylinder([100.0, 0.0, 0.0], 5.0, 12.0).unwrap();
        let boxed = make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
        let (_, faces, _) = made(&boxed);
        let (_, _, edges) = made(&cylinder);
        for e in 0..edges {
            assert_eq!(
                (0..faces).filter(|f| on(&boxed, &cylinder, e, *f)).count(),
                0
            );
        }
    }

    #[test]
    fn no_edges_are_on_every_face_and_bad_input_is_refused() {
        let body = make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
        let (order, faces, edges) = made(&body);
        assert!(edges_lie_on_face(&body, &order, 0, &body, &order, &[], 1.0e-6).unwrap());
        assert_eq!(
            edges_lie_on_face(&body, &order, faces, &body, &order, &[], 1.0e-6),
            Err(Error::Argument)
        );
        assert_eq!(
            edges_lie_on_face(&body, &order, 0, &body, &order, &[edges], 1.0e-6),
            Err(Error::Argument)
        );
        assert_eq!(
            edges_lie_on_face(&body, &order, 0, &body, &order, &[0], 0.0),
            Err(Error::Argument)
        );
    }

    #[test]
    fn a_part_is_kept_dropped_or_reaching_past_by_its_caps_and_section() {
        let body = make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
        let (order, faces, _) = made(&body);
        // an edge of the box lying on some face stands for the section; edge 0 of the box
        let on_face = (0..faces).find(|f| on(&body, &body, 0, *f)).unwrap();
        let other = (0..faces).find(|f| !on(&body, &body, 0, *f)).unwrap();
        let cut = PrismCut {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            start: 0.0,
            far: 100.0,
            cap_tolerance: 1.0e-6,
            section_tolerance: 1.0e-6,
        };
        let cap = |z: f64| PlanarFace {
            origin: [1.0, 1.0, z],
            normal: [0.0, 0.0, 1.0],
        };
        let judge = |face: usize, planar: &[PlanarFace]| {
            prism_part(&body, &order, face, &body, &order, planar, &[0], &cut).unwrap()
        };
        assert_eq!(judge(on_face, &[cap(0.0)]), PrismPart::Kept);
        assert_eq!(
            judge(on_face, &[cap(5.0)]),
            PrismPart::Dropped,
            "no start cap"
        );
        assert_eq!(
            judge(on_face, &[cap(0.0), cap(100.0)]),
            PrismPart::ReachesPast,
            "the far cap is there"
        );
        assert_eq!(
            judge(other, &[cap(0.0)]),
            PrismPart::ReachesPast,
            "the section is off the face"
        );
        assert_eq!(
            prism_part(
                &body,
                &order,
                on_face,
                &body,
                &order,
                &[cap(0.0)],
                &[],
                &cut
            )
            .unwrap(),
            PrismPart::ReachesPast,
            "an empty section shows nothing at the face"
        );
    }
}
