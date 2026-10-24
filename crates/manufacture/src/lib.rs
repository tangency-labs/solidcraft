//! CNC manufacture domain: named setups (stock, WCS, tools, operations), machine and post
//! profiles, and normalized toolpath segment contracts.
//!
//! This crate is persistence-only: it defines what is saved in the document and what the
//! engine produces for a post-processor. It does not evaluate toolpaths, run commands, or
//! contain any UI. Feature selection is resolved by the in-process SolidCraft kernel
//! against the design B-rep. There is no CadQuery, FreeCAD Path, or other GPL/LGPL CAM
//! runtime in this crate.
//!
//! # Document integration
//! [`ManufactureSettings`] lives in `solidcraft_doc::Document::manufacture`. It serialises only
//! when non-empty (`is_empty()` → `skip_serializing_if`). Format 22 introduces the field;
//! older documents open with an empty default.
//!
//! # Validation
//! [`ManufactureSettings`] uses `#[serde(try_from = "Unchecked…")]` so that every document
//! that deserialises is already valid. [`ManufactureSettings::validate`] is also available
//! for use before saving. Soft diagnostics are surfaced through [`ValidationIssue`].

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod additive;
pub mod library;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Bounds on collections
// ---------------------------------------------------------------------------

/// Maximum number of named setups in a document.
pub const MAX_SETUPS: usize = 64;
/// Maximum byte length of a setup, machine, or post profile name.
pub const MAX_NAME_BYTES: usize = 128;
/// Maximum tools per setup.
pub const MAX_TOOLS_PER_SETUP: usize = 64;
/// Maximum operations per setup.
pub const MAX_OPS_PER_SETUP: usize = 256;
/// Maximum named machine profiles in a document.
pub const MAX_MACHINES: usize = 32;
/// Maximum named post profiles in a document.
pub const MAX_POSTS: usize = 32;
/// Maximum model-space sample points per operation (face/profile/pocket/adaptive selection).
pub const MAX_SAMPLE_POINTS_PER_OP: usize = 64;
/// Maximum explicit drill-hole centre points per DrillOperation.
pub const MAX_DRILL_POINTS_PER_OP: usize = 256;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// A hard error returned when manufacture settings violate an invariant.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ManufactureError {
    #[error("{0}")]
    Invalid(String),
}

type Result<T> = std::result::Result<T, ManufactureError>;

fn invalid(msg: impl Into<String>) -> ManufactureError {
    ManufactureError::Invalid(msg.into())
}

// ---------------------------------------------------------------------------
// Soft validation (non-fatal diagnostics)
// ---------------------------------------------------------------------------

/// Severity of a soft validation diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}

/// A non-fatal diagnostic produced by [`ManufactureSettings::issues`].
///
/// Hard invariants (collection bounds, positive dimensions, orthonormal WCS) fail
/// deserialisation; softer concerns (cross-reference mismatches, near-limit feeds) are
/// returned here so callers can decide how to surface them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: Severity,
    /// JSON-path-style location (e.g. `"setups.Setup1.tools[0]"`).
    pub path: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Normalised toolpath segments
// ---------------------------------------------------------------------------

/// A single normalised toolpath move. Used by post-processors; not persisted in the document.
///
/// All coordinates are in the WCS frame, in millimetres. Feed rates are in mm/min.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathSegment {
    /// Rapid (non-cutting) linear move.
    Rapid { to: [f64; 3] },
    /// Linear cutting move at the given feed rate.
    Linear { to: [f64; 3], feed: f64 },
    /// Helical or circular arc in the XY plane. `center` is the arc centre; `clockwise`
    /// follows the right-hand rule about the +Z axis.
    Arc { center: [f64; 3], to: [f64; 3], feed: f64, clockwise: bool },
}

impl PathSegment {
    /// Returns `Ok(())` when all coordinates are finite and the feed (where present) is
    /// strictly positive.
    pub fn validate(&self) -> Result<()> {
        let finite3 = |v: &[f64; 3]| v.iter().all(|x| x.is_finite());
        match self {
            PathSegment::Rapid { to } => {
                if !finite3(to) {
                    return Err(invalid("rapid: `to` coordinates must be finite"));
                }
            }
            PathSegment::Linear { to, feed } => {
                if !finite3(to) {
                    return Err(invalid("linear: `to` coordinates must be finite"));
                }
                if !(*feed > 0.0 && feed.is_finite()) {
                    return Err(invalid("linear: feed rate must be strictly positive and finite"));
                }
            }
            PathSegment::Arc { center, to, feed, .. } => {
                if !finite3(center) || !finite3(to) {
                    return Err(invalid("arc: `center` and `to` coordinates must be finite"));
                }
                if !(*feed > 0.0 && feed.is_finite()) {
                    return Err(invalid("arc: feed rate must be strictly positive and finite"));
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Machine profile
// ---------------------------------------------------------------------------

/// The number of controlled axes a machine supports.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineAxes {
    #[default]
    ThreeAxis,
    FourAxis,
    FiveAxis,
}

/// Spindle / toolholder interface (static library metadata; not a live driver).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpindleTaper {
    #[default]
    Unknown,
    Cat40,
    Cat50,
    Bt30,
    Bt40,
    HskA63,
    HskA100,
    CaptoC5,
    CaptoC6,
    /// Light mill / router ER collet spindle (no ATC taper).
    ErCollet,
}

/// Automatic / manual tool change hardware class.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChangerKind {
    /// Single spindle tool; operator swaps offline.
    #[default]
    None,
    /// Operator changes tools at the spindle between ops (no magazine).
    Manual,
    /// Drum / carousel magazine.
    Carousel,
    /// Swing-arm ATC (often dual-arm).
    Arm,
    /// Haas-style side-mount tool changer.
    SideMount,
    /// Chain / matrix magazine.
    Chain,
}

/// ATC / magazine limits used for soft validation and operator planning.
///
/// `pockets == 0` means unconstrained / unknown. Times and max tool sizes of `0` mean unknown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ToolChanger {
    pub kind: ToolChangerKind,
    /// Magazine capacity (station count). 0 = unconstrained / unknown.
    pub pockets: u32,
    /// True when any pocket can be selected without sequencing through neighbors.
    pub random_access: bool,
    pub dual_arm: bool,
    /// Typical chip-to-chip tool change time in seconds; 0 = unknown.
    pub change_time_s: f64,
    pub max_tool_diameter_mm: f64,
    pub max_tool_length_mm: f64,
    pub max_tool_weight_kg: f64,
}

impl Default for ToolChanger {
    fn default() -> Self {
        Self {
            kind: ToolChangerKind::None,
            pockets: 0,
            random_access: false,
            dual_arm: false,
            change_time_s: 0.0,
            max_tool_diameter_mm: 0.0,
            max_tool_length_mm: 0.0,
            max_tool_weight_kg: 0.0,
        }
    }
}

/// Easy OEM integration surface — read-oriented shop-floor hooks only.
///
/// Hard / partner-only stacks (CELOS app store, TRUMPF deep APIs, etc.) are intentionally
/// omitted; prefer MTConnect, Fanuc FOCAS, or Okuma THINC.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineIntegrationKind {
    #[default]
    None,
    /// Write NC / G-code to a file; operator loads it on the control (USB stick, SD, sender).
    FileGcode,
    /// Serial / USB G-code stream to a hobby controller (GRBL, FluidNC, Marlin, Repetier, …).
    SerialGcode,
    /// Galvo laser (BJJCZ / JCZ / EzCAD class): LightBurn or vendor host owns the link.
    JczLaser,
    /// Open MTConnect agent (Haas NGC, Mazak Smooth, Okuma adapter, many others).
    MtConnect,
    /// Fanuc FOCAS Ethernet (also DN Solutions / Doosan Fanuc controls).
    Focas,
    /// Okuma THINC Open API (Windows-on-control apps).
    Thinc,
}

/// How SolidCraft expects to talk to this machine class later (metadata today).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MachineIntegration {
    pub kind: MachineIntegrationKind,
    /// Default TCP port when applicable (8082 Haas MTConnect, 8193 FOCAS, 5000 common MTConnect).
    pub default_port: u16,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Citation for ports / protocol (vendor manual or public doc).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}

impl Default for MachineIntegration {
    fn default() -> Self {
        Self { kind: MachineIntegrationKind::None, default_port: 0, notes: String::new(), source: String::new() }
    }
}

/// Saved description of a CNC machine's capabilities.
///
/// Used during toolpath validation and post-processor output; not enforced at cut time.
/// OEM presets land in a follow-up machine-pack PR.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MachineProfile {
    pub name: String,
    /// OEM brand label (`Haas`, `Okuma`, `Fanuc`, `Mazak`, …).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub oem: String,
    /// Optional model string for display.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    pub axes: MachineAxes,
    pub spindle_taper: SpindleTaper,
    pub tool_changer: ToolChanger,
    pub integration: MachineIntegration,
    /// Workpiece / tool probe macros available on this machine class.
    pub probe: ProbeCapability,
    /// Maximum spindle speed in RPM; 0 means unconstrained.
    pub max_spindle_rpm: u32,
    /// Maximum feedrate in mm/min; 0.0 means unconstrained.
    pub max_feedrate_mmpm: f64,
    /// Rapid traverse rate written into the gcode preamble (mm/min).
    pub rapid_feedrate_mmpm: f64,
}

impl Default for MachineProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            oem: String::new(),
            model: String::new(),
            axes: MachineAxes::ThreeAxis,
            spindle_taper: SpindleTaper::Unknown,
            tool_changer: ToolChanger::default(),
            integration: MachineIntegration::default(),
            probe: ProbeCapability::default(),
            max_spindle_rpm: 24_000,
            max_feedrate_mmpm: 10_000.0,
            rapid_feedrate_mmpm: 10_000.0,
        }
    }
}

/// Probe hardware / macro package on a machine (planning metadata; snippets in [`probe`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProbeCapability {
    pub present: bool,
    pub style: ProbeMacroStyle,
    /// Typical ATC pocket for the spindle probe; 0 = unknown / not magazine-resident.
    pub tool_number: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl Default for ProbeCapability {
    fn default() -> Self {
        Self { present: false, style: ProbeMacroStyle::None, tool_number: 0, notes: String::new() }
    }
}

/// Macro dialect used when emitting review probe snippets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeMacroStyle {
    #[default]
    None,
    /// Renishaw Inspection Plus on Haas (G65 P981x / P9832–P9833).
    HaasInspectionPlus,
    /// Renishaw Inspection Plus on Fanuc-class controls (same O98xx family).
    FanucInspectionPlus,
}

pub(crate) fn validate_machine(m: &MachineProfile, path: &str) -> Result<()> {
    if m.max_feedrate_mmpm < 0.0 || !m.max_feedrate_mmpm.is_finite() {
        return Err(invalid(format!("{path}: max_feedrate_mmpm must be non-negative and finite")));
    }
    if m.rapid_feedrate_mmpm < 0.0 || !m.rapid_feedrate_mmpm.is_finite() {
        return Err(invalid(format!("{path}: rapid_feedrate_mmpm must be non-negative and finite")));
    }
    let tc = &m.tool_changer;
    for (field, v) in [
        ("change_time_s", tc.change_time_s),
        ("max_tool_diameter_mm", tc.max_tool_diameter_mm),
        ("max_tool_length_mm", tc.max_tool_length_mm),
        ("max_tool_weight_kg", tc.max_tool_weight_kg),
    ] {
        if !(v.is_finite() && v >= 0.0) {
            return Err(invalid(format!("{path}.tool_changer.{field}: must be finite and ≥ 0")));
        }
    }
    if matches!(tc.kind, ToolChangerKind::None | ToolChangerKind::Manual) && tc.dual_arm {
        return Err(invalid(format!("{path}.tool_changer: dual_arm requires a magazine ATC")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Post profile
// ---------------------------------------------------------------------------

/// G-code dialect variant produced by a post-processor.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GcodeDialect {
    #[default]
    Generic,
    Fanuc,
    Haas,
    Grbl,
    /// Marlin CNC / Snapmaker-class (file or serial; not a shop MTConnect mill).
    Marlin,
    /// Repetier-Firmware / Repetier-Host style senders.
    Repetier,
    LinuxCnc,
    Mach3Mach4,
}

/// Line ending written into the generated G-code file.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

/// Saved description of a post-processor's output format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct PostProfile {
    pub name: String,
    pub dialect: GcodeDialect,
    pub line_ending: LineEnding,
    /// Lines to emit before the main program body (e.g. `G21` unit selection).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub preamble: String,
    /// Lines to emit after the main program body (e.g. `M30`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub postamble: String,
}

fn validate_post(p: &PostProfile, path: &str) -> Result<()> {
    // Preamble/postamble must not contain null bytes.
    for (field, s) in [("preamble", &p.preamble), ("postamble", &p.postamble)] {
        if s.contains('\0') {
            return Err(invalid(format!("{path}.{field}: must not contain null bytes")));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Stock (Setup → Stock tab)
// ---------------------------------------------------------------------------

/// How relative-box stock offsets are applied (stock offset mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockOffsetMode {
    #[default]
    NoAdditional,
    SidesAndTopBottom,
    AllSides,
}

impl StockOffsetMode {
    pub fn label(self) -> &'static str {
        match self {
            StockOffsetMode::NoAdditional => "No additional stock",
            StockOffsetMode::SidesAndTopBottom => "Add stock to sides and top-bottom",
            StockOffsetMode::AllSides => "Add stock to all sides",
        }
    }
}

/// The raw material for a setup (stock mode).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stock {
    /// Fixed-size rectangular block (mm).
    Box { width: f64, depth: f64, height: f64 },
    /// Relative box: model bounding box + offsets (mm).
    RelativeBox {
        mode: StockOffsetMode,
        side_offset: f64,
        top_offset: f64,
        bottom_offset: f64,
        /// Optional round-up increment (0 = off).
        round_up: f64,
        /// When true, sit the relative box on the model origin plane.
        #[serde(default)]
        ground_at_model_origin: bool,
    },
    /// Fixed-size cylinder (diameter × length mm).
    Cylinder { diameter: f64, height: f64 },
    /// Relative cylinder around the model.
    RelativeCylinder { radial_offset: f64, front_offset: f64, back_offset: f64, round_up: f64 },
    /// Fixed-size tube (OD / ID / length mm).
    Tube { outer_diameter: f64, inner_diameter: f64, height: f64 },
    /// Inherit stock from the preceding setup (rest machining).
    FromPreceding { continue_rest: bool },
    /// Stock is an existing solid body in the design (named body).
    FromSolid { body: String },
}

impl Default for Stock {
    fn default() -> Self {
        Self::Box { width: 100.0, depth: 100.0, height: 25.0 }
    }
}

impl Stock {
    pub fn mode_label(&self) -> &'static str {
        match self {
            Stock::Box { .. } => "Fixed size box",
            Stock::RelativeBox { .. } => "Relative size box",
            Stock::Cylinder { .. } => "Fixed size cylinder",
            Stock::RelativeCylinder { .. } => "Relative size cylinder",
            Stock::Tube { .. } => "Fixed size tube",
            Stock::FromPreceding { .. } => "From preceding setup",
            Stock::FromSolid { .. } => "From solid",
        }
    }

    /// Bounding box extents (width, depth, height) for simulation overlays when applicable.
    pub fn sim_box_extents(&self) -> Option<(f64, f64, f64)> {
        match self {
            Stock::Box { width, depth, height } => Some((*width, *depth, *height)),
            Stock::RelativeBox { side_offset, top_offset, bottom_offset, .. } => {
                Some((100.0 + 2.0 * side_offset, 100.0 + 2.0 * side_offset, 25.0 + top_offset + bottom_offset))
            }
            Stock::Cylinder { .. } | Stock::RelativeCylinder { .. } | Stock::Tube { .. } | Stock::FromPreceding { .. } | Stock::FromSolid { .. } => {
                None
            }
        }
    }
}

fn validate_stock(s: &Stock, path: &str) -> Result<()> {
    match s {
        Stock::Box { width, depth, height } => {
            if !(*width > 0.0 && width.is_finite() && *depth > 0.0 && depth.is_finite() && *height > 0.0 && height.is_finite()) {
                return Err(invalid(format!("{path}: stock box dimensions must be strictly positive and finite")));
            }
        }
        Stock::RelativeBox { side_offset, top_offset, bottom_offset, round_up, .. } => {
            for (label, v) in [("side_offset", side_offset), ("top_offset", top_offset), ("bottom_offset", bottom_offset), ("round_up", round_up)] {
                if !(v.is_finite() && *v >= 0.0) {
                    return Err(invalid(format!("{path}.{label}: must be ≥ 0 and finite")));
                }
            }
        }
        Stock::Cylinder { diameter, height } => {
            if !(*diameter > 0.0 && diameter.is_finite() && *height > 0.0 && height.is_finite()) {
                return Err(invalid(format!("{path}: stock cylinder dimensions must be strictly positive and finite")));
            }
        }
        Stock::RelativeCylinder { radial_offset, front_offset, back_offset, round_up } => {
            for (label, v) in [("radial_offset", radial_offset), ("front_offset", front_offset), ("back_offset", back_offset), ("round_up", round_up)]
            {
                if !(v.is_finite() && *v >= 0.0) {
                    return Err(invalid(format!("{path}.{label}: must be ≥ 0 and finite")));
                }
            }
        }
        Stock::Tube { outer_diameter, inner_diameter, height } => {
            if !(*outer_diameter > 0.0
                && outer_diameter.is_finite()
                && *inner_diameter > 0.0
                && inner_diameter.is_finite()
                && *height > 0.0
                && height.is_finite())
            {
                return Err(invalid(format!("{path}: tube dimensions must be strictly positive and finite")));
            }
            if *inner_diameter >= *outer_diameter {
                return Err(invalid(format!("{path}: tube inner_diameter must be < outer_diameter")));
            }
        }
        Stock::FromPreceding { .. } => {}
        Stock::FromSolid { body } => {
            if body.trim().is_empty() || body.len() > MAX_NAME_BYTES {
                return Err(invalid(format!("{path}: from-solid stock needs a valid body name")));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Work Coordinate System (Setup → WCS)
// ---------------------------------------------------------------------------

/// How the WCS axes are chosen (orientation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WcsOrientationMode {
    #[default]
    ModelOrientation,
    ZAndX,
    ZAndY,
    XAndY,
    CoordinateSystem,
}

impl WcsOrientationMode {
    pub fn label(self) -> &'static str {
        match self {
            WcsOrientationMode::ModelOrientation => "Model orientation",
            WcsOrientationMode::ZAndX => "Select Z axis/plane & X axis",
            WcsOrientationMode::ZAndY => "Select Z axis/plane & Y axis",
            WcsOrientationMode::XAndY => "Select X & Y axes",
            WcsOrientationMode::CoordinateSystem => "Select coordinate system",
        }
    }
}

/// How part zero is located (origin).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WcsOriginMode {
    #[default]
    ModelOrigin,
    SelectedPoint,
    StockBoxPoint,
    ModelBoxPoint,
}

impl WcsOriginMode {
    pub fn label(self) -> &'static str {
        match self {
            WcsOriginMode::ModelOrigin => "Model origin",
            WcsOriginMode::SelectedPoint => "Selected point",
            WcsOriginMode::StockBoxPoint => "Stock box point",
            WcsOriginMode::ModelBoxPoint => "Model box point",
        }
    }
}

/// Axis reference on a stock/model box (stock/model box point picker).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoxAxisRef {
    #[default]
    Negative,
    Center,
    Positive,
}

impl BoxAxisRef {
    pub fn label(self) -> &'static str {
        match self {
            BoxAxisRef::Negative => "−",
            BoxAxisRef::Center => "Center",
            BoxAxisRef::Positive => "+",
        }
    }
}

/// Corner / face-centre pick on a bounding box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BoxPoint {
    pub x: BoxAxisRef,
    pub y: BoxAxisRef,
    pub z: BoxAxisRef,
}

/// Operation type selected with the machine (Setup).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupOperationType {
    #[default]
    Milling,
    Turning,
    Cutting,
    Additive,
}

impl SetupOperationType {
    pub fn label(self) -> &'static str {
        match self {
            SetupOperationType::Milling => "Milling",
            SetupOperationType::Turning => "Turning / Mill-Turn",
            SetupOperationType::Cutting => "Cutting (waterjet / laser / plasma)",
            SetupOperationType::Additive => "Additive",
        }
    }
}

/// Post Process tab on the Setup dialog (program name + machine WCS offset).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SetupPostOpts {
    pub program_name: String,
    pub program_comment: String,
    /// Machine work offset number: 0/1 → G54, 2 → G55, …
    pub wcs_offset: u32,
    pub multiple_wcs_offsets: bool,
}

impl Default for SetupPostOpts {
    fn default() -> Self {
        Self { program_name: "1001".into(), program_comment: String::new(), wcs_offset: 1, multiple_wcs_offsets: false }
    }
}

/// A right-handed orthonormal frame in the body's model space (mm).
///
/// `z_axis = x_axis × y_axis` is implied. The WCS origin is where the machine's G54 (or
/// equivalent) offset is set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkCoordinateSystem {
    /// WCS origin in model space (mm).
    pub origin: [f64; 3],
    /// Unit vector along the WCS X axis.
    pub x_axis: [f64; 3],
    /// Unit vector along the WCS Y axis (must be orthogonal to `x_axis`).
    pub y_axis: [f64; 3],
}

impl Default for WorkCoordinateSystem {
    fn default() -> Self {
        Self { origin: [0.0; 3], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }
    }
}

fn validate_wcs(wcs: &WorkCoordinateSystem, path: &str) -> Result<()> {
    let norm2 = |v: [f64; 3]| v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

    for (label, v) in [("x_axis", wcs.x_axis), ("y_axis", wcs.y_axis)] {
        if !v.iter().all(|x| x.is_finite()) || (norm2(v) - 1.0).abs() > 1e-6 {
            return Err(invalid(format!("{path}.{label}: must be a finite unit vector")));
        }
    }
    if dot(wcs.x_axis, wcs.y_axis).abs() > 1e-6 {
        return Err(invalid(format!("{path}: x_axis and y_axis must be orthogonal")));
    }
    if !wcs.origin.iter().all(|x| x.is_finite() && x.abs() <= 1e9) {
        return Err(invalid(format!("{path}.origin: coordinates must be finite and within ±1 km")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/// The cutting geometry of a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    #[default]
    FlatEndmill,
    BallEndmill,
    /// Flat endmill with a corner radius (bull-nose).
    BullnoseEndmill,
    Drill,
    Chamfer,
    Facemill,
    Tap,
    /// Touch probe (Renishaw / OEM). `diameter` is stylus ball Ø; not a cutter.
    Probe,
    /// Fabrication: abrasive waterjet nozzle.
    Waterjet,
    /// Fabrication: laser cutter.
    Laser,
    /// Fabrication: plasma torch.
    Plasma,
}

impl ToolKind {
    pub const ALL: &'static [ToolKind] = &[
        ToolKind::FlatEndmill,
        ToolKind::BallEndmill,
        ToolKind::BullnoseEndmill,
        ToolKind::Drill,
        ToolKind::Chamfer,
        ToolKind::Facemill,
        ToolKind::Tap,
        ToolKind::Probe,
        ToolKind::Waterjet,
        ToolKind::Laser,
        ToolKind::Plasma,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::FlatEndmill => "Flat end mill",
            ToolKind::BallEndmill => "Ball end mill",
            ToolKind::BullnoseEndmill => "Bull nose end mill",
            ToolKind::Drill => "Drill",
            ToolKind::Chamfer => "Engrave/Chamfer mill",
            ToolKind::Facemill => "Face mill",
            ToolKind::Tap => "Tap",
            ToolKind::Probe => "Probe",
            ToolKind::Waterjet => "Waterjet",
            ToolKind::Laser => "Laser cutter",
            ToolKind::Plasma => "Plasma cutter",
        }
    }

    /// Rough category for Tool Library Filters.
    pub fn category(self) -> ToolCategory {
        match self {
            ToolKind::Drill | ToolKind::Tap => ToolCategory::HoleMaking,
            ToolKind::Probe => ToolCategory::Probe,
            ToolKind::Waterjet | ToolKind::Laser | ToolKind::Plasma => ToolCategory::Cutting,
            _ => ToolCategory::Milling,
        }
    }

    pub fn is_cutting_nozzle(self) -> bool {
        matches!(self, ToolKind::Waterjet | ToolKind::Laser | ToolKind::Plasma)
    }
}

/// Tool Library filter category (Filters pane).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    #[default]
    Milling,
    HoleMaking,
    Turning,
    Cutting,
    Probe,
    Holders,
}

impl ToolCategory {
    pub const ALL: &'static [ToolCategory] =
        &[ToolCategory::Milling, ToolCategory::HoleMaking, ToolCategory::Turning, ToolCategory::Cutting, ToolCategory::Probe, ToolCategory::Holders];

    pub fn label(self) -> &'static str {
        match self {
            ToolCategory::Milling => "Milling",
            ToolCategory::HoleMaking => "Hole making",
            ToolCategory::Turning => "Turning",
            ToolCategory::Cutting => "Cutting",
            ToolCategory::Probe => "Probe",
            ToolCategory::Holders => "Holders",
        }
    }
}

/// Cutter substrate (Cutter → Material).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolMaterial {
    Hss,
    #[default]
    Carbide,
    Cobalt,
    Ceramic,
    Diamond,
    Unknown,
}

impl ToolMaterial {
    pub const ALL: &'static [ToolMaterial] =
        &[ToolMaterial::Hss, ToolMaterial::Carbide, ToolMaterial::Cobalt, ToolMaterial::Ceramic, ToolMaterial::Diamond, ToolMaterial::Unknown];

    pub fn label(self) -> &'static str {
        match self {
            ToolMaterial::Hss => "HSS",
            ToolMaterial::Carbide => "Carbide",
            ToolMaterial::Cobalt => "Cobalt",
            ToolMaterial::Ceramic => "Ceramic",
            ToolMaterial::Diamond => "Diamond",
            ToolMaterial::Unknown => "Unknown",
        }
    }
}

/// One conical/cylindrical shaft segment (Shaft tab).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShaftSegment {
    pub height_mm: f64,
    pub upper_diameter_mm: f64,
    pub lower_diameter_mm: f64,
}

impl Default for ShaftSegment {
    fn default() -> Self {
        Self { height_mm: 10.0, upper_diameter_mm: 6.0, lower_diameter_mm: 6.0 }
    }
}

/// Tool holder metadata (Holder tab).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ToolHolder {
    pub description: String,
    pub product_id: String,
    pub product_link: String,
    pub vendor: String,
    pub gauge_length_mm: f64,
}

impl Default for ToolHolder {
    fn default() -> Self {
        Self { description: String::new(), product_id: String::new(), product_link: String::new(), vendor: String::new(), gauge_length_mm: 60.0 }
    }
}

/// Named cutting-data preset (Cutting data tab).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CuttingPreset {
    pub name: String,
    pub spindle_rpm: u32,
    pub cutting_feedrate: f64,
    pub lead_in_feedrate: f64,
    pub lead_out_feedrate: f64,
    pub transition_feedrate: f64,
    pub ramp_feedrate: f64,
    pub ramp_angle_deg: f64,
    pub plunge_feedrate: f64,
    pub use_stepdown: bool,
    pub stepdown_mm: f64,
    pub use_stepover: bool,
    pub stepover_mm: f64,
    pub coolant: Coolant,
}

impl Default for CuttingPreset {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            spindle_rpm: 12_000,
            cutting_feedrate: 500.0,
            lead_in_feedrate: 200.0,
            lead_out_feedrate: 200.0,
            transition_feedrate: 300.0,
            ramp_feedrate: 200.0,
            ramp_angle_deg: 3.0,
            plunge_feedrate: 200.0,
            use_stepdown: true,
            stepdown_mm: 1.0,
            use_stepover: true,
            stepover_mm: 2.0,
            coolant: Coolant::Air,
        }
    }
}

/// A cutting tool definition within a setup's tool library.
///
/// Maps to Tool Library edit tabs: General / Cutter / Shaft / Holder /
/// Cutting data / Post processor. `number` must be unique within its setup.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tool {
    /// Tool pocket / ATC number; must be ≥ 1 and unique within the setup.
    pub number: u32,
    pub kind: ToolKind,
    /// Cutting diameter in mm (strictly positive).
    pub diameter: f64,
    /// Corner radius in mm; must be ≥ 0 and ≤ diameter / 2.
    pub corner_radius: f64,
    /// Overall tool length in mm (strictly positive).
    pub length: f64,
    /// Flute (usable cutting) length in mm; must be ≤ `length`.
    pub flute_length: f64,
    /// Number of cutting flutes (1–64).
    pub flutes: u8,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    // ── General ────────────────────────────────────────────────────────────
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub vendor: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub product_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub product_link: String,
    // ── Cutter ─────────────────────────────────────────────────────────────
    pub unit: NcUnit,
    pub clockwise: bool,
    pub material: ToolMaterial,
    pub shaft_diameter: f64,
    pub length_below_holder: f64,
    pub shoulder_length: f64,
    pub gauge_length: f64,
    // ── Shaft ──────────────────────────────────────────────────────────────
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shaft_segments: Vec<ShaftSegment>,
    // ── Holder ─────────────────────────────────────────────────────────────
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<ToolHolder>,
    // ── Cutting data ───────────────────────────────────────────────────────
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cutting_presets: Vec<CuttingPreset>,
    // ── Post processor ─────────────────────────────────────────────────────
    pub length_offset: u32,
    pub diameter_offset: u32,
    pub turret: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    pub manual_tool_change: bool,
    pub live_tool: bool,
    pub break_control: bool,
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            number: 1,
            kind: ToolKind::FlatEndmill,
            diameter: 6.0,
            corner_radius: 0.0,
            length: 50.0,
            flute_length: 16.0,
            flutes: 4,
            description: String::new(),
            vendor: String::new(),
            product_id: String::new(),
            product_link: String::new(),
            unit: NcUnit::Millimeters,
            clockwise: true,
            material: ToolMaterial::Carbide,
            shaft_diameter: 6.0,
            length_below_holder: 30.0,
            shoulder_length: 20.0,
            gauge_length: 80.0,
            shaft_segments: Vec::new(),
            holder: None,
            cutting_presets: Vec::new(),
            length_offset: 1,
            diameter_offset: 1,
            turret: 0,
            comment: String::new(),
            manual_tool_change: false,
            live_tool: true,
            break_control: false,
        }
    }
}

impl Tool {
    pub fn display_name(&self) -> String {
        if self.description.is_empty() {
            format!("{} - ø{:.2}mm ({})", self.number, self.diameter, self.kind.label())
        } else {
            format!("{} - ø{:.2}mm ({})", self.number, self.diameter, self.description)
        }
    }

    /// Surface speed in m/min from diameter + spindle rpm.
    pub fn surface_speed_m_min(&self, spindle_rpm: u32) -> f64 {
        std::f64::consts::PI * self.diameter * f64::from(spindle_rpm) / 1_000.0
    }

    /// Feed per tooth (mm) from cutting feedrate + rpm + flutes.
    pub fn feed_per_tooth(&self, feedrate: f64, spindle_rpm: u32) -> f64 {
        if spindle_rpm == 0 || self.flutes == 0 { 0.0 } else { feedrate / (f64::from(spindle_rpm) * f64::from(self.flutes)) }
    }
}

fn validate_tool(t: &Tool, path: &str) -> Result<()> {
    if t.number == 0 {
        return Err(invalid(format!("{path}: tool number must be ≥ 1")));
    }
    if !(t.diameter > 0.0 && t.diameter.is_finite()) {
        return Err(invalid(format!("{path}: tool diameter must be strictly positive and finite")));
    }
    if !(t.corner_radius >= 0.0 && t.corner_radius.is_finite() && t.corner_radius <= t.diameter / 2.0) {
        return Err(invalid(format!("{path}: corner_radius must be ≥ 0 and ≤ diameter/2")));
    }
    if !(t.length > 0.0 && t.length.is_finite()) {
        return Err(invalid(format!("{path}: tool length must be strictly positive and finite")));
    }
    if !(t.flute_length > 0.0 && t.flute_length.is_finite() && t.flute_length <= t.length) {
        return Err(invalid(format!("{path}: flute_length must be > 0 and ≤ length")));
    }
    if t.flutes == 0 {
        return Err(invalid(format!("{path}: flutes must be ≥ 1")));
    }
    for (label, v) in [
        ("shaft_diameter", t.shaft_diameter),
        ("length_below_holder", t.length_below_holder),
        ("shoulder_length", t.shoulder_length),
        ("gauge_length", t.gauge_length),
    ] {
        if !(v >= 0.0 && v.is_finite()) {
            return Err(invalid(format!("{path}.{label}: must be ≥ 0 and finite")));
        }
    }
    if t.shaft_segments.len() > 32 {
        return Err(invalid(format!("{path}: too many shaft segments")));
    }
    for (i, seg) in t.shaft_segments.iter().enumerate() {
        let sp = format!("{path}.shaft_segments[{i}]");
        for (label, v) in [("height_mm", seg.height_mm), ("upper_diameter_mm", seg.upper_diameter_mm), ("lower_diameter_mm", seg.lower_diameter_mm)] {
            if !(v > 0.0 && v.is_finite()) {
                return Err(invalid(format!("{sp}.{label}: must be strictly positive and finite")));
            }
        }
    }
    if t.cutting_presets.len() > 64 {
        return Err(invalid(format!("{path}: too many cutting presets")));
    }
    for (i, p) in t.cutting_presets.iter().enumerate() {
        let pp = format!("{path}.cutting_presets[{i}]");
        if p.spindle_rpm == 0 {
            return Err(invalid(format!("{pp}: spindle_rpm must be ≥ 1")));
        }
        for (label, v) in [
            ("cutting_feedrate", p.cutting_feedrate),
            ("plunge_feedrate", p.plunge_feedrate),
            ("lead_in_feedrate", p.lead_in_feedrate),
            ("lead_out_feedrate", p.lead_out_feedrate),
        ] {
            if !(v > 0.0 && v.is_finite()) {
                return Err(invalid(format!("{pp}.{label}: must be strictly positive and finite")));
            }
        }
    }
    if let Some(h) = &t.holder
        && !(h.gauge_length_mm >= 0.0 && h.gauge_length_mm.is_finite())
    {
        return Err(invalid(format!("{path}.holder.gauge_length_mm: must be ≥ 0 and finite")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Feeds and speeds
// ---------------------------------------------------------------------------

/// Cutting feed rates and spindle speed for a single pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Feeds {
    /// Cutting feedrate in mm/min (strictly positive).
    pub feedrate: f64,
    /// Plunge (axial entry) feedrate in mm/min (strictly positive).
    pub plunge: f64,
    /// Spindle speed in RPM (≥ 1).
    pub spindle_rpm: u32,
}

impl Default for Feeds {
    fn default() -> Self {
        Self { feedrate: 1_000.0, plunge: 300.0, spindle_rpm: 10_000 }
    }
}

/// Neutral feeds for ops that do not carry cutting parameters (e.g. Hole Recognition).
static FEEDS_PLACEHOLDER: Feeds = Feeds { feedrate: 1.0, plunge: 1.0, spindle_rpm: 1 };

fn validate_feeds(f: &Feeds, path: &str) -> Result<()> {
    if !(f.feedrate > 0.0 && f.feedrate.is_finite()) {
        return Err(invalid(format!("{path}.feedrate: must be strictly positive and finite")));
    }
    if !(f.plunge > 0.0 && f.plunge.is_finite()) {
        return Err(invalid(format!("{path}.plunge: must be strictly positive and finite")));
    }
    if f.spindle_rpm == 0 {
        return Err(invalid(format!("{path}.spindle_rpm: must be ≥ 1")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Geometric targets (model-space feature selection for CAM operations)
// ---------------------------------------------------------------------------

/// Model-space picks that select faces or edges on the design B-rep for a CAM operation.
///
/// The in-process SolidCraft kernel resolves each `face_sample_points` entry to the nearest
/// face by centroid distance in model millimetres, and rejects the operation if:
/// - the nearest face centre is more than `FACE_SELECT_TOLERANCE_MM` away, or
/// - a second face is within `FACE_SELECT_TIE_TOLERANCE_MM` of the nearest (ambiguous).
///
/// `drill_points` supplies explicit XY hole-centre coordinates for [`DrillOperation`]; the
/// kernel selects the nearest cylindrical face to each point. If both are provided on a
/// DrillOperation, `drill_points` takes precedence.
///
/// This is a SolidCraft document contract (sample points in mm). It does not name, call, or
/// depend on an external CAD scripting API.
///
/// # Backwards compatibility
/// This field is absent from existing documents.  `#[serde(default)]` plus
/// `skip_serializing_if` ensure that round-tripping old documents is lossless.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeometricTargets {
    /// Model-space points, one per intended face/edge.  Each element is `[x, y, z]` in mm.
    /// At most [`MAX_SAMPLE_POINTS_PER_OP`] entries; all coordinates must be finite.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_sample_points: Vec<[f64; 3]>,
    /// Explicit drill-hole centre coordinates in the WCS XY plane (`[x, y, z_ignored]`).
    /// At most [`MAX_DRILL_POINTS_PER_OP`] entries; all coordinates must be finite.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drill_points: Vec<[f64; 3]>,
}

impl GeometricTargets {
    /// `true` when no targets have been specified (used by `skip_serializing_if`).
    pub fn is_empty(&self) -> bool {
        self.face_sample_points.is_empty() && self.drill_points.is_empty()
    }
}

fn validate_geometric_targets(t: &GeometricTargets, path: &str) -> Result<()> {
    if t.face_sample_points.len() > MAX_SAMPLE_POINTS_PER_OP {
        return Err(invalid(format!("{path}.face_sample_points: too many entries (maximum {MAX_SAMPLE_POINTS_PER_OP})")));
    }
    if t.drill_points.len() > MAX_DRILL_POINTS_PER_OP {
        return Err(invalid(format!("{path}.drill_points: too many entries (maximum {MAX_DRILL_POINTS_PER_OP})")));
    }
    for (i, pt) in t.face_sample_points.iter().enumerate() {
        if !pt.iter().all(|x| x.is_finite()) {
            return Err(invalid(format!("{path}.face_sample_points[{i}]: all coordinates must be finite")));
        }
    }
    for (i, pt) in t.drill_points.iter().enumerate() {
        if !pt.iter().all(|x| x.is_finite()) {
            return Err(invalid(format!("{path}.drill_points[{i}]: all coordinates must be finite")));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------
//
// Each operation variant is a separate struct to allow clear field-level docs and
// independent defaults. The Operation enum uses an internally-tagged representation
// (`"type"` key) matching the pattern used elsewhere in the document model.

// ---------------------------------------------------------------------------
// Shared milling heights / coolant / high-feed (Drill + 2D Contour)
// ---------------------------------------------------------------------------

/// Coolant mode written into posts / shown on the Tool tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coolant {
    #[default]
    None,
    Flood,
    Mist,
    Air,
    ThroughSpindle,
}

impl Coolant {
    pub const ALL: &'static [Coolant] = &[Coolant::None, Coolant::Flood, Coolant::Mist, Coolant::Air, Coolant::ThroughSpindle];

    pub fn label(self) -> &'static str {
        match self {
            Coolant::None => "Disabled",
            Coolant::Flood => "Flood",
            Coolant::Mist => "Mist",
            Coolant::Air => "Air",
            Coolant::ThroughSpindle => "Through spindle",
        }
    }
}

/// Absolute reference used by a height plane (Heights tab).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeightFrom {
    ClearanceHeight,
    RetractHeight,
    FeedHeight,
    #[default]
    StockTop,
    TopHeight,
    HoleTop,
    HoleBottom,
    ModelTop,
    ModelBottom,
    /// Bottom of selected contour geometry (2D Contour).
    SelectedContours,
}

impl HeightFrom {
    pub fn label(self) -> &'static str {
        match self {
            HeightFrom::ClearanceHeight => "Clearance height",
            HeightFrom::RetractHeight => "Retract height",
            HeightFrom::FeedHeight => "Feed height",
            HeightFrom::StockTop => "Stock top",
            HeightFrom::TopHeight => "Top height",
            HeightFrom::HoleTop => "Hole top",
            HeightFrom::HoleBottom => "Hole bottom",
            HeightFrom::ModelTop => "Model top",
            HeightFrom::ModelBottom => "Model bottom",
            HeightFrom::SelectedContours => "Selected contour(s)",
        }
    }
}

/// One height plane: reference + offset (mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeightPlane {
    pub from: HeightFrom,
    pub offset_mm: f64,
}

impl Default for HeightPlane {
    fn default() -> Self {
        Self { from: HeightFrom::StockTop, offset_mm: 0.0 }
    }
}

/// Clearance / retract / feed / top / bottom (Heights) — shared by drill and contour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DrillHeights {
    pub clearance: HeightPlane,
    pub retract: HeightPlane,
    pub feed: HeightPlane,
    pub top: HeightPlane,
    pub bottom: HeightPlane,
    pub tip_through_bottom: bool,
}

impl Default for DrillHeights {
    fn default() -> Self {
        Self {
            clearance: HeightPlane { from: HeightFrom::RetractHeight, offset_mm: 10.0 },
            retract: HeightPlane { from: HeightFrom::StockTop, offset_mm: 5.0 },
            feed: HeightPlane { from: HeightFrom::TopHeight, offset_mm: 5.0 },
            top: HeightPlane { from: HeightFrom::HoleTop, offset_mm: 0.0 },
            bottom: HeightPlane { from: HeightFrom::HoleBottom, offset_mm: 0.0 },
            tip_through_bottom: false,
        }
    }
}

/// Contour height planes — same layout as [`DrillHeights`].
pub type ContourHeights = DrillHeights;

/// Absolute Z positions for the five height planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrillHeightZs {
    pub clearance: f64,
    pub retract: f64,
    pub feed: f64,
    pub top: f64,
    pub bottom: f64,
}

fn resolve_height_from(from: HeightFrom, stock_top_z: f64, depth: f64, top_z: f64, retract_z: f64, clearance_z: f64, feed_z: f64) -> f64 {
    let hole_top = stock_top_z;
    let hole_bottom = stock_top_z - depth;
    match from {
        HeightFrom::StockTop | HeightFrom::ModelTop | HeightFrom::HoleTop => hole_top,
        HeightFrom::HoleBottom | HeightFrom::ModelBottom | HeightFrom::SelectedContours => hole_bottom,
        HeightFrom::TopHeight => top_z,
        HeightFrom::RetractHeight => retract_z,
        HeightFrom::ClearanceHeight => clearance_z,
        HeightFrom::FeedHeight => feed_z,
    }
}

/// Resolve the five height planes for a given stock top and cut depth.
pub fn resolve_height_zs(heights: &DrillHeights, stock_top_z: f64, depth: f64) -> DrillHeightZs {
    let mut top = stock_top_z;
    let mut retract = stock_top_z;
    let mut clearance = stock_top_z;
    let mut feed = stock_top_z;
    top = resolve_height_from(heights.top.from, stock_top_z, depth, top, retract, clearance, feed) + heights.top.offset_mm;
    feed = resolve_height_from(heights.feed.from, stock_top_z, depth, top, retract, clearance, feed) + heights.feed.offset_mm;
    retract = resolve_height_from(heights.retract.from, stock_top_z, depth, top, retract, clearance, feed) + heights.retract.offset_mm;
    clearance = resolve_height_from(heights.clearance.from, stock_top_z, depth, top, retract, clearance, feed) + heights.clearance.offset_mm;
    let bottom = resolve_height_from(heights.bottom.from, stock_top_z, depth, top, retract, clearance, feed) + heights.bottom.offset_mm;
    DrillHeightZs { clearance, retract, feed, top, bottom }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighFeedMode {
    #[default]
    PreserveRapid,
    AlwaysUseHighFeed,
    NeverUseHighFeed,
}

impl HighFeedMode {
    pub fn label(self) -> &'static str {
        match self {
            HighFeedMode::PreserveRapid => "Preserve rapid movement",
            HighFeedMode::AlwaysUseHighFeed => "Always use high feedrate",
            HighFeedMode::NeverUseHighFeed => "Never use high feedrate",
        }
    }
}

/// Which side of the profile contour to machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSide {
    #[default]
    Outside,
    Inside,
    /// Tool centreline follows the contour exactly.
    On,
}

impl ProfileSide {
    pub fn label(self) -> &'static str {
        match self {
            ProfileSide::Outside => "Outside",
            ProfileSide::Inside => "Inside",
            ProfileSide::On => "On",
        }
    }
}

/// Sideways cutter compensation (Passes tab).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidewaysCompensation {
    #[default]
    Left,
    Right,
    Center,
}

impl SidewaysCompensation {
    pub fn label(self) -> &'static str {
        match self {
            SidewaysCompensation::Left => "Left (climb)",
            SidewaysCompensation::Right => "Right (conventional)",
            SidewaysCompensation::Center => "Center (on)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationType {
    #[default]
    InComputer,
    InControl,
    Off,
}

impl CompensationType {
    pub fn label(self) -> &'static str {
        match self {
            CompensationType::InComputer => "In computer",
            CompensationType::InControl => "In control",
            CompensationType::Off => "Off",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabShape {
    #[default]
    Rectangular,
    Triangular,
}

impl TabShape {
    pub fn label(self) -> &'static str {
        match self {
            TabShape::Rectangular => "Rectangular",
            TabShape::Triangular => "Triangular",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RampType {
    #[default]
    Plunge,
    ZigZag,
    Helix,
}

impl RampType {
    pub fn label(self) -> &'static str {
        match self {
            RampType::Plunge => "Plunge",
            RampType::ZigZag => "Zig-zag",
            RampType::Helix => "Helix",
        }
    }
}

/// Holding tabs on a 2D contour (Geometry → Tabs).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContourTabs {
    pub enabled: bool,
    pub shape: TabShape,
    pub width_mm: f64,
    pub height_mm: f64,
    /// When true, place tabs by distance; otherwise by count (`count`).
    pub by_distance: bool,
    pub spacing_mm: f64,
    pub count: u32,
}

impl Default for ContourTabs {
    fn default() -> Self {
        Self { enabled: false, shape: TabShape::Rectangular, width_mm: 3.0, height_mm: 0.4, by_distance: true, spacing_mm: 40.0, count: 4 }
    }
}

/// Geometry options for a 2D Contour (Geometry tab).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct ContourGeometryOpts {
    pub rest_machining: bool,
    pub stock_contours: bool,
    pub tabs: ContourTabs,
}

/// Passes / depths for a 2D Contour (Passes tab).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContourPasses {
    pub tolerance_mm: f64,
    pub sideways: SidewaysCompensation,
    pub compensation_type: CompensationType,
    pub multiple_depths: bool,
    pub max_stepdown_mm: f64,
    pub use_even_stepdowns: bool,
    pub roughing_passes: bool,
    pub finishing_stepdowns: u32,
    pub finishing_stepdown_mm: f64,
}

impl Default for ContourPasses {
    fn default() -> Self {
        Self {
            tolerance_mm: 0.01,
            sideways: SidewaysCompensation::Left,
            compensation_type: CompensationType::InComputer,
            multiple_depths: true,
            max_stepdown_mm: 0.8,
            use_even_stepdowns: true,
            roughing_passes: false,
            finishing_stepdowns: 0,
            finishing_stepdown_mm: 0.2,
        }
    }
}

/// Linking / leads / ramp for a 2D Contour (Linking tab).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContourLinking {
    pub high_feed_mode: HighFeedMode,
    pub allow_rapid_retract: bool,
    pub safe_distance_mm: f64,
    pub keep_tool_down: bool,
    pub max_stay_down_mm: f64,
    pub lift_height_mm: f64,
    pub lead_in: bool,
    pub lead_in_radius_mm: f64,
    pub lead_in_sweep_deg: f64,
    pub lead_in_linear_mm: f64,
    pub lead_out: bool,
    pub ramp_type: RampType,
    pub ramp_angle_deg: f64,
    pub max_ramp_stepdown_mm: f64,
    pub ramp_clearance_mm: f64,
}

impl Default for ContourLinking {
    fn default() -> Self {
        Self {
            high_feed_mode: HighFeedMode::PreserveRapid,
            allow_rapid_retract: false,
            safe_distance_mm: 0.34,
            keep_tool_down: true,
            max_stay_down_mm: 50.0,
            lift_height_mm: 0.0,
            lead_in: true,
            lead_in_radius_mm: 0.85,
            lead_in_sweep_deg: 90.0,
            lead_in_linear_mm: 0.17,
            lead_out: false,
            ramp_type: RampType::Plunge,
            ramp_angle_deg: 3.0,
            max_ramp_stepdown_mm: 1.0,
            ramp_clearance_mm: 0.5,
        }
    }
}

/// 2D Contour / 2.5D profile (Tool / Geometry / Heights / Passes / Linking).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileOperation {
    /// Operator-facing name (browser + dialog title).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    /// Lead-in feedrate mm/min (strictly positive).
    #[serde(default = "default_lead_feed")]
    pub lead_in_feedrate: f64,
    /// Lead-out feedrate mm/min (strictly positive).
    #[serde(default = "default_lead_feed")]
    pub lead_out_feedrate: f64,
    pub side: ProfileSide,
    /// Axial depth of cut in mm (strictly positive).
    pub depth: f64,
    /// Number of radial finishing passes (≥ 1).
    pub passes: u32,
    /// Stock left on walls in mm (≥ 0); 0 = finish pass.
    pub finish_allowance: f64,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub pass_opts: ContourPasses,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default)]
    pub geometry: ContourGeometryOpts,
    /// Geometric targets: face/edge sample points used to select the profile contour
    /// on the design body. Optional; absent from older documents.
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

fn default_lead_feed() -> f64 {
    300.0
}

impl Default for ProfileOperation {
    fn default() -> Self {
        Self {
            name: "2D Contour".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 600.0, plunge: 200.0, spindle_rpm: 18_000 },
            coolant: Coolant::Air,
            lead_in_feedrate: 300.0,
            lead_out_feedrate: 500.0,
            side: ProfileSide::Outside,
            depth: 3.0,
            passes: 1,
            finish_allowance: 0.0,
            heights: DrillHeights {
                clearance: HeightPlane { from: HeightFrom::RetractHeight, offset_mm: 10.0 },
                retract: HeightPlane { from: HeightFrom::StockTop, offset_mm: 5.0 },
                feed: HeightPlane { from: HeightFrom::TopHeight, offset_mm: 5.0 },
                top: HeightPlane { from: HeightFrom::StockTop, offset_mm: 0.0 },
                bottom: HeightPlane { from: HeightFrom::SelectedContours, offset_mm: 0.0 },
                tip_through_bottom: false,
            },
            pass_opts: ContourPasses::default(),
            linking: ContourLinking::default(),
            geometry: ContourGeometryOpts::default(),
            targets: GeometricTargets::default(),
        }
    }
}

impl ProfileOperation {
    /// Resolved clearance / retract / feed / top / bottom Z (stock top at `stock_top_z`).
    pub fn height_zs(&self, stock_top_z: f64) -> DrillHeightZs {
        resolve_height_zs(&self.heights, stock_top_z, self.depth)
    }

    /// Contour sample points for overlays (`face_sample_points`, else drill_points).
    pub fn contour_points(&self) -> &[[f64; 3]] {
        if !self.targets.face_sample_points.is_empty() { &self.targets.face_sample_points } else { &self.targets.drill_points }
    }
}

/// Face milling (2D → Face).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FaceOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub depth: f64,
    pub stepover: f64,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default)]
    pub pass_opts: ContourPasses,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for FaceOperation {
    fn default() -> Self {
        Self {
            name: "Face".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 1_000.0, plunge: 300.0, spindle_rpm: 12_000 },
            coolant: Coolant::Air,
            depth: 0.5,
            stepover: 0.75,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            pass_opts: ContourPasses::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// 2D Offset Roughing / Pocket (2D → 2D Pocket).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PocketOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub depth: f64,
    pub stepover: f64,
    pub finish_allowance: f64,
    #[serde(default)]
    pub stock_contours: bool,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default)]
    pub pass_opts: ContourPasses,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for PocketOperation {
    fn default() -> Self {
        Self {
            name: "2D Pocket".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 800.0, plunge: 200.0, spindle_rpm: 12_000 },
            coolant: Coolant::Flood,
            depth: 3.0,
            stepover: 0.45,
            finish_allowance: 0.1,
            stock_contours: false,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            pass_opts: ContourPasses::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// Drilling cycle variant (Cycle tab).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillCycle {
    /// Single-shot plunge / rapid out (G81).
    #[default]
    Simple,
    /// Full retract on each peck (G83).
    Peck,
    /// Partial retract chip-breaking (G73).
    ChipBreak,
    /// Dwell at full depth (G82).
    Dwell,
}

impl DrillCycle {
    pub fn label(self) -> &'static str {
        match self {
            DrillCycle::Simple => "Drilling - rapid out",
            DrillCycle::Peck => "Deep drilling - full retract",
            DrillCycle::ChipBreak => "Chip breaking - partial retract",
            DrillCycle::Dwell => "Counterboring - dwell",
        }
    }
}

/// Linking moves between holes (Linking tab).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetractionPolicy {
    #[default]
    AvoidCollisions,
    FullRetract,
    MinimumRetract,
}

impl RetractionPolicy {
    pub fn label(self) -> &'static str {
        match self {
            RetractionPolicy::AvoidCollisions => "Avoid collisions",
            RetractionPolicy::FullRetract => "Full retract",
            RetractionPolicy::MinimumRetract => "Minimum retract",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DrillLinking {
    pub ignore_gouges: bool,
    pub retraction_policy: RetractionPolicy,
    pub high_feed_mode: HighFeedMode,
    pub safe_distance_mm: f64,
}

impl Default for DrillLinking {
    fn default() -> Self {
        Self {
            ignore_gouges: false,
            retraction_policy: RetractionPolicy::AvoidCollisions,
            high_feed_mode: HighFeedMode::PreserveRapid,
            safe_distance_mm: 2.0,
        }
    }
}

/// Hole selection / ordering options (Geometry tab).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillOrder {
    #[default]
    ShortestDistance,
    XThenY,
    YThenX,
    OrderOfSelection,
}

impl DrillOrder {
    pub fn label(self) -> &'static str {
        match self {
            DrillOrder::ShortestDistance => "Shortest distance",
            DrillOrder::XThenY => "X then Y",
            DrillOrder::YThenX => "Y then X",
            DrillOrder::OrderOfSelection => "Order of selection",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DrillGeometryOpts {
    pub select_same_diameter: bool,
    pub only_same_hole_depth: bool,
    pub only_same_z_top: bool,
    pub order: DrillOrder,
    pub reverse_order: bool,
}

impl Default for DrillGeometryOpts {
    fn default() -> Self {
        Self {
            select_same_diameter: true,
            only_same_hole_depth: false,
            only_same_z_top: false,
            order: DrillOrder::ShortestDistance,
            reverse_order: false,
        }
    }
}

/// Action assigned to a recognized hole diameter group (Hole Recognition → Hole Groups).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleAction {
    #[default]
    Drill,
    SpotDrill,
    SpotAndDrill,
    CounterBore,
    Bore,
    Tap,
    Ream,
    Ignore,
}

impl HoleAction {
    pub fn label(self) -> &'static str {
        match self {
            HoleAction::Drill => "Drilling",
            HoleAction::SpotDrill => "Spot drilling",
            HoleAction::SpotAndDrill => "Spot + Drill",
            HoleAction::CounterBore => "Counter boring",
            HoleAction::Bore => "Boring",
            HoleAction::Tap => "Tapping",
            HoleAction::Ream => "Reaming",
            HoleAction::Ignore => "Ignore",
        }
    }

    pub fn needs_thread(self) -> bool {
        matches!(self, HoleAction::Tap)
    }
}

/// How generated hole operations are ordered (Hole Recognition → Options).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleOrganizeMode {
    #[default]
    MinimizeToolChanges,
    GroupBySize,
}

impl HoleOrganizeMode {
    pub fn label(self) -> &'static str {
        match self {
            HoleOrganizeMode::MinimizeToolChanges => "Minimize tool changes",
            HoleOrganizeMode::GroupBySize => "Group by size",
        }
    }
}

/// One diameter cohort discovered by Hole Recognition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HoleGroup {
    pub diameter_mm: f64,
    pub depth_mm: f64,
    pub count: u32,
    pub action: HoleAction,
    /// Thread designation when [`HoleAction::Tap`] (e.g. M6x1).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thread_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub centres: Vec<[f64; 3]>,
}

impl Default for HoleGroup {
    fn default() -> Self {
        Self { diameter_mm: 6.0, depth_mm: 10.0, count: 1, action: HoleAction::Drill, thread_type: String::new(), centres: Vec::new() }
    }
}

/// Hole Recognition wizard (Drilling → Hole Recognition).
///
/// Persists groups + options; Generate later expands into individual drill/tap ops.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HoleRecognitionOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default)]
    pub groups: Vec<HoleGroup>,
    /// Tool Libraries tab — include tools from the active setup / document.
    pub use_document_tools: bool,
    pub use_metric_sample_library: bool,
    pub use_inch_sample_library: bool,
    /// Options tab.
    pub multi_axis_machining: bool,
    pub include_partial_holes: bool,
    pub find_by_angle: bool,
    pub min_angle_deg: f64,
    pub max_angle_deg: f64,
    pub find_by_diameter: bool,
    pub max_diameter_mm: f64,
    pub organize: HoleOrganizeMode,
    pub fewest_spot_drills: bool,
    /// Optional Template Library folder path / name (empty = all templates).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub template_folder: String,
}

impl Default for HoleRecognitionOperation {
    fn default() -> Self {
        Self {
            name: "Hole Recognition".into(),
            groups: Vec::new(),
            use_document_tools: true,
            use_metric_sample_library: true,
            use_inch_sample_library: true,
            multi_axis_machining: false,
            include_partial_holes: false,
            find_by_angle: false,
            min_angle_deg: 0.0,
            max_angle_deg: 90.0,
            find_by_diameter: false,
            max_diameter_mm: 50.0,
            organize: HoleOrganizeMode::MinimizeToolChanges,
            fewest_spot_drills: true,
            template_folder: String::new(),
        }
    }
}

impl HoleRecognitionOperation {
    pub fn actionable_groups(&self) -> impl Iterator<Item = &HoleGroup> {
        self.groups.iter().filter(|g| g.action != HoleAction::Ignore)
    }
}

/// Drilling operation (Drill dialog: Tool / Geometry / Heights / Cycle / Linking).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrillOperation {
    /// Operator-facing name (browser + dialog title).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub cycle: DrillCycle,
    /// Hole depth in mm (strictly positive). Used when bottom height is not fully resolved.
    pub depth: f64,
    /// Peck depth increment in mm; used for `Peck` and `ChipBreak` cycles (strictly positive).
    pub peck_depth: f64,
    /// Dwell duration at full depth in seconds (≥ 0); used for `Dwell` cycle.
    pub dwell_s: f64,
    #[serde(default)]
    pub heights: DrillHeights,
    #[serde(default)]
    pub linking: DrillLinking,
    #[serde(default)]
    pub geometry: DrillGeometryOpts,
    /// Geometric targets: explicit drill-centre XY points or face sample points used to
    /// select hole faces on the design body. `drill_points` takes precedence over
    /// `face_sample_points` when both are present.
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for DrillOperation {
    fn default() -> Self {
        Self {
            name: "Drill holes".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 1_000.0, plunge: 200.0, spindle_rpm: 18_000 },
            coolant: Coolant::Air,
            cycle: DrillCycle::Simple,
            depth: 10.0,
            peck_depth: 2.0,
            dwell_s: 0.0,
            heights: DrillHeights::default(),
            linking: DrillLinking::default(),
            geometry: DrillGeometryOpts::default(),
            targets: GeometricTargets::default(),
        }
    }
}

impl DrillOperation {
    /// Resolved clearance / retract / feed / top / bottom Z for overlays and posts.
    pub fn height_zs(&self, stock_top_z: f64) -> DrillHeightZs {
        resolve_height_zs(&self.heights, stock_top_z, self.depth)
    }

    /// Hole centres used for path indicators (`drill_points`, else face samples).
    pub fn hole_centres(&self) -> &[[f64; 3]] {
        if !self.targets.drill_points.is_empty() { &self.targets.drill_points } else { &self.targets.face_sample_points }
    }
}

/// 2D Adaptive Roughing (2D → 2D Adaptive Clearing).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdaptiveOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub depth: f64,
    /// Optimal radial engagement (load) as a fraction of the tool diameter; must be in (0, 1].
    pub optimal_load: f64,
    pub finish_allowance: f64,
    #[serde(default)]
    pub stock_contours: bool,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default)]
    pub pass_opts: ContourPasses,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for AdaptiveOperation {
    fn default() -> Self {
        Self {
            name: "2D Adaptive".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 1_200.0, plunge: 200.0, spindle_rpm: 14_000 },
            coolant: Coolant::Air,
            depth: 4.0,
            optimal_load: 0.35,
            finish_allowance: 0.1,
            stock_contours: false,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            pass_opts: ContourPasses::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// Slot milling (2D → Slot).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlotOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub depth: f64,
    pub finish_allowance: f64,
    #[serde(default)]
    pub backoff_mm: f64,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default)]
    pub pass_opts: ContourPasses,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for SlotOperation {
    fn default() -> Self {
        Self {
            name: "Slot".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 600.0, plunge: 150.0, spindle_rpm: 12_000 },
            coolant: Coolant::Flood,
            depth: 3.0,
            finish_allowance: 0.0,
            backoff_mm: 0.5,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            pass_opts: ContourPasses::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// Trace along edges/curves (2D → Trace).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TraceOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub depth: f64,
    /// Axial offset below the selected curve (mm).
    pub axial_offset: f64,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for TraceOperation {
    fn default() -> Self {
        Self {
            name: "Trace".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 500.0, plunge: 150.0, spindle_rpm: 10_000 },
            coolant: Coolant::Air,
            depth: 1.0,
            axial_offset: 0.0,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// Thread milling (2D → Thread).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub depth: f64,
    /// Thread pitch in mm.
    pub pitch_mm: f64,
    pub major_diameter: f64,
    pub internal: bool,
    pub do_multiple_threads: bool,
    pub thread_count: u32,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for ThreadOperation {
    fn default() -> Self {
        Self {
            name: "Thread".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 400.0, plunge: 100.0, spindle_rpm: 6_000 },
            coolant: Coolant::Flood,
            depth: 10.0,
            pitch_mm: 1.25,
            major_diameter: 8.0,
            internal: true,
            do_multiple_threads: false,
            thread_count: 1,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// Helical bore (2D → Bore).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoreOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub depth: f64,
    pub diameter: f64,
    pub pitch_mm: f64,
    pub finish_allowance: f64,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default)]
    pub pass_opts: ContourPasses,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for BoreOperation {
    fn default() -> Self {
        Self {
            name: "Bore".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 500.0, plunge: 150.0, spindle_rpm: 8_000 },
            coolant: Coolant::Flood,
            depth: 10.0,
            diameter: 12.0,
            pitch_mm: 2.0,
            finish_allowance: 0.05,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            pass_opts: ContourPasses::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// Circular milling of holes/bosses (2D → Circular).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CircularOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub depth: f64,
    pub diameter: f64,
    pub outside: bool,
    pub finish_allowance: f64,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default)]
    pub pass_opts: ContourPasses,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for CircularOperation {
    fn default() -> Self {
        Self {
            name: "Circular".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 600.0, plunge: 150.0, spindle_rpm: 10_000 },
            coolant: Coolant::Flood,
            depth: 5.0,
            diameter: 20.0,
            outside: false,
            finish_allowance: 0.0,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            pass_opts: ContourPasses::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// Engrave with pointed tools (2D → Engrave).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngraveOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub depth: f64,
    pub tip_angle_deg: f64,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for EngraveOperation {
    fn default() -> Self {
        Self {
            name: "Engrave".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 300.0, plunge: 100.0, spindle_rpm: 16_000 },
            coolant: Coolant::Air,
            depth: 0.3,
            tip_angle_deg: 60.0,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// 2D Chamfer (2D → 2D Chamfer).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChamferOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub depth: f64,
    pub chamfer_width: f64,
    pub tip_offset: f64,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for ChamferOperation {
    fn default() -> Self {
        Self {
            name: "2D Chamfer".into(),
            tool_number: 1,
            feeds: Feeds { feedrate: 400.0, plunge: 100.0, spindle_rpm: 12_000 },
            coolant: Coolant::Air,
            depth: 0.5,
            chamfer_width: 0.5,
            tip_offset: 0.1,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// Manufacture → 3D milling strategy (shared Tool / Geometry / Heights / Passes / Linking).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreedStrategy {
    #[default]
    AdaptiveRoughing,
    OffsetRoughing,
    ThreePlusTwoRoughing,
    SteepAndShallow,
    Flat,
    Parallel,
    Scallop,
    Contour,
    Ramp,
    Pencil,
    Horizontal,
    Spiral,
    Radial,
    MorphedSpiral,
    Project,
    Blend,
    Morph,
    Corner,
    Flow,
    Deburr,
    Geodesic,
}

impl ThreedStrategy {
    pub fn label(self) -> &'static str {
        match self {
            ThreedStrategy::AdaptiveRoughing => "3D Adaptive Roughing",
            ThreedStrategy::OffsetRoughing => "3D Offset Roughing (Pocket)",
            ThreedStrategy::ThreePlusTwoRoughing => "3+2 Roughing",
            ThreedStrategy::SteepAndShallow => "Steep and Shallow",
            ThreedStrategy::Flat => "Flat",
            ThreedStrategy::Parallel => "Parallel",
            ThreedStrategy::Scallop => "Scallop",
            ThreedStrategy::Contour => "3D Contour",
            ThreedStrategy::Ramp => "Ramp",
            ThreedStrategy::Pencil => "Pencil",
            ThreedStrategy::Horizontal => "Horizontal",
            ThreedStrategy::Spiral => "Spiral",
            ThreedStrategy::Radial => "Radial",
            ThreedStrategy::MorphedSpiral => "Morphed Spiral",
            ThreedStrategy::Project => "Project",
            ThreedStrategy::Blend => "Blend",
            ThreedStrategy::Morph => "Morph",
            ThreedStrategy::Corner => "Corner",
            ThreedStrategy::Flow => "Flow",
            ThreedStrategy::Deburr => "Deburr",
            ThreedStrategy::Geodesic => "Geodesic",
        }
    }

    pub fn is_roughing(self) -> bool {
        matches!(self, ThreedStrategy::AdaptiveRoughing | ThreedStrategy::OffsetRoughing | ThreedStrategy::ThreePlusTwoRoughing)
    }
}

/// Shallow-area strategy used by Steep and Shallow (Passes tab).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShallowStyle {
    #[default]
    Parallel,
    Scallop,
}

impl ShallowStyle {
    pub fn label(self) -> &'static str {
        match self {
            ShallowStyle::Parallel => "Parallel",
            ShallowStyle::Scallop => "Scallop",
        }
    }
}

/// 3D milling operation (3D menu — one dialog shell, strategy-specific passes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreedOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub strategy: ThreedStrategy,
    /// Overall cut depth / stock height envelope (mm).
    pub depth: f64,
    /// Adaptive optimal load (0–1]; also used as engagement hint for offset roughing.
    pub optimal_load: f64,
    /// Radial stepover as a fraction of tool diameter (finishing / offset).
    pub stepover: f64,
    /// Scallop / cusp height in mm (Scallop, Morphed Spiral).
    pub cusp_height_mm: f64,
    /// Parallel / Radial pass angle (deg).
    pub pass_direction_deg: f64,
    /// Steep/shallow threshold angle (deg from horizontal).
    pub threshold_angle_deg: f64,
    pub shallow_style: ShallowStyle,
    pub radial_stock_mm: f64,
    pub axial_stock_mm: f64,
    pub machine_shallow_areas: bool,
    pub flat_area_detection: bool,
    pub use_slot_clearing: bool,
    pub use_morphed_spiral: bool,
    pub max_stepdown_mm: f64,
    pub fine_stepdown_mm: f64,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default)]
    pub pass_opts: ContourPasses,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for ThreedOperation {
    fn default() -> Self {
        Self {
            name: String::new(),
            tool_number: 1,
            feeds: Feeds { feedrate: 1_000.0, plunge: 200.0, spindle_rpm: 12_000 },
            coolant: Coolant::Air,
            strategy: ThreedStrategy::AdaptiveRoughing,
            depth: 10.0,
            optimal_load: 0.4,
            stepover: 0.4,
            cusp_height_mm: 0.1,
            pass_direction_deg: 0.0,
            threshold_angle_deg: 45.0,
            shallow_style: ShallowStyle::Scallop,
            radial_stock_mm: 0.2,
            axial_stock_mm: 0.1,
            machine_shallow_areas: true,
            flat_area_detection: true,
            use_slot_clearing: false,
            use_morphed_spiral: false,
            max_stepdown_mm: 1.0,
            fine_stepdown_mm: 0.25,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            pass_opts: ContourPasses::default(),
            targets: GeometricTargets::default(),
        }
    }
}

impl ThreedOperation {
    pub fn with_strategy(strategy: ThreedStrategy) -> Self {
        let mut op = Self { strategy, name: strategy.label().into(), ..Default::default() };
        match strategy {
            ThreedStrategy::AdaptiveRoughing => {
                op.optimal_load = 0.4;
                op.max_stepdown_mm = 2.0;
            }
            ThreedStrategy::OffsetRoughing => {
                op.stepover = 0.45;
                op.use_morphed_spiral = false;
            }
            ThreedStrategy::ThreePlusTwoRoughing => {
                op.optimal_load = 0.35;
            }
            ThreedStrategy::SteepAndShallow => {
                op.threshold_angle_deg = 45.0;
                op.shallow_style = ShallowStyle::Scallop;
            }
            ThreedStrategy::Scallop | ThreedStrategy::MorphedSpiral => {
                op.cusp_height_mm = 0.1;
            }
            ThreedStrategy::Parallel | ThreedStrategy::Radial => {
                op.stepover = 0.35;
                op.pass_direction_deg = 0.0;
            }
            ThreedStrategy::Deburr => {
                op.depth = 0.2;
                op.axial_stock_mm = 0.0;
            }
            _ => {}
        }
        op
    }

    pub fn height_zs(&self, stock_top_z: f64) -> DrillHeightZs {
        resolve_height_zs(&self.heights, stock_top_z, self.depth)
    }
}

/// Manufacture → Multi-Axis milling strategy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiAxisStrategy {
    #[default]
    RotaryOffsetRoughing,
    MultiAxisRoughing,
    Swarf,
    AdvancedSwarf,
    MultiAxisContour,
    RotaryParallel,
    RotaryContour,
    MultiAxisFinishing,
}

impl MultiAxisStrategy {
    pub fn label(self) -> &'static str {
        match self {
            MultiAxisStrategy::RotaryOffsetRoughing => "Rotary Offset Roughing (Pocket)",
            MultiAxisStrategy::MultiAxisRoughing => "Multi-Axis Roughing",
            MultiAxisStrategy::Swarf => "Swarf",
            MultiAxisStrategy::AdvancedSwarf => "Advanced Swarf",
            MultiAxisStrategy::MultiAxisContour => "Multi-Axis Contour",
            MultiAxisStrategy::RotaryParallel => "Rotary Parallel",
            MultiAxisStrategy::RotaryContour => "Rotary Contour",
            MultiAxisStrategy::MultiAxisFinishing => "Multi-Axis Finishing",
        }
    }

    pub fn is_rotary(self) -> bool {
        matches!(self, MultiAxisStrategy::RotaryOffsetRoughing | MultiAxisStrategy::RotaryParallel | MultiAxisStrategy::RotaryContour)
    }

    pub fn is_swarf(self) -> bool {
        matches!(self, MultiAxisStrategy::Swarf | MultiAxisStrategy::AdvancedSwarf)
    }
}

/// Rotary Parallel / Contour pass style (Passes → Rotary Passes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotaryPassStyle {
    #[default]
    Spiral,
    Line,
    Circular,
}

impl RotaryPassStyle {
    pub fn label(self) -> &'static str {
        match self {
            RotaryPassStyle::Spiral => "Spiral",
            RotaryPassStyle::Line => "Line",
            RotaryPassStyle::Circular => "Circular",
        }
    }
}

/// Swarf drive mode (Geometry → Drive Mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarfDriveMode {
    #[default]
    Contours,
    Surfaces,
}

impl SwarfDriveMode {
    pub fn label(self) -> &'static str {
        match self {
            SwarfDriveMode::Contours => "Contours",
            SwarfDriveMode::Surfaces => "Surfaces",
        }
    }
}

/// Advanced Swarf geometry selection mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvSwarfGeometryMode {
    #[default]
    Surfaces,
    GuideCurves,
}

impl AdvSwarfGeometryMode {
    pub fn label(self) -> &'static str {
        match self {
            AdvSwarfGeometryMode::Surfaces => "Surfaces",
            AdvSwarfGeometryMode::GuideCurves => "Guide Curves",
        }
    }
}

/// Machining type on the Multi-Axis tab (Advanced Swarf / Finishing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiAxisMachiningType {
    ThreeAxis,
    #[default]
    FourAxis,
    FiveAxis,
}

impl MultiAxisMachiningType {
    pub fn label(self) -> &'static str {
        match self {
            MultiAxisMachiningType::ThreeAxis => "3-axis",
            MultiAxisMachiningType::FourAxis => "4-axis",
            MultiAxisMachiningType::FiveAxis => "5-axis",
        }
    }
}

/// Rotary axis orientation (Geometry → Orientation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotaryAxisOrientation {
    #[default]
    SetupX,
    SetupY,
    SetupZ,
    Selection,
}

impl RotaryAxisOrientation {
    pub fn label(self) -> &'static str {
        match self {
            RotaryAxisOrientation::SetupX => "Setup X Axis",
            RotaryAxisOrientation::SetupY => "Setup Y Axis",
            RotaryAxisOrientation::SetupZ => "Setup Z Axis",
            RotaryAxisOrientation::Selection => "Selection",
        }
    }
}

/// Multi-Axis Finishing pattern (floor/wall).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiAxisFinishPattern {
    #[default]
    Scallop,
    Blend,
}

impl MultiAxisFinishPattern {
    pub fn label(self) -> &'static str {
        match self {
            MultiAxisFinishPattern::Scallop => "Scallop",
            MultiAxisFinishPattern::Blend => "Blend",
        }
    }
}

/// Multi-axis milling operation (MULTI-AXIS menu).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MultiAxisOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub strategy: MultiAxisStrategy,
    pub depth: f64,
    pub stepover: f64,
    /// Angular stepover (deg) for Line-style rotary passes.
    pub stepover_angle_deg: f64,
    pub max_stepdown_mm: f64,
    pub radial_stock_mm: f64,
    pub axial_stock_mm: f64,
    /// Swarf: flute engagement offset along tool axis (mm).
    pub tool_offset_mm: f64,
    pub rotary_style: RotaryPassStyle,
    pub swarf_drive: SwarfDriveMode,
    pub adv_geometry: AdvSwarfGeometryMode,
    pub machining_type: MultiAxisMachiningType,
    pub rotary_orientation: RotaryAxisOrientation,
    pub finish_pattern: MultiAxisFinishPattern,
    pub set_angular_limits: bool,
    pub start_angle_deg: f64,
    pub end_angle_deg: f64,
    pub lead_angle_deg: f64,
    pub tilt_angle_deg: f64,
    pub check_for_gouges: bool,
    pub multiple_depths: bool,
    pub cusp_height_mm: f64,
    #[serde(default)]
    pub heights: ContourHeights,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default)]
    pub pass_opts: ContourPasses,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for MultiAxisOperation {
    fn default() -> Self {
        Self {
            name: String::new(),
            tool_number: 1,
            feeds: Feeds { feedrate: 800.0, plunge: 150.0, spindle_rpm: 10_000 },
            coolant: Coolant::Flood,
            strategy: MultiAxisStrategy::RotaryOffsetRoughing,
            depth: 10.0,
            stepover: 0.4,
            stepover_angle_deg: 5.0,
            max_stepdown_mm: 1.5,
            radial_stock_mm: 0.2,
            axial_stock_mm: 0.1,
            tool_offset_mm: 0.0,
            rotary_style: RotaryPassStyle::Spiral,
            swarf_drive: SwarfDriveMode::Contours,
            adv_geometry: AdvSwarfGeometryMode::Surfaces,
            machining_type: MultiAxisMachiningType::FourAxis,
            rotary_orientation: RotaryAxisOrientation::SetupX,
            finish_pattern: MultiAxisFinishPattern::Scallop,
            set_angular_limits: false,
            start_angle_deg: 0.0,
            end_angle_deg: 360.0,
            lead_angle_deg: 0.0,
            tilt_angle_deg: 0.0,
            check_for_gouges: true,
            multiple_depths: false,
            cusp_height_mm: 0.1,
            heights: ContourHeights::default(),
            linking: ContourLinking::default(),
            pass_opts: ContourPasses::default(),
            targets: GeometricTargets::default(),
        }
    }
}

impl MultiAxisOperation {
    pub fn with_strategy(strategy: MultiAxisStrategy) -> Self {
        let mut op = Self { strategy, name: strategy.label().into(), ..Default::default() };
        match strategy {
            MultiAxisStrategy::RotaryOffsetRoughing => {
                op.stepover = 0.45;
                op.max_stepdown_mm = 2.0;
                op.machining_type = MultiAxisMachiningType::FourAxis;
            }
            MultiAxisStrategy::MultiAxisRoughing => {
                op.stepover = 0.4;
                op.max_stepdown_mm = 2.0;
                op.machining_type = MultiAxisMachiningType::FiveAxis;
            }
            MultiAxisStrategy::Swarf => {
                op.swarf_drive = SwarfDriveMode::Contours;
                op.tool_offset_mm = 2.0;
                op.machining_type = MultiAxisMachiningType::FiveAxis;
            }
            MultiAxisStrategy::AdvancedSwarf => {
                op.adv_geometry = AdvSwarfGeometryMode::Surfaces;
                op.check_for_gouges = true;
                op.machining_type = MultiAxisMachiningType::FiveAxis;
            }
            MultiAxisStrategy::MultiAxisContour => {
                op.stepover = 0.3;
                op.machining_type = MultiAxisMachiningType::FiveAxis;
            }
            MultiAxisStrategy::RotaryParallel => {
                op.rotary_style = RotaryPassStyle::Spiral;
                op.stepover = 0.35;
                op.machining_type = MultiAxisMachiningType::FourAxis;
            }
            MultiAxisStrategy::RotaryContour => {
                op.rotary_style = RotaryPassStyle::Circular;
                op.stepover = 0.3;
                op.machining_type = MultiAxisMachiningType::FourAxis;
            }
            MultiAxisStrategy::MultiAxisFinishing => {
                op.finish_pattern = MultiAxisFinishPattern::Scallop;
                op.cusp_height_mm = 0.05;
                op.machining_type = MultiAxisMachiningType::FiveAxis;
            }
        }
        op
    }

    pub fn height_zs(&self, stock_top_z: f64) -> DrillHeightZs {
        resolve_height_zs(&self.heights, stock_top_z, self.depth)
    }
}

/// Spindle rotation for turning (Tool tab).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurningSpindleRotation {
    #[default]
    Forward,
    Reverse,
}

impl TurningSpindleRotation {
    pub fn label(self) -> &'static str {
        match self {
            TurningSpindleRotation::Forward => "Forward (CW)",
            TurningSpindleRotation::Reverse => "Reverse (CCW)",
        }
    }
}

/// Outside vs inside turning (OD / ID).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurningMode {
    #[default]
    Outside,
    Inside,
}

impl TurningMode {
    pub fn label(self) -> &'static str {
        match self {
            TurningMode::Outside => "Outside",
            TurningMode::Inside => "Inside",
        }
    }
}

/// Manufacture → Turning strategies (Turning tab menu order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurningStrategy {
    #[default]
    Face,
    ProfileRoughing,
    ProfileFinishing,
    GrooveRoughing,
    GrooveFinishing,
    AdaptiveRoughing,
    Groove,
    SingleGroove,
    Thread,
    Chamfer,
    Part,
    Trace,
}

impl TurningStrategy {
    pub const ALL: &'static [TurningStrategy] = &[
        TurningStrategy::Face,
        TurningStrategy::ProfileRoughing,
        TurningStrategy::ProfileFinishing,
        TurningStrategy::GrooveRoughing,
        TurningStrategy::GrooveFinishing,
        TurningStrategy::AdaptiveRoughing,
        TurningStrategy::Groove,
        TurningStrategy::SingleGroove,
        TurningStrategy::Thread,
        TurningStrategy::Chamfer,
        TurningStrategy::Part,
        TurningStrategy::Trace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TurningStrategy::Face => "Turning Face",
            TurningStrategy::ProfileRoughing => "Turning Profile Roughing",
            TurningStrategy::ProfileFinishing => "Turning Profile Finishing",
            TurningStrategy::GrooveRoughing => "Turning Groove Roughing",
            TurningStrategy::GrooveFinishing => "Turning Groove Finishing",
            TurningStrategy::AdaptiveRoughing => "Turning Adaptive Roughing",
            TurningStrategy::Groove => "Turning Groove",
            TurningStrategy::SingleGroove => "Turning Single Groove",
            TurningStrategy::Thread => "Turning Thread",
            TurningStrategy::Chamfer => "Turning Chamfer",
            TurningStrategy::Part => "Turning Part",
            TurningStrategy::Trace => "Turning Trace",
        }
    }

    pub fn is_groove(self) -> bool {
        matches!(
            self,
            TurningStrategy::GrooveRoughing
                | TurningStrategy::GrooveFinishing
                | TurningStrategy::Groove
                | TurningStrategy::SingleGroove
                | TurningStrategy::AdaptiveRoughing
        )
    }

    pub fn is_roughing(self) -> bool {
        matches!(self, TurningStrategy::ProfileRoughing | TurningStrategy::GrooveRoughing | TurningStrategy::AdaptiveRoughing)
    }
}

/// Radii containment for turning (Radii tab): outer / inner X extents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TurningRadii {
    pub outer_radius_mm: f64,
    pub inner_radius_mm: f64,
    pub clearance_radius_mm: f64,
    pub retract_radius_mm: f64,
}

impl Default for TurningRadii {
    fn default() -> Self {
        Self { outer_radius_mm: 50.0, inner_radius_mm: 0.0, clearance_radius_mm: 55.0, retract_radius_mm: 52.0 }
    }
}

/// Z-axis front/back containment for turning (Geometry tab).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TurningBounds {
    pub front_z_mm: f64,
    pub back_z_mm: f64,
    pub front_offset_mm: f64,
    pub back_offset_mm: f64,
}

impl Default for TurningBounds {
    fn default() -> Self {
        Self { front_z_mm: 0.0, back_z_mm: -50.0, front_offset_mm: 0.0, back_offset_mm: 0.0 }
    }
}

/// A Turning operation (Face / Profile / Groove / Thread / Part / …).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TurningOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    #[serde(default)]
    pub coolant: Coolant,
    pub strategy: TurningStrategy,
    pub mode: TurningMode,
    pub spindle: TurningSpindleRotation,
    /// Depth of cut / groove depth / face stock (mm).
    pub depth_mm: f64,
    /// Max X stepdown for roughing (mm).
    pub max_stepdown_mm: f64,
    /// Finishing allowance radial X (mm).
    pub x_stock_mm: f64,
    /// Finishing allowance axial Z (mm).
    pub z_stock_mm: f64,
    /// Thread pitch (mm) when strategy is Thread.
    pub thread_pitch_mm: f64,
    /// Chamfer width (mm) when strategy is Chamfer.
    pub chamfer_width_mm: f64,
    /// Part-off: cut past centreline (mm).
    pub past_center_mm: f64,
    pub use_parts_catcher: bool,
    pub peck_depth_mm: f64,
    pub machine_undercuts: bool,
    pub make_sharp_corners: bool,
    #[serde(default)]
    pub radii: TurningRadii,
    #[serde(default)]
    pub bounds: TurningBounds,
    #[serde(default)]
    pub linking: ContourLinking,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for TurningOperation {
    fn default() -> Self {
        Self {
            name: String::new(),
            tool_number: 1,
            feeds: Feeds { feedrate: 200.0, plunge: 100.0, spindle_rpm: 1_200 },
            coolant: Coolant::Flood,
            strategy: TurningStrategy::Face,
            mode: TurningMode::Outside,
            spindle: TurningSpindleRotation::Forward,
            depth_mm: 2.0,
            max_stepdown_mm: 1.0,
            x_stock_mm: 0.2,
            z_stock_mm: 0.1,
            thread_pitch_mm: 1.5,
            chamfer_width_mm: 0.5,
            past_center_mm: 0.5,
            use_parts_catcher: false,
            peck_depth_mm: 1.0,
            machine_undercuts: false,
            make_sharp_corners: true,
            radii: TurningRadii::default(),
            bounds: TurningBounds::default(),
            linking: ContourLinking::default(),
            targets: GeometricTargets::default(),
        }
    }
}

impl TurningOperation {
    pub fn with_strategy(strategy: TurningStrategy) -> Self {
        let mut op = Self { strategy, name: strategy.label().into(), ..Default::default() };
        match strategy {
            TurningStrategy::Face => {
                op.depth_mm = 1.0;
                op.max_stepdown_mm = 0.5;
            }
            TurningStrategy::ProfileRoughing | TurningStrategy::GrooveRoughing => {
                op.depth_mm = 5.0;
                op.max_stepdown_mm = 1.5;
                op.x_stock_mm = 0.3;
            }
            TurningStrategy::ProfileFinishing | TurningStrategy::GrooveFinishing => {
                op.depth_mm = 0.5;
                op.max_stepdown_mm = 0.5;
                op.x_stock_mm = 0.0;
                op.z_stock_mm = 0.0;
            }
            TurningStrategy::AdaptiveRoughing => {
                op.depth_mm = 8.0;
                op.max_stepdown_mm = 1.0;
                op.mode = TurningMode::Outside;
            }
            TurningStrategy::Groove | TurningStrategy::SingleGroove => {
                op.depth_mm = 3.0;
                op.max_stepdown_mm = 1.0;
            }
            TurningStrategy::Thread => {
                op.thread_pitch_mm = 1.5;
                op.depth_mm = 0.9;
            }
            TurningStrategy::Chamfer => {
                op.chamfer_width_mm = 0.5;
                op.depth_mm = 0.5;
            }
            TurningStrategy::Part => {
                op.depth_mm = 25.0;
                op.peck_depth_mm = 2.0;
                op.past_center_mm = 0.5;
            }
            TurningStrategy::Trace => {
                op.depth_mm = 0.2;
                op.max_stepdown_mm = 0.2;
            }
        }
        op
    }
}

/// Turning → Part Handling strategy (no cutting toolpath; post emits machine motion).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartHandlingKind {
    #[default]
    SubspindleGrab,
    BarPull,
    SubspindleReturn,
    ToolCall,
}

impl PartHandlingKind {
    pub const ALL: &'static [PartHandlingKind] =
        &[PartHandlingKind::SubspindleGrab, PartHandlingKind::BarPull, PartHandlingKind::SubspindleReturn, PartHandlingKind::ToolCall];

    pub fn label(self) -> &'static str {
        match self {
            PartHandlingKind::SubspindleGrab => "Subspindle Grab",
            PartHandlingKind::BarPull => "Bar Pull",
            PartHandlingKind::SubspindleReturn => "Subspindle Return",
            PartHandlingKind::ToolCall => "Tool Call",
        }
    }
}

/// Chuck open behaviour on Subspindle Return (Open Spindle Chucks).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChuckOpenMode {
    #[default]
    KeepBothClamped,
    UnclampPrimary,
    UnclampSecondary,
}

impl ChuckOpenMode {
    pub const ALL: &'static [ChuckOpenMode] = &[ChuckOpenMode::KeepBothClamped, ChuckOpenMode::UnclampPrimary, ChuckOpenMode::UnclampSecondary];

    pub fn label(self) -> &'static str {
        match self {
            ChuckOpenMode::KeepBothClamped => "Keep both clamped",
            ChuckOpenMode::UnclampPrimary => "Unclamp primary spindle",
            ChuckOpenMode::UnclampSecondary => "Unclamp secondary spindle",
        }
    }
}

/// How bar-pull shifts subsequent Z (Pull Mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarPullMode {
    #[default]
    CompensateToolpaths,
    NewWcsOffset,
}

impl BarPullMode {
    pub fn label(self) -> &'static str {
        match self {
            BarPullMode::CompensateToolpaths => "Compensate toolpaths",
            BarPullMode::NewWcsOffset => "New WCS offset",
        }
    }
}

/// Part Handling operation (Subspindle Grab / Bar Pull / Return / Tool Call).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PartHandlingOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub kind: PartHandlingKind,
    /// Feedrate for subspindle / bar motion (mm/min).
    pub feedrate_mmpm: f64,
    /// Spindle rpm during grab (optional sync); 0 = leave unchanged.
    pub spindle_rpm: u32,
    pub stop_spindle_during_chuck: bool,
    pub stop_spindle_during_return: bool,
    /// Angular shift between spindle 0° references (deg).
    pub spindle_orientation_deg: f64,
    pub use_part_catcher: bool,
    /// Dwell between steps (s).
    pub dwell_s: f64,
    /// Clearance / rapid approach plane in Z (mm) — Feed Plane.
    pub feed_plane_z_mm: f64,
    /// Final clamp depth in Z (mm) — Chuck Plane (Grab).
    pub chuck_plane_z_mm: f64,
    pub chuck_open: ChuckOpenMode,
    /// Bar Pull distance (mm).
    pub pull_distance_mm: f64,
    pub pull_mode: BarPullMode,
    /// Tool Call pocket number.
    pub tool_number: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

impl Default for PartHandlingOperation {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: PartHandlingKind::SubspindleGrab,
            feedrate_mmpm: 2_000.0,
            spindle_rpm: 0,
            stop_spindle_during_chuck: true,
            stop_spindle_during_return: false,
            spindle_orientation_deg: 0.0,
            use_part_catcher: false,
            dwell_s: 0.5,
            feed_plane_z_mm: 5.0,
            chuck_plane_z_mm: -10.0,
            chuck_open: ChuckOpenMode::UnclampPrimary,
            pull_distance_mm: 25.0,
            pull_mode: BarPullMode::CompensateToolpaths,
            tool_number: 1,
            comment: String::new(),
        }
    }
}

impl PartHandlingOperation {
    pub fn with_kind(kind: PartHandlingKind) -> Self {
        let mut op = Self { kind, name: kind.label().into(), ..Default::default() };
        match kind {
            PartHandlingKind::SubspindleGrab => {
                op.feed_plane_z_mm = 5.0;
                op.chuck_plane_z_mm = -10.0;
                op.stop_spindle_during_chuck = true;
            }
            PartHandlingKind::BarPull => {
                op.pull_distance_mm = 25.0;
                op.feedrate_mmpm = 1_500.0;
                op.dwell_s = 0.25;
            }
            PartHandlingKind::SubspindleReturn => {
                op.feed_plane_z_mm = 0.0;
                op.chuck_open = ChuckOpenMode::UnclampPrimary;
                op.stop_spindle_during_return = true;
            }
            PartHandlingKind::ToolCall => {
                op.tool_number = 1;
                op.dwell_s = 0.0;
                op.feedrate_mmpm = 0.0;
            }
        }
        op
    }
}

/// Additive toolpath source (generated layers vs imported print file).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdditiveKind {
    #[default]
    GeneratedToolpath,
    ImportedGcode,
}

impl AdditiveKind {
    pub fn label(self) -> &'static str {
        match self {
            AdditiveKind::GeneratedToolpath => "Additive Toolpath",
            AdditiveKind::ImportedGcode => "Imported Additive Toolpath",
        }
    }
}

/// Additive (FFF) operation — G-code setup / print file, not a cutting toolpath.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdditiveOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub kind: AdditiveKind,
    pub layer_height_mm: f64,
    pub layer_count: u32,
    pub nozzle_diameter_mm: f64,
    pub size_xy_mm: f64,
    pub print_temp_c: u32,
    pub bed_temp_c: u32,
    pub feedrate_mmpm: f64,
    pub travel_mmpm: f64,
    pub relative_extrusion: bool,
    /// Source path when imported (display only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_path: String,
    /// Embedded G-code (truncated) for Simulate Additive Toolpath.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gcode: String,
}

impl Default for AdditiveOperation {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: AdditiveKind::GeneratedToolpath,
            layer_height_mm: 0.2,
            layer_count: 20,
            nozzle_diameter_mm: 0.4,
            size_xy_mm: 40.0,
            print_temp_c: 210,
            bed_temp_c: 60,
            feedrate_mmpm: 1_200.0,
            travel_mmpm: 3_000.0,
            relative_extrusion: true,
            source_path: String::new(),
            gcode: String::new(),
        }
    }
}

impl AdditiveOperation {
    pub fn with_kind(kind: AdditiveKind) -> Self {
        Self { kind, name: kind.label().into(), ..Default::default() }
    }

    pub fn from_gcode(source_path: &str, gcode: &str) -> Self {
        let mut op = Self::with_kind(AdditiveKind::ImportedGcode);
        op.source_path = source_path.into();
        op.gcode = additive::truncate_gcode(gcode);
        let layers = op
            .gcode
            .lines()
            .filter(|l| {
                let u = l.to_ascii_uppercase();
                u.contains("G0") || u.contains("G1") || u.contains("G00") || u.contains("G01")
            })
            .count();
        op.layer_count = (layers as u32 / 16).max(1);
        op
    }

    pub fn simulation_segments(&self) -> Vec<PathSegment> {
        match self.kind {
            AdditiveKind::ImportedGcode if !self.gcode.is_empty() => additive::segments_from_gcode(&self.gcode),
            _ => additive::segments_for_generated(self.layer_height_mm, self.layer_count, self.size_xy_mm, self.feedrate_mmpm, self.travel_mmpm),
        }
    }
}

/// CNC probe / surface inspection strategy (Inspection → Probing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionKind {
    #[default]
    ProbeWcs,
    ProbeGeometry,
    InspectSurface,
    PartAlignment,
}

impl InspectionKind {
    pub const ALL: &'static [InspectionKind] =
        &[InspectionKind::ProbeWcs, InspectionKind::ProbeGeometry, InspectionKind::InspectSurface, InspectionKind::PartAlignment];

    pub fn label(self) -> &'static str {
        match self {
            InspectionKind::ProbeWcs => "Probe WCS",
            InspectionKind::ProbeGeometry => "Probe Geometry",
            InspectionKind::InspectSurface => "Inspect Surface",
            InspectionKind::PartAlignment => "Part Alignment",
        }
    }
}

/// Probe / Inspect Surface / Part Alignment operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InspectionOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub kind: InspectionKind,
    pub tool_number: u32,
    /// Inspection Plus cycle id (`single_surface`, `bore_boss`, …).
    pub probe_cycle: String,
    pub work_offset_s: u32,
    pub clearance_z_mm: f64,
    pub overtravel_mm: f64,
    pub protected_feed_mmpm: f64,
    pub print_results: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub feature_label: String,
    pub nominal_mm: f64,
    pub upper_tol_mm: f64,
    pub lower_tol_mm: f64,
    pub point_spacing_mm: f64,
    pub use_best_fit: bool,
}

impl Default for InspectionOperation {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: InspectionKind::ProbeWcs,
            tool_number: 25,
            probe_cycle: "single_surface".into(),
            work_offset_s: 1,
            clearance_z_mm: 25.0,
            overtravel_mm: 2.0,
            protected_feed_mmpm: 150.0,
            print_results: true,
            feature_label: String::new(),
            nominal_mm: 10.0,
            upper_tol_mm: 0.05,
            lower_tol_mm: 0.05,
            point_spacing_mm: 5.0,
            use_best_fit: true,
        }
    }
}

impl InspectionOperation {
    pub fn with_kind(kind: InspectionKind) -> Self {
        let mut op = Self { kind, name: kind.label().into(), ..Default::default() };
        match kind {
            InspectionKind::ProbeWcs => {
                op.probe_cycle = "single_surface".into();
            }
            InspectionKind::ProbeGeometry => {
                op.probe_cycle = "bore_boss".into();
                op.feature_label = "Feature".into();
            }
            InspectionKind::InspectSurface => {
                op.probe_cycle = "protected_move".into();
                op.point_spacing_mm = 5.0;
            }
            InspectionKind::PartAlignment => {
                op.probe_cycle = "protected_move".into();
                op.use_best_fit = true;
            }
        }
        op
    }
}

/// Manual inspection item type (Create Manual Inspection).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualItemType {
    #[default]
    Distance,
    PassFail,
    Text,
}

impl ManualItemType {
    pub const ALL: &'static [ManualItemType] = &[ManualItemType::Distance, ManualItemType::PassFail, ManualItemType::Text];

    pub fn label(self) -> &'static str {
        match self {
            ManualItemType::Distance => "Distance",
            ManualItemType::PassFail => "Pass/Fail",
            ManualItemType::Text => "Text",
        }
    }
}

/// One row in a manual inspection plan / record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManualInspectionItem {
    pub label: String,
    pub item_type: ManualItemType,
    pub nominal_mm: f64,
    pub upper_tol_mm: f64,
    pub lower_tol_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<bool>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl Default for ManualInspectionItem {
    fn default() -> Self {
        Self {
            label: "Item 1".into(),
            item_type: ManualItemType::Distance,
            nominal_mm: 10.0,
            upper_tol_mm: 0.1,
            lower_tol_mm: 0.1,
            measured_mm: None,
            pass: None,
            notes: String::new(),
        }
    }
}

/// Create vs Record manual inspection (Inspection → Manual).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualInspectionKind {
    #[default]
    Create,
    Record,
}

impl ManualInspectionKind {
    pub fn label(self) -> &'static str {
        match self {
            ManualInspectionKind::Create => "Manual Inspection",
            ManualInspectionKind::Record => "Record Manual Inspection",
        }
    }
}

/// Manual inspection plan or recorded measurements (caliper / micrometer / height gauge).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManualInspectionOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub kind: ManualInspectionKind,
    pub items: Vec<ManualInspectionItem>,
}

impl Default for ManualInspectionOperation {
    fn default() -> Self {
        Self { name: String::new(), kind: ManualInspectionKind::Create, items: vec![ManualInspectionItem::default()] }
    }
}

impl ManualInspectionOperation {
    pub fn with_kind(kind: ManualInspectionKind) -> Self {
        let mut op = Self { kind, name: kind.label().into(), ..Default::default() };
        if kind == ManualInspectionKind::Record {
            for item in &mut op.items {
                item.measured_mm = Some(item.nominal_mm);
                item.pass = Some(true);
            }
        }
        op
    }
}

/// Fabrication 2D Profile cutting mode (Tool → Cutting Mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FabricationCuttingMode {
    #[default]
    ThroughAuto,
    ThroughHighQuality,
    ThroughMediumQuality,
    ThroughLowQuality,
    Etch,
    Vaporize,
}

impl FabricationCuttingMode {
    pub const ALL: &'static [FabricationCuttingMode] = &[
        FabricationCuttingMode::ThroughAuto,
        FabricationCuttingMode::ThroughHighQuality,
        FabricationCuttingMode::ThroughMediumQuality,
        FabricationCuttingMode::ThroughLowQuality,
        FabricationCuttingMode::Etch,
        FabricationCuttingMode::Vaporize,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FabricationCuttingMode::ThroughAuto => "Through — auto",
            FabricationCuttingMode::ThroughHighQuality => "Through — high quality",
            FabricationCuttingMode::ThroughMediumQuality => "Through — medium quality",
            FabricationCuttingMode::ThroughLowQuality => "Through — low quality",
            FabricationCuttingMode::Etch => "Etch",
            FabricationCuttingMode::Vaporize => "Vaporize",
        }
    }
}

/// Whether feeds come from the program or the machine quality tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FabricationQualityControl {
    #[default]
    MachineUsesFeeds,
    MachineUsesQuality,
}

impl FabricationQualityControl {
    pub fn label(self) -> &'static str {
        match self {
            FabricationQualityControl::MachineUsesFeeds => "Machine uses feeds",
            FabricationQualityControl::MachineUsesQuality => "Machine uses quality",
        }
    }
}

/// Sideways kerf compensation for 2D Profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FabricationSidewaysCompensation {
    Left,
    Right,
    #[default]
    Center,
}

impl FabricationSidewaysCompensation {
    pub fn label(self) -> &'static str {
        match self {
            FabricationSidewaysCompensation::Left => "Left",
            FabricationSidewaysCompensation::Right => "Right",
            FabricationSidewaysCompensation::Center => "Center",
        }
    }
}

/// Kerf compensation computed in CAM vs at the control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FabricationCompensationType {
    #[default]
    InComputer,
    InControl,
}

impl FabricationCompensationType {
    pub fn label(self) -> &'static str {
        match self {
            FabricationCompensationType::InComputer => "In computer",
            FabricationCompensationType::InControl => "In control",
        }
    }
}

/// Fabrication → Cutting → 2D Profile (waterjet / laser / plasma / wire EDM).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FabricationProfileOperation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub tool_number: u32,
    pub feeds: Feeds,
    pub lead_in_feedrate: f64,
    pub lead_out_feedrate: f64,
    pub cutting_mode: FabricationCuttingMode,
    pub quality_control: FabricationQualityControl,
    pub sideways: FabricationSidewaysCompensation,
    pub compensation: FabricationCompensationType,
    /// Sheet / stock thickness to cut through (mm).
    pub thickness_mm: f64,
    pub tolerance_mm: f64,
    pub finishing_overlap_mm: f64,
    pub use_tabs: bool,
    pub tab_width_mm: f64,
    pub tab_distance_mm: f64,
    pub keep_nozzle_down: bool,
    pub max_stay_down_mm: f64,
    pub pierce_clearance_mm: f64,
    pub lead_in_radius_mm: f64,
    pub lead_in_sweep_deg: f64,
    pub linear_lead_in_mm: f64,
    pub heights: ContourHeights,
    pub linking: ContourLinking,
    #[serde(default, skip_serializing_if = "GeometricTargets::is_empty")]
    pub targets: GeometricTargets,
}

impl Default for FabricationProfileOperation {
    fn default() -> Self {
        Self {
            name: "2D Profile".into(),
            tool_number: 1,
            feeds: Feeds {
                feedrate: 1_000.0,
                plunge: 1_000.0,
                // No rotary spindle on waterjet/laser/plasma; placeholder for Feeds schema.
                spindle_rpm: 1,
            },
            lead_in_feedrate: 500.0,
            lead_out_feedrate: 500.0,
            cutting_mode: FabricationCuttingMode::ThroughAuto,
            quality_control: FabricationQualityControl::MachineUsesFeeds,
            sideways: FabricationSidewaysCompensation::Left,
            compensation: FabricationCompensationType::InComputer,
            thickness_mm: 3.0,
            tolerance_mm: 0.1,
            finishing_overlap_mm: 0.0,
            use_tabs: false,
            tab_width_mm: 3.0,
            tab_distance_mm: 50.0,
            keep_nozzle_down: false,
            max_stay_down_mm: 100.0,
            pierce_clearance_mm: 5.0,
            lead_in_radius_mm: 2.0,
            lead_in_sweep_deg: 90.0,
            linear_lead_in_mm: 2.0,
            heights: DrillHeights {
                clearance: HeightPlane { from: HeightFrom::RetractHeight, offset_mm: 10.0 },
                retract: HeightPlane { from: HeightFrom::StockTop, offset_mm: 5.0 },
                feed: HeightPlane { from: HeightFrom::TopHeight, offset_mm: 5.0 },
                top: HeightPlane { from: HeightFrom::StockTop, offset_mm: 0.0 },
                bottom: HeightPlane { from: HeightFrom::ModelBottom, offset_mm: 0.0 },
                tip_through_bottom: true,
            },
            linking: ContourLinking::default(),
            targets: GeometricTargets::default(),
        }
    }
}

/// Simple sheet nest parameters (Fabrication → Nest).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NestSettings {
    pub sheet_width_mm: f64,
    pub sheet_height_mm: f64,
    pub part_spacing_mm: f64,
    pub sheet_margin_mm: f64,
    pub rotate_parts: bool,
}

impl Default for NestSettings {
    fn default() -> Self {
        Self { sheet_width_mm: 1_220.0, sheet_height_mm: 2_440.0, part_spacing_mm: 5.0, sheet_margin_mm: 10.0, rotate_parts: true }
    }
}

/// A single machining operation within a setup.
///
/// Serialised as a JSON object with a `"type"` discriminant key, matching the convention used
/// elsewhere in the SolidCraft document model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operation {
    Face(FaceOperation),
    Profile(ProfileOperation),
    Pocket(PocketOperation),
    Drill(DrillOperation),
    Adaptive(AdaptiveOperation),
    Slot(SlotOperation),
    Trace(TraceOperation),
    Thread(ThreadOperation),
    Bore(BoreOperation),
    Circular(CircularOperation),
    Engrave(EngraveOperation),
    Chamfer(ChamferOperation),
    Threed(ThreedOperation),
    HoleRecognition(HoleRecognitionOperation),
    MultiAxis(MultiAxisOperation),
    Turning(TurningOperation),
    PartHandling(PartHandlingOperation),
    Additive(AdditiveOperation),
    Inspection(InspectionOperation),
    ManualInspection(ManualInspectionOperation),
    FabricationProfile(FabricationProfileOperation),
}

impl Operation {
    pub fn tool_number(&self) -> u32 {
        match self {
            Operation::Face(o) => o.tool_number,
            Operation::Profile(o) => o.tool_number,
            Operation::Pocket(o) => o.tool_number,
            Operation::Drill(o) => o.tool_number,
            Operation::Adaptive(o) => o.tool_number,
            Operation::Slot(o) => o.tool_number,
            Operation::Trace(o) => o.tool_number,
            Operation::Thread(o) => o.tool_number,
            Operation::Bore(o) => o.tool_number,
            Operation::Circular(o) => o.tool_number,
            Operation::Engrave(o) => o.tool_number,
            Operation::Chamfer(o) => o.tool_number,
            Operation::Threed(o) => o.tool_number,
            // Hole Recognition assigns tools per group at expand time; no single tool.
            Operation::HoleRecognition(_) => 0,
            Operation::MultiAxis(o) => o.tool_number,
            Operation::Turning(o) => o.tool_number,
            Operation::PartHandling(o) => {
                if o.kind == PartHandlingKind::ToolCall {
                    o.tool_number
                } else {
                    0
                }
            }
            Operation::Additive(_) | Operation::ManualInspection(_) => 0,
            Operation::Inspection(o) => o.tool_number,
            Operation::FabricationProfile(o) => o.tool_number,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Operation::Face(o) => {
                if o.name.is_empty() {
                    "Face"
                } else {
                    &o.name
                }
            }
            Operation::Profile(o) => {
                if o.name.is_empty() {
                    "2D Contour"
                } else {
                    &o.name
                }
            }
            Operation::Pocket(o) => {
                if o.name.is_empty() {
                    "2D Pocket"
                } else {
                    &o.name
                }
            }
            Operation::Drill(o) => {
                if o.name.is_empty() {
                    "Drill"
                } else {
                    &o.name
                }
            }
            Operation::Adaptive(o) => {
                if o.name.is_empty() {
                    "2D Adaptive"
                } else {
                    &o.name
                }
            }
            Operation::Slot(o) => {
                if o.name.is_empty() {
                    "Slot"
                } else {
                    &o.name
                }
            }
            Operation::Trace(o) => {
                if o.name.is_empty() {
                    "Trace"
                } else {
                    &o.name
                }
            }
            Operation::Thread(o) => {
                if o.name.is_empty() {
                    "Thread"
                } else {
                    &o.name
                }
            }
            Operation::Bore(o) => {
                if o.name.is_empty() {
                    "Bore"
                } else {
                    &o.name
                }
            }
            Operation::Circular(o) => {
                if o.name.is_empty() {
                    "Circular"
                } else {
                    &o.name
                }
            }
            Operation::Engrave(o) => {
                if o.name.is_empty() {
                    "Engrave"
                } else {
                    &o.name
                }
            }
            Operation::Chamfer(o) => {
                if o.name.is_empty() {
                    "2D Chamfer"
                } else {
                    &o.name
                }
            }
            Operation::Threed(o) => {
                if o.name.is_empty() {
                    o.strategy.label()
                } else {
                    &o.name
                }
            }
            Operation::HoleRecognition(o) => {
                if o.name.is_empty() {
                    "Hole Recognition"
                } else {
                    &o.name
                }
            }
            Operation::MultiAxis(o) => {
                if o.name.is_empty() {
                    o.strategy.label()
                } else {
                    &o.name
                }
            }
            Operation::Turning(o) => {
                if o.name.is_empty() {
                    o.strategy.label()
                } else {
                    &o.name
                }
            }
            Operation::PartHandling(o) => {
                if o.name.is_empty() {
                    o.kind.label()
                } else {
                    &o.name
                }
            }
            Operation::Additive(o) => {
                if o.name.is_empty() {
                    o.kind.label()
                } else {
                    &o.name
                }
            }
            Operation::Inspection(o) => {
                if o.name.is_empty() {
                    o.kind.label()
                } else {
                    &o.name
                }
            }
            Operation::ManualInspection(o) => {
                if o.name.is_empty() {
                    o.kind.label()
                } else {
                    &o.name
                }
            }
            Operation::FabricationProfile(o) => {
                if o.name.is_empty() {
                    "2D Profile"
                } else {
                    &o.name
                }
            }
        }
    }

    fn feeds(&self) -> &Feeds {
        match self {
            Operation::Face(o) => &o.feeds,
            Operation::Profile(o) => &o.feeds,
            Operation::Pocket(o) => &o.feeds,
            Operation::Drill(o) => &o.feeds,
            Operation::Adaptive(o) => &o.feeds,
            Operation::Slot(o) => &o.feeds,
            Operation::Trace(o) => &o.feeds,
            Operation::Thread(o) => &o.feeds,
            Operation::Bore(o) => &o.feeds,
            Operation::Circular(o) => &o.feeds,
            Operation::Engrave(o) => &o.feeds,
            Operation::Chamfer(o) => &o.feeds,
            Operation::Threed(o) => &o.feeds,
            Operation::HoleRecognition(_)
            | Operation::PartHandling(_)
            | Operation::Additive(_)
            | Operation::Inspection(_)
            | Operation::ManualInspection(_) => &FEEDS_PLACEHOLDER,
            Operation::MultiAxis(o) => &o.feeds,
            Operation::Turning(o) => &o.feeds,
            Operation::FabricationProfile(o) => &o.feeds,
        }
    }

    fn validate(&self, path: &str) -> Result<()> {
        if !matches!(
            self,
            Operation::HoleRecognition(_)
                | Operation::PartHandling(_)
                | Operation::Additive(_)
                | Operation::Inspection(_)
                | Operation::ManualInspection(_)
        ) {
            validate_feeds(self.feeds(), path)?;
        }
        match self {
            Operation::Face(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.stepover > 0.0 && o.stepover <= 1.0 && o.stepover.is_finite()) {
                    return Err(invalid(format!("{path}: stepover must be in (0, 1]")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Profile(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if o.passes == 0 {
                    return Err(invalid(format!("{path}: passes must be ≥ 1")));
                }
                if !(o.finish_allowance >= 0.0 && o.finish_allowance.is_finite()) {
                    return Err(invalid(format!("{path}: finish_allowance must be ≥ 0 and finite")));
                }
                if !(o.lead_in_feedrate > 0.0 && o.lead_in_feedrate.is_finite()) {
                    return Err(invalid(format!("{path}: lead_in_feedrate must be strictly positive and finite")));
                }
                if !(o.lead_out_feedrate > 0.0 && o.lead_out_feedrate.is_finite()) {
                    return Err(invalid(format!("{path}: lead_out_feedrate must be strictly positive and finite")));
                }
                if !(o.pass_opts.tolerance_mm > 0.0 && o.pass_opts.tolerance_mm.is_finite()) {
                    return Err(invalid(format!("{path}: pass_opts.tolerance_mm must be strictly positive and finite")));
                }
                if !(o.pass_opts.max_stepdown_mm > 0.0 && o.pass_opts.max_stepdown_mm.is_finite()) {
                    return Err(invalid(format!("{path}: pass_opts.max_stepdown_mm must be strictly positive and finite")));
                }
                if !(o.linking.safe_distance_mm >= 0.0 && o.linking.safe_distance_mm.is_finite()) {
                    return Err(invalid(format!("{path}: linking.safe_distance_mm must be ≥ 0 and finite")));
                }
                if !(o.linking.max_stay_down_mm >= 0.0 && o.linking.max_stay_down_mm.is_finite()) {
                    return Err(invalid(format!("{path}: linking.max_stay_down_mm must be ≥ 0 and finite")));
                }
                for (label, plane) in [
                    ("heights.clearance", &o.heights.clearance),
                    ("heights.retract", &o.heights.retract),
                    ("heights.feed", &o.heights.feed),
                    ("heights.top", &o.heights.top),
                    ("heights.bottom", &o.heights.bottom),
                ] {
                    if !plane.offset_mm.is_finite() {
                        return Err(invalid(format!("{path}.{label}.offset_mm: must be finite")));
                    }
                }
                if o.geometry.tabs.enabled {
                    if !(o.geometry.tabs.width_mm > 0.0 && o.geometry.tabs.width_mm.is_finite()) {
                        return Err(invalid(format!("{path}: tabs.width_mm must be strictly positive and finite")));
                    }
                    if !(o.geometry.tabs.height_mm >= 0.0 && o.geometry.tabs.height_mm.is_finite()) {
                        return Err(invalid(format!("{path}: tabs.height_mm must be ≥ 0 and finite")));
                    }
                }
                validate_geometric_targets(&o.targets, &format!("{path}.targets"))?;
            }
            Operation::Pocket(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.stepover > 0.0 && o.stepover <= 1.0 && o.stepover.is_finite()) {
                    return Err(invalid(format!("{path}: stepover must be in (0, 1]")));
                }
                if !(o.finish_allowance >= 0.0 && o.finish_allowance.is_finite()) {
                    return Err(invalid(format!("{path}: finish_allowance must be ≥ 0 and finite")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Drill(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.peck_depth > 0.0 && o.peck_depth.is_finite()) {
                    return Err(invalid(format!("{path}: peck_depth must be strictly positive and finite")));
                }
                if !(o.dwell_s >= 0.0 && o.dwell_s.is_finite()) {
                    return Err(invalid(format!("{path}: dwell_s must be ≥ 0 and finite")));
                }
                if !(o.linking.safe_distance_mm >= 0.0 && o.linking.safe_distance_mm.is_finite()) {
                    return Err(invalid(format!("{path}: linking.safe_distance_mm must be ≥ 0 and finite")));
                }
                for (label, plane) in [
                    ("heights.clearance", &o.heights.clearance),
                    ("heights.retract", &o.heights.retract),
                    ("heights.feed", &o.heights.feed),
                    ("heights.top", &o.heights.top),
                    ("heights.bottom", &o.heights.bottom),
                ] {
                    if !plane.offset_mm.is_finite() {
                        return Err(invalid(format!("{path}.{label}.offset_mm: must be finite")));
                    }
                }
                validate_geometric_targets(&o.targets, &format!("{path}.targets"))?;
            }
            Operation::Adaptive(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.optimal_load > 0.0 && o.optimal_load <= 1.0 && o.optimal_load.is_finite()) {
                    return Err(invalid(format!("{path}: optimal_load must be in (0, 1]")));
                }
                if !(o.finish_allowance >= 0.0 && o.finish_allowance.is_finite()) {
                    return Err(invalid(format!("{path}: finish_allowance must be ≥ 0 and finite")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Slot(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.finish_allowance >= 0.0 && o.finish_allowance.is_finite()) {
                    return Err(invalid(format!("{path}: finish_allowance must be ≥ 0 and finite")));
                }
                if !(o.backoff_mm >= 0.0 && o.backoff_mm.is_finite()) {
                    return Err(invalid(format!("{path}: backoff_mm must be ≥ 0 and finite")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Trace(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !o.axial_offset.is_finite() {
                    return Err(invalid(format!("{path}: axial_offset must be finite")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Thread(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.pitch_mm > 0.0 && o.pitch_mm.is_finite()) {
                    return Err(invalid(format!("{path}: pitch_mm must be strictly positive and finite")));
                }
                if !(o.major_diameter > 0.0 && o.major_diameter.is_finite()) {
                    return Err(invalid(format!("{path}: major_diameter must be strictly positive and finite")));
                }
                if o.thread_count == 0 {
                    return Err(invalid(format!("{path}: thread_count must be ≥ 1")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Bore(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.diameter > 0.0 && o.diameter.is_finite()) {
                    return Err(invalid(format!("{path}: diameter must be strictly positive and finite")));
                }
                if !(o.pitch_mm > 0.0 && o.pitch_mm.is_finite()) {
                    return Err(invalid(format!("{path}: pitch_mm must be strictly positive and finite")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Circular(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.diameter > 0.0 && o.diameter.is_finite()) {
                    return Err(invalid(format!("{path}: diameter must be strictly positive and finite")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Engrave(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.tip_angle_deg > 0.0 && o.tip_angle_deg < 180.0 && o.tip_angle_deg.is_finite()) {
                    return Err(invalid(format!("{path}: tip_angle_deg must be in (0, 180)")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Chamfer(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.chamfer_width > 0.0 && o.chamfer_width.is_finite()) {
                    return Err(invalid(format!("{path}: chamfer_width must be strictly positive and finite")));
                }
                if !(o.tip_offset >= 0.0 && o.tip_offset.is_finite()) {
                    return Err(invalid(format!("{path}: tip_offset must be ≥ 0 and finite")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Threed(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.optimal_load > 0.0 && o.optimal_load <= 1.0 && o.optimal_load.is_finite()) {
                    return Err(invalid(format!("{path}: optimal_load must be in (0, 1]")));
                }
                if !(o.stepover > 0.0 && o.stepover <= 1.0 && o.stepover.is_finite()) {
                    return Err(invalid(format!("{path}: stepover must be in (0, 1]")));
                }
                if !(o.cusp_height_mm > 0.0 && o.cusp_height_mm.is_finite()) {
                    return Err(invalid(format!("{path}: cusp_height_mm must be strictly positive and finite")));
                }
                if !o.pass_direction_deg.is_finite() {
                    return Err(invalid(format!("{path}: pass_direction_deg must be finite")));
                }
                if !(o.threshold_angle_deg > 0.0 && o.threshold_angle_deg < 90.0 && o.threshold_angle_deg.is_finite()) {
                    return Err(invalid(format!("{path}: threshold_angle_deg must be in (0, 90)")));
                }
                if !(o.radial_stock_mm >= 0.0 && o.radial_stock_mm.is_finite()) {
                    return Err(invalid(format!("{path}: radial_stock_mm must be ≥ 0 and finite")));
                }
                if !(o.axial_stock_mm >= 0.0 && o.axial_stock_mm.is_finite()) {
                    return Err(invalid(format!("{path}: axial_stock_mm must be ≥ 0 and finite")));
                }
                if !(o.max_stepdown_mm > 0.0 && o.max_stepdown_mm.is_finite()) {
                    return Err(invalid(format!("{path}: max_stepdown_mm must be strictly positive and finite")));
                }
                if !(o.fine_stepdown_mm > 0.0 && o.fine_stepdown_mm.is_finite()) {
                    return Err(invalid(format!("{path}: fine_stepdown_mm must be strictly positive and finite")));
                }
                if !(o.pass_opts.tolerance_mm > 0.0 && o.pass_opts.tolerance_mm.is_finite()) {
                    return Err(invalid(format!("{path}: pass_opts.tolerance_mm must be strictly positive and finite")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::HoleRecognition(o) => {
                if o.find_by_angle {
                    if !(o.min_angle_deg >= 0.0 && o.min_angle_deg.is_finite()) {
                        return Err(invalid(format!("{path}: min_angle_deg must be ≥ 0 and finite")));
                    }
                    if !(o.max_angle_deg >= o.min_angle_deg && o.max_angle_deg <= 180.0 && o.max_angle_deg.is_finite()) {
                        return Err(invalid(format!("{path}: max_angle_deg must be in [min_angle_deg, 180]")));
                    }
                }
                if o.find_by_diameter && !(o.max_diameter_mm > 0.0 && o.max_diameter_mm.is_finite()) {
                    return Err(invalid(format!("{path}: max_diameter_mm must be strictly positive and finite")));
                }
                for (gi, g) in o.groups.iter().enumerate() {
                    let gp = format!("{path}.groups[{gi}]");
                    if !(g.diameter_mm > 0.0 && g.diameter_mm.is_finite()) {
                        return Err(invalid(format!("{gp}: diameter_mm must be strictly positive and finite")));
                    }
                    if !(g.depth_mm > 0.0 && g.depth_mm.is_finite()) {
                        return Err(invalid(format!("{gp}: depth_mm must be strictly positive and finite")));
                    }
                    if g.count == 0 {
                        return Err(invalid(format!("{gp}: count must be ≥ 1")));
                    }
                    if g.action.needs_thread() && g.thread_type.trim().is_empty() {
                        return Err(invalid(format!("{gp}: thread_type required for tapping action")));
                    }
                    if g.centres.len() > MAX_DRILL_POINTS_PER_OP {
                        return Err(invalid(format!("{gp}: too many centres (max {MAX_DRILL_POINTS_PER_OP})")));
                    }
                    for (ci, c) in g.centres.iter().enumerate() {
                        if !c.iter().all(|v| v.is_finite()) {
                            return Err(invalid(format!("{gp}.centres[{ci}]: coordinates must be finite")));
                        }
                    }
                }
            }
            Operation::MultiAxis(o) => {
                if !(o.depth > 0.0 && o.depth.is_finite()) {
                    return Err(invalid(format!("{path}: depth must be strictly positive and finite")));
                }
                if !(o.stepover > 0.0 && o.stepover <= 1.0 && o.stepover.is_finite()) {
                    return Err(invalid(format!("{path}: stepover must be in (0, 1]")));
                }
                if !(o.stepover_angle_deg > 0.0 && o.stepover_angle_deg <= 180.0 && o.stepover_angle_deg.is_finite()) {
                    return Err(invalid(format!("{path}: stepover_angle_deg must be in (0, 180]")));
                }
                if !(o.max_stepdown_mm > 0.0 && o.max_stepdown_mm.is_finite()) {
                    return Err(invalid(format!("{path}: max_stepdown_mm must be strictly positive and finite")));
                }
                if !(o.radial_stock_mm >= 0.0 && o.radial_stock_mm.is_finite()) {
                    return Err(invalid(format!("{path}: radial_stock_mm must be ≥ 0 and finite")));
                }
                if !(o.axial_stock_mm >= 0.0 && o.axial_stock_mm.is_finite()) {
                    return Err(invalid(format!("{path}: axial_stock_mm must be ≥ 0 and finite")));
                }
                if !o.tool_offset_mm.is_finite() {
                    return Err(invalid(format!("{path}: tool_offset_mm must be finite")));
                }
                if !(o.cusp_height_mm > 0.0 && o.cusp_height_mm.is_finite()) {
                    return Err(invalid(format!("{path}: cusp_height_mm must be strictly positive and finite")));
                }
                if !o.lead_angle_deg.is_finite() || !o.tilt_angle_deg.is_finite() {
                    return Err(invalid(format!("{path}: lead/tilt angles must be finite")));
                }
                if o.set_angular_limits && (!o.start_angle_deg.is_finite() || !o.end_angle_deg.is_finite()) {
                    return Err(invalid(format!("{path}: angular limits must be finite")));
                }
                if !(o.pass_opts.tolerance_mm > 0.0 && o.pass_opts.tolerance_mm.is_finite()) {
                    return Err(invalid(format!("{path}: pass_opts.tolerance_mm must be strictly positive and finite")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
            Operation::Turning(o) => {
                if !(o.depth_mm > 0.0 && o.depth_mm.is_finite()) {
                    return Err(invalid(format!("{path}: depth_mm must be strictly positive and finite")));
                }
                if !(o.max_stepdown_mm > 0.0 && o.max_stepdown_mm.is_finite()) {
                    return Err(invalid(format!("{path}: max_stepdown_mm must be strictly positive and finite")));
                }
                if !(o.x_stock_mm >= 0.0 && o.x_stock_mm.is_finite()) {
                    return Err(invalid(format!("{path}: x_stock_mm must be ≥ 0 and finite")));
                }
                if !(o.z_stock_mm >= 0.0 && o.z_stock_mm.is_finite()) {
                    return Err(invalid(format!("{path}: z_stock_mm must be ≥ 0 and finite")));
                }
                if !(o.radii.outer_radius_mm >= o.radii.inner_radius_mm && o.radii.outer_radius_mm.is_finite() && o.radii.inner_radius_mm.is_finite())
                {
                    return Err(invalid(format!("{path}: outer_radius_mm must be ≥ inner_radius_mm and finite")));
                }
                for (label, v) in [
                    ("radii.clearance_radius_mm", o.radii.clearance_radius_mm),
                    ("radii.retract_radius_mm", o.radii.retract_radius_mm),
                    ("bounds.front_z_mm", o.bounds.front_z_mm),
                    ("bounds.back_z_mm", o.bounds.back_z_mm),
                    ("bounds.front_offset_mm", o.bounds.front_offset_mm),
                    ("bounds.back_offset_mm", o.bounds.back_offset_mm),
                ] {
                    if !v.is_finite() {
                        return Err(invalid(format!("{path}.{label}: must be finite")));
                    }
                }
                if o.strategy == TurningStrategy::Thread && !(o.thread_pitch_mm > 0.0 && o.thread_pitch_mm.is_finite()) {
                    return Err(invalid(format!("{path}: thread_pitch_mm must be strictly positive and finite")));
                }
                if o.strategy == TurningStrategy::Chamfer && !(o.chamfer_width_mm > 0.0 && o.chamfer_width_mm.is_finite()) {
                    return Err(invalid(format!("{path}: chamfer_width_mm must be strictly positive and finite")));
                }
                if o.strategy == TurningStrategy::Part {
                    if !(o.peck_depth_mm > 0.0 && o.peck_depth_mm.is_finite()) {
                        return Err(invalid(format!("{path}: peck_depth_mm must be strictly positive and finite")));
                    }
                    if !(o.past_center_mm >= 0.0 && o.past_center_mm.is_finite()) {
                        return Err(invalid(format!("{path}: past_center_mm must be ≥ 0 and finite")));
                    }
                }
                if !(o.linking.safe_distance_mm >= 0.0 && o.linking.safe_distance_mm.is_finite()) {
                    return Err(invalid(format!("{path}: linking.safe_distance_mm must be ≥ 0 and finite")));
                }
                validate_geometric_targets(&o.targets, &format!("{path}.targets"))?;
            }
            Operation::PartHandling(o) => {
                if !o.dwell_s.is_finite() || o.dwell_s < 0.0 {
                    return Err(invalid(format!("{path}: dwell_s must be ≥ 0 and finite")));
                }
                if !o.spindle_orientation_deg.is_finite() {
                    return Err(invalid(format!("{path}: spindle_orientation_deg must be finite")));
                }
                if !o.feed_plane_z_mm.is_finite() {
                    return Err(invalid(format!("{path}: feed_plane_z_mm must be finite")));
                }
                if !o.chuck_plane_z_mm.is_finite() {
                    return Err(invalid(format!("{path}: chuck_plane_z_mm must be finite")));
                }
                match o.kind {
                    PartHandlingKind::ToolCall => {
                        if o.tool_number == 0 {
                            return Err(invalid(format!("{path}: tool_number must be ≥ 1")));
                        }
                    }
                    PartHandlingKind::BarPull => {
                        if !(o.pull_distance_mm > 0.0 && o.pull_distance_mm.is_finite()) {
                            return Err(invalid(format!("{path}: pull_distance_mm must be strictly positive and finite")));
                        }
                        if !(o.feedrate_mmpm > 0.0 && o.feedrate_mmpm.is_finite()) {
                            return Err(invalid(format!("{path}: feedrate_mmpm must be strictly positive and finite")));
                        }
                    }
                    PartHandlingKind::SubspindleGrab | PartHandlingKind::SubspindleReturn => {
                        if !(o.feedrate_mmpm > 0.0 && o.feedrate_mmpm.is_finite()) {
                            return Err(invalid(format!("{path}: feedrate_mmpm must be strictly positive and finite")));
                        }
                    }
                }
            }
            Operation::Additive(o) => {
                if !(o.layer_height_mm > 0.0 && o.layer_height_mm.is_finite()) {
                    return Err(invalid(format!("{path}: layer_height_mm must be strictly positive and finite")));
                }
                if o.layer_count == 0 {
                    return Err(invalid(format!("{path}: layer_count must be ≥ 1")));
                }
                if !(o.nozzle_diameter_mm > 0.0 && o.nozzle_diameter_mm.is_finite()) {
                    return Err(invalid(format!("{path}: nozzle_diameter_mm must be strictly positive and finite")));
                }
                if !(o.size_xy_mm > 0.0 && o.size_xy_mm.is_finite()) {
                    return Err(invalid(format!("{path}: size_xy_mm must be strictly positive and finite")));
                }
                if !(o.feedrate_mmpm > 0.0 && o.feedrate_mmpm.is_finite()) {
                    return Err(invalid(format!("{path}: feedrate_mmpm must be strictly positive and finite")));
                }
                if !(o.travel_mmpm > 0.0 && o.travel_mmpm.is_finite()) {
                    return Err(invalid(format!("{path}: travel_mmpm must be strictly positive and finite")));
                }
                if o.kind == AdditiveKind::ImportedGcode && o.gcode.trim().is_empty() {
                    return Err(invalid(format!("{path}: imported G-code must not be empty")));
                }
            }
            Operation::Inspection(o) => {
                if o.tool_number == 0 {
                    return Err(invalid(format!("{path}: tool_number must be ≥ 1")));
                }
                if o.probe_cycle.trim().is_empty() {
                    return Err(invalid(format!("{path}: probe_cycle required")));
                }
                for (label, v) in [
                    ("clearance_z_mm", o.clearance_z_mm),
                    ("overtravel_mm", o.overtravel_mm),
                    ("protected_feed_mmpm", o.protected_feed_mmpm),
                    ("nominal_mm", o.nominal_mm),
                    ("upper_tol_mm", o.upper_tol_mm),
                    ("lower_tol_mm", o.lower_tol_mm),
                    ("point_spacing_mm", o.point_spacing_mm),
                ] {
                    if !v.is_finite() || v < 0.0 {
                        return Err(invalid(format!("{path}.{label}: must be ≥ 0 and finite")));
                    }
                }
                if o.protected_feed_mmpm == 0.0 {
                    return Err(invalid(format!("{path}: protected_feed_mmpm must be strictly positive")));
                }
            }
            Operation::ManualInspection(o) => {
                if o.items.is_empty() {
                    return Err(invalid(format!("{path}: at least one inspection item required")));
                }
                if o.items.len() > 128 {
                    return Err(invalid(format!("{path}: too many items (max 128)")));
                }
                for (i, item) in o.items.iter().enumerate() {
                    let ip = format!("{path}.items[{i}]");
                    if item.label.trim().is_empty() {
                        return Err(invalid(format!("{ip}: label required")));
                    }
                    for (label, v) in [("nominal_mm", item.nominal_mm), ("upper_tol_mm", item.upper_tol_mm), ("lower_tol_mm", item.lower_tol_mm)] {
                        if !v.is_finite() || v < 0.0 {
                            return Err(invalid(format!("{ip}.{label}: must be ≥ 0 and finite")));
                        }
                    }
                    if let Some(m) = item.measured_mm
                        && !m.is_finite()
                    {
                        return Err(invalid(format!("{ip}.measured_mm: must be finite")));
                    }
                }
            }
            Operation::FabricationProfile(o) => {
                if !(o.thickness_mm > 0.0 && o.thickness_mm.is_finite()) {
                    return Err(invalid(format!("{path}: thickness_mm must be strictly positive and finite")));
                }
                if !(o.tolerance_mm > 0.0 && o.tolerance_mm.is_finite()) {
                    return Err(invalid(format!("{path}: tolerance_mm must be strictly positive and finite")));
                }
                if !(o.lead_in_feedrate > 0.0 && o.lead_in_feedrate.is_finite()) {
                    return Err(invalid(format!("{path}: lead_in_feedrate must be strictly positive and finite")));
                }
                if !(o.lead_out_feedrate > 0.0 && o.lead_out_feedrate.is_finite()) {
                    return Err(invalid(format!("{path}: lead_out_feedrate must be strictly positive and finite")));
                }
                for (label, v) in [
                    ("finishing_overlap_mm", o.finishing_overlap_mm),
                    ("tab_width_mm", o.tab_width_mm),
                    ("tab_distance_mm", o.tab_distance_mm),
                    ("max_stay_down_mm", o.max_stay_down_mm),
                    ("pierce_clearance_mm", o.pierce_clearance_mm),
                    ("lead_in_radius_mm", o.lead_in_radius_mm),
                    ("lead_in_sweep_deg", o.lead_in_sweep_deg),
                    ("linear_lead_in_mm", o.linear_lead_in_mm),
                ] {
                    if !v.is_finite() || v < 0.0 {
                        return Err(invalid(format!("{path}.{label}: must be ≥ 0 and finite")));
                    }
                }
                if o.use_tabs && !(o.tab_width_mm > 0.0) {
                    return Err(invalid(format!("{path}: tab_width_mm must be strictly positive when tabs are enabled")));
                }
                validate_twod_shell(path, &o.heights, &o.linking, &o.targets)?;
            }
        }
        Ok(())
    }
}

fn validate_twod_shell(path: &str, heights: &DrillHeights, linking: &ContourLinking, targets: &GeometricTargets) -> Result<()> {
    if !(linking.safe_distance_mm >= 0.0 && linking.safe_distance_mm.is_finite()) {
        return Err(invalid(format!("{path}: linking.safe_distance_mm must be ≥ 0 and finite")));
    }
    for (label, plane) in [
        ("heights.clearance", &heights.clearance),
        ("heights.retract", &heights.retract),
        ("heights.feed", &heights.feed),
        ("heights.top", &heights.top),
        ("heights.bottom", &heights.bottom),
    ] {
        if !plane.offset_mm.is_finite() {
            return Err(invalid(format!("{path}.{label}.offset_mm: must be finite")));
        }
    }
    validate_geometric_targets(targets, &format!("{path}.targets"))
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

/// A complete CNC setup: stock, work coordinate system, tool library and operation list.
///
/// `machine` and `post` are optional names that reference entries in
/// [`ManufactureSettings::machines`] and [`ManufactureSettings::posts`] respectively.
/// References to absent profiles are accepted at deserialization (the named profile may be
/// added later) but are reported as warnings by [`ManufactureSettings::issues`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MfgSetup {
    pub stock: Stock,
    pub wcs: WorkCoordinateSystem,
    pub tools: Vec<Tool>,
    pub operations: Vec<Operation>,
    /// Name of a machine profile (in the same document's `machines` map).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    /// Name of a post profile (in the same document's `posts` map).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<String>,
    /// Setup → Operation Type.
    #[serde(default)]
    pub operation_type: SetupOperationType,
    /// Setup → WCS Orientation mode.
    #[serde(default)]
    pub wcs_orientation: WcsOrientationMode,
    /// Setup → Origin mode.
    #[serde(default)]
    pub wcs_origin_mode: WcsOriginMode,
    /// When origin is stock/model box point.
    #[serde(default)]
    pub box_point: BoxPoint,
    #[serde(default)]
    pub flip_z: bool,
    #[serde(default)]
    pub flip_x: bool,
    /// Bodies that define this setup's work area. Empty is not a valid "machine everything".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_bodies: Vec<String>,
    /// Clamp / vise / fixture bodies for this setup. Distinct from [`Self::model_bodies`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixture_bodies: Vec<String>,
    /// Setup → Post Process tab.
    #[serde(default)]
    pub post_opts: SetupPostOpts,
}

impl Default for MfgSetup {
    fn default() -> Self {
        Self {
            stock: Stock::default(),
            wcs: WorkCoordinateSystem::default(),
            tools: Vec::new(),
            operations: Vec::new(),
            machine: None,
            post: None,
            operation_type: SetupOperationType::Milling,
            wcs_orientation: WcsOrientationMode::ModelOrientation,
            wcs_origin_mode: WcsOriginMode::SelectedPoint,
            box_point: BoxPoint { x: BoxAxisRef::Negative, y: BoxAxisRef::Positive, z: BoxAxisRef::Positive },
            flip_z: false,
            flip_x: false,
            model_bodies: Vec::new(),
            fixture_bodies: Vec::new(),
            post_opts: SetupPostOpts::default(),
        }
    }
}

fn validate_setup(setup: &MfgSetup, path: &str) -> Result<()> {
    validate_stock(&setup.stock, &format!("{path}.stock"))?;
    validate_wcs(&setup.wcs, &format!("{path}.wcs"))?;
    if setup.post_opts.program_name.len() > MAX_NAME_BYTES {
        return Err(invalid(format!("{path}.post_opts.program_name: too long")));
    }
    if setup.post_opts.program_comment.len() > 512 {
        return Err(invalid(format!("{path}.post_opts.program_comment: too long")));
    }
    if setup.model_bodies.len() > 256 {
        return Err(invalid(format!("{path}.model_bodies: too many bodies")));
    }
    for (i, body) in setup.model_bodies.iter().enumerate() {
        if body.trim().is_empty() || body.len() > MAX_NAME_BYTES {
            return Err(invalid(format!("{path}.model_bodies[{i}]: invalid body name")));
        }
    }
    if setup.fixture_bodies.len() > 256 {
        return Err(invalid(format!("{path}.fixture_bodies: too many bodies")));
    }
    for (i, body) in setup.fixture_bodies.iter().enumerate() {
        if body.trim().is_empty() || body.len() > MAX_NAME_BYTES {
            return Err(invalid(format!("{path}.fixture_bodies[{i}]: invalid body name")));
        }
    }

    if setup.tools.len() > MAX_TOOLS_PER_SETUP {
        return Err(invalid(format!("{path}: too many tools (maximum {MAX_TOOLS_PER_SETUP})")));
    }
    let mut seen_numbers = std::collections::BTreeSet::new();
    for (i, tool) in setup.tools.iter().enumerate() {
        let tp = format!("{path}.tools[{i}]");
        validate_tool(tool, &tp)?;
        if !seen_numbers.insert(tool.number) {
            return Err(invalid(format!("{path}: duplicate tool number {}", tool.number)));
        }
    }

    if setup.operations.len() > MAX_OPS_PER_SETUP {
        return Err(invalid(format!("{path}: too many operations (maximum {MAX_OPS_PER_SETUP})")));
    }
    for (i, op) in setup.operations.iter().enumerate() {
        let op_path = format!("{path}.operations[{i}]");
        op.validate(&op_path)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// SETUP menu containers (NC Program / Folder / Pattern / Manual NC / Mfg Model)
// ---------------------------------------------------------------------------

/// Maximum NC programs / folders / patterns stored in the document.
pub const MAX_SETUP_CONTAINERS: usize = 64;

/// Output unit for NC Program / Post Process (Settings → Unit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NcUnit {
    #[default]
    Millimeters,
    Inches,
}

impl NcUnit {
    pub const ALL: &'static [NcUnit] = &[NcUnit::Millimeters, NcUnit::Inches];

    pub fn label(self) -> &'static str {
        match self {
            NcUnit::Millimeters => "Millimeters",
            NcUnit::Inches => "Inches",
        }
    }
}

/// One operation included in an NC Program (Operations tab).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct NcOpRef {
    pub setup: String,
    pub operation_index: usize,
}

/// NC Program (Setup → Create NC Program / Actions → Post Process).
///
/// Groups setups and operations for one post output — the path from toolpaths to machine G-code.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NcProgram {
    pub name: String,
    /// Program name/number written into the NC header.
    pub program_name: String,
    pub file_name: String,
    pub comment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<String>,
    /// When empty and [`Self::operation_refs`] is empty, all ops in these setups are posted.
    pub setup_names: Vec<String>,
    /// Explicit operation selection (Operations tab). When non-empty, takes precedence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operation_refs: Vec<NcOpRef>,
    pub use_machine_configuration: bool,
    pub use_cascading_post: bool,
    pub output_folder: String,
    pub nc_extension: String,
    pub unit: NcUnit,
    pub open_nc_in_editor: bool,
    pub post_to_hub: bool,
    /// Persist this NC program in the document browser (Create NC program).
    pub create_in_document: bool,
    pub reorder_minimize_tool_changes: bool,
    /// Post-processor-specific key/value properties (Post properties column).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub post_properties: BTreeMap<String, String>,
}

impl Default for NcProgram {
    fn default() -> Self {
        Self {
            name: "NC Program".into(),
            program_name: "1001".into(),
            file_name: "1001".into(),
            comment: String::new(),
            post: None,
            setup_names: Vec::new(),
            operation_refs: Vec::new(),
            use_machine_configuration: false,
            use_cascading_post: false,
            output_folder: String::new(),
            nc_extension: "nc".into(),
            unit: NcUnit::Millimeters,
            open_nc_in_editor: false,
            post_to_hub: false,
            create_in_document: true,
            reorder_minimize_tool_changes: false,
            post_properties: BTreeMap::new(),
        }
    }
}

impl NcProgram {
    /// Resolved output filename with extension.
    pub fn output_file_name(&self) -> String {
        let base = if self.file_name.trim().is_empty() { self.program_name.as_str() } else { self.file_name.as_str() };
        let ext = self.nc_extension.trim().trim_start_matches('.');
        if ext.is_empty() || base.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case(ext)) {
            base.to_string()
        } else {
            format!("{base}.{ext}")
        }
    }
}

/// Estimate cycle time from normalised segments (seconds).
///
/// Linear/arc moves use `feed` (mm/min); rapids use `rapid_mmpm`. `feed_scale` multiplies cutting
/// feeds (1.0 = programmed). Tool changes add `tool_change_s * tool_changes`.
pub fn estimate_machining_time_s(segments: &[PathSegment], rapid_mmpm: f64, feed_scale: f64, tool_change_s: f64, tool_changes: u32) -> f64 {
    let rapid = if rapid_mmpm > 0.0 && rapid_mmpm.is_finite() { rapid_mmpm } else { 5_000.0 };
    let scale = if feed_scale > 0.0 && feed_scale.is_finite() { feed_scale } else { 1.0 };
    let mut cur = [0.0_f64; 3];
    let mut secs = 0.0_f64;
    for seg in segments {
        match seg {
            PathSegment::Rapid { to } => {
                let d = dist3(cur, *to);
                secs += (d / rapid) * 60.0;
                cur = *to;
            }
            PathSegment::Linear { to, feed } => {
                let d = dist3(cur, *to);
                let f = (*feed * scale).max(1.0);
                secs += (d / f) * 60.0;
                cur = *to;
            }
            PathSegment::Arc { to, feed, .. } => {
                let d = dist3(cur, *to);
                let f = (*feed * scale).max(1.0);
                secs += (d / f) * 60.0;
                cur = *to;
            }
        }
    }
    let tc = if tool_change_s.is_finite() && tool_change_s > 0.0 { tool_change_s } else { 0.0 };
    secs + tc * f64::from(tool_changes)
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let dz = b[2] - a[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Format seconds as `H:MM:SS` (status-bar style).
pub fn format_cycle_time(secs: f64) -> String {
    let total = if secs.is_finite() && secs > 0.0 { secs.round() as u64 } else { 0 };
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{h}:{m:02}:{s:02}")
}

/// Build a simple HTML setup sheet for the shop floor.
pub fn build_setup_sheet_html(doc_title: &str, program: &NcProgram, rows: &[(String, String, String, String)], cycle_time: &str) -> String {
    let mut ops = String::new();
    for (op, setup, tool, wcs) in rows {
        ops.push_str(&format!("<tr><td>{op}</td><td>{setup}</td><td>{tool}</td><td>{wcs}</td></tr>\n"));
    }
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Setup Sheet — {title}</title>
<style>
body{{font-family:Segoe UI,Arial,sans-serif;margin:24px;color:#222}}
h1{{font-size:20px}} table{{border-collapse:collapse;width:100%;margin-top:12px}}
th,td{{border:1px solid #ccc;padding:6px 8px;text-align:left;font-size:13px}}
th{{background:#f0f0f0}} .meta{{margin:4px 0;color:#444}}
</style></head><body>
<h1>Setup Sheet</h1>
<p class="meta"><b>Document:</b> {title}</p>
<p class="meta"><b>NC Program:</b> {prog}</p>
<p class="meta"><b>File:</b> {file}</p>
<p class="meta"><b>Post:</b> {post}</p>
<p class="meta"><b>Estimated cycle time:</b> {cycle}</p>
<p class="meta"><b>Comment:</b> {comment}</p>
<table>
<thead><tr><th>Operation</th><th>Setup</th><th>Tool</th><th>Work Offset</th></tr></thead>
<tbody>
{ops}
</tbody></table>
</body></html>"#,
        title = escape_html(doc_title),
        prog = escape_html(&program.program_name),
        file = escape_html(&program.output_file_name()),
        post = escape_html(program.post.as_deref().unwrap_or("(setup default)")),
        cycle = escape_html(cycle_time),
        comment = escape_html(&program.comment),
        ops = ops,
    )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Browser folder that groups operations inside a setup.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpFolder {
    pub name: String,
    pub setup: String,
    pub operation_indices: Vec<usize>,
}

impl Default for OpFolder {
    fn default() -> Self {
        Self { name: "Folder".into(), setup: String::new(), operation_indices: Vec::new() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    #[default]
    Linear,
    Circular,
    Mirror,
    Duplication,
}

impl PatternKind {
    pub fn label(self) -> &'static str {
        match self {
            PatternKind::Linear => "Linear",
            PatternKind::Circular => "Circular",
            PatternKind::Mirror => "Mirror",
            PatternKind::Duplication => "Duplication",
        }
    }
}

/// Toolpath pattern that duplicates selected operations (Setup → New Pattern).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpPattern {
    pub name: String,
    pub setup: String,
    pub kind: PatternKind,
    pub operation_indices: Vec<usize>,
    pub count: u32,
    pub spacing_mm: f64,
    pub angle_deg: f64,
}

impl Default for OpPattern {
    fn default() -> Self {
        Self {
            name: "Pattern".into(),
            setup: String::new(),
            kind: PatternKind::Linear,
            operation_indices: Vec::new(),
            count: 2,
            spacing_mm: 50.0,
            angle_deg: 90.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualNcKind {
    #[default]
    Comment,
    Stop,
    OptionalStop,
    ToolBreak,
    Custom,
}

impl ManualNcKind {
    pub fn label(self) -> &'static str {
        match self {
            ManualNcKind::Comment => "Comment",
            ManualNcKind::Stop => "Stop (M0)",
            ManualNcKind::OptionalStop => "Optional stop (M1)",
            ManualNcKind::ToolBreak => "Tool break check",
            ManualNcKind::Custom => "Custom",
        }
    }
}

/// Manual NC insert (Setup → Manual NC).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManualNc {
    pub name: String,
    pub setup: String,
    pub kind: ManualNcKind,
    pub code: String,
}

impl Default for ManualNc {
    fn default() -> Self {
        Self { name: "Manual NC".into(), setup: String::new(), kind: ManualNcKind::Comment, code: String::new() }
    }
}

/// Manufacturing model placeholder (Setup → Create Manufacturing Model).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManufacturingModel {
    pub name: String,
    pub note: String,
}

impl Default for ManufacturingModel {
    fn default() -> Self {
        Self { name: "Manufacturing Model".into(), note: String::new() }
    }
}

// ---------------------------------------------------------------------------
// Toolpath modifications (Manufacture → MODIFY)
// ---------------------------------------------------------------------------

/// Trim region type (Modify → Trim).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrimType {
    #[default]
    Polygon,
    Plane,
    Boundary,
}

impl TrimType {
    pub fn label(self) -> &'static str {
        match self {
            TrimType::Polygon => "Polygon",
            TrimType::Plane => "Plane",
            TrimType::Boundary => "Boundary",
        }
    }
}

/// Which side of a trim to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrimKeep {
    #[default]
    Outside,
    Inside,
    Both,
    Side1,
    Side2,
}

impl TrimKeep {
    pub fn label(self) -> &'static str {
        match self {
            TrimKeep::Outside => "Outside",
            TrimKeep::Inside => "Inside",
            TrimKeep::Both => "Both",
            TrimKeep::Side1 => "Side 1",
            TrimKeep::Side2 => "Side 2",
        }
    }
}

/// Delete Passes selection mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletePassesSelection {
    #[default]
    Passes,
    Segments,
}

impl DeletePassesSelection {
    pub fn label(self) -> &'static str {
        match self {
            DeletePassesSelection::Passes => "Passes",
            DeletePassesSelection::Segments => "Segments",
        }
    }
}

/// Scope for Leads and Links modification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeadsLinksScope {
    #[default]
    AllMoves,
    SelectedMoves,
}

impl LeadsLinksScope {
    pub fn label(self) -> &'static str {
        match self {
            LeadsLinksScope::AllMoves => "All moves",
            LeadsLinksScope::SelectedMoves => "Selected moves",
        }
    }
}

/// Shared header for every toolpath modification (target op + notes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct ToolpathModHeader {
    pub name: String,
    pub setup: String,
    /// Index into the setup's operations list.
    pub operation_index: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// Modify → Trim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrimModification {
    #[serde(flatten)]
    pub header: ToolpathModHeader,
    pub trim_type: TrimType,
    pub keep: TrimKeep,
    pub visible_only: bool,
    /// Polygon / boundary sample points (model space).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f64; 3]>,
}

impl Default for TrimModification {
    fn default() -> Self {
        Self {
            header: ToolpathModHeader { name: "Trim".into(), ..Default::default() },
            trim_type: TrimType::Polygon,
            keep: TrimKeep::Outside,
            visible_only: false,
            points: Vec::new(),
        }
    }
}

/// Modify → Delete Passes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeletePassesModification {
    #[serde(flatten)]
    pub header: ToolpathModHeader,
    pub selection_type: DeletePassesSelection,
    pub invert_selection: bool,
    /// Selected pass or segment indices (canvas picks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indices: Vec<u32>,
}

impl Default for DeletePassesModification {
    fn default() -> Self {
        Self {
            header: ToolpathModHeader { name: "Delete Passes".into(), ..Default::default() },
            selection_type: DeletePassesSelection::Passes,
            invert_selection: false,
            indices: Vec::new(),
        }
    }
}

/// Modify → Leads and Links.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LeadsLinksModification {
    #[serde(flatten)]
    pub header: ToolpathModHeader,
    pub scope: LeadsLinksScope,
    /// Leads tab.
    pub trim_segments_to_fit: bool,
    pub overlap_mm: f64,
    pub lead_in: bool,
    pub lead_out: bool,
    pub lead_in_radius_mm: f64,
    pub lead_out_radius_mm: f64,
    /// Linking tab.
    pub keep_tool_down: bool,
    pub safe_distance_mm: f64,
    pub high_feed_mode: HighFeedMode,
    /// Heights tab offsets (mm) applied to clearance / retract / feed.
    pub clearance_offset_mm: f64,
    pub retract_offset_mm: f64,
    pub feed_offset_mm: f64,
}

impl Default for LeadsLinksModification {
    fn default() -> Self {
        Self {
            header: ToolpathModHeader { name: "Leads and Links".into(), ..Default::default() },
            scope: LeadsLinksScope::AllMoves,
            trim_segments_to_fit: false,
            overlap_mm: 0.0,
            lead_in: true,
            lead_out: true,
            lead_in_radius_mm: 2.0,
            lead_out_radius_mm: 2.0,
            keep_tool_down: false,
            safe_distance_mm: 2.0,
            high_feed_mode: HighFeedMode::PreserveRapid,
            clearance_offset_mm: 0.0,
            retract_offset_mm: 0.0,
            feed_offset_mm: 0.0,
        }
    }
}

/// Modify → Replace Tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplaceToolModification {
    #[serde(flatten)]
    pub header: ToolpathModHeader,
    pub new_tool_number: u32,
    /// When true, override feeds on the target operation.
    pub override_feeds: bool,
    pub feeds: Feeds,
}

impl Default for ReplaceToolModification {
    fn default() -> Self {
        Self {
            header: ToolpathModHeader { name: "Replace Tool".into(), ..Default::default() },
            new_tool_number: 1,
            override_feeds: false,
            feeds: Feeds::default(),
        }
    }
}

/// One sketched line used by Move Entry Positions (two endpoints).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EntrySketchLine {
    pub a: [f64; 3],
    pub b: [f64; 3],
}

impl Default for EntrySketchLine {
    fn default() -> Self {
        Self { a: [0.0, 0.0, 0.0], b: [10.0, 0.0, 0.0] }
    }
}

/// Modify → Move Entry Positions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MoveEntryModification {
    #[serde(flatten)]
    pub header: ToolpathModHeader,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch_lines: Vec<EntrySketchLine>,
}

impl Default for MoveEntryModification {
    fn default() -> Self {
        Self { header: ToolpathModHeader { name: "Move Entry Positions".into(), ..Default::default() }, sketch_lines: Vec::new() }
    }
}

/// Persisted toolpath modification (MODIFY menu).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolpathModification {
    Trim(TrimModification),
    DeletePasses(DeletePassesModification),
    LeadsAndLinks(LeadsLinksModification),
    ReplaceTool(ReplaceToolModification),
    MoveEntryPositions(MoveEntryModification),
}

impl ToolpathModification {
    pub fn header(&self) -> &ToolpathModHeader {
        match self {
            ToolpathModification::Trim(m) => &m.header,
            ToolpathModification::DeletePasses(m) => &m.header,
            ToolpathModification::LeadsAndLinks(m) => &m.header,
            ToolpathModification::ReplaceTool(m) => &m.header,
            ToolpathModification::MoveEntryPositions(m) => &m.header,
        }
    }

    pub fn header_mut(&mut self) -> &mut ToolpathModHeader {
        match self {
            ToolpathModification::Trim(m) => &mut m.header,
            ToolpathModification::DeletePasses(m) => &mut m.header,
            ToolpathModification::LeadsAndLinks(m) => &mut m.header,
            ToolpathModification::ReplaceTool(m) => &mut m.header,
            ToolpathModification::MoveEntryPositions(m) => &mut m.header,
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            ToolpathModification::Trim(_) => "Trim",
            ToolpathModification::DeletePasses(_) => "Delete Passes",
            ToolpathModification::LeadsAndLinks(_) => "Leads and Links",
            ToolpathModification::ReplaceTool(_) => "Replace Tool",
            ToolpathModification::MoveEntryPositions(_) => "Move Entry Positions",
        }
    }

    pub fn validate(&self, path: &str) -> Result<()> {
        let h = self.header();
        if h.setup.trim().is_empty() {
            return Err(invalid(format!("{path}: setup must be non-empty")));
        }
        match self {
            ToolpathModification::Trim(m) => {
                if m.trim_type == TrimType::Polygon && m.points.len() < 3 && !m.points.is_empty() {
                    return Err(invalid(format!("{path}: polygon needs ≥ 3 points")));
                }
                for (i, p) in m.points.iter().enumerate() {
                    if !p.iter().all(|v| v.is_finite()) {
                        return Err(invalid(format!("{path}.points[{i}]: must be finite")));
                    }
                }
                if m.points.len() > MAX_SAMPLE_POINTS_PER_OP {
                    return Err(invalid(format!("{path}: too many points")));
                }
            }
            ToolpathModification::DeletePasses(m) => {
                if m.indices.len() > MAX_DRILL_POINTS_PER_OP {
                    return Err(invalid(format!("{path}: too many indices")));
                }
            }
            ToolpathModification::LeadsAndLinks(m) => {
                if !(m.overlap_mm >= 0.0 && m.overlap_mm.is_finite()) {
                    return Err(invalid(format!("{path}: overlap_mm must be ≥ 0 and finite")));
                }
                if !(m.lead_in_radius_mm >= 0.0 && m.lead_in_radius_mm.is_finite()) {
                    return Err(invalid(format!("{path}: lead_in_radius_mm must be ≥ 0 and finite")));
                }
                if !(m.lead_out_radius_mm >= 0.0 && m.lead_out_radius_mm.is_finite()) {
                    return Err(invalid(format!("{path}: lead_out_radius_mm must be ≥ 0 and finite")));
                }
                if !(m.safe_distance_mm >= 0.0 && m.safe_distance_mm.is_finite()) {
                    return Err(invalid(format!("{path}: safe_distance_mm must be ≥ 0 and finite")));
                }
                for (label, v) in
                    [("clearance_offset_mm", m.clearance_offset_mm), ("retract_offset_mm", m.retract_offset_mm), ("feed_offset_mm", m.feed_offset_mm)]
                {
                    if !v.is_finite() {
                        return Err(invalid(format!("{path}.{label}: must be finite")));
                    }
                }
            }
            ToolpathModification::ReplaceTool(m) => {
                if m.new_tool_number == 0 {
                    return Err(invalid(format!("{path}: new_tool_number must be ≥ 1")));
                }
                if m.override_feeds {
                    validate_feeds(&m.feeds, &format!("{path}.feeds"))?;
                }
            }
            ToolpathModification::MoveEntryPositions(m) => {
                if m.sketch_lines.len() > 64 {
                    return Err(invalid(format!("{path}: too many sketch lines")));
                }
                for (i, line) in m.sketch_lines.iter().enumerate() {
                    if !line.a.iter().chain(line.b.iter()).all(|v| v.is_finite()) {
                        return Err(invalid(format!("{path}.sketch_lines[{i}]: endpoints must be finite")));
                    }
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ManufactureSettings (top-level; stored in the document)
// ---------------------------------------------------------------------------

/// Deserialization proxy: accepts the raw JSON shape without applying invariants.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UncheckedManufactureSettings {
    setups: BTreeMap<String, MfgSetup>,
    machines: BTreeMap<String, MachineProfile>,
    posts: BTreeMap<String, PostProfile>,
    #[serde(default)]
    nc_programs: Vec<NcProgram>,
    #[serde(default)]
    folders: Vec<OpFolder>,
    #[serde(default)]
    patterns: Vec<OpPattern>,
    #[serde(default)]
    manual_nc: Vec<ManualNc>,
    #[serde(default)]
    manufacturing_models: Vec<ManufacturingModel>,
    #[serde(default)]
    toolpath_modifications: Vec<ToolpathModification>,
    #[serde(default)]
    templates: BTreeMap<String, library::CamTemplate>,
    #[serde(default)]
    library_remotes: Vec<library::LibraryRemote>,
}

impl TryFrom<UncheckedManufactureSettings> for ManufactureSettings {
    type Error = ManufactureError;
    fn try_from(u: UncheckedManufactureSettings) -> std::result::Result<Self, ManufactureError> {
        let s = ManufactureSettings {
            setups: u.setups,
            machines: u.machines,
            posts: u.posts,
            nc_programs: u.nc_programs,
            folders: u.folders,
            patterns: u.patterns,
            manual_nc: u.manual_nc,
            manufacturing_models: u.manufacturing_models,
            toolpath_modifications: u.toolpath_modifications,
            templates: u.templates,
            library_remotes: u.library_remotes,
        };
        s.validate()?;
        Ok(s)
    }
}

/// Named CNC manufacture setups, machine profiles and post profiles, persisted in the document.
///
/// Toolpaths (the computed sequences of [`PathSegment`] values) are ephemeral; they are not
/// stored here but derived from the operations by the manufacture engine on demand.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "UncheckedManufactureSettings")]
pub struct ManufactureSettings {
    pub setups: BTreeMap<String, MfgSetup>,
    pub machines: BTreeMap<String, MachineProfile>,
    pub posts: BTreeMap<String, PostProfile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nc_programs: Vec<NcProgram>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<OpFolder>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<OpPattern>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manual_nc: Vec<ManualNc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manufacturing_models: Vec<ManufacturingModel>,
    /// MODIFY timeline entries (Trim, Delete Passes, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub toolpath_modifications: Vec<ToolpathModification>,
    /// Template Library entries owned by this document.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub templates: BTreeMap<String, library::CamTemplate>,
    /// Linked git remotes for Post / Machine / Template libraries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_remotes: Vec<library::LibraryRemote>,
}

impl ManufactureSettings {
    /// `true` when no setups, machines or posts have been defined; used by `skip_serializing_if`.
    pub fn is_empty(&self) -> bool {
        self.setups.is_empty()
            && self.machines.is_empty()
            && self.posts.is_empty()
            && self.nc_programs.is_empty()
            && self.folders.is_empty()
            && self.patterns.is_empty()
            && self.manual_nc.is_empty()
            && self.manufacturing_models.is_empty()
            && self.toolpath_modifications.is_empty()
            && self.templates.is_empty()
            && self.library_remotes.is_empty()
    }

    /// Validate a name: trimmed, non-empty, no control characters, at most [`MAX_NAME_BYTES`]
    /// bytes.
    pub fn validate_name(name: &str) -> Result<()> {
        if name.trim().is_empty() || name != name.trim() || name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
            return Err(invalid("names must be trimmed, non-empty, at most 128 bytes, and contain no control characters"));
        }
        Ok(())
    }

    /// Hard validation: enforces all structural and numeric invariants.
    ///
    /// Called automatically during deserialisation (via `TryFrom`). May also be called
    /// explicitly before saving to catch mutations made after construction.
    pub fn validate(&self) -> Result<()> {
        if self.setups.len() > MAX_SETUPS {
            return Err(invalid(format!("too many setups (maximum {MAX_SETUPS})")));
        }
        if self.machines.len() > MAX_MACHINES {
            return Err(invalid(format!("too many machine profiles (maximum {MAX_MACHINES})")));
        }
        if self.posts.len() > MAX_POSTS {
            return Err(invalid(format!("too many post profiles (maximum {MAX_POSTS})")));
        }
        if self.templates.len() > library::MAX_TEMPLATES {
            return Err(invalid(format!("too many templates (maximum {})", library::MAX_TEMPLATES)));
        }
        if self.library_remotes.len() > library::MAX_LIBRARY_REMOTES {
            return Err(invalid(format!("too many library remotes (maximum {})", library::MAX_LIBRARY_REMOTES)));
        }
        for (i, r) in self.library_remotes.iter().enumerate() {
            r.validate(&format!("library_remotes[{i}]"))?;
        }
        {
            let mut ids = std::collections::BTreeSet::new();
            for r in &self.library_remotes {
                if !ids.insert(r.id.as_str()) {
                    return Err(invalid(format!("duplicate library remote id `{}`", r.id)));
                }
            }
        }
        for (name, t) in &self.templates {
            Self::validate_name(name)?;
            t.validate(&format!("templates.{name}"))?;
        }
        for (label, n) in [
            ("nc_programs", self.nc_programs.len()),
            ("folders", self.folders.len()),
            ("patterns", self.patterns.len()),
            ("manual_nc", self.manual_nc.len()),
            ("manufacturing_models", self.manufacturing_models.len()),
            ("toolpath_modifications", self.toolpath_modifications.len()),
        ] {
            if n > MAX_SETUP_CONTAINERS {
                return Err(invalid(format!("too many {label} (maximum {MAX_SETUP_CONTAINERS})")));
            }
        }
        for (i, m) in self.toolpath_modifications.iter().enumerate() {
            m.validate(&format!("toolpath_modifications[{i}]"))?;
            let setup_name = &m.header().setup;
            match self.setups.get(setup_name.as_str()) {
                None => {
                    return Err(invalid(format!("toolpath_modifications[{i}]: setup `{setup_name}` not found")));
                }
                Some(setup) if m.header().operation_index >= setup.operations.len() => {
                    return Err(invalid(format!("toolpath_modifications[{i}]: operation_index out of range")));
                }
                Some(_) => {}
            }
        }

        for name in self.setups.keys().chain(self.machines.keys()).chain(self.posts.keys()) {
            Self::validate_name(name)?;
        }

        for (name, setup) in &self.setups {
            validate_setup(setup, &format!("setups.{name}"))?;
        }
        for (name, machine) in &self.machines {
            validate_machine(machine, &format!("machines.{name}"))?;
        }
        for (name, post) in &self.posts {
            validate_post(post, &format!("posts.{name}"))?;
        }
        for (i, p) in self.patterns.iter().enumerate() {
            if p.count == 0 {
                return Err(invalid(format!("patterns[{i}].count: must be ≥ 1")));
            }
            if !(p.spacing_mm >= 0.0 && p.spacing_mm.is_finite()) {
                return Err(invalid(format!("patterns[{i}].spacing_mm: must be ≥ 0 and finite")));
            }
        }
        Ok(())
    }

    /// Soft validation: returns diagnostics that should be presented to the user but do not
    /// prevent saving. Hard errors are not repeated here (they would have blocked
    /// deserialisation / [`Self::validate`]).
    pub fn issues(&self) -> Vec<ValidationIssue> {
        let mut out = Vec::new();
        for (sname, setup) in &self.setups {
            // Warn if the referenced machine or post profile doesn't exist.
            if let Some(m) = &setup.machine
                && !self.machines.contains_key(m.as_str())
            {
                out.push(ValidationIssue {
                    severity: Severity::Warning,
                    path: format!("setups.{sname}.machine"),
                    message: format!("machine profile `{m}` is not defined"),
                });
            }
            if let Some(p) = &setup.post
                && !self.posts.contains_key(p.as_str())
            {
                out.push(ValidationIssue {
                    severity: Severity::Warning,
                    path: format!("setups.{sname}.post"),
                    message: format!("post profile `{p}` is not defined"),
                });
            }
            // Warn if an operation references a tool number that doesn't exist.
            let tool_numbers: std::collections::BTreeSet<u32> = setup.tools.iter().map(|t| t.number).collect();
            for (i, op) in setup.operations.iter().enumerate() {
                if matches!(op, Operation::HoleRecognition(_) | Operation::Additive(_) | Operation::ManualInspection(_)) {
                    continue;
                }
                if matches!(op, Operation::PartHandling(o) if o.kind != PartHandlingKind::ToolCall) {
                    continue;
                }
                if !tool_numbers.is_empty() && !tool_numbers.contains(&op.tool_number()) {
                    out.push(ValidationIssue {
                        severity: Severity::Warning,
                        path: format!("setups.{sname}.operations[{i}]"),
                        message: format!("tool number {} is not in this setup's tool library", op.tool_number()),
                    });
                }
            }
            // Warn if operations are defined but no tools are listed.
            let needs_tools = setup.operations.iter().any(|op| match op {
                Operation::HoleRecognition(_) | Operation::Additive(_) | Operation::ManualInspection(_) => false,
                Operation::PartHandling(o) => o.kind == PartHandlingKind::ToolCall,
                _ => true,
            });
            if needs_tools && setup.tools.is_empty() {
                out.push(ValidationIssue {
                    severity: Severity::Warning,
                    path: format!("setups.{sname}"),
                    message: "setup has operations but no tools defined".into(),
                });
            }
            // Check feed rates and ATC capacity against machine limits.
            if let Some(machine_name) = &setup.machine
                && let Some(machine) = self.machines.get(machine_name.as_str())
            {
                for (i, op) in setup.operations.iter().enumerate() {
                    if matches!(
                        op,
                        Operation::HoleRecognition(_)
                            | Operation::PartHandling(_)
                            | Operation::Additive(_)
                            | Operation::Inspection(_)
                            | Operation::ManualInspection(_)
                    ) {
                        continue;
                    }
                    let f = op.feeds();
                    if machine.max_feedrate_mmpm > 0.0 && f.feedrate > machine.max_feedrate_mmpm {
                        out.push(ValidationIssue {
                            severity: Severity::Warning,
                            path: format!("setups.{sname}.operations[{i}].feeds.feedrate"),
                            message: format!("feedrate {:.0} mm/min exceeds machine limit {:.0} mm/min", f.feedrate, machine.max_feedrate_mmpm),
                        });
                    }
                    if machine.max_spindle_rpm > 0 && f.spindle_rpm > machine.max_spindle_rpm {
                        out.push(ValidationIssue {
                            severity: Severity::Warning,
                            path: format!("setups.{sname}.operations[{i}].feeds.spindle_rpm"),
                            message: format!("spindle_rpm {} exceeds machine limit {}", f.spindle_rpm, machine.max_spindle_rpm),
                        });
                    }
                }
                let pockets = machine.tool_changer.pockets;
                if pockets > 0 {
                    if setup.tools.len() as u32 > pockets {
                        out.push(ValidationIssue {
                            severity: Severity::Warning,
                            path: format!("setups.{sname}.tools"),
                            message: format!("{} tools exceed ATC capacity of {} pockets on `{machine_name}`", setup.tools.len(), pockets),
                        });
                    }
                    for (i, tool) in setup.tools.iter().enumerate() {
                        if tool.number > pockets {
                            out.push(ValidationIssue {
                                severity: Severity::Warning,
                                path: format!("setups.{sname}.tools[{i}]"),
                                message: format!("tool number {} is above ATC pocket count {} on `{machine_name}`", tool.number, pockets),
                            });
                        }
                        if machine.tool_changer.max_tool_diameter_mm > 0.0 && tool.diameter > machine.tool_changer.max_tool_diameter_mm {
                            out.push(ValidationIssue {
                                severity: Severity::Warning,
                                path: format!("setups.{sname}.tools[{i}].diameter"),
                                message: format!(
                                    "tool diameter {:.3} mm exceeds ATC max {:.3} mm on `{machine_name}`",
                                    tool.diameter, machine.tool_changer.max_tool_diameter_mm
                                ),
                            });
                        }
                        if machine.tool_changer.max_tool_length_mm > 0.0 && tool.length > machine.tool_changer.max_tool_length_mm {
                            out.push(ValidationIssue {
                                severity: Severity::Warning,
                                path: format!("setups.{sname}.tools[{i}].length"),
                                message: format!(
                                    "tool length {:.3} mm exceeds ATC max {:.3} mm on `{machine_name}`",
                                    tool.length, machine.tool_changer.max_tool_length_mm
                                ),
                            });
                        }
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn default_setup() -> MfgSetup {
        MfgSetup {
            stock: Stock::Box { width: 120.0, depth: 80.0, height: 30.0 },
            wcs: WorkCoordinateSystem::default(),
            tools: vec![
                Tool {
                    number: 1,
                    kind: ToolKind::FlatEndmill,
                    diameter: 6.0,
                    corner_radius: 0.0,
                    length: 50.0,
                    flute_length: 16.0,
                    flutes: 4,
                    description: "6mm 4-flute".into(),
                    ..Default::default()
                },
                Tool {
                    number: 2,
                    kind: ToolKind::Drill,
                    diameter: 3.0,
                    corner_radius: 0.0,
                    length: 40.0,
                    flute_length: 20.0,
                    flutes: 2,
                    description: String::new(),
                    ..Default::default()
                },
            ],
            operations: vec![
                Operation::Face(FaceOperation { tool_number: 1, depth: 0.5, stepover: 0.75, ..Default::default() }),
                Operation::Profile(ProfileOperation { tool_number: 1, depth: 5.0, ..Default::default() }),
                Operation::Pocket(PocketOperation { tool_number: 1, depth: 4.0, ..Default::default() }),
                Operation::Drill(DrillOperation { tool_number: 2, depth: 12.0, cycle: DrillCycle::Peck, ..Default::default() }),
                Operation::Adaptive(AdaptiveOperation { tool_number: 1, depth: 4.0, ..Default::default() }),
            ],
            machine: None,
            post: None,
            ..Default::default()
        }
    }

    fn default_settings() -> ManufactureSettings {
        let mut setups = BTreeMap::new();
        setups.insert("Setup1".into(), default_setup());
        let mut machines = BTreeMap::new();
        machines.insert(
            "Tormach 440".into(),
            MachineProfile {
                name: "Tormach 440".into(),
                oem: "Tormach".into(),
                model: "PCNC 440".into(),
                axes: MachineAxes::ThreeAxis,
                spindle_taper: SpindleTaper::ErCollet,
                tool_changer: ToolChanger { kind: ToolChangerKind::Manual, pockets: 1, ..ToolChanger::default() },
                max_spindle_rpm: 10_000,
                max_feedrate_mmpm: 5_000.0,
                rapid_feedrate_mmpm: 5_000.0,
                ..MachineProfile::default()
            },
        );
        let mut posts = BTreeMap::new();
        posts.insert(
            "LinuxCNC".into(),
            PostProfile {
                name: "LinuxCNC".into(),
                dialect: GcodeDialect::LinuxCnc,
                line_ending: LineEnding::Lf,
                preamble: "G21 G90".into(),
                postamble: "M30".into(),
            },
        );
        ManufactureSettings { setups, machines, posts, ..Default::default() }
    }

    #[test]
    fn empty_settings_roundtrip() {
        let s = ManufactureSettings::default();
        assert!(s.is_empty());
        let json = serde_json::to_string(&s).unwrap();
        let back: ManufactureSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        // An empty JSON object also deserialises to the default.
        let from_empty: ManufactureSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(from_empty, s);
    }

    #[test]
    fn full_settings_roundtrip() {
        let s = default_settings();
        assert!(!s.is_empty());
        let json = serde_json::to_string(&s).unwrap();
        let back: ManufactureSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        // Serialisation is deterministic (BTreeMap → alphabetical keys).
        assert_eq!(json, serde_json::to_string(&s).unwrap());
    }

    #[test]
    fn turning_strategies_validate_and_label() {
        for strategy in TurningStrategy::ALL {
            let op = Operation::Turning(TurningOperation::with_strategy(*strategy));
            assert_eq!(op.display_name(), strategy.label());
            op.validate("op").unwrap();
        }
        let bad = Operation::Turning(TurningOperation { depth_mm: 0.0, ..TurningOperation::with_strategy(TurningStrategy::Face) });
        assert!(bad.validate("op").is_err());
        let bad_thread = Operation::Turning(TurningOperation { thread_pitch_mm: 0.0, ..TurningOperation::with_strategy(TurningStrategy::Thread) });
        assert!(bad_thread.validate("op").is_err());
    }

    #[test]
    fn all_operation_types_roundtrip() {
        let ops = vec![
            Operation::Face(FaceOperation::default()),
            Operation::Profile(ProfileOperation::default()),
            Operation::Pocket(PocketOperation::default()),
            Operation::Drill(DrillOperation::default()),
            Operation::Adaptive(AdaptiveOperation::default()),
            Operation::Threed(ThreedOperation::with_strategy(ThreedStrategy::Scallop)),
            Operation::HoleRecognition(HoleRecognitionOperation::default()),
            Operation::MultiAxis(MultiAxisOperation::with_strategy(MultiAxisStrategy::Swarf)),
            Operation::Turning(TurningOperation::with_strategy(TurningStrategy::ProfileFinishing)),
            Operation::PartHandling(PartHandlingOperation::with_kind(PartHandlingKind::BarPull)),
            Operation::Additive(AdditiveOperation::with_kind(AdditiveKind::GeneratedToolpath)),
            Operation::Inspection(InspectionOperation::with_kind(InspectionKind::ProbeGeometry)),
            Operation::ManualInspection(ManualInspectionOperation::with_kind(ManualInspectionKind::Create)),
            Operation::FabricationProfile(FabricationProfileOperation::default()),
        ];
        for op in &ops {
            let json = serde_json::to_value(op).unwrap();
            let back: Operation = serde_json::from_value(json).unwrap();
            assert_eq!(op, &back);
        }
    }

    #[test]
    fn fabrication_2d_profile_validates() {
        let op = Operation::FabricationProfile(FabricationProfileOperation::default());
        assert_eq!(op.display_name(), "2D Profile");
        op.validate("op").unwrap();
        let bad = Operation::FabricationProfile(FabricationProfileOperation { thickness_mm: 0.0, ..FabricationProfileOperation::default() });
        assert!(bad.validate("op").is_err());
    }

    #[test]
    fn additive_and_inspection_ops_validate() {
        for kind in InspectionKind::ALL {
            let op = Operation::Inspection(InspectionOperation::with_kind(*kind));
            assert_eq!(op.display_name(), kind.label());
            op.validate("op").unwrap();
        }
        let add = Operation::Additive(AdditiveOperation::with_kind(AdditiveKind::GeneratedToolpath));
        add.validate("op").unwrap();
        assert!(!AdditiveOperation::default().simulation_segments().is_empty());
        let man = Operation::ManualInspection(ManualInspectionOperation::with_kind(ManualInspectionKind::Record));
        man.validate("op").unwrap();
    }

    #[test]
    fn part_handling_kinds_validate_and_label() {
        for kind in PartHandlingKind::ALL {
            let op = Operation::PartHandling(PartHandlingOperation::with_kind(*kind));
            assert_eq!(op.display_name(), kind.label());
            op.validate("op").unwrap();
        }
        let bad_pull =
            Operation::PartHandling(PartHandlingOperation { pull_distance_mm: 0.0, ..PartHandlingOperation::with_kind(PartHandlingKind::BarPull) });
        assert!(bad_pull.validate("op").is_err());
        let bad_tool =
            Operation::PartHandling(PartHandlingOperation { tool_number: 0, ..PartHandlingOperation::with_kind(PartHandlingKind::ToolCall) });
        assert!(bad_tool.validate("op").is_err());
    }

    #[test]
    fn nc_program_output_name_and_cycle_time() {
        let mut p = NcProgram { file_name: "part".into(), nc_extension: "nc".into(), ..Default::default() };
        assert_eq!(p.output_file_name(), "part.nc");
        p.file_name = "part.nc".into();
        assert_eq!(p.output_file_name(), "part.nc");

        let segs = vec![PathSegment::Rapid { to: [10.0, 0.0, 0.0] }, PathSegment::Linear { to: [20.0, 0.0, 0.0], feed: 1_000.0 }];
        let t = estimate_machining_time_s(&segs, 6_000.0, 1.0, 5.0, 1);
        assert!(t > 5.0);
        assert_eq!(format_cycle_time(3661.0), "1:01:01");
    }

    #[test]
    fn toolpath_modifications_roundtrip_and_validate() {
        let mut s = ManufactureSettings::default();
        let mut setup = MfgSetup::default();
        setup.operations.push(Operation::Face(FaceOperation::default()));
        setup.tools.push(Tool::default());
        s.setups.insert("Setup1".into(), setup);

        let trim = ToolpathModification::Trim(TrimModification {
            header: ToolpathModHeader { name: "Trim1".into(), setup: "Setup1".into(), operation_index: 0, notes: "keep outside".into() },
            trim_type: TrimType::Polygon,
            keep: TrimKeep::Outside,
            visible_only: true,
            points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0]],
        });
        trim.validate("m").unwrap();
        s.toolpath_modifications.push(trim);
        s.validate().unwrap();
        let json = serde_json::to_value(&s).unwrap();
        let back: ManufactureSettings = serde_json::from_value(json).unwrap();
        assert_eq!(back.toolpath_modifications.len(), 1);
        assert_eq!(back.toolpath_modifications[0].kind_label(), "Trim");
    }

    #[test]
    fn multiaxis_strategies_validate_and_label() {
        for strategy in [
            MultiAxisStrategy::RotaryOffsetRoughing,
            MultiAxisStrategy::MultiAxisRoughing,
            MultiAxisStrategy::Swarf,
            MultiAxisStrategy::AdvancedSwarf,
            MultiAxisStrategy::MultiAxisContour,
            MultiAxisStrategy::RotaryParallel,
            MultiAxisStrategy::RotaryContour,
            MultiAxisStrategy::MultiAxisFinishing,
        ] {
            let op = Operation::MultiAxis(MultiAxisOperation::with_strategy(strategy));
            assert!(!strategy.label().is_empty());
            op.validate("op").unwrap();
            assert_eq!(op.display_name(), strategy.label());
        }
    }

    #[test]
    fn hole_recognition_validates_groups_and_options() {
        let mut op = HoleRecognitionOperation::default();
        Operation::HoleRecognition(op.clone()).validate("op").unwrap();
        op.groups.push(HoleGroup {
            diameter_mm: 6.0,
            depth_mm: 10.0,
            count: 2,
            action: HoleAction::Tap,
            thread_type: String::new(),
            centres: vec![[0.0, 0.0, 0.0]],
        });
        assert!(Operation::HoleRecognition(op.clone()).validate("op").is_err());
        op.groups[0].thread_type = "M6x1".into();
        Operation::HoleRecognition(op).validate("op").unwrap();
    }

    #[test]
    fn threed_strategies_validate_and_label() {
        for strategy in [
            ThreedStrategy::AdaptiveRoughing,
            ThreedStrategy::OffsetRoughing,
            ThreedStrategy::ThreePlusTwoRoughing,
            ThreedStrategy::SteepAndShallow,
            ThreedStrategy::Flat,
            ThreedStrategy::Parallel,
            ThreedStrategy::Scallop,
            ThreedStrategy::Contour,
            ThreedStrategy::Ramp,
            ThreedStrategy::Pencil,
            ThreedStrategy::Horizontal,
            ThreedStrategy::Spiral,
            ThreedStrategy::Radial,
            ThreedStrategy::MorphedSpiral,
            ThreedStrategy::Project,
            ThreedStrategy::Blend,
            ThreedStrategy::Morph,
            ThreedStrategy::Corner,
            ThreedStrategy::Flow,
            ThreedStrategy::Deburr,
            ThreedStrategy::Geodesic,
        ] {
            let op = Operation::Threed(ThreedOperation::with_strategy(strategy));
            assert!(!strategy.label().is_empty());
            op.validate("op").unwrap();
            assert_eq!(op.display_name(), strategy.label());
        }
        let bad = Operation::Threed(ThreedOperation { optimal_load: 0.0, ..ThreedOperation::with_strategy(ThreedStrategy::AdaptiveRoughing) });
        assert!(bad.validate("op").is_err());
    }

    #[test]
    fn path_segment_validate_accepts_valid_rejects_invalid() {
        PathSegment::Rapid { to: [0.0, 0.0, 0.0] }.validate().unwrap();
        PathSegment::Linear { to: [1.0, 2.0, 3.0], feed: 500.0 }.validate().unwrap();
        PathSegment::Arc { center: [0.0, 0.0, 0.0], to: [10.0, 0.0, 0.0], feed: 300.0, clockwise: false }.validate().unwrap();

        assert!(PathSegment::Rapid { to: [f64::NAN, 0.0, 0.0] }.validate().is_err());
        assert!(PathSegment::Linear { to: [0.0, 0.0, 0.0], feed: 0.0 }.validate().is_err());
        assert!(PathSegment::Linear { to: [0.0, 0.0, 0.0], feed: -1.0 }.validate().is_err());
        assert!(PathSegment::Arc { center: [f64::INFINITY, 0.0, 0.0], to: [0.0, 0.0, 0.0], feed: 100.0, clockwise: true }.validate().is_err());
    }

    #[test]
    fn too_many_setups_is_rejected() {
        let mut s = ManufactureSettings::default();
        for i in 0..=MAX_SETUPS {
            s.setups.insert(format!("Setup{i}"), MfgSetup::default());
        }
        let json = serde_json::to_string(&s).unwrap();
        assert!(serde_json::from_str::<ManufactureSettings>(&json).is_err());
    }

    #[test]
    fn too_many_tools_rejected() {
        let mut setup = MfgSetup::default();
        for i in 1..=(MAX_TOOLS_PER_SETUP + 1) as u32 {
            setup.tools.push(Tool { number: i, ..Tool::default() });
        }
        let err = validate_setup(&setup, "setups.S1").unwrap_err();
        assert!(err.to_string().contains("too many tools"), "{err}");
    }

    #[test]
    fn too_many_operations_rejected() {
        let mut setup = MfgSetup { tools: vec![Tool::default()], ..MfgSetup::default() };
        for _ in 0..=MAX_OPS_PER_SETUP {
            setup.operations.push(Operation::Face(FaceOperation::default()));
        }
        let err = validate_setup(&setup, "setups.S1").unwrap_err();
        assert!(err.to_string().contains("too many operations"), "{err}");
    }

    #[test]
    fn duplicate_tool_numbers_rejected() {
        let mut setup = MfgSetup {
            tools: vec![Tool { number: 1, ..Tool::default() }, Tool { number: 1, diameter: 3.0, ..Tool::default() }],
            ..MfgSetup::default()
        };
        setup.stock = Stock::Box { width: 100.0, depth: 100.0, height: 25.0 };
        let err = validate_setup(&setup, "setups.S1").unwrap_err();
        assert!(err.to_string().contains("duplicate tool number"), "{err}");
    }

    #[test]
    fn invalid_tool_geometry_rejected() {
        let bad_diameter = Tool { diameter: 0.0, ..Tool::default() };
        assert!(validate_tool(&bad_diameter, "t").is_err());
        let bad_cr = Tool { corner_radius: 10.0, ..Tool::default() }; // > diameter/2
        assert!(validate_tool(&bad_cr, "t").is_err());
        let bad_flute = Tool { flute_length: 200.0, ..Tool::default() }; // > length
        assert!(validate_tool(&bad_flute, "t").is_err());
        let no_flutes = Tool { flutes: 0, ..Tool::default() };
        assert!(validate_tool(&no_flutes, "t").is_err());
    }

    #[test]
    fn wcs_must_be_orthonormal() {
        let bad_unit = WorkCoordinateSystem { x_axis: [2.0, 0.0, 0.0], ..Default::default() };
        assert!(validate_wcs(&bad_unit, "wcs").is_err());
        let bad_orth = WorkCoordinateSystem { x_axis: [1.0, 0.0, 0.0], y_axis: [1.0, 0.0, 0.0], ..Default::default() };
        assert!(validate_wcs(&bad_orth, "wcs").is_err());
        let nan_origin = WorkCoordinateSystem { origin: [f64::NAN, 0.0, 0.0], ..Default::default() };
        assert!(validate_wcs(&nan_origin, "wcs").is_err());
        let far_origin = WorkCoordinateSystem { origin: [2e9, 0.0, 0.0], ..Default::default() };
        assert!(validate_wcs(&far_origin, "wcs").is_err());
        // Valid diagonal WCS should pass.
        let diag = WorkCoordinateSystem {
            origin: [0.0, 0.0, 0.0],
            x_axis: [std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2, 0.0],
            y_axis: [-std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2, 0.0],
        };
        validate_wcs(&diag, "wcs").unwrap();
    }

    #[test]
    fn name_validation_enforces_rules() {
        assert!(ManufactureSettings::validate_name("Setup1").is_ok());
        assert!(ManufactureSettings::validate_name("").is_err(), "empty");
        assert!(ManufactureSettings::validate_name(" padded").is_err(), "leading space");
        assert!(ManufactureSettings::validate_name("line\nfeed").is_err(), "control char");
        assert!(ManufactureSettings::validate_name(&"a".repeat(129)).is_err(), "too long");
        assert!(ManufactureSettings::validate_name(&"a".repeat(128)).is_ok(), "exactly at limit");
    }

    #[test]
    fn unknown_fields_at_top_level_are_rejected() {
        let json = r#"{"setups":{},"machines":{},"posts":{},"unknown_field":true}"#;
        assert!(serde_json::from_str::<ManufactureSettings>(json).is_err());
    }

    #[test]
    fn unknown_fields_in_setup_are_rejected() {
        let json = r#"{"setups":{"S1":{"stock":{"kind":"box","width":10,"depth":10,"height":10},"unknown":1}}}"#;
        assert!(serde_json::from_str::<ManufactureSettings>(json).is_err());
    }

    #[test]
    fn stock_dimensions_must_be_positive() {
        assert!(validate_stock(&Stock::Box { width: 0.0, depth: 10.0, height: 10.0 }, "s").is_err());
        assert!(validate_stock(&Stock::Cylinder { diameter: 10.0, height: -1.0 }, "s").is_err());
        assert!(validate_stock(&Stock::Box { width: f64::NAN, depth: 10.0, height: 10.0 }, "s").is_err());
        validate_stock(&Stock::Cylinder { diameter: 50.0, height: 100.0 }, "s").unwrap();
    }

    #[test]
    fn from_solid_stock_requires_a_body_name() {
        assert!(validate_stock(&Stock::FromSolid { body: String::new() }, "s").is_err());
        validate_stock(&Stock::FromSolid { body: "StockBody".into() }, "s").unwrap();
    }

    #[test]
    fn fixture_bodies_are_validated_separately_from_model_bodies() {
        let mut setup = MfgSetup::default();
        setup.stock = Stock::FromSolid { body: "StockBody".into() };
        setup.model_bodies = vec!["Part".into()];
        setup.fixture_bodies = vec!["Vise".into()];
        validate_setup(&setup, "setups.S1").unwrap();
        setup.fixture_bodies = vec!["  ".into()];
        assert!(validate_setup(&setup, "setups.S1").is_err());
    }

    #[test]
    fn feeds_must_be_positive() {
        let zero_feed = Feeds { feedrate: 0.0, ..Feeds::default() };
        assert!(validate_feeds(&zero_feed, "op").is_err());
        let zero_rpm = Feeds { spindle_rpm: 0, ..Feeds::default() };
        assert!(validate_feeds(&zero_rpm, "op").is_err());
        let neg_plunge = Feeds { plunge: -10.0, ..Feeds::default() };
        assert!(validate_feeds(&neg_plunge, "op").is_err());
    }

    #[test]
    fn issues_warns_on_missing_machine_post_and_tool_references() {
        let mut s = default_settings();
        // Point setup at machines/posts that don't exist.
        if let Some(setup) = s.setups.get_mut("Setup1") {
            setup.machine = Some("NonExistentMachine".into());
            setup.post = Some("NonExistentPost".into());
            // Add an operation referencing tool number 99.
            setup.operations.push(Operation::Face(FaceOperation { tool_number: 99, ..Default::default() }));
        }
        let issues = s.issues();
        assert!(issues.iter().any(|i| i.message.contains("NonExistentMachine")), "missing machine warning");
        assert!(issues.iter().any(|i| i.message.contains("NonExistentPost")), "missing post warning");
        assert!(issues.iter().any(|i| i.message.contains("99")), "missing tool number warning");
    }

    #[test]
    fn issues_warns_when_feeds_exceed_machine_limits() {
        let mut s = default_settings();
        if let Some(setup) = s.setups.get_mut("Setup1") {
            setup.machine = Some("Tormach 440".into());
            setup.operations = vec![Operation::Face(FaceOperation {
                tool_number: 1,
                feeds: Feeds { feedrate: 9_000.0, spindle_rpm: 20_000, ..Feeds::default() },
                ..Default::default()
            })];
        }
        let issues = s.issues();
        assert!(issues.iter().any(|i| i.message.contains("feedrate")), "feedrate warning");
        assert!(issues.iter().any(|i| i.message.contains("spindle_rpm")), "spindle warning");
    }

    #[test]
    fn operation_validate_rejects_bad_parameters() {
        // Stepover out of range.
        let bad_face = Operation::Face(FaceOperation { stepover: 0.0, ..Default::default() });
        assert!(bad_face.validate("op").is_err());
        let bad_face2 = Operation::Face(FaceOperation { stepover: 1.1, ..Default::default() });
        assert!(bad_face2.validate("op").is_err());
        // Zero passes.
        let bad_profile = Operation::Profile(ProfileOperation { passes: 0, ..Default::default() });
        assert!(bad_profile.validate("op").is_err());
        // Negative depth.
        let bad_drill = Operation::Drill(DrillOperation { depth: -5.0, ..Default::default() });
        assert!(bad_drill.validate("op").is_err());
        // optimal_load = 0.
        let bad_adaptive = Operation::Adaptive(AdaptiveOperation { optimal_load: 0.0, ..Default::default() });
        assert!(bad_adaptive.validate("op").is_err());
    }

    // ── GeometricTargets ──────────────────────────────────────────────────────

    #[test]
    fn geometric_targets_default_is_empty_and_roundtrips() {
        let t = GeometricTargets::default();
        assert!(t.is_empty());
        // empty targets serialize to `{}` (no keys)
        let json = serde_json::to_string(&t).unwrap();
        let back: GeometricTargets = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn geometric_targets_with_points_roundtrips() {
        let t = GeometricTargets { face_sample_points: vec![[10.0, 20.0, 5.0], [0.0, 0.0, 0.0]], drill_points: vec![[15.0, 15.0, 0.0]] };
        assert!(!t.is_empty());
        let json = serde_json::to_string(&t).unwrap();
        let back: GeometricTargets = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn geometric_targets_validation_rejects_too_many_points() {
        let many: Vec<[f64; 3]> = (0..=MAX_SAMPLE_POINTS_PER_OP).map(|i| [i as f64, 0.0, 0.0]).collect();
        let t = GeometricTargets { face_sample_points: many, drill_points: vec![] };
        assert!(validate_geometric_targets(&t, "t").is_err());

        let many_drill: Vec<[f64; 3]> = (0..=MAX_DRILL_POINTS_PER_OP).map(|i| [i as f64, 0.0, 0.0]).collect();
        let t2 = GeometricTargets { face_sample_points: vec![], drill_points: many_drill };
        assert!(validate_geometric_targets(&t2, "t").is_err());
    }

    #[test]
    fn geometric_targets_validation_rejects_non_finite_coords() {
        let nan_face = GeometricTargets { face_sample_points: vec![[f64::NAN, 0.0, 0.0]], drill_points: vec![] };
        assert!(validate_geometric_targets(&nan_face, "t").is_err());

        let inf_drill = GeometricTargets { face_sample_points: vec![], drill_points: vec![[f64::INFINITY, 0.0, 0.0]] };
        assert!(validate_geometric_targets(&inf_drill, "t").is_err());
    }

    #[test]
    fn operation_targets_skip_serializing_when_empty() {
        // A ProfileOperation with default (empty) targets should not include "targets" in JSON.
        let op = Operation::Profile(ProfileOperation::default());
        let json = serde_json::to_value(&op).unwrap();
        assert!(!json.as_object().unwrap().contains_key("targets"), "empty targets should be omitted");

        // With targets populated, the field should appear.
        let op_with = Operation::Profile(ProfileOperation {
            targets: GeometricTargets { face_sample_points: vec![[1.0, 2.0, 3.0]], ..Default::default() },
            ..Default::default()
        });
        let json_with = serde_json::to_value(&op_with).unwrap();
        assert!(json_with.as_object().unwrap().contains_key("targets"), "non-empty targets should be present");
    }

    #[test]
    fn operation_with_targets_roundtrips() {
        let ops = vec![
            Operation::Profile(ProfileOperation {
                targets: GeometricTargets { face_sample_points: vec![[10.0, 0.0, 5.0]], drill_points: vec![] },
                ..Default::default()
            }),
            Operation::Pocket(PocketOperation {
                targets: GeometricTargets { face_sample_points: vec![[5.0, 5.0, 0.0]], ..Default::default() },
                ..Default::default()
            }),
            Operation::Drill(DrillOperation {
                targets: GeometricTargets { face_sample_points: vec![], drill_points: vec![[10.0, 10.0, 0.0], [20.0, 10.0, 0.0]] },
                ..Default::default()
            }),
            Operation::Adaptive(AdaptiveOperation {
                targets: GeometricTargets { face_sample_points: vec![[0.0, 0.0, 10.0]], ..Default::default() },
                ..Default::default()
            }),
        ];
        for op in &ops {
            let json = serde_json::to_value(op).unwrap();
            let back: Operation = serde_json::from_value(json).unwrap();
            assert_eq!(op, &back);
        }
    }

    #[test]
    fn old_operation_json_without_targets_deserializes_to_empty_targets() {
        // Simulates a document written before GeometricTargets was added.
        let json_profile = r#"{"type":"profile","tool_number":1,"depth":3.0,"passes":1,"finish_allowance":0.0}"#;
        let op: Operation = serde_json::from_str(json_profile).unwrap();
        if let Operation::Profile(p) = op {
            assert!(p.targets.is_empty(), "old JSON should have empty targets");
        } else {
            panic!("expected Profile");
        }
    }

    #[test]
    fn targets_too_many_points_fails_operation_validation() {
        let many: Vec<[f64; 3]> = (0..=MAX_SAMPLE_POINTS_PER_OP).map(|i| [i as f64, 0.0, 0.0]).collect();
        let op = Operation::Profile(ProfileOperation {
            targets: GeometricTargets { face_sample_points: many, drill_points: vec![] },
            ..Default::default()
        });
        assert!(op.validate("op").is_err());
    }

    #[test]
    fn drill_height_zs_match_default_offsets() {
        let op = DrillOperation::default();
        let zs = op.height_zs(0.0);
        // top = hole top + 0, retract = stock top + 5, clearance = retract + 10, feed = top + 5
        assert!((zs.top - 0.0).abs() < 1e-9);
        assert!((zs.retract - 5.0).abs() < 1e-9);
        assert!((zs.clearance - 15.0).abs() < 1e-9);
        assert!((zs.feed - 5.0).abs() < 1e-9);
        assert!((zs.bottom - (-op.depth)).abs() < 1e-9);
    }

    #[test]
    fn atc_capacity_warns_on_overflow() {
        let mut s = ManufactureSettings::default();
        let id = "Test Mill";
        let mut machine = MachineProfile::default();
        machine.tool_changer.pockets = 20;
        machine.tool_changer.max_tool_diameter_mm = 50.0;
        machine.tool_changer.max_tool_length_mm = 200.0;
        s.machines.insert(id.into(), machine);
        let mut setup = MfgSetup {
            machine: Some(id.into()),
            tools: (1..=21).map(|n| Tool { number: n, diameter: 80.0, length: 300.0, ..Tool::default() }).collect(),
            ..MfgSetup::default()
        };
        setup.stock = Stock::Box { width: 100.0, depth: 100.0, height: 25.0 };
        s.setups.insert("S1".into(), setup);
        s.validate().unwrap();
        let issues = s.issues();
        assert!(issues.iter().any(|i| i.message.contains("exceed ATC capacity")), "{issues:?}");
        assert!(issues.iter().any(|i| i.message.contains("above ATC pocket count")), "{issues:?}");
        assert!(issues.iter().any(|i| i.message.contains("diameter")), "{issues:?}");
    }
}
