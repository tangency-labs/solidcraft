//! The room a round's band has along a straight edge between two planar faces (the kernel's
//! `blend::band_room`), for an application's taper (a variable radius following the room), and
//! the kernel's round refusals ([`solidcore::blend::round_room::RoundRefusal`]) as [`Error`]s.

use solidcore::blend::band_room::{self as kernel, band_room as kernel_band_room};
use solidcore::blend::round_room::RoundRefusal;
use solidcore::{Brep, SolidError};

use crate::error::status;
use crate::session::measure_body;
use crate::{Body, Error, Result};

/// A room as a radius: every radius at most `lower` is proved to have its band inside both
/// faces, none above `upper` has (`upper` infinite where no boundary is proved to close it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    /// The largest radius proved to fit.
    pub lower: f64,
    /// The radius past which none fits.
    pub upper: f64,
}

/// The room at an end of the edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EndRoom {
    /// Both faces' sides are square to the edge there: the room their reach gives.
    Square(Room),
    /// A face closes to a point there: no room.
    ClosesToPoint,
    /// The kernel's bounds do not decide whether a side is square there.
    Undecided,
}

/// The room along an edge ([`band_room`]), from the start the call named.
#[derive(Clone, Debug, PartialEq)]
pub struct BandRoom {
    /// The room at the start.
    pub at_start: EndRoom,
    /// The room at the far end.
    pub at_end: EndRoom,
    /// The room at each station asked.
    pub stations: Vec<Room>,
}

fn room(r: kernel::Room) -> Room {
    Room {
        lower: r.lower,
        upper: r.upper,
    }
}

fn end_room(r: kernel::EndRoom) -> EndRoom {
    match r {
        kernel::EndRoom::Square(r) => EndRoom::Square(room(r)),
        kernel::EndRoom::ClosesToPoint => EndRoom::ClosesToPoint,
        kernel::EndRoom::Undecided => EndRoom::Undecided,
    }
}

/// The kernel's round refusal in `e` as an [`Error`], its vertex as `b`'s record index and its
/// station measured from the edge's end when `reversed`; any other kernel error by its type.
pub(crate) fn refusal(b: &Brep, e: SolidError, reversed: bool) -> Error {
    let SolidError::RoundRefused { refusal, .. } = e else {
        return status(e);
    };
    match refusal {
        RoundRefusal::RoundsCross { at, fit } => Error::RoundsCross {
            at: at.iter().map(|p| [p.x, p.y, p.z]).collect(),
            fit,
        },
        RoundRefusal::TaperEndUnsupported { vertex, lean } => {
            match b.vertex_list().iter().position(|&v| v == vertex) {
                Some(vertex) => Error::TaperEndUnsupported { vertex, lean },
                None => Error::Exception,
            }
        }
        RoundRefusal::FacesCloseAtBothEnds { .. } => Error::FacesCloseAtBothEnds,
        #[allow(
            clippy::float_arithmetic,
            reason = "the parameter on the reversed edge, 1 - u: a choice of parameter, not a geometric quantity"
        )]
        RoundRefusal::RoomRunsOut { at, largest, .. } => Error::RoomRunsOut {
            at: if reversed { 1.0 - at } else { at },
            largest,
        },
        _ => Error::Exception,
    }
}

/// Whether `start` (a vertex) is the end vertex of edge `e` of `b`: `None` when it is neither
/// end.
pub(crate) fn runs_back(
    b: &Brep,
    e: solidcore::topology::EdgeId,
    v: solidcore::topology::VertexId,
) -> solidcore::Result<Option<bool>> {
    Ok(match solidcore::topology::edge::start_end_vertices(b, e)? {
        (Some(s), _) if s == v => Some(false),
        (_, Some(t)) if t == v => Some(true),
        _ => None,
    })
}

/// The room a round's band has along the straight edge of `body` at record index `edge`
/// between two planar faces: at its end at vertex `start`, at its other end, and at each of
/// `stations` (fractions of the way from `start`, strictly between 0 and 1), each certified by
/// the kernel.
///
/// # Errors
/// [`Error::Argument`] for an index past its list, a `start` that is not a vertex of the edge,
/// or a station not strictly inside (0, 1); [`Error::TaperEndUnsupported`] for an end where a
/// face's side leans away from the edge or is not straight; [`Error::FacesCloseAtBothEnds`];
/// [`Error::Unsupported`] for an edge that is not straight between two planar faces it bounds
/// once each, or whose faces are tangent; the kernel's other errors.
pub fn band_room(body: &Body, edge: usize, start: usize, stations: &[f64]) -> Result<BandRoom> {
    if !stations.iter().all(|&u| u > 0.0 && u < 1.0) {
        return Err(Error::Argument);
    }
    measure_body("band_room", body, |k, b| {
        let (Some(&e), Some(&v)) = (b.edge_list().get(edge), b.vertex_list().get(start)) else {
            return Ok(Err(Error::Argument));
        };
        let Some(reversed) = runs_back(b, e, v)? else {
            return Ok(Err(Error::Argument));
        };
        #[allow(
            clippy::float_arithmetic,
            reason = "the parameter on the reversed edge, 1 - u: a choice of parameter, not a geometric quantity"
        )]
        let asked: Vec<f64> = stations
            .iter()
            .map(|&u| if reversed { 1.0 - u } else { u })
            .collect();
        let r = match kernel_band_room(k, b, e, &asked) {
            Ok(r) => r,
            Err(e @ SolidError::RoundRefused { .. }) => return Ok(Err(refusal(b, e, reversed))),
            Err(e) => return Err(e),
        };
        let (at_start, at_end) = if reversed {
            (r.at_end, r.at_start)
        } else {
            (r.at_start, r.at_end)
        };
        Ok(Ok(BandRoom {
            at_start: end_room(at_start),
            at_end: end_room(at_end),
            stations: r.stations.into_iter().map(room).collect(),
        }))
    })?
}
