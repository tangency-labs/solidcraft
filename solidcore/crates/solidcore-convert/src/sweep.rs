//! Field copies between the portable sweep record and the kernel's plain-data DTO.

use solidcore::eval::geometry::sweep as k;
use solidcore_record::brep::sweep as r;

fn curve(c: &r::NurbsCurveRecord) -> k::NurbsCurveRecord {
    k::NurbsCurveRecord {
        dim: c.dim,
        points_3d: c.points_3d,
        degree: c.degree,
        knots: c.knots.clone(),
        points: c.points.clone(),
        weights: c.weights.clone(),
    }
}

fn record_curve(c: k::NurbsCurveRecord) -> r::NurbsCurveRecord {
    r::NurbsCurveRecord {
        dim: c.dim,
        points_3d: c.points_3d,
        degree: c.degree,
        knots: c.knots,
        points: c.points,
        weights: c.weights,
    }
}

fn net(n: &r::NurbsSurfaceRecord) -> k::NurbsSurfaceRecord {
    k::NurbsSurfaceRecord {
        u_degree: n.u_degree,
        v_degree: n.v_degree,
        u_count: n.u_count,
        v_count: n.v_count,
        u_knots: n.u_knots.clone(),
        v_knots: n.v_knots.clone(),
        points: n.points.clone(),
        weights: n.weights.clone(),
    }
}

fn record_net(n: k::NurbsSurfaceRecord) -> r::NurbsSurfaceRecord {
    r::NurbsSurfaceRecord {
        u_degree: n.u_degree,
        v_degree: n.v_degree,
        u_count: n.u_count,
        v_count: n.v_count,
        u_knots: n.u_knots,
        v_knots: n.v_knots,
        points: n.points,
        weights: n.weights,
    }
}

fn motion(m: &r::ScrewMotionRecord) -> k::ScrewMotionRecord {
    k::ScrewMotionRecord {
        origin: m.origin,
        axis: m.axis,
        pitch: m.pitch,
        law: m.law.as_ref().map(|l| k::RadialLawRecord {
            direction: l.direction,
            angles: l.angles.clone(),
            offsets: l.offsets.clone(),
        }),
    }
}

fn record_motion(m: k::ScrewMotionRecord) -> r::ScrewMotionRecord {
    r::ScrewMotionRecord {
        origin: m.origin,
        axis: m.axis,
        pitch: m.pitch,
        law: m.law.map(|l| r::RadialLawRecord {
            direction: l.direction,
            angles: l.angles,
            offsets: l.offsets,
        }),
    }
}

/// A portable sweep as the kernel's DTO, copying each stored field.
pub(crate) fn sweep(s: &r::ProceduralSweepRecord) -> k::ProceduralSweepRecord {
    let definition = match &s.definition {
        r::SweepDefinitionRecord::Screw {
            profile,
            motion: m,
            offset,
        } => k::SweepDefinitionRecord::Screw {
            profile: curve(profile),
            motion: motion(m),
            offset: offset.as_ref().map(|o| k::ScrewOffsetRecord {
                source: curve(&o.source),
                signed_distance: o.signed_distance,
                approximation_bound: o.approximation_bound,
            }),
        },
    };
    k::ProceduralSweepRecord {
        definition,
        net: net(&s.net),
        stations: s.stations.clone(),
        half: s.half,
        bound: s.bound,
        map_a: s.map_a,
        map_b: s.map_b,
        swapped: s.swapped,
    }
}

/// The kernel's sweep DTO as the portable record, moving its stored vectors.
pub(crate) fn record_sweep(s: k::ProceduralSweepRecord) -> r::ProceduralSweepRecord {
    let definition = match s.definition {
        k::SweepDefinitionRecord::Screw {
            profile,
            motion: m,
            offset,
        } => r::SweepDefinitionRecord::Screw {
            profile: record_curve(profile),
            motion: record_motion(m),
            offset: offset.map(|o| r::ScrewOffsetRecord {
                source: record_curve(o.source),
                signed_distance: o.signed_distance,
                approximation_bound: o.approximation_bound,
            }),
        },
    };
    r::ProceduralSweepRecord {
        definition,
        net: record_net(s.net),
        stations: s.stations,
        half: s.half,
        bound: s.bound,
        map_a: s.map_a,
        map_b: s.map_b,
        swapped: s.swapped,
    }
}
