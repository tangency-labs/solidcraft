//! Real-solid public fillet API records. Native and Rust operate on actual bodies.
#[path = "prefix.rs"]
mod prefix;

use crate::{HarnessError, Output};
use serde::{Deserialize, Serialize};
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::topology::brep::p1;
use solidcore::topology::{edge, edgeuse::p1 as eu, face::p2 as face, loopuse, Brep, EdgeId};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::blend_fillet_executive_p3 as ffi;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Checkpoint {
    #[default]
    Full,
    Prepared,
    Corners,
    FirstTopology,
    FirstVertexGeometry,
    CornerVerticesComponent,
    AdjustedTopology,
    SolverGeometry,
    CornerEdges,
    TrimRails,
    ClassifiedRails,
    IntermediateFaces,
    StitchedFaces,
    ShrunkFaces,
    CreatedFilletBrep,
    MappedFilletEdges,
    InsertedIntersectionTopology,
    SelectedOriginalSurfaces,
    DeletedOriginalFaces,
    MergedFilletFaces,
}
impl Checkpoint {
    fn all_corners(self) -> bool {
        self.code() >= 5
    }
    fn has_offsets(self) -> bool {
        self.code() >= 4
    }
    fn status_count(self) -> usize {
        match self {
            Self::AdjustedTopology => 5,
            Self::SolverGeometry => 6,
            Self::CornerEdges => 7,
            Self::TrimRails => 8,
            Self::ClassifiedRails => 9,
            Self::IntermediateFaces => 10,
            Self::StitchedFaces => 11,
            Self::ShrunkFaces => 12,
            Self::FirstVertexGeometry => 4,
            _ => 3,
        }
    }
    fn code(self) -> u32 {
        match self {
            Self::Full => 0,
            Self::Prepared => 1,
            Self::Corners => 2,
            Self::FirstTopology => 3,
            Self::FirstVertexGeometry => 4,
            Self::CornerVerticesComponent => 5,
            Self::AdjustedTopology => 6,
            Self::SolverGeometry => 7,
            Self::CornerEdges => 8,
            Self::TrimRails => 9,
            Self::ClassifiedRails => 10,
            Self::IntermediateFaces => 11,
            Self::StitchedFaces => 12,
            Self::ShrunkFaces => 13,
            Self::CreatedFilletBrep => 14,
            Self::MappedFilletEdges => 15,
            Self::InsertedIntersectionTopology => 16,
            Self::SelectedOriginalSurfaces => 17,
            Self::DeletedOriginalFaces => 18,
            Self::MergedFilletFaces => 19,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RealSolid {
    #[serde(default)]
    checkpoint: Checkpoint,
    dimensions: [f64; 3],
    radius: f64,
    #[serde(default)]
    radius_end: Option<f64>,
    xsect: u32,
    continuity: u32,
    thumbweight: f64,
    edges: Vec<usize>,
}
impl RealSolid {
    fn validate(&self) -> Result<(), HarnessError> {
        if let Some(end) = self.radius_end {
            if self.checkpoint != Checkpoint::Full {
                return Err(HarnessError::new("radius_end requires the full checkpoint"));
            }
            ffi::validate_real_variable_solid(&self.wire(), end)
                .map_err(|e| HarnessError::new(e.to_string()))
        } else {
            ffi::validate_real_solid(&self.wire()).map_err(|e| HarnessError::new(e.to_string()))
        }
    }
    fn wire(&self) -> Vec<f64> {
        let mut out = self.dimensions.to_vec();
        out.extend([
            self.radius,
            f64::from(self.xsect),
            f64::from(self.continuity),
            self.thumbweight,
            self.edges.len() as f64,
        ]);
        out.extend(self.edges.iter().map(|v| *v as f64));
        out
    }
}

fn take_section<'a>(values: &mut &'a [f64]) -> Result<&'a [f64], HarnessError> {
    let (&length, tail) = values
        .split_first()
        .ok_or_else(|| HarnessError::new("missing section length"))?;
    if !length.is_finite() || length < 0.0 || length.fract() != 0.0 || length > tail.len() as f64 {
        return Err(HarnessError::new("invalid section length"));
    }
    let (section, rest) = tail.split_at(length as usize);
    *values = rest;
    Ok(section)
}
fn output(mut values: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    out.values.insert(
        "selected_edge_before".into(),
        take_section(&mut values)?.to_vec(),
    );
    let status = values
        .get(..4)
        .ok_or_else(|| HarnessError::new("missing status/counts"))?;
    if status.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
        return Err(HarnessError::new("invalid status/counts"));
    }
    out.ints.insert(
        "api_status_vertex_edge_face_counts".into(),
        status.iter().map(|v| *v as i64).collect(),
    );
    values = values
        .get(4..)
        .ok_or_else(|| HarnessError::new("missing status/counts"))?;
    let topology = take_section(&mut values)?;
    if topology.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
        return Err(HarnessError::new("invalid topology record"));
    }
    out.ints.insert(
        "result_topology".into(),
        topology.iter().map(|v| *v as i64).collect(),
    );
    out.values.insert(
        "result_geometry".into(),
        take_section(&mut values)?.to_vec(),
    );
    let (&mass_status, mass) = values
        .split_first()
        .ok_or_else(|| HarnessError::new("missing mass status"))?;
    if !mass_status.is_finite()
        || mass_status.fract() != 0.0
        || mass.len() != if mass_status == 0.0 { 12 } else { 0 }
    {
        return Err(HarnessError::new("invalid mass record"));
    }
    out.ints
        .insert("mass_status".into(), vec![mass_status as i64]);
    out.values.insert("mass_properties".into(), mass.to_vec());
    Ok(out)
}

pub(super) fn cpp(input: &RealSolid) -> Result<Output, HarnessError> {
    input.validate()?;
    if input.checkpoint != Checkpoint::Full {
        return prefix::cpp(input);
    }
    let values = match input.radius_end {
        Some(end) => ffi::real_variable_solid(&input.wire(), end),
        None => ffi::real_solid(&input.wire()),
    }
    .map_err(|e| HarnessError::new(e.to_string()))?;
    output(&values)
}

fn section(out: &mut Vec<f64>, data: &[f64]) {
    out.push(data.len() as f64);
    out.extend_from_slice(data);
}
fn point(out: &mut Vec<f64>, p: Vec3) {
    out.extend([p.x, p.y, p.z]);
}
fn status(error: SolidError) -> SolidResult<f64> {
    match error {
        SolidError::InvalidInput { .. } => Ok(1007.0),
        SolidError::Upstream { code, .. } => Ok(f64::from(code)),
        error => Err(error),
    }
}
fn sample(out: &mut Vec<f64>, value: SolidResult<Vec3>) -> SolidResult<()> {
    match value {
        Ok(p) => {
            out.push(0.0);
            point(out, p);
        }
        Err(error) => out.push(status(error)?),
    }
    Ok(())
}
fn label<T: PartialEq>(list: &[T], value: Option<T>) -> f64 {
    match value {
        None => -1.0,
        Some(value) => list
            .iter()
            .position(|other| *other == value)
            .map_or(-2.0, |index| index as f64),
    }
}
fn topology(b: &Brep, out: &mut Vec<f64>) -> SolidResult<()> {
    let vertices = p1::vertices(b, None)?;
    let edges = p1::edges(b, None)?;
    let faces = p1::faces(b, None)?;
    out.push(edges.len() as f64);
    for &id in &edges {
        let e = b.edge(id)?;
        out.push(e.curve.as_ref().map_or(0.0, |c| c.type_code() as f64));
        let ends = edge::vertices(b, id, None)?;
        out.push(ends.len() as f64);
        for end in ends {
            out.push(label(&vertices, end));
        }
        let uses = edge::edgeuses(b, id, None, None)?;
        out.push(uses.len() as f64);
        for use_ in uses {
            out.extend([
                label(&faces, eu::face(b, use_)?),
                f64::from(eu::orientation(b, use_)? as i32),
            ]);
        }
    }
    out.push(faces.len() as f64);
    for id in faces {
        let f = b.face(id)?;
        out.push(f.surface.as_ref().map_or(0.0, |s| s.type_code() as f64));
        let loops = face::loops(b, id, false, None)?;
        out.push(loops.len() as f64);
        for loop_ in loops {
            let uses = loopuse::loop_edgeuses(b, loop_)?;
            out.push(uses.len() as f64);
            for use_ in uses {
                let edgeuse = b.edgeuse(use_)?;
                out.extend([
                    label(&edges, edgeuse.edge),
                    f64::from(edgeuse.orientation as i32),
                ]);
            }
        }
    }
    Ok(())
}
fn run_rust(input: &RealSolid) -> SolidResult<Vec<f64>> {
    let mut k = Kernel::default();
    let [x, y, z] = input.dimensions;
    let mut body = solidcore::primitive::api_primitives::create_box(&mut k, Vec3::ZERO, x, y, z)?;
    let edges = p1::edges(&body, None)?;
    let mut selected = Vec::<EdgeId>::new();
    let mut before = Vec::new();
    for &index in &input.edges {
        let id = *edges.get(index).ok_or_else(|| {
            SolidError::invalid_input("harness::real_fillet", "selected edge out of range")
        })?;
        selected.push(id);
        let edge = body.edge(id)?;
        before.extend([
            index as f64,
            edge.interval.min(),
            edge.interval.max(),
            f64::from(edge.curve.is_some()),
        ]);
        if let Some(curve) = &edge.curve {
            before.push(curve.type_code() as f64);
            for fraction in [0.17, 0.37, 0.83] {
                sample(
                    &mut before,
                    curve.evaluate_point(&k, edge.interval.evaluate(fraction)),
                )?;
            }
        }
    }
    if input.checkpoint != Checkpoint::Full {
        return prefix::run(&mut k, &mut body, &selected, input, &before);
    }
    let result = apply(&mut k, &mut body, &selected, input);
    let result_status = match result {
        Ok(()) => 0.0,
        Err(error) => status(error)?,
    };
    let mut values = Vec::new();
    section(&mut values, &before);
    values.extend([
        result_status,
        p1::vertices(&body, None)?.len() as f64,
        p1::edges(&body, None)?.len() as f64,
        p1::faces(&body, None)?.len() as f64,
    ]);
    let mut topology_record = Vec::new();
    topology(&body, &mut topology_record)?;
    section(&mut values, &topology_record);
    let mut geometry = Vec::new();
    crate::util::primitive::append(&k, &body, &mut geometry)?;
    section(&mut values, &geometry);
    // The public API leaves no attribute on the body: the executive's fillet-edge
    // attribute and the temporary merge tags are destroyed with its session, as
    // upstream's executive destructor and SmObjsDelete do, and the box carries none.
    let store = solidcore::geom::attribute::AttributeStore::default();
    let attributes = solidcore::mass::integrator::properties::PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    match solidcore::mass::integrator::properties::brep_mass_properties(
        &k,
        &mut body,
        1e-4,
        1.0,
        Vec3::ZERO,
        &attributes,
    ) {
        Ok(mass) => {
            values.extend([
                0.0,
                mass.properties.area,
                mass.properties.volume,
                mass.properties.mass,
            ]);
            for vector in [mass.centroid, mass.inertia, mass.products] {
                point(&mut values, vector);
            }
        }
        Err(error) => values.push(status(error)?),
    }
    Ok(values)
}
fn apply(k: &mut Kernel, body: &mut Brep, edges: &[EdgeId], input: &RealSolid) -> SolidResult<()> {
    if let Some(end) = input.radius_end {
        return solidcore::blend::fillet_executive::api::variable_radius_fillet(
            k,
            body,
            edges,
            input.radius,
            end,
            input.xsect,
            input.continuity,
        );
    }
    solidcore::blend::fillet_executive::api::fillet_edges(
        k,
        body,
        edges,
        input.radius,
        input.xsect,
        input.continuity,
        input.thumbweight,
    )
}
/// The `TestSmFillets.cpp` examples 0-6, as the shim runs them (examples 2-4 on the
/// 10x10x10 box with its first four edges in `GetEdges` order: `SmApiFilletEdges(r=1,
/// circular)`, `SmApiFilletEdgesPerEdge(r = 0.5 + 0.25 i, circular)` and
/// `SmApiVariableRadiusFillet(0.5 -> 2.0, circular, G1)`). Returns the shim's
/// `[status, faces, edges, vertices]` record, or `None` for an unknown example.
pub(super) fn upstream_example(example: usize) -> Result<Option<Output>, HarnessError> {
    if !matches!(example, 0..=6) {
        return Ok(None);
    }
    let run = || -> SolidResult<Vec<f64>> {
        let mut k = Kernel::default();
        let (dx, dy, dz) = if example < 2 {
            (5.0, 10.0, 15.0)
        } else {
            (10.0, 10.0, 10.0)
        };
        let mut body =
            solidcore::primitive::api_primitives::create_box(&mut k, Vec3::ZERO, dx, dy, dz)?;
        let edges = p1::edges(&body, None)?;
        let selected: Vec<EdgeId> = edges.iter().copied().take(4).collect();
        let result = match example {
            0 => solidcore::blend::fillet_executive::api::circular_fillet(&mut k, &mut body, 0.5),
            1 => solidcore::blend::fillet_executive::api::chamfer_fillet(&mut k, &mut body, 0.5),
            2 => solidcore::blend::fillet_executive::api::fillet_edges(
                &mut k, &mut body, &selected, 1.0, 1, 1, 1.0,
            ),
            3 => {
                let radii: Vec<f64> = (0..selected.len()).map(|i| 0.5 + 0.25 * i as f64).collect();
                let types = vec![
                    solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType::Circular;
                    selected.len()
                ];
                solidcore::blend::fillet_executive::api::fillet_edges_per_edge(
                    &mut k, &mut body, &selected, &radii, &types, 1, 1.0,
                )
            }
            4 => solidcore::blend::fillet_executive::api::variable_radius_fillet(
                &mut k, &mut body, &selected, 0.5, 2.0, 1, 1,
            ),
            _ => {
                let bevel: Vec<_> = if example == 6 {
                    p1::vertices(&body, None)?.into_iter().take(1).collect()
                } else {
                    Vec::new()
                };
                solidcore::blend::fillet_executive::api::set_bevel_corners(
                    &mut k, &mut body, &edges, 1.0, &bevel,
                )
            }
        };
        let result_status = match result {
            Ok(()) => 0.0,
            Err(error) => status(error)?,
        };
        Ok(vec![
            result_status,
            p1::faces(&body, None)?.len() as f64,
            p1::edges(&body, None)?.len() as f64,
            p1::vertices(&body, None)?.len() as f64,
        ])
    };
    match run() {
        Ok(values) => {
            let mut out = Output::new();
            out.ints.insert(
                "api_status_counts".into(),
                values.into_iter().map(|v| v as i64).collect(),
            );
            Ok(Some(out))
        }
        Err(error) => match Output::unported(&error) {
            Some(deferred) => Ok(Some(deferred)),
            None => Err(HarnessError::new(error.to_string())),
        },
    }
}
pub(super) fn rust(input: &RealSolid) -> Result<Output, HarnessError> {
    input.validate()?;
    match run_rust(input) {
        Ok(values) => {
            if input.checkpoint == Checkpoint::Full {
                output(&values)
            } else {
                prefix::output(&values, input.checkpoint)
            }
        }
        Err(error) => match Output::unported(&error) {
            Some(deferred) => Ok(deferred),
            None => Err(HarnessError::new(error.to_string())),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::output;

    fn variable_input() -> super::RealSolid {
        serde_json::from_value(serde_json::json!({
            "dimensions": [10,20,30], "radius": 0.5, "radius_end": 2.0,
            "xsect": 1, "continuity": 1, "thumbweight": 1, "edges": [2]
        }))
        .expect("valid fixture schema")
    }

    #[test]
    fn variable_radius_preserves_old_wire_and_rejects_prefix_before_execution() {
        let mut input = variable_input();
        let wire = input.wire();
        assert_eq!(wire, [10.0, 20.0, 30.0, 0.5, 1.0, 1.0, 1.0, 1.0, 2.0]);
        assert!(input.validate().is_ok());
        input.radius_end = None;
        assert_eq!(input.wire(), wire);
        input.radius_end = Some(2.0);
        input.checkpoint = super::Checkpoint::IntermediateFaces;
        assert!(super::cpp(&input).is_err());
        assert!(super::rust(&input).is_err());
    }

    #[test]
    fn variable_radius_rejects_ignored_thumbweight_and_nonfinite_endpoint() {
        let mut input = variable_input();
        input.thumbweight = 1.5;
        assert!(super::cpp(&input).is_err());
        assert!(super::rust(&input).is_err());
        input.thumbweight = 1.0;
        input.radius_end = Some(f64::NAN);
        assert!(super::cpp(&input).is_err());
        assert!(super::rust(&input).is_err());
    }

    #[test]
    fn failed_mass_has_status_without_substitute_values() {
        let record = [0.0, 0.0, 10.0, 15.0, 7.0, 0.0, 0.0, 1007.0];
        let result = output(&record).expect("valid failed mass record");
        assert_eq!(result.ints["mass_status"], [1007]);
        assert!(result.values["mass_properties"].is_empty());
        let mut fabricated = record.to_vec();
        fabricated.extend([0.0; 12]);
        assert!(output(&fabricated).is_err());
    }

    #[test]
    fn successful_mass_requires_all_twelve_values() {
        let mut record = vec![0.0, 0.0, 10.0, 15.0, 7.0, 0.0, 0.0, 0.0];
        assert!(output(&record).is_err());
        record.extend([1.0; 12]);
        assert_eq!(
            output(&record).expect("complete mass").values["mass_properties"].len(),
            12
        );
        record.push(1.0);
        assert!(output(&record).is_err());
    }

    #[test]
    fn topology_and_statuses_remain_exact_integer_records() {
        let record = [0.0, 0.0, 10.0, 15.0, 7.0, 2.0, 14010.0, -1.0, 0.0, 1007.0];
        let result = output(&record).expect("integer topology");
        assert_eq!(result.ints["result_topology"], [14010, -1]);
        let mut invalid = record;
        invalid[6] = 14010.5;
        assert!(output(&invalid).is_err());
    }
}
