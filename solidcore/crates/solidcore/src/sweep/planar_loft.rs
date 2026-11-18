//! A loft of two sections of lines whose sides are all flat, built as planar faces.
//!
//! Not upstream: SolidCraft's `planar_loft` (`crates/smlib/src/core/sweep.rs`,
//! solidcraft#13331), moved into the kernel so no application builds it. The skin
//! (`SmApiCreateSkinPrimitive`) makes each side of a loft a B-spline surface and each cap a
//! face trimmed by the sections' curves joined into one; between two sections of lines whose
//! corners line up, every side is a planar quadrilateral (a rectangle of a prism, a
//! trapezoid of a tapered duct), which is built here as the plane it is, on its own four
//! 3D lines, as [`crate::topology::brep::p8::create_planar_face_with_3d_curves`] builds a
//! planar face, and the faces are stitched and oriented. The result's faces are planes to
//! everything that asks (sheet metal, offsets, a sketch on a side), and a section of
//! thousands of lines stays thousands of short edges, not one long trim.
//!
//! | Rust            | Upstream |
//! |-----------------|----------|
//! | [`planar_loft`] | (none; SolidCraft's `planar_loft`) |

// The range tests are NaN-safe negations (`!(x > y)`), kept as written.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::error::Result;
use crate::geom::tol::USE_DEFAULT;
use crate::geom::vector3d::Point3;
use crate::kernel::Kernel;
use crate::sweep::segments::build_faces;
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use crate::topology::brep::p8::{create_planar_face_with_3d_curves, FaceBuildEnv};
use crate::topology::Brep;

/// The gap within which a section's lines run end to end round one loop.
const LOOP_GAP: f64 = 1e-6;
/// How far a side's fourth corner may lie from the plane of its other three, and how short a
/// side's line may be, as a fraction of the loft's size.
const FLAT: f64 = 1e-9;

/// Whether `curves`, in order, run end to end round one closed loop, within `gap`.
fn one_loop(k: &Kernel, curves: &[Curve], gap: f64) -> Result<bool> {
    let (mut first, mut prev) = (None, None);
    for c in curves {
        let e = c.ends(k)?;
        match prev {
            None => first = Some(e.start),
            Some(p) if (e.start - p).length() > gap => return Ok(false),
            Some(_) => {}
        }
        prev = Some(e.end);
    }
    Ok(matches!((first, prev), (Some(f), Some(p)) if (p - f).length() <= gap))
}

/// The loft of the two sections `first` and `second` as planar faces, when it is one: each
/// section three or more lines running round one loop (to 1e-6), in equal number, and every
/// side (a line of the first section and its partner in the second, by position) flat. The
/// sides are planar faces on their four 3D lines and, with `cap`, the sections are the caps;
/// the faces are stitched and oriented, and the result's cached topology refreshed.
///
/// `None` for any other pair of sections: a curve that is no line, sections of different
/// counts or fewer than three lines, a section that is not one closed loop, a side whose
/// fourth corner is not in the plane of its other three within 1e-9 of the loft's size (the
/// largest distance of a corner from the first), or a side one of whose lines is shorter
/// than that. The skin takes those.
///
/// The body is as the stitch leaves it: whether it is the closed solid a capped loft must
/// be is the caller's check.
///
/// # Errors
///
/// Those of the curves' ends, the face construction and the stitch.
pub fn planar_loft(
    k: &mut Kernel,
    first: &[Curve],
    second: &[Curve],
    cap: bool,
) -> Result<Option<Brep>> {
    let n = first.len();
    let lines = |section: &[Curve]| {
        section
            .iter()
            .all(|c| matches!(c.kind(), CurveKind::Line(_)))
    };
    if second.len() != n || n < 3 || !lines(first) || !lines(second) {
        return Ok(None);
    }
    if !one_loop(k, first, LOOP_GAP)? || !one_loop(k, second, LOOP_GAP)? {
        return Ok(None);
    }
    // Each section's corners: its lines' starts, in order.
    let mut bottom = Vec::new();
    for c in first {
        bottom.push(c.ends(k)?.start);
    }
    let mut top = Vec::new();
    for c in second {
        top.push(c.ends(k)?.start);
    }
    let Some(&origin) = bottom.first() else {
        return Ok(None);
    };
    let mut size = 0.0_f64;
    for q in bottom.iter().chain(&top) {
        size = size.max((*q - origin).length());
    }
    if !(size > 0.0) {
        return Ok(None);
    }
    // The sides' corners, each side the line from corner i to the next and its partner.
    let mut sides = Vec::new();
    for i in 0..n {
        let j = if i + 1 == n { 0 } else { i + 1 };
        let (Some(&a0), Some(&a1), Some(&b0), Some(&b1)) =
            (bottom.get(i), bottom.get(j), top.get(i), top.get(j))
        else {
            return Ok(None);
        };
        // Every side flat: its fourth corner in the plane of the other three, and no side
        // degenerate.
        if (a1 - a0).length() <= FLAT * size || (b1 - b0).length() <= FLAT * size {
            return Ok(None);
        }
        let normal = (a1 - a0).cross(b1 - a0);
        if normal.length() <= 1e-12 * size * size
            || (b0 - a0).dot(normal).abs() > FLAT * size * normal.length()
        {
            return Ok(None);
        }
        sides.push([a0, a1, b1, b0]);
    }
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    let region = b.infinite_region();
    build_faces(&mut b, |b, fenv| -> Result<()> {
        let face = |k: &mut Kernel,
                    b: &mut Brep,
                    fenv: &mut FaceBuildEnv<'_, '_>,
                    corners: &[Point3]|
         -> Result<()> {
            let mut edges = Vec::new();
            for (i, &p) in corners.iter().enumerate() {
                let q = corners
                    .get(i + 1)
                    .or_else(|| corners.first())
                    .copied()
                    .unwrap_or(p);
                edges.push(Curve::from(Line::create_line_segment(3, p, q, None)?));
            }
            create_planar_face_with_3d_curves(k, b, region, &edges, LOOP_GAP, fenv).map(|_| ())
        };
        if cap {
            face(k, b, fenv, &bottom)?;
            face(k, b, fenv, &top)?;
        }
        for side in &sides {
            face(k, b, fenv, side)?;
        }
        Ok(())
    })?;
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default())?;
    b.refresh_cached_topology();
    Ok(Some(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::KernelOptions;
    use crate::surface::surface::p2::SurfaceKind;
    use crate::topology::brep::p2::is_manifold_solid;

    fn polygon(points: &[[f64; 3]]) -> Vec<Curve> {
        (0..points.len())
            .map(|i| {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                Curve::from(
                    Line::create_line_segment(
                        3,
                        Point3::new(a[0], a[1], a[2]),
                        Point3::new(b[0], b[1], b[2]),
                        None,
                    )
                    .unwrap(),
                )
            })
            .collect()
    }

    fn square(half: f64, z: f64) -> Vec<Curve> {
        polygon(&[
            [-half, -half, z],
            [half, -half, z],
            [half, half, z],
            [-half, half, z],
        ])
    }

    fn planes(b: &Brep) -> usize {
        b.face_list()
            .iter()
            .filter(|&&f| {
                b.face(f)
                    .unwrap()
                    .surface
                    .as_ref()
                    .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)))
            })
            .count()
    }

    /// A square to a smaller square above it (a tapered duct): four planar trapezoids and,
    /// capped, two planar caps, a closed solid; uncapped, the four sides alone.
    #[test]
    fn a_tapered_duct_is_planar_faces() {
        let mut k = Kernel::new(&KernelOptions::default());
        let (bottom, top) = (square(10.0, 0.0), square(5.0, 20.0));
        let solid = planar_loft(&mut k, &bottom, &top, true).unwrap().unwrap();
        assert_eq!(solid.face_list().len(), 6);
        assert_eq!(planes(&solid), 6);
        assert_eq!(solid.edge_list().len(), 12);
        assert!(is_manifold_solid(&solid).unwrap());
        let sheet = planar_loft(&mut k, &bottom, &top, false).unwrap().unwrap();
        assert_eq!(sheet.face_list().len(), 4);
        assert_eq!(planes(&sheet), 4);
        assert!(!is_manifold_solid(&sheet).unwrap());
    }

    /// The sections it leaves to the skin: a twisted side (the top turned an eighth of a
    /// turn, so no side's four corners are in one plane), sections of different counts,
    /// fewer than three lines, an open section and a curve that is no line.
    #[test]
    fn other_sections_are_left_to_the_skin() {
        let mut k = Kernel::new(&KernelOptions::default());
        let bottom = square(10.0, 0.0);
        let r = 10.0 * std::f64::consts::SQRT_2;
        let turned = polygon(&[
            [0.0, -r, 20.0],
            [r, 0.0, 20.0],
            [0.0, r, 20.0],
            [-r, 0.0, 20.0],
        ]);
        assert!(planar_loft(&mut k, &bottom, &turned, true)
            .unwrap()
            .is_none());
        let triangle = polygon(&[[0.0, 0.0, 20.0], [5.0, 0.0, 20.0], [0.0, 5.0, 20.0]]);
        assert!(planar_loft(&mut k, &bottom, &triangle, true)
            .unwrap()
            .is_none());
        let two = polygon(&[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]]);
        assert!(planar_loft(&mut k, &two, &two, true).unwrap().is_none());
        let open = &bottom[..3];
        assert!(planar_loft(&mut k, open, &square(10.0, 20.0)[..3], true)
            .unwrap()
            .is_none());
        let circle = Curve::from(
            crate::curve::ellipse::Ellipse::create_canonical_circle(
                &k,
                &crate::geom::axis2_placement::Axis2Placement::default(),
                5.0,
                None,
                None,
            )
            .unwrap(),
        );
        assert!(planar_loft(&mut k, &[circle.clone()], &[circle], true)
            .unwrap()
            .is_none());
    }
}
