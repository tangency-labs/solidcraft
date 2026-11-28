//! A loft guided by rails and a blend through ordered cross-sections (#3140): the kernel's
//! [`solidcore::sweep::loft_rails`] and [`solidcore::sweep::section_blend`] behind ops. Ops
//! converts and checks shapes and maps the kernel's refusals; it computes no geometry.

use solidcore::geom::geom_utility::p2::segment_segment_closest_point;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::sweep::loft_rails::{
    arc_nurbs as kernel_arc_nurbs, loft_rails as kernel_loft_rails,
    loft_rails_space as kernel_loft_rails_space, polyline, RailLoftOptions,
    RailSection as KernelSection, SpaceRailSection as KernelSpaceSection, DEFAULT_MAX_WIRES,
};
use solidcore::sweep::section_blend::{section_blend_surface, EndOrder, SectionFrame};
use solidcore::sweep::stations::RailFit as KernelRailFit;
use solidcore::topology::brep::p3::new_brep;
use solidcore::Vec3;

use super::{making, parse, Curve};
use crate::body::{finish_closed, Body, Expect};
use crate::error::status;
use crate::{Error, Result};
pub use solidcore::sweep::loft_rails::Tolerance;

/// How guide rails size a station (the kernel's [`KernelRailFit`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RailFit {
    /// Evenly, by the distance across to the rail.
    #[default]
    Scale,
    /// Only across to the rail, the other way kept.
    Stretch,
}

/// A section of a railed loft: a point of its plane, the plane's normal and its curves in order.
#[derive(Clone, Debug, PartialEq)]
pub struct RailSection {
    /// A point of the section's plane.
    pub origin: [f64; 3],
    /// The plane's normal.
    pub normal: [f64; 3],
    /// The section's curves, in order; each section's k-th curve pairs with the others' (the
    /// same degree, knots and weights).
    pub curves: Vec<Curve>,
}

/// A section of a railed loft: optionally in a plane (`Some((origin, normal))`) or a 3D curve
/// chain in space (`None`).
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceRailSection {
    /// A point of the section's plane and its normal, or `None` when the section is a 3D curve
    /// chain in space.
    pub plane: Option<([f64; 3], [f64; 3])>,
    /// The section's curves, in order; each section's k-th curve pairs with the others' (the
    /// same degree, knots and weights).
    pub curves: Vec<Curve>,
}

/// How a railed loft is built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailLoft {
    /// How the rails size a station.
    pub fit: RailFit,
    /// The most a guided edge may stray from its rail (finite and positive), a length or a
    /// fraction of the loft's size: the kernel certifies every guided edge within it.
    pub tolerance: Tolerance,
    /// The most wires (sections and stations together) the loft is skinned through: a
    /// resource bound, the kernel's default [`DEFAULT_MAX_WIRES`].
    pub max_wires: usize,
    /// Whether the loft is capped into a solid (the sections closed).
    pub cap: bool,
}

impl RailLoft {
    /// A capped loft scaled by its rails, certified within `tolerance`, through at most the
    /// kernel's default wires.
    #[must_use]
    pub const fn new(tolerance: f64) -> Self {
        Self {
            fit: RailFit::Scale,
            tolerance: Tolerance::Absolute(tolerance),
            max_wires: DEFAULT_MAX_WIRES,
            cap: true,
        }
    }

    /// A capped loft scaled by its rails, certified within `fraction` of the loft's size.
    #[must_use]
    pub const fn relative(fraction: f64) -> Self {
        Self {
            tolerance: Tolerance::Relative(fraction),
            ..Self::new(0.0)
        }
    }
}

/// The loft through `sections` guided by one to sixteen `rails`, skinned cubic, each guided edge
/// certified within `how.tolerance` of its rail ([`solidcore::sweep::loft_rails`]).
///
/// # Errors
/// [`Error::Argument`] for a curve list refused or a count out of range;
/// [`Error::RailPastTolerance`] where the kernel's certificate is not met within
/// `how.max_wires`; the kernel's other refusals ([`Error::Refused`] in its words for input it
/// refuses, else by their status); [`Error::Empty`],
/// [`Error::NotSolid`].
pub fn loft_rails(sections: &[RailSection], rails: &[Curve], how: &RailLoft) -> Result<Body> {
    if !(2..=solidcore::sweep::loft_rails::MAX_SECTIONS).contains(&sections.len())
        || !(1..=solidcore::sweep::loft_rails::MAX_RAILS).contains(&rails.len())
    {
        return Err(Error::Argument);
    }
    let all: Vec<Curve> = sections
        .iter()
        .flat_map(|s| s.curves.iter().cloned())
        .chain(rails.iter().cloned())
        .collect();
    making("loft_rails", |k| {
        let made = match parse(k, &all) {
            Ok(m) => m,
            Err(e) => return Ok(Err(e)),
        };
        let mut at = 0usize;
        let mut kernel_sections = Vec::with_capacity(sections.len());
        for s in sections {
            let n = s.curves.len();
            let Some(curves) = made.curves.get(at..at + n) else {
                return Ok(Err(Error::Argument));
            };
            at += n;
            kernel_sections.push(KernelSection {
                origin: Vec3::new(s.origin[0], s.origin[1], s.origin[2]),
                normal: Vec3::new(s.normal[0], s.normal[1], s.normal[2]),
                curves: curves.iter().collect(),
            });
        }
        let Some(rail_curves) = made.curves.get(at..) else {
            return Ok(Err(Error::Argument));
        };
        let rail_refs: Vec<_> = rail_curves.iter().collect();
        let opts = RailLoftOptions {
            fit: match how.fit {
                RailFit::Scale => KernelRailFit::Scale,
                RailFit::Stretch => KernelRailFit::Stretch,
            },
            closed: how.cap,
            degree: 3,
            cap: how.cap,
            tolerance: how.tolerance,
            max_wires: how.max_wires,
        };
        match kernel_loft_rails(k, &kernel_sections, &rail_refs, &opts) {
            Ok(made) => finish_closed(
                k,
                made.skin.brep,
                if how.cap {
                    Expect::SOLID
                } else {
                    Expect::FACES
                },
            ),
            Err(e) => Ok(Err(refused(e))),
        }
    })
}

/// [`loft_rails`] through sections that may be planar or 3D curve chains in space
/// ([`SpaceRailSection`]).
///
/// # Errors
/// As [`loft_rails`].
pub fn loft_rails_space(
    sections: &[SpaceRailSection],
    rails: &[Curve],
    how: &RailLoft,
) -> Result<Body> {
    if !(2..=solidcore::sweep::loft_rails::MAX_SECTIONS).contains(&sections.len())
        || !(1..=solidcore::sweep::loft_rails::MAX_RAILS).contains(&rails.len())
    {
        return Err(Error::Argument);
    }
    let all: Vec<Curve> = sections
        .iter()
        .flat_map(|s| s.curves.iter().cloned())
        .chain(rails.iter().cloned())
        .collect();
    making("loft_rails", |k| {
        let made = match parse(k, &all) {
            Ok(m) => m,
            Err(e) => return Ok(Err(e)),
        };
        let mut at = 0usize;
        let mut kernel_sections = Vec::with_capacity(sections.len());
        for s in sections {
            let n = s.curves.len();
            let Some(curves) = made.curves.get(at..at + n) else {
                return Ok(Err(Error::Argument));
            };
            at += n;
            kernel_sections.push(KernelSpaceSection {
                plane: s
                    .plane
                    .map(|(o, n)| (Vec3::new(o[0], o[1], o[2]), Vec3::new(n[0], n[1], n[2]))),
                curves: curves.iter().collect(),
            });
        }
        let Some(rail_curves) = made.curves.get(at..) else {
            return Ok(Err(Error::Argument));
        };
        let rail_refs: Vec<_> = rail_curves.iter().collect();
        let opts = RailLoftOptions {
            fit: match how.fit {
                RailFit::Scale => KernelRailFit::Scale,
                RailFit::Stretch => KernelRailFit::Stretch,
            },
            closed: how.cap,
            degree: 3,
            cap: how.cap,
            tolerance: how.tolerance,
            max_wires: how.max_wires,
        };
        match kernel_loft_rails_space(k, &kernel_sections, &rail_refs, &opts) {
            Ok(made) => finish_closed(
                k,
                made.skin.brep,
                if how.cap {
                    Expect::SOLID
                } else {
                    Expect::FACES
                },
            ),
            Err(e) => Ok(Err(refused(e))),
        }
    })
}

/// A guide rail through `points` in order: solidcore's [`polyline`] (a degree-1 curve through
/// them, its knots spaced as the points are in order), as ops' [`Curve::Nurbs`] with unit weights.
///
/// # Errors
/// [`Error::Argument`] for fewer than two points or one not finite (solidcore's refusal).
pub fn polyline_rail(points: &[[f64; 3]]) -> Result<Curve> {
    let v: Vec<Vec3> = points.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    let rail = polyline(&v).map_err(refused)?;
    let base = rail.b_spline_base().ok_or(Error::Argument)?;
    let degree = base.degree().map_err(refused)?;
    let (unique, mult) = base.knots(None).map_err(refused)?;
    let knots: Vec<f64> = unique
        .iter()
        .zip(&mult)
        .flat_map(|(u, m)| std::iter::repeat_n(*u, *m))
        .collect();
    let (cp, weights) = base.control_polygon().map_err(refused)?;
    let points = cp
        .iter()
        .enumerate()
        .map(|(i, q)| [q.x, q.y, q.z, weights.get(i).copied().unwrap_or(1.0)])
        .collect();
    Ok(Curve::Nurbs {
        degree,
        points,
        knots,
    })
}

/// A 3D circular arc from `start` about `center` and `axis` turning by `sweep` radians as a
/// clamped rational quadratic NURBS curve on `[0, 1]`: `(knots, ctrl)` where each control point
/// is homogeneous `[x * w, y * w, z * w, w]` ([`solidcore::sweep::loft_rails::arc_nurbs`]).
///
/// # Errors
/// [`Error::Argument`] for a non-finite coordinate or sweep; the kernel's refusal otherwise.
pub fn arc_nurbs(
    start: [f64; 3],
    center: [f64; 3],
    axis: [f64; 3],
    sweep: f64,
) -> Result<(Vec<f64>, Vec<[f64; 4]>)> {
    if ![start, center, axis]
        .iter()
        .flatten()
        .all(|x| x.is_finite())
        || !sweep.is_finite()
    {
        return Err(Error::Argument);
    }
    let v = |[x, y, z]: [f64; 3]| Vec3::new(x, y, z);
    kernel_arc_nurbs(v(start), v(center), v(axis), sweep).map_err(refused)
}

/// Closest normalised parameter on segment `[a0, a1]` to segment `[b0, b1]` and the distance
/// between them ([`solidcore::geom::geom_utility::p2::segment_segment_closest_point`]).
///
/// # Errors
/// [`Error::Argument`] for a non-finite coordinate; the kernel's refusal otherwise.
pub fn segment_segment_closest(
    a0: [f64; 3],
    a1: [f64; 3],
    b0: [f64; 3],
    b1: [f64; 3],
) -> Result<(f64, f64)> {
    if ![a0, a1, b0, b1].iter().flatten().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    let v = |[x, y, z]: [f64; 3]| Vec3::new(x, y, z);
    let hits = segment_segment_closest_point(v(a0), v(a1), v(b0), v(b1), 0.0).map_err(status)?;
    if hits.count == 0 {
        return Err(Error::Argument);
    }
    let [s, _] = hits.params1;
    let [d, _] = hits.distances;
    Ok((s, d))
}

/// The continuity a blend holds at an end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Continuity {
    /// Position.
    #[default]
    G0,
    /// And tangent direction.
    G1,
    /// And curvature.
    G2,
    /// And change of curvature.
    G3,
}

/// One station of a blend's cross-section: its point and, at the first and last section, how
/// the blend leaves it ([`solidcore::sweep::section_blend::SectionFrame`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlendFrame {
    /// The point.
    pub p: [f64; 3],
    /// The direction the blend leaves in (at the last section, back into the blend).
    pub leave: [f64; 3],
    /// The curvature vector along `leave`.
    pub bend: [f64; 3],
    /// The arc-length derivative of `bend` along `leave`.
    pub bend_change: [f64; 3],
}

/// A sheet of one face: the blend surface through `sections` (each a cross-section of the same
/// stations, in order), holding `orders[0]` at the first and `orders[1]` at the last, `crown`
/// how fast it leaves each ([`solidcore::sweep::section_blend`]). The face keeps its
/// definition.
///
/// # Errors
/// [`Error::Argument`] for a frame not finite; the kernel's refusals ([`Error::Refused`] in its
/// words for input it refuses, else by their status);
/// [`Error::Empty`].
pub fn loft_blend(
    sections: &[Vec<BlendFrame>],
    orders: [Continuity; 2],
    crown: [f64; 2],
) -> Result<Body> {
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    let frames: Vec<Vec<SectionFrame>> = sections
        .iter()
        .map(|s| {
            s.iter()
                .map(|f| SectionFrame {
                    p: v(f.p),
                    leave: v(f.leave),
                    bend: v(f.bend),
                    bend_change: v(f.bend_change),
                })
                .collect()
        })
        .collect();
    let order = |c: Continuity| match c {
        Continuity::G0 => EndOrder::G0,
        Continuity::G1 => EndOrder::G1,
        Continuity::G2 => EndOrder::G2,
        Continuity::G3 => EndOrder::G3,
    };
    making("loft_blend", |k| {
        let surface =
            match section_blend_surface(frames, [order(orders[0]), order(orders[1])], crown) {
                Ok(s) => s,
                Err(e) => return Ok(Err(status(e))),
            };
        let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        if let Err(e) =
            solidcore::blend::fillet_executive::api::create_face_from_surface(k, &mut b, &surface)
        {
            return Ok(Err(status(e)));
        }
        b.refresh_cached_topology();
        finish_closed(k, b, Expect::FACES)
    })
}

/// The kernel's refusal of a railed loft as ops' error.
pub(super) fn refused(e: solidcore::SolidError) -> Error {
    use solidcore::error::sweep::SweepFailure;
    use solidcore::error::Reason;
    if let solidcore::SolidError::Failed(f) = &e {
        if let Reason::Sweep(SweepFailure::RailPastStationCap {
            asked,
            reached,
            wires,
        }) = f.reason
        {
            return Error::RailPastTolerance {
                asked,
                reached,
                wires,
            };
        }
    }
    status(e)
}

/// The frames of a cross-section that is the edge chain `edges` (indices into the body's edge
/// list, in order, each sharing a vertex with the next) of `body`, at `stations` stations of
/// even arc length, leaving along `leave` ([`solidcore::sweep::section_blend::section_frames`]):
/// a section for [`loft_blend`]. `min_in_plane` is the least share of `leave` the support
/// face's tangent plane keeps (SolidCraft's is 0.2).
///
/// # Errors
/// [`Error::Argument`] for an edge index not in the body; the kernel's refusals
/// ([`Error::Refused`] in its words for input it refuses, else by their status).
pub fn section_frames(
    body: &Body,
    edges: &[usize],
    leave: [f64; 3],
    stations: usize,
    min_in_plane: f64,
) -> Result<Vec<BlendFrame>> {
    use solidcore::sweep::section_blend::{section_frames as kernel_frames, FrameOptions};
    let arr = |v: Vec3| [v.x, v.y, v.z];
    crate::session::read_body("section_frames", body, |k, b| {
        let list = b.edge_list();
        let mut chain = Vec::with_capacity(edges.len());
        for &i in edges {
            let Some(&e) = list.get(i) else {
                return Ok(Err(Error::Argument));
            };
            chain.push(e);
        }
        let opts = FrameOptions {
            stations,
            min_in_plane,
        };
        Ok(
            match kernel_frames(k, b, &chain, Vec3::new(leave[0], leave[1], leave[2]), &opts) {
                Ok(frames) => Ok(frames
                    .into_iter()
                    .map(|f| BlendFrame {
                        p: arr(f.p),
                        leave: arr(f.leave),
                        bend: arr(f.bend),
                        bend_change: arr(f.bend_change),
                    })
                    .collect()),
                Err(e) => Err(status(e)),
            },
        )
    })?
}

/// The frames of a cross-section given by its points in order (the polyline through them), at
/// `stations` stations of even arc length, leaving along `leave` with no bend
/// ([`solidcore::sweep::section_blend::point_frames`]): a section for [`loft_blend`] with no
/// face behind it, such as a sketch's chain.
///
/// # Errors
/// The kernel's refusals ([`Error::Refused`] in its words: stations out of range, a point or
/// `leave` not finite, a chain of no length).
pub fn point_frames(
    points: &[[f64; 3]],
    leave: [f64; 3],
    stations: usize,
) -> Result<Vec<BlendFrame>> {
    use solidcore::sweep::section_blend::point_frames as kernel_frames;
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    let arr = |v: Vec3| [v.x, v.y, v.z];
    let points: Vec<Vec3> = points.iter().map(|&p| v(p)).collect();
    match kernel_frames(&points, v(leave), stations) {
        Ok(frames) => Ok(frames
            .into_iter()
            .map(|f| BlendFrame {
                p: arr(f.p),
                leave: arr(f.leave),
                bend: arr(f.bend),
                bend_change: arr(f.bend_change),
            })
            .collect()),
        Err(e) => Err(status(e)),
    }
}

/// Which open cross-sections of a blend to turn round so that each runs the way the one before
/// it does, given each one's first and last point in the blend's order
/// ([`solidcore::sweep::section_blend::open_section_turns`]): `true` for a section whose frames
/// the caller reverses before [`loft_blend`].
///
/// # Errors
/// [`Error::Argument`] for an end not finite; [`Error::Unsupported`] for a closed section (its
/// ends within the kernel's zone tolerance), whose winding and seam a turn does not match.
pub fn open_section_turns(ends: &[[[f64; 3]; 2]]) -> Result<Vec<bool>> {
    use solidcore::sweep::section_blend::open_section_turns as kernel_turns;
    if !ends.iter().flatten().flatten().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    let ends: Vec<[Vec3; 2]> = ends.iter().map(|&[s, e]| [v(s), v(e)]).collect();
    crate::session::run("open_section_turns", |k| {
        Ok(kernel_turns(k, &ends).map_err(status))
    })?
}
