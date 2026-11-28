//! Spatial bounds for the two control triangles used to seed a surface-point solve.

use super::p1::{control_point, triangles_dist, well_behaved_control_net};
use crate::error::Result;
use crate::geom::extent3d::Extent3d;
use crate::geom::vector3d::Point3;
use crate::math::BIG_DOUBLE;
use crate::nurbs::math::CPoint;

const CELLS_PER_BLOCK: usize = 16;

#[derive(Clone, Copy)]
pub(super) struct Net<'a> {
    pub points: &'a [CPoint],
    pub u: usize,
    pub v: usize,
    pub rational: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NetShape {
    pub suspicious: bool,
    pub average_spacing: f64,
}

impl Net<'_> {
    pub(super) fn shape(self) -> Result<NetShape> {
        let mut suspicious = self.rational;
        let mut distance = 0.0;
        let mut count = 0u64;
        for i in 0..self.u {
            for j in 0..self.v {
                if !suspicious
                    && !well_behaved_control_net(
                        Point3::ZERO,
                        self.points,
                        self.rational,
                        i,
                        j,
                        self.u,
                        self.v,
                    )?
                {
                    suspicious = true;
                }
                let point = self.point(i, j)?;
                if i > 0 {
                    distance += point.distance(self.point(i - 1, j)?);
                    count += 1;
                }
                if j > 0 {
                    distance += point.distance(self.point(i, j - 1)?);
                    count += 1;
                }
            }
        }
        Ok(NetShape {
            suspicious,
            average_spacing: distance / count as f64,
        })
    }

    fn point(self, i: usize, j: usize) -> Result<Point3> {
        control_point(self.points, self.v, i, j, self.rational)
    }
}

// Keep non-finite values and extreme coordinates on the original arithmetic path.
// Within this range the products used to project onto a control triangle cannot overflow.
pub(super) fn indexable(p: Point3) -> bool {
    [p.x, p.y, p.z].iter().all(|x| x.abs() <= 1.0e12)
}

#[derive(Debug)]
pub(crate) struct CellIndex {
    blocks: Vec<Extent3d>,
    pub(super) shape: NetShape,
}

impl CellIndex {
    pub(super) fn build(net: Net<'_>) -> Result<Option<Self>> {
        if net.u < 2 || net.v < 2 || net.u.checked_mul(net.v) != Some(net.points.len()) {
            return Ok(None);
        }
        let cells = (net.u - 1) * (net.v - 1);
        let mut blocks = Vec::with_capacity(cells.div_ceil(CELLS_PER_BLOCK));
        for first in (0..cells).step_by(CELLS_PER_BLOCK) {
            let mut bounds = Extent3d::new();
            let mut scale = 1.0_f64;
            for cell in first..(first + CELLS_PER_BLOCK).min(cells) {
                let (i, j) = (cell / (net.v - 1), cell % (net.v - 1));
                for (u, v) in [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)] {
                    let point = net.point(u, v)?;
                    if !indexable(point) {
                        return Ok(None);
                    }
                    scale = scale
                        .max(point.x.abs())
                        .max(point.y.abs())
                        .max(point.z.abs());
                    bounds.add_point(point);
                }
            }
            // The distance routine reconstructs a point from a corner and two scaled
            // edge vectors. Allow for roundoff in the differences, products and sum.
            let pad = 128.0 * f64::EPSILON * scale;
            let margin = Point3::new(pad, pad, pad);
            blocks.push(Extent3d::from_points(
                bounds.min() - margin,
                bounds.max() + margin,
            ));
        }
        Ok(Some(Self {
            blocks,
            shape: net.shape()?,
        }))
    }

    pub(super) fn nearest(
        &self,
        net: Net<'_>,
        query: Point3,
        initial: (usize, usize),
    ) -> Result<CellHit> {
        self.search(query, net.u, net.v, initial, |i, j| {
            triangles_dist(query, net.points, net.rational, i, j, net.u, net.v)
        })
    }

    pub(super) fn search(
        &self,
        query: Point3,
        u: usize,
        v: usize,
        initial: (usize, usize),
        mut evaluate: impl FnMut(usize, usize) -> Result<(f64, f64, f64)>,
    ) -> Result<CellHit> {
        // The largest separation along one axis is a lower bound on 3D distance.
        let lower = |b: &Extent3d| {
            let (lo, hi) = (b.min(), b.max());
            0.0_f64
                .max(lo.x - query.x)
                .max(query.x - hi.x)
                .max(lo.y - query.y)
                .max(query.y - hi.y)
                .max(lo.z - query.z)
                .max(query.z - hi.z)
        };
        let mut best = CellHit {
            cell: initial,
            distance: BIG_DOUBLE,
            local: (-1.0, -1.0),
        };
        let blocks = self.blocks.iter().enumerate();
        let closest = blocks
            .clone()
            .map(|(i, bounds)| (i, bounds, lower(bounds)))
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((first, bounds, _)) = closest {
            let cells = (u - 1) * (v - 1);
            let others = blocks.filter(|&(i, _)| i != first);
            for (block, bounds) in std::iter::once((first, bounds)).chain(others) {
                // Allow for rounding of the query distance as well as the box itself.
                if lower(bounds) > best.distance * (1.0 + 16.0 * f64::EPSILON) {
                    continue;
                }
                let start = block * CELLS_PER_BLOCK;
                for cell in start..(start + CELLS_PER_BLOCK).min(cells) {
                    let ij = (cell / (v - 1), cell % (v - 1));
                    let (distance, lu, lv) = evaluate(ij.0, ij.1)?;
                    // A tie belongs to the first cell in the original row-major scan.
                    if distance < best.distance
                        || (distance == best.distance && distance < BIG_DOUBLE && ij < best.cell)
                    {
                        best = CellHit {
                            cell: ij,
                            distance,
                            local: (lu, lv),
                        };
                    }
                }
            }
        }
        Ok(best)
    }
}

#[derive(Debug)]
pub(super) struct CellHit {
    pub cell: (usize, usize),
    pub distance: f64,
    pub local: (f64, f64),
}
