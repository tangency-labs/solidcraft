//! Bounded optical-guide diagnostics, separate from radiance and final colour.
use serde::{Serialize, Serializer, ser::SerializeMap};

/// Stable wire codes shared by native CPU guide generation and GPU shaders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum DenoiseGuideClass {
    Unavailable = 0,
    Surface = 1,
    Optical = 2,
    Background = 3,
    Emitter = 4,
    UnsupportedMaterial = 5,
    NestedMedium = 6,
    DepthOfField = 7,
    Volume = 8,
    PathDepth = 9,
    DegeneratePath = 10,
    IncompatibleExit = 11,
    TotalInternalReflection = 12,
    FurtherTransmission = 13,
    ReflectiveEndpoint = 14,
    IdentityLimit = 15,
    PartialCoverage = 16,
}

impl DenoiseGuideClass {
    pub const ALL: &[Self] = &[
        Self::Unavailable,
        Self::Surface,
        Self::Optical,
        Self::Background,
        Self::Emitter,
        Self::UnsupportedMaterial,
        Self::NestedMedium,
        Self::DepthOfField,
        Self::Volume,
        Self::PathDepth,
        Self::DegeneratePath,
        Self::IncompatibleExit,
        Self::TotalInternalReflection,
        Self::FurtherTransmission,
        Self::ReflectiveEndpoint,
        Self::IdentityLimit,
        Self::PartialCoverage,
    ];

    pub fn from_code(code: u32) -> Option<Self> {
        Self::ALL.get(code as usize).copied()
    }

    pub const fn code(self) -> u32 {
        self as u32
    }

    pub const fn eligible(self) -> bool {
        matches!(self, Self::Surface | Self::Optical)
    }

    pub const fn color(self) -> [u8; 4] {
        let rgb = match self {
            Self::Unavailable => [80, 80, 80],
            Self::Surface => [48, 190, 96],
            Self::Optical => [35, 198, 220],
            Self::Background => [25, 33, 46],
            Self::Emitter => [255, 232, 90],
            Self::UnsupportedMaterial => [229, 121, 42],
            Self::NestedMedium => [202, 82, 185],
            Self::DepthOfField => [143, 105, 223],
            Self::Volume => [116, 137, 168],
            Self::PathDepth => [196, 45, 63],
            Self::DegeneratePath => [145, 81, 49],
            Self::IncompatibleExit => [239, 103, 115],
            Self::TotalInternalReflection => [77, 108, 220],
            Self::FurtherTransmission => [173, 62, 228],
            Self::ReflectiveEndpoint => [235, 157, 190],
            Self::IdentityLimit => [178, 167, 98],
            Self::PartialCoverage => [229, 229, 229],
        };
        [rgb[0], rgb[1], rgb[2], 255]
    }
}

fn serialize_counts<S: Serializer>(counts: &[u64; 17], serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(DenoiseGuideClass::ALL.len()))?;
    for (class, count) in DenoiseGuideClass::ALL.iter().zip(counts) {
        map.serialize_entry(class, count)?;
    }
    map.end()
}

/// Counts describe the internal trace grid. The separate categorical PNG is
/// resolved to the selected output size; it never averages class numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DenoiseDiagnostics {
    pub width: u32,
    pub height: u32,
    #[serde(serialize_with = "serialize_counts")]
    pub counts: [u64; 17],
}

impl DenoiseDiagnostics {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, counts: [0; 17] }
    }

    pub fn record(&mut self, class: DenoiseGuideClass) {
        if let Some(count) = self.counts.get_mut(class.code() as usize) {
            *count = count.saturating_add(1);
        }
    }

    pub fn pixels(&self, class: DenoiseGuideClass) -> u64 {
        self.counts.get(class.code() as usize).copied().unwrap_or(0)
    }
}

/// A bypass is never hidden by a neighbouring eligible subpixel. Otherwise
/// mixed optical/surface guidance is labelled unavailable, not interpolated.
pub fn resolve_denoise_class(classes: impl IntoIterator<Item = DenoiseGuideClass>) -> DenoiseGuideClass {
    let mut selected = None;
    let mut mixed = false;
    for class in classes {
        if !class.eligible() {
            return class;
        }
        if selected.is_some_and(|first| first != class) {
            mixed = true;
        }
        selected = Some(class);
    }
    if mixed { DenoiseGuideClass::Unavailable } else { selected.unwrap_or(DenoiseGuideClass::Unavailable) }
}
