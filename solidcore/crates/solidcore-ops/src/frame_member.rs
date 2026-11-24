//! Frame-stock path preparation: exact curves, traversal, section planes and bend clearance.
//!
//! The kernel's [`solidcore::sweep::member`] computes every geometric quantity. This facade
//! converts the record, guards kernel calls and returns their values or typed refusals.

use solidcore::curve::path::Segment as KSegment;
use solidcore::sweep::member as km;
use solidcore_convert as convert;
use solidcore_record::{Plane, Region2, Seg2, Vec2, Vec3};

use crate::session::{run, Answer};
use crate::Error;

pub use km::{Refusal, VERTICAL_TOLERANCE};

/// A refusal of stock-path geometry, or a guarded kernel-call failure.
#[derive(Clone, Debug)]
pub enum Failure {
    /// The exact path cannot be used, with a path index when one is involved.
    Geometry(Refusal),
    /// The kernel call itself failed.
    Kernel(Error),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Geometry(e) => e.fmt(f),
            Self::Kernel(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Failure {}

struct Checked<T>(Result<T, Refusal>);

impl<T> Answer for Checked<T> {
    fn refusal(&self) -> Option<Error> {
        self.0.as_ref().err().cloned().map(Error::FrameMember)
    }
}

fn query<T>(f: impl FnOnce() -> Result<T, Refusal>) -> Result<T, Failure> {
    run("frame_member", |_| Ok(Checked(f())))
        .map_err(Failure::Kernel)?
        .0
        .map_err(Failure::Geometry)
}

/// An exact segment, as data for an application's existing sweep boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// A straight run between its two ends.
    Line {
        /// Start.
        a: Vec3,
        /// End.
        b: Vec3,
    },
    /// An open circular arc.
    Arc {
        /// Start.
        a: Vec3,
        /// Centre.
        center: Vec3,
        /// Unit normal of the positive turn.
        axis: Vec3,
        /// Positive turn, radians.
        angle: f64,
    },
}

/// The terminal and sweep data of one validated piece.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceData {
    /// Start.
    pub a: Vec3,
    /// End.
    pub b: Vec3,
    /// Unit start tangent.
    pub t0: Vec3,
    /// Unit end tangent.
    pub t1: Vec3,
    /// Its exact sweep segment.
    pub segment: Segment,
}

/// One validated stock-path piece. Construction and calculations stay in the kernel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece(km::Piece);

impl Piece {
    /// Its terminal and sweep data, field for field.
    #[must_use]
    pub fn data(&self) -> PieceData {
        let p = self.0;
        let segment = match p.segment {
            KSegment::Line { start, end } => Segment::Line {
                a: convert::record_vec3(start),
                b: convert::record_vec3(end),
            },
            KSegment::Arc {
                start,
                centre,
                axis,
                angle,
            } => Segment::Arc {
                a: convert::record_vec3(start),
                center: convert::record_vec3(centre),
                axis: convert::record_vec3(axis),
                angle,
            },
        };
        PieceData {
            a: convert::record_vec3(p.a),
            b: convert::record_vec3(p.b),
            t0: convert::record_vec3(p.t0),
            t1: convert::record_vec3(p.t1),
            segment,
        }
    }

    /// The line's length; `None` for a bend.
    ///
    /// # Errors
    /// A guarded kernel-call failure.
    pub fn line_length(&self) -> Result<Option<f64>, Failure> {
        query(|| Ok(self.0.line_length()))
    }
}

/// One finite straight stock segment.
///
/// # Errors
/// [`Refusal::Line`] or a guarded kernel-call failure.
pub fn line(a: Vec3, b: Vec3) -> Result<Piece, Failure> {
    query(|| km::Piece::line(convert::vec3(a), convert::vec3(b)).map(Piece))
}

/// One open spatial sketch arc, defined by its exact circle and terminal points.
///
/// # Errors
/// [`Refusal::Arc`] or a guarded kernel-call failure.
pub fn spatial_arc(
    a: Vec3,
    b: Vec3,
    center: Vec3,
    normal: Vec3,
    radius: f64,
) -> Result<Piece, Failure> {
    query(|| {
        km::Piece::spatial_arc(
            convert::vec3(a),
            convert::vec3(b),
            convert::vec3(center),
            convert::vec3(normal),
            radius,
        )
        .map(Piece)
    })
}

/// A planar sketch line or arc, placed in the component's frame.
///
/// # Errors
/// [`Refusal::Line`], [`Refusal::Arc`] or a guarded kernel-call failure.
pub fn planar(plane: &Plane, segment: &Seg2) -> Result<Piece, Failure> {
    query(|| km::Piece::planar(&convert::plane(plane), &convert::seg2(segment)).map(Piece))
}

/// A retained exact line over its parameter interval.
///
/// # Errors
/// [`Refusal::Line`] or a guarded kernel-call failure.
pub fn retained_line(start: Vec3, end: Vec3, interval: [f64; 2]) -> Result<Piece, Failure> {
    query(|| {
        km::Piece::retained_line(convert::vec3(start), convert::vec3(end), interval).map(Piece)
    })
}

/// Reverse curves as needed to traverse one open tangent chain in the supplied order.
/// The caller's array is changed only on success.
///
/// # Errors
/// The kernel's path refusal or a guarded kernel-call failure.
pub fn orient(pieces: &mut [Piece]) -> Result<(), Failure> {
    if pieces.is_empty() || pieces.len() > 1000 {
        return Err(Failure::Geometry(Refusal::Count));
    }
    let mut path: Vec<km::Piece> = pieces.iter().map(|p| p.0).collect();
    query(|| km::orient(&mut path))?;
    for (p, oriented) in pieces.iter_mut().zip(path) {
        p.0 = oriented;
    }
    Ok(())
}

/// Section axes with y as near world up as possible and the requested turn about the path.
///
/// # Errors
/// A guarded kernel-call failure. A degenerate direction has no axes.
pub fn member_axes(direction: Vec3, turn: f64) -> Result<Option<(Vec3, Vec3)>, Failure> {
    query(|| Ok(km::member_axes(convert::vec3(direction), turn).map(convert::record_pair)))
}

/// The starting section plane, including its anchor offset and turn.
///
/// # Errors
/// [`Refusal::Frame`] or a guarded kernel-call failure.
pub fn section_plane(first: &Piece, at: Vec2, turn: f64) -> Result<Plane, Failure> {
    query(|| {
        km::section_plane(&first.0, convert::vec2(at), turn).map(|f| convert::record_plane(&f))
    })
}

/// The section planes at both stock ends, carried through all bends. The final frame is
/// squared to the last tangent, including after compound spatial bends.
/// Both normals point along travel. Origins include the transported anchor offset: pass
/// `Vec2::ZERO` to obtain planes centred on the path endpoints.
///
/// # Errors
/// [`Refusal::Count`], [`Refusal::Frame`] or a guarded kernel-call failure.
pub fn terminal_planes(path: &[Piece], at: Vec2, turn: f64) -> Result<[Plane; 2], Failure> {
    if path.is_empty() || path.len() > 1000 {
        return Err(Failure::Geometry(Refusal::Count));
    }
    let path: Vec<km::Piece> = path.iter().map(|p| p.0).collect();
    query(|| {
        km::terminal_planes(&path, convert::vec2(at), turn)
            .map(|f| f.map(|f| convert::record_plane(&f)))
    })
}

/// Check section clearance at every bend using its exact control-hull enclosure.
///
/// # Errors
/// The kernel's clearance refusal or a guarded kernel-call failure.
pub fn check_bend_clearance(
    path: &[Piece],
    region: &Region2,
    at: Vec2,
    turn: f64,
) -> Result<(), Failure> {
    if path.is_empty() || path.len() > 1000 {
        return Err(Failure::Geometry(Refusal::Count));
    }
    let path: Vec<km::Piece> = path.iter().map(|p| p.0).collect();
    query(|| km::check_bend_clearance(&path, &convert::region2(region), convert::vec2(at), turn))
}
