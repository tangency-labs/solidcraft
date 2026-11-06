//! One case of jyviko's SSI corpus (`cases/data/<id>.json` of
//! <https://github.com/jyviko/ssi-corpus>, commit `b04c8a6`, MIT OR Apache-2.0): the two
//! typed surfaces, the true surfaces for exact distances and the expected topology.

use std::path::Path;

use serde_json::Value;

use super::geom::{arr_at, f, nums, points, Truth, P3};
use crate::HarnessError;

fn err(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// A surface as the corpus describes it (the corpus runner's `build_surface` input).
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceSpec {
    /// `plane`: origin, axes and the `u`, `v` intervals.
    Plane {
        /// Origin.
        origin: P3,
        /// X axis.
        xaxis: P3,
        /// Y axis.
        yaxis: P3,
        /// `[u0, u1]`.
        u: [f64; 2],
        /// `[v0, v1]`.
        v: [f64; 2],
    },
    /// `cylinder`: radius, angle interval (degrees) and height interval.
    Cylinder {
        /// Origin of the axis.
        origin: P3,
        /// X axis.
        xaxis: P3,
        /// Y axis.
        yaxis: P3,
        /// Radius.
        radius: f64,
        /// Angles (degrees).
        angle: [f64; 2],
        /// Heights.
        height: [f64; 2],
    },
    /// `cone`: apex-based, full turn.
    Cone {
        /// Apex.
        apex: P3,
        /// X axis.
        xaxis: P3,
        /// Y axis.
        yaxis: P3,
        /// Half angle (degrees).
        half_angle_deg: f64,
        /// Heights from the apex.
        height: [f64; 2],
    },
    /// `sphere`: longitude and latitude intervals (degrees).
    Sphere {
        /// Centre.
        center: P3,
        /// X axis.
        xaxis: P3,
        /// Y axis.
        yaxis: P3,
        /// Radius.
        radius: f64,
        /// Longitudes.
        lon: [f64; 2],
        /// Latitudes.
        lat: [f64; 2],
    },
    /// `torus`: major and minor angle intervals (degrees).
    Torus {
        /// Centre.
        center: P3,
        /// X axis.
        xaxis: P3,
        /// Y axis.
        yaxis: P3,
        /// Major radius `R`.
        major_radius: f64,
        /// Minor radius `r`.
        minor_radius: f64,
        /// Major angles.
        major: [f64; 2],
        /// Minor angles.
        minor: [f64; 2],
    },
    /// `nurbs`: degrees, knots, net and weights (Cartesian points, row `i` = u index).
    Nurbs {
        /// `(p, q)`.
        degree: [usize; 2],
        /// U knots.
        knots_u: Vec<f64>,
        /// V knots.
        knots_v: Vec<f64>,
        /// Control points in u.
        nu: usize,
        /// Control points in v.
        nv: usize,
        /// Points, `i * nv + j`.
        points: Vec<P3>,
        /// Weights, same layout.
        weights: Vec<f64>,
    },
}

impl SurfaceSpec {
    /// The corpus type name.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Plane { .. } => "plane",
            Self::Cylinder { .. } => "cylinder",
            Self::Cone { .. } => "cone",
            Self::Sphere { .. } => "sphere",
            Self::Torus { .. } => "torus",
            Self::Nurbs { .. } => "nurbs",
        }
    }

    fn parse(v: &Value) -> Result<(Self, Option<[f64; 4]>), HarnessError> {
        let ty = v
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| err("surface without type"))?;
        let s = match ty {
            "plane" => Self::Plane {
                origin: arr_at(v, "origin")?,
                xaxis: arr_at(v, "xaxis")?,
                yaxis: arr_at(v, "yaxis")?,
                u: arr_at(v, "u")?,
                v: arr_at(v, "v")?,
            },
            "cylinder" => Self::Cylinder {
                origin: arr_at(v, "origin")?,
                xaxis: arr_at(v, "xaxis")?,
                yaxis: arr_at(v, "yaxis")?,
                radius: f(v, "radius")?,
                angle: arr_at(v, "angle")?,
                height: arr_at(v, "height")?,
            },
            "cone" => Self::Cone {
                apex: arr_at(v, "apex")?,
                xaxis: arr_at(v, "xaxis")?,
                yaxis: arr_at(v, "yaxis")?,
                half_angle_deg: f(v, "half_angle_deg")?,
                height: arr_at(v, "height")?,
            },
            "sphere" => Self::Sphere {
                center: arr_at(v, "center")?,
                xaxis: arr_at(v, "xaxis")?,
                yaxis: arr_at(v, "yaxis")?,
                radius: f(v, "radius")?,
                lon: arr_at(v, "lon")?,
                lat: arr_at(v, "lat")?,
            },
            "torus" => Self::Torus {
                center: arr_at(v, "center")?,
                xaxis: arr_at(v, "xaxis")?,
                yaxis: arr_at(v, "yaxis")?,
                major_radius: f(v, "R")?,
                minor_radius: f(v, "r")?,
                major: arr_at(v, "major")?,
                minor: arr_at(v, "minor")?,
            },
            "nurbs" => {
                let deg: [f64; 2] = arr_at(v, "degree")?;
                let count = |x: f64, what: &str| {
                    if x >= 0.0 && x.fract() == 0.0 && x <= 1e9 {
                        Ok(x as usize)
                    } else {
                        Err(err(format!("bad nurbs {what}")))
                    }
                };
                Self::Nurbs {
                    degree: [count(deg[0], "degree")?, count(deg[1], "degree")?],
                    knots_u: nums(v.get("knots_u").ok_or_else(|| err("missing knots_u"))?)?,
                    knots_v: nums(v.get("knots_v").ok_or_else(|| err("missing knots_v"))?)?,
                    nu: count(f(v, "nu")?, "nu")?,
                    nv: count(f(v, "nv")?, "nv")?,
                    points: points(v.get("points").ok_or_else(|| err("missing points"))?)?,
                    weights: nums(v.get("weights").ok_or_else(|| err("missing weights"))?)?,
                }
            }
            other => return Err(err(format!("unknown surface type `{other}`"))),
        };
        let domain = match v.get("domain") {
            Some(d) => Some(super::geom::arr::<4>(d)?),
            None => None,
        };
        Ok((s, domain))
    }
}

/// A case: inputs, truth and expectations.
#[derive(Debug, Clone)]
pub struct SsiCase {
    /// `id`.
    pub id: String,
    /// `class` (e.g. `1 transversal`).
    pub class: String,
    /// The two surfaces with their optional trim boxes.
    pub surfaces: [(SurfaceSpec, Option<[f64; 4]>); 2],
    /// `truth`: one entry per surface.
    pub truth: [Truth; 2],
    /// `expected.branches[*].points`.
    pub branches: Vec<Vec<P3>>,
    /// `expected.coincident_regions[*].boundary`.
    pub coincident_regions: Vec<Vec<P3>>,
    /// `expected.isolated_points`.
    pub isolated_points: Vec<P3>,
    /// `expected.singular_points`.
    pub singular_points: Vec<P3>,
    /// `cov_tol` (default `1e-3`).
    pub cov_tol: f64,
    /// `ill_cond`, when given.
    pub ill_cond: Option<f64>,
    /// `feature_size`, when given.
    pub feature_size: Option<f64>,
}

fn pair<T: Clone>(v: Vec<T>, what: &str) -> Result<[T; 2], HarnessError> {
    <[T; 2]>::try_from(v).map_err(|_| err(format!("`{what}` needs exactly 2 entries")))
}

fn list<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], HarnessError> {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| err(format!("missing list `{key}`")))
}

impl SsiCase {
    /// Parses a case document.
    pub fn parse(v: &Value) -> Result<Self, HarnessError> {
        let id = v
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| err("case without id"))?
            .to_string();
        let class = v
            .get("class")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let surfaces = pair(
            list(v, "surfaces")?
                .iter()
                .map(SurfaceSpec::parse)
                .collect::<Result<Vec<_>, _>>()?,
            "surfaces",
        )?;
        let truth = pair(
            list(v, "truth")?
                .iter()
                .map(Truth::parse)
                .collect::<Result<Vec<_>, _>>()?,
            "truth",
        )?;
        let exp = v.get("expected").ok_or_else(|| err("missing expected"))?;
        let poly = |key: &str, field: &str| -> Result<Vec<Vec<P3>>, HarnessError> {
            list(exp, key)?
                .iter()
                .map(|b| {
                    points(
                        b.get(field)
                            .ok_or_else(|| err(format!("missing {field}")))?,
                    )
                })
                .collect()
        };
        let pts = |key: &str| -> Result<Vec<P3>, HarnessError> {
            points(exp.get(key).ok_or_else(|| err(format!("missing {key}")))?)
        };
        Ok(Self {
            class,
            surfaces,
            truth,
            branches: poly("branches", "points")?,
            coincident_regions: poly("coincident_regions", "boundary")?,
            isolated_points: pts("isolated_points")?,
            singular_points: pts("singular_points")?,
            cov_tol: v.get("cov_tol").and_then(Value::as_f64).unwrap_or(1e-3),
            ill_cond: v.get("ill_cond").and_then(Value::as_f64),
            feature_size: v.get("feature_size").and_then(Value::as_f64),
            id,
        })
    }

    /// Reads and parses `path`.
    pub fn load(path: &Path) -> Result<Self, HarnessError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| err(format!("{}: {e}", path.display())))?;
        let v: Value =
            serde_json::from_str(&text).map_err(|e| err(format!("{}: {e}", path.display())))?;
        Self::parse(&v).map_err(|e| err(format!("{}: {e}", path.display())))
    }
}
