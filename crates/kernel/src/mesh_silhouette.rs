//! Reading a mesh as a run of silhouettes to loft a solid through.
//!
//! [`slab_plan`](crate::slab_plan) reads a mesh as the stack of layers a 2.5D part was modelled
//! from, and says so honestly when the shape is nothing of the kind. A scanned handle, a cast
//! housing or a generated shape is exactly that case: it has no levels to step between and no
//! outline that holds still across a layer, so no amount of fitting lines and arcs will describe
//! it.
//!
//! This reads the same mesh the other way. It cuts the shape at a run of heights along one
//! direction, takes the outline the cut leaves at each one, and hands back a run of closed
//! profiles for the caller to loft a solid through. Nothing is recognized and nothing is
//! dimensioned: what comes out is the shape's own silhouette, resampled to a size a loft can
//! handle, and the solid built from it is boolean-able, measurable and printable where the mesh
//! itself was none of those things.
//!
//! Two things make this work on scans, where the obvious version does not.
//!
//! The first is what to do when a cut does not close. A scan has holes in it and loose debris
//! around it, so a section through it often comes back as a handful of broken arcs rather than
//! one loop. Where the chaining gives a single outline that is used as it stands, concavities and
//! all; where it does not, the convex hull of every crossing point at that height stands in. The
//! hull always closes, and a section that fell back to one says so, because a hull quietly loses
//! every concavity at that height and the caller should know which heights those were.
//!
//! The second is where each profile starts. Lofting through profiles whose first points do not
//! line up puts a twist in the surface between them, and taking the point furthest along one axis
//! does not line up: which vertex happens to be furthest flips between neighbouring sections, and
//! the surface kinks wherever it does. Each profile instead starts at the point nearest a fixed
//! direction from its own centre, which moves smoothly from one section to the next.
//!
//! Reading one direction loses every concavity the hull fell back on, and every concavity that
//! faces along the cut: a waist is caught by a cut across it and invisible to a cut along it.
//! Reading several directions and intersecting the solids catches what any one of them misses,
//! which is why the options take a list of axes rather than one.

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::body::Body;
use crate::mesh_slabs::{Facets, distance, section, signed_area, spread};
use crate::meshbody::MAX_TRIANGLES;
use crate::{KernelError, Result};

/// Directions one call may read the shape along. More than three buys nothing: three square
/// directions already catch every concavity a silhouette can show.
const MAX_AXES: usize = 3;
/// Sections one direction may be cut into. `solid.loft` takes no more.
pub const MAX_SECTIONS: usize = 50;
/// Points one section's profile may carry. `sketch.spline.fit_point` takes no more.
pub const MAX_POINTS: usize = 500;
/// Points one section's profile needs at least, for a spline that is a shape and not a triangle.
pub const MIN_POINTS: usize = 12;

fn invalid(message: &str) -> KernelError {
    KernelError::Invalid(format!("mesh silhouette: {message}"))
}

/// How to read a shape as silhouettes. Lengths are mm.
#[derive(Debug, Clone, PartialEq)]
pub struct SilhouetteOptions {
    /// The directions to cut across, each read on its own. One gives a single loft; several give
    /// one loft each, for the caller to intersect.
    pub axes: Vec<Vec3>,
    /// How far apart the cuts sit along the axis. Without one, `count` cuts are spread evenly,
    /// which keeps a short part and a long one equally well described.
    pub spacing: Option<f64>,
    /// How many cuts to take when `spacing` is left out.
    pub count: usize,
    /// How many points each profile is resampled to. Every profile gets the same number, because
    /// a loft between profiles of different sizes has to guess how they correspond.
    pub points: usize,
    /// How far in from each tip of the shape to start cutting. A cut at the very tip catches a
    /// sliver of noise or nothing at all, and either way says nothing about the shape.
    pub margin: f64,
    /// Take the convex hull at every height, whether or not the outline closes. Slower to lose
    /// detail but impossible to trip up, which is what a badly broken scan wants.
    pub hull: bool,
}

impl Default for SilhouetteOptions {
    fn default() -> SilhouetteOptions {
        SilhouetteOptions { axes: vec![Vec3::Z], spacing: None, count: 16, points: 60, margin: 1.5, hull: false }
    }
}

/// One cut through the shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// Where the cut sits along the axis, measured from the world origin.
    pub at: f64,
    /// The closed profile, in the sketch plane's own coordinates, turning counter-clockwise, every
    /// section starting at the same bearing from its own centre.
    pub points: Vec<[f64; 2]>,
    /// The area the profile encloses (mm^2).
    pub area: f64,
    /// Whether the convex hull stood in because the cut would not close into one outline. Every
    /// concavity at this height is lost.
    pub hull: bool,
}

/// The silhouettes of one shape read along one direction.
#[derive(Debug, Clone, PartialEq)]
pub struct Silhouettes {
    /// The direction the cuts are square to.
    pub axis: Vec3,
    /// The sketch plane's own axes, right-handed about `axis`.
    pub x_dir: Vec3,
    pub y_dir: Vec3,
    /// The cuts, lowest first.
    pub sections: Vec<Section>,
    /// Where the shape starts and ends along the axis (mm).
    pub reach: [f64; 2],
    /// How many sections fell back to the convex hull.
    pub hulled: usize,
}

/// What a whole reading found.
#[derive(Debug, Clone, PartialEq)]
pub struct Silhouetted {
    /// One reading per axis asked for, in the order they were asked for.
    pub along: Vec<Silhouettes>,
    /// Triangles the shape was read from, after welding.
    pub triangles: usize,
    /// The mesh's area (mm^2).
    pub area: f64,
}

/// Read a mesh body as the silhouettes a solid can be lofted through.
///
/// Fails rather than guesses: a direction the shape barely reaches along, or one whose cuts leave
/// fewer than two usable profiles, is refused with what was missing, because a loft through one
/// profile is not a shape.
pub fn silhouettes(body: &Body, options: &SilhouetteOptions) -> Result<Silhouetted> {
    crate::guard("read a mesh as silhouettes", || {
        let o = checked(options)?;
        let input = body.mesh.as_ref().ok_or_else(|| invalid("select a mesh body"))?;
        let welded = crate::mesh_convert::weld(input, 0.01, MAX_TRIANGLES)?;
        let mesh = welded.mesh.as_ref().ok_or_else(|| invalid("empty mesh"))?;
        let facets = Facets::read(mesh)?;
        if facets.count() < 20 {
            return Err(invalid("too few triangles to read a shape from"));
        }
        let mut along = Vec::new();
        for axis in &o.axes {
            along.push(read_along(mesh, *axis, &o)?);
        }
        Ok(Silhouetted { along, triangles: facets.count(), area: facets.total })
    })
}

/// The options, with every number checked, so nothing below has to wonder whether a count is zero
/// or an axis is the zero vector.
fn checked(o: &SilhouetteOptions) -> Result<SilhouetteOptions> {
    if o.axes.is_empty() || o.axes.len() > MAX_AXES {
        return Err(invalid(&format!("give between 1 and {MAX_AXES} axes to read along")));
    }
    let mut axes = Vec::with_capacity(o.axes.len());
    for axis in &o.axes {
        axes.push(axis.normalized().ok_or_else(|| invalid("an axis must not be zero length"))?);
    }
    if !(2..=MAX_SECTIONS).contains(&o.count) {
        return Err(invalid(&format!("count must be between 2 and {MAX_SECTIONS} sections")));
    }
    if !(MIN_POINTS..=MAX_POINTS).contains(&o.points) {
        return Err(invalid(&format!("points must be between {MIN_POINTS} and {MAX_POINTS} per section")));
    }
    if !o.margin.is_finite() || !(0. ..=1000.).contains(&o.margin) {
        return Err(invalid("margin must be between 0 and 1000 mm"));
    }
    if let Some(spacing) = o.spacing
        && (!spacing.is_finite() || !(0.01..=1000.).contains(&spacing))
    {
        return Err(invalid("spacing must be between 0.01 and 1000 mm"));
    }
    Ok(SilhouetteOptions { axes, ..o.clone() })
}

/// The sketch plane's own axes for a direction. The world axes are used where the direction is one
/// of them, so a shape read along Z gives profiles in ordinary X and Y and the numbers that come
/// back are the ones someone would expect.
fn plane_axes(axis: Vec3) -> Result<(Vec3, Vec3)> {
    let square = 1. - 1e-9;
    for (w, u) in [(Vec3::Z, Vec3::X), (Vec3::X, Vec3::Y), (Vec3::Y, Vec3::Z)] {
        if axis.dot(w) >= square {
            return Ok((u, w.cross(u)));
        }
    }
    let helper = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = axis.cross(helper).normalized().ok_or_else(|| invalid("the axis is degenerate"))?;
    Ok((u, axis.cross(u)))
}

fn read_along(mesh: &crate::meshbody::TriMesh, axis: Vec3, o: &SilhouetteOptions) -> Result<Silhouettes> {
    let (x_dir, y_dir) = plane_axes(axis)?;
    let local: Vec<[f64; 3]> = mesh.positions.iter().map(|p| [p.dot(x_dir), p.dot(y_dir), p.dot(axis)]).collect();
    let (lo, hi) = local.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
    if !(hi > lo) {
        return Err(invalid("the shape is flat along that direction, so there is nothing to cut across"));
    }
    // Trim the tips, but never so far that nothing is left: a thin part with a generous margin
    // should still be read, not refused on a number the caller never thought about.
    let margin = o.margin.min(0.4 * (hi - lo));
    let (from, to) = (lo + margin, hi - margin);
    if !(to - from > 1e-6) {
        return Err(invalid("the shape barely reaches along that direction; give another axis"));
    }
    let count = match o.spacing {
        Some(spacing) => (((to - from) / spacing).round() as usize + 1).clamp(2, MAX_SECTIONS),
        None => o.count,
    };
    let least = 1e-6 * (to - from) * (to - from);
    let mut sections = Vec::with_capacity(count);
    let mut hulled = 0;
    for step in 0..count {
        let at = from + (to - from) * (step as f64) / ((count - 1).max(1) as f64);
        let (loops, crossings) = section(&local, &mesh.triangles, at, least);
        let Some(section) = profile(&loops, &crossings, at, o) else { continue };
        if section.hull {
            hulled += 1;
        }
        sections.push(section);
    }
    if sections.len() < 2 {
        return Err(invalid(&format!(
            "only {} of {count} cuts along that direction gave an outline; try another axis, a smaller margin, or hull",
            sections.len()
        )));
    }
    Ok(Silhouettes { axis, x_dir, y_dir, sections, reach: [lo, hi], hulled })
}

/// The one closed profile a cut stands for.
///
/// A cut that leaves exactly one outline is that outline, concavities and all. A cut that leaves
/// several — two lobes of a fork, or a scan broken into arcs — has no single outline to give, and
/// a loft needs one profile per section, so the convex hull around everything the cut touched
/// stands in.
fn profile(loops: &[Vec<[f64; 2]>], crossings: &[[f64; 2]], at: f64, o: &SilhouetteOptions) -> Option<Section> {
    let outer: Vec<&Vec<[f64; 2]>> = loops.iter().filter(|outline| signed_area(outline) > 0.).collect();
    let (points, hull) = match outer.as_slice() {
        [only] if !o.hull => ((*only).clone(), false),
        [] if crossings.len() < 3 => return None,
        [] => (convex_hull(crossings), true),
        several => (convex_hull(&several.iter().flat_map(|outline| outline.iter().copied()).collect::<Vec<_>>()), true),
    };
    if points.len() < 3 {
        return None;
    }
    // Every profile turns the same way and starts at the same bearing, so the loft between two of
    // them runs point to point instead of winding round.
    let turned = if signed_area(&points) < 0. { points.into_iter().rev().collect() } else { points };
    let sampled = resample_to(&turned, o.points)?;
    let aligned = start_at_bearing(sampled);
    let area = signed_area(&aligned);
    (area.abs() > 0.).then_some(Section { at, points: aligned, area, hull })
}

/// Andrew's monotone chain: the smallest convex outline around a set of points, counter-clockwise.
fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    sorted.dedup_by(|a, b| distance(*a, *b) < 1e-9);
    if sorted.len() < 3 {
        return sorted;
    }
    let turn = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let half = |points: &mut dyn Iterator<Item = [f64; 2]>| {
        let mut side: Vec<[f64; 2]> = Vec::new();
        for p in points {
            while side.len() >= 2 && matches!((side.get(side.len() - 2), side.last()), (Some(&a), Some(&b)) if turn(a, b, p) <= 0.) {
                side.pop();
            }
            side.push(p);
        }
        side.pop();
        side
    };
    let mut out = half(&mut sorted.iter().copied());
    out.extend(half(&mut sorted.iter().rev().copied()));
    out
}

/// Resample a closed outline to exactly `count` points, evenly spaced along it.
///
/// Every profile getting the same count is what lets a loft pair them up: a section that happened
/// to be cut through a finely tessellated part of the mesh carries no more points than one cut
/// through a coarse part.
fn resample_to(points: &[[f64; 2]], count: usize) -> Option<Vec<[f64; 2]>> {
    let mut lengths = vec![0.];
    let mut total = 0.;
    for i in 0..points.len() {
        let (Some(p), Some(q)) = (points.get(i), points.get((i + 1) % points.len())) else { continue };
        total += distance(*p, *q);
        lengths.push(total);
    }
    if !(total > 0.) {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    let mut at = 0usize;
    for i in 0..count {
        let target = total * (i as f64) / (count as f64);
        while lengths.get(at + 1).is_some_and(|next| *next < target) {
            at += 1;
        }
        let (Some(&from), Some(&to)) = (lengths.get(at), lengths.get(at + 1)) else { break };
        let (Some(p), Some(q)) = (points.get(at), points.get((at + 1) % points.len())) else { break };
        let f = if to - from > 1e-12 { (target - from) / (to - from) } else { 0. };
        out.push([p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f]);
    }
    (out.len() == count).then_some(out)
}

/// Turn a closed profile so it starts at the point nearest the +x bearing from its own centre.
///
/// Starting at the point furthest along x, which is the obvious choice, is what puts a twist in a
/// loft: different sections have different vertices at the furthest spot, and the chosen one
/// flips between neighbours, so the lofted surface kinks sharply wherever it does. The nearest
/// point to a fixed bearing moves smoothly from one section to the next.
fn start_at_bearing(points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let n = points.len();
    if n == 0 {
        return points;
    }
    let cx = points.iter().map(|p| p[0]).sum::<f64>() / n as f64;
    let cy = points.iter().map(|p| p[1]).sum::<f64>() / n as f64;
    let mut best = (f64::INFINITY, 0usize);
    for (i, p) in points.iter().enumerate() {
        let off = (p[1] - cy).atan2(p[0] - cx).abs();
        if off < best.0 {
            best = (off, i);
        }
    }
    let mut out: Vec<[f64; 2]> = points.get(best.1..).unwrap_or(&[]).to_vec();
    out.extend_from_slice(points.get(..best.1).unwrap_or(&[]));
    out
}

/// How far the shape reaches across the plane of one reading, along the sketch's own axes (mm):
/// what the lofted solid will measure.
pub fn across(found: &Silhouettes) -> [f64; 2] {
    let points: Vec<[f64; 2]> = found.sections.iter().flat_map(|s| s.points.iter().copied()).collect();
    [spread(points.iter().map(|p| p[0])), spread(points.iter().map(|p| p[1]))]
}

#[cfg(test)]
#[path = "mesh_silhouette_tests.rs"]
mod tests;
