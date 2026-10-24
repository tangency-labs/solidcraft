//! Strict conversion from schema-framed raw nodes to the native B-Rep model.

use std::collections::{BTreeMap, BTreeSet};

use crate::{ErrorDetails, ErrorKind, FieldValue, ParseError, RawField, RawNode, XbDocument, XtDocument};

use super::metrics::derive_metrics;
use super::model::{
    BlendType, Body, BodyKind, BrepDiagnostic, BrepId, BrepMetrics, BrepModel, BrepSourceFormat, CurveGeometry, CurveKind, Edge, Face, HalfEdge,
    Loop, NurbsCurve, NurbsSurface, PointGeometry, Region, RegionKind, Sense, Shell, SourceNodeRef, SurfaceGeometry, SurfaceKind, TopologyValidation,
    Vector3, Vertex,
};
use super::profile_roles::RoleAccess;

/// Default modeller linear precision, in metres: the bound within which a trim
/// point with unset parameters must lie on its basis line.
const LINE_TRIM_TOLERANCE: f64 = 1.0e-8;

/// Map a complete `X_B` raw document into the Parasolid-native B-Rep model.
///
/// # Errors
///
/// Returns a strict semantic error for missing bodies, malformed required fields,
/// unresolved typed references, inconsistent NURBS arrays, or invalid topology rings.
pub fn map_xb_brep(document: &XbDocument) -> Result<BrepModel, ParseError> {
    map_xb_brep_with_diagnostic_limit(document, usize::MAX)
}

/// Map an `X_B` document while bounding retained non-fatal diagnostics.
///
/// # Errors
///
/// Returns a strict semantic error, an invalid-limit error for zero, or a
/// limit error when mapping would retain more diagnostics than requested.
pub fn map_xb_brep_with_diagnostic_limit(document: &XbDocument, max_diagnostics: usize) -> Result<BrepModel, ParseError> {
    validate_diagnostic_limit(max_diagnostics)?;
    Mapper::new(
        BrepSourceFormat::Binary,
        document.schema_key.raw(),
        &document.nodes,
        RoleAccess::select(&document.schema_provider, document.schema_key.raw(), &document.schemas)?,
    )
    .with_max_diagnostics(max_diagnostics)
    .map()
}

/// Map a complete `X_T` raw document into the Parasolid-native B-Rep model.
///
/// # Errors
///
/// Returns a strict semantic error for missing bodies, malformed required fields,
/// unresolved typed references, inconsistent NURBS arrays, or invalid topology rings.
pub fn map_xt_brep(document: &XtDocument) -> Result<BrepModel, ParseError> {
    map_xt_brep_with_diagnostic_limit(document, usize::MAX)
}

/// Map an `X_T` document while bounding retained non-fatal diagnostics.
///
/// # Errors
///
/// Returns a strict semantic error, an invalid-limit error for zero, or a
/// limit error when mapping would retain more diagnostics than requested.
pub fn map_xt_brep_with_diagnostic_limit(document: &XtDocument, max_diagnostics: usize) -> Result<BrepModel, ParseError> {
    validate_diagnostic_limit(max_diagnostics)?;
    Mapper::new(
        BrepSourceFormat::Text,
        document.schema_key.raw(),
        &document.nodes,
        RoleAccess::select(&document.schema_provider, document.schema_key.raw(), &document.schemas)?,
    )
    .with_max_diagnostics(max_diagnostics)
    .map()
}

fn validate_diagnostic_limit(max_diagnostics: usize) -> Result<(), ParseError> {
    if max_diagnostics == 0 {
        return Err(ParseError::invalid_limit("max_diagnostics", 0));
    }
    Ok(())
}

#[derive(Default)]
struct IdMaps {
    bodies: BTreeMap<u32, BrepId>,
    regions: BTreeMap<u32, BrepId>,
    shells: BTreeMap<u32, BrepId>,
    faces: BTreeMap<u32, BrepId>,
    loops: BTreeMap<u32, BrepId>,
    half_edges: BTreeMap<u32, BrepId>,
    edges: BTreeMap<u32, BrepId>,
    vertices: BTreeMap<u32, BrepId>,
    points: BTreeMap<u32, BrepId>,
    curves: BTreeMap<u32, BrepId>,
    surfaces: BTreeMap<u32, BrepId>,
}

impl IdMaps {
    fn new(nodes: &[RawNode], roles: RoleAccess) -> Self {
        Self {
            bodies: assign_ids(nodes, |node| roles.type_name(node) == "BODY"),
            regions: assign_ids(nodes, |node| roles.type_name(node) == "REGION"),
            shells: assign_ids(nodes, |node| roles.type_name(node) == "SHELL"),
            faces: assign_ids(nodes, |node| roles.type_name(node) == "FACE"),
            loops: assign_ids(nodes, |node| roles.type_name(node) == "LOOP"),
            half_edges: assign_ids(nodes, |node| roles.type_name(node) == "HALFEDGE"),
            edges: assign_ids(nodes, |node| roles.type_name(node) == "EDGE"),
            vertices: assign_ids(nodes, |node| roles.type_name(node) == "VERTEX"),
            points: assign_ids(nodes, |node| roles.type_name(node) == "POINT"),
            curves: assign_ids(nodes, |node| roles.is_curve(node)),
            surfaces: assign_ids(nodes, |node| roles.is_surface(node)),
        }
    }
}

fn assign_ids(nodes: &[RawNode], predicate: impl Fn(&RawNode) -> bool) -> BTreeMap<u32, BrepId> {
    nodes.iter().filter(|node| predicate(node)).enumerate().map(|(id, node)| (node.index, u32::try_from(id).unwrap_or(u32::MAX))).collect()
}

struct Mapper<'a> {
    source_format: BrepSourceFormat,
    schema_key: &'a str,
    nodes: BTreeMap<u32, &'a RawNode>,
    ids: IdMaps,
    roles: RoleAccess,
    diagnostics: Vec<BrepDiagnostic>,
    max_diagnostics: usize,
}

impl<'a> Mapper<'a> {
    fn new(source_format: BrepSourceFormat, schema_key: &'a str, nodes: &'a [RawNode], roles: RoleAccess) -> Self {
        Self {
            source_format,
            schema_key,
            nodes: nodes.iter().map(|node| (node.index, node)).collect(),
            ids: IdMaps::new(nodes, roles),
            roles,
            diagnostics: Vec::new(),
            max_diagnostics: usize::MAX,
        }
    }

    const fn with_max_diagnostics(mut self, max_diagnostics: usize) -> Self {
        self.max_diagnostics = max_diagnostics;
        self
    }

    #[allow(clippy::too_many_lines)]
    fn map(mut self) -> Result<BrepModel, ParseError> {
        if self.ids.bodies.is_empty() {
            return Err(ParseError::new(ErrorKind::MissingBrepBody, 0, "raw document contains no BODY node", ErrorDetails::None));
        }

        let bodies = self.ordered_indices(&self.ids.bodies).into_iter().map(|index| self.map_body(index)).collect::<Result<Vec<_>, _>>()?;
        let regions = self.ordered_indices(&self.ids.regions).into_iter().map(|index| self.map_region(index)).collect::<Result<Vec<_>, _>>()?;
        let shells = self.ordered_indices(&self.ids.shells).into_iter().map(|index| self.map_shell(index)).collect::<Result<Vec<_>, _>>()?;
        let faces = self.ordered_indices(&self.ids.faces).into_iter().map(|index| self.map_face(index)).collect::<Result<Vec<_>, _>>()?;
        let loops = self.ordered_indices(&self.ids.loops).into_iter().map(|index| self.map_loop(index)).collect::<Result<Vec<_>, _>>()?;
        let half_edges =
            self.ordered_indices(&self.ids.half_edges).into_iter().map(|index| self.map_half_edge(index)).collect::<Result<Vec<_>, _>>()?;
        let edges = self.ordered_indices(&self.ids.edges).into_iter().map(|index| self.map_edge(index)).collect::<Result<Vec<_>, _>>()?;
        let vertices = self.ordered_indices(&self.ids.vertices).into_iter().map(|index| self.map_vertex(index)).collect::<Result<Vec<_>, _>>()?;
        let points = self.ordered_indices(&self.ids.points).into_iter().map(|index| self.map_point(index)).collect::<Result<Vec<_>, _>>()?;

        let mut curves = Vec::with_capacity(self.ids.curves.len());
        for index in self.ordered_indices(&self.ids.curves) {
            curves.push(self.map_curve(index)?);
        }
        let mut surfaces = Vec::with_capacity(self.ids.surfaces.len());
        for index in self.ordered_indices(&self.ids.surfaces) {
            surfaces.push(self.map_surface(index)?);
        }

        let topology = self.validate_topology(&bodies, &regions, &shells, &faces, &loops, &half_edges, &edges, &vertices)?;
        let mut model = BrepModel {
            source_format: self.source_format,
            schema_key: self.schema_key.to_owned(),
            complete: false,
            bodies,
            regions,
            shells,
            faces,
            loops,
            half_edges,
            edges,
            vertices,
            points,
            curves,
            surfaces,
            topology,
            metrics: BrepMetrics { bounding_box: None, surface_area: None, volume: None },
            diagnostics: self.diagnostics,
        };
        model.metrics = derive_metrics(&model);
        model.complete = model.topology.valid && model.diagnostics.is_empty();
        Ok(model)
    }

    #[allow(clippy::unused_self)]
    fn ordered_indices(&self, ids: &BTreeMap<u32, BrepId>) -> Vec<u32> {
        let mut values = ids.iter().map(|(index, id)| (*id, *index)).collect::<Vec<_>>();
        values.sort_unstable();
        values.into_iter().map(|(_, index)| index).collect()
    }

    fn map_body(&self, index: u32) -> Result<Body, ParseError> {
        let node = self.node(index)?;
        let kind = match self.byte(node, "body_type")? {
            1 => BodyKind::Solid,
            2 => BodyKind::Wire,
            3 => BodyKind::Sheet,
            6 => BodyKind::General,
            value => {
                return Err(self.invalid_field(node, "body_type", format!("unknown body type {value}")));
            }
        };
        Ok(Body {
            id: self.id(&self.ids.bodies, node, "BODY")?,
            kind,
            size_resolution: self.double(node, "res_size")?,
            linear_resolution: self.double(node, "res_linear")?,
            regions: self.null_chain(node, "region", "REGION", "next", &self.ids.regions)?,
            edges: self.null_chain(node, "edge", "EDGE", "next", &self.ids.edges)?,
            vertices: self.null_chain(node, "vertex", "VERTEX", "next", &self.ids.vertices)?,
            source: self.source(node),
        })
    }

    fn map_region(&self, index: u32) -> Result<Region, ParseError> {
        let node = self.node(index)?;
        let kind = match self.character(node, "type")? {
            b'S' => RegionKind::Solid,
            b'V' => RegionKind::Void,
            value => {
                return Err(self.invalid_field(node, "type", format!("unknown region type 0x{value:02x}")));
            }
        };
        Ok(Region {
            id: self.id(&self.ids.regions, node, "REGION")?,
            kind,
            body: self.required_id(node, "body", "BODY", &self.ids.bodies)?,
            shells: self.null_chain(node, "shell", "SHELL", "next", &self.ids.shells)?,
            source: self.source(node),
        })
    }

    fn map_shell(&self, index: u32) -> Result<Shell, ParseError> {
        let node = self.node(index)?;
        Ok(Shell {
            id: self.id(&self.ids.shells, node, "SHELL")?,
            region: self.required_id(node, "region", "REGION", &self.ids.regions)?,
            back_faces: self.null_chain(node, "face", "FACE", "next", &self.ids.faces)?,
            front_faces: self.null_chain(node, "front_face", "FACE", "next_front", &self.ids.faces)?,
            wire_edges: self.null_chain(node, "edge", "EDGE", "next", &self.ids.edges)?,
            isolated_vertex: self.optional_id(node, "vertex", "VERTEX", &self.ids.vertices)?,
            source: self.source(node),
        })
    }

    fn map_face(&self, index: u32) -> Result<Face, ParseError> {
        let node = self.node(index)?;
        Ok(Face {
            id: self.id(&self.ids.faces, node, "FACE")?,
            back_shell: self.required_id(node, "shell", "SHELL", &self.ids.shells)?,
            front_shell: self.required_id(node, "front_shell", "SHELL", &self.ids.shells)?,
            loops: self.null_chain(node, "loop", "LOOP", "next", &self.ids.loops)?,
            surface: self.optional_id(node, "surface", "surface geometry", &self.ids.surfaces)?,
            sense: self.sense(node, "sense")?,
            source: self.source(node),
        })
    }

    fn map_loop(&self, index: u32) -> Result<Loop, ParseError> {
        let node = self.node(index)?;
        Ok(Loop {
            id: self.id(&self.ids.loops, node, "LOOP")?,
            face: self.required_id(node, "face", "FACE", &self.ids.faces)?,
            half_edges: self.required_ring(node, "halfedge", "HALFEDGE", "forward", &self.ids.half_edges)?,
            source: self.source(node),
        })
    }

    fn map_half_edge(&self, index: u32) -> Result<HalfEdge, ParseError> {
        let node = self.node(index)?;
        let loop_id = self.optional_id(node, "loop", "LOOP", &self.ids.loops)?;
        Ok(HalfEdge {
            id: self.id(&self.ids.half_edges, node, "HALFEDGE")?,
            loop_id,
            forward: self.optional_id(node, "forward", "HALFEDGE", &self.ids.half_edges)?,
            backward: self.optional_id(node, "backward", "HALFEDGE", &self.ids.half_edges)?,
            vertex: self.optional_id(node, "vertex", "VERTEX", &self.ids.vertices)?,
            other: self.optional_id(node, "other", "HALFEDGE", &self.ids.half_edges)?,
            edge: self.optional_id(node, "edge", "EDGE", &self.ids.edges)?,
            curve: self.optional_id(node, "curve", "curve geometry", &self.ids.curves)?,
            sense: self.sense(node, "sense")?,
            dummy: loop_id.is_none(),
            source: self.source(node),
        })
    }

    fn map_edge(&self, index: u32) -> Result<Edge, ParseError> {
        let node = self.node(index)?;
        let half_edges = self.required_ring(node, "halfedge", "HALFEDGE", "other", &self.ids.half_edges)?;
        let first_index = self.required_pointer(node, "halfedge")?;
        let first = self.node(first_index)?;
        let other_index = self.required_pointer(first, "other")?;
        let other = self.node(other_index)?;
        let end_vertex = self.optional_id(first, "vertex", "VERTEX", &self.ids.vertices)?;
        let start_vertex = self.optional_id(other, "vertex", "VERTEX", &self.ids.vertices)?;
        if start_vertex.is_some() != end_vertex.is_some() {
            return Err(self.invalid_topology(node, "edge_endpoints", "edge must have either zero or two endpoint vertices"));
        }
        Ok(Edge {
            id: self.id(&self.ids.edges, node, "EDGE")?,
            owner: self.required_source(node, "owner")?,
            half_edges,
            start_vertex,
            end_vertex,
            curve: self.optional_id(node, "curve", "curve geometry", &self.ids.curves)?,
            tolerance: self.optional_double(node, "tolerance")?,
            source: self.source(node),
        })
    }

    fn map_vertex(&self, index: u32) -> Result<Vertex, ParseError> {
        let node = self.node(index)?;
        Ok(Vertex {
            id: self.id(&self.ids.vertices, node, "VERTEX")?,
            point: self.required_id(node, "point", "POINT", &self.ids.points)?,
            tolerance: self.optional_double(node, "tolerance")?,
            owner: self.required_source(node, "owner")?,
            source: self.source(node),
        })
    }

    fn map_point(&self, index: u32) -> Result<PointGeometry, ParseError> {
        let node = self.node(index)?;
        Ok(PointGeometry {
            id: self.id(&self.ids.points, node, "POINT")?,
            position: self.vector(node, "pvec")?,
            owner: self.optional_source(node, "owner")?,
            source: self.source(node),
        })
    }

    #[allow(clippy::too_many_lines)]
    fn map_curve(&mut self, index: u32) -> Result<CurveGeometry, ParseError> {
        let node = self.node(index)?;
        let kind = match self.roles.type_name(node) {
            "LINE" => CurveKind::Line { point: self.vector(node, "pvec")?, direction: self.vector(node, "direction")? },
            "CIRCLE" => CurveKind::Circle {
                center: self.vector(node, "centre")?,
                normal: self.vector(node, "normal")?,
                x_axis: self.vector(node, "x_axis")?,
                radius: self.positive_double(node, "radius")?,
            },
            "ELLIPSE" => CurveKind::Ellipse {
                center: self.vector(node, "centre")?,
                normal: self.vector(node, "normal")?,
                x_axis: self.vector(node, "x_axis")?,
                major_radius: self.positive_double(node, "major_radius")?,
                minor_radius: self.positive_double(node, "minor_radius")?,
            },
            "PARABOLA" => CurveKind::Parabola {
                origin: self.vector(node, "origin")?,
                normal: self.vector(node, "normal")?,
                x_axis: self.vector(node, "x_axis")?,
                focal_length: self.positive_double(node, "focal_length")?,
            },
            "HYPERBOLA" => CurveKind::Hyperbola {
                origin: self.vector(node, "origin")?,
                normal: self.vector(node, "normal")?,
                x_axis: self.vector(node, "x_axis")?,
                transverse_radius: self.positive_double(node, "transverse_radius")?,
                conjugate_radius: self.positive_double(node, "conjugate_radius")?,
            },
            "TRIMMED_CURVE" => self.trimmed_curve(node)?,
            "B_CURVE" => CurveKind::Nurbs(self.nurbs_curve(node)?),
            "SP_CURVE" => self.surface_parametric_curve(node)?,
            "INTERSECTION" => self.intersection_curve(node)?,
            type_name => {
                let source = self.source(node);
                push_diagnostic(
                    &mut self.diagnostics,
                    self.max_diagnostics,
                    node.byte_range.start,
                    BrepDiagnostic {
                        code: "geometry.unsupported_curve",
                        message: format!("curve type {type_name} is retained without semantic decoding"),
                        role: "curve",
                        source,
                    },
                )?;
                CurveKind::Unsupported { type_name: type_name.to_owned() }
            }
        };
        Ok(CurveGeometry {
            id: self.id(&self.ids.curves, node, "curve geometry")?,
            sense: self.sense(node, "sense")?,
            owner: self.optional_source(node, "owner")?,
            kind,
            source: self.source(node),
        })
    }

    #[allow(clippy::too_many_lines)]
    fn map_surface(&mut self, index: u32) -> Result<SurfaceGeometry, ParseError> {
        let node = self.node(index)?;
        let kind = match self.roles.type_name(node) {
            "PLANE" => {
                SurfaceKind::Plane { point: self.vector(node, "pvec")?, normal: self.vector(node, "normal")?, x_axis: self.vector(node, "x_axis")? }
            }
            "CYLINDER" => SurfaceKind::Cylinder {
                point: self.vector(node, "pvec")?,
                axis: self.vector(node, "axis")?,
                radius: self.positive_double(node, "radius")?,
                x_axis: self.vector(node, "x_axis")?,
            },
            "CONE" => SurfaceKind::Cone {
                point: self.vector(node, "pvec")?,
                axis: self.vector(node, "axis")?,
                radius: self.nonnegative_double(node, "radius")?,
                sin_half_angle: self.double(node, "sin_half_angle")?,
                cos_half_angle: self.double(node, "cos_half_angle")?,
                x_axis: self.vector(node, "x_axis")?,
            },
            "SPHERE" => SurfaceKind::Sphere {
                center: self.vector(node, "centre")?,
                radius: self.positive_double(node, "radius")?,
                axis: self.vector(node, "axis")?,
                x_axis: self.vector(node, "x_axis")?,
            },
            "TORUS" => SurfaceKind::Torus {
                center: self.vector(node, "centre")?,
                axis: self.vector(node, "axis")?,
                major_radius: self.double(node, "major_radius")?,
                minor_radius: self.positive_double(node, "minor_radius")?,
                x_axis: self.vector(node, "x_axis")?,
            },
            "BLENDED_EDGE" => self.blended_edge_surface(node)?,
            "BLEND_BOUND" => {
                let boundary_index = self.nonnegative_short(node, "boundary")?;
                if boundary_index > 1 {
                    return Err(self.invalid_geometry(node, "boundary", "blend boundary index must be zero or one"));
                }
                SurfaceKind::BlendBoundary {
                    boundary_index: u8::try_from(boundary_index).unwrap_or(u8::MAX),
                    blend_surface: self.required_id(node, "blend", "surface geometry", &self.ids.surfaces)?,
                }
            }
            "OFFSET_SURF" => SurfaceKind::Offset {
                basis_surface: self.required_id(node, "surface", "surface geometry", &self.ids.surfaces)?,
                offset: self.double(node, "offset")?,
            },
            "B_SURFACE" => SurfaceKind::Nurbs(self.nurbs_surface(node)?),
            type_name => {
                let source = self.source(node);
                push_diagnostic(
                    &mut self.diagnostics,
                    self.max_diagnostics,
                    node.byte_range.start,
                    BrepDiagnostic {
                        code: "geometry.unsupported_surface",
                        message: format!("surface type {type_name} is retained without semantic decoding"),
                        role: "surface",
                        source,
                    },
                )?;
                SurfaceKind::Unsupported { type_name: type_name.to_owned() }
            }
        };
        Ok(SurfaceGeometry {
            id: self.id(&self.ids.surfaces, node, "surface geometry")?,
            sense: self.sense(node, "sense")?,
            owner: self.optional_source(node, "owner")?,
            kind,
            source: self.source(node),
        })
    }

    fn blended_edge_surface(&self, node: &RawNode) -> Result<SurfaceKind, ParseError> {
        let blend_type = match self.character(node, "blend_type")? {
            b'R' => BlendType::RollingBall,
            b'E' => BlendType::CliffEdge,
            value => {
                return Err(self.invalid_geometry(node, "blend_type", format!("blend type must be 'R' or 'E', observed 0x{value:02x}")));
            }
        };
        let supporting_surfaces = self.required_id_pair(node, "surface", "surface geometry", &self.ids.surfaces)?;
        let boundary_surfaces = self.optional_id_pair(node, "boundary", "surface geometry", &self.ids.surfaces)?;
        Ok(SurfaceKind::BlendedEdge {
            blend_type,
            supporting_surfaces,
            spine_curve: self.required_id(node, "spine", "curve geometry", &self.ids.curves)?,
            ranges: self.double_pair(node, "range")?,
            thumb_weights: self.double_pair(node, "thumb_weight")?,
            boundary_surfaces,
            start: self.optional_source(node, "start")?,
            end: self.optional_source(node, "end")?,
        })
    }

    #[allow(clippy::float_cmp)] // Equal parameters are forbidden, however short a valid trim is.
    fn trimmed_curve(&self, node: &RawNode) -> Result<CurveKind, ParseError> {
        let basis_curve = self.required_id(node, "basis_curve", "curve geometry", &self.ids.curves)?;
        let basis_index = self.required_pointer(node, "basis_curve")?;
        if basis_index == node.index {
            return Err(self.invalid_geometry(node, "basis_curve", "trimmed curve references itself"));
        }
        if self.sense(node, "sense")? != Sense::Positive {
            return Err(self.invalid_geometry(node, "sense", "trimmed curve sense must be positive"));
        }
        let start_point = self.vector(node, "point_1")?;
        let end_point = self.vector(node, "point_2")?;
        let basis = self.node(basis_index)?;
        let stored = [self.optional_double(node, "parm_1")?, self.optional_double(node, "parm_2")?];
        let [start_parameter, end_parameter] = match stored {
            [Some(start), Some(end)] => [start, end],
            // Both parameters can be unset on a line. Its published
            // parameterisation R(t) = P + tD, with D a unit vector, gives them
            // from the stored end points, which must lie on that line.
            [None, None] if self.roles.type_name(basis) == "LINE" => {
                let origin = self.vector(basis, "pvec")?.to_array();
                let direction = self.vector(basis, "direction")?.to_array();
                let unit = direction.iter().map(|d| d * d).sum::<f64>().sqrt();
                if (unit - 1.0).abs() > LINE_TRIM_TOLERANCE {
                    return Err(self.invalid_geometry(basis, "direction", "basis line direction is not a unit vector"));
                }
                let mut parameters = [0.0; 2];
                for (parameter, (point, field)) in parameters.iter_mut().zip([(start_point, "point_1"), (end_point, "point_2")]) {
                    let point = point.to_array();
                    let along = (0..3).map(|i| (point[i] - origin[i]) * direction[i]).sum::<f64>();
                    let off = (0..3).map(|i| (origin[i] + along * direction[i] - point[i]).powi(2)).sum::<f64>().sqrt();
                    if off.is_nan() || off > LINE_TRIM_TOLERANCE {
                        return Err(self.invalid_geometry(node, field, "trim point is not on its basis line"));
                    }
                    *parameter = along;
                }
                parameters
            }
            _ => {
                let field = if stored[0].is_none() { "parm_1" } else { "parm_2" };
                return Err(self.invalid_geometry(node, field, "required double is null or non-finite"));
            }
        };
        let basis_sense = self.sense(basis, "sense")?;
        if start_parameter == end_parameter
            || (basis_sense == Sense::Positive && end_parameter < start_parameter)
            || (basis_sense == Sense::Negative && end_parameter > start_parameter)
        {
            return Err(self.invalid_geometry(node, "parm_2", "trim parameters must be distinct and follow the basis curve sense"));
        }
        Ok(CurveKind::Trimmed { basis_curve, start_point, end_point, start_parameter, end_parameter })
    }

    fn intersection_curve(&self, node: &RawNode) -> Result<CurveKind, ParseError> {
        let chart = self.required_typed_node(node, "chart", "CHART")?;
        self.double(chart, "base_parameter")?;
        self.double(chart, "base_scale")?;
        self.optional_double(chart, "chordal_error")?;
        self.optional_double(chart, "angular_error")?;
        let count = self.nonnegative_integer(chart, "chart_count")?;
        let chart_points = self.intersection_points(chart, usize::try_from(count).unwrap_or(usize::MAX))?;
        let [start_points, end_points] = ["start", "end"].map(|field| {
            let limit = self.required_typed_node(node, field, "LIMIT")?;
            let count = match self.character(limit, "type")? {
                b'H' | b'L' => 1,
                b'T' => 2,
                _ => {
                    return Err(self.invalid_geometry(limit, "type", "invalid intersection limit kind"));
                }
            };
            self.intersection_points(limit, count)
        });
        let surface_indices = self.pointer_array(node, "surface")?;
        if surface_indices.len() != 2 {
            return Err(self.invalid_field(node, "surface", "intersection curve requires exactly two surfaces"));
        }
        let surfaces = [
            self.id_from_target(node, "surface", surface_indices[0], "surface geometry", &self.ids.surfaces)?,
            self.id_from_target(node, "surface", surface_indices[1], "surface geometry", &self.ids.surfaces)?,
        ];
        let intersection_data = self.optional_source_if_field(node, "intersection_data")?;
        if intersection_data.is_some() {
            self.required_typed_node(node, "intersection_data", "INTERSECTION_DATA")?;
        }
        Ok(CurveKind::Intersection {
            surfaces,
            chart: self.required_source(node, "chart")?,
            chart_points,
            start: self.required_source(node, "start")?,
            start_points: start_points?,
            end: self.required_source(node, "end")?,
            end_points: end_points?,
            intersection_data,
        })
    }

    fn intersection_points(&self, node: &RawNode, count: usize) -> Result<Vec<Vector3>, ParseError> {
        let values = &self.field(node, "hvec")?.values;
        if count == 0 || values.len() != count {
            return Err(self.invalid_geometry(node, "hvec", "intersection point count mismatch"));
        }
        values
            .iter()
            .map(|value| match value {
                FieldValue::IntersectionPoint([Some(x), Some(y), Some(z)]) if x.is_finite() && y.is_finite() && z.is_finite() => {
                    Ok(Vector3 { x: *x, y: *y, z: *z })
                }
                _ => Err(self.invalid_geometry(node, "hvec", "intersection point is null or non-finite")),
            })
            .collect()
    }

    fn surface_parametric_curve(&self, node: &RawNode) -> Result<CurveKind, ParseError> {
        let parameter_curve = self.required_typed_node(node, "b_curve", "B_CURVE")?;
        let nurbs = self.required_typed_node(parameter_curve, "nurbs", "NURBS_CURVE")?;
        let rational = self.logical(nurbs, "rational")?;
        let dimension = self.positive_short(nurbs, "vertex_dim")?;
        if dimension != if rational { 3 } else { 2 } {
            return Err(self.invalid_geometry(nurbs, "vertex_dim", "surface parameter curve must have two coordinates plus an optional weight"));
        }
        let tolerance_to_original = self.optional_double(node, "tolerance_to_original")?;
        if tolerance_to_original.is_some_and(|value| value < 0.0) {
            return Err(self.invalid_geometry(node, "tolerance_to_original", "negative tolerance"));
        }
        Ok(CurveKind::SurfaceParametric {
            surface: self.required_id(node, "surface", "surface geometry", &self.ids.surfaces)?,
            parameter_curve: self.required_id(node, "b_curve", "curve geometry", &self.ids.curves)?,
            original_curve: self.optional_id(node, "original", "curve geometry", &self.ids.curves)?,
            tolerance_to_original,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn nurbs_curve(&self, owner: &RawNode) -> Result<NurbsCurve, ParseError> {
        let nurbs = self.required_typed_node(owner, "nurbs", "NURBS_CURVE")?;
        let control_count = self.nonnegative_integer(nurbs, "n_vertices")?;
        let vertex_dimension = self.positive_short(nurbs, "vertex_dim")?;
        let distinct_knot_count = self.nonnegative_integer(nurbs, "n_knots")?;
        let vertices_node = self.required_typed_node(nurbs, "bspline_vertices", "BSPLINE_VERTICES")?;
        let multiplicities_node = self.required_typed_node(nurbs, "knot_mult", "KNOT_MULT")?;
        let knots_node = self.required_typed_node(nurbs, "knots", "KNOT_SET")?;
        let flat_vertices = self.double_array(vertices_node, "vertices")?;
        let distinct_knot_count_usize = usize::try_from(distinct_knot_count).unwrap_or(usize::MAX);
        let multiplicities = self.positive_short_array_prefix(multiplicities_node, "mult", distinct_knot_count_usize)?;
        let knots = self.double_array_prefix(knots_node, "knots", distinct_knot_count_usize)?;
        self.validate_count(
            nurbs,
            "vertices",
            usize::try_from(control_count).unwrap_or(usize::MAX).saturating_mul(usize::from(vertex_dimension)),
            flat_vertices.len(),
        )?;
        let degree = self.positive_short(nurbs, "degree")?;
        let rational = self.logical(nurbs, "rational")?;
        let periodic = self.logical(nurbs, "periodic")?;
        let closed = self.logical(nurbs, "closed")?;
        if control_count <= u32::from(degree) || !(if rational { 3..=4 } else { 2..=3 }).contains(&vertex_dimension) || (periodic && !closed) {
            return Err(self.invalid_geometry(nurbs, "n_vertices", "invalid NURBS dimensions or closure"));
        }
        if rational && flat_vertices.chunks(usize::from(vertex_dimension)).any(|vertex| vertex.last().is_none_or(|weight| *weight <= 0.0)) {
            return Err(self.invalid_geometry(nurbs, "vertices", "NURBS weights must be positive"));
        }
        self.validate_nurbs_knots(nurbs, "knots", &knots, &multiplicities, control_count, degree)?;
        Ok(NurbsCurve {
            degree,
            control_vertex_count: control_count,
            vertex_dimension,
            knot_type: self.byte(nurbs, "knot_type")?,
            periodic,
            closed,
            rational,
            curve_form: self.byte(nurbs, "curve_form")?,
            control_vertices: flat_vertices.chunks(usize::from(vertex_dimension)).map(<[f64]>::to_vec).collect(),
            knots,
            knot_multiplicities: multiplicities,
            sources: vec![self.source(nurbs), self.source(vertices_node), self.source(multiplicities_node), self.source(knots_node)],
        })
    }

    #[allow(clippy::too_many_lines)]
    fn nurbs_surface(&self, owner: &RawNode) -> Result<NurbsSurface, ParseError> {
        let nurbs = self.required_typed_node(owner, "nurbs", "NURBS_SURF")?;
        let u_count = self.nonnegative_integer(nurbs, "n_u_vertices")?;
        let v_count = self.nonnegative_integer(nurbs, "n_v_vertices")?;
        let vertex_dimension = self.positive_short(nurbs, "vertex_dim")?;
        let vertices_node = self.required_typed_node(nurbs, "bspline_vertices", "BSPLINE_VERTICES")?;
        let u_mult_node = self.required_typed_node(nurbs, "u_knot_mult", "KNOT_MULT")?;
        let v_mult_node = self.required_typed_node(nurbs, "v_knot_mult", "KNOT_MULT")?;
        let u_knots_node = self.required_typed_node(nurbs, "u_knots", "KNOT_SET")?;
        let v_knots_node = self.required_typed_node(nurbs, "v_knots", "KNOT_SET")?;
        let flat_vertices = self.double_array(vertices_node, "vertices")?;
        let u_knot_count = usize::try_from(self.nonnegative_integer(nurbs, "n_u_knots")?).unwrap_or(usize::MAX);
        let v_knot_count = usize::try_from(self.nonnegative_integer(nurbs, "n_v_knots")?).unwrap_or(usize::MAX);
        let u_multiplicities = self.positive_short_array_prefix(u_mult_node, "mult", u_knot_count)?;
        let v_multiplicities = self.positive_short_array_prefix(v_mult_node, "mult", v_knot_count)?;
        let u_knots = self.double_array_prefix(u_knots_node, "knots", u_knot_count)?;
        let v_knots = self.double_array_prefix(v_knots_node, "knots", v_knot_count)?;
        let vertex_count = usize::try_from(u_count).unwrap_or(usize::MAX).saturating_mul(usize::try_from(v_count).unwrap_or(usize::MAX));
        self.validate_count(nurbs, "vertices", vertex_count.saturating_mul(usize::from(vertex_dimension)), flat_vertices.len())?;
        let u_degree = self.positive_short(nurbs, "u_degree")?;
        let v_degree = self.positive_short(nurbs, "v_degree")?;
        self.validate_nurbs_knots(nurbs, "u_knots", &u_knots, &u_multiplicities, u_count, u_degree)?;
        self.validate_nurbs_knots(nurbs, "v_knots", &v_knots, &v_multiplicities, v_count, v_degree)?;
        let rational = self.logical(nurbs, "rational")?;
        let u_periodic = self.logical(nurbs, "u_periodic")?;
        let v_periodic = self.logical(nurbs, "v_periodic")?;
        let u_closed = self.logical(nurbs, "u_closed")?;
        let v_closed = self.logical(nurbs, "v_closed")?;
        if u_count <= u32::from(u_degree)
            || v_count <= u32::from(v_degree)
            || vertex_dimension != if rational { 4 } else { 3 }
            || (u_periodic && !u_closed)
            || (v_periodic && !v_closed)
        {
            return Err(self.invalid_geometry(nurbs, "vertices", "invalid NURBS surface dimensions or closure"));
        }
        if rational && flat_vertices.chunks(usize::from(vertex_dimension)).any(|vertex| vertex.last().is_none_or(|weight| *weight <= 0.0)) {
            return Err(self.invalid_geometry(nurbs, "vertices", "NURBS weights must be positive"));
        }
        Ok(NurbsSurface {
            u_degree,
            v_degree,
            u_control_vertex_count: u_count,
            v_control_vertex_count: v_count,
            vertex_dimension,
            u_knot_type: self.byte(nurbs, "u_knot_type")?,
            v_knot_type: self.byte(nurbs, "v_knot_type")?,
            u_periodic,
            v_periodic,
            u_closed,
            v_closed,
            rational,
            surface_form: self.byte(nurbs, "surface_form")?,
            control_vertices: flat_vertices.chunks(usize::from(vertex_dimension)).map(<[f64]>::to_vec).collect(),
            u_knots,
            v_knots,
            u_knot_multiplicities: u_multiplicities,
            v_knot_multiplicities: v_multiplicities,
            sources: vec![
                self.source(nurbs),
                self.source(vertices_node),
                self.source(u_mult_node),
                self.source(v_mult_node),
                self.source(u_knots_node),
                self.source(v_knots_node),
            ],
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_nurbs_knots(
        &self,
        node: &RawNode,
        field: &'static str,
        knots: &[f64],
        multiplicities: &[u16],
        control_count: u32,
        degree: u16,
    ) -> Result<(), ParseError> {
        if knots.len() < 2
            || knots.windows(2).any(|pair| pair[0] >= pair[1])
            || multiplicities.iter().any(|&count| count > degree + 1)
            || multiplicities[1..multiplicities.len() - 1].iter().any(|&count| count > degree)
        {
            return Err(self.invalid_geometry(node, field, "invalid distinct knots or multiplicities"));
        }
        let expected_expanded = u64::from(control_count) + u64::from(degree) + 1;
        let actual_expanded = multiplicities.iter().map(|value| u64::from(*value)).sum::<u64>();
        if actual_expanded != expected_expanded {
            return Err(self.invalid_geometry(
                node,
                field,
                format!("expanded knot count {actual_expanded} does not equal vertices + degree + 1 ({expected_expanded})"),
            ));
        }
        // Only [t_degree, t_control_count] is fully defined. Find these knots
        // without allocating the expanded vector, including imaginary knots.
        let knot_at = |index: u64| {
            let mut end = 0_u64;
            knots.iter().zip(multiplicities).find_map(|(&knot, &count)| {
                end += u64::from(count);
                (index < end).then_some(knot)
            })
        };
        if knot_at(u64::from(degree)) >= knot_at(u64::from(control_count)) {
            return Err(self.invalid_geometry(node, field, "NURBS active parameter range must have positive length"));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn validate_topology(
        &self,
        bodies: &[Body],
        regions: &[Region],
        shells: &[Shell],
        faces: &[Face],
        loops: &[Loop],
        half_edges: &[HalfEdge],
        edges: &[Edge],
        vertices: &[Vertex],
    ) -> Result<TopologyValidation, ParseError> {
        for region in regions {
            let Some(body) = bodies.iter().find(|body| body.id == region.body) else {
                return Err(self.invalid_topology_source(&region.source, "region_body", "region owner does not exist"));
            };
            if !body.regions.contains(&region.id) {
                return Err(self.invalid_topology_source(
                    &region.source,
                    "region_body_inverse",
                    "region owner does not contain the region in its region chain",
                ));
            }
        }
        for shell in shells {
            let Some(region) = regions.iter().find(|region| region.id == shell.region) else {
                return Err(self.invalid_topology_source(&shell.source, "shell_region", "shell region does not exist"));
            };
            if !region.shells.contains(&shell.id) {
                return Err(self.invalid_topology_source(
                    &shell.source,
                    "shell_region_inverse",
                    "shell region does not contain the shell in its shell chain",
                ));
            }
        }
        for face in faces {
            let Some(back_shell) = shells.iter().find(|shell| shell.id == face.back_shell) else {
                return Err(self.invalid_topology_source(&face.source, "face_shells", "face references an unmapped back shell"));
            };
            let Some(front_shell) = shells.iter().find(|shell| shell.id == face.front_shell) else {
                return Err(self.invalid_topology_source(&face.source, "face_shells", "face references an unmapped front shell"));
            };
            if !back_shell.back_faces.contains(&face.id) || !front_shell.front_faces.contains(&face.id) {
                return Err(self.invalid_topology_source(
                    &face.source,
                    "face_shell_inverse",
                    "face shell owners do not contain the face in the matching face chains",
                ));
            }
            for loop_id in &face.loops {
                let Some(loop_value) = loops.get(usize::try_from(*loop_id).unwrap_or(usize::MAX)) else {
                    return Err(self.invalid_topology_source(&face.source, "face_loops", "face references an unmapped loop"));
                };
                if loop_value.face != face.id {
                    return Err(self.invalid_topology_source(
                        &loop_value.source,
                        "loop_face_inverse",
                        "loop owner does not match the face loop chain",
                    ));
                }
            }
        }
        for loop_value in loops {
            let count = loop_value.half_edges.len();
            for (position, half_edge_id) in loop_value.half_edges.iter().enumerate() {
                let half_edge = self.entity(half_edges, *half_edge_id, &loop_value.source, "loop_half_edges")?;
                if half_edge.loop_id != Some(loop_value.id) {
                    return Err(self.invalid_topology_source(
                        &half_edge.source,
                        "halfedge_loop_inverse",
                        "half-edge loop does not match its containing ring",
                    ));
                }
                let expected_forward = loop_value.half_edges[(position + 1) % count];
                let expected_backward = loop_value.half_edges[(position + count - 1) % count];
                if half_edge.forward != Some(expected_forward) || half_edge.backward != Some(expected_backward) {
                    return Err(self.invalid_topology_source(
                        &half_edge.source,
                        "loop_ring_inverse",
                        "half-edge forward/backward links are not inverse ring links",
                    ));
                }
            }
        }
        for edge in edges {
            if matches!(self.roles, RoleAccess::OnshapeSch30000)
                && !bodies.iter().any(|body| body.source.node_index == edge.owner.node_index && body.edges.contains(&edge.id))
                && !shells.iter().any(|shell| shell.source.node_index == edge.owner.node_index && shell.wire_edges.contains(&edge.id))
            {
                return Err(self.invalid_topology_source(&edge.source, "edge_owner_inverse", "edge owner does not contain the edge in its chain"));
            }
            let count = edge.half_edges.len();
            for (position, half_edge_id) in edge.half_edges.iter().enumerate() {
                let half_edge = self.entity(half_edges, *half_edge_id, &edge.source, "edge_half_edges")?;
                if half_edge.edge != Some(edge.id) {
                    return Err(self.invalid_topology_source(
                        &half_edge.source,
                        "halfedge_edge_inverse",
                        "half-edge owner does not match its containing edge ring",
                    ));
                }
                let expected_other = edge.half_edges[(position + 1) % count];
                if half_edge.other != Some(expected_other) {
                    return Err(self.invalid_topology_source(
                        &half_edge.source,
                        "edge_ring_inverse",
                        "half-edge other links do not form the mapped edge ring",
                    ));
                }
            }
        }
        for vertex in vertices {
            if matches!(self.roles, RoleAccess::OnshapeSch30000)
                && !bodies.iter().any(|body| body.source.node_index == vertex.owner.node_index && body.vertices.contains(&vertex.id))
                && !shells.iter().any(|shell| shell.source.node_index == vertex.owner.node_index && shell.isolated_vertex == Some(vertex.id))
            {
                return Err(self.invalid_topology_source(&vertex.source, "vertex_owner_inverse", "vertex owner does not contain the vertex"));
            }
            if usize::try_from(vertex.point).map_or(true, |id| id >= self.ids.points.len()) {
                return Err(self.invalid_topology_source(&vertex.source, "vertex_point", "vertex point geometry does not exist"));
            }
        }
        for body in bodies {
            let body_regions = body.regions.iter().filter_map(|id| regions.get(usize::try_from(*id).ok()?)).collect::<Vec<_>>();
            let body_shell_ids = body_regions.iter().flat_map(|region| region.shells.iter().copied()).collect::<BTreeSet<_>>();
            let body_face_ids = body_shell_ids
                .iter()
                .filter_map(|id| shells.get(usize::try_from(*id).ok()?))
                .flat_map(|shell| shell.back_faces.iter().chain(&shell.front_faces).copied())
                .collect::<BTreeSet<_>>();
            match body.kind {
                BodyKind::Solid => {
                    if body_regions.len() < 2 || !body_regions.iter().any(|region| region.kind == RegionKind::Solid) {
                        return Err(self.invalid_topology_source(
                            &body.source,
                            "solid_regions",
                            "solid body requires at least two regions including one solid region",
                        ));
                    }
                    if body_face_ids.is_empty() {
                        return Err(self.invalid_topology_source(&body.source, "solid_faces", "solid body requires at least one face"));
                    }
                    for edge_id in &body.edges {
                        let edge = self.entity(edges, *edge_id, &body.source, "body_edges")?;
                        let visible = edge
                            .half_edges
                            .iter()
                            .filter_map(|id| half_edges.get(usize::try_from(*id).ok()?))
                            .filter(|half_edge| !half_edge.dummy)
                            .collect::<Vec<_>>();
                        if visible.len() != 2
                            || !matches!(
                                (visible[0].sense, visible[1].sense),
                                (Sense::Positive, Sense::Negative) | (Sense::Negative, Sense::Positive)
                            )
                        {
                            return Err(self.invalid_topology_source(
                                &edge.source,
                                "solid_edge_fins",
                                "solid edge requires exactly two non-dummy fins with opposite senses",
                            ));
                        }
                    }
                }
                BodyKind::Sheet if body_face_ids.is_empty() => {
                    return Err(self.invalid_topology_source(&body.source, "sheet_faces", "sheet body requires at least one face"));
                }
                BodyKind::Wire if body_regions.len() != 1 || body_regions[0].kind != RegionKind::Void => {
                    return Err(self.invalid_topology_source(&body.source, "wire_regions", "wire body requires one void region"));
                }
                BodyKind::Wire | BodyKind::Sheet | BodyKind::General => {}
            }
        }
        let euler_characteristic = i64::try_from(vertices.len()).unwrap_or(i64::MAX) - i64::try_from(edges.len()).unwrap_or(i64::MAX)
            + i64::try_from(faces.len()).unwrap_or(i64::MAX);
        Ok(TopologyValidation { valid: true, closed_loop_count: loops.len(), closed_edge_ring_count: edges.len(), euler_characteristic })
    }

    fn entity<'b, T>(&self, values: &'b [T], id: BrepId, source: &SourceNodeRef, relationship: &'static str) -> Result<&'b T, ParseError> {
        values
            .get(usize::try_from(id).unwrap_or(usize::MAX))
            .ok_or_else(|| self.invalid_topology_source(source, relationship, "document-local B-Rep identifier is out of range"))
    }

    fn node(&self, index: u32) -> Result<&'a RawNode, ParseError> {
        self.nodes.get(&index).copied().ok_or_else(|| {
            ParseError::new(
                ErrorKind::InvalidBrepReference,
                0,
                format!("raw node index {index} is unresolved"),
                ErrorDetails::NodeIndex { node_index: index },
            )
        })
    }

    fn field<'b>(&self, node: &'b RawNode, name: &'static str) -> Result<&'b RawField, ParseError> {
        self.roles.field(node, name).ok_or_else(|| self.invalid_field(node, name, "required effective field is absent"))
    }

    fn value<'b>(&self, node: &'b RawNode, name: &'static str) -> Result<&'b FieldValue, ParseError> {
        let field = self.field(node, name)?;
        if field.values.len() != 1 {
            return Err(self.invalid_field(node, name, format!("required scalar has {} values", field.values.len())));
        }
        Ok(&field.values[0])
    }

    fn pointer(&self, node: &RawNode, name: &'static str) -> Result<u32, ParseError> {
        match self.value(node, name)? {
            FieldValue::PointerIndex(value) => Ok(*value),
            _ => Err(self.invalid_field(node, name, "required field is not a pointer")),
        }
    }

    fn required_pointer(&self, node: &RawNode, name: &'static str) -> Result<u32, ParseError> {
        let target = self.pointer(node, name)?;
        if target == 0 {
            return Err(self.invalid_reference(node, name, target, "non-null raw node", "required B-Rep pointer is null"));
        }
        Ok(target)
    }

    fn pointer_array(&self, node: &RawNode, name: &'static str) -> Result<Vec<u32>, ParseError> {
        self.field(node, name)?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::PointerIndex(index) => Ok(*index),
                _ => Err(self.invalid_field(node, name, "array contains a non-pointer value")),
            })
            .collect()
    }

    fn id(&self, ids: &BTreeMap<u32, BrepId>, node: &RawNode, expected: &'static str) -> Result<BrepId, ParseError> {
        ids.get(&node.index)
            .copied()
            .ok_or_else(|| self.invalid_reference(node, "self", node.index, expected, "node was not assigned a document-local B-Rep identifier"))
    }

    fn id_from_target(
        &self,
        node: &RawNode,
        field: &'static str,
        target: u32,
        expected: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<BrepId, ParseError> {
        if target == 0 {
            return Err(self.invalid_reference(node, field, target, expected, "required B-Rep pointer is null"));
        }
        let target_node = self
            .nodes
            .get(&target)
            .copied()
            .ok_or_else(|| self.invalid_reference(node, field, target, expected, "required B-Rep pointer is unresolved"))?;
        ids.get(&target).copied().ok_or_else(|| {
            self.invalid_reference(node, field, target, expected, format!("pointer targets {} rather than {expected}", target_node.definition.name))
        })
    }

    fn required_id(&self, node: &RawNode, field: &'static str, expected: &'static str, ids: &BTreeMap<u32, BrepId>) -> Result<BrepId, ParseError> {
        self.id_from_target(node, field, self.pointer(node, field)?, expected, ids)
    }

    fn optional_id(
        &self,
        node: &RawNode,
        field: &'static str,
        expected: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<Option<BrepId>, ParseError> {
        let target = self.pointer(node, field)?;
        if target == 0 {
            return Ok(None);
        }
        self.id_from_target(node, field, target, expected, ids).map(Some)
    }

    fn required_id_pair(
        &self,
        node: &RawNode,
        field: &'static str,
        expected: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<[BrepId; 2], ParseError> {
        let targets = self.pointer_array(node, field)?;
        if targets.len() != 2 {
            return Err(self.invalid_field(node, field, "required pointer pair must contain exactly two values"));
        }
        Ok([self.id_from_target(node, field, targets[0], expected, ids)?, self.id_from_target(node, field, targets[1], expected, ids)?])
    }

    fn optional_id_pair(
        &self,
        node: &RawNode,
        field: &'static str,
        expected: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<[Option<BrepId>; 2], ParseError> {
        let targets = self.pointer_array(node, field)?;
        if targets.len() != 2 {
            return Err(self.invalid_field(node, field, "optional pointer pair must contain exactly two values"));
        }
        let convert = |target| {
            if target == 0 { Ok(None) } else { self.id_from_target(node, field, target, expected, ids).map(Some) }
        };
        Ok([convert(targets[0])?, convert(targets[1])?])
    }

    fn required_typed_node(&self, node: &RawNode, field: &'static str, expected: &'static str) -> Result<&'a RawNode, ParseError> {
        let target = self.required_pointer(node, field)?;
        let target_node = self
            .nodes
            .get(&target)
            .copied()
            .ok_or_else(|| self.invalid_reference(node, field, target, expected, "required geometry auxiliary pointer is unresolved"))?;
        if self.roles.type_name(target_node) != expected {
            return Err(self.invalid_reference(node, field, target, expected, format!("pointer targets {}", target_node.definition.name)));
        }
        Ok(target_node)
    }

    fn required_source(&self, node: &RawNode, field: &'static str) -> Result<SourceNodeRef, ParseError> {
        let target = self.required_pointer(node, field)?;
        self.nodes
            .get(&target)
            .copied()
            .map(|item| self.source(item))
            .ok_or_else(|| self.invalid_reference(node, field, target, "raw node", "required source pointer is unresolved"))
    }

    fn optional_source(&self, node: &RawNode, field: &'static str) -> Result<Option<SourceNodeRef>, ParseError> {
        let target = self.pointer(node, field)?;
        if target == 0 {
            return Ok(None);
        }
        self.nodes
            .get(&target)
            .copied()
            .map(|item| Some(self.source(item)))
            .ok_or_else(|| self.invalid_reference(node, field, target, "raw node", "optional source pointer is non-null but unresolved"))
    }

    fn optional_source_if_field(&self, node: &RawNode, field: &'static str) -> Result<Option<SourceNodeRef>, ParseError> {
        if self.roles.field(node, field).is_none() {
            return Ok(None);
        }
        self.optional_source(node, field)
    }

    fn null_chain(
        &self,
        owner: &RawNode,
        first_field: &'static str,
        expected: &'static str,
        next_field: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<Vec<BrepId>, ParseError> {
        let mut current = self.pointer(owner, first_field)?;
        let mut seen = BTreeSet::new();
        let mut output = Vec::new();
        while current != 0 {
            if !seen.insert(current) {
                return Err(self.invalid_topology(owner, first_field, format!("{expected} chain contains a cycle at raw index {current}")));
            }
            let node = self
                .nodes
                .get(&current)
                .copied()
                .ok_or_else(|| self.invalid_reference(owner, first_field, current, expected, "chain pointer is unresolved"))?;
            let Some(id) = ids.get(&current).copied() else {
                return Err(self.invalid_reference(owner, first_field, current, expected, format!("chain targets {}", node.definition.name)));
            };
            output.push(id);
            current = self.pointer(node, next_field)?;
        }
        Ok(output)
    }

    fn required_ring(
        &self,
        owner: &RawNode,
        first_field: &'static str,
        expected: &'static str,
        next_field: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<Vec<BrepId>, ParseError> {
        let first = self.required_pointer(owner, first_field)?;
        let mut current = first;
        let mut seen = BTreeSet::new();
        let mut output = Vec::new();
        loop {
            if !seen.insert(current) {
                if current == first {
                    break;
                }
                return Err(self.invalid_topology(owner, first_field, format!("{expected} ring repeats raw index {current} before closing")));
            }
            let node = self
                .nodes
                .get(&current)
                .copied()
                .ok_or_else(|| self.invalid_reference(owner, first_field, current, expected, "ring pointer is unresolved"))?;
            let Some(id) = ids.get(&current).copied() else {
                return Err(self.invalid_reference(owner, first_field, current, expected, format!("ring targets {}", node.definition.name)));
            };
            output.push(id);
            current = self.pointer(node, next_field)?;
            if current == 0 {
                return Err(self.invalid_topology(node, next_field, format!("{expected} ring terminated at null rather than closing")));
            }
        }
        Ok(output)
    }

    fn byte(&self, node: &RawNode, field: &'static str) -> Result<u8, ParseError> {
        match self.value(node, field)? {
            FieldValue::UnsignedByte(value) => Ok(*value),
            _ => Err(self.invalid_field(node, field, "required field is not an unsigned byte")),
        }
    }

    fn character(&self, node: &RawNode, field: &'static str) -> Result<u8, ParseError> {
        match self.value(node, field)? {
            FieldValue::Character(value) => Ok(*value),
            _ => Err(self.invalid_field(node, field, "required field is not a character")),
        }
    }

    fn sense(&self, node: &RawNode, field: &'static str) -> Result<Sense, ParseError> {
        match self.character(node, field)? {
            b'+' => Ok(Sense::Positive),
            b'-' => Ok(Sense::Negative),
            b'?' => Ok(Sense::Unknown),
            value => Err(self.invalid_field(node, field, format!("sense must be '+', '-', or '?', observed 0x{value:02x}"))),
        }
    }

    fn logical(&self, node: &RawNode, field: &'static str) -> Result<bool, ParseError> {
        match self.value(node, field)? {
            FieldValue::Logical(value) => Ok(*value),
            _ => Err(self.invalid_field(node, field, "required field is not logical")),
        }
    }

    fn double(&self, node: &RawNode, field: &'static str) -> Result<f64, ParseError> {
        match self.value(node, field)? {
            FieldValue::Double(Some(value)) if value.is_finite() => Ok(*value),
            _ => Err(self.invalid_geometry(node, field, "required double is null or non-finite")),
        }
    }

    fn optional_double(&self, node: &RawNode, field: &'static str) -> Result<Option<f64>, ParseError> {
        match self.value(node, field)? {
            FieldValue::Double(None) => Ok(None),
            FieldValue::Double(Some(value)) if value.is_finite() => Ok(Some(*value)),
            _ => Err(self.invalid_geometry(node, field, "optional double is not finite")),
        }
    }

    fn positive_double(&self, node: &RawNode, field: &'static str) -> Result<f64, ParseError> {
        let value = self.double(node, field)?;
        if value <= 0.0 {
            return Err(self.invalid_geometry(node, field, "value must be positive"));
        }
        Ok(value)
    }

    fn nonnegative_double(&self, node: &RawNode, field: &'static str) -> Result<f64, ParseError> {
        let value = self.double(node, field)?;
        if value < 0.0 {
            return Err(self.invalid_geometry(node, field, "value must be non-negative"));
        }
        Ok(value)
    }

    fn vector(&self, node: &RawNode, field: &'static str) -> Result<Vector3, ParseError> {
        let (FieldValue::Vector(values) | FieldValue::IntersectionPoint(values)) = self.value(node, field)? else {
            return Err(self.invalid_field(node, field, "required field is not a vector"));
        };
        let [Some(x), Some(y), Some(z)] = values else {
            return Err(self.invalid_geometry(node, field, "required vector is null"));
        };
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(self.invalid_geometry(node, field, "vector contains a non-finite component"));
        }
        Ok(Vector3 { x: *x, y: *y, z: *z })
    }

    fn nonnegative_integer(&self, node: &RawNode, field: &'static str) -> Result<u32, ParseError> {
        match self.value(node, field)? {
            FieldValue::Integer(Some(value)) if *value >= 0 => Ok(u32::try_from(*value).unwrap_or(u32::MAX)),
            _ => Err(self.invalid_geometry(node, field, "required integer is null or negative")),
        }
    }

    fn positive_short(&self, node: &RawNode, field: &'static str) -> Result<u16, ParseError> {
        match self.value(node, field)? {
            FieldValue::ShortInteger(Some(value)) if *value > 0 => Ok(u16::try_from(*value).unwrap_or(u16::MAX)),
            _ => Err(self.invalid_geometry(node, field, "required short integer is null or non-positive")),
        }
    }

    fn nonnegative_short(&self, node: &RawNode, field: &'static str) -> Result<u16, ParseError> {
        match self.value(node, field)? {
            FieldValue::ShortInteger(Some(value)) if *value >= 0 => Ok(u16::try_from(*value).unwrap_or(u16::MAX)),
            _ => Err(self.invalid_geometry(node, field, "required short integer is null or negative")),
        }
    }

    fn double_pair(&self, node: &RawNode, field: &'static str) -> Result<[f64; 2], ParseError> {
        let values = self.double_array(node, field)?;
        self.validate_count(node, field, 2, values.len())?;
        Ok([values[0], values[1]])
    }

    fn double_array(&self, node: &RawNode, field: &'static str) -> Result<Vec<f64>, ParseError> {
        self.field(node, field)?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::Double(Some(value)) if value.is_finite() => Ok(*value),
                _ => Err(self.invalid_geometry(node, field, "double array contains null or non-finite data")),
            })
            .collect()
    }

    fn double_array_prefix(&self, node: &RawNode, field: &'static str, count: usize) -> Result<Vec<f64>, ParseError> {
        let values = &self.field(node, field)?.values;
        if values.len() < count {
            return Err(ParseError::new(
                ErrorKind::InvalidGeometryParameter,
                node.byte_range.start,
                format!("{field} has {} values; expected at least {count}", values.len()),
                ErrorDetails::CountMismatch { field, expected: count, actual: values.len() },
            ));
        }
        let output = values[..count]
            .iter()
            .map(|value| match value {
                FieldValue::Double(Some(value)) if value.is_finite() => Ok(*value),
                _ => Err(self.invalid_geometry(node, field, "meaningful knot prefix contains null or non-finite data")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if values[count..].iter().any(|value| !matches!(value, FieldValue::Double(None))) {
            return Err(self.invalid_geometry(node, field, "values after the declared knot count are not null padding"));
        }
        Ok(output)
    }

    fn positive_short_array_prefix(&self, node: &RawNode, field: &'static str, count: usize) -> Result<Vec<u16>, ParseError> {
        let values = &self.field(node, field)?.values;
        if values.len() < count {
            return Err(ParseError::new(
                ErrorKind::InvalidGeometryParameter,
                node.byte_range.start,
                format!("{field} has {} values; expected at least {count}", values.len()),
                ErrorDetails::CountMismatch { field, expected: count, actual: values.len() },
            ));
        }
        let output = values[..count]
            .iter()
            .map(|value| match value {
                FieldValue::ShortInteger(Some(value)) if *value > 0 => Ok(u16::try_from(*value).unwrap_or(u16::MAX)),
                _ => Err(self.invalid_geometry(node, field, "multiplicity array contains null or non-positive data")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if values[count..].iter().any(|value| !matches!(value, FieldValue::ShortInteger(Some(0)))) {
            return Err(self.invalid_geometry(node, field, "values after the declared multiplicity count are not zero padding"));
        }
        Ok(output)
    }

    #[allow(clippy::unused_self)]
    fn validate_count(&self, node: &RawNode, field: &'static str, expected: usize, actual: usize) -> Result<(), ParseError> {
        if expected != actual {
            return Err(ParseError::new(
                ErrorKind::InvalidGeometryParameter,
                node.byte_range.start,
                format!("{field} has {actual} values; expected {expected}"),
                ErrorDetails::CountMismatch { field, expected, actual },
            ));
        }
        Ok(())
    }

    fn source(&self, node: &RawNode) -> SourceNodeRef {
        let node_id = self.roles.field(node, "node_id").and_then(|field| field.values.first()).and_then(|value| match value {
            FieldValue::Integer(value) => *value,
            _ => None,
        });
        SourceNodeRef {
            node_index: node.index,
            node_type: node.node_type,
            type_name: node.definition.name.clone(),
            node_id,
            byte_range: node.byte_range.clone(),
        }
    }

    #[allow(clippy::unused_self)]
    fn invalid_field(&self, node: &RawNode, field: &'static str, message: impl Into<String>) -> ParseError {
        ParseError::new(ErrorKind::InvalidBrepField, node.byte_range.start, message, ErrorDetails::BrepField { node_index: node.index, field })
    }

    #[allow(clippy::unused_self)]
    fn invalid_reference(
        &self,
        node: &RawNode,
        field: &'static str,
        target_index: u32,
        expected_type: &'static str,
        message: impl Into<String>,
    ) -> ParseError {
        ParseError::new(
            ErrorKind::InvalidBrepReference,
            node.byte_range.start,
            message,
            ErrorDetails::BrepReference { node_index: node.index, field, target_index, expected_type },
        )
    }

    fn invalid_topology(&self, node: &RawNode, relationship: &'static str, message: impl Into<String>) -> ParseError {
        self.invalid_topology_source(&self.source(node), relationship, message)
    }

    #[allow(clippy::unused_self)]
    fn invalid_topology_source(&self, source: &SourceNodeRef, relationship: &'static str, message: impl Into<String>) -> ParseError {
        ParseError::new(
            ErrorKind::InvalidBrepTopology,
            source.byte_range.start,
            message,
            ErrorDetails::BrepInvariant { node_index: source.node_index, relationship },
        )
    }

    #[allow(clippy::unused_self)]
    fn invalid_geometry(&self, node: &RawNode, field: &'static str, message: impl Into<String>) -> ParseError {
        ParseError::new(
            ErrorKind::InvalidGeometryParameter,
            node.byte_range.start,
            message,
            ErrorDetails::BrepField { node_index: node.index, field },
        )
    }
}

fn push_diagnostic(
    diagnostics: &mut Vec<BrepDiagnostic>,
    max_diagnostics: usize,
    offset: usize,
    diagnostic: BrepDiagnostic,
) -> Result<(), ParseError> {
    let actual = diagnostics.len().saturating_add(1);
    if actual > max_diagnostics {
        return Err(ParseError::limit(offset, "diagnostics", actual, max_diagnostics));
    }
    diagnostics.push(diagnostic);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{FieldDefinition, FieldType, RawField, SchemaSource, TypeDefinition};

    use super::*;

    fn field(name: &str, pointer_class: u16, values: Vec<FieldValue>) -> (FieldDefinition, Vec<FieldValue>) {
        let field_type = values.first().map_or(FieldType::Double, FieldValue::field_type);
        (FieldDefinition { name: name.to_owned(), field_type, pointer_class, element_count: u32::from(values.len() > 1), transmitted: true }, values)
    }

    fn node(index: u32, node_type: u16, name: &str, fields: Vec<(FieldDefinition, Vec<FieldValue>)>) -> RawNode {
        let definitions = fields.iter().map(|(definition, _)| definition.clone()).collect::<Vec<_>>();
        let definition = TypeDefinition::from_fields(node_type, name, format!("Synthetic {name}"), definitions, SchemaSource::Base);
        let variable_length = definition.variable.then(|| fields.last().map_or(0, |(_, values)| u32::try_from(values.len()).unwrap_or(u32::MAX)));
        RawNode {
            node_type,
            index,
            user_fields: Vec::new(),
            variable_length,
            definition,
            first_schema: None,
            fields: fields
                .into_iter()
                .map(|(definition, values)| RawField {
                    definition,
                    values,
                    byte_range: usize::try_from(index).unwrap_or(usize::MAX)..usize::try_from(index).unwrap_or(usize::MAX).saturating_add(1),
                })
                .collect(),
            byte_range: usize::try_from(index).unwrap_or(usize::MAX)..usize::try_from(index).unwrap_or(usize::MAX).saturating_add(1),
        }
    }

    fn common_curve_fields(owner: u32) -> Vec<(FieldDefinition, Vec<FieldValue>)> {
        vec![
            field("owner", 1010, vec![FieldValue::PointerIndex(owner)]),
            field("next", 1008, vec![FieldValue::PointerIndex(0)]),
            field("previous", 1008, vec![FieldValue::PointerIndex(0)]),
            field("geometric_owner", 141, vec![FieldValue::PointerIndex(0)]),
            field("sense", 0, vec![FieldValue::Character(b'+')]),
        ]
    }

    fn common_surface_fields(owner: u32) -> Vec<(FieldDefinition, Vec<FieldValue>)> {
        vec![
            field("owner", 1007, vec![FieldValue::PointerIndex(owner)]),
            field("next", 1006, vec![FieldValue::PointerIndex(0)]),
            field("previous", 1006, vec![FieldValue::PointerIndex(0)]),
            field("geometric_owner", 141, vec![FieldValue::PointerIndex(0)]),
            field("sense", 0, vec![FieldValue::Character(b'+')]),
        ]
    }

    fn wire_acorn_nodes() -> Vec<RawNode> {
        vec![
            node(
                1,
                12,
                "BODY",
                vec![
                    field("body_type", 0, vec![FieldValue::UnsignedByte(2)]),
                    field("res_size", 0, vec![FieldValue::Double(Some(1.0e-6))]),
                    field("res_linear", 0, vec![FieldValue::Double(Some(1.0e-8))]),
                    field("region", 13, vec![FieldValue::PointerIndex(2)]),
                    field("edge", 17, vec![FieldValue::PointerIndex(0)]),
                    field("vertex", 26, vec![FieldValue::PointerIndex(4)]),
                ],
            ),
            node(
                2,
                13,
                "REGION",
                vec![
                    field("type", 0, vec![FieldValue::Character(b'V')]),
                    field("body", 12, vec![FieldValue::PointerIndex(1)]),
                    field("shell", 14, vec![FieldValue::PointerIndex(3)]),
                    field("next", 13, vec![FieldValue::PointerIndex(0)]),
                ],
            ),
            node(
                3,
                14,
                "SHELL",
                vec![
                    field("region", 13, vec![FieldValue::PointerIndex(2)]),
                    field("face", 15, vec![FieldValue::PointerIndex(0)]),
                    field("front_face", 15, vec![FieldValue::PointerIndex(0)]),
                    field("edge", 17, vec![FieldValue::PointerIndex(0)]),
                    field("vertex", 26, vec![FieldValue::PointerIndex(4)]),
                    field("next", 14, vec![FieldValue::PointerIndex(0)]),
                ],
            ),
            node(
                4,
                26,
                "VERTEX",
                vec![
                    field("point", 88, vec![FieldValue::PointerIndex(5)]),
                    field("tolerance", 0, vec![FieldValue::Double(None)]),
                    field("owner", 1003, vec![FieldValue::PointerIndex(3)]),
                    field("next", 26, vec![FieldValue::PointerIndex(0)]),
                ],
            ),
            node(
                5,
                88,
                "POINT",
                vec![
                    field("pvec", 0, vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])]),
                    field("owner", 1003, vec![FieldValue::PointerIndex(4)]),
                ],
            ),
        ]
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn maps_wire_acorn_analytic_line_and_exact_nurbs_curve() {
        let mut nodes = wire_acorn_nodes();
        let mut line_fields = common_curve_fields(4);
        line_fields.extend([
            field("pvec", 0, vec![FieldValue::Vector([Some(0.0), Some(0.0), Some(0.0)])]),
            field("direction", 0, vec![FieldValue::Vector([Some(1.0), Some(0.0), Some(0.0)])]),
        ]);
        nodes.push(node(6, 30, "LINE", line_fields));

        let mut b_curve_fields = common_curve_fields(4);
        b_curve_fields.push(field("nurbs", 136, vec![FieldValue::PointerIndex(8)]));
        nodes.push(node(7, 134, "B_CURVE", b_curve_fields));
        nodes.extend([
            node(
                8,
                136,
                "NURBS_CURVE",
                vec![
                    field("degree", 0, vec![FieldValue::ShortInteger(Some(2))]),
                    field("n_vertices", 0, vec![FieldValue::Integer(Some(3))]),
                    field("vertex_dim", 0, vec![FieldValue::ShortInteger(Some(3))]),
                    field("n_knots", 0, vec![FieldValue::Integer(Some(2))]),
                    field("knot_type", 0, vec![FieldValue::UnsignedByte(1)]),
                    field("periodic", 0, vec![FieldValue::Logical(false)]),
                    field("closed", 0, vec![FieldValue::Logical(false)]),
                    field("rational", 0, vec![FieldValue::Logical(false)]),
                    field("curve_form", 0, vec![FieldValue::UnsignedByte(0)]),
                    field("bspline_vertices", 45, vec![FieldValue::PointerIndex(9)]),
                    field("knot_mult", 127, vec![FieldValue::PointerIndex(10)]),
                    field("knots", 128, vec![FieldValue::PointerIndex(11)]),
                ],
            ),
            node(
                9,
                45,
                "BSPLINE_VERTICES",
                vec![field(
                    "vertices",
                    0,
                    [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 2.0, 0.0, 0.0].into_iter().map(|value| FieldValue::Double(Some(value))).collect(),
                )],
            ),
            node(
                10,
                127,
                "KNOT_MULT",
                vec![field(
                    "mult",
                    0,
                    vec![
                        FieldValue::ShortInteger(Some(3)),
                        FieldValue::ShortInteger(Some(3)),
                        FieldValue::ShortInteger(Some(0)),
                        FieldValue::ShortInteger(Some(0)),
                    ],
                )],
            ),
            node(
                11,
                128,
                "KNOT_SET",
                vec![field(
                    "knots",
                    0,
                    vec![FieldValue::Double(Some(0.0)), FieldValue::Double(Some(1.0)), FieldValue::Double(None), FieldValue::Double(None)],
                )],
            ),
        ]);

        let model = Mapper::new(BrepSourceFormat::Binary, "SCH_TEST", &nodes, RoleAccess::Named).map();
        assert!(model.is_ok());
        if let Ok(model) = model {
            assert!(model.complete);
            assert_eq!(model.bodies[0].kind, BodyKind::Wire);
            assert_eq!(model.vertices.len(), 1);
            assert_eq!(model.metrics.bounding_box.map(|bounds| bounds.minimum), Some(Vector3 { x: 1.0, y: 2.0, z: 3.0 }));
            assert!(matches!(model.curves[0].kind, CurveKind::Line { .. }));
            assert!(matches!(model.curves[1].kind, CurveKind::Nurbs(_)));
            if let CurveKind::Nurbs(nurbs) = &model.curves[1].kind {
                assert_eq!(nurbs.degree, 2);
                assert_eq!(nurbs.control_vertices.len(), 3);
                assert_eq!(nurbs.knot_multiplicities, [3, 3]);
                assert_eq!(nurbs.knots, [0.0, 1.0]);
                assert_eq!(nurbs.sources.len(), 4);
            }
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn maps_i7_exact_analytic_trim_nurbs_and_offset_geometry() {
        fn vector(name: &str, values: [f64; 3]) -> (FieldDefinition, Vec<FieldValue>) {
            field(name, 0, vec![FieldValue::Vector(values.map(Some))])
        }

        fn double(name: &str, value: f64) -> (FieldDefinition, Vec<FieldValue>) {
            field(name, 0, vec![FieldValue::Double(Some(value))])
        }

        let mut nodes = Vec::new();

        let mut line = common_curve_fields(0);
        line.extend([vector("pvec", [0.0, 0.0, 0.0]), vector("direction", [1.0, 0.0, 0.0])]);
        nodes.push(node(1, 30, "LINE", line));

        let mut circle = common_curve_fields(0);
        circle.extend([
            vector("centre", [0.0, 0.0, 0.0]),
            vector("normal", [0.0, 0.0, 1.0]),
            vector("x_axis", [1.0, 0.0, 0.0]),
            double("radius", 5.0),
        ]);
        nodes.push(node(2, 31, "CIRCLE", circle));

        let mut ellipse = common_curve_fields(0);
        ellipse.extend([
            vector("centre", [0.0, 0.0, 0.0]),
            vector("normal", [0.0, 0.0, 1.0]),
            vector("x_axis", [1.0, 0.0, 0.0]),
            double("major_radius", 8.0),
            double("minor_radius", 4.0),
        ]);
        nodes.push(node(3, 32, "ELLIPSE", ellipse));

        let mut parabola = common_curve_fields(0);
        parabola.extend([
            vector("origin", [0.0, 0.0, 0.0]),
            vector("normal", [0.0, 0.0, 1.0]),
            vector("x_axis", [1.0, 0.0, 0.0]),
            double("focal_length", 2.0),
        ]);
        nodes.push(node(4, 33, "PARABOLA", parabola));

        let mut hyperbola = common_curve_fields(0);
        hyperbola.extend([
            vector("origin", [0.0, 0.0, 0.0]),
            vector("normal", [0.0, 0.0, 1.0]),
            vector("x_axis", [1.0, 0.0, 0.0]),
            double("transverse_radius", 3.0),
            double("conjugate_radius", 2.0),
        ]);
        nodes.push(node(5, 34, "HYPERBOLA", hyperbola));

        let mut trimmed = common_curve_fields(0);
        trimmed.extend([
            field("basis_curve", 1008, vec![FieldValue::PointerIndex(2)]),
            vector("point_1", [5.0, 0.0, 0.0]),
            vector("point_2", [-5.0, 0.0, 0.0]),
            double("parm_1", 0.0),
            double("parm_2", std::f64::consts::PI),
        ]);
        nodes.push(node(6, 35, "TRIMMED_CURVE", trimmed));

        let mut plane = common_surface_fields(0);
        plane.extend([vector("pvec", [0.0, 0.0, 0.0]), vector("normal", [0.0, 0.0, 1.0]), vector("x_axis", [1.0, 0.0, 0.0])]);
        nodes.push(node(7, 50, "PLANE", plane));

        let mut cylinder = common_surface_fields(0);
        cylinder.extend([vector("pvec", [0.0, 0.0, 0.0]), vector("axis", [0.0, 0.0, 1.0]), double("radius", 5.0), vector("x_axis", [1.0, 0.0, 0.0])]);
        nodes.push(node(8, 51, "CYLINDER", cylinder));

        let mut cone = common_surface_fields(0);
        cone.extend([
            vector("pvec", [0.0, 0.0, 0.0]),
            vector("axis", [0.0, 0.0, 1.0]),
            double("radius", 8.0),
            double("sin_half_angle", -0.316_227_766_016_837_94),
            double("cos_half_angle", 0.948_683_298_050_513_8),
            vector("x_axis", [1.0, 0.0, 0.0]),
        ]);
        nodes.push(node(9, 52, "CONE", cone));

        let mut sphere = common_surface_fields(0);
        sphere.extend([vector("centre", [0.0, 0.0, 0.0]), double("radius", 6.0), vector("axis", [0.0, 0.0, 1.0]), vector("x_axis", [1.0, 0.0, 0.0])]);
        nodes.push(node(10, 53, "SPHERE", sphere));

        let mut torus = common_surface_fields(0);
        torus.extend([
            vector("centre", [0.0, 0.0, 0.0]),
            vector("axis", [0.0, 0.0, 1.0]),
            double("major_radius", 8.0),
            double("minor_radius", 2.0),
            vector("x_axis", [1.0, 0.0, 0.0]),
        ]);
        nodes.push(node(11, 54, "TORUS", torus));

        let mut offset = common_surface_fields(0);
        offset.extend([field("surface", 1006, vec![FieldValue::PointerIndex(7)]), double("offset", 2.0)]);
        nodes.push(node(12, 55, "OFFSET_SURF", offset));

        let mut b_surface = common_surface_fields(0);
        b_surface.push(field("nurbs", 137, vec![FieldValue::PointerIndex(14)]));
        nodes.push(node(13, 135, "B_SURFACE", b_surface));
        nodes.extend([
            node(
                14,
                137,
                "NURBS_SURF",
                vec![
                    field("u_degree", 0, vec![FieldValue::ShortInteger(Some(1))]),
                    field("v_degree", 0, vec![FieldValue::ShortInteger(Some(1))]),
                    field("n_u_vertices", 0, vec![FieldValue::Integer(Some(2))]),
                    field("n_v_vertices", 0, vec![FieldValue::Integer(Some(2))]),
                    field("vertex_dim", 0, vec![FieldValue::ShortInteger(Some(3))]),
                    field("n_u_knots", 0, vec![FieldValue::Integer(Some(2))]),
                    field("n_v_knots", 0, vec![FieldValue::Integer(Some(2))]),
                    field("u_knot_type", 0, vec![FieldValue::UnsignedByte(1)]),
                    field("v_knot_type", 0, vec![FieldValue::UnsignedByte(1)]),
                    field("u_periodic", 0, vec![FieldValue::Logical(false)]),
                    field("v_periodic", 0, vec![FieldValue::Logical(false)]),
                    field("u_closed", 0, vec![FieldValue::Logical(false)]),
                    field("v_closed", 0, vec![FieldValue::Logical(false)]),
                    field("rational", 0, vec![FieldValue::Logical(false)]),
                    field("surface_form", 0, vec![FieldValue::UnsignedByte(0)]),
                    field("bspline_vertices", 45, vec![FieldValue::PointerIndex(15)]),
                    field("u_knot_mult", 127, vec![FieldValue::PointerIndex(16)]),
                    field("v_knot_mult", 127, vec![FieldValue::PointerIndex(17)]),
                    field("u_knots", 128, vec![FieldValue::PointerIndex(18)]),
                    field("v_knots", 128, vec![FieldValue::PointerIndex(19)]),
                ],
            ),
            node(
                15,
                45,
                "BSPLINE_VERTICES",
                vec![field(
                    "vertices",
                    0,
                    [0.0, 0.0, 0.0, 0.0, 10.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0]
                        .into_iter()
                        .map(|value| FieldValue::Double(Some(value)))
                        .collect(),
                )],
            ),
            node(16, 127, "KNOT_MULT", vec![field("mult", 0, vec![FieldValue::ShortInteger(Some(2)), FieldValue::ShortInteger(Some(2))])]),
            node(17, 127, "KNOT_MULT", vec![field("mult", 0, vec![FieldValue::ShortInteger(Some(2)), FieldValue::ShortInteger(Some(2))])]),
            node(18, 128, "KNOT_SET", vec![field("knots", 0, vec![FieldValue::Double(Some(0.0)), FieldValue::Double(Some(1.0))])]),
            node(19, 128, "KNOT_SET", vec![field("knots", 0, vec![FieldValue::Double(Some(0.0)), FieldValue::Double(Some(1.0))])]),
        ]);

        let mut mapper = Mapper::new(BrepSourceFormat::Text, "SCH_TEST", &nodes, RoleAccess::Named);
        let curve_kinds = (1..=6).map(|index| mapper.map_curve(index).map(|curve| curve.kind.as_str())).collect::<Result<Vec<_>, _>>();
        assert_eq!(curve_kinds, Ok(vec!["line", "circle", "ellipse", "parabola", "hyperbola", "trimmed",]));
        let surface_kinds = (7..=13).map(|index| mapper.map_surface(index).map(|surface| surface.kind.as_str())).collect::<Result<Vec<_>, _>>();
        assert_eq!(surface_kinds, Ok(vec!["plane", "cylinder", "cone", "sphere", "torus", "offset", "nurbs",]));
        let nurbs = mapper.map_surface(13);
        assert!(matches!(
            nurbs.map(|surface| surface.kind),
            Ok(SurfaceKind::Nurbs(NurbsSurface {
                u_degree: 1,
                v_degree: 1,
                u_control_vertex_count: 2,
                v_control_vertex_count: 2,
                rational: false,
                ..
            }))
        ));
    }

    #[test]
    fn derives_unset_trim_parameters_only_on_a_line() {
        fn vector(name: &str, values: [f64; 3]) -> (FieldDefinition, Vec<FieldValue>) {
            field(name, 0, vec![FieldValue::Vector(values.map(Some))])
        }
        // R(t) = (1, 2, 3) + t (0, 0.6, 0.8) passes (1, 3.5, 5) at t = 2.5 and
        // (1, 6.5, 9) at t = 7.5.
        let low = [1.0, 3.5, 5.0];
        let high = [1.0, 6.5, 9.0];
        let trim = |sense: u8, direction: [f64; 3], points: [[f64; 3]; 2], parameters: [Option<f64>; 2], on_circle: bool| {
            let mut basis = common_curve_fields(0);
            basis[4] = field("sense", 0, vec![FieldValue::Character(sense)]);
            let basis = if on_circle {
                basis.extend([
                    vector("centre", [1.0, 2.0, 3.0]),
                    vector("normal", [1.0, 0.0, 0.0]),
                    vector("x_axis", [0.0, 0.6, 0.8]),
                    field("radius", 0, vec![FieldValue::Double(Some(2.5))]),
                ]);
                node(1, 31, "CIRCLE", basis)
            } else {
                basis.extend([vector("pvec", [1.0, 2.0, 3.0]), vector("direction", direction)]);
                node(1, 30, "LINE", basis)
            };
            let mut trimmed = common_curve_fields(0);
            trimmed.extend([
                field("basis_curve", 1008, vec![FieldValue::PointerIndex(1)]),
                vector("point_1", points[0]),
                vector("point_2", points[1]),
                field("parm_1", 0, vec![FieldValue::Double(parameters[0])]),
                field("parm_2", 0, vec![FieldValue::Double(parameters[1])]),
            ]);
            let nodes = [basis, node(2, 133, "TRIMMED_CURVE", trimmed)];
            Mapper::new(BrepSourceFormat::Text, "SCH_TEST", &nodes, RoleAccess::Named).map_curve(2).map(|curve| curve.kind)
        };
        let unit = [0.0, 0.6, 0.8];
        for (sense, points, expected) in [(b'+', [low, high], [2.5, 7.5]), (b'-', [high, low], [7.5, 2.5])] {
            let kind = trim(sense, unit, points, [None, None], false);
            assert!(matches!(
                kind,
                Ok(CurveKind::Trimmed {
                    start_parameter,
                    end_parameter,
                    ..
                }) if (start_parameter - expected[0]).abs() < 1.0e-12
                    && (end_parameter - expected[1]).abs() < 1.0e-12
            ));
        }
        // Stored parameters are never replaced.
        assert!(matches!(
            trim(b'+', unit, [low, high], [Some(-4.0), Some(9.0)], false),
            Ok(CurveKind::Trimmed { start_parameter: -4.0, end_parameter: 9.0, .. })
        ));
        let off_line = [1.000_001, 6.5, 9.0];
        for kind in [
            // The derived order must still follow the basis sense.
            trim(b'+', unit, [high, low], [None, None], false),
            trim(b'-', unit, [low, high], [None, None], false),
            trim(b'+', unit, [low, high], [Some(2.5), None], false),
            trim(b'+', unit, [low, high], [None, Some(7.5)], false),
            trim(b'+', unit, [low, high], [None, None], true),
            trim(b'+', unit, [low, off_line], [None, None], false),
            trim(b'+', [0.0, 0.6, 0.9], [low, high], [None, None], false),
        ] {
            assert_eq!(kind.err().map(|error| error.kind()), Some(ErrorKind::InvalidGeometryParameter));
        }
    }

    #[test]
    fn retains_an_unsupported_curve_and_marks_the_model_incomplete() {
        let mut nodes = wire_acorn_nodes();
        nodes.push(node(6, 200, "FUTURE_CURVE", common_curve_fields(4)));

        let model = Mapper::new(BrepSourceFormat::Text, "SCH_TEST", &nodes, RoleAccess::Named).map();
        assert!(model.is_ok());
        if let Ok(model) = model {
            assert!(!model.complete);
            assert_eq!(model.diagnostics.len(), 1);
            assert_eq!(model.diagnostics[0].code, "geometry.unsupported_curve");
            assert!(matches!(model.curves[0].kind, CurveKind::Unsupported { .. }));
            assert_eq!(model.curves[0].source.node_index, 6);
        }
    }

    #[test]
    fn bounds_retained_geometry_diagnostics() {
        let mut nodes = wire_acorn_nodes();
        nodes.push(node(6, 200, "FUTURE_CURVE", common_curve_fields(4)));
        nodes.push(node(7, 201, "ANOTHER_FUTURE_CURVE", common_curve_fields(4)));

        let result = Mapper::new(BrepSourceFormat::Text, "SCH_TEST", &nodes, RoleAccess::Named).with_max_diagnostics(1).map();

        assert!(result.is_err());
        if let Err(error) = result {
            assert_eq!(error.kind(), ErrorKind::LimitExceeded);
            assert_eq!(error.offset(), 7);
            assert_eq!(error.details(), &ErrorDetails::LimitExceeded { resource: "diagnostics", actual: 2, limit: 1 });
        }
    }

    #[test]
    fn retains_unknown_sense_used_by_an_isolated_fin() {
        let nodes = [node(1, 16, "HALFEDGE", vec![field("sense", 0, vec![FieldValue::Character(b'?')])])];
        let mapper = Mapper::new(BrepSourceFormat::Binary, "SCH_TEST", &nodes, RoleAccess::Named);

        assert_eq!(mapper.sense(&nodes[0], "sense"), Ok(Sense::Unknown));
        assert_eq!(Sense::Unknown.multiplier(), None);
    }

    #[test]
    fn maps_exact_blended_edge_and_boundary_surface_definitions() {
        let mut blend_fields = common_surface_fields(0);
        blend_fields.extend([
            field("blend_type", 0, vec![FieldValue::Character(b'R')]),
            field("surface", 1006, vec![FieldValue::PointerIndex(1), FieldValue::PointerIndex(2)]),
            field("spine", 1008, vec![FieldValue::PointerIndex(3)]),
            field("range", 0, vec![FieldValue::Double(Some(0.005)), FieldValue::Double(Some(-0.005))]),
            field("thumb_weight", 0, vec![FieldValue::Double(Some(1.0)), FieldValue::Double(Some(1.0))]),
            field("boundary", 1006, vec![FieldValue::PointerIndex(0), FieldValue::PointerIndex(0)]),
            field("start", 41, vec![FieldValue::PointerIndex(0)]),
            field("end", 41, vec![FieldValue::PointerIndex(0)]),
        ]);
        let mut boundary_fields = common_surface_fields(0);
        boundary_fields
            .extend([field("boundary", 0, vec![FieldValue::ShortInteger(Some(1))]), field("blend", 1006, vec![FieldValue::PointerIndex(4)])]);
        let nodes = [
            node(1, 50, "PLANE", common_surface_fields(4)),
            node(2, 50, "PLANE", common_surface_fields(4)),
            node(3, 30, "LINE", common_curve_fields(4)),
            node(4, 56, "BLENDED_EDGE", blend_fields),
            node(5, 59, "BLEND_BOUND", boundary_fields),
        ];
        let mut mapper = Mapper::new(BrepSourceFormat::Binary, "SCH_TEST", &nodes, RoleAccess::Named);

        let blend = mapper.map_surface(4);
        assert!(blend.is_ok());
        if let Ok(blend) = blend {
            assert!(matches!(
                blend.kind,
                SurfaceKind::BlendedEdge {
                    blend_type: BlendType::RollingBall,
                    supporting_surfaces: [0, 1],
                    spine_curve: 0,
                    ranges: [0.005, -0.005],
                    boundary_surfaces: [None, None],
                    ..
                }
            ));
        }
        let boundary = mapper.map_surface(5);
        assert!(boundary.is_ok());
        if let Ok(boundary) = boundary {
            assert_eq!(boundary.kind, SurfaceKind::BlendBoundary { boundary_index: 1, blend_surface: 2 });
        }
    }

    #[test]
    fn rejects_an_unresolved_required_body_chain_pointer() {
        let mut nodes = wire_acorn_nodes();
        let body = &mut nodes[0];
        let region = body.fields.iter_mut().find(|field| field.definition.name == "region");
        assert!(region.is_some());
        if let Some(region) = region {
            region.values = vec![FieldValue::PointerIndex(99)];
        }

        let error = Mapper::new(BrepSourceFormat::Binary, "SCH_TEST", &nodes, RoleAccess::Named).map().err();
        assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::InvalidBrepReference));
        assert_eq!(
            error.as_ref().map(ParseError::details),
            Some(&ErrorDetails::BrepReference { node_index: 1, field: "region", target_index: 99, expected_type: "REGION" })
        );
    }
}
