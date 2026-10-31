//! What a point cloud holds besides its points: segments and triangles between them, and named
//! numbers on its points, segments and triangles (a PLY file's properties), to colour the cloud
//! by and to filter it with. A point's numbers vary along the segments and across the triangles
//! that meet there; a segment's or a triangle's are constant on it. A segment with `width` and
//! `height` numbers is drawn as a bead of that cross-section; without them, as a line. Whole
//! numbers may have names (`role: 0 outer wall, 1 inner wall` in a PLY file's comments): they
//! are labels, filtered by a set of values rather than a range.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::canvas::{base64_decode, base64_encode};
use crate::point_cloud::MAX_POINTS;
use crate::{DocError, Result};

/// Most segments or triangles in one cloud.
pub const MAX_CELLS: usize = 20_000_000;
/// Most named numbers per point, segment and triangle together.
pub const MAX_COLUMNS: usize = 64;

/// The elements a column of numbers is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum On {
    #[default]
    Point,
    Segment,
    Triangle,
}

impl On {
    pub fn name(self) -> &'static str {
        match self {
            On::Point => "point",
            On::Segment => "segment",
            On::Triangle => "triangle",
        }
    }
}

/// One named number on every element of a kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    /// Points unless said: a design saved with numbers on points alone says nothing.
    #[serde(default)]
    pub on: On,
    /// Whole numbers (labels, counts): coloured by a palette rather than a ramp.
    #[serde(default)]
    pub integer: bool,
    pub values: Floats,
}

impl Column {
    /// The smallest and largest finite value.
    pub fn span(&self) -> Option<[f64; 2]> {
        let mut it = self.values.0.iter().filter(|v| v.is_finite()).map(|v| f64::from(*v));
        let first = it.next()?;
        Some(it.fold([first, first], |[lo, hi], v| [lo.min(v), hi.max(v)]))
    }
}

/// The complex a cloud's points span: segments and triangles over them, and named numbers on
/// all three. Without segments or triangles, the points alone are a complex of dimension 0.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Complex {
    /// Point indices, two per segment.
    #[serde(default, skip_serializing_if = "Indices::is_empty")]
    pub segments: Indices,
    /// Point indices, three per triangle, counter-clockwise seen from its front.
    #[serde(default, skip_serializing_if = "Indices::is_empty")]
    pub triangles: Indices,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<Column>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<Names>,
}

/// Most named values of one label.
pub const MAX_NAMES: usize = 256;

/// The names of a label's values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Names {
    pub property: String,
    pub values: Vec<(i32, String)>,
}

impl Names {
    /// `property: 0 name, 1 another name, -1 none`, or `None` for any other text.
    pub fn parse(text: &str) -> Option<Names> {
        let (property, rest) = text.split_once(':')?;
        let property = property.trim();
        if property.is_empty() || property.contains(char::is_whitespace) {
            return None;
        }
        let values = rest
            .split(',')
            .map(|entry| {
                let (value, name) = entry.trim().split_once(' ')?;
                let name = name.trim();
                (!name.is_empty()).then(|| Some((value.parse::<i32>().ok()?, name.to_string()))).flatten()
            })
            .collect::<Option<Vec<_>>>()?;
        (!values.is_empty() && values.len() <= MAX_NAMES).then(|| Names { property: property.to_string(), values })
    }
    pub fn name(&self, value: i32) -> Option<&str> {
        self.values.iter().find(|(v, _)| *v == value).map(|(_, n)| n.as_str())
    }
}

impl Complex {
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty() && self.triangles.is_empty() && self.columns.is_empty() && self.names.is_empty()
    }
    pub fn segment_count(&self) -> usize {
        self.segments.0.len() / 2
    }
    pub fn triangle_count(&self) -> usize {
        self.triangles.0.len() / 3
    }
    /// The column named `name` on elements of kind `on`: a name may be on several kinds.
    pub fn column(&self, name: &str, on: On) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name && c.on == on)
    }
    /// The smallest and largest value named `name`, on whichever elements carry it.
    pub fn span(&self, name: &str) -> Option<[f64; 2]> {
        self.columns.iter().filter(|c| c.name == name).filter_map(Column::span).reduce(|[a, b], [c, d]| [a.min(c), b.max(d)])
    }
    /// The names of `property`'s values, when it is a label.
    pub fn names(&self, property: &str) -> Option<&Names> {
        self.names.iter().find(|n| n.property == property).filter(|_| self.integer(property))
    }
    /// Whether the values named `name` are whole numbers wherever they are.
    pub fn integer(&self, name: &str) -> bool {
        let mut named = self.columns.iter().filter(|c| c.name == name).peekable();
        named.peek().is_some() && named.all(|c| c.integer)
    }
    /// How many elements of a kind there are, the cloud having `points` points.
    pub fn count(&self, on: On, points: usize) -> usize {
        match on {
            On::Point => points,
            On::Segment => self.segment_count(),
            On::Triangle => self.triangle_count(),
        }
    }
    /// Every index names a point, and every column has one number per element.
    pub fn check(&self, points: usize) -> Result<()> {
        let bad = |what: String| Err(DocError::Invalid(format!("point cloud complex: {what}")));
        if !self.segments.0.len().is_multiple_of(2) || !self.triangles.0.len().is_multiple_of(3) {
            return bad("segments are not pairs or triangles not triples".into());
        }
        if self.segment_count() > MAX_CELLS || self.triangle_count() > MAX_CELLS || self.columns.len() > MAX_COLUMNS {
            return bad("too many segments, triangles or properties".into());
        }
        if self.segments.0.iter().chain(self.triangles.0.iter()).any(|i| *i as usize >= points) {
            return bad("an index names a missing point".into());
        }
        for c in &self.columns {
            if c.values.0.len() != self.count(c.on, points) {
                return bad(format!("property `{}` does not have one number per {}", c.name, c.on.name()));
            }
        }
        if self.names.len() > MAX_COLUMNS || self.names.iter().any(|n| n.values.is_empty() || n.values.len() > MAX_NAMES) {
            return bad("too many named values".into());
        }
        Ok(())
    }
}

/// Which elements are shown: those whose property lies in a range (both ends included), or, for
/// a label, is one of a set of values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub property: String,
    #[serde(flatten)]
    pub keep: Keep,
}

/// The values a filter keeps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Keep {
    Range { range: [f64; 2] },
    Values { values: Vec<i32> },
}

impl Filter {
    pub fn range(&self) -> Option<[f64; 2]> {
        match &self.keep {
            Keep::Range { range } => Some(*range),
            Keep::Values { .. } => None,
        }
    }
}

/// Indices into a cloud's points, shared between copies of the design.
#[derive(Clone, Debug, Default)]
pub struct Indices(pub Arc<[u32]>);

impl Indices {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Numbers, shared between copies of the design.
#[derive(Clone, Debug, Default)]
pub struct Floats(pub Arc<[f32]>);

impl PartialEq for Indices {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl PartialEq for Floats {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.iter().map(|v| v.to_bits()).eq(other.0.iter().map(|v| v.to_bits()))
    }
}

/// The largest base64 text of `n` four-byte words.
fn base64_len(n: usize) -> usize {
    (n * 4).div_ceil(3) * 4 + 16
}

fn words<'de, D: Deserializer<'de>>(d: D, max: usize) -> std::result::Result<Vec<[u8; 4]>, D::Error> {
    let s = String::deserialize(d)?;
    if s.len() > base64_len(max) {
        return Err(serde::de::Error::custom("too many numbers"));
    }
    let b = base64_decode(&s).ok_or_else(|| serde::de::Error::custom("numbers are not base64"))?;
    if !b.len().is_multiple_of(4) {
        return Err(serde::de::Error::custom("numbers are not whole four-byte words"));
    }
    Ok(b.as_chunks::<4>().0.to_vec())
}

impl Serialize for Indices {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&base64_encode(&self.0.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()))
    }
}

impl<'de> Deserialize<'de> for Indices {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Ok(Indices(words(d, 3 * MAX_CELLS)?.into_iter().map(u32::from_le_bytes).collect()))
    }
}

impl Serialize for Floats {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&base64_encode(&self.0.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()))
    }
}

impl<'de> Deserialize<'de> for Floats {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Ok(Floats(words(d, MAX_POINTS.max(MAX_CELLS))?.into_iter().map(f32::from_le_bytes).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_complex_round_trips_and_is_checked_against_its_points() {
        let complex = Complex {
            segments: Indices(vec![0, 1, 1, 2].into()),
            triangles: Indices(vec![0, 1, 2].into()),
            columns: vec![
                Column { name: "t".into(), on: On::Point, integer: false, values: Floats(vec![0.0, 0.5, 1.0].into()) },
                Column { name: "role".into(), on: On::Segment, integer: true, values: Floats(vec![0.0, 4.0].into()) },
            ],
            names: Vec::new(),
        };
        let back: Complex = serde_json::from_str(&serde_json::to_string(&complex).unwrap()).unwrap();
        assert_eq!(back, complex);
        let points: Column = serde_json::from_str(r#"{"name": "t", "values": ""}"#).unwrap();
        assert_eq!(points.on, On::Point, "a column without `on` is on the points");
        complex.check(3).unwrap();
        assert!(complex.check(2).is_err(), "index 2 names a missing point");
        assert_eq!(complex.span("role"), Some([0.0, 4.0]));
        assert!(complex.integer("role") && !complex.integer("t") && !complex.integer("nope"));
        assert!(complex.column("role", On::Segment).is_some() && complex.column("role", On::Point).is_none());
        // Named values make a label, and a filter keeps a range or a set of them.
        let names = Names::parse("role: 0 outer wall, 4 bridge, -1 none").unwrap();
        assert_eq!((names.name(4), names.name(-1), names.name(2)), (Some("bridge"), Some("none"), None));
        for text in ["t: seconds since the start", "role 0 wall", ": 0 a", "role: x wall", "role: 0 "] {
            assert!(Names::parse(text).is_none(), "{text}");
        }
        let mut named = complex.clone();
        named.names = vec![names, Names::parse("t: 0 zero").unwrap()];
        assert!(named.names("role").is_some() && named.names("t").is_none(), "only whole numbers are labels");
        let set: Filter = serde_json::from_str(r#"{"property": "role", "values": [0, 4]}"#).unwrap();
        let range: Filter = serde_json::from_str(r#"{"property": "t", "range": [1, 3]}"#).unwrap();
        assert_eq!((set.range(), range.range()), (None, Some([1.0, 3.0])));
        assert_eq!(serde_json::to_value(&set).unwrap(), serde_json::json!({"property": "role", "values": [0, 4]}));
        let mut short = complex.clone();
        short.columns[1].values = Floats(vec![0.0].into());
        assert!(short.check(3).is_err(), "one role for two segments");
    }
}
