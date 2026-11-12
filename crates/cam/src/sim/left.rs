//! Material the program left on the part over the whole part, not only where it cut.

use super::*;

/// Material the program left on the part over the whole part, not only where it cut: each
/// column of the stock (every few cells, about [`WHOLE_SAMPLES`] of them) held against all of the
/// part's stretches along it, so stock never cut over a pocket, a bore's core on a 4th axis or
/// what stands under the part's own material is found too. Columns that meet no part (the stock
/// round it, a bar beyond it) and stock below the part's bottom are not the part's. A piece counts
/// when it is thicker than the tolerance and a cell, measured in 3D (from its ends and its middle
/// to the part's surface), so a sliver beside a wall is as thin as it is there.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartLeft {
    /// The volume left (mm³), and how much of it stands under the part's own material along Z
    /// (along the ray), where no tool of this setup reaches.
    pub volume: f64,
    pub hidden_volume: f64,
    /// The worst spots, thickest first (mm and where, work coordinates), at most [`SPOTS`] and
    /// [`SPOT_APART`] apart.
    pub spots: Vec<(f64, Vec3)>,
    /// The area (mm²; on a 4th axis, at the stock's surface) over which the part has material over
    /// air over material along Z (along the ray): faces a height map cannot see.
    pub hidden_area: f64,
    /// Every piece counted, with whether it stands under the part's own material, for a caller
    /// to account for each (a feature left uncut, a later setup, a holding tab).
    pub pieces: Vec<LeftPiece>,
}

/// A piece of material left on the part: its ends along its column (work coordinates), its volume
/// and thickness, whether it stands under the part's own material along Z (along the ray), and
/// whether part material stands both over and under it along Z on a height map (`enclosed`: no
/// setup along Z, from above or turned over, reaches it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeftPiece {
    pub from: Vec3,
    pub to: Vec3,
    pub volume: f64,
    pub thick: f64,
    pub hidden: bool,
    pub enclosed: bool,
}

impl LeftPiece {
    pub fn middle(&self) -> Vec3 {
        (self.from + self.to) * 0.5
    }
}

/// The worst spots [`PartLeft`] keeps, and how far apart (mm).
pub const SPOTS: usize = 5;
pub const SPOT_APART: f64 = 3.0;

impl PartLeft {
    /// The thickest piece left, 0 for none.
    pub fn max(&self) -> f64 {
        self.spots.first().map_or(0.0, |s| s.0)
    }

    pub(super) fn spot(&mut self, thick: f64, at: Vec3) {
        if let Some(s) = self.spots.iter_mut().find(|s| (s.1 - at).len() < SPOT_APART) {
            if thick > s.0 {
                *s = (thick, at);
            }
        } else {
            self.spots.push((thick, at));
        }
        self.spots.sort_by(|a, b| b.0.total_cmp(&a.0));
        self.spots.truncate(SPOTS);
    }

    pub(super) fn add(&mut self, o: &PartLeft) {
        self.pieces.extend_from_slice(&o.pieces);
        self.volume += o.volume;
        self.hidden_volume += o.hidden_volume;
        self.hidden_area += o.hidden_area;
        for &(t, at) in &o.spots {
            self.spot(t, at);
        }
    }
}

/// What `model` leaves on the part over the whole part ([`PartLeft`]), looking at every
/// `stride`-th column each way (from the whole grid's first, so bands agree). With `sides` (the
/// stock has [`SideDexels`]), what has part material both over and under it along Z is theirs to
/// measure and is left out here.
pub(super) fn part_left(model: &Model, part: &Surface, tol: f64, stride: usize, sides: bool) -> PartLeft {
    let mut left = PartLeft::default();
    let stride = stride.max(1);
    let weight = (stride * stride) as f64;
    // The thickness of a piece `[a, b]` of the column `point`: the farthest its ends are from the
    // part and twice its middle's distance (a slab between two faces is as thick as it is long),
    // never more than its length.
    let thickness = |a: f64, b: f64, point: &dyn Fn(f64) -> Vec3| -> f64 {
        let len = b - a;
        let ends = distance_within(part, point(a), len).max(distance_within(part, point(b), len));
        ends.max(2.0 * distance_within(part, point(0.5 * (a + b)), len)).min(len)
    };
    match model {
        Model::Heights(h) => {
            let allowance = tol + h.dx.max(h.dy);
            let lowest = part.bounds().min.z.max(h.bottom);
            let span = (h.top - h.bottom).max(0.0) + 2.0;
            let area = h.dx * h.dy * weight;
            for j in (0..h.ny).filter(|j| (h.j0 + j) % stride == 0) {
                for i in (0..h.nx).step_by(stride) {
                    let Some(&z) = h.h.get(j * h.nx + i) else { continue };
                    let (x, y) = h.centre(i, j);
                    let top = h.top + 1.0;
                    let Some(list) = segment_stretches(part, Vec3::new(x, y, top), Vec3::new(0.0, 0.0, -span)) else { continue };
                    if list.is_empty() {
                        continue;
                    }
                    let stretches: Vec<[f64; 2]> = list.iter().map(|p| [top - p[1] * span, top - p[0] * span]).collect();
                    if stretches.len() > 1 {
                        left.hidden_area += area;
                    }
                    // The cell's material ends below where an earlier setup turned over cut it.
                    let lowest = h.under.get(j * h.nx + i).map_or(lowest, |u| lowest.max(*u));
                    if z <= lowest {
                        continue;
                    }
                    let point = |v: f64| Vec3::new(x, y, v);
                    for (o, hidden) in outside_part(&[[lowest, z]], &stretches) {
                        if o[1] - o[0] <= allowance {
                            continue;
                        }
                        let enclosed = hidden && stretches.iter().any(|s| s[1] <= o[0] + 1e-9);
                        if enclosed && sides {
                            continue;
                        }
                        let thick = thickness(o[0], o[1], &point);
                        if thick > allowance {
                            let volume = (o[1] - o[0]) * area;
                            left.volume += volume;
                            if hidden {
                                left.hidden_volume += volume;
                            }
                            left.spot(thick, point(0.5 * (o[0] + o[1])));
                            left.pieces.push(LeftPiece { from: point(o[0]), to: point(o[1]), volume, thick, hidden, enclosed });
                        }
                    }
                }
            }
        }
        Model::Radii(r) => {
            let dtheta = r.dtheta();
            let allowance = tol + r.dx.max(r.radius * dtheta);
            let out = r.radius + 1.0;
            for k in (0..r.nt).filter(|k| (r.k0 + k) % stride == 0) {
                let (Some(&s), Some(&c)) = (r.sin.get(k), r.cos.get(k)) else { continue };
                let dir = Vec3::new(0.0, s, c);
                for i in (0..r.nx).step_by(stride) {
                    let cell = k * r.nx + i;
                    let Some(&rho) = r.rho.get(cell) else { continue };
                    let x = r.x0 + (i as f64 + 0.5) * r.dx;
                    let point = |v: f64| Vec3::new(x, 0.0, 0.0) + dir * v;
                    let Some(list) = segment_stretches(part, point(out), dir * -out) else { continue };
                    if list.is_empty() {
                        continue;
                    }
                    let stretches: Vec<[f64; 2]> = list.iter().map(|p| [out * (1.0 - p[1]), out * (1.0 - p[0])]).collect();
                    if stretches.len() > 1 {
                        left.hidden_area += r.dx * r.radius * dtheta * weight;
                    }
                    // The ray's material: from the axis out to `rho`, less its air.
                    let mut material: Vec<[f64; 2]> = Vec::new();
                    let mut from = 0.0f64;
                    for g in r.gaps.get(cell).map_or(&[][..], Vec::as_slice) {
                        if g[0] > from {
                            material.push([from, g[0].min(rho)]);
                        }
                        from = from.max(g[1]);
                    }
                    if rho > from {
                        material.push([from, rho]);
                    }
                    for (o, hidden) in outside_part(&material, &stretches) {
                        if o[1] - o[0] <= allowance {
                            continue;
                        }
                        let thick = thickness(o[0], o[1], &point);
                        if thick > allowance {
                            let volume = 0.5 * (o[1] * o[1] - o[0] * o[0]) * dtheta * r.dx * weight;
                            left.volume += volume;
                            if hidden {
                                left.hidden_volume += volume;
                            }
                            left.spot(thick, point(0.5 * (o[0] + o[1])));
                            left.pieces.push(LeftPiece { from: point(o[0]), to: point(o[1]), volume, thick, hidden, enclosed: false });
                        }
                    }
                }
            }
        }
    }
    left
}
