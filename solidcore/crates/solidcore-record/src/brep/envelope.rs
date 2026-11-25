//! Stable version-one body metadata records. Runtime model types never define this wire contract.
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub version: u32,
    pub color: Option<[f32; 3]>,
    pub paint: Option<Paint>,
    pub boundaries: Option<Vec<Vec<[f64; 3]>>>,
    pub face_sources: Option<Vec<Option<FaceSource>>>,
    pub history: Option<History>,
    pub preserve_boundaries: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plane {
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Segment {
    Line {
        start: [f64; 2],
        end: [f64; 2],
    },
    Arc {
        center: [f64; 2],
        radius: f64,
        start: f64,
        sweep: f64,
    },
    Cubic {
        points: [[f64; 2]; 4],
    },
    Conic {
        points: [[f64; 2]; 3],
        middle_weight: f64,
    },
    Bezier {
        points: Vec<[f64; 2]>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub outer: Vec<Segment>,
    pub holes: Vec<Vec<Segment>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FaceSource {
    ExtrusionSide {
        segment: Segment,
        plane: Option<Plane>,
        coalesced: bool,
    },
    CertifiedExtrusionSide {
        producer: u64,
        sketch: u64,
        curve: String,
        plane: Option<Plane>,
    },
    ExtrusionCap {
        high: bool,
        region: Region,
        plane: Plane,
    },
    CertifiedExtrusionCap {
        producer: u64,
        sketch: u64,
        high: bool,
        outer: Vec<String>,
        holes: Vec<Vec<String>>,
        plane: Plane,
    },
    CylinderCap {
        producer: u64,
        high: bool,
        plane: Plane,
    },
    BoxFace {
        producer: Option<u64>,
        axis: u8,
        high: bool,
        plane: Plane,
    },
    PlanarShell {
        producer: Option<u64>,
        source: Box<FaceSource>,
        inner: bool,
        plane: Plane,
    },
    Imported {
        item: u64,
        face: u64,
        payload_sha256: Option<String>,
        producer: Option<u64>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paint {
    pub opacity: f32,
    pub faces: Vec<FacePaint>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FacePaint {
    pub face: u32,
    pub color: [f32; 3],
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub operands: u32,
    pub faces: Vec<FaceHistory>,
    pub edges: Vec<EdgeHistory>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaceHistory {
    pub origins: Vec<FaceOrigin>,
    pub change: Change,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeHistory {
    pub faces: Vec<u32>,
    pub origins: Vec<EdgeOrigin>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FaceOrigin {
    Face { operand: u32, face: u32 },
    Profile { segment: u32 },
    Cap { end: bool },
    Edge { operand: u32, edge: u32 },
    Offset { operand: u32, face: u32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EdgeOrigin {
    Edge { operand: u32, edge: u32 },
    Meeting { sides: [Vec<FaceOrigin>; 2] },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Kept,
    Modified,
    Split,
    Merged,
    Generated,
}
