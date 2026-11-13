//! A body face's exact surface, for an application: what the face is ([`Exact`]: its analytic
//! label, the torus a round of the exact circle class is, a blend by its definition, or its
//! NURBS), and on it, in the face's own (u, v), its point, its unit normal and the point of it
//! nearest a given point from a guess. A blend is evaluated on its definition S, rebuilt from the
//! record and its net certified again; the nearest point is a bounded Newton iteration, refused
//! when it does not settle. The kernel computes every value (`surface::face_evaluation`); this
//! module reads the record and converts.

use solidcore::kernel::Kernel;
use solidcore::surface::face_evaluation as fe;
use solidcore::surface::face_surface::{face_surface, FaceSurface as KernelFace};
use solidcore::surface::procedural_blend::convert::definition_surface;
use solidcore_record::brep::{Brep, Loop, Surface};

use crate::error::{Error, Result};
use crate::session::{run, Answer};

/// Whether the natural orientations of two qualified coplanar supports oppose.
pub fn opposed(
    face: &solidcore_record::brep::Face,
    support: &solidcore_record::brep::Face,
) -> Result<bool> {
    let evaluator = |surface: &Surface| {
        let surface = solidcore_convert::surface(surface).map_err(|_| Error::Argument)?;
        solidcore::eval::SurfaceEval::new(&surface).map_err(|_| Error::Argument)
    };
    let a = evaluator(&face.surface)?;
    let b = evaluator(&support.surface)?;
    a.opposed_to(face.domain, &b, support.domain)
        .ok_or(Error::Argument)
}

/// Give a planar tensor face an analytic support, projecting its existing
/// boundary curves without changing their intervals or weights.
pub fn planar_tensor(
    face: &mut solidcore_record::brep::Face,
    edges: &[solidcore_record::brep::Edge],
) -> Result<bool> {
    use solidcore::eval::planar_tensor::PlanarTensor;
    let surface = solidcore_convert::surface(&face.surface).map_err(|_| Error::Argument)?;
    let evaluator = solidcore::eval::SurfaceEval::new(&surface).map_err(|_| Error::Argument)?;
    let Some(plane) = PlanarTensor::of(&evaluator, face.domain) else {
        return Ok(false);
    };
    let mut loops = face.loops.clone();
    let mut curves = Vec::new();
    for boundary in &mut loops {
        let Loop::Edges { edges: uses } = boundary else {
            return Err(Error::Argument);
        };
        for edge_use in uses {
            let edge = edges.get(edge_use.edge as usize).ok_or(Error::Argument)?;
            let curve = solidcore_convert::curve(&edge.curve).map_err(|_| Error::Argument)?;
            let pcurve = plane.parameter_curve(curve).ok_or(Error::Argument)?;
            edge_use.pcurve = solidcore_convert::record_curve2(pcurve.clone());
            curves.push(pcurve);
        }
    }
    let domain = PlanarTensor::parameter_box(curves.iter()).ok_or(Error::Argument)?;
    let xyz = |p: solidcore::Vec3| [p.x, p.y, p.z];
    face.nurbs = None;
    face.surface = Surface::Plane {
        origin: xyz(plane.origin),
        u_axis: xyz(plane.u),
        v_axis: xyz(plane.v),
    };
    face.domain = domain;
    face.loops = loops;
    Ok(true)
}

/// Exact spline evaluator and parameter map for a native surface record.
pub fn eval_surface(
    surface: &Surface,
    domain: [[f64; 2]; 2],
) -> Result<(
    solidcore::eval::plan::EvalSurface,
    solidcore::eval::surface_plan::Params,
)> {
    let surface = solidcore_convert::surface(surface).map_err(|_| Error::Argument)?;
    solidcore::eval::surface_plan::eval_surface(&surface, domain).map_err(|error| match error {
        solidcore::eval::EvalError::Unsupported(_) => {
            Error::Unsupported("the face has no supported exact evaluator")
        }
        other => Error::Record(other.to_string()),
    })
}

/// Resolve a point on a qualified chart, retaining the unwrapped branch near a hint.
pub fn chart_parameters(
    surface: &solidcore::eval::plan::EvalSurface,
    point: solidcore_record::Vec3,
    near: Option<[f64; 2]>,
    tolerance: f64,
) -> Option<[f64; 2]> {
    solidcore::eval::plan::uv::qualified_parameters(
        surface,
        solidcore_convert::vec3(point),
        near,
        tolerance,
    )
}

/// Verify that one outer trim follows all sides of the chart rectangle.
pub fn rectangular_trim(face: &solidcore_record::brep::Face, span: [[f64; 2]; 2]) -> bool {
    let [Loop::Edges { edges }] = face.loops.as_slice() else {
        return false;
    };
    let curves: Vec<_> = edges
        .iter()
        .map(|edge| solidcore_convert::curve2(&edge.pcurve))
        .collect();
    solidcore::eval::planar_tensor::rectangular_trim(&curves, span)
}

/// What a face's exact surface is.
#[derive(Clone, Debug, PartialEq)]
pub enum Exact {
    /// An analytic surface, as the record labels it (a plane, cylinder, cone, sphere, torus, ball
    /// patch or revolution); the face's (u, v) are its NURBS form's.
    Label(Surface),
    /// A round of the exact circle class: the torus its definition gives.
    Torus {
        /// The torus's centre.
        centre: [f64; 3],
        /// Its unit axis.
        axis: [f64; 3],
        /// The major radius: the circle the ball's centre runs on.
        major: f64,
        /// The minor radius: the ball's.
        minor: f64,
    },
    /// A blend by its definition S, its net within `bound` of it (0 for an exact round).
    Blend {
        /// The net's certified bound from S.
        bound: f64,
    },
    /// A B-spline or NURBS surface: the face is its NURBS.
    Nurbs,
}

/// The point of a face's surface nearest a given point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Closest {
    /// Its (u, v), the face's.
    pub uv: [f64; 2],
    /// The point.
    pub point: [f64; 3],
    /// Its distance from the given point.
    pub distance: f64,
}

/// A body face's exact surface ([`FaceSurface::of`]).
#[derive(Debug)]
pub struct FaceSurface {
    face: KernelFace,
    exact: Exact,
}

// ([f64; 3] answers through measure.rs's impl.)
impl Answer for Closest {}

fn triple(p: solidcore::Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

impl FaceSurface {
    /// Face `face` of `record`'s exact surface. A blend's definition is rebuilt from the record
    /// and its net, the face's NURBS form, certified against it again.
    ///
    /// # Errors
    ///
    /// [`Error::Argument`] for a face past the record's, or a record the conversion refuses;
    /// [`Error::Unsupported`] for a face whose (u, v) are its label's own (an analytic label with
    /// no NURBS form, whose parameters are not the kernel's); [`Error::DefinitionNotCertified`]
    /// for a blend whose net its definition does not certify; the kernel's refusals.
    pub fn of(record: &Brep, face: usize) -> Result<FaceSurface> {
        let f = record.faces.get(face).ok_or(Error::Argument)?;
        let conv = |s: &Surface| solidcore_convert::surface(s).map_err(|_| Error::Argument);
        // A blend is built on its net; a label on its form; a ball patch's loops, which the
        // kernel certifies on its sphere.
        let (label, form, definition) = match (&f.surface, &f.nurbs) {
            (Surface::Blend { definition }, Some(net)) => (
                conv(net)?,
                None,
                Some(solidcore_convert::blend::blend(definition)),
            ),
            (Surface::Blend { .. }, None) => return Err(Error::Argument),
            (label, form) => (conv(label)?, form.as_ref().map(conv).transpose()?, None),
        };
        let mut boundary = Vec::new();
        if matches!(f.surface, Surface::BallPatch { .. }) {
            for l in &f.loops {
                if let Loop::Edges { edges } = l {
                    for u in edges {
                        let e = record.edges.get(u.edge as usize).ok_or(Error::Argument)?;
                        boundary.push(
                            solidcore_convert::curve(e.nurbs.as_ref().unwrap_or(&e.curve))
                                .map_err(|_| Error::Argument)?,
                        );
                    }
                }
            }
        }
        let domain = f.domain;
        let labelled = !matches!(
            f.surface,
            Surface::Spline { .. } | Surface::RationalSpline { .. } | Surface::Blend { .. }
        );
        let surface_label = f.surface.clone();
        run("face_surface::of", move |k: &mut Kernel| {
            let mut made = face_surface(k, &label, form.as_ref(), domain, &boundary)?;
            if !made.trims {
                return Ok(Err(Error::Unsupported(
                    "a face whose (u, v) are its label's own",
                )));
            }
            let exact = match &definition {
                Some(definition) => match definition_surface(k, definition, &made.surface)? {
                    Ok(surface) => {
                        made.surface = surface;
                        match fe::circle_round_torus(k, &made)? {
                            Some(t) => Exact::Torus {
                                centre: triple(t.centre),
                                axis: triple(t.axis),
                                major: t.major,
                                minor: t.minor,
                            },
                            None => Exact::Blend {
                                bound: made.surface.procedural_blend().map_or(0.0, |b| b.bound()),
                            },
                        }
                    }
                    Err(eps) => {
                        return Ok(Err(Error::DefinitionNotCertified { face, eps }));
                    }
                },
                None if labelled => Exact::Label(surface_label),
                None => Exact::Nurbs,
            };
            Ok(Ok(FaceSurface { face: made, exact }))
        })?
    }

    /// What the face's exact surface is.
    #[must_use]
    pub fn exact(&self) -> &Exact {
        &self.exact
    }

    /// The face's parameter box, `[[u0, u1], [v0, v1]]`, in its own (u, v).
    #[must_use]
    pub fn domain(&self) -> [[f64; 2]; 2] {
        fe::face_domain(&self.face)
    }

    /// The point at the face's (u, v).
    ///
    /// # Errors
    ///
    /// The kernel's refusal of the evaluation.
    pub fn point(&self, uv: [f64; 2]) -> Result<[f64; 3]> {
        run("face_surface::point", |k| {
            fe::point(k, &self.face, uv).map(triple)
        })
    }

    /// The unit normal at the face's (u, v), `∂S/∂u × ∂S/∂v` made unit.
    ///
    /// # Errors
    ///
    /// [`Error::Degenerate`] where the partials are parallel; the kernel's refusal of the
    /// evaluation.
    pub fn normal(&self, uv: [f64; 2]) -> Result<[f64; 3]> {
        run("face_surface::normal", |k| {
            fe::normal(k, &self.face, uv).map(triple)
        })
    }

    /// The point of the face's surface nearest `p`, from the guess `guess` (the face's (u, v)),
    /// within the face's parameter box: a bounded Newton iteration
    /// (`face_evaluation::MAX_NEAREST_STEPS` steps).
    ///
    /// # Errors
    ///
    /// The kernel's `NotConverged` past its steps, as [`Error::Status`]; [`Error::Degenerate`];
    /// the kernel's refusal of the evaluations.
    pub fn closest_point(&self, p: [f64; 3], guess: [f64; 2]) -> Result<Closest> {
        let p = solidcore::Vec3::new(p[0], p[1], p[2]);
        run("face_surface::closest_point", |k| {
            fe::nearest(k, &self.face, p, guess).map(|n| Closest {
                uv: n.uv,
                point: triple(n.point),
                distance: n.distance,
            })
        })
    }
}
