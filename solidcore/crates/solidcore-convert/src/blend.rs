//! A face's blend definition, the record's ([`r::BlendDefinition`]) and the kernel's
//! ([`kb::FaceDefinition`]), both ways: the same records, field by field. The kernel reads a
//! definition off a face ([`solidcore::surface::procedural_blend::convert::face_definition`]) and
//! gives it back, rebuilt and its net certified again
//! ([`solidcore::surface::procedural_blend::convert::adopt_definitions`]).
//!
//! Recursion: a support that is itself a blend is copied with its own supports, at most
//! three definitions deep: a record's validation and the kernel's own records bound it.

use solidcore::surface::procedural_blend::record as kb;
use solidcore_record::brep::blend as r;

/// The record's definition as the kernel's.
#[must_use]
pub fn blend(d: &r::BlendDefinition) -> kb::FaceDefinition {
    match d {
        r::BlendDefinition::Procedural(d) => {
            kb::FaceDefinition::Procedural(Box::new(definition(d)))
        }
        r::BlendDefinition::ExactRound(d) => {
            kb::FaceDefinition::ExactRound(Box::new(kb::ExactRoundDefinition {
                class: class(d.class),
                supports: [support(&d.supports[0]), support(&d.supports[1])],
                radius: d.radius,
                order: d.order,
                leader: d.leader.as_ref().map(curve),
                inputs: kb::ClassInputsRecord {
                    uv: d.inputs.uv,
                    offsets: d.inputs.offsets,
                    centre: d.inputs.centre,
                    tol: d.inputs.tol,
                    ball_on_collapsed: d.inputs.ball_on_collapsed,
                    radii_can_change: d.inputs.radii_can_change,
                },
                chamfer: d.chamfer.as_ref().map(chamfer),
                leader_map: kb::LeaderMapRecord {
                    a: d.leader_map.a,
                    b: d.leader_map.b,
                    along_v: d.leader_map.along_v,
                },
                net_kind: net_kind(d.net_kind),
            }))
        }
    }
}

/// The kernel's definition as the record's.
#[must_use]
pub fn record_blend(d: kb::FaceDefinition) -> r::BlendDefinition {
    match d {
        kb::FaceDefinition::Procedural(d) => {
            r::BlendDefinition::Procedural(Box::new(record_definition(*d)))
        }
        kb::FaceDefinition::ExactRound(d) => {
            let d = *d;
            let [s0, s1] = d.supports;
            r::BlendDefinition::ExactRound(Box::new(r::ExactRoundDefinition {
                class: record_class(d.class),
                supports: [record_support(s0), record_support(s1)],
                radius: d.radius,
                order: d.order,
                leader: d.leader.map(record_curve),
                inputs: r::ClassInputsRecord {
                    uv: d.inputs.uv,
                    offsets: d.inputs.offsets,
                    centre: d.inputs.centre,
                    tol: d.inputs.tol,
                    ball_on_collapsed: d.inputs.ball_on_collapsed,
                    radii_can_change: d.inputs.radii_can_change,
                },
                chamfer: d.chamfer.map(record_chamfer),
                leader_map: r::LeaderMapRecord {
                    a: d.leader_map.a,
                    b: d.leader_map.b,
                    along_v: d.leader_map.along_v,
                },
                net_kind: record_net_kind(d.net_kind),
            }))
        }
    }
}

fn definition(d: &r::BlendDefinitionRecord) -> kb::BlendDefinitionRecord {
    kb::BlendDefinitionRecord {
        supports: [support(&d.supports[0]), support(&d.supports[1])],
        sides: d.sides,
        radius: match &d.radius {
            r::RadiusRecord::Constant { offsets } => {
                kb::RadiusRecord::Constant { offsets: *offsets }
            }
            r::RadiusRecord::Law(law) => kb::RadiusRecord::Law(curve(law)),
        },
        order: d.order,
        thumbweight: d.thumbweight,
        leader: curve(&d.leader),
        seeds: [curve(&d.seeds[0]), curve(&d.seeds[1])],
        bound: d.bound,
        map: kb::ParameterMapRecord {
            a: d.map.a,
            b: d.map.b,
            swapped: d.map.swapped,
        },
        chamfer: d.chamfer.as_ref().map(chamfer),
    }
}

fn record_definition(d: kb::BlendDefinitionRecord) -> r::BlendDefinitionRecord {
    let [s0, s1] = d.supports;
    let [seed0, seed1] = d.seeds;
    r::BlendDefinitionRecord {
        supports: [record_support(s0), record_support(s1)],
        sides: d.sides,
        radius: match d.radius {
            kb::RadiusRecord::Constant { offsets } => r::RadiusRecord::Constant { offsets },
            kb::RadiusRecord::Law(law) => r::RadiusRecord::Law(record_curve(law)),
        },
        order: d.order,
        thumbweight: d.thumbweight,
        leader: record_curve(d.leader),
        seeds: [record_curve(seed0), record_curve(seed1)],
        bound: d.bound,
        map: r::ParameterMapRecord {
            a: d.map.a,
            b: d.map.b,
            swapped: d.map.swapped,
        },
        chamfer: d.chamfer.map(record_chamfer),
    }
}

fn support(s: &r::SupportRecord) -> kb::SupportRecord {
    match s {
        r::SupportRecord::Nurbs { kind, form } => kb::SupportRecord::Nurbs {
            kind: support_kind(*kind),
            form: surface(form),
        },
        r::SupportRecord::Blend(b) => {
            kb::SupportRecord::Blend(Box::new(kb::ProceduralBlendRecord {
                definition: definition(&b.definition),
                net: surface(&b.net),
            }))
        }
    }
}

fn record_support(s: kb::SupportRecord) -> r::SupportRecord {
    match s {
        kb::SupportRecord::Nurbs { kind, form } => r::SupportRecord::Nurbs {
            kind: record_support_kind(kind),
            form: record_surface(form),
        },
        kb::SupportRecord::Blend(b) => {
            let b = *b;
            r::SupportRecord::Blend(Box::new(r::ProceduralBlendRecord {
                definition: record_definition(b.definition),
                net: record_surface(b.net),
            }))
        }
    }
}

fn curve(c: &r::NurbsCurveRecord) -> kb::NurbsCurveRecord {
    kb::NurbsCurveRecord {
        dim: c.dim,
        points_3d: c.points_3d,
        degree: c.degree,
        knots: c.knots.clone(),
        points: c.points.clone(),
        weights: c.weights.clone(),
    }
}

fn record_curve(c: kb::NurbsCurveRecord) -> r::NurbsCurveRecord {
    r::NurbsCurveRecord {
        dim: c.dim,
        points_3d: c.points_3d,
        degree: c.degree,
        knots: c.knots,
        points: c.points,
        weights: c.weights,
    }
}

fn surface(s: &r::NurbsSurfaceRecord) -> kb::NurbsSurfaceRecord {
    kb::NurbsSurfaceRecord {
        u_degree: s.u_degree,
        v_degree: s.v_degree,
        u_count: s.u_count,
        v_count: s.v_count,
        u_knots: s.u_knots.clone(),
        v_knots: s.v_knots.clone(),
        points: s.points.clone(),
        weights: s.weights.clone(),
    }
}

fn record_surface(s: kb::NurbsSurfaceRecord) -> r::NurbsSurfaceRecord {
    r::NurbsSurfaceRecord {
        u_degree: s.u_degree,
        v_degree: s.v_degree,
        u_count: s.u_count,
        v_count: s.v_count,
        u_knots: s.u_knots,
        v_knots: s.v_knots,
        points: s.points,
        weights: s.weights,
    }
}

fn chamfer(c: &r::ChamferRecord) -> kb::ChamferRecord {
    kb::ChamferRecord {
        size: match c.size {
            r::ChamferSizeRecord::Distances(x) => kb::ChamferSizeRecord::Distances(x),
            r::ChamferSizeRecord::DistanceAngle { distance, angle } => {
                kb::ChamferSizeRecord::DistanceAngle { distance, angle }
            }
            r::ChamferSizeRecord::Chord(l) => kb::ChamferSizeRecord::Chord(l),
        },
        wedge: c.wedge,
    }
}

fn record_chamfer(c: kb::ChamferRecord) -> r::ChamferRecord {
    r::ChamferRecord {
        size: match c.size {
            kb::ChamferSizeRecord::Distances(x) => r::ChamferSizeRecord::Distances(x),
            kb::ChamferSizeRecord::DistanceAngle { distance, angle } => {
                r::ChamferSizeRecord::DistanceAngle { distance, angle }
            }
            kb::ChamferSizeRecord::Chord(l) => r::ChamferSizeRecord::Chord(l),
        },
        wedge: c.wedge,
    }
}

fn support_kind(k: r::SupportKindRecord) -> kb::SupportKindRecord {
    match k {
        r::SupportKindRecord::Free => kb::SupportKindRecord::Free,
        r::SupportKindRecord::Plane => kb::SupportKindRecord::Plane,
        r::SupportKindRecord::Cylinder => kb::SupportKindRecord::Cylinder,
        r::SupportKindRecord::Cone => kb::SupportKindRecord::Cone,
        r::SupportKindRecord::Sphere => kb::SupportKindRecord::Sphere,
        r::SupportKindRecord::Torus => kb::SupportKindRecord::Torus,
    }
}

fn record_support_kind(k: kb::SupportKindRecord) -> r::SupportKindRecord {
    match k {
        kb::SupportKindRecord::Free => r::SupportKindRecord::Free,
        kb::SupportKindRecord::Plane => r::SupportKindRecord::Plane,
        kb::SupportKindRecord::Cylinder => r::SupportKindRecord::Cylinder,
        kb::SupportKindRecord::Cone => r::SupportKindRecord::Cone,
        kb::SupportKindRecord::Sphere => r::SupportKindRecord::Sphere,
        kb::SupportKindRecord::Torus => r::SupportKindRecord::Torus,
    }
}

fn class(c: r::ExactClassRecord) -> kb::ExactClassRecord {
    match c {
        r::ExactClassRecord::Line => kb::ExactClassRecord::Line,
        r::ExactClassRecord::Circle => kb::ExactClassRecord::Circle,
        r::ExactClassRecord::Point => kb::ExactClassRecord::Point,
        r::ExactClassRecord::Chord => kb::ExactClassRecord::Chord,
        r::ExactClassRecord::Conic => kb::ExactClassRecord::Conic,
        r::ExactClassRecord::ConicPair => kb::ExactClassRecord::ConicPair,
        r::ExactClassRecord::NodalQuartic => kb::ExactClassRecord::NodalQuartic,
    }
}

fn record_class(c: kb::ExactClassRecord) -> r::ExactClassRecord {
    match c {
        kb::ExactClassRecord::Line => r::ExactClassRecord::Line,
        kb::ExactClassRecord::Circle => r::ExactClassRecord::Circle,
        kb::ExactClassRecord::Point => r::ExactClassRecord::Point,
        kb::ExactClassRecord::Chord => r::ExactClassRecord::Chord,
        kb::ExactClassRecord::Conic => r::ExactClassRecord::Conic,
        kb::ExactClassRecord::ConicPair => r::ExactClassRecord::ConicPair,
        kb::ExactClassRecord::NodalQuartic => r::ExactClassRecord::NodalQuartic,
    }
}

fn net_kind(k: r::ExactNetKindRecord) -> kb::ExactNetKindRecord {
    match k {
        r::ExactNetKindRecord::Plane => kb::ExactNetKindRecord::Plane,
        r::ExactNetKindRecord::Cylinder => kb::ExactNetKindRecord::Cylinder,
        r::ExactNetKindRecord::Cone => kb::ExactNetKindRecord::Cone,
        r::ExactNetKindRecord::Sphere => kb::ExactNetKindRecord::Sphere,
        r::ExactNetKindRecord::Torus => kb::ExactNetKindRecord::Torus,
        r::ExactNetKindRecord::Revolution => kb::ExactNetKindRecord::Revolution,
        r::ExactNetKindRecord::BallPatch => kb::ExactNetKindRecord::BallPatch,
        r::ExactNetKindRecord::BSpline => kb::ExactNetKindRecord::BSpline,
    }
}

fn record_net_kind(k: kb::ExactNetKindRecord) -> r::ExactNetKindRecord {
    match k {
        kb::ExactNetKindRecord::Plane => r::ExactNetKindRecord::Plane,
        kb::ExactNetKindRecord::Cylinder => r::ExactNetKindRecord::Cylinder,
        kb::ExactNetKindRecord::Cone => r::ExactNetKindRecord::Cone,
        kb::ExactNetKindRecord::Sphere => r::ExactNetKindRecord::Sphere,
        kb::ExactNetKindRecord::Torus => r::ExactNetKindRecord::Torus,
        kb::ExactNetKindRecord::Revolution => r::ExactNetKindRecord::Revolution,
        kb::ExactNetKindRecord::BallPatch => r::ExactNetKindRecord::BallPatch,
        kb::ExactNetKindRecord::BSpline => r::ExactNetKindRecord::BSpline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A definition crosses to the kernel's record and back field for field.
    #[test]
    fn a_definition_crosses_both_ways_unchanged() {
        let form = r::NurbsSurfaceRecord {
            u_degree: 1,
            v_degree: 1,
            u_count: 2,
            v_count: 2,
            u_knots: vec![0., 0., 1., 1.],
            v_knots: vec![0., 0., 1., 1.],
            points: vec![[0., 0., 0.], [0., 1., 0.], [1., 0., 0.], [1., 1., 0.]],
            weights: Some(vec![1., 0.5, 1., 0.5]),
        };
        let curve = |dim: u32| r::NurbsCurveRecord {
            dim,
            points_3d: dim == 3,
            degree: 1,
            knots: vec![0., 0., 1., 1.],
            points: vec![[0., 0., 0.], [1., 2., 3.]],
            weights: None,
        };
        let inner = r::BlendDefinitionRecord {
            supports: [
                r::SupportRecord::Nurbs {
                    kind: r::SupportKindRecord::Cylinder,
                    form: form.clone(),
                },
                r::SupportRecord::Nurbs {
                    kind: r::SupportKindRecord::Free,
                    form: form.clone(),
                },
            ],
            sides: [1., -1.],
            radius: r::RadiusRecord::Law(curve(2)),
            order: 2,
            thumbweight: 0.25,
            leader: curve(3),
            seeds: [curve(2), curve(2)],
            bound: 1e-7,
            map: r::ParameterMapRecord {
                a: [0.5, 1.],
                b: [2., -1.],
                swapped: true,
            },
            chamfer: Some(r::ChamferRecord {
                size: r::ChamferSizeRecord::DistanceAngle {
                    distance: 1.,
                    angle: 0.5,
                },
                wedge: 1.5,
            }),
        };
        let procedural = r::BlendDefinition::Procedural(Box::new(r::BlendDefinitionRecord {
            supports: [
                r::SupportRecord::Blend(Box::new(r::ProceduralBlendRecord {
                    definition: inner.clone(),
                    net: form.clone(),
                })),
                r::SupportRecord::Nurbs {
                    kind: r::SupportKindRecord::Plane,
                    form: form.clone(),
                },
            ],
            radius: r::RadiusRecord::Constant { offsets: [2., 3.] },
            chamfer: None,
            ..inner
        }));
        let exact = r::BlendDefinition::ExactRound(Box::new(r::ExactRoundDefinition {
            class: r::ExactClassRecord::Circle,
            supports: [
                r::SupportRecord::Nurbs {
                    kind: r::SupportKindRecord::Plane,
                    form: form.clone(),
                },
                r::SupportRecord::Nurbs {
                    kind: r::SupportKindRecord::Cylinder,
                    form,
                },
            ],
            radius: 0.3,
            order: 1,
            leader: Some(curve(3)),
            inputs: r::ClassInputsRecord {
                uv: [[0.1, 0.2], [0.3, 0.4]],
                offsets: [0.3, -0.3],
                centre: [15., 15., 9.7],
                tol: 1e-6,
                ball_on_collapsed: false,
                radii_can_change: true,
            },
            chamfer: None,
            leader_map: r::LeaderMapRecord {
                a: 0.5,
                b: 2.,
                along_v: true,
            },
            net_kind: r::ExactNetKindRecord::Revolution,
        }));
        for d in [procedural, exact] {
            assert_eq!(record_blend(blend(&d)), d);
        }
    }
}
