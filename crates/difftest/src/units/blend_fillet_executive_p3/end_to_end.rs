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
    /// An upstream ASCII `.smb` file: `SmBrep::ReadFromFile` in the shim, imported
    /// through the `topology.brep_p1` export on the Rust side (upstream list order).
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
            for (slot, v) in p
                .iter_mut()
                .zip(a.origin.iter().chain(&a.size).chain(&b.origin).chain(&b.size))
            {
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
}
impl RealSolid {
    fn fillet_wire(&self) -> Vec<f64> {
        let mut out = vec![
            self.radius,
            f64::from(self.xsect),
            f64::from(self.continuity),
            self.thumbweight,
            self.edges.len() as f64,
        ];
        out.extend(self.edges.iter().map(|v| *v as f64));
        out
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
            return Err(HarnessError::new("piecewise merge needs the full checkpoint"));
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
        let mut out: Vec<f64> = self.first.iter().chain(&self.second).flatten().copied().collect();
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
        Some(Source::File { path }) => {
            let mut body = crate::units::topology_brep_p1::import_file(path, false)?;
            // The import labels geometry owners with arena identities; record them as
            // the B-rep's own owner keys, as a kernel-built B-rep does, so `GetFace` /
            // `GetEdge` on that geometry resolve to the owning topology.
            body.use_arena_naming().map(|()| body)
        }
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
fn run_rust(k: &mut Kernel, body: SolidResult<Brep>, input: &RealSolid) -> SolidResult<Vec<f64>> {
    let mut body = body?;
    let edges = p1::edges(&body, None)?;
    let mut selected = Vec::<EdgeId>::new();
    let mut before = Vec::new();
    let all: Vec<usize> = (0..edges.len()).collect();
    let indices = if input.all_edges { &all } else { &input.edges };
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
        return prefix::run(k, &mut body, &selected, input, &before);
    }
    let result = apply(k, &mut body, &selected, input);
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
        Err(error) => match Output::unported(&error) {
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
        head.extend(rest.get(..3).ok_or_else(|| HarnessError::new("missing counts"))?);
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
