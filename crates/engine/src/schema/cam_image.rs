//! Contracts and result types for the pictures of a CAM job's toolpaths (`cmd/cam_image.rs`):
//! the `cam.view` command, the `images` part of a `cam.job` reply, and the extents of each
//! toolpath in work coordinates that every `cam.job` reply carries. They are how an agent, which
//! cannot open the HTML preview, checks a toolpath before it writes G-code: a PNG it can look at,
//! and numbers it can read without one (docs/cam-jobs.md, "Pictures and extents for agents").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::cam::{CamJob, RotaryRange};

/// Draw a CAM job's toolpaths over the design as a PNG, without writing G-code: the job is
/// planned and checked as `cam.job {dry_run: true}` plans it, and the picture shows the part,
/// every toolpath (or one operation's) in its own colour, rapids dashed, entries marked, the
/// tool where it starts cutting, work zero's axes and, on a machine with a rotary table, its
/// axis.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamViewParams {
    /// The job itself (see docs/cam-jobs.md). Give this or `job_path`.
    #[serde(default)]
    pub job: Option<CamJob>,
    /// A `.json` file holding the job, as `cam.job` reads it.
    #[serde(default)]
    pub job_path: Option<String>,
    /// The PNG file to write.
    pub path: String,
    /// Draw only the operation of this name. Default: every operation.
    #[serde(default)]
    pub op: Option<String>,
    /// The view: `iso`, `top`, `bottom`, `front`, `back`, `left` or `right`. Default: `iso`, or
    /// `front` (from -Y, the A axis level across the picture) on a machine with a rotary axis.
    #[serde(default)]
    pub view: Option<String>,
    /// Picture width in pixels, 16 to 4096. Default: 960.
    #[serde(default)]
    pub width: Option<u32>,
    /// Picture height in pixels, 16 to 4096. Default: 600.
    #[serde(default)]
    pub height: Option<u32>,
}

/// What `cam.view` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamViewReply {
    /// The PNG written.
    pub path: String,
    /// How the picture was drawn and how to read it.
    pub image: ImagesReport,
    /// Every operation of the job, in program order, with its toolpath's extents.
    pub operations: Vec<ViewOperation>,
    /// Whether every check of the job passed (as `cam.job` with `dry_run`).
    pub passed: bool,
    /// The checks that failed.
    pub failures: Vec<String>,
    /// How the bodies were tessellated, and anything else worth knowing.
    pub notes: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One operation of a `cam.view` reply.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ViewOperation {
    pub name: String,
    /// Its `type` (an indexed operation: `indexed <type>`).
    pub kind: String,
    /// Whether the picture draws it.
    pub drawn: bool,
    pub extents: ToolpathExtents,
}

/// Where a toolpath goes, in work coordinates (the design less the job's origin), mm.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct ToolpathExtents {
    /// Lowest and highest tool tip X, Y, Z of every move, rapids included.
    pub all: [[f64; 3]; 2],
    /// Lowest and highest tool tip X, Y, Z of the cutting moves (absent with none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cut: Option<[[f64; 3]; 2]>,
    /// Each rotary axis's lowest and highest angle as the written program turns it in this
    /// operation, degrees, rewinds included (`cam.gcode.check` reads the same from the file); none on
    /// a 3-axis machine.
    pub rotary: Vec<RotaryRange>,
    /// The same in one line, e.g. `cut X -14..14 Y -14..14 Z -1..15; all moves X -14..14 Y
    /// -14..14 Z -1..30; A 0..360°`.
    pub summary: String,
}

/// The pictures `cam.job` (`image`, `images_dir`) or `cam.view` drew.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ImagesReport {
    /// The view they are drawn from.
    pub view: String,
    pub width: u32,
    pub height: u32,
    /// What drew the part (`gpu`, `gpu-software` or `cpu`); the toolpaths are drawn over it in
    /// software either way.
    pub renderer: String,
    /// The camera, in design coordinates, as `view.apply {camera}` and the MCP `screenshot`
    /// tool's `camera` take it: the same view of the design without the toolpaths.
    pub camera: ViewCamera,
    /// The whole job's picture (`image`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// One picture per operation (`images_dir`), in program order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    /// The colour each operation is drawn in.
    pub legend: Vec<LegendEntry>,
    /// How to read the pictures.
    pub key: Vec<String>,
}

/// An orthographic camera: what it looks at, from where, how far back.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ViewCamera {
    pub target: [f64; 3],
    /// Radians about Z; 0 looks along +Y.
    pub yaw: f64,
    /// Radians above the XY plane.
    pub pitch: f64,
    pub distance: f64,
    /// 0: orthographic.
    pub fov: f64,
}

/// An operation's colour in the pictures.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct LegendEntry {
    pub operation: String,
    /// The colour's name (`orange`, `purple`, ...).
    pub colour: String,
    /// The colour as `#rrggbb`.
    pub rgb: String,
}

#[cfg(test)]
mod tests {
    use crate::schema::cam_image::CamViewParams;
    use crate::schema::input_schema_for;
    use crate::schema::tests::assert_keys;

    #[test]
    fn the_view_contract_lists_its_keys() {
        assert_keys(input_schema_for::<CamViewParams>(), &["height", "job", "job_path", "op", "path", "view", "width"]);
    }
}
