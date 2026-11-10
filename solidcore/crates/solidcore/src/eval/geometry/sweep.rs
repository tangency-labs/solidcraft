//! Procedural sweep definitions and their display nets as plain data.
//!
//! These kernel DTOs mirror the portable record without depending on that layer.
//! Reconstruction and certification belong to the procedural sweep implementation.

/// A definition curve, with its original polynomial or rational coefficients.
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsCurveRecord {
    /// Image dimension.
    pub dim: u32,
    /// Whether the stored coefficients include z.
    pub points_3d: bool,
    /// Polynomial degree.
    pub degree: u32,
    /// Full knot vector.
    pub knots: Vec<f64>,
    /// Weighted coordinates when rational, unweighted coordinates otherwise.
    pub points: Vec<[f64; 3]>,
    /// Rational weights, absent for a polynomial curve.
    pub weights: Option<Vec<f64>>,
}

/// The fitted surface net, with its original coefficients.
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsSurfaceRecord {
    /// Degree along u.
    pub u_degree: u32,
    /// Degree along v.
    pub v_degree: u32,
    /// Control-point count along u.
    pub u_count: u32,
    /// Control-point count along v.
    pub v_count: u32,
    /// Full u knot vector.
    pub u_knots: Vec<f64>,
    /// Full v knot vector.
    pub v_knots: Vec<f64>,
    /// Coefficients in u-major order, weighted when rational.
    pub points: Vec<[f64; 3]>,
    /// Rational weights, absent for a polynomial surface.
    pub weights: Option<Vec<f64>>,
}

/// A piecewise linear radial displacement, held constant beyond its endpoints.
#[derive(Clone, Debug, PartialEq)]
pub struct RadialLawRecord {
    /// Unit displacement direction.
    pub direction: [f64; 3],
    /// Strictly ascending angles in radians.
    pub angles: Vec<f64>,
    /// Displacement at each angle.
    pub offsets: Vec<f64>,
}

/// A screw motion about an oriented axis.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrewMotionRecord {
    /// A point on the axis.
    pub origin: [f64; 3],
    /// Unit axis direction.
    pub axis: [f64; 3],
    /// Signed rise per full turn.
    pub pitch: f64,
    /// Optional radial displacement law.
    pub law: Option<RadialLawRecord>,
}

/// The exact source of an approximated normal-offset section.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrewOffsetRecord {
    /// Original affine profile, before the normal offset.
    pub source: NurbsCurveRecord,
    /// Signed normal-offset distance.
    pub signed_distance: f64,
    /// Certified section approximation error.
    pub approximation_bound: f64,
}

/// The construction defining a sweep; each motion has its own data.
#[derive(Clone, Debug, PartialEq)]
pub enum SweepDefinitionRecord {
    /// A profile carried by a screw motion.
    Screw {
        /// Profile used to construct the fitted net.
        profile: NurbsCurveRecord,
        /// Screw motion.
        motion: ScrewMotionRecord,
        /// Exact offset source, when the profile approximates an offset section.
        offset: Option<ScrewOffsetRecord>,
    },
}

/// A sweep's definition, fitted net and certified relationship, in the face's parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct ProceduralSweepRecord {
    /// Exact defining construction.
    pub definition: SweepDefinitionRecord,
    /// Fitted display and intersection net.
    pub net: NurbsSurfaceRecord,
    /// Net stations along the turn, in radians.
    pub stations: Vec<f64>,
    /// Half angle of the net's spans.
    pub half: f64,
    /// Certified net-to-definition distance; the kernel verifies it on reconstruction.
    pub bound: f64,
    /// Affine parameter-map origins.
    pub map_a: [f64; 2],
    /// Affine parameter-map scales.
    pub map_b: [f64; 2],
    /// Whether the face's u and v exchange roles in the map.
    pub swapped: bool,
}

/// The rigid motion `p ↦ rotation · p + translation` (rows of an orthonormal rotation).
struct Rigid<'a> {
    rotation: &'a [[f64; 3]; 3],
    translation: [f64; 3],
}

impl Rigid<'_> {
    fn direction(&self, d: [f64; 3]) -> [f64; 3] {
        self.rotation
            .map(|row| row[0] * d[0] + row[1] * d[1] + row[2] * d[2])
    }

    fn point(&self, p: [f64; 3]) -> [f64; 3] {
        let r = self.direction(p);
        [
            r[0] + self.translation[0],
            r[1] + self.translation[1],
            r[2] + self.translation[2],
        ]
    }

    /// Coefficients that are weighted (`[x w, y w, z w]`) when `weights` is given, plain
    /// otherwise: the point moves, its weight stays.
    fn coefficients(&self, points: &[[f64; 3]], weights: Option<&[f64]>) -> Vec<[f64; 3]> {
        points
            .iter()
            .enumerate()
            .map(|(i, p)| match weights {
                Some(w) => {
                    let w = w.get(i).copied().unwrap_or(1.0);
                    let r = self.direction(*p);
                    [
                        r[0] + w * self.translation[0],
                        r[1] + w * self.translation[1],
                        r[2] + w * self.translation[2],
                    ]
                }
                None => self.point(*p),
            })
            .collect()
    }

    fn curve(&self, c: &NurbsCurveRecord) -> NurbsCurveRecord {
        NurbsCurveRecord {
            points: self.coefficients(&c.points, c.weights.as_deref()),
            ..c.clone()
        }
    }
}

impl ProceduralSweepRecord {
    /// This sweep moved by the rigid motion `p ↦ rotation · p + translation` (`rotation`'s
    /// rows orthonormal, determinant +1): its profile, screw axis, radial-law direction,
    /// offset source and net move; its stations, parameter map, pitch, offsets and certified
    /// bound stay, as a rigid motion keeps distances and handedness.
    #[must_use]
    pub fn moved(&self, rotation: &[[f64; 3]; 3], translation: [f64; 3]) -> Self {
        let m = Rigid {
            rotation,
            translation,
        };
        let SweepDefinitionRecord::Screw {
            profile,
            motion,
            offset,
        } = &self.definition;
        let mut motion = motion.clone();
        motion.origin = m.point(motion.origin);
        motion.axis = m.direction(motion.axis);
        if let Some(law) = &mut motion.law {
            law.direction = m.direction(law.direction);
        }
        let offset = offset.as_ref().map(|o| ScrewOffsetRecord {
            source: m.curve(&o.source),
            ..o.clone()
        });
        ProceduralSweepRecord {
            definition: SweepDefinitionRecord::Screw {
                profile: m.curve(profile),
                motion,
                offset,
            },
            net: NurbsSurfaceRecord {
                points: m.coefficients(&self.net.points, self.net.weights.as_deref()),
                ..self.net.clone()
            },
            ..self.clone()
        }
    }

    /// A box `(min, max)` enclosing the exact sweep: the hull of its net's control points
    /// widened by its certified bound (the exact sweep lies within `bound` of its net, and the
    /// net within the hull of its controls). `None` for a non-finite or negative bound or a
    /// net without a usable control point.
    #[must_use]
    pub fn outline(&self) -> Option<([f64; 3], [f64; 3])> {
        let r = self.bound;
        if !(r.is_finite() && r >= 0.0) {
            return None;
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for (i, p) in self.net.points.iter().enumerate() {
            let w = match &self.net.weights {
                Some(w) => match w.get(i) {
                    Some(&w) if w > 0.0 => w,
                    _ => continue,
                },
                None => 1.0,
            };
            for ((lo, hi), x) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                let x = x / w;
                if x.is_finite() {
                    *lo = lo.min(x - r);
                    *hi = hi.max(x + r);
                }
            }
        }
        (lo.iter().all(|x| x.is_finite()) && hi.iter().all(|x| x.is_finite())).then_some((lo, hi))
    }
}
