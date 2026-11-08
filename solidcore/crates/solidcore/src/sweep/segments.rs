//! The sweep of a profile along a path of lines and arcs, turning with it, built exactly
//! piece by piece: each segment swept on its own (a line by the linear sweep, an arc by
//! the rotational sweep, so every wall keeps the analytic kind those sweeps give it),
//! the profile carried by the rigid motion from each segment to the next, the walls
//! sewn along the sections they share, and the ends capped on request. The segments
//! are checked to make one path ([`segments_join`]) whoever builds them.
//! TODO(d0b44726f): an arc profile's walls keep the circle class of
//! [`crate::curve::ellipse::Ellipse::arc`], at bound 0 by its label until the rebase
//! onto d0b44726f proves it (`implicit::curve_on_quadrics`).
//!
//! Not upstream: SolidCraft's `piecewise` sweep (with `turned`, `Rigid`, `Segment` and
//! `carried`). The path's segments come from [`crate::curve::path::path_as_segments`].
//! Deviations: none (no upstream counterpart). No recursion.

use std::f64::consts::PI;

use crate::curve::curve::Curve;
use crate::curve::path::{rotation, segments_join, PathShape, PathTolerances, Segment};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::certified::unit;
use crate::geom::tol::USE_DEFAULT;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::Transform;
use crate::sweep::api_brep::{create_linear_sweep, create_rotational_sweep};
use crate::topology::brep::p1 as brep_p1;
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use crate::topology::brep::p7::copy_faces;
use crate::topology::brep::p8::{create_planar_faces_with_3d_curves, FaceBuildEnv};
use crate::topology::topology_traverser::Marks;
use crate::topology::{Brep, FaceId};

const OP: &str = "sweep::segments::sweep_along_segments";

/// The motion `m` carried the fraction `f` (`0` to `1`) along segment `g`: moved by
/// that fraction of a line, turned by that fraction of an arc's turn.
fn carry(g: &Segment, m: &Axis2Placement, f: f64) -> Axis2Placement {
    let mut out = *m;
    match *g {
        Segment::Arc {
            centre,
            axis,
            angle,
            ..
        } => out.rotate_about_axis_at_point(angle * f, centre, axis),
        Segment::Line { start, end } => out.translate((end - start) * f),
    }
    out
}

/// Copies of the profile's curves carried by `m`.
fn carried(k: &Kernel, profile: &[Curve], m: &Axis2Placement) -> Result<Vec<Curve>> {
    let mut out = Vec::new();
    for c in profile {
        let mut copy = c.clone();
        copy.transform(k, m, None)?;
        out.push(copy);
    }
    Ok(out)
}

/// Runs `f` with a face-construction environment whose faces are known by their arena
/// identity (as the sweeps' caps and loft faces are built).
pub fn build_faces<T>(
    b: &mut Brep,
    f: impl FnOnce(&mut Brep, &mut FaceBuildEnv<'_, '_>) -> T,
) -> T {
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    f(b, &mut fenv)
}

/// The motions that place a profile at the start, half way along and at the end of a
/// straight sweep along the unit `direction` for `distance`: moves by
/// `direction * (distance * j / 2)`.
#[must_use]
pub fn line_placements(direction: Vec3, distance: f64) -> [Transform; 3] {
    let at = |j: u32| Transform::translation(direction * (distance * 0.5 * f64::from(j)));
    [at(0), at(1), at(2)]
}

/// The motions that place a profile at the start, half way and at the end of a turn of
/// `angle_deg` degrees right-handed about `axis` (any length) through `centre`
/// ([`rotation`]).
///
/// # Errors
///
/// `InvalidInput` for an axis not finite or without a length.
pub fn turn_placements(centre: Point3, axis: Vec3, angle_deg: f64) -> Result<[Transform; 3]> {
    if unit(axis).is_none() {
        return Err(SolidError::invalid_input(
            "sweep::segments::turn_placements",
            "an axis without a length",
        ));
    }
    let turn = angle_deg * PI / 180.0;
    let at = |j: u32| Transform::of_placement(&rotation(centre, axis, turn * 0.5 * f64::from(j)));
    Ok([at(0), at(1), at(2)])
}

/// The profile swept along the path of `segments`, turning with it, exactly piece by
/// piece; capped at both ends with `cap`. With the motions that place the profile at
/// the path's start, half way along it and at its end. The segments must be proved to
/// make one path to `tol` ([`segments_join`]: each starting where the one before ends,
/// leaving in the direction it arrives in, every turn positive and at most a whole
/// one), whatever built them.
///
/// # Errors
///
/// `InvalidInput` for segments not proved to make one path; those of the sweeps, the
/// face copies, the caps and the stitch.
pub fn sweep_along_segments(
    k: &mut Kernel,
    profile: &[Curve],
    segments: &[Segment],
    cap: bool,
    tol: &PathTolerances,
) -> Result<(Brep, [Transform; 3])> {
    if segments_join(segments, tol) != PathShape::Is(()) {
        return Err(SolidError::invalid_input(
            OP,
            "segments not proved to make one path",
        ));
    }
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    // The path starts where the profile lies: the identity.
    let start = Axis2Placement::IDENTITY;
    let mut m = start;
    let mut starts = Vec::new();
    let mut lengths = Vec::new();
    let mut total = 0.0;
    for g in segments {
        starts.push(m);
        let l = g.length()?;
        lengths.push(l);
        total += l;
        let section = carried(k, profile, &m)?;
        let refs: Vec<Option<&Curve>> = section.iter().map(Some).collect();
        let mut piece = match *g {
            Segment::Arc {
                centre,
                axis,
                angle,
                ..
            } => create_rotational_sweep(k, &refs, centre, axis, angle * 180.0 / PI, false)?,
            Segment::Line { start, end } => {
                let Some(direction) = unit(end - start) else {
                    return Err(SolidError::invalid_input(OP, "a line without a length"));
                };
                create_linear_sweep(k, &refs, direction, l, false)?
            }
        };
        let faces = brep_p1::faces(&piece, None)?;
        let mut state = ArenaNotifyState::new(&piece);
        state.seed(&b);
        let mut env = state.env(None);
        let key_of = env.key_of;
        let this_face = |f: FaceId| key_of(EntityRef::Face(f));
        let (_, copied) = copy_faces(k, &mut piece, &faces, &mut b, false, &this_face, &mut env)?;
        if !copied {
            return Err(SolidError::sm_err("CopyFaces"));
        }
        m = carry(g, &m, 1.0);
    }
    if cap {
        // The caps: the profile where the path starts and where it ends.
        for at in [start, m] {
            let section = carried(k, profile, &at)?;
            let (region, tol) = (b.infinite_region(), b.tolerance());
            build_faces(&mut b, |b, fenv| {
                create_planar_faces_with_3d_curves(k, b, region, &section, tol, fenv)
            })?;
        }
    }
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default())?;
    b.refresh_cached_topology();
    let mut half = 0.5 * total;
    let mut middle = m;
    for (i, ((g, from), &l)) in segments.iter().zip(&starts).zip(&lengths).enumerate() {
        if half <= l || i + 1 == segments.len() {
            middle = carry(g, from, half.min(l) / l);
            break;
        }
        half -= l;
    }
    let placed = [start, middle, m].map(|p| Transform::of_placement(&p));
    Ok((b, placed))
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use super::*;
    use crate::curve::arc::ArcDefinition;
    use crate::curve::ellipse::Ellipse;
    use crate::topology::brep::p2::is_manifold_solid;

    const TOL: PathTolerances = PathTolerances {
        gap: 1e-9,
        turn: 1e-12,
        length: 1e-12,
        closed: 1e-6,
    };

    /// A circle of radius `r` about the origin in the plane x = 0.
    fn disc(k: &Kernel, r: f64) -> Vec<Curve> {
        let d = ArcDefinition {
            centre: Point3::ZERO,
            axis: Vec3::X,
            reference: Vec3::Y,
            radius: r,
            start: 0.0,
            end: std::f64::consts::TAU,
        };
        vec![Curve::from(Ellipse::arc(k, &d).unwrap())]
    }

    /// Along x for 4, then a turn of `angle` about z through (4, 2, 0), radius 2.
    fn bent(angle: f64) -> [Segment; 2] {
        [
            Segment::Line {
                start: Point3::ZERO,
                end: Point3::new(4.0, 0.0, 0.0),
            },
            Segment::Arc {
                start: Point3::new(4.0, 0.0, 0.0),
                centre: Point3::new(4.0, 2.0, 0.0),
                axis: Vec3::Z,
                angle,
            },
        ]
    }

    #[test]
    fn a_bent_rod_is_a_cylinder_into_a_quarter_torus() {
        let mut k = Kernel::default();
        let profile = disc(&k, 0.5);
        let segments = bent(FRAC_PI_2);
        let (b, placed) = sweep_along_segments(&mut k, &profile, &segments, true, &TOL).unwrap();
        assert!(is_manifold_solid(&b).unwrap());
        // The end: turned a quarter about z, at (6, 2, 0).
        let end = placed[2];
        assert!((end.transform_point(Point3::ZERO) - Point3::new(6.0, 2.0, 0.0)).length() < 1e-12);
        assert!((end.transform_vector(Vec3::X) - Vec3::Y).length() < 1e-15);
        // Half way along (2 + pi / 2 of 4 + pi): still on the line, not turned.
        let half = 0.5 * (4.0 + 2.0 * FRAC_PI_2);
        let mid = placed[1].transform_point(Point3::ZERO);
        assert!((mid - Point3::new(half, 0.0, 0.0)).length() < 1e-12);
        assert!((placed[1].transform_vector(Vec3::X) - Vec3::X).length() < 1e-15);
        assert_eq!(placed[0], Transform::IDENTITY);
        // Walls and caps: the cylinder, the torus piece, two discs.
        assert_eq!(brep_p1::faces(&b, None).unwrap().len(), 4);
    }

    #[test]
    fn placements_of_a_straight_sweep_and_a_turn() {
        let l = line_placements(Vec3::new(0.0, 0.6, 0.8), 10.0);
        assert_eq!(
            l[1].transform_point(Point3::ZERO),
            Point3::new(0.0, 3.0, 4.0)
        );
        assert_eq!(
            l[2].transform_point(Point3::ZERO),
            Point3::new(0.0, 6.0, 8.0)
        );
        // A half turn about z through (1, 0, 0): the origin goes to (2, 0, 0), a quarter
        // turn half way to (1, -1, 0).
        let t =
            turn_placements(Point3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 3.0), 180.0).unwrap();
        assert!(
            (t[1].transform_point(Point3::ZERO) - Point3::new(1.0, -1.0, 0.0)).length() < 1e-15
        );
        assert!((t[2].transform_point(Point3::ZERO) - Point3::new(2.0, 0.0, 0.0)).length() < 1e-15);
        assert!((t[2].transform_vector(Vec3::X) + Vec3::X).length() < 1e-15);
        assert!(turn_placements(Point3::ZERO, Vec3::ZERO, 90.0).is_err());
    }

    #[test]
    fn segments_that_do_not_make_one_path_are_refused() {
        let mut k = Kernel::default();
        let profile = disc(&k, 0.5);
        let refused = |k: &mut Kernel, segments: &[Segment]| {
            matches!(
                sweep_along_segments(k, &profile, segments, true, &TOL),
                Err(SolidError::InvalidInput { .. })
            )
        };
        assert!(refused(&mut k, &[]));
        // A kink: along x, then along y (the second leg would sweep the disc in its own
        // plane).
        let kink = [
            Segment::Line {
                start: Point3::ZERO,
                end: Point3::new(4.0, 0.0, 0.0),
            },
            Segment::Line {
                start: Point3::new(4.0, 0.0, 0.0),
                end: Point3::new(4.0, 4.0, 0.0),
            },
        ];
        assert!(refused(&mut k, &kink));
        // An arc turning backwards into the line's wall.
        assert!(refused(&mut k, &bent(-FRAC_PI_2)));
        // A line of no length.
        let zero = [Segment::Line {
            start: Point3::ZERO,
            end: Point3::ZERO,
        }];
        assert!(refused(&mut k, &zero));
        // The arc starting off the line's end.
        let [l, a] = bent(FRAC_PI_2);
        let Segment::Arc {
            centre,
            axis,
            angle,
            ..
        } = a
        else {
            panic!()
        };
        let apart = [
            l,
            Segment::Arc {
                start: Point3::new(4.0, 1e-6, 0.0),
                centre,
                axis,
                angle,
            },
        ];
        assert!(refused(&mut k, &apart));
    }
}
