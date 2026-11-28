//! Exact shared topology for the mesh converter's known faces and boundary curves.
//! No tolerance stitching or healing chooses which faces share an edge.
//!
//! Deviations: no upstream counterpart. No recursion.

use std::collections::BTreeMap;

use crate::curve::curve::{Curve, CurveKind};
use crate::geom::attribute::{AttributeStore, ObjectKey};
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::surface::surface::Surface;
use crate::topology::brep::{
    make_topology_from_data, MakeTopologyOptions, TopologyEnv, HEALER_VERSION,
};
use crate::topology::brep_data::{
    BrepData, DataSourceType, EdgeData, EuData, FaceData, FaceuseData, Keyed, LoopData, RegionData,
    ShellData, VertexData, NO_OBJECT,
};
use crate::topology::loopuse::notify_ref;
use crate::topology::topology::NotifyRecords;
use crate::{Brep, Result};

use super::invalid;

/// A directed use of an exact boundary curve. Reversing a use keeps the curve's parameters.
pub(super) struct Side {
    pub curve: Curve,
    pub forward: bool,
}

impl Side {
    pub fn new(curve: Curve) -> Self {
        Self {
            curve,
            forward: true,
        }
    }
    pub fn reversed(&self, k: &Kernel) -> Result<Self> {
        Ok(Self {
            curve: self.curve.copy(k)?,
            forward: !self.forward,
        })
    }
}

/// Loops follow the outward material normal; `forward` relates it to the surface normal.
pub(super) struct Face {
    pub loops: Vec<Vec<Side>>,
    pub surface: Surface,
    pub domain: Extent2d,
    pub forward: bool,
}

/// Each region lists its outer shell first and any cavity shells after it.
pub(super) struct Region {
    pub void: bool,
    pub shells: Vec<usize>,
}

type PointKey = (i64, i64, i64);
const COINCIDENT: f64 = 1e-9;
const ZONE: f64 = 5e-7;

/// Cell coordinates are relative to this body's source point and scaled only for integer
/// representability. The actual coincidence decision remains the fixed COINCIDENT distance.
struct Grid {
    origin: Vec3,
    cell: f64,
}
impl Grid {
    fn key(&self, p: Vec3) -> PointKey {
        let p = p - self.origin;
        (
            (p.x / self.cell).round() as i64,
            (p.y / self.cell).round() as i64,
            (p.z / self.cell).round() as i64,
        )
    }
}

fn index(n: usize) -> Result<u64> {
    u64::try_from(n).map_err(|_| invalid("reconstructed topology index overflow"))
}

fn vertex(
    data: &mut BrepData,
    grid: &mut BTreeMap<PointKey, Vec<usize>>,
    point: Vec3,
    coordinates: &Grid,
) -> usize {
    let cell = coordinates.key(point);
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                for &i in grid
                    .get(&(cell.0 + dx, cell.1 + dy, cell.2 + dz))
                    .into_iter()
                    .flatten()
                {
                    if data
                        .vertices
                        .get(i)
                        .is_some_and(|v| v.point.distance(point) <= COINCIDENT)
                    {
                        return i;
                    }
                }
            }
        }
    }
    let i = data.vertices.len();
    data.vertices.push(VertexData {
        point,
        zone_tol_3d: ZONE,
        ..VertexData::zeroed()
    });
    grid.entry(cell).or_default().push(i);
    i
}

/// Assemble the supplied shells with one shared edge for each supplied curve and endpoints.
/// The caller already knows the material side and nesting from the source construction.
pub(super) fn build(
    k: &mut Kernel,
    faces: Vec<Face>,
    shells: &[Vec<usize>],
    regions: &[Region],
) -> Result<Brep> {
    let first = faces
        .first()
        .and_then(|f| f.loops.first())
        .and_then(|lp| lp.first())
        .ok_or_else(|| invalid("empty reconstructed faces"))?;
    let origin = first.curve.ends(k)?.start;
    let mut reach = 0f64;
    for side in faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        let ends = side.curve.ends(k)?;
        let mid = side
            .curve
            .evaluate_point(k, side.curve.natural_interval().evaluate(0.5))?;
        for p in [ends.start, ends.end, mid] {
            let d = p - origin;
            reach = reach.max(d.x.abs()).max(d.y.abs()).max(d.z.abs());
        }
    }
    let coordinates = Grid {
        origin,
        cell: COINCIDENT.max(reach * 1e-15),
    };
    let mut data = BrepData::new(1, DataSourceType::Stl);
    data.healer_version = HEALER_VERSION;
    data.zone_tol_3d = ZONE;
    data.approx_tol_3d = ZONE * 0.5;
    data.start_region = 0;
    data.num_regions = index(regions.len())?;
    let mut geometry_key = 0u64;
    let mut next_key = || {
        geometry_key += 1;
        ObjectKey(geometry_key)
    };
    for (ri, region) in regions.iter().enumerate() {
        data.regions.push(RegionData {
            start_shell: index(data.shells.len())?,
            num_shells: index(region.shells.len())?,
            is_void: i32::from(ri == 0 || region.void),
            ..RegionData::zeroed()
        });
        for &si in &region.shells {
            let list = shells
                .get(si)
                .ok_or_else(|| invalid("missing reconstructed shell"))?;
            data.shells.push(ShellData {
                shell_type: 0,
                faceuse_start: index(data.faceuses.len())?,
                num_faceuses: index(list.len())?,
                ..ShellData::zeroed()
            });
            for &fi in list {
                let face = faces
                    .get(fi)
                    .ok_or_else(|| invalid("missing reconstructed face"))?;
                data.faceuses.push(FaceuseData {
                    face: index(fi)?,
                    orientation: i32::from(face.forward == (ri == 0 || region.void)),
                    ..FaceuseData::zeroed()
                });
            }
        }
    }
    let mut vertices = BTreeMap::new();
    let mut edges = BTreeMap::<(usize, usize, PointKey), (usize, usize, usize, Vec3)>::new();
    for face in faces {
        k.check_cancelled("reconstruct mesh")?;
        data.faces.push(FaceData {
            zone_tol_3d: ZONE,
            surface: index(data.surfaces.len())?,
            start_loop: index(data.loops.len())?,
            num_loops: index(face.loops.len())?,
            uv_domain: face.domain,
            rectangular_trim: 0,
            ..FaceData::zeroed()
        });
        data.surfaces.push(Keyed {
            key: next_key(),
            object: face.surface,
        });
        for mut lp in face.loops {
            if !face.forward {
                lp.reverse();
                for side in &mut lp {
                    side.forward = !side.forward;
                }
            }
            data.loops.push(LoopData {
                loop_type: 0,
                start_eu: index(data.edgeuses.len())?,
                num_eu: index(lp.len())?,
                ..LoopData::zeroed()
            });
            let loop_id = data.loops.len() - 1;
            for side in lp {
                let interval = side.curve.natural_interval();
                let ends = side.curve.ends(k)?;
                let a = vertex(&mut data, &mut vertices, ends.start, &coordinates);
                let b = vertex(&mut data, &mut vertices, ends.end, &coordinates);
                let mid = if matches!(side.curve.kind(), CurveKind::Line(_)) {
                    (ends.start + ends.end) * 0.5
                } else {
                    side.curve.evaluate_point(k, interval.evaluate(0.5))?
                };
                let cell = coordinates.key(mid);
                let mut known = None;
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            let edge_key =
                                (a.min(b), a.max(b), (cell.0 + dx, cell.1 + dy, cell.2 + dz));
                            if let Some(&entry) = edges
                                .get(&edge_key)
                                .filter(|entry| entry.3.distance(mid) <= COINCIDENT)
                            {
                                known = Some(entry);
                            }
                        }
                    }
                }
                let edge_key = (a.min(b), a.max(b), cell);
                let (edge, start, _end, _) = if let Some(entry) = known {
                    entry
                } else {
                    let edge = data.edges.len();
                    data.edges.push(EdgeData {
                        zone_tol_3d: ZONE,
                        interval,
                        curve: index(data.curves_3d.len())?,
                        start_vertex: index(a)?,
                        end_vertex: index(b)?,
                        prim_eu: i64::try_from(data.edgeuses.len())
                            .map_err(|_| invalid("edge use index overflow"))?,
                        ..EdgeData::zeroed()
                    });
                    data.curves_3d.push(Keyed {
                        key: next_key(),
                        object: side.curve,
                    });
                    edges.insert(edge_key, (edge, a, b, mid));
                    (edge, a, b, mid)
                };
                data.edgeuses.push(EuData {
                    eu_type: 0,
                    edge: index(edge)?,
                    loop_: index(loop_id)?,
                    orientation: i32::from(side.forward == (a == start)),
                    uv_curve: NO_OBJECT,
                    ..EuData::zeroed()
                });
            }
        }
    }
    let mut body = Brep::new_with_infinite_region()?;
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &notify_ref,
        records: &mut records,
        next_key: geometry_key + 1,
    };
    make_topology_from_data(
        k,
        &mut body,
        &mut data,
        &mut env,
        MakeTopologyOptions {
            force_same_tol: true,
            healer_enabled: false,
            add_analytics: false,
            ..MakeTopologyOptions::default()
        },
    )?;
    body.refresh_cached_topology();
    crate::topology::brep::create_missing_uv_trim_curves(k, &mut body)?;
    Ok(body)
}
