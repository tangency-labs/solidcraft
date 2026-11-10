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

/// The record's structure ([`Brep::validate`]), metadata agreement and finite metadata and
/// runtime values, including a nonnegative runtime size.
pub fn check(saved: &Saved) -> Result<()> {
    saved.solid.validate().map_err(invalid)?;
    let (faces, edges) = (saved.solid.faces.len(), saved.solid.edges.len());
    let size = saved.runtime.size;
    let coarse_missing = saved.runtime.coarse_missing;
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
    if !(size.is_finite() && size >= 0.0) {
        return Err(invalid("runtime values"));
    }
    if coarse_missing.is_some_and(|(value, _)| !value.is_finite())
        || saved.boundaries.as_ref().is_some_and(|boundaries| {
            boundaries
                .iter()
                .flatten()
                .flatten()
                .any(|x| !x.is_finite())
        })
        || saved.face_sources.as_ref().is_some_and(|sources| {
            sources
                .iter()
                .flatten()
                .any(|source| !source_finite(source))
        })
    {
        return Err(invalid("non-finite metadata"));
    }
    Ok(())
}

fn plane_finite(plane: &envelope::Plane) -> bool {
    plane
        .origin
        .iter()
        .chain(&plane.x_axis)
        .chain(&plane.y_axis)
        .all(|v| v.is_finite())
}

fn segment_finite(segment: &envelope::Segment) -> bool {
    use envelope::Segment;
    match segment {
        Segment::Line { start, end } => start.iter().chain(end).all(|v| v.is_finite()),
        Segment::Arc {
            center,
            radius,
            start,
            sweep,
        } => center
            .iter()
            .chain([radius, start, sweep])
            .all(|v| v.is_finite()),
        Segment::Cubic { points } => points.iter().flatten().all(|v| v.is_finite()),
        Segment::Conic {
            points,
            middle_weight,
        } => middle_weight.is_finite() && points.iter().flatten().all(|v| v.is_finite()),
        Segment::Bezier { points } => points.iter().flatten().all(|v| v.is_finite()),
    }
}

// PlanarShell has one child, so walk the borrowed chain without recursion or a depth limit.
fn source_finite(mut source: &envelope::FaceSource) -> bool {
    use envelope::FaceSource;
    loop {
        match source {
            FaceSource::ExtrusionSide { segment, plane, .. } => {
                return segment_finite(segment) && plane.as_ref().is_none_or(plane_finite);
            }
            FaceSource::CertifiedExtrusionSide { plane, .. } => {
                return plane.as_ref().is_none_or(plane_finite);
            }
            FaceSource::ExtrusionCap { region, plane, .. } => {
                return plane_finite(plane)
                    && region
                        .outer
                        .iter()
                        .chain(region.holes.iter().flatten())
                        .all(segment_finite);
            }
            FaceSource::CertifiedExtrusionCap { plane, .. }
            | FaceSource::CylinderCap { plane, .. }
            | FaceSource::BoxFace { plane, .. } => return plane_finite(plane),
            FaceSource::PlanarShell {
                source: child,
                plane,
                ..
            } => {
                if !plane_finite(plane) {
                    return false;
                }
                source = child;
            }
            FaceSource::Imported { .. } => return true,
        }
    }
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
    fn nonfinite_boundary_is_refused() {
        let mut saved = point();
        saved.metadata.boundaries = Some(vec![vec![[f64::NAN, 0.0, 0.0]]]);
        assert!(check(&saved).is_err());
        assert!(encode(&saved).is_err());
    }

    // The single square sheet recipe from brep::tests::faceted.
    fn sheet() -> Saved {
        use crate::brep::{
            Curve, Curve2, Edge, EdgeUseRef, Face, Loop, OrientedEdge, OrientedFace, Surface,
        };
        let points = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
        let mut saved = point();
        saved.solid.vertices = points
            .iter()
            .map(|&point| Vertex {
                point,
                tolerance: 1e-7,
            })
            .collect();
        saved.solid.edges = (0..4)
            .map(|i| Edge {
                nurbs: None,
                curve: Curve::Line {
                    start: points[i],
                    end: points[(i + 1) % 4],
                },
                interval: [0., 1.],
                vertices: [i as u32, ((i + 1) % 4) as u32],
                tolerance: 1e-7,
                uses: vec![EdgeUseRef {
                    face: 0,
                    r#loop: 0,
                    position: i as u32,
                    upper: false,
                }],
            })
            .collect();
        saved.solid.faces = vec![Face {
            nurbs: None,
            surface: Surface::Plane {
                origin: points[0],
                u_axis: points[1],
                v_axis: points[2],
            },
            domain: [[-10., 10.], [-10., 10.]],
            loops: vec![Loop::Edges {
                edges: (0..4)
                    .map(|i| OrientedEdge {
                        edge: i,
                        forward: true,
                        pcurve: Curve2::Spline {
                            knots: vec![0., 0., 1., 1.],
                            points: vec![[0., 0.], [1., 1.]],
                        },
                    })
                    .collect(),
            }],
        }];
        saved.solid.shells = vec![Shell::Faces {
            faces: vec![OrientedFace {
                face: 0,
                forward: true,
            }],
            wires: vec![],
        }];
        saved.solid.validate().unwrap();
        saved
    }

    fn plane() -> envelope::Plane {
        envelope::Plane {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        }
    }

    fn assert_source_refused(source: envelope::FaceSource) {
        let mut saved = sheet();
        saved.metadata.face_sources = Some(vec![Some(source)]);
        assert_eq!(
            check(&saved).err().unwrap().to_string(),
            "native B-rep: non-finite metadata"
        );
        assert!(encode(&saved).is_err());
    }

    #[test]
    fn nonfinite_metadata_fields_are_refused() {
        use envelope::{FaceSource as F, Segment as S};
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for coordinate in 0..3 {
                let mut saved = point();
                let mut p = [0.; 3];
                p[coordinate] = bad;
                saved.metadata.boundaries = Some(vec![vec![p]]);
                assert!(check(&saved).is_err());
                assert!(encode(&saved).is_err());
                for field in 0..3 {
                    let mut p = plane();
                    match field {
                        0 => p.origin[coordinate] = bad,
                        1 => p.x_axis[coordinate] = bad,
                        _ => p.y_axis[coordinate] = bad,
                    }
                    assert_source_refused(F::BoxFace {
                        producer: None,
                        axis: 0,
                        high: false,
                        plane: p,
                    });
                }
            }
            let mut saved = point();
            saved.runtime.coarse_missing = Some((bad, 0));
            assert!(check(&saved).is_err());
            assert!(encode(&saved).is_err());
            let mut segments = Vec::new();
            for i in 0..2 {
                let mut p = [0.; 2];
                p[i] = bad;
                segments.push(S::Line {
                    start: p,
                    end: [0.; 2],
                });
                segments.push(S::Line {
                    start: [0.; 2],
                    end: p,
                });
                segments.push(S::Arc {
                    center: p,
                    radius: 1.,
                    start: 0.,
                    sweep: 1.,
                });
                for n in 0..4 {
                    let mut points = [[0.; 2]; 4];
                    points[n][i] = bad;
                    segments.push(S::Cubic { points });
                }
                for n in 0..3 {
                    let mut points = [[0.; 2]; 3];
                    points[n][i] = bad;
                    segments.push(S::Conic {
                        points,
                        middle_weight: 1.,
                    });
                }
                segments.push(S::Bezier { points: vec![p] });
            }
            segments.extend([
                S::Arc {
                    center: [0.; 2],
                    radius: bad,
                    start: 0.,
                    sweep: 1.,
                },
                S::Arc {
                    center: [0.; 2],
                    radius: 1.,
                    start: bad,
                    sweep: 1.,
                },
                S::Arc {
                    center: [0.; 2],
                    radius: 1.,
                    start: 0.,
                    sweep: bad,
                },
                S::Conic {
                    points: [[0.; 2]; 3],
                    middle_weight: bad,
                },
            ]);
            for segment in segments {
                assert_source_refused(F::ExtrusionSide {
                    segment: segment.clone(),
                    plane: None,
                    coalesced: false,
                });
                assert_source_refused(F::ExtrusionCap {
                    high: false,
                    plane: plane(),
                    region: envelope::Region {
                        outer: vec![],
                        holes: vec![vec![segment]],
                    },
                });
            }
        }
    }

    #[test]
    fn source_variants_and_finite_bytes() {
        use envelope::FaceSource as F;
        let imported = || F::Imported {
            item: 1,
            face: 2,
            payload_sha256: None,
            producer: None,
        };
        let variants = vec![
            F::ExtrusionSide {
                segment: envelope::Segment::Bezier {
                    points: vec![[-0., 1e-300]],
                },
                plane: Some(plane()),
                coalesced: false,
            },
            F::CertifiedExtrusionSide {
                producer: 0,
                sketch: 0,
                curve: String::new(),
                plane: Some(plane()),
            },
            F::ExtrusionCap {
                high: false,
                region: envelope::Region {
                    outer: vec![],
                    holes: vec![],
                },
                plane: plane(),
            },
            F::CertifiedExtrusionCap {
                producer: 0,
                sketch: 0,
                high: false,
                outer: vec![],
                holes: vec![],
                plane: plane(),
            },
            F::CylinderCap {
                producer: 0,
                high: false,
                plane: plane(),
            },
            F::BoxFace {
                producer: None,
                axis: 0,
                high: false,
                plane: plane(),
            },
            F::PlanarShell {
                producer: None,
                source: Box::new(imported()),
                inner: false,
                plane: plane(),
            },
            imported(),
        ];
        for source in variants {
            let mut saved = sheet();
            saved.metadata.boundaries = Some(vec![vec![[-0., 1e-300, 1e12]]]);
            saved.runtime.coarse_missing = Some((-0., 0));
            saved.metadata.face_sources = Some(vec![Some(source)]);
            let bytes = encode(&saved).unwrap();
            let mut value_bytes = MAGIC.to_vec();
            write_value(&serde_json::to_value(&saved).unwrap(), &mut value_bytes).unwrap();
            assert_eq!(bytes, value_bytes);
            let back = decode(&bytes).unwrap();
            for (a, b) in saved.metadata.boundaries.as_ref().unwrap()[0][0]
                .iter()
                .zip(back.metadata.boundaries.as_ref().unwrap()[0][0])
            {
                assert_eq!(a.to_bits(), b.to_bits());
            }
            assert_eq!(
                back.runtime.coarse_missing.unwrap().0.to_bits(),
                (-0.0_f64).to_bits()
            );
            assert_eq!(encode(&back).unwrap(), bytes);
        }
        let mut bad = plane();
        bad.origin[0] = f64::NAN;
        for source in [
            F::CertifiedExtrusionSide {
                producer: 0,
                sketch: 0,
                curve: String::new(),
                plane: Some(bad.clone()),
            },
            F::CertifiedExtrusionCap {
                producer: 0,
                sketch: 0,
                high: false,
                outer: vec![],
                holes: vec![],
                plane: bad.clone(),
            },
            F::CylinderCap {
                producer: 0,
                high: false,
                plane: bad.clone(),
            },
            F::PlanarShell {
                producer: None,
                source: Box::new(imported()),
                inner: false,
                plane: bad,
            },
        ] {
            assert_source_refused(source);
        }
        for depth in [40, 60] {
            let mut source = F::ExtrusionSide {
                segment: envelope::Segment::Line {
                    start: [f64::NAN, 0.],
                    end: [0.; 2],
                },
                plane: None,
                coalesced: false,
            };
            for _ in 0..depth {
                source = F::PlanarShell {
                    producer: None,
                    source: Box::new(source),
                    inner: false,
                    plane: plane(),
                };
            }
            let mut saved = sheet();
            saved.metadata.face_sources = Some(vec![Some(source)]);
            assert!(check(&saved).is_err());
            let mut source = saved
                .metadata
                .face_sources
                .take()
                .unwrap()
                .pop()
                .unwrap()
                .unwrap();
            while let F::PlanarShell { source: child, .. } = source {
                source = *child;
            }
        }
    }

    #[test]
    fn existing_errors_precede_nonfinite_metadata() {
        let mut saved = point();
        saved.metadata.boundaries = Some(vec![vec![[f64::NAN, 0., 0.]]]);
        saved.solid.version = 0;
        assert!(check(&saved).unwrap_err().to_string() != "native B-rep: non-finite metadata");
        saved.solid.version = crate::brep::VERSION;
        saved.metadata.version = 0;
        assert_eq!(check(&saved).unwrap_err().0, "metadata version");
        saved.metadata.version = envelope::VERSION;
        saved.metadata.face_sources = Some(vec![None]);
        assert_eq!(check(&saved).unwrap_err().0, "face source count");
        saved.metadata.face_sources = None;
        saved.metadata.history = Some(envelope::History {
            operands: 0,
            faces: vec![],
            edges: vec![envelope::EdgeHistory {
                faces: vec![],
                origins: vec![],
            }],
        });
        assert_eq!(check(&saved).unwrap_err().0, "history topology");
        saved.metadata.history = None;
        saved.metadata.color = Some([2., 0., 0.]);
        assert_eq!(check(&saved).unwrap_err().0, "colour");
        saved.metadata.color = None;
        saved.metadata.paint = Some(envelope::Paint {
            opacity: -1.,
            faces: vec![],
        });
        assert_eq!(check(&saved).unwrap_err().0, "face paint");
        saved.metadata.paint = None;
        saved.runtime.size = -1.;
        assert_eq!(check(&saved).unwrap_err().0, "runtime values");
    }

    #[test]
    fn finite_writer_limits_and_empty_metadata() {
        use envelope::FaceSource as F;
        let mut saved = point();
        saved.metadata.color = None;
        saved.metadata.boundaries = Some(vec![]);
        saved.metadata.face_sources = Some(vec![]);
        let bytes = encode(&saved).unwrap();
        assert_eq!(encode(&decode(&bytes).unwrap()).unwrap(), bytes);
        saved.metadata.boundaries = Some(vec![vec![[1e13, 0., 0.]]]);
        assert!(check(&saved).is_ok());
        assert_eq!(encode(&saved).unwrap_err().0, "non-finite metadata");
        for depth in [40, 60] {
            let mut source = F::Imported {
                item: 0,
                face: 0,
                payload_sha256: None,
                producer: None,
            };
            for _ in 0..depth {
                source = F::PlanarShell {
                    producer: None,
                    source: Box::new(source),
                    inner: false,
                    plane: plane(),
                };
            }
            let mut saved = sheet();
            saved.metadata.face_sources = Some(vec![Some(source)]);
            assert!(check(&saved).is_ok());
            if depth == 40 {
                assert!(encode(&saved).is_ok());
            } else {
                assert_eq!(encode(&saved).err().unwrap().0, "nesting");
            }
            let mut source = saved
                .metadata
                .face_sources
                .take()
                .unwrap()
                .pop()
                .unwrap()
                .unwrap();
            while let F::PlanarShell { source: child, .. } = source {
                source = *child;
            }
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

    macro_rules! invalid_runtime_size {
        ($name:ident, $size:expr) => {
            #[test]
            fn $name() {
                let mut saved = point();
                saved.runtime.size = $size;
                let expected = Some("native B-rep: runtime values".to_owned());
                assert_eq!(
                    (
                        check(&saved).err().map(|e| e.to_string()),
                        encode(&saved).err().map(|e| e.to_string())
                    ),
                    (expected.clone(), expected)
                );
            }
        };
    }

    invalid_runtime_size!(runtime_size_negative_refused, -1.0);
    invalid_runtime_size!(runtime_size_nan_refused, f64::NAN);
    invalid_runtime_size!(runtime_size_positive_infinity_refused, f64::INFINITY);
    invalid_runtime_size!(runtime_size_negative_infinity_refused, f64::NEG_INFINITY);

    #[test]
    fn runtime_size_valid_bits_and_bytes_roundtrip() {
        for size in [0.0_f64, -0.0, 2.5, 1e12] {
            let mut saved = point();
            saved.runtime.size = size;
            check(&saved).unwrap();
            let bytes = encode(&saved).unwrap();
            let mut value_bytes = MAGIC.to_vec();
            write_value(&serde_json::to_value(&saved).unwrap(), &mut value_bytes).unwrap();
            assert_eq!(bytes, value_bytes);
            let back = decode(&bytes).unwrap();
            assert_eq!(back.runtime.size.to_bits(), size.to_bits());
            assert_eq!(encode(&back).unwrap(), bytes);
        }
    }

    #[test]
    fn runtime_size_negative_file_refused() {
        let mut saved = point();
        saved.runtime.size = -1.0;
        let mut bytes = MAGIC.to_vec();
        write_value(&serde_json::to_value(&saved).unwrap(), &mut bytes).unwrap();
        assert_eq!(
            decode(&bytes).err().map(|e| e.to_string()),
            Some("native B-rep: runtime values".into())
        );
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
