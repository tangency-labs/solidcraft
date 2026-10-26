//! `--solidcraft-cases`: whole-operation regression cases from SolidCraft, scored
//! against analytic expected values.
//!
//! Each file in `fixtures/solidcraft/` holds the cases of one SolidCraft issue that was
//! found and fixed on SolidCraft's earlier kernel: the issue, the fixing PR and its test,
//! and per case a list of `SM_API`-level steps (primitives, booleans, fillet, offset)
//! whose last body, `result`, is checked against values known in closed form (volume,
//! lump count, face bounds). They keep those bugs fixed once the operations run end to
//! end on solidcore.
//!
//! Like `--ssi-corpus`, this is a correctness report, not the gate: it never changes a
//! unit's difftest status and verdicts never fail the run. Only the Rust engine runs for
//! now (the C++ engine needs its own shim). A step that reaches unported code stops the
//! case as `DEFERRED` on the unit returned in [`SolidError::Unported`]; a step whose
//! `SM_API` entry point has no Rust function yet is deferred on the unit that owns that
//! entry point (`harness/callgraph/units.json`), and one with no `SM_API` entry point at
//! all says so. Nothing is skipped or faked.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use serde_json::{json, Value};

use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives as primitives;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep as sweeps;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::Brep;
use solidcore::SolidError;

use crate::ssi::glob_match;
use crate::HarnessError;

/// Relative accuracy passed to `SmApiBrepComputeVolume`: its finest (upstream clamps
/// to `[1e-8, 1e-1]`). The cases check volumes to absolute tolerances as tight as
/// `0.01` on `30603` (3e-7 relative), finer than `1e-6` integrates: at `1e-6` both
/// engines measure the same B-rep of #3396's stack `0.157` over its closed form, at
/// `1e-8` `0.0008`.
pub const VOLUME_ACCURACY: f64 = 1e-8;

/// The body a case's expectations are checked on.
pub const RESULT: &str = "result";

/// Verdicts, in summary order.
pub const VERDICTS: [&str; 5] = ["PASS", "FAIL", "UNCHECKED", "ERROR", "DEFERRED"];

type P3 = [f64; 3];

/// One issue's file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseFile {
    /// The SolidCraft issue.
    pub issue: String,
    /// The SolidCraft PR that fixed it.
    pub pr: String,
    /// That PR's test the cases come from.
    pub test: String,
    /// What broke, where the geometry and values come from.
    pub summary: String,
    /// The cases.
    pub cases: Vec<Case>,
}

/// One case: steps building `result`, and what `result` must be.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Unique within its file.
    pub name: String,
    /// What this variant is.
    #[serde(default)]
    pub note: Option<String>,
    /// Opt-in `Kernel::fixes` fields (`UpstreamFixes`) the case runs with, by field name
    /// (none: pinned SMLib behaviour).
    #[serde(default)]
    pub fixes: Vec<String>,
    /// Run in order; each writes the body `id`.
    pub steps: Vec<Step>,
    /// Checked on the body `result`.
    pub expect: Expect,
}

/// One operation (an `SM_API` call, or a deferred higher-level recipe). `id` is the
/// body it writes; `a`, `b` and `body` name bodies written by earlier steps.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// `SmApiCreateBox`.
    Box {
        /// Body written.
        id: String,
        /// Lower corner.
        origin: P3,
        /// Size along X, Y, Z.
        size: P3,
    },
    /// `SmApiCreateSphere`.
    Sphere {
        /// Body written.
        id: String,
        /// Centre.
        center: P3,
        /// Radius.
        radius: f64,
    },
    /// `SmApiCreateCylinder`, built along Z and, for another `axis`, turned onto it
    /// with `SmApiRotate` about `origin`.
    Cylinder {
        /// Body written.
        id: String,
        /// Centre of the base.
        origin: P3,
        /// Axis direction (default +Z).
        #[serde(default = "z_axis")]
        axis: P3,
        /// Radius.
        radius: f64,
        /// Length along the axis.
        height: f64,
    },
    /// A copy (`SmApiBrepCopy`) of `body` turned about the line through `point` along `axis`, with
    /// `SmApiRotate`.
    Rotate {
        /// Body written.
        id: String,
        /// Body turned (left as it is).
        body: String,
        /// A point on the axis.
        point: P3,
        /// Axis direction.
        axis: P3,
        /// Angle in degrees.
        degrees: f64,
    },
    /// `SmApiBooleanUnion(a, b)`.
    Union {
        /// Body written.
        id: String,
        /// Target.
        a: String,
        /// Tool.
        b: String,
    },
    /// `SmApiBooleanDifference(a, b)`.
    Difference {
        /// Body written.
        id: String,
        /// Target.
        a: String,
        /// Tool.
        b: String,
        /// When given, `b` is cut once per position (moved with `SmApiTranslate`), one
        /// cut after the other.
        #[serde(default)]
        pattern: Option<Pattern>,
    },
    /// `SmApiBooleanIntersection(a, b)`.
    Intersection {
        /// Body written.
        id: String,
        /// Target.
        a: String,
        /// Tool.
        b: String,
    },
    /// `SmApiCreateLinearSweep`: the closed `profile` (built in `plane`, moved by
    /// `origin`) swept `distance` along the plane's normal, capped at both ends.
    Extrude {
        /// Body written.
        id: String,
        /// Closed loops of segments, in the plane's (u, v) coordinates.
        profile: Vec<Seg>,
        /// Plane of the profile (default XY).
        #[serde(default)]
        plane: ProfilePlane,
        /// Where the plane's origin sits.
        #[serde(default)]
        origin: P3,
        /// Sweep length along the normal.
        distance: f64,
    },
    /// `SmApiCreateRotationalSweep`: the closed `profile` (built in `plane`, moved by
    /// `origin`) turned `angle_deg` about the axis through `axis_point` along
    /// `axis_dir`, capped at both ends.
    Revolve {
        /// Body written.
        id: String,
        /// Closed loops of segments, in the plane's (u, v) coordinates.
        profile: Vec<Seg>,
        /// Plane of the profile (default XY).
        #[serde(default)]
        plane: ProfilePlane,
        /// Where the plane's origin sits.
        #[serde(default)]
        origin: P3,
        /// A point on the axis (global coordinates).
        axis_point: P3,
        /// Axis direction (global coordinates).
        axis_dir: P3,
        /// Turn in degrees.
        angle_deg: f64,
    },
    /// A circle of `radius` swept along `path` (built in `plane`, moved by `origin`),
    /// capped at both ends: `SmApiCreatePipeSweep` for one segment,
    /// `SmApiCreateSweepAlongPlanarPath` of a circle normal to the path start for more.
    Pipe {
        /// Body written.
        id: String,
        /// Pipe radius.
        radius: f64,
        /// Lines and arcs joined end to end, in the plane's (u, v) coordinates.
        path: Vec<Seg>,
        /// Plane of the path (default XY).
        #[serde(default)]
        plane: ProfilePlane,
        /// Where the plane's origin sits.
        #[serde(default)]
        origin: P3,
    },
    /// `SmApiCreateSkinPrimitive`: a solid lofted through `sections` in order, each one
    /// closed profile, of `degree` in the loft direction (1, ruled, by default), capped
    /// at the first and last section.
    Loft {
        /// Body written.
        id: String,
        /// The sections, at least two.
        sections: Vec<LoftSection>,
        /// Degree in the loft direction: 1 linear, 2 quadratic, 3 cubic.
        #[serde(default = "ruled")]
        degree: usize,
    },
    /// `SmApiCreateCurveSweep`: the closed `profile` (built in `plane`, moved by
    /// `origin`, so its plane is normal to the path's start tangent) swept along the
    /// single `path` arc or line (in the XY plane at the world origin), capped at both
    /// ends. The profile keeps its place relative to the path's start.
    Sweep {
        /// Body written.
        id: String,
        /// Closed loops of segments, in the plane's (u, v) coordinates.
        profile: Vec<Seg>,
        /// Plane of the profile (default XY).
        #[serde(default)]
        plane: ProfilePlane,
        /// Where the plane's origin sits.
        #[serde(default)]
        origin: P3,
        /// The path: one arc or line in the plane's (u, v) coordinates.
        path: Seg,
    },
    /// Reflection of `body` in the plane `normal = at`; the original is not changed, join
    /// the two with a `union` step. `SM_API` has no mirror, so this defers (see `step_rust`).
    Mirror {
        /// Body written.
        id: String,
        /// Body reflected.
        body: String,
        /// Axis the mirror plane is perpendicular to.
        normal: MirrorAxis,
        /// Where the plane cuts that axis.
        #[serde(default)]
        at: f64,
    },
    /// `SmApiFilletEdges`, circular section.
    Fillet {
        /// Body written.
        id: String,
        /// Body filleted.
        body: String,
        /// A point on each edge to fillet.
        edges: Vec<P3>,
        /// Radius.
        radius: f64,
    },
    /// Offset selected faces along their outward normals, the rest of the body
    /// following (SolidCraft's Offset Face).
    OffsetFaces {
        /// Body written.
        id: String,
        /// Body offset.
        body: String,
        /// A point on each face to offset.
        faces: Vec<P3>,
        /// Offset distance (negative: inward).
        distance: f64,
    },
    /// SolidCraft #3170: four inside flanges on an XY rectangle, then close the
    /// corner between flanges 0 (front) and 3 (left). Flange order is front,
    /// right, rear, left; negative angles reverse the bend direction.
    SheetCorner {
        /// Body written.
        id: String,
        /// Rectangle width and depth, starting at the origin.
        base_size: [f64; 2],
        /// Sheet thickness.
        thickness: f64,
        /// Inside bend radius.
        bend_radius: f64,
        /// Neutral-axis fraction used for the flat pattern.
        k_factor: f64,
        /// Flange height, using SolidCraft's inside-height convention.
        height: f64,
        /// Signed bend angle in degrees.
        angle_degrees: f64,
        /// Clearance between the covered end and covering wall's inner face.
        gap: f64,
        /// Notch relief width and depth.
        relief_size: [f64; 2],
        /// Corner notch size.
        corner_relief: f64,
        /// Which of flanges 0 and 3 covers the other.
        covering: usize,
        /// Overlap length: half the thickness for Partial, the thickness for Full.
        overlap: f64,
    },
}

/// One piece of a profile, in the profile plane's (u, v) coordinates. Pieces that share
/// end points form closed loops; a profile may hold several (outer boundary and holes).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Seg {
    /// `SmApiCreateLineSegment`.
    Line {
        /// Start.
        from: [f64; 2],
        /// End.
        to: [f64; 2],
    },
    /// A closed polygon: one line segment per consecutive pair, the last back to the
    /// first.
    Polygon {
        /// Corners, at least three.
        points: Vec<[f64; 2]>,
    },
    /// `SmApiCreateArc`, counter-clockwise from `start_deg` to `end_deg`.
    Arc {
        /// Centre.
        center: [f64; 2],
        /// Radius.
        radius: f64,
        /// Start angle from +u.
        start_deg: f64,
        /// End angle from +u.
        end_deg: f64,
    },
    /// `SmApiCreateCircle`, analytic.
    Circle {
        /// Centre.
        center: [f64; 2],
        /// Radius.
        radius: f64,
    },
}

/// One section of a `loft`: a closed profile built in `plane` and moved by `origin`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoftSection {
    /// One closed loop of segments, in the plane's (u, v) coordinates.
    pub profile: Vec<Seg>,
    /// Plane of the profile (default XY).
    #[serde(default)]
    pub plane: ProfilePlane,
    /// Where the plane's origin sits.
    #[serde(default)]
    pub origin: P3,
}

/// The plane a profile is drawn in, as a right-handed rotation of XY.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfilePlane {
    /// u along X, v along Y, normal +Z.
    #[default]
    Xy,
    /// u along Y, v along Z, normal +X.
    Yz,
    /// u along Z, v along X, normal +Y.
    Zx,
    /// u along X, v along Z, normal -Y (XY turned +90 degrees about X).
    Xz,
}

/// The axis a mirror plane is perpendicular to.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MirrorAxis {
    /// The plane `x = at`.
    X,
    /// The plane `y = at`.
    Y,
    /// The plane `z = at`.
    Z,
}

/// A grid of positions: `pitch[0] * i + pitch[1] * j` for `i < count[0]`,
/// `j < count[1]`, `i` varying fastest.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    /// Positions along each pitch vector.
    pub count: [usize; 2],
    /// The two pitch vectors.
    pub pitch: [P3; 2],
}

/// What `result` must be. Absent fields are not checked.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Bounded regions (`0`: the operation leaves nothing).
    #[serde(default)]
    pub lumps: Option<usize>,
    /// Enclosed volume.
    #[serde(default)]
    pub volume: Option<VolumeExpect>,
    /// At most this many faces on cylinders.
    #[serde(default)]
    pub cylinder_faces_max: Option<usize>,
    /// Sheet-corner geometry and flat-pattern invariants from SolidCraft #3170.
    #[serde(default)]
    pub sheet_corner: Option<SheetCornerExpect>,
}

/// A valid folded solid, one connected non-overlapping flat pattern whose area
/// changes when the corner is joined, and volume matching flat area times thickness.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetCornerExpect {
    /// Minimum signed distances of flange ends 0 and 3 to the other wall's inner
    /// plane. Covering is negative overlap; covered is positive gap.
    pub end_face_distances: [f64; 2],
    /// Absolute tolerance of the signed-distance checks, in millimetres.
    pub distance_tol: f64,
    /// Number of connected flat-pattern regions.
    pub flat_regions: usize,
    /// Relative tolerance of volume against flat area times thickness.
    pub volume_relative_tol: f64,
}

/// An analytic volume.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeExpect {
    /// mm^3.
    pub value: f64,
    /// Absolute tolerance, mm^3.
    pub tol: f64,
    /// How `value` was derived.
    pub formula: String,
}

fn z_axis() -> P3 {
    [0.0, 0.0, 1.0]
}

fn ruled() -> usize {
    1
}

/// A loaded case: `<file stem>/<name>` and its file.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// `<file stem>/<name>`.
    pub id: String,
    /// The file's fields (shared by its cases).
    pub file: CaseFile,
    /// The case.
    pub case: Case,
}

/// The case directory.
pub fn cases_dir(root: &Path) -> PathBuf {
    root.join("fixtures").join("solidcraft")
}

/// Every case in `dir` whose id matches `glob`, sorted by file then file order, after
/// checking each case ([`validate`]).
///
/// # Errors
///
/// An unreadable or invalid file, or duplicate case names in one file.
pub fn load(dir: &Path, glob: Option<&str>) -> Result<Vec<Loaded>, HarnessError> {
    let rd =
        std::fs::read_dir(dir).map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?;
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let text = std::fs::read_to_string(&path)
            .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
        let file: CaseFile = serde_json::from_str(&text)
            .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
        let mut names = std::collections::BTreeSet::new();
        for case in &file.cases {
            if !names.insert(case.name.as_str()) {
                return Err(HarnessError::new(format!(
                    "{}: duplicate case `{}`",
                    path.display(),
                    case.name
                )));
            }
            validate(case).map_err(|e| {
                HarnessError::new(format!("{}: {}: {e}", path.display(), case.name))
            })?;
        }
        for case in &file.cases {
            let id = format!("{stem}/{}", case.name);
            if glob.is_none_or(|g| glob_match(g, &id)) {
                out.push(Loaded {
                    id,
                    file: file.clone(),
                    case: case.clone(),
                });
            }
        }
    }
    Ok(out)
}

impl Step {
    /// The body this step writes.
    pub fn id(&self) -> &str {
        match self {
            Self::Box { id, .. }
            | Self::Sphere { id, .. }
            | Self::Cylinder { id, .. }
            | Self::Rotate { id, .. }
            | Self::Union { id, .. }
            | Self::Difference { id, .. }
            | Self::Intersection { id, .. }
            | Self::Extrude { id, .. }
            | Self::Revolve { id, .. }
            | Self::Pipe { id, .. }
            | Self::Loft { id, .. }
            | Self::Sweep { id, .. }
            | Self::Mirror { id, .. }
            | Self::Fillet { id, .. }
            | Self::OffsetFaces { id, .. }
            | Self::SheetCorner { id, .. } => id,
        }
    }

    /// The bodies this step reads.
    pub fn inputs(&self) -> Vec<&str> {
        match self {
            Self::Box { .. }
            | Self::Sphere { .. }
            | Self::Cylinder { .. }
            | Self::Extrude { .. }
            | Self::Revolve { .. }
            | Self::Pipe { .. }
            | Self::Loft { .. }
            | Self::Sweep { .. }
            | Self::SheetCorner { .. } => Vec::new(),
            Self::Union { a, b, .. }
            | Self::Difference { a, b, .. }
            | Self::Intersection { a, b, .. } => vec![a, b],
            Self::Fillet { body, .. }
            | Self::OffsetFaces { body, .. }
            | Self::Rotate { body, .. }
            | Self::Mirror { body, .. } => vec![body],
        }
    }
}

/// A profile has pieces, and every number in them is finite.
fn valid_profile(profile: &[Seg]) -> bool {
    let pt = |p: &[f64; 2]| p.iter().all(|x| x.is_finite());
    !profile.is_empty()
        && profile.iter().all(|s| match s {
            Seg::Line { from, to } => pt(from) && pt(to),
            Seg::Polygon { points } => points.len() >= 3 && points.iter().all(pt),
            Seg::Arc {
                center,
                radius,
                start_deg,
                end_deg,
            } => pt(center) && radius.is_finite() && start_deg.is_finite() && end_deg.is_finite(),
            Seg::Circle { center, radius } => pt(center) && radius.is_finite(),
        })
}

/// A sweep path has only lines and arcs (and its numbers are finite, checked by
/// [`valid_profile`]).
fn valid_path_segments(path: &[Seg]) -> bool {
    path.iter()
        .all(|s| matches!(s, Seg::Line { .. } | Seg::Arc { .. }))
}

/// Every step reads only bodies written before it, the last step writes [`RESULT`],
/// and every number is finite.
///
/// # Errors
///
/// What is wrong, as text.
pub fn validate(case: &Case) -> Result<(), String> {
    let mut defined = std::collections::BTreeSet::new();
    for (i, step) in case.steps.iter().enumerate() {
        for input in step.inputs() {
            if !defined.contains(input) {
                return Err(format!("step {i} reads `{input}` before it is written"));
            }
        }
        let finite = match step {
            Step::Box { origin, size, .. } => origin.iter().chain(size).all(|x| x.is_finite()),
            Step::Sphere { center, radius, .. } => {
                center.iter().all(|x| x.is_finite()) && radius.is_finite()
            }
            Step::Cylinder {
                origin,
                axis,
                radius,
                height,
                ..
            } => {
                origin.iter().chain(axis).all(|x| x.is_finite())
                    && radius.is_finite()
                    && height.is_finite()
            }
            Step::Rotate {
                point,
                axis,
                degrees,
                ..
            } => point.iter().chain(axis).all(|x| x.is_finite()) && degrees.is_finite(),
            Step::Difference {
                pattern: Some(p), ..
            } => p.pitch.iter().flatten().all(|x| x.is_finite()),
            Step::Extrude {
                profile,
                origin,
                distance,
                ..
            } => {
                origin.iter().all(|x| x.is_finite())
                    && distance.is_finite()
                    && valid_profile(profile)
            }
            Step::Revolve {
                profile,
                origin,
                axis_point,
                axis_dir,
                angle_deg,
                ..
            } => {
                origin
                    .iter()
                    .chain(axis_point)
                    .chain(axis_dir)
                    .all(|x| x.is_finite())
                    && angle_deg.is_finite()
                    && valid_profile(profile)
            }
            Step::Pipe {
                radius,
                path,
                origin,
                ..
            } => {
                radius.is_finite()
                    && origin.iter().all(|x| x.is_finite())
                    && valid_path_segments(path)
                    && valid_profile(path)
            }
            Step::Loft { sections, .. } => {
                sections.len() >= 2
                    && sections.iter().all(|s| {
                        s.origin.iter().all(|x| x.is_finite()) && valid_profile(&s.profile)
                    })
            }
            Step::Mirror { at, .. } => at.is_finite(),
            Step::Sweep {
                profile,
                origin,
                path,
                ..
            } => {
                origin.iter().all(|x| x.is_finite())
                    && valid_profile(profile)
                    && valid_path_segments(std::slice::from_ref(path))
            }
            Step::Fillet { edges, radius, .. } => {
                edges.iter().flatten().all(|x| x.is_finite()) && radius.is_finite()
            }
            Step::OffsetFaces {
                faces, distance, ..
            } => faces.iter().flatten().all(|x| x.is_finite()) && distance.is_finite(),
            Step::SheetCorner {
                base_size,
                thickness,
                bend_radius,
                k_factor,
                height,
                angle_degrees,
                gap,
                relief_size,
                corner_relief,
                covering,
                overlap,
                ..
            } => {
                if !matches!(covering, 0 | 3)
                    || !base_size
                        .iter()
                        .chain(relief_size)
                        .all(|x| x.is_finite() && *x > 0.0)
                    || ![
                        *thickness,
                        *bend_radius,
                        *height,
                        *gap,
                        *corner_relief,
                        *overlap,
                    ]
                    .iter()
                    .all(|x| x.is_finite() && *x > 0.0)
                    || !k_factor.is_finite()
                    || !(0.0..=1.0).contains(k_factor)
                    || !angle_degrees.is_finite()
                    || *angle_degrees == 0.0
                    || angle_degrees.abs() >= 180.0
                    || overlap > thickness
                {
                    return Err(format!("step {i} has invalid sheet-corner parameters"));
                }
                true
            }
            Step::Union { .. } | Step::Difference { .. } | Step::Intersection { .. } => true,
        };
        if !finite {
            return Err(format!("step {i} has a non-finite number"));
        }
        defined.insert(step.id());
    }
    if case.steps.last().map(Step::id) != Some(RESULT) {
        return Err(format!("the last step must write `{RESULT}`"));
    }
    if let Some(v) = &case.expect.volume {
        if !(v.value.is_finite() && v.tol.is_finite() && v.tol > 0.0) {
            return Err("volume value and tol must be finite, tol positive".into());
        }
    }
    if let Some(e) = &case.expect.sheet_corner {
        if !e.end_face_distances.iter().all(|x| x.is_finite())
            || !e.distance_tol.is_finite()
            || e.distance_tol <= 0.0
            || !e.volume_relative_tol.is_finite()
            || e.volume_relative_tol <= 0.0
            || e.flat_regions == 0
        {
            return Err("invalid sheet-corner expectations".into());
        }
    }
    Ok(())
}

// ---- the Rust engine -----------------------------------------------------------------

/// What blocks a deferred case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deferral {
    /// The plan unit that will port the missing code, or `no SM_API entry point`.
    pub blocked_by: String,
    /// The upstream symbol (or what is missing).
    pub symbol: String,
}

/// What `result` measured as.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    /// `SmApiBrepComputeVolume`.
    pub volume: f64,
    /// Regions other than the infinite one.
    pub lumps: usize,
    /// Faces, edges and vertices (`GetNumFaces`, `GetNumEdges`, `GetNumVertices`).
    pub topology: [usize; 3],
    /// Faces on a cylinder (`SmCone::IsCylinder`).
    pub cylinder_faces: usize,
}

/// How a case's run ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Run {
    /// Every step ran; `result` measured.
    Done(Measured),
    /// A step reached code that is not ported yet.
    Deferred(Deferral),
    /// A step returned an error other than `Unported`.
    Error(String),
}

fn p3(p: P3) -> Point3 {
    let [x, y, z] = p;
    Vec3::new(x, y, z)
}

fn solid(e: &SolidError) -> Run {
    match e {
        SolidError::Unported { upstream, unit } => Run::Deferred(Deferral {
            blocked_by: (*unit).to_string(),
            symbol: (*upstream).to_string(),
        }),
        _ => Run::Error(e.to_string()),
    }
}

/// The operand `id` for the next step. A plain [`Clone`] resets every geometry owner, and
/// a B-rep cache made from the clone then holds no edge or face, so the boolean's ray
/// fires found nothing and classified a nested operand's faces as outside (a void was
/// lost). `clone_standing_for_self` keeps the owners, as upstream's move does.
fn take(bodies: &BTreeMap<String, Brep>, id: &str) -> Result<Brep, Run> {
    bodies
        .get(id)
        .ok_or_else(|| Run::Error(format!("no body `{id}`")))?
        .clone_standing_for_self()
        .map_err(|e| solid(&e))
}

/// `SmApiBrepCopy(b)`: a copy that owns its geometry, so a later `SmApiTranslate`
/// moves its curves and surfaces with its vertices. A plain [`Clone`] is not upstream's
/// copy: its geometry keeps the source's owner names and stays put when the clone is
/// transformed. Copy `n` names its objects in its own key domain.
fn copy(k: &mut Kernel, b: &Brep, n: u64) -> Result<Brep, Run> {
    let named = |entity, domain: u64| {
        let r = p3::topology_ref(0, entity);
        ObjectRef {
            type_code: r.type_code,
            key: ObjectKey(domain + r.type_code as u64 * 1_000_000 + (r.key.0 & 0xffff_ffff)),
        }
    };
    let target_key = |entity| named(entity, n * 100_000_000);
    let mut state = ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target_key;
    p3::api_brep_copy(k, &mut env, Some(b), &|entity| named(entity, 0)).map_err(|e| solid(&e))
}

/// A cylinder along Z turned onto `axis` about `origin`.
fn cylinder(k: &mut Kernel, origin: P3, axis: P3, radius: f64, height: f64) -> Result<Brep, Run> {
    let mut b =
        primitives::create_cylinder(k, p3(origin), radius, height).map_err(|e| solid(&e))?;
    let a = p3(axis).unitize().map_err(|e| solid(&e))?;
    let z = Vec3::new(0.0, 0.0, 1.0);
    let cos = a.dot(z).clamp(-1.0, 1.0);
    if cos < 1.0 {
        let turn = if cos > -1.0 {
            z.cross(a)
        } else {
            Vec3::new(1.0, 0.0, 0.0)
        };
        api_rotate(
            k,
            Some(TransformObject::Brep(&mut b)),
            p3(origin),
            turn,
            cos.acos().to_degrees(),
        )
        .map_err(|e| solid(&e))?;
    }
    Ok(b)
}

/// The curves of `profile`, drawn in XY, turned onto `plane` about the origin and moved
/// by `origin`.
fn profile_curves(
    k: &mut Kernel,
    profile: &[Seg],
    plane: ProfilePlane,
    origin: P3,
) -> Result<Vec<Curve>, Run> {
    let at = |[u, v]: [f64; 2]| Vec3::new(u, v, 0.0);
    let mut out: Vec<Curve> = Vec::new();
    for seg in profile {
        match seg {
            Seg::Line { from, to } => {
                let l = curves::create_line_segment(at(*from), at(*to)).map_err(|e| solid(&e))?;
                out.push(Curve::from(l));
            }
            Seg::Polygon { points } => {
                // Each point to the next, the last back to the first.
                for (from, to) in points.iter().zip(points.iter().cycle().skip(1)) {
                    let l =
                        curves::create_line_segment(at(*from), at(*to)).map_err(|e| solid(&e))?;
                    out.push(Curve::from(l));
                }
            }
            Seg::Arc {
                center,
                radius,
                start_deg,
                end_deg,
            } => {
                let a = curves::create_arc(at(*center), *radius, *start_deg, *end_deg)
                    .map_err(|e| solid(&e))?;
                out.push(Curve::from(a));
            }
            Seg::Circle { center, radius } => {
                let c = curves::create_analytic_circle(k, at(*center), *radius)
                    .map_err(|e| solid(&e))?;
                out.push(Curve::from(c));
            }
        }
    }
    // XY -> YZ is the turn x -> y -> z -> x: 120 degrees about (1, 1, 1); ZX is 240.
    // XZ is XY turned 90 degrees about X, so u is x and v is z.
    let turn = match plane {
        ProfilePlane::Xy => None,
        ProfilePlane::Yz => Some((Vec3::new(1.0, 1.0, 1.0), 120.0)),
        ProfilePlane::Zx => Some((Vec3::new(1.0, 1.0, 1.0), 240.0)),
        ProfilePlane::Xz => Some((Vec3::X, 90.0)),
    };
    for c in &mut out {
        if let Some((axis, deg)) = turn {
            api_rotate(
                k,
                Some(TransformObject::Curve(c)),
                Vec3::new(0.0, 0.0, 0.0),
                axis,
                deg,
            )
            .map_err(|e| solid(&e))?;
        }
        api_translate(k, Some(TransformObject::Curve(c)), p3(origin)).map_err(|e| solid(&e))?;
    }
    Ok(out)
}

fn step_rust(k: &mut Kernel, bodies: &mut BTreeMap<String, Brep>, step: &Step) -> Result<(), Run> {
    let body = match step {
        Step::Box { origin, size, .. } => {
            let [x, y, z] = *size;
            primitives::create_box(k, p3(*origin), x, y, z).map_err(|e| solid(&e))?
        }
        Step::Sphere { center, radius, .. } => {
            primitives::create_sphere(k, p3(*center), *radius).map_err(|e| solid(&e))?
        }
        Step::Cylinder {
            origin,
            axis,
            radius,
            height,
            ..
        } => cylinder(k, *origin, *axis, *radius, *height)?,
        Step::Rotate {
            body,
            point,
            axis,
            degrees,
            ..
        } => {
            // A real copy (see `copy`): a plain clone would stay put when turned.
            let source = take(bodies, body)?;
            let mut turned = copy(k, &source, 1000 + bodies.len() as u64)?;
            api_rotate(
                k,
                Some(TransformObject::Brep(&mut turned)),
                p3(*point),
                Vec3::new(axis[0], axis[1], axis[2]),
                *degrees,
            )
            .map_err(|e| solid(&e))?;
            turned
        }
        Step::Union { a, b, .. } => {
            boolean::boolean_union(k, take(bodies, a)?, take(bodies, b)?).map_err(|e| solid(&e))?
        }
        Step::Intersection { a, b, .. } => {
            boolean::boolean_intersection(k, take(bodies, a)?, take(bodies, b)?)
                .map_err(|e| solid(&e))?
        }
        Step::Difference {
            a,
            b,
            pattern: None,
            ..
        } => boolean::boolean_difference(k, take(bodies, a)?, take(bodies, b)?)
            .map_err(|e| solid(&e))?,
        Step::Difference {
            a,
            b,
            pattern: Some(p),
            ..
        } => {
            let mut target = take(bodies, a)?;
            let tool_source = take(bodies, b)?;
            let mut copies = 0;
            let [p0, p1] = p.pitch;
            for j in 0..p.count[1] {
                for i in 0..p.count[0] {
                    copies += 1;
                    let mut tool = copy(k, &tool_source, copies)?;
                    let (fi, fj) = (i as f64, j as f64);
                    let shift = Vec3::new(
                        p0[0] * fi + p1[0] * fj,
                        p0[1] * fi + p1[1] * fj,
                        p0[2] * fi + p1[2] * fj,
                    );
                    api_translate(k, Some(TransformObject::Brep(&mut tool)), shift)
                        .map_err(|e| solid(&e))?;
                    target = boolean::boolean_difference(k, target, tool).map_err(|e| solid(&e))?;
                }
            }
            target
        }
        Step::Extrude {
            profile,
            plane,
            origin,
            distance,
            ..
        } => {
            let cs = profile_curves(k, profile, *plane, *origin)?;
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            let normal = match plane {
                ProfilePlane::Xy => Vec3::new(0.0, 0.0, 1.0),
                ProfilePlane::Yz => Vec3::new(1.0, 0.0, 0.0),
                ProfilePlane::Zx => Vec3::new(0.0, 1.0, 0.0),
                ProfilePlane::Xz => Vec3::new(0.0, -1.0, 0.0),
            };
            sweeps::create_linear_sweep(k, &refs, normal, *distance, true).map_err(|e| solid(&e))?
        }
        Step::Revolve {
            profile,
            plane,
            origin,
            axis_point,
            axis_dir,
            angle_deg,
            ..
        } => {
            let cs = profile_curves(k, profile, *plane, *origin)?;
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            sweeps::create_rotational_sweep(
                k,
                &refs,
                p3(*axis_point),
                p3(*axis_dir),
                *angle_deg,
                true,
            )
            .map_err(|e| solid(&e))?
        }
        Step::Pipe {
            radius,
            path,
            plane,
            origin,
            ..
        } => {
            let cs = profile_curves(k, path, *plane, *origin)?;
            if let [single] = cs.as_slice() {
                sweeps::create_pipe_sweep(k, *radius, Some(single), true).map_err(|e| solid(&e))?
            } else {
                // The profile: a circle at the origin turned from +Z onto the path's start
                // tangent; the sweep moves its centre to the path start.
                let (u, v) = match path.first() {
                    Some(Seg::Line { from, to }) => (to[0] - from[0], to[1] - from[1]),
                    Some(Seg::Arc { start_deg, .. }) => {
                        let (s, c) = start_deg.to_radians().sin_cos();
                        (-s, c)
                    }
                    _ => return Err(Run::Error("pipe path starts with no line or arc".into())),
                };
                let t = match plane {
                    ProfilePlane::Xy => Vec3::new(u, v, 0.0),
                    ProfilePlane::Yz => Vec3::new(0.0, u, v),
                    ProfilePlane::Zx => Vec3::new(v, 0.0, u),
                    ProfilePlane::Xz => Vec3::new(u, 0.0, v),
                }
                .unitize()
                .map_err(|e| solid(&e))?;
                let mut circle = Curve::from(
                    curves::create_analytic_circle(k, Vec3::new(0.0, 0.0, 0.0), *radius)
                        .map_err(|e| solid(&e))?,
                );
                let z = Vec3::new(0.0, 0.0, 1.0);
                let cos = t.dot(z).clamp(-1.0, 1.0);
                if cos < 1.0 {
                    let axis = if cos > -1.0 {
                        z.cross(t)
                    } else {
                        Vec3::new(1.0, 0.0, 0.0)
                    };
                    api_rotate(
                        k,
                        Some(TransformObject::Curve(&mut circle)),
                        Vec3::new(0.0, 0.0, 0.0),
                        axis,
                        cos.acos().to_degrees(),
                    )
                    .map_err(|e| solid(&e))?;
                }
                let refs: Vec<&Curve> = cs.iter().collect();
                sweeps::create_sweep_along_planar_path(k, &[&circle], &refs, true, 3)
                    .map_err(|e| solid(&e))?
            }
        }
        Step::Loft {
            sections, degree, ..
        } => {
            let mut counts = Vec::new();
            let mut cs = Vec::new();
            for s in sections {
                let section = profile_curves(k, &s.profile, s.plane, s.origin)?;
                counts.push(section.len());
                cs.extend(section);
            }
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            create_skin_primitive(k, &counts, &refs, *degree, true)
                .map_err(|e| solid(&e))?
                .brep
        }
        Step::Sweep {
            profile,
            plane,
            origin,
            path,
            ..
        } => {
            let cs = profile_curves(k, profile, *plane, *origin)?;
            // The path is one arc or line in the XY plane at the world origin (no plane
            // turn, no translation), so the profile's plane is normal to its start tangent.
            let path = profile_curves(k, std::slice::from_ref(path), ProfilePlane::Xy, [0.0; 3])?
                .into_iter()
                .next()
                .ok_or_else(|| Run::Error("sweep path has no curve".into()))?;
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            let options = SweepOptions {
                cap_ends: true,
                // `SMLIB_SWEEP_FOLLOW`: the profile turns with the path's frame.
                translational_sweep: false,
                move_profile_center_to_path: false,
                orient_profile_perpendicular_to_path: false,
                move_to_profile: false,
            };
            sweeps::create_curve_sweep(k, &refs, Some(&path), None, None, Some(&options))
                .map_err(|e| solid(&e))?
                .brep
        }
        // SM_API has no mirror and no matrix transform: SmBrep::Mirror and
        // SmBrep::Transform(double[4][4]) are SMLib-level and not ported. The nearest
        // SM_API route, SmApiScaleByPt with a reflecting scale, is refused for any body
        // with a circular edge (SmEllipse::Transform takes uniform scales only), and a
        // uniform -1 scale plus a half turn leaves bodies built by booleans inside out
        // (negative volume): SmBrep::Transform skips the FindAndSetInfiniteRegion and
        // UV-trim rebuild that SmBrep::Mirror does. Neither is run as a mirror.
        Step::Mirror { .. } => {
            return Err(Run::Deferred(Deferral {
                blocked_by: "no SM_API entry point".into(),
                symbol: "mirror of a body (SmBrep::Mirror is not in SM_API; SmApiScaleByPt \
                          with a reflecting scale fails or inverts boolean results)"
                    .into(),
            }))
        }
        // No Rust `SmApiFilletEdges` yet: it belongs to `blend.fillet_executive_p1`.
        Step::Fillet { .. } => {
            return Err(Run::Deferred(Deferral {
                blocked_by: "blend.fillet_executive_p1".into(),
                symbol: "SmApiFilletEdges".into(),
            }))
        }
        // SM_API offsets or shells whole bodies only (SmApiOffsetBrepFull,
        // SmApiShellBrepFull); nothing offsets selected faces.
        Step::OffsetFaces { .. } => {
            return Err(Run::Deferred(Deferral {
                blocked_by: "no SM_API entry point".into(),
                symbol: "offset of selected faces (SmApiOffsetBrepFull moves every face)".into(),
            }))
        }
        Step::SheetCorner { .. } => {
            return Err(Run::Deferred(Deferral {
                blocked_by: "no SM_API entry point".into(),
                symbol: "sheet flange corner joining and flat pattern (SolidCraft #3170)".into(),
            }))
        }
    };
    bodies.insert(step.id().to_string(), body);
    Ok(())
}

/// Makes the NURB of every analytic surface that the operation left without one (a
/// boolean result's swept faces carry an `SmSurfOfExtrusion` whose base has none).
/// Upstream's first `SmBSplineSurface::CalculateBoundingBox` makes it through the virtual
/// `GetOrCreateGwNurbPointer`; the port's bounding box is a `const` method and returns
/// `SM_ERR` instead, so the volume of such a body would fail here (recorded in
/// plan/findings/query.brep--boolean-results-need-their-analytic-nurbs.md).
fn make_missing_nurbs(k: &Kernel, b: &mut Brep) -> Result<(), SolidError> {
    for f in b.face_list().to_vec() {
        let Ok(face) = b.face_mut(f) else { continue };
        let Some(surface) = face.surface.as_mut() else {
            continue;
        };
        let missing = match surface.kind() {
            SurfaceKind::Plane(s) => s.base().gw_nurb().is_none(),
            SurfaceKind::Cone(s) => s.rev().base().gw_nurb().is_none(),
            SurfaceKind::Sphere(s) => s.base().base().gw_nurb().is_none(),
            SurfaceKind::Torus(s) => s.base().base().gw_nurb().is_none(),
            SurfaceKind::SurfOfRevolution(s) => s.base().gw_nurb().is_none(),
            SurfaceKind::SurfOfExtrusion(s) => s.base().gw_nurb().is_none(),
            SurfaceKind::BSpline(_)
            | SurfaceKind::CurveBounded(_)
            | SurfaceKind::Step(_)
            | SurfaceKind::OffsetSurface(_)
            | SurfaceKind::SrfInVolume(_) => false,
        };
        if !missing {
            continue;
        }
        match surface.kind_mut() {
            SurfaceKind::Plane(s) => s.make_nurb()?,
            SurfaceKind::Cone(s) => s.make_nurb(k)?,
            SurfaceKind::Sphere(s) => s.make_nurb(k)?,
            SurfaceKind::Torus(s) => s.make_nurb(k)?,
            SurfaceKind::SurfOfRevolution(s) => s.make_nurb(k)?,
            SurfaceKind::SurfOfExtrusion(s) => s.make_nurb(k)?,
            _ => {}
        }
    }
    Ok(())
}

fn measure(k: &Kernel, b: &mut Brep) -> Result<Measured, SolidError> {
    make_missing_nurbs(k, b)?;
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = brep_volume(k, b, VOLUME_ACCURACY, &attrs)?;
    let infinite = b.infinite_region();
    let lumps = regions(b, None)?
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count();
    let mut cylinder_faces = 0;
    for &f in b.face_list() {
        if let Some(SurfaceKind::Cone(c)) = b.face(f)?.surface.as_ref().map(Surface::kind) {
            if c.is_cylinder() {
                cylinder_faces += 1;
            }
        }
    }
    Ok(Measured {
        volume,
        lumps,
        topology: [
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len(),
        ],
        cylinder_faces,
    })
}

/// Turns on the `UpstreamFixes` field `name` a case asks for.
fn enable_fix(k: &mut Kernel, name: &str) -> Result<(), String> {
    let field = match name {
        "sweep_frame_on_planar_path_normal" => &mut k.fixes.sweep_frame_on_planar_path_normal,
        "cover_refuses_folded_continuation" => &mut k.fixes.cover_refuses_folded_continuation,
        "cone_parallel_line_midpoint" => &mut k.fixes.cone_parallel_line_midpoint,
        _ => return Err(format!("unknown `fixes` entry `{name}`")),
    };
    *field = true;
    Ok(())
}

/// Runs `case` on solidcore.
pub fn run_rust(case: &Case) -> Run {
    let mut k = Kernel::default();
    for fix in &case.fixes {
        if let Err(msg) = enable_fix(&mut k, fix) {
            return Run::Error(msg);
        }
    }
    let mut bodies = BTreeMap::new();
    for (n, step) in case.steps.iter().enumerate() {
        match step_rust(&mut k, &mut bodies, step) {
            Ok(()) => {}
            // Name the failing step (1-based) and the body it writes.
            Err(Run::Error(msg)) => {
                return Run::Error(format!("step {} (`{}`): {msg}", n + 1, step.id()))
            }
            Err(stop) => return stop,
        }
    }
    match bodies.get_mut(RESULT) {
        Some(b) => measure(&k, b).map_or_else(|e| solid(&e), Run::Done),
        None => Run::Error(format!("no body `{RESULT}`")),
    }
}

// ---- scoring --------------------------------------------------------------------------

/// One expectation checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    /// `lumps`, `volume`, `cylinder_faces_max`, `sheet_corner`.
    pub name: &'static str,
    /// `pass`, `fail` or `unchecked`.
    pub status: &'static str,
    /// Expected and measured, as text.
    pub detail: String,
}

/// Checks `m` against `e`.
pub fn checks(e: &Expect, m: &Measured) -> Vec<Check> {
    let mut out = Vec::new();
    let pass = |ok: bool| if ok { "pass" } else { "fail" };
    if let Some(n) = e.lumps {
        out.push(Check {
            name: "lumps",
            status: pass(m.lumps == n),
            detail: format!("expected {n}, got {}", m.lumps),
        });
    }
    if let Some(v) = &e.volume {
        let err = (m.volume - v.value).abs();
        out.push(Check {
            name: "volume",
            // A NaN volume fails: `err <= tol` is false.
            status: pass(err <= v.tol),
            detail: format!(
                "expected {} +- {}, got {} (off by {err})",
                v.value, v.tol, m.volume
            ),
        });
    }
    if let Some(n) = e.cylinder_faces_max {
        out.push(Check {
            name: "cylinder_faces_max",
            status: pass(m.cylinder_faces <= n),
            detail: format!("at most {n}, got {}", m.cylinder_faces),
        });
    }
    if e.sheet_corner.is_some() {
        out.push(Check {
            name: "sheet_corner",
            status: "unchecked",
            detail: "sheet-corner distances, validity, flat pattern and volume/area invariants need a sheet adapter".into(),
        });
    }
    out
}

/// A run's verdict and its checks.
pub fn verdict(e: &Expect, run: &Run) -> (&'static str, Vec<Check>) {
    match run {
        Run::Deferred(_) => ("DEFERRED", Vec::new()),
        Run::Error(_) => ("ERROR", Vec::new()),
        Run::Done(m) => {
            let c = checks(e, m);
            let v = if c.iter().any(|c| c.status == "fail") {
                "FAIL"
            } else if c.iter().any(|c| c.status == "unchecked") {
                "UNCHECKED"
            } else {
                "PASS"
            };
            (v, c)
        }
    }
}

/// Options of a `--solidcraft-cases` run.
#[derive(Debug, Clone, Default)]
pub struct CasesOptions {
    /// Case id glob (`<file stem>/<name>`; `*` and `?`).
    pub glob: Option<String>,
    /// Report directory (default `reports/`).
    pub report_dir: Option<PathBuf>,
    /// Do not write the report.
    pub no_report: bool,
}

fn run_entry(run: &Run, verdict: &str, checks: &[Check]) -> Value {
    let mut e = serde_json::Map::new();
    e.insert("verdict".into(), json!(verdict));
    match run {
        Run::Deferred(d) => {
            e.insert(
                "deferred".into(),
                json!({"blocked_by": d.blocked_by, "symbol": d.symbol}),
            );
        }
        Run::Error(msg) => {
            e.insert("error".into(), json!(msg));
        }
        Run::Done(m) => {
            e.insert(
                "measured".into(),
                json!({
                    "volume": crate::json::num(m.volume),
                    "lumps": m.lumps,
                    "faces": m.topology[0],
                    "edges": m.topology[1],
                    "vertices": m.topology[2],
                    "cylinder_faces": m.cylinder_faces,
                }),
            );
            e.insert(
                "checks".into(),
                Value::Array(
                    checks
                        .iter()
                        .map(|c| json!({"name": c.name, "status": c.status, "detail": c.detail}))
                        .collect(),
                ),
            );
        }
    }
    Value::Object(e)
}

/// Runs every case matching `opts.glob`, prints one line per case and the summary, and
/// writes `reports/solidcraft-cases.json` (`reports/solidcraft-cases_<glob>.json` for
/// a glob). Exit `0` whatever the verdicts; `2` for an unreadable case file or report.
pub fn run_cases(root: &Path, opts: &CasesOptions) -> ExitCode {
    let cases = match load(&cases_dir(root), opts.glob.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("difftest: {e}");
            return ExitCode::from(2);
        }
    };
    if cases.is_empty() {
        println!(
            "SOLIDCRAFT-CASES: no case matches `{}`",
            opts.glob.as_deref().unwrap_or("*")
        );
        return ExitCode::SUCCESS;
    }
    println!(
        "SOLIDCRAFT-CASES: {} case(s), Rust engine only",
        cases.len()
    );
    let mut counts: BTreeMap<&str, usize> = VERDICTS.iter().map(|v| (*v, 0)).collect();
    let mut blocked: BTreeMap<String, usize> = BTreeMap::new();
    let mut entries = Vec::new();
    for l in &cases {
        let run = run_rust(&l.case);
        let (v, c) = verdict(&l.case.expect, &run);
        *counts.entry(v).or_default() += 1;
        let detail = match &run {
            Run::Deferred(d) => {
                *blocked.entry(d.blocked_by.clone()).or_default() += 1;
                format!("{}: {}", d.blocked_by, d.symbol)
            }
            Run::Error(msg) => msg.clone(),
            Run::Done(m) => {
                let [f, e, v] = m.topology;
                let checked = c
                    .iter()
                    .map(|c| format!("{} {}: {}", c.name, c.status, c.detail))
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("{checked} [F/E/V {f}/{e}/{v}]")
            }
        };
        println!("{:<64} rust {v:<10} {detail}", l.id);
        entries.push(json!({
            "id": l.id,
            "issue": l.file.issue,
            "pr": l.file.pr,
            "test": l.file.test,
            "rust": run_entry(&run, v, &c),
        }));
    }
    let summary = VERDICTS
        .iter()
        .map(|v| format!("{v}={}", counts.get(v).copied().unwrap_or(0)))
        .collect::<Vec<_>>()
        .join(" ");
    println!("SOLIDCRAFT-CASES rust: {summary} (cases={})", cases.len());
    if !blocked.is_empty() {
        let by = blocked
            .iter()
            .map(|(u, n)| format!("{u}({n})"))
            .collect::<Vec<_>>()
            .join(",");
        println!("SOLIDCRAFT-CASES rust: blocked_by={by}");
    }

    if !opts.no_report {
        let report = json!({
            "glob": opts.glob,
            "volume_accuracy": VOLUME_ACCURACY,
            "summary": {"rust": counts, "blocked_by": blocked},
            "cases": entries,
        });
        let dir = opts
            .report_dir
            .clone()
            .unwrap_or_else(|| root.join("reports"));
        let mut name = "solidcraft-cases".to_string();
        if let Some(g) = &opts.glob {
            name.push('_');
            name.extend(g.chars().filter(|c| c.is_alphanumeric() || *c == '_'));
        }
        let path = dir.join(format!("{name}.json"));
        let written = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&path, crate::json::to_canonical_string(&report)));
        match written {
            Ok(()) => println!(
                "  report: {}",
                path.strip_prefix(root).unwrap_or(&path).display()
            ),
            Err(e) => {
                eprintln!("difftest: {}: {e}", path.display());
                return ExitCode::from(2);
            }
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Loaded> {
        load(&cases_dir(&crate::case::repo_root()), None).unwrap()
    }

    #[test]
    fn every_case_file_loads() {
        // No hard-coded total (every new case file would edit it and conflict): every
        // file in the directory is a case file, and every case of every file loads.
        let dir = cases_dir(&crate::case::repo_root());
        let mut expected = 0;
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            assert_eq!(
                p.extension().and_then(|x| x.to_str()),
                Some("json"),
                "{p:?}"
            );
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            let n = v["cases"].as_array().map_or(0, Vec::len);
            assert!(n > 0, "{p:?}: no cases");
            expected += n;
        }
        let cases = all();
        assert_eq!(
            cases.len(),
            expected,
            "{:?}",
            cases.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
        for l in &cases {
            assert!(
                l.id.starts_with("a52-")
                    || l.id.starts_with("catics-")
                    || (l.file.issue.contains("/issues/") && l.file.pr.contains("/pull/")),
                "{}",
                l.id
            );
            let e = &l.case.expect;
            assert!(
                e.lumps.is_some() || e.volume.is_some() || e.sheet_corner.is_some(),
                "{}: nothing to check",
                l.id
            );
        }
    }

    #[test]
    fn every_case_reaches_a_verdict_without_a_harness_error() {
        for l in all() {
            let run = run_rust(&l.case);
            assert!(
                !matches!(run, Run::Error(ref m) if m.starts_with("no body")),
                "{}: {run:?}",
                l.id
            );
            let (v, _) = verdict(&l.case.expect, &run);
            assert!(VERDICTS.contains(&v), "{}", l.id);
        }
    }

    #[test]
    fn fixes_are_enabled_by_name() {
        let mut k = Kernel::default();
        enable_fix(&mut k, "sweep_frame_on_planar_path_normal").unwrap();
        enable_fix(&mut k, "cover_refuses_folded_continuation").unwrap();
        assert!(k.fixes.sweep_frame_on_planar_path_normal);
        assert!(k.fixes.cover_refuses_folded_continuation);
        assert!(enable_fix(&mut k, "no_such_fix").is_err());
    }

    #[test]
    fn deferrals_name_a_plan_unit() {
        let units: Vec<String> = crate::plan::load(&crate::case::repo_root())
            .unwrap()
            .into_iter()
            .map(|u| u.id)
            .collect();
        for l in all() {
            if let Run::Deferred(d) = run_rust(&l.case) {
                assert!(
                    d.blocked_by == "no SM_API entry point" || units.contains(&d.blocked_by),
                    "{}: {d:?}",
                    l.id
                );
            }
        }
    }

    #[test]
    fn scoring() {
        let e = Expect {
            lumps: Some(2),
            volume: Some(VolumeExpect {
                value: 268.08,
                tol: 0.27,
                formula: String::new(),
            }),
            cylinder_faces_max: None,
            sheet_corner: None,
        };
        let ok = Measured {
            volume: 268.02,
            lumps: 2,
            topology: [4, 4, 4],
            cylinder_faces: 0,
        };
        assert_eq!(verdict(&e, &Run::Done(ok)).0, "PASS");
        // The bug: the whole intersect came out empty.
        let empty = Measured {
            volume: 0.0,
            lumps: 0,
            topology: [0, 0, 0],
            cylinder_faces: 0,
        };
        let (v, c) = verdict(&e, &Run::Done(empty));
        assert_eq!(v, "FAIL");
        assert!(c.iter().all(|c| c.status == "fail"));
        let nan = Measured {
            volume: f64::NAN,
            lumps: 2,
            ..ok
        };
        assert_eq!(verdict(&e, &Run::Done(nan)).0, "FAIL");
        let faces = Expect {
            cylinder_faces_max: Some(4),
            ..e.clone()
        };
        assert_eq!(verdict(&faces, &Run::Done(ok)).0, "PASS");
        let split = Measured {
            cylinder_faces: 5,
            ..ok
        };
        assert_eq!(verdict(&faces, &Run::Done(split)).0, "FAIL");
        assert_eq!(verdict(&faces, &Run::Done(empty)).0, "FAIL");
        let d = Run::Deferred(Deferral {
            blocked_by: "u".into(),
            symbol: "s".into(),
        });
        assert_eq!(verdict(&e, &d).0, "DEFERRED");
        assert_eq!(verdict(&e, &Run::Error("x".into())).0, "ERROR");
    }

    #[test]
    fn sheet_corner_preserves_the_original_matrix_and_defers_explicitly() {
        let cases = load(&cases_dir(&crate::case::repo_root()), Some("3170-*")).unwrap();
        assert_eq!(cases.len(), 16);
        let mut matrix = std::collections::BTreeSet::new();
        for loaded in cases {
            let [Step::SheetCorner {
                base_size,
                thickness,
                bend_radius,
                k_factor,
                height,
                angle_degrees,
                gap,
                relief_size,
                corner_relief,
                covering,
                overlap,
                ..
            }] = loaded.case.steps.as_slice()
            else {
                panic!(
                    "{}: expected the original sheet-corner operation",
                    loaded.id
                );
            };
            assert_eq!(*base_size, [120.0, 90.0]);
            assert_eq!((*thickness, *bend_radius, *k_factor), (2.0, 1.0, 0.5));
            assert_eq!((*height, *gap), (38.6, 0.5));
            assert_eq!((*relief_size, *corner_relief), ([2.0, 1.0], 8.0));
            assert!([90.0, 72.0, -90.0, -72.0].contains(angle_degrees));
            assert!([1.0, 2.0].contains(overlap));
            assert!(matrix.insert((*angle_degrees as i32, *overlap as i32, *covering)));
            let expected = loaded.case.expect.sheet_corner.as_ref().unwrap();
            let distances = if *covering == 0 {
                [-overlap, *gap]
            } else {
                [*gap, -overlap]
            };
            assert_eq!(expected.end_face_distances, distances);
            assert_eq!(expected.distance_tol, 1e-9);
            assert_eq!(expected.flat_regions, 1);
            assert_eq!(expected.volume_relative_tol, 1e-3);
            let run = run_rust(&loaded.case);
            let Run::Deferred(ref deferred) = run else {
                panic!(
                    "{}: missing sheet adapter must not pass: {run:?}",
                    loaded.id
                );
            };
            assert_eq!(deferred.blocked_by, "no SM_API entry point");
            assert!(deferred.symbol.contains("#3170"));
            assert_eq!(verdict(&loaded.case.expect, &run).0, "DEFERRED");
            assert_eq!(
                verdict(
                    &loaded.case.expect,
                    &Run::Done(Measured {
                        volume: 1.0,
                        lumps: 1,
                        topology: [1, 0, 0],
                        cylinder_faces: 0,
                    })
                )
                .0,
                "UNCHECKED"
            );
        }
    }

    #[test]
    fn sheet_corner_rejects_invalid_parameters_and_expectations() {
        let file: Value = serde_json::from_str(include_str!(
            "../../../fixtures/solidcraft/3170-sheet-corner-overlap.json"
        ))
        .unwrap();
        let original = &file["cases"][0];
        for (key, value) in [
            ("base_size", json!([120, 0])),
            ("thickness", json!(0)),
            ("bend_radius", json!(-1)),
            ("height", json!(0)),
            ("k_factor", json!(1.1)),
            ("angle_degrees", json!(0)),
            ("angle_degrees", json!(180)),
            ("gap", json!(-0.5)),
            ("relief_size", json!([2, 0])),
            ("corner_relief", json!(-8)),
            ("covering", json!(1)),
            ("overlap", json!(3)),
        ] {
            let mut changed = original.clone();
            changed["steps"][0][key] = value;
            let case: Case = serde_json::from_value(changed).unwrap();
            assert!(validate(&case).is_err(), "accepted invalid {key}");
        }
        for key in ["distance_tol", "volume_relative_tol", "flat_regions"] {
            let mut changed = original.clone();
            changed["expect"]["sheet_corner"][key] = json!(0);
            let case: Case = serde_json::from_value(changed).unwrap();
            assert!(validate(&case).is_err(), "accepted invalid {key}");
        }
        let mut case: Case = serde_json::from_value(original.clone()).unwrap();
        case.expect
            .sheet_corner
            .as_mut()
            .unwrap()
            .end_face_distances[0] = f64::NAN;
        assert!(validate(&case).is_err());
        let mut changed = original.clone();
        changed["steps"][0]["coverng"] = json!(0);
        assert!(serde_json::from_value::<Case>(changed).is_err());
    }

    #[test]
    fn validation() {
        let step = |json: &str| serde_json::from_str::<Step>(json).unwrap();
        let case = |steps: Vec<Step>| Case {
            name: "c".into(),
            note: None,
            fixes: Vec::new(),
            steps,
            expect: Expect::default(),
        };
        let bx = r#"{"op": "box", "id": "result", "origin": [0, 0, 0], "size": [1, 1, 1]}"#;
        assert!(validate(&case(vec![step(bx)])).is_ok());
        let late = r#"{"op": "union", "id": "result", "a": "x", "b": "x"}"#;
        assert!(validate(&case(vec![step(late)])).is_err());
        let other = r#"{"op": "box", "id": "other", "origin": [0, 0, 0], "size": [1, 1, 1]}"#;
        assert!(validate(&case(vec![step(other)])).is_err());
        assert!(serde_json::from_str::<Step>(
            r#"{"op": "box", "id": "a", "origin": [0, 0, 0], "size": [1, 1, 1], "typo": 1}"#
        )
        .is_err());
        assert!(serde_json::from_str::<Step>(r#"{"op": "chamfer", "id": "a"}"#).is_err());
        let ext = r#"{"op": "extrude", "id": "result", "distance": 1, "profile": [{"kind": "polygon", "points": [[0, 0], [1, 0], [1, 1]]}]}"#;
        assert!(validate(&case(vec![step(ext)])).is_ok());
        let flat = ext.replace("[[0, 0], [1, 0], [1, 1]]", "[[0, 0], [1, 0]]");
        assert!(validate(&case(vec![step(&flat)])).is_err());
        let empty = ext.replace(
            r#"[{"kind": "polygon", "points": [[0, 0], [1, 0], [1, 1]]}]"#,
            "[]",
        );
        assert!(validate(&case(vec![step(&empty)])).is_err());
        let mirror = r#"{"op": "mirror", "id": "result", "body": "nope", "normal": "y"}"#;
        assert!(validate(&case(vec![step(mirror)])).is_err());
        assert!(serde_json::from_str::<Step>(
            r#"{"op": "mirror", "id": "a", "body": "b", "normal": "w"}"#
        )
        .is_err());
        let pipe = r#"{"op": "pipe", "id": "result", "radius": 1, "path": [{"kind": "line", "from": [0, 0], "to": [5, 0]}]}"#;
        assert!(validate(&case(vec![step(pipe)])).is_ok());
        let no_path = pipe.replace(r#"[{"kind": "line", "from": [0, 0], "to": [5, 0]}]"#, "[]");
        assert!(validate(&case(vec![step(&no_path)])).is_err());
        let circle_path = pipe.replace(
            r#"{"kind": "line", "from": [0, 0], "to": [5, 0]}"#,
            r#"{"kind": "circle", "center": [0, 0], "radius": 5}"#,
        );
        assert!(validate(&case(vec![step(&circle_path)])).is_err());
        let loft = r#"{"op": "loft", "id": "result", "sections": [{"profile": [{"kind": "circle", "center": [0, 0], "radius": 2}]}, {"profile": [{"kind": "circle", "center": [0, 0], "radius": 1}], "origin": [0, 0, 5]}]}"#;
        assert!(validate(&case(vec![step(loft)])).is_ok());
        let Step::Loft { degree, .. } = step(loft) else {
            panic!("not a loft")
        };
        assert_eq!(degree, 1);
        let one = r#"{"op": "loft", "id": "result", "sections": [{"profile": [{"kind": "circle", "center": [0, 0], "radius": 2}]}]}"#;
        assert!(validate(&case(vec![step(one)])).is_err());
        assert!(serde_json::from_str::<Step>(&loft.replace("\"origin\"", "\"orign\"")).is_err());
    }
}
