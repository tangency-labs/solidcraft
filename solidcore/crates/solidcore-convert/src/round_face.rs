//! The round-face data of the record and the kernel's (`solidcore::teeth::round_face`), field for
//! field: a seat goes to the kernel, the readings come back, and a body record's faces and edges go
//! to the kernel as the numbers its band reading takes.

use solidcore::teeth::round_face as k;
use solidcore::teeth::seat as kseat;
use solidcore_record::brep::{Brep, Curve, Loop, Surface};
use solidcore_record::round_face::{End, Extent, GrowBlock, RoundFaceRefusal};
use solidcore_record::shaft_spline::SeatFrame;

use crate::{frame, plane, record_plane, vec3};

/// A record seat as the kernel's.
#[must_use]
pub fn seat_frame(s: &SeatFrame) -> kseat::SeatFrame {
    kseat::SeatFrame {
        plane: plane(&s.plane),
        inward: s.inward,
        room: s.room,
        at: vec3(s.at),
    }
}

/// A kernel extent as the record's.
#[must_use]
pub fn record_extent(e: &k::Extent) -> Extent {
    Extent {
        low: e.low,
        high: e.high,
        across: e.across,
    }
}

/// A record end as the kernel's.
#[must_use]
pub fn end(e: &End) -> k::End {
    k::End {
        free: e.free,
        open: e.open,
        out: e.out,
        through: e.through,
        step: e.step,
        neck: e.neck,
        end: e.end,
        rim: e.rim,
        cone: e.cone,
    }
}

/// A kernel end as the record's.
#[must_use]
pub fn record_end(e: &k::End) -> End {
    End {
        free: e.free,
        open: e.open,
        out: e.out,
        through: e.through,
        step: e.step,
        neck: e.neck,
        end: e.end,
        rim: e.rim,
        cone: e.cone,
    }
}

/// A kernel grow block as the record's.
#[must_use]
pub fn record_grow_block(b: &k::GrowBlock) -> GrowBlock {
    GrowBlock {
        plane: record_plane(&b.plane),
        radius: b.radius,
        from: b.from,
        to: b.to,
    }
}

/// A kernel refusal as the record's.
#[must_use]
pub fn record_refusal(r: k::RoundFaceRefusal) -> RoundFaceRefusal {
    match r {
        k::RoundFaceRefusal::Number => RoundFaceRefusal::Number,
        k::RoundFaceRefusal::Seat => RoundFaceRefusal::Seat,
        k::RoundFaceRefusal::Extent => RoundFaceRefusal::Extent,
        k::RoundFaceRefusal::Probe => RoundFaceRefusal::Probe,
        k::RoundFaceRefusal::NoBand => RoundFaceRefusal::NoBand,
        k::RoundFaceRefusal::Faces => RoundFaceRefusal::Faces,
    }
}

/// Edge `index` of `record` as the band reading takes it: its circle where its curve is one, a
/// straight line where it is one, and its two ends where its vertices are found. An edge the
/// record does not have comes as one with nothing known of it. No points along it: those are
/// the body's to give.
#[must_use]
pub fn band_edge(record: &Brep, index: usize) -> k::BandEdge {
    let Some(edge) = record.edges.get(index) else {
        return k::BandEdge {
            index,
            ..k::BandEdge::default()
        };
    };
    let point = |v: u32| {
        let v = usize::try_from(v).ok()?;
        record
            .vertices
            .get(v)
            .map(|v| solidcore::Vec3::from_array(v.point))
    };
    let [a, b] = edge.vertices;
    k::BandEdge {
        index,
        circle: match &edge.curve {
            Curve::Circle { frame: f, radius } => Some(k::Round {
                frame: frame(f),
                radius: *radius,
            }),
            _ => None,
        },
        line: matches!(edge.curve, Curve::Line { .. }),
        ends: point(a).zip(point(b)).map(|(a, b)| [a, b]),
        points: Vec::new(),
    }
}

/// Every face of `record` as the band reading takes it: its cylinder where its surface is one,
/// and every edge of its loops ([`band_edge`]).
#[must_use]
pub fn band_faces(record: &Brep) -> Vec<k::BandFace> {
    record
        .faces
        .iter()
        .map(|f| k::BandFace {
            cylinder: match &f.surface {
                Surface::Cylinder { frame: c, radius } => Some(k::Round {
                    frame: frame(c),
                    radius: *radius,
                }),
                _ => None,
            },
            edges: f
                .loops
                .iter()
                .flat_map(|l| match l {
                    Loop::Edges { edges } => edges.iter().map(|u| u.edge).collect::<Vec<u32>>(),
                    Loop::Vertex { .. } => Vec::new(),
                })
                .map(|e| band_edge(record, usize::try_from(e).unwrap_or(usize::MAX)))
                .collect(),
        })
        .collect()
}
