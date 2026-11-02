//! What a point cloud holds besides its points' places and colours: named numbers on its points
//! (a PLY file's other vertex properties), to colour the cloud by and to filter it with. Whole
//! numbers may have names (`role: 0 outer wall, 1 inner wall` in a PLY file's comments): they
//! are labels, filtered by a set of values rather than a range.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::canvas::{base64_decode, base64_encode};
use crate::point_cloud::MAX_POINTS;
use crate::{DocError, Result};

/// Most named numbers per point.
pub const MAX_COLUMNS: usize = 64;

/// One named number on every point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
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

/// The complex a cloud's points form: named numbers on the points, and the names of
/// whole-numbered ones. Points alone are a complex of dimension 0.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Complex {
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
        self.columns.is_empty() && self.names.is_empty()
    }
    /// The column named `name`.
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }
    /// The smallest and largest value named `name`.
    pub fn span(&self, name: &str) -> Option<[f64; 2]> {
        self.column(name).and_then(Column::span)
    }
    /// The names of `property`'s values, when it is a label.
    pub fn names(&self, property: &str) -> Option<&Names> {
        self.names.iter().find(|n| n.property == property).filter(|_| self.integer(property))
    }
    /// Whether the values named `name` are whole numbers.
    pub fn integer(&self, name: &str) -> bool {
        self.column(name).is_some_and(|c| c.integer)
    }
    /// Every column has one number per point, the cloud having `points` points.
    pub fn check(&self, points: usize) -> Result<()> {
        let bad = |what: String| Err(DocError::Invalid(format!("point cloud properties: {what}")));
        if self.columns.len() > MAX_COLUMNS {
            return bad("too many properties".into());
        }
        for c in &self.columns {
            if c.values.0.len() != points {
                return bad(format!("property `{}` does not have one number per point", c.name));
            }
        }
        if self.names.len() > MAX_COLUMNS || self.names.iter().any(|n| n.values.is_empty() || n.values.len() > MAX_NAMES) {
            return bad("too many named values".into());
        }
        Ok(())
    }
}

/// Which points are shown: those whose property lies in a range (both ends included), or, for a
/// label, is one of a set of values.
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

/// Numbers, shared between copies of the design.
#[derive(Clone, Debug, Default)]
pub struct Floats(pub Arc<[f32]>);

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

impl Serialize for Floats {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&base64_encode(&self.0.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()))
    }
}

impl<'de> Deserialize<'de> for Floats {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Ok(Floats(words(d, MAX_POINTS)?.into_iter().map(f32::from_le_bytes).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_round_trip_and_are_checked_against_the_points() {
        let complex = Complex {
            columns: vec![
                Column { name: "t".into(), integer: false, values: Floats(vec![0.0, 0.5, 1.0].into()) },
                Column { name: "role".into(), integer: true, values: Floats(vec![0.0, 4.0, 4.0].into()) },
            ],
            names: Vec::new(),
        };
        let back: Complex = serde_json::from_str(&serde_json::to_string(&complex).unwrap()).unwrap();
        assert_eq!(back, complex);
        complex.check(3).unwrap();
        assert!(complex.check(2).is_err(), "three numbers for two points");
        assert_eq!(complex.span("role"), Some([0.0, 4.0]));
        assert!(complex.integer("role") && !complex.integer("t") && !complex.integer("nope"));
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
    }
}
