//! Explicit conversions between runtime metadata and the versioned wire records.
use super::{Result, invalid};
use crate::{Body, FaceSource, history as h};
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3, brep::envelope as w};

fn index(n: usize) -> Result<u32> {
    u32::try_from(n).map_err(invalid)
}
fn point(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn point2(p: Vec2) -> [f64; 2] {
    [p.x, p.y]
}
fn p([x, y, z]: [f64; 3]) -> Vec3 {
    Vec3::new(x, y, z)
}
fn p2([x, y]: [f64; 2]) -> Vec2 {
    Vec2::new(x, y)
}
fn plane(p: Plane) -> w::Plane {
    w::Plane { origin: point(p.origin), x_axis: point(p.x), y_axis: point(p.y) }
}
fn load_plane(v: w::Plane) -> Plane {
    Plane { origin: p(v.origin), x: p(v.x_axis), y: p(v.y_axis) }
}
fn segment(s: &Seg2) -> Result<w::Segment> {
    Ok(match *s {
        Seg2::Line { a, b } => w::Segment::Line { start: point2(a), end: point2(b) },
        Seg2::Arc { center, radius, start, sweep } => w::Segment::Arc { center: point2(center), radius, start, sweep },
        Seg2::Cubic { p0, p1, p2, p3 } => w::Segment::Cubic { points: [p0, p1, p2, p3].map(point2) },
        Seg2::Conic { a, apex, b, w } => w::Segment::Conic { points: [a, apex, b].map(point2), middle_weight: w },
        Seg2::Bezier { n, p } => {
            let count = usize::from(n) + 1;
            if !(5..=solidcraft_geom::MAX_BEZIER_CTRL).contains(&count) || p.get(count..).is_none_or(|unused| unused.iter().any(|p| *p != Vec2::ZERO))
            {
                return Err(invalid("unsupported certificate Bezier layout"));
            }
            w::Segment::Bezier { points: p.get(..count).ok_or_else(|| invalid("Bezier count"))?.iter().copied().map(point2).collect() }
        }
    })
}
fn load_segment(s: w::Segment) -> Result<Seg2> {
    Ok(match s {
        w::Segment::Line { start, end } => Seg2::Line { a: p2(start), b: p2(end) },
        w::Segment::Arc { center, radius, start, sweep } => Seg2::Arc { center: p2(center), radius, start, sweep },
        w::Segment::Cubic { points: [a, b, c, d] } => Seg2::Cubic { p0: p2(a), p1: p2(b), p2: p2(c), p3: p2(d) },
        w::Segment::Conic { points: [a, apex, b], middle_weight } => Seg2::Conic { a: p2(a), apex: p2(apex), b: p2(b), w: middle_weight },
        w::Segment::Bezier { points } => {
            if !(5..=solidcraft_geom::MAX_BEZIER_CTRL).contains(&points.len()) {
                return Err(invalid("certificate Bezier degree"));
            }
            let mut p = [Vec2::ZERO; solidcraft_geom::MAX_BEZIER_CTRL];
            let n = (points.len() - 1) as u8;
            for (slot, value) in p.iter_mut().zip(points) {
                *slot = p2(value);
            }
            Seg2::Bezier { n, p }
        }
    })
}
fn region(r: &Region2) -> Result<w::Region> {
    Ok(w::Region {
        outer: r.outer.segs.iter().map(segment).collect::<Result<_>>()?,
        holes: r.holes.iter().map(|l| l.segs.iter().map(segment).collect()).collect::<Result<_>>()?,
    })
}
fn load_region(r: w::Region) -> Result<Region2> {
    let loop_ = |s: Vec<w::Segment>| Ok(Loop2 { segs: s.into_iter().map(load_segment).collect::<Result<_>>()? });
    Ok(Region2 { outer: loop_(r.outer)?, holes: r.holes.into_iter().map(loop_).collect::<Result<_>>()? })
}
fn source(s: &FaceSource, depth: usize) -> Result<w::FaceSource> {
    if depth > 32 {
        return Err(invalid("face-source nesting"));
    }
    Ok(match s {
        FaceSource::ExtrudeSide { segment: s, plane: p, coalesced } => {
            w::FaceSource::ExtrusionSide { segment: segment(s)?, plane: p.map(plane), coalesced: *coalesced }
        }
        FaceSource::CertifiedExtrudeSide { producer, sketch, curve, plane: p } => {
            w::FaceSource::CertifiedExtrusionSide { producer: *producer, sketch: *sketch, curve: curve.clone(), plane: p.map(plane) }
        }
        FaceSource::ExtrudeCap { high, region: r, plane: p } => w::FaceSource::ExtrusionCap { high: *high, region: region(r)?, plane: plane(*p) },
        FaceSource::CertifiedExtrudeCap { producer, sketch, high, outer, holes, plane: p } => w::FaceSource::CertifiedExtrusionCap {
            producer: *producer,
            sketch: *sketch,
            high: *high,
            outer: outer.clone(),
            holes: holes.clone(),
            plane: plane(*p),
        },
        FaceSource::CylinderCap { producer, high, plane: p } => w::FaceSource::CylinderCap { producer: *producer, high: *high, plane: plane(*p) },
        FaceSource::Box { producer, axis, high, plane: p } => {
            w::FaceSource::BoxFace { producer: *producer, axis: *axis, high: *high, plane: plane(*p) }
        }
        FaceSource::ShellPlanar { producer, source: s, inner, plane: p } => {
            w::FaceSource::PlanarShell { producer: *producer, source: Box::new(source(s, depth + 1)?), inner: *inner, plane: plane(*p) }
        }
        FaceSource::Imported { item, face, payload_sha256, producer } => {
            w::FaceSource::Imported { item: *item, face: *face, payload_sha256: payload_sha256.clone(), producer: *producer }
        }
    })
}
fn load_source(s: w::FaceSource, depth: usize) -> Result<FaceSource> {
    if depth > 32 {
        return Err(invalid("face-source nesting"));
    }
    Ok(match s {
        w::FaceSource::ExtrusionSide { segment, plane, coalesced } => {
            FaceSource::ExtrudeSide { segment: load_segment(segment)?, plane: plane.map(load_plane), coalesced }
        }
        w::FaceSource::CertifiedExtrusionSide { producer, sketch, curve, plane } => {
            FaceSource::CertifiedExtrudeSide { producer, sketch, curve, plane: plane.map(load_plane) }
        }
        w::FaceSource::ExtrusionCap { high, region, plane } => {
            FaceSource::ExtrudeCap { high, region: load_region(region)?, plane: load_plane(plane) }
        }
        w::FaceSource::CertifiedExtrusionCap { producer, sketch, high, outer, holes, plane } => {
            FaceSource::CertifiedExtrudeCap { producer, sketch, high, outer, holes, plane: load_plane(plane) }
        }
        w::FaceSource::CylinderCap { producer, high, plane } => FaceSource::CylinderCap { producer, high, plane: load_plane(plane) },
        w::FaceSource::BoxFace { producer, axis, high, plane } => FaceSource::Box { producer, axis, high, plane: load_plane(plane) },
        w::FaceSource::PlanarShell { producer, source, inner, plane } => {
            FaceSource::ShellPlanar { producer, source: Box::new(load_source(*source, depth + 1)?), inner, plane: load_plane(plane) }
        }
        w::FaceSource::Imported { item, face, payload_sha256, producer } => FaceSource::Imported { item, face, payload_sha256, producer },
    })
}
fn origin(o: h::FaceOrigin) -> Result<w::FaceOrigin> {
    Ok(match o {
        h::FaceOrigin::Face { operand, face } => w::FaceOrigin::Face { operand: index(operand)?, face: index(face)? },
        h::FaceOrigin::Profile { segment } => w::FaceOrigin::Profile { segment: index(segment)? },
        h::FaceOrigin::Cap { end } => w::FaceOrigin::Cap { end },
        h::FaceOrigin::Edge { operand, edge } => w::FaceOrigin::Edge { operand: index(operand)?, edge: index(edge)? },
        h::FaceOrigin::Offset { operand, face } => w::FaceOrigin::Offset { operand: index(operand)?, face: index(face)? },
    })
}
fn load_origin(o: w::FaceOrigin) -> h::FaceOrigin {
    match o {
        w::FaceOrigin::Face { operand, face } => h::FaceOrigin::Face { operand: operand as usize, face: face as usize },
        w::FaceOrigin::Profile { segment } => h::FaceOrigin::Profile { segment: segment as usize },
        w::FaceOrigin::Cap { end } => h::FaceOrigin::Cap { end },
        w::FaceOrigin::Edge { operand, edge } => h::FaceOrigin::Edge { operand: operand as usize, edge: edge as usize },
        w::FaceOrigin::Offset { operand, face } => h::FaceOrigin::Offset { operand: operand as usize, face: face as usize },
    }
}
fn change(c: h::Change) -> w::Change {
    match c {
        h::Change::Kept => w::Change::Kept,
        h::Change::Modified => w::Change::Modified,
        h::Change::Split => w::Change::Split,
        h::Change::Merged => w::Change::Merged,
        h::Change::Generated => w::Change::Generated,
    }
}
fn load_change(c: w::Change) -> h::Change {
    match c {
        w::Change::Kept => h::Change::Kept,
        w::Change::Modified => h::Change::Modified,
        w::Change::Split => h::Change::Split,
        w::Change::Merged => h::Change::Merged,
        w::Change::Generated => h::Change::Generated,
    }
}
fn edge_origin(o: &h::EdgeOrigin) -> Result<w::EdgeOrigin> {
    Ok(match o {
        h::EdgeOrigin::Edge { operand, edge } => w::EdgeOrigin::Edge { operand: index(*operand)?, edge: index(*edge)? },
        h::EdgeOrigin::Meet { a, b } => {
            let mut sides = [
                a.origins().iter().copied().map(origin).collect::<Result<Vec<_>>>()?,
                b.origins().iter().copied().map(origin).collect::<Result<Vec<_>>>()?,
            ];
            for side in &mut sides {
                side.sort();
                side.dedup();
            }
            sides.sort();
            w::EdgeOrigin::Meeting { sides }
        }
    })
}
fn load_edge_origin(o: w::EdgeOrigin) -> Result<h::EdgeOrigin> {
    Ok(match o {
        w::EdgeOrigin::Edge { operand, edge } => h::EdgeOrigin::Edge { operand: operand as usize, edge: edge as usize },
        w::EdgeOrigin::Meeting { sides: [a, b] } => {
            if a > b || [&a, &b].iter().any(|s| s.is_empty() || s.windows(2).any(|pair| pair[0] >= pair[1])) {
                return Err(invalid("noncanonical wire history meeting"));
            }
            let side = |o: Vec<w::FaceOrigin>| h::Side::new(o.into_iter().map(load_origin)).ok_or_else(|| invalid("empty history side"));
            let (mut a, mut b) = (side(a)?, side(b)?);
            // The runtime may order its variants differently from the wire schema.
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }
            h::EdgeOrigin::Meet { a, b }
        }
    })
}
fn history(v: &h::History) -> Result<w::History> {
    Ok(w::History {
        operands: index(v.operands)?,
        faces: v
            .faces
            .iter()
            .map(|f| Ok(w::FaceHistory { origins: f.origins.iter().copied().map(origin).collect::<Result<_>>()?, change: change(f.change) }))
            .collect::<Result<_>>()?,
        edges: v
            .edges
            .iter()
            .map(|e| {
                Ok(w::EdgeHistory {
                    faces: e.faces.iter().copied().map(index).collect::<Result<_>>()?,
                    origins: e.origins.iter().map(edge_origin).collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?,
    })
}
fn load_history(v: w::History) -> Result<h::History> {
    Ok(h::History {
        operands: v.operands as usize,
        faces: v
            .faces
            .into_iter()
            .map(|f| h::FaceProv { origins: f.origins.into_iter().map(load_origin).collect(), change: load_change(f.change) })
            .collect(),
        edges: v
            .edges
            .into_iter()
            .map(|e| {
                Ok(h::EdgeProv {
                    faces: e.faces.into_iter().map(|n| n as usize).collect(),
                    origins: e.origins.into_iter().map(load_edge_origin).collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?,
    })
}
pub(super) fn encode(body: &Body) -> Result<w::Metadata> {
    Ok(w::Metadata {
        version: w::VERSION,
        color: body.color,
        paint: body
            .paint
            .as_ref()
            .map(|p| {
                Ok::<_, crate::KernelError>(w::Paint {
                    opacity: p.opacity,
                    faces: p
                        .faces
                        .iter()
                        .map(|f| Ok(w::FacePaint { face: index(f.face)?, color: f.color, opacity: f.opacity }))
                        .collect::<Result<_>>()?,
                })
            })
            .transpose()?,
        boundaries: body.splits.as_ref().map(|s| s.iter().map(|line| line.iter().copied().map(point).collect()).collect()),
        face_sources: body
            .face_sources
            .as_ref()
            .map(|s| s.iter().map(|s| s.as_ref().map(|s| source(s, 0)).transpose()).collect::<Result<_>>())
            .transpose()?,
        history: body.history.as_ref().map(|h| history(h)).transpose()?,
        preserve_boundaries: body.regularization_protected,
    })
}
pub(super) fn apply(body: &mut Body, saved: w::Metadata) -> Result<()> {
    if saved.version != w::VERSION {
        return Err(invalid("metadata version"));
    }
    body.color = saved.color;
    body.paint = saved.paint.map(|p| {
        std::sync::Arc::new(crate::Paint {
            opacity: p.opacity,
            faces: p.faces.into_iter().map(|f| crate::FacePaint { face: f.face as usize, color: f.color, opacity: f.opacity }).collect(),
        })
    });
    body.splits = saved.boundaries.map(|s| std::sync::Arc::new(s.into_iter().map(|line| line.into_iter().map(p).collect()).collect()));
    body.face_sources = saved
        .face_sources
        .map(|s| s.into_iter().map(|s| s.map(|s| load_source(s, 0)).transpose()).collect::<Result<Vec<_>>>())
        .transpose()?
        .map(std::sync::Arc::new);
    body.history = saved.history.map(load_history).transpose()?.map(std::sync::Arc::new);
    body.regularization_protected = saved.preserve_boundaries;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_certificate_records_preserve_all_variants() {
        let plane = w::Plane { origin: [1., 2., 3.], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
        let segments = vec![
            w::Segment::Line { start: [0., 0.], end: [1., 2.] },
            w::Segment::Arc { center: [1., 2.], radius: 3., start: 0.2, sweep: -0.7 },
            w::Segment::Cubic { points: [[0., 0.], [1., 2.], [3., 4.], [5., 6.]] },
            w::Segment::Conic { points: [[0., 0.], [1., 2.], [3., 4.]], middle_weight: 0.7 },
            w::Segment::Bezier { points: vec![[1., 2.]; 5] },
            w::Segment::Bezier { points: vec![[1., 2.]; 8] },
        ];
        for value in &segments {
            assert_eq!(*value, segment(&load_segment(value.clone()).unwrap()).unwrap());
        }
        let region = w::Region { outer: segments.clone(), holes: vec![segments.clone()] };
        let mut sources: Vec<_> =
            segments.into_iter().map(|segment| w::FaceSource::ExtrusionSide { segment, plane: Some(plane.clone()), coalesced: true }).collect();
        sources.extend([
            w::FaceSource::CertifiedExtrusionSide { producer: 5, sketch: 7, curve: "curve-9".into(), plane: None },
            w::FaceSource::ExtrusionCap { high: true, region, plane: plane.clone() },
            w::FaceSource::CertifiedExtrusionCap {
                producer: 5,
                sketch: 7,
                high: false,
                outer: vec!["a".into()],
                holes: vec![vec!["b".into()]],
                plane: plane.clone(),
            },
            w::FaceSource::CylinderCap { producer: 8, high: true, plane: plane.clone() },
            w::FaceSource::BoxFace { producer: Some(5), axis: 2, high: true, plane: plane.clone() },
            w::FaceSource::Imported { item: 2, face: 3, payload_sha256: Some("a".repeat(64)), producer: Some(5) },
        ]);
        sources.push(w::FaceSource::PlanarShell { producer: Some(6), source: Box::new(sources[0].clone()), inner: true, plane });
        for value in sources {
            assert_eq!(value, source(&load_source(value.clone(), 0).unwrap(), 0).unwrap());
        }
        assert!(load_segment(w::Segment::Bezier { points: vec![[0., 0.]; solidcraft_geom::MAX_BEZIER_CTRL + 1] }).is_err());
    }

    #[test]
    fn history_wire_order_is_explicit_and_meetings_round_trip() {
        let origins = vec![
            w::FaceOrigin::Face { operand: 0, face: 2 },
            w::FaceOrigin::Profile { segment: 3 },
            w::FaceOrigin::Cap { end: true },
            w::FaceOrigin::Edge { operand: 1, edge: 4 },
            w::FaceOrigin::Offset { operand: 0, face: 5 },
        ];
        for value in &origins {
            assert_eq!(*value, origin(load_origin(value.clone())).unwrap());
        }
        for value in [w::Change::Kept, w::Change::Modified, w::Change::Split, w::Change::Merged, w::Change::Generated] {
            assert_eq!(value, change(load_change(value)));
        }
        let value = w::EdgeOrigin::Meeting { sides: [origins.clone(), vec![w::FaceOrigin::Offset { operand: 1, face: 2 }]] };
        assert_eq!(value, edge_origin(&load_edge_origin(value.clone()).unwrap()).unwrap());
        assert!(load_edge_origin(w::EdgeOrigin::Meeting { sides: [vec![], origins] }).is_err());
    }
}
