//! Surfacing (overlay, hardfacing) welds: layers laid on faces of a part the user picked
//! ([`surfacing`]), Q37: whole faces or a region of them, a layer thickness, any number of layers,
//! the layer's edges square, run out at 45° or rounded (Q195: rounded by default), and optionally
//! the top laid as bead rows (Q197).
//!
//! SolidCraft's own, as [`crate::weld_spot`] is: every measurement and every body is a kernel call;
//! this module adds the weld's rules and sizes. The `weld.surfacing` command and its storage come
//! with #15131.
//!
//! # The geometry
//!
//! The picked faces are copied out of the part as a sheet and thickened away from the material
//! (`copy_faces`, `thicken`): layer `k` is the part's faces thickened by `k t` less by `(k - 1) t`.
//! A region (a closed sketch on the face, or a loop of picked curves, Q196) is extruded square
//! through the layers and met with them. The outermost layer's edges away from the part are
//! chamfered (`t`, a 45° run-out) or rounded (radius `t`); where the kernel refuses that, they stay
//! square with a notice. Bead rows replace the outermost layer by circular segments `t` high and
//! `width` wide, one every `width (1 - overlap)` across the face, along the face's longest edge or
//! a direction given, met with the smooth layer's room. Display only: the part is never changed.
//!
//! # Not covered
//!
//! Bead rows on flat faces only.

use solidcore_record::{Plane, Region2, Vec3};
use solidcraft_kernel::curves as curve;
use solidcraft_kernel::profiles::{self, ProfileCurve as Curve};
use solidcraft_kernel::{Body, BoolOp, KernelError, affine};

use crate::weld_seam::{add, scale, sub};
use crate::weld_spot::positive;

/// The layer thickness a new surfacing weld opens with when nothing was used before (Q172).
pub const FIRST_THICKNESS: f64 = 3.0;
/// The bead width bead rows open with when nothing was used before (Q197).
pub const FIRST_ROW_WIDTH: f64 = 10.0;
/// The overlap of neighbouring bead rows they open with when nothing was used before (Q197).
pub const FIRST_ROW_OVERLAP: f64 = 0.5;

/// Why a surfacing weld is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SurfacingRefusal {
    /// Input the weld cannot take: a size, point or count out of range, or sizes the model cannot
    /// hold.
    #[error("{0}")]
    Invalid(&'static str),
    /// The layers would close up on themselves (a bore narrower than they are thick). `fits` is the
    /// thickest that built, for all the layers together.
    #[error("layers this thick close up on the picked faces; at most {fits} in all fits")]
    TooThick { fits: f64 },
    /// The region meets none of the picked faces.
    #[error("the region lies off the picked faces")]
    RegionOff,
    /// Bead rows asked on a face that is not flat.
    #[error("bead rows on a face that is not flat are not built yet: use a smooth layer there")]
    RowsNeedFlat,
    /// The kernel refused a profile's body, in its words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A curve call the kernel refused, in its words.
    #[error(transparent)]
    Curve(#[from] curve::CurveRefused),
    /// A kernel call on a body failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
}

type Result<T> = std::result::Result<T, SurfacingRefusal>;

fn bad<T>(why: &'static str) -> Result<T> {
    Err(SurfacingRefusal::Invalid(why))
}

/// How the layer ends at its edges away from the part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerEdges {
    /// Square to the face.
    Square,
    /// A 45° run-out down to the face.
    RunOut,
    /// Rounded, radius the layer's thickness (the default, Q195).
    Rounded,
}

/// The top laid as bead rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rows {
    /// Each bead's width.
    pub width: f64,
    /// How much of a bead the next one covers, from 0 up to below 1.
    pub overlap: f64,
    /// The rows' direction; `None` runs them along the face's longest edge (Q197).
    pub along: Option<[f64; 3]>,
}

/// One surfacing weld's style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacingStyle {
    /// Each layer's thickness.
    pub thickness: f64,
    /// How many layers, at least one.
    pub layers: usize,
    pub edges: LayerEdges,
    pub rows: Option<Rows>,
}

/// Where a region of the picked faces lies: closed regions on a plane, seen square to it.
#[derive(Debug, Clone, Copy)]
pub struct SurfacingRegion<'a> {
    pub plane: &'a Plane,
    pub regions: &'a [Region2],
}

/// Something about a surfacing weld the user should know, though it builds.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfacingNotice {
    /// The edges asked for were refused by the kernel, in its words; they stay square.
    EdgesSquare { why: String },
}

/// A surfacing weld's layers, innermost first, and what the user should know about them.
#[derive(Debug)]
pub struct Surfacing {
    pub layers: Vec<Body>,
    pub notices: Vec<SurfacingNotice>,
}

/// The surfacing weld with `style` on the faces of `part` at `points` (one point on each picked
/// face), cut to `region` where one is given. The part is read, never changed.
///
/// # Errors
/// [`SurfacingRefusal::Invalid`] for no points, a thickness, count, width or overlap out of range;
/// [`SurfacingRefusal::TooThick`] where the layers close up on themselves;
/// [`SurfacingRefusal::RegionOff`]; [`SurfacingRefusal::RowsNeedFlat`]. The errors of the kernel
/// calls otherwise.
pub fn surfacing(part: &Body, points: &[[f64; 3]], region: Option<SurfacingRegion>, style: &SurfacingStyle) -> Result<Surfacing> {
    let t = style.thickness;
    if points.is_empty() || !points.iter().flatten().all(|v| v.is_finite()) {
        return bad("no picked face, or a point that is not finite");
    }
    if !positive(t) || style.layers == 0 || style.layers > 100 {
        return bad("a layer thickness that is not finite and positive, or a layer count not from 1 to 100");
    }
    if let Some(r) = style.rows
        && !(positive(r.width) && (0.0..1.0).contains(&r.overlap))
    {
        return bad("a bead width that is not finite and positive, or an overlap not from 0 up to below 1");
    }
    let at: Vec<Vec3> = points.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    let sheet = solidcraft_kernel::copy_faces(part, &at)?;
    let total = t * style.layers as f64;
    let thick = |d: f64| -> Result<Body> {
        match solidcraft_kernel::thicken(&sheet, d, false) {
            Ok(b) => Ok(b),
            Err(e) => Err(match largest_that_builds(&sheet, d) {
                Some(fits) => SurfacingRefusal::TooThick { fits },
                None => e.into(),
            }),
        }
    };
    let outer = thick(total)?;
    let clip = match region {
        Some(r) => Some(region_prism(r, &outer)?),
        None => None,
    };
    let cut = |b: Body| -> Result<Body> {
        match &clip {
            Some(c) => solidcraft_kernel::boolean(&b, c, BoolOp::Intersect)?.ok_or(SurfacingRefusal::RegionOff),
            None => Ok(b),
        }
    };

    let mut notices = Vec::new();
    let mut layers = Vec::new();
    let mut below: Option<Body> = None;
    for k in 1..=style.layers {
        let reach = if k == style.layers { outer.clone() } else { thick(t * k as f64)? };
        let reach = cut(reach)?;
        let top = k == style.layers;
        let reach = match (top, style.rows) {
            (true, Some(rows)) => cut(bead_rows(part, points[0], &reach, t * (k - 1) as f64, t, rows)?)?,
            (true, None) if style.edges != LayerEdges::Square => match edged(part, &reach, t, style.edges) {
                Ok(b) => b,
                Err(why) => {
                    notices.push(SurfacingNotice::EdgesSquare { why });
                    reach
                }
            },
            _ => reach,
        };
        let layer = match &below {
            Some(b) => solidcraft_kernel::boolean(&reach, b, BoolOp::Cut)?.ok_or(SurfacingRefusal::Invalid("a surfacing layer left empty"))?,
            None => reach.clone(),
        };
        below = Some(match below {
            Some(b) => solidcraft_kernel::union_all(&[&b, &layer])?,
            None => reach,
        });
        layers.push(layer);
    }
    Ok(Surfacing { layers, notices })
}

/// The thickest layer under `d` that `sheet` thickens to, found by halving between a built and a
/// refused thickness twelve times (to `d / 4096`); `None` where none builds. Only a thickness that
/// really built is named (Q150's rule).
fn largest_that_builds(sheet: &Body, d: f64) -> Option<f64> {
    let (mut built, mut refused) = (0.0, d);
    for _ in 0..12 {
        let mid = (built + refused) / 2.0;
        if solidcraft_kernel::thicken(sheet, mid, false).is_ok() {
            built = mid;
        } else {
            refused = mid;
        }
    }
    (built > 0.0).then_some(built)
}

/// `layer`'s edges away from `part` chamfered by `t` or rounded at radius `t`; the kernel's words
/// where it refuses.
fn edged(part: &Body, layer: &Body, t: f64, edges: LayerEdges) -> std::result::Result<Body, String> {
    let all: Vec<usize> = (0..part.face_count()).collect();
    let mut away = Vec::new();
    for e in layer.edges(t / 100.0).map_err(|e| e.to_string())? {
        // an edge away from the part: its middle a whole layer off the part
        let off = part.closest_to_point(&all, &[], [e.mid.x, e.mid.y, e.mid.z]).map_err(|e| e.to_string())?;
        if off.is_some_and(|c| c.distance > 0.99 * t) {
            away.push(e.mid);
        }
    }
    if away.is_empty() {
        return Ok(layer.clone());
    }
    let made = match edges {
        LayerEdges::Rounded => solidcraft_kernel::fillet(layer, &away, t),
        LayerEdges::RunOut => solidcraft_kernel::chamfer(layer, &away, t),
        LayerEdges::Square => Ok(layer.clone()),
    };
    made.map_err(|e| e.to_string())
}

/// The prism of `region` standing square to its plane through all of `layers`.
fn region_prism(region: SurfacingRegion, layers: &Body) -> Result<Body> {
    let b = layers.exact_bounds()?;
    let far = (b.max.x - b.min.x).abs()
        + (b.max.y - b.min.y).abs()
        + (b.max.z - b.min.z).abs()
        + [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z].iter().fold(0.0_f64, |m, c| m.max(c.abs()))
        + 1.0;
    Ok(solidcraft_kernel::extrude_planar_regions(region.plane, region.regions, -far, far)?)
}

/// The outermost layer as bead rows on the flat face of `part` at `point`, standing `base` off it,
/// `t` high, within `room` (the smooth layer, already cut to any region).
fn bead_rows(part: &Body, point: [f64; 3], room: &Body, base: f64, t: f64, rows: Rows) -> Result<Body> {
    let face = picked_face(part, point)?;
    let Some(n) = part.face_plane_normal(face)? else {
        return Err(SurfacingRefusal::RowsNeedFlat);
    };
    let n = n.value;
    let along = match rows.along {
        Some(a) => a,
        None => longest_edge(part, face)?,
    };
    let [_, a] = affine::orthonormal_axes(n, along)?;
    let quarter = affine::rotation([0.0; 3], n, std::f64::consts::FRAC_PI_2)?;
    let s = affine::apply_point(&quarter, a);
    let bb = room.exact_bounds()?;
    let (min, max) = ([bb.min.x, bb.min.y, bb.min.z], [bb.max.x, bb.max.y, bb.max.z]);
    let o = add(point, scale(n, base));
    let [a0, a1] = affine::span_along(min, max, o, a)?;
    let [s0, s1] = affine::span_along(min, max, o, s)?;
    let w = rows.width;
    let pitch = w * (1.0 - rows.overlap);
    let count = ((s1 - s0) / pitch).ceil() + 1.0;
    if !count.is_finite() || count > 10_000.0 {
        return bad("bead rows so narrow that the face has too many");
    }
    let length = a1 - a0 + 2.0;
    let mut beads = Vec::new();
    for i in 0..count as usize {
        let mid = s0 + i as f64 * pitch;
        let p = |off: f64, z: f64| add(add(add(o, scale(a, a0 - 1.0)), scale(s, mid + off)), scale(n, z));
        let arc = curve::arc_through([p(-w / 2.0, 0.0), p(0.0, t), p(w / 2.0, 0.0)])?;
        let mut profile = profiles::curve_pieces(&arc, [0.0, 1.0])?;
        profile.push(Curve::Line { start: p(w / 2.0, 0.0), end: p(-w / 2.0, 0.0) });
        beads.push(profiles::extrude_curves(&profile, a, length, true)?);
    }
    let rows = solidcraft_kernel::union_all(&beads.iter().collect::<Vec<_>>())?;
    // within the smooth layer's footprint, however high the beads stand: the layer grown up by t
    let tall = solidcraft_kernel::union_all(&[room, &solidcraft_kernel::transform_matrix(room, translation(scale(n, t)))?])?;
    solidcraft_kernel::boolean(&rows, &tall, BoolOp::Intersect)?.ok_or(SurfacingRefusal::Invalid("bead rows left empty"))
}

/// The record face of `part` that `point` lies on: the one nearest it.
fn picked_face(part: &Body, point: [f64; 3]) -> Result<usize> {
    let mut best: Option<(f64, usize)> = None;
    for f in 0..part.face_count() {
        if let Some(c) = part.closest_to_point(&[f], &[], point)?
            && best.is_none_or(|(d, _)| c.distance < d)
        {
            best = Some((c.distance, f));
        }
    }
    best.map(|(_, f)| f).ok_or(SurfacingRefusal::Invalid("no face of the part near the picked point"))
}

/// The direction of the longest straight edge round record face `face` of `part`.
fn longest_edge(part: &Body, face: usize) -> Result<[f64; 3]> {
    let rim = part.rim_edges(&[face])?;
    let edges = part.edges(1e-3)?;
    let longest = rim.iter().filter_map(|&e| edges.get(e)).max_by(|x, y| x.length.total_cmp(&y.length));
    let Some(e) = longest.filter(|e| e.points.len() >= 2) else {
        return bad("a face with no edge to run bead rows along");
    };
    let (p, q) = (e.points[0], e.points[e.points.len() - 1]);
    Ok(sub([q.x, q.y, q.z], [p.x, p.y, p.z]))
}

/// The move by `d`, as a column-major 4 x 4 matrix ([`solidcraft_kernel::transform_matrix`]).
fn translation(d: [f64; 3]) -> [[f64; 4]; 4] {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [d[0], d[1], d[2], 1.0]]
}

#[cfg(test)]
#[path = "weld_surfacing_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
