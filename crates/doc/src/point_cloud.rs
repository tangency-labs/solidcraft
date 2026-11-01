//! Point clouds: scanned points (a PLY or XYZ file) kept in the design as a reference to model
//! from (#5151). They are not bodies: nothing is built from them, they are drawn as points.
//! The points travel inside the design file (a binary part of the container, base64 in plain
//! JSON), so it reopens without the original file. They are single precision, relative to the
//! cloud's own `origin` (double precision), so a scan far from the design's origin keeps its
//! detail.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solidcraft_geom::Vec3;

use crate::canvas::{base64_decode, base64_encode};
pub use crate::point_cloud_complex::{Column, Complex, Filter, Floats, Indices, Keep, MAX_CELLS, MAX_COLUMNS, MAX_NAMES, Names, On};
use crate::{DocError, IDENTITY, Mat, Result};

/// Most points one cloud may hold.
pub const MAX_POINTS: usize = 20_000_000;
/// Largest point coordinate (mm) from the cloud's origin, and largest origin coordinate.
pub const MAX_COORD: f64 = 1e9;
/// Point sizes on screen (pixels).
pub const POINT_SIZE: std::ops::RangeInclusive<f64> = 0.5..=16.0;
/// Most filters on one cloud.
pub const MAX_FILTERS: usize = 8;
/// Properties every cloud has besides its file's: the coordinates in the design.
pub const COORDINATES: [&str; 3] = ["x", "y", "z"];
/// Length units a point cloud file may be in, and millimetres per unit.
pub const UNITS: [(&str, f64); 5] = [("mm", 1.0), ("cm", 10.0), ("m", 1000.0), ("in", 25.4), ("ft", 304.8)];

/// Millimetres per `unit` (one of [`UNITS`], any case).
pub fn unit_scale(unit: &str) -> Option<f64> {
    UNITS.iter().find(|(u, _)| u.eq_ignore_ascii_case(unit)).map(|(_, k)| *k)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointCloud {
    pub id: u64,
    pub name: String,
    /// The file it was read from (its name only).
    #[serde(default)]
    pub file: String,
    /// Where the points' own frame sits in the cloud's placement (mm).
    pub origin: Vec3,
    /// The points from `origin` (mm).
    pub points: Points,
    /// The file's colour of each point, when it has them.
    #[serde(default, skip_serializing_if = "Colors::is_empty")]
    pub colors: Colors,
    /// Points the file had (more than `points` when the cloud was thinned as it came in).
    #[serde(default)]
    pub source_points: u64,
    /// The file's length unit (one of [`UNITS`]): the points were scaled from it to millimetres.
    #[serde(default = "mm")]
    pub units: String,
    /// Where the cloud sits in the design: a rigid move of its points.
    #[serde(default = "identity", skip_serializing_if = "is_identity")]
    pub transform: Mat,
    /// Size of a point on screen (pixels).
    #[serde(default = "two")]
    pub point_size: f64,
    /// How the points are coloured.
    #[serde(default, skip_serializing_if = "ColorBy::is_file")]
    pub color_by: ColorBy,
    /// The colour of every point with [`ColorBy::Single`] (straight sRGB, 0..1; `None`: [`PLAIN`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
    /// The heights (design Z, mm) the [`ColorBy::Height`] ramp runs between, lowest first;
    /// `None`: worked out from the points ([`PointCloud::auto_height_range`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_range: Option<[f64; 2]>,
    #[serde(default = "yes")]
    pub visible: bool,
    /// Segments and triangles over the points, and the file's other properties.
    #[serde(default, skip_serializing_if = "Complex::is_empty")]
    pub complex: Complex,
    /// The property `color_by: property` colours by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_property: Option<String>,
    /// Only the elements within every filter's range are shown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Filter>,
}

/// Where a property's values come from on one kind of element.
#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
    /// A coordinate of each corner in the design.
    Coordinate(usize),
    /// A column on the points: each corner's.
    Corners(&'a [f32]),
    /// A column on the element itself.
    Element(&'a [f32]),
}

/// How a cloud's points are coloured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorBy {
    /// The file's colour of each point ([`PLAIN`] when it has none).
    #[default]
    File,
    /// One colour for every point ([`PointCloud::color`]).
    Single,
    /// By height in the design: blue low, through cyan, green and yellow, to red high.
    Height,
    /// By `color_property`: the height ramp over its range or, for whole numbers, a palette.
    Property,
}

impl ColorBy {
    fn is_file(&self) -> bool {
        *self == ColorBy::File
    }
    /// From its name (`file`, `single`, `height`).
    pub fn parse(s: &str) -> Option<ColorBy> {
        match s {
            "file" => Some(ColorBy::File),
            "single" => Some(ColorBy::Single),
            "height" => Some(ColorBy::Height),
            "property" => Some(ColorBy::Property),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ColorBy::File => "file",
            ColorBy::Single => "single",
            ColorBy::Height => "height",
            ColorBy::Property => "property",
        }
    }
}

/// The colour of points that have no other (straight sRGB, 0..1).
pub const PLAIN: [f32; 3] = [0.59, 0.63, 0.69];
/// Points sampled for the automatic height range.
const HEIGHT_SAMPLES: usize = 200_000;
/// The share of points left below and above the automatic height range, so a few stray points
/// don't flatten the ramp.
const HEIGHT_TAIL: f64 = 0.02;

/// A colour of the height ramp at `t` (0: low, 1: high): blue, cyan, green, yellow, red.
pub fn height_ramp(t: f64) -> [u8; 3] {
    const STOPS: [[f64; 3]; 5] = [[40.0, 70.0, 200.0], [30.0, 170.0, 230.0], [60.0, 200.0, 90.0], [240.0, 220.0, 60.0], [230.0, 70.0, 50.0]];
    let k = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 } * (STOPS.len() - 1) as f64;
    let i = (k as usize).min(STOPS.len() - 2);
    let f = k - i as f64;
    let (a, b) = (STOPS[i], STOPS[i + 1]);
    std::array::from_fn(|c| (a[c] * (1.0 - f) + b[c] * f).round().clamp(0.0, 255.0) as u8)
}

/// Colours of whole-number labels, the label modulo their count; negative labels are grey.
pub const PALETTE: [[u8; 3]; 10] = [
    [240, 140, 40],
    [245, 205, 65],
    [230, 65, 65],
    [165, 75, 190],
    [65, 190, 115],
    [75, 150, 220],
    [240, 120, 180],
    [140, 105, 75],
    [120, 215, 215],
    [190, 190, 80],
];

pub fn label_color(v: f64) -> [u8; 3] {
    if !(v.is_finite() && v >= 0.0) {
        return PLAIN.map(byte);
    }
    PALETTE[(v.round() as usize) % PALETTE.len()]
}

/// A colour channel (0..1) as a byte.
pub fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn identity() -> Mat {
    IDENTITY
}
fn is_identity(m: &Mat) -> bool {
    *m == IDENTITY
}
fn mm() -> String {
    "mm".into()
}
fn two() -> f64 {
    2.0
}
fn yes() -> bool {
    true
}

impl PointCloud {
    pub fn len(&self) -> usize {
        self.points.len()
    }
    pub fn is_empty(&self) -> bool {
        self.points.len() == 0
    }
    /// The `i`th point in the design (mm).
    pub fn world_point(&self, i: usize) -> Option<Vec3> {
        let p = self.points.get(i)?;
        let local = self.origin + Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        Some(crate::apply_point(&self.transform, local))
    }
    /// The corners of the box around the points, in the cloud's placement (mm), or `None` for
    /// an empty cloud.
    pub fn local_bounds(&self) -> Option<(Vec3, Vec3)> {
        let (lo, hi) = self.points.bounds()?;
        let v = |p: [f32; 3]| self.origin + Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        Some((v(lo), v(hi)))
    }
    /// The design Z (mm) of a point from the cloud's origin.
    fn world_z(&self, p: [f32; 3]) -> f64 {
        let m = &self.transform;
        let l = [self.origin.x + f64::from(p[0]), self.origin.y + f64::from(p[1]), self.origin.z + f64::from(p[2])];
        m[0][2] * l[0] + m[1][2] * l[1] + m[2][2] * l[2] + m[3][2]
    }
    /// The heights (design Z, mm) holding all but the lowest and highest 2 % of the points (an
    /// even sample of them), or `None` for an empty cloud. A flat cloud gets a range 1 mm high.
    pub fn auto_height_range(&self) -> Option<[f64; 2]> {
        let n = self.points.len();
        if n == 0 {
            return None;
        }
        let step = n.div_ceil(HEIGHT_SAMPLES).max(1);
        let mut z: Vec<f64> = (0..n).step_by(step).filter_map(|i| self.points.get(i)).map(|p| self.world_z(p)).collect();
        z.sort_by(f64::total_cmp);
        let last = z.len().checked_sub(1)?;
        let at = |q: f64| z.get((last as f64 * q).round() as usize).copied();
        let (lo, hi) = (at(HEIGHT_TAIL)?, at(1.0 - HEIGHT_TAIL)?);
        Some(if hi - lo > 1e-9 { [lo, hi] } else { [lo - 0.5, lo + 0.5] })
    }
    /// The property colours come from: a column, or a coordinate (`z` for the height ramp).
    pub fn colored_property(&self) -> Option<&str> {
        match self.color_by {
            ColorBy::Height => Some("z"),
            ColorBy::Property => self.color_property.as_deref(),
            ColorBy::File | ColorBy::Single => None,
        }
    }
    /// Where the values of `name` are on elements of kind `on`: a coordinate of each corner, a
    /// column on the points (each corner's, varying across the element) or one on the element.
    pub fn source(&self, name: &str, on: On) -> Option<Source<'_>> {
        if let Some(k) = COORDINATES.iter().position(|n| *n == name) {
            return Some(Source::Coordinate(k));
        }
        if let Some(col) = self.complex.column(name, on) {
            return Some(Source::Element(&col.values.0));
        }
        self.complex.column(name, On::Point).map(|col| Source::Corners(&col.values.0))
    }
    /// Every property the cloud can be coloured and filtered by: its coordinates, then its columns.
    pub fn property_names(&self) -> impl Iterator<Item = &str> {
        let mut seen = std::collections::BTreeSet::new();
        COORDINATES.into_iter().chain(self.complex.columns.iter().map(|c| c.name.as_str())).filter(move |n| seen.insert(*n))
    }
    /// The smallest and largest value of a property, the coordinates in the design.
    pub fn span(&self, property: &str) -> Option<[f64; 2]> {
        if let Some(span) = self.complex.span(property) {
            return Some(span);
        }
        let k = COORDINATES.iter().position(|n| *n == property)?;
        let v = self.world_corners()?.map(|p| [p.x, p.y, p.z][k]);
        Some([v.iter().copied().fold(f64::INFINITY, f64::min), v.iter().copied().fold(f64::NEG_INFINITY, f64::max)])
    }
    /// The ramp's range as far as it can be told without the elements: the one set, or else the
    /// coloured property's span within every range filtering it. The automatic range colouring
    /// by a property is exactly what the filters keep
    /// ([`crate::point_cloud_distribution::color_range`]), which this bounds.
    pub fn color_range(&self) -> Option<[f64; 2]> {
        if self.height_range.is_some() {
            return self.height_range;
        }
        let property = self.colored_property()?;
        let span = if self.color_by == ColorBy::Height { self.auto_height_range() } else { self.span(property) }?;
        let shown =
            self.filters.iter().filter(|f| f.property == property).filter_map(Filter::range).fold(span, |[lo, hi], [a, b]| [lo.max(a), hi.min(b)]);
        Some(if shown[0] <= shown[1] { crate::point_cloud_distribution::ramp_range(shown) } else { span })
    }
    /// Points no segment or triangle ends on: those drawn as dots.
    pub fn isolated_points(&self) -> usize {
        if self.complex.segments.is_empty() && self.complex.triangles.is_empty() {
            return self.len();
        }
        let mut used = vec![false; self.len()];
        for i in self.complex.segments.0.iter().chain(self.complex.triangles.0.iter()) {
            if let Some(u) = used.get_mut(*i as usize) {
                *u = true;
            }
        }
        used.iter().filter(|u| !**u).count()
    }
    /// The heights the height ramp runs between now: the cloud's own, else the automatic ones.
    pub fn height_range_now(&self) -> Option<[f64; 2]> {
        self.height_range.or_else(|| self.auto_height_range())
    }
    /// The colour of each point (by index) as the cloud is shown, the ramp over `range`.
    pub fn shade(&self, range: Option<[f64; 2]>) -> impl Fn(usize) -> [u8; 3] + '_ {
        let single = self.color.unwrap_or(PLAIN).map(byte);
        let plain = PLAIN.map(byte);
        let column = self.color_property.as_deref().and_then(|n| self.complex.column(n, On::Point));
        move |i| match self.color_by {
            ColorBy::File => self.colors.get(i).unwrap_or(plain),
            ColorBy::Single => single,
            ColorBy::Height => match (range, self.points.get(i)) {
                (Some([lo, hi]), Some(p)) => height_ramp((self.world_z(p) - lo) / (hi - lo)),
                _ => plain,
            },
            ColorBy::Property => match (column, column.and_then(|c| c.values.0.get(i))) {
                (Some(c), Some(v)) if c.integer => label_color(f64::from(*v)),
                (Some(_), Some(v)) => match range {
                    Some([lo, hi]) if hi > lo => height_ramp((f64::from(*v) - lo) / (hi - lo)),
                    _ => plain,
                },
                _ => plain,
            },
        }
    }
    /// The corners of the box around the points, placed in the design (mm).
    pub fn world_corners(&self) -> Option<[Vec3; 8]> {
        let (lo, hi) = self.local_bounds()?;
        Some(std::array::from_fn(|i| {
            let q = Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
            crate::apply_point(&self.transform, q)
        }))
    }
    /// The cloud is whole: as many colours as points, finite coordinates within range, a size
    /// on screen in range and a rigid placement.
    pub fn check(&self) -> Result<()> {
        let bad = |what: &str| Err(DocError::Invalid(format!("point cloud `{}`: {what}", self.name)));
        if self.points.len() > MAX_POINTS {
            return bad("too many points");
        }
        if !self.colors.is_empty() && self.colors.len() != self.points.len() {
            return bad("its colours do not match its points");
        }
        if ![self.origin.x, self.origin.y, self.origin.z].iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
            return bad("its origin is out of range");
        }
        if self.points.values().iter().any(|v| !v.is_finite() || f64::from(v.abs()) > MAX_COORD) {
            return bad("a point is out of range");
        }
        if unit_scale(&self.units).is_none() {
            return bad("its units are not mm, cm, m, in or ft");
        }
        if !(self.point_size.is_finite() && POINT_SIZE.contains(&self.point_size)) {
            return bad("its point size is out of range");
        }
        if self.color.is_some_and(|c| c.iter().any(|v| !(0.0..=1.0).contains(v))) {
            return bad("its colour is out of range");
        }
        if self.height_range.is_some_and(|[lo, hi]| !(lo.is_finite() && hi.is_finite() && lo < hi && lo.abs() <= MAX_COORD && hi.abs() <= MAX_COORD))
        {
            return bad("its height range is not two heights, lowest first");
        }
        if !rigid(&self.transform) {
            return bad("its placement is not a rigid move");
        }
        self.complex.check(self.points.len())?;
        if self.filters.len() > MAX_FILTERS
            || self.filters.iter().any(|f| match &f.keep {
                Keep::Range { range: [lo, hi] } => !(lo.is_finite() && hi.is_finite() && lo <= hi),
                Keep::Values { values } => values.len() > MAX_NAMES,
            })
        {
            return bad("its filters are not ranges, lowest first");
        }
        if self.color_by == ColorBy::Property && self.color_property.is_none() {
            return bad("it is coloured by a property without naming it");
        }
        Ok(())
    }
}

/// A rotation and a translation within range (no scale, shear or mirror).
pub fn rigid(m: &Mat) -> bool {
    let column = |i: usize| Vec3::new(m[i][0], m[i][1], m[i][2]);
    let (x, y, z) = (column(0), column(1), column(2));
    m.iter().flatten().all(|v| v.is_finite())
        && m[3][..3].iter().all(|v| v.abs() <= MAX_COORD)
        && [x, y, z].iter().all(|axis| (axis.dot(*axis) - 1.0).abs() < 1e-6)
        && x.dot(y).abs() < 1e-6
        && x.dot(z).abs() < 1e-6
        && y.dot(z).abs() < 1e-6
        && x.cross(y).dot(z) > 0.0
        && [m[0][3], m[1][3], m[2][3]].iter().all(|v| v.abs() < 1e-9)
        && (m[3][3] - 1.0).abs() < 1e-9
}

/// Corners of the box around some points.
type Corners = Option<([f32; 3], [f32; 3])>;

/// x, y, z per point (single precision, little-endian when stored). Shared, so undo states and
/// copies of the design don't copy the points; the box around them is worked out once.
#[derive(Clone, Debug, Default)]
pub struct Points {
    values: Arc<[f32]>,
    bounds: Arc<std::sync::OnceLock<Corners>>,
}

impl Points {
    pub fn new(values: Arc<[f32]>) -> Points {
        Points { values, bounds: Default::default() }
    }
    /// x, y, z per point.
    pub fn values(&self) -> &Arc<[f32]> {
        &self.values
    }
    pub fn len(&self) -> usize {
        self.values.len() / 3
    }
    pub fn is_empty(&self) -> bool {
        self.values.len() < 3
    }
    pub fn get(&self, i: usize) -> Option<[f32; 3]> {
        let s = self.values.get(i.checked_mul(3)?..i.checked_mul(3)?.checked_add(3)?)?;
        Some([*s.first()?, *s.get(1)?, *s.get(2)?])
    }
    pub fn iter(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        self.values.as_chunks::<3>().0.iter().copied()
    }
    /// The lowest and highest x, y and z, or `None` without points.
    pub fn bounds(&self) -> Corners {
        *self.bounds.get_or_init(|| {
            let mut it = self.iter();
            let first = it.next()?;
            Some(it.fold((first, first), |(mut lo, mut hi), p| {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
                (lo, hi)
            }))
        })
    }
    /// The points as stored: little-endian f32, x, y, z per point.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.values.len() * 4);
        for v in self.values.iter() {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
    /// The points from stored bytes; `None` unless whole points of finite coordinates.
    pub fn from_bytes(b: &[u8]) -> Option<Points> {
        if !b.len().is_multiple_of(12) || b.len() / 12 > MAX_POINTS {
            return None;
        }
        let v: Vec<f32> = b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
        v.iter().all(|x| x.is_finite()).then(|| Points::new(v.into()))
    }
}

/// The same points: the same shared storage, or equal values.
impl PartialEq for Points {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.values, &other.values) || self.values == other.values
    }
}

impl Serialize for Points {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&base64_encode(&self.to_bytes()))
    }
}

impl<'de> Deserialize<'de> for Points {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() > (MAX_POINTS * 12).div_ceil(3) * 4 + 16 {
            return Err(serde::de::Error::custom("too many points"));
        }
        let b = base64_decode(&s).ok_or_else(|| serde::de::Error::custom("points are not base64"))?;
        Points::from_bytes(&b).ok_or_else(|| serde::de::Error::custom("points are not whole, finite x, y, z"))
    }
}

/// r, g, b per point (bytes).
#[derive(Clone, Debug, Default)]
pub struct Colors(pub Arc<[u8]>);

impl Colors {
    pub fn len(&self) -> usize {
        self.0.len() / 3
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn get(&self, i: usize) -> Option<[u8; 3]> {
        let s = self.0.get(i.checked_mul(3)?..i.checked_mul(3)?.checked_add(3)?)?;
        Some([*s.first()?, *s.get(1)?, *s.get(2)?])
    }
    pub fn from_bytes(b: &[u8]) -> Option<Colors> {
        (b.len().is_multiple_of(3) && b.len() / 3 <= MAX_POINTS).then(|| Colors(b.into()))
    }
}

impl PartialEq for Colors {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl Serialize for Colors {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&base64_encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Colors {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() > (MAX_POINTS * 3).div_ceil(3) * 4 + 16 {
            return Err(serde::de::Error::custom("too many colours"));
        }
        let b = base64_decode(&s).ok_or_else(|| serde::de::Error::custom("colours are not base64"))?;
        Colors::from_bytes(&b).ok_or_else(|| serde::de::Error::custom("colours are not whole r, g, b"))
    }
}

/// Every cloud of a design is whole ([`PointCloud::check`]) and has its own id.
pub fn check_all(clouds: &[PointCloud]) -> Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    for c in clouds {
        c.check()?;
        if !ids.insert(c.id) {
            return Err(DocError::Invalid(format!("two point clouds have the id {}", c.id)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud(points: Vec<f32>, colors: Vec<u8>) -> PointCloud {
        PointCloud {
            id: 7,
            name: "Scan".into(),
            file: "scan.ply".into(),
            origin: Vec3::new(1000.0, 0.0, 0.0),
            points: Points::new(points.into()),
            colors: Colors(colors.into()),
            source_points: 2,
            units: "m".into(),
            transform: IDENTITY,
            point_size: 2.0,
            color_by: ColorBy::File,
            color: None,
            height_range: None,
            visible: true,
            complex: Complex::default(),
            color_property: None,
            filters: Vec::new(),
        }
    }

    #[test]
    fn points_round_trip_through_json_and_stay_shared() {
        let c = cloud(vec![0.0, 1.0, 2.0, -3.5, 4.25, 1e6], vec![255, 0, 10, 1, 2, 3]);
        let json = serde_json::to_string(&c).unwrap();
        let back: PointCloud = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.points.get(1), Some([-3.5, 4.25, 1e6]));
        assert_eq!(back.colors.get(0), Some([255, 0, 10]));
        assert_eq!(back.world_point(0), Some(Vec3::new(1000.0, 1.0, 2.0)));
        assert_eq!(back.points.get(2), None);
        let copy = c.clone();
        assert!(Arc::ptr_eq(copy.points.values(), c.points.values()), "a copy shares the points");
        let (lo, hi) = c.local_bounds().unwrap();
        assert_eq!((lo, hi), (Vec3::new(996.5, 1.0, 2.0), Vec3::new(1000.0, 4.25, 1e6)));
        check_all(&[c]).unwrap();
        assert_eq!((unit_scale("M"), unit_scale("yd")), (Some(1000.0), None));
    }

    #[test]
    fn the_ramp_spans_what_is_shown_unless_set_and_isolated_points_are_counted() {
        let mut c = cloud(vec![0.0; 9], Vec::new());
        c.complex.segments = Indices(vec![0, 1].into());
        c.complex.columns.push(Column { name: "t".into(), on: On::Point, integer: false, values: Floats(vec![0.0, 10.0, 20.0].into()) });
        (c.color_by, c.color_property) = (ColorBy::Property, Some("t".into()));
        assert_eq!(c.color_range(), Some([0.0, 20.0]), "the whole span");
        c.filters = vec![Filter { property: "t".into(), keep: Keep::Range { range: [5.0, 50.0] } }];
        assert_eq!(c.color_range(), Some([5.0, 20.0]), "the span within the filter");
        c.filters.push(Filter { property: "layer".into(), keep: Keep::Range { range: [0.0, 1.0] } });
        assert_eq!(c.color_range(), Some([5.0, 20.0]), "another property's filter changes nothing");
        c.height_range = Some([1.0, 2.0]);
        assert_eq!(c.color_range(), Some([1.0, 2.0]), "a range set is kept");
        assert_eq!(c.isolated_points(), 1, "point 2 ends no segment");
    }

    #[test]
    fn damaged_clouds_are_refused() {
        // Not whole points, not finite, not base64.
        for points in ["AAAA", "AADAfwAAwH8AAMB/", "!!!!"] {
            let json = serde_json::to_string(&cloud(vec![0.0; 3], Vec::new())).unwrap().replace("\"AAAAAAAAAAAAAAAA\"", &format!("\"{points}\""));
            assert!(serde_json::from_str::<PointCloud>(&json).is_err(), "{points}");
        }
        assert!(cloud(vec![0.0; 6], vec![1, 2, 3]).check().is_err(), "one colour for two points");
        let mut c = cloud(vec![0.0; 3], Vec::new());
        c.point_size = 100.0;
        assert!(c.check().is_err());
        let mut c = cloud(vec![0.0; 3], Vec::new());
        c.transform[0][0] = 2.0;
        assert!(c.check().is_err(), "a scale is not a placement");
        let c = cloud(vec![0.0; 3], Vec::new());
        assert!(check_all(&[c.clone(), c]).is_err(), "two clouds with one id");
        for range in [[5.0, 5.0], [9.0, 1.0], [f64::NAN, 1.0], [0.0, 1e12]] {
            let mut c = cloud(vec![0.0; 3], Vec::new());
            c.height_range = Some(range);
            assert!(c.check().is_err(), "{range:?}");
        }
    }

    #[test]
    fn points_are_coloured_by_the_file_one_colour_or_their_height() {
        // Heights 0..=100 mm, and a stray point far above the rest.
        let mut values: Vec<f32> = (0..=100).flat_map(|z| [0.0, 0.0, z as f32]).collect();
        values.extend([0.0, 0.0, 1000.0]);
        let colors: Vec<u8> = (0..102).flat_map(|i| [i as u8, 7, 9]).collect();
        let mut c = cloud(values, colors);
        c.origin = Vec3::ZERO;
        assert_eq!(c.shade(c.color_range())(3), [3, 7, 9], "the file's colours");
        c.colors = Colors::default();
        assert_eq!(c.shade(c.color_range())(3), PLAIN.map(byte), "none: plain");
        c.color_by = ColorBy::Single;
        c.color = Some([1.0, 0.5, 0.0]);
        assert_eq!(c.shade(c.color_range())(50), [255, 128, 0]);
        // The stray point doesn't stretch the automatic range.
        c.color_by = ColorBy::Height;
        // 102 points: the 2nd and the 99th (of 0…101) once sorted.
        assert_eq!(c.auto_height_range(), Some([2.0, 99.0]));
        {
            let shade = c.shade(c.color_range());
            assert_eq!((shade(0), shade(100), shade(101)), (height_ramp(0.0), height_ramp(1.0), height_ramp(1.0)));
            assert_eq!(shade(50), height_ramp(48.0 / 97.0));
        }
        // A range of its own; moved up 100 mm, everything is above it.
        c.height_range = Some([0.0, 50.0]);
        assert_eq!(c.shade(c.color_range())(25), height_ramp(0.5));
        c.transform[3][2] = 100.0;
        assert_eq!(c.shade(c.color_range())(0), height_ramp(1.0));
        assert_eq!((height_ramp(0.0), height_ramp(1.0), height_ramp(f64::NAN)), ([40, 70, 200], [230, 70, 50], [40, 70, 200]));
        let back: PointCloud = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}
