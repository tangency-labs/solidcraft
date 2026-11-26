//! Frames: the record's frame data (`solidcore_record::frame`) and the kernel's frame values
//! (`solidcore::sweep::member`, `solidcore::prism::member`, `solidcore::prism::gusset`), both
//! ways, field for field. Rebuilding a checked kernel value from data (`from_parts`) is the
//! kernel's; these only copy.

use solidcore::curve::path::Segment as KSegment;
use solidcore::math::Transform;
use solidcore::prism::gusset as kg;
use solidcore::prism::member as kt;
use solidcore::sweep::member as km;
use solidcore_record::frame::{
    Axis, Cut, Dimensions, End, Extent, MemberData, PieceData, Placement, Segment, Side,
    SupportData, SupportFrames,
};

use crate::{plane, record_plane, record_region2, record_vec2, record_vec3, region2, vec3};

/// The record's stock segment as the kernel's path segment.
#[must_use]
pub fn segment(s: &Segment) -> KSegment {
    match *s {
        Segment::Line { a, b } => KSegment::Line {
            start: vec3(a),
            end: vec3(b),
        },
        Segment::Arc {
            a,
            center,
            axis,
            angle,
        } => KSegment::Arc {
            start: vec3(a),
            centre: vec3(center),
            axis: vec3(axis),
            angle,
        },
    }
}

/// A kernel path segment as the record's stock segment.
#[must_use]
pub fn record_segment(s: &KSegment) -> Segment {
    match *s {
        KSegment::Line { start, end } => Segment::Line {
            a: record_vec3(start),
            b: record_vec3(end),
        },
        KSegment::Arc {
            start,
            centre,
            axis,
            angle,
        } => Segment::Arc {
            a: record_vec3(start),
            center: record_vec3(centre),
            axis: record_vec3(axis),
            angle,
        },
    }
}

/// A kernel stock piece as the record's data.
#[must_use]
pub fn record_piece(p: &km::Piece) -> PieceData {
    PieceData {
        a: record_vec3(p.a),
        b: record_vec3(p.b),
        t0: record_vec3(p.t0),
        t1: record_vec3(p.t1),
        segment: record_segment(&p.segment),
    }
}

/// A kernel member as the record's data.
#[must_use]
pub fn record_member(m: &kt::Member) -> MemberData {
    MemberData {
        frame: record_plane(&m.frame),
        dir: record_vec3(m.dir),
        length: m.length,
        lo: record_vec2(m.lo),
        hi: record_vec2(m.hi),
    }
}

/// The record's end choice as the kernel's.
#[must_use]
pub fn end(e: End) -> kt::End {
    match e {
        End::Nearer => kt::End::Nearer,
        End::Start => kt::End::Start,
        End::End => kt::End::End,
    }
}

/// The record's side as the kernel's.
#[must_use]
pub fn side(s: Side) -> kt::Side {
    match s {
        Side::Right => kt::Side::Right,
        Side::Left => kt::Side::Left,
        Side::Top => kt::Side::Top,
        Side::Bottom => kt::Side::Bottom,
    }
}

/// A kernel middle line as the record's.
#[must_use]
pub fn record_axis(a: &kt::Axis) -> Axis {
    Axis {
        centre: record_vec3(a.centre),
        t0: a.t0,
        t1: a.t1,
        length: a.length,
    }
}

/// A kernel end cut as the record's.
#[must_use]
pub fn record_cut(c: &kt::Cut) -> Cut {
    Cut {
        origin: record_vec3(c.origin),
        normal: record_vec3(c.normal),
    }
}

/// A kernel extrusion range as the record's.
#[must_use]
pub fn record_extent(e: &kt::Extent) -> Extent {
    Extent {
        from: e.from,
        to: e.to,
        cut: e.cut,
    }
}

/// The record's gusset dimensions as the kernel's.
#[must_use]
pub fn dimensions(d: &Dimensions) -> kg::Dimensions {
    kg::Dimensions {
        leg1: d.leg1,
        leg2: d.leg2,
        thickness: d.thickness,
        relief: d.relief,
        clip: d.clip,
        offset: d.offset,
    }
}

/// The record's gusset placement as the kernel's.
#[must_use]
pub fn placement(p: Placement) -> kg::Placement {
    match p {
        Placement::Center => kg::Placement::Center,
        Placement::Positive => kg::Placement::Positive,
        Placement::Negative => kg::Placement::Negative,
    }
}

/// The record's support frames and material regions as the kernel's support face.
#[must_use]
pub fn support_face(frames: &SupportFrames, regions: &[solidcore_record::Region2]) -> kg::Face {
    kg::Face {
        plane: plane(&frames.plane),
        source: plane(&frames.source),
        regions: regions.iter().map(region2).collect(),
        to_source: Transform {
            m: frames.to_source,
        },
        to_author: Transform {
            m: frames.to_author,
        },
        tolerance: frames.tolerance,
    }
}

/// A kernel support face as the record's support data.
#[must_use]
pub fn record_support(f: &kg::Face) -> SupportData {
    SupportData {
        frames: SupportFrames {
            plane: record_plane(&f.plane),
            source: record_plane(&f.source),
            to_source: f.to_source.m,
            to_author: f.to_author.m,
            tolerance: f.tolerance,
        },
        regions: f.regions.iter().map(record_region2).collect(),
    }
}
