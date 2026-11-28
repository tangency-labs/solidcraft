//! SolidCraft's kernel interface (`docs/design/kernel-layer.md`).
//!
//! SolidCraft asks a kernel for bodies through one trait, [`Backend`]. A kernel joins SolidCraft
//! through an adapter that implements it; `solidcraft-kernel` is the router that picks which
//! adapter runs each operation, and the rest of the application calls only the router.
//!
//! solidcore's adapter, over `solidcore-ops`, is the one SolidCraft maintains, and the default.
//! Which kernels a build holds is decided at compile time, by features of `solidcraft-kernel`.
//!
//! This crate holds the contract only: the trait, the operations it covers ([`OpKind`]), their
//! inputs, and the typed error every adapter maps its kernel's failures into ([`BackendError`]).
//! The trait comes in families, one module each ([`modelling`]): a family brings its trait over
//! [`Backend`], its inputs and its [`OpKind`]s, solidcore's adapter for it in
//! `crates/kernel/src/backend/<family>.rs`, and the router's entry points in
//! `crates/kernel/src/route/<family>.rs`.
//! It computes nothing. Bodies cross it as the record (`solidcore_record::brep::Brep`), the body
//! format, with the operation's history in the record's indices.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use solidcore_record::brep::Brep;
use solidcore_record::brep::history::History;

pub mod affine;
pub mod cell_grid;
pub mod curves;
pub mod develop;
pub mod dxf;
pub mod exact;
pub mod exchange;
pub mod frames;
pub mod hatch;
pub mod hole_stacks;
pub mod mesh;
pub mod modelling;
pub mod planar;
pub mod points;
pub mod profiles;
pub mod projection;
pub mod query;
pub mod runtime;
pub mod sections;
pub mod shaft_spline;
pub mod sweep;
pub mod tessellate;
pub mod tube_cope;

pub use affine::{Affine, AffineRefused, AxisRefusal, Matrix, OffsetQuad, SquareFaceLevel};
pub use cell_grid::{CellGrid, CellGridLimits};
pub use curves::{CurveRefused, CurveSample, Curves};
pub use dxf::Dxf;
pub use exact::{ExactAssembled, ExactBody, ExactBound, ExactEdge, ExactExported, ExactFace, ExactHistory, ExactKind, ExactUse};
pub use exchange::{CadirBodies, CadirFailure, CadirRead, CadirRefused, CadirWritten, Exchange};
pub use frames::{Frames, GussetFailure, MemberRefusal, PathFailure, PathRefusal, TrimRefusal, TrimTarget};
pub use hole_stacks::{HoleStacks, HoleStacksRefused};
pub use mesh::{MeshRebuilt, MeshRefused, Meshes, ReconstructionKind};
pub use modelling::{
    Axis2, BoolOp, ChamferSide, DraftCorner, ExtrudeSides, FilletTurn, Modelling, OneCurve, Primitive, RevolveSides, SplitTool, Taper, TrimKeep,
};
pub use planar::{Planar, PlanarRefused};
pub use points::{ClassifiedPoint, Distance, PointNormalFace, PointSetSummary, Points, PointsRefused, UnitNormal};
pub use profiles::{ProfileCurve, ProfileRefused, Profiles};
pub use projection::{ProjectionRefused, Projections};
pub use query::{FaceHit, MembersClosest, OutlineStretch, Queries, QueryRefused, SurfacePoint};
pub use runtime::{ErrorClass, FailureCode, Runtime};
pub use sections::{BlendFrame, FramesRefused, Sections, TurnRefusal};
pub use shaft_spline::ShaftSplines;
pub use sweep::{Helix, PathSpan, PointPath, PointPiece, RailFit, SpaceSpan, SweepOptions, SweepOrientation};
pub use tessellate::{PlanarFill, TessRefused, Tessellate};
pub use tube_cope::{CopeFailed, CopeRefusal, TemplateRefusal, TubeCopes};

/// A kernel's stable name: what build features, the support matrix and reports call it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BackendId(pub &'static str);

impl std::fmt::Display for BackendId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// The operations the router sends to a kernel, by family: the body-making calls ([`Modelling`]).
/// The services that read records (queries, meshing, the validity check, import and export)
/// join as families of their own (`docs/design/kernel-layer.md` §2); until then they run on
/// solidcore whichever kernel made the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    /// An axis-aligned box ([`Primitive::Box`]).
    Box,
    /// A cylinder ([`Primitive::Cylinder`]).
    Cylinder,
    /// A sphere ([`Primitive::Sphere`]).
    Sphere,
    /// A torus ([`Primitive::Torus`]).
    Torus,
    /// Planar regions extruded along their plane's normal ([`Modelling::extrude`]).
    Extrude,
    /// Planar regions turned about an axis ([`Modelling::revolve`]).
    Revolve,
    /// Union, cut or intersection of two bodies ([`Modelling::boolean`]).
    Boolean,
    /// Constant-radius rounds of edges ([`Modelling::fillet`]).
    Fillet,
    /// Equal and unequal chamfers of edges ([`Modelling::chamfer`], [`Modelling::chamfer_sides`]).
    Chamfer,
    /// A body hollowed to a wall ([`Modelling::shell`]).
    Shell,
    /// A sheet thickened into a solid ([`Modelling::thicken`]).
    Thicken,
    /// Faces moved along their normals ([`Modelling::offset_faces`]).
    OffsetFaces,
    /// One face retrimmed on its rigidly moved surface ([`Modelling::transform_face`]).
    TransformFace,
    /// Faces tilted about a neutral plane ([`Modelling::draft`]).
    Draft,
    /// Faces and bodies split by a tool, and surface bodies trimmed ([`Modelling::split_body`],
    /// [`Modelling::split_faces`], [`Modelling::trim_keep`]).
    Split,
    /// A profile swept along a path or a helix ([`Modelling::sweep`], [`Modelling::sweep_helix`]).
    Sweep,
    /// Sections lofted into a solid ([`Modelling::loft`]).
    Loft,
    /// A cell grid planned over a profile ([`CellGrid`]).
    CellGrid,
    /// The exchange helpers: a STEP string, CADIR read and written ([`Exchange`]).
    Exchange,
    /// Sheet development: a sheet metal part laid out flat and folded back
    /// ([`develop::Develop`]).
    Develop,
    /// Points and picks: distances, centroids, stored picks, flush contacts ([`Points`]).
    Points,
    /// Exact queries of a body's record ([`Queries`]).
    Query,
    /// Hole stacks through touching bodies ([`HoleStacks`]).
    HoleStacks,
    /// Planar helpers: sketch arcs, path turns, section corners, paper strokes ([`Planar`]).
    Planar,
    /// Hatching: line families clipped into marks, pen dots ([`hatch::Hatching`]).
    Hatch,
    /// Frame-stock paths, member trims and gusset plates ([`Frames`]).
    Frames,
    /// Planar projection: sketch curves between planes, edges read in a plane, outlines tidied
    /// ([`Projections`]).
    Projection,
    /// Placement maths: affine maps, axes and reaches ([`Affine`]).
    Affine,
    /// Curves queried and splines fitted ([`Curves`]).
    Curves,
    /// Source meshes rebuilt as solids ([`Meshes`]).
    Mesh,
    /// Round-tube copes and their templates ([`TubeCopes`]).
    TubeCope,
    /// Shaft splines planned, and round faces' seats and ends ([`ShaftSplines`]).
    ShaftSpline,
    /// DXF geometry: paths converted for a file, bounds, scales, entity checks ([`Dxf`]).
    Dxf,
    /// Blend sections: a section's frames from its points, which open sections to turn
    /// ([`Sections`]).
    Sections,
    /// Planar fills: loops and unions of regions filled with triangles ([`Tessellate`]).
    Tessellate,
    /// The kernel's runtime: stopping its work, the code of its last failed call ([`Runtime`]).
    Runtime,
    /// Bodies from profiles of curves in space: revolved, extruded, planar sheets, balls
    /// ([`Profiles`]).
    Profiles,
}

impl OpKind {
    /// Every operation, in the order above.
    pub const ALL: &[OpKind] = &[
        OpKind::Box,
        OpKind::Cylinder,
        OpKind::Sphere,
        OpKind::Torus,
        OpKind::Extrude,
        OpKind::Revolve,
        OpKind::Boolean,
        OpKind::Fillet,
        OpKind::Chamfer,
        OpKind::Shell,
        OpKind::Thicken,
        OpKind::OffsetFaces,
        OpKind::TransformFace,
        OpKind::Draft,
        OpKind::Split,
        OpKind::Sweep,
        OpKind::Loft,
        OpKind::CellGrid,
        OpKind::Exchange,
        OpKind::Develop,
        OpKind::Points,
        OpKind::Query,
        OpKind::HoleStacks,
        OpKind::Planar,
        OpKind::Hatch,
        OpKind::Frames,
        OpKind::Projection,
        OpKind::Affine,
        OpKind::Curves,
        OpKind::Mesh,
        OpKind::TubeCope,
        OpKind::ShaftSpline,
        OpKind::Dxf,
        OpKind::Sections,
        OpKind::Tessellate,
        OpKind::Runtime,
        OpKind::Profiles,
    ];

    /// The operation's stable name, as the support matrix and route files spell it.
    pub const fn name(self) -> &'static str {
        match self {
            OpKind::Box => "box",
            OpKind::Cylinder => "cylinder",
            OpKind::Sphere => "sphere",
            OpKind::Torus => "torus",
            OpKind::Extrude => "extrude",
            OpKind::Revolve => "revolve",
            OpKind::Boolean => "boolean",
            OpKind::Fillet => "fillet",
            OpKind::Chamfer => "chamfer",
            OpKind::Shell => "shell",
            OpKind::Thicken => "thicken",
            OpKind::OffsetFaces => "offset_faces",
            OpKind::TransformFace => "transform_face",
            OpKind::Draft => "draft",
            OpKind::Split => "split",
            OpKind::Sweep => "sweep",
            OpKind::Loft => "loft",
            OpKind::CellGrid => "cell_grid",
            OpKind::Exchange => "exchange",
            OpKind::Develop => "develop",
            OpKind::Points => "points",
            OpKind::Query => "query",
            OpKind::HoleStacks => "hole_stacks",
            OpKind::Planar => "planar",
            OpKind::Hatch => "hatch",
            OpKind::Frames => "frames",
            OpKind::Projection => "projection",
            OpKind::Affine => "affine",
            OpKind::Curves => "curves",
            OpKind::Mesh => "mesh",
            OpKind::TubeCope => "tube_cope",
            OpKind::ShaftSpline => "shaft_spline",
            OpKind::Dxf => "dxf",
            OpKind::Sections => "sections",
            OpKind::Tessellate => "tessellate",
            OpKind::Runtime => "runtime",
            OpKind::Profiles => "profiles",
        }
    }

    /// The operation a primitive is.
    pub const fn of_primitive(p: &Primitive) -> OpKind {
        match p {
            Primitive::Box { .. } => OpKind::Box,
            Primitive::Cylinder { .. } => OpKind::Cylinder,
            Primitive::Sphere { .. } => OpKind::Sphere,
            Primitive::Torus { .. } => OpKind::Torus,
        }
    }
}

/// Whether a kernel takes an operation, asked before any conversion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Support {
    /// The kernel takes the operation.
    Yes,
    /// The kernel doesn't take the operation; it stays on another kernel.
    No,
    /// Taken only in part; the reason says which part.
    Some(String),
}

/// A body as it leaves a kernel: its record in canonical order, and the operation's history over
/// its operands in the record's indices (`None` where the kernel can't report it exactly).
#[derive(Clone, Debug)]
pub struct Exported {
    /// The body's record, in canonical order.
    pub record: Brep,
    /// The operation's history over its operands, in the record's indices.
    pub history: Option<History>,
}

/// Why a kernel didn't make a body, in words every kernel's failures map into. The router turns
/// it into the application's error, so the user reads the same kind of message whichever kernel
/// ran.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum BackendError {
    /// The input is wrong whatever the kernel: not finite, out of range, an open profile, an
    /// index past its list.
    #[error("invalid input: {0}")]
    Invalid(String),
    /// This kernel doesn't build this (yet).
    #[error("not supported yet: {0}")]
    Unsupported(String),
    /// A size too large for the geometry around it, with the largest size that builds where the
    /// kernel names one.
    #[error("{why}")]
    TooLarge {
        /// What was too large, in words.
        why: String,
        /// The largest size that builds, where the kernel names one.
        largest: Option<f64>,
    },
    /// No room: an inward offset leaves none, walls touch, a wall folds.
    #[error("{why}")]
    NoRoom {
        /// Where there was no room, in words.
        why: String,
        /// The largest size that builds, where the kernel names one.
        largest: Option<f64>,
    },
    /// A numerical step failed: no convergence, a singular system.
    #[error("{0}")]
    Numerical(String),
    /// The kernel made something, and it fails the shared check.
    #[error("{0}")]
    InvalidResult(String),
    /// The operation failed for a reason the kernel doesn't sort into the kinds above.
    #[error("{0}")]
    Failed(String),
    /// Stopped by the cancel token.
    #[error("{0} cancelled")]
    Cancelled(String),
    /// The kernel panicked, and the panic was caught.
    #[error("{0}")]
    Internal(String),
    /// A record the kernel's own form can't hold, or a form that doesn't export as a record.
    #[error("{0}")]
    Exchange(String),
}

/// One kernel, as SolidCraft uses it, implemented by its adapter.
///
/// The router holds each compiled-in adapter in a closed enum, so a build with solidcore only
/// makes exactly the call it would make without the router.
///
/// A body is the kernel's own form ([`Backend::Form`]); it enters as the record
/// ([`Backend::import`]) and leaves as the record with its history ([`Backend::export`]). A
/// kernel that can't write the record's canonical order itself takes and gives bodies in memory
/// as exact bodies instead ([`Backend::import_exact`], [`Backend::export_exact`]), which solidcore
/// assembles into the record: no body crosses as a file.
///
/// What a kernel does is in families, one trait over this one for each, in its own module:
/// [`Modelling`], the body-making operations. A family names its operations in [`OpKind`], so
/// [`Backend::supports`] and the kernel support matrix (`docs/kernel-support.md`) list them.
pub trait Backend: Send + Sync + 'static {
    /// The kernel's own body.
    type Form: Send + Sync + 'static;
    /// The kernel's own error, sorted into [`BackendError`] for the application.
    type Error: std::error::Error + Into<BackendError>;

    /// The kernel's stable name.
    const ID: BackendId;

    /// The kernel's version, told apart from other builds of it.
    fn version(&self) -> &'static str;

    /// Whether this kernel takes `op` at all.
    fn supports(&self, op: OpKind) -> Support;

    /// The record as the kernel's form.
    fn import(&self, record: &Brep) -> Result<Self::Form, Self::Error>;
    /// The form as a record in canonical order, with the history that made it.
    fn export(&self, form: &Self::Form) -> Result<Exported, Self::Error>;

    /// Whether the router hands this kernel bodies as exact bodies ([`Backend::import_exact`])
    /// rather than as the record ([`Backend::import`]).
    fn takes_exact(&self) -> bool {
        false
    }
    /// The exact body as the kernel's form, for a kernel that takes bodies that way
    /// ([`Backend::takes_exact`]): `None` for one that takes the record.
    fn import_exact(&self, _body: &ExactBody) -> Option<Result<Self::Form, Self::Error>> {
        None
    }
    /// The form as an exact body with the history that made it, which the router assembles into
    /// the record: `None` for a kernel that gives the record itself ([`Backend::export`]).
    fn export_exact(&self, _form: &Self::Form) -> Option<Result<ExactExported, Self::Error>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;

    #[test]
    fn every_operation_has_its_own_name() {
        let mut names: Vec<&str> = OpKind::ALL.iter().map(|k| k.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OpKind::ALL.len());
    }

    #[test]
    fn operation_names_are_their_serde_names() {
        for &k in OpKind::ALL {
            assert_eq!(serde_json::to_string(&k).ok(), Some(format!("\"{}\"", k.name())));
        }
    }

    #[test]
    fn primitives_name_their_operation() {
        let o = Vec3::new(0.0, 0.0, 0.0);
        assert_eq!(OpKind::of_primitive(&Primitive::Box { a: o, b: o }), OpKind::Box);
        assert_eq!(OpKind::of_primitive(&Primitive::Sphere { center: o, radius: 1.0 }), OpKind::Sphere);
        assert_eq!(OpKind::of_primitive(&Primitive::Torus { center: o, major: 2.0, minor: 1.0 }), OpKind::Torus);
        assert_eq!(OpKind::of_primitive(&Primitive::Cylinder { base: o, axis: o, radius: 1.0, height: 1.0 }), OpKind::Cylinder);
    }

    #[test]
    fn bool_op_keeps_its_saved_names() {
        assert_eq!(serde_json::to_string(&BoolOp::Cut).ok().as_deref(), Some("\"cut\""));
        assert_eq!(serde_json::from_str::<BoolOp>("\"intersect\"").ok(), Some(BoolOp::Intersect));
    }
}
