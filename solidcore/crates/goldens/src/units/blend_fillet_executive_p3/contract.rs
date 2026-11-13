//! The reference wrapper's input contract for unit `blend.fillet_executive_p3`'s records,
//! ported from that wrapper's `validate*` checks: every record the Rust side runs keeps it,
//! so an input the reference fixtures could not encode is a harness error, as it was.

use std::fmt;

/// A record outside the contract, with the reason.
#[derive(Debug)]
pub(super) struct Bad(String);

impl fmt::Display for Bad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid argument: {}", self.0)
    }
}

impl From<&str> for Bad {
    fn from(s: &str) -> Self {
        Bad(s.to_string())
    }
}

/// A sequential reader over a record.
struct Cursor<'a> {
    input: &'a [f64],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(input: &'a [f64]) -> Self {
        Self { input, at: 0 }
    }

    fn num(&mut self) -> Result<f64, Bad> {
        let v = self.input.get(self.at).copied().ok_or_else(|| Bad::from("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    /// `k` finite values.
    fn slice(&mut self, k: usize) -> Result<&'a [f64], Bad> {
        let end = self.at.checked_add(k).ok_or_else(|| Bad::from("overflow"))?;
        let s = self.input.get(self.at..end).ok_or_else(|| Bad::from("input too short"))?;
        if s.iter().any(|x| !x.is_finite()) {
            return Err(Bad::from("non-finite input"));
        }
        self.at = end;
        Ok(s)
    }

    fn done(&self) -> Result<(), Bad> {
        if self.at != self.input.len() {
            return Err(Bad::from("trailing input"));
        }
        Ok(())
    }

    /// An integer in `0..=max`.
    fn index(&mut self, max: usize) -> Result<usize, Bad> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(Bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, Bad> {
        Ok(self.index(1)? == 1)
    }
}

/// Validate the fixed record before entering native code. Boolean and enum codes
/// are exact; tolerances are finite (the native constructor applies `fabs`).
/// Noncanonical Boolean encodings and NaN/infinite tolerances are rejected as
/// fixture-encoding choices, not as upstream UB guards. There are no input
/// allocations or recursive native paths for these empty B-rep fixtures.
pub(super) fn validate(input: &[f64]) -> Result<(), Bad> {
    let mut reader = Cursor::new(input);
    reader.index(2)?;
    for _ in 0..5 {
        reader.flag()?;
    }
    reader.index(7)?;
    let tolerance = reader.num()?;
    if !tolerance.is_finite() {
        return Err(Bad("invalid tolerance".into()));
    }
    for _ in 0..5 {
        reader.flag()?;
    }
    reader.done()
}

/// Check the selector of the seven fixed upstream API examples (0 through 6).
pub(super) fn validate_upstream_fixture(example: usize) -> Result<(), Bad> {
    if example > 6 {
        return Err(Bad(
            "unknown upstream fillet fixture".into(),
        ));
    }
    Ok(())
}

/// Validate the real-box fixture wire: XYZ dimensions, radius, cross-section,
/// continuity, thumbweight, selected-edge count, then owning-list edge indices.
/// Bounds describe the fixture's finite box and enum encoding, not a claim that
/// the public fillet API rejects other finite parameter values.
pub(super) fn validate_real_solid(input: &[f64]) -> Result<(), Bad> {
    let mut r = Cursor::new(input);
    for value in r.slice(3)? {
        if *value <= 1e-12 {
            return Err(Bad(
                "non-positive box dimension".into(),
            ));
        }
    }
    r.slice(1)?;
    // Native ULONG is unsigned long (64-bit on this Linux reference). Reject
    // the rounded 2^64 endpoint before a floating-point-to-integer conversion.
    for _ in 0..2 {
        let code = r.num()?;
        if !code.is_finite() || code < 0.0 || code.fract() != 0.0 || code >= 18446744073709551616.0
        {
            return Err(Bad("invalid native ULONG".into()));
        }
    }
    r.slice(1)?;
    let remaining = input.len().saturating_sub(8);
    let count = r.index(remaining)?;
    if count == 0 || count != remaining {
        return Err(Bad(
            "invalid edge selection length".into(),
        ));
    }
    for _ in 0..count {
        r.index(11)?;
    }
    r.done()
}

/// Validate the unchanged real-box wire and a finite linear-law endpoint.
/// The variable-radius public API has no thumbweight argument.
pub(super) fn validate_real_variable_solid(input: &[f64], end_radius: f64) -> Result<(), Bad> {
    validate_real_solid(input)?;
    if !end_radius.is_finite() || input.get(6) != Some(&1.0) {
        return Err(Bad(
            "variable radius requires a finite endpoint and default thumbweight".into(),
        ));
    }
    Ok(())
}

/// Largest degree and control-point count of a fixture radius law.
const LAW_MAX_DEGREE: f64 = 3.0;

const LAW_MAX_POINTS: usize = 64;

/// Validate the unchanged real-box wire (default thumbweight) and a B-spline radius
/// law block: degree, radius count, the radii (control points `(r, 0, 0)`), unique
/// knot count, the strictly increasing knots, their multiplicities (`degree + 1` at
/// the ends, `1..=degree` inside, summing to radii + degree + 1), then the edge map's
/// min and max. Every value finite. These bounds define the fixture format: within
/// them `SmBSplineCurve::CreateCanonical` builds the curve on both sides.
pub(super) fn validate_real_law_solid(input: &[f64], law: &[f64]) -> Result<(), Bad> {
    validate_real_solid(input)?;
    let bad = |what: &str| Bad(format!("radius law: {what}"));
    if input.get(6) != Some(&1.0) {
        return Err(bad("default thumbweight required"));
    }
    if law.iter().any(|value| !value.is_finite()) {
        return Err(bad("non-finite value"));
    }
    let mut r = Cursor::new(law);
    let degree = r.num()?;
    if degree.fract() != 0.0 || !(1.0..=LAW_MAX_DEGREE).contains(&degree) {
        return Err(bad("degree outside 1..=3"));
    }
    let degree = degree as usize;
    let points = r.index(LAW_MAX_POINTS)?;
    if points <= degree {
        return Err(bad("fewer radii than degree + 1"));
    }
    r.slice(points)?;
    let knots = r.index(LAW_MAX_POINTS)?;
    if knots < 2 {
        return Err(bad("fewer than two knots"));
    }
    let values = r.slice(knots)?;
    if values.iter().zip(values.iter().skip(1)).any(|(a, b)| a >= b) {
        return Err(bad("knots not strictly increasing"));
    }
    let mut total = 0;
    for i in 0..knots {
        let end = i == 0 || i + 1 == knots;
        let mult = r.index(degree + 1)?;
        if (end && mult != degree + 1) || (!end && !(1..=degree).contains(&mult)) {
            return Err(bad("invalid knot multiplicity"));
        }
        total += mult;
    }
    if total != points + degree + 1 {
        return Err(bad("multiplicities do not match the radius count"));
    }
    r.slice(2)?;
    r.done()
}

/// Validate the unchanged box wire and the separate XYZ/distance fixture block.
/// Finiteness defines this bridge's input format, not the native setter's policy;
/// zero and negative finite distances remain representable.
pub(super) fn validate_real_solid_setback(input: &[f64], setback: [f64; 4]) -> Result<(), Bad> {
    validate_real_solid(input)?;
    if setback.iter().any(|value| !value.is_finite()) {
        return Err(Bad(
            "non-finite corner setback fixture".into(),
        ));
    }
    Ok(())
}

/// Number of source parameters on the [`real_source`] wire.
pub(super) const SOURCE_PARAMS: usize = 12;

/// Largest edge index a source wire may name (the shim rejects indices past the
/// built body's edge list; this only keeps the conversion exact).
const MAX_SOURCE_EDGE: usize = 1 << 20;

/// Validate the non-box real-solid wire: source kind (1 `.smb` file, 2 cylinder,
/// 3 box union, 4 box difference, 5 the union of a box (parameters 0..6) and a
/// cylinder (base centre 6..9, radius 9, height 10)), [`SOURCE_PARAMS`] source
/// parameters, then radius, cross-section, continuity, thumbweight, selected-edge count (0 = every edge) and
/// owning-list edge indices. `has_path` says whether a file path accompanies it
/// (exactly kind 1). Cylinder radius/height and box sizes must be positive, as
/// `SmApiCreateCylinder` / `SmApiCreateBox` require.
pub(super) fn validate_real_source(input: &[f64], has_path: bool) -> Result<(), Bad> {
    if input.iter().any(|v| !v.is_finite()) {
        return Err(Bad("non-finite source wire".into()));
    }
    let mut r = Cursor::new(input);
    let kind = r.index(5)?;
    if kind == 0 || (kind == 1) != has_path {
        return Err(Bad("invalid solid source".into()));
    }
    let params = r.slice(SOURCE_PARAMS)?;
    let positive: &[usize] = match kind {
        2 => &[3, 4],
        3 | 4 => &[3, 4, 5, 9, 10, 11],
        5 => &[3, 4, 5, 9, 10],
        _ => &[],
    };
    for &i in positive {
        if params.get(i).is_none_or(|v| *v <= 1e-12) {
            return Err(Bad("non-positive source size".into()));
        }
    }
    r.slice(1)?;
    for _ in 0..2 {
        let code = r.num()?;
        if code < 0.0 || code.fract() != 0.0 || code >= 18446744073709551616.0 {
            return Err(Bad("invalid native ULONG".into()));
        }
    }
    r.slice(1)?;
    let head = 1 + SOURCE_PARAMS + 5;
    let remaining = input.len().saturating_sub(head);
    let count = r.index(remaining)?;
    if count != remaining {
        return Err(Bad(
            "invalid edge selection length".into(),
        ));
    }
    for _ in 0..count {
        r.index(MAX_SOURCE_EDGE)?;
    }
    r.done()
}

/// Validate the non-box real-solid wire ([`validate_real_source`]) with a finite
/// linear-law endpoint and the default thumbweight (the variable-radius public API
/// has no thumbweight argument).
pub(super) fn validate_real_variable_source(
    input: &[f64],
    has_path: bool,
    end_radius: f64,
) -> Result<(), Bad> {
    validate_real_source(input, has_path)?;
    if !end_radius.is_finite() || input.get(1 + SOURCE_PARAMS + 3) != Some(&1.0) {
        return Err(Bad(
            "variable radius requires a finite endpoint and default thumbweight".into(),
        ));
    }
    Ok(())
}

/// Validate the surface-surface wire: two surfaces' four corner points (24 values),
/// radius1, radius2, tolerance, cross-section (native ULONG), mirror and complement
/// (0/1).
pub(super) fn validate_surface_surface(input: &[f64]) -> Result<(), Bad> {
    if input.len() != 30 || input.iter().any(|v| !v.is_finite()) {
        return Err(Bad(
            "invalid surface-surface wire".into(),
        ));
    }
    let code = input.get(27).copied().unwrap_or(-1.0);
    if code < 0.0 || code.fract() != 0.0 || code >= 18446744073709551616.0 {
        return Err(Bad("invalid native ULONG".into()));
    }
    if input
        .get(28..30)
        .is_none_or(|f| f.iter().any(|v| *v != 0.0 && *v != 1.0))
    {
        return Err(Bad("invalid flag".into()));
    }
    Ok(())
}

/// Validate the owned-surface surface-surface wire: two surfaces' four corner points
/// (24 values), owner1 (0 free, 1 own sheet body), owner2 (0 free, 1 own sheet body,
/// 2 a second face of surface 1's body, only with owner1 1, 3 surface 1 itself),
/// radius1, radius2, tolerance, cross-section (native ULONG), mirror and complement
/// (0/1).
pub(super) fn validate_surface_surface_bodies(input: &[f64]) -> Result<(), Bad> {
    if input.len() != 32 || input.iter().any(|v| !v.is_finite()) {
        return Err(Bad(
            "invalid surface-surface bodies wire".into(),
        ));
    }
    let owner1 = input.get(24).copied().unwrap_or(-1.0);
    let owner2 = input.get(25).copied().unwrap_or(-1.0);
    if (owner1 != 0.0 && owner1 != 1.0)
        || ![0.0, 1.0, 2.0, 3.0].contains(&owner2)
        || (owner2 == 2.0 && owner1 != 1.0)
    {
        return Err(Bad("invalid surface owner".into()));
    }
    let code = input.get(29).copied().unwrap_or(-1.0);
    if code < 0.0 || code.fract() != 0.0 || code >= 18446744073709551616.0 {
        return Err(Bad("invalid native ULONG".into()));
    }
    if input
        .get(30..32)
        .is_none_or(|f| f.iter().any(|v| *v != 0.0 && *v != 1.0))
    {
        return Err(Bad("invalid flag".into()));
    }
    Ok(())
}

/// Number of fillet parameters on the [`attributed`] wire before the edge indices:
/// radius, end radius, cross-section, continuity, thumbweight, edge count.
pub(super) const ATTRIBUTED_FILLET_PARAMS: usize = 6;

/// Validate the attributed-fillet wire (issue #1462): api (0 `SmApiFilletEdges`,
/// 1 `SmApiFilletEdgesPerEdge`, 2 `SmApiVariableRadiusFillet`, 3 `SmApiCircularFillet`,
/// 4 `SmApiChamferFillet`), attribute behaviour (`SmAttributeBehaviorType` 0..=4), body
/// kind (0 box: origin and positive size; 2 cylinder, 3 box union, 4 box difference, 5 box
/// and cylinder union as on the [`real_source`] wire), [`SOURCE_PARAMS`] body parameters, then
/// [`ATTRIBUTED_FILLET_PARAMS`] and the `GetEdges` indices. The cross-section is a
/// `ULONG` 0..=2 for apis 0 and 2 and an `SmFilletSurfaceGeneratorType` 1..=3 for api 1;
/// continuity 1..=3. Apis 0..=2 take at least one edge, apis 3 and 4 none. The shim
/// rejects an index past the built body's edges.
pub(super) fn validate_attributed(input: &[f64]) -> Result<(), Bad> {
    let bad = |what: &str| Bad(format!("attributed fillet: {what}"));
    let int_in = |v: f64, lo: f64, hi: f64| v.is_finite() && v.fract() == 0.0 && v >= lo && v <= hi;
    if input.iter().any(|v| !v.is_finite()) {
        return Err(bad("non-finite value"));
    }
    let head = 3 + SOURCE_PARAMS + ATTRIBUTED_FILLET_PARAMS;
    let (Some(&api), Some(&behavior), Some(&kind)) = (input.first(), input.get(1), input.get(2))
    else {
        return Err(bad("short wire"));
    };
    if !int_in(api, 0.0, 4.0)
        || !int_in(behavior, 0.0, 4.0)
        || ![0.0, 2.0, 3.0, 4.0, 5.0].contains(&kind)
    {
        return Err(bad("api, behaviour or body kind"));
    }
    let p = input.get(3..3 + SOURCE_PARAMS).ok_or_else(|| bad("short wire"))?;
    let positive: &[usize] = match kind as u32 {
        0 => &[3, 4, 5],
        2 => &[3, 4],
        5 => &[3, 4, 5, 9, 10],
        _ => &[3, 4, 5, 9, 10, 11],
    };
    if positive.iter().any(|&i| p.get(i).is_none_or(|&v| v <= 1e-12)) {
        return Err(bad("non-positive body size"));
    }
    let f = input
        .get(3 + SOURCE_PARAMS..head)
        .ok_or_else(|| bad("short wire"))?;
    let &[_, _, xsect, continuity, _, count] = f else {
        return Err(bad("short wire"));
    };
    let xsect_ok = if api == 1.0 {
        int_in(xsect, 1.0, 3.0)
    } else {
        int_in(xsect, 0.0, 2.0)
    };
    if !xsect_ok || !int_in(continuity, 1.0, 3.0) {
        return Err(bad("cross-section or continuity"));
    }
    let indices = input.get(head..).unwrap_or_default();
    if !int_in(count, 0.0, MAX_SOURCE_EDGE as f64) || count as usize != indices.len() {
        return Err(bad("edge count"));
    }
    if (api <= 2.0) == indices.is_empty() {
        return Err(bad("edges for apis 0..=2 only"));
    }
    if indices.iter().any(|&i| !int_in(i, 0.0, MAX_SOURCE_EDGE as f64)) {
        return Err(bad("edge index"));
    }
    Ok(())
}

/// Validate the [`fillet_preview`] wire: source kind (0 a box at the origin, its size in
/// the first three parameters; 1 to 4 those of [`validate_real_source`]),
/// [`SOURCE_PARAMS`] source parameters, the radius, the selected-edge count and the
/// owning-list edge indices. Every value is finite; box, cylinder and source box sizes
/// are positive, as the constructors require. The radius is not limited:
/// `SmApiFilletPreview` does not check it.
pub(super) fn validate_fillet_preview(input: &[f64], has_path: bool) -> Result<(), Bad> {
    if input.iter().any(|v| !v.is_finite()) {
        return Err(Bad("non-finite preview wire".into()));
    }
    let mut r = Cursor::new(input);
    let kind = r.index(4)?;
    if (kind == 1) != has_path {
        return Err(Bad("invalid solid source".into()));
    }
    let params = r.slice(SOURCE_PARAMS)?;
    let positive: &[usize] = match kind {
        0 => &[0, 1, 2],
        2 => &[3, 4],
        3 | 4 => &[3, 4, 5, 9, 10, 11],
        _ => &[],
    };
    for &i in positive {
        if params.get(i).is_none_or(|v| *v <= 1e-12) {
            return Err(Bad("non-positive source size".into()));
        }
    }
    r.slice(1)?;
    let head = 1 + SOURCE_PARAMS + 2;
    let remaining = input.len().saturating_sub(head);
    let count = r.index(remaining)?;
    if count != remaining {
        return Err(Bad(
            "invalid edge selection length".into(),
        ));
    }
    for _ in 0..count {
        r.index(MAX_SOURCE_EDGE)?;
    }
    r.done()
}

/// Length of the [`two_offset_chamfer`] wire.
pub(super) const TWO_OFFSET_CHAMFER_WIRE: usize = 28;

/// Validate the [`two_offset_chamfer`] wire: cylinder radius and height; placement
/// origin, X axis and Y axis; cutting box origin and size; rim z; derive (1: `a`, `b`
/// are setbacks, 0: they are the two offsets); `a`, `b`; up; body (0: `SmApiCreateCylinder`,
/// 1: the capped linear sweep of two half-circle arcs); the first arc's start angle in
/// degrees; which matching rim edge (0 or 1); all (1: every matching rim edge chamfered in
/// one executive by the given offsets, which needs derive 0 and rim edge 0). Every value is
/// finite; the
/// cylinder and box sizes and `a`, `b` are positive; the axes are not parallel (a
/// degenerate placement is not a body this case builds) and, when deriving, `up` is not
/// zero. These are fixture contracts, not limits of the public API.
pub(super) fn validate_two_offset_chamfer(input: &[f64]) -> Result<(), Bad> {
    let bad = |what: &str| Err(Bad(what.into()));
    let Ok(w) = <&[f64; TWO_OFFSET_CHAMFER_WIRE]>::try_from(input) else {
        return bad("two-offset chamfer wire length");
    };
    if w.iter().any(|v| !v.is_finite()) {
        return bad("non-finite two-offset chamfer wire");
    }
    if [w[0], w[1], w[14], w[15], w[16], w[19], w[20]]
        .iter()
        .any(|v| *v <= 1e-12)
    {
        return bad("non-positive size or offset");
    }
    let (x, y) = ([w[5], w[6], w[7]], [w[8], w[9], w[10]]);
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if norm(cross) <= 1e-12 * norm(x) * norm(y) || norm(x) <= 1e-12 || norm(y) <= 1e-12 {
        return bad("degenerate placement axes");
    }
    if w[18] != 0.0 && w[18] != 1.0 {
        return bad("invalid derive flag");
    }
    if w[18] == 1.0 && norm([w[21], w[22], w[23]]) <= 1e-12 {
        return bad("zero up direction");
    }
    if w[24] != 0.0 && w[24] != 1.0 {
        return bad("invalid body kind");
    }
    if w[26] != 0.0 && w[26] != 1.0 {
        return bad("invalid rim index");
    }
    if w[27] != 0.0 && w[27] != 1.0 {
        return bad("invalid all-rims flag");
    }
    if w[27] == 1.0 && (w[18] != 0.0 || w[26] != 0.0) {
        return bad("all rims needs given offsets and rim 0");
    }
    Ok(())
}
