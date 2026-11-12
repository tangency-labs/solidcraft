//! The simulated stock drawn for the view, one tile ([`TILE`] × [`TILE`] cells) at a time so that
//! a view rebuilds only the tiles a move cut ([`SimState::tile_version`]). Display only: nothing
//! decides with these meshes.
//!
//! A heightfield tile is the tops of its cells (every `step`-th cell when the stock has more
//! than `max_side` cells along a side), walls where a cell stands above its neighbour, the
//! stock's sides where the tile is at its edge, and its bottom. Where the stock is kept along X
//! and Y too ([`SimState::side_column`], a part with faces under its own material), each cell
//! also shows what those lines hold that its height cannot: material over its top (left above a
//! cut that went under it, as a T-slot cutter's under an overhang) as a block, and air inside
//! its column (cut under material) as a floor and a ceiling. A 4th axis's tile is the round
//! surface at each cell's radius, walls between cells of different radii, and the ends of the
//! bar. A tile draws the walls on its far sides (+X and +Y, or +X and round), so it depends on
//! the cells next to it there: rebuild it when its own version or those neighbours' change.
//! Every triangle's `tri_face` is its colour class: [`CLASS_UNCUT`], or [`CLASS_TOOL`] plus
//! the slot of the tool that cut it last. Ported from the author's own MIT-licensed G-code
//! simulator: the stock drawn as tops, walls and skirt.

use std::f64::consts::TAU;

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;

use crate::sim::{Grid, SimPlan, SimState, TILE, UNCUT};

/// The colour class of stock no tool has cut.
pub const CLASS_UNCUT: u32 = 0;
/// The colour class of stock cut last by the tool in slot `k`: `CLASS_TOOL + k`.
pub const CLASS_TOOL: u32 = 1;

/// The cells a display cell spans along a side, for a stock at most `max_side` display cells
/// along each side (a power of two, at most [`TILE`], so tiles hold whole display cells).
pub fn display_step(plan: &SimPlan, max_side: usize) -> usize {
    let side = plan.grid().columns().max(plan.grid().rows());
    let mut s = 1usize;
    while s < TILE && side.div_ceil(s) > max_side.max(1) {
        s *= 2;
    }
    s
}

fn class(slot: u8) -> u32 {
    if slot == UNCUT { CLASS_UNCUT } else { CLASS_TOOL + u32::from(slot) }
}

struct Builder {
    m: Mesh,
}

impl Builder {
    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, face: u32) {
        let Ok(base) = u32::try_from(self.m.positions.len()) else { return };
        self.m.positions.extend([a, b, c, d]);
        self.m.triangles.push([base, base + 1, base + 2]);
        self.m.triangles.push([base, base + 2, base + 3]);
        self.m.tri_face.extend([face, face]);
    }
}

/// What the side lines hold over display cell `xs` × `ys` (column top `h`, lines' layer `dz`)
/// that its height does not show: material over its top as a block (its top, its underside and
/// its four sides), and air inside its column under material as a floor and a ceiling. A
/// stretch within half a layer of the top is the top itself (the lines are read a layer apart).
fn side_cell(b: &mut Builder, (xa, xb): (f64, f64), (ya, yb): (f64, f64), h: f64, dz: f64, col: &[[f64; 2]], c: u32) {
    let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let level = |b: &mut Builder, z: f64, up: bool| {
        if up {
            b.quad(p(xa, ya, z), p(xb, ya, z), p(xb, yb, z), p(xa, yb, z), c);
        } else {
            b.quad(p(xa, ya, z), p(xa, yb, z), p(xb, yb, z), p(xb, ya, z), c);
        }
    };
    for (n, s) in col.iter().enumerate() {
        // Material over the top.
        if s[1] - 0.5 * dz > h {
            let lo = s[0].max(h);
            level(b, s[1], true);
            if lo > h + 0.5 * dz {
                level(b, lo, false);
            }
            let hi = s[1];
            b.quad(p(xa, ya, lo), p(xb, ya, lo), p(xb, ya, hi), p(xa, ya, hi), c);
            b.quad(p(xb, ya, lo), p(xb, yb, lo), p(xb, yb, hi), p(xb, ya, hi), c);
            b.quad(p(xb, yb, lo), p(xa, yb, lo), p(xa, yb, hi), p(xb, yb, hi), c);
            b.quad(p(xa, yb, lo), p(xa, ya, lo), p(xa, ya, hi), p(xa, yb, hi), c);
        }
        // Air between this stretch and the next, under the top.
        if let Some(next) = col.get(n + 1)
            && next[0] < h - 0.5 * dz
        {
            level(b, s[1], true);
            level(b, next[0], false);
        }
    }
}

/// Tile `(ti, tj)` of the stock as it is in `state`; see the module documentation.
pub fn tile_mesh(plan: &SimPlan, state: &SimState, ti: usize, tj: usize, step: usize) -> Mesh {
    let grid = *plan.grid();
    let (nu, nv) = (grid.columns(), grid.rows());
    let step = step.clamp(1, TILE);
    let (u0, v0) = (ti * TILE, tj * TILE);
    if u0 >= nu || v0 >= nv {
        return Mesh::default();
    }
    let (u1, v1) = ((u0 + TILE).min(nu), (v0 + TILE).min(nv));
    let mut b = Builder { m: Mesh::default() };
    // A display cell `(i, j)` (its first grid cell) and its value and class.
    let at = |i: usize, j: usize| state.value(plan, i, j).map(|v| (v, class(state.last_tool(plan, i, j))));
    match grid {
        Grid::Heights { x0, y0, dx, dy, bottom, .. } => {
            let x = |i: usize| x0 + i as f64 * dx;
            let y = |j: usize| y0 + j as f64 * dy;
            let layer = state.side_layer();
            let mut j = v0;
            while j < v1 {
                let jn = (j + step).min(nv);
                let mut i = u0;
                while i < u1 {
                    let inext = (i + step).min(nu);
                    let Some((h, c)) = at(i, j) else {
                        i = inext;
                        continue;
                    };
                    b.quad(Vec3::new(x(i), y(j), h), Vec3::new(x(inext), y(j), h), Vec3::new(x(inext), y(jn), h), Vec3::new(x(i), y(jn), h), c);
                    // Walls to the next cells along X and Y (or down to the bottom at the stock's
                    // far sides), whichever stands higher, in its colour; the near sides' skirt.
                    let right = if inext < nu { at(inext, j) } else { Some((bottom, c)) };
                    if let Some((r, rc)) = right.filter(|(r, _)| *r != h) {
                        let (lo, hi, cc) = if r < h { (r, h, c) } else { (h, r, rc) };
                        b.quad(
                            Vec3::new(x(inext), y(j), lo),
                            Vec3::new(x(inext), y(jn), lo),
                            Vec3::new(x(inext), y(jn), hi),
                            Vec3::new(x(inext), y(j), hi),
                            cc,
                        );
                    }
                    let up = if jn < nv { at(i, jn) } else { Some((bottom, c)) };
                    if let Some((u, uc)) = up.filter(|(u, _)| *u != h) {
                        let (lo, hi, cc) = if u < h { (u, h, c) } else { (h, u, uc) };
                        b.quad(
                            Vec3::new(x(inext), y(jn), lo),
                            Vec3::new(x(i), y(jn), lo),
                            Vec3::new(x(i), y(jn), hi),
                            Vec3::new(x(inext), y(jn), hi),
                            cc,
                        );
                    }
                    if i == 0 && bottom < h {
                        b.quad(Vec3::new(x(i), y(jn), bottom), Vec3::new(x(i), y(j), bottom), Vec3::new(x(i), y(j), h), Vec3::new(x(i), y(jn), h), c);
                    }
                    if j == 0 && bottom < h {
                        b.quad(
                            Vec3::new(x(i), y(j), bottom),
                            Vec3::new(x(inext), y(j), bottom),
                            Vec3::new(x(inext), y(j), h),
                            Vec3::new(x(i), y(j), h),
                            c,
                        );
                    }
                    if let Some(dz) = layer
                        && let Some(col) = state.side_column(0.5 * (x(i) + x(inext)), 0.5 * (y(j) + y(jn)))
                    {
                        side_cell(&mut b, (x(i), x(inext)), (y(j), y(jn)), h, dz, &col, c);
                    }
                    i = inext;
                }
                j = jn;
            }
            // The bottom, facing down.
            b.quad(
                Vec3::new(x(u0), y(v0), bottom),
                Vec3::new(x(u0), y(v1), bottom),
                Vec3::new(x(u1), y(v1), bottom),
                Vec3::new(x(u1), y(v0), bottom),
                CLASS_UNCUT,
            );
        }
        Grid::Radii { x0, dx, nt, .. } => {
            let x = |i: usize| x0 + i as f64 * dx;
            let dir = |k: usize| {
                let (s, c) = (k as f64 * TAU / nt.max(1) as f64).sin_cos();
                Vec3::new(0.0, s, c)
            };
            let p = |i: usize, k: usize, r: f64| Vec3::new(x(i), 0.0, 0.0) + dir(k) * r;
            let mut k = v0;
            while k < v1 {
                let kn = (k + step).min(nv);
                let mut i = u0;
                while i < u1 {
                    let inext = (i + step).min(nu);
                    let Some((r, c)) = at(i, k) else {
                        i = inext;
                        continue;
                    };
                    b.quad(p(i, k, r), p(i, kn, r), p(inext, kn, r), p(inext, k, r), c);
                    // Air inside the ray's material (a flat, a pocket or a hole in the side
                    // seen at a slant): where it starts, facing out like the surface, and where
                    // it ends, facing in.
                    for g in state.gaps(plan, i, k) {
                        b.quad(p(i, k, g[0]), p(i, kn, g[0]), p(inext, kn, g[0]), p(inext, k, g[0]), c);
                        b.quad(p(inext, k, g[1]), p(inext, kn, g[1]), p(i, kn, g[1]), p(i, k, g[1]), c);
                    }
                    // Walls to the next cells along X and round, whichever stands higher, in its
                    // colour; the bar's ends.
                    let next_x = if inext < nu { at(inext, k) } else { Some((0.0, c)) };
                    if let Some((o, oc)) = next_x.filter(|(o, _)| *o != r) {
                        let (lo, hi, cc) = if o < r { (o, r, c) } else { (r, o, oc) };
                        b.quad(p(inext, k, lo), p(inext, k, hi), p(inext, kn, hi), p(inext, kn, lo), cc);
                    }
                    if i == 0 {
                        b.quad(p(i, k, 0.0), p(i, kn, 0.0), p(i, kn, r), p(i, k, r), c);
                    }
                    if let Some((o, oc)) = at(i, kn % nv.max(1)).filter(|(o, _)| *o != r) {
                        let (lo, hi, cc) = if o < r { (o, r, c) } else { (r, o, oc) };
                        b.quad(p(i, kn, lo), p(inext, kn, lo), p(inext, kn, hi), p(i, kn, hi), cc);
                    }
                    i = inext;
                }
                k = kn;
            }
        }
    }
    b.m
}
