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
//! It computes nothing. Bodies cross it as the record (`solidcore_record::brep::Brep`), the body
//! format, with the operation's history in the record's indices.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use solidcore_record::brep::Brep;
use solidcore_record::brep::history::History;
use solidcore_record::{Plane, Region2, Vec2, Vec3};

/// A kernel's stable name: what build features, the support matrix and reports call it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BackendId(pub &'static str);

impl std::fmt::Display for BackendId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// The operations the router sends to a kernel, one per family of body-making calls. Queries,
/// meshing, the validity check and import and export are not here: they run on the record, the
/// same code whichever kernel made the body.
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
    /// Planar regions extruded along their plane's normal ([`Backend::extrude`]).
    Extrude,
    /// Planar regions turned about an axis ([`Backend::revolve`]).
    Revolve,
    /// Union, cut or intersection of two bodies ([`Backend::boolean`]).
    Boolean,
    /// Constant-radius rounds of edges ([`Backend::fillet`]).
    Fillet,
    /// Equal and unequal chamfers of edges ([`Backend::chamfer`], [`Backend::chamfer_sides`]).
    Chamfer,
    /// A body hollowed to a wall ([`Backend::shell`]).
    Shell,
    /// A sheet thickened into a solid ([`Backend::thicken`]).
    Thicken,
    /// Faces moved along their normals ([`Backend::offset_faces`]).
    OffsetFaces,
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

/// Which boolean: the names are saved in designs, so they never change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoolOp {
    /// Material in either body.
    Union,
    /// The first body with the second taken away.
    Cut,
    /// Material in both bodies.
    Intersect,
}

/// How two rounds turn where they meet at a sharp side edge on a face they share, such as the
/// rounds along a boss's base at its upright edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilletTurn {
    /// The rounds run on to where they cross and meet in a mitre.
    #[default]
    Mitre,
    /// The rounds end at the side edge and the corner is their section revolved about it, a
    /// torus tangent to both: the rolling ball kept on the side edge. Refused where it is not
    /// built, with the reason: a side edge of the rounds' own convexity (the ball cannot pass
    /// it), unequal or variable rounds, a curved or tangent side edge, a corner of four edges.
    Swept,
    /// Each turn swept where it is built, as [`FilletTurn::Swept`], and mitred where it is not:
    /// decided per corner before anything is built, so never refused where a mitre is made.
    Auto,
}

/// The second side of an unequal chamfer: a distance along the other face, or the angle the
/// chamfer makes with the first face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferSide {
    /// The setback along the other face, in millimetres.
    Distance(f64),
    /// The angle the chamfer makes with the first face, in radians.
    Angle(f64),
}

/// A primitive solid, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Primitive {
    /// The axis-aligned box between two opposite corners.
    Box {
        /// One corner.
        a: Vec3,
        /// The opposite corner.
        b: Vec3,
    },
    /// A cylinder standing on `base`, along `axis`, `height` long.
    Cylinder {
        /// The centre of its base.
        base: Vec3,
        /// The direction it stands along.
        axis: Vec3,
        /// Its radius.
        radius: f64,
        /// Its length along `axis`.
        height: f64,
    },
    /// A sphere about `center`.
    Sphere {
        /// Its centre.
        center: Vec3,
        /// Its radius.
        radius: f64,
    },
    /// A ring torus about the z axis through `center`.
    Torus {
        /// Its centre.
        center: Vec3,
        /// The radius of the ring's centre line.
        major: f64,
        /// The radius of the ring's section.
        minor: f64,
    },
}

/// The axis a profile turns about, in the profile plane's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis2 {
    /// A point on the axis.
    pub origin: Vec2,
    /// The axis's direction.
    pub dir: Vec2,
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
/// Edges and faces are picked by points, as the application picks them: the edge or face nearest
/// each point. A body is the kernel's own form ([`Backend::Form`]); it enters as the record
/// ([`Backend::import`]) and leaves as the record with its history ([`Backend::export`]).
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

    /// A primitive solid.
    fn primitive(&self, p: &Primitive) -> Result<Self::Form, Self::Error>;
    /// `regions` on `plane`, extruded from `start` to `end` along its normal: one body per region
    /// that stays apart.
    fn extrude(&self, plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Self::Form>, Self::Error>;
    /// `regions` on `plane`, turned by `angle` (radians) about `axis`. `whole` asks for the whole
    /// revolution's form where the angle is a full turn.
    fn revolve(&self, plane: &Plane, regions: &[Region2], axis: Axis2, angle: f64, whole: bool) -> Result<Vec<Self::Form>, Self::Error>;
    /// `a` with `b` by `op`. `None` when nothing is left.
    fn boolean(&self, a: &Self::Form, b: &Self::Form, op: BoolOp) -> Result<Option<Self::Form>, Self::Error>;
    /// Constant-radius rounds of the edges nearest `edges`, turning at side edges as `turn` says.
    fn fillet(&self, body: &Self::Form, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<Self::Form, Self::Error>;
    /// An equal chamfer of the edges nearest `edges`.
    fn chamfer(&self, body: &Self::Form, edges: &[Vec3], distance: f64) -> Result<Self::Form, Self::Error>;
    /// An unequal chamfer: `d1` along the first face (the one facing up most, the other with
    /// `flip`), the second side by distance or angle.
    fn chamfer_sides(&self, body: &Self::Form, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Self::Form, Self::Error>;
    /// The body hollowed to `thickness`, open at the faces nearest `open`.
    fn shell(&self, body: &Self::Form, open: &[Vec3], thickness: f64) -> Result<Self::Form, Self::Error>;
    /// A sheet thickened to `thickness`, to both sides when `symmetric`.
    fn thicken(&self, sheet: &Self::Form, thickness: f64, symmetric: bool) -> Result<Self::Form, Self::Error>;
    /// The faces nearest `at` moved `distance` along their normals.
    fn offset_faces(&self, body: &Self::Form, at: &[Vec3], distance: f64) -> Result<Self::Form, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

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
