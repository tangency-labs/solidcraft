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
use super::contract;

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

/// One box of a two-box Boolean source: `SmApiCreateBox(origin, size)`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BoxSpec {
    origin: [f64; 3],
    size: [f64; 3],
}

/// A non-box body the fillet runs on, built the same way on both sides.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Source {
    /// An upstream ASCII `.smb` file: `SmBrep::ReadFromFile` in the shim, and the
    /// Rust reader (`topology::brep::p7::read_from_file`) on the Rust side.
    File {
        /// Absolute path (`{"$upstream_file": ..}` in the fixture).
        path: String,
    },
    /// `SmApiCreateCylinder(origin, radius, height)`.
    Cylinder {
        /// Centre of the base.
        origin: [f64; 3],
        /// Radius.
        radius: f64,
        /// Height along Z.
        height: f64,
    },
    /// `SmApiBooleanUnion` of two boxes.
    BoxUnion {
        /// First operand (the result).
        a: BoxSpec,
        /// Second operand (consumed).
        b: BoxSpec,
    },
    /// `SmApiBooleanDifference` of two boxes.
    BoxDifference {
        /// First operand (the result).
        a: BoxSpec,
        /// Second operand (consumed).
        b: BoxSpec,
    },
}
impl Source {
    /// Source kind code and the shim's fixed-length parameter block.
    pub(super) fn wire(&self) -> (f64, [f64; contract::SOURCE_PARAMS]) {
        let mut p = [0.0; contract::SOURCE_PARAMS];
        let boxes = |p: &mut [f64; contract::SOURCE_PARAMS], a: &BoxSpec, b: &BoxSpec| {
            for (slot, v) in p.iter_mut().zip(
                a.origin
                    .iter()
                    .chain(&a.size)
                    .chain(&b.origin)
                    .chain(&b.size),
            ) {
                *slot = *v;
            }
        };
        match self {
            Self::File { .. } => (1.0, p),
            Self::Cylinder {
                origin,
                radius,
                height,
            } => {
                for (slot, v) in p.iter_mut().zip(origin.iter().chain([radius, height])) {
                    *slot = *v;
                }
                (2.0, p)
            }
            Self::BoxUnion { a, b } => {
                boxes(&mut p, a, b);
                (3.0, p)
            }
            Self::BoxDifference { a, b } => {
                boxes(&mut p, a, b);
                (4.0, p)
            }
        }
    }
    pub(super) fn path(&self) -> Option<&str> {
        match self {
            Self::File { path } => Some(path),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CornerSetback {
    vertex: [f64; 3],
    distance: f64,
}
impl CornerSetback {
    fn wire(self) -> [f64; 4] {
        let [x, y, z] = self.vertex;
        [x, y, z, self.distance]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RealSolid {
    #[serde(default)]
    checkpoint: Checkpoint,
    /// The box of the default source (`SmApiCreateBox` at the origin).
    #[serde(default)]
    dimensions: Option<[f64; 3]>,
    /// A non-box body instead of the box (full checkpoint only).
    #[serde(default)]
    source: Option<Source>,
    radius: f64,
    #[serde(default)]
    radius_end: Option<f64>,
    /// A B-spline radius law (`SmBSplineFilletLaw`) instead of `radius_end`'s linear
    /// one; `radius` is still `SetFilletParameters`' radius (full checkpoint, box only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    radius_law: Option<RadiusLaw>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    corner_setback: Option<CornerSetback>,
    xsect: u32,
    continuity: u32,
    thumbweight: f64,
    /// Selected edges by index into `GetEdges`; empty with `all_edges`.
    #[serde(default)]
    edges: Vec<usize>,
    /// Select every edge of the body (non-box sources only).
    #[serde(default)]
    all_edges: bool,
    /// Fillet with the executive's piecewise merge (`SetDoPiecewiseMerge(TRUE)` after
    /// `SetFilletParameters`) instead of `SmApiFilletEdges`' default merge (full
    /// checkpoint only).
    #[serde(default)]
    piecewise_merge: bool,
    /// Fillet with an `SmMakeCurveBlendSIH(step back factor)` self-intersection
    /// handler installed on `SmApiFilletEdges`' executive, so every `FlushCurve`
    /// runs `SmoothOutIntersection` (full checkpoint only; not with
    /// `piecewise_merge`).
    #[serde(default)]
    curve_blend_sih: Option<f64>,
    /// tangency-labs/solidcore#460: edges chosen by geometry instead of `edges` (box
    /// only).
    #[serde(default)]
    select: Option<EdgeSelect>,
    /// tangency-labs/solidcore#460: shell the filleted box (`SmApiShellBrepFull`).
    #[serde(default)]
    shell: Option<ShellAfter>,
}

/// A radius law curve for `SmBSplineCurve::CreateCanonical(dim 3, degree, (r, 0, 0)
/// control points, unique knots with multiplicities)` and the edge map of
/// `SmBSplineFilletLaw(pCurve, crEdgeMap)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RadiusLaw {
    degree: u32,
    /// Control-point radii (the curve's x coordinates).
    radii: Vec<f64>,
    /// Unique knots, strictly increasing.
    knots: Vec<f64>,
    /// One multiplicity per unique knot.
    multiplicities: Vec<u32>,
    /// The part of the law's parameter range mapped onto the edge.
    #[serde(default = "unit_edge_map")]
    edge_map: [f64; 2],
}

const fn unit_edge_map() -> [f64; 2] {
    [0.0, 1.0]
}

impl RadiusLaw {
    /// The law block of [`ffi::real_law_solid`].
    fn wire(&self) -> Vec<f64> {
        let mut out = vec![f64::from(self.degree), self.radii.len() as f64];
        out.extend(&self.radii);
        out.push(self.knots.len() as f64);
        out.extend(&self.knots);
        out.extend(self.multiplicities.iter().map(|m| f64::from(*m)));
        out.extend(self.edge_map);
        out
    }

    /// The law curve and `SmBSplineFilletLaw(pCurve, crEdgeMap)`.
    fn law(&self) -> SolidResult<solidcore::blend::fillet_standard_solver::p1::FilletLaw> {
        use solidcore::nurbs::b_spline_curve::p1::{
            BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
        };
        let control: Vec<Vec3> = self.radii.iter().map(|r| Vec3::new(*r, 0.0, 0.0)).collect();
        let multiplicities: Vec<usize> =
            self.multiplicities.iter().map(|m| *m as usize).collect();
        let curve = BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: self.degree as usize,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &multiplicities,
            knots: &self.knots,
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })?;
        let [min, max] = self.edge_map;
        Ok(solidcore::blend::fillet_standard_solver::p1::FilletLaw::bspline(
            curve,
            // `SmExtent1d(dMin, dMax)` swaps reversed ends, as the shim's does.
            &solidcore::geom::extent1d::Extent1d::from_min_max(min, max),
        ))
    }
}

/// Edges a case picks by geometry on the constructed box (the same owning-list order
/// on both sides).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EdgeSelect {
    /// The four edges parallel to Z.
    Vertical,
    /// The first edge parallel to Z.
    FirstVertical,
}

/// The shell after the fillet: `SmApiShellBrepFull(distance, extended convex edges,
/// merged results, no offset solid)`, closed or without the plane faces at the box's
/// height.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct ShellAfter {
    distance: f64,
    #[serde(default)]
    open_top: bool,
}
impl RealSolid {
    fn fillet_wire(&self) -> Vec<f64> {
        let edges = self.edge_indices().unwrap_or_default();
        let mut out = vec![
            self.radius,
            f64::from(self.xsect),
            f64::from(self.continuity),
            self.thumbweight,
            edges.len() as f64,
        ];
        out.extend(edges.iter().map(|v| *v as f64));
        out
    }
    /// The selected owning-list edge indices: `edges`, or those `select` picks on the
    /// box (empty when the box cannot be built; the shim then rejects the wire).
    fn edge_indices(&self) -> SolidResult<Vec<usize>> {
        let Some(select) = self.select else {
            return Ok(self.edges.clone());
        };
        let mut k = crate::runner_kernel();
        let [x, y, z] = self.dimensions.unwrap_or_default();
        let body = solidcore::primitive::api_primitives::create_box(&mut k, Vec3::ZERO, x, y, z)?;
        let mut picked = Vec::new();
        for (index, id) in p1::edges(&body, None)?.into_iter().enumerate() {
            let edge = body.edge(id)?;
            let Some(curve) = edge.curve.as_ref() else {
                continue;
            };
            let p = curve.evaluate_point(&k, edge.interval.min())?;
            let q = curve.evaluate_point(&k, edge.interval.max())?;
            if (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9 {
                picked.push(index);
            }
        }
        if select == EdgeSelect::FirstVertical {
            picked.truncate(1);
        }
        Ok(picked)
    }
    /// The box wire: dimensions then the fillet wire.
    fn wire(&self) -> Vec<f64> {
        let mut out = self.dimensions.unwrap_or_default().to_vec();
        out.extend(self.fillet_wire());
        out
    }
    /// The source wire of [`ffi::real_source`]: kind, parameters, fillet wire.
    fn source_wire(&self, source: &Source) -> Vec<f64> {
        let (kind, params) = source.wire();
        let mut out = vec![kind];
        out.extend(params);
        out.extend(self.fillet_wire());
        out
    }
    /// Rejects mixed or incomplete forms (a box needs `dimensions`; a source no
    /// `dimensions`, the full checkpoint, and exactly one of `edges` / `all_edges`).
    fn check_form(&self) -> Result<(), HarnessError> {
        if self
            .corner_setback
            .is_some_and(|setback| setback.wire().iter().any(|value| !value.is_finite()))
        {
            return Err(HarnessError::new("non-finite corner setback fixture"));
        }
        if self.corner_setback.is_some()
            && (self.checkpoint != Checkpoint::Full
                || self.source.is_some()
                || self.radius_end.is_some()
                || self.piecewise_merge
                || self.curve_blend_sih.is_some()
                || self.shell.is_some())
        {
            return Err(HarnessError::new(
                "corner setback requires the full constant-radius box without merge options",
            ));
        }
        if (self.select.is_some() || self.shell.is_some())
            && (self.checkpoint != Checkpoint::Full
                || self.source.is_some()
                || self.radius_end.is_some()
                || self.piecewise_merge
                || self.curve_blend_sih.is_some())
        {
            return Err(HarnessError::new(
                "select and shell take the full checkpoint on a box without other options",
            ));
        }
        if self.select.is_some() && !self.edges.is_empty() {
            return Err(HarnessError::new("select replaces edges"));
        }
        if self.radius_law.is_some()
            && (self.radius_end.is_some()
                || self.corner_setback.is_some()
                || self.select.is_some()
                || self.shell.is_some())
        {
            return Err(HarnessError::new(
                "radius_law excludes radius_end, corner setback, select and shell",
            ));
        }
        if (self.radius_end.is_some() || self.radius_law.is_some())
            && (self.checkpoint != Checkpoint::Full
                || self.piecewise_merge
                || self.curve_blend_sih.is_some())
        {
            return Err(HarnessError::new(
                "radius_end and radius_law require the full checkpoint without merge options",
            ));
        }
        if self.radius_law.is_some() && self.source.is_some() {
            return Err(HarnessError::new("radius_law requires the box"));
        }
        if self.piecewise_merge && self.checkpoint != Checkpoint::Full {
            return Err(HarnessError::new(
                "piecewise merge needs the full checkpoint",
            ));
        }
        if self.curve_blend_sih.is_some()
            && (self.piecewise_merge || self.checkpoint != Checkpoint::Full)
        {
            return Err(HarnessError::new(
                "curve-blend handler needs the full checkpoint without piecewise merge",
            ));
        }
        match (&self.source, self.dimensions) {
            (None, Some(_)) if !self.all_edges => Ok(()),
            // a file's body at a checkpoint (the shim's `real_prefix_file`): selected edges
            // and the box form's options only
            (Some(Source::File { .. }), None)
                if self.checkpoint != Checkpoint::Full
                    && !self.all_edges
                    && !self.edges.is_empty()
                    && self.radius_end.is_none()
                    && self.select.is_none()
                    && self.shell.is_none() =>
            {
                Ok(())
            }
            (Some(_), None)
                if self.checkpoint == Checkpoint::Full
                    && self.all_edges == self.edges.is_empty() =>
            {
                Ok(())
            }
            _ => Err(HarnessError::new("invalid real-solid form")),
        }
    }
    /// [`Self::check_form`], then the shim's wire validation for the chosen form.
    fn validate(&self) -> Result<(), HarnessError> {
        self.check_form()?;
        if let Some(setback) = self.corner_setback {
            return contract::validate_real_solid_setback(&self.wire(), setback.wire())
                .map_err(|e| HarnessError::new(e.to_string()));
        }
        if let Some(law) = &self.radius_law {
            return contract::validate_real_law_solid(&self.wire(), &law.wire())
                .map_err(|e| HarnessError::new(e.to_string()));
        }
        match (&self.source, self.radius_end) {
            (Some(source), Some(end)) => contract::validate_real_variable_source(
                &self.source_wire(source),
                source.path().is_some(),
                end,
            ),
            (Some(source), None) => {
                contract::validate_real_source(&self.source_wire(source), source.path().is_some())
            }
            (None, Some(end)) => contract::validate_real_variable_solid(&self.wire(), end),
            (None, None) => contract::validate_real_solid(&self.wire()),
        }
        .map_err(|e| HarnessError::new(e.to_string()))
    }
}

/// `TestSmSurfaceSurfaceFillet`'s set-up: `SmApiSurfaceSurfaceFillet` on two
/// `SmApiCreateSurfaceFromCornerPoints` surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SurfaceSurface {
    /// First surface corners `u0v0, u1v0, u0v1, u1v1`.
    first: [[f64; 3]; 4],
    /// Second surface corners.
    second: [[f64; 3]; 4],
    radius1: f64,
    radius2: f64,
    tolerance: f64,
    xsect: u32,
    mirror: bool,
    complement: bool,
}
impl SurfaceSurface {
    fn wire(&self) -> Vec<f64> {
        let mut out: Vec<f64> = self
            .first
            .iter()
            .chain(&self.second)
            .flatten()
            .copied()
            .collect();
        out.extend([
            self.radius1,
            self.radius2,
            self.tolerance,
            f64::from(self.xsect),
            f64::from(u8::from(self.mirror)),
            f64::from(u8::from(self.complement)),
        ]);
        out
    }
}

pub(super) fn take_section<'a>(values: &mut &'a [f64]) -> Result<&'a [f64], HarnessError> {
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
pub(super) fn output(mut values: &[f64]) -> Result<Output, HarnessError> {
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

pub(super) fn section(out: &mut Vec<f64>, data: &[f64]) {
    out.push(data.len() as f64);
    out.extend_from_slice(data);
}
fn point(out: &mut Vec<f64>, p: Vec3) {
    out.extend([p.x, p.y, p.z]);
}
pub(super) fn status(error: SolidError) -> SolidResult<f64> {
    match error {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => Ok(1007.0),
        // every error carrying its exact C++ status (an upstream status, a typed refusal of
        // an edge, a corner or a size, a failure in solidcore's terms) is that status
        error => match error.sm_status() {
            Some(code) => Ok(f64::from(code)),
            None => Err(error),
        },
    }
}
pub(super) fn sample(out: &mut Vec<f64>, value: SolidResult<Vec3>) -> SolidResult<()> {
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
pub(super) fn topology(b: &Brep, out: &mut Vec<f64>) -> SolidResult<()> {
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
/// The body the full-checkpoint case `name` ends with, as [`rust`] makes it before its
/// record (tests only, #18195).
#[cfg(test)]
pub(crate) fn result_body_of(k: &mut Kernel, name: &str) -> Brep {
    let case = crate::case::regression_case(&super::Runner, name);
    let inputs: super::Inputs = case.parse().expect("inputs");
    let input = inputs.real_solid.expect("a real_solid case");
    assert!(input.shell.is_none() && input.checkpoint == Checkpoint::Full, "{name}");
    let mut body = build(k, &input).expect("fixture").expect("body");
    let Ok((selected, _)) = select_edges(k, &body, &input) else {
        panic!("{name}: the selected edges");
    };
    // (a refused fillet leaves the body the record measures, as `run_rust` records it)
    let Ok(_) = fillet_selected(k, &mut body, &selected, &input) else {
        panic!("{name}: the fillet's fixture");
    };
    body
}
/// The Rust body of `input`'s source (the box by default).
fn build(k: &mut Kernel, input: &RealSolid) -> Result<SolidResult<Brep>, HarnessError> {
    use solidcore::primitive::api_primitives as primitives;
    Ok(match &input.source {
        None => {
            let [x, y, z] = input.dimensions.unwrap_or_default();
            primitives::create_box(k, Vec3::ZERO, x, y, z)
        }
        Some(source) => build_source(k, source)?,
    })
}
/// The Rust body of a non-box `source` (the shim's `build_source`).
pub(super) fn build_source(
    k: &mut Kernel,
    source: &Source,
) -> Result<SolidResult<Brep>, HarnessError> {
    use solidcore::primitive::api_primitives as primitives;
    let boxed = |k: &mut Kernel, b: &BoxSpec| {
        let [x, y, z] = b.size;
        primitives::create_box(k, Vec3::from(b.origin), x, y, z)
    };
    Ok(match source {
        Source::File { path } => crate::own_reader::read_file(k, path)?,
        Source::Cylinder {
            origin,
            radius,
            height,
        } => primitives::create_cylinder(k, Vec3::from(*origin), *radius, *height),
        Source::BoxUnion { a, b } => boxed(k, a).and_then(|first| {
            let second = boxed(k, b)?;
            solidcore::boolean::api_brep::boolean_union(k, first, second)
        }),
        Source::BoxDifference { a, b } => boxed(k, a).and_then(|first| {
            let second = boxed(k, b)?;
            solidcore::boolean::api_brep::boolean_difference(k, first, second)
        }),
    })
}
enum RealSolidRunError {
    Fixture(HarnessError),
    Kernel(SolidError),
}
impl From<SolidError> for RealSolidRunError {
    fn from(error: SolidError) -> Self {
        Self::Kernel(error)
    }
}

/// One generated fillet surface's type code, and its net's bound and the distance proved
/// between the net and the definition when it is a blend by its definition.
pub(super) type Certificate = (f64, Option<(f64, f64)>);

/// The edges `input` selects on `body`, with the record of each before the fillet.
fn select_edges(
    k: &Kernel,
    body: &Brep,
    input: &RealSolid,
) -> Result<(Vec<EdgeId>, Vec<f64>), RealSolidRunError> {
    let edges = p1::edges(body, None)?;
    let mut selected = Vec::<EdgeId>::new();
    let mut before = Vec::new();
    let all: Vec<usize> = (0..edges.len()).collect();
    let picked = input.edge_indices()?;
    let indices = if input.all_edges { &all } else { &picked };
    for &index in indices {
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
                    curve.evaluate_point(k, edge.interval.evaluate(fraction)),
                )?;
            }
        }
    }
    Ok((selected, before))
}

/// The whole fillet of `selected` on `body` that `input` asks for: the kernel's result,
/// or a fixture error.
fn fillet_selected(
    k: &mut Kernel,
    body: &mut Brep,
    selected: &[EdgeId],
    input: &RealSolid,
) -> Result<SolidResult<()>, RealSolidRunError> {
    Ok(if let Some(setback) = input.corner_setback {
        match setback_fillet_edges(k, body, selected, input, setback) {
            Ok(()) => Ok(()),
            Err(RealSolidRunError::Kernel(error)) => Err(error),
            Err(error @ RealSolidRunError::Fixture(_)) => return Err(error),
        }
    } else {
        apply(k, body, selected, input)
    })
}

fn run_rust(
    k: &mut Kernel,
    body: SolidResult<Brep>,
    input: &RealSolid,
) -> Result<Vec<f64>, RealSolidRunError> {
    let mut body = body?;
    let (selected, before) = select_edges(k, &body, input)?;
    if input.checkpoint != Checkpoint::Full {
        return prefix::run(k, &mut body, &selected, input, &before, None).map_err(Into::into);
    }
    let result = fillet_selected(k, &mut body, &selected, input)?;
    let result_status = match result {
        Ok(()) => 0.0,
        Err(error) => status(error)?,
    };
    Ok(record(k, &mut body, &before, result_status)?)
}

/// The full checkpoint's record of `body` after a fillet that ended with `result_status`:
/// the selected edges `before` it, the status and counts, the topology, the geometry and
/// the mass properties.
fn record(
    k: &Kernel,
    body: &mut Brep,
    before: &[f64],
    result_status: f64,
) -> SolidResult<Vec<f64>> {
    let mut values = Vec::new();
    section(&mut values, before);
    values.extend([
        result_status,
        p1::vertices(body, None)?.len() as f64,
        p1::edges(body, None)?.len() as f64,
        p1::faces(body, None)?.len() as f64,
    ]);
    let mut topology_record = Vec::new();
    topology(body, &mut topology_record)?;
    section(&mut values, &topology_record);
    let mut geometry = Vec::new();
    crate::util::primitive::append(k, body, &mut geometry)?;
    section(&mut values, &geometry);
    // The public API leaves no attribute on the body: the executive's fillet-edge
    // attribute and the temporary merge tags are destroyed with its session, as
    // upstream's executive destructor and SmObjsDelete do, and the box carries none.
    let store = solidcore::geom::attribute::AttributeStore::default();
    let attributes = solidcore::mass::integrator::properties::PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    crate::dump::body(k, "mass", body);
    match solidcore::mass::integrator::properties::brep_mass_properties(
        k,
        body,
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
    // (a soundness probe's measure of the result: nothing unless capture is on)
    crate::soundness::measure(body);
    // (a certificate's look at the result: nothing unless a test installs a hook)
    crate::trim_hook::observe(body);
    Ok(values)
}
/// Configure one actual retained corner between native corner creation and filleting.
fn setback_fillet_edges(
    k: &mut Kernel,
    body: &mut Brep,
    edges: &[EdgeId],
    input: &RealSolid,
    setback: CornerSetback,
) -> Result<(), RealSolidRunError> {
    use solidcore::blend::fillet_executive::session::FilletSession;
    use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};

    let point = Vec3::from(setback.vertex);
    let mut matches = Vec::new();
    for vertex in p1::vertices(body, None)? {
        if solidcore::topology::vertex::point(body, vertex)? == point {
            matches.push(vertex);
        }
    }
    let [vertex] = matches.as_slice() else {
        return Err(RealSolidRunError::Fixture(HarnessError::new(
            "corner setback requires one actual vertex at the requested coordinates",
        )));
    };
    {
        let mut session = FilletSession::new(k, body)?;
        session.prepare_constant_radius_edges(
            k,
            edges,
            input.radius,
            input.xsect,
            input.continuity,
            input.thumbweight,
        )?;
        // A set-back vertex is no Ball: its corner is the one the setback is set on, as
        // the product's blend marks it. A zero distance sets nothing back.
        if setback.distance > 0.0 {
            session.add_set_back_vertex(*vertex);
        }
        session.create_corners(k, None)?;
        let mut corners = Vec::new();
        for corner in session.fillet_corners()? {
            if session.corner(corner)?.filleted_vertex() == Some(*vertex) {
                corners.push(corner);
            }
        }
        let [corner] = corners.as_slice() else {
            return Err(RealSolidRunError::Fixture(HarnessError::new(
                "corner setback requires one created corner at the requested vertex",
            )));
        };
        session.set_corner_setback(*corner, setback.distance)?;
        session.do_filleting(k, None)?;
    }
    // SmApiSetBevelCorners ignores the ordinary StitchAndOrient return status.
    match stitch_and_orient(k, body, &StitchAndOrientOptions::default()) {
        Ok(()) | Err(SolidError::Upstream { .. } | SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn apply(k: &mut Kernel, body: &mut Brep, edges: &[EdgeId], input: &RealSolid) -> SolidResult<()> {
    if let Some(law) = &input.radius_law {
        return law_fillet_edges(k, body, edges, input, law.law()?);
    }
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
    if input.piecewise_merge {
        return piecewise_fillet_edges(k, body, edges, input);
    }
    if let Some(step_back) = input.curve_blend_sih {
        return curve_sih_fillet_edges(k, body, edges, input, step_back);
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
/// The shim's `piecewise_fillet_edges`: `SmApiFilletEdges`' executive set-up, then
/// `SetDoPiecewiseMerge(TRUE)` after `SetFilletParameters`, `DoFillet` and
/// `StitchAndOrient` with its defaults.
fn piecewise_fillet_edges(
    k: &mut Kernel,
    body: &mut Brep,
    edges: &[EdgeId],
    input: &RealSolid,
) -> SolidResult<()> {
    use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
    if edges.is_empty() {
        return Err(SolidError::invalid_input(
            "harness::piecewise_fillet",
            "no edges",
        ));
    }
    {
        let mut session = solidcore::blend::fillet_executive::session::FilletSession::new(k, body)?;
        session.prepare_constant_radius_edges(
            k,
            edges,
            input.radius,
            input.xsect,
            input.continuity,
            input.thumbweight,
        )?;
        session.set_do_piecewise_merge(true);
        session.do_fillet(k)?;
    }
    stitch_and_orient(k, body, &StitchAndOrientOptions::default())
}
/// The shim's `curve_sih_fillet_edges`: `SmApiFilletEdges` with an
/// `SmMakeCurveBlendSIH(step_back)` installed on the executive before
/// `SetDoGlobalMerge(TRUE)`, `SetFilletParameters`, `DoFillet` and
/// `StitchAndOrient` with its defaults.
/// The shim's `law_fillet_edges`: `SmApiVariableRadiusFillet`'s body with an
/// `SmBSplineFilletLaw` (no radius-support check; `StitchAndOrient`'s status ignored).
fn law_fillet_edges(
    k: &mut Kernel,
    body: &mut Brep,
    edges: &[EdgeId],
    input: &RealSolid,
    law: solidcore::blend::fillet_standard_solver::p1::FilletLaw,
) -> SolidResult<()> {
    use solidcore::blend::fillet_executive::session::FilletSession;
    use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
    {
        let mut session = FilletSession::new(k, body)?;
        session.prepare_variable_radius_edges(
            k,
            edges,
            input.radius,
            law,
            input.xsect,
            input.continuity,
        )?;
        session.do_fillet(k)?;
    }
    match stitch_and_orient(k, body, &StitchAndOrientOptions::default()) {
        Ok(()) | Err(SolidError::Upstream { .. } | SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => Ok(()),
        Err(error) => Err(error),
    }
}

fn curve_sih_fillet_edges(
    k: &mut Kernel,
    body: &mut Brep,
    edges: &[EdgeId],
    input: &RealSolid,
    step_back: f64,
) -> SolidResult<()> {
    use solidcore::blend::fillet_solver::curve_blend_sih::MakeCurveBlendSih;
    use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
    if edges.is_empty() {
        return Err(SolidError::invalid_input(
            "harness::curve_sih_fillet",
            "no edges",
        ));
    }
    {
        let mut session = solidcore::blend::fillet_executive::session::FilletSession::new(k, body)?;
        session.set_curve_blend_self_intersection_handler(Some(MakeCurveBlendSih::new(step_back)));
        session.set_do_global_merge(true);
        session.prepare_constant_radius_edges(
            k,
            edges,
            input.radius,
            input.xsect,
            input.continuity,
            input.thumbweight,
        )?;
        session.do_fillet(k)?;
    }
    stitch_and_orient(k, body, &StitchAndOrientOptions::default())
}
/// The fillet-then-shell record (`sc_blend_fillet_executive_p3_fillet_then_shell`):
/// fillet status; then, after a fillet, the shell status and the open-face count;
/// after a shell, the result's v/e/f counts, the manifold query's status and flag,
/// the mass status with area, volume and mass, and the result's geometry section.
fn shell_output(values: &[f64]) -> Result<Output, HarnessError> {
    let integer = |v: &f64| v.is_finite() && v.fract() == 0.0;
    let bad = || HarnessError::new("invalid fillet-then-shell record");
    let (&fillet, rest) = values.split_first().ok_or_else(bad)?;
    let mut out = Output::new();
    if !integer(&fillet) {
        return Err(bad());
    }
    if fillet != 0.0 {
        if !rest.is_empty() {
            return Err(bad());
        }
        out.ints
            .insert("fillet_shell_status".into(), vec![fillet as i64]);
        return Ok(out);
    }
    let (statuses, rest) = (
        rest.get(..2).ok_or_else(bad)?,
        rest.get(2..).ok_or_else(bad)?,
    );
    if !statuses.iter().all(integer) {
        return Err(bad());
    }
    let mut status = vec![0];
    status.extend(statuses.iter().map(|v| *v as i64));
    out.ints.insert("fillet_shell_status".into(), status);
    if statuses.first() != Some(&0.0) {
        if !rest.is_empty() {
            return Err(bad());
        }
        return Ok(out);
    }
    let counts = rest.get(..6).ok_or_else(bad)?;
    if !counts.iter().all(integer) {
        return Err(bad());
    }
    out.ints.insert(
        "shell_counts_manifold_mass_status".into(),
        counts.iter().map(|v| *v as i64).collect(),
    );
    let mass_len = if counts.get(5) == Some(&0.0) { 3 } else { 0 };
    let mass = rest.get(6..6 + mass_len).ok_or_else(bad)?;
    out.values
        .insert("shell_mass_properties".into(), mass.to_vec());
    let mut tail = rest.get(6 + mass_len..).ok_or_else(bad)?;
    let geometry = take_section(&mut tail)?;
    if !tail.is_empty() {
        return Err(bad());
    }
    out.values
        .insert("shell_geometry".into(), geometry.to_vec());
    Ok(out)
}

/// The Rust side of the fillet-then-shell record: `fillet_edges`, then
/// `OffsetExecutive::shell_brep` with `SmApiShellBrepFull`'s options, on the
/// identities of [`shell_notifications`].
fn shell_rust(k: &mut Kernel, input: &RealSolid, shell: ShellAfter) -> SolidResult<Vec<f64>> {
    use solidcore::offset::offset_executive::p1::OffsetExecutive;
    use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
    let [x, y, z] = input.dimensions.unwrap_or_default();
    let mut body = solidcore::primitive::api_primitives::create_box(k, Vec3::ZERO, x, y, z)?;
    let all = p1::edges(&body, None)?;
    let edges: Vec<EdgeId> = input
        .edge_indices()?
        .into_iter()
        .map(|i| {
            all.get(i).copied().ok_or_else(|| {
                SolidError::invalid_input(
                    "harness::fillet_then_shell",
                    "selected edge out of range",
                )
            })
        })
        .collect::<SolidResult<_>>()?;
    let mut values = Vec::new();
    match apply(k, &mut body, &edges, input) {
        Ok(()) => values.push(0.0),
        Err(error) => {
            values.push(status(error)?);
            return Ok(values);
        }
    }
    let mut open = Vec::new();
    if shell.open_top {
        for f in p1::faces(&body, None)? {
            let face = body.face(f)?;
            let Some(s) = face.surface.as_ref() else {
                continue;
            };
            if s.is_kind_of(solidcore::surface::plane::Plane::TYPE)?
                && (s.evaluate_point(k, face.uv_domain.mid())?.z - z).abs() < 1e-6
            {
                open.push(f);
            }
        }
    }
    let mut stores = shell_notifications::Stores::new(&mut body);
    let shelled = OffsetExecutive::shell_brep(
        k,
        body,
        ShellOptions {
            offset: OffsetOptions {
                distance: shell.distance,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: false,
            merge_sheet_side_walls: false,
        },
        &open,
        &mut stores.context(),
    )
    .result;
    let mut result = match shelled {
        Ok(result) => result,
        Err(error) => {
            values.extend([status(error)?, open.len() as f64]);
            return Ok(values);
        }
    };
    values.extend([0.0, open.len() as f64]);
    values.extend([
        p1::vertices(&result, None)?.len() as f64,
        p1::edges(&result, None)?.len() as f64,
        p1::faces(&result, None)?.len() as f64,
    ]);
    match solidcore::query::api_queries::brep_is_manifold_solid(&result) {
        Ok(manifold) => values.extend([0.0, f64::from(u8::from(manifold))]),
        Err(error) => values.extend([status(error)?, 0.0]),
    }
    let store = solidcore::geom::attribute::AttributeStore::default();
    let attributes = solidcore::mass::integrator::properties::PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    crate::dump::body(k, "shell_mass", &result);
    match solidcore::mass::integrator::properties::brep_mass_properties(
        k,
        &mut result,
        1e-4,
        1.0,
        Vec3::ZERO,
        &attributes,
    ) {
        Ok(mass) => values.extend([
            0.0,
            mass.properties.area,
            mass.properties.volume,
            mass.properties.mass,
        ]),
        Err(error) => values.push(status(error)?),
    }
    // (a soundness probe's measure and a certificate's look at the shelled body: nothing
    // unless capture is on or a test installs a hook)
    crate::soundness::measure(&mut result);
    crate::trim_hook::observe(&mut result);
    let mut geometry = Vec::new();
    crate::util::primitive::append(k, &result, &mut geometry)?;
    section(&mut values, &geometry);
    Ok(values)
}

/// The caller-owned notification identities `OffsetExecutive::shell_brep` needs: the
/// filleted body as B-rep 0 of one namespace, results in another, merge bodies and
/// curves in their own (all disjoint).
mod shell_notifications {
    

    use solidcore::curve::curve::Curve;
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::geom::object::ObjectRef;
    use solidcore::offset::offset_executive::p2::OffsetNotifications;
    use solidcore::sweep::topology_sweep::store::SweepIdentities;
    use solidcore::topology::topology::Topology;
    use solidcore::topology::{Brep, BrepId, EdgeId, Region, RegionId, Shell};
    use solidcore::{EntityRef, Result as SolidResult};

    fn key(entity: EntityRef, namespace: u64) -> ObjectRef {
        let (index, type_code) = match entity {
            EntityRef::Brep(id) => (id.index(), solidcore::geom::a_object::BREP_TYPE),
            EntityRef::Shell(id) => (id.index(), Shell::type_code()),
            EntityRef::Face(id) => (id.index(), solidcore::topology::face::p2::TYPE),
            EntityRef::Faceuse(id) => (id.index(), 16022),
            EntityRef::Edge(id) => (id.index(), solidcore::topology::edge::TYPE),
            EntityRef::Edgeuse(id) => (id.index(), solidcore::topology::edgeuse::p1::TYPE),
            EntityRef::Vertex(id) => (id.index(), solidcore::topology::vertex::TYPE),
            EntityRef::Vertexuse(id) => (id.index(), solidcore::topology::vertexuse::TYPE),
            EntityRef::Loopuse(id) => (id.index(), solidcore::topology::loopuse::TYPE),
            EntityRef::Region(id) => (id.index(), Region::type_code()),
            EntityRef::Loop(id) => (id.index(), solidcore::topology::topology::TYPE),
        };
        ObjectRef {
            key: ObjectKey(namespace + type_code.unsigned_abs() * 1_000_000 + u64::from(index)),
            type_code,
        }
    }
    fn original_key(entity: EntityRef) -> ObjectRef {
        key(entity, 100_000_000_000)
    }
    fn result_key(entity: EntityRef) -> ObjectRef {
        key(entity, 200_000_000_000)
    }
    fn original_curve(edge: EdgeId, type_code: i64) -> ObjectRef {
        ObjectRef {
            key: ObjectKey(400_000_000_000 + u64::from(edge.index())),
            type_code,
        }
    }
    fn result_curve(edge: EdgeId, type_code: i64) -> ObjectRef {
        ObjectRef {
            key: ObjectKey(300_000_000_000 + u64::from(edge.index())),
            type_code,
        }
    }
    fn source_lists(result: bool, region: RegionId) -> [ObjectRef; 2] {
        [0, 1].map(|list| ObjectRef {
            key: ObjectKey(
                500_000_000_000
                    + u64::from(result) * 10_000_000_000
                    + u64::from(region.index()) * 2
                    + list,
            ),
            type_code: 0,
        })
    }
    fn merge_curve(brep: BrepId, edge: EdgeId, type_code: i64) -> ObjectRef {
        ObjectRef {
            key: ObjectKey(
                700_000_000_000 + u64::from(brep.index()) * 1_000_000_000 + u64::from(edge.index()),
            ),
            type_code,
        }
    }
    struct MergeBodies;
    impl SweepIdentities for MergeBodies {
        fn topology(&self, brep: BrepId, entity: EntityRef) -> ObjectRef {
            key(
                entity,
                1_000_000_000_000 + u64::from(brep.index()) * 100_000_000_000,
            )
        }
        fn owned_auxiliary_objects(&self, brep: BrepId, model: &Brep) -> Vec<ObjectKey> {
            model
                .edges()
                .iter()
                .filter(|(_, e)| e.curve.is_some())
                .map(|(edge, _)| merge_curve(brep, edge, 0).key)
                .collect()
        }
        fn curve(&self, brep: BrepId, edge: EdgeId, curve: &Curve) -> ObjectRef {
            merge_curve(brep, edge, curve.type_code())
        }
        fn new_curve(&self, brep: BrepId, edge: EdgeId, type_code: i64) -> SolidResult<ObjectRef> {
            Ok(merge_curve(brep, edge, type_code))
        }
        fn source_lists(&self, brep: BrepId, region: RegionId) -> SolidResult<[ObjectRef; 2]> {
            Ok([0, 1].map(|list| ObjectRef {
                key: ObjectKey(
                    800_000_000_000
                        + u64::from(brep.index()) * 1_000_000_000
                        + u64::from(region.index()) * 2
                        + list,
                ),
                type_code: 0,
            }))
        }
    }

    /// The topology records and attributes of one shell run.
    pub(super) struct Stores {
        records: solidcore::topology::topology::NotifyRecords,
        attributes: AttributeStore,
    }
    impl Stores {
        /// Keys `body` as B-rep 0 of the original namespace and registers it.
        pub(super) fn new(body: &mut Brep) -> Self {
            body.set_key(original_key(EntityRef::Brep(BrepId::from_index(0))).key);
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            records.insert(body.key(), Topology::new());
            records.insert(
                result_key(EntityRef::Brep(BrepId::from_index(0))).key,
                Topology::new(),
            );
            let entities: Vec<EntityRef> = body
                .shells()
                .iter()
                .map(|(id, _)| EntityRef::Shell(id))
                .chain(body.faces().iter().map(|(id, _)| EntityRef::Face(id)))
                .chain(body.faceuses().iter().map(|(id, _)| EntityRef::Faceuse(id)))
                .chain(body.edges().iter().map(|(id, _)| EntityRef::Edge(id)))
                .chain(body.edgeuses().iter().map(|(id, _)| EntityRef::Edgeuse(id)))
                .chain(body.vertices().iter().map(|(id, _)| EntityRef::Vertex(id)))
                .chain(
                    body.vertexuses()
                        .iter()
                        .map(|(id, _)| EntityRef::Vertexuse(id)),
                )
                .chain(body.loopuses().iter().map(|(id, _)| EntityRef::Loopuse(id)))
                .chain(body.regions().iter().map(|(id, _)| EntityRef::Region(id)))
                .collect();
            for entity in entities {
                records.insert(original_key(entity).key, Topology::new());
            }
            Self {
                records,
                attributes: AttributeStore::default(),
            }
        }
        /// The executive's view of the stores.
        pub(super) fn context(&mut self) -> OffsetNotifications<'_> {
            OffsetNotifications {
                records: &mut self.records,
                attributes: &mut self.attributes,
                original_key_of: &original_key,
                result_key_of: &result_key,
                original_curve_ref: &original_curve,
                result_curve_ref: &result_curve,
                boolean_source_lists: &source_lists,
                result_brep: result_key(EntityRef::Brep(BrepId::from_index(0))),
                merge_identities: Some(&MergeBodies),
            }
        }
    }
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
    let mut k = crate::runner_kernel();
    let mut run = || -> SolidResult<Vec<f64>> {
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
    if let Some(shell) = input.shell {
        let mut k = crate::runner_kernel();
        return match shell_rust(&mut k, input, shell) {
            Ok(values) => shell_output(&values),
            Err(error) => match Output::unported(&error) {
                Some(deferred) => Ok(deferred),
                None => Err(HarnessError::new(error.to_string())),
            },
        };
    }
    let mut k = crate::runner_kernel();
    let body = build(&mut k, input)?;
    match run_rust(&mut k, body, input) {
        Ok(values) => {
            if input.checkpoint == Checkpoint::Full {
                output(&values)
            } else {
                prefix::output(&values, input.checkpoint)
            }
        }
        Err(RealSolidRunError::Fixture(error)) => Err(error),
        Err(RealSolidRunError::Kernel(error)) => match Output::unported(&error) {
            Some(deferred) => Ok(deferred),
            None => Err(HarnessError::new(error.to_string())),
        },
    }
}

fn surface_surface_output(values: &[f64]) -> Result<Output, HarnessError> {
    let (&status, mut rest) = values
        .split_first()
        .ok_or_else(|| HarnessError::new("missing status"))?;
    let mut out = Output::new();
    let mut head = vec![status];
    if !rest.is_empty() {
        head.extend(
            rest.get(..3)
                .ok_or_else(|| HarnessError::new("missing counts"))?,
        );
        rest = rest.get(3..).unwrap_or_default();
        let topology = take_section(&mut rest)?;
        if topology.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
            return Err(HarnessError::new("invalid topology record"));
        }
        out.ints.insert(
            "result_topology".into(),
            topology.iter().map(|v| *v as i64).collect(),
        );
        out.values
            .insert("result_geometry".into(), take_section(&mut rest)?.to_vec());
        if !rest.is_empty() {
            return Err(HarnessError::new("trailing surface-surface record"));
        }
    }
    if head.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
        return Err(HarnessError::new("invalid status/counts"));
    }
    out.ints.insert(
        "api_status_vertex_edge_face_counts".into(),
        head.iter().map(|v| *v as i64).collect(),
    );
    Ok(out)
}
/// The Rust side of [`SurfaceSurface`]: both surfaces are built as the shim builds
/// them (`SmApiCreateSurfaceFromCornerPoints`), then
/// `solidcore::blend::fillet_executive::api::surface_surface_fillet` runs and the
/// result B-rep is recorded as the shim records it.
pub(super) fn surface_surface_rust(input: &SurfaceSurface) -> Result<Output, HarnessError> {
    contract::validate_surface_surface(&input.wire()).map_err(|e| HarnessError::new(e.to_string()))?;
    let mut k = crate::runner_kernel();
    let mut run = || -> SolidResult<Vec<f64>> {
        let surface = |c: &[[f64; 3]; 4]| {
            solidcore::nurbs::b_spline_surface::BSplineSurface::create_bilinear_surface(
                Vec3::from(c[0]),
                Vec3::from(c[1]),
                Vec3::from(c[2]),
                Vec3::from(c[3]),
            )
            .map(solidcore::surface::surface::Surface::from)
        };
        let first = surface(&input.first)?;
        let second = surface(&input.second)?;
        let result = solidcore::blend::fillet_executive::api::surface_surface_fillet(
            &mut k,
            solidcore::blend::fillet_executive::api::FilletSurfaceArg::Free(&first),
            solidcore::blend::fillet_executive::api::FilletSurfaceArg::Free(&second),
            input.radius1,
            input.radius2,
            input.tolerance,
            input.xsect,
            input.mirror,
            input.complement,
        );
        let body = match result {
            Ok(body) => body,
            Err(error) => return Ok(vec![status(error)?]),
        };
        let mut values = vec![0.0];
        if let Some(body) = body {
            values.extend([
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
        }
        Ok(values)
    };
    match run() {
        Ok(values) => surface_surface_output(&values),
        Err(error) => Output::unported(&error)
            .ok_or_else(|| HarnessError::new(error.to_string())),
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

    fn setback_input(distance: f64) -> super::RealSolid {
        serde_json::from_value(serde_json::json!({
            "dimensions": [60,40,20], "radius": 5,
            "xsect": 1, "continuity": 1, "thumbweight": 1, "edges": [0,1,5],
            "corner_setback": { "vertex": [60,40,20], "distance": distance }
        }))
        .expect("valid setback fixture schema")
    }

    #[test]
    fn corner_setback_preserves_legacy_wire_and_absent_field_serialization() {
        let mut input = setback_input(8.0);
        let wire = input.wire();
        assert_eq!(
            wire,
            [60.0, 40.0, 20.0, 5.0, 1.0, 1.0, 1.0, 3.0, 0.0, 1.0, 5.0]
        );
        assert_eq!(
            input.corner_setback.expect("setback").wire(),
            [60.0, 40.0, 20.0, 8.0]
        );
        assert!(input.validate().is_ok());
        input.corner_setback = None;
        assert_eq!(input.wire(), wire);
        assert!(serde_json::to_value(&input)
            .expect("serialize")
            .get("corner_setback")
            .is_none());
    }

    #[test]
    fn corner_setback_rejects_mixed_forms_before_execution() {
        let original = setback_input(8.0);
        let mut invalid = Vec::new();
        for checkpoint in [
            super::Checkpoint::Prepared,
            super::Checkpoint::IntermediateFaces,
        ] {
            let mut input = original.clone();
            input.checkpoint = checkpoint;
            invalid.push(input);
        }
        let mut input = original.clone();
        input.radius_end = Some(2.0);
        invalid.push(input);
        let mut input = original.clone();
        input.piecewise_merge = true;
        invalid.push(input);
        let mut input = original.clone();
        input.curve_blend_sih = Some(1.0);
        invalid.push(input);
        let mut input = original.clone();
        input.dimensions = None;
        input.source = Some(super::Source::Cylinder {
            origin: [0.0; 3],
            radius: 10.0,
            height: 20.0,
        });
        invalid.push(input);
        let mut input = original;
        input.all_edges = true;
        invalid.push(input);
        for input in invalid {
            assert!(super::rust(&input).is_err());
        }
    }

    #[test]
    fn corner_setback_nonfinite_values_are_fixture_errors() {
        for (vertex, distance) in [
            ([f64::NAN, 40.0, 20.0], 8.0),
            ([60.0, 40.0, 20.0], f64::INFINITY),
        ] {
            let mut input = setback_input(8.0);
            input.corner_setback = Some(super::CornerSetback { vertex, distance });
            assert!(input.check_form().is_err());
            assert!(super::rust(&input).is_err());
        }
    }

    #[test]
    fn corner_setback_requires_actual_vertex_and_prepared_corner() {
        for vertex in [[59.0, 40.0, 20.0], [0.0, 0.0, 0.0]] {
            let mut input = setback_input(8.0);
            input.corner_setback.as_mut().expect("setback").vertex = vertex;
            // A malformed fixture must not turn into a compared 1006/1007 or rejection.
            assert!(super::rust(&input).is_err());
        }
    }

    #[test]
    fn corner_setback_changes_the_actual_full_pipeline_result() {
        let result = super::rust(&setback_input(8.0)).expect("actual setback pipeline");
        let zero = super::rust(&setback_input(0.0)).expect("actual zero setback pipeline");
        assert_eq!(
            result.ints["api_status_vertex_edge_face_counts"],
            [0, 23, 36, 15]
        );
        assert_eq!(
            zero.ints["api_status_vertex_edge_face_counts"],
            [0, 13, 21, 10]
        );
        assert_eq!(result.ints["mass_status"], [0]);
        assert_eq!(zero.ints["mass_status"], [0]);
        assert_ne!(
            result.values["mass_properties"],
            zero.values["mass_properties"]
        );
        assert!(!result.values["result_geometry"].is_empty());
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
        assert!(super::rust(&input).is_err());
    }

    #[test]
    fn variable_radius_rejects_ignored_thumbweight_and_nonfinite_endpoint() {
        let mut input = variable_input();
        input.thumbweight = 1.5;
        assert!(super::rust(&input).is_err());
        input.thumbweight = 1.0;
        input.radius_end = Some(f64::NAN);
        assert!(super::rust(&input).is_err());
    }

    #[test]
    fn variable_radius_on_a_source_validates_its_endpoint_and_thumbweight() {
        let mut input: super::RealSolid = serde_json::from_value(serde_json::json!({
            "source": { "kind": "cylinder", "origin": [0,0,0], "radius": 10, "height": 20 },
            "radius": 10, "radius_end": 10,
            "xsect": 1, "continuity": 1, "thumbweight": 1, "edges": [1]
        }))
        .expect("valid variable source fixture schema");
        assert!(input.validate().is_ok());
        input.thumbweight = 1.5;
        assert!(input.validate().is_err());
        input.thumbweight = 1.0;
        input.radius_end = Some(f64::NAN);
        assert!(input.validate().is_err());
    }

    fn law_input() -> super::RealSolid {
        serde_json::from_value(serde_json::json!({
            "dimensions": [20,20,20], "radius": 4,
            "radius_law": { "degree": 1, "radii": [4,2,4], "knots": [0,0.5,1],
                            "multiplicities": [2,1,2] },
            "xsect": 1, "continuity": 1, "thumbweight": 1, "edges": [0]
        }))
        .expect("valid law fixture schema")
    }

    #[test]
    fn radius_law_wire_and_form() {
        let mut input = law_input();
        let law = input.radius_law.clone().expect("law");
        assert_eq!(
            law.wire(),
            [1.0, 3.0, 4.0, 2.0, 4.0, 3.0, 0.0, 0.5, 1.0, 2.0, 1.0, 2.0, 0.0, 1.0]
        );
        assert!(input.validate().is_ok());
        input.radius_end = Some(2.0);
        assert!(input.validate().is_err());
        input.radius_end = None;
        input.checkpoint = super::Checkpoint::IntermediateFaces;
        assert!(input.validate().is_err());
        input.checkpoint = super::Checkpoint::Full;
        input.thumbweight = 1.5;
        assert!(input.validate().is_err());
    }

    #[test]
    fn radius_law_rejects_malformed_curves() {
        let set = |f: &dyn Fn(&mut super::RadiusLaw)| {
            let mut input = law_input();
            f(input.radius_law.as_mut().expect("law"));
            input
        };
        for input in [
            set(&|law| law.multiplicities = vec![2, 2, 2]),
            set(&|law| law.multiplicities = vec![1, 1, 2]),
            set(&|law| law.knots = vec![0.0, 0.5, 0.5]),
            set(&|law| law.degree = 0),
            set(&|law| law.degree = 4),
            set(&|law| law.radii[1] = f64::NAN),
            set(&|law| law.edge_map = [0.0, f64::INFINITY]),
        ] {
            assert!(input.validate().is_err(), "{input:?}");
            assert!(super::rust(&input).is_err());
        }
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

    /// A refusal that names the size that builds (`SolidError::FilletRefused`) is the
    /// executive's failure status, as `SolidError::sm_status` reads it: the C++ returns the
    /// status alone. Compared as a status, not a harness error (#2400: the blend corner's
    /// refusals of rounds the pinned corner built).
    #[test]
    fn a_refusal_naming_its_size_is_its_status() {
        use solidcore::blend::fillet_executive::{FitKind, FitSize};
        let refused = solidcore::SolidError::FilletRefused {
            operation: "blend::fillet_executive::session::finish",
            code: 1001,
            entity: None,
            reason: None,
            fit: FitSize {
                size: 0.5,
                edge: None,
                kind: FitKind::Radius,
            },
        };
        assert_eq!(super::status(refused).expect("a status"), 1001.0);
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


/// The proof of the golden rows that moved when a blend face became its definition
/// (`fillet_round_as_definition` retired, #16580), replayed on the rows' own inputs.
///
/// Each moved row's integers move because its blend faces' type codes are now the
/// definition's, and its values because its geometry is evaluated on the definition `S`
/// (and its net, certified within its bound of `S`, where a [`NetScope`] asks). Checked per
/// row: every blend face by its definition re-certifies its net (`recertify` proves a
/// distance `d` between the net and `S` over the whole face, at most the bound); and on a
/// closed result, the volume. [`solidcore::mass::enclosure::enclose_volume`] encloses the
/// volume of the body as stored (the nets) with a certified `[lo, hi]`; the body bounded by
/// the definitions differs from it by at most `sum A_f d_f` (each face within `d_f` of its
/// net, `A_f` its area), bounded above by summing each blend face's certified area
/// enclosure's upper end times its `d_f`. The
/// volume the integrator converges to (its finest accuracy) lies in that band. The area,
/// centroid, inertias and products are enclosed likewise
/// ([`solidcore::mass::enclosure::enclose_mass_properties`], the body as stored), and the
/// golden's estimates (`brep_mass_properties` at the requested relative accuracy `1e-4`)
/// lie within that accuracy of their bands; every operation's logged blend faces are
/// definitions (`blend::definition_check`).
///
/// [`NetScope`]: solidcore::surface::procedural_blend::NetScope
#[cfg(test)]
pub(super) mod round_definition_proof {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use solidcore::kernel::Kernel;
    use solidcore::mass::enclosure::{enclose_face_area_relative, enclose_volume};
    use solidcore::mass::integrator::properties::{
        brep_mass_properties, MassProperties, PropertyAttributes, ESTIMATE_ACCURACY_FINEST,
    };
    use solidcore::math::Vec3;
    use solidcore::surface::surface::Surface;
    use solidcore::topology::brep::p1;
    use solidcore::topology::{Brep, FaceId};

    /// The golden's requested relative accuracy of the mass properties.
    pub(in super::super) const MASS_ACCURACY: f64 = 1e-4;

    /// The integrator's estimates of a closed body's mass values (density 1, about the
    /// origin), taken once by [`volume_band`] and read again by [`mass_bands`]: at the
    /// golden's accuracy and at the integrator's finest, where it converges.
    pub(in super::super) struct Estimates {
        /// At [`MASS_ACCURACY`], as the golden measures.
        pub(in super::super) golden: MassProperties,
        /// At [`ESTIMATE_ACCURACY_FINEST`].
        converged: MassProperties,
    }

    /// The faces of `b` whose surface is a blend by its definition, each with the proved
    /// distance `d_f` between its net and its definition (`recertify`, at most its bound);
    /// asserts there is at least one.
    pub(in super::super) fn certified_blends(
        k: &Kernel,
        b: &mut Brep,
        name: &str,
    ) -> Vec<(FaceId, f64)> {
        let mut blends = Vec::new();
        for f in p1::faces(b, None).unwrap() {
            let surface = b.face(f).unwrap().surface.clone();
            let Some(blend) = surface.as_ref().and_then(Surface::procedural_blend) else {
                continue;
            };
            let d = blend
                .recertify(k)
                .unwrap_or_else(|e| panic!("{name}: {f:?} does not re-certify: {e}"));
            let bound = blend.bound();
            assert!(
                d <= bound,
                "{name}: {f:?} proved {d:e} past its bound {bound:e}"
            );
            blends.push((f, d));
        }
        assert!(!blends.is_empty(), "{name}: no blend face is a definition");
        blends
    }

    /// `sum A_f d_f` bounded above: each face's area certified
    /// ([`enclose_face_area_relative`], refined in one pass until it is within 1% of its
    /// upper end or its budget is spent; the band holds either way), each product and the
    /// sum rounded up.
    fn spread(k: &Kernel, b: &mut Brep, blends: &[(FaceId, f64)]) -> f64 {
        let mut sum = 0.0_f64;
        for &(f, d) in blends {
            let a = enclose_face_area_relative(k, b, &[f], 1e-2).unwrap();
            sum = (sum + (a.hi * d).next_up()).next_up();
        }
        sum
    }

    /// The certified volume band of the closed body `b`: its enclosure (asked 1e-6 wide
    /// relative; refinement stops earlier where the gaps between faces, which no cut
    /// narrows, are wider) widened by an upper bound of `sum A_f d_f` over the blend faces
    /// ([`spread`]: each face's certified area times its proved net distance `d_f` from
    /// [`certified_blends`]). Asserts that the volume the integrator
    /// converges to (its estimate at its finest accuracy, 1e-8 relative) lies in it. The
    /// golden's own estimate (at 1e-4) is printed beside the band for the record only: an
    /// estimate is not a bound. Returns both estimates, for [`mass_bands`].
    pub(in super::super) fn volume_band(
        k: &Kernel,
        b: &mut Brep,
        blends: &[(FaceId, f64)],
        name: &str,
    ) -> Estimates {
        let spread = spread(k, b, blends);
        let store = solidcore::geom::attribute::AttributeStore::default();
        let attributes = PropertyAttributes {
            store: &store,
            key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
        };
        let mut at = |accuracy: f64| {
            brep_mass_properties(k, b, accuracy, 1.0, Vec3::ZERO, &attributes).unwrap()
        };
        let estimates = Estimates {
            golden: at(MASS_ACCURACY),
            converged: at(ESTIMATE_ACCURACY_FINEST),
        };
        let new = estimates.golden.properties.volume;
        let converged = estimates.converged.properties.volume;
        let enclosure = enclose_volume(k, b, 1e-6 * new.abs()).unwrap();
        let (lo, hi) = (enclosure.lo - spread, enclosure.hi + spread);
        assert!(
            (lo..=hi).contains(&converged),
            "{name}: the converged volume {converged} is off the certified [{lo}, {hi}]"
        );
        eprintln!(
            "{name}: converged {converged}, the golden's estimate {new}, \
             certified [{lo}, {hi}], band width {:e}",
            hi - lo
        );
        estimates
    }

    /// The certified bands of the closed body `b`'s other mass values, as the golden
    /// records them (density 1, about the origin): its area, centroid, axial inertias and
    /// products (`enclose_mass_properties`, each asked 1e-5 of its scale wide: the area's,
    /// the volume's times the box's half diagonal `R` to the moment's degree; one that
    /// spends its cell budget first stays a valid, wider band). The golden's requested
    /// accuracy is `1e-4` of the value's
    /// scale (the area, `R` for a centroid, `V R^2` for an inertia or product, or the value
    /// where it is larger). Asserts, for each value, that the value the integrator
    /// converges to (1e-8) lies in its band and that the golden's own estimate (1e-4) lies
    /// within that accuracy of it; prints `PROVEN` where the band is at most that accuracy
    /// wide (the golden's value is then within twice its accuracy of the true one) and
    /// `WIDE` where it is not. The bands are of the body as stored (its nets), which is the
    /// body the golden measures. The estimates are [`volume_band`]'s, of the same body.
    pub(in super::super) fn mass_bands(k: &Kernel, b: &mut Brep, estimates: &Estimates, name: &str) {
        use solidcore::mass::enclosure::{enclose_mass_properties, Bounds};
        let at = |m: &MassProperties| {
            let v = |p: Vec3| [p.x, p.y, p.z];
            (m.properties.area, v(m.centroid), v(m.inertia), v(m.products))
        };
        let golden = at(&estimates.golden);
        let converged = at(&estimates.converged);
        let e = enclose_mass_properties(k, b, 1e-5).unwrap();
        if let Some((q, stop)) = e.short {
            eprintln!("{name}: the {q:?} enclosure stopped at {stop:?}, its band wider");
        }
        let r = {
            let bbox = solidcore::query::brep::calculate_bounding_box(
                k,
                &mut *b,
                solidcore::query::brep::BoundingBoxOptions::default(),
            )
            .unwrap();
            0.5 * bbox.size().length()
        };
        let vr2 = e.volume.hi * r * r;
        let check = |what: &str, band: &Bounds, conv: f64, gold: f64, scale: f64| {
            let scale = scale.max(conv.abs());
            let accuracy = MASS_ACCURACY * scale;
            assert!(
                band.contains(conv),
                "{name}: the converged {what} {conv} is off the certified {band:?}"
            );
            let off = (band.lo - gold).max(gold - band.hi).max(0.0);
            assert!(
                off <= accuracy,
                "{name}: the golden's {what} {gold} is {off:e} off the certified {band:?}, \
                 past its accuracy {accuracy:e}"
            );
            // PROVEN: the band is at most the accuracy wide, so the golden's value is within
            // twice its accuracy of the true one; WIDE: the band holds the converged and
            // golden values but is wider than the accuracy
            let label = if band.width() <= accuracy {
                "PROVEN"
            } else {
                "WIDE"
            };
            eprintln!(
                "MASSBAND {label} {name}: {what} converged {conv}, golden {gold}, certified \
                 [{}, {}], width {:e} (accuracy {accuracy:e})",
                band.lo,
                band.hi,
                band.width()
            );
        };
        check("area", &e.area, converged.0, golden.0, converged.0);
        let axes = ["x", "y", "z"];
        for i in 0..3 {
            check(
                &format!("centroid {}", axes[i]),
                &e.centroid[i],
                converged.1[i],
                golden.1[i],
                r,
            );
            check(
                &format!("inertia {}", axes[i]),
                &e.inertia[i],
                converged.2[i],
                golden.2[i],
                vr2,
            );
            check(
                &format!("product {}", ["yz", "zx", "xy"][i]),
                &e.products[i],
                converged.3[i],
                golden.3[i],
                vr2,
            );
        }
    }
}

#[cfg(test)]
pub(super) mod round_definition_rows {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::round_definition_proof::{certified_blends, mass_bands, volume_band};
    use super::*;
    use solidcore::blend::definition_check::{
        bare_blend_faces, BlendFaceKind, BlendFaceLog, CornerKind,
    };

    /// The rows of the full real-solid form whose rounds are definitions: name, whether
    /// the result is closed (an open result has no volume), and its counts.
    type Row = (&'static str, bool, [i64; 4]);

    const ROWS: &[Row] = &[
        ("law-box10x20x30-e0-deg1-edge-map-degenerate", true, [0, 12, 19, 9]),
        ("law-cube20-e0-deg1-4-2-4-c0-corner", true, [0, 12, 19, 9]),
        ("law-cube20-e0-deg1-linear-2-4", true, [0, 10, 15, 7]),
        ("law-cube20-e0-deg1-two-corners", true, [0, 16, 26, 12]),
        ("law-cube20-e0-deg2-c0-knot", true, [0, 12, 19, 9]),
        ("law-cube20-e3-deg1-4-2-4-c0-corner", true, [0, 12, 19, 9]),
        ("source-file-corner_box-all-edges", true, [0, 30, 60, 32]),
        ("source-file-corner_box-edge13", true, [0, 12, 18, 8]),
        ("source-file-nurb_box-all-edges", true, [0, 24, 48, 26]),
        ("source-file-nurb_box-edge0", true, [0, 10, 15, 7]),
        ("source-file-nurb_box-edge5", true, [0, 10, 15, 7]),
        ("stubs4-Heart-all-edges-r0.1", true, [0, 14, 26, 14]),
        ("stubs4-coons_01_03_29-e0-r0.5", false, [0, 8, 12, 5]),
        ("stubs4-coons_01_03_29-e3-r0.5", false, [0, 8, 12, 5]),
        ("stubs4-coons_01_03_29-e6-r0.5", false, [0, 8, 12, 5]),
        ("stubs4-delete-edge-bottle-e8-r0.1", true, [0, 31, 47, 18]),
        ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1", true, [0, 29, 46, 11]),
        ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", true, [0, 29, 46, 11]),
        ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1", true, [0, 29, 46, 11]),
        ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5", true, [0, 29, 46, 11]),
        ("stubs4-sphere_plane-e3-r0.5", false, [0, 4, 6, 3]),
        ("stubs4-test8-e3-r0.5", false, [0, 6, 9, 3]),
        ("stubs4-torii_unioned-e2-r0.5", true, [0, 4, 7, 3]),
        ("sweep5-ext_w_hole-e15-r1.0", true, [0, 16, 24, 10]),
        ("sweep5-letter_g-e1-r1.0", true, [0, 22, 33, 13]),
        ("sweep5-letter_g-e10-r1.0", true, [0, 23, 35, 14]),
        ("sweep5-nurb_box1-all-r0.5", true, [0, 18, 36, 20]),
    ];

    /// No face the operation logged as a blend face (`blend::definition_check`) is a bare
    /// net that a definition should hold: a face without a definition is either an exact
    /// analytic surface (a plane, sphere, cone or torus: the ball's own surface, nothing
    /// approximated) or a face of a pinned corner (a corner hierarchy that is not built as a
    /// definition; its faces are definitions only with the blend corner's fixes).
    pub(in super::super) fn no_bare_blend_face(k: &Kernel, b: &Brep, name: &str) {
        let made = k.blend_face_log.as_ref().unwrap().last().to_vec();
        assert!(!made.is_empty(), "{name}: no blend face logged");
        for bare in bare_blend_faces(b, &made).unwrap() {
            let exact = matches!(bare.surface, "plane" | "sphere" | "cone" | "torus");
            let pinned = bare.kind == BlendFaceKind::Corner(CornerKind::Pinned);
            assert!(exact || pinned, "{name}: a bare blend face {bare:?}");
        }
    }

    /// The fixture inputs of `name`.
    pub(super) fn inputs(name: &str) -> super::super::Inputs {
        crate::fixtures::load_fixture_dir(&crate::case::repo_root(), "blend.fillet_executive_p3")
            .unwrap()
            .into_iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no case {name}"))
            .parse()
            .unwrap()
    }

    /// The status and the vertex, edge and face counts of `b`, as the golden records them.
    pub(super) fn counts(b: &Brep, status: i64) -> [i64; 4] {
        [
            status,
            p1::vertices(b, None).unwrap().len() as i64,
            p1::edges(b, None).unwrap().len() as i64,
            p1::faces(b, None).unwrap().len() as i64,
        ]
    }

    /// One test per moved row of the full real-solid form, so the test runner spreads the
    /// rows over its threads (each row's certificates take minutes; the rows in sequence
    /// outran the CI job's time limit). Each checks its row as [`check`] says.
    macro_rules! row_tests {
        ($($test:ident => $name:literal,)*) => {
            $(
                #[test]
                fn $test() {
                    check($name);
                }
            )*

            /// The rows the generated tests take, in [`ROWS`]'s order.
            const ROW_TESTS: &[&str] = &[$($name),*];
        };
    }

    row_tests! {
        law_box10x20x30_e0_deg1_edge_map_degenerate => "law-box10x20x30-e0-deg1-edge-map-degenerate",
        law_cube20_e0_deg1_4_2_4_c0_corner => "law-cube20-e0-deg1-4-2-4-c0-corner",
        law_cube20_e0_deg1_linear_2_4 => "law-cube20-e0-deg1-linear-2-4",
        law_cube20_e0_deg1_two_corners => "law-cube20-e0-deg1-two-corners",
        law_cube20_e0_deg2_c0_knot => "law-cube20-e0-deg2-c0-knot",
        law_cube20_e3_deg1_4_2_4_c0_corner => "law-cube20-e3-deg1-4-2-4-c0-corner",
        source_file_corner_box_all_edges => "source-file-corner_box-all-edges",
        source_file_corner_box_edge13 => "source-file-corner_box-edge13",
        source_file_nurb_box_all_edges => "source-file-nurb_box-all-edges",
        source_file_nurb_box_edge0 => "source-file-nurb_box-edge0",
        source_file_nurb_box_edge5 => "source-file-nurb_box-edge5",
        stubs4_heart_all_edges_r0_1 => "stubs4-Heart-all-edges-r0.1",
        stubs4_coons_01_03_29_e0_r0_5 => "stubs4-coons_01_03_29-e0-r0.5",
        stubs4_coons_01_03_29_e3_r0_5 => "stubs4-coons_01_03_29-e3-r0.5",
        stubs4_coons_01_03_29_e6_r0_5 => "stubs4-coons_01_03_29-e6-r0.5",
        stubs4_delete_edge_bottle_e8_r0_1 => "stubs4-delete-edge-bottle-e8-r0.1",
        stubs4_delete_edge_sphere_diff_3cylinders_e1_r0_1 => "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        stubs4_delete_edge_sphere_diff_3cylinders_e1_r0_5 => "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        stubs4_delete_edge_sphere_diff_3cylinders_e4_r0_1 => "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        stubs4_delete_edge_sphere_diff_3cylinders_e4_r0_5 => "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        stubs4_sphere_plane_e3_r0_5 => "stubs4-sphere_plane-e3-r0.5",
        stubs4_test8_e3_r0_5 => "stubs4-test8-e3-r0.5",
        stubs4_torii_unioned_e2_r0_5 => "stubs4-torii_unioned-e2-r0.5",
        sweep5_ext_w_hole_e15_r1_0 => "sweep5-ext_w_hole-e15-r1.0",
        sweep5_letter_g_e1_r1_0 => "sweep5-letter_g-e1-r1.0",
        sweep5_letter_g_e10_r1_0 => "sweep5-letter_g-e10-r1.0",
        sweep5_nurb_box1_all_r0_5 => "sweep5-nurb_box1-all-r0.5",
    }

    /// Every row of [`ROWS`] has exactly one test.
    #[test]
    fn every_moved_row_has_its_own_test() {
        let rows: Vec<&str> = ROWS.iter().map(|row| row.0).collect();
        assert_eq!(rows, ROW_TESTS);
    }

    /// The [`ROWS`] row named `row`: its blend faces are definitions whose nets
    /// re-certify, its counts are the golden's (the bottle's rail split at the shoulder
    /// face's C1 knot; the `sphere_diff_3cylinders` rows keep the target's two sphere
    /// patches, WR-459), and a closed result's converged volume and mass values lie in
    /// their certified bands.
    fn check(row: &str) {
        let picked: Vec<&Row> = ROWS.iter().filter(|r| r.0 == row).collect();
        assert_eq!(picked.len(), 1, "{row}: one row of that name");
        for &&(name, closed, after) in &picked {
            let input = inputs(name).real_solid.expect("a real solid");
            assert_eq!(input.checkpoint, Checkpoint::Full, "{name}");
            let mut k = Kernel::default();
            let mut body = build(&mut k, &input).unwrap().unwrap();
            let (selected, _) = select_edges(&k, &body, &input).ok().unwrap();
            k.blend_face_log = Some(BlendFaceLog::default());
            let result = fillet_selected(&mut k, &mut body, &selected, &input).ok().unwrap();
            assert!(result.is_ok(), "{name}: {result:?}");
            no_bare_blend_face(&k, &body, name);
            assert_eq!(counts(&body, 0), after, "{name}");
            if name == "stubs4-delete-edge-bottle-e8-r0.1" {
                rail_splits_at_a_c1_knot(&k, &body);
            }
            let blends = certified_blends(&k, &mut body, name);
            if closed {
                let estimates = volume_band(&k, &mut body, &blends, name);
                mass_bands(&k, &mut body, &estimates, name);
            }
        }
    }

    /// The bottle's extra vertex: two edges between the same B-spline face and blend face
    /// meet at a vertex that lies, within its tolerance, on that face's iso-line at a u
    /// knot of multiplicity at least the degree less one (where the face is only C1, so the
    /// rail, the feet of the ball's centres, turns), the knot nearest the edges' trim end.
    fn rail_splits_at_a_c1_knot(k: &Kernel, b: &Brep) {
        use solidcore::math::Point2;
        use solidcore::nurbs::b_spline_surface::SurfParamType;
        use solidcore::surface::surface::SurfaceKind;
        let edges = p1::edges(b, None).unwrap();
        let faces_of = |e: EdgeId| {
            let mut f = edge::faces(b, e, None).unwrap();
            f.sort();
            f
        };
        let mut found = 0;
        for (i, &e1) in edges.iter().enumerate() {
            for &e2 in edges.iter().skip(i + 1) {
                let faces = faces_of(e1);
                if faces.len() != 2 || faces != faces_of(e2) {
                    continue;
                }
                let ends = |e: EdgeId| edge::vertices(b, e, None).unwrap();
                let Some(v) = ends(e1).into_iter().flatten().find(|v| ends(e2).contains(&Some(*v)))
                else {
                    continue;
                };
                let surface = |f| b.face(f).unwrap().surface.clone().unwrap();
                let (Some(&shoulder), Some(_)) = (
                    faces.iter().find(|&&f| matches!(surface(f).kind(), SurfaceKind::BSpline(_))),
                    faces.iter().find(|&&f| surface(f).procedural_blend().is_some()),
                ) else {
                    continue;
                };
                let s = surface(shoulder);
                let SurfaceKind::BSpline(net) = s.kind() else {
                    unreachable!()
                };
                let p = solidcore::topology::vertex::point(b, v).unwrap();
                let tol = solidcore::topology::vertex::tolerance(b, v).unwrap();
                // the end of e1's trim on the shoulder at the vertex
                let eu = edge::edgeuses(b, e1, None, None)
                    .unwrap()
                    .into_iter()
                    .find(|&u| eu::face(b, u).unwrap() == Some(shoulder))
                    .unwrap();
                let trim = eu::uv_trim_curve(b, eu).unwrap().0.unwrap();
                let ivl = trim.natural_interval();
                let uv = [ivl.min(), ivl.max()]
                    .into_iter()
                    .map(|t| {
                        let q = trim.evaluate_point(k, t).unwrap();
                        Point2::new(q.x, q.y)
                    })
                    .min_by(|a, c| {
                        let d = |w: &Point2| s.evaluate_point(k, *w).unwrap().distance(p);
                        d(a).total_cmp(&d(c))
                    })
                    .unwrap();
                let degree = net.degree(SurfParamType::U);
                let (knots, mults) = net.knots(SurfParamType::U, None).unwrap();
                // interior knots only: the clamped end knots bound the domain, where the
                // face is not C1-limited
                let last = knots.len() - 1;
                let (_, (&knot, _)) = knots
                    .iter()
                    .zip(&mults)
                    .enumerate()
                    .filter(|&(i, (_, &m))| i > 0 && i < last && m + 1 >= degree)
                    .min_by(|a, c| (a.1 .0 - uv.x).abs().total_cmp(&(c.1 .0 - uv.x).abs()))
                    .expect("an interior knot of multiplicity at least the degree less one");
                let on_iso = s.evaluate_point(k, Point2::new(knot, uv.y)).unwrap().distance(p);
                assert!(
                    on_iso <= tol,
                    "the split vertex is {on_iso:e} off the u = {knot} iso-line (tolerance {tol:e})"
                );
                found += 1;
            }
        }
        assert_eq!(found, 1, "one rail split at a C1 knot");
    }

    /// issue #2349's teardrop at its solver-geometry and corner-edge checkpoints: every
    /// generated fillet surface is a blend by its definition whose net re-certifies.
    #[test]
    fn teardrop_generated_rounds_are_certified_definitions() {
        for name in [
            "issue-2349-teardrop-solver_geometry-r0.5",
            "issue-2349-teardrop-corner_edges-r0.5",
        ] {
            let input = inputs(name).real_solid.expect("a real solid");
            let mut k = Kernel::default();
            let mut body = build(&mut k, &input).unwrap().unwrap();
            let (selected, before) = select_edges(&k, &body, &input).ok().unwrap();
            let mut certificates = Vec::new();
            prefix::run(
                &mut k,
                &mut body,
                &selected,
                &input,
                &before,
                Some(&mut certificates),
            )
            .unwrap();
            assert!(!certificates.is_empty(), "{name}: no generated surface");
            for (type_code, certified) in certificates {
                let (bound, d) =
                    certified.unwrap_or_else(|| panic!("{name}: a bare surface ({type_code})"));
                assert!(d <= bound, "{name}: proved {d:e} past its bound {bound:e}");
            }
        }
    }
}

/// WR-457's tangent side-edge skip, now the only path, builds the 30 box-union rounds of
/// `stubs_round3.json` and `corner_3x2.json` that upstream refused with status 1001. Each
/// built body is certified: `check_brep` Level 2 reports no failure, it is one closed
/// manifold solid, and its volume is enclosed by [`enclose_volume`] to an asserted width.
#[cfg(test)]
mod wr457_certificates {
    use super::{apply, build, RealSolid};
    use solidcore::kernel::Kernel;
    use solidcore::mass::enclosure::enclose_volume;
    use solidcore::surface::surface::p2::SurfaceKind;
    use solidcore::topology::brep::p1;
    use solidcore::topology::brep::p2::{create_missing_uv_trim_curves, is_manifold_solid};
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};

    /// The moved rows: (fixture file, case name).
    const CASES: &[(&str, &str)] = &[
        ("corner_3x2.json", "corner3x2-union-overhang-corner-edges6-12-r1"),
        ("corner_3x2.json", "corner3x2-union-overhang-corner-edges6-13-r1"),
        ("stubs_round3.json", "stubs3-union-overhang-corner-edges12-13-r1"),
        ("stubs_round3.json", "stubs3-union-overhang-corner-edges14-15-r1"),
        ("stubs_round3.json", "stubs3-union-overhang-corner-edges14-22-r1"),
        ("stubs_round3.json", "stubs3-union-overhang-corner-edges15-22-r1"),
        ("stubs_round3.json", "stubs3-union-overhang-front-edges14-15-r1"),
        ("stubs_round3.json", "stubs3-union-overhang-front-edges14-22-r1"),
        ("stubs_round3.json", "stubs3-union-overhang-front-edges15-16-r1"),
    ];

    /// The edges of the `nx1-multi-neighbour-end-face` cases (the box 20 x 10 x 10 and the
    /// box [5, 15] x [-2, 4] x [10, 20] standing on it, overhanging its front face y = 0),
    /// each rounded 0.5, 1 and 2, with the sign of the round's volume change: -1 for a
    /// convex edge, whose round takes material off, +1 for a concave one, whose round adds
    /// it. Convex: 9 and 12, the lower box's top front edge either side of the upper box;
    /// 17 and 19, the overhang's underside meeting the upper box's side faces. Concave: 13,
    /// the lower box's front face meeting the overhang's underside; 14 and 16, the lower
    /// box's top meeting the upper box's side faces.
    const NX1_EDGES: [(usize, f64); 7] = [
        (9, -1.0),
        (12, -1.0),
        (13, 1.0),
        (14, 1.0),
        (16, 1.0),
        (17, -1.0),
        (19, -1.0),
    ];

    /// The union's volume, 20 x 10 x 10 + 10 x 6 x 10.
    const UNION_VOLUME: f64 = 2600.0;

    /// The two-edge cases whose edges are both concave and meet at a corner where the
    /// rounds' ends are square to their edges: their volume has a closed form (see
    /// [`two_concave_rounds`]).
    const CONCAVE_PAIRS: [&str; 4] = [
        "stubs3-union-overhang-corner-edges12-13-r1",
        "stubs3-union-overhang-corner-edges14-15-r1",
        "stubs3-union-overhang-front-edges14-15-r1",
        "stubs3-union-overhang-front-edges15-16-r1",
    ];

    /// The two-edge cases that round one concave edge and one convex edge meeting at a
    /// vertex whose third edge is concave and not rounded, with the concave and convex
    /// edges' lengths: the corner patch where the rounds meet is a B-spline construction,
    /// not an exact surface, so these have no closed form; see [`Form::Mixed`].
    const MIXED: [(&str, f64, f64); 5] = [
        ("corner3x2-union-overhang-corner-edges6-12-r1", 4.0, 10.0),
        ("corner3x2-union-overhang-corner-edges6-13-r1", 8.0, 10.0),
        ("stubs3-union-overhang-corner-edges14-22-r1", 8.0, 10.0),
        ("stubs3-union-overhang-corner-edges15-22-r1", 4.0, 10.0),
        ("stubs3-union-overhang-front-edges14-22-r1", 4.0, 10.0),
    ];

    /// What closed form, if any, a case's volume must match.
    #[derive(Clone, Copy, Debug)]
    enum Form {
        /// One round, with the sign of its volume change.
        Single(f64),
        /// Two concave rounds meeting at a corner.
        ConcavePair,
        /// A concave and a convex round (their lengths) meeting at a corner patch: the
        /// body must be planes, the two rounds' cylinders and exactly one B-spline face
        /// (the patch), and its volume less the two straight rounds ending square at the
        /// corner, `2600 + A (concave - convex)`, must be one shared constant across the
        /// five cases (their intervals must meet): the same patch construction at r 1.
        Mixed(f64, f64),
    }

    /// Each case with the closed form its volume must hold.
    fn case_names() -> Vec<(&'static str, String, Form)> {
        let mut out: Vec<_> = CASES
            .iter()
            .map(|&(f, n)| {
                let form = if let Some(&(_, concave, convex)) = MIXED.iter().find(|m| m.0 == n) {
                    Form::Mixed(concave, convex)
                } else {
                    assert!(CONCAVE_PAIRS.contains(&n), "{n}");
                    Form::ConcavePair
                };
                (f, n.to_string(), form)
            })
            .collect();
        for (e, sign) in NX1_EDGES {
            for r in ["0.5", "1.0", "2.0"] {
                out.push((
                    "stubs_round3.json",
                    format!("stubs3-overhang-union-nx1-multi-neighbour-end-face-edge{e}-r{r}"),
                    Form::Single(sign),
                ));
            }
        }
        out
    }

    /// The volume of the union with two concave rounds of radius `r` whose edges, of
    /// summed length `length`, meet at a vertex whose third edge is convex. Each round
    /// adds its section, the square of side `r` less its quarter disc, of area
    /// `A = r^2 (1 - pi/4)`, along its edge up to the plane through the convex edge square
    /// to it. The common face's corner between the edges is concave, so the rounds are
    /// mitred there (WR-642, the owner's ruling of 2026-10-30; before its flag was retired
    /// the section turned a quarter about the convex edge, adding `(pi/2) A d` by Pappus):
    /// the mitre adds `r^3 (5/3 - pi/2)`, as `g5_blend_corner_certificates::mitred`
    /// derives. So the volume is `2600 + A L + r^3 (5/3 - pi/2)`. The premises (square
    /// ends, a convex third edge) are ARGUED from the fixture geometry; the cases are
    /// picked by name in [`CONCAVE_PAIRS`].
    fn two_concave_rounds(r: f64, length: f64) -> f64 {
        use core::f64::consts::{FRAC_PI_2, FRAC_PI_4};
        let a = r * r * (1.0 - FRAC_PI_4);
        UNION_VOLUME + a * length + r * r * r * (5.0 / 3.0 - FRAC_PI_2)
    }

    fn load(file: &str, name: &str) -> RealSolid {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/blend.fillet_executive_p3")
            .join(file);
        let text = std::fs::read_to_string(&path).expect("fixture");
        let json: serde_json::Value = serde_json::from_str(&text).expect("fixture json");
        let case = json["cases"]
            .as_array()
            .expect("cases")
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("no case {name}"));
        serde_json::from_value(case["inputs"]["real_solid"].clone()).expect("real_solid")
    }

    /// Every built body is valid at Level 2 and one closed manifold solid, and its volume
    /// is enclosed in an interval no wider than `1e-7 * 2600`. A single round's ends lie in
    /// the end faces' planes, square to its straight edge (every face is axis-aligned;
    /// ARGUED from the fixture geometry, not checked here), so its volume is the union's,
    /// changed by its section `r^2 (1 - pi/4)` along the edge's length `L`. Two concave
    /// rounds meeting at a convex corner hold [`two_concave_rounds`]. The enclosure must
    /// hold the closed form, up to its f64 rounding. The live harness row (which the
    /// goldens gate compares with `expected/blend.fillet_executive_p3.jsonl`) must record
    /// the same body: status 0, the body's vertex, edge and face counts, and a
    /// mass-properties volume within `1e-4` of the enclosure (MEASURED: the
    /// mass integrator's accuracy is an estimate, not a bound).
    #[test]
    fn the_rounds_upstream_refused_build_valid_closed_solids() {
        use core::f64::consts::FRAC_PI_4;
        let names = case_names();
        assert_eq!(names.len(), 30);
        let width = 1e-7 * UNION_VOLUME;
        // each mixed case's volume less its two straight rounds
        let mut corner: Vec<(f64, f64)> = Vec::new();
        for (file, name, form) in &names {
            let input = load(file, name);
            let mut k = Kernel::default();
            let mut body = build(&mut k, &input)
                .expect("fixture")
                .expect("the box union");
            let edges = p1::edges(&body, None).expect("edges");
            let picked = input.edge_indices().expect("edge indices");
            let selected: Vec<_> = picked
                .iter()
                .map(|&i| *edges.get(i).expect("selected edge"))
                .collect();
            let mut length = 0.0;
            for &e in &selected {
                let ivl = solidcore::topology::edge::interval(&body, e).expect("interval");
                let c = solidcore::topology::edge::curve(&body, e)
                    .expect("curve")
                    .expect("a curve");
                let a = c.evaluate_point(&k, ivl.min()).expect("start");
                let b = c.evaluate_point(&k, ivl.max()).expect("end");
                length += a.distance(b);
            }
            apply(&mut k, &mut body, &selected, &input)
                .unwrap_or_else(|e| panic!("{name}: the round failed: {e:?}"));
            // The round keeps the union's own faces (WR-459), which come without UV trims,
            // so the trims are made once here, as the enclosure would make them: the check
            // then runs on the body itself (#21105) and the trims its walk repairs, the
            // corner patch's edges among them, stay on the body the enclosure measures.
            create_missing_uv_trim_curves(&k, &mut body).expect("trims");
            let lines = check_brep(&mut k, &mut body, CheckLevel::Level2)
                .expect("check")
                .lines();
            assert!(lines.is_empty(), "{name}: Level 2: {lines:?}");
            assert!(
                is_manifold_solid(&body).expect("manifold check"),
                "{name}: not one closed manifold solid"
            );
            let e = enclose_volume(&k, &mut body, width).expect("enclosure");
            assert!(e.width() <= width, "{name}: enclosure {e:?} wider than {width}");
            eprintln!("{name}: volume in [{}, {}] ({:?})", e.lo, e.hi, e.stop);
            let r = input.radius;
            let exact = match *form {
                Form::Single(sign) => {
                    assert_eq!(selected.len(), 1, "{name}");
                    Some(UNION_VOLUME + sign * length * r * r * (1.0 - FRAC_PI_4))
                }
                Form::ConcavePair => {
                    assert_eq!(selected.len(), 2, "{name}");
                    Some(two_concave_rounds(r, length))
                }
                Form::Mixed(concave, convex) => {
                    assert_eq!(selected.len(), 2, "{name}");
                    assert!((length - (concave + convex)).abs() < 1e-9, "{name}: {length}");
                    let (mut patches, mut cylinders) = (0, 0);
                    for f in p1::faces(&body, None).expect("faces") {
                        let surface = solidcore::topology::face::surface(&body, f)
                            .expect("surface")
                            .expect("a surface");
                        match surface.kind() {
                            SurfaceKind::Plane(_) => {}
                            SurfaceKind::Cone(_) => cylinders += 1,
                            SurfaceKind::BSpline(_) => patches += 1,
                            other => panic!("{name}: unexpected face kind {other:?}"),
                        }
                    }
                    assert_eq!((patches, cylinders), (1, 2), "{name}: face kinds");
                    let straight =
                        UNION_VOLUME + (concave - convex) * r * r * (1.0 - FRAC_PI_4);
                    let slack = 16.0 * f64::EPSILON * straight;
                    corner.push((e.lo - straight - slack, e.hi - straight + slack));
                    None
                }
            };
            if let Some(exact) = exact {
                // About ten f64 operations, plus about ten ulps of d from cancelling 3 pi
                // and the rounding of the stored pi constants, all on terms of at most 9
                // added to 2600: 16 eps |exact| bounds the error with ample margin.
                let slack = 16.0 * f64::EPSILON * exact.abs();
                assert!(
                    e.lo - slack <= exact && exact <= e.hi + slack,
                    "{name}: closed form {exact} outside [{}, {}]",
                    e.lo,
                    e.hi
                );
            }
            let row = super::rust(&input).unwrap_or_else(|e| panic!("{name}: harness: {e:?}"));
            let counts = [
                0,
                p1::vertices(&body, None).expect("vertices").len() as i64,
                p1::edges(&body, None).expect("edges").len() as i64,
                p1::faces(&body, None).expect("faces").len() as i64,
            ];
            assert_eq!(
                row.ints["api_status_vertex_edge_face_counts"], counts,
                "{name}: row status and counts"
            );
            assert_eq!(row.ints["mass_status"], [0], "{name}: mass status");
            let volume = *row.values["mass_properties"].get(1).expect("volume");
            assert!(
                e.lo - 1e-4 <= volume && volume <= e.hi + 1e-4,
                "{name}: row volume {volume} not within 1e-4 of [{}, {}]",
                e.lo,
                e.hi
            );
        }
        // one patch construction: the five corners' contributions share a value
        assert_eq!(corner.len(), MIXED.len());
        let lo = corner.iter().map(|c| c.0).fold(f64::NEG_INFINITY, f64::max);
        let hi = corner.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
        eprintln!("mixed corners: their contribution in [{lo}, {hi}]");
        assert!(lo <= hi, "the mixed corners differ: {corner:?}");
    }
}

/// WR-455 (#17404): moving a contact to the curve end it runs on to moved one row,
/// `stubs4-degenerate-brepForFillet2-e9-r0.5`. The round of edge 9 of `brepForFillet2.smb`
/// builds with main's status, counts and topology record, and its volume moves by about
/// `2.2e-7` in 3616. The body is certified, not compared with main's bits.
#[cfg(test)]
mod wr455_certificates {
    use super::{apply, build, RealSolid};
    use solidcore::kernel::Kernel;
    use solidcore::mass::enclosure::enclose_volume;
    use solidcore::topology::brep::p1;
    use solidcore::topology::brep::p2::is_manifold_solid;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};

    const NAME: &str = "stubs4-degenerate-brepForFillet2-e9-r0.5";

    /// The row's input, read as the goldens gate reads it (upstream file paths resolved).
    fn load() -> RealSolid {
        let case = crate::case::regression_case(&super::super::Runner, NAME);
        let inputs: super::super::Inputs = case.parse().expect("inputs");
        inputs.real_solid.expect("a real_solid case")
    }

    /// The built body is valid at Level 2 (PROVEN as far as `check_brep` Level 2 checks)
    /// and one closed manifold solid. Its volume is enclosed in an interval no wider than
    /// `0.02` (PROVEN, [`enclose_volume`]; the cell budget stops it near `0.0127`). The harness row records that body: status
    /// 0, its vertex, edge and face counts, and a mass-properties volume within `1e-4` of
    /// the enclosure (MEASURED: the integrator's accuracy is an estimate, not a bound).
    #[test]
    fn the_moved_round_builds_a_valid_closed_solid() {
        let input = load();
        let mut k = Kernel::default();
        let mut body = build(&mut k, &input)
            .expect("fixture")
            .expect("brepForFillet2");
        let edges = p1::edges(&body, None).expect("edges");
        let selected: Vec<_> = input
            .edge_indices()
            .expect("edge indices")
            .iter()
            .map(|&i| *edges.get(i).expect("selected edge"))
            .collect();
        apply(&mut k, &mut body, &selected, &input)
            .unwrap_or_else(|e| panic!("{NAME}: the round failed: {e:?}"));
        let lines = check_brep(&mut k, &mut body, CheckLevel::Level2)
            .expect("check")
            .lines();
        assert!(lines.is_empty(), "{NAME}: Level 2: {lines:?}");
        assert!(
            is_manifold_solid(&body).expect("manifold check"),
            "{NAME}: not one closed manifold solid"
        );
        let width = 0.02;
        let e = enclose_volume(&k, &mut body, width).expect("enclosure");
        assert!(e.width() <= width, "{NAME}: enclosure {e:?} wider than {width}");
        eprintln!("{NAME}: volume in [{}, {}] ({:?})", e.lo, e.hi, e.stop);
        let row = super::rust(&input).unwrap_or_else(|e| panic!("{NAME}: harness: {e:?}"));
        let counts = [
            0,
            p1::vertices(&body, None).expect("vertices").len() as i64,
            p1::edges(&body, None).expect("edges").len() as i64,
            p1::faces(&body, None).expect("faces").len() as i64,
        ];
        assert_eq!(counts, [0, 11, 17, 8], "{NAME}: main's counts");
        assert_eq!(
            row.ints["api_status_vertex_edge_face_counts"], counts,
            "{NAME}: row status and counts"
        );
        assert_eq!(row.ints["mass_status"], [0], "{NAME}: mass status");
        let volume = *row.values["mass_properties"].get(1).expect("volume");
        assert!(
            e.lo - 1e-4 <= volume && volume <= e.hi + 1e-4,
            "{NAME}: row volume {volume} not within 1e-4 of [{}, {}]",
            e.lo,
            e.hi
        );
    }
}

/// #18930: with the tangent curve tracer (then the opt-in `ssi_tangent_curve_tracer`) on,
/// the round of edge 8 of `bottle.smb` at r 0.1 (`stubs4-delete-edge-bottle-e8-r0.1`) failed
/// (status 1001). In two intersections of a round with its support face the tracer
/// reported 16 and 11 touching points, all on the rail, on the edge of the round's domain,
/// where the marcher returns a tangent curve: contacts on that one curve, taken for nodes
/// where crossing branches meet. The round builds a manifold body of 31 vertices, 47 edges
/// and 18 faces, the counts the row records (the row holds the rest of the body).
#[cfg(test)]
mod wr75_rail_contacts {
    use super::{apply, build, RealSolid};
    use solidcore::kernel::Kernel;
    use solidcore::topology::brep::p1;
    use solidcore::topology::brep::p2::is_manifold_solid;

    const NAME: &str = "stubs4-delete-edge-bottle-e8-r0.1";

    #[test]
    fn a_round_touching_its_face_along_the_rail_builds_with_the_tracer() {
        let case = crate::case::regression_case(&super::super::Runner, NAME);
        let inputs: super::super::Inputs = case.parse().expect("inputs");
        let input: RealSolid = inputs.real_solid.expect("a real_solid case");
        let mut k = Kernel::default();
        let mut body = build(&mut k, &input).expect("fixture").expect("bottle");
        let edges = p1::edges(&body, None).expect("edges");
        let selected: Vec<_> = input
            .edge_indices()
            .expect("edge indices")
            .iter()
            .map(|&i| *edges.get(i).expect("selected edge"))
            .collect();
        apply(&mut k, &mut body, &selected, &input)
            .unwrap_or_else(|e| panic!("{NAME}: the round failed: {e:?}"));
        assert!(
            is_manifold_solid(&body).expect("manifold check"),
            "{NAME}: not a manifold solid"
        );
        let counts = [
            p1::vertices(&body, None).expect("vertices").len(),
            p1::edges(&body, None).expect("edges").len(),
            p1::faces(&body, None).expect("faces").len(),
        ];
        assert_eq!(counts, [31, 47, 18], "{NAME}: vertices, edges, faces");
    }
}

/// WR-56 (#17467): the two rows it moved, `source-file-6_sided_corner-all-edges` and
/// `source-file-nurb_box-all-edges`, round every edge of a B-spline body. Their mass
/// properties did not move (bit for bit), and their topology and geometry records hold the
/// same values in another order (the same body, its entities numbered differently).
/// Each body is certified: `check_brep` Level 2 reports nothing, it is one manifold solid
/// of positive volume, and its volume is enclosed ([`enclose_volume`]; asked for a relative
/// width of 1e-7, the budget stops it at 2.3e-6 and 1.2e-5, asserted under 2e-5); the
/// row's volume (the mass integrator's) lies in the enclosure.
///
/// [`enclose_volume`]: solidcore::mass::enclosure::enclose_volume
#[cfg(test)]
mod wr56_certificates {
    use crate::UnitDiff;
    use solidcore::mass::enclosure::VolumeSign;

    #[test]
    fn the_moved_rounds_are_valid_solids_with_enclosed_volumes() {
        for name in [
            "source-file-6_sided_corner-all-edges",
            "source-file-nurb_box-all-edges",
        ] {
            let case = crate::case::regression_case(&super::super::Runner, name);
            crate::soundness::start_enclosing(1e-7);
            let row = super::super::Runner.run_rust(&case);
            let found = crate::soundness::take();
            let row = row.unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!(found.len(), 1, "{name}: one result body");
            let s = &found[0];
            assert!(s.sound(), "{name}: {}", s.line());
            assert_eq!(s.manifold, Some(true), "{name}: {}", s.line());
            assert!(matches!(s.volume, Some(VolumeSign::Positive(_))), "{name}: {}", s.line());
            let e = match &s.enclosure {
                Some(Ok(e)) => *e,
                other => panic!("{name}: no enclosure: {other:?}"),
            };
            // the enclosure is certified whatever width its budget reached
            eprintln!("{name}: enclosure {e:?}, relative width {:e}", (e.hi - e.lo) / e.hi);
            assert!(e.hi - e.lo <= 2e-5 * e.hi, "{name}: {e:?} is wider than 2e-5");
            let mass = row.values.get("mass_properties").expect("mass properties");
            let volume = *mass.get(1).expect("volume");
            eprintln!(
                "{name}: volume in [{}, {}], row {volume}; {}",
                e.lo,
                e.hi,
                s.line()
            );
            assert!(
                e.lo <= volume && volume <= e.hi,
                "{name}: row volume {volume} not in [{}, {}]",
                e.lo,
                e.hi
            );
        }
    }
}

/// WR-493 (#18195): `stubs4-sphere_plane-e3-r0.5` rounds the circle edge of
/// `Fillet/sphere_plane.smb` (the ball of radius 5 about the origin cut by the plane
/// x = -4) with radius 0.5. Its result geometry moved: the file's sphere now keeps its own
/// NURB and trims. The exact round is closed form: the ball of radius 0.5 rolls with its
/// centre on x = -3.5 at distance 4.5 from the origin, so the round is the torus about the
/// x axis with major radius sqrt(8) and minor radius 0.5, through the circle x = -4,
/// rho = sqrt(8) on the plane and the circle on the sphere at 10/9 of the centre circle.
/// The round face's net and its contact edges on the plane and on the sphere each lie
/// within their own tolerance (4e-5) of it, enclosed over every rational Bezier piece (at
/// most 2.3e-6, 8.8e-12 and 2.1e-6).
#[cfg(test)]
mod wr493_certificates {
    use super::{apply, build, eu, RealSolid};
    use crate::interval::{exact_sum, homogeneous, two_prod, Bern, Iv};
    use solidcore::curve::curve::CurveKind;
    use solidcore::kernel::Kernel;
    use solidcore::nurbs::math::CPoint;
    use solidcore::nurbs::srf_tool::p1::decompose_to_bez;
    use solidcore::topology::{edge, Brep, EdgeId, FaceId};
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::brep::p1;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};

    const NAME: &str = "stubs4-sphere_plane-e3-r0.5";

    fn load() -> RealSolid {
        let case = crate::case::regression_case(&super::super::Runner, NAME);
        let inputs: super::super::Inputs = case.parse().expect("inputs");
        inputs.real_solid.expect("a real_solid case")
    }

    /// The round on `NAME`'s body.
    fn round() -> (Kernel, Brep) {
        let input = load();
        let mut k = Kernel::default();
        let mut body = build(&mut k, &input).expect("fixture").expect("file");
        let edges = p1::edges(&body, None).expect("edges");
        let selected: Vec<_> = input
            .edge_indices()
            .expect("edge indices")
            .iter()
            .map(|&i| *edges.get(i).expect("selected edge"))
            .collect();
        apply(&mut k, &mut body, &selected, &input).expect("the round");
        (k, body)
    }

    /// What a face of the rounded body is: the file's plane (0), its sphere (1) or the
    /// round (2).
    fn role(b: &Brep, f: FaceId) -> usize {
        match b.face(f).expect("face").surface.as_ref().expect("surface").kind() {
            SurfaceKind::Plane(_) => 0,
            SurfaceKind::Sphere(_) => 1,
            SurfaceKind::ProceduralBlend(_) => 2,
            other => panic!("{NAME}: a face {other:?}"),
        }
    }

    /// The faces an edge's uses lie on, by [`role`], sorted.
    fn sides(b: &Brep, e: EdgeId) -> Vec<usize> {
        let mut out: Vec<usize> = edge::edgeuses(b, e, None, None)
            .expect("uses")
            .into_iter()
            .map(|u| role(b, eu::face(b, u).expect("face").expect("a face")))
            .collect();
        out.sort_unstable();
        out
    }

    /// The round face and its edges: on the plane, on the sphere, and its seam.
    struct Parts {
        face: FaceId,
        on_plane: EdgeId,
        on_sphere: EdgeId,
        seam: EdgeId,
    }

    fn parts(b: &Brep) -> Parts {
        let faces = p1::faces(b, None).expect("faces");
        let round: Vec<_> = faces.iter().copied().filter(|&f| role(b, f) == 2).collect();
        let &[face] = round.as_slice() else {
            panic!("{NAME}: {} round faces, not one", round.len());
        };
        let edges = p1::edges(b, None).expect("edges");
        let find = |want: &[usize]| {
            let found: Vec<_> = edges
                .iter()
                .copied()
                .filter(|&e| {
                    let mut s = sides(b, e);
                    s.dedup();
                    s == want
                })
                .collect();
            let &[e] = found.as_slice() else {
                panic!("{NAME}: {} edges between {want:?}", found.len());
            };
            e
        };
        Parts {
            face,
            on_plane: find(&[0, 2]),
            on_sphere: find(&[1, 2]),
            seam: find(&[2]),
        }
    }

    /// The exact round's torus: about the x axis, its centre circle at `x = CX` of radius
    /// `sqrt(R2)`, its tube radius `TUBE`.
    const CX: f64 = -3.5;
    /// `R^2`.
    const R2: f64 = 8.0;
    /// `r`.
    const TUBE: f64 = 0.5;

    /// A proved upper bound of the distance from the torus over a rational Bezier piece
    /// (`net[i][j]`, a curve's as one row of one column each).
    ///
    /// With `q = p - (CX, 0, 0)`, `rho^2 = q_y^2 + q_z^2` and `s` the distance from the
    /// centre circle (the distance from the torus is `|s - r|`), the torus's implicit
    /// function factors:
    /// `F = (|q|^2 + R^2 - r^2)^2 - 4 R^2 rho^2 = (s - r) (s + r) G`,
    /// `G = (rho + R)^2 + q_x^2 - r^2`, so `|s - r| = |F| / ((s + r) G)`. On the piece
    /// `p = X / W` and, with `Y = X - (CX, 0, 0) W`,
    /// `F W^4 = (|Y|^2 + (R^2 - r^2) W^2)^2 - 4 R^2 W^2 (Y_y^2 + Y_z^2)`: a polynomial whose
    /// Bernstein coefficients, in interval arithmetic from the exact control points, bound
    /// it over the piece. The piece is halved `depth` times each way (de Casteljau, `X`,
    /// `W` and `F W^4` together); on each part `W >= min w > 0` (its weights' hull), the
    /// point lies in the box of its control points `X_i / W_i` (a convex combination,
    /// the weights positive), and `(s + r) G` is bounded below over that box in interval
    /// arithmetic. The part's bound is `max |F W^4| / (min W^4 min (s + r) G)`.
    fn torus_bound(net: &[Vec<CPoint>], depth: u32) -> f64 {
        let [x, y, z, w] = homogeneous(net);
        let yx = x.zip(&w, |a, b| a.sub(b.mul(Iv::exact(CX))));
        let k = R2 - TUBE * TUBE;
        let ww = w.mul(&w);
        let rho2 = y.mul(&y).zip(&z.mul(&z), Iv::add);
        let a = yx.mul(&yx).zip(&rho2, Iv::add).zip(&ww.scale(k), Iv::add);
        let f = a.mul(&a).zip(&ww.mul(&rho2).scale(4.0 * R2), Iv::sub);
        let mut parts = vec![[x, y, z, w, f]];
        for _ in 0..depth {
            let mut next = Vec::new();
            for part in parts {
                let curve = part[0].q == 0;
                let halves = |p: &[Bern; 5], along_u: bool| {
                    let s = p.each_ref().map(|b| b.split(along_u));
                    [s.clone().map(|h| h.0), s.map(|h| h.1)]
                };
                for half in halves(&part, true) {
                    if curve {
                        next.push(half);
                    } else {
                        next.extend(halves(&half, false));
                    }
                }
            }
            parts = next;
        }
        let square = |v: Iv| {
            let m = v.mul(v);
            if v.mig() == 0.0 {
                Iv { lo: 0.0, hi: m.hi }
            } else {
                m
            }
        };
        let (r, big_r) = (Iv::exact(TUBE), Iv::exact(R2).sqrt());
        let mut out = 0.0_f64;
        for [x, y, z, w, f] in &parts {
            let wmin = w.c.iter().map(|c| c.lo).fold(f64::INFINITY, f64::min);
            assert!(wmin > 0.0, "weights not positive");
            let hull = |b: &Bern, shift: f64| {
                b.c.iter()
                    .zip(&w.c)
                    .map(|(&c, &wc)| c.div(wc).sub(Iv::exact(shift)))
                    .reduce(|p, q| Iv {
                        lo: p.lo.min(q.lo),
                        hi: p.hi.max(q.hi),
                    })
                    .expect("control points")
            };
            let (qx, py, pz) = (hull(x, CX), hull(y, 0.0), hull(z, 0.0));
            let rho = square(py).add(square(pz)).sqrt();
            let qx2 = square(qx);
            let s = square(rho.sub(big_r)).add(qx2).sqrt();
            let g = square(rho.add(big_r)).add(qx2).sub(r.mul(r));
            let den = s.add(r).mul(g);
            assert!(den.lo > 0.0, "(s + r) G not proved positive");
            let w4 = Iv::exact(wmin).mul(Iv::exact(wmin));
            let bound = Iv::exact(f.hull_mag(0)).div(w4.mul(w4).mul(den));
            out = out.max(bound.hi);
        }
        out
    }

    /// An edge's curve as rational Bezier pieces, each with its parameter span; its
    /// interval is its NURB's whole knot domain (asserted).
    fn edge_pieces(b: &Brep, e: EdgeId) -> Vec<Vec<Vec<CPoint>>> {
        let edge = b.edge(e).expect("edge");
        let CurveKind::BSpline(curve) = edge.curve.as_ref().expect("curve").kind() else {
            panic!("{NAME}: edge {e:?} not a B-spline");
        };
        let nurb = curve.gw_nurb().expect("nurb");
        let knots = nurb.knots();
        let (a, z) = (knots[nurb.degree()], knots[knots.len() - 1 - nurb.degree()]);
        assert_eq!(
            (edge.interval.min(), edge.interval.max()),
            (a, z),
            "{NAME}: edge {e:?} interval"
        );
        nurb.decompose_bez()
            .expect("pieces")
            .iter()
            .map(|p| p.cpts().iter().map(|&c| vec![c]).collect())
            .collect()
    }

    /// The round face's net as rational Bezier pieces, and the net's certified distance
    /// from the round's definition, which the face evaluates.
    fn face_pieces(b: &Brep, f: FaceId) -> (Vec<Vec<Vec<CPoint>>>, f64) {
        let surface = b.face(f).expect("face").surface.as_ref().expect("surface");
        let SurfaceKind::ProceduralBlend(blend) = surface.kind() else {
            panic!("{NAME}: the round not a procedural blend");
        };
        let nurb = blend.net().gw_nurb().expect("net");
        let pieces = decompose_to_bez(nurb)
            .expect("pieces")
            .into_iter()
            .flatten()
            .map(|p| p.cpts())
            .collect();
        (pieces, blend.bound())
    }

    /// The largest [`torus_bound`] over pieces.
    fn largest_above(pieces: &[Vec<Vec<CPoint>>], depth: u32) -> f64 {
        pieces
            .iter()
            .map(|net| torus_bound(net, depth))
            .fold(0.0, f64::max)
    }

    /// The distance of `p` from the circle `x = x0`, `y^2 + z^2 = rho0^2`, with
    /// `x0 = xn / xd` and `rho0^2 = qn / qd` (`xd`, `qd` small integers): from
    /// `(xd x - xn) / xd` and `(qd rho^2 - qn) / (qd (rho + rho0))`, each numerator summed
    /// exactly from exact products ([`exact_sum`], [`two_prod`]), the rest in interval
    /// arithmetic.
    fn off_circle(p: [f64; 3], xn: f64, xd: f64, qn: f64, qd: f64) -> Iv {
        let (a, b) = two_prod(xd, p[0]);
        let dx = exact_sum(&[a, b, -xn]).div(Iv::exact(xd));
        let mut terms = Vec::new();
        for c in [p[1], p[2]] {
            let (h, l) = two_prod(c, c);
            for part in [h, l] {
                let (s, e) = two_prod(qd, part);
                terms.extend([s, e]);
            }
        }
        terms.push(-qn);
        let rho = Iv::exact(p[1])
            .mul(Iv::exact(p[1]))
            .add(Iv::exact(p[2]).mul(Iv::exact(p[2])))
            .sqrt();
        let rho0 = Iv::exact(qn).div(Iv::exact(qd)).sqrt();
        let dr = exact_sum(&terms).div(Iv::exact(qd).mul(rho.add(rho0)));
        let d = dx.mul(dx).add(dr.mul(dr)).sqrt();
        Iv {
            lo: d.lo.max(0.0),
            hi: d.hi,
        }
    }

    /// The `sphere_plane` round against its exact torus (about the x axis, centre circle
    /// `x = -3.5`, `y^2 + z^2 = 8`, tube 0.5: the ball of radius 0.5 touching the plane
    /// `x = -4` and, inside, the sphere of radius 5 about the origin).
    ///
    /// - The body passes the level-2 check.
    /// - The contact edges' curves and the round face's net (rational Bezier pieces): each
    ///   piece's distance from the torus is enclosed ([`torus_bound`]) and lies within
    ///   the edge's or the face's tolerance (the net's with its certified distance from
    ///   the round's definition, which the face evaluates). The seam (a circle of the
    ///   torus, one piece) is within `1e-12`.
    /// - Topology: the round's edges follow the file's three, the sphere's contact edge
    ///   first (`EdgeId(5)`), then the plane's and the seam.
    /// - Geometry (`util::primitive::append`): the round face, its edges and its two
    ///   vertices have the tolerance the net and curves are made to (`4e-5`). The vertex on
    ///   the plane lies on `x = -4`, `2.4e-15` from its exact contact circle (a few units in
    ///   the last place of `sqrt(8)`; its certified upper bound, below `2.5e-15`, is
    ///   pinned); both round vertices are within their tolerance of their circles (proved).
    #[test]
    fn the_round_is_within_its_tolerance_of_its_torus() {
        let (mut k, new) = round();
        // (on a copy: the check writes the edges' curves, which the record below measures)
        let mut checked = new.clone_standing_for_self().expect("copy");
        let lines = check_brep(&mut k, &mut checked, CheckLevel::Level2)
            .expect("check")
            .lines();
        assert!(lines.is_empty(), "{NAME}: Level 2: {lines:?}");
        let pn = parts(&new);

        // the curves and the net
        for (what, en) in [
            ("edge on the plane", pn.on_plane),
            ("edge on the sphere", pn.on_sphere),
        ] {
            let now = largest_above(&edge_pieces(&new, en), 8);
            let tol = new.edge(en).expect("edge").tolerance;
            eprintln!("{NAME}: {what} at most {now:e} off the torus (tol {tol:e})");
            assert!(now <= tol, "{NAME}: {what} {now:e} beyond its tolerance {tol:e}");
        }
        let off = largest_above(&edge_pieces(&new, pn.seam), 4);
        assert!(off <= 1e-12, "{NAME}: the seam {off:e} off the torus");
        let (new_net, net_bound) = face_pieces(&new, pn.face);
        let now = largest_above(&new_net, 3);
        let tol = new.face(pn.face).expect("face").tolerance;
        eprintln!("{NAME}: net at most {now:e} off the torus, from its definition {net_bound:e} (tol {tol:e})");
        assert!(now + net_bound <= tol, "{NAME}: the net beyond the face's tolerance");

        // topology
        let edges = p1::edges(&new, None).expect("edges");
        assert_eq!(edges[3..], [pn.on_sphere, pn.on_plane, pn.seam]);
        assert_eq!(
            [pn.on_sphere, pn.on_plane, pn.seam],
            [5, 6, 7].map(EdgeId::from_index)
        );

        // geometry: the tolerances (vertices first, then edges, then faces: sphere, plane,
        // round) and the round's vertices' positions
        let mut g = Vec::new();
        crate::util::primitive::append(&Kernel::default(), &new, &mut g).expect("geometry");
        assert_eq!(g.len(), 4 * 4 + 6 * 18 + 3 * 18, "{NAME}: geometry length");
        let tol = 4e-5;
        for e in [pn.on_sphere, pn.on_plane, pn.seam] {
            let at = 16 + 18 * edges.iter().position(|&x| x == e).expect("edge");
            assert_eq!(g[at], tol, "{NAME}: edge {e:?} tolerance");
        }
        assert_eq!(g[124 + 2 * 18], tol, "{NAME}: the round face's tolerance");
        let vertices: Vec<&[f64]> = g[..16].chunks(4).collect();
        for (i, v) in vertices.iter().enumerate().skip(2) {
            assert_eq!(v[3], tol, "{NAME}: vertex {i} tolerance");
        }
        let point = |v: &[f64]| [v[0], v[1], v[2]];
        assert!(vertices[2][0] == -4.0, "{NAME}: the plane vertex off x = -4");
        let now = off_circle(point(vertices[2]), -4.0, 1.0, 8.0, 1.0);
        eprintln!("{NAME}: plane vertex {now:?} off its circle");
        // a few units in the last place: pinned, and within the vertex's tolerance
        assert!(now.hi < 2.5e-15, "{NAME}: plane vertex {now:?}");
        assert!(now.hi <= vertices[2][3], "{NAME}: plane vertex {now:?} beyond its tolerance");
        let now = off_circle(point(vertices[3]), -35.0, 9.0, 800.0, 81.0);
        eprintln!("{NAME}: sphere vertex {now:?} off its circle");
        assert!(now.hi <= vertices[3][3], "{NAME}: sphere vertex {now:?} beyond its tolerance");
    }
}

/// WR-661 (#14246, `fillet_statuses_pass_on` retired): a fillet of an open sheet may keep
/// its result. The stitched result has lamina edges, so the closed-solid orientation's
/// "face uses gathered from both sides" check fires (walking a face use's neighbours
/// crosses the sheet's boundary to its other side) without the sheet being
/// non-orientable. The executive keeps the stitched body only when the input was open,
/// every lamina edge of the result has its ends on the input's boundary, no lamina edge
/// runs along another (an unstitched seam), and, once the filleting is done, the body
/// passes the level-2 check; otherwise the 1005 is returned and the body is restored.
/// The `degenerate-test5` and `shell_fillet2` rows pass all three and are kept, as with the
/// status ignored; the open-shell row fails level 2 and is refused.
#[cfg(test)]
mod wr661_open_sheets {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::round_definition_rows::inputs;
    use super::*;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};

    /// `name`'s sheet is open before and after; the fillet adds one face, and the
    /// result passes the level-2 check.
    fn keeps_the_filleted_sheet(name: &str, faces_before: usize) {
        let input = inputs(name).real_solid.expect("a real solid");
        let mut k = Kernel::default();
        let mut body = build(&mut k, &input).unwrap().unwrap();
        assert_eq!(p1::faces(&body, None).unwrap().len(), faces_before, "{name}");
        assert!(!solidcore::topology::brep::p2::is_manifold_solid(&body).unwrap(), "{name}");
        let (selected, _) = select_edges(&k, &body, &input).ok().unwrap();
        let result = fillet_selected(&mut k, &mut body, &selected, &input).ok().unwrap();
        assert!(result.is_ok(), "{name}: {result:?}");
        assert_eq!(p1::faces(&body, None).unwrap().len(), faces_before + 1, "{name}");
        assert!(!solidcore::topology::brep::p2::is_manifold_solid(&body).unwrap(), "{name}");
        let lines = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap().lines();
        assert!(lines.is_empty(), "{name}: {lines:?}");
    }

    #[test]
    fn a_degenerate_test5_sheet_fillet_is_kept() {
        keeps_the_filleted_sheet("stubs4-degenerate-test5-e2-r0.5", 4);
    }

    #[test]
    fn a_shell_fillet2_sheet_fillet_is_kept() {
        keeps_the_filleted_sheet("stubs4-shell_fillet2-e5-r0.5", 3);
    }

    /// The open-shell sheet is open too, and its stitched result has its ends on the sheet's
    /// boundary. Its nine level-2 gaps (up to 3.0e-5, tangency-labs/solidcraft#13733) were
    /// vertex and edge gaps the healer had left under their tolerances; with them covered
    /// (#18230 for edges, #17739 for vertices) the result passes the check and is kept.
    #[test]
    fn an_open_shell_sheet_fillet_is_kept() {
        keeps_the_filleted_sheet("stubs4-open_shell-e5-r0.5", 6);
    }
}

/// The G5 blend-corner flag retirement (#14246) of WR-642's mitred concave face corner
/// (`fillet_mitre_concave_face_corners`) moved 10 rows.
#[cfg(test)]
mod g5_blend_corner_certificates {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::round_definition_rows::inputs;
    use super::{apply, build, select_edges};
    use solidcore::kernel::Kernel;
    use solidcore::mass::enclosure::enclose_volume;
    use solidcore::topology::brep::p2::is_manifold_solid;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    use solidcore::topology::edge;

    /// The rows the mitred concave face corner moved, each two concave rounds r 1 of the
    /// box union meeting at one corner whose third edge is convex (a boss's root at its
    /// sharp corner, WR-642's case), with the volume its golden row held before.
    const MITRE: &[(&str, f64)] = &[
        ("stubs3-union-inset-side-step-edges13-15-r1", 2603.938129447188),
        ("stubs3-union-inset-side-step-edges13-17-r1", 2602.865120264477),
        ("stubs3-union-inset-side-step-edges14-15-r1", 2603.938129447186),
        ("stubs3-union-inset-side-step-edges14-18-r1", 2602.8651202644614),
        ("stubs3-union-inset-side-step-edges16-17-r1", 2603.2943239376245),
        ("stubs3-union-inset-side-step-edges16-18-r1", 2603.294323937609),
        ("stubs3-union-overhang-corner-edges12-13-r1", 2602.65051842809),
        ("stubs3-union-overhang-corner-edges14-15-r1", 2602.65051842809),
        ("stubs3-union-overhang-front-edges14-15-r1", 2603.0797221012954),
        ("stubs3-union-overhang-front-edges15-16-r1", 2603.0797221012954),
    ];

    /// The union's volume, 20 x 10 x 10 + 10 x 6 x 10.
    const UNION_VOLUME: f64 = 2600.0;

    /// The volume of the union with two concave rounds of radius `r` along edges of summed
    /// length `length` that meet at a corner whose third edge is convex, mitred there
    /// (WR-642: the owner's ruling of 2026-10-30, the fork's 4f06e72). Each round adds its
    /// section, the square of side `r` less its quarter disc, `A = r^2 (1 - pi/4)`, along
    /// its edge up to the corner, and the mitre fills the square of side `r` beyond the
    /// corner up to the lower of the two sections: with `g(t) = r - sqrt(r^2 - (r - t)^2)`
    /// the section's height `t` from its wall (decreasing), that is
    /// `int int g(max(x, y)) = int_0^r 2 m g(m) dm = r^3 - 2 int_0^r (r - u) sqrt(r^2 - u^2) du
    /// = r^3 - (pi r^3 / 2 - 2 r^3 / 3) = r^3 (5/3 - pi/2)` (PROVEN; the corner term of
    /// WR-642's closed forms). The premises (straight axis-aligned edges meeting square, a
    /// convex third edge) are ARGUED from the fixture geometry and the edges' lengths.
    fn mitred(r: f64, length: f64) -> f64 {
        use core::f64::consts::{FRAC_PI_2, FRAC_PI_4};
        UNION_VOLUME + r * r * (1.0 - FRAC_PI_4) * length + r * r * r * (5.0 / 3.0 - FRAC_PI_2)
    }

    /// Main's swept corner: the section turned a quarter about the convex edge (Pappus),
    /// `(pi/2) A d` with `d = r (10 - 3 pi) / (12 - 3 pi)` the section's centroid distance
    /// from its square corner (the former closed form of `wr457_certificates`).
    fn swept(r: f64, length: f64) -> f64 {
        use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        let a = r * r * (1.0 - FRAC_PI_4);
        let d = r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
        UNION_VOLUME + a * length + FRAC_PI_2 * a * d
    }

    /// Each mitred row builds one closed manifold solid with no Level 2 report whose
    /// enclosed volume (an interval no wider than `1e-7 * 2600`) holds the mitred closed
    /// form [`mitred`] (up to its f64 rounding, `16 eps |V|` as in `wr457_certificates`),
    /// and excludes main's swept corner [`swept`], which the golden held before (the
    /// golden's old volume, an estimate, lies within `1e-4` of it). So each row's volume
    /// is PROVEN closer to the ruled shape's than main's: main's is `(5/3 - pi/2) -
    /// (pi/2) A d = 0.02057` away, the new one inside the enclosure.
    #[test]
    fn mitred_rows_hold_the_mitred_closed_form() {
        let width = 1e-7 * UNION_VOLUME;
        for &(name, before) in MITRE {
            let input = inputs(name).real_solid.expect("a real solid");
            let mut k = Kernel::default();
            let mut body = build(&mut k, &input).unwrap().unwrap();
            let (selected, _) = select_edges(&k, &body, &input).ok().unwrap();
            assert_eq!(selected.len(), 2, "{name}");
            let mut length = 0.0;
            for &e in &selected {
                let ivl = edge::interval(&body, e).unwrap();
                let c = edge::curve(&body, e).unwrap().unwrap();
                let a = c.evaluate_point(&k, ivl.min()).unwrap();
                let b = c.evaluate_point(&k, ivl.max()).unwrap();
                length += a.distance(b);
            }
            apply(&mut k, &mut body, &selected, &input)
                .unwrap_or_else(|e| panic!("{name}: the round failed: {e:?}"));
            let lines = check_brep(&mut k, &mut body, CheckLevel::Level2)
                .unwrap()
                .lines();
            assert!(lines.is_empty(), "{name}: Level 2: {lines:?}");
            assert!(is_manifold_solid(&body).unwrap(), "{name}: not one closed solid");
            let e = enclose_volume(&k, &mut body, width).unwrap();
            assert!(e.width() <= width, "{name}: enclosure {e:?} wider than {width}");
            let r = input.radius;
            let (exact, old) = (mitred(r, length), swept(r, length));
            let slack = 16.0 * f64::EPSILON * exact.abs();
            eprintln!(
                "MITRE {name}: length {length}, volume in [{}, {}], mitred {exact}, swept {old}",
                e.lo, e.hi
            );
            assert!(
                e.lo - slack <= exact && exact <= e.hi + slack,
                "{name}: the mitred closed form {exact} is outside [{}, {}]",
                e.lo,
                e.hi
            );
            assert!(
                old < e.lo - slack || e.hi + slack < old,
                "{name}: main's swept form {old} is inside [{}, {}]",
                e.lo,
                e.hi
            );
            assert!((before - old).abs() <= 1e-4, "{name}: main's row {before} is not swept {old}");
            assert!((before - exact).abs() > 1e-2, "{name}: main's row {before} is not farther");
            let row = super::rust(&input).unwrap_or_else(|e| panic!("{name}: harness: {e:?}"));
            let volume = *row.values["mass_properties"].get(1).unwrap();
            assert!(
                e.lo - 1e-4 <= volume && volume <= e.hi + 1e-4,
                "{name}: the row's volume {volume} is not within 1e-4 of [{}, {}]",
                e.lo,
                e.hi
            );
        }
    }
}

/// WR-613 (#18640): the rows with corner-patch sides on plane faces, each checked on its
/// finished body.
#[cfg(test)]
#[path = "corner_patch_certificates.rs"]
mod corner_patch_certificates;

/// WR-646 (#18917): the variable-radius rows whose rounds now follow their law, each
/// certified on its finished body.
#[cfg(test)]
#[path = "wr646_certificates.rs"]
mod wr646_certificates;

/// WR-458, WR-633 and WR-650 (#18921): the real-solid rows the offset surface's
/// extension from its base moves, each certified on the body the golden records.
#[cfg(test)]
#[path = "wr458_certificates.rs"]
mod wr458_certificates;

#[cfg(test)]
mod wr664_near_knot_trims {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::round_definition_rows::{counts, inputs};
    use super::*;
    use solidcore::blend::definition_check::BlendFaceLog;
    use solidcore::geom::tol::{brep_zone_tol_3d, SmallTopoType};

    /// The rows whose results the near-knot snap (WR-664) moves: name, the golden's status
    /// and vertex, edge and face counts, which the snap leaves as they were, and the number
    /// of rail edges. (The `sphere_diff` rows keep the target's two sphere patches since
    /// WR-459's retirement, `wr459_sphere_rows`: 29/46/11, main's 27/42/10.)
    const ROWS: [(&str, [i64; 4], usize); 5] = [
        ("stubs4-delete-edge-bottle-e8-r0.1", [0, 31, 47, 18], 5),
        ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1", [0, 29, 46, 11], 2),
        ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", [0, 29, 46, 11], 2),
        ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1", [0, 29, 46, 11], 2),
        ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5", [0, 29, 46, 11], 2),
    ];

    /// `name` builds with the golden's counts and `rails` rail edges (an edge between a blend
    /// face the round made and a face it did not make, with knots inside: the traced rail,
    /// not a one-span end of the blend), and every rail edge starts in a span at
    /// least as long as the B-rep's zone tolerance: the chord from the edge's start to its
    /// curve's first knot inside the edge, a lower bound of the span's length. The start is where
    /// `TrimRailCurves` trims the rail at its vertex's drop. On main the `sphere_diff` rails
    /// started 9.5e-7 to 9.8e-7 short of their knot at 0.004 (a span of 9.5e-7 to 9.6e-7),
    /// their control points coinciding there.
    ///
    /// Their other ends are not checked here: there the trace's own last span, from knot to
    /// knot, is 9.5e-7 long on the `sphere_diff` rails, and no trim at a knot can lengthen
    /// it (on main one of them ended 2.2e-8 past that knot; the snap moves it onto it). The
    /// snap of an end is `blend::fillet_geom::p5`'s
    /// `rail_end_snaps_to_a_knot_within_the_zone_tolerance`. The bottle's rails start in long
    /// spans on main too; its golden pins what the snap moves there.
    fn check(name: &str, expected: [i64; 4], rails: usize) {
        let input = inputs(name).real_solid.expect("a real solid");
        let mut k = Kernel::default();
        let mut body = build(&mut k, &input).unwrap().unwrap();
        let (selected, _) = select_edges(&k, &body, &input).ok().unwrap();
        k.blend_face_log = Some(BlendFaceLog::default());
        let result = fillet_selected(&mut k, &mut body, &selected, &input).ok().unwrap();
        assert!(result.is_ok(), "{name}: {result:?}");
        assert_eq!(counts(&body, 0), expected, "{name}");
        let made: Vec<_> = k.blend_face_log.as_ref().unwrap().last().iter().map(|m| m.face).collect();
        let zone = brep_zone_tol_3d(&k, &body, SmallTopoType::NormalSize);
        let mut found = 0;
        for e in p1::edges(&body, None).unwrap() {
            let faces = edge::faces(&body, e, None).unwrap();
            if faces.iter().filter(|f| made.contains(f)).count() != 1 {
                continue;
            }
            let curve = edge::curve(&body, e).unwrap().expect("an edge curve");
            let ivl = edge::interval(&body, e).unwrap();
            let (knots, _) = curve.knots(&k, None).unwrap();
            // a traced rail has knots inside; an edge of one span (a blend's end) is no rail
            let Some(&first) = knots.iter().find(|&&t| t > ivl.min() && t < ivl.max()) else {
                continue;
            };
            found += 1;
            let start = curve.evaluate_point(&k, ivl.min()).unwrap();
            let span = start.distance(curve.evaluate_point(&k, first).unwrap());
            println!("WR664 {name}: rail edge {e:?} starts at {} in a span of {span:e}", ivl.min());
            assert!(span >= zone, "{name}: rail edge {e:?} starts in a span of {span:e} < {zone:e}");
        }
        assert_eq!(found, rails, "{name}: rail edges");
    }

    /// Every loop of every rolled blend face `name`'s round made (a face whose surface has a
    /// procedural blend definition: the faces whose rails `TrimRailCurves` trims) closes in
    /// the face's UV space: at each vertex the start of an edge use's UV trim curve and the
    /// end of its clockwise neighbour's are no farther apart than `EFF_ZERO * (1 + |uv|)`,
    /// the gap below which the face builder counts a loop corner as closed
    /// (`topology::face::p3`'s `create_uv_trim_curves_body`, `uv_scaled_zero`). The near-knot
    /// snap moves a rail's end along it by about the zone tolerance after the end curve is
    /// built; an end curve left where it was opens the loop there by that much, and the
    /// volume enclosure of the body can then no longer prove its volume positive
    /// (`round_definition_rows`). Where the snap moved the rail's end, the end curve's end is
    /// set to it, so the two agree to rounding. Where no end snaps, they differ by the
    /// drop's accuracy (4.2e-9 in u on main's e1 r0.1), more than this bound: on these five
    /// rows every corner of a rolled blend face closes (MEASURED), so a corner that stops
    /// snapping fails here and needs its own look.
    ///
    /// The bottle's other blend face, a patch without a definition, is not checked: three of
    /// its corners are open by 1.4e-10 to 3.9e-6 on main as well.
    fn loops_close(name: &str) {
        use solidcore::topology::edgeuse::p3::normalized_evaluate_read_only;
        let input = inputs(name).real_solid.expect("a real solid");
        let mut k = Kernel::default();
        let mut body = build(&mut k, &input).unwrap().unwrap();
        let (selected, _) = select_edges(&k, &body, &input).ok().unwrap();
        k.blend_face_log = Some(BlendFaceLog::default());
        let result = fillet_selected(&mut k, &mut body, &selected, &input).ok().unwrap();
        assert!(result.is_ok(), "{name}: {result:?}");
        let made: Vec<_> = k
            .blend_face_log
            .as_ref()
            .unwrap()
            .last()
            .iter()
            .map(|m| m.face)
            .filter(|&f| {
                let surface = body.face(f).unwrap().surface.as_ref().expect("a blend surface");
                surface.procedural_blend().is_some()
            })
            .collect();
        assert!(!made.is_empty(), "{name}: no rolled blend face");
        let mut open = Vec::new();
        for &f in &made {
            for l in face::loops(&body, f, false, None).unwrap() {
                for use_ in loopuse::loop_edgeuses(&body, l).unwrap() {
                    let cw = eu::cw_edgeuse(&body, use_).unwrap().expect("a clockwise neighbour");
                    let start = normalized_evaluate_read_only(&k, &body, use_, 0.0, true, false).unwrap().point;
                    let end = normalized_evaluate_read_only(&k, &body, cw, 1.0, true, false).unwrap().point;
                    let gap = start.distance(end);
                    let closed = solidcore::math::EFF_ZERO * (1.0 + start.x.abs().max(start.y.abs()));
                    println!("WR664 {name}: face {f:?} at ({}, {}) gap {gap:e} (closed below {closed:e})", start.x, start.y);
                    // Written to accept: a NaN gap or bound counts the corner as open.
                    let closes = gap.is_finite() && closed.is_finite() && gap <= closed;
                    if !closes {
                        open.push((f, start.x, start.y, gap));
                    }
                }
            }
        }
        assert!(open.is_empty(), "{name}: blend face loops open at {open:?}");
    }

    #[test]
    fn bottle_e8_r0_1_loops_close() {
        loops_close(ROWS[0].0);
    }

    #[test]
    fn sphere_diff_3cylinders_e1_r0_1_loops_close() {
        loops_close(ROWS[1].0);
    }

    #[test]
    fn sphere_diff_3cylinders_e1_r0_5_loops_close() {
        loops_close(ROWS[2].0);
    }

    #[test]
    fn sphere_diff_3cylinders_e4_r0_1_loops_close() {
        loops_close(ROWS[3].0);
    }

    #[test]
    fn sphere_diff_3cylinders_e4_r0_5_loops_close() {
        loops_close(ROWS[4].0);
    }

    #[test]
    fn bottle_e8_r0_1() {
        check(ROWS[0].0, ROWS[0].1, ROWS[0].2);
    }

    #[test]
    fn sphere_diff_3cylinders_e1_r0_1() {
        check(ROWS[1].0, ROWS[1].1, ROWS[1].2);
    }

    #[test]
    fn sphere_diff_3cylinders_e1_r0_5() {
        check(ROWS[2].0, ROWS[2].1, ROWS[2].2);
    }

    #[test]
    fn sphere_diff_3cylinders_e4_r0_1() {
        check(ROWS[3].0, ROWS[3].1, ROWS[3].2);
    }

    #[test]
    fn sphere_diff_3cylinders_e4_r0_5() {
        check(ROWS[4].0, ROWS[4].1, ROWS[4].2);
    }
}

/// Rounds on files with a found sphere, read on the file's kept sphere NURB and trims: the
/// round edges' ends lie within their edges' and vertices' tolerances, the trims the cone
/// face's classification makes are within their edges' tolerances, certified, and the
/// found spheres keep the file's trims and their samples lie on the sphere.
#[cfg(test)]
mod found_sphere_certificates {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::UnitDiff;
    use solidcore::topology::brep::p1;

    /// The rows of the classified trims (#20179): their result body holds five UV trims the
    /// file's read makes when it classifies on the cone face [`TRIM_FACE`] (the face has loop
    /// uses, so the read point-tests there and `CreateUVTrimCurves` makes its missing trims),
    /// and the fillet keeps them.
    const TRIM_ROWS: [&str; 6] = [
        "stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1",
        "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1",
        "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5",
        "sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1",
        "sweep4-sphere_diff_3cylinders-all-r0.1",
        "sweep5-sphere_diff_3cylinders-e29-r1.0",
    ];
    /// Those rows' result body's vertex count (the record's edges start at 4 per vertex).
    const TRIM_VERTICES: usize = 25;
    /// The face (in `faces` order) whose classification makes the trims.
    const TRIM_FACE: usize = 6;
    /// The edges (in `edges` order) whose use on [`TRIM_FACE`] holds a trim.
    const TRIM_EDGES: [usize; 5] = [20, 22, 27, 31, 33];
    /// Those edge uses, by index: the same uses the file's read classifies trims onto.
    const TRIM_EDGEUSES: [u32; 5] = [127, 129, 130, 132, 135];
    /// Offset of an edge's trim count in its 18 record entries (`util::primitive::append`).
    const TRIM_COUNT: usize = 17;

    /// `source-file-box_w_slot-all-edges`: the result record (`util::primitive::append`) is
    /// the body's; 24 edges with a use on a found sphere count two UV trims (the file's trim
    /// on the sphere is kept), and every sample the record holds of a found sphere face
    /// lies on the sphere to `1e-12` of its radius.
    #[test]
    fn box_w_slot_keeps_the_sphere_trims_and_samples_the_sphere() {
        use solidcore::surface::surface::SurfaceKind;
        use solidcore::topology::edgeuse::p1::face as face_of;
        const CASE: &str = "source-file-box_w_slot-all-edges";
        let runner = crate::units::blend_fillet_executive_p3::Runner;
        let output = runner
            .run_rust(&crate::case::regression_case(&runner, CASE))
            .unwrap();
        assert!(output.error.is_none(), "{CASE}: {:?}", output.error);
        let g = &output.values["result_geometry"];
        let mut k = Kernel::default();
        let body = result_body_of(&mut k, CASE);
        let (vertices, edges, faces) = (
            p1::vertices(&body, None).unwrap(),
            p1::edges(&body, None).unwrap(),
            p1::faces(&body, None).unwrap(),
        );
        let at_edges = 4 * vertices.len();
        let at_faces = at_edges + 18 * edges.len();
        assert_eq!(g.len(), at_faces + 18 * faces.len(), "{CASE}: record length");
        let sphere = |f| match body.face(f).unwrap().surface.as_ref().unwrap().kind() {
            SurfaceKind::Sphere(s) => Some((s.base().position().origin(), s.radius())),
            _ => None,
        };
        let kept = edges
            .iter()
            .enumerate()
            .filter(|&(i, &e)| {
                g[at_edges + 18 * i + 17] == 2.0
                    && body.edge(e).unwrap().edgeuses.iter().any(|&eu| {
                        face_of(&body, eu)
                            .unwrap()
                            .is_some_and(|f| sphere(f).is_some())
                    })
            })
            .count();
        assert_eq!(kept, 24, "{CASE}: edges on a found sphere with two trims");
        let mut sampled = 0;
        for (i, &f) in faces.iter().enumerate() {
            let Some((centre, radius)) = sphere(f) else {
                continue;
            };
            let at = at_faces + 18 * i;
            for s in 0..3 {
                let p = &g[at + 7 + 4 * s..at + 10 + 4 * s];
                assert_eq!(g[at + 6 + 4 * s], 0.0, "{CASE}: face {i} sample {s} status");
                let d = (Vec3::new(p[0], p[1], p[2]) - centre).length();
                assert!(
                    (d - radius).abs() <= 1e-12 * radius,
                    "{CASE}: face {i} sample {s} {:e} off the sphere",
                    d - radius
                );
            }
            sampled += 1;
        }
        assert!(sampled > 0, "{CASE}: no found sphere face");
    }

    /// `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5` and `-e4-r0.5` (rounds of 0.5 on
    /// two edges that are mirror images), each with its round edge (in `p1::edges` order)
    /// whose end lies 2.1e-8 or 9.9e-7 from its end vertex: each round edge holds the
    /// round's tolerance (4e-5) from its interval's start, and both its ends lie inside the
    /// edge's tolerance and their vertex's (the gaps are float evaluations, good to about
    /// 1e-13 here, far inside the margins). WR-459 keeps the target's own faces, which splits
    /// the round into two edges with the same start: the one checked here is edge 41 in
    /// `e1-r0.5` (37 before) and edge 40 in `e4-r0.5` (36 before), as main renumbered them.
    #[test]
    fn sphere_diff_r05_round_ends_are_within_tolerance() {
        use solidcore::topology::edge::{end_vertex, start_vertex};
        for (case, i) in [
            ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", 41),
            ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5", 40),
        ] {
            let mut k = Kernel::default();
            let b = result_body_of(&mut k, case);
            let e = p1::edges(&b, None).unwrap()[i];
            let edge = b.edge(e).unwrap();
            assert_eq!(
                (edge.tolerance, edge.interval.min()),
                (4e-5, 4.000_000_000_004_888_5e-3),
                "{case}"
            );
            let c = edge.curve.as_ref().unwrap();
            for (t, v) in [
                (edge.interval.min(), start_vertex(&b, e).unwrap().unwrap()),
                (edge.interval.max(), end_vertex(&b, e).unwrap().unwrap()),
            ] {
                let vertex = b.vertex(v).unwrap();
                let gap = (c.evaluate_point(&k, t).unwrap() - vertex.point).length();
                eprintln!("{case} edge {i} at {t}: gap {gap:e}, vertex tol {:e}", vertex.tolerance);
                assert!(vertex.tolerance >= 4e-5, "{case}: vertex tolerance");
                assert!(
                    gap < 4e-5 && gap < vertex.tolerance,
                    "{case} edge {i} at {t}: gap {gap:e}"
                );
            }
        }
    }

    /// The [`TRIM_ROWS`]: the five trims the classification on [`TRIM_FACE`] made are the
    /// only difference from the body without them, each within its edge's tolerance of the
    /// edge (certified). For each row:
    /// * the run's record is the result body's record, and the face is a cone with loop uses;
    /// * each [`TRIM_EDGES`] edge has exactly two uses holding a trim, one of them its use on
    ///   the face, the [`TRIM_EDGEUSES`];
    /// * with those five trims taken off, the body's record is the run's with exactly those
    ///   five trim counts lowered from 2 to 1 and nothing else changed, bit for bit;
    /// * the five are bit for bit the trims the file's read holds on the same edge uses, so the
    ///   fillet keeps them as the read makes them; and with them taken off the read, one B-rep
    ///   ray classification while it is edited makes them again, bit for bit (since WR-732's
    ///   retirement, #18934, the ray builds the cone face's tree against its current trims, and
    ///   the implant makes them): they are what the implant makes, not another change;
    /// * each trim's same-parameter gap to its edge is certified (`curve_uv_gap_bound`,
    ///   interval Taylor models, every rounding included) within the edge's tolerance.
    ///
    /// (A face classification on the finished body, as this test asked before #18934, makes
    /// edge 20's trim with control points up to 2.7e-9 off the implant's in UV, as #18934
    /// measured; that is not how the read makes them, so it is not asked.)
    #[test]
    fn classified_trims_are_the_only_change_and_within_tolerance() {
        use solidcore::curve::curve::CurveKind;
        use solidcore::query::curve_surface_gap::curve_uv_gap_bound;
        use solidcore::surface::surface::SurfaceKind;
        use solidcore::topology::edgeuse::p1 as eu1;
        use solidcore::topology::{edge, face};
        let record = |k: &Kernel, b: &Brep| {
            let mut out = Vec::new();
            crate::util::primitive::append(k, b, &mut out).unwrap();
            out
        };
        let encoding = |k: &Kernel, c: &solidcore::curve::curve::Curve| {
            let mut enc = Vec::new();
            crate::util::trim_curve::put_trim_curve(k, &mut enc, Some(c)).unwrap();
            enc.iter().map(|x| x.to_bits()).collect::<Vec<u64>>()
        };
        // the file's read, as the runner reads it
        let read = {
            let path = crate::case::repo_root()
                .join(crate::fixtures::UPSTREAM_TEST_FILES)
                .join("Fillet/sphere_diff_3cylinders.smb");
            let mut k = crate::runner_kernel();
            crate::own_reader::read_file(&mut k, path.to_str().unwrap())
                .unwrap()
                .unwrap()
        };
        // the read with its five classified trims taken off, and one B-rep ray classification
        // while it is edited: the ray builds the cone face's tree against its current trims,
        // and the implant makes the five (from 20 off the read's box middle along y, whose rays
        // cross the cone face; from the middle itself they do not reach it)
        let remade = {
            use solidcore::query::brep::BrepQuery;
            let k = crate::runner_kernel();
            // (the box on a copy of the read: building it leaves caches on the B-rep it measures)
            let mid = {
                let mut c = read.clone_standing_for_self().unwrap();
                solidcore::query::brep::calculate_tight_bounding_box(&k, &mut c)
                    .unwrap()
                    .mid()
            };
            let mut b = read.clone_standing_for_self().unwrap();
            let ids: Vec<_> = b
                .edgeuses()
                .iter()
                .filter(|(id, _)| TRIM_EDGEUSES.contains(&id.index()))
                .map(|(id, _)| id)
                .collect();
            assert_eq!(ids.len(), 5, "the read's classified edge uses");
            for id in ids {
                assert!(b.edgeuse_mut(id).unwrap().uv_trim_curve.take().is_some());
            }
            let p = solidcore::math::Point3::new(mid.x, mid.y + 20.0, mid.z);
            b.set_editing_enabled(true);
            BrepQuery::of_brep(&b)
                .point_3d_classify(&k, &mut b, p, 1e-6, false)
                .unwrap();
            b.set_editing_enabled(false);
            b
        };
        let runner = crate::units::blend_fillet_executive_p3::Runner;
        let at = |i: usize| 4 * TRIM_VERTICES + 18 * i + TRIM_COUNT;
        for case in TRIM_ROWS {
            let output = runner
                .run_rust(&crate::case::regression_case(&runner, case))
                .unwrap();
            assert!(output.error.is_none(), "{case}: {:?}", output.error);
            let now = output.values["result_geometry"].clone();
            let mut k = Kernel::default();
            let mut body = result_body_of(&mut k, case);
            assert_eq!(record(&k, &body), now, "{case}: the run is not the body's record");
            assert_eq!(p1::vertices(&body, None).unwrap().len(), TRIM_VERTICES, "{case}");
            let edges = p1::edges(&body, None).unwrap();
            let f = p1::faces(&body, None).unwrap()[TRIM_FACE];
            let surface = face::p2::surface(&body, f).unwrap().unwrap().clone();
            assert!(
                matches!(surface.kind(), SurfaceKind::Cone(_)),
                "{case}: the face is not a cone"
            );
            assert!(face::p4::has_loopuses(&body, f).unwrap());
            // the five trims, taken off
            let mut taken = Vec::new();
            for i in TRIM_EDGES {
                let e = edges[i];
                let uses = body.edge(e).unwrap().edgeuses.clone();
                let trimmed: Vec<_> = uses
                    .iter()
                    .copied()
                    .filter(|&eu| body.edgeuse(eu).unwrap().uv_trim_curve.is_some())
                    .collect();
                assert_eq!(trimmed.len(), 2, "{case}: edge {i}: uses holding a trim");
                let on_face: Vec<_> = trimmed
                    .iter()
                    .copied()
                    .filter(|&eu| eu1::face(&body, eu).unwrap() == Some(f))
                    .collect();
                assert_eq!(on_face.len(), 1, "{case}: edge {i}: trims on the face");
                let eu = on_face[0];
                let trim = body.edgeuse_mut(eu).unwrap().uv_trim_curve.take().unwrap();
                // the trim fits its edge: certified same-parameter gap within the tolerance
                let tol = edge::tolerance(&body, e).unwrap();
                let interval = edge::interval(&body, e).unwrap();
                let rail = edge::curve(&body, e).unwrap().unwrap().clone();
                let c = match rail.kind() {
                    CurveKind::BSpline(c) => c.clone_exact(),
                    CurveKind::Line(l) => l.base_with_nurb().unwrap().into_owned(),
                    other => panic!("{case}: edge {i}: an edge curve {other:?}"),
                };
                let w = match trim.kind() {
                    CurveKind::BSpline(w) => w.clone_exact(),
                    CurveKind::Line(l) => l.base_with_nurb().unwrap().into_owned(),
                    other => panic!("{case}: edge {i}: a trim {other:?}"),
                };
                let gap = curve_uv_gap_bound(
                    &k,
                    &c,
                    &w,
                    &surface,
                    [interval.min(), interval.max()],
                    tol / 100.0,
                )
                .unwrap()
                .unwrap_or_else(|| panic!("{case}: edge {i}: the gap is not enclosed"));
                assert!(
                    gap <= tol,
                    "{case}: edge {i}: certified gap {gap:e} over the tolerance {tol:e}"
                );
                taken.push((i, eu, encoding(&k, &trim)));
            }
            let without = record(&k, &body);
            let moved: Vec<usize> = (0..now.len())
                .filter(|&j| without[j].to_bits() != now[j].to_bits())
                .collect();
            assert_eq!(moved, TRIM_EDGES.map(at).to_vec(), "{case}: entries the trims move");
            for i in TRIM_EDGES {
                assert_eq!((now[at(i)], without[at(i)]), (2.0, 1.0), "{case}: edge {i}");
            }
            let mut ids: Vec<u32> = taken.iter().map(|t| t.1.index()).collect();
            ids.sort_unstable();
            assert_eq!(ids, TRIM_EDGEUSES, "{case}: not the read's classified trims");
            // the five are the read's own trims, bit for bit: the fillet keeps them as the read
            // of the file makes them (`read`), and the ray's implant makes them (`remade`)
            for (i, eu, bits) in &taken {
                for (what, b) in [("the read", &read), ("the ray's implant", &remade)] {
                    let made = b.edgeuse(*eu).unwrap().uv_trim_curve.as_ref();
                    let made = made.unwrap_or_else(|| panic!("{case}: edge {i}: not on {what}"));
                    assert_eq!(
                        &encoding(&k, made),
                        bits,
                        "{case}: edge {i}: another trim on {what}"
                    );
                }
            }
        }
    }
}

/// WR-459 (#17169): the four `sphere_diff_3cylinders` rows the kept target faces move from
/// main's 27/42/10 to 29/46/11. The target is a sphere less three cylinders, in two sphere
/// patches on one surface; the round splits one patch and the clean-up now keeps both, so
/// their boundary (the input's own same-surface edge) stays. These tests check, row by
/// row, that this is the only change: every face but the round's has a target face's
/// surface, the round's surface is main's, and the Level 2 failures are main's (constants
/// printed on main a2b7ec60e2 by a one-off probe of the same rows, not a recorder kept
/// here) less its "Face Near missing Seam" on the sphere. On main a2b7ec60e2 the round
/// placed a vertex a few nanometres off the sphere's seam, well inside that vertex's
/// tolerance, which the kept boundary exposed as one more such failure; since #21421 a
/// fillet's body vertex stays on the seam it is on, so main's near-seam vertices are
/// checked on the seam at the closed-form seam point of main's position, and the kept
/// boundary adds no failure. On e4 the round's rail cone (the target's face 5, whose B-spline
/// seam lies 2^-20 off the sphere's) keeps the target's own seam near miss, a failure main's
/// e4 result did not have. The volume is certified in `round_definition_rows`.
#[cfg(test)]
mod wr459_sphere_rows {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::round_definition_rows::inputs;
    use super::*;
    use solidcore::geom::extent1d::Extent1d;
    use solidcore::math::EFF_ZERO;
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    use solidcore::topology::{vertex, FaceId, VertexId};

    /// A Level 2 failure: its label and its reporter's key (the object space in the high
    /// 32 bits, the arena index in the low).
    type Failure = (&'static str, Option<(u64, u64)>);

    const SEAM: &str = "Face Near missing Seam";
    const FACE: u64 = 16002;
    const EDGE: u64 = 20001;
    const EDGEUSE: u64 = 20003;

    /// One row: the case; main's (a2b7ec60e2) Level 2 failures of its result, less the three
    /// of edgeuse 142's trim that the clamped-sliver drop rule (#21353) took out of the
    /// target itself (the target's own failures are checked in `check`); the sphere
    /// patch the kept boundary adds (whose seam main's placement failed before #21421); the
    /// round's vertex near the seam, with main's coordinate bits and the bound on its
    /// distance from the seam (the measured distance rounded up); the other vertices within
    /// 1e-4 of the seam that the check reported, each with main's bits; the hash of the round surface
    /// (`fnv1a64` of its `Debug` form: main's, but for e1 r0.1, e1 r0.5 and e4 r0.5, whose
    /// definitions' leader and seed curves and nets moved by at most 1.03e-13 relative when
    /// the offset's extension became the default, each round re-certified against its
    /// definition) and of main's other faces (`fnv1a64` of their sorted `geometry_text`s,
    /// joined by newlines); and main's one sphere face.
    struct Row {
        name: &'static str,
        main_failures: &'static [Failure],
        added_on: usize,
        vertex: (usize, [u64; 3], f64),
        others: &'static [(usize, [u64; 3])],
        round_surface: u64,
        main_faces: u64,
        main_sphere: u64,
    }

    /// The failures every row's body shares with the target (`check_brep` of the target:
    /// the cone faces 4, 5 and 7 near their seams) less face 4,
    /// which the round splits, plus the round's rail on the cone (edge and edgeuse).
    const fn shared(rail: u64, rail_use: u64, cone: u64) -> [Failure; 5] {
        [
            (SEAM, Some((FACE, 5))),
            (SEAM, Some((FACE, 7))),
            (SEAM, Some((FACE, cone))),
            ("Bad Repeated CPts", Some((EDGE, rail))),
            ("Bad Repeated CPts", Some((EDGEUSE, rail_use))),
        ]
    }

    const E1: [Failure; 5] = shared(40, 162, 9);
    const E4_R01: [Failure; 5] = [
        (SEAM, Some((FACE, 0))),
        (SEAM, Some((FACE, 4))),
        (SEAM, Some((FACE, 7))),
        ("Bad Repeated CPts", Some((EDGE, 43))),
        ("Bad Repeated CPts", Some((EDGEUSE, 172))),
    ];
    const E4_R05: [Failure; 7] = [
        (SEAM, Some((FACE, 0))),
        (SEAM, Some((FACE, 4))),
        (SEAM, Some((FACE, 7))),
        ("Bad Repeated CPts", Some((EDGE, 41))),
        ("Bad Repeated CPts", Some((EDGEUSE, 164))),
        ("Bad Repeated CPts", Some((EDGE, 43))),
        ("Bad Repeated CPts", Some((EDGEUSE, 172))),
    ];

    /// e4's vertex 2, the target's patch corner on the seam (4.4e-14 off it in the
    /// target), which the round left 9.5e-7 off the seam on main a2b7ec60e2 too.
    const E4_V2: (usize, [u64; 3]) = (2, [0x4036026500000000, 0x3eafffffffc57405, 0x4045540c8e2b6b68]);

    const ROWS: [Row; 4] = [
        Row {
            name: "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
            main_failures: &E1,
            added_on: 8,
            vertex: (27, [0x403627cf90d5c75d, 0xbe31ee9f830c0a25, 0xc0454a5b0aeac7b9], 4.2e-9),
            others: &[],
            round_surface: 17087500646884337043,
            main_faces: 14750437352760065621,
            main_sphere: 8,
        },
        Row {
            name: "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
            main_failures: &E1,
            added_on: 8,
            vertex: (27, [0x4036bf0d23081593, 0xbe56aa7ba247c332, 0xc0452256756ad4f1], 2.12e-8),
            others: &[],
            round_surface: 4904801595854769515,
            main_faces: 14750437352760065621,
            main_sphere: 8,
        },
        Row {
            name: "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
            main_failures: &E4_R01,
            added_on: 1,
            vertex: (25, [0x403627cf90d5c76b, 0xbe31ee9f8404242c, 0x40454a5b0aeac7b6], 4.2e-9),
            others: &[E4_V2],
            round_surface: 2325416645222036088,
            main_faces: 16065100554185304934,
            main_sphere: 0,
        },
        Row {
            name: "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
            main_failures: &E4_R05,
            added_on: 1,
            vertex: (25, [0x4036bf0d230815a1, 0xbe56aa7bab70d538, 0x40452256756ad4ed], 2.12e-8),
            others: &[E4_V2],
            round_surface: 12247167273146979092,
            main_faces: 16065100554185304934,
            main_sphere: 0,
        },
    ];

    fn failures(b: &mut Brep) -> Vec<Failure> {
        let check = check_brep(&mut Kernel::default(), b, CheckLevel::Level2).unwrap();
        let mut out: Vec<Failure> = check
            .failures()
            .map(|f| (f.name, f.reporter.map(|r| (r.0 >> 32, r.0 & 0xffff_ffff))))
            .collect();
        out.sort();
        out
    }

    /// The face's whole surface object, as `Debug` prints it (its kind and its owner,
    /// tolerance and cache state), less the `fitted_net` field of every `Surface` in it
    /// (the round's and its supports': whether the stored net is a fitted approximation
    /// of a definition, which the certified line pass reads). The text main hashed
    /// (a2b7ec60e2) has no such field, and it says nothing of the geometry.
    fn surface_text(b: &Brep, f: FaceId) -> String {
        format!("{:?}", b.face(f).unwrap().surface.as_ref().unwrap())
            .replace(", fitted_net: false", "")
            .replace(", fitted_net: true", "")
    }

    /// The face's surface geometry, as `Debug` prints its kind (the surface's type and
    /// its defining data), less the lazily built NURBS form (`NurbSlot`, a cache the
    /// first evaluation fills, so it differs with what has been evaluated).
    fn geometry_text(b: &Brep, f: FaceId) -> String {
        let text = format!("{:?}", b.face(f).unwrap().surface.as_ref().unwrap().kind());
        let mut out = String::new();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("NurbSlot(") {
            out.push_str(&rest[..at]);
            out.push_str("NurbSlot(..)");
            let mut depth = 0usize;
            let mut end = rest.len();
            for (i, c) in rest[at + "NurbSlot".len()..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = at + "NurbSlot".len() + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            rest = &rest[end..];
        }
        out.push_str(rest);
        out
    }

    fn is_sphere(b: &Brep, f: FaceId) -> bool {
        let surface = b.face(f).unwrap().surface.as_ref().unwrap();
        matches!(surface.kind(), SurfaceKind::Sphere(_))
    }

    /// The vertices of the sphere face `f` within 1e-4 of its seam (the half-plane of
    /// its frame's x axis, u = 0) but at least the near-miss distance (`EFF_ZERO * 1000`,
    /// `CurveClassification::has_near_misses`) off it: each with its distance from the
    /// seam's plane, its tolerance and its coordinate bits.
    fn near_seam(b: &Brep, f: FaceId) -> Vec<(usize, f64, f64, [u64; 3])> {
        let surface = b.face(f).unwrap().surface.as_ref().unwrap();
        let SurfaceKind::Sphere(sphere) = surface.kind() else {
            panic!("{f:?} is not a sphere face")
        };
        let frame = sphere.base().position();
        let mut out = Vec::new();
        for v in solidcore::topology::face::p4::vertices(b, f, None, false, None).unwrap() {
            let p = vertex::point(b, v).unwrap();
            let local = p - frame.origin();
            let (x, y) = (local.dot(frame.x_axis()), local.dot(frame.y_axis()).abs());
            if x > 0.0 && (EFF_ZERO * 1000.0..1e-4).contains(&y) {
                let bits = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
                out.push((v.index() as usize, y, vertex::tolerance(b, v).unwrap(), bits));
            }
        }
        out.sort_by_key(|n| n.0);
        out
    }

    /// The edges of `b` between two distinct sphere faces, each with its 3D curve's
    /// interval, its tolerance and its end points.
    fn sphere_seams(b: &Brep) -> Vec<(EdgeId, Extent1d, f64, [Vec3; 2])> {
        let mut out = Vec::new();
        for e in p1::edges(b, None).unwrap() {
            let mut faces = edge::faces(b, e, None).unwrap();
            faces.sort();
            faces.dedup();
            if faces.len() == 2 && faces.iter().all(|&f| is_sphere(b, f)) {
                let (s, t) = edge::start_end_vertices(b, e).unwrap();
                let ends = [s.unwrap(), t.unwrap()].map(|v| vertex::point(b, v).unwrap());
                out.push((e, edge::interval(b, e).unwrap(), edge::tolerance(b, e).unwrap(), ends));
            }
        }
        out
    }

    /// The result's boundary between the two sphere patches is the target's: the target's
    /// sphere patches meet along arcs of one great circle of the sphere (each sampled
    /// point within its edge's tolerance of the circle); the result keeps at least one
    /// edge between its two sphere patches, and every point sampled on every such edge
    /// lies on that circle within the edge's tolerance and inside the span of one of the
    /// target's arcs (by angle, the round shortening an arc at its end); and the edges kept
    /// on every target arc form one chain from one of the arc's own end points (bit for
    /// bit) to its other end point (bit for bit) or to a vertex of the round.
    /// Compared by geometry, not by arena ids, so a boundary split into more edges over
    /// the same span passes too.
    fn shares_the_target_boundary(name: &str, target: &Brep, body: &Brep) {
        const SAMPLES: usize = 32;
        let k = Kernel::default();
        let sample = |b: &Brep, e: EdgeId, iv: &Extent1d| -> Vec<Vec3> {
            let curve = edge::curve(b, e).unwrap().unwrap();
            (0..=SAMPLES)
                .map(|i| {
                    let t = iv.min() + (iv.max() - iv.min()) * (i as f64 / SAMPLES as f64);
                    curve.evaluate_point(&k, t).unwrap()
                })
                .collect()
        };
        let sphere_face = p1::faces(target, None).unwrap().into_iter().find(|&f| is_sphere(target, f)).unwrap();
        let surface = target.face(sphere_face).unwrap().surface.as_ref().unwrap();
        let SurfaceKind::Sphere(sphere) = surface.kind() else { unreachable!() };
        let c = sphere.base().position().origin();
        let arcs = sphere_seams(target);
        assert!(!arcs.is_empty(), "{name}: the target's sphere patches share no edge");
        // the circle's plane, through the centre and the first arc's ends
        let [a0, b0] = arcs[0].3;
        let n = (a0 - c).cross(b0 - c);
        let n = n * (1.0 / n.length());
        let radius = (a0 - c).length();
        let off_circle = |p: Vec3| {
            let d = p - c;
            let h = d.dot(n);
            let r = (d - n * h).length();
            (h * h + (r - radius) * (r - radius)).sqrt()
        };
        let angle = |p: Vec3, q: Vec3| (p - c).cross(q - c).length().atan2((p - c).dot(q - c));
        for (e, iv, tol, _) in &arcs {
            for p in sample(target, *e, iv) {
                let d = off_circle(p);
                assert!(d <= *tol, "{name}: target edge {e:?} is {d:e} off its circle, past {tol:e}");
            }
        }
        let seams = sphere_seams(body);
        assert!(!seams.is_empty(), "{name}: the result's sphere patches share no edge");
        let within = |p: Vec3, tol: f64, (_, _, atol, [s, t]): &(EdgeId, Extent1d, f64, [Vec3; 2])| {
            let excess = angle(*s, p) + angle(p, *t) - angle(*s, *t);
            excess * radius <= tol + atol
        };
        // each result edge lies on the circle and inside one target arc
        let mut on_arc = vec![Vec::new(); arcs.len()];
        for (e, iv, tol, _) in &seams {
            let points = sample(body, *e, iv);
            for &p in &points {
                let d = off_circle(p);
                assert!(d <= *tol, "{name}: edge {e:?} is {d:e} off the target's circle, past {tol:e}");
            }
            let arc = arcs.iter().position(|a| points.iter().all(|&p| within(p, *tol, a)));
            let arc = arc.unwrap_or_else(|| panic!("{name}: edge {e:?} leaves the target's arcs"));
            on_arc[arc].push(*e);
        }
        // the round's vertices
        let mut round_vertices = Vec::new();
        for f in p1::faces(body, None).unwrap() {
            if body.face(f).unwrap().surface.as_ref().unwrap().procedural_blend().is_some() {
                round_vertices.extend(solidcore::topology::face::p4::vertices(body, f, None, false, None).unwrap());
            }
        }
        // and the edges kept on every target arc run as one chain, with no gap or overlap,
        // from one of the arc's own ends (bit for bit) to its other end (bit for bit) or to
        // a vertex of the round: the round only shortens an arc at its other end, and never
        // further than the round reaches
        for ((e, _, _, [s, t]), kept) in arcs.iter().zip(&on_arc) {
            let ends: Vec<[VertexId; 2]> = kept
                .iter()
                .map(|&k| {
                    let (a, b) = edge::start_end_vertices(body, k).unwrap();
                    [a.unwrap(), b.unwrap()]
                })
                .collect();
            let point = |v: VertexId| vertex::point(body, v).unwrap();
            let start = ends.iter().flatten().copied().find(|&v| point(v) == *s || point(v) == *t);
            let start = start.unwrap_or_else(|| panic!("{name}: the target's arc {e:?} keeps no edge from its own ends"));
            let other = if point(start) == *s { *t } else { *s };
            let mut used = vec![false; ends.len()];
            let mut at = start;
            while let Some(i) = (0..ends.len()).find(|&i| !used[i] && ends[i].contains(&at)) {
                used[i] = true;
                at = if ends[i][0] == at { ends[i][1] } else { ends[i][0] };
            }
            assert!(used.iter().all(|&u| u), "{name}: the edges kept on the target's arc {e:?} are not one chain");
            assert!(
                point(at) == other || round_vertices.contains(&at),
                "{name}: the edges kept on the target's arc {e:?} end at {:?}, neither the arc's other end nor the round",
                point(at)
            );
        }
    }

    /// The row's target and its rounded result.
    fn bodies(name: &str) -> (Brep, Brep) {
        let input = inputs(name).real_solid.expect("a real solid");
        let mut k = Kernel::default();
        let target = build(&mut k, &input).unwrap().unwrap();
        let mut body = build(&mut k, &input).unwrap().unwrap();
        let (selected, _) = select_edges(&k, &body, &input).ok().unwrap();
        let result = fillet_selected(&mut k, &mut body, &selected, &input).ok().unwrap();
        assert!(result.is_ok(), "{name}: {result:?}");
        (target, body)
    }

    fn check(row: &Row) {
        let name = row.name;
        let (target, mut body) = bodies(name);
        let counts = |b: &Brep| {
            [
                p1::vertices(b, None).unwrap().len(),
                p1::edges(b, None).unwrap().len(),
                p1::faces(b, None).unwrap().len(),
            ]
        };
        assert_eq!(counts(&body), [29, 46, 11], "{name}");

        // the faces but the round's are main's (fnv1a64 of their sorted geometry texts,
        // printed on main a2b7ec60e2) plus one more patch of the target's sphere: the
        // sphere both patches share is the target's, with the `swap_uv` bit the round's
        // set-up clears (main's merged patch has it cleared too); the round's surface is
        // main's
        let target_sphere: Vec<String> = p1::faces(&target, None)
            .unwrap()
            .into_iter()
            .filter(|&f| is_sphere(&target, f))
            .map(|f| geometry_text(&target, f).replace("swap_uv: true", "swap_uv: false"))
            .collect();
        let sphere = target_sphere.first().unwrap().clone();
        assert!(target_sphere.iter().all(|t| *t == sphere), "{name}: one sphere in the target");
        let mut rounds = Vec::new();
        let mut others = Vec::new();
        for f in p1::faces(&body, None).unwrap() {
            if body.face(f).unwrap().surface.as_ref().unwrap().procedural_blend().is_some() {
                rounds.push(crate::rng::fnv1a64(&surface_text(&body, f)));
            } else {
                if is_sphere(&body, f) {
                    assert_eq!(geometry_text(&body, f), sphere, "{name}: {f:?} is not the target's sphere");
                }
                others.push(geometry_text(&body, f));
            }
        }
        let extra = others.iter().position(|t| *t == sphere).unwrap();
        others.remove(extra);
        others.sort();
        assert_eq!(
            crate::rng::fnv1a64(&others.join("\n")),
            row.main_faces,
            "{name}: the faces but the round's and the added patch are not main's"
        );
        assert_eq!(rounds, [row.round_surface], "{name}: the round's surface is not the row's");
        // and the round is its definition, its net re-certified within its bound of it
        round_definition_proof::certified_blends(&Kernel::default(), &mut body, name);

        // main's vertices near the seam (the round's, within its bound, and on e4 the
        // target's patch corner) are on the seam now (#21421: a fillet's body vertex stays
        // on the seam it is on): the patch has no vertex between the near-miss distance and
        // 1e-4 of the seam, and each of main's (by arena index) lies on the seam's side,
        // closer to its plane than the near-miss distance, and within its own tolerance of
        // main's point (main's constants are checked against that tolerance and the row's
        // bound, as on main); the closed form below then pins where on the seam it is
        let added = FaceId::from_index(row.added_on as u32);
        let near = near_seam(&body, added);
        assert!(near.is_empty(), "{name}: vertices near the seam of {added:?}: {near:?}");
        let surface = body.face(added).unwrap().surface.as_ref().unwrap();
        let SurfaceKind::Sphere(sphere) = surface.kind() else {
            panic!("{name}: {added:?} is not a sphere face")
        };
        let frame = sphere.base().position();
        let (vertex, bits, bound) = row.vertex;
        let mut main_near: Vec<(usize, [u64; 3])> = row.others.to_vec();
        main_near.push((vertex, bits));
        for (v, bits) in main_near {
            let id = VertexId::from_index(v as u32);
            let p = vertex::point(&body, id).unwrap();
            let tol = vertex::tolerance(&body, id).unwrap();
            let [x, y, z] = bits.map(f64::from_bits);
            let main_point = Vec3::new(x, y, z);
            let local = p - frame.origin();
            let main_local = main_point - frame.origin();
            let (d, d_main) = (local.dot(frame.y_axis()).abs(), main_local.dot(frame.y_axis()).abs());
            // Written to accept: a NaN distance fails.
            assert!(
                local.dot(frame.x_axis()) > 0.0 && d < EFF_ZERO * 1000.0,
                "{name}: vertex {v} is {d:e} off the seam"
            );
            assert!(d_main <= tol, "{name}: vertex {v} was {d_main:e} off the seam, past its tolerance {tol:e}");
            if v == vertex {
                assert!(d_main <= bound, "{name}: vertex {v} was {d_main:e} off the seam, past {bound:e}");
            }
            let moved = (p - main_point).length();
            assert!(moved <= tol, "{name}: vertex {v} moved {moved:e}, past its tolerance {tol:e}");
            // the closed form of the fix (`onto_seam`): main's point is the fillet's position,
            // and the vertex is the sphere's point S(0, v) = O + r (cos v X + sin v Z) at the
            // latitude v of that position dropped onto the sphere, cos v = rho / n and
            // sin v = z / n in the frame (rho the distance from the axis, n from the centre).
            // Both evaluations round each coordinate of O + r (...), a few ulp of |O| + r
            // apiece; 64 ulp of it bounds their difference (measured: at most 6.3e-14 at r 48,
            // where the moves are 4.2e-9, 2.1e-8 and 9.5e-7)
            let r = sphere.radius();
            let (mx, my, mz) = (
                main_local.dot(frame.x_axis()),
                main_local.dot(frame.y_axis()),
                main_local.dot(frame.z_axis()),
            );
            let n = (mx * mx + my * my + mz * mz).sqrt();
            let on_seam = frame.origin()
                + r * ((mx * mx + my * my).sqrt() / n * frame.x_axis() + mz / n * frame.z_axis());
            let allowance = 64.0 * f64::EPSILON * (frame.origin().length() + r);
            let off = (p - on_seam).length();
            assert!(
                off <= allowance && d <= allowance,
                "{name}: vertex {v} is {off:e} from the seam point of main's {main_point:?} \
                 ({d:e} off the seam), past {allowance:e}"
            );
        }

        shares_the_target_boundary(name, &target, &body);
        // the target's own failures: the cone faces 4, 5 and 7 near their seams (edgeuse
        // 142's trim failures went with the clamped-sliver drop rule, #21353, which leaves
        // the trim's folded end span out on read)
        let target_failures = failures(&mut target.clone_standing_for_self().unwrap());
        assert_eq!(
            target_failures,
            [(SEAM, Some((FACE, 4))), (SEAM, Some((FACE, 5))), (SEAM, Some((FACE, 7)))],
            "{name}: the target's Level 2 failures"
        );

        // the kept boundary adds no failure: main's, less every seam near miss on a sphere
        // face (the vertices above, now on the seam; on e4 main's sphere face 0 is one of
        // the two patches), and no seam near miss on the added patch. On e4 the round's
        // rail runs on the target's cone face 5 (face 9 here), whose B-spline seam lies
        // 2^-20 off the sphere's: the target near-misses it, and with the rail's end vertex
        // on the sphere's seam the result keeps that near miss of the target (on e1 it is
        // the `cone` of `shared`, on main too); so the body near-misses no more faces than
        // the target does
        let mut want: Vec<Failure> = row
            .main_failures
            .iter()
            .copied()
            .filter(|&(label, key)| {
                !(label == SEAM
                    && key.is_some_and(|(space, i)| {
                        space == FACE && is_sphere(&body, FaceId::from_index(i as u32))
                    }))
            })
            .collect();
        want.sort();
        let removed = row.main_failures.len() - want.len();
        assert_eq!(removed, usize::from(name.contains("-e4-")), "{name}: main's sphere failures");
        if name.contains("-e4-") {
            let (cone, target_cone) = (FaceId::from_index(9), FaceId::from_index(5));
            assert_eq!(
                geometry_text(&body, cone).replace("swap_uv: true", "swap_uv: false"),
                geometry_text(&target, target_cone).replace("swap_uv: true", "swap_uv: false"),
                "{name}: face 9 is not the target's face 5"
            );
            assert!(target_failures.contains(&(SEAM, Some((FACE, 5)))), "{name}");
            want.push((SEAM, Some((FACE, 9))));
            want.sort();
        }
        let got = failures(&mut body);
        assert_eq!(got, want, "{name}: Level 2 failures");

        // on main the two patches were one face (main_sphere, holding the same vertices
        // at the same bits: printed by a one-off probe on main a2b7ec60e2); on e4
        // main's merged patch already fails the seam check, so the near-seam placement is
        // main's, which the kept boundary exposes on e1 too and does not cause
        let main_fails_seam = row.main_failures.contains(&(SEAM, Some((FACE, row.main_sphere))));
        assert_eq!(main_fails_seam, name.contains("-e4-"), "{name}: main's merged patch");
    }

    #[test]
    fn e1_r0_1() {
        check(&ROWS[0]);
    }

    #[test]
    fn e1_r0_5() {
        check(&ROWS[1]);
    }

    #[test]
    fn e4_r0_1() {
        check(&ROWS[2]);
    }

    #[test]
    fn e4_r0_5() {
        check(&ROWS[3]);
    }
}

/// #21421: rounding an edge of `sphere_diff_3cylinders` whose rail ends on the sphere's seam
/// edge places the round's end vertex on that seam. The seam is the meridian at the start of
/// the sphere's closed u range (the half-plane of local x > 0, y = 0). On main the vertex sat
/// 4.2e-9 (r 0.1) and 2.1e-8 (r 0.5) off it, inside its 4.1e-5 tolerance but past the
/// `1000 * EFF_ZERO` beyond which `CurveClassification::has_near_misses` counts a vertex as a
/// near miss of the seam, so `check_brep` Level 2 reported "Face Near missing Seam" on the
/// rows' sphere patches (on e1 since WR-459's clean-up keeps both patches; `wr459_sphere_rows`
/// checks main's near-seam vertices on the seam).
///
/// The cylinder faces' near misses on these rows come from the input body
/// (`sphere_diff_3cylinders.smb` reports three of them at Level 2 before the round) and are
/// not checked here.
#[cfg(test)]
mod issue_21421_round_ends_on_the_sphere_seam {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::round_definition_rows::{counts, inputs};
    use super::*;
    use solidcore::math::EFF_ZERO;
    use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
    use solidcore::surface::surface::p2::SurfaceKind;
    use solidcore::topology::brep::p1::HealLoopHandle;
    use solidcore::topology::face::p4::{has_seam_problem_in_brep_mut, HasSeamProblemOptions};
    use solidcore::topology::vertex;

    /// The rows, whose golden status and vertex, edge and face counts (`[0, 29, 46, 11]`, WR-459's kept target faces)
    /// placing the vertex on the seam leaves as they were.
    const ROWS: [&str; 4] = [
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
    ];

    /// The number of faces of `body` whose seam check reports a near miss.
    fn near_missed_faces(k: &Kernel, body: &mut Brep) -> usize {
        let mut n = 0;
        for f in p1::faces(body, None).unwrap() {
            let check =
                has_seam_problem_in_brep_mut::<HealLoopHandle>(k, body, f, HasSeamProblemOptions::default())
                    .unwrap();
            if check.problem.near_miss != SurfParamType::Neither {
                n += 1;
            }
        }
        n
    }

    /// `name` builds with the golden's counts; every vertex of a sphere face that lies within
    /// its own tolerance of the sphere's seam lies closer to it than `1000 * EFF_ZERO`, the
    /// near-miss distance of the seam check; no sphere face has a near-missed seam; and no
    /// more faces near-miss a seam than in the input.
    fn check(name: &str) {
        let input = inputs(name).real_solid.expect("a real solid");
        let mut k = Kernel::default();
        let mut body = build(&mut k, &input).unwrap().unwrap();
        let (selected, _) = select_edges(&k, &body, &input).ok().unwrap();
        let near_missed_before = near_missed_faces(&k, &mut body);
        let result = fillet_selected(&mut k, &mut body, &selected, &input).ok().unwrap();
        assert!(result.is_ok(), "{name}: {result:?}");
        assert_eq!(counts(&body, 0), [0, 29, 46, 11], "{name}");
        // The input's near misses (its B-spline faces' seams lie 2^-20 off the sphere's) stay
        // where they are: the round adds none.
        let near_missed_after = near_missed_faces(&k, &mut body);
        assert!(
            near_missed_after <= near_missed_before,
            "{name}: {near_missed_after} faces near-miss a seam, {near_missed_before} before"
        );
        let near_miss = 1000.0 * EFF_ZERO;
        let mut on_seam = Vec::new();
        let mut spheres = 0;
        for f in p1::faces(&body, None).unwrap() {
            let Some(surface) = face::surface(&body, f).unwrap() else {
                continue;
            };
            let SurfaceKind::Sphere(sphere) = surface.kind() else {
                continue;
            };
            spheres += 1;
            // S(u, v) = O + r (cos v (cos u X + sin u Y) + sin v Z), u in degrees: the seam
            // is the meridian at the start of u's range, in the half-plane through the axis
            // along `out`, whose normal is `across`
            let rev = sphere.base();
            let frame = rev.position();
            let (centre, x, y) = (frame.origin(), frame.x_axis(), frame.y_axis());
            let u0 = rev.step_uv_domain().min().x.to_radians();
            let out = x * u0.cos() + y * u0.sin();
            let across = y * u0.cos() - x * u0.sin();
            for v in p1::vertices(&body, None).unwrap() {
                if !vertex::faces(&body, v).unwrap().contains(&f) {
                    continue;
                }
                let p = vertex::point(&body, v).unwrap();
                let tol = vertex::tolerance(&body, v).unwrap();
                let d = (p - centre).dot(across).abs();
                // Written to accept: a NaN distance counts as off the seam.
                if (p - centre).dot(out) > 0.0 && (d.is_nan() || d <= tol) {
                    println!(
                        "21421 {name}: face {f:?} vertex {v:?} at {p:?} is {d:e} off the seam"
                    );
                    on_seam.push((f, v, d));
                }
            }
            let check = has_seam_problem_in_brep_mut::<HealLoopHandle>(
                &k,
                &mut body,
                f,
                HasSeamProblemOptions::default(),
            )
            .unwrap();
            assert_eq!(
                check.problem.near_miss,
                SurfParamType::Neither,
                "{name}: face {f:?} {:?}",
                check.problem
            );
        }
        assert!(spheres > 0, "{name}: no sphere face");
        assert!(!on_seam.is_empty(), "{name}: no vertex on the sphere's seam");
        for &(f, v, d) in &on_seam {
            // Written to accept: a NaN distance fails.
            assert!(d < near_miss, "{name}: face {f:?} vertex {v:?} is {d:e} off the seam");
        }
    }

    /// The certificate of `name`'s blessed row, whose only moved values are mass properties
    /// (centroid `x`, products `zx` and `xy`: values the integrator estimates at `1e-4` of
    /// their scale, which the vertex now on the seam moves, as any change of a face's trims
    /// does): the golden's line holds this body's estimates bit for bit, and each of the
    /// body's 12 mass values (density 1, about the origin) lies within the golden's
    /// accuracy of its certified band (`round_definition_proof`'s `volume_band` and
    /// `mass_bands`: certified enclosures of the body as stored, widened by its blend faces'
    /// proved distance from their definitions), so of the exact value.
    fn mass_certified(name: &str) {
        use super::round_definition_proof::{certified_blends, mass_bands, volume_band};
        let golden = crate::golden::read(
            &crate::golden::default_dir(),
            "blend.fillet_executive_p3",
        )
        .unwrap();
        let row: Vec<f64> = golden[name]["values"]["mass_properties"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect();
        let mut k = Kernel::default();
        let mut body = result_body_of(&mut k, name);
        let blends = certified_blends(&k, &mut body, name);
        let estimates = volume_band(&k, &mut body, &blends, name);
        let m = &estimates.golden;
        let at = |p: Vec3| [p.x, p.y, p.z];
        let mut ours = vec![m.properties.area, m.properties.volume, m.properties.mass];
        ours.extend(at(m.centroid));
        ours.extend(at(m.inertia));
        ours.extend(at(m.products));
        let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&ours), bits(&row), "{name}: not the golden's estimates");
        mass_bands(&k, &mut body, &estimates, name);
    }

    #[test]
    fn e1_r0_1_mass() {
        mass_certified(ROWS[0]);
    }

    #[test]
    fn e1_r0_5_mass() {
        mass_certified(ROWS[1]);
    }

    #[test]
    fn e4_r0_1_mass() {
        mass_certified(ROWS[2]);
    }

    #[test]
    fn e4_r0_5_mass() {
        mass_certified(ROWS[3]);
    }

    #[test]
    fn e1_r0_1() {
        check(ROWS[0]);
    }

    #[test]
    fn e1_r0_5() {
        check(ROWS[1]);
    }

    #[test]
    fn e4_r0_1() {
        check(ROWS[2]);
    }

    #[test]
    fn e4_r0_5() {
        check(ROWS[3]);
    }
}

