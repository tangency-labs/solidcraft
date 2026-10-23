//! Opt-in real cache inputs/observations for `geom.grid` only.
//!
//! Geometry is the explicit affine map `origin + u * U + v * V`, represented by
//! a clamped, non-rational degree-1 tensor-product B-spline with the given domain
//! and uniform span counts. Leaf ordinals refer only to eligible nodes of that
//! surface's actual cache, in `GetAllTreeNodes` order; they are not arena ids.

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::polar_box::PolarBoxState;
use solidcore::geom::tree::{AuxDataType, NodeClass, NodeData};
use solidcore::geom::vector2d::Vec2;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::surface_cache;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::SurfaceCache;

use super::{
    p3, put3, put_elem, put_ext, ElementId, Extent3d, Grid, GridItem, HierarchyElement,
    HierarchyId, NodeId, SurfaceGridElement, Vec3, P3,
};
use solidcore::SolidError;

pub(super) fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input("difftest::geom_grid::native", reason)
}

struct CachedSurface {
    id: i64,
    cache: SurfaceCache<i64>,
    leaves: Vec<NodeId>,
}

/// Definitions remain in input order; no global cache or pointer-derived identity.
#[derive(Default)]
pub(super) struct Caches {
    kernel: Kernel,
    surfaces: Vec<CachedSurface>,
    hierarchies: Vec<(HierarchyId, i64)>,
}

/// A cache for `Grid::expand_hierarchy` on a base-class element, which fails with
/// `SM_ERR` before it reads the cache.
pub(super) fn placeholder_cache(kernel: &Kernel) -> solidcore::Result<SurfaceCache<i64>> {
    let plane = Surface::from(Plane::new(
        Vec3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec2::new(1.0, 1.0),
        &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
    ));
    surface_cache(kernel, &plane, None)
}

fn leaf_nodes(cache: &SurfaceCache<i64>) -> solidcore::Result<Vec<NodeId>> {
    let tree = cache.tree().ok_or_else(|| invalid("cache has no tree"))?;
    let mut out = Vec::new();
    for id in tree.all_tree_nodes() {
        let n = tree.node(id)?;
        if n.aux_type != AuxDataType::BezierSurface
            || n.child1
                .map(|c| {
                    tree.node(c)
                        .map(|n| n.aux_type == AuxDataType::BezierSurface)
                })
                .transpose()?
                .unwrap_or(false)
        {
            continue;
        }
        let NodeData::Surface(aux) = n.data else {
            return Err(invalid("Bezier node has no cache data"));
        };
        if aux.node_class != NodeClass::Outside {
            cache.patch(id)?;
            out.push(id);
        }
    }
    if out.is_empty() || out.len() > 256 {
        return Err(invalid("unexpected affine cache leaf count"));
    }
    Ok(out)
}

fn init_leaf(
    kernel: &Kernel,
    s: &mut CachedSurface,
    node: NodeId,
) -> solidcore::Result<SurfaceGridElement<i64>> {
    // `SmSurfaceGridElement::Init` of the actual cache node: the library's own port, so
    // this harness runs the code `Grid::expand_hierarchy` and `Grid::add_surface_node`
    // run (the leaf's surface is the node's owner object, here the surface's id).
    SurfaceGridElement::init(kernel, &mut s.cache, node)
}

impl Caches {
    pub(super) fn define(
        &mut self,
        id: i64,
        origin: P3,
        u: P3,
        v: P3,
        domain: [f64; 4],
        spans: [usize; 2],
    ) -> solidcore::Result<()> {
        let [u0, v0, u1, v1] = domain;
        let [nu, nv] = spans;
        // The FFI guard has already bounded counts, parameters and conditioning.
        let params = |lo: f64, hi: f64, n: usize| -> Vec<f64> {
            (0..=n)
                .map(|i| lo + (hi - lo) * i as f64 / n as f64)
                .collect()
        };
        let us = params(u0, u1, nu);
        let vs = params(v0, v1, nv);
        let knots = |p: &[f64]| {
            std::iter::once(p.first().copied().unwrap_or(0.0))
                .chain(p.iter().copied())
                .chain(std::iter::once(p.last().copied().unwrap_or(0.0)))
                .collect()
        };
        let net = us
            .iter()
            .map(|&a| {
                vs.iter()
                    .map(|&b| {
                        let p = p3(origin) + p3(u) * a + p3(v) * b;
                        CPoint::new(p.x, p.y, p.z, NOW)
                    })
                    .collect()
            })
            .collect();
        let nurb = NurbSurface::new(net, 1, 1, knots(&us), knots(&vs))?;
        let surface = Surface::from(BSplineSurface::from_nurb(nurb, false));
        let mut cache = surface_cache(&self.kernel, &surface, Some(id))?;
        if cache.get_tree(&self.kernel)?.owner_object()? != Some(id)
            || cache.surface_handle() != Some(id)
        {
            return Err(invalid("surface cache identity was not retained"));
        }
        let leaves = leaf_nodes(&cache)?;
        self.surfaces.push(CachedSurface { id, cache, leaves });
        Ok(())
    }

    fn surface_mut(&mut self, id: i64) -> solidcore::Result<&mut CachedSurface> {
        self.surfaces
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| invalid("unknown surface identity"))
    }

    pub(super) fn add_hierarchy(
        &mut self,
        grid: &mut Grid<i64>,
        id: i64,
        tol: f64,
        bbox: &Extent3d,
    ) -> solidcore::Result<()> {
        self.surface_mut(id)?;
        let h = grid.add_hierarchy_element(HierarchyElement::new_surface(id, tol), bbox)?;
        self.hierarchies.push((h, id));
        Ok(())
    }

    pub(super) fn expand(
        &mut self,
        grid: &mut Grid<i64>,
        h: HierarchyId,
        control: &mut impl FnMut(SurfaceGridElement<i64>) -> SurfaceGridElement<i64>,
    ) -> solidcore::Result<()> {
        let id = self
            .hierarchies
            .iter()
            .find(|(key, _)| *key == h)
            .map(|(_, id)| *id)
            .ok_or_else(|| invalid("not an explicit surface hierarchy"))?;
        let kernel = &self.kernel;
        let s = self
            .surfaces
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| invalid("unknown surface identity"))?;
        let tree = s
            .cache
            .tree()
            .ok_or_else(|| invalid("cache has no tree"))?
            .clone();
        grid.expand_hierarchy_with(h, &tree, |owner, node| {
            if owner != id || !s.leaves.contains(&node) {
                return Err(invalid("node does not belong to selected cache"));
            }
            Ok(control(init_leaf(kernel, s, node)?))
        })
    }

    pub(super) fn add_leaf(
        &mut self,
        grid: &mut Grid<i64>,
        id: i64,
        ordinal: usize,
        bbox: &Extent3d,
        control: &mut impl FnMut(SurfaceGridElement<i64>) -> SurfaceGridElement<i64>,
    ) -> solidcore::Result<()> {
        let kernel = &self.kernel;
        let s = self
            .surfaces
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| invalid("unknown surface identity"))?;
        let node = s
            .leaves
            .get(ordinal)
            .copied()
            .ok_or_else(|| invalid("leaf ordinal outside selected cache"))?;
        grid.add_surface_element(control(init_leaf(kernel, s, node)?), bbox)?;
        Ok(())
    }

    fn ordinal(&self, leaf: &SurfaceGridElement<i64>) -> solidcore::Result<usize> {
        let s = self
            .surfaces
            .iter()
            .find(|s| s.id == leaf.surface)
            .ok_or_else(|| invalid("unknown leaf owner"))?;
        for (i, &node) in s.leaves.iter().enumerate() {
            if s.cache.patch(node)?.aux.uv_domain == leaf.uv_domain {
                return Ok(i);
            }
        }
        Err(invalid("leaf domain absent from owning cache"))
    }

    pub(super) fn observe(
        &self,
        grid: &Grid<i64>,
        oi: &mut Vec<i64>,
        of: &mut Vec<f64>,
    ) -> solidcore::Result<()> {
        // Independent cache inventory in input/eligible-node order.
        oi.push(self.surfaces.len() as i64);
        for s in &self.surfaces {
            oi.extend([s.id, s.leaves.len() as i64]);
            for &node in &s.leaves {
                put_domain(of, &s.cache.patch(node)?.aux.uv_domain);
                put_ext(
                    of,
                    &s.cache
                        .tree()
                        .ok_or_else(|| invalid("cache has no tree"))?
                        .node(node)?
                        .bbox,
                );
            }
        }
        oi.push(grid.hierarchy_elements().len() as i64);
        for (i, _) in grid.hierarchy_elements().iter().enumerate() {
            oi.push(
                self.hierarchies
                    .iter()
                    .find(|(h, _)| h.0 == i)
                    .map_or(-1, |(_, id)| *id),
            );
        }
        // Surface pool allocation order, independent of voxel encounter order.
        let leaves: Vec<_> = grid
            .elements()
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                if let GridItem::Surface(leaf) = e.item() {
                    Some((ElementId(i), e, leaf))
                } else {
                    None
                }
            })
            .collect();
        oi.push(leaves.len() as i64);
        for (_, e, leaf) in &leaves {
            oi.extend([leaf.surface, self.ordinal(leaf)? as i64]);
            put3(of, e.sphere_center());
            of.push(e.sphere_radius());
            put_ext(of, e.bounding_box());
            put_leaf(of, leaf)?;
        }
        let [nx, ny, nz] = grid.size();
        for ix in 0..nx {
            for iy in 0..ny {
                for iz in 0..nz {
                    let voxel = grid.voxel([ix, iy, iz])?;
                    match voxel.grid_elements() {
                        None => oi.push(-1),
                        Some(es) => {
                            oi.push(es.len() as i64);
                            for &id in es {
                                put_elem(oi, grid, id)?;
                                oi.push(
                                    leaves
                                        .iter()
                                        .position(|(key, _, _)| *key == id)
                                        .map_or(-1, |i| i as i64),
                                );
                            }
                        }
                    }
                    match voxel.hierarchy_elements() {
                        None => oi.push(-1),
                        Some(hs) => {
                            oi.push(hs.len() as i64);
                            oi.extend(hs.iter().map(|h| h.0 as i64));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn put_domain(out: &mut Vec<f64>, d: &Extent2d) {
    out.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

fn put_leaf(out: &mut Vec<f64>, leaf: &SurfaceGridElement<i64>) -> solidcore::Result<()> {
    put_domain(out, &leaf.uv_domain);
    put_ext(out, &leaf.bbox);
    for v in [leaf.mid_point, leaf.mid_du, leaf.mid_dv, leaf.mid_normal] {
        put3(out, v);
    }
    for j in 0..3 {
        put3(
            out,
            leaf.pseudo_box
                .basis_vector(j)
                .ok_or_else(|| invalid("missing pseudo box basis"))?,
        );
    }
    for j in 0..3 {
        let iv = leaf
            .pseudo_box
            .interval(j)
            .ok_or_else(|| invalid("missing pseudo box interval"))?;
        out.extend([iv.min(), iv.max()]);
    }
    let polar = &leaf.polar_box;
    let state = match polar.state() {
        PolarBoxState::Uninitialized => 0,
        PolarBoxState::PointWithBasis => 1,
        PolarBoxState::SinglePoint => 2,
        PolarBoxState::Arc => 3,
        PolarBoxState::Region => 4,
        PolarBoxState::Unbounded => 5,
    };
    out.push(f64::from(state));
    let bounded = (2..=4).contains(&state);
    let d = polar.domain();
    out.extend(if bounded {
        [d.min().x, d.min().y, d.max().x, d.max().y]
    } else {
        [0.0; 4]
    });
    for b in polar.basis() {
        put3(
            out,
            if bounded {
                *b
            } else {
                Vec3::new(0.0, 0.0, 0.0)
            },
        );
    }
    Ok(())
}
