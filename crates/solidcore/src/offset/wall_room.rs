//! Not upstream (SolidCraft's `leaves_cavity`, from the fork's `capi/src/smlib_ops.cpp`): the
//! room test before an inward shell or a face offset inward.
//!
//! | Rust | SolidCraft |
//! |------|-----------|
//! | [`shell_wall_room`] | `leaves_cavity` |
//! | [`WallRoom`] | `leaves_cavity`'s `bool`, as a certified witness or none |
//! | `FaceSet` (private) | `FaceSet` (`add`, `nearest`) |
//!
//! An inward offset of `wall` from the faces of a solid leaves a cavity when some point inside
//! the solid lies farther than `wall` (and a margin) from every offset face. A point counts
//! only as the centre of a ball inside the solid, so that it is reached through material: the
//! balls start at face samples and step inward along the normal, then climb from ball to ball
//! away from the nearest offset face. A step of `h` from a sample ends at a ball's centre when
//! no face is nearer to it than `h`. The test is a witness search on the closest-point
//! solves, then the witness certified on lower bounds of its distances
//! ([`face_distance_lower_bound`]): [`WallRoom::Room`] names a point that is there,
//! [`WallRoom::NotShown`] claims nothing (the capi refuses the shell then; C++ SMLib corrupts
//! memory on a wall with no room, solidcore#380).

use crate::geom::certified::above;
use crate::geom::object::Shape;
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::nurbs::math::gamma;
use crate::query::api_queries::{face_bounding_box, face_classify_uv, face_closest_point_in_shape};
use crate::query::brep::{BrepQuery, LiveBrepObjects};
use crate::query::distance_bound::face_distance_lower_bound;
use crate::query::distance_bound::measurable;
use crate::topology::brep::is_outward_solid;
use crate::topology::brep::p1 as brep_p1;
use crate::topology::brep::p2::is_manifold_solid;
use crate::topology::face::p2::outer_faceuse;
use crate::topology::{Brep, BrepId, FaceId};
use crate::{Result, SolidError};

const OP: &str = "offset::wall_room::shell_wall_room";

/// The B-rep id every shape and face key here names the one body by.
const ONE: BrepId = BrepId::from_index(0);

/// Face samples per side of a face's domain.
const GRID: usize = 5;

/// The most face samples taken.
const MAX_SAMPLES: usize = 2048;

/// The most climbing steps from one sample.
const STEPS: usize = 48;

/// Faces with their loose bounding boxes, for nearest-face queries.
struct FaceSet {
    faces: Vec<FaceId>,
    boxes: Vec<(Point3, Point3)>,
    /// Each face's shape (`SmApiFaceClosestPoint` makes one per query; it depends only on
    /// the face, so it is made once).
    shapes: Vec<Shape>,
}

impl FaceSet {
    const fn new() -> Self {
        Self {
            faces: Vec::new(),
            boxes: Vec::new(),
            shapes: Vec::new(),
        }
    }

    /// `add`: `false` when the face's box or shape cannot be made (a status).
    fn add(&mut self, k: &mut Kernel, b: &Brep, f: FaceId) -> Result<bool> {
        let bx = match face_bounding_box(k, b, f, false) {
            Ok(bx) => bx,
            Err(e) if e.sm_status().is_some() => return Ok(false),
            Err(e) => return Err(e),
        };
        let shape = match Shape::new(k, &(ONE, b), &[], &[(ONE, f)], &[], &[]) {
            Ok(s) => s,
            Err(e) if e.sm_status().is_some() => return Ok(false),
            Err(e) => return Err(e),
        };
        self.faces.push(f);
        self.boxes.push((bx.min(), bx.max()));
        self.shapes.push(shape);
        Ok(true)
    }

    /// `nearest`: the distance from `p` to the nearest face, with the nearest point; infinite
    /// when none answers (a face whose solve returns a status is skipped). A face whose box is
    /// farther than the nearest face so far is skipped.
    fn nearest(
        &self,
        k: &Kernel,
        live: &LiveBrepObjects<'_, '_>,
        p: Point3,
    ) -> Result<(f64, Option<Point3>)> {
        let (mut best, mut at) = (f64::INFINITY, None);
        for ((&f, (lo, hi)), shape) in self.faces.iter().zip(&self.boxes).zip(&self.shapes) {
            let mut bx = 0.0;
            for (l, (h, x)) in [lo.x, lo.y, lo.z]
                .into_iter()
                .zip([hi.x, hi.y, hi.z].into_iter().zip([p.x, p.y, p.z]))
            {
                let out = (l - x).max((x - h).max(0.0));
                bx += out * out;
            }
            if f64::sqrt(bx) >= best {
                continue;
            }
            let c = match face_closest_point_in_shape(k, shape, live, (ONE, f), p) {
                Ok(c) => c,
                Err(e) if e.sm_status().is_some() => continue,
                Err(e) => return Err(e),
            };
            if c.distance.is_finite() && c.distance < best {
                best = c.distance;
                at = Some(c.point);
            }
        }
        Ok((best, at))
    }
}

/// What [`shell_wall_room`] shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WallRoom {
    /// Room, with its witness: `centre` is at least `clearance` (a certified lower bound,
    /// above the wall) from every offset face. It is reached from a face sample, a step along
    /// the inward normal of the face's outer faceuse (so into the closed solid's material)
    /// and then balls proved free of the boundary: the whole path is proved free of the
    /// boundary but its first `gap` (rounded up) at the sample, where the first ball touches
    /// the sample's face. So `centre` is inside the solid unless the boundary comes back
    /// within `gap` of the sample along its normal.
    Room {
        /// The witness.
        centre: Point3,
        /// Its certified distance from the offset faces.
        clearance: f64,
        /// The length of its path from the sample not proved free of the boundary.
        gap: f64,
    },
    /// No witness was found or proved: no claim either way.
    NotShown,
    /// Every face is open: nothing is offset, so no point is near an offset face.
    NoWalls,
    /// The kernel cannot bound the distance to `face` (a surface kind without a NURBS form:
    /// an offset, STEP, curve-bounded or in-volume surface): no search, as any face may be
    /// the nearest to a witness, so the call refuses as a whole.
    Unmeasurable {
        /// The first such face, in the body's face order.
        face: FaceId,
    },
}

/// Whether an inward offset of `wall` (> 0) from every face of the solid `b` but `open`
/// leaves a cavity: a point inside `b` farther than `wall`, and a margin
/// (`max(10 tol, 1e-3 wall)`, `tol` the body's), from every face that is offset. Samples: a
/// 5 × 5 grid over each offset face's domain, the points inside its trims, each with the
/// inward unit normal there (at most 2048); first the direct step of `wall + margin` inward
/// from each, then up to 48 climbing steps from a small ball at each. Every face open is
/// [`WallRoom::NoWalls`]; a face of a kind the bound cannot measure is
/// [`WallRoom::Unmeasurable`], before any search; a face whose box or shape cannot be made is
/// [`WallRoom::NotShown`]. The search runs on the closest-point solves; a witness it finds
/// is then certified ([`face_distance_lower_bound`]): its clearance from the offset faces,
/// each ball of its path proved free of the boundary farther than the step to the next, and
/// the first ball's gap at its sample. A witness that does not certify is passed over.
/// Samples and faces whose evaluations or solves return a status are skipped; the face
/// classifications may make UV trims on `b`.
///
/// # Errors
///
/// `InvalidInput` for a wall not finite and positive, a body that is not a closed manifold
/// solid, or one that is not a valid solid facing out ([`is_outward_solid`]: a body with every
/// face flipped is refused by the validity check's pointer check); non-status errors of the
/// face boxes, shapes, classifications, evaluations, closest-point solves and distance bounds.
pub fn shell_wall_room(
    k: &mut Kernel,
    b: &mut Brep,
    open: &[FaceId],
    wall: f64,
) -> Result<WallRoom> {
    if !(wall.is_finite() && wall > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a wall is finite and positive",
        ));
    }
    if !is_manifold_solid(b)? {
        return Err(SolidError::invalid_input(
            OP,
            "a wall is offset inside a closed solid",
        ));
    }
    let list = brep_p1::faces(b, None)?;
    for &f in &list {
        let measurable = match b.face(f)?.surface.as_ref() {
            Some(s) => measurable(k, s)?,
            None => false,
        };
        if !measurable {
            return Ok(WallRoom::Unmeasurable { face: f });
        }
    }
    // the inward direction is read off the outer faceuses: they must face out of the
    // material, which manifold topology alone does not say (a body with every face flipped
    // is still manifold, and its volume still positive: the validity check's pointer check
    // is what refuses it)
    if !is_outward_solid(k, b)? {
        return Err(SolidError::invalid_input(
            OP,
            "a valid closed solid facing out",
        ));
    }
    let (mut all, mut walls) = (FaceSet::new(), FaceSet::new());
    for &f in &list {
        if !all.add(k, b, f)? {
            return Ok(WallRoom::NotShown);
        }
        if !open.contains(&f) && !walls.add(k, b, f)? {
            return Ok(WallRoom::NotShown);
        }
    }
    if walls.faces.is_empty() {
        return Ok(WallRoom::NoWalls);
    }
    let tol = b.tolerance();
    let margin = (10.0 * tol).max(1e-3 * wall);
    let need = wall + margin;
    let mut samples: Vec<(Point3, Vec3)> = Vec::new();
    for &f in &walls.faces {
        let Some(fu) = outer_faceuse(b, f)? else {
            continue;
        };
        let inward = if b.faceuse(fu)?.orientation == OrientType::Same {
            -1.0
        } else {
            1.0
        };
        let dom = b.face(f)?.uv_domain;
        for i in 0..GRID {
            for j in 0..GRID {
                if samples.len() >= MAX_SAMPLES {
                    break;
                }
                let uv = dom.evaluate(
                    (i as f64 + 0.5) / GRID as f64,
                    (j as f64 + 0.5) / GRID as f64,
                );
                match face_classify_uv(k, b, f, uv) {
                    Ok(PointClassType::Face) => {}
                    Ok(_) => continue,
                    Err(e) if e.sm_status().is_some() => continue,
                    Err(e) => return Err(e),
                }
                let Some(surf) = b.face(f)?.surface.as_ref() else {
                    break;
                };
                let at = surf.evaluate_point(k, uv);
                let n = surf.evaluate_normal(k, uv, Side::Left, Side::Left);
                let (at, n) = match (at, n) {
                    (Ok(at), Ok(n)) => (at, n),
                    (Err(e), _) | (_, Err(e)) if e.sm_status().is_none() => return Err(e),
                    _ => continue,
                };
                let len = n.length();
                if !above(len, 0.0) || !len.is_finite() {
                    continue;
                }
                samples.push((at, n * (inward / len)));
            }
        }
    }
    let query = BrepQuery::of_brep(b);
    let identity = query.effective_identity();
    let objects = LiveBrepObjects::new(b, &identity, None)?;
    let k: &Kernel = k;
    // A step of `h` inward from a sample ends at a ball's centre when no face is nearer to it
    // than `h` (the step crossed none, and the ball touches only the sample's face).
    let ball = |s: Point3, n: Vec3, h: f64| -> Result<(bool, Point3, f64)> {
        let c = s + n * h;
        let (r, _) = all.nearest(k, &objects, c)?;
        Ok((r.is_finite() && r >= h * (1.0 - 1e-9), c, r))
    };
    // First the direct step of `need` from each sample.
    let certify = |s: Point3, chain: &[Point3]| -> Result<Option<WallRoom>> {
        objects.with_brep(|b| certified(k, b, &all.faces, &walls.faces, s, chain, wall, need))
    };
    for &(s, n) in &samples {
        let (inside, c, _) = ball(s, n, need)?;
        if inside && walls.nearest(k, &objects, c)?.0 >= need {
            if let Some(room) = certify(s, &[c])? {
                return Ok(room);
            }
        }
    }
    // Then climbing, from a small ball at each sample: step away from the nearest offset face
    // by most of the ball's radius (staying inside the ball, so inside the body) while the
    // distance from the offset faces grows.
    for &(s, n) in &samples {
        let (inside, mut c, mut r) = ball(s, n, margin.max(1e3 * tol))?;
        if !inside {
            continue;
        }
        let mut chain = vec![c];
        for _ in 0..STEPS {
            let (d, q) = walls.nearest(k, &objects, c)?;
            if d >= need {
                if let Some(room) = certify(s, &chain)? {
                    return Ok(room);
                }
                break;
            }
            let Some(q) = q.filter(|_| d > 0.0 && d.is_finite()) else {
                break;
            };
            let next = c + (c - q) * (0.9 * r / d);
            let (rn, _) = all.nearest(k, &objects, next)?;
            if !above(rn, 0.0)
                || !rn.is_finite()
                || walls.nearest(k, &objects, next)?.0 <= d * (1.0 + 1e-9)
            {
                break;
            }
            c = next;
            r = rn;
            chain.push(c);
        }
    }
    Ok(WallRoom::NotShown)
}

/// The part of a step of computed length `step` from the sample not proved free of the
/// boundary, the step's end proved `lower` from every face: the step's length rounded up
/// (within `gamma_4` of exact), less `lower`, the difference rounded up; never below 0. A
/// step whose computed length equals `lower` still has a positive gap.
fn gap_of(step: f64, lower: f64) -> f64 {
    ((step * (1.0 + gamma(4)) - lower) * (1.0 + gamma(1))).max(0.0)
}

/// The certified lower bound of the distance from `p` to the nearest of `faces`, asked to
/// reach `want`.
fn least(k: &Kernel, b: &Brep, faces: &[FaceId], p: Point3, want: f64) -> Result<f64> {
    let mut best = f64::INFINITY;
    for &f in faces {
        best = best.min(face_distance_lower_bound(k, b, f, p, want)?);
    }
    Ok(best)
}

/// The witness at the end of `chain`, reached from the sample `s`, certified: each ball's
/// centre farther from every face than the step to the next (the step stays in a ball free
/// of the boundary), the last one farther than `wall` from the offset faces (the bound asked
/// to reach `need`, the wall and its margin, which the search found; the margin is the
/// search's, the claim is the wall's); `None` where a bound falls short.
#[allow(clippy::too_many_arguments)]
fn certified(
    k: &Kernel,
    b: &Brep,
    all: &[FaceId],
    walls: &[FaceId],
    s: Point3,
    chain: &[Point3],
    wall: f64,
    need: f64,
) -> Result<Option<WallRoom>> {
    let (Some(&first), Some(&last)) = (chain.first(), chain.last()) else {
        return Ok(None);
    };
    let step = (first - s).length();
    let gap = gap_of(step, least(k, b, all, first, step * (1.0 + gamma(4)))?);
    for (a, z) in chain.iter().zip(chain.iter().skip(1)) {
        // the step's length within gamma_4 of exact: rounded up, as the first one
        let step = (*z - *a).length() * (1.0 + gamma(4));
        if !above(least(k, b, all, *a, step * (1.0 + 1e-6))?, step) {
            return Ok(None);
        }
    }
    let clearance = least(k, b, walls, last, need)?;
    Ok((clearance > wall).then_some(WallRoom::Room {
        centre: last,
        clearance,
        gap,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives::{create_box, create_sphere};
    use crate::query::api_queries::face_bounding_box;

    /// The 20 mm block's top face (`z = 20`).
    fn top(k: &Kernel, b: &Brep) -> FaceId {
        brep_p1::faces(b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let bx = face_bounding_box(k, b, f, true).unwrap();
                (bx.min().z - 20.0).abs() < 1e-9 && (bx.max().z - 20.0).abs() < 1e-9
            })
            .unwrap()
    }

    fn room(r: WallRoom) -> (Point3, f64) {
        match r {
            WallRoom::Room {
                centre,
                clearance,
                gap,
            } => {
                // the path's unproved part is the first ball's rounding at its sample
                assert!((0.0..=1e-9).contains(&gap), "{gap}");
                (centre, clearance)
            }
            other => panic!("no room shown: {other:?}"),
        }
    }

    /// A 20 mm block open at the top: a wall leaves room while it and its margin are within
    /// the half width, 10; the witness is proved that far from the four sides and the bottom.
    #[test]
    fn an_open_block_has_room_up_to_its_half_width() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
        let open = [top(&k, &b)];
        let wall = 9.9;
        let (c, clearance) = room(shell_wall_room(&mut k, &mut b, &open, wall).unwrap());
        assert!(clearance > wall, "{clearance}");
        // the clearance is a lower bound of the closed form: the nearest of the five walls
        let exact = [c.x, 20.0 - c.x, c.y, 20.0 - c.y, c.z]
            .into_iter()
            .fold(f64::INFINITY, f64::min);
        assert!(clearance <= exact + 1e-12, "{clearance} {exact}");
        assert_eq!(
            shell_wall_room(&mut k, &mut b, &open, 10.5).unwrap(),
            WallRoom::NotShown
        );
    }

    #[test]
    fn a_ball_has_room_inside_its_radius() {
        let mut k = Kernel::default();
        let mut b = create_sphere(&mut k, Point3::ZERO, 10.0).unwrap();
        let (c, clearance) = room(shell_wall_room(&mut k, &mut b, &[], 9.0).unwrap());
        assert!(
            clearance > 9.0 && clearance <= 10.0 - c.length() + 1e-12,
            "{clearance}"
        );
        assert_eq!(
            shell_wall_room(&mut k, &mut b, &[], 10.5).unwrap(),
            WallRoom::NotShown
        );
    }

    #[test]
    fn every_face_open_has_no_walls() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
        let all = brep_p1::faces(&b, None).unwrap();
        assert_eq!(
            shell_wall_room(&mut k, &mut b, &all, 50.0).unwrap(),
            WallRoom::NoWalls
        );
    }

    /// A wall that is not finite and positive, and a body that is no closed solid, are refused
    /// before any search: a wall of -1 in the 20 mm block found a "room" outside it before.
    #[test]
    fn a_wall_or_body_out_of_its_domain_is_refused() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
        let open = [top(&k, &b)];
        for wall in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
            assert!(
                matches!(
                    shell_wall_room(&mut k, &mut b, &open, wall),
                    Err(SolidError::InvalidInput { .. })
                ),
                "{wall}"
            );
        }
        let mut sheet =
            crate::primitive::api_primitives::create_plane(&mut k, Point3::ZERO, 10.0, 10.0)
                .unwrap();
        assert!(matches!(
            shell_wall_room(&mut k, &mut sheet, &[], 1.0),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    /// A face on an offset surface (no NURBS form to bound): refused naming it, before the
    /// search.
    #[test]
    fn a_face_the_bound_cannot_measure_is_named() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
        let f = top(&k, &b);
        let base = b.face(f).unwrap().surface.clone().unwrap();
        let off =
            crate::surface::offset_surface::OffsetSurface::new(&k, 0.0, base, true, 0).unwrap();
        b.face_mut(f).unwrap().surface = Some(crate::surface::surface::Surface::from(off));
        assert_eq!(
            shell_wall_room(&mut k, &mut b, &[], 5.0).unwrap(),
            WallRoom::Unmeasurable { face: f }
        );
    }

    /// A witness whose path leaves a ball (its step longer than the ball is proved free) does
    /// not certify; nor one whose last centre is within the wall.
    #[test]
    fn a_witness_that_does_not_certify_is_passed_over() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
        let all = brep_p1::faces(&b, None).unwrap();
        let s = Point3::new(10.0, 10.0, 0.0);
        let c0 = Point3::new(10.0, 10.0, 1.0);
        // a step of 15 from a ball 1 from the bottom
        let far = Point3::new(10.0, 10.0, 16.0);
        assert_eq!(
            certified(&k, &b, &all, &all, s, &[c0, far], 2.0, 2.1).unwrap(),
            None
        );
        // in the middle, 10 from every face, but asked for a wall of 10
        let mid = Point3::new(10.0, 10.0, 10.0);
        // a ball 7 from the bottom, the step of 3 to the middle inside it
        let up = Point3::new(10.0, 10.0, 7.0);
        assert_eq!(
            certified(&k, &b, &all, &all, s, &[up, mid], 10.0, 10.01).unwrap(),
            None
        );
        // and a good one
        assert!(certified(&k, &b, &all, &all, s, &[up, mid], 9.0, 9.01)
            .unwrap()
            .is_some());
    }

    /// A 20 mm block with every face flipped is still manifold, but faces inward: refused (a
    /// wall of 15 found a "room" outside it before).
    #[test]
    fn an_inside_out_body_is_refused() {
        use crate::topology::face::p1::flip_face_orientation;
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
        for f in brep_p1::faces(&b, None).unwrap() {
            flip_face_orientation(&mut b, f).unwrap();
        }
        let vol = crate::mass::integrator::properties::brep_volume_default(&k, &mut b, 1e-3);
        let r = shell_wall_room(&mut k, &mut b, &[], 15.0);
        assert!(
            matches!(r, Err(SolidError::InvalidInput { .. })),
            "{r:?} {vol:?}"
        );
    }

    /// A first ball exactly as far from the sample's face as its step: the computed gap is
    /// rounded up, never 0 though the bound meets the step (the step's own rounding).
    #[test]
    fn the_gap_is_rounded_up() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
        let all = brep_p1::faces(&b, None).unwrap();
        let s = Point3::new(10.0, 10.0, 0.0);
        let c0 = Point3::new(10.0, 10.0, 8.0);
        let Some(WallRoom::Room { gap, .. }) =
            certified(&k, &b, &all, &all, s, &[c0], 7.0, 7.5).unwrap()
        else {
            panic!("the witness certifies");
        };
        // (above 0 though the plane is exact: the bound carries its form's rounding)
        assert!(gap > 0.0 && gap < 1e-9, "{gap}");
    }

    /// The gap's rounding up: a step whose computed length equals the bound at its end (the
    /// unrounded difference 0) leaves a positive gap, at least the step's own rounding.
    #[test]
    fn a_step_as_long_as_its_bound_has_a_gap() {
        let g = gap_of(1.0, 1.0);
        assert!(g > 0.0, "{g}");
        assert_eq!(gap_of(1.0, 2.0), 0.0);
    }
}
