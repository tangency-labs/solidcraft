//! CAM setups persisted on the design: tools, setups and 2D milling operations.
//! Toolpaths are never stored; they are computed when posting (`cam.post`).
//!
//! Operation geometry is a sketch profile or a planar face (resolved when posting), not a
//! copied polygon, so toolpaths follow the design. Depths, feeds and stepdowns are [`Expr`]
//! values, mapped onto `solidcraft-cam` only at post time.

use serde::{Deserialize, Serialize};
use solidcraft_cam::{Post, Tool};
use solidcraft_geom::{Loop2, Region2, Vec2, Vec3};

use crate::{DocError, Expr, ProfileSel, Result};

/// Most tools in one design.
pub const MAX_TOOLS: usize = 256;
/// Most setups in one design.
pub const MAX_SETUPS: usize = 64;
/// Most operations in one setup.
pub const MAX_OPS: usize = 256;
/// Most vertices on one polygon loop.
pub const MAX_LOOP_POINTS: usize = 10_000;
/// Most holes in one region.
pub const MAX_HOLES: usize = 1_000;
/// Most regions in one operation.
pub const MAX_REGIONS: usize = 256;

/// The design's CAM data: tools, setups and optional named posts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CamSettings {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub setups: Vec<CamSetup>,
    /// Named posts saved with the design. Absent means the built-in RS-274 post at post time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub posts: Vec<Post>,
}

impl CamSettings {
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty() && self.setups.is_empty() && self.posts.is_empty()
    }

    pub fn tool(&self, number: u32) -> Option<&Tool> {
        self.tools.iter().find(|t| t.number == number)
    }

    pub fn setup(&self, name: &str) -> Option<&CamSetup> {
        self.setups.iter().find(|s| s.name == name)
    }

    pub fn setup_mut(&mut self, name: &str) -> Option<&mut CamSetup> {
        self.setups.iter_mut().find(|s| s.name == name)
    }

    pub fn post(&self, name: &str) -> Option<&Post> {
        self.posts.iter().find(|p| p.name == name)
    }
}

/// One machining setup: work offset, optional tool axis / origin, optional stock, and operations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CamSetup {
    pub name: String,
    /// Work offset 1…6 (`G54`…`G59`).
    #[serde(default = "one")]
    pub work_offset: u8,
    /// Tool axis in work coordinates. Default `[0, 0, 1]` (from +Z). Reserved for later 3+2 work.
    #[serde(default = "tool_axis_z", skip_serializing_if = "is_tool_axis_z")]
    pub tool_axis: [f64; 3],
    /// Work origin in model coordinates (mm). Default origin. Reserved for later WCS mapping.
    #[serde(default, skip_serializing_if = "is_origin")]
    pub work_origin: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock: Option<StockBox>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<CamOperation>,
}

fn one() -> u8 {
    1
}

fn tool_axis_z() -> [f64; 3] {
    [0.0, 0.0, 1.0]
}

fn is_tool_axis_z(v: &[f64; 3]) -> bool {
    *v == [0.0, 0.0, 1.0]
}

fn is_origin(v: &[f64; 3]) -> bool {
    *v == [0.0, 0.0, 0.0]
}

fn profile_all() -> ProfileSel {
    ProfileSel::All
}

impl CamSetup {
    pub fn new(name: impl Into<String>) -> CamSetup {
        CamSetup { name: name.into(), work_offset: 1, tool_axis: [0.0, 0.0, 1.0], work_origin: [0.0, 0.0, 0.0], stock: None, operations: Vec::new() }
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() || self.name.len() > 128 {
            return Err(DocError::Invalid("a CAM setup name must be 1 to 128 characters".into()));
        }
        if !(1..=6).contains(&self.work_offset) {
            return Err(DocError::Invalid("the work offset must be 1 to 6 (G54 to G59)".into()));
        }
        if self.tool_axis.iter().any(|x| !x.is_finite()) || self.tool_axis == [0.0, 0.0, 0.0] {
            return Err(DocError::Invalid("the tool axis must be a finite non-zero vector".into()));
        }
        if self.work_origin.iter().any(|x| !x.is_finite() || x.abs() > 1.0e6) {
            return Err(DocError::Invalid("the work origin must be finite and within 1e6 mm".into()));
        }
        if self.operations.len() > MAX_OPS {
            return Err(DocError::Invalid(format!("a CAM setup has at most {MAX_OPS} operations")));
        }
        if let Some(stock) = &self.stock {
            stock.validate()?;
        }
        for op in &self.operations {
            op.validate()?;
        }
        Ok(())
    }

    pub fn tool_axis_vec(&self) -> Vec3 {
        Vec3::new(self.tool_axis[0], self.tool_axis[1], self.tool_axis[2])
    }

    pub fn work_origin_vec(&self) -> Vec3 {
        Vec3::new(self.work_origin[0], self.work_origin[1], self.work_origin[2])
    }
}

/// Axis-aligned stock box in work coordinates (mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StockBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl StockBox {
    pub fn validate(&self) -> Result<()> {
        for (a, b) in self.min.iter().zip(self.max.iter()) {
            if !a.is_finite() || !b.is_finite() || !(b > a) {
                return Err(DocError::Invalid("stock min must be below max on each axis".into()));
            }
            if a.abs() > 1.0e6 || b.abs() > 1.0e6 {
                return Err(DocError::Invalid("stock coordinates must be within 1e6 mm".into()));
            }
        }
        Ok(())
    }
}

/// A planar region as polygon loops (outer CCW, holes CW), work XY in mm.
/// Used only when a command passes explicit polylines instead of a sketch or face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolygonRegion {
    pub outer: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<[f64; 2]>>,
}

impl PolygonRegion {
    pub fn validate(&self) -> Result<()> {
        validate_loop(&self.outer, "outer")?;
        if self.holes.len() > MAX_HOLES {
            return Err(DocError::Invalid(format!("a region has at most {MAX_HOLES} holes")));
        }
        for (i, hole) in self.holes.iter().enumerate() {
            validate_loop(hole, &format!("hole {i}"))?;
        }
        Ok(())
    }

    pub fn to_region(&self) -> Result<Region2> {
        self.validate()?;
        let outer = Loop2::polygon(&self.outer.iter().map(|p| Vec2::new(p[0], p[1])).collect::<Vec<_>>());
        let holes = self.holes.iter().map(|h| Loop2::polygon(&h.iter().map(|p| Vec2::new(p[0], p[1])).collect::<Vec<_>>())).collect();
        Ok(Region2 { outer, holes })
    }
}

fn validate_loop(pts: &[[f64; 2]], label: &str) -> Result<()> {
    if pts.len() < 3 || pts.len() > MAX_LOOP_POINTS {
        return Err(DocError::Invalid(format!("a {label} loop needs 3 to {MAX_LOOP_POINTS} points")));
    }
    if pts.iter().any(|p| !p[0].is_finite() || !p[1].is_finite() || p[0].abs() > 1.0e6 || p[1].abs() > 1.0e6) {
        return Err(DocError::Invalid(format!("a {label} loop has non-finite or huge coordinates")));
    }
    Ok(())
}

/// Where an operation's regions come from. Resolved against the evaluated model at post time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CamGeometry {
    /// Closed profiles of a sketch (true lines and arcs, not a tessellated copy).
    Sketch {
        sketch: u64,
        #[serde(default = "profile_all")]
        profiles: ProfileSel,
    },
    /// Outlines of planar body faces (inner loops are islands).
    Faces { faces: Vec<CamFaceRef> },
    /// Explicit polylines in work XY (tests and commands that do not name a sketch).
    Polygons { regions: Vec<PolygonRegion> },
}

/// A body face: persistent name `name` on `body`, else the piece nearest `at`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CamFaceRef {
    pub body: String,
    pub name: String,
    #[serde(default)]
    pub at: [f64; 3],
}

impl CamFaceRef {
    pub fn at_vec(&self) -> Vec3 {
        Vec3::new(self.at[0], self.at[1], self.at[2])
    }
}

impl CamGeometry {
    pub fn validate(&self) -> Result<()> {
        match self {
            CamGeometry::Sketch { .. } => Ok(()),
            CamGeometry::Faces { faces } => {
                if faces.is_empty() || faces.len() > MAX_REGIONS {
                    return Err(DocError::Invalid(format!("an operation needs 1 to {MAX_REGIONS} faces")));
                }
                for f in faces {
                    if f.body.is_empty() || f.body.len() > 128 || f.name.is_empty() || f.name.len() > 128 {
                        return Err(DocError::Invalid("each face needs a body and a persistent name".into()));
                    }
                    if f.at.iter().any(|x| !x.is_finite() || x.abs() > 1.0e6) {
                        return Err(DocError::Invalid("face point must be finite and within 1e6 mm".into()));
                    }
                }
                Ok(())
            }
            CamGeometry::Polygons { regions } => {
                if regions.is_empty() || regions.len() > MAX_REGIONS {
                    return Err(DocError::Invalid(format!("an operation needs 1 to {MAX_REGIONS} regions")));
                }
                for r in regions {
                    r.validate()?;
                }
                Ok(())
            }
        }
    }
}

/// Which side of a contour the tool rides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamSide {
    #[default]
    Outside,
    Inside,
    On,
}

/// Contour settings held as expressions so the file format is not the cam crate's param struct.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CamContourParams {
    #[serde(default)]
    pub side: CamSide,
    #[serde(default = "true_val")]
    pub climb: bool,
    #[serde(default = "zero_mm")]
    pub stock_to_leave: Expr,
    #[serde(default = "tol_mm")]
    pub tolerance: Expr,
    #[serde(default = "zero_deg")]
    pub ramp_angle: Expr,
}

/// Pocket settings held as expressions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CamPocketParams {
    pub stepover: Expr,
    #[serde(default = "true_val")]
    pub climb: bool,
    #[serde(default = "zero_mm")]
    pub stock_to_leave: Expr,
    #[serde(default = "tol_mm")]
    pub tolerance: Expr,
    #[serde(default = "zero_deg")]
    pub ramp_angle: Expr,
}

/// Heights and feeds as expressions (`depth + 1 mm`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CamCut {
    #[serde(default = "clearance_mm")]
    pub clearance: Expr,
    #[serde(default = "retract_mm")]
    pub retract: Expr,
    #[serde(default = "zero_mm")]
    pub top: Expr,
    pub bottom: Expr,
    pub stepdown: Expr,
    #[serde(default = "feed_mm")]
    pub feed: Expr,
    #[serde(default = "plunge_mm")]
    pub plunge_feed: Expr,
}

fn true_val() -> bool {
    true
}

fn zero_mm() -> Expr {
    Expr::new("0 mm")
}

fn tol_mm() -> Expr {
    Expr::new("0.01 mm")
}

fn zero_deg() -> Expr {
    Expr::new("0 deg")
}

fn clearance_mm() -> Expr {
    Expr::new("10 mm")
}

fn retract_mm() -> Expr {
    Expr::new("2 mm")
}

fn feed_mm() -> Expr {
    Expr::new("600 mm")
}

fn plunge_mm() -> Expr {
    Expr::new("200 mm")
}

fn check_expr(e: &Expr, label: &str) -> Result<()> {
    let s = e.as_str().trim();
    if s.is_empty() || s.len() > 4096 {
        return Err(DocError::Invalid(format!("{label} must be a non-empty expression")));
    }
    Ok(())
}

/// A 2D milling operation. Geometry is linked; numeric cam-crate params are produced at post time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CamOperation {
    Contour {
        name: String,
        tool: u32,
        geometry: CamGeometry,
        params: CamContourParams,
        cut: CamCut,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spindle_rpm: Option<f64>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        coolant: bool,
    },
    Pocket {
        name: String,
        tool: u32,
        geometry: CamGeometry,
        params: CamPocketParams,
        cut: CamCut,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spindle_rpm: Option<f64>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        coolant: bool,
    },
}

impl CamOperation {
    pub fn name(&self) -> &str {
        match self {
            CamOperation::Contour { name, .. } | CamOperation::Pocket { name, .. } => name,
        }
    }

    pub fn tool(&self) -> u32 {
        match self {
            CamOperation::Contour { tool, .. } | CamOperation::Pocket { tool, .. } => *tool,
        }
    }

    pub fn geometry(&self) -> &CamGeometry {
        match self {
            CamOperation::Contour { geometry, .. } | CamOperation::Pocket { geometry, .. } => geometry,
        }
    }

    pub fn cut(&self) -> &CamCut {
        match self {
            CamOperation::Contour { cut, .. } | CamOperation::Pocket { cut, .. } => cut,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let name = self.name();
        if name.is_empty() || name.len() > 128 {
            return Err(DocError::Invalid("a CAM operation name must be 1 to 128 characters".into()));
        }
        self.geometry().validate()?;
        let cut = self.cut();
        check_expr(&cut.clearance, "clearance")?;
        check_expr(&cut.retract, "retract")?;
        check_expr(&cut.top, "top")?;
        check_expr(&cut.bottom, "bottom")?;
        check_expr(&cut.stepdown, "stepdown")?;
        check_expr(&cut.feed, "feed")?;
        check_expr(&cut.plunge_feed, "plunge_feed")?;
        match self {
            CamOperation::Contour { params, spindle_rpm, .. } => {
                check_expr(&params.stock_to_leave, "stock_to_leave")?;
                check_expr(&params.tolerance, "tolerance")?;
                check_expr(&params.ramp_angle, "ramp_angle")?;
                check_spindle(*spindle_rpm)?;
            }
            CamOperation::Pocket { params, spindle_rpm, .. } => {
                check_expr(&params.stepover, "stepover")?;
                check_expr(&params.stock_to_leave, "stock_to_leave")?;
                check_expr(&params.tolerance, "tolerance")?;
                check_expr(&params.ramp_angle, "ramp_angle")?;
                check_spindle(*spindle_rpm)?;
            }
        }
        Ok(())
    }
}

fn check_spindle(spindle: Option<f64>) -> Result<()> {
    if let Some(rpm) = spindle {
        if !(rpm > 0.0) || !rpm.is_finite() || rpm > 1.0e6 {
            return Err(DocError::Invalid("spindle_rpm must be above 0".into()));
        }
    }
    Ok(())
}

/// Default contour params.
pub fn default_contour_params() -> CamContourParams {
    CamContourParams { side: CamSide::Outside, climb: true, stock_to_leave: zero_mm(), tolerance: tol_mm(), ramp_angle: zero_deg() }
}

/// Default pocket params; `stepover` must still be set by the caller.
pub fn default_pocket_params(stepover: Expr) -> CamPocketParams {
    CamPocketParams { stepover, climb: true, stock_to_leave: zero_mm(), tolerance: tol_mm(), ramp_angle: zero_deg() }
}

/// Default cut expressions. `bottom` and `stepdown` come from the command.
pub fn default_cut(bottom: Expr, stepdown: Expr) -> CamCut {
    CamCut { clearance: clearance_mm(), retract: retract_mm(), top: zero_mm(), bottom, stepdown, feed: feed_mm(), plunge_feed: plunge_mm() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_region_roundtrips_to_region2() {
        let p = PolygonRegion { outer: vec![[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]], holes: vec![] };
        let r = p.to_region().unwrap();
        assert!((r.outer.signed_area() - 1200.0).abs() < 1e-9);
    }

    #[test]
    fn empty_settings_skip_serialize() {
        assert!(CamSettings::default().is_empty());
    }
}
