//! Issue #1462: caller attributes through the public fillet APIs. Each side tags its own
//! fresh body (the C++ shim with `SmLongAttribute`s, the Rust side with
//! `Brep::add_attribute`): every face (`GetFaces` order) gets id = value =
//! `ATTRIBUTE_BASE + index`, every edge (`GetEdges` order) `ATTRIBUTE_BASE + 500 + index`.
//! Then `SmApiFilletEdges`, `SmApiFilletEdgesPerEdge`, `SmApiVariableRadiusFillet`,
//! `SmApiCircularFillet` or `SmApiChamferFillet` runs, and the record compares the API
//! status, the body's counts and, per face (edge), its area (length) with the set of
//! attributes it carries, rows sorted by attributes.
//!
//! C++: `solidcore_ffi::units::blend_fillet_executive_p3::attributed`.

use super::end_to_end::status;
use crate::{HarnessError, Output};
use serde::{Deserialize, Serialize};
use solidcore::blend::fillet_executive::api;
use solidcore::blend::fillet_standard_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{edge_length, face_area};
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives as primitives;
use solidcore::topology::brep::p1;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Result as SolidResult, SolidError};
use solidcore_ffi::units::blend_fillet_executive_p3 as ffi;
use solidcore_ffi::units::boolean_api_brep as rows;

/// The public fillet entry point.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Api {
    /// `SmApiFilletEdges`.
    FilletEdges,
    /// `SmApiFilletEdgesPerEdge`, every edge with `radius` and the generator `xsect`.
    PerEdge,
    /// `SmApiVariableRadiusFillet` from `radius` to `radius_end`.
    VariableRadius,
    /// `SmApiCircularFillet` (every blendable edge).
    Circular,
    /// `SmApiChamferFillet` (every blendable edge).
    Chamfer,
}

/// The body: `SmApiCreateBox`, `SmApiCreateCylinder`, the union / difference of two
/// boxes, or the union of a box (`a`: origin, size) with a cylinder (base centre
/// `origin`, `radius`, `height`; #1485).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Body {
    Box { origin: [f64; 3], size: [f64; 3] },
    Cylinder { origin: [f64; 3], radius: f64, height: f64 },
    BoxUnion { a: [[f64; 3]; 2], b: [[f64; 3]; 2] },
    BoxDifference { a: [[f64; 3]; 2], b: [[f64; 3]; 2] },
    BoxCylinderUnion { a: [[f64; 3]; 2], origin: [f64; 3], radius: f64, height: f64 },
}

fn one() -> f64 {
    1.0
}
fn circular() -> u32 {
    1
}

/// One attributed fillet case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AttributedFillet {
    api: Api,
    body: Body,
    /// `SmAttributeBehaviorType` of every tag (0 copy).
    #[serde(default)]
    behavior: u8,
    radius: f64,
    #[serde(default)]
    radius_end: f64,
    /// Apis `fillet_edges` / `variable_radius`: 0 linear, 1 circular, 2 blend curve;
    /// `per_edge`: the `SmFilletSurfaceGeneratorType` (1 linear, 2 circular, 3 blend).
    #[serde(default = "circular")]
    xsect: u32,
    #[serde(default = "circular")]
    continuity: u32,
    #[serde(default = "one")]
    thumbweight: f64,
    /// `GetEdges` indices (none for `circular` / `chamfer`).
    #[serde(default)]
    edges: Vec<usize>,
}

impl AttributedFillet {
    fn wire(&self) -> Vec<f64> {
        let api = match self.api {
            Api::FilletEdges => 0.0,
            Api::PerEdge => 1.0,
            Api::VariableRadius => 2.0,
            Api::Circular => 3.0,
            Api::Chamfer => 4.0,
        };
        let mut out = vec![api, f64::from(self.behavior)];
        let mut params = Vec::new();
        let kind = match &self.body {
            Body::Box { origin, size } => {
                params.extend(origin.iter().chain(size));
                0.0
            }
            Body::Cylinder {
                origin,
                radius,
                height,
            } => {
                params.extend(origin);
                params.extend([*radius, *height]);
                2.0
            }
            Body::BoxUnion { a, b } | Body::BoxDifference { a, b } => {
                params.extend(a.iter().chain(b).flatten());
                if matches!(self.body, Body::BoxUnion { .. }) {
                    3.0
                } else {
                    4.0
                }
            }
            Body::BoxCylinderUnion {
                a,
                origin,
                radius,
                height,
            } => {
                params.extend(a.iter().flatten());
                params.extend(origin);
                params.extend([*radius, *height]);
                5.0
            }
        };
        params.resize(ffi::SOURCE_PARAMS, 0.0);
        out.push(kind);
        out.extend(params);
        out.extend([
            self.radius,
            self.radius_end,
            f64::from(self.xsect),
            f64::from(self.continuity),
            self.thumbweight,
            self.edges.len() as f64,
        ]);
        out.extend(self.edges.iter().map(|&i| i as f64));
        out
    }
}

/// `status V E F`, then the face rows and the edge rows.
fn output(values: &[f64]) -> Result<Output, HarnessError> {
    let (head, rest) = values
        .split_at_checked(4)
        .ok_or_else(|| HarnessError::new("missing status and counts"))?;
    if head.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
        return Err(HarnessError::new("invalid status/counts"));
    }
    let mut out = Output::new();
    out.ints.insert(
        "api_status_vertex_edge_face_counts".into(),
        head.iter().map(|v| *v as i64).collect(),
    );
    Ok(out.values("attributes", rest.to_vec()))
}

/// The C++ record of `input`.
pub(super) fn cpp(input: &AttributedFillet) -> Result<Output, HarnessError> {
    let (values, step) = super::fresh_cpp(|| ffi::attributed(&input.wire()))
        .map_err(|e| HarnessError::new(e.to_string()))?;
    Ok(super::with_point_step(output(&values)?, step))
}

fn build(k: &mut Kernel, body: &Body) -> SolidResult<Brep> {
    let boxed = |k: &mut Kernel, b: &[[f64; 3]; 2]| {
        let [origin, [x, y, z]] = *b;
        primitives::create_box(k, Vec3::from(origin), x, y, z)
    };
    match body {
        Body::Box { origin, size } => {
            let [x, y, z] = *size;
            primitives::create_box(k, Vec3::from(*origin), x, y, z)
        }
        Body::Cylinder {
            origin,
            radius,
            height,
        } => primitives::create_cylinder(k, Vec3::from(*origin), *radius, *height),
        Body::BoxUnion { a, b } => {
            let (a, b) = (boxed(k, a)?, boxed(k, b)?);
            solidcore::boolean::api_brep::boolean_union(k, a, b)
        }
        Body::BoxDifference { a, b } => {
            let (a, b) = (boxed(k, a)?, boxed(k, b)?);
            solidcore::boolean::api_brep::boolean_difference(k, a, b)
        }
        Body::BoxCylinderUnion {
            a,
            origin,
            radius,
            height,
        } => {
            let a = boxed(k, a)?;
            let b = primitives::create_cylinder(k, Vec3::from(*origin), *radius, *height)?;
            solidcore::boolean::api_brep::boolean_union(k, a, b)
        }
    }
}

/// The shim's `tag`.
fn tag(b: &mut Brep, behavior: Behavior) -> SolidResult<()> {
    let faces = p1::faces(b, None)?;
    let edges = p1::edges(b, None)?;
    let tagged = faces
        .into_iter()
        .enumerate()
        .map(|(i, f)| (EntityRef::Face(f), i as u64))
        .chain(
            edges
                .into_iter()
                .enumerate()
                .map(|(i, e)| (EntityRef::Edge(e), 500 + i as u64)),
        );
    for (entity, offset) in tagged {
        let id = rows::ATTRIBUTE_BASE + offset;
        b.add_attribute(entity, Attribute::long(id, id as i64, behavior))?;
    }
    Ok(())
}

/// The shim's `row`: `measure n` then per attribute `id behavior value users`, ascending.
fn row(b: &Brep, entity: EntityRef, measure: f64) -> Vec<f64> {
    let mut entries: Vec<[f64; 4]> = b
        .entity_attributes(entity)
        .into_iter()
        .map(|(_, a)| {
            [
                a.attribute_id() as f64,
                f64::from(a.behavior() as u8),
                a.long_value().map_or(0.0, |v| v as f64),
                a.users().len() as f64,
            ]
        })
        .collect();
    entries.sort_by(|x, y| {
        x.iter()
            .zip(y)
            .map(|(p, q)| p.total_cmp(q))
            .find(|o| o.is_ne())
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    let mut out = vec![measure, entries.len() as f64];
    out.extend(entries.into_iter().flatten());
    out
}

/// The shim's `put_attributes`.
fn put_attributes(k: &Kernel, o: &mut Vec<f64>, b: &mut Brep) -> SolidResult<()> {
    let measured = |r: SolidResult<f64>| -> SolidResult<f64> {
        match r {
            Ok(v) => Ok(v),
            Err(e) if e.is_unported() => Err(e),
            Err(e) => Ok(-status(e)?),
        }
    };
    let mut face_rows = Vec::new();
    for f in p1::faces(b, None)? {
        let area = measured(face_area(k, b, f, rows::AREA_ACCURACY))?;
        face_rows.push(row(b, EntityRef::Face(f), area));
    }
    rows::sort_rows(&mut face_rows);
    o.push(face_rows.len() as f64);
    o.extend(face_rows.into_iter().flatten());
    let mut edge_rows = Vec::new();
    for e in p1::edges(b, None)? {
        let length = measured(edge_length(k, b, e, rows::LENGTH_ACCURACY))?;
        edge_rows.push(row(b, EntityRef::Edge(e), length));
    }
    rows::sort_rows(&mut edge_rows);
    o.push(edge_rows.len() as f64);
    o.extend(edge_rows.into_iter().flatten());
    Ok(())
}

fn generator(code: u32) -> SolidResult<FilletSurfaceGeneratorType> {
    Ok(match code {
        1 => FilletSurfaceGeneratorType::Linear,
        2 => FilletSurfaceGeneratorType::Circular,
        3 => FilletSurfaceGeneratorType::BlendCurve,
        _ => {
            return Err(SolidError::invalid_input(
                "harness::attributed_fillet",
                "generator type",
            ))
        }
    })
}

/// The Rust side of `input`: the body is built and tagged as the shim does, then the
/// `solidcore::blend::fillet_executive::api` entry point runs on it.
pub(super) fn rust(input: &AttributedFillet) -> Result<Output, HarnessError> {
    let wire = input.wire();
    ffi::validate_attributed(&wire).map_err(|e| HarnessError::new(e.to_string()))?;
    let mut k = Kernel::default();
    let mut run = || -> SolidResult<Vec<f64>> {
        let behavior = Behavior::from_code(i32::from(input.behavior))?;
        let mut body = build(&mut k, &input.body)?;
        tag(&mut body, behavior)?;
        let all = p1::edges(&body, None)?;
        let edges = input
            .edges
            .iter()
            .map(|&i| {
                all.get(i).copied().ok_or_else(|| {
                    SolidError::invalid_input("harness::attributed_fillet", "edge index")
                })
            })
            .collect::<SolidResult<Vec<_>>>()?;
        let k = &mut k;
        let result = match input.api {
            Api::FilletEdges => api::fillet_edges(
                k,
                &mut body,
                &edges,
                input.radius,
                input.xsect,
                input.continuity,
                input.thumbweight,
            ),
            Api::PerEdge => {
                let radii = vec![input.radius; edges.len()];
                let types = vec![generator(input.xsect)?; edges.len()];
                api::fillet_edges_per_edge(
                    k,
                    &mut body,
                    &edges,
                    &radii,
                    &types,
                    input.continuity,
                    input.thumbweight,
                )
            }
            Api::VariableRadius => api::variable_radius_fillet(
                k,
                &mut body,
                &edges,
                input.radius,
                input.radius_end,
                input.xsect,
                input.continuity,
            ),
            Api::Circular => api::circular_fillet(k, &mut body, input.radius),
            Api::Chamfer => api::chamfer_fillet(k, &mut body, input.radius),
        };
        let mut values = vec![match result {
            Ok(()) => 0.0,
            Err(error) => status(error)?,
        }];
        values.extend([
            p1::vertices(&body, None)?.len() as f64,
            p1::edges(&body, None)?.len() as f64,
            p1::faces(&body, None)?.len() as f64,
        ]);
        put_attributes(k, &mut values, &mut body)?;
        Ok(values)
    };
    let result = run();
    let step = super::rust_point_step(&k);
    match result {
        Ok(values) => Ok(super::with_point_step(output(&values)?, step)),
        Err(error) => {
            Output::unported(&error).ok_or_else(|| HarnessError::new(error.to_string()))
        }
    }
}
