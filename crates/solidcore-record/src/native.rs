//! SolidCraft's native, versioned binary body file: a body's record, its metadata and the
//! values it measured about itself, as bounded tagged binary values. Exact: a decoded record is
//! the encoded one, number for number. Checks are structural only (the record's
//! [`Brep::validate`], the metadata's counts and ranges); the kernel checks the geometry and
//! builds the body.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use crate::brep::{Brep, envelope};

mod binary;

// 01/02 were development-only prototypes; independent metadata records start at 03; 04 stores
// procedural intersections and placed revolutions exactly, and the measured runtime values; 05
// nests geometry schema 4 (regions, unbounded geometry, pcurves, canonical order).
/// The file's first eight bytes.
pub const MAGIC: &[u8; 8] = b"SCBREP05";
// A body's own limits. A design's cache holds many bodies within the container's budgets, and
// decoding is linear in the bytes, so one large part (an imported casting) fits.
/// The most bytes a file may have.
pub const MAX_BYTES: usize = 256 << 20;
/// The most items in one collection, and faces or edges in one body.
pub const MAX_ITEMS: usize = 1_000_000;
/// The most values a file may hold.
pub const MAX_VALUES: usize = 20_000_000;

/// Why a file was refused or could not be written.
#[derive(Clone, Debug, thiserror::Error)]
#[error("native B-rep: {0}")]
pub struct Error(pub String);

/// A native-file operation's result.
pub type Result<T> = std::result::Result<T, Error>;

/// An [`Error`] saying `detail`.
pub fn invalid(detail: impl std::fmt::Display) -> Error {
    Error(detail.to_string())
}

/// One body as saved.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    /// The body's record.
    pub solid: Brep,
    /// What it carries: colour, paint, boundaries, face sources, history.
    pub metadata: envelope::Metadata,
    /// What it measured about itself.
    pub runtime: Runtime,
}

/// What a body measured about itself that later operations read: kept so a decoded body works
/// to the same tolerances as the one encoded, without measuring again.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    /// The body's size as stored (0: not measured).
    pub size: f64,
    /// Saved by earlier versions (an orientation mesh's missing-face count); read and ignored.
    pub coarse_missing: Option<(f64, u64)>,
}

/// `saved` as a file: [`check`]ed, then written as it serializes.
pub fn encode(saved: &Saved) -> Result<Vec<u8>> {
    check(saved)?;
    let mut out = MAGIC.to_vec();
    binary::write_serialized(saved, &mut out)?;
    Ok(out)
}

/// A file's body, from bounded untrusted bytes: its size, header, values and structure checked
/// ([`check_value`], [`check`]) before anything walks it.
pub fn decode(bytes: &[u8]) -> Result<Saved> {
    if bytes.len() > MAX_BYTES || bytes.get(..8) != Some(MAGIC.as_slice()) {
        return Err(invalid("version or size"));
    }
    let mut input = bytes.get(8..).ok_or_else(|| invalid("header"))?;
    let mut remaining = MAX_VALUES;
    let value = binary::read(&mut input, 0, &mut remaining)?;
    if !input.is_empty() {
        return Err(invalid("trailing data"));
    }
    check_value(&value)?;
    let saved: Saved = serde_json::from_value(value).map_err(invalid)?;
    check(&saved)?;
    let size = saved.runtime.size;
    if !(size.is_finite() && size >= 0.0) {
        return Err(invalid("runtime values"));
    }
    Ok(saved)
}

/// The tagged binary value at the start of `input`, which it moves past, taking from `budget`
/// one per value read.
pub fn read_value(input: &mut &[u8], budget: &mut usize) -> Result<Value> {
    binary::read(input, 0, budget)
}

/// `value` appended to `out` as a tagged binary value. [`encode`] writes a body as it
/// serializes; this writes a value already built (a hostile one, in tests).
pub fn write_value(value: &Value, out: &mut Vec<u8>) -> Result<()> {
    binary::write(value, out, 0)
}

/// The record's structure ([`Brep::validate`]) and the metadata's agreement with it.
pub fn check(saved: &Saved) -> Result<()> {
    saved.solid.validate().map_err(invalid)?;
    let (faces, edges) = (saved.solid.faces.len(), saved.solid.edges.len());
    let saved = &saved.metadata;
    if saved.version != envelope::VERSION {
        return Err(invalid("metadata version"));
    }
    if saved
        .face_sources
        .as_ref()
        .is_some_and(|s| s.len() != faces)
    {
        return Err(invalid("face source count"));
    }
    if let Some(history) = &saved.history
        && (history.faces.len() != faces
            || history.edges.len() != edges
            || history.operands as usize > MAX_ITEMS
            || history
                .edges
                .iter()
                .flat_map(|e| &e.faces)
                .any(|&f| f as usize >= faces))
    {
        return Err(invalid("history topology"));
    }
    if saved
        .color
        .is_some_and(|c| c.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)))
    {
        return Err(invalid("colour"));
    }
    if let Some(paint) = &saved.paint
        && (!paint.opacity.is_finite()
            || !(0.0..=1.0).contains(&paint.opacity)
            || paint.faces.iter().any(|p| {
                p.face as usize >= faces
                    || !p.opacity.is_finite()
                    || !(0.0..=1.0).contains(&p.opacity)
                    || p.color
                        .iter()
                        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            }))
    {
        return Err(invalid("face paint"));
    }
    Ok(())
}

// The binary reader bounds allocation before deserialization. Geometry semantics
// are checked by the independent schema validator, not by backend serde internals.
/// A value's nesting, string sizes, numbers and array sizes within the file's budgets.
pub fn check_value(v: &Value) -> Result<()> {
    value_within(v, 0)
}

fn value_within(v: &Value, depth: usize) -> Result<()> {
    if depth > 48 {
        return Err(invalid("nesting"));
    }
    match v {
        Value::String(s) if s.len() > 1 << 20 => return Err(invalid("string budget")),
        Value::Number(n)
            if n.is_f64() && n.as_f64().is_none_or(|x| !x.is_finite() || x.abs() > 1e12) =>
        {
            return Err(invalid("non-finite metadata"));
        }
        Value::Array(a) => {
            if a.len() > MAX_ITEMS {
                return Err(invalid("array budget"));
            }
            for x in a {
                value_within(x, depth + 1)?;
            }
        }
        Value::Object(o) => {
            for x in o.values() {
                value_within(x, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brep::{Region, Shell, Vertex};

    /// One isolated point: the smallest valid body, with colour and nothing else.
    fn point() -> Saved {
        let solid = Brep {
            version: crate::brep::VERSION,
            tolerance: 1e-7,
            regions: vec![Region {
                shells: vec![0],
                void: false,
            }],
            shells: vec![Shell::Vertex { vertex: 0 }],
            faces: vec![],
            edges: vec![],
            vertices: vec![Vertex {
                point: [1., -2.5, 3e5],
                tolerance: 1e-7,
            }],
        };
        let metadata = envelope::Metadata {
            version: envelope::VERSION,
            color: Some([0.1, 0.5, 0.9]),
            paint: None,
            boundaries: None,
            face_sources: None,
            history: None,
            preserve_boundaries: false,
        };
        Saved {
            solid,
            metadata,
            runtime: Runtime {
                size: 2.5,
                coarse_missing: None,
            },
        }
    }

    #[test]
    fn a_file_is_its_record_number_for_number() {
        let saved = point();
        let bytes = encode(&saved).unwrap();
        assert!(bytes.starts_with(MAGIC));
        let back = decode(&bytes).unwrap();
        assert_eq!(back.solid, saved.solid);
        assert_eq!(back.metadata, saved.metadata);
        assert_eq!(back.runtime.size.to_bits(), saved.runtime.size.to_bits());
        assert_eq!(encode(&back).unwrap(), bytes);
        // Written as it serializes, the bytes are those of its JSON value.
        let mut through_a_value = MAGIC.to_vec();
        write_value(&serde_json::to_value(&saved).unwrap(), &mut through_a_value).unwrap();
        assert_eq!(through_a_value, bytes);
        let mut raw = bytes.get(8..).unwrap();
        assert_eq!(
            read_value(&mut raw, &mut 1000).unwrap(),
            serde_json::to_value(&saved).unwrap()
        );
        assert!(raw.is_empty());
    }

    #[test]
    fn malformed_files_are_refused_and_never_panic() {
        let bytes = encode(&point()).unwrap();
        for n in [0, 1, 7, 8, 9, bytes.len() - 1] {
            assert!(decode(&bytes[..n]).is_err());
        }
        for i in 0..bytes.len() {
            let mut bad = bytes.clone();
            bad[i] ^= 0xff;
            assert!(std::panic::catch_unwind(|| decode(&bad)).is_ok());
        }
        let mut huge = MAGIC.to_vec();
        huge.push(7);
        huge.extend(u32::MAX.to_le_bytes());
        assert!(decode(&huge).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(
            decode(&trailing).err().map(|e| e.to_string()),
            Some("native B-rep: trailing data".into())
        );
    }

    #[test]
    fn metadata_that_disagrees_with_its_record_is_refused() {
        let mut saved = point();
        saved.metadata.face_sources = Some(vec![None]);
        assert_eq!(
            check(&saved).err().map(|e| e.to_string()),
            Some("native B-rep: face source count".into())
        );
        let mut saved = point();
        saved.metadata.color = Some([2., 0., 0.]);
        assert!(encode(&saved).is_err());
        let mut saved = point();
        saved.runtime.size = f64::NAN;
        let mut bytes = MAGIC.to_vec();
        write_value(&serde_json::to_value(&saved).unwrap(), &mut bytes).unwrap();
        assert!(decode(&bytes).is_err());
        assert!(check_value(&serde_json::json!({ "x": 1e13 })).is_err());
    }
}
