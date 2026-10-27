//! Bounded conversion of borrowed planar paths to DXF geometry.
//!
//! Admission precedes recognition and fitting. A failed conversion returns no partial path;
//! piece errors always refer to the original input, including after arc splitting or fitting.

use std::f64::consts::{PI, TAU};

use solidcraft_geom::{Loop2, MAX_BEZIER_DEGREE, MAX_FIT_PIECES, Seg2, Vec2, conic_ellipse};

use super::{DxfShape, bezier_spline, wrap};
use crate::{IoError, Result};

/// A borrowed sequence of planar pieces, optionally carrying the caller's closed-loop intent.
/// This is not a certificate of continuity or topology. Whole-circle recognition uses geom's
/// bounded angular rounding cells, not bitwise endpoint identity. Polyline joins require
/// equal stored endpoints and otherwise emit separate open runs without filling or snapping.
#[derive(Clone, Copy, Debug)]
pub enum DxfPath<'a> {
    Open(&'a [Seg2]),
    /// Permit whole-loop recognition and an explicit closing flag when demonstrated.
    /// This does not certify or repair the input topology; recognition has the policies above.
    Closed(&'a Loop2),
}

/// How a path's curves are represented. Coordinates and tolerances are in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DxfPathStyle {
    /// Preserve analytic curves and the original polynomial/rational spline controls.
    Exact,
    /// Collect consecutive lines and circular arcs into polylines with signed bulges.
    /// Other curves retain their exact representation and interrupt the polyline run.
    /// An arc that cannot preserve its support as a bulge retains its ARC entity instead.
    Polyline,
    /// Fit free-form pieces with the existing bounded line/arc fitter.
    /// Its sampling is not an analytic, uniform error certificate.
    Arcs { tolerance_mm: f64 },
}

/// A polyline vertex. `bulge` describes the edge leaving `at`: `tan(sweep / 4)`.
/// The terminal vertex of an open polyline has zero bulge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DxfVertex {
    pub at: Vec2,
    pub bulge: f64,
}

// Separate per-path budgets: one entity may contain many vertices. These do not bound an
// entire drawing made from many calls. Check before allocating from or expanding input.
pub(super) const MAX_PATH_ITEMS: usize = 200_000;
const MAX_FITTED_PATH_PIECES: usize = MAX_FIT_PIECES * 8;

#[derive(Clone, Copy)]
struct Piece {
    index: usize,
    total: usize,
}

impl Piece {
    fn invalid(self, reason: impl std::fmt::Display) -> IoError {
        IoError::Invalid(format!("DXF path: piece {} of {}: {reason}", self.index + 1, self.total))
    }
}

fn invalid(reason: &str) -> IoError {
    IoError::Invalid(format!("DXF path: {reason}"))
}

/// Convert a path without mutating it, dropping pieces, or changing their order.
///
/// Invalid options are rejected even for empty paths. Input pieces, output entities and
/// accumulated polyline vertices each have a separate limit of 200,000. If fitting is needed,
/// its 2,048-piece per-input and 16,384-piece per-path limits include retained lines/arcs.
/// Pure line/arc paths bypass the fitting limit. Positive finite conic weights are supported;
/// other rational domains are outside this converter's admission contract.
/// `Closed` permits recognition, not an invented closing edge: only equal stored endpoints
/// share a polyline vertex. Mixed or fragmented runs remain open, in original piece order.
pub fn path_shapes(path: DxfPath<'_>, style: DxfPathStyle) -> Result<Vec<DxfShape>> {
    if let DxfPathStyle::Arcs { tolerance_mm } = style
        && !(tolerance_mm.is_finite() && tolerance_mm >= 1e-9)
    {
        return Err(invalid("arc tolerance must be finite and at least 1e-9 mm"));
    }
    let (segs, closed) = match path {
        DxfPath::Open(segs) => (segs, None),
        DxfPath::Closed(lp) => (lp.segs.as_slice(), Some(lp)),
    };
    if segs.len() > MAX_PATH_ITEMS {
        return Err(invalid("input piece budget exceeded"));
    }
    // Do not call endpoint, recognition or fitting helpers before validating degrees.
    for (index, seg) in segs.iter().enumerate() {
        admit(seg, Piece { index, total: segs.len() })?;
    }
    if let Some(lp) = closed {
        if let Some((c, r)) = lp.as_circle() {
            // Every circular piece was admitted, including its support bounds.
            legacy_positive(r, Piece { index: 0, total: segs.len() }, "circle radius")?;
            return Ok(vec![DxfShape::Circle { c, r }]);
        }
        if !matches!(style, DxfPathStyle::Arcs { .. })
            && let Some((c, major, ratio)) = lp.as_ellipse()
        {
            // Keep the recognizer's validated support. Only its axis sign may follow the
            // first conic's historical DXF frame: negating the axis preserves a full ellipse,
            // whereas replacing its centre/ratio from another extraction need not do so.
            let first = lp.segs.first().and_then(|seg| match *seg {
                Seg2::Conic { a, apex, b, w } => conic_ellipse(a, apex, b, w),
                _ => None,
            });
            let mut major = major;
            if let Some(first) = first {
                let scale = major.x.abs().max(major.y.abs());
                let first_scale = first.major.x.abs().max(first.major.y.abs());
                if scale > 0.0 && first_scale > 0.0 && (major / scale).dot(first.major / first_scale) < 0.0 {
                    major = -major;
                }
            }
            if ellipse_representable(c, major, ratio, 0.0, TAU) {
                return Ok(vec![DxfShape::Ellipse { c, major, ratio, t0: 0.0, t1: TAU }]);
            }
        }
    }
    match style {
        DxfPathStyle::Polyline => polylines(segs, closed.is_some()),
        DxfPathStyle::Arcs { tolerance_mm } if segs.iter().any(|seg| !seg.is_line_or_arc()) => fitted(segs, tolerance_mm),
        DxfPathStyle::Exact | DxfPathStyle::Arcs { .. } => {
            let mut shapes = Vec::new();
            for (index, seg) in segs.iter().enumerate() {
                push_shape(&mut shapes, exact(seg, Piece { index, total: segs.len() })?)?;
            }
            Ok(shapes)
        }
    }
}

fn admit(seg: &Seg2, piece: Piece) -> Result<()> {
    let finite = match *seg {
        Seg2::Line { a, b } => a.is_finite() && b.is_finite(),
        Seg2::Arc { center, radius, start, sweep } => {
            if !(center.is_finite() && radius.is_finite() && start.is_finite() && sweep.is_finite()) {
                return Err(piece.invalid("non-finite circular arc"));
            }
            if radius <= 0.0 {
                return Err(piece.invalid("arc radius must be positive"));
            }
            if sweep == 0.0 {
                return Err(piece.invalid("arc sweep must be nonzero"));
            }
            if sweep.abs() > TAU {
                return Err(piece.invalid("arc sweep exceeds one turn"));
            }
            let end = start + sweep;
            if !end.is_finite() || end == start {
                return Err(piece.invalid("arc end angle is non-finite or unresolved"));
            }
            if !(start.to_degrees().is_finite() && end.to_degrees().is_finite()) {
                return Err(piece.invalid("arc angles cannot be represented in degrees"));
            }
            if !circle_bounds_finite(center, radius) {
                return Err(piece.invalid("circular support bounds are not finite"));
            }
            seg.start().is_finite() && seg.end().is_finite()
        }
        Seg2::Conic { a, apex, b, w } => {
            if !(w.is_finite() && w > 0.0) {
                return Err(piece.invalid("conic weight must be finite and positive"));
            }
            a.is_finite() && apex.is_finite() && b.is_finite()
        }
        Seg2::Cubic { p0, p1, p2, p3 } => [p0, p1, p2, p3].iter().all(|p| p.is_finite()),
        Seg2::Bezier { n, ref p } => {
            if !(4..=MAX_BEZIER_DEGREE).contains(&usize::from(n)) {
                return Err(piece.invalid(format_args!("unsupported Bezier degree {n}")));
            }
            p.get(..=usize::from(n)).is_some_and(|pts| pts.iter().all(|p| p.is_finite()))
        }
    };
    if !finite {
        return Err(piece.invalid("non-finite coordinate"));
    }
    Ok(())
}

fn circle_bounds_finite(c: Vec2, r: f64) -> bool {
    c.is_finite() && r.is_finite() && r > 0.0 && (c - Vec2::new(r, r)).is_finite() && (c + Vec2::new(r, r)).is_finite()
}

// The existing writer uses twelve decimals. Preserve the original rational controls rather
// than clamp a thinner ellipse's ratio to its decimal resolution. This is a representation
// restriction, not a minimum permitted size for source geometry.
fn ellipse_representable(c: Vec2, major: Vec2, ratio: f64, t0: f64, t1: f64) -> bool {
    if !(c.is_finite() && major.is_finite() && ratio.is_finite() && (1e-12..=1.0).contains(&ratio) && t0.is_finite() && t1.is_finite()) {
        return false;
    }
    if major == Vec2::ZERO || legacy_point(major) == Some(Vec2::ZERO) || legacy_number(t0) == legacy_number(t1) {
        return false;
    }
    let minor = major.perp() * ratio;
    // Bound the writer's actual c + major*cos(t) + minor*sin(t) evaluation, including
    // intermediate additions; a finite centre and two finite axes alone are insufficient.
    let extent = Vec2::new(major.x.abs() + minor.x.abs(), major.y.abs() + minor.y.abs());
    minor.is_finite() && extent.is_finite() && (c - extent).is_finite() && (c + extent).is_finite()
}

fn exact(seg: &Seg2, piece: Piece) -> Result<DxfShape> {
    let shape = match *seg {
        Seg2::Line { a, b } => {
            if a != b && legacy_point(a) == legacy_point(b) {
                return Err(piece.invalid("line endpoints collapse at DXF writer precision"));
            }
            DxfShape::Line(a, b)
        }
        Seg2::Arc { center, radius, start, sweep } => {
            legacy_positive(radius, piece, "arc radius")?;
            // Admission already refused >TAU. A shorter arc must not acquire a closing edge.
            if sweep.abs() == TAU {
                return Ok(DxfShape::Circle { c: center, r: radius });
            }
            let (a0, a1) = if sweep > 0.0 { (start, start + sweep) } else { (start + sweep, start) };
            let (a0, a1) = (a0.to_degrees().rem_euclid(360.0), a1.to_degrees().rem_euclid(360.0));
            if legacy_number(a0).map(|a| a.rem_euclid(360.0)) == legacy_number(a1).map(|a| a.rem_euclid(360.0)) {
                return Err(piece.invalid("arc endpoints collapse in the DXF angular representation"));
            }
            DxfShape::Arc { c: center, r: radius, a0, a1 }
        }
        Seg2::Conic { a, apex, b, w } => {
            if let Some(e) = conic_ellipse(a, apex, b, w) {
                // Geom preserves direction; DXF ELLIPSE parameters run counter-clockwise.
                // Keep the helper's endpoint expressions intact, including its CW ordering.
                let (start, end) = if e.end > e.start { (e.start, e.end) } else { (e.end, e.start) };
                let (t0, t1) = (wrap(start), wrap(end));
                if ellipse_representable(e.center, e.major, e.ratio, t0, t1) {
                    return Ok(DxfShape::Ellipse { c: e.center, major: e.major, ratio: e.ratio, t0, t1 });
                }
            }
            // Recognition failure is not invalid input. Positive weights keep this spline
            // inside the finite control hull even if ellipse extraction would overflow.
            legacy_positive(w, piece, "conic weight")?;
            bezier_spline(&[a, apex, b], (w != 1.0).then(|| vec![1.0, w, 1.0]))
        }
        Seg2::Cubic { p0, p1, p2, p3 } => bezier_spline(&[p0, p1, p2, p3], None),
        Seg2::Bezier { n, ref p } => {
            let pts = p.get(..=usize::from(n)).ok_or_else(|| piece.invalid(format_args!("unsupported Bezier degree {n}")))?;
            bezier_spline(pts, None)
        }
    };
    if let DxfShape::Spline { ctrl, .. } = &shape
        && let Some(first) = ctrl.first()
        && ctrl.iter().any(|p| p != first)
        && ctrl.iter().all(|p| legacy_point(*p) == legacy_point(*first))
    {
        return Err(piece.invalid("spline controls collapse at DXF writer precision"));
    }
    Ok(shape)
}

// Mirror Out::num's numeric precision for admission only; never rewrite its output or
// substitute rounded input controls. New bulged-polyline fields use round-trip precision.
fn legacy_number(value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    format!("{value:.12}").parse::<f64>().ok().filter(|v| v.is_finite())
}

fn legacy_point(point: Vec2) -> Option<Vec2> {
    Some(Vec2::new(legacy_number(point.x)?, legacy_number(point.y)?))
}

fn legacy_positive(value: f64, piece: Piece, name: &str) -> Result<()> {
    if !legacy_number(value).is_some_and(|v| v > 0.0) {
        return Err(piece.invalid(format_args!("{name} is not positive at DXF writer precision")));
    }
    Ok(())
}

fn fitted(segs: &[Seg2], tolerance: f64) -> Result<Vec<DxfShape>> {
    let mut shapes = Vec::new();
    let mut count = 0usize;
    for (index, seg) in segs.iter().enumerate() {
        let piece = Piece { index, total: segs.len() };
        if seg.is_line_or_arc() {
            count_fit(&mut count, 1, piece)?;
            push_shape(&mut shapes, exact(seg, piece)?)?;
            continue;
        }
        let adjusted = seg.fit_arcs(tolerance).ok_or_else(|| piece.invalid("curve could not be fitted within the arc fitting budget"))?;
        if adjusted.is_empty() {
            return Err(piece.invalid("fitting produced no pieces"));
        }
        if adjusted.len() > MAX_FIT_PIECES {
            return Err(piece.invalid("per-piece arc fitting budget exceeded"));
        }
        count_fit(&mut count, adjusted.len(), piece)?;
        for fitted in adjusted {
            if !fitted.is_line_or_arc() {
                return Err(piece.invalid("arc fitting produced a non-line/arc piece"));
            }
            admit(&fitted, piece)?;
            push_shape(&mut shapes, exact(&fitted, piece)?)?;
        }
    }
    Ok(shapes)
}

fn count_fit(count: &mut usize, additional: usize, piece: Piece) -> Result<()> {
    let total = count.checked_add(additional).ok_or_else(|| piece.invalid("path arc fitting budget exceeded"))?;
    if total > MAX_FITTED_PATH_PIECES {
        return Err(piece.invalid("path arc fitting budget exceeded"));
    }
    *count = total;
    Ok(())
}

fn push_shape(shapes: &mut Vec<DxfShape>, shape: DxfShape) -> Result<()> {
    if shapes.len() >= MAX_PATH_ITEMS {
        return Err(invalid("output entity budget exceeded"));
    }
    shapes.try_reserve(1).map_err(|_| invalid("cannot allocate output entities"))?;
    shapes.push(shape);
    Ok(())
}

#[derive(Default)]
struct Run {
    vertices: Vec<DxfVertex>,
    end: Option<Vec2>,
}

struct Polylines {
    shapes: Vec<DxfShape>,
    run: Run,
    vertices: usize,
    uninterrupted: bool,
}

impl Polylines {
    fn reserve_vertices(&self, additional: usize) -> Result<()> {
        if self.vertices.checked_add(additional).is_none_or(|n| n > MAX_PATH_ITEMS) {
            return Err(invalid("polyline vertex budget exceeded"));
        }
        Ok(())
    }

    fn vertex(&mut self, vertex: DxfVertex) -> Result<()> {
        self.reserve_vertices(1)?;
        self.run.vertices.try_reserve(1).map_err(|_| invalid("cannot allocate polyline vertices"))?;
        self.run.vertices.push(vertex);
        self.vertices += 1;
        Ok(())
    }

    fn edge(&mut self, a: Vec2, b: Vec2, bulge: f64, piece: Piece) -> Result<()> {
        if self.run.end.is_some_and(|end| end != a) {
            // Sharing is exact in stored coordinates (including +/-0). Never replace an
            // incoming endpoint with the outgoing one, or bridge them with a straight edge.
            self.flush(false)?;
            self.uninterrupted = false;
        }
        if !(a.is_finite() && b.is_finite() && bulge.is_finite()) {
            return Err(piece.invalid("non-finite polyline vertex or bulge"));
        }
        self.vertex(DxfVertex { at: a, bulge })?;
        self.run.end = Some(b);
        Ok(())
    }

    fn flush(&mut self, closed: bool) -> Result<()> {
        let Some(end) = self.run.end else { return Ok(()) };
        if !closed {
            self.vertex(DxfVertex { at: end, bulge: 0.0 })?;
        }
        let vertices = std::mem::take(&mut self.run.vertices);
        self.run.end = None;
        push_shape(&mut self.shapes, DxfShape::BulgedPolyline { vertices, closed })
    }

    fn exact(&mut self, shape: DxfShape) -> Result<()> {
        self.flush(false)?;
        self.uninterrupted = false;
        push_shape(&mut self.shapes, shape)
    }
}

fn polylines(segs: &[Seg2], closed: bool) -> Result<Vec<DxfShape>> {
    let mut out = Polylines { shapes: Vec::new(), run: Run::default(), vertices: 0, uninterrupted: true };
    for (index, seg) in segs.iter().enumerate() {
        let piece = Piece { index, total: segs.len() };
        match *seg {
            Seg2::Line { a, b } => out.edge(a, b, 0.0, piece)?,
            Seg2::Arc { center, radius, start, sweep } if sweep.abs() < TAU => {
                let (a, b) = (seg.start(), seg.end());
                if sweep.abs() > PI {
                    // Split at the actual midpoint angle, keeping both source endpoints.
                    let mid_angle = start + sweep * 0.5;
                    if mid_angle == start || mid_angle == start + sweep {
                        out.exact(exact(seg, piece)?)?;
                        continue;
                    }
                    let mid = center + Vec2::from_angle(mid_angle) * radius;
                    let Some(bulge) = arc_bulge(sweep * 0.5) else {
                        out.exact(exact(seg, piece)?)?;
                        continue;
                    };
                    if !can_pack(a, mid, bulge, center, radius, sweep * 0.5) || !can_pack(mid, b, bulge, center, radius, sweep * 0.5) {
                        out.exact(exact(seg, piece)?)?;
                        continue;
                    }
                    // Both halves must qualify before either is appended.
                    // A prior disjoint run also needs its terminal zero-bulge vertex.
                    let terminal = usize::from(out.run.end.is_some_and(|end| end != a));
                    out.reserve_vertices(2 + terminal)?;
                    out.edge(a, mid, bulge, piece)?;
                    out.edge(mid, b, bulge, piece)?;
                } else {
                    if let Some(bulge) = arc_bulge(sweep)
                        && can_pack(a, b, bulge, center, radius, sweep)
                    {
                        out.edge(a, b, bulge, piece)?;
                    } else {
                        out.exact(exact(seg, piece)?)?;
                    }
                }
            }
            _ => out.exact(exact(seg, piece)?)?,
        }
    }
    // Only a run that covers the entire closed path can receive the closing flag. Its last
    // outgoing bulge stays on the final vertex; the first vertex is not appended a second time.
    // A single authored zero-length line stays an open two-vertex entity; it cannot form a
    // writable one-vertex closed polyline. Exact preserves the original LINE unchanged.
    let close =
        closed && out.uninterrupted && out.run.vertices.len() >= 2 && out.run.vertices.first().is_some_and(|first| Some(first.at) == out.run.end);
    out.flush(close)?;
    Ok(out.shapes)
}

fn arc_bulge(sweep: f64) -> Option<f64> {
    // A semicircle has bulge exactly +/-1; all other admitted pieces use tan(sweep/4).
    let bulge = if sweep.abs() == PI { sweep.signum() } else { (sweep * 0.25).tan() };
    (bulge.is_finite() && bulge != 0.0 && bulge.abs() <= 1.0).then_some(bulge)
}

// Packing is optional. Check algebraic support residuals of the values that will actually
// be serialized, normalized around the ORIGINAL centre/radius. A world-coordinate ULP
// allowance would hide local distortion (for example radius .75 at centre (2^52, 2^52)).
// This is an operational acceptance policy at arithmetic roundoff scale, not an identity
// proof or a universal error guarantee for sin/cos/tan. No coordinates or parameters are
// adjusted to pass it. Serialization must round-trip these same f64 vertices and bulges.
fn can_pack(a: Vec2, z: Vec2, bulge: f64, center: Vec2, radius: f64, sweep: f64) -> bool {
    if !radius.is_normal() || bulge.signum() != sweep.signum() || sweep.abs() > PI {
        return false;
    }
    let (from, to) = (a - center, z - center);
    if [from.x, from.y, to.x, to.y].iter().any(|&value| normal_or_zero(value).is_none()) {
        return false;
    }
    let (u, v) = (from / radius, to / radius);
    if [(from.x, u.x), (from.y, u.y), (to.x, v.x), (to.y, v.y)].iter().any(|&(input, output)| input != 0.0 && output == 0.0) {
        return false;
    }
    let Some((ui, vi)) = SupportPoint::normalized(a, center, radius).zip(SupportPoint::normalized(z, center, radius)) else {
        return false;
    };
    support_residuals(u, v, bulge, ui, vi).is_some() && bulge_arc(DxfVertex { at: a, bulge }, z).is_some()
}

// The gamma_n relative-error model applies to normal arithmetic. Conservatively retain
// ARC on subnormal intermediate results instead of adding an unbounded underflow margin.
fn normal_or_zero(value: f64) -> Option<f64> {
    (value == 0.0 || value.is_normal()).then_some(value)
}

fn normal_product(a: f64, b: f64) -> Option<f64> {
    let product = normal_or_zero(a * b)?;
    (product != 0.0 || a == 0.0 || b == 0.0).then_some(product)
}

// These nonnegative quantities are admission limits, so round DOWN: uncertainty in the
// comparison must not enlarge the allowed geometric discrepancy.
fn lower_add(a: f64, b: f64) -> f64 {
    if a == 0.0 {
        b
    } else if b == 0.0 {
        a
    } else {
        (a + b).next_down()
    }
}

fn lower_mul(a: f64, b: f64) -> f64 {
    if a == 0.0 || b == 0.0 { 0.0 } else { (a * b).next_down() }
}

fn gamma_lower(operations: f64) -> f64 {
    let nu = lower_mul(operations, f64::EPSILON * 0.5);
    (nu / (1.0 - nu).next_up()).next_down()
}

/// Outward enclosure starting at the actual serialized vertices and original support,
/// including their normalization. This does not enclose a hypothetical exact trigonometric
/// construction of those vertices; their stored f64 values are the operands being checked.
#[derive(Clone, Copy)]
struct SupportInterval {
    lo: f64,
    hi: f64,
}

impl SupportInterval {
    fn exact(value: f64) -> Self {
        Self { lo: value, hi: value }
    }

    fn enclosing(lo: f64, hi: f64) -> Option<Self> {
        let (lo, hi) = (lo.next_down(), hi.next_up());
        Self::checked(lo, hi)
    }

    fn checked(lo: f64, hi: f64) -> Option<Self> {
        (lo.is_finite() && hi.is_finite() && lo <= hi).then_some(Self { lo, hi })
    }

    fn is_exact(self, value: f64) -> bool {
        self.lo == value && self.hi == value
    }

    fn negated(self) -> Self {
        Self { lo: -self.hi, hi: -self.lo }
    }

    fn from_residual(value: f64, residual: f64, exact_zero: bool) -> Option<Self> {
        if !value.is_finite() || !residual.is_finite() {
            return None;
        }
        if residual == 0.0 && !exact_zero {
            // An FMA residual can underflow. Zero alone does not prove exactness.
            return Self::enclosing(value, value);
        }
        let lo = if residual < 0.0 { value.next_down() } else { value };
        let hi = if residual > 0.0 { value.next_up() } else { value };
        Self::checked(lo, hi)
    }

    fn sum(a: f64, b: f64) -> Option<Self> {
        // TwoSum recovers the exact addition residual in the absence of overflow.
        let sum = a + b;
        let virtual_b = sum - a;
        let virtual_a = sum - virtual_b;
        let error_a = a - virtual_a;
        let error_b = b - virtual_b;
        let residual = error_a + error_b;
        if [sum, virtual_b, virtual_a, error_a, error_b, residual].iter().any(|v| !v.is_finite()) {
            return Self::enclosing(sum, sum);
        }
        Self::from_residual(sum, residual, true)
    }

    fn product(a: f64, b: f64) -> Option<Self> {
        let product = a * b;
        if !product.is_finite() {
            return None;
        }
        if a == 0.0
            || b == 0.0
            || a.abs() == 1.0
            || b.abs() == 1.0
            || (product.is_normal() && ((power_of_two(a) && b.is_normal()) || (power_of_two(b) && a.is_normal())))
        {
            return Some(Self::exact(product));
        }
        Self::from_residual(product, a.mul_add(b, -product), false)
    }

    fn quotient(a: f64, divisor: f64) -> Option<Self> {
        let quotient = a / divisor;
        if !quotient.is_finite() {
            return None;
        }
        if a == 0.0 || divisor == 1.0 || a.abs() == divisor || (power_of_two(divisor) && a.is_normal() && quotient.is_normal()) {
            return Some(Self::exact(quotient));
        }
        // A positive divisor preserves the sign of a - quotient*divisor.
        Self::from_residual(quotient, (-quotient).mul_add(divisor, a), false)
    }

    fn add(self, rhs: Self) -> Option<Self> {
        if self.is_exact(0.0) {
            return Some(rhs);
        }
        if rhs.is_exact(0.0) {
            return Some(self);
        }
        Self::checked(Self::sum(self.lo, rhs.lo)?.lo, Self::sum(self.hi, rhs.hi)?.hi)
    }

    fn sub(self, rhs: Self) -> Option<Self> {
        self.add(rhs.negated())
    }

    fn mul(self, rhs: Self) -> Option<Self> {
        if self.is_exact(0.0) || rhs.is_exact(0.0) {
            return Some(Self::exact(0.0));
        }
        if self.is_exact(1.0) {
            return Some(rhs);
        }
        if rhs.is_exact(1.0) {
            return Some(self);
        }
        if self.is_exact(-1.0) {
            return Some(rhs.negated());
        }
        if rhs.is_exact(-1.0) {
            return Some(self.negated());
        }
        let products =
            [Self::product(self.lo, rhs.lo)?, Self::product(self.lo, rhs.hi)?, Self::product(self.hi, rhs.lo)?, Self::product(self.hi, rhs.hi)?];
        let lo = products.iter().map(|p| p.lo).fold(f64::INFINITY, f64::min);
        let hi = products.iter().map(|p| p.hi).fold(f64::NEG_INFINITY, f64::max);
        Self::checked(lo, hi)
    }

    fn div_positive(self, divisor: f64) -> Option<Self> {
        if !divisor.is_finite() || divisor <= 0.0 {
            return None;
        }
        Self::checked(Self::quotient(self.lo, divisor)?.lo, Self::quotient(self.hi, divisor)?.hi)
    }

    fn fits(self, bound: f64) -> bool {
        bound.is_normal() && bound > 0.0 && self.lo >= -bound && self.hi <= bound
    }
}

fn power_of_two(value: f64) -> bool {
    // A normal binary64 power of two has no fraction bits, independent of its sign.
    value.is_normal() && value.to_bits() & ((1_u64 << 52) - 1) == 0
}

struct SupportPoint {
    x: SupportInterval,
    y: SupportInterval,
}

impl SupportPoint {
    fn normalized(point: Vec2, center: Vec2, radius: f64) -> Option<Self> {
        let exact = SupportInterval::exact;
        Some(Self { x: exact(point.x).sub(exact(center.x))?.div_positive(radius)?, y: exact(point.y).sub(exact(center.y))?.div_positive(radius)? })
    }
}

fn support_residuals(u: Vec2, v: Vec2, b: f64, ui: SupportPoint, vi: SupportPoint) -> Option<()> {
    if [u.x, u.y, v.x, v.y, b].iter().any(|&value| normal_or_zero(value).is_none()) {
        return None;
    }
    let sum = u + v;
    let d = v - u;
    if [sum.x, sum.y, d.x, d.y].iter().any(|&value| normal_or_zero(value).is_none()) {
        return None;
    }
    let square = normal_product(b, b)?;
    if square == 0.0 {
        return None;
    }
    let twice = normal_product(2.0, b)?;
    let minus = normal_or_zero(1.0 - square)?;
    normal_or_zero(normal_product(twice, sum.x)? - normal_product(d.y, minus)?)?;
    normal_or_zero(normal_product(twice, sum.y)? + normal_product(d.x, minus)?)?;
    let plus = normal_or_zero(1.0 + square)?;
    let chord2 = normal_or_zero(normal_product(d.x, d.x)? + normal_product(d.y, d.y)?)?;
    if chord2 == 0.0 {
        return None;
    }
    let left = normal_product(chord2, normal_product(plus, plus)?)?;
    let right = normal_product(16.0, square)?;
    normal_or_zero(left - right)?;

    // qC = 2*b*(u+v) + perp(v-u)*(1-b*b); qC/(4*b) is the reconstructed
    // centre relative to the original. qR = |v-u|²*(1+b²)² - 16*b²;
    // qR/(16*b²) is its squared-radius discrepancy. Bounds use unit-circle scale,
    // with a shared infinity norm for centre coordinates, including near-zero ones.
    let norm = |p: Vec2| p.x.abs().max(p.y.abs());
    let b2_lo = lower_mul(b.abs(), b.abs());
    let plus_lo = lower_add(1.0, b2_lo);
    let center_scale = lower_add(lower_mul(lower_mul(2.0, b.abs()), lower_add(norm(u), norm(v))), lower_mul(norm(d), plus_lo));
    let chord2_lo = lower_add(lower_mul(d.x.abs(), d.x.abs()), lower_mul(d.y.abs(), d.y.abs()));
    let radius_scale = lower_add(lower_mul(chord2_lo, lower_mul(plus_lo, plus_lo)), lower_mul(16.0, b2_lo));

    // Centre: 8 scalar normalizations, 4 vector +/- components, b² and 2b,
    // 1-b², four products and two final +/- = 21 operations.
    // Radius: 8 normalizations, 2 chord components, b², 1+b², three operations
    // for |d|², a square, a product, scaling by 16 and a subtraction = 19.
    // Also cap normalized centre and squared-radius error independently of conditioning:
    // |qC|/(4|b|) <= gamma_21 and |qR|/(16b²) <= gamma_19. Small b must not amplify
    // an accepted residual into large geometric distortion. Limits round inward.
    let center_bound = lower_mul(gamma_lower(21.0), center_scale).min(lower_mul(lower_mul(4.0, b.abs()), gamma_lower(21.0)));
    let radius_bound = lower_mul(gamma_lower(19.0), radius_scale).min(lower_mul(lower_mul(16.0, b2_lo), gamma_lower(19.0)));

    let exact = SupportInterval::exact;
    let ib = exact(b);
    let square = ib.mul(ib)?;
    let twice = exact(2.0).mul(ib)?;
    let minus = exact(1.0).sub(square)?;
    let plus = exact(1.0).add(square)?;
    let dx = vi.x.sub(ui.x)?;
    let dy = vi.y.sub(ui.y)?;
    let qx = twice.mul(ui.x.add(vi.x)?)?.sub(dy.mul(minus)?)?;
    let qy = twice.mul(ui.y.add(vi.y)?)?.add(dx.mul(minus)?)?;
    let qr = dx.mul(dx)?.add(dy.mul(dy)?)?.mul(plus.mul(plus)?)?.sub(exact(16.0).mul(square)?)?;
    // Inclusion of the WHOLE enclosure is required; merely containing zero is insufficient.
    (qx.fits(center_bound) && qy.fits(center_bound) && qr.fits(radius_bound)).then_some(())
}

/// Reconstruct a non-straight polyline edge with a signed sweep for bounds and validation.
/// Avoid squaring the chord length or first forming 1/bulge: those can overflow even when
/// the resulting radius is finite. A zero chord with a nonzero bulge is not an arc.
pub(super) fn bulge_arc(from: DxfVertex, to: Vec2) -> Option<Seg2> {
    let b = from.bulge;
    if !(from.at.is_finite() && to.is_finite() && b.is_finite()) || b == 0.0 || b.abs() > 1.0 {
        return None;
    }
    let chord = to - from.at;
    let length = chord.len();
    if !chord.is_finite() || !length.is_finite() || length == 0.0 {
        return None;
    }
    let scaled = (length * 0.25) / b;
    let radius = scaled.abs() * (1.0 + b * b);
    let center = from.at * 0.5 + to * 0.5 + (chord / length).perp() * (scaled * (1.0 - b * b));
    if !circle_bounds_finite(center, radius) {
        return None;
    }
    let radial = from.at - center;
    if !radial.is_finite() || radial == Vec2::ZERO {
        return None;
    }
    let start = radial.angle();
    let sweep = 4.0 * b.atan();
    (start.is_finite() && sweep.is_finite() && sweep != 0.0).then_some(Seg2::Arc { center, radius, start, sweep })
}
