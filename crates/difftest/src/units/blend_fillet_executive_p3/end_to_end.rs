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
    fn wire(&self) -> (f64, [f64; ffi::SOURCE_PARAMS]) {
        let mut p = [0.0; ffi::SOURCE_PARAMS];
        let boxes = |p: &mut [f64; ffi::SOURCE_PARAMS], a: &BoxSpec, b: &BoxSpec| {
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
    fn path(&self) -> Option<&str> {
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
        let mut k = Kernel::default();
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
        if self.radius_end.is_some()
            && (self.checkpoint != Checkpoint::Full
                || self.source.is_some()
                || self.piecewise_merge
                || self.curve_blend_sih.is_some())
        {
            return Err(HarnessError::new(
                "radius_end requires the full checkpoint on a box without merge options",
            ));
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
            return ffi::validate_real_solid_setback(&self.wire(), setback.wire())
                .map_err(|e| HarnessError::new(e.to_string()));
        }
        match (&self.source, self.radius_end) {
            (Some(source), _) => {
                ffi::validate_real_source(&self.source_wire(source), source.path().is_some())
            }
            (None, Some(end)) => ffi::validate_real_variable_solid(&self.wire(), end),
            (None, None) => ffi::validate_real_solid(&self.wire()),
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

/// A source case's C++ record, or its rejection when the native fillet crashed in the
/// case's fresh child process (undefined in C++: AGENTS.md section 5.6; the upstream
/// crashes are UB-209 and UB-261). Rust is not run on a rejected case.
fn source_output(
    result: Result<(Vec<f64>, f64), solidcore_ffi::FfiError>,
) -> Result<Output, HarnessError> {
    match result {
        Ok((values, step)) => Ok(super::with_point_step(output(&values)?, step)),
        Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_CHILD_CRASHED)) => Ok(Output::rejected(
            "native C++ crash in SmApiFilletEdges (fresh child process; UB-209, UB-261)",
        )),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

/// A box case's C++ record (see [`super::fresh_cpp`]).
fn solid_output(
    result: Result<(Vec<f64>, f64), solidcore_ffi::FfiError>,
) -> Result<Output, HarnessError> {
    let (values, step) = result.map_err(|e| HarnessError::new(e.to_string()))?;
    Ok(super::with_point_step(output(&values)?, step))
}

pub(super) fn cpp(input: &RealSolid) -> Result<Output, HarnessError> {
    input.check_form()?;
    if let Some(setback) = input.corner_setback {
        let (values, step) = super::fresh_cpp(|| ffi::real_solid_setback(&input.wire(), setback.wire()))
            .map_err(|e| HarnessError::new(e.to_string()))?;
        return Ok(super::with_point_step(output(&values)?, step));
    }
    if let Some(shell) = input.shell {
        let mut wire = input.wire();
        wire.extend([shell.distance, f64::from(u8::from(shell.open_top))]);
        let (values, step) = super::fresh_cpp(|| ffi::fillet_then_shell(&wire))
            .map_err(|e| HarnessError::new(e.to_string()))?;
        return Ok(super::with_point_step(shell_output(&values)?, step));
    }
    if let (Some(step_back), Some(source)) = (input.curve_blend_sih, &input.source) {
        return source_output(super::fresh_cpp(|| {
            ffi::curve_sih_source(step_back, &input.source_wire(source), source.path())
        }));
    }
    if let Some(step_back) = input.curve_blend_sih {
        return solid_output(super::fresh_cpp(|| {
            ffi::curve_sih_solid(step_back, &input.wire())
        }));
    }
    if let Some(source) = &input.source {
        let run = if input.piecewise_merge {
            ffi::piecewise_source
        } else {
            ffi::real_source
        };
        return source_output(super::fresh_cpp(|| {
            run(&input.source_wire(source), source.path())
        }));
    }
    if input.checkpoint != Checkpoint::Full {
        return prefix::cpp(input);
    }
    if let Some(end) = input.radius_end {
        return solid_output(super::fresh_cpp(|| ffi::real_variable_solid(&input.wire(), end)));
    }
    let run = if input.piecewise_merge {
        ffi::piecewise_solid
    } else {
        ffi::real_solid
    };
    solid_output(super::fresh_cpp(|| run(&input.wire())))
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
/// The Rust body of `input`'s source (the box by default).
fn build(k: &mut Kernel, input: &RealSolid) -> Result<SolidResult<Brep>, HarnessError> {
    use solidcore::primitive::api_primitives as primitives;
    let boxed = |k: &mut Kernel, b: &BoxSpec| {
        let [x, y, z] = b.size;
        primitives::create_box(k, Vec3::from(b.origin), x, y, z)
    };
    Ok(match &input.source {
        None => {
            let [x, y, z] = input.dimensions.unwrap_or_default();
            primitives::create_box(k, Vec3::ZERO, x, y, z)
        }
        Some(Source::File { path }) => crate::own_reader::read_file(k, path)?,
        Some(Source::Cylinder {
            origin,
            radius,
            height,
        }) => primitives::create_cylinder(k, Vec3::from(*origin), *radius, *height),
        Some(Source::BoxUnion { a, b }) => boxed(k, a).and_then(|first| {
            let second = boxed(k, b)?;
            solidcore::boolean::api_brep::boolean_union(k, first, second)
        }),
        Some(Source::BoxDifference { a, b }) => boxed(k, a).and_then(|first| {
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

fn run_rust(
    k: &mut Kernel,
    body: SolidResult<Brep>,
    input: &RealSolid,
) -> Result<Vec<f64>, RealSolidRunError> {
    let mut body = body?;
    let edges = p1::edges(&body, None)?;
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
    if input.checkpoint != Checkpoint::Full {
        return prefix::run(k, &mut body, &selected, input, &before).map_err(Into::into);
    }
    let result = if let Some(setback) = input.corner_setback {
        match setback_fillet_edges(k, &mut body, &selected, input, setback) {
            Ok(()) => Ok(()),
            Err(RealSolidRunError::Kernel(error)) => Err(error),
            Err(error @ RealSolidRunError::Fixture(_)) => return Err(error),
        }
    } else {
        apply(k, &mut body, &selected, input)
    };
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
    crate::util::primitive::append(k, &body, &mut geometry)?;
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
        k,
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
        Ok(()) | Err(SolidError::Upstream { .. } | SolidError::InvalidInput { .. }) => Ok(()),
        Err(error) => Err(error.into()),
    }
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
    let mut k = Kernel::default();
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
    let result = run();
    let step = super::rust_point_step(&k);
    match result {
        Ok(values) => {
            let mut out = Output::new();
            out.ints.insert(
                "api_status_counts".into(),
                values.into_iter().map(|v| v as i64).collect(),
            );
            Ok(Some(super::with_point_step(out, step)))
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
        let mut k = Kernel::default();
        return match shell_rust(&mut k, input, shell) {
            Ok(values) => Ok(super::with_point_step(
                shell_output(&values)?,
                super::rust_point_step(&k),
            )),
            Err(error) => match Output::unported(&error) {
                Some(deferred) => Ok(deferred),
                None => Err(HarnessError::new(error.to_string())),
            },
        };
    }
    let mut k = Kernel::default();
    let body = build(&mut k, input)?;
    match run_rust(&mut k, body, input) {
        Ok(values) => {
            let out = if input.checkpoint == Checkpoint::Full {
                output(&values)?
            } else {
                prefix::output(&values, input.checkpoint)?
            };
            Ok(super::with_point_step(out, super::rust_point_step(&k)))
        }
        Err(RealSolidRunError::Fixture(error)) => Err(error),
        Err(RealSolidRunError::Kernel(error)) => match Output::unported(&error) {
            Some(deferred) => Ok(deferred),
            None => Err(HarnessError::new(error.to_string())),
        },
    }
}

/// The C++ record of [`SurfaceSurface`]: `api_status`, then for a result B-rep its
/// counts, topology and geometry.
pub(super) fn surface_surface_cpp(input: &SurfaceSurface) -> Result<Output, HarnessError> {
    let (values, step) = super::fresh_cpp(|| ffi::surface_surface(&input.wire()))
        .map_err(|e| HarnessError::new(e.to_string()))?;
    Ok(super::with_point_step(surface_surface_output(&values)?, step))
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
    ffi::validate_surface_surface(&input.wire()).map_err(|e| HarnessError::new(e.to_string()))?;
    let mut k = Kernel::default();
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
    let result = run();
    let step = super::rust_point_step(&k);
    match result {
        Ok(values) => Ok(super::with_point_step(surface_surface_output(&values)?, step)),
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
            assert!(super::cpp(&input).is_err());
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
            assert!(super::cpp(&input).is_err());
            assert!(super::rust(&input).is_err());
        }
    }

    #[test]
    fn corner_setback_requires_actual_vertex_and_prepared_corner() {
        for vertex in [[59.0, 40.0, 20.0], [0.0, 0.0, 0.0]] {
            let mut input = setback_input(8.0);
            input.corner_setback.as_mut().expect("setback").vertex = vertex;
            // A malformed fixture must not turn into a compared 1006/1007 or rejection.
            assert!(super::cpp(&input).is_err());
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
